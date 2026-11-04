//! NVIDIA through NVML. Only called while the device is awake and the
//! session is open; every field tolerates `NotSupported`.

use std::time::Duration;

use monitor_core::{Bytes, BytesPerSecond, Celsius, DeviceKey, Megahertz, Percent, Pid, Watts};
use nvml_wrapper::enum_wrappers::device::{
    Clock, PcieUtilCounter, PerformanceState as PState, TemperatureSensor,
};
use nvml_wrapper::enums::device::UsedGpuMemory;
use nvml_wrapper::{Device, Nvml};

use super::DeviceSample;
use crate::gpu::engine::{EngineClass, EngineUsage};
use crate::gpu::snapshot::{
    ClockEventReasons, GpuClocks, GpuFan, GpuMetrics, GpuProcessUsage, PerformanceState,
};
use monitor_core::util::cadence::Cadence;

/// Each PCIe throughput query blocks for ~20 ms inside NVML.
const PCIE_THROUGHPUT_INTERVAL: Duration = Duration::from_secs(5);
/// Off the GPU page, figures nothing records or alerts on (clocks, fan,
/// engines, limits) are refreshed this often and carried forward between;
/// while minimized they are not read at all.
const SECONDARY_INTERVAL: Duration = Duration::from_secs(5);
const KIB: f64 = 1024.0;

/// Identity facts only the library knows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct NvidiaIdentity {
    pub(crate) name: Option<String>,
    pub(crate) vbios: Option<String>,
    pub(crate) driver_version: Option<String>,
}

/// What the GPU page shows but history and alerts do not need.
#[derive(Clone, Copy, Debug, Default)]
struct SecondaryMetrics {
    engines: EngineUsage,
    clocks: GpuClocks,
    power_cap: Option<Watts>,
    fan: GpuFan,
    performance: Option<PerformanceState>,
    clock_events: ClockEventReasons,
}

impl SecondaryMetrics {
    fn apply(&self, metrics: &mut GpuMetrics) {
        for (class, value) in self.engines.iter() {
            metrics.engines.set(class, value);
        }
        metrics.clocks = self.clocks;
        metrics.power_cap = self.power_cap;
        metrics.fan = self.fan;
        metrics.performance = self.performance;
        metrics.clock_events = self.clock_events;
    }
}

#[derive(Debug, Default)]
pub(crate) struct NvidiaBackend {
    index: Option<u32>,
    secondary: SecondaryMetrics,
    secondary_cadence: Cadence,
    maximum_clocks: Option<(Option<Megahertz>, Option<Megahertz>)>,
    pcie_cadence: Cadence,
    pcie_throughput: (Option<BytesPerSecond>, Option<BytesPerSecond>),
    last_process_timestamp: u64,
}

impl NvidiaBackend {
    /// Forgets library handles after the session closed.
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }

    fn device<'n>(&mut self, nvml: &'n Nvml, bus_id: &DeviceKey) -> Option<Device<'n>> {
        if let Some(index) = self.index {
            return nvml.device_by_index(index).ok();
        }
        let count = nvml.device_count().ok()?;
        let (index, device) = (0..count).find_map(|index| {
            let device = nvml.device_by_index(index).ok()?;
            let pci = device.pci_info().ok()?;
            same_bus_id(&pci.bus_id, bus_id.as_str()).then_some((index, device))
        })?;
        self.index = Some(index);
        Some(device)
    }

    pub(crate) fn identity(&mut self, nvml: &Nvml, bus_id: &DeviceKey) -> NvidiaIdentity {
        let driver_version = nvml.sys_driver_version().ok();
        let Some(device) = self.device(nvml, bus_id) else {
            return NvidiaIdentity {
                driver_version,
                ..NvidiaIdentity::default()
            };
        };
        NvidiaIdentity {
            name: device.name().ok(),
            vbios: device.vbios_version().ok(),
            driver_version,
        }
    }

    pub(crate) fn sample(
        &mut self,
        nvml: &Nvml,
        bus_id: &DeviceKey,
        sample: &mut DeviceSample<'_>,
    ) {
        let Some(device) = self.device(nvml, bus_id) else {
            return;
        };
        let metrics = &mut *sample.metrics;
        if let Ok(rates) = device.utilization_rates() {
            let busy = Percent::clamped(rates.gpu as f32);
            metrics.utilization = Some(busy);
            metrics.engines.set(EngineClass::Graphics, busy);
        }
        if let Ok(memory) = device.memory_info() {
            metrics.memory.vram_used = Some(Bytes::new(memory.used));
            metrics.memory.vram_total = Some(Bytes::new(memory.total));
        }
        metrics.temperatures.edge = device
            .temperature(TemperatureSensor::Gpu)
            .ok()
            .map(|celsius| Celsius::new(celsius as f32));
        metrics.power_draw = device
            .power_usage()
            .ok()
            .map(|mw| Watts::from_milliwatts(mw.into()));
        if sample.detailed
            || (sample.visible && self.secondary_cadence.due(sample.now, SECONDARY_INTERVAL))
        {
            self.secondary = self.read_secondary(&device);
        }
        self.secondary.apply(metrics);
        if sample.detailed && self.pcie_cadence.due(sample.now, PCIE_THROUGHPUT_INTERVAL) {
            let throughput = |counter: PcieUtilCounter| {
                device
                    .pcie_throughput(counter)
                    .ok()
                    .map(|kib| BytesPerSecond::new(f64::from(kib) * KIB))
            };
            self.pcie_throughput = (
                throughput(PcieUtilCounter::Send),
                throughput(PcieUtilCounter::Receive),
            );
        }
        (metrics.pcie.transmit, metrics.pcie.receive) = self.pcie_throughput;
    }

    fn read_secondary(&mut self, device: &Device<'_>) -> SecondaryMetrics {
        let mut engines = EngineUsage::default();
        if let Ok(encoder) = device.encoder_utilization() {
            engines.set(
                EngineClass::VideoEncode,
                Percent::clamped(encoder.utilization as f32),
            );
        }
        if let Ok(decoder) = device.decoder_utilization() {
            engines.set(
                EngineClass::Video,
                Percent::clamped(decoder.utilization as f32),
            );
        }
        let clock = |domain: Clock| device.clock_info(domain).ok().map(Megahertz::new);
        let (core_max, memory_max) = *self.maximum_clocks.get_or_insert_with(|| {
            let maximum = |domain: Clock| device.max_clock_info(domain).ok().map(Megahertz::new);
            (maximum(Clock::Graphics), maximum(Clock::Memory))
        });
        SecondaryMetrics {
            engines,
            clocks: GpuClocks {
                core: clock(Clock::Graphics),
                core_max,
                memory: clock(Clock::Memory),
                memory_max,
            },
            power_cap: device
                .enforced_power_limit()
                .ok()
                .map(|mw| Watts::from_milliwatts(mw.into())),
            fan: GpuFan {
                duty: device
                    .fan_speed(0)
                    .ok()
                    .map(|duty| Percent::clamped(duty as f32)),
                ..GpuFan::default()
            },
            performance: device
                .performance_state()
                .ok()
                .and_then(pstate_number)
                .map(PerformanceState::NvidiaPState),
            clock_events: device
                .current_throttle_reasons()
                .map(|reasons| ClockEventReasons::from_bits(reasons.bits()))
                .unwrap_or_default(),
        }
    }

    /// Appends per-process memory and SM/encoder/decoder use for every user.
    /// The utilization query costs ~4 ms, so it only runs while some process
    /// holds a context and the device as a whole is busy.
    pub(crate) fn sample_processes(
        &mut self,
        nvml: &Nvml,
        bus_id: &DeviceKey,
        device_busy: bool,
        usage: &mut Vec<GpuProcessUsage>,
    ) {
        let Some(device) = self.device(nvml, bus_id) else {
            return;
        };
        let first = usage.len();
        let entry = |usage: &mut Vec<GpuProcessUsage>, pid: u32| -> usize {
            let pid = Pid::new(pid);
            usage[first..]
                .iter()
                .position(|existing| existing.pid == pid)
                .map_or_else(
                    || {
                        usage.push(GpuProcessUsage {
                            pid,
                            device: bus_id.clone(),
                            utilization: Percent::ZERO,
                            engines: EngineUsage::default(),
                            memory: Bytes::ZERO,
                            shared_memory: Bytes::ZERO,
                        });
                        usage.len() - 1
                    },
                    |offset| first + offset,
                )
        };
        let lists = [
            device.running_compute_processes(),
            device.running_graphics_processes(),
        ];
        for process in lists.into_iter().flatten().flatten() {
            let index = entry(usage, process.pid);
            if let UsedGpuMemory::Used(bytes) = process.used_gpu_memory {
                let memory = &mut usage[index].memory;
                *memory = (*memory).max(Bytes::new(bytes));
            }
        }
        if usage.len() == first || !device_busy {
            return;
        }
        if let Ok(mut samples) = device.process_utilization_stats(self.last_process_timestamp) {
            samples.sort_unstable_by_key(|sample| sample.timestamp);
            for sample in samples {
                self.last_process_timestamp = self.last_process_timestamp.max(sample.timestamp);
                let index = entry(usage, sample.pid);
                let process = &mut usage[index];
                let graphics = Percent::clamped(sample.sm_util as f32);
                process.engines.set(EngineClass::Graphics, graphics);
                process.engines.set(
                    EngineClass::VideoEncode,
                    Percent::clamped(sample.enc_util as f32),
                );
                process
                    .engines
                    .set(EngineClass::Video, Percent::clamped(sample.dec_util as f32));
                process.utilization = process.engines.busiest().unwrap_or(graphics);
            }
        }
    }
}

/// NVML spells the domain with eight hex digits (`00000000:01:00.0`).
fn same_bus_id(nvml_bus_id: &str, sysfs_bus_id: &str) -> bool {
    let split = |id: &str| -> Option<(u32, String)> {
        let (domain, rest) = id.split_once(':')?;
        Some((
            u32::from_str_radix(domain, 16).ok()?,
            rest.to_ascii_lowercase(),
        ))
    };
    matches!((split(nvml_bus_id), split(sysfs_bus_id)), (Some(left), Some(right)) if left == right)
}

fn pstate_number(state: PState) -> Option<u8> {
    Some(match state {
        PState::Zero => 0,
        PState::One => 1,
        PState::Two => 2,
        PState::Three => 3,
        PState::Four => 4,
        PState::Five => 5,
        PState::Six => 6,
        PState::Seven => 7,
        PState::Eight => 8,
        PState::Nine => 9,
        PState::Ten => 10,
        PState::Eleven => 11,
        PState::Twelve => 12,
        PState::Thirteen => 13,
        PState::Fourteen => 14,
        PState::Fifteen => 15,
        PState::Unknown => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_nvml_bus_ids() {
        assert!(same_bus_id("00000000:01:00.0", "0000:01:00.0"));
        assert!(same_bus_id("00000000:0A:00.0", "0000:0a:00.0"));
        assert!(!same_bus_id("00000000:02:00.0", "0000:01:00.0"));
        assert!(!same_bus_id("garbage", "0000:01:00.0"));
    }
}
