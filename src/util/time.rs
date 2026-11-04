//! Wall-clock figures in the user's local time: "14:05", "Today", "Tue 3 Nov".

use std::fmt::Write;

use monitor_core::util::time::{DAY_SECONDS, HOUR_SECONDS, MINUTE_SECONDS, local_day};
const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
/// 1970-01-01 was a Thursday: Monday-based weekday 3.
const EPOCH_WEEKDAY: i64 = 3;
const DAYS_PER_ERA: i64 = 146_097;
const ERA_SHIFT_DAYS: i64 = 719_468;

/// Month (1-based) and day of month of a day count since 1970-01-01.
fn month_and_day(days: i64) -> (usize, i64) {
    let shifted = days + ERA_SHIFT_DAYS;
    let day_of_era = shifted.rem_euclid(DAYS_PER_ERA);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    (month as usize, day)
}

/// "14:05".
pub fn write_clock(out: &mut String, unix: i64, utc_offset_seconds: i32) {
    let seconds = (unix + i64::from(utc_offset_seconds)).rem_euclid(DAY_SECONDS);
    let _ = write!(
        out,
        "{:02}:{:02}",
        seconds / HOUR_SECONDS,
        seconds % HOUR_SECONDS / MINUTE_SECONDS
    );
}

pub fn clock(unix: i64, utc_offset_seconds: i32) -> String {
    let mut out = String::new();
    write_clock(&mut out, unix, utc_offset_seconds);
    out
}

/// "Tue 3 Nov".
pub fn write_date(out: &mut String, unix: i64, utc_offset_seconds: i32) {
    let day = local_day(unix, utc_offset_seconds);
    let weekday = WEEKDAYS[(day + EPOCH_WEEKDAY).rem_euclid(7) as usize];
    let (month, day_of_month) = month_and_day(day);
    let _ = write!(out, "{weekday} {day_of_month} {}", MONTHS[month - 1]);
}

/// "Today", "Yesterday" or "Tue 3 Nov".
pub fn day_label(unix: i64, now_unix: i64, utc_offset_seconds: i32) -> String {
    match local_day(now_unix, utc_offset_seconds) - local_day(unix, utc_offset_seconds) {
        0 => "Today".to_owned(),
        1 => "Yesterday".to_owned(),
        _ => {
            let mut out = String::new();
            write_date(&mut out, unix, utc_offset_seconds);
            out
        }
    }
}

/// "14:05" today, "Yesterday 14:05", otherwise "Tue 3 Nov 14:05".
pub fn moment(unix: i64, now_unix: i64, utc_offset_seconds: i32) -> String {
    let mut out = String::new();
    if local_day(unix, utc_offset_seconds) != local_day(now_unix, utc_offset_seconds) {
        out.push_str(&day_label(unix, now_unix, utc_offset_seconds));
        out.push(' ');
    }
    write_clock(&mut out, unix, utc_offset_seconds);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOV_4_2026_MIDNIGHT_UTC: i64 = 1_793_750_400;

    #[test]
    fn dates_follow_the_civil_calendar() {
        let mut out = String::new();
        write_date(&mut out, NOV_4_2026_MIDNIGHT_UTC, 0);
        assert_eq!(out, "Wed 4 Nov");
        out.clear();
        write_date(&mut out, 0, 0);
        assert_eq!(out, "Thu 1 Jan");
        out.clear();
        write_date(&mut out, 951_782_400, 0);
        assert_eq!(out, "Tue 29 Feb");
    }

    #[test]
    fn clocks_and_days_use_the_local_offset() {
        let afternoon = NOV_4_2026_MIDNIGHT_UTC + 14 * HOUR_SECONDS + 5 * MINUTE_SECONDS;
        assert_eq!(clock(afternoon, 0), "14:05");
        assert_eq!(clock(afternoon, 2 * 3600), "16:05");
        assert_eq!(day_label(afternoon, afternoon, 0), "Today");
        assert_eq!(
            day_label(afternoon - DAY_SECONDS, afternoon, 0),
            "Yesterday"
        );
        assert_eq!(
            moment(afternoon - 2 * DAY_SECONDS, afternoon, 0),
            "Mon 2 Nov 14:05"
        );
        assert_eq!(moment(afternoon, afternoon, 0), "14:05");
    }
}
