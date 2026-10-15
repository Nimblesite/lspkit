//! In-process py-spy sampling. Implements [PROF-PY-CPU-PYSPY].
//!
//! Attaches with the embedded `py-spy` crate and runs
//! `get_stack_traces()` on a dedicated OS thread, forwarding each tick as a
//! [`SampleBatch`] of neutral thread samples.

use std::time::{Duration, Instant};

use lsp_profiler::sampler::{SampleBatch, SamplerError, SamplerHandle, SamplerSink};
use lsp_profiler::wire::{StackFrame, ThreadSample};
use lsp_profiler::SamplerConfig;
use tokio::sync::oneshot;
use tracing::{debug, error, info, warn};

use crate::attach_errors::classify_attach_error;
use crate::RUNTIME_DISPLAY_NAME;

/// How long `stop` waits for the sampler thread's last tick.
const FINISH_TIMEOUT: Duration = Duration::from_secs(2);

/// py-spy's PID type: `i32` on Unix, `u32` on Windows.
fn to_pyspy_pid(pid: u32) -> py_spy::Pid {
    #[cfg(target_os = "windows")]
    {
        pid
    }
    #[cfg(not(target_os = "windows"))]
    {
        i32::try_from(pid).unwrap_or(0)
    }
}

/// Convert py-spy traces (leaf first) into neutral thread samples.
#[must_use]
pub fn to_samples(traces: Vec<py_spy::StackTrace>) -> Vec<ThreadSample> {
    traces
        .into_iter()
        .map(|trace| ThreadSample {
            thread_id: i64::try_from(trace.thread_id).unwrap_or(i64::MAX),
            thread_name: trace.thread_name,
            active: trace.active,
            frames: trace
                .frames
                .into_iter()
                .map(|frame| StackFrame {
                    name: frame.name,
                    file: Some(frame.filename),
                    line: Some(i64::from(frame.line)),
                    module: frame.module,
                })
                .collect(),
        })
        .collect()
}

/// Attach py-spy to `config.pid` and start sampling.
///
/// # Errors
///
/// Returns a classified [`SamplerError`] when py-spy cannot attach.
pub fn start_sampler(config: &SamplerConfig) -> Result<SamplerHandle, SamplerError> {
    let spy_config = py_spy::Config {
        sampling_rate: config.sample_rate,
        include_idle: config.include_idle,
        native: config.include_native,
        ..Default::default()
    };
    info!(pid = config.pid, sample_rate = config.sample_rate, "attaching py-spy");
    let spy = py_spy::PythonSpy::new(to_pyspy_pid(config.pid), &spy_config).map_err(|err| {
        let message = err.to_string();
        SamplerError::from_kind(&classify_attach_error(&message), config.pid, RUNTIME_DISPLAY_NAME, message)
    })?;
    let version = format!("{}.{}.{}", spy.version.major, spy.version.minor, spy.version.patch);
    info!(pid = config.pid, python_version = %version, "attached to Python process");
    let parts = SamplerHandle::channel(config.pid, version, FINISH_TIMEOUT);
    let interval = Duration::from_micros(1_000_000 / config.sample_rate.max(1));
    let limit = config.duration;
    let (sink, done) = (parts.sink, parts.done);
    let _thread = std::thread::Builder::new()
        .name(format!("profiler-pid-{}", config.pid))
        .spawn(move || sampler_loop(spy, &sink, interval, limit, done))
        .map_err(|err| SamplerError::AttachFailed(format!("cannot spawn sampler thread: {err}")))?;
    Ok(parts.handle)
}

/// Sample until stopped, the duration limit passes, or the target exits.
fn sampler_loop(
    mut spy: py_spy::PythonSpy,
    sink: &SamplerSink,
    interval: Duration,
    limit: Option<Duration>,
    done: oneshot::Sender<()>,
) {
    let started = Instant::now();
    while !sink.stop_requested() && limit.is_none_or(|limit| started.elapsed() <= limit) {
        match spy.get_stack_traces() {
            Ok(traces) => {
                if sink.sender.blocking_send(SampleBatch::new(to_samples(traces))).is_err() {
                    warn!("sampler: receiver dropped");
                    break;
                }
            }
            Err(err) if target_exited(&err.to_string()) => {
                info!("sampler: target process exited");
                break;
            }
            Err(err) => error!(error = %err, "sampler: failed to get stack traces"),
        }
        std::thread::sleep(interval);
    }
    debug!("sampler thread exiting");
    let _ = done.send(());
}

/// Whether a py-spy error means the target is gone.
fn target_exited(message: &str) -> bool {
    message.contains("No such process") || message.contains("exited")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trace(thread_id: u64, frames: Vec<(&str, &str, i32)>) -> py_spy::StackTrace {
        py_spy::StackTrace {
            pid: 0,
            thread_id,
            thread_name: Some("MainThread".to_owned()),
            owns_gil: true,
            active: true,
            frames: frames
                .into_iter()
                .map(|(name, file, line)| py_spy::Frame {
                    name: name.to_owned(),
                    filename: file.to_owned(),
                    line,
                    short_filename: None,
                    module: Some("app".to_owned()),
                    locals: None,
                    is_entry: false,
                    is_shim_entry: false,
                })
                .collect(),
            os_thread_id: None,
            process_info: None,
        }
    }

    #[test]
    fn converts_pyspy_traces_preserving_every_field() {
        let samples = to_samples(vec![trace(9, vec![("hot", "/app/x.py", 42), ("main", "/app/x.py", 1)])]);
        let only = samples.first();
        assert_eq!(only.map(|s| (s.thread_id, s.thread_name.as_deref(), s.active)), Some((9, Some("MainThread"), true)));
        let frames: Vec<(&str, Option<&str>, Option<i64>, Option<&str>)> = only
            .map(|s| s.frames.iter().map(|f| (f.name.as_str(), f.file.as_deref(), f.line, f.module.as_deref())).collect())
            .unwrap_or_default();
        assert_eq!(frames, vec![("hot", Some("/app/x.py"), Some(42), Some("app")), ("main", Some("/app/x.py"), Some(1), Some("app"))]);
    }

    #[test]
    fn recognises_target_exit_messages() {
        assert!(target_exited("No such process (os error 3)"));
        assert!(target_exited("process 12 exited"));
        assert!(!target_exited("transient read failure"));
    }

    #[test]
    fn attaching_to_a_missing_pid_is_process_not_found() {
        let config = SamplerConfig { pid: 999_999, sample_rate: 100, include_native: false, include_idle: false, duration: None };
        let result = start_sampler(&config);
        assert!(
            matches!(&result, Err(SamplerError::ProcessNotFound(999_999) | SamplerError::PermissionDenied(_))),
            "a dead PID must classify as not-found (or permission-denied where the OS refuses first): {result:?}"
        );
    }
}
