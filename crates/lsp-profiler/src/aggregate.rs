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

/// Per-sample dedup sets: an inclusive counter credits each distinct line or
/// function once per sample, so recursion can never push a share above 100%.
#[derive(Default)]
struct SampleSeen {
    lines: HashSet<(String, i64)>,
    functions: HashSet<FunctionKey>,
}

impl ProfileData {
    /// Ingest one sampling tick. Implements [PROF-AGGREGATION-LOGIC].
    ///
    /// Idle threads are skipped unless `include_idle`; scaffolding frames are
    /// stripped first and a thread left with nothing is dropped as pure
    /// machinery ([PROF-AGGREGATION-SCAFFOLD]). The tick counts once in
    /// `total_samples` whatever its thread count.
    pub fn ingest(
        &mut self,
        batch: &SampleBatch,
        default_weight: f64,
        include_idle: bool,
        filter: &dyn FrameFilter,
    ) {
        let weight = batch.weight_secs.unwrap_or(default_weight);
        for sample in batch.samples.iter().filter(|s| s.active || include_idle) {
            let kept: Vec<&StackFrame> = sample
                .frames
                .iter()
                .filter(|frame| !filter.is_scaffolding(frame))
                .collect();
            if !kept.is_empty() {
                self.ingest_thread(sample, &kept, weight);
            }
        }
        self.total_samples += 1;
    }

    /// Record one kept thread stack (frames leaf first).
    fn ingest_thread(&mut self, sample: &ThreadSample, kept: &[&StackFrame], weight: f64) {
        let thread_id = sample.thread_id;
        if let Some(name) = &sample.thread_name {
            let _ = self
                .thread_names
                .entry(thread_id)
                .or_insert_with(|| name.clone());
        }
        *self.thread_samples.entry(thread_id).or_insert(0) += 1;
        let mut seen = SampleSeen::default();
        let mut stack: Vec<usize> = kept
            .iter()
            .enumerate()
            .map(|(depth, frame)| self.ingest_frame(frame, depth == 0, &mut seen))
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
        stats.self_samples += u64::from(is_leaf);
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

    /// Kept thread samples — the single denominator of every percentage.
    /// Equals the sum of self samples, since each kept stack has one leaf.
    #[must_use]
    pub fn sample_count(&self) -> u64 {
        self.thread_samples.values().sum()
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
        result.sort_by_key(|func| std::cmp::Reverse(func.samples));
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
mod tests {
    use super::*;
    use crate::runtime::KeepAllFrames;

    fn frame(name: &str, file: &str, line: i64) -> StackFrame {
        StackFrame {
            name: name.to_owned(),
            file: Some(file.to_owned()),
            line: Some(line),
            module: None,
        }
    }

    fn tick(thread_id: i64, active: bool, frames: Vec<StackFrame>) -> SampleBatch {
        SampleBatch::new(vec![ThreadSample {
            thread_id,
            thread_name: Some(format!("Thread-{thread_id}")),
            active,
            frames,
        }])
    }

    fn ingest(data: &mut ProfileData, batch: &SampleBatch) {
        data.ingest(batch, 0.01, false, &KeepAllFrames);
    }

    fn stats<'a>(data: &'a ProfileData, file: &str, name: &str) -> Option<&'a FunctionStats> {
        data.function_stats.get(&FunctionKey {
            file: Some(file.to_owned()),
            name: name.to_owned(),
        })
    }

    // [PROF-AGGREGATION-LOGIC] Leaf gets self samples; every frame gets an
    // inclusive count and a line hit.
    #[test]
    fn ingest_single_trace_counts_correctly() {
        let mut data = ProfileData::default();
        let stack = vec![
            frame("leaf_fn", "src/a.py", 10),
            frame("caller_fn", "src/a.py", 5),
            frame("main", "main.py", 1),
        ];
        ingest(&mut data, &tick(1, true, stack));
        assert_eq!(data.total_samples, 1);
        assert_eq!(
            data.line_hits.get("src/a.py").and_then(|m| m.get(&10)),
            Some(&1)
        );
        let leaf = stats(&data, "src/a.py", "leaf_fn").map(|s| (s.total_samples, s.self_samples));
        assert_eq!(leaf, Some((1, 1)));
        let caller =
            stats(&data, "src/a.py", "caller_fn").map(|s| (s.total_samples, s.self_samples));
        assert_eq!(caller, Some((1, 0)));
        let root_first: Vec<&str> = data
            .thread_stacks
            .get(&1)
            .and_then(|stacks| stacks.first())
            .map(|stack| {
                stack
                    .iter()
                    .filter_map(|i| data.frames.get(*i))
                    .map(|f| f.name.as_str())
                    .collect()
            })
            .unwrap_or_default();
        assert_eq!(root_first, vec!["main", "caller_fn", "leaf_fn"]);
    }

    // [PROF-AGGREGATION-LOGIC] Idle threads are skipped unless included.
    #[test]
    fn idle_traces_skipped_when_not_included() {
        let mut data = ProfileData::default();
        ingest(
            &mut data,
            &tick(1, false, vec![frame("idle_fn", "src/a.py", 10)]),
        );
        assert_eq!(data.total_samples, 1);
        assert!(data.line_hits.is_empty());
        data.ingest(
            &tick(1, false, vec![frame("idle_fn", "src/a.py", 10)]),
            0.01,
            true,
            &KeepAllFrames,
        );
        assert_eq!(
            data.line_hits.get("src/a.py").and_then(|m| m.get(&10)),
            Some(&1)
        );
    }

    // [PROF-AGGREGATION-LOGIC] Recursion counts once per sample, so no share
    // can exceed 100%.
    #[test]
    fn recursive_stack_counts_inclusive_totals_once_per_sample() {
        let mut data = ProfileData::default();
        let stack = vec![
            frame("build_topo", "src/a.py", 98),
            frame("build_topo", "src/a.py", 101),
            frame("build_topo", "src/a.py", 101),
            frame("main", "main.py", 1),
        ];
        ingest(&mut data, &tick(1, true, stack));
        let topo =
            stats(&data, "src/a.py", "build_topo").map(|s| (s.total_samples, s.self_samples));
        assert_eq!(topo, Some((1, 1)), "one sample credits one inclusive count");
        assert_eq!(
            data.line_hits.get("src/a.py").and_then(|m| m.get(&101)),
            Some(&1)
        );
        let config = HotspotConfig {
            function_threshold_pct: 0.0,
            ..HotspotConfig::default()
        };
        let hot = data.hot_functions(&config);
        let topo_pct = hot
            .iter()
            .find(|f| f.name == "build_topo")
            .map(|f| f.percentage);
        assert_eq!(topo_pct, Some(100.0));
        let line_pct = data
            .hot_lines(&HotspotConfig::default())
            .iter()
            .find(|l| l.line == 101)
            .map(|l| l.percentage);
        assert_eq!(line_pct, Some(100.0));
    }

    // [PROF-AGGREGATION-THRESHOLD] Lines below the threshold are dropped.
    #[test]
    fn hot_lines_filters_by_threshold() {
        let mut data = ProfileData::default();
        for _ in 0..50 {
            ingest(
                &mut data,
                &tick(1, true, vec![frame("fn_a", "src/a.py", 10)]),
            );
        }
        ingest(
            &mut data,
            &tick(1, true, vec![frame("fn_b", "src/a.py", 20)]),
        );
        let config = HotspotConfig {
            line_threshold_pct: 5.0,
            ..HotspotConfig::default()
        };
        let lines: Vec<i64> = data.hot_lines(&config).iter().map(|h| h.line).collect();
        assert_eq!(lines, vec![10]);
    }

    // [PROF-AGGREGATION-STRUCTS] Frames without a source location (a runtime
    // that reports only names) still yield hot functions, never hot lines.
    #[test]
    fn name_only_frames_produce_functions_but_no_lines() {
        let mut data = ProfileData::default();
        let nameless = StackFrame {
            name: "Ns.Type.Work()".to_owned(),
            file: None,
            line: None,
            module: Some("App".to_owned()),
        };
        ingest(&mut data, &tick(4, true, vec![nameless]));
        assert!(data.line_hits.is_empty());
        let hot = data.hot_functions(&HotspotConfig::default());
        assert_eq!(
            hot.first()
                .map(|f| (f.name.as_str(), f.file.is_none(), f.percentage)),
            Some(("Ns.Type.Work()", true, 100.0))
        );
    }

    // [PROF-AGGREGATION-SCAFFOLD] A runtime filter strips plumbing frames, and
    // a thread left with nothing is dropped entirely.
    #[test]
    fn filter_strips_scaffolding_and_drops_machinery_threads() {
        #[derive(Debug)]
        struct DropLauncher;
        impl FrameFilter for DropLauncher {
            fn is_scaffolding(&self, frame: &StackFrame) -> bool {
                frame.name == "launcher"
            }
        }
        let mut data = ProfileData::default();
        let batch = SampleBatch::new(vec![
            ThreadSample {
                thread_id: 1,
                thread_name: None,
                active: true,
                frames: vec![frame("work", "a.py", 3), frame("launcher", "l.py", 1)],
            },
            ThreadSample {
                thread_id: 2,
                thread_name: None,
                active: true,
                frames: vec![frame("launcher", "l.py", 1)],
            },
        ]);
        data.ingest(&batch, 0.01, false, &DropLauncher);
        assert_eq!(
            data.sample_count(),
            1,
            "the machinery-only thread is dropped"
        );
        assert!(data.frames.iter().all(|f| f.name != "launcher"));
        assert!(!data.thread_samples.contains_key(&2));
    }

    // [PROF-AGGREGATION-LOGIC] A batch weight overrides the session default.
    #[test]
    fn batch_weight_overrides_default() {
        let mut data = ProfileData::default();
        let mut batch = tick(1, true, vec![frame("f", "a.py", 1)]);
        batch.weight_secs = Some(0.25);
        ingest(&mut data, &batch);
        assert_eq!(data.thread_weights.get(&1).cloned(), Some(vec![0.25]));
    }
}
