//! Per-driver metric sources, dispatched through an enum.

mod amd;
mod generic;
mod intel;
mod nvidia;

use std::path::Path;
use std::time::Duration;

pub(crate) use nvidia::NvidiaBackend;

use super::snapshot::{BackendKind, GpuMetrics};
use amd::AmdBackend;
use generic::DevfreqBackend;
use intel::{I915Backend, XeBackend};

const MILLISECONDS_PER_SECOND: f64 = 1000.0;

/// Everything a backend may touch while sampling an active device.
pub(crate) struct DeviceSample<'a> {
    pub(crate) metrics: &'a mut GpuMetrics,
    pub(crate) buffer: &'a mut Vec<u8>,
    pub(crate) now: Duration,
    /// A GPU view is on screen, so slow optional queries may run.
    pub(crate) detailed: bool,
    /// The window is visible; minimized, only what history records is read.
    pub(crate) visible: bool,
}

#[derive(Debug)]
pub(crate) enum Backend {
    Amd(Box<AmdBackend>),
    Nvidia(NvidiaBackend),
    IntelI915(I915Backend),
    IntelXe(XeBackend),
    /// nouveau exposes hwmon only, which the device's hwmon reader covers.
    Nouveau,
    Generic(DevfreqBackend),
}

impl Backend {
    pub(crate) fn open(kind: BackendKind, card_directory: &Path, device_directory: &Path) -> Self {
        match kind {
            BackendKind::Amd => Self::Amd(Box::new(AmdBackend::open(device_directory))),
            BackendKind::Nvidia => Self::Nvidia(NvidiaBackend::default()),
            BackendKind::IntelI915 => Self::IntelI915(I915Backend::open(card_directory)),
            BackendKind::IntelXe => Self::IntelXe(XeBackend::open(device_directory)),
            BackendKind::Nouveau => Self::Nouveau,
            BackendKind::Generic => Self::Generic(DevfreqBackend::open(device_directory)),
        }
    }

    pub(crate) fn kind_for_driver(driver: &str) -> BackendKind {
        match driver {
            "amdgpu" => BackendKind::Amd,
            "nvidia" => BackendKind::Nvidia,
            "i915" => BackendKind::IntelI915,
            "xe" => BackendKind::IntelXe,
            "nouveau" => BackendKind::Nouveau,
            _ => BackendKind::Generic,
        }
    }

    /// Samples the sysfs-backed drivers; NVIDIA is sampled through NVML by
    /// the collector, which owns the library session.
    pub(crate) fn sample_sysfs(&mut self, sample: &mut DeviceSample<'_>) {
        match self {
            Self::Amd(amd) => amd.sample(sample),
            Self::IntelI915(i915) => i915.sample(sample),
            Self::IntelXe(xe) => xe.sample(sample),
            Self::Generic(devfreq) => devfreq.sample(sample),
            Self::Nvidia(_) | Self::Nouveau => {}
        }
    }
}

/// Busy share of an engine derived from a monotonic idle-residency counter
/// in milliseconds (`rc6_residency_ms`, `idle_residency_ms`).
#[derive(Debug, Default)]
pub(crate) struct IdleResidency {
    file: Option<monitor_core::util::sysfs::PersistentFile>,
    previous: Option<(u64, Duration)>,
}

impl IdleResidency {
    pub(crate) fn open(path: &Path) -> Self {
        Self {
            file: crate::util::attribute::open(path),
            previous: None,
        }
    }

    pub(crate) fn busy(&mut self, now: Duration) -> Option<monitor_core::Percent> {
        let idle = crate::util::attribute::read_u64(self.file.as_ref())?;
        let previous = self.previous.replace((idle, now))?;
        let elapsed_ms = now.saturating_sub(previous.1).as_secs_f64() * MILLISECONDS_PER_SECOND;
        if elapsed_ms <= 0.0 {
            return None;
        }
        let idle_ratio = idle.saturating_sub(previous.0) as f64 / elapsed_ms;
        Some(monitor_core::Percent::clamped_ratio(
            1.0 - idle_ratio.min(1.0),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_backend_by_driver() {
        assert_eq!(Backend::kind_for_driver("amdgpu"), BackendKind::Amd);
        assert_eq!(Backend::kind_for_driver("xe"), BackendKind::IntelXe);
        assert_eq!(Backend::kind_for_driver("panfrost"), BackendKind::Generic);
    }
}
