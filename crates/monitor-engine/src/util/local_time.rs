use chrono::{Local, Offset};

/// The local time zone's current offset from UTC, for day boundaries.
pub fn utc_offset_seconds() -> i32 {
    Local::now().offset().fix().local_minus_utc()
}
