use std::path::Path;

use monitor_core::Megahertz;
use monitor_core::util::sysfs::PersistentFile;

use super::{DeviceSample, IdleResidency};
use crate::gpu::engine::EngineClass;
use crate::gpu::snapshot::ClockEventReasons;
use crate::util::attribute;

fn megahertz(file: &Option<PersistentFile>) -> Option<Megahertz> {
    attribute::read_u64(file.as_ref())
        .and_then(|mhz| u32::try_from(mhz).ok())
        .map(Megahertz::new)
}

#[derive(Debug)]
pub(crate) struct I915Backend {
    render_idle: IdleResidency,
    actual_frequency: Option<PersistentFile>,
    maximum_frequency: Option<PersistentFile>,
}

impl I915Backend {
    pub(crate) fn open(card: &Path) -> Self {
        let rc6 = [
            card.join("gt/gt0/rc6_residency_ms"),
            card.join("power/rc6_residency_ms"),
        ]
        .into_iter()
        .find(|path| path.exists())
        .unwrap_or_default();
        Self {
            render_idle: IdleResidency::open(&rc6),
            actual_frequency: attribute::open(card.join("gt_act_freq_mhz")),
            maximum_frequency: attribute::open(card.join("gt_RP0_freq_mhz"))
                .or_else(|| attribute::open(card.join("gt_max_freq_mhz"))),
        }
    }

    pub(crate) fn sample(&mut self, sample: &mut DeviceSample<'_>) {
        let metrics = &mut *sample.metrics;
        metrics.utilization = self.render_idle.busy(sample.now);
        if let Some(busy) = metrics.utilization {
            metrics.engines.set(EngineClass::Graphics, busy);
        }
        metrics.clocks.core = megahertz(&self.actual_frequency);
        metrics.clocks.core_max = megahertz(&self.maximum_frequency);
    }
}

const XE_THROTTLE_REASONS: [(&str, u64); 4] = [
    ("reason_pl1", ClockEventReasons::SOFTWARE_POWER_CAP),
    ("reason_pl2", ClockEventReasons::SOFTWARE_POWER_CAP),
    ("reason_thermal", ClockEventReasons::SOFTWARE_THERMAL),
    ("reason_prochot", ClockEventReasons::HARDWARE_SLOWDOWN),
];

#[derive(Debug)]
pub(crate) struct XeBackend {
    idle: IdleResidency,
    actual_frequency: Option<PersistentFile>,
    maximum_frequency: Option<PersistentFile>,
    throttle_reasons: Vec<(PersistentFile, u64)>,
}

impl XeBackend {
    pub(crate) fn open(device: &Path) -> Self {
        let gt = device.join("tile0/gt0");
        let throttle = gt.join("freq0/throttle");
        Self {
            idle: IdleResidency::open(&gt.join("gtidle/idle_residency_ms")),
            actual_frequency: attribute::open(gt.join("freq0/act_freq")),
            maximum_frequency: attribute::open(gt.join("freq0/rp0_freq"))
                .or_else(|| attribute::open(gt.join("freq0/max_freq"))),
            throttle_reasons: XE_THROTTLE_REASONS
                .iter()
                .filter_map(|(name, reason)| Some((attribute::open(throttle.join(name))?, *reason)))
                .collect(),
        }
    }

    pub(crate) fn sample(&mut self, sample: &mut DeviceSample<'_>) {
        let metrics = &mut *sample.metrics;
        metrics.utilization = self.idle.busy(sample.now);
        if let Some(busy) = metrics.utilization {
            metrics.engines.set(EngineClass::Graphics, busy);
        }
        metrics.clocks.core = megahertz(&self.actual_frequency);
        metrics.clocks.core_max = megahertz(&self.maximum_frequency);
        for (file, reason) in &self.throttle_reasons {
            if file.read_u64().is_ok_and(|active| active != 0) {
                metrics.clock_events.insert(*reason);
            }
        }
    }
}
