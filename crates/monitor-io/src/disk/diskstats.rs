//! `/proc/diskstats` lines and the rates derived from two readings.

use monitor_core::util::parse;
use monitor_core::{Bytes, BytesPerSecond, Percent};

use crate::rate::per_second;

/// `diskstats` sectors are always 512 bytes, whatever the device uses.
const SECTOR_BYTES: u64 = 512;
const MILLISECONDS_PER_SECOND: f64 = 1000.0;

/// Cumulative counters of one block device since boot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct DiskCounters {
    pub reads: u64,
    pub sectors_read: u64,
    pub read_milliseconds: u64,
    pub writes: u64,
    pub sectors_written: u64,
    pub write_milliseconds: u64,
    pub in_flight: u64,
    pub busy_milliseconds: u64,
    pub discards: u64,
}

impl DiskCounters {
    pub(crate) fn read_bytes(&self) -> Bytes {
        Bytes::new(self.sectors_read.saturating_mul(SECTOR_BYTES))
    }

    pub(crate) fn written_bytes(&self) -> Bytes {
        Bytes::new(self.sectors_written.saturating_mul(SECTOR_BYTES))
    }

    pub(crate) fn accumulate(&mut self, other: &Self) {
        self.reads = self.reads.saturating_add(other.reads);
        self.sectors_read = self.sectors_read.saturating_add(other.sectors_read);
        self.read_milliseconds = self
            .read_milliseconds
            .saturating_add(other.read_milliseconds);
        self.writes = self.writes.saturating_add(other.writes);
        self.sectors_written = self.sectors_written.saturating_add(other.sectors_written);
        self.write_milliseconds = self
            .write_milliseconds
            .saturating_add(other.write_milliseconds);
        self.in_flight = self.in_flight.saturating_add(other.in_flight);
        self.busy_milliseconds = self
            .busy_milliseconds
            .saturating_add(other.busy_milliseconds);
        self.discards = self.discards.saturating_add(other.discards);
    }
}

/// Splits a `diskstats` line into the device name and its counters.
/// Kernels before 4.18 lack the discard columns; they read as zero.
pub(crate) fn parse_line(line: &[u8]) -> Option<(&[u8], DiskCounters)> {
    let mut columns = parse::fields(line).skip(2);
    let name = columns.next()?;
    let mut values = [0_u64; 11];
    for slot in &mut values {
        match columns.next() {
            Some(column) => *slot = parse::parse_u64(column)?,
            None => break,
        }
    }
    let [
        reads,
        _,
        sectors_read,
        read_ms,
        writes,
        _,
        sectors_written,
        write_ms,
        in_flight,
        busy_ms,
        _,
    ] = values;
    let discards = columns.next().and_then(parse::parse_u64).unwrap_or(0);
    Some((
        name,
        DiskCounters {
            reads,
            sectors_read,
            read_milliseconds: read_ms,
            writes,
            sectors_written,
            write_milliseconds: write_ms,
            in_flight,
            busy_milliseconds: busy_ms,
            discards,
        },
    ))
}

/// Throughput and service figures of one device (or the sum of all
/// physical devices) over the last interval.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DiskRates {
    pub read: BytesPerSecond,
    pub write: BytesPerSecond,
    pub read_iops: f32,
    pub write_iops: f32,
    /// Mean time per completed read in the interval, `None` without reads.
    pub read_latency_ms: Option<f32>,
    pub write_latency_ms: Option<f32>,
    /// Share of the interval the device had I/O in flight.
    pub utilization: Percent,
    /// Requests in flight right now (queue depth).
    pub in_flight: u32,
    pub discards_per_second: f32,
}

impl DiskRates {
    pub(crate) fn between(earlier: &DiskCounters, later: &DiskCounters, seconds: f64) -> Self {
        let busy = later
            .busy_milliseconds
            .saturating_sub(earlier.busy_milliseconds) as f64;
        Self {
            read: BytesPerSecond::between(
                earlier.read_bytes().get(),
                later.read_bytes().get(),
                seconds,
            ),
            write: BytesPerSecond::between(
                earlier.written_bytes().get(),
                later.written_bytes().get(),
                seconds,
            ),
            read_iops: per_second(earlier.reads, later.reads, seconds),
            write_iops: per_second(earlier.writes, later.writes, seconds),
            read_latency_ms: latency(
                earlier.reads,
                later.reads,
                earlier.read_milliseconds,
                later.read_milliseconds,
            ),
            write_latency_ms: latency(
                earlier.writes,
                later.writes,
                earlier.write_milliseconds,
                later.write_milliseconds,
            ),
            utilization: if seconds > 0.0 {
                Percent::clamped_ratio(busy / (seconds * MILLISECONDS_PER_SECOND))
            } else {
                Percent::ZERO
            },
            in_flight: Self::queue(later),
            discards_per_second: per_second(earlier.discards, later.discards, seconds),
        }
    }

    /// Rates are unknown (first sample, gap) but the queue is still current.
    pub(crate) fn idle(counters: &DiskCounters) -> Self {
        Self {
            in_flight: Self::queue(counters),
            ..Self::default()
        }
    }

    fn queue(counters: &DiskCounters) -> u32 {
        u32::try_from(counters.in_flight).unwrap_or(u32::MAX)
    }
}

fn latency(earlier_ops: u64, later_ops: u64, earlier_ms: u64, later_ms: u64) -> Option<f32> {
    let operations = later_ops.checked_sub(earlier_ops).filter(|ops| *ops > 0)?;
    let milliseconds = later_ms.checked_sub(earlier_ms)?;
    Some((milliseconds as f64 / operations as f64) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DISKSTATS: &[u8] = b" 259       0 nvme1n1 154002 75102 10512814 29079 19628 26384 1451210 44631 0 27756 74777 0 0 0 0 820 1066
 259       1 nvme1n1p5 536 1645 11188 318 3 0 10 8 0 28 326 0 0 0 0 0 0
 259       2 nvme1n1p6 153368 73457 10498490 28752 19622 26384 1451200 44622 0 26724 73374 0 0 0 0 0 0
 259       3 nvme0n1 544531 243183 8302378 61796 55231 37651 11883000 185071 0 62508 247494 0 0 0 0 182 626
 259       4 nvme0n1p3 544462 243183 8300138 61730 55231 37651 11883000 185071 0 62475 246802 0 0 0 0 0 0
 253       0 zram0 41131 0 333856 315 673061 0 6161208 13342 0 14037 13657 0 0 0 0 0 0
";

    #[test]
    fn parses_every_line() {
        let parsed: Vec<_> = parse::lines(DISKSTATS).filter_map(parse_line).collect();
        assert_eq!(parsed.len(), 6);
        let (name, counters) = parsed[3];
        assert_eq!(name, b"nvme0n1");
        assert_eq!(counters.reads, 544_531);
        assert_eq!(counters.read_bytes().get(), 8_302_378 * 512);
        assert_eq!(counters.writes, 55_231);
        assert_eq!(counters.write_milliseconds, 185_071);
        assert_eq!(counters.busy_milliseconds, 62_508);
    }

    #[test]
    fn parses_old_kernels_without_discards() {
        let line = b"   8       0 sda 100 0 800 50 10 0 80 20 1 60 70";
        let Some((_, counters)) = parse_line(line) else {
            panic!("line without discard columns must parse");
        };
        assert_eq!(counters.in_flight, 1);
        assert_eq!(counters.discards, 0);
    }

    #[test]
    fn computes_rates_latency_and_utilization() {
        let earlier = DiskCounters {
            reads: 100,
            sectors_read: 1000,
            read_milliseconds: 50,
            busy_milliseconds: 1000,
            ..DiskCounters::default()
        };
        let later = DiskCounters {
            reads: 110,
            sectors_read: 3048,
            read_milliseconds: 80,
            busy_milliseconds: 1250,
            in_flight: 3,
            ..DiskCounters::default()
        };
        let rates = DiskRates::between(&earlier, &later, 0.5);
        assert_eq!(rates.read.get(), 2048.0 * 512.0 / 0.5);
        assert_eq!(rates.read_iops, 20.0);
        assert_eq!(rates.read_latency_ms, Some(3.0));
        assert_eq!(rates.write_latency_ms, None);
        assert_eq!(rates.utilization.get(), 50.0);
        assert_eq!(rates.in_flight, 3);
    }

    #[test]
    fn counter_reset_yields_zero() {
        let earlier = DiskCounters {
            reads: 500,
            sectors_read: 9000,
            ..DiskCounters::default()
        };
        let rates = DiskRates::between(&earlier, &DiskCounters::default(), 1.0);
        assert_eq!(rates.read, BytesPerSecond::ZERO);
        assert_eq!(rates.read_iops, 0.0);
        assert_eq!(rates.read_latency_ms, None);
    }
}
