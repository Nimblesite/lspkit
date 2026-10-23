//! Sample aggregation. Implements [PROF-AGGREGATION].
//!
//! Accumulates [`ThreadSample`]s into per-line hit counts, per-function
//! inclusive/self counts, and deduplicated per-thread stacks for flame-graph
//! exports. Runtime scaffolding is stripped at this single ingest point by the
//! runtime's [`FrameFilter`], so every export roots at user code.

use std::collections::{HashMap, HashSet};

use crate::runtime::FrameFilter;
use crate::sampler::SampleBatch;
use crate::wire::{HotFunction, HotLine, StackFrame, ThreadSample};

/// Accumulated profiling data for one session. Implements
/// [PROF-AGGREGATION-STRUCTS].
#[derive(Debug, Clone, Default)]
pub struct ProfileData {
    /// `file -> line -> samples touching the line`.
    pub line_hits: HashMap<String, HashMap<i64, u64>>,
    /// Per-function inclusive and self counts.
    pub function_stats: HashMap<FunctionKey, FunctionStats>,
    /// Sampling ticks ingested.
    pub total_samples: u64,
    /// Ticks in which at least one thread's stack was kept.
    pub busy_ticks: u64,
    /// Kept thread samples per thread.
    pub thread_samples: HashMap<i64, u64>,
    /// Frame deduplication index for exports.
    pub frame_index: HashMap<FrameKey, usize>,
    /// Deduplicated frames, indexed by `frame_index`.
    pub frames: Vec<FrameKey>,
    /// Per-thread stacks as root-first frame indices.
    pub thread_stacks: HashMap<i64, Vec<Vec<usize>>>,
    /// Per-thread sample weights in seconds, parallel to `thread_stacks`.
    pub thread_weights: HashMap<i64, Vec<f64>>,
    /// Thread names by thread ID.
    pub thread_names: HashMap<i64, String>,
}

/// Identity of a deduplicated frame.
#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct FrameKey {
    /// Function name.
    pub name: String,
    /// Source path, when known.
    pub file: Option<String>,
    /// 1-based line, when known.
    pub line: Option<i64>,
}

impl FrameKey {
    fn of(frame: &StackFrame) -> Self {
        Self {
            name: frame.name.clone(),
            file: frame.file.clone(),
            line: frame.line,
        }
    }
}

/// Identity of a function: its name within its file.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct FunctionKey {
    /// Source path, when known.
    pub file: Option<String>,
    /// Function name.
    pub name: String,
}

/// Per-function statistics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionStats {
    /// Function name.
    pub name: String,
    /// Source path, when known.
    pub file: Option<String>,
    /// First line seen for the function, when known.
    pub line: Option<i64>,
    /// Samples with the function anywhere on the stack.
    pub total_samples: u64,
    /// Samples with the function as the leaf.
    pub self_samples: u64,
}

/// Thresholds for what counts as hot. Implements [PROF-AGGREGATION-THRESHOLD].
#[derive(Debug, Clone, PartialEq)]
pub struct HotspotConfig {
    /// Minimum share of samples for a hot line.
    pub line_threshold_pct: f64,
    /// Minimum share of samples for a hot function.
    pub function_threshold_pct: f64,
    /// Maximum hot lines (and diagnostics) per file.
    pub max_diagnostics_per_file: usize,
}

impl Default for HotspotConfig {
    fn default() -> Self {
        Self {
            line_threshold_pct: 1.0,
            function_threshold_pct: 2.0,
            max_diagnostics_per_file: 20,
        }
    }
}

/// Per-tick dedup sets: a counter credits each distinct line or function once
/// per tick, across every thread's stack, so neither recursion nor a frame
/// every thread shares can push a share above 100%.
#[derive(Default)]
struct SampleSeen {
    lines: HashSet<(String, i64)>,
    functions: HashSet<FunctionKey>,
    leaves: HashSet<FunctionKey>,
}

impl ProfileData {
    /// Ingest one sampling tick. Implements [PROF-AGGREGATION-LOGIC].
    ///
    /// Idle threads are skipped unless `include_idle`; scaffolding frames are
    /// stripped first and a thread left with nothing is dropped as pure
    /// machinery ([PROF-AGGREGATION-SCAFFOLD]). The tick counts once in
    /// `total_samples` whatever its thread count, and once in `busy_ticks`
    /// when any thread's stack is kept.
    pub fn ingest(
        &mut self,
        batch: &SampleBatch,
        default_weight: f64,
        include_idle: bool,
        filter: &dyn FrameFilter,
    ) {
        let weight = batch.weight_secs.unwrap_or(default_weight);
        let mut seen = SampleSeen::default();
        let mut busy = false;
        for sample in batch.samples.iter().filter(|s| s.active || include_idle) {
            let kept: Vec<&StackFrame> = sample
                .frames
                .iter()
                .filter(|frame| !filter.is_scaffolding(frame))
                .collect();
            if !kept.is_empty() {
                self.ingest_thread(sample, &kept, weight, &mut seen);
                busy = true;
            }
        }
        self.busy_ticks += u64::from(busy);
        self.total_samples += 1;
    }

    /// Record one kept thread stack (frames leaf first).
    fn ingest_thread(
        &mut self,
        sample: &ThreadSample,
        kept: &[&StackFrame],
        weight: f64,
        seen: &mut SampleSeen,
    ) {
        let thread_id = sample.thread_id;
        if let Some(name) = &sample.thread_name {
            let _ = self
                .thread_names
                .entry(thread_id)
                .or_insert_with(|| name.clone());
        }
        *self.thread_samples.entry(thread_id).or_insert(0) += 1;
        let mut stack: Vec<usize> = kept
            .iter()
            .enumerate()
            .map(|(depth, frame)| self.ingest_frame(frame, depth == 0, seen))
            .collect();
        // Samplers report leaf first; exports want root first.
        stack.reverse();
        self.thread_stacks.entry(thread_id).or_default().push(stack);
        self.thread_weights
            .entry(thread_id)
            .or_default()
            .push(weight);
    }

    /// Count one frame and return its deduplicated index.
    fn ingest_frame(&mut self, frame: &StackFrame, is_leaf: bool, seen: &mut SampleSeen) -> usize {
        if let (Some(file), Some(line)) = (&frame.file, frame.line) {
            if seen.lines.insert((file.clone(), line)) {
                *self
                    .line_hits
                    .entry(file.clone())
                    .or_default()
                    .entry(line)
                    .or_insert(0) += 1;
            }
        }
        let key = FunctionKey {
            file: frame.file.clone(),
            name: frame.name.clone(),
        };
        let newly_seen = seen.functions.insert(key.clone());
        let newly_leaf = is_leaf && seen.leaves.insert(key.clone());
        let stats = self
            .function_stats
            .entry(key)
            .or_insert_with(|| FunctionStats {
                name: frame.name.clone(),
                file: frame.file.clone(),
                line: frame.line,
                total_samples: 0,
                self_samples: 0,
            });
        stats.total_samples += u64::from(newly_seen);
        stats.self_samples += u64::from(newly_leaf);
        self.frame_slot(FrameKey::of(frame))
    }

    /// Index of `key` in the deduplicated frame table, inserting it if new.
    fn frame_slot(&mut self, key: FrameKey) -> usize {
        if let Some(index) = self.frame_index.get(&key) {
            return *index;
        }
        let index = self.frames.len();
        self.frames.push(key.clone());
        let _ = self.frame_index.insert(key, index);
        index
    }

    /// Ticks that kept a thread stack — the single denominator of every
    /// percentage. Every count is per tick, so no share exceeds 100%.
    #[must_use]
    pub fn sample_count(&self) -> u64 {
        self.busy_ticks
    }

    /// Lines at or above the line threshold, hottest first, capped per file.
    #[must_use]
    pub fn hot_lines(&self, config: &HotspotConfig) -> Vec<HotLine> {
        let total = self.sample_count();
        let mut result: Vec<HotLine> = self
            .line_hits
            .iter()
            .flat_map(|(file, lines)| hot_lines_in_file(file, lines, total, config))
            .collect();
        result.sort_by_key(|line| std::cmp::Reverse(line.samples));
        result
    }

    /// Functions at or above the function threshold, hottest first.
    #[must_use]
    pub fn hot_functions(&self, config: &HotspotConfig) -> Vec<HotFunction> {
        let total = self.sample_count();
        let mut result: Vec<HotFunction> = self
            .function_stats
            .values()
            .filter(|stats| {
                total > 0 && pct_of(stats.total_samples, total) >= config.function_threshold_pct
            })
            .map(|stats| HotFunction {
                name: stats.name.clone(),
                file: stats.file.clone(),
                line: stats.line,
                samples: saturating_i64(stats.total_samples),
                percentage: pct_of(stats.total_samples, total),
                self_samples: saturating_i64(stats.self_samples),
                self_percentage: pct_of(stats.self_samples, total),
            })
            .collect();
        // Equal inclusive totals (a stack's root and its callees) put the one
        // with more time of its own first, then order by name: never by hash order.
        result.sort_by(|a, b| {
            b.samples
                .cmp(&a.samples)
                .then_with(|| b.self_samples.cmp(&a.self_samples))
                .then_with(|| a.name.cmp(&b.name))
        });
        result
    }
}

/// Hot lines of one file, hottest first, truncated to the per-file cap.
fn hot_lines_in_file(
    file: &str,
    lines: &HashMap<i64, u64>,
    total: u64,
    config: &HotspotConfig,
) -> Vec<HotLine> {
    let mut hot: Vec<HotLine> = lines
        .iter()
        .filter(|(_, samples)| total > 0 && pct_of(**samples, total) >= config.line_threshold_pct)
        .map(|(line, samples)| HotLine {
            file: file.to_owned(),
            line: *line,
            samples: saturating_i64(*samples),
            percentage: pct_of(*samples, total),
        })
        .collect();
    hot.sort_by_key(|line| std::cmp::Reverse(line.samples));
    hot.truncate(config.max_diagnostics_per_file);
    hot
}

/// `part` as a percentage of `total`, saturating at `u32::MAX` samples.
#[must_use]
pub fn pct_of(part: u64, total: u64) -> f64 {
    if total == 0 {
        return 0.0;
    }
    let part = f64::from(u32::try_from(part).unwrap_or(u32::MAX));
    let total = f64::from(u32::try_from(total).unwrap_or(u32::MAX));
    part / total * 100.0
}

/// Convert a count to the wire's signed integer, saturating.
#[must_use]
pub fn saturating_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests;
