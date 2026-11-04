use monitor_core::Bytes;
use monitor_core::util::parse::{fields, key_value, kib_value, lines, parse_u64};

/// PSS, USS and swap from `/proc/<pid>/smaps_rollup`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SmapsRollup {
    pub proportional: Bytes,
    pub unique: Bytes,
    pub swap: Bytes,
}

impl SmapsRollup {
    pub fn parse(buffer: &[u8]) -> Self {
        let mut rollup = Self::default();
        for (key, value) in lines(buffer).filter_map(key_value) {
            let bytes = || Bytes::new(kib_value(value).unwrap_or(0));
            match key {
                b"Pss" => rollup.proportional = bytes(),
                b"Private_Clean" | b"Private_Dirty" | b"Private_Hugetlb" => {
                    rollup.unique += bytes()
                }
                b"Swap" => rollup.swap = bytes(),
                _ => {}
            }
        }
        rollup
    }
}

/// Peak and swapped resident memory from `/proc/<pid>/status`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct StatusMemory {
    pub peak: Bytes,
    pub swap: Bytes,
}

impl StatusMemory {
    pub fn parse(buffer: &[u8]) -> Self {
        let mut memory = Self::default();
        for (key, value) in lines(buffer).filter_map(key_value) {
            match key {
                b"VmHWM" => memory.peak = Bytes::new(kib_value(value).unwrap_or(0)),
                b"VmSwap" => {
                    memory.swap = Bytes::new(kib_value(value).unwrap_or(0));
                    break;
                }
                _ => {}
            }
        }
        memory
    }
}

/// `/proc/<pid>/io` counters, cumulative bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IoCounters {
    pub read_chars: u64,
    pub write_chars: u64,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub cancelled_write_bytes: u64,
}

impl IoCounters {
    pub fn parse(buffer: &[u8]) -> Self {
        let mut counters = Self::default();
        for (key, value) in lines(buffer).filter_map(key_value) {
            let value = fields(value).next().and_then(parse_u64).unwrap_or(0);
            match key {
                b"rchar" => counters.read_chars = value,
                b"wchar" => counters.write_chars = value,
                b"read_bytes" => counters.read_bytes = value,
                b"write_bytes" => counters.write_bytes = value,
                b"cancelled_write_bytes" => counters.cancelled_write_bytes = value,
                _ => {}
            }
        }
        counters
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SMAPS_ROLLUP: &[u8] =
        b"557047eb4000-7ffe42d2e000 ---p 00000000 00:00 0                          [rollup]\n\
Rss:                4136 kB\n\
Pss:                 196 kB\n\
Pss_Dirty:           112 kB\n\
Pss_Anon:            112 kB\n\
Pss_File:             84 kB\n\
Pss_Shmem:             0 kB\n\
Shared_Clean:       4024 kB\n\
Shared_Dirty:          0 kB\n\
Private_Clean:         0 kB\n\
Private_Dirty:       112 kB\n\
Referenced:         4136 kB\n\
Anonymous:           112 kB\n\
Private_Hugetlb:        0 kB\n\
Swap:                  8 kB\n\
SwapPss:               0 kB\n\
Locked:                0 kB\n";

    const STATUS: &[u8] = b"Name:\tcat\nUmask:\t0022\nState:\tR (running)\nUid:\t1000\t1000\t1000\t1000\n\
VmPeak:\t    6480 kB\nVmHWM:\t    4136 kB\nVmRSS:\t    4136 kB\nVmSwap:\t       0 kB\nThreads:\t1\n";

    const IO: &[u8] = b"rchar: 2884\nwchar: 0\nsyscr: 7\nsyscw: 0\nread_bytes: 4096\nwrite_bytes: 0\ncancelled_write_bytes: 0\n";

    #[test]
    fn parses_smaps_rollup() {
        let rollup = SmapsRollup::parse(SMAPS_ROLLUP);
        assert_eq!(rollup.proportional, Bytes::from_kib(196));
        assert_eq!(rollup.unique, Bytes::from_kib(112));
        assert_eq!(rollup.swap, Bytes::from_kib(8));
    }

    #[test]
    fn parses_status_memory() {
        let memory = StatusMemory::parse(STATUS);
        assert_eq!(memory.peak, Bytes::from_kib(4136));
        assert_eq!(memory.swap, Bytes::ZERO);
    }

    #[test]
    fn parses_io() {
        let counters = IoCounters::parse(IO);
        assert_eq!(counters.read_chars, 2884);
        assert_eq!(counters.read_bytes, 4096);
        assert_eq!(counters.write_bytes, 0);
    }
}
