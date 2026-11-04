//! The hwmon chip a GPU driver registers under its device: temperatures,
//! power and fan for amdgpu, Intel dGPUs, nouveau and radeon.

use std::path::Path;

use monitor_core::util::sysfs::PersistentFile;
use monitor_core::{Celsius, Percent, Rpm, Watts};

use super::snapshot::GpuMetrics;
use crate::util::{attribute, directory};

const MAX_TEMPERATURE_CHANNELS: u32 = 8;
const PWM_FULL_SCALE: f32 = 255.0;

#[derive(Debug, Default)]
pub(crate) struct DeviceHwmon {
    edge: Option<PersistentFile>,
    junction: Option<PersistentFile>,
    memory: Option<PersistentFile>,
    power: Option<PersistentFile>,
    power_cap: Option<PersistentFile>,
    fan: Option<PersistentFile>,
    pwm: Option<PersistentFile>,
}

impl DeviceHwmon {
    pub(crate) fn open(device_directory: &Path) -> Self {
        let hwmon_root = device_directory.join("hwmon");
        let Some(chip) =
            directory::entries(&hwmon_root, |name| directory::is_numbered(name, "hwmon"))
                .into_iter()
                .next()
        else {
            return Self::default();
        };
        let chip = hwmon_root.join(chip);
        let mut hwmon = Self {
            power: attribute::open(chip.join("power1_average"))
                .or_else(|| attribute::open(chip.join("power1_input"))),
            power_cap: attribute::open(chip.join("power1_cap")),
            fan: attribute::open(chip.join("fan1_input")),
            pwm: attribute::open(chip.join("pwm1")),
            ..Self::default()
        };
        for channel in 1..=MAX_TEMPERATURE_CHANNELS {
            let input = chip.join(format!("temp{channel}_input"));
            if !input.exists() {
                continue;
            }
            let label = attribute::read_text(chip.join(format!("temp{channel}_label")));
            let slot = match label.as_deref() {
                Some("junction" | "hotspot") => &mut hwmon.junction,
                Some("mem" | "vram") => &mut hwmon.memory,
                Some("edge") | None => &mut hwmon.edge,
                Some(_) => continue,
            };
            if slot.is_none() {
                *slot = attribute::open(input);
            }
        }
        hwmon
    }

    pub(crate) fn sample(&self, metrics: &mut GpuMetrics) {
        let temperature = |file: &Option<PersistentFile>| {
            attribute::read_i64(file.as_ref()).map(Celsius::from_millidegrees)
        };
        metrics.temperatures.edge = temperature(&self.edge);
        metrics.temperatures.junction = temperature(&self.junction);
        metrics.temperatures.memory = temperature(&self.memory);
        metrics.power_draw = attribute::read_u64(self.power.as_ref()).map(Watts::from_microwatts);
        metrics.power_cap = attribute::read_u64(self.power_cap.as_ref())
            .filter(|&cap| cap > 0)
            .map(Watts::from_microwatts);
        metrics.fan.rpm = attribute::read_u64(self.fan.as_ref())
            .and_then(|rpm| u32::try_from(rpm).ok())
            .map(Rpm::new);
        metrics.fan.duty = attribute::read_u64(self.pwm.as_ref())
            .map(|pwm| Percent::clamped(pwm as f32 / PWM_FULL_SCALE * 100.0));
    }
}
