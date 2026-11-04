pub use monitor_core::PressureLevel;
use monitor_core::{Bytes, MemoryStall};

use super::snapshot::MemoryPressure;

/// Plan §2.1: from PSI when present, otherwise from the available ratio.
pub fn classify_pressure(
    pressure: Option<&MemoryPressure>,
    available: Bytes,
    total: Bytes,
) -> PressureLevel {
    let stall = pressure.map(|pressure| MemoryStall {
        some_avg10: pressure.some.avg10,
        full_avg10: pressure.full.avg10,
    });
    PressureLevel::classify(stall, available, total)
}

#[cfg(test)]
mod tests {
    use monitor_core::Percent;

    use super::*;
    use crate::pressure::PressureAverages;

    #[test]
    fn classifies_from_memory_pressure() {
        let averages = |avg10| PressureAverages {
            avg10: Percent::new(avg10),
            ..PressureAverages::default()
        };
        let pressure = MemoryPressure {
            some: averages(45.0),
            full: averages(0.0),
        };
        let total = Bytes::new(100);
        assert_eq!(
            classify_pressure(Some(&pressure), total, total),
            PressureLevel::Critical
        );
        assert_eq!(
            classify_pressure(None, Bytes::new(5), total),
            PressureLevel::Critical
        );
    }
}
