use std::path::Path;

use monitor_core::util::sysfs::PersistentFile;

use super::snapshot::GpuPower;
use crate::util::attribute;

/// Reads a device's runtime PM state, which never resumes it. Every other
/// device query must be skipped unless this reports [`GpuPower::Active`].
#[derive(Debug)]
pub(crate) struct RuntimePmGuard {
    status: Option<PersistentFile>,
}

impl RuntimePmGuard {
    pub(crate) fn open(device_directory: &Path) -> Self {
        Self {
            status: attribute::open(device_directory.join("power/runtime_status")),
        }
    }

    /// Devices without runtime PM (`unsupported`, or no attribute) are always on.
    pub(crate) fn state(&self, buffer: &mut Vec<u8>) -> GpuPower {
        let Some(status) = self.status.as_ref() else {
            return GpuPower::Active;
        };
        match status.read(buffer).map(<[u8]>::trim_ascii) {
            Ok(b"suspended" | b"suspending" | b"resuming") => GpuPower::Asleep,
            Ok(b"error") | Err(_) => GpuPower::Unknown,
            Ok(_) => GpuPower::Active,
        }
    }
}
