//! CPU histories and the few figures derived from each tick; everything else the CPU page
//! reads straight from the latest engine snapshot.

use monitor_engine::Snapshot;
use monitor_engine::alerts::StrainLimit;
use monitor_engine::hardware::{SensorCategory, SensorSnapshot, TemperatureReading};

use crate::app::live::{HISTORY_CAPACITY, push_sample};

/// Used when no sensor reports a critical or high limit; Intel and AMD parts throttle near it.
pub const TYPICAL_TJMAX_CELSIUS: f32 = 100.0;
const DEFAULT_BUSY_PERCENT: f32 = 90.0;
const DEFAULT_HOT_CELSIUS: f32 = 85.0;

/// CPU series are percentages of the whole machine.
#[derive(Debug, Default)]
pub struct CpuReadings {
    pub total: Vec<f32>,
    pub user: Vec<f32>,
    pub system: Vec<f32>,
    pub iowait: Vec<f32>,
    /// One-minute load average.
    pub load: Vec<f32>,
    /// Percent of time some task waited for a CPU over the last 10 s; NaN without PSI.
    pub pressure: Vec<f32>,
    pub context_switches_per_second: Vec<f32>,
    pub running_tasks: Vec<f32>,
    /// Megahertz.
    pub frequency_average: Vec<f32>,
    /// Megahertz of the fastest core.
    pub frequency_peak: Vec<f32>,
    /// °C of the package sensor.
    pub package_temperature: Vec<f32>,
    /// °C of the hottest other CPU sensor (CCD, die or core).
    pub hottest_core_temperature: Vec<f32>,
    /// Tjmax at every sample, so it charts as a reference line.
    pub tjmax_line: Vec<f32>,
    /// One value per fan in `sensors.fans`, NaN where unreadable.
    pub fan_rpm: Vec<f32>,
    pub temperature_celsius: f32,
    pub frequency_mhz: f32,
    pub load_average: [f32; 3],
    pub tjmax_celsius: f32,
    /// Machine-wide average since local midnight, from history.
    pub average_today: Option<f32>,
    pub busy_limit: f32,
    pub hot_limit: f32,
    pub busy_strained: bool,
    pub hot_strained: bool,
}

/// Sensors that measure the processor, in the engine's order.
pub fn cpu_sensors(sensors: &SensorSnapshot) -> impl Iterator<Item = &TemperatureReading> {
    sensors
        .temperatures
        .iter()
        .filter(|reading| reading.identity.category == SensorCategory::Cpu)
}

/// The processor's throttle limit: a sensor's critical or high mark, else the typical Tjmax.
pub fn tjmax(sensors: &SensorSnapshot) -> f32 {
    cpu_sensors(sensors)
        .find_map(|reading| reading.identity.critical.or(reading.identity.high))
        .map_or(TYPICAL_TJMAX_CELSIUS, |celsius| celsius.get())
}

/// The hottest CPU sensor other than the package reading.
fn hottest_core(sensors: &SensorSnapshot) -> f32 {
    let package = sensors.cpu_temperature.map(|celsius| celsius.get());
    cpu_sensors(sensors)
        .filter_map(|reading| reading.value.map(|celsius| celsius.get()))
        .filter(|value| Some(*value) != package)
        .fold(f32::NAN, f32::max)
}

impl CpuReadings {
    pub(super) fn histories(&mut self) -> impl Iterator<Item = &mut Vec<f32>> {
        [
            &mut self.total,
            &mut self.user,
            &mut self.system,
            &mut self.iowait,
            &mut self.frequency_average,
            &mut self.frequency_peak,
            &mut self.package_temperature,
            &mut self.hottest_core_temperature,
            &mut self.tjmax_line,
            &mut self.load,
            &mut self.pressure,
            &mut self.context_switches_per_second,
            &mut self.running_tasks,
        ]
        .into_iter()
    }

    pub fn prepare(&mut self) {
        for history in self.histories() {
            history.clear();
            history.resize(HISTORY_CAPACITY, f32::NAN);
        }
        self.temperature_celsius = f32::NAN;
        self.frequency_mhz = f32::NAN;
        self.tjmax_celsius = f32::NAN;
        self.busy_limit = DEFAULT_BUSY_PERCENT;
        self.hot_limit = DEFAULT_HOT_CELSIUS;
    }

    pub fn push(&mut self, snapshot: &Snapshot) {
        let cpu = &snapshot.cpu;
        let states = &cpu.states;
        push_sample(&mut self.total, cpu.total.get());
        push_sample(&mut self.user, states.user.get() + states.nice.get());
        push_sample(
            &mut self.system,
            states.system.get() + states.irq.get() + states.softirq.get(),
        );
        push_sample(&mut self.iowait, states.iowait.get());
        push_sample(&mut self.load, cpu.load.one);
        push_sample(
            &mut self.pressure,
            cpu.pressure
                .map_or(f32::NAN, |pressure| pressure.avg10.get()),
        );
        push_sample(
            &mut self.context_switches_per_second,
            cpu.events.context_switches,
        );
        push_sample(&mut self.running_tasks, cpu.procs_running as f32);
        let megahertz =
            |value: Option<monitor_core::Megahertz>| value.map_or(f32::NAN, |mhz| mhz.get() as f32);
        push_sample(
            &mut self.frequency_average,
            megahertz(cpu.frequency.average),
        );
        push_sample(&mut self.frequency_peak, megahertz(cpu.frequency.max));
        let sensors = &snapshot.sensors;
        push_sample(
            &mut self.package_temperature,
            sensors
                .cpu_temperature
                .map_or(f32::NAN, |celsius| celsius.get()),
        );
        push_sample(&mut self.hottest_core_temperature, hottest_core(sensors));
        let limit = tjmax(sensors);
        if limit != self.tjmax_celsius {
            self.tjmax_celsius = limit;
            self.tjmax_line.fill(limit);
        }
    }

    pub fn push_gap(&mut self) {
        for history in self.histories() {
            push_sample(history, f32::NAN);
        }
        let limit = self.tjmax_celsius;
        if let Some(last) = self.tjmax_line.last_mut() {
            *last = limit;
        }
    }

    pub fn refresh(&mut self, snapshot: &Snapshot) {
        self.fan_rpm.clear();
        self.fan_rpm.extend(
            snapshot
                .sensors
                .fans
                .iter()
                .map(|fan| fan.rpm.map_or(f32::NAN, |rpm| rpm.get() as f32)),
        );
        self.temperature_celsius = snapshot
            .sensors
            .cpu_temperature
            .map_or(f32::NAN, |celsius| celsius.get());
        if let Some(average) = snapshot.cpu.frequency.average {
            self.frequency_mhz = average.get() as f32;
        }
        let load = snapshot.cpu.load;
        self.load_average = [load.one, load.five, load.fifteen];
        let strain = &snapshot.strain;
        if let StrainLimit::Percent(limit) = strain.cpu.limit {
            self.busy_limit = limit.get();
        }
        if let StrainLimit::Celsius(limit) = strain.temperature.limit {
            self.hot_limit = limit.get();
        }
        self.busy_strained = strain.cpu.is_strained();
        self.hot_strained = self.temperature_celsius >= self.hot_limit;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use monitor_core::Celsius;
    use monitor_engine::hardware::sensors::SensorIdentity;

    use super::*;

    fn reading(label: &str, celsius: f32, critical: Option<f32>) -> TemperatureReading {
        TemperatureReading {
            identity: Arc::new(SensorIdentity {
                key: label.into(),
                chip: "k10temp".into(),
                label: label.into(),
                category: SensorCategory::Cpu,
                high: None,
                critical: critical.map(Celsius::new),
            }),
            value: Some(Celsius::new(celsius)),
        }
    }

    #[test]
    fn tjmax_prefers_the_sensor_limit() {
        let mut sensors = SensorSnapshot::default();
        assert_eq!(tjmax(&sensors), TYPICAL_TJMAX_CELSIUS);
        sensors.temperatures = vec![
            reading("Tctl", 60.0, None),
            reading("Tccd1", 72.0, Some(95.0)),
        ];
        sensors.cpu_temperature = Some(Celsius::new(60.0));
        assert_eq!(tjmax(&sensors), 95.0);
        assert_eq!(hottest_core(&sensors), 72.0);
    }

    #[test]
    fn a_gap_keeps_the_reference_line() {
        let mut readings = CpuReadings::default();
        readings.prepare();
        readings.push(&Snapshot::default());
        readings.push_gap();
        assert!(readings.total.last().is_some_and(|value| value.is_nan()));
        assert_eq!(
            readings.tjmax_line.last().copied(),
            Some(TYPICAL_TJMAX_CELSIUS)
        );
        assert!(readings.load.last().is_some_and(|value| value.is_nan()));
    }
}
