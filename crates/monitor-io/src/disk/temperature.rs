//! Drive temperature from the device's hwmon: `nvme` registers one under the
//! controller, SATA disks get one from the `drivetemp` module.

use std::path::{Path, PathBuf};

use monitor_core::Celsius;
use monitor_core::util::sysfs::PersistentFile;

const SYS_BLOCK: &str = "/sys/block";
const NVME_CLASS: &str = "/sys/class/nvme";
const SENSOR_FILE: &str = "temp1_input";
const PLAUSIBLE_CELSIUS: std::ops::Range<f32> = -40.0..125.0;

#[derive(Debug)]
pub(crate) struct DriveSensor {
    input: PersistentFile,
}

impl DriveSensor {
    pub(crate) fn find(disk: &str) -> Option<Self> {
        let device = Path::new(SYS_BLOCK).join(disk).join("device");
        let controller =
            nvme_controller(disk).map(|controller| Path::new(NVME_CLASS).join(controller));
        let device_hwmon = device.join("hwmon");
        [Some(device), Some(device_hwmon), controller]
            .into_iter()
            .flatten()
            .find_map(|directory| hwmon_input(&directory))
            .and_then(|path| PersistentFile::open(path).ok())
            .map(|input| Self { input })
    }

    /// The current temperature; driver sentinels (0, -273, 127+) read as `None`.
    pub(crate) fn read(&self) -> Option<Celsius> {
        let celsius = Celsius::from_millidegrees(self.input.read_i64().ok()?);
        (PLAUSIBLE_CELSIUS.contains(&celsius.get()) && celsius.get() != 0.0).then_some(celsius)
    }
}

fn hwmon_input(directory: &Path) -> Option<PathBuf> {
    std::fs::read_dir(directory)
        .ok()?
        .flatten()
        .filter(|entry| entry.file_name().as_encoded_bytes().starts_with(b"hwmon"))
        .map(|entry| entry.path().join(SENSOR_FILE))
        .find(|path| path.exists())
}

/// `nvme0n1` → `nvme0`, for multipath setups whose `device` link points at
/// the subsystem instead of the controller.
fn nvme_controller(disk: &str) -> Option<&str> {
    let namespace = disk.strip_prefix("nvme")?;
    let digits = namespace.find('n')?;
    disk.get(..4 + digits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_nvme_controller_names() {
        assert_eq!(nvme_controller("nvme0n1"), Some("nvme0"));
        assert_eq!(nvme_controller("nvme12n3"), Some("nvme12"));
        assert_eq!(nvme_controller("sda"), None);
    }

    #[test]
    fn reads_live_nvme_temperature() {
        let Some(sensor) = DriveSensor::find("nvme0n1") else {
            return;
        };
        assert!(sensor.read().is_some_and(|celsius| celsius.get() > 0.0));
    }
}
