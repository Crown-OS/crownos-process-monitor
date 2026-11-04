use std::sync::Arc;

use monitor_core::Bytes;

use super::pressure::PressureLevel;
use crate::pressure::PressureAverages;

#[derive(Clone, Debug, Default)]
pub struct MemorySnapshot {
    /// The interval spans a suspend or is the first sample: rates are zero.
    pub gap: bool,
    pub total: Bytes,
    /// `MemTotal − MemAvailable`.
    pub used: Bytes,
    pub available: Bytes,
    pub free: Bytes,
    pub categories: MemoryCategories,
    pub locked: Bytes,
    pub unevictable: Bytes,
    pub dirty: Bytes,
    pub writeback: Bytes,
    pub vmalloc_used: Bytes,
    pub huge_pages: HugePages,
    pub swap_total: Bytes,
    pub swap_used: Bytes,
    pub swap_devices: Vec<SwapDevice>,
    /// Pages per second.
    pub swap_in_rate: f32,
    /// Pages per second.
    pub swap_out_rate: f32,
    /// Faults per second.
    pub minor_fault_rate: f32,
    /// Faults per second.
    pub major_fault_rate: f32,
    /// OOM kills since boot.
    pub oom_kills: u64,
    pub zram: Vec<ZramDevice>,
    /// `None` when the kernel reports no zswap lines.
    pub zswap: Option<ZswapUsage>,
    /// `None` without PSI.
    pub pressure: Option<MemoryPressure>,
    pub pressure_level: PressureLevel,
}

/// The breakdown ring: the fields always add up to `MemTotal`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MemoryCategories {
    /// `AnonPages`.
    pub apps: Bytes,
    /// `Shmem` (tmpfs, shared memory, GPU buffers).
    pub shared: Bytes,
    /// `Cached − Shmem + Buffers + SReclaimable`.
    pub cache: Bytes,
    /// `SUnreclaim + KernelStack + PageTables + SecPageTables + Percpu`.
    pub kernel: Bytes,
    /// RAM used by zram devices and the zswap pool.
    pub compressed: Bytes,
    pub free: Bytes,
    /// The remainder, so the ring is consistent.
    pub other: Bytes,
}

impl MemoryCategories {
    pub fn sum(&self) -> Bytes {
        self.apps
            + self.shared
            + self.cache
            + self.kernel
            + self.compressed
            + self.free
            + self.other
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HugePages {
    /// Transparent huge pages backing anonymous memory (`AnonHugePages`).
    pub transparent: Bytes,
    pub total: u64,
    pub free: u64,
    pub reserved: u64,
    pub surplus: u64,
    pub page_size: Bytes,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SwapKind {
    #[default]
    Partition,
    File,
    Zram,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SwapDevice {
    pub name: Arc<str>,
    pub kind: SwapKind,
    pub size: Bytes,
    pub used: Bytes,
    pub priority: i32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ZramDevice {
    pub name: Arc<str>,
    pub disk_size: Bytes,
    /// Uncompressed size of the data stored.
    pub original: Bytes,
    pub compressed: Bytes,
    /// RAM consumed, allocator overhead included.
    pub memory_used: Bytes,
    pub algorithm: Option<Arc<str>>,
}

impl ZramDevice {
    /// Original over compressed size; zero when empty.
    pub fn ratio(&self) -> f64 {
        if self.compressed.get() == 0 {
            0.0
        } else {
            self.original.get() as f64 / self.compressed.get() as f64
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ZswapUsage {
    /// RAM used by the compressed pool (`Zswap`).
    pub pool: Bytes,
    /// Uncompressed size of what the pool holds (`Zswapped`).
    pub stored: Bytes,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MemoryPressure {
    pub some: PressureAverages,
    pub full: PressureAverages,
}
