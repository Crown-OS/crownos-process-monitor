use std::sync::Arc;
use std::time::Duration;

use monitor_core::{Collector, Coverage, Interest, Pid, SampleContext, SourceResult, Tick};

use crate::gpu::accel::Accelerator;
use crate::gpu::backend::Backend;
use crate::gpu::clients::{self, ClientTracker, DeviceTotals};
use crate::gpu::device_hwmon::DeviceHwmon;
use crate::gpu::nvml_session::NvmlSession;
use crate::gpu::paths::SystemPaths;
use crate::gpu::pcie::PcieFiles;
use crate::gpu::wake::RuntimePmGuard;
use crate::gpu::{
    GpuDevice, GpuIdentity, GpuMetrics, GpuProcessUsage, GpuSnapshot, HISTORY_PROCESS_INTERVAL,
    MINIMIZED_PROCESS_INTERVAL, MINIMIZED_TICK_SLACK, wants_drm_fds,
};
use monitor_core::util::cadence::{Cadence, interval_elapsed};

mod device_sampling;
mod inventory;

#[derive(Debug)]
pub(super) struct GpuRuntime {
    identity: Arc<GpuIdentity>,
    guard: RuntimePmGuard,
    hwmon: DeviceHwmon,
    pcie: PcieFiles,
    backend: Backend,
    metrics: GpuMetrics,
}

/// Collects [`GpuSnapshot`]s.
///
/// Per-process figures are gathered only on ticks where the engine refreshed
/// the DRM scope of the `FdIndex` (see [`GpuCollector::drm_fds_due`]); in
/// between, the previous per-process list is carried forward. Minimized,
/// devices are read only on those passes too and the output is left as it
/// was in between, so it must live across ticks.
#[derive(Debug)]
pub struct GpuCollector {
    paths: SystemPaths,
    devices: Vec<GpuRuntime>,
    pub(super) identities: Vec<Arc<GpuIdentity>>,
    accelerators: Vec<Accelerator>,
    signatures: (u64, u64),
    discovery: Cadence,
    nvml: NvmlSession,
    clients: ClientTracker,
    fdinfo_usage: Vec<GpuProcessUsage>,
    fdinfo_totals: Vec<DeviceTotals>,
    nvml_usage: Vec<GpuProcessUsage>,
    last_process_pass: Option<Duration>,
    last_device_pass: Option<Duration>,
    /// Helper fdinfo arrived and the next sample makes it a process pass.
    external_pending: bool,
    /// The last process pass used the helper's all-user fdinfo.
    external_coverage: bool,
    buffer: Vec<u8>,
}

impl Default for GpuCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl GpuCollector {
    pub fn new() -> Self {
        Self::with_paths(SystemPaths::default())
    }

    pub(super) fn with_paths(paths: SystemPaths) -> Self {
        let mut collector = Self {
            paths,
            devices: Vec::new(),
            identities: Vec::new(),
            accelerators: Vec::new(),
            signatures: (0, 0),
            discovery: Cadence::default(),
            nvml: NvmlSession::default(),
            clients: ClientTracker::default(),
            fdinfo_usage: Vec::new(),
            fdinfo_totals: Vec::new(),
            nvml_usage: Vec::new(),
            last_process_pass: None,
            last_device_pass: None,
            external_pending: false,
            external_coverage: false,
            buffer: Vec::new(),
        };
        collector.rediscover();
        collector
    }

    /// Whether the engine should refresh the `FdIndex` DRM scope before the
    /// next sample: always while a GPU view is visible, otherwise every
    /// [`HISTORY_PROCESS_INTERVAL`] ([`MINIMIZED_PROCESS_INTERVAL`] while
    /// minimized) for history.
    pub fn drm_fds_due(&self, interest: &Interest, tick: &Tick) -> bool {
        wants_drm_fds(interest) || pass_due(self.last_process_pass, interest, tick.monotonic())
    }

    /// Visible, devices are read every tick; minimized, on the DRM pass or
    /// every [`MINIMIZED_PROCESS_INTERVAL`] without one.
    pub(super) fn device_pass_due(
        &self,
        interest: &Interest,
        per_process: bool,
        now: Duration,
    ) -> bool {
        interest.is_visible() || per_process || pass_due(self.last_device_pass, interest, now)
    }

    /// Per-process usage from fdinfo texts of every user's DRM clients (the
    /// privileged helper's `DrmClients`), read at `now`. It replaces the
    /// `FdIndex` DRM scope: the next sample publishes it with full coverage.
    pub fn use_external_clients<'a>(
        &mut self,
        clients: impl IntoIterator<Item = (Pid, &'a [u8])>,
        now: Duration,
    ) {
        self.clients.scan_texts(
            clients,
            &self.identities,
            now,
            &mut self.fdinfo_usage,
            &mut self.fdinfo_totals,
        );
        self.nvml_usage.clear();
        self.last_process_pass = Some(now);
        self.external_pending = true;
        self.external_coverage = true;
    }

    fn publish(&self, output: &mut GpuSnapshot) {
        output.devices.clear();
        output
            .devices
            .extend(self.devices.iter().map(|device| GpuDevice {
                identity: Arc::clone(&device.identity),
                metrics: device.metrics,
            }));
        output.processes.clear();
        output.processes.extend_from_slice(&self.fdinfo_usage);
        output.processes.extend_from_slice(&self.nvml_usage);
        clients::merge_by_process(&mut output.processes);
        output.process_coverage = match (self.last_process_pass, self.fdinfo_usage.is_empty()) {
            (None, _) => Coverage::Unavailable,
            (Some(_), _) if self.external_coverage => Coverage::Full,
            (Some(_), false) => Coverage::OwnProcesses,
            (Some(_), true) if self.nvml.get().is_some() => Coverage::Full,
            (Some(_), true) => Coverage::OwnProcesses,
        };
    }
}

/// Whether a history pass last taken at `last` is due at `now`.
fn pass_due(last: Option<Duration>, interest: &Interest, now: Duration) -> bool {
    let interval = if interest.is_visible() {
        HISTORY_PROCESS_INTERVAL
    } else {
        MINIMIZED_PROCESS_INTERVAL.saturating_sub(MINIMIZED_TICK_SLACK)
    };
    interval_elapsed(last, now, interval)
}

impl Collector for GpuCollector {
    type Output = GpuSnapshot;

    fn sample(
        &mut self,
        context: &SampleContext<'_>,
        output: &mut GpuSnapshot,
    ) -> SourceResult<()> {
        let now = context.tick.monotonic();
        let external = std::mem::take(&mut self.external_pending);
        let per_process = external || context.fds.scope().drm;
        if !self.device_pass_due(context.interest, per_process, now) {
            return Ok(());
        }
        self.last_device_pass = Some(now);
        self.refresh_devices(now);
        for device in &mut self.devices {
            device.metrics = GpuMetrics {
                power_state: device.guard.state(&mut self.buffer),
                ..GpuMetrics::default()
            };
        }
        self.sync_nvml(now);
        if per_process && !external {
            self.external_coverage = false;
            self.clients.scan(
                context.fds.drm_fds(),
                &self.identities,
                now,
                &mut self.fdinfo_usage,
                &mut self.fdinfo_totals,
            );
            self.nvml_usage.clear();
            self.last_process_pass = Some(now);
        }
        self.sample_devices(now, per_process, context.interest);
        output.accelerators.clear();
        for accelerator in &mut self.accelerators {
            output
                .accelerators
                .push(accelerator.sample(now, &mut self.buffer).clone());
        }
        self.publish(output);
        Ok(())
    }
}
