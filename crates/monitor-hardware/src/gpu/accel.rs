//! NPUs under `/sys/class/accel` (intel_vpu, amdxdna).

use std::sync::Arc;
use std::time::Duration;

use monitor_core::util::sysfs::{self, PersistentFile};
use monitor_core::{DeviceKey, Percent};

use super::paths::SystemPaths;
use super::snapshot::{AcceleratorSnapshot, GpuPower, GpuVendor};
use super::wake::RuntimePmGuard;
use crate::util::{attribute, directory, pci_ids};

const MICROSECONDS_PER_SECOND: f64 = 1_000_000.0;

#[derive(Debug)]
pub(crate) struct Accelerator {
    snapshot: AcceleratorSnapshot,
    guard: RuntimePmGuard,
    busy_time: Option<PersistentFile>,
    previous: Option<(u64, Duration)>,
}

pub(crate) fn is_accelerator(name: &str) -> bool {
    directory::is_numbered(name, "accel")
}

pub(crate) fn signature(paths: &SystemPaths) -> u64 {
    directory::signature(&paths.accel(), is_accelerator)
}

pub(crate) fn discover(paths: &SystemPaths) -> Vec<Accelerator> {
    let root = paths.accel();
    directory::entries(&root, is_accelerator)
        .into_iter()
        .filter_map(|node| {
            let device = std::fs::canonicalize(root.join(node).join("device")).ok()?;
            let driver: Arc<str> = sysfs::link_name(device.join("driver"))?.into();
            let key = DeviceKey::new(device.file_name()?.to_str()?);
            let vendor =
                attribute::read_hex(device.join("vendor")).and_then(|id| u16::try_from(id).ok());
            let model =
                attribute::read_hex(device.join("device")).and_then(|id| u16::try_from(id).ok());
            let name = match (vendor, model) {
                (Some(vendor), Some(model)) => pci_ids::lookup_file(&paths.pci_ids, vendor, model)
                    .device
                    .map(|device| {
                        let prefix = GpuVendor::from_pci(vendor).short_name().unwrap_or_default();
                        format!("{prefix} {}", pci_ids::marketing_name(&device))
                            .trim()
                            .to_owned()
                    }),
                _ => None,
            };
            Some(Accelerator {
                snapshot: AcceleratorSnapshot {
                    name: name.map_or_else(|| Arc::clone(&driver), Arc::from),
                    key,
                    driver,
                    power_state: GpuPower::Active,
                    busy: None,
                },
                guard: RuntimePmGuard::open(&device),
                busy_time: attribute::open(device.join("npu_busy_time_us")),
                previous: None,
            })
        })
        .collect()
}

impl Accelerator {
    pub(crate) fn sample(&mut self, now: Duration, buffer: &mut Vec<u8>) -> &AcceleratorSnapshot {
        self.snapshot.power_state = self.guard.state(buffer);
        self.snapshot.busy = match self.snapshot.power_state {
            GpuPower::Active => self.busy(now),
            GpuPower::Asleep => Some(Percent::ZERO),
            GpuPower::Unknown => None,
        };
        &self.snapshot
    }

    fn busy(&mut self, now: Duration) -> Option<Percent> {
        let busy_us = attribute::read_u64(self.busy_time.as_ref())?;
        let (previous_us, previous_at) = self.previous.replace((busy_us, now))?;
        let elapsed_us = now.saturating_sub(previous_at).as_secs_f64() * MICROSECONDS_PER_SECOND;
        (elapsed_us > 0.0).then(|| {
            Percent::clamped_ratio(busy_us.saturating_sub(previous_us) as f64 / elapsed_us)
        })
    }
}
