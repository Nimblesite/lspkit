//! RFC 3339 UTC timestamps from [`SystemTime`] with std only. [PROF-CPU-SESSIONS]
//!
//! Replaces `humantime`, which RUSTSEC-2026-0542 marks unmaintained.

use std::time::{SystemTime, UNIX_EPOCH};

/// The current time as RFC 3339 UTC, to the second.
#[must_use]
pub fn rfc3339_now() -> String {
    rfc3339(SystemTime::now())
}

/// Seconds in one civil day.
const DAY_SECS: i128 = 86_400;
/// Days in one 400-year Gregorian era.
const ERA_DAYS: i128 = 146_097;
/// Days from 0000-03-01 to 1970-01-01.
const EPOCH_SHIFT: i128 = 719_468;

/// `time` as RFC 3339 UTC, truncated (floored) to the second.
#[must_use]
pub fn rfc3339(time: SystemTime) -> String {
    let secs = epoch_seconds(time);
    let (year, month, day) = civil(secs.div_euclid(DAY_SECS));
    let clock = secs.rem_euclid(DAY_SECS);
    let (hour, minute, second) = (
        clock.div_euclid(3600),
        clock.rem_euclid(3600).div_euclid(60),
        clock.rem_euclid(60),
    );
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Whole seconds since the Unix epoch, floored, negative before it.
fn epoch_seconds(time: SystemTime) -> i128 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(after) => i128::from(after.as_secs()),
        Err(before) => {
            let gap = before.duration();
            -i128::from(gap.as_secs()) - i128::from(gap.subsec_nanos() > 0)
        }
    }
}

/// The proleptic Gregorian (year, month, day) of `days` since 1970-01-01.
fn civil(days: i128) -> (i128, i128, i128) {
    let shifted = days + EPOCH_SHIFT;
    let era = shifted.div_euclid(ERA_DAYS);
    let day_of_era = shifted - era * ERA_DAYS;
    let year_of_era = (day_of_era - day_of_era.div_euclid(1460) + day_of_era.div_euclid(36_524)
        - day_of_era.div_euclid(ERA_DAYS - 1))
    .div_euclid(365);
    let day_of_year =
        day_of_era - (365 * year_of_era + year_of_era.div_euclid(4) - year_of_era.div_euclid(100));
    let march_month = (5 * day_of_year + 2).div_euclid(153);
    let day = day_of_year - (153 * march_month + 2).div_euclid(5) + 1;
    let month = if march_month < 10 {
        march_month + 3
    } else {
        march_month - 9
    };
    (year_of_era + era * 400 + i128::from(month <= 2), month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Seconds after the epoch and the timestamp they must format as.
    const KNOWN: [(u64, &str); 6] = [
        (0, "1970-01-01T00:00:00Z"),
        (951_782_400, "2000-02-29T00:00:00Z"),
        (951_868_799, "2000-02-29T23:59:59Z"),
        (1_700_000_000, "2023-11-14T22:13:20Z"),
        (4_107_542_400, "2100-03-01T00:00:00Z"),
        (253_402_300_799, "9999-12-31T23:59:59Z"),
    ];

    #[test]
    fn known_instants_format_as_rfc3339_utc_seconds() {
        for (secs, want) in KNOWN {
            let exact = UNIX_EPOCH + Duration::from_secs(secs);
            assert_eq!(rfc3339(exact), want, "{secs}s");
            let later = exact + Duration::from_nanos(999_999_999);
            assert_eq!(rfc3339(later), want, "{secs}s truncates sub-seconds");
        }
        let before = UNIX_EPOCH - Duration::from_millis(1);
        assert_eq!(
            rfc3339(before),
            "1969-12-31T23:59:59Z",
            "floors before the epoch"
        );
        let early = UNIX_EPOCH - Duration::from_secs(2_208_988_800);
        assert_eq!(rfc3339(early), "1900-01-01T00:00:00Z");
        let now = rfc3339_now();
        assert_eq!(now.len(), "2026-10-14T00:00:00Z".len(), "{now}");
        assert!(now.ends_with('Z') && now.get(10..11) == Some("T"), "{now}");
    }
}
