use monitor_core::util::time::unix_now;
use monitor_engine::history::{RecordedShare, TimeRange};

use crate::components::range_picker::HistoryRange;
use crate::util::format::write_duration;
use crate::util::time::{write_clock, write_date};

/// The history query behind a range picker option.
pub const fn time_range(range: HistoryRange) -> TimeRange {
    match range {
        HistoryRange::OneHour => TimeRange::Hour1,
        HistoryRange::TwelveHours => TimeRange::Hours12,
        HistoryRange::Day => TimeRange::Hours24,
        HistoryRange::Week => TimeRange::Days7,
        HistoryRange::Month => TimeRange::Days30,
        HistoryRange::Quarter => TimeRange::Days90,
        HistoryRange::HalfYear => TimeRange::Months6,
        HistoryRange::Year => TimeRange::Year1,
    }
}

/// The range picker's `history` index as a query range; no pick means 1 h.
pub fn picked_range(index: Option<usize>) -> TimeRange {
    time_range(
        index
            .and_then(|index| HistoryRange::ALL.get(index).copied())
            .unwrap_or_default(),
    )
}

/// "Recorded 9h 40m of 24 h".
pub fn recorded_share_text(share: &RecordedShare, range_label: &str) -> String {
    let mut out = String::from("Recorded ");
    write_duration(&mut out, share.recorded_seconds.max(0.0) as u64);
    out.push_str(" of ");
    out.push_str(range_label);
    out
}

/// "Recording since Tue 3 Nov 14:05", or that nothing was recorded yet.
pub fn recording_since_text(since_unix: Option<i64>, utc_offset_seconds: i32) -> String {
    let Some(since) = since_unix.filter(|since| *since <= unix_now()) else {
        return "Nothing recorded yet".to_owned();
    };
    let mut out = String::from("Recording since ");
    write_date(&mut out, since, utc_offset_seconds);
    out.push(' ');
    write_clock(&mut out, since, utc_offset_seconds);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picker_options_map_onto_query_ranges_in_order() {
        for (option, preset) in HistoryRange::ALL.iter().zip(TimeRange::PRESETS) {
            assert_eq!(time_range(*option), preset);
        }
        assert_eq!(picked_range(None), TimeRange::Hour1);
        assert_eq!(picked_range(Some(99)), TimeRange::Hour1);
    }

    #[test]
    fn the_recorded_share_reads_naturally() {
        let share = RecordedShare {
            recorded_seconds: 9.0 * 3600.0 + 40.0 * 60.0,
            span_seconds: 86_400,
        };
        assert_eq!(
            recorded_share_text(&share, "24 h"),
            "Recorded 9h 40m of 24 h"
        );
        assert_eq!(recording_since_text(None, 0), "Nothing recorded yet");
        assert_eq!(
            recording_since_text(Some(0), 0),
            "Recording since Thu 1 Jan 00:00"
        );
    }
}
