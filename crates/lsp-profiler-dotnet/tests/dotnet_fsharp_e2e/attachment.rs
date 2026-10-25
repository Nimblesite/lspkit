//! Explicit PID attachment is independent of unrelated process exits. [PROF-DOTNET-ENGINE]
//! Real F# runtime, real native process churn and the actual engine CLI. [PROF-ERRORS]
use std::io::{BufReader, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier};
use std::thread::JoinHandle;

use serde_json::Value;

use crate::harness::FIXTURE;
use crate::support::{engine, fixture_executable, ready_pid, Fixture};

struct ProcessChurn {
    stopped: Arc<AtomicBool>,
    workers: Vec<JoinHandle<()>>,
}

impl ProcessChurn {
    fn start() -> Self {
        let stopped = Arc::new(AtomicBool::new(false));
        let ready = Arc::new(Barrier::new(5));
        let workers = (0..4)
            .map(|_| worker(Arc::clone(&stopped), Arc::clone(&ready)))
            .collect();
        let _ = ready.wait();
        Self { stopped, workers }
    }
}

fn worker(stopped: Arc<AtomicBool>, ready: Arc<Barrier>) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let _ = ready.wait();
        while !stopped.load(Ordering::Acquire) {
            let status = Command::new("true").status().expect("owned native process");
            assert!(status.success(), "native process exits naturally");
            published_process_exit();
        }
    })
}

fn published_process() -> Child {
    Command::new(fixture_executable(FIXTURE))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("owned unrelated F# runtime")
}

fn published_process_exit() {
    let mut child = published_process();
    let stdout = child.stdout.take().expect("runtime readiness stream");
    let pid = ready_pid(&mut BufReader::new(stdout));
    assert_eq!(child.id(), pid);
    child
        .stdin
        .take()
        .expect("runtime command stream")
        .write_all(b"stop\n")
        .expect("request natural runtime exit");
    assert!(child.wait().expect("reap unrelated runtime").success());
}

impl Drop for ProcessChurn {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        for worker in self.workers.drain(..) {
            worker
                .join()
                .expect("all owned native processes are reaped");
        }
    }
}

#[test]
fn explicit_target_attachment_survives_unrelated_process_exit() {
    let mut fixture = Fixture::start(FIXTURE);
    assert_eq!(fixture.child.id(), fixture.pid, "actual ready F# runtime");
    let _churn = ProcessChurn::start();
    for _ in 0..32 {
        assert!(fixture.child.try_wait().expect("target alive").is_none());
        let report = collect(&mut Command::new(engine()), fixture.pid);
        assert_eq!(report["kind"], "gc");
        assert_eq!(report["observed"], true);
        assert_complete_heap(&report);
        assert!(fixture.child.try_wait().expect("same target").is_none());
    }
}

fn collect(command: &mut Command, pid: u32) -> Value {
    let output = command
        .args(["gc", "--pid", &pid.to_string()])
        .output()
        .expect("run actual profiling engine");
    let rendered = String::from_utf8(output.stdout).expect("rendered engine output");
    assert!(
        output.status.success(),
        "an unrelated process exit cannot break live PID {pid}: {rendered}"
    );
    serde_json::from_str(&rendered).expect("one actual GC result")
}

#[test]
fn explicit_pid_attachment_never_reads_unrelated_process_status() {
    let mut target = Fixture::start(FIXTURE);
    let mut unrelated = Fixture::start(FIXTURE);
    let capture = tempfile::tempdir().expect("actual syscall capture");
    let trace = capture.path().join("unrelated-status.trace");
    let report = collect(&mut traced_engine(unrelated.pid, &trace), target.pid);
    assert_eq!(report["kind"], "gc");
    assert_eq!(report["observed"], true);
    assert_complete_heap(&report);
    assert!(target
        .child
        .try_wait()
        .expect("target remains alive")
        .is_none());
    assert!(unrelated
        .child
        .try_wait()
        .expect("other runtime remains alive")
        .is_none());
    let accesses = std::fs::read_to_string(trace).expect("actual filtered kernel operations");
    assert!(
        accesses.is_empty(),
        "targeted attach inspected unrelated /proc status: {accesses}"
    );
}

fn traced_engine(unrelated: u32, trace: &Path) -> Command {
    let mut command = Command::new("strace");
    let _ = command
        .args(["-f", "-qq", "-e", "trace=%file", "-e", "signal=none", "-P"])
        .arg(format!("/proc/{unrelated}/status"))
        .arg("-o")
        .arg(trace)
        .arg(engine());
    command
}

fn assert_complete_heap(report: &Value) {
    let generations = report["generation_bytes"]
        .as_array()
        .expect("real heap generations");
    assert!(!generations.is_empty());
    let total: i64 = generations
        .iter()
        .map(|value| value.as_i64().expect("generation bytes"))
        .sum();
    assert!(total > 0);
    assert_eq!(report["heap_bytes_after"], total);
}
