//! The real engine, a real fixture app and the shared service, as the .NET e2e
//! suites drive them. The engine and the fixtures must be built first
//! (`make dotnet`); their absence fails a suite rather than skipping it.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Arc;

use async_trait::async_trait;
use lsp_profiler::{ProfilerConfig, ProfilerEvents, ProfilerService};
use lsp_profiler_dotnet::{DotNetConfig, DotNetRuntime};
use serde_json::Value;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn built(relative: &str, hint: &str) -> PathBuf {
    let path = repo_root().join(relative);
    assert!(
        path.is_file(),
        "{} is missing: run `{hint}` first",
        path.display()
    );
    path
}

/// A fixture app under `dotnet/fixtures`, killed on drop. Its stdout stays
/// open so a fixture with a line protocol can be driven like a user would.
pub struct Fixture {
    pub child: Child,
    pub pid: u32,
    stdin: Option<ChildStdin>,
    lines: BufReader<ChildStdout>,
}

impl Fixture {
    /// Start the fixture `name` and wait for its `READY <pid>` line.
    pub fn start(name: &str) -> Self {
        let exe = built(
            &format!("dotnet/fixtures/{name}/bin/Release/net8.0/{name}"),
            "make dotnet",
        );
        let mut child = Command::new(format!("./{name}"))
            .current_dir(exe.parent().expect("fixture directory"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap_or_else(|error| panic!("spawn {name}: {error}"));
        let stdin = child.stdin.take().expect("fixture stdin");
        let stdout = child.stdout.take().expect("fixture stdout");
        let mut lines = BufReader::new(stdout);
        let mut line = String::new();
        let _ = lines.read_line(&mut line).expect("READY line");
        let pid: u32 = line
            .trim()
            .strip_prefix("READY ")
            .expect("READY <pid>")
            .parse()
            .expect("pid");
        Self {
            child,
            pid,
            stdin: Some(stdin),
            lines,
        }
    }

    /// Send one line command and return the one reply line the fixture writes.
    /// The fixture must answer every command with exactly one line.
    pub fn send(&mut self, command: &str) -> String {
        let stdin = self.stdin.as_mut().expect("fixture stdin is open");
        writeln!(stdin, "{command}").unwrap_or_else(|error| panic!("send {command}: {error}"));
        stdin
            .flush()
            .unwrap_or_else(|error| panic!("flush {command}: {error}"));
        let mut line = String::new();
        let read = self
            .lines
            .read_line(&mut line)
            .unwrap_or_else(|error| panic!("reply to {command}: {error}"));
        assert!(read > 0, "the fixture exited before answering {command}");
        line.trim().to_owned()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(mut stdin) = self.stdin.take() {
            let _ = stdin.write_all(b"\n");
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Debug, Default)]
struct Silent;

#[async_trait]
impl ProfilerEvents for Silent {
    async fn notify(&self, _method: &'static str, _params: Value) {}
}

/// The built engine executable.
pub fn engine() -> PathBuf {
    built(
        "dotnet/LspProfiler.DotNet.Engine/bin/Release/net8.0/lsp-profiler-dotnet-engine",
        "make dotnet",
    )
}

/// The shared service with only the .NET runtime, writing under `output_dir`.
pub fn service(output_dir: PathBuf) -> ProfilerService {
    let runtime = DotNetRuntime::new(DotNetConfig {
        engine: Some(engine()),
        output_dir: output_dir.clone(),
    });
    let config = ProfilerConfig {
        output_dir,
        ..ProfilerConfig::default()
    };
    ProfilerService::new(vec![Arc::new(runtime)], config, Arc::new(Silent))
}

/// Call a wire method, failing the test on an error.
pub async fn call(service: &ProfilerService, method: &str, params: Value) -> Value {
    service
        .handle(method, params)
        .await
        .unwrap_or_else(|error| panic!("{method} failed: {error}"))
}

/// [PROF-AGGREGATION-LOGIC] and [PROF-DOTNET-CPU-SYMBOLS]: only a thread
/// executing managed code counts. A fixture's main thread waits on stdin for
/// the whole session, so its entry point is never hot, and the worker's hot
/// function holds most of the share. Counting the idle thread caps that share
/// at half, since the runtime samples both threads every tick.
pub fn assert_only_busy_threads_count(report: &Value, entry_point: &str, hot_function: &str) {
    let functions = report["hot_functions"].as_array().expect("hot_functions");
    let named = |prefix: &str| {
        functions
            .iter()
            .find(|f| f["name"].as_str().is_some_and(|n| n.starts_with(prefix)))
    };
    assert!(
        named(entry_point).is_none(),
        "{entry_point} only runs on the idle main thread: {report}"
    );
    let share = named(hot_function).map_or(0.0, |f| f["percentage"].as_f64().unwrap_or(0.0));
    assert!(
        share > 50.0,
        "{hot_function} owns the busy thread, {share}%: {report}"
    );
}

/// The result of a memory step the direct backend completed.
pub fn completed(step: &Value) -> &Value {
    step.get("result")
        .or_else(|| step.pointer("/step/result"))
        .unwrap_or_else(|| panic!("direct backend must complete every step: {step}"))
}
