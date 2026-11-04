use std::time::Duration;

use monitor_core::{Coverage, Interest, Percent};

use super::GpuCollector;
use crate::gpu::backend::{Backend, DeviceSample};
use crate::gpu::clients::DeviceTotals;
use crate::gpu::{GpuMetrics, GpuPower, wants_drm_fds};

impl GpuCollector {
    pub(super) fn sample_devices(&mut self, now: Duration, per_process: bool, interest: &Interest) {
        let detailed = wants_drm_fds(interest);
        let visible = interest.is_visible();
        let nvml = self.nvml.get();
        for (index, device) in self.devices.iter_mut().enumerate() {
            if device.metrics.power_state == GpuPower::Active {
                let mut sample = DeviceSample {
                    metrics: &mut device.metrics,
                    buffer: &mut self.buffer,
                    now,
                    detailed,
                    visible,
                };
                device.hwmon.sample(sample.metrics);
                if visible {
                    device.pcie.sample(&mut sample.metrics.pcie, sample.buffer);
                }
                match (&mut device.backend, nvml) {
                    (Backend::Nvidia(nvidia), Some(nvml)) => {
                        nvidia.sample(nvml, &device.identity.key, &mut sample);
                        if per_process {
                            let busy = sample
                                .metrics
                                .engines
                                .busiest()
                                .is_some_and(|busiest| busiest > Percent::ZERO);
                            nvidia.sample_processes(
                                nvml,
                                &device.identity.key,
                                busy,
                                &mut self.nvml_usage,
                            );
                        }
                    }
                    (backend, _) => backend.sample_sysfs(&mut sample),
                }
            }
            if let Some(totals) = self.fdinfo_totals.get(index) {
                apply_fdinfo_totals(&mut device.metrics, totals);
            }
        }
    }
}

/// fdinfo fills what the device itself does not report. It only sees the
/// clients whose descriptors were readable, hence the coverage mark.
fn apply_fdinfo_totals(metrics: &mut GpuMetrics, totals: &DeviceTotals) {
    let device_engines = !metrics.engines.is_empty();
    let mut filled = false;
    for (class, value) in totals.engines.iter() {
        if metrics.engines.get(class).is_none() {
            metrics.engines.set(class, value);
            filled = true;
        }
    }
    metrics.engine_coverage = match (device_engines, filled) {
        (_, true) => Coverage::OwnProcesses,
        (true, false) => Coverage::Full,
        (false, false) => Coverage::Unavailable,
    };
    if totals.clients == 0 {
        return;
    }
    if metrics.utilization.is_none() && metrics.power_state == GpuPower::Active {
        metrics.utilization = totals.engines.busiest();
    }
    let memory = &mut metrics.memory;
    if memory.vram_used.is_none() && totals.local_memory.get() > 0 {
        memory.vram_used = Some(totals.local_memory);
    }
    if memory.gtt_used.is_none() && totals.shared_memory.get() > 0 {
        memory.gtt_used = Some(totals.shared_memory);
    }
}
