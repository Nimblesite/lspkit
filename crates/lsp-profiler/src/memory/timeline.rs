//! Memory timeline. Implements [PROF-MEMORY-TIMELINE].
//!
//! Every snapshot of a session becomes a point; the dashboard charts the
//! points and flags a steady rise as a leak indicator.

use std::time::Instant;

use crate::wire::{HeapSnapshot, TimelinePoint};

/// Points kept per session; the oldest are evicted beyond this.
pub const MAX_POINTS: usize = 120;
/// Consecutive rising points that count as trending upward.
pub const TREND_POINTS: usize = 5;

/// A session's timeline.
#[derive(Debug)]
pub struct MemoryTimeline {
    /// Chart points in chronological order, bounded to the configured capacity.
    points: Vec<TimelinePoint>,
    /// Session origin used to calculate each point's relative elapsed time.
    started_at: Instant,
    /// Retention limit that bounds memory use in long-running sessions.
    max_points: usize,
}

impl Default for MemoryTimeline {
    fn default() -> Self {
        Self::with_capacity(MAX_POINTS)
    }
}

impl MemoryTimeline {
    /// A timeline keeping at most `max_points` points.
    #[must_use]
    pub fn with_capacity(max_points: usize) -> Self {
        Self {
            points: Vec::new(),
            started_at: Instant::now(),
            max_points,
        }
    }

    /// Record one snapshot, evicting the oldest points beyond the cap.
    pub fn record(&mut self, snapshot: &HeapSnapshot) {
        self.points.push(TimelinePoint {
            elapsed_secs: self.started_at.elapsed().as_secs_f64(),
            current_bytes: snapshot.current_bytes,
            peak_bytes: snapshot.peak_bytes,
            object_count: snapshot.object_count,
            snapshot_id: snapshot.snapshot_id.clone(),
        });
        let excess = self.points.len().saturating_sub(self.max_points);
        let _ = self.points.drain(..excess);
    }

    /// Points, oldest first.
    #[must_use]
    pub fn points(&self) -> &[TimelinePoint] {
        &self.points
    }

    /// Whether the last `min_points` points rise strictly.
    #[must_use]
    pub fn is_trending_upward(&self, min_points: usize) -> bool {
        let start = self.points.len().saturating_sub(min_points);
        let recent = self.points.get(start..).unwrap_or_default();
        recent.len() >= min_points.max(2)
            && recent.windows(2).all(|pair| match pair {
                [a, b] => b.current_bytes > a.current_bytes,
                _ => false,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(id: &str, current: i64) -> HeapSnapshot {
        HeapSnapshot {
            snapshot_id: id.to_owned(),
            current_bytes: current,
            peak_bytes: current,
            object_count: 1,
            gc_generations: vec![],
            generation_sizes: None,
            sites: vec![],
        }
    }

    fn recorded(values: &[i64], cap: usize) -> MemoryTimeline {
        let mut timeline = MemoryTimeline::with_capacity(cap);
        for (i, value) in values.iter().enumerate() {
            timeline.record(&snapshot(&format!("s{i}"), *value));
        }
        timeline
    }

    #[test]
    fn records_points_in_order() {
        let timeline = recorded(&[100, 200], 10);
        let ids: Vec<&str> = timeline
            .points()
            .iter()
            .map(|p| p.snapshot_id.as_str())
            .collect();
        assert_eq!(ids, vec!["s0", "s1"]);
        assert_eq!(timeline.points().last().map(|p| p.current_bytes), Some(200));
    }

    #[test]
    fn evicts_oldest_when_full() {
        let timeline = recorded(&[1, 2, 3, 4, 5], 3);
        let ids: Vec<&str> = timeline
            .points()
            .iter()
            .map(|p| p.snapshot_id.as_str())
            .collect();
        assert_eq!(ids, vec!["s2", "s3", "s4"]);
    }

    #[test]
    fn detects_upward_trend_only_on_strict_rise() {
        assert!(recorded(&[1, 2, 3, 4, 5], 10).is_trending_upward(5));
        assert!(!recorded(&[1, 3, 2, 4, 5], 10).is_trending_upward(5));
        assert!(
            !recorded(&[1, 2, 3], 10).is_trending_upward(5),
            "too few points"
        );
        assert!(
            !recorded(&[5, 5, 5, 5, 5], 10).is_trending_upward(5),
            "flat is not rising"
        );
    }
}
