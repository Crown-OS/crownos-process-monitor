use crate::{Bytes, Percent};

/// PSI memory `some.avg10` (%) at which pressure counts as elevated.
const ELEVATED_SOME_AVG10: f32 = 10.0;
/// PSI memory `some.avg10` (%) at which pressure counts as critical.
const CRITICAL_SOME_AVG10: f32 = 40.0;
/// PSI memory `full.avg10` (%) that is elevated whatever `some` says.
const ELEVATED_FULL_AVG10: f32 = 5.0;
/// Without PSI: available share of total below which pressure is elevated.
const ELEVATED_AVAILABLE_RATIO: f64 = 0.20;
const CRITICAL_AVAILABLE_RATIO: f64 = 0.10;

/// Memory pressure classes, ordered by severity.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PressureLevel {
    #[default]
    Normal,
    Elevated,
    Critical,
}

/// PSI memory `avg10` shares of `some` and `full` stalls.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MemoryStall {
    pub some_avg10: Percent,
    pub full_avg10: Percent,
}

impl PressureLevel {
    /// Plan §2.1: from PSI when present, otherwise from the available ratio.
    #[must_use]
    pub fn classify(stall: Option<MemoryStall>, available: Bytes, total: Bytes) -> Self {
        match stall {
            Some(stall) => Self::from_stall(stall),
            None => Self::from_available_ratio(available.ratio_of(total)),
        }
    }

    #[must_use]
    pub fn from_stall(stall: MemoryStall) -> Self {
        let some = stall.some_avg10.get();
        if some >= CRITICAL_SOME_AVG10 {
            Self::Critical
        } else if some >= ELEVATED_SOME_AVG10 || stall.full_avg10.get() >= ELEVATED_FULL_AVG10 {
            Self::Elevated
        } else {
            Self::Normal
        }
    }

    #[must_use]
    pub fn from_available_ratio(ratio: f64) -> Self {
        if ratio < CRITICAL_AVAILABLE_RATIO {
            Self::Critical
        } else if ratio < ELEVATED_AVAILABLE_RATIO {
            Self::Elevated
        } else {
            Self::Normal
        }
    }

    /// Stable integer for persistence.
    #[must_use]
    pub const fn as_code(self) -> i64 {
        self as i64
    }

    #[must_use]
    pub const fn from_code(code: i64) -> Self {
        match code {
            0 => Self::Normal,
            1 => Self::Elevated,
            _ => Self::Critical,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stall(some: f32, full: f32) -> Option<MemoryStall> {
        Some(MemoryStall {
            some_avg10: Percent::new(some),
            full_avg10: Percent::new(full),
        })
    }

    #[test]
    fn classifies_psi() {
        let total = Bytes::new(100);
        let level = |some, full| PressureLevel::classify(stall(some, full), total, total);
        assert_eq!(level(0.0, 0.0), PressureLevel::Normal);
        assert_eq!(level(12.0, 0.0), PressureLevel::Elevated);
        assert_eq!(level(2.0, 6.0), PressureLevel::Elevated);
        assert_eq!(level(45.0, 0.0), PressureLevel::Critical);
    }

    #[test]
    fn falls_back_to_available_ratio() {
        let total = Bytes::new(100);
        let level = |available| PressureLevel::classify(None, Bytes::new(available), total);
        assert_eq!(level(50), PressureLevel::Normal);
        assert_eq!(level(15), PressureLevel::Elevated);
        assert_eq!(level(5), PressureLevel::Critical);
    }

    #[test]
    fn codes_round_trip() {
        for level in [
            PressureLevel::Normal,
            PressureLevel::Elevated,
            PressureLevel::Critical,
        ] {
            assert_eq!(PressureLevel::from_code(level.as_code()), level);
        }
    }
}
