//! Classification of py-spy attach failures. Implements [PROF-ERRORS].

use lsp_profiler::wire::AttachErrorKind;

/// Classify a py-spy attach error message. Shared by the in-process sampler
/// and the elevated helper so both report identical, distinct failure modes.
/// Matching is lowercase substring because py-spy's wording differs across
/// platforms (macOS "check if it is running", Linux "Check that the process
/// is running").
#[must_use]
pub fn classify_attach_error(message: &str) -> AttachErrorKind {
    let lowered = message.to_lowercase();
    if lowered.contains("permission") || lowered.contains("operation not permitted") {
        AttachErrorKind::PermissionDenied
    } else if lowered.contains("not a python") {
        AttachErrorKind::UnsupportedRuntime
    } else if ["no such process", "not found", "check if it is running", "check that the process is running"]
        .iter()
        .any(|needle| lowered.contains(needle))
    {
        AttachErrorKind::ProcessNotFound
    } else {
        AttachErrorKind::AttachFailed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_attach_errors_into_distinct_kinds() {
        let cases = [
            ("Operation not permitted (os error 1)", AttachErrorKind::PermissionDenied),
            ("Permission denied attaching to PID", AttachErrorKind::PermissionDenied),
            ("PID 7 is not a python process", AttachErrorKind::UnsupportedRuntime),
            ("PID 9 is Not a Python process", AttachErrorKind::UnsupportedRuntime),
            ("No such process (os error 3)", AttachErrorKind::ProcessNotFound),
            ("Failed to open process - check if it is running.", AttachErrorKind::ProcessNotFound),
            ("Failed to get process executable name. Check that the process is running.", AttachErrorKind::ProcessNotFound),
            ("something exotic exploded", AttachErrorKind::AttachFailed),
        ];
        for (message, kind) in cases {
            assert_eq!(classify_attach_error(message), kind, "{message}");
        }
    }
}
