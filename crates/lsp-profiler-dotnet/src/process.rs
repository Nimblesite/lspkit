//! Recognising .NET processes. Implements [PROF-DOTNET-PROCESSES].
//!
//! A process is .NET when it is the `dotnet` host muxer (`dotnet run`,
//! `dotnet App.dll`) or an apphost whose directory carries the
//! `<name>.runtimeconfig.json` the SDK writes beside every framework-dependent
//! or self-contained build. The runtime version comes from that file. A
//! recognised process is attachable only while it advertises the diagnostics
//! IPC endpoint every live `CoreCLR` opens — its absence is the deterministic
//! "cannot profile" signal (`DOTNET_EnableDiagnostics=0`, or a native host).

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use lsp_profiler::runtime::{Classification, ProcessCandidate};

const MUXER_STEM: &str = "dotnet";
const RUNTIMECONFIG_SUFFIX: &str = ".runtimeconfig.json";
const NO_ENDPOINT: &str = "no .NET diagnostics endpoint: the process was started with DOTNET_EnableDiagnostics=0 or is not CoreCLR";

/// What a directory probe found, memoised per directory within one classification pass.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Probe {
    /// No runtimeconfig: not a .NET output directory.
    NotDotNet,
    /// A .NET output directory, with its runtime version when the file names one.
    DotNet(Option<String>),
}

/// Classify every `.NET` candidate, memoising directory probes per call.
#[must_use]
pub fn classify_all(candidates: &[ProcessCandidate]) -> Vec<(usize, Classification)> {
    let mut cache: HashMap<PathBuf, Probe> = HashMap::new();
    candidates
        .iter()
        .enumerate()
        .filter_map(|(index, candidate)| classify(candidate, &mut cache).map(|c| (index, c)))
        .collect()
}

/// One candidate's classification, `None` when it is not .NET.
fn classify(
    candidate: &ProcessCandidate,
    cache: &mut HashMap<PathBuf, Probe>,
) -> Option<Classification> {
    let exe = candidate
        .exe
        .clone()
        .or_else(|| candidate.cmd.first().map(PathBuf::from))?;
    let (version, entry) = if is_muxer(&exe) {
        let assembly = candidate
            .cmd
            .iter()
            .skip(1)
            .find(|arg| has_extension(Path::new(arg), "dll"))
            .map(PathBuf::from);
        let version = assembly
            .as_deref()
            .and_then(Path::parent)
            .and_then(|dir| version_of(&probe(dir, cache)));
        (
            version,
            assembly.or_else(|| candidate.cmd.get(1).map(PathBuf::from)),
        )
    } else {
        match probe(exe.parent()?, cache) {
            Probe::NotDotNet => return None,
            Probe::DotNet(version) => (version, Some(exe.clone())),
        }
    };
    let attachable = has_endpoint(candidate.pid).unwrap_or(true);
    Some(Classification {
        runtime_version: version,
        executable: Some(exe.display().to_string()),
        entry: entry.map(|p| p.display().to_string()),
        launcher: None,
        requires_elevation: candidate.owned_by_other_user,
        unattachable_reason: (!attachable).then(|| NO_ENDPOINT.to_owned()),
    })
}

fn is_muxer(exe: &Path) -> bool {
    exe.file_stem()
        .is_some_and(|stem| stem.eq_ignore_ascii_case(MUXER_STEM))
}

fn has_extension(path: &Path, extension: &str) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case(extension))
}

/// Whether `dir` holds a runtimeconfig, memoised per directory.
fn probe(dir: &Path, cache: &mut HashMap<PathBuf, Probe>) -> Probe {
    cache
        .entry(dir.to_path_buf())
        .or_insert_with(|| {
            runtimeconfig_in(dir).map_or(Probe::NotDotNet, |path| {
                Probe::DotNet(runtimeconfig_version(&path))
            })
        })
        .clone()
}

fn version_of(probe: &Probe) -> Option<String> {
    match probe {
        Probe::NotDotNet => None,
        Probe::DotNet(version) => version.clone(),
    }
}

fn runtimeconfig_in(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(OsStr::to_str)
                .is_some_and(|name| name.ends_with(RUNTIMECONFIG_SUFFIX))
        })
}

/// The shared-framework version, falling back to the target framework moniker.
#[must_use]
pub fn runtimeconfig_version(path: &Path) -> Option<String> {
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let options = json.get("runtimeOptions")?;
    framework_version(options)
        .or_else(|| options.get("tfm").and_then(serde_json::Value::as_str))
        .map(str::to_owned)
}

fn framework_version(options: &serde_json::Value) -> Option<&str> {
    let single = options.get("framework").and_then(|fw| fw.get("version"));
    let first_of_many = || {
        options
            .get("frameworks")?
            .as_array()?
            .iter()
            .find_map(|fw| fw.get("version"))
    };
    single
        .or_else(first_of_many)
        .and_then(serde_json::Value::as_str)
}

/// Whether `pid` exposes a .NET diagnostics IPC endpoint; `None` when the
/// registry cannot be enumerated, in which case callers fail open.
#[must_use]
pub fn has_endpoint(pid: u32) -> Option<bool> {
    endpoint_exists(pid)
}

#[cfg(windows)]
fn endpoint_exists(pid: u32) -> Option<bool> {
    let exact = format!("dotnet-diagnostic-{pid}");
    let prefixed = format!("{exact}-");
    let entries = std::fs::read_dir(r"\\.\pipe\").ok()?;
    Some(entries.flatten().any(|entry| {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        name == exact.as_str() || name.starts_with(&prefixed)
    }))
}

#[cfg(unix)]
fn endpoint_exists(pid: u32) -> Option<bool> {
    let prefix = format!("dotnet-diagnostic-{pid}-");
    let mut dirs = vec![std::env::temp_dir()];
    let linux_tmp = PathBuf::from("/tmp");
    if !dirs.contains(&linux_tmp) {
        dirs.push(linux_tmp);
    }
    let mut enumerable = false;
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        enumerable = true;
        if entries
            .flatten()
            .any(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
        {
            return Some(true);
        }
    }
    enumerable.then_some(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(exe: &str, cmd: &[&str]) -> ProcessCandidate {
        ProcessCandidate {
            pid: 1,
            ppid: None,
            name: "x".to_owned(),
            exe: Some(PathBuf::from(exe)),
            cmd: cmd.iter().map(Into::into).collect(),
            cwd: None,
            owned_by_other_user: false,
            child_of_current: false,
        }
    }

    fn write_runtimeconfig(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("App.runtimeconfig.json");
        let _ = std::fs::write(&path, body);
        path
    }

    #[test]
    fn the_muxer_reports_the_assembly_directory_version() {
        let dir = tempfile::tempdir().ok();
        assert!(dir.is_some(), "tempdir");
        let Some(dir) = dir else {
            return;
        };
        let _ = write_runtimeconfig(
            dir.path(),
            r#"{"runtimeOptions":{"tfm":"net10.0","framework":{"name":"Microsoft.NETCore.App","version":"10.0.7"}}}"#,
        );
        let dll = dir.path().join("App.dll").display().to_string();
        let found = classify_all(&[candidate("/usr/share/dotnet/dotnet", &["dotnet", &dll])]);
        let version = found.first().and_then(|(_, c)| c.runtime_version.clone());
        assert_eq!(version, Some("10.0.7".to_owned()));
        assert_eq!(found.first().and_then(|(_, c)| c.entry.clone()), Some(dll));
    }

    #[test]
    fn the_muxer_without_an_assembly_is_dotnet_with_unknown_version() {
        let found = classify_all(&[candidate("/usr/share/dotnet/dotnet", &["dotnet", "run"])]);
        assert_eq!(
            found
                .first()
                .map(|(_, c)| (c.runtime_version.clone(), c.entry.clone())),
            Some((None, Some("run".to_owned())))
        );
    }

    #[test]
    fn an_apphost_is_dotnet_only_with_a_runtimeconfig_beside_it() {
        let dir = tempfile::tempdir().ok();
        assert!(dir.is_some(), "tempdir");
        let Some(dir) = dir else {
            return;
        };
        let exe = dir.path().join("App").display().to_string();
        assert_eq!(classify_all(&[candidate(&exe, &[&exe])]).len(), 0);
        let _ = write_runtimeconfig(dir.path(), r#"{"runtimeOptions":{"tfm":"net8.0"}}"#);
        let found = classify_all(&[candidate(&exe, &[&exe])]);
        assert_eq!(
            found.first().and_then(|(_, c)| c.runtime_version.clone()),
            Some("net8.0".to_owned())
        );
    }

    #[test]
    fn a_frameworks_array_yields_its_first_version() {
        let options = serde_json::json!({"frameworks": [{"name": "A", "version": "10.0.7"}, {"name": "B", "version": "9.0.0"}]});
        assert_eq!(framework_version(&options), Some("10.0.7"));
    }

    #[test]
    fn a_pid_without_an_endpoint_is_not_attachable() {
        assert_eq!(has_endpoint(u32::MAX), Some(false));
    }
}
