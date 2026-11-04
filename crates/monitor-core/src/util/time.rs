//! Unix-second calendar arithmetic shared by history, alerts and the app.

use std::time::{SystemTime, UNIX_EPOCH};

pub const MINUTE_SECONDS: i64 = 60;
pub const FIVE_MINUTES_SECONDS: i64 = 5 * MINUTE_SECONDS;
pub const HOUR_SECONDS: i64 = 60 * MINUTE_SECONDS;
pub const DAY_SECONDS: i64 = 24 * HOUR_SECONDS;
pub const WEEK_SECONDS: i64 = 7 * DAY_SECONDS;
/// 1970-01-05, the first Monday after the epoch: weekly buckets start on Mondays.
pub const MONDAY_ORIGIN_SECONDS: i64 = 4 * DAY_SECONDS;

/// Wall-clock seconds since the Unix epoch, or 0 when the clock is before it.
#[must_use]
pub fn wall_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// [`wall_unix_seconds`] as the signed seconds calendar arithmetic uses.
#[must_use]
pub fn unix_now() -> i64 {
    i64::try_from(wall_unix_seconds()).unwrap_or(i64::MAX)
}

#[must_use]
pub const fn align_down(unix_seconds: i64, step: i64) -> i64 {
    align_down_from(unix_seconds, step, 0)
}

#[must_use]
pub const fn align_down_from(unix_seconds: i64, step: i64, origin: i64) -> i64 {
    unix_seconds - (unix_seconds - origin).rem_euclid(step)
}

/// The local calendar day `unix` falls on, counted from 1970-01-01.
#[must_use]
pub const fn local_day(unix: i64, utc_offset_seconds: i32) -> i64 {
    (unix + utc_offset_seconds as i64).div_euclid(DAY_SECONDS)
}

/// Unix seconds of the local midnight starting `unix`'s day.
#[must_use]
pub const fn local_midnight(unix: i64, utc_offset_seconds: i32) -> i64 {
    local_day(unix, utc_offset_seconds) * DAY_SECONDS - utc_offset_seconds as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aligns_to_steps_and_mondays() {
        assert_eq!(align_down(125, MINUTE_SECONDS), 120);
        assert_eq!(align_down(-1, MINUTE_SECONDS), -60);
        let tuesday_noon = MONDAY_ORIGIN_SECONDS + DAY_SECONDS + 12 * HOUR_SECONDS;
        assert_eq!(
            align_down_from(tuesday_noon, WEEK_SECONDS, MONDAY_ORIGIN_SECONDS),
            MONDAY_ORIGIN_SECONDS
        );
    }

    #[test]
    fn local_midnight_honours_offset() {
        let utc_noon = 20_000 * DAY_SECONDS + 12 * HOUR_SECONDS;
        assert_eq!(local_midnight(utc_noon, 0), 20_000 * DAY_SECONDS);
        let ist = 5 * 3600 + 1800;
        assert_eq!(
            local_midnight(utc_noon, ist),
            20_000 * DAY_SECONDS - i64::from(ist)
        );
        assert_eq!(local_day(utc_noon, 0), 20_000);
    }

    #[test]
    fn the_clock_is_past_the_epoch() {
        assert!(unix_now() > 0);
        assert_eq!(u64::try_from(unix_now()).ok(), Some(wall_unix_seconds()));
    }
}
