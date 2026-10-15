//! `workspace/executeCommand` names for the protocol. Implements [PROF-LSP].
//!
//! Every operation is reachable as a standard LSP command
//! `<prefix>.lspProfiler.<area>.<action>`, so a host advertises them in its
//! `executeCommandProvider` and routes them to [`crate::ProfilerService`]
//! without a custom request in sight. The prefix is the host's own (for
//! example `sharplsp`), which keeps two hosts in one editor window apart.

use crate::wire::methods;

/// The segment every command carries after the host prefix.
pub const COMMAND_NAMESPACE: &str = "lspProfiler";

/// Every method the protocol exposes, in a stable order.
const METHODS: [&str; 18] = [
    methods::CAPABILITIES,
    methods::PROCESSES,
    methods::KILL,
    methods::CPU_START,
    methods::CPU_STOP,
    methods::CPU_SNAPSHOT,
    methods::CPU_LIST,
    methods::CPU_COOPERATIVE_SCRIPT,
    methods::CPU_COOPERATIVE_ATTACH,
    methods::MEMORY_START,
    methods::MEMORY_SNAPSHOT,
    methods::MEMORY_DIFF,
    methods::MEMORY_REFERENCES,
    methods::MEMORY_OBJECTS_BY_TYPE,
    methods::MEMORY_GC_COLLECT,
    methods::MEMORY_STOP,
    methods::MEMORY_INGEST,
    methods::CPU_PROGRESS,
];

/// The command name of `method` under `prefix`: `lspProfiler/cpu/start` → `<prefix>.lspProfiler.cpu.start`.
#[must_use]
pub fn command_for(prefix: &str, method: &str) -> String {
    format!("{prefix}.{}", method.replace('/', "."))
}

/// The method a command maps to, when it is one of this protocol's under `prefix`.
#[must_use]
pub fn method_for(prefix: &str, command: &str) -> Option<&'static str> {
    let rest = command.strip_prefix(prefix)?.strip_prefix('.')?;
    let method = rest.replacen('.', "/", 1).replace('.', "/");
    METHODS
        .iter()
        .copied()
        .find(|known| *known == method && *known != methods::CPU_PROGRESS)
}

/// Every command a host advertises in its `executeCommandProvider`.
#[must_use]
pub fn command_names(prefix: &str) -> Vec<String> {
    METHODS
        .iter()
        .filter(|m| **m != methods::CPU_PROGRESS)
        .map(|m| command_for(prefix, m))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_round_trip_to_methods() {
        for method in METHODS.iter().filter(|m| **m != methods::CPU_PROGRESS) {
            let command = command_for("sharplsp", method);
            assert!(command.starts_with("sharplsp.lspProfiler."), "{command}");
            assert_eq!(method_for("sharplsp", &command), Some(*method), "{command}");
        }
    }

    #[test]
    fn foreign_and_misprefixed_commands_are_not_ours() {
        assert_eq!(method_for("sharplsp", "sharplsp.profiler.start"), None);
        assert_eq!(
            method_for("sharplsp", "basilisk.lspProfiler.cpu.start"),
            None
        );
        assert_eq!(
            method_for("sharplsp", "sharplsp.lspProfiler.cpu.progress"),
            None,
            "progress is a notification"
        );
    }

    #[test]
    fn advertised_commands_cover_every_request() {
        let names = command_names("x");
        assert_eq!(names.len(), METHODS.len() - 1);
        assert!(names.contains(&"x.lspProfiler.memory.objectsByType".to_owned()));
    }
}
