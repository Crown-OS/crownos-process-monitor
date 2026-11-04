use monitor_core::util::time::DAY_SECONDS;

/// How long hour/day/five-minute-reading history is kept.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Retention {
    #[default]
    Days30,
    Days90,
    Months6,
    Year1,
}

impl Retention {
    pub const fn seconds(self) -> i64 {
        DAY_SECONDS
            * match self {
                Self::Days30 => 30,
                Self::Days90 => 90,
                Self::Months6 => 183,
                Self::Year1 => 366,
            }
    }
}
