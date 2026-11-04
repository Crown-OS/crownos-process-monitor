//! Enumeration of hwmon chips and thermal zones into open, ready-to-read
//! channels. Chips are keyed by driver name plus device path because hwmon
//! numbering changes across boots.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use monitor_core::util::sysfs::PersistentFile;
use monitor_core::{Celsius, Rpm};

use super::chip;
use super::snapshot::{
    FanControlSupport, FanId, FanIdentity, FanReading, SensorCategory, SensorIdentity,
    TemperatureReading,
};
use crate::gpu::wake::RuntimePmGuard;
use crate::util::{attribute, directory};

const BOGUS_RPM: u64 = u16::MAX as u64;

#[derive(Debug)]
pub(crate) struct FanChannel {
    pub(crate) rpm: PersistentFile,
    pub(crate) pwm: Option<PersistentFile>,
    pub(crate) index: usize,
}

/// One chip's open inputs, read together after its wake guard passes.
#[derive(Debug)]
pub(crate) struct ChipReader {
    pub(crate) guard: RuntimePmGuard,
    pub(crate) temperatures: Vec<(PersistentFile, usize)>,
    pub(crate) fans: Vec<FanChannel>,
}

#[derive(Debug, Default)]
pub(crate) struct SensorSet {
    pub(crate) chips: Vec<ChipReader>,
    /// Chips too slow to read on the sampler thread.
    pub(crate) slow_chips: Vec<ChipReader>,
    pub(crate) temperatures: Vec<TemperatureReading>,
    pub(crate) fans: Vec<FanReading>,
    /// `(rank, index into temperatures)`, best rank first.
    pub(crate) cpu_candidates: Vec<(u8, usize)>,
}

pub(crate) fn hwmon_root(sysfs: &Path) -> PathBuf {
    sysfs.join("class/hwmon")
}

pub(crate) fn discover(sysfs: &Path) -> SensorSet {
    let mut set = SensorSet::default();
    let root = hwmon_root(sysfs);
    let mut hwmon_devices = Vec::new();
    for name in directory::entries(&root, |name| directory::is_numbered(name, "hwmon")) {
        let Ok(chip_directory) = std::fs::canonicalize(root.join(name)) else {
            continue;
        };
        let device = std::fs::canonicalize(chip_directory.join("device"))
            .unwrap_or_else(|_| chip_directory.clone());
        add_hwmon_chip(&mut set, &chip_directory, &device);
        hwmon_devices.push(device);
    }
    add_thermal_zones(&mut set, sysfs, &hwmon_devices);
    set.cpu_candidates.sort_unstable();
    separate_slow_chips(&mut set);
    set
}

/// Reads every chip once, which also seeds the first readings, and moves
/// the slow ones aside.
fn separate_slow_chips(set: &mut SensorSet) {
    let mut buffer = Vec::new();
    for chip in std::mem::take(&mut set.chips) {
        if chip.read_timed(&mut buffer, &mut set.temperatures, &mut set.fans) {
            set.slow_chips.push(chip);
        } else {
            set.chips.push(chip);
        }
    }
}

fn channels(chip_directory: &Path, prefix: &str) -> Vec<u8> {
    let mut numbers: Vec<u8> = directory::entries(chip_directory, |name| {
        name.starts_with(prefix) && name.ends_with("_input")
    })
    .iter()
    .filter_map(|name| name[prefix.len()..name.len() - "_input".len()].parse().ok())
    .collect();
    numbers.sort_unstable();
    numbers
}

/// What every channel of one hwmon chip shares.
struct ChipFacts<'a> {
    directory: &'a Path,
    driver: Arc<str>,
    display: Arc<str>,
    key_prefix: String,
    category: SensorCategory,
}

impl ChipFacts<'_> {
    fn file(&self, name: String) -> PathBuf {
        self.directory.join(name)
    }

    fn threshold(&self, name: String) -> Option<Celsius> {
        attribute::read_number(self.file(name))
            .and_then(|milli| i64::try_from(milli).ok())
            .and_then(chip::plausible_temperature)
    }

    fn rpm_limit(&self, name: String) -> Option<Rpm> {
        attribute::read_number(self.file(name))
            .and_then(|rpm| u32::try_from(rpm).ok())
            .map(Rpm::new)
    }
}

fn add_hwmon_chip(set: &mut SensorSet, chip_directory: &Path, device: &Path) {
    let Some(driver) = attribute::read_text(chip_directory.join("name")) else {
        return;
    };
    let category = chip::category(&driver);
    let display: Arc<str> = match (category, device.file_name().and_then(|name| name.to_str())) {
        (SensorCategory::Drive, Some(device_name)) if &*driver == "nvme" => Arc::from(device_name),
        _ => Arc::clone(&driver),
    };
    let chip = ChipFacts {
        directory: chip_directory,
        key_prefix: format!("{driver}@{}", device.display()),
        driver,
        display,
        category,
    };
    let mut reader = ChipReader {
        guard: RuntimePmGuard::open(device),
        temperatures: Vec::new(),
        fans: Vec::new(),
    };
    add_temperature_channels(set, &mut reader, &chip);
    add_fan_channels(set, &mut reader, &chip);
    if !reader.temperatures.is_empty() || !reader.fans.is_empty() {
        set.chips.push(reader);
    }
}

fn add_temperature_channels(set: &mut SensorSet, reader: &mut ChipReader, chip: &ChipFacts<'_>) {
    for channel in channels(chip.directory, "temp") {
        let Some(input) = attribute::open(chip.file(format!("temp{channel}_input"))) else {
            continue;
        };
        let label = attribute::read_text(chip.file(format!("temp{channel}_label")))
            .unwrap_or_else(|| Arc::from(format!("temp{channel}")));
        let index = set.temperatures.len();
        if let Some(rank) = chip::cpu_rank(&chip.driver, &label, chip.category) {
            set.cpu_candidates.push((rank, index));
        }
        set.temperatures.push(TemperatureReading {
            identity: Arc::new(SensorIdentity {
                key: Arc::from(format!("{}/temp{channel}", chip.key_prefix)),
                chip: Arc::clone(&chip.display),
                label,
                category: chip.category,
                high: chip.threshold(format!("temp{channel}_max")),
                critical: chip.threshold(format!("temp{channel}_crit")),
            }),
            value: None,
        });
        reader.temperatures.push((input, index));
    }
}

fn add_fan_channels(set: &mut SensorSet, reader: &mut ChipReader, chip: &ChipFacts<'_>) {
    for channel in channels(chip.directory, "fan") {
        let Some(rpm) = attribute::open(chip.file(format!("fan{channel}_input"))) else {
            continue;
        };
        let pwm = attribute::open(chip.file(format!("pwm{channel}")));
        let control = if pwm.is_some() && chip.file(format!("pwm{channel}_enable")).exists() {
            FanControlSupport::Pwm
        } else {
            FanControlSupport::ReadOnly
        };
        let index = set.fans.len();
        set.fans.push(FanReading {
            identity: Arc::new(FanIdentity {
                id: FanId {
                    chip_directory: Arc::from(chip.directory),
                    channel,
                },
                key: Arc::from(format!("{}/fan{channel}", chip.key_prefix)),
                chip: Arc::clone(&chip.display),
                label: attribute::read_text(chip.file(format!("fan{channel}_label")))
                    .unwrap_or_else(|| Arc::from(format!("Fan {channel}"))),
                min: chip.rpm_limit(format!("fan{channel}_min")),
                max: chip.rpm_limit(format!("fan{channel}_max")),
                control,
            }),
            rpm: None,
            duty: None,
        });
        reader.fans.push(FanChannel { rpm, pwm, index });
    }
}

/// Thermal zones that no hwmon chip already mirrors (Arm SoCs, `x86_pkg_temp`).
fn add_thermal_zones(set: &mut SensorSet, sysfs: &Path, hwmon_devices: &[PathBuf]) {
    let root = sysfs.join("class/thermal");
    for name in directory::entries(&root, |name| directory::is_numbered(name, "thermal_zone")) {
        let Ok(zone) = std::fs::canonicalize(root.join(name)) else {
            continue;
        };
        if hwmon_devices.contains(&zone) {
            continue;
        }
        let (Some(kind), Some(input)) = (
            attribute::read_text(zone.join("type")),
            attribute::open(zone.join("temp")),
        ) else {
            continue;
        };
        let category = chip::category(&kind);
        let index = set.temperatures.len();
        if let Some(rank) = chip::cpu_rank(&kind, &kind, category) {
            set.cpu_candidates.push((rank, index));
        }
        set.temperatures.push(TemperatureReading {
            identity: Arc::new(SensorIdentity {
                key: Arc::from(format!("thermal:{kind}@{}", zone.display())),
                chip: Arc::from("ACPI"),
                label: Arc::clone(&kind),
                category,
                high: None,
                critical: None,
            }),
            value: None,
        });
        set.chips.push(ChipReader {
            guard: RuntimePmGuard::open(&zone),
            temperatures: vec![(input, index)],
            fans: Vec::new(),
        });
    }
}

pub(crate) fn plausible_rpm(rpm: u64) -> Option<Rpm> {
    (rpm != BOGUS_RPM)
        .then(|| u32::try_from(rpm).ok().map(Rpm::new))
        .flatten()
}
