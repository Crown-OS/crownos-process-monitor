//! Plain-text readings of one GPU for the fact grids.

use std::fmt::Write;

use monitor_core::{Bytes, Celsius, Megahertz};
use monitor_engine::hardware::gpu::{AmdPerformanceLevel, GpuMemory, PcieLink, PcieSpeed};
use monitor_engine::hardware::{ClockEventReasons, GpuDevice, GpuPower, PerformanceState};

use crate::util::format::{TemperatureScale, bytes, count, frequency, percent, power, temperature};

pub(super) const NOT_REPORTED: &str = "–";

pub(super) fn megahertz(value: Option<Megahertz>) -> String {
    value.map_or_else(
        || NOT_REPORTED.to_owned(),
        |mhz| frequency(mhz.get() as f32),
    )
}

/// "1.80 GHz of 2.70 GHz".
pub(super) fn clock(current: Option<Megahertz>, maximum: Option<Megahertz>) -> String {
    match maximum {
        Some(_) => format!("{} of {}", megahertz(current), megahertz(maximum)),
        None => megahertz(current),
    }
}

/// "Gen 4 ×8 of Gen 4 ×16".
pub(super) fn describe_link(link: &PcieLink) -> String {
    let side = |speed: Option<PcieSpeed>, width: Option<u8>| {
        let mut out = String::new();
        if let Some(generation) = speed.and_then(|speed| speed.generation()) {
            let _ = write!(out, "Gen {generation}");
        }
        if let Some(width) = width {
            if !out.is_empty() {
                out.push(' ');
            }
            let _ = write!(out, "×{width}");
        }
        out
    };
    let current = side(link.speed, link.width);
    let maximum = side(link.max_speed, link.max_width);
    match (current.is_empty(), maximum.is_empty()) {
        (true, true) => NOT_REPORTED.to_owned(),
        (false, true) => current,
        (true, false) => format!("max {maximum}"),
        (false, false) => format!("{current} of {maximum}"),
    }
}

pub(super) fn describe_performance(state: Option<PerformanceState>) -> String {
    match state {
        Some(PerformanceState::NvidiaPState(level)) => format!("P{level}"),
        Some(PerformanceState::Amd(level)) => match level {
            AmdPerformanceLevel::Auto => "auto",
            AmdPerformanceLevel::Low => "low",
            AmdPerformanceLevel::High => "high",
            AmdPerformanceLevel::Manual => "manual",
            AmdPerformanceLevel::ProfileStandard => "profile standard",
            AmdPerformanceLevel::ProfileMinSclk => "profile min core clock",
            AmdPerformanceLevel::ProfileMinMclk => "profile min memory clock",
            AmdPerformanceLevel::ProfilePeak => "profile peak",
            AmdPerformanceLevel::PerfDeterminism => "determinism",
        }
        .to_owned(),
        None => NOT_REPORTED.to_owned(),
    }
}

pub(super) fn describe_throttle(reasons: ClockEventReasons) -> String {
    let labels: Vec<_> = reasons.labels().collect();
    if labels.is_empty() {
        "None".to_owned()
    } else {
        labels.join(", ")
    }
}

pub(super) const fn power_state_label(state: GpuPower) -> &'static str {
    match state {
        GpuPower::Active => "Active",
        GpuPower::Asleep => "Asleep",
        GpuPower::Unknown => "Unknown (left alone)",
    }
}

pub(super) fn temperatures(device: &GpuDevice, scale: TemperatureScale) -> String {
    let temperatures = &device.metrics.temperatures;
    let shown = |value: Option<Celsius>| {
        temperature(value.map_or(f32::NAN, |celsius| celsius.get()), scale)
    };
    format!(
        "{} · {} · {}",
        shown(temperatures.edge),
        shown(temperatures.junction),
        shown(temperatures.memory)
    )
}

pub(super) fn power_draw(device: &GpuDevice) -> String {
    let metrics = &device.metrics;
    match (metrics.power_draw, metrics.power_cap) {
        (Some(draw), Some(cap)) => format!("{} of {} cap", power(draw.get()), power(cap.get())),
        (Some(draw), None) => power(draw.get()),
        (None, _) => NOT_REPORTED.to_owned(),
    }
}

pub(super) fn fan(device: &GpuDevice) -> String {
    let fan = device.metrics.fan;
    match (fan.rpm, fan.duty) {
        (Some(rpm), Some(duty)) => format!(
            "{} rpm · {}",
            count(u64::from(rpm.get())),
            percent(duty.get())
        ),
        (Some(rpm), None) => format!("{} rpm", count(u64::from(rpm.get()))),
        (None, Some(duty)) => percent(duty.get()),
        (None, None) => NOT_REPORTED.to_owned(),
    }
}

pub(super) fn memory_line(memory: &GpuMemory) -> String {
    let pair = |used: Option<Bytes>, total: Option<Bytes>| match (used, total) {
        (Some(used), Some(total)) => format!("{} of {}", bytes(used.get()), bytes(total.get())),
        (Some(used), None) => bytes(used.get()),
        _ => NOT_REPORTED.to_owned(),
    };
    format!(
        "VRAM {} · GTT {} · visible VRAM {}",
        pair(memory.vram_used, memory.vram_total),
        pair(memory.gtt_used, memory.gtt_total),
        pair(memory.visible_vram_used, memory.visible_vram_total)
    )
}

/// The weights of the VRAM/GTT bar: VRAM used, VRAM free, GTT used, GTT free.
pub(super) fn memory_weights(memory: &GpuMemory) -> [f32; 4] {
    let split = |used: Option<Bytes>, total: Option<Bytes>| {
        let used = used.map_or(0, |bytes| bytes.get());
        let total = total.map_or(used, |bytes| bytes.get());
        [used as f32, total.saturating_sub(used) as f32]
    };
    let [vram_used, vram_free] = split(memory.vram_used, memory.vram_total);
    let [gtt_used, gtt_free] = split(memory.gtt_used, memory.gtt_total);
    [vram_used, vram_free, gtt_used, gtt_free]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_compare_current_with_maximum() {
        let link = PcieLink {
            speed: Some(PcieSpeed::from_gigatransfers(16.0)),
            width: Some(8),
            max_speed: Some(PcieSpeed::from_gigatransfers(16.0)),
            max_width: Some(16),
            ..PcieLink::default()
        };
        assert_eq!(describe_link(&link), "Gen 4 ×8 of Gen 4 ×16");
        assert_eq!(describe_link(&PcieLink::default()), NOT_REPORTED);
    }

    #[test]
    fn memory_splits_into_used_and_free() {
        let memory = GpuMemory {
            vram_used: Some(Bytes::new(256)),
            vram_total: Some(Bytes::new(512)),
            gtt_used: Some(Bytes::new(100)),
            ..GpuMemory::default()
        };
        assert_eq!(memory_weights(&memory), [256.0, 256.0, 100.0, 0.0]);
    }

    #[test]
    fn performance_and_throttle_read_plainly() {
        assert_eq!(
            describe_performance(Some(PerformanceState::NvidiaPState(8))),
            "P8"
        );
        assert_eq!(describe_throttle(ClockEventReasons::default()), "None");
        let capped = ClockEventReasons::from_bits(ClockEventReasons::SOFTWARE_POWER_CAP);
        assert_eq!(describe_throttle(capped), "Power cap");
    }
}
