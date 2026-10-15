//! Python process recognition. Implements [PROF-PY-PROCESSES].
//!
//! Recognises interpreters by name, executable and `argv[0]`; labels each
//! with its script, launcher framework and version; marks debugger machinery
//! as non-attachable (listed, never hidden); and decides whether attaching
//! needs elevation. Version probing is cached per interpreter and bounded per
//! enumeration.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::Path;

use lsp_profiler::{Classification, ProcessCandidate};

/// Maximum number of distinct interpreter binaries we will invoke with
/// `--version` per enumeration. Bounds the cost of version resolution when many
/// virtualenvs are running; beyond this, versions fall back to the path pattern.
const VERSION_RESOLVE_BUDGET: u32 = 32;

/// Console-script / module launchers that run *on* a Python interpreter. When a
/// process matches one, its `launcher` field carries the framework name so the
/// panel can render a `[uvicorn]`-style chip ([PROFILE-PROCESSES-DISPLAY]).
const LAUNCHERS: &[&str] = &[
    "uvicorn",
    "gunicorn",
    "pytest",
    "celery",
    "flask",
    "hypercorn",
    "daphne",
    "uwsgi",
    "sanic",
];

/// Reason a process cannot be profiled, surfaced in the panel tooltip and
/// driving the 🚫 marker / greying ([PROFILE-PROCESSES-MODEL] debuggability).
/// Elevation is deliberately *not* here — an other-user process is still
/// profilable via the privilege helper, so it stays debuggable with a lock hint.
const REASON_MACHINERY: &str = "debugger machinery";
const REASON_NO_INTERPRETER: &str = "interpreter path could not be resolved";

/// Classify every Python process among `candidates`.
#[must_use]
pub fn classify(candidates: &[ProcessCandidate]) -> Vec<(usize, Classification)> {
    let mut cache: HashMap<String, Option<String>> = HashMap::new();
    let mut budget = VERSION_RESOLVE_BUDGET;
    candidates
        .iter()
        .enumerate()
        .filter(|(_, candidate)| is_python(candidate))
        .map(|(index, candidate)| (index, classify_one(candidate, &mut cache, &mut budget)))
        .collect()
}

/// Whether the name, executable or `argv[0]` names a Python interpreter.
fn is_python(candidate: &ProcessCandidate) -> bool {
    let argv0 = candidate.cmd.first().map(|arg| arg.to_string_lossy().into_owned());
    is_python_interpreter(file_basename(&candidate.name))
        || is_python_interpreter(&candidate.exe_basename())
        || argv0.as_deref().map(file_basename).is_some_and(is_python_interpreter)
}

/// Label one Python process.
///
/// A debuggee (debugpy running the developer's own program) is a real target
/// labelled with that program; every other debugpy/pydevd process is
/// machinery — listed, but not attachable and carrying no script.
fn classify_one(candidate: &ProcessCandidate, cache: &mut HashMap<String, Option<String>>, budget: &mut u32) -> Classification {
    let cmd = candidate.cmd.as_slice();
    let executable = candidate
        .exe
        .as_deref()
        .map(|exe| exe.display().to_string())
        .or_else(|| cmd.first().map(|arg| arg.to_string_lossy().into_owned()));
    let debuggee_program = debugpy_debuggee_program(cmd);
    let is_debuggee = debuggee_program.is_some();
    let is_machinery = !is_debuggee && is_debugger_infrastructure(cmd);
    let entry = if is_machinery { None } else { debuggee_program.or_else(|| extract_script(cmd)) };
    Classification {
        runtime_version: executable.as_deref().and_then(|exe| resolve_python_version(exe, cache, budget)),
        launcher: classify_launcher(cmd, entry.as_deref()),
        requires_elevation: requires_elevation_to_profile(is_debuggee, candidate.child_of_current, candidate.owned_by_other_user),
        unattachable_reason: undebuggable_reason(is_machinery, executable.is_some()),
        executable,
        entry,
    }
}

/// Whether profiling this process from the panel would need elevation, matching
/// the attach-time rule in [`super::privilege`] ([PROFILE-PERMISSIONS]):
///
/// - A **debuggee** the editor launched is sampled cooperatively / as a child, and
///   a process we **directly spawned** can be traced by its parent — neither
///   needs elevation, on any platform.
/// - Otherwise on **macOS** `vm_read`/`task_for_pid` always needs root for an
///   external process — even a same-user one started in another terminal — so it
///   requires elevation.
/// - On **Linux/Windows** only *another user's* process needs elevation
///   (same-user attach works via `ptrace`/`ReadProcessMemory`).
///
/// Drives the panel's lock badge + "needs elevation" tooltip. The UID-only
/// signal it replaces was wrong on macOS, where a same-user external process
/// still needs elevation (the "permission denied on Profile CPU" report).
/// Implements [PROFILE-PROCESSES-MODEL].
fn requires_elevation_to_profile(
    is_debuggee: bool,
    is_child_of_current: bool,
    owned_by_other_user: bool,
) -> bool {
    if is_debuggee || is_child_of_current {
        false
    } else if cfg!(target_os = "macos") {
        true
    } else {
        owned_by_other_user
    }
}

/// The reason a process cannot be profiled, or `None` when it is debuggable.
/// Machinery wins over a missing interpreter so the tooltip names the most
/// specific blocker. Elevation is *not* a blocker — an other-user (or external
/// macOS) process stays debuggable via the privilege helper. Implements
/// [PROFILE-PROCESSES-MODEL] (debuggability).
fn undebuggable_reason(is_machinery: bool, has_interpreter: bool) -> Option<String> {
    if is_machinery {
        Some(REASON_MACHINERY.to_owned())
    } else if !has_interpreter {
        Some(REASON_NO_INTERPRETER.to_owned())
    } else {
        None
    }
}

/// Return the final path component of `path`, or `path` itself if it has none.
fn file_basename(path: &str) -> &str {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
}






/// Whether `basename` names a Python (or `PyPy`) interpreter: `python`,
/// `python3`, `pythonX.Y`, `pypy`, with an optional `.exe` suffix.
fn is_python_interpreter(basename: &str) -> bool {
    let trimmed = basename.strip_suffix(".exe").unwrap_or(basename);
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("pypy") {
        return true;
    }
    match lower.strip_prefix("python") {
        Some(rest) => rest.chars().all(|ch| ch.is_ascii_digit() || ch == '.'),
        None => false,
    }
}

/// Best-effort target script: the first positional argument after the
/// interpreter, skipping flags and the values of value-taking flags.
///
/// `-c <code>` and `-m <module>` END the interpreter's own option list — every
/// following token is the command's/module's argv, so such an invocation has
/// no script at all. Scanning past them mislabelled the debugpy adapter's
/// `--port 50581` value as the script (#268).
fn extract_script(cmd: &[OsString]) -> Option<String> {
    let mut skip_next = false;
    for arg in cmd.iter().skip(1) {
        let token = arg.to_string_lossy();
        if skip_next {
            skip_next = false;
            continue;
        }
        if token.starts_with('-') {
            if matches!(token.as_ref(), "-c" | "-m") {
                return None;
            }
            if matches!(token.as_ref(), "-W" | "-X") {
                skip_next = true;
            }
            continue;
        }
        return Some(token.into_owned());
    }
    None
}

/// The launcher framework a Python process runs (`uvicorn`, `pytest`, …), or
/// `None` for a bare interpreter. Looks at both `-m <module>` invocations and the
/// target script's basename. Implements [PROFILE-PROCESSES-MODEL].
fn classify_launcher(cmd: &[OsString], script: Option<&str>) -> Option<String> {
    if let Some(module) = module_arg(cmd) {
        let first_segment = module.split('.').next().unwrap_or(&module);
        if LAUNCHERS.contains(&first_segment) {
            return Some(first_segment.to_owned());
        }
    }
    if let Some(path) = script {
        let base = file_basename(path);
        let stem = base.strip_suffix(".py").unwrap_or(base);
        if LAUNCHERS.contains(&stem) {
            return Some(stem.to_owned());
        }
    }
    None
}



/// Debugger machinery module prefixes. Such processes are still listed
/// (zero-filter), but flagged non-debuggable.
const INFRASTRUCTURE_MODULES: &[&str] = &["debugpy", "pydevd"];

/// Whether this Python process is debugger machinery rather than a profilable
/// target: `python -m debugpy.adapter`, `-m pydevd`, or the debugpy/pydevd
/// launcher/adapter living inside the package directory. Drives the
/// `debuggable = false` marking ([PROFILE-PROCESSES-MODEL]); the process is NOT
/// hidden.
///
/// The caller exempts a *debuggee* ([`debugpy_debuggee_program`]) first, so the
/// user's own program running under the bundled debugpy — whose path also
/// contains `/debugpy/` — stays debuggable rather than being mistaken for
/// machinery by the path check.
fn is_debugger_infrastructure(cmd: &[OsString]) -> bool {
    if let Some(module) = module_arg(cmd) {
        let first_segment = module.split('.').next().unwrap_or(&module);
        if INFRASTRUCTURE_MODULES.contains(&first_segment) {
            return true;
        }
    }
    cmd.iter().skip(1).any(|arg| {
        let token = arg.to_string_lossy();
        INFRASTRUCTURE_MODULES.iter().any(|module| {
            token.contains(&format!("/{module}/")) || token.contains(&format!("\\{module}\\"))
        })
    })
}

/// Flags in a debugpy debuggee invocation that consume the following argument,
/// so the user's program is never mistaken for one of their values (e.g. the
/// `127.0.0.1:5679` after `--connect`). Mirrors debugpy's launcher cmdline.
const DEBUGPY_VALUE_FLAGS: &[&str] = &[
    "--connect",
    "--listen",
    "--host",
    "--port",
    "--adapter-access-token",
];

/// If `cmd` runs the developer's own program *under* debugpy
/// (`python <…>/debugpy --connect <addr> … <program>`, the exact shape VS Code's
/// debugger launches), return that program. Such a process is a real debuggee —
/// the thing the user wants to see and profile — not debugger machinery, even
/// though its argv references the bundled `debugpy` package (whose path contains
/// `/debugpy/`). Implements [PROFILE-PROCESSES-MODEL] (debuggee surfacing).
fn debugpy_debuggee_program(cmd: &[OsString]) -> Option<String> {
    let entry = debugpy_entry_index(cmd)?;
    let mut skip_next = false;
    for arg in cmd.iter().skip(entry + 1) {
        let token = arg.to_string_lossy();
        if skip_next {
            skip_next = false;
            continue;
        }
        if token.starts_with('-') {
            if DEBUGPY_VALUE_FLAGS.contains(&token.as_ref()) || token.starts_with("--configure-") {
                skip_next = true;
            }
            continue;
        }
        return Some(token.into_owned());
    }
    None
}

/// Index of the `debugpy` package entry in `cmd`: the `<…>/debugpy` directory run
/// directly (basename `debugpy`) or `-m debugpy`. The `launcher`/`adapter`
/// submodules have a different basename and are deliberately *not* matched —
/// those are machinery, caught by [`is_debugger_infrastructure`].
fn debugpy_entry_index(cmd: &[OsString]) -> Option<usize> {
    let mut expect_module = false;
    for (index, arg) in cmd.iter().enumerate().skip(1) {
        let token = arg.to_string_lossy();
        if expect_module {
            return (token == "debugpy").then_some(index);
        }
        if token == "-m" {
            expect_module = true;
        } else if file_basename(&token) == "debugpy" {
            return Some(index);
        }
    }
    None
}

/// Extract the module name from a `python -m <module>` invocation, if present.
fn module_arg(cmd: &[OsString]) -> Option<String> {
    let mut expect_module = false;
    for arg in cmd {
        let token = arg.to_string_lossy();
        if expect_module {
            return Some(token.into_owned());
        }
        if token == "-m" {
            expect_module = true;
        }
    }
    None
}

/// Resolve a Python version string for `exe`, caching per executable path.
///
/// Implements [PROFILE-PROCESSES-MODEL] (version resolution): an exact version
/// from `<exe> --version` (cached per interpreter, bounded by `budget` per
/// enumeration), falling back to the `pythonX.Y` path pattern, then `null`.
fn resolve_python_version(
    exe: &str,
    cache: &mut HashMap<String, Option<String>>,
    budget: &mut u32,
) -> Option<String> {
    if let Some(cached) = cache.get(exe) {
        return cached.clone();
    }

    let resolved = if *budget > 0 {
        *budget -= 1;
        version_via_command(exe).or_else(|| version_from_basename(file_basename(exe)))
    } else {
        version_from_basename(file_basename(exe))
    };

    let _ = cache.insert(exe.to_owned(), resolved.clone());
    resolved
}

/// Run `<exe> --version` and parse the reported version (stdout or stderr).
///
/// Public so a host can probe its own resolved interpreter (or a tool such as
/// `uv`) with the same parser.
pub fn version_via_command(exe: &str) -> Option<String> {
    let output = std::process::Command::new(exe)
        .arg("--version")
        .output()
        .ok()?;
    let stream = if output.stdout.is_empty() {
        output.stderr
    } else {
        output.stdout
    };
    parse_version_output(&String::from_utf8_lossy(&stream))
}

/// Extract the first dotted version token from a `--version` line — handles
/// `Python 3.12.13`, `PyPy`'s `Python 3.9.18 [PyPy …]`, and `uv 0.5.11 (…)`.
fn parse_version_output(text: &str) -> Option<String> {
    text.split_whitespace()
        .find(|token| {
            let starts_with_digit = token.chars().next().is_some_and(|ch| ch.is_ascii_digit());
            starts_with_digit && token.contains('.')
        })
        .map(str::to_owned)
}

/// Derive `major.minor` from a `pythonX.Y` interpreter basename, if it encodes
/// a version. Returns `None` for unversioned names like `python` or `python3`.
fn version_from_basename(basename: &str) -> Option<String> {
    let trimmed = basename.strip_suffix(".exe").unwrap_or(basename);
    let rest = trimmed.strip_prefix("python")?;
    let mut parts = rest.split('.');
    let major = numeric_segment(parts.next())?;
    let minor = numeric_segment(parts.next())?;
    Some(format!("{major}.{minor}"))
}

/// Return the segment if it is a non-empty run of ASCII digits.
fn numeric_segment(segment: Option<&str>) -> Option<&str> {
    segment.filter(|value| !value.is_empty() && value.chars().all(|ch| ch.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_python_interpreter_names() {
        assert!(is_python_interpreter("python"));
        assert!(is_python_interpreter("python3"));
        assert!(is_python_interpreter("python3.12"));
        assert!(is_python_interpreter("python.exe"));
        assert!(is_python_interpreter("pypy3"));
        assert!(!is_python_interpreter("pythonista"));
        assert!(!is_python_interpreter("node"));
        assert!(!is_python_interpreter("ws_features_tests"));
    }

    #[test]
    fn version_from_basename_needs_major_and_minor() {
        assert_eq!(version_from_basename("python3.12"), Some("3.12".to_owned()));
        assert_eq!(version_from_basename("python3"), None);
        assert_eq!(version_from_basename("python"), None);
    }

    #[test]
    fn parses_version_output_from_either_stream() {
        assert_eq!(
            parse_version_output("Python 3.12.13"),
            Some("3.12.13".to_owned())
        );
        assert_eq!(
            parse_version_output("Python 3.9.18\n[PyPy 7.3.13]"),
            Some("3.9.18".to_owned())
        );
        assert_eq!(parse_version_output("garbage"), None);
        // uv-style banner ([LSPARCH-RESOLVED-ENV] probes uv with this parser).
        assert_eq!(
            parse_version_output("uv 0.5.11 (Homebrew 2025-01-01)"),
            Some("0.5.11".to_owned())
        );
    }

    #[test]
    fn extracts_script_and_skips_flag_values() {
        let cmd = vec![
            OsString::from("python3"),
            OsString::from("-X"),
            OsString::from("utf8"),
            OsString::from("app.py"),
        ];
        assert_eq!(extract_script(&cmd), Some("app.py".to_owned()));

        let dash_c = vec![
            OsString::from("python3"),
            OsString::from("-c"),
            OsString::from("print(1)"),
        ];
        assert_eq!(extract_script(&dash_c), None);
    }

    #[test]
    fn module_invocation_never_reports_a_flag_value_as_the_script() {
        // The exact argv of the debugpy adapter VS Code spawns (#268): the
        // panel rendered "Python — 50581", labelling the adapter's TCP port as
        // if it were the target script (and click-to-open then targeted a file
        // literally named `50581`). After `-m <module>` the interpreter's own
        // option list is over — every following token belongs to the module —
        // so a module invocation has no script at all.
        let adapter = vec![
            OsString::from("/usr/bin/python3"),
            OsString::from("-m"),
            OsString::from("debugpy.adapter"),
            OsString::from("--port"),
            OsString::from("50581"),
        ];
        assert_eq!(
            extract_script(&adapter),
            None,
            "a flag value after `-m <module>` must never be labelled the script (#268)"
        );

        // Same rule for `-c`: trailing tokens are the command's argv
        // (`sys.argv[1:]`), not a script.
        let dash_c_with_args = vec![
            OsString::from("python3"),
            OsString::from("-c"),
            OsString::from("import sys; print(sys.argv)"),
            OsString::from("50581"),
        ];
        assert_eq!(
            extract_script(&dash_c_with_args),
            None,
            "arguments after `-c <code>` must never be labelled the script (#268)"
        );
    }

    #[test]
    fn classify_launcher_names_the_framework() {
        let module = vec![
            OsString::from("python3"),
            OsString::from("-m"),
            OsString::from("uvicorn"),
        ];
        assert_eq!(classify_launcher(&module, None).as_deref(), Some("uvicorn"));

        let script = vec![OsString::from("python3"), OsString::from("/srv/pytest.py")];
        assert_eq!(
            classify_launcher(&script, Some("/srv/pytest.py")).as_deref(),
            Some("pytest")
        );

        let plain = vec![OsString::from("python3"), OsString::from("main.py")];
        assert_eq!(classify_launcher(&plain, Some("main.py")), None);
    }

    #[test]
    fn undebuggable_reason_flags_machinery_then_missing_interpreter() {
        // Machinery wins even when it also lacks a resolvable interpreter.
        assert_eq!(
            undebuggable_reason(true, false).as_deref(),
            Some(REASON_MACHINERY)
        );
        // A missing interpreter is the only other blocker.
        assert_eq!(
            undebuggable_reason(false, false).as_deref(),
            Some(REASON_NO_INTERPRETER)
        );
        // A resolvable interpreter (even one needing elevation) is debuggable —
        // elevation is a lock hint, not a blocker.
        assert_eq!(undebuggable_reason(false, true), None);
    }

    #[test]
    fn elevation_rule_exempts_debuggees_and_direct_children() {
        // A debuggee (cooperative sampler) or a process we directly spawned
        // (parent can trace it) never needs elevation, on any platform.
        assert!(!requires_elevation_to_profile(true, false, false));
        assert!(!requires_elevation_to_profile(false, true, false));
        assert!(!requires_elevation_to_profile(true, true, true));
    }

    #[test]
    fn elevation_rule_for_external_processes_is_platform_specific() {
        // An external process (not a debuggee, not our child).
        let same_user = requires_elevation_to_profile(false, false, false);
        let other_user = requires_elevation_to_profile(false, false, true);
        if cfg!(target_os = "macos") {
            // macOS needs root (vm_read) for ANY external process — even a
            // same-user one started in another terminal (the Flask-server report).
            assert!(
                same_user,
                "macOS: a same-user external process still needs elevation"
            );
            assert!(
                other_user,
                "macOS: another user's external process needs elevation"
            );
        } else {
            // Linux/Windows: a same-user external process attaches without
            // elevation; only another user's process needs it.
            assert!(
                !same_user,
                "same-user external process attaches without elevation"
            );
            assert!(other_user, "another user's process needs elevation");
        }
    }

    #[test]
    fn debug_machinery_is_marked_not_filtered() {
        // The adapter the editor spawns is machinery (no user program) — it is no
        // longer hidden; build_process_info marks it `debuggable = false`. The
        // `is_machinery` predicate it relies on is debuggee-absent + infra.
        let adapter = vec![
            OsString::from("python"),
            OsString::from("-m"),
            OsString::from("debugpy.adapter"),
            OsString::from("--port"),
            OsString::from("0"),
        ];
        let adapter_is_machinery =
            debugpy_debuggee_program(&adapter).is_none() && is_debugger_infrastructure(&adapter);
        assert!(
            adapter_is_machinery,
            "the adapter must be flagged machinery"
        );

        // A debuggee running the user's program is NOT machinery — it stays
        // debuggable and surfaced.
        let debuggee = vec![
            OsString::from("/usr/bin/python3"),
            OsString::from("/ext/bundled/debugpy/debugpy"),
            OsString::from("--connect"),
            OsString::from("127.0.0.1:5679"),
            OsString::from("/workspace/cpu_demo.py"),
        ];
        let debuggee_is_machinery =
            debugpy_debuggee_program(&debuggee).is_none() && is_debugger_infrastructure(&debuggee);
        assert!(
            !debuggee_is_machinery,
            "the user's debuggee program must not be flagged machinery"
        );
    }

    #[test]
    fn surfaces_debugpy_debuggee_running_user_program() {
        // The exact argv VS Code's debugger produces with the *bundled* debugpy:
        // the package nests at `.../debugpy/debugpy`, so the path contains
        // `/debugpy/` — the over-broad infra filter used to hide the user's own
        // running script. It must now be surfaced and labelled with the program.
        let cmd = vec![
            OsString::from("/usr/bin/python3"),
            OsString::from("/ext/bundled/debugpy/debugpy"),
            OsString::from("--connect"),
            OsString::from("127.0.0.1:5679"),
            OsString::from("--adapter-access-token"),
            OsString::from("deadbeef"),
            OsString::from("/workspace/cpu_demo.py"),
        ];
        assert_eq!(
            debugpy_debuggee_program(&cmd),
            Some("/workspace/cpu_demo.py".to_owned()),
            "the debuggee's user program must be recovered past debugpy's flags"
        );
        assert!(
            !is_debugger_infrastructure(&cmd) || debugpy_debuggee_program(&cmd).is_some(),
            "a debuggee running the user's program must not be hidden as infrastructure"
        );
    }

    #[test]
    fn debuggee_program_handles_listen_and_module_forms() {
        // `--listen` server form (basename `debugpy`).
        let listen = vec![
            OsString::from("python"),
            OsString::from("/ext/bundled/debugpy/debugpy"),
            OsString::from("--listen"),
            OsString::from("127.0.0.1:0"),
            OsString::from("--wait-for-client"),
            OsString::from("/ws/app.py"),
        ];
        assert_eq!(
            debugpy_debuggee_program(&listen),
            Some("/ws/app.py".to_owned())
        );

        // `-m debugpy` module form.
        let module = vec![
            OsString::from("python"),
            OsString::from("-m"),
            OsString::from("debugpy"),
            OsString::from("--connect"),
            OsString::from("127.0.0.1:1"),
            OsString::from("/ws/main.py"),
        ];
        assert_eq!(
            debugpy_debuggee_program(&module),
            Some("/ws/main.py".to_owned())
        );
    }

    #[test]
    fn launcher_and_adapter_remain_infrastructure() {
        // The launcher (`.../debugpy/launcher`) carries no user program and must
        // stay hidden.
        let launcher = vec![
            OsString::from("python"),
            OsString::from("/ext/bundled/debugpy/debugpy/launcher"),
            OsString::from("53412"),
        ];
        assert_eq!(debugpy_debuggee_program(&launcher), None);
        assert!(is_debugger_infrastructure(&launcher));

        // The adapter the editor itself spawns.
        let adapter = vec![
            OsString::from("python"),
            OsString::from("-m"),
            OsString::from("debugpy.adapter"),
            OsString::from("--port"),
            OsString::from("0"),
        ];
        assert_eq!(debugpy_debuggee_program(&adapter), None);
        assert!(is_debugger_infrastructure(&adapter));
    }







}
