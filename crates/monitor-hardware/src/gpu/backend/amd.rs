use std::path::Path;

use monitor_core::util::parse;
use monitor_core::util::sysfs::PersistentFile;
use monitor_core::{Bytes, Megahertz, Percent};

use super::DeviceSample;
use crate::gpu::engine::EngineClass;
use crate::gpu::gpu_metrics;
use crate::gpu::snapshot::{AmdPerformanceLevel, PerformanceState};
use crate::util::attribute;

#[derive(Debug)]
pub(crate) struct AmdBackend {
    busy: Option<PersistentFile>,
    video_busy: Option<PersistentFile>,
    gpu_metrics: Option<PersistentFile>,
    vram_used: Option<PersistentFile>,
    vram_total: Option<PersistentFile>,
    visible_vram_used: Option<PersistentFile>,
    visible_vram_total: Option<PersistentFile>,
    gtt_used: Option<PersistentFile>,
    gtt_total: Option<PersistentFile>,
    core_levels: Option<PersistentFile>,
    memory_levels: Option<PersistentFile>,
    performance_level: Option<PersistentFile>,
    maximum_clocks: Option<(Option<Megahertz>, Option<Megahertz>)>,
}

impl AmdBackend {
    pub(crate) fn open(device: &Path) -> Self {
        let open = |name: &str| attribute::open(device.join(name));
        Self {
            busy: open("gpu_busy_percent"),
            video_busy: open("vcn_busy_percent"),
            gpu_metrics: open("gpu_metrics"),
            vram_used: open("mem_info_vram_used"),
            vram_total: open("mem_info_vram_total"),
            visible_vram_used: open("mem_info_vis_vram_used"),
            visible_vram_total: open("mem_info_vis_vram_total"),
            gtt_used: open("mem_info_gtt_used"),
            gtt_total: open("mem_info_gtt_total"),
            core_levels: open("pp_dpm_sclk"),
            memory_levels: open("pp_dpm_mclk"),
            performance_level: open("power_dpm_force_performance_level"),
            maximum_clocks: None,
        }
    }

    pub(crate) fn sample(&mut self, sample: &mut DeviceSample<'_>) {
        let metrics = &mut *sample.metrics;
        let percent = |file: &Option<PersistentFile>| {
            attribute::read_u64(file.as_ref()).map(|value| Percent::clamped(value as f32))
        };
        let bytes =
            |file: &Option<PersistentFile>| attribute::read_u64(file.as_ref()).map(Bytes::new);
        metrics.utilization = percent(&self.busy);
        if let Some(video) = percent(&self.video_busy) {
            metrics.engines.set(EngineClass::Video, video);
        }
        metrics.memory.vram_used = bytes(&self.vram_used);
        metrics.memory.vram_total = bytes(&self.vram_total);
        metrics.memory.visible_vram_used = bytes(&self.visible_vram_used);
        metrics.memory.visible_vram_total = bytes(&self.visible_vram_total);
        metrics.memory.gtt_used = bytes(&self.gtt_used);
        metrics.memory.gtt_total = bytes(&self.gtt_total);

        if let Some(table) = attribute::read_bytes(self.gpu_metrics.as_ref(), sample.buffer)
            .and_then(gpu_metrics::parse)
        {
            metrics.utilization = metrics.utilization.or(table.gfx_activity);
            let temperatures = &mut metrics.temperatures;
            temperatures.edge = temperatures.edge.or(table.edge);
            temperatures.junction = temperatures.junction.or(table.hotspot);
            temperatures.memory = temperatures.memory.or(table.memory_temperature);
            metrics.power_draw = metrics.power_draw.or(table.socket_power);
            metrics.fan.rpm = metrics.fan.rpm.or(table.fan);
            metrics.clocks.core = table.gfx_clock;
            metrics.clocks.memory = table.memory_clock;
        }
        if metrics.clocks.core.is_none() {
            metrics.clocks.core = attribute::read_bytes(self.core_levels.as_ref(), sample.buffer)
                .and_then(current_level);
        }
        if metrics.clocks.memory.is_none() {
            metrics.clocks.memory =
                attribute::read_bytes(self.memory_levels.as_ref(), sample.buffer)
                    .and_then(current_level);
        }
        let (core_max, memory_max) = *self.maximum_clocks.get_or_insert_with(|| {
            (
                attribute::read_bytes(self.core_levels.as_ref(), sample.buffer)
                    .and_then(highest_level),
                attribute::read_bytes(self.memory_levels.as_ref(), sample.buffer)
                    .and_then(highest_level),
            )
        });
        metrics.clocks.core_max = core_max;
        metrics.clocks.memory_max = memory_max;
        metrics.performance = attribute::read_bytes(self.performance_level.as_ref(), sample.buffer)
            .and_then(AmdPerformanceLevel::parse)
            .map(PerformanceState::Amd);
    }
}

/// `pp_dpm_*` lines look like `1: 1100Mhz *`; the starred one is current.
fn levels(table: &[u8]) -> impl Iterator<Item = (Megahertz, bool)> + '_ {
    parse::lines(table).filter_map(|line| {
        let mut fields = parse::fields(line).skip(1);
        let frequency = fields.next()?;
        let digits = frequency
            .iter()
            .position(|byte| !byte.is_ascii_digit())
            .map_or(frequency, |end| &frequency[..end]);
        let mhz = parse::parse_u32(digits)?;
        Some((Megahertz::new(mhz), fields.next() == Some(b"*")))
    })
}

fn current_level(table: &[u8]) -> Option<Megahertz> {
    levels(table).find_map(|(mhz, current)| current.then_some(mhz))
}

fn highest_level(table: &[u8]) -> Option<Megahertz> {
    levels(table).map(|(mhz, _)| mhz).max()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCLK: &[u8] = b"0: 800Mhz *\n1: 1100Mhz \n2: 2700Mhz \n";
    const MCLK: &[u8] = b"0: 1000Mhz \n1: 2800Mhz *\n2: 2800Mhz *\n";

    #[test]
    fn reads_dpm_levels() {
        assert_eq!(current_level(SCLK), Some(Megahertz::new(800)));
        assert_eq!(highest_level(SCLK), Some(Megahertz::new(2700)));
        assert_eq!(current_level(MCLK), Some(Megahertz::new(2800)));
        assert_eq!(current_level(b"0: 500Mhz\n"), None);
    }

    #[test]
    fn parses_performance_level() {
        assert_eq!(
            AmdPerformanceLevel::parse(b"auto"),
            Some(AmdPerformanceLevel::Auto)
        );
        assert_eq!(AmdPerformanceLevel::parse(b"bogus"), None);
    }
}
