use std::ops::{Index, IndexMut};

/// Time-weighted average and peak of a level-type reading.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Gauge {
    seconds: f64,
    weighted_sum: f64,
    peak: f64,
}

impl Gauge {
    pub fn add(&mut self, value: f64, seconds: f64) {
        if !value.is_finite() || seconds <= 0.0 {
            return;
        }
        self.peak = if self.seconds > 0.0 {
            self.peak.max(value)
        } else {
            value
        };
        self.seconds += seconds;
        self.weighted_sum += value * seconds;
    }

    pub fn add_optional(&mut self, value: Option<f64>, seconds: f64) {
        if let Some(value) = value {
            self.add(value, seconds);
        }
    }

    pub fn average(&self) -> Option<f64> {
        (self.seconds > 0.0).then(|| self.weighted_sum / self.seconds)
    }

    pub fn peak(&self) -> Option<f64> {
        (self.seconds > 0.0).then_some(self.peak)
    }
}

/// The gauges stored in `readings` as `<column>_avg` / `<column>_peak`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GaugeMetric {
    CpuPercent,
    MemoryBytes,
    PressurePercent,
    SwapBytes,
    GpuPercent,
    NpuPercent,
    CpuCelsius,
    HottestCelsius,
    FanRpm,
    PowerWatts,
    BatteryPercent,
}

impl GaugeMetric {
    pub const COUNT: usize = 11;
    pub const ALL: [Self; Self::COUNT] = [
        Self::CpuPercent,
        Self::MemoryBytes,
        Self::PressurePercent,
        Self::SwapBytes,
        Self::GpuPercent,
        Self::NpuPercent,
        Self::CpuCelsius,
        Self::HottestCelsius,
        Self::FanRpm,
        Self::PowerWatts,
        Self::BatteryPercent,
    ];

    pub const fn column(self) -> &'static str {
        match self {
            Self::CpuPercent => "cpu_percent",
            Self::MemoryBytes => "memory_bytes",
            Self::PressurePercent => "pressure_percent",
            Self::SwapBytes => "swap_bytes",
            Self::GpuPercent => "gpu_percent",
            Self::NpuPercent => "npu_percent",
            Self::CpuCelsius => "cpu_celsius",
            Self::HottestCelsius => "hottest_celsius",
            Self::FanRpm => "fan_rpm",
            Self::PowerWatts => "power_watts",
            Self::BatteryPercent => "battery_percent",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Gauges([Gauge; GaugeMetric::COUNT]);

impl Gauges {
    pub fn iter(&self) -> impl Iterator<Item = (GaugeMetric, &Gauge)> {
        GaugeMetric::ALL.into_iter().zip(&self.0)
    }
}

impl Index<GaugeMetric> for Gauges {
    type Output = Gauge;

    fn index(&self, metric: GaugeMetric) -> &Gauge {
        &self.0[metric as usize]
    }
}

impl IndexMut<GaugeMetric> for Gauges {
    fn index_mut(&mut self, metric: GaugeMetric) -> &mut Gauge {
        &mut self.0[metric as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weights_by_time_and_tracks_peak() {
        let mut gauge = Gauge::default();
        assert_eq!(gauge.average(), None);
        gauge.add(10.0, 1.0);
        gauge.add(40.0, 2.0);
        gauge.add(f64::NAN, 5.0);
        assert_eq!(gauge.average(), Some(30.0));
        assert_eq!(gauge.peak(), Some(40.0));
    }

    #[test]
    fn metric_order_matches_index() {
        assert!(
            GaugeMetric::ALL
                .iter()
                .enumerate()
                .all(|(index, metric)| *metric as usize == index)
        );
    }
}
