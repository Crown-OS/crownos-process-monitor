use monitor_core::util::time::{
    DAY_SECONDS, FIVE_MINUTES_SECONDS, HOUR_SECONDS, MINUTE_SECONDS, MONDAY_ORIGIN_SECONDS,
    WEEK_SECONDS, local_midnight,
};

const MINUTE_TIER_LIMIT: i64 = 3 * HOUR_SECONDS;
const FIVE_MINUTE_TIER_LIMIT: i64 = 48 * HOUR_SECONDS;
const HOUR_TIER_LIMIT: i64 = 90 * DAY_SECONDS;

/// A half-open interval `[from, to)` of Unix seconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Span {
    pub from_unix: i64,
    pub to_unix: i64,
}

impl Span {
    pub const fn new(from_unix: i64, to_unix: i64) -> Self {
        Self { from_unix, to_unix }
    }

    pub const fn seconds(self) -> i64 {
        self.to_unix.saturating_sub(self.from_unix)
    }

    /// From local midnight to `now`.
    pub fn today(now_unix: i64, utc_offset_seconds: i32) -> Self {
        Self::last_days(1, now_unix, utc_offset_seconds)
    }

    /// The last `days` local calendar days including today.
    pub fn last_days(days: u16, now_unix: i64, utc_offset_seconds: i32) -> Self {
        let earlier_days = i64::from(days.saturating_sub(1));
        let midnight = local_midnight(now_unix, utc_offset_seconds);
        Self::new(midnight - earlier_days * DAY_SECONDS, now_unix)
    }

    pub fn last_7_days(now_unix: i64, utc_offset_seconds: i32) -> Self {
        Self::last_days(7, now_unix, utc_offset_seconds)
    }

    pub fn last_30_days(now_unix: i64, utc_offset_seconds: i32) -> Self {
        Self::last_days(30, now_unix, utc_offset_seconds)
    }

    pub fn resolution(self) -> Resolution {
        match self.seconds() {
            seconds if seconds <= MINUTE_TIER_LIMIT => Resolution::Minute,
            seconds if seconds <= FIVE_MINUTE_TIER_LIMIT => Resolution::FiveMinutes,
            seconds if seconds <= HOUR_TIER_LIMIT => Resolution::Hour,
            _ => Resolution::Week,
        }
    }
}

/// The history ranges offered by the UI, each ending now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TimeRange {
    Hour1,
    Hours12,
    Hours24,
    Days7,
    Days30,
    Days90,
    Months6,
    Year1,
    Custom(Span),
}

impl TimeRange {
    pub const PRESETS: [Self; 8] = [
        Self::Hour1,
        Self::Hours12,
        Self::Hours24,
        Self::Days7,
        Self::Days30,
        Self::Days90,
        Self::Months6,
        Self::Year1,
    ];

    pub const fn span(self, now_unix: i64) -> Span {
        let length = match self {
            Self::Hour1 => HOUR_SECONDS,
            Self::Hours12 => 12 * HOUR_SECONDS,
            Self::Hours24 => DAY_SECONDS,
            Self::Days7 => 7 * DAY_SECONDS,
            Self::Days30 => 30 * DAY_SECONDS,
            Self::Days90 => 90 * DAY_SECONDS,
            Self::Months6 => 183 * DAY_SECONDS,
            Self::Year1 => 365 * DAY_SECONDS,
            Self::Custom(span) => return span,
        };
        Span::new(now_unix - length, now_unix)
    }
}

/// Bucket size of a query, chosen from the span length (plan §6.3): minutes
/// up to 3 h, five minutes up to 48 h, hours up to 90 days, then weeks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Resolution {
    Minute,
    FiveMinutes,
    Hour,
    Week,
}

impl Resolution {
    pub const fn step_seconds(self) -> i64 {
        match self {
            Self::Minute => MINUTE_SECONDS,
            Self::FiveMinutes => FIVE_MINUTES_SECONDS,
            Self::Hour => HOUR_SECONDS,
            Self::Week => WEEK_SECONDS,
        }
    }

    pub const fn origin_unix(self) -> i64 {
        match self {
            Self::Week => MONDAY_ORIGIN_SECONDS,
            Self::Minute | Self::FiveMinutes | Self::Hour => 0,
        }
    }

    pub(crate) const fn system_table(self) -> &'static str {
        match self {
            Self::Minute => "system_minutes",
            Self::FiveMinutes => "system_five_minutes",
            Self::Hour | Self::Week => "system_hours",
        }
    }

    pub(crate) const fn app_table(self) -> &'static str {
        match self {
            Self::Minute | Self::FiveMinutes => "app_hours",
            Self::Hour | Self::Week => "app_days",
        }
    }

    /// Readings exist every five minutes, so finer requests use that.
    pub(crate) const fn readings_step_seconds(self) -> i64 {
        match self {
            Self::Minute => FIVE_MINUTES_SECONDS,
            _ => self.step_seconds(),
        }
    }
}

/// What an Over Time chart can plot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HistoryMetric {
    /// Percent of the whole machine.
    Cpu,
    /// Average bytes in use.
    Memory,
    /// PSI-based pressure percent.
    MemoryPressure,
    /// Average swap bytes in use.
    Swap,
    /// Busiest GPU, percent.
    Gpu,
    /// Bytes per second averaged over recorded time.
    DiskRead,
    DiskWrite,
    NetworkReceive,
    NetworkTransmit,
    /// Watts.
    Power,
    /// Hottest sensor, °C.
    Temperature,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_tier_from_length() {
        let now = 1_800_000_000;
        let resolutions: Vec<_> = TimeRange::PRESETS
            .iter()
            .map(|range| range.span(now).resolution())
            .collect();
        assert_eq!(
            resolutions,
            [
                Resolution::Minute,
                Resolution::FiveMinutes,
                Resolution::FiveMinutes,
                Resolution::Hour,
                Resolution::Hour,
                Resolution::Hour,
                Resolution::Week,
                Resolution::Week,
            ]
        );
    }

    #[test]
    fn last_days_start_at_local_midnight() {
        let midnight = 20_000 * DAY_SECONDS;
        let now = midnight + 10 * HOUR_SECONDS;
        assert_eq!(Span::today(now, 0), Span::new(midnight, now));
        assert_eq!(
            Span::last_7_days(now, 0).from_unix,
            midnight - 6 * DAY_SECONDS
        );
    }
}
