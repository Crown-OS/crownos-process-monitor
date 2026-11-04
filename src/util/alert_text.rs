//! Words for alerts: what fired, and its figures in the units the settings ask for.

use monitor_engine::alerts::{AlertKind, AlertUnit};

use super::format::{
    RateUnit, TemperatureScale, write_bytes, write_percent, write_rate, write_temperature,
};

/// The units a figure is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AlertUnits {
    pub rates: RateUnit,
    pub temperature: TemperatureScale,
}

/// A short name for a kind, as the quiet rules list them.
pub const fn kind_label(kind: AlertKind) -> &'static str {
    match kind {
        AlertKind::SustainedCpu => "Busy CPU",
        AlertKind::MemoryGrowth => "Memory growth",
        AlertKind::DiskBurst => "Disk burst",
        AlertKind::NetworkBurst => "Network burst",
        AlertKind::MachineHot => "Running hot",
        AlertKind::GpuBusy => "Busy GPU",
        AlertKind::MemoryLimit => "Memory limit",
        AlertKind::CpuLimit => "CPU limit",
    }
}

/// Appends `value`, measured in `unit`.
pub fn write_alert_value(out: &mut String, unit: AlertUnit, value: f64, units: AlertUnits) {
    match unit {
        AlertUnit::Percent => write_percent(out, value as f32),
        AlertUnit::Bytes => write_bytes(out, value.max(0.0) as u64),
        AlertUnit::BytesPerSecond => write_rate(out, value, units.rates),
        AlertUnit::Celsius => write_temperature(out, value as f32, units.temperature),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_has_a_label() {
        assert!(
            AlertKind::ALL
                .iter()
                .all(|kind| !kind_label(*kind).is_empty())
        );
    }

    #[test]
    fn values_use_their_unit() {
        let mut out = String::new();
        write_alert_value(&mut out, AlertUnit::Celsius, 91.4, AlertUnits::default());
        assert_eq!(out, "91 °C");
        out.clear();
        write_alert_value(
            &mut out,
            AlertUnit::Bytes,
            (2_u64 << 30) as f64,
            AlertUnits::default(),
        );
        assert_eq!(out, "2.00 GB");
    }
}
