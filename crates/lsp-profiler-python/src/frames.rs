//! Python runtime plumbing frames. Implements [PROF-PY-SCAFFOLD].

use lsp_profiler::wire::StackFrame;
use lsp_profiler::FrameFilter;

/// Strips the runpy/debugpy/pydevd launcher spine, the stdlib thread
/// bootstrap, and the injected sampler's own frames.
#[derive(Debug, Clone, Copy, Default)]
pub struct PythonFrames;

impl FrameFilter for PythonFrames {
    fn is_scaffolding(&self, frame: &StackFrame) -> bool {
        frame.file.as_deref().is_some_and(|file| is_runtime_scaffolding(file, &frame.name))
    }
}

/// Whether a frame is interpreter/debugger scaffolding rather than code the
/// user (or a library they call) wrote: the runpy/debugpy launcher spine that
/// wraps every debug-launched program, pydevd tracer frames, and the injected
/// cooperative sampler's own frames. Anchored matching only — basename prefix
/// or exact path segment, never a full-path substring — so a user file under
/// `debugpy_utils/` is never mistaken for the debugger (mirrors the memory
/// profiler's `_is_runtime_glue`, [PROFILE-MEMORY-FINAL]).
///
/// `<string>` is matched on the FUNCTION, not the filename: `python -c`,
/// `exec`, `eval`, and the REPL all run legitimate user code with
/// `co_filename == "<string>"`, so only the sampler's `__lspprof*`/`_lspprof*`
/// frames there are scaffolding. Blanket-stripping every `<string>` frame
/// discarded real user samples — a `python -c` workload profiled to zero
/// (#251 follow-up).
fn is_runtime_scaffolding(filename: &str, function: &str) -> bool {
    if filename == "<frozen runpy>" {
        return true;
    }
    if filename == "<string>" {
        return function.starts_with("__lspprof") || function.starts_with("_lspprof");
    }
    let normalized = filename.replace('\\', "/");
    let basename = normalized.rsplit('/').next().unwrap_or(&normalized);
    if basename == "runpy.py"
        || basename.starts_with("pydevd")
        || basename.starts_with("debugpy")
        || basename.starts_with("_pydev")
    {
        return true;
    }
    // The stdlib thread-bootstrap spine. Every non-main thread starts inside
    // `Thread._bootstrap`, so keeping it roots each thread's flame chart three
    // rows above the code the user wrote, and — because the debugger's own
    // housekeeping threads are started by the stdlib too — leaves a
    // debugpy-only thread non-empty, so it survives as pure machinery instead
    // of being dropped. Only the bootstrap trio qualifies: a genuine
    // `threading` wait (`join`, `acquire`) is behaviour the user asked about,
    // and a user's own `run` override lives in the user's file, not here.
    if basename == "threading.py" {
        return matches!(function, "_bootstrap" | "_bootstrap_inner" | "run");
    }
    normalized
        .split('/')
        .any(|segment| segment == "debugpy" || segment == "pydevd")
}


#[cfg(test)]
mod tests {
    use super::*;

    fn frame(file: &str, name: &str) -> StackFrame {
        StackFrame { name: name.to_owned(), file: Some(file.to_owned()), line: Some(1), module: None }
    }

    #[test]
    fn strips_launcher_debugger_and_bootstrap_frames() {
        for (file, name) in [
            ("<frozen runpy>", "_run_code"),
            ("/usr/lib/python3.12/runpy.py", "_run_module_as_main"),
            ("/x/site-packages/debugpy/server/cli.py", "main"),
            ("/x/pydevd.py", "run"),
            ("/x/_pydevd_bundle/pydevd_trace.py", "trace"),
            ("C:\\py\\Lib\\threading.py", "_bootstrap_inner"),
            ("/usr/lib/python3.12/threading.py", "run"),
            ("<string>", "__lspprof_cpu_start"),
            ("<string>", "_lspprof_emit"),
        ] {
            assert!(PythonFrames.is_scaffolding(&frame(file, name)), "{file}:{name} is scaffolding");
        }
    }

    // Anchored matching: user code that merely resembles plumbing survives.
    #[test]
    fn keeps_user_code_that_resembles_plumbing() {
        for (file, name) in [
            ("/home/me/debugpy_utils/helper.py", "work"),
            ("/home/me/my_pydevd_tools/x.py", "work"),
            ("/usr/lib/python3.12/threading.py", "join"),
            ("/home/me/app/threading.py", "helper"),
            ("<string>", "user_function"),
            ("/home/me/app/runpy_tool.py", "main"),
        ] {
            assert!(!PythonFrames.is_scaffolding(&frame(file, name)), "{file}:{name} is user code");
        }
        let unlocated = StackFrame { name: "x".to_owned(), file: None, line: None, module: None };
        assert!(!PythonFrames.is_scaffolding(&unlocated));
    }
}
