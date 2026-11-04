//! Physical disks, volumes, I/O pressure and drive health.

mod collector;
mod device;
mod diskstats;
mod health;
mod mount_watch;
mod mountinfo;
mod pressure;
mod stacking;
mod temperature;
mod udisks;
mod volumes;

use std::sync::Arc;

use monitor_core::{Bytes, Celsius};

pub use collector::DiskCollector;
pub use device::{DeviceInfo, MediaKind, Transport};
pub use diskstats::DiskRates;
pub use health::{AtaAttribute, AtaHealth, AttributeUnit, DriveHealth, HealthVerdict, NvmeHealth};
pub use pressure::IoPressure;
pub use stacking::{StackKind, StackedDevice};
pub use udisks::{RequestReceiver, UDisks};
pub use volumes::{Capacity, DeviceNumber, MountPoint, Volume, VolumeLocation, VolumeRole};

use crate::RateStatus;

/// One whole physical disk.
#[derive(Clone, Debug, PartialEq)]
pub struct DiskDevice {
    pub info: Arc<DeviceInfo>,
    pub rates: DiskRates,
    /// Since boot.
    pub read_total: Bytes,
    pub written_total: Bytes,
    /// From the drive's hwmon, else from its last SMART reading.
    pub temperature: Option<Celsius>,
    pub health: DriveHealth,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DiskSnapshot {
    pub status: RateStatus,
    pub devices: Vec<DiskDevice>,
    /// Sum over physical disks only, so stacked devices never count twice.
    /// `utilization` is that of the busiest disk.
    pub totals: DiskRates,
    pub read_total: Bytes,
    pub written_total: Bytes,
    /// Rebuilt when mounts change and every 10 s for capacity.
    pub volumes: Arc<[Volume]>,
    /// `None` without PSI (`psi=0`).
    pub pressure: Option<IoPressure>,
}
