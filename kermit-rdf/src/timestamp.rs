//! Hand-rolled UTC ISO-8601 timestamps for benchmark provenance metadata.
//!
//! Used by both pipelines (`pipeline.rs` and `lubm/pipeline.rs`) when
//! writing `meta.json`. Avoids pulling in `chrono` or `time` for what is
//! ultimately a sortable provenance string. Calendar correctness around
//! leap seconds does not matter; the simple year-loop breaks at year 4801
//! (Gregorian leap-year edge case in the iterative computation), which we
//! consider acceptable for a benchmark-generation timestamp.

use std::time::{SystemTime, UNIX_EPOCH};

const SECS_PER_DAY: u64 = 86_400;
const SECS_PER_HOUR: u64 = 3_600;
const SECS_PER_MINUTE: u64 = 60;

/// Returns the current wall-clock time as a UTC ISO-8601 string of the
/// form `YYYY-MM-DDTHH:MM:SSZ`. Returns the epoch (`1970-01-01T00:00:00Z`)
/// if the system clock is set before the Unix epoch.
pub(crate) fn utc_iso8601_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / SECS_PER_DAY;
    let rem = secs % SECS_PER_DAY;
    let h = rem / SECS_PER_HOUR;
    let m = (rem % SECS_PER_HOUR) / SECS_PER_MINUTE;
    let s = rem % SECS_PER_MINUTE;
    let (y, mo, d) = days_since_epoch_to_ymd(days as i64);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

/// Converts days since `1970-01-01` to a `(year, month, day)` tuple.
/// Months and days are 1-indexed.
pub(crate) fn days_since_epoch_to_ymd(mut days: i64) -> (i32, u32, u32) {
    let mut y: i32 = 1970;
    loop {
        let leap = is_leap_year(y);
        let year_days = if leap {
            366
        } else {
            365
        };
        if days < year_days as i64 {
            break;
        }
        days -= year_days as i64;
        y += 1;
    }
    let leap = is_leap_year(y);
    let months = [
        31,
        if leap {
            29
        } else {
            28
        },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut mo: u32 = 1;
    for &mlen in &months {
        if days < mlen as i64 {
            break;
        }
        days -= mlen as i64;
        mo += 1;
    }
    (y, mo, days as u32 + 1)
}

fn is_leap_year(y: i32) -> bool { (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ymd_epoch_zero_is_jan_1_1970() {
        assert_eq!(days_since_epoch_to_ymd(0), (1970, 1, 1));
    }

    #[test]
    fn ymd_handles_one_year() {
        assert_eq!(days_since_epoch_to_ymd(365), (1971, 1, 1));
    }

    #[test]
    fn iso_timestamp_well_formed() {
        let s = utc_iso8601_now();
        assert_eq!(s.len(), 20);
        assert!(s.ends_with('Z'));
        assert_eq!(s.chars().nth(4), Some('-'));
        assert_eq!(s.chars().nth(10), Some('T'));
    }
}
