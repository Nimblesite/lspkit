//! The cooperative Python sampler injected into a paused debuggee.
//! Implements [PROF-PY-CPU-COOPERATIVE].
//!
//! On macOS reading another process's memory needs a task port Apple gates
//! behind signed, debugger-entitled callers, so for debug-launched sessions
//! the editor injects this daemon thread instead. It samples
//! `sys._current_frames()` and streams the core's cooperative JSONL format
//! ([`lsp_profiler::cooperative`]), which the core tails like any sampler.

use std::path::Path;

use lsp_profiler::SamplerError;

/// The Python injected into the debuggee. `@SAMPLE_FILE@` / `@RATE@` are
/// replaced before injection; the printed marker is the courier's ack.
const SCRIPT_TEMPLATE: &str = r#"def __lspprof_cpu_start():
    import json, os, sys, threading, time
    path = @SAMPLE_FILE@
    rate = @RATE@
    interval = 1.0 / max(rate, 1)
    stop_path = path + ".stop"
    wait_files = ("threading.py", "selectors.py", "queue.py", "socket.py", "ssl.py", "subprocess.py")

    def base(name):
        return name.replace("\\", "/").rsplit("/", 1)[-1]

    def is_debugger(name):
        # ANCHORED matching only, mirroring the Rust-side
        # is_runtime_scaffolding: debugger basename prefix or an exact
        # debugpy/pydevd path segment — never a full-path substring, so a
        # user file under my_pydevd_tools/ is never mistaken for the tracer.
        norm = name.replace("\\", "/")
        if base(norm).startswith(("pydevd", "debugpy", "_pydev")):
            return True
        return "/debugpy/" in norm or "/pydevd/" in norm

    def clean(text):
        # An undecodable filesystem name reaches co_filename as lone
        # surrogates; json.dumps escapes them, but strict JSON parsers
        # (the Rust tailer) reject those escapes — one weird path would
        # silently poison every tick its thread appears in. Scrub instead.
        return text if text.isascii() else text.encode("utf-8", "replace").decode("utf-8")

    def snap(me):
        ticks = []
        for tid, top in sys._current_frames().items():
            if tid == me:
                continue
            frames = []
            frame = top
            while frame is not None and len(frames) < 128:
                code = frame.f_code
                frames.append([clean(code.co_filename), frame.f_lineno, clean(code.co_name)])
                frame = frame.f_back
            # Drop `<string>` machinery frames. The cooperative sampler only
            # runs in a debug-launched session, where the program is always a
            # real file (`program: <path>`), so a `<string>` frame is never the
            # user's code — it is this injected script's own frames or a
            # pydevd/frame-eval exec wrapper (which appears on CI under line
            # tracing). The py-spy attach path CAN legitimately profile
            # `python -c` user code in `<string>`, so that path keeps it — that
            # filtering lives in the Rust aggregator, not here.
            frames = [f for f in frames if f[0] != "<string>"]
            # Attribute tracer overhead to the user code being traced: the
            # sys.settrace callbacks (pydevd/debugpy) sit on TOP of the user
            # frame, so drop leading debugger frames instead of discarding
            # the sample — otherwise traced hot code is starved.
            while frames and is_debugger(frames[0][0]):
                frames.pop(0)
            if not frames:
                continue
            # Idle means the leaf is parked in a stdlib wait module. Match the
            # BASENAME exactly — a suffix match reads user files like
            # websocket.py or task_queue.py as waiters and hides their threads
            # from the profile entirely.
            active = base(frames[0][0]) not in wait_files
            ticks.append([tid, active, frames])
        return ticks

    def run():
        me = threading.get_ident()
        with open(path, "a", encoding="utf-8") as out:
            header = {"runtime_version": "%d.%d.%d" % sys.version_info[:3], "pid": os.getpid()}
            out.write(json.dumps({"header": header}) + "\n")
            out.flush()
            last_flush = time.monotonic()
            next_tick = time.monotonic()
            while not os.path.exists(stop_path):
                try:
                    out.write(json.dumps({"ticks": snap(me)}) + "\n")
                except Exception:
                    pass  # one bad sample must never kill the whole session
                now = time.monotonic()
                if now - last_flush >= 0.5:
                    out.flush()
                    last_flush = now
                # Deadline pacing: plain sleep(interval) adds the stack-walk
                # time to every period, silently under-sampling below the
                # configured rate (and overstating each sample's weight).
                next_tick += interval
                delay = next_tick - time.monotonic()
                if delay > 0:
                    time.sleep(delay)
                else:
                    next_tick = time.monotonic()
            out.flush()

    threading.Thread(target=run, name="lspprof-cpu-sampler", daemon=True).start()
    print("__LSPPROF_CPU_ACK__")


__lspprof_cpu_start()
"#;

/// Render the injection script for one session.
///
/// # Errors
///
/// Returns an error if the sample-file path cannot be JSON-encoded.
pub fn sampling_script(sample_file: &Path, sample_rate: u64) -> Result<String, SamplerError> {
    let path_json = serde_json::to_string(&sample_file.display().to_string())
        .map_err(|err| SamplerError::AttachFailed(format!("encode sample path: {err}")))?;
    Ok(SCRIPT_TEMPLATE
        .replace("@SAMPLE_FILE@", &path_json)
        .replace("@RATE@", &sample_rate.to_string()))
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_embeds_path_and_rate_as_literals() -> Result<(), SamplerError> {
        let script = sampling_script(Path::new("/tmp/it's \"here\".jsonl"), 250)?;
        assert!(script.contains(r#"path = "/tmp/it's \"here\".jsonl""#), "path must be a JSON/Python string literal");
        assert!(script.contains("rate = 250"));
        assert!(script.contains(r#"header = {"runtime_version":"#), "header uses the core's neutral field");
        assert!(!script.contains("@SAMPLE_FILE@") && !script.contains("@RATE@"));
        Ok(())
    }
}
