//! Intel thermal throttle event counters.

use std::path::Path;

use monitor_core::util::sysfs::PersistentFile;

use crate::util::{attribute, directory};

#[derive(Debug, Default)]
pub(crate) struct ThrottleCounters {
    core: Vec<PersistentFile>,
    package: Vec<PersistentFile>,
}

impl ThrottleCounters {
    pub(crate) fn discover(sysfs: &Path) -> Self {
        let cpus = sysfs.join("devices/system/cpu");
        let mut counters = Self::default();
        for cpu in directory::entries(&cpus, |name| directory::is_numbered(name, "cpu")) {
            let throttle = cpus.join(cpu).join("thermal_throttle");
            counters
                .core
                .extend(attribute::open(throttle.join("core_throttle_count")));
            counters
                .package
                .extend(attribute::open(throttle.join("package_throttle_count")));
        }
        counters
    }

    /// Core events summed over CPUs, plus package events, which every CPU of
    /// a package repeats, counted once.
    pub(crate) fn read(&self) -> Option<u64> {
        if self.core.is_empty() && self.package.is_empty() {
            return None;
        }
        let core: u64 = self
            .core
            .iter()
            .filter_map(|file| file.read_u64().ok())
            .sum();
        let package = self
            .package
            .iter()
            .filter_map(|file| file.read_u64().ok())
            .max()
            .unwrap_or(0);
        Some(core + package)
    }
}
