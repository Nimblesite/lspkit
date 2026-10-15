//! Memory diagnostics. Implements [PROF-DIAGNOSTICS-MEMORY].
//!
//! - `MEM-ALLOC` (Hint): an allocation site above the size threshold.
//! - `MEM-GROWTH` (Warning): a site that grew between snapshots.
//! - `MEM-LEAK` (Warning, Error when Definite): Medium+ leak confidence.
//!
//! Only located sites produce diagnostics; type-only findings are shown by
//! the dashboard instead of being pinned to an arbitrary file.

use super::leaks::{label, rank};
use super::{format_bytes, to_bytes};
use crate::diagnostics::{codes, diagnostic, DiagnosticSet, DiagnosticStyle};
use crate::wire::{DiagnosticLevel, FileDiagnostics, HeapSnapshot, LeakConfidence, SuspectedLeak};

/// Thresholds for allocation diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryHotspotConfig {
    /// Minimum live bytes for an allocation diagnostic.
    pub min_alloc_bytes: i64,
    /// Maximum diagnostics per file.
    pub max_per_file: usize,
}

impl Default for MemoryHotspotConfig {
    fn default() -> Self {
        Self {
            min_alloc_bytes: 1_048_576,
            max_per_file: 20,
        }
    }
}

/// Hints for large allocation sites.
#[must_use]
pub fn allocation_diagnostics(
    style: &DiagnosticStyle,
    snapshot: &HeapSnapshot,
    config: &MemoryHotspotConfig,
) -> Vec<FileDiagnostics> {
    let mut set = DiagnosticSet::default();
    for site in snapshot
        .sites
        .iter()
        .filter(|site| site.size_bytes >= config.min_alloc_bytes)
    {
        let (Some(file), Some(line)) = (&site.file, site.line) else {
            continue;
        };
        let message = format!(
            "Memory hotspot: {} allocated ({} objects)",
            format_bytes(to_bytes(site.size_bytes)),
            site.count
        );
        set.push(
            file,
            diagnostic(
                style,
                codes::MEM_ALLOC,
                &style.memory_source,
                line,
                DiagnosticLevel::Hint,
                message,
            ),
            config.max_per_file,
        );
    }
    set.into_files()
}

/// Growth warnings for every scored site, plus leak diagnostics at Medium+.
#[must_use]
pub fn leak_diagnostics(
    style: &DiagnosticStyle,
    leaks: &[SuspectedLeak],
    max_per_file: usize,
) -> Vec<FileDiagnostics> {
    let mut set = DiagnosticSet::default();
    let source = style.memory_source.as_str();
    for leak in leaks {
        let (Some(file), Some(line)) = (&leak.file, leak.line) else {
            continue;
        };
        let growth = format!(
            "Memory growth: +{} ({} new objects)",
            format_bytes(to_bytes(leak.size_growth)),
            leak.count_growth
        );
        set.push(
            file,
            diagnostic(
                style,
                codes::MEM_GROWTH,
                source,
                line,
                DiagnosticLevel::Warning,
                growth,
            ),
            max_per_file,
        );
        if rank(&leak.confidence) >= rank(&LeakConfidence::Medium) {
            let message = format!(
                "Suspected memory leak ({} confidence): {} \u{2014} {}",
                label(&leak.confidence),
                format_bytes(to_bytes(leak.current_size)),
                leak.reason
            );
            set.push(
                file,
                diagnostic(
                    style,
                    codes::MEM_LEAK,
                    source,
                    line,
                    severity(&leak.confidence),
                    message,
                ),
                max_per_file,
            );
        }
    }
    set.into_files()
}

/// Leak severity: Error when Definite, Warning otherwise.
fn severity(confidence: &LeakConfidence) -> DiagnosticLevel {
    if *confidence == LeakConfidence::Definite {
        DiagnosticLevel::Error
    } else {
        DiagnosticLevel::Warning
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::AllocationSite;

    fn abs(name: &str) -> String {
        std::env::temp_dir().join(name).display().to_string()
    }

    fn located(name: &str, line: i64, size: i64) -> AllocationSite {
        AllocationSite {
            file: Some(abs(name)),
            line: Some(line),
            type_name: None,
            size_bytes: size,
            count: 3,
            traceback: vec![],
        }
    }

    fn snapshot(sites: Vec<AllocationSite>) -> HeapSnapshot {
        HeapSnapshot {
            snapshot_id: "s".to_owned(),
            current_bytes: 0,
            peak_bytes: 0,
            object_count: 0,
            gc_generations: vec![],
            sites,
        }
    }

    fn leak(confidence: LeakConfidence, file: Option<String>) -> SuspectedLeak {
        SuspectedLeak {
            file,
            line: Some(8),
            type_name: None,
            size_growth: 2_097_152,
            count_growth: 40,
            current_size: 4_194_304,
            current_count: 80,
            confidence,
            reason: "because".to_owned(),
            traceback: vec![],
        }
    }

    fn codes_of(files: &[FileDiagnostics]) -> Vec<String> {
        files
            .iter()
            .flat_map(|f| f.diagnostics.iter().map(|d| d.code.clone()))
            .collect()
    }

    #[test]
    fn allocation_hints_respect_threshold_and_skip_type_sites() {
        let type_only = AllocationSite {
            file: None,
            line: None,
            type_name: Some("Big".to_owned()),
            size_bytes: 9_999_999,
            count: 1,
            traceback: vec![],
        };
        let files = allocation_diagnostics(
            &DiagnosticStyle::default(),
            &snapshot(vec![
                located("a.py", 4, 2_000_000),
                located("a.py", 9, 10),
                type_only,
            ]),
            &MemoryHotspotConfig::default(),
        );
        assert_eq!(codes_of(&files), vec!["MEM-ALLOC"]);
        let first = files.first().and_then(|f| f.diagnostics.first());
        assert_eq!(
            first.map(|d| (d.line, d.message.as_str())),
            Some((4, "Memory hotspot: 1.9 MB allocated (3 objects)"))
        );
    }

    #[test]
    fn allocation_hints_are_capped_per_file() {
        let sites: Vec<AllocationSite> =
            (1..=10).map(|n| located("busy.py", n, 5_000_000)).collect();
        let config = MemoryHotspotConfig {
            max_per_file: 3,
            ..MemoryHotspotConfig::default()
        };
        let files = allocation_diagnostics(&DiagnosticStyle::default(), &snapshot(sites), &config);
        assert_eq!(files.first().map(|f| f.diagnostics.len()), Some(3));
    }

    #[test]
    fn low_confidence_is_growth_only_and_medium_adds_a_leak() {
        let style = DiagnosticStyle {
            code_prefix: "SL-".to_owned(),
            ..DiagnosticStyle::default()
        };
        let low = leak_diagnostics(&style, &[leak(LeakConfidence::Low, Some(abs("l.py")))], 20);
        assert_eq!(codes_of(&low), vec!["SL-MEM-GROWTH"]);
        let medium = leak_diagnostics(
            &style,
            &[leak(LeakConfidence::Medium, Some(abs("m.py")))],
            20,
        );
        assert_eq!(codes_of(&medium), vec!["SL-MEM-GROWTH", "SL-MEM-LEAK"]);
        let message = medium
            .first()
            .and_then(|f| f.diagnostics.get(1))
            .map(|d| (d.message.clone(), d.severity.clone()));
        assert_eq!(
            message,
            Some((
                "Suspected memory leak (MEDIUM confidence): 4.0 MB \u{2014} because".to_owned(),
                DiagnosticLevel::Warning
            ))
        );
    }

    #[test]
    fn definite_leaks_are_errors_and_unlocated_leaks_are_skipped() {
        let files = leak_diagnostics(
            &DiagnosticStyle::default(),
            &[
                leak(LeakConfidence::Definite, Some(abs("d.py"))),
                leak(LeakConfidence::High, None),
            ],
            20,
        );
        let severities: Vec<DiagnosticLevel> = files
            .iter()
            .flat_map(|f| f.diagnostics.iter().map(|d| d.severity.clone()))
            .collect();
        assert_eq!(
            severities,
            vec![DiagnosticLevel::Warning, DiagnosticLevel::Error]
        );
    }
}
