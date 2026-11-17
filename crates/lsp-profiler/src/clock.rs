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
/// Seconds in one hour.
const HOUR_SECS: i128 = 3_600;
/// Seconds in one minute.
const MINUTE_SECS: i128 = 60;
/// Days in a common year.
const YEAR_DAYS: i128 = 365;
/// Days in four years, one of them leap.
const LEAP_CYCLE_DAYS: i128 = 1_460;
/// Days in a century that skips its leap day.
const CENTURY_DAYS: i128 = 36_524;
/// Years between Julian leap days.
const LEAP_YEAR_EVERY: i128 = 4;
/// Years in a century, whose leap day is skipped.
const CENTURY_YEARS: i128 = 100;
/// Years in a Gregorian era.
const ERA_YEARS: i128 = 400;
/// Hinnant's month-from-day-of-year line: month = (5·doy + 2) / 153.
const MONTH_SLOPE: (i128, i128, i128) = (5, 2, 153);
/// March-based months before the year rolls into January.
const MARCH_MONTHS_TO_YEAR_END: i128 = 10;
/// Calendar months in a year.
const YEAR_MONTHS: i128 = 12;
/// March as a calendar month number.
const MARCH: i128 = 3;

/// `time` as RFC 3339 UTC, truncated (floored) to the second.
#[must_use]
pub fn rfc3339(time: SystemTime) -> String {
    rfc3339_seconds(epoch_seconds(time))
}

/// Whole seconds since the Unix epoch as RFC 3339 UTC. Kept apart from
/// [`SystemTime`], which cannot hold every instant on every platform: on
/// Windows it starts at 1601-01-01.
fn rfc3339_seconds(secs: i128) -> String {
    let (year, month, day) = civil(secs.div_euclid(DAY_SECS));
    let clock = secs.rem_euclid(DAY_SECS);
    let (hour, minute, second) = (
        clock.div_euclid(HOUR_SECS),
        clock.rem_euclid(HOUR_SECS).div_euclid(MINUTE_SECS),
        clock.rem_euclid(MINUTE_SECS),
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
    let year_of_era = (day_of_era - day_of_era.div_euclid(LEAP_CYCLE_DAYS)
        + day_of_era.div_euclid(CENTURY_DAYS)
        - day_of_era.div_euclid(ERA_DAYS - 1))
    .div_euclid(YEAR_DAYS);
    let day_of_year = day_of_era
        - (YEAR_DAYS * year_of_era + year_of_era.div_euclid(LEAP_YEAR_EVERY)
            - year_of_era.div_euclid(CENTURY_YEARS));
    let (slope, offset, span) = MONTH_SLOPE;
    let march_month = (slope * day_of_year + offset).div_euclid(span);
    let day = day_of_year - (span * march_month + offset).div_euclid(slope) + 1;
    let month = if march_month < MARCH_MONTHS_TO_YEAR_END {
        march_month + MARCH
    } else {
        march_month + MARCH - YEAR_MONTHS
    };
    (
        year_of_era + era * ERA_YEARS + i128::from(month < MARCH),
        month,
        day,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Seconds after the epoch and the timestamp they must format as.
    const KNOWN: [(u64, &str); 8] = [
        (0, "1970-01-01T00:00:00Z"),
        (951_782_400, "2000-02-29T00:00:00Z"),
        (951_868_799, "2000-02-29T23:59:59Z"),
        (1_700_000_000, "2023-11-14T22:13:20Z"),
        (4_107_542_399, "2100-02-28T23:59:59Z"),
        (4_107_542_400, "2100-03-01T00:00:00Z"),
        (13_574_606_400, "2400-02-29T12:00:00Z"),
        (253_402_300_799, "9999-12-31T23:59:59Z"),
    ];

    /// Seconds before the epoch and the timestamp they must format as.
    const BEFORE_EPOCH: [(u64, &str); 4] = [
        (2_208_988_800, "1900-01-01T00:00:00Z"),
        (2_203_891_201, "1900-02-28T23:59:59Z"),
        (2_203_891_200, "1900-03-01T00:00:00Z"),
        (11_670_998_400, "1600-02-29T00:00:00Z"),
    ];

    #[test]
    fn century_leap_rules_hold_before_the_epoch() {
        for (secs, want) in BEFORE_EPOCH {
            assert_eq!(rfc3339_seconds(-i128::from(secs)), want, "-{secs}s");
            // Windows' SystemTime starts at 1601, so the 1600 row exists
            // there only as seconds; elsewhere it goes through SystemTime too.
            if let Some(instant) = UNIX_EPOCH.checked_sub(Duration::from_secs(secs)) {
                assert_eq!(rfc3339(instant), want, "-{secs}s as SystemTime");
            }
        }
    }

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
        let now = rfc3339_now();
        assert_eq!(now.len(), "2026-10-14T00:00:00Z".len(), "{now}");
        assert!(now.ends_with('Z') && now.get(10..11) == Some("T"), "{now}");
    }
}
