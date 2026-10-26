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
    let caller = stats(&data, "src/a.py", "caller_fn").map(|s| (s.total_samples, s.self_samples));
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
    let topo = stats(&data, "src/a.py", "build_topo").map(|s| (s.total_samples, s.self_samples));
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

// [PROF-AGGREGATION-LOGIC] A function on several threads' stacks in one tick
// (every managed thread starts in the runtime's thread entry point) counts once
// for that tick, inclusive and self, so its samples never outnumber the ticks.
#[test]
fn threads_sharing_a_function_in_one_tick_count_it_once() {
    let mut data = ProfileData::default();
    let thread = |thread_id, leaf| ThreadSample {
        thread_id,
        thread_name: None,
        active: true,
        frames: vec![frame(leaf, "src/a.py", 7), frame("start", "src/a.py", 1)],
    };
    ingest(
        &mut data,
        &SampleBatch::new(vec![thread(1, "work"), thread(2, "work")]),
    );
    ingest(
        &mut data,
        &SampleBatch::new(vec![thread(1, "work"), thread(2, "other")]),
    );
    assert_eq!(data.total_samples, 2);
    let counts = |name| stats(&data, "src/a.py", name).map(|s| (s.total_samples, s.self_samples));
    assert_eq!(
        counts("start"),
        Some((2, 0)),
        "one count per tick, not per thread"
    );
    assert_eq!(
        counts("work"),
        Some((2, 2)),
        "self counts once per tick too"
    );
    assert_eq!(counts("other"), Some((1, 1)));
    assert_eq!(
        data.line_hits
            .get("src/a.py")
            .map(|lines| (lines.get(&1), lines.get(&7))),
        Some((Some(&2), Some(&2))),
        "a line counts once per tick"
    );
    let config = HotspotConfig {
        function_threshold_pct: 0.0,
        ..HotspotConfig::default()
    };
    assert!(data
        .hot_functions(&config)
        .iter()
        .all(|f| f.samples <= 2 && f.percentage <= 100.0 && f.self_percentage <= f.percentage));
}

// [PROF-CPU-REPORT] Hot functions are ordered by inclusive share, then self
// share, then name: a stack's root and its leaf tie on inclusive samples, and
// equal functions never fall back to hash order.
#[test]
fn hot_functions_break_equal_totals_by_self_samples_then_name() {
    let mut data = ProfileData::default();
    for _ in 0..3 {
        ingest(
            &mut data,
            &tick(
                1,
                true,
                vec![frame("spin", "a.osp", 2), frame("main", "a.osp", 1)],
            ),
        );
    }
    for name in ["zeta", "alpha"] {
        ingest(&mut data, &tick(1, true, vec![frame(name, "b.osp", 1)]));
    }
    let order: Vec<String> = data
        .hot_functions(&HotspotConfig::default())
        .into_iter()
        .map(|hot| hot.name)
        .collect();
    assert_eq!(order, vec!["spin", "main", "alpha", "zeta"]);
}
