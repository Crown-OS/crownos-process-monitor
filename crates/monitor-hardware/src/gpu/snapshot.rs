use std::sync::Arc;

use monitor_core::{
    Bytes, BytesPerSecond, Celsius, Coverage, DeviceKey, Megahertz, Percent, Pid, Rpm, Watts,
};

use super::engine::EngineUsage;

#[derive(Clone, Debug, Default)]
pub struct GpuSnapshot {
    pub devices: Vec<GpuDevice>,
    /// One entry per process and device it uses.
    pub processes: Vec<GpuProcessUsage>,
    /// fdinfo covers own processes only without the helper; NVML covers all.
    pub process_coverage: Coverage,
    pub accelerators: Vec<AcceleratorSnapshot>,
}

impl GpuSnapshot {
    pub fn device(&self, key: &DeviceKey) -> Option<&GpuDevice> {
        self.devices
            .iter()
            .find(|device| &device.identity.key == key)
    }
}

#[derive(Clone, Debug)]
pub struct GpuDevice {
    pub identity: Arc<GpuIdentity>,
    pub metrics: GpuMetrics,
}

/// What a GPU is. Rebuilt only on hotplug or when a better name arrives.
#[derive(Clone, Debug, PartialEq)]
pub struct GpuIdentity {
    /// PCI address (`0000:01:00.0`), or the platform device name.
    pub key: DeviceKey,
    pub card: Arc<str>,
    pub name: Arc<str>,
    pub vendor: GpuVendor,
    pub pci: PciIdentity,
    pub driver: Arc<str>,
    pub driver_version: Option<Arc<str>>,
    pub vbios: Option<Arc<str>>,
    pub kind: GpuKind,
    pub boot_vga: bool,
    /// Connector names whose status is `connected` (`eDP-1`, `HDMI-A-1`).
    pub connected_displays: Arc<[Arc<str>]>,
    pub backend: BackendKind,
    pub support: DriverSupport,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PciIdentity {
    pub vendor: u16,
    pub device: u16,
    pub subsystem_vendor: u16,
    pub subsystem_device: u16,
    pub revision: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GpuVendor {
    Amd,
    Nvidia,
    Intel,
    Other(u16),
}

impl GpuVendor {
    pub const fn from_pci(vendor: u16) -> Self {
        match vendor {
            0x1002 => Self::Amd,
            0x10de => Self::Nvidia,
            0x8086 => Self::Intel,
            other => Self::Other(other),
        }
    }

    pub const fn short_name(self) -> Option<&'static str> {
        match self {
            Self::Amd => Some("AMD"),
            Self::Nvidia => Some("NVIDIA"),
            Self::Intel => Some("Intel"),
            Self::Other(_) => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GpuKind {
    Integrated,
    Discrete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BackendKind {
    Amd,
    Nvidia,
    IntelI915,
    IntelXe,
    Nouveau,
    Generic,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DriverSupport {
    #[default]
    Full,
    /// The driver exposes only a few figures (nouveau, legacy radeon).
    Limited,
    /// The vendor library is missing (no `libnvidia-ml.so.1`).
    LibraryUnavailable,
}

/// Runtime power management state, read without waking the device.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GpuPower {
    #[default]
    Active,
    /// Runtime-suspended (D3hot/D3cold); nothing but fdinfo was read.
    Asleep,
    /// The runtime PM state reported an error; the device was left alone.
    Unknown,
}

/// Live figures of one GPU. Every field is `None` when the driver does not
/// report it or the device is asleep.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GpuMetrics {
    pub power_state: GpuPower,
    pub utilization: Option<Percent>,
    pub engines: EngineUsage,
    /// Engine figures summed from fdinfo cover own processes only.
    pub engine_coverage: Coverage,
    pub memory: GpuMemory,
    pub clocks: GpuClocks,
    pub temperatures: GpuTemperatures,
    pub power_draw: Option<Watts>,
    pub power_cap: Option<Watts>,
    pub fan: GpuFan,
    pub pcie: PcieLink,
    pub performance: Option<PerformanceState>,
    pub clock_events: ClockEventReasons,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GpuMemory {
    pub vram_used: Option<Bytes>,
    pub vram_total: Option<Bytes>,
    pub visible_vram_used: Option<Bytes>,
    pub visible_vram_total: Option<Bytes>,
    /// GTT / shared system memory mapped for the GPU.
    pub gtt_used: Option<Bytes>,
    pub gtt_total: Option<Bytes>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GpuClocks {
    pub core: Option<Megahertz>,
    pub core_max: Option<Megahertz>,
    pub memory: Option<Megahertz>,
    pub memory_max: Option<Megahertz>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GpuTemperatures {
    pub edge: Option<Celsius>,
    pub junction: Option<Celsius>,
    pub memory: Option<Celsius>,
}

impl GpuTemperatures {
    /// The figure a single "GPU temperature" reading shows.
    pub fn primary(&self) -> Option<Celsius> {
        self.edge.or(self.junction)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GpuFan {
    pub rpm: Option<Rpm>,
    pub duty: Option<Percent>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PcieLink {
    pub speed: Option<PcieSpeed>,
    pub width: Option<u8>,
    pub max_speed: Option<PcieSpeed>,
    pub max_width: Option<u8>,
    /// NVML only, refreshed every few seconds.
    pub transmit: Option<BytesPerSecond>,
    pub receive: Option<BytesPerSecond>,
}

/// Link transfer rate per lane, in GT/s.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct PcieSpeed(f32);

impl PcieSpeed {
    const GENERATION_RATES: [f32; 7] = [2.5, 5.0, 8.0, 16.0, 32.0, 64.0, 128.0];

    pub const fn from_gigatransfers(rate: f32) -> Self {
        Self(rate)
    }

    /// PCIe generation, 1 for 2.5 GT/s up to 7 for 128 GT/s.
    pub fn generation(self) -> Option<u8> {
        Self::GENERATION_RATES
            .iter()
            .position(|rate| (rate - self.0).abs() < 0.5)
            .and_then(|index| u8::try_from(index + 1).ok())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PerformanceState {
    /// NVIDIA P-state, 0 (maximum) to 15 (minimum).
    NvidiaPState(u8),
    Amd(AmdPerformanceLevel),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AmdPerformanceLevel {
    Auto,
    Low,
    High,
    Manual,
    ProfileStandard,
    ProfileMinSclk,
    ProfileMinMclk,
    ProfilePeak,
    PerfDeterminism,
}

impl AmdPerformanceLevel {
    pub fn parse(text: &[u8]) -> Option<Self> {
        Some(match text {
            b"auto" => Self::Auto,
            b"low" => Self::Low,
            b"high" => Self::High,
            b"manual" => Self::Manual,
            b"profile_standard" => Self::ProfileStandard,
            b"profile_min_sclk" => Self::ProfileMinSclk,
            b"profile_min_mclk" => Self::ProfileMinMclk,
            b"profile_peak" => Self::ProfilePeak,
            b"perf_determinism" => Self::PerfDeterminism,
            _ => return None,
        })
    }
}

/// Why clocks are held down. Bit values follow NVML's clock event reasons so
/// they convert losslessly; other drivers map onto the closest reason.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ClockEventReasons(u64);

impl ClockEventReasons {
    pub const GPU_IDLE: u64 = 0x1;
    pub const APPLICATION_CLOCKS: u64 = 0x2;
    pub const SOFTWARE_POWER_CAP: u64 = 0x4;
    pub const HARDWARE_SLOWDOWN: u64 = 0x8;
    pub const SYNC_BOOST: u64 = 0x10;
    pub const SOFTWARE_THERMAL: u64 = 0x20;
    pub const HARDWARE_THERMAL: u64 = 0x40;
    pub const HARDWARE_POWER_BRAKE: u64 = 0x80;
    pub const DISPLAY_CLOCKS: u64 = 0x100;

    const LABELS: [(u64, &'static str); 9] = [
        (Self::GPU_IDLE, "Idle"),
        (Self::APPLICATION_CLOCKS, "Application clocks"),
        (Self::SOFTWARE_POWER_CAP, "Power cap"),
        (Self::HARDWARE_SLOWDOWN, "Hardware slowdown"),
        (Self::SYNC_BOOST, "Sync boost"),
        (Self::SOFTWARE_THERMAL, "Thermal (software)"),
        (Self::HARDWARE_THERMAL, "Thermal (hardware)"),
        (Self::HARDWARE_POWER_BRAKE, "Power brake"),
        (Self::DISPLAY_CLOCKS, "Display clocks"),
    ];

    pub const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }

    pub const fn bits(self) -> u64 {
        self.0
    }

    pub const fn contains(self, reason: u64) -> bool {
        self.0 & reason == reason
    }

    pub fn insert(&mut self, reason: u64) {
        self.0 |= reason;
    }

    /// True when clocks are limited by power or temperature, not idleness.
    pub const fn is_throttling(self) -> bool {
        self.0 & !(Self::GPU_IDLE | Self::APPLICATION_CLOCKS | Self::DISPLAY_CLOCKS) != 0
    }

    pub fn labels(self) -> impl Iterator<Item = &'static str> {
        Self::LABELS
            .into_iter()
            .filter(move |(bit, _)| self.0 & bit != 0)
            .map(|(_, label)| label)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GpuProcessUsage {
    pub pid: Pid,
    pub device: DeviceKey,
    /// The busiest engine of this process on this device.
    pub utilization: Percent,
    pub engines: EngineUsage,
    /// Device-local memory (VRAM / local0).
    pub memory: Bytes,
    /// GTT / system memory mapped for the GPU.
    pub shared_memory: Bytes,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AcceleratorSnapshot {
    pub key: DeviceKey,
    pub name: Arc<str>,
    pub driver: Arc<str>,
    pub power_state: GpuPower,
    pub busy: Option<Percent>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcie_generation_from_rate() {
        assert_eq!(PcieSpeed::from_gigatransfers(16.0).generation(), Some(4));
        assert_eq!(PcieSpeed::from_gigatransfers(2.5).generation(), Some(1));
        assert_eq!(PcieSpeed::from_gigatransfers(3.0).generation(), None);
    }

    #[test]
    fn clock_event_labels() {
        let reasons = ClockEventReasons::from_bits(
            ClockEventReasons::GPU_IDLE | ClockEventReasons::SOFTWARE_POWER_CAP,
        );
        assert!(reasons.is_throttling());
        assert_eq!(reasons.labels().collect::<Vec<_>>(), ["Idle", "Power cap"]);
        assert!(!ClockEventReasons::from_bits(ClockEventReasons::GPU_IDLE).is_throttling());
    }
}
