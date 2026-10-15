//! CPU reports. Implements [PROF-CPU-REPORT].
//!
//! Turns a recording into the wire report: hotspots, the diagnostics they
//! imply, and the exported artifacts.

use std::path::Path;

use crate::aggregate::{saturating_i64, HotspotConfig};
use crate::diagnostics::{hotspot_diagnostics, DiagnosticStyle};
use crate::export::{export_artifacts, ExportMeta};
use crate::session::RecordedProfile;
use crate::wire::ProfileReport;

/// Build the report for a recording, writing artifacts into `output_dir`.
#[must_use]
pub fn build_report(
    recorded: &RecordedProfile,
    hotspots: &HotspotConfig,
    style: &DiagnosticStyle,
    output_dir: &Path,
) -> ProfileReport {
    let data = &recorded.data;
    let hot_functions = data.hot_functions(hotspots);
    let hot_lines = data.hot_lines(hotspots);
    let sample_count = data.sample_count();
    let diagnostics = hotspot_diagnostics(
        style,
        &hot_lines,
        &hot_functions,
        sample_count,
        hotspots.max_diagnostics_per_file,
    );
    let meta = ExportMeta {
        session_id: &recorded.session_id,
        pid: recorded.pid,
        runtime: recorded.runtime.display_name(),
        duration_secs: recorded.duration_secs,
        sample_rate: recorded.sample_rate,
    };
    ProfileReport {
        session_id: recorded.session_id.clone(),
        pid: i64::from(recorded.pid),
        runtime: recorded.runtime.id().to_owned(),
        duration_secs: recorded.duration_secs,
        total_samples: saturating_i64(data.total_samples),
        user_samples: saturating_i64(sample_count),
        sample_rate: saturating_i64(recorded.sample_rate),
        hot_functions,
        hot_lines,
        artifacts: export_artifacts(data, &meta, output_dir),
        diagnostics,
    }
}
