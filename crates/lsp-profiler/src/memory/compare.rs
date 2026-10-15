//! Snapshot comparison for runtimes that cannot diff in the target.
//! Implements [PROF-MEMORY-COMPARE].
//!
//! Sites are matched on their identity (file, line, type); every site that
//! changed size lands in `grown` or `freed`, largest change first.

use std::collections::HashMap;

use crate::wire::{AllocationSite, HeapDiff, HeapSnapshot, SiteGrowth};

/// Identity of an allocation site across snapshots.
#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct SiteKey {
    /// Source path.
    pub file: Option<String>,
    /// 1-based line.
    pub line: Option<i64>,
    /// Type name.
    pub type_name: Option<String>,
}

impl SiteKey {
    /// The key of an allocation site.
    #[must_use]
    pub fn of_site(site: &AllocationSite) -> Self {
        Self {
            file: site.file.clone(),
            line: site.line,
            type_name: site.type_name.clone(),
        }
    }

    /// The key of a growth record.
    #[must_use]
    pub fn of_growth(growth: &SiteGrowth) -> Self {
        Self {
            file: growth.file.clone(),
            line: growth.line,
            type_name: growth.type_name.clone(),
        }
    }
}

/// Compare `previous` with `current`.
#[must_use]
pub fn compare(previous: &HeapSnapshot, current: &HeapSnapshot) -> HeapDiff {
    let before: HashMap<SiteKey, &AllocationSite> = previous
        .sites
        .iter()
        .map(|site| (SiteKey::of_site(site), site))
        .collect();
    let after: HashMap<SiteKey, &AllocationSite> = current
        .sites
        .iter()
        .map(|site| (SiteKey::of_site(site), site))
        .collect();
    let mut changes: Vec<SiteGrowth> = after
        .iter()
        .map(|(key, site)| growth(key, before.get(key).copied(), Some(site)))
        .chain(
            before
                .iter()
                .filter(|(key, _)| !after.contains_key(*key))
                .map(|(key, site)| growth(key, Some(site), None)),
        )
        .filter(|change| change.size_diff != 0)
        .collect();
    changes.sort_by_key(|change| std::cmp::Reverse(change.size_diff.abs()));
    from_changes(changes)
}

/// Split signed changes into a diff with totals.
#[must_use]
pub fn from_changes(changes: Vec<SiteGrowth>) -> HeapDiff {
    let (grown, freed): (Vec<SiteGrowth>, Vec<SiteGrowth>) =
        changes.into_iter().partition(|change| change.size_diff > 0);
    let total_growth: i64 = grown.iter().map(|g| g.size_diff).sum();
    let total_freed: i64 = freed.iter().map(|f| f.size_diff.abs()).sum();
    HeapDiff {
        total_growth,
        total_freed,
        net_growth: total_growth - total_freed,
        grown,
        freed,
    }
}

/// The change at one site; a site missing on one side counts as zero there.
fn growth(
    key: &SiteKey,
    before: Option<&AllocationSite>,
    after: Option<&AllocationSite>,
) -> SiteGrowth {
    let (size_before, count_before) = before.map_or((0, 0), |s| (s.size_bytes, s.count));
    let (size_after, count_after) = after.map_or((0, 0), |s| (s.size_bytes, s.count));
    SiteGrowth {
        file: key.file.clone(),
        line: key.line,
        type_name: key.type_name.clone(),
        size_diff: size_after - size_before,
        count_diff: count_after - count_before,
        size_bytes: size_after,
        count: count_after,
        traceback: after
            .or(before)
            .map(|s| s.traceback.clone())
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn type_site(name: &str, size: i64, count: i64) -> AllocationSite {
        AllocationSite {
            file: None,
            line: None,
            type_name: Some(name.to_owned()),
            size_bytes: size,
            count,
            traceback: vec![],
        }
    }

    fn snapshot(id: &str, sites: Vec<AllocationSite>) -> HeapSnapshot {
        HeapSnapshot {
            snapshot_id: id.to_owned(),
            current_bytes: sites.iter().map(|s| s.size_bytes).sum(),
            peak_bytes: 0,
            object_count: 0,
            gc_generations: vec![],
            generation_sizes: None,
            sites,
        }
    }

    // [PROF-MEMORY-COMPARE] Growth, shrinkage, appearance and disappearance
    // are all reported, largest change first, with consistent totals.
    #[test]
    fn compares_type_keyed_snapshots() {
        let previous = snapshot(
            "a",
            vec![
                type_site("Cache", 1_000, 10),
                type_site("Gone", 300, 3),
                type_site("Same", 50, 1),
            ],
        );
        let current = snapshot(
            "b",
            vec![
                type_site("Cache", 5_000, 50),
                type_site("New", 700, 7),
                type_site("Same", 50, 1),
            ],
        );
        let diff = compare(&previous, &current);
        let grown: Vec<(Option<&str>, i64, i64)> = diff
            .grown
            .iter()
            .map(|g| (g.type_name.as_deref(), g.size_diff, g.count_diff))
            .collect();
        assert_eq!(
            grown,
            vec![(Some("Cache"), 4_000, 40), (Some("New"), 700, 7)]
        );
        let freed: Vec<(Option<&str>, i64, i64)> = diff
            .freed
            .iter()
            .map(|g| (g.type_name.as_deref(), g.size_diff, g.size_bytes))
            .collect();
        assert_eq!(freed, vec![(Some("Gone"), -300, 0)]);
        assert_eq!(
            (diff.total_growth, diff.total_freed, diff.net_growth),
            (4_700, 300, 4_400)
        );
    }

    #[test]
    fn identical_snapshots_have_no_changes() {
        let only = snapshot("a", vec![type_site("T", 10, 1)]);
        let diff = compare(&only, &only);
        assert!(diff.grown.is_empty() && diff.freed.is_empty());
        assert_eq!(diff.net_growth, 0);
    }
}
