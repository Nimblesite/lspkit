//! Profiler diagnostics. Implements [PROF-DIAGNOSTICS].
//!
//! Hot lines, hot functions and memory findings become [`FileDiagnostics`]
//! returned inside each result. The editor shows them in its own profiler
//! collection, so they never replace the language server's analysis
//! diagnostics for the same file, and never count as errors of the code.

use std::collections::BTreeMap;
use std::path::Path;

use crate::wire::{DiagnosticLevel, FileDiagnostics, HotFunction, HotLine, ProfilerDiagnostic};

/// Stable diagnostic codes, before the host's prefix. [PROF-DIAGNOSTICS-CODES]
pub mod codes {
    /// A hot line.
    pub const PROF_LINE: &str = "PROF-LINE";
    /// A hot function.
    pub const PROF_FUNC: &str = "PROF-FUNC";
    /// A large allocation site.
    pub const MEM_ALLOC: &str = "MEM-ALLOC";
    /// An allocation site that grew between snapshots.
    pub const MEM_GROWTH: &str = "MEM-GROWTH";
    /// A suspected leak.
    pub const MEM_LEAK: &str = "MEM-LEAK";
    /// An uncollectable reference cycle.
    pub const MEM_CYCLE: &str = "MEM-CYCLE";
}

/// How the host brands profiler diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticStyle {
    /// Prepended to every code, e.g. `BSK-` yields `BSK-PROF-LINE`.
    pub code_prefix: String,
    /// Source label for CPU diagnostics.
    pub cpu_source: String,
    /// Source label for memory diagnostics.
    pub memory_source: String,
}

impl Default for DiagnosticStyle {
    fn default() -> Self {
        Self {
            code_prefix: String::new(),
            cpu_source: "profiler".to_owned(),
            memory_source: "memory".to_owned(),
        }
    }
}

/// Accumulates diagnostics per file, in a stable file order.
#[derive(Debug, Default)]
pub struct DiagnosticSet {
    /// Diagnostics grouped by file URI, with deterministic publication order.
    files: BTreeMap<String, Vec<ProfilerDiagnostic>>,
}

impl DiagnosticSet {
    /// Add one diagnostic for `file`, at most `cap` per file. Files that cannot
    /// be expressed as a `file://` URI are skipped.
    pub fn push(&mut self, file: &str, diagnostic: ProfilerDiagnostic, cap: usize) {
        let Some(uri) = file_uri(file) else {
            return;
        };
        let entries = self.files.entry(uri).or_default();
        if entries.len() < cap {
            entries.push(diagnostic);
        }
    }

    /// The collected diagnostics.
    #[must_use]
    pub fn into_files(self) -> Vec<FileDiagnostics> {
        self.files
            .into_iter()
            .map(|(uri, diagnostics)| FileDiagnostics { uri, diagnostics })
            .collect()
    }
}

/// `file://` URI for an absolute path, when it has one.
#[must_use]
pub fn file_uri(path: &str) -> Option<String> {
    url::Url::from_file_path(Path::new(path))
        .ok()
        .map(String::from)
}

/// Build one diagnostic.
#[must_use]
pub fn diagnostic(
    style: &DiagnosticStyle,
    code: &str,
    source: &str,
    line: i64,
    severity: DiagnosticLevel,
    message: String,
) -> ProfilerDiagnostic {
    ProfilerDiagnostic {
        line: line.max(1),
        message,
        code: format!("{}{code}", style.code_prefix),
        severity,
        source: source.to_owned(),
    }
}

/// Hint diagnostics for hot lines and located hot functions.
/// Implements [PROF-DIAGNOSTICS-CPU]. `sample_count` is the same denominator
/// the percentages divide by, so "N of M samples" is always consistent.
#[must_use]
pub fn hotspot_diagnostics(
    style: &DiagnosticStyle,
    hot_lines: &[HotLine],
    hot_functions: &[HotFunction],
    sample_count: u64,
    max_per_file: usize,
) -> Vec<FileDiagnostics> {
    let mut set = DiagnosticSet::default();
    let source = style.cpu_source.as_str();
    for line in hot_lines {
        let message = format!(
            "Hot line: {:.1}% CPU ({}/{sample_count} samples)",
            line.percentage, line.samples
        );
        let item = diagnostic(
            style,
            codes::PROF_LINE,
            source,
            line.line,
            DiagnosticLevel::Hint,
            message,
        );
        set.push(&line.file, item, max_per_file);
    }
    for func in hot_functions {
        let (Some(file), Some(line)) = (&func.file, func.line) else {
            continue;
        };
        let message = format!(
            "Hot function: {} \u{2014} {:.1}% CPU ({} samples, {:.1}% self)",
            func.name, func.percentage, func.samples, func.self_percentage
        );
        let item = diagnostic(
            style,
            codes::PROF_FUNC,
            source,
            line,
            DiagnosticLevel::Hint,
            message,
        );
        set.push(file, item, max_per_file);
    }
    set.into_files()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn abs(path: &str) -> String {
        std::env::temp_dir().join(path).display().to_string()
    }

    fn hot_line(file: &str, line: i64, pct: f64) -> HotLine {
        HotLine {
            file: file.to_owned(),
            line,
            samples: 42,
            percentage: pct,
        }
    }

    fn hot_fn(file: Option<String>, line: Option<i64>) -> HotFunction {
        HotFunction {
            name: "parse".to_owned(),
            file,
            line,
            samples: 30,
            percentage: 30.0,
            self_samples: 10,
            self_percentage: 10.0,
        }
    }

    #[test]
    fn generates_line_and_function_hints_with_prefixed_codes() {
        let style = DiagnosticStyle {
            code_prefix: "X-".to_owned(),
            ..DiagnosticStyle::default()
        };
        let file = abs("app.py");
        let files = hotspot_diagnostics(
            &style,
            &[hot_line(&file, 12, 42.0)],
            &[hot_fn(Some(file.clone()), Some(3))],
            100,
            20,
        );
        assert_eq!(files.len(), 1);
        let only = files.first().map(|f| &f.diagnostics);
        let codes: Vec<&str> = only
            .map(|d| d.iter().map(|x| x.code.as_str()).collect())
            .unwrap_or_default();
        assert_eq!(codes, vec!["X-PROF-LINE", "X-PROF-FUNC"]);
        let first = only.and_then(|d| d.first());
        assert_eq!(
            first.map(|d| d.message.as_str()),
            Some("Hot line: 42.0% CPU (42/100 samples)")
        );
        assert_eq!(
            first.map(|d| (d.line, d.severity.clone(), d.source.as_str())),
            Some((12, DiagnosticLevel::Hint, "profiler"))
        );
        assert!(files
            .first()
            .is_some_and(|f| f.uri.starts_with("file://") && f.uri.ends_with("app.py")));
    }

    // Functions a runtime could not locate produce no diagnostic, and relative
    // paths (no file URI) are skipped rather than mis-attributed.
    #[test]
    fn skips_unlocated_functions_and_relative_paths() {
        let style = DiagnosticStyle::default();
        let files = hotspot_diagnostics(
            &style,
            &[hot_line("relative.py", 1, 50.0)],
            &[hot_fn(None, None)],
            10,
            20,
        );
        assert_eq!(files, Vec::<FileDiagnostics>::new());
    }

    #[test]
    fn caps_diagnostics_per_file() {
        let file = abs("busy.py");
        let lines: Vec<HotLine> = (1..=30).map(|n| hot_line(&file, n, 3.0)).collect();
        let files = hotspot_diagnostics(&DiagnosticStyle::default(), &lines, &[], 100, 5);
        assert_eq!(files.first().map(|f| f.diagnostics.len()), Some(5));
    }

    #[test]
    fn line_zero_is_clamped_to_the_first_line() {
        let item = diagnostic(
            &DiagnosticStyle::default(),
            codes::MEM_CYCLE,
            "memory",
            0,
            DiagnosticLevel::Error,
            String::new(),
        );
        assert_eq!(item.line, 1);
    }
}
