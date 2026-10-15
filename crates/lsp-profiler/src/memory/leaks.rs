//! Leak-confidence scoring across diffs. Implements [PROF-MEMORY-CONFIDENCE].
//!
//! One growth is Low; two consecutive growths, or one growth over 10 MiB, is
//! Medium; three or more consecutive growths is High. A site that did not grow
//! in a diff resets its streak. Definite is decided from collector output
//! (a finaliser in an uncollectable cycle), never from growth alone.

use std::collections::{HashMap, HashSet};

use super::compare::SiteKey;
use super::format_bytes;
use crate::wire::{LeakConfidence, SiteGrowth, SuspectedLeak};

/// Single-diff growth that is Medium on its own: 10 MiB.
const LARGE_GROWTH_BYTES: i64 = 10 * 1024 * 1024;

/// Ordinal of a confidence level, for comparisons.
#[must_use]
pub fn rank(confidence: &LeakConfidence) -> u8 {
    match confidence {
        LeakConfidence::Low => 0,
        LeakConfidence::Medium => 1,
        LeakConfidence::High => 2,
        LeakConfidence::Definite => 3,
    }
}

/// Upper-case label of a confidence level.
#[must_use]
pub fn label(confidence: &LeakConfidence) -> &'static str {
    match confidence {
        LeakConfidence::Low => "LOW",
        LeakConfidence::Medium => "MEDIUM",
        LeakConfidence::High => "HIGH",
        LeakConfidence::Definite => "DEFINITE",
    }
}

/// Consecutive-growth streaks per site, across one session's diffs.
#[derive(Debug, Default)]
pub struct LeakTracker {
    streaks: HashMap<SiteKey, u32>,
}

impl LeakTracker {
    /// Score one diff's growing sites, updating every site's streak.
    pub fn process_growths(&mut self, growths: &[SiteGrowth]) -> Vec<SuspectedLeak> {
        let seen: HashSet<SiteKey> = growths.iter().map(SiteKey::of_growth).collect();
        self.streaks.retain(|key, _| seen.contains(key));
        for key in &seen {
            *self.streaks.entry(key.clone()).or_insert(0) += 1;
        }
        growths
            .iter()
            .map(|growth| {
                let streak = self
                    .streaks
                    .get(&SiteKey::of_growth(growth))
                    .copied()
                    .unwrap_or(1);
                suspected(growth, streak)
            })
            .collect()
    }
}

/// Score one growing site with its current streak.
fn suspected(growth: &SiteGrowth, streak: u32) -> SuspectedLeak {
    let (confidence, reason) = score(streak, growth.size_diff);
    SuspectedLeak {
        file: growth.file.clone(),
        line: growth.line,
        type_name: growth.type_name.clone(),
        size_growth: growth.size_diff,
        count_growth: growth.count_diff,
        current_size: growth.size_bytes,
        current_count: growth.count,
        confidence,
        reason,
        traceback: growth.traceback.clone(),
    }
}

fn score(streak: u32, size_diff: i64) -> (LeakConfidence, String) {
    if streak >= 3 {
        (
            LeakConfidence::High,
            format!("Consistent growth across {streak} consecutive snapshots"),
        )
    } else if streak == 2 {
        (
            LeakConfidence::Medium,
            "Growth in 2 consecutive diffs".to_owned(),
        )
    } else if size_diff > LARGE_GROWTH_BYTES {
        let growth = format_bytes(u64::try_from(size_diff).unwrap_or(0));
        (
            LeakConfidence::Medium,
            format!("Large single-diff growth ({growth})"),
        )
    } else {
        (LeakConfidence::Low, "May be normal cache warmup".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::TraceFrame;

    fn growth(file: &str, line: i64, size_diff: i64) -> SiteGrowth {
        SiteGrowth {
            file: Some(file.to_owned()),
            line: Some(line),
            type_name: None,
            size_diff,
            count_diff: 100,
            size_bytes: 1_000_000,
            count: 500,
            traceback: vec![TraceFrame {
                file: file.to_owned(),
                line,
            }],
        }
    }

    fn confidences(tracker: &mut LeakTracker, growths: &[SiteGrowth]) -> Vec<LeakConfidence> {
        tracker
            .process_growths(growths)
            .into_iter()
            .map(|leak| leak.confidence)
            .collect()
    }

    #[test]
    fn escalates_low_medium_high_with_consecutive_growth() {
        let mut tracker = LeakTracker::default();
        let site = [growth("src/app.py", 10, 1000)];
        assert_eq!(confidences(&mut tracker, &site), vec![LeakConfidence::Low]);
        assert_eq!(
            confidences(&mut tracker, &site),
            vec![LeakConfidence::Medium]
        );
        let third = tracker.process_growths(&site);
        assert_eq!(
            third.first().map(|l| (&l.confidence, l.reason.as_str())),
            Some((
                &LeakConfidence::High,
                "Consistent growth across 3 consecutive snapshots"
            ))
        );
    }

    #[test]
    fn large_single_growth_is_medium() {
        let mut tracker = LeakTracker::default();
        let leaks = tracker.process_growths(&[growth("src/big.py", 5, 20 * 1024 * 1024)]);
        assert_eq!(
            leaks.first().map(|l| (&l.confidence, l.reason.as_str())),
            Some((
                &LeakConfidence::Medium,
                "Large single-diff growth (20.0 MB)"
            ))
        );
    }

    #[test]
    fn a_diff_without_growth_resets_the_streak() {
        let mut tracker = LeakTracker::default();
        let site = [growth("src/app.py", 10, 1000)];
        let _ = tracker.process_growths(&site);
        let _ = tracker.process_growths(&site);
        let _ = tracker.process_growths(&[growth("src/other.py", 1, 1000)]);
        assert_eq!(confidences(&mut tracker, &site), vec![LeakConfidence::Low]);
    }

    // Type-keyed sites (runtimes without line attribution) escalate the same way.
    #[test]
    fn type_keyed_sites_escalate() {
        let mut tracker = LeakTracker::default();
        let by_type = SiteGrowth {
            file: None,
            line: None,
            type_name: Some("Cache".to_owned()),
            traceback: vec![],
            ..growth("x", 1, 10)
        };
        let _ = tracker.process_growths(std::slice::from_ref(&by_type));
        let _ = tracker.process_growths(std::slice::from_ref(&by_type));
        let leaks = tracker.process_growths(std::slice::from_ref(&by_type));
        assert_eq!(
            leaks
                .first()
                .map(|l| (l.type_name.as_deref(), &l.confidence)),
            Some((Some("Cache"), &LeakConfidence::High))
        );
    }

    #[test]
    fn rank_orders_confidence() {
        let ranks: Vec<u8> = [
            LeakConfidence::Low,
            LeakConfidence::Medium,
            LeakConfidence::High,
            LeakConfidence::Definite,
        ]
        .iter()
        .map(rank)
        .collect();
        assert_eq!(ranks, vec![0, 1, 2, 3]);
        assert_eq!(label(&LeakConfidence::Definite), "DEFINITE");
    }
}
