//! GPUs and NPUs: discovery, per-vendor metrics behind a wake guard, and
//! per-process usage from DRM fdinfo and NVML.

mod accel;
mod backend;
mod clients;
mod collector;
mod device_hwmon;
mod discovery;
mod engine;
mod fdinfo;
mod gpu_metrics;
mod nvml_session;
mod paths;
mod pcie;
mod snapshot;
pub(crate) mod wake;

use std::time::Duration;

use monitor_core::{Interest, Page};

pub use collector::GpuCollector;
pub use engine::{EngineClass, EngineUsage};
pub use snapshot::{
    AcceleratorSnapshot, AmdPerformanceLevel, BackendKind, ClockEventReasons, DriverSupport,
    GpuClocks, GpuDevice, GpuFan, GpuIdentity, GpuKind, GpuMemory, GpuMetrics, GpuPower,
    GpuProcessUsage, GpuSnapshot, GpuTemperatures, GpuVendor, PciIdentity, PcieLink, PcieSpeed,
    PerformanceState,
};

/// How often per-process GPU usage is gathered for history while nobody
/// looks at a GPU view…
pub const HISTORY_PROCESS_INTERVAL: Duration = Duration::from_secs(5);
/// …and while the window is minimized, which is also how often device
/// metrics (busy, VRAM, power, temperature) are read then; in between the
/// last figures are carried, and history weights them by elapsed time.
pub const MINIMIZED_PROCESS_INTERVAL: Duration = Duration::from_secs(15);
/// Minimized ticks arrive every 5 s give or take scheduling latency; a pass
/// due within this is taken now rather than a whole tick late.
const MINIMIZED_TICK_SLACK: Duration = Duration::from_secs(1);

/// True when a visible view shows per-process GPU figures, so the engine
/// should refresh the DRM scope of the `FdIndex` on this tick.
pub fn wants_drm_fds(interest: &Interest) -> bool {
    interest.shows(Page::Gpu) || (interest.is_visible() && interest.inspected.is_some())
}

#[cfg(test)]
mod tests;
