//! OS process discovery shared by every runtime. Implements [PROF-PROCESSES].
//!
//! Enumeration only *reads* the process table (via `sysinfo`), so discovery
//! never needs elevation. Each process becomes a [`ProcessCandidate`] that
//! every registered [`Runtime`] may recognise; the first runtime to claim a
//! process classifies it. Nothing is filtered by workspace — membership is a
//! display attribute ([PROF-PROCESSES-SCOPE]).
//!
//! Logging discipline: only counts are logged. Command lines and user names
//! may contain secrets or personal data and are never logged.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sysinfo::{
    get_current_pid, Pid, Process, ProcessRefreshKind, ProcessesToUpdate, System, Uid, UpdateKind,
    Users,
};
use tracing::info;

use crate::runtime::{Classification, ProcessCandidate, Runtime};
use crate::wire::ProcessInfo;

/// One recognised process and the runtime that claimed it.
#[derive(Debug, Clone)]
pub struct RecognisedProcess {
    /// Wire description.
    pub info: ProcessInfo,
    /// Index of the claiming runtime in the registry.
    pub runtime_index: usize,
}

/// The process table, refreshed twice so CPU percentages are meaningful.
struct ProcessTable {
    system: System,
    users: Users,
    current_uid: Option<Uid>,
    self_pid: Option<u32>,
    argv_fallback: HashMap<u32, Vec<OsString>>,
}

impl ProcessTable {
    /// Snapshot the table. Blocks for `sysinfo::MINIMUM_CPU_UPDATE_INTERVAL`,
    /// so call it from a blocking task.
    fn read() -> Self {
        let mut system = System::new();
        // The 2-arg refresh omits `cmd` and `cwd`; request them explicitly so
        // argv and the working directory are populated wherever readable.
        let refresh = ProcessRefreshKind::nothing()
            .with_memory()
            .with_cpu()
            .with_exe(UpdateKind::OnlyIfNotSet)
            .with_cmd(UpdateKind::OnlyIfNotSet)
            .with_cwd(UpdateKind::OnlyIfNotSet)
            .with_user(UpdateKind::OnlyIfNotSet);
        let _ = system.refresh_processes_specifics(ProcessesToUpdate::All, true, refresh);
        std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        let _ = system.refresh_processes_specifics(ProcessesToUpdate::All, true, refresh);
        let own_pid = get_current_pid().ok();
        let current_uid = own_pid
            .and_then(|pid| system.process(pid))
            .and_then(Process::user_id)
            .cloned();
        Self {
            system,
            users: Users::new_with_refreshed_list(),
            current_uid,
            self_pid: own_pid.map(Pid::as_u32),
            argv_fallback: argv_by_pid(),
        }
    }

    /// Every process except the language server itself, as candidates.
    fn candidates(&self) -> Vec<(Pid, ProcessCandidate)> {
        self.system
            .processes()
            .iter()
            .filter(|(pid, _)| Some(pid.as_u32()) != self.self_pid)
            .map(|(pid, process)| (*pid, self.candidate(*pid, process)))
            .collect()
    }

    fn candidate(&self, pid: Pid, process: &Process) -> ProcessCandidate {
        // sysinfo returns an empty argv for other users' processes on macOS;
        // the batched `ps` snapshot recovers it there.
        let cmd = if process.cmd().is_empty() {
            self.argv_fallback
                .get(&pid.as_u32())
                .cloned()
                .unwrap_or_default()
        } else {
            process.cmd().to_vec()
        };
        ProcessCandidate {
            pid: pid.as_u32(),
            ppid: process.parent().map(Pid::as_u32),
            name: process.name().to_string_lossy().into_owned(),
            exe: process.exe().map(Path::to_path_buf),
            cmd,
            cwd: process.cwd().map(Path::to_path_buf),
            owned_by_other_user: match (process.user_id(), self.current_uid.as_ref()) {
                (Some(owner), Some(current)) => owner != current,
                _ => false,
            },
            child_of_current: process.parent().map(Pid::as_u32) == self.self_pid
                && self.self_pid.is_some(),
        }
    }

    fn user_of(&self, process: &Process) -> Option<String> {
        process
            .user_id()
            .and_then(|uid| self.users.get_user_by_id(uid))
            .map(|owner| owner.name().to_owned())
    }
}

/// Enumerate every process any runtime recognises. Implements [PROF-PROCESSES].
///
/// Attachable processes sort first, then by CPU usage descending. Blocks for
/// ~200 ms; call from a blocking task.
#[must_use]
pub fn enumerate(runtimes: &[Arc<dyn Runtime>], roots: &[PathBuf]) -> Vec<RecognisedProcess> {
    let table = ProcessTable::read();
    let roots: Vec<PathBuf> = roots.iter().map(|root| normalize_path(root)).collect();
    let candidates = table.candidates();
    let bare: Vec<ProcessCandidate> = candidates.iter().map(|(_, c)| c.clone()).collect();
    let mut claimed: HashMap<usize, (usize, Classification)> = HashMap::new();
    for (runtime_index, runtime) in runtimes.iter().enumerate() {
        for (candidate_index, classification) in runtime.classify(&bare) {
            let _ = claimed
                .entry(candidate_index)
                .or_insert((runtime_index, classification));
        }
    }
    let mut processes: Vec<RecognisedProcess> = claimed
        .into_iter()
        .filter_map(|(candidate_index, (runtime_index, classification))| {
            let (pid, candidate) = candidates.get(candidate_index)?;
            let process = table.system.process(*pid)?;
            let runtime = runtimes.get(runtime_index)?;
            let info = to_info(
                &table,
                process,
                candidate,
                runtime.id(),
                classification,
                &roots,
            );
            Some(RecognisedProcess {
                info,
                runtime_index,
            })
        })
        .collect();
    processes.sort_by(|a, b| {
        b.info
            .attachable
            .cmp(&a.info.attachable)
            .then_with(|| b.info.cpu_percent.total_cmp(&a.info.cpu_percent))
    });
    info!(count = processes.len(), "enumerated profilable processes");
    processes
}

/// Find the runtime that recognises `pid`, if any.
#[must_use]
pub fn recognise(runtimes: &[Arc<dyn Runtime>], pid: u32) -> Option<RecognisedProcess> {
    enumerate(runtimes, &[])
        .into_iter()
        .find(|process| process.info.pid == i64::from(pid))
}

/// Build the wire description of one recognised process.
fn to_info(
    table: &ProcessTable,
    process: &Process,
    candidate: &ProcessCandidate,
    runtime: &str,
    classification: Classification,
    roots: &[PathBuf],
) -> ProcessInfo {
    let in_workspace = process_in_workspace(
        candidate.cwd.as_deref(),
        classification.entry.as_deref(),
        classification.executable.as_deref(),
        roots,
    );
    ProcessInfo {
        pid: i64::from(candidate.pid),
        ppid: candidate.ppid.map(i64::from),
        name: candidate.name.clone(),
        runtime: runtime.to_owned(),
        runtime_version: classification.runtime_version,
        executable: classification.executable,
        entry: classification.entry,
        cpu_percent: f64::from(process.cpu_usage()),
        memory_bytes: i64::try_from(process.memory()).unwrap_or(i64::MAX),
        runtime_secs: i64::try_from(process.run_time()).unwrap_or(i64::MAX),
        user: table.user_of(process),
        requires_elevation: classification.requires_elevation,
        in_workspace,
        launcher: classification.launcher,
        attachable: classification.unattachable_reason.is_none(),
        unattachable_reason: classification.unattachable_reason,
    }
}

/// Whether `pid` names a live process at all, recognised or not.
#[must_use]
pub fn exists(pid: u32) -> bool {
    let mut system = System::new();
    let target = Pid::from_u32(pid);
    let _ = system.refresh_processes(ProcessesToUpdate::Some(&[target]), true);
    system.process(target).is_some()
}

/// Terminate `pid` forcibly. Implements [PROF-PROCESSES-KILL].
///
/// # Errors
///
/// Returns a message when the process does not exist or refuses the signal.
pub fn kill(pid: u32) -> Result<(), String> {
    let mut system = System::new();
    let target = Pid::from_u32(pid);
    let _ = system.refresh_processes(ProcessesToUpdate::Some(&[target]), true);
    let process = system
        .process(target)
        .ok_or_else(|| format!("Process not found: PID {pid}"))?;
    if process.kill() {
        info!(pid, "terminated process");
        Ok(())
    } else {
        Err(format!("PID {pid} refused termination"))
    }
}

/// Whether a process whose working directory is `cwd`, target `script`, and
/// `interpreter` belongs to one of the (already canonicalized) workspace
/// `roots` — the green-row hint, not a filter. A process is a member when any of
/// those paths resolves to a location inside a root. With no roots — a
/// single-file or no-folder session — there is no workspace to belong to, so
/// nothing is a member. Implements [PROF-PROCESSES-SCOPE].
pub(crate) fn process_in_workspace(
    cwd: Option<&Path>,
    script: Option<&str>,
    interpreter: Option<&str>,
    roots: &[PathBuf],
) -> bool {
    if roots.is_empty() {
        return false;
    }
    workspace_candidate_paths(cwd, script, interpreter)
        .iter()
        .map(|candidate| normalize_path(candidate))
        .any(|candidate| path_in_roots(&candidate, roots))
}

/// The set of paths that tie a process to a workspace: its working directory,
/// its target script, and its interpreter. Relative script/interpreter paths
/// are resolved against `cwd` so `python app.py` launched from the project is
/// attributed to it.
fn workspace_candidate_paths(
    cwd: Option<&Path>,
    script: Option<&str>,
    interpreter: Option<&str>,
) -> Vec<PathBuf> {
    let cwd_candidate = cwd.map(Path::to_path_buf);
    let arg_candidates = [script, interpreter]
        .into_iter()
        .flatten()
        .map(|raw| resolve_against(raw, cwd));
    cwd_candidate.into_iter().chain(arg_candidates).collect()
}

/// Resolve `raw` to an absolute path: absolute paths pass through, relative ones
/// join onto `base` (the process working directory) when it is known.
fn resolve_against(raw: &str, base: Option<&Path>) -> PathBuf {
    let path = Path::new(raw);
    match base {
        Some(base) if path.is_relative() => base.join(path),
        _ => path.to_path_buf(),
    }
}

/// Whether `candidate` is equal to, or nested under, any of `roots`.
fn path_in_roots(candidate: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| candidate.starts_with(root))
}

/// Best-effort canonicalization so symlinked roots and `.`/`..` segments compare
/// correctly. Falls back to the input when the path cannot be resolved (e.g. a
/// script that has since been deleted).
fn normalize_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// One batched argv snapshot for the whole process table (macOS only).
///
/// sysinfo cannot read other processes' argv on macOS (`KERN_PROCARGS2` is
/// not exposed through it), so `cmd()` comes back empty and script labels,
/// launcher detection, and the infrastructure filter would all be blind. A
/// single `ps` call per enumeration recovers best-effort argv strings —
/// whitespace-split, so paths containing spaces degrade gracefully to
/// classification-only use.
#[cfg(target_os = "macos")]
fn argv_by_pid() -> HashMap<u32, Vec<OsString>> {
    let Ok(output) = std::process::Command::new("ps")
        .args(["-axo", "pid=,args="])
        .output()
    else {
        return HashMap::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut tokens = line.split_whitespace();
            let pid = tokens.next()?.parse::<u32>().ok()?;
            let argv: Vec<OsString> = tokens.map(OsString::from).collect();
            (!argv.is_empty()).then_some((pid, argv))
        })
        .collect()
}

/// Non-macOS: sysinfo reads argv directly; no fallback needed.
#[cfg(not(target_os = "macos"))]
fn argv_by_pid() -> HashMap<u32, Vec<OsString>> {
    HashMap::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_roots_means_no_workspace_membership() {
        // With no workspace open there is nothing to belong to, so the green-row
        // hint is off for everything ([PROF-PROCESSES-SCOPE]).
        assert!(!process_in_workspace(
            Some(Path::new("/anywhere/at/all")),
            Some("/anywhere/at/all/app.py"),
            Some("/usr/bin/python3"),
            &[],
        ));
    }

    #[test]
    fn cwd_inside_a_root_is_a_member() {
        let roots = vec![PathBuf::from("/home/dev/project")];
        assert!(process_in_workspace(
            Some(Path::new("/home/dev/project/sub")),
            None,
            None,
            &roots,
        ));
        // A sibling that merely shares a name prefix is NOT inside the root.
        assert!(!process_in_workspace(
            Some(Path::new("/home/dev/project-other")),
            None,
            None,
            &roots,
        ));
    }

    #[test]
    fn unrelated_process_outside_every_root_is_not_a_member() {
        // It is still LISTED (zero filters) — it just isn't a workspace member,
        // so it is not greened.
        let roots = vec![PathBuf::from("/home/dev/project")];
        assert!(!process_in_workspace(
            Some(Path::new("/")),
            None,
            Some("/usr/bin/python3"),
            &roots,
        ));
    }

    #[test]
    fn absolute_script_inside_a_root_is_a_member_even_when_cwd_is_not() {
        let roots = vec![PathBuf::from("/home/dev/project")];
        assert!(process_in_workspace(
            Some(Path::new("/home/dev")),
            Some("/home/dev/project/app.py"),
            None,
            &roots,
        ));
    }

    #[test]
    fn relative_script_resolves_against_cwd() {
        let roots = vec![PathBuf::from("/home/dev/project")];
        // `python app.py` launched from the project dir: argv[0]'s script is
        // relative and only lands in the root once joined to the cwd.
        assert!(process_in_workspace(
            Some(Path::new("/home/dev/project")),
            Some("app.py"),
            None,
            &roots,
        ));
    }

    #[test]
    fn candidate_paths_resolve_relative_args_against_cwd() {
        let cwd = Path::new("/home/dev/project");
        let candidates =
            workspace_candidate_paths(Some(cwd), Some("app.py"), Some("/usr/bin/python3"));
        assert!(candidates.contains(&PathBuf::from("/home/dev/project")));
        assert!(candidates.contains(&PathBuf::from("/home/dev/project/app.py")));
        assert!(candidates.contains(&PathBuf::from("/usr/bin/python3")));
    }
}
