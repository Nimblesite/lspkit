//! Cooperative sampling: tail a sample file an in-process sampler writes.
//! Implements [PROF-CPU-COOPERATIVE].
//!
//! When reading another process's memory is impossible or needs elevation,
//! the editor injects a small sampler thread into the paused debuggee through
//! the debug adapter. The thread writes one JSON record per tick to a minted
//! file; this module tails that file and exposes it as a [`SamplerHandle`],
//! so the whole pipeline downstream is shared with every other backend.
//!
//! Wire format (one JSON value per line, frames leaf first):
//!
//! ```text
//! {"header":{"runtime_version":"3.13.7","pid":12345}}
//! {"ticks":[[140234,true,[["/app/x.py",12,"hot_function"],...]],...]}
//! ```
//!
//! Stopping writes a `<file>.stop` sentinel; the injected thread sees it and
//! exits, and the tailer drains what was flushed before removing both files.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

use tokio::sync::oneshot;
use tokio::time::timeout;
use tracing::{debug, info, warn};

use crate::sampler::{SampleBatch, SamplerError, SamplerHandle, SamplerSink};
use crate::wire::{StackFrame, ThreadSample};

/// How long to wait for the injected sampler's header line.
const HEADER_TIMEOUT: Duration = Duration::from_secs(15);
/// Poll interval while waiting for new lines.
const TAIL_POLL_INTERVAL: Duration = Duration::from_millis(100);
/// How long to keep draining after a stop is requested.
const STOP_DRAIN_GRACE: Duration = Duration::from_secs(2);

/// One thread of one tick: `[thread_id, active, [[file, line, name], ...]]`.
type TickThread = (i64, bool, Vec<(String, i64, String)>);

/// Outer record that distinguishes the handshake from subsequent tick records.
#[derive(serde::Deserialize)]
struct HeaderLine {
    /// Runtime identity required before samples can enter the shared pipeline.
    header: Header,
}

/// The first record of a sample file.
#[derive(serde::Deserialize)]
struct Header {
    /// Runtime version reported by the injected sampler for session metadata.
    runtime_version: String,
    /// Process ID used to associate the cooperative session with its debuggee.
    pid: u32,
}

/// One newline-delimited record containing every thread observed at a tick.
#[derive(serde::Deserialize)]
struct TickLine {
    /// Compact per-thread stacks decoded into the common sampler wire model.
    ticks: Vec<TickThread>,
}

/// Mint a unique sample-file path. A process-wide counter breaks ties that
/// wall-clock nanoseconds alone would not.
#[must_use]
pub fn mint_sample_file() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "lsp-profiler-cpu-{}-{nanos}-{sequence}.jsonl",
        std::process::id()
    ))
}

/// The stop-sentinel path for a sample file.
#[must_use]
pub fn stop_sentinel(sample_file: &Path) -> PathBuf {
    PathBuf::from(format!("{}.stop", sample_file.display()))
}

/// Start tailing `sample_file` once its header arrives.
///
/// # Errors
///
/// Returns [`SamplerError::AttachFailed`] when no header arrives in time —
/// usually because the injection script was never evaluated.
pub async fn tail(sample_file: PathBuf) -> Result<SamplerHandle, SamplerError> {
    let (ready_tx, ready_rx) = oneshot::channel::<Header>();
    let (sink_tx, sink_rx) = oneshot::channel::<(SamplerSink, oneshot::Sender<()>)>();
    let tail_file = sample_file.clone();
    // The thread is detached: on a missing header it gives up at its own
    // deadline, and dropping `sink_tx` ends it right after a late header.
    let _detached = std::thread::Builder::new()
        .name("profiler-cooperative-tail".to_owned())
        .spawn(move || tail_thread(&tail_file, ready_tx, sink_rx))
        .map_err(|err| SamplerError::AttachFailed(format!("cannot spawn tail thread: {err}")))?;
    let Ok(Ok(header)) = timeout(HEADER_TIMEOUT, ready_rx).await else {
        cleanup(&sample_file);
        drop(sink_tx);
        return Err(SamplerError::AttachFailed(format!(
            "the in-process sampler never reported its header in {}; \
             was the injection script evaluated in the paused debuggee?",
            sample_file.display()
        )));
    };
    info!(pid = header.pid, runtime_version = %header.runtime_version, "cooperative sampler attached");
    let parts = SamplerHandle::channel(header.pid, header.runtime_version, STOP_DRAIN_GRACE * 2);
    sink_tx.send((parts.sink, parts.done)).map_err(|_unsent| {
        SamplerError::AttachFailed("cooperative tail thread exited".to_owned())
    })?;
    Ok(parts.handle)
}

/// Thread body: handshake on the header, then stream ticks until stopped.
fn tail_thread(
    sample_file: &Path,
    ready_tx: oneshot::Sender<Header>,
    sink_rx: oneshot::Receiver<(SamplerSink, oneshot::Sender<()>)>,
) {
    let deadline = Instant::now() + HEADER_TIMEOUT;
    let Some((reader, header)) = await_header(sample_file, deadline) else {
        return;
    };
    if ready_tx.send(header).is_err() {
        return;
    }
    let Ok((sink, done)) = sink_rx.blocking_recv() else {
        cleanup(sample_file);
        return;
    };
    stream_ticks(sample_file, reader, &sink);
    cleanup(sample_file);
    let _ = done.send(());
    debug!("cooperative tail finished");
}

/// Wait (bounded) for the file and its header line.
fn await_header(sample_file: &Path, deadline: Instant) -> Option<(BufReader<File>, Header)> {
    let mut reader: Option<BufReader<File>> = None;
    let mut line = String::new();
    while Instant::now() < deadline {
        if reader.is_none() {
            reader = File::open(sample_file).ok().map(BufReader::new);
        }
        if let Some(open) = reader.as_mut() {
            match open.read_line(&mut line) {
                Ok(0) => {}
                Ok(_) => {
                    let header = serde_json::from_str::<HeaderLine>(&line).ok()?.header;
                    return reader.map(|r| (r, header));
                }
                Err(_) => return None,
            }
        }
        std::thread::sleep(TAIL_POLL_INTERVAL);
    }
    None
}

/// Stream tick records into the sink until a stop drains or the receiver closes.
fn stream_ticks(sample_file: &Path, mut reader: BufReader<File>, sink: &SamplerSink) {
    let mut line = String::new();
    let mut drain_deadline: Option<Instant> = None;
    let mut warned = false;
    loop {
        if sink.stop_requested() && drain_deadline.is_none() {
            let _ = std::fs::write(stop_sentinel(sample_file), b"stop");
            drain_deadline = Some(Instant::now() + STOP_DRAIN_GRACE);
        }
        match reader.read_line(&mut line) {
            Ok(0) if drain_deadline.is_some_and(|d| Instant::now() >= d) => return,
            Ok(0) => std::thread::sleep(TAIL_POLL_INTERVAL),
            // A writer can flush mid-record; keep the fragment for the next read.
            Ok(_) if !line.ends_with('\n') => std::thread::sleep(TAIL_POLL_INTERVAL),
            Ok(_) => {
                if !deliver(&line, sink, &mut warned) {
                    return;
                }
                line.clear();
            }
            Err(err) => {
                warn!(%err, "cooperative sample file read failed");
                return;
            }
        }
    }
}

/// Parse one complete record and send it; `false` when the session is gone.
fn deliver(line: &str, sink: &SamplerSink, warned: &mut bool) -> bool {
    match serde_json::from_str::<TickLine>(line) {
        Ok(tick) => sink
            .sender
            .blocking_send(SampleBatch::new(to_samples(tick.ticks)))
            .is_ok(),
        Err(err) => {
            // Dropped data is never silent, but one warning is enough.
            if !*warned {
                *warned = true;
                warn!(%err, "dropping unparseable cooperative sample line(s)");
            }
            true
        }
    }
}

/// Convert compact ticks into thread samples.
fn to_samples(ticks: Vec<TickThread>) -> Vec<ThreadSample> {
    ticks
        .into_iter()
        .map(|(thread_id, active, frames)| ThreadSample {
            thread_id,
            thread_name: None,
            active,
            frames: frames
                .into_iter()
                .map(|(file, line, name)| StackFrame {
                    name,
                    file: Some(file),
                    line: Some(line),
                    module: None,
                })
                .collect(),
        })
        .collect()
}

/// Remove the sample file and its sentinel.
fn cleanup(sample_file: &Path) {
    let _ = std::fs::remove_file(sample_file);
    let _ = std::fs::remove_file(stop_sentinel(sample_file));
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn write_lines(path: &Path, lines: &[&str]) -> std::io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        lines.iter().try_for_each(|line| writeln!(file, "{line}"))
    }

    #[test]
    fn minted_files_are_unique_temp_paths() {
        let (a, b) = (mint_sample_file(), mint_sample_file());
        assert_ne!(a, b);
        assert!(a.starts_with(std::env::temp_dir()));
        assert_eq!(
            stop_sentinel(&a).extension().and_then(|e| e.to_str()),
            Some("stop")
        );
    }

    // [PROF-CPU-COOPERATIVE] Header handshake, tick streaming (including a
    // record split across two flushes), drain on stop, and cleanup.
    #[tokio::test(flavor = "multi_thread")]
    async fn tails_ticks_and_cleans_up() {
        let path = mint_sample_file();
        write_lines(
            &path,
            &[r#"{"header":{"runtime_version":"9.9.9","pid":4321}}"#],
        )
        .expect("write cooperative sampler header");
        let mut handle = tail(path.clone())
            .await
            .expect("attach cooperative sampler");
        assert_eq!(
            (handle.pid, handle.runtime_version.as_str()),
            (4321, "9.9.9")
        );
        write_lines(
            &path,
            &[r#"{"ticks":[[7,true,[["/app/x.py",12,"hot"],["/app/x.py",3,"main"]]]]}"#],
        )
        .expect("write complete cooperative sampler tick");
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open sample file for a split tick");
        write!(file, r#"{{"ticks":[[7,false,[["/app/y.py","#)
            .expect("write first half of split tick");
        file.flush().expect("flush first half of split tick");
        tokio::time::sleep(Duration::from_millis(300)).await;
        writeln!(file, r#"5,"idle"]]]]}}"#).expect("write second half of split tick");
        file.flush().expect("flush second half of split tick");
        let first = handle.receiver.recv().await.expect("receive first tick");
        let second = handle
            .receiver
            .recv()
            .await
            .expect("receive rejoined second tick");
        handle.stop(|_batch| {}).await;
        let names: Vec<&str> = first
            .samples
            .iter()
            .flat_map(|s| s.frames.iter().map(|f| f.name.as_str()))
            .collect();
        assert_eq!(names, vec!["hot", "main"]);
        assert_eq!(
            second.samples.first().map(|s| (s.active, s.frames.len())),
            Some((false, 1))
        );
        assert!(!path.exists(), "sample file is removed after stop");
        assert!(
            !stop_sentinel(&path).exists(),
            "sentinel is removed after stop"
        );
    }

    #[tokio::test]
    async fn missing_header_fails_with_a_hint() {
        tokio::time::pause();
        let path = mint_sample_file();
        let attempt = tokio::spawn(tail(path));
        tokio::time::advance(HEADER_TIMEOUT + Duration::from_secs(1)).await;
        let result = attempt.await.map_err(|e| e.to_string());
        assert!(
            matches!(&result, Ok(Err(SamplerError::AttachFailed(message))) if message.contains("never reported its header")),
            "got {result:?}"
        );
    }
}
