use std::time::Duration;

use monitor_core::util::sysfs::PersistentFile;
use monitor_core::{Bytes, Collector, SampleContext, SourceResult};

use super::meminfo::MemInfo;
use super::pressure::classify_pressure;
use super::snapshot::{HugePages, MemoryPressure, MemorySnapshot, ZswapUsage};
use super::swaps::parse_swaps;
use super::vmstat::VmCounters;
use super::zram::ZramReader;
use crate::util::psi;

const DEVICE_SCAN_INTERVAL: Duration = Duration::from_secs(10);

/// `/proc/meminfo`, `vmstat`, `swaps`, `pressure/memory` and zram, read
/// through descriptors kept open between samples.
pub struct MemoryCollector {
    meminfo: PersistentFile,
    vmstat: PersistentFile,
    swaps: Option<PersistentFile>,
    pressure: Option<PersistentFile>,
    zram: ZramReader,
    zram_scanned_at: Option<Duration>,
    buffer: Vec<u8>,
    previous_counters: Option<VmCounters>,
}

impl MemoryCollector {
    pub fn new() -> SourceResult<Self> {
        Ok(Self {
            meminfo: PersistentFile::open("/proc/meminfo")?,
            vmstat: PersistentFile::open("/proc/vmstat")?,
            swaps: PersistentFile::open("/proc/swaps").ok(),
            pressure: PersistentFile::open("/proc/pressure/memory").ok(),
            zram: ZramReader::open(),
            zram_scanned_at: None,
            buffer: Vec::with_capacity(8192),
            previous_counters: None,
        })
    }

    fn read_counters(
        &mut self,
        output: &mut MemorySnapshot,
        seconds: Option<f64>,
    ) -> SourceResult<()> {
        let counters = VmCounters::parse(self.vmstat.read(&mut self.buffer)?);
        output.oom_kills = counters.oom_kills;
        let previous = self.previous_counters.replace(counters);
        let (swap_in, swap_out, minor, major) = match (previous, seconds) {
            (Some(previous), Some(seconds)) => {
                let rate = |earlier: u64, later: u64| {
                    (later.saturating_sub(earlier) as f64 / seconds) as f32
                };
                (
                    rate(previous.swap_in_pages, counters.swap_in_pages),
                    rate(previous.swap_out_pages, counters.swap_out_pages),
                    rate(previous.minor_faults(), counters.minor_faults()),
                    rate(previous.major_faults, counters.major_faults),
                )
            }
            _ => (0.0, 0.0, 0.0, 0.0),
        };
        output.swap_in_rate = swap_in;
        output.swap_out_rate = swap_out;
        output.minor_fault_rate = minor;
        output.major_fault_rate = major;
        output.gap = previous.is_none() || seconds.is_none();
        Ok(())
    }

    fn read_zram(&mut self, output: &mut MemorySnapshot, now: Duration) {
        let due = self
            .zram_scanned_at
            .is_none_or(|scanned| now.saturating_sub(scanned) >= DEVICE_SCAN_INTERVAL);
        if due {
            self.zram_scanned_at = Some(now);
            self.zram.rescan(&mut output.zram);
        }
        self.zram.read(&mut output.zram);
    }

    fn read_pressure(&mut self, output: &mut MemorySnapshot) {
        output.pressure = self
            .pressure
            .as_ref()
            .and_then(|file| file.read(&mut self.buffer).ok())
            .and_then(|bytes| match psi::parse_pressure(bytes) {
                (Some(some), full) => Some(MemoryPressure {
                    some,
                    full: full.unwrap_or_default(),
                }),
                (None, _) => None,
            });
    }
}

fn fill_meminfo(info: &MemInfo, output: &mut MemorySnapshot) {
    output.total = Bytes::new(info.total);
    output.used = info.used();
    output.available = Bytes::new(info.available);
    output.free = Bytes::new(info.free);
    output.locked = Bytes::new(info.mlocked);
    output.unevictable = Bytes::new(info.unevictable);
    output.dirty = Bytes::new(info.dirty);
    output.writeback = Bytes::new(info.writeback);
    output.vmalloc_used = Bytes::new(info.vmalloc_used);
    output.swap_total = Bytes::new(info.swap_total);
    output.swap_used = Bytes::new(info.swap_total.saturating_sub(info.swap_free));
    output.zswap = info.zswap.map(|pool| ZswapUsage {
        pool: Bytes::new(pool),
        stored: Bytes::new(info.zswapped.unwrap_or(0)),
    });
    output.huge_pages = HugePages {
        transparent: Bytes::new(info.anon_huge_pages),
        total: info.huge_pages_total,
        free: info.huge_pages_free,
        reserved: info.huge_pages_reserved,
        surplus: info.huge_pages_surplus,
        page_size: Bytes::new(info.huge_page_size),
    };
}

impl Collector for MemoryCollector {
    type Output = MemorySnapshot;

    fn sample(
        &mut self,
        context: &SampleContext<'_>,
        output: &mut MemorySnapshot,
    ) -> SourceResult<()> {
        let info = MemInfo::parse(self.meminfo.read(&mut self.buffer)?);
        fill_meminfo(&info, output);
        let seconds = Some(context.elapsed.seconds)
            .filter(|&seconds| seconds > 0.0 && !context.elapsed.suspended);
        self.read_counters(output, seconds)?;
        if let Some(swaps) = &self.swaps
            && let Ok(bytes) = swaps.read(&mut self.buffer)
        {
            parse_swaps(bytes, &mut output.swap_devices);
        }
        self.read_zram(output, context.tick.monotonic());
        let in_zram: Bytes = output.zram.iter().map(|device| device.memory_used).sum();
        let in_zswap = output.zswap.map_or(Bytes::ZERO, |zswap| zswap.pool);
        output.categories = info.categories(in_zram + in_zswap);
        self.read_pressure(output);
        output.pressure_level =
            classify_pressure(output.pressure.as_ref(), output.available, output.total);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use monitor_core::{Elapsed, FdIndex, Interest, Tick};

    use super::*;

    #[test]
    fn samples_live_machine() -> SourceResult<()> {
        let mut collector = MemoryCollector::new()?;
        let interest = Interest::default();
        let fds = FdIndex::default();
        let mut snapshot = MemorySnapshot::default();
        let mut context = SampleContext {
            tick: Tick::now(),
            elapsed: Elapsed::default(),
            interest: &interest,
            fds: &fds,
        };
        collector.sample(&context, &mut snapshot)?;
        assert!(snapshot.gap);
        context.elapsed = Elapsed {
            seconds: 1.0,
            suspended: false,
        };
        collector.sample(&context, &mut snapshot)?;
        assert!(!snapshot.gap);
        assert!(snapshot.total.get() > 0);
        assert!(snapshot.used <= snapshot.total);
        assert_eq!(snapshot.categories.sum(), snapshot.total);
        assert!(snapshot.swap_used <= snapshot.swap_total);
        assert!(snapshot.minor_fault_rate >= 0.0);
        Ok(())
    }
}
