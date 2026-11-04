use std::sync::Arc;
use std::time::Duration;

use super::{GpuCollector, GpuRuntime};
use crate::gpu::accel;
use crate::gpu::backend::Backend;
use crate::gpu::device_hwmon::DeviceHwmon;
use crate::gpu::discovery;
use crate::gpu::nvml_session::SessionChange;
use crate::gpu::pcie::PcieFiles;
use crate::gpu::wake::RuntimePmGuard;
use crate::gpu::{BackendKind, DriverSupport, GpuMetrics, GpuPower};

const DISCOVERY_INTERVAL: Duration = Duration::from_secs(5);

impl GpuCollector {
    pub(super) fn rediscover(&mut self) {
        self.signatures = (
            discovery::signature(&self.paths),
            accel::signature(&self.paths),
        );
        self.devices = discovery::discover(&self.paths, &mut self.buffer)
            .into_iter()
            .map(|gpu| {
                let guard = RuntimePmGuard::open(&gpu.device_directory);
                let power_state = guard.state(&mut self.buffer);
                GpuRuntime {
                    guard,
                    hwmon: DeviceHwmon::open(&gpu.device_directory),
                    pcie: PcieFiles::open(&gpu.device_directory),
                    backend: Backend::open(
                        gpu.identity.backend,
                        &gpu.card_directory,
                        &gpu.device_directory,
                    ),
                    identity: Arc::new(gpu.identity),
                    metrics: GpuMetrics {
                        power_state,
                        ..GpuMetrics::default()
                    },
                }
            })
            .collect();
        self.accelerators = accel::discover(&self.paths);
        self.clients.reset();
        self.fdinfo_usage.clear();
        self.fdinfo_totals.clear();
        self.nvml_usage.clear();
        self.apply_nvml_identities();
        self.sync_identities();
    }

    pub(super) fn refresh_devices(&mut self, now: Duration) {
        if !self.discovery.due(now, DISCOVERY_INTERVAL) {
            return;
        }
        if (
            discovery::signature(&self.paths),
            accel::signature(&self.paths),
        ) != self.signatures
        {
            self.rediscover();
            return;
        }
        let drm = self.paths.drm();
        let mut changed = false;
        for device in &mut self.devices {
            let displays = discovery::connected_displays(&drm, &device.identity.card);
            if displays != device.identity.connected_displays {
                Arc::make_mut(&mut device.identity).connected_displays = displays;
                changed = true;
            }
        }
        if changed {
            self.sync_identities();
        }
    }

    fn sync_identities(&mut self) {
        self.identities.clear();
        self.identities.extend(
            self.devices
                .iter()
                .map(|device| Arc::clone(&device.identity)),
        );
    }

    fn apply_nvml_identities(&mut self) {
        let Some(nvml) = self.nvml.get() else {
            return;
        };
        for device in &mut self.devices {
            let Backend::Nvidia(nvidia) = &mut device.backend else {
                continue;
            };
            if device.metrics.power_state != GpuPower::Active {
                continue;
            }
            let facts = nvidia.identity(nvml, &device.identity.key);
            let identity = Arc::make_mut(&mut device.identity);
            if let Some(name) = facts.name {
                identity.name = Arc::from(name);
            }
            identity.vbios = facts.vbios.map(Arc::from).or(identity.vbios.take());
            identity.driver_version = facts
                .driver_version
                .map(Arc::from)
                .or(identity.driver_version.take());
            identity.support = DriverSupport::Full;
        }
    }

    pub(super) fn sync_nvml(&mut self, now: Duration) {
        let nvidia_awake = self.devices.iter().any(|device| {
            device.identity.backend == BackendKind::Nvidia
                && device.metrics.power_state == GpuPower::Active
        });
        match self.nvml.sync(nvidia_awake, now) {
            SessionChange::Opened => {
                self.apply_nvml_identities();
                self.sync_identities();
            }
            SessionChange::Closed => {
                for device in &mut self.devices {
                    if let Backend::Nvidia(nvidia) = &mut device.backend {
                        nvidia.reset();
                    }
                }
            }
            SessionChange::Unchanged if self.nvml.is_library_missing() => {
                self.mark_library_missing()
            }
            SessionChange::Unchanged => {}
        }
    }

    fn mark_library_missing(&mut self) {
        let mut changed = false;
        for device in &mut self.devices {
            if device.identity.backend == BackendKind::Nvidia
                && device.identity.support != DriverSupport::LibraryUnavailable
            {
                Arc::make_mut(&mut device.identity).support = DriverSupport::LibraryUnavailable;
                changed = true;
            }
        }
        if changed {
            self.sync_identities();
        }
    }
}
