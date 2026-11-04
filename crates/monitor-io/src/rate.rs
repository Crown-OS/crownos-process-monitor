/// Whether the rates of a snapshot describe the last interval.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum RateStatus {
    /// First sample: counters were read but there is nothing to compare.
    #[default]
    Warmup,
    Measured,
    /// The machine was suspended in between: record a gap, not a rate.
    Gap,
}

impl RateStatus {
    pub(crate) fn from_elapsed(had_previous: bool, suspended: bool) -> Self {
        match (had_previous, suspended) {
            (false, _) => Self::Warmup,
            (true, true) => Self::Gap,
            (true, false) => Self::Measured,
        }
    }
}

/// Events per second from two counter readings; resets yield zero.
pub(crate) fn per_second(earlier: u64, later: u64, seconds: f64) -> f32 {
    monitor_core::BytesPerSecond::between(earlier, later, seconds).get() as f32
}
