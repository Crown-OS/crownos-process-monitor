use monitor_core::Bytes;
use monitor_core::util::parse::{key_value, kib_value, lines, parse_u64};

use super::snapshot::MemoryCategories;

/// The `/proc/meminfo` lines the memory page uses, in bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct MemInfo {
    pub total: u64,
    pub free: u64,
    pub available: u64,
    pub buffers: u64,
    pub cached: u64,
    pub swap_total: u64,
    pub swap_free: u64,
    pub zswap: Option<u64>,
    pub zswapped: Option<u64>,
    pub dirty: u64,
    pub writeback: u64,
    pub anon_pages: u64,
    pub shmem: u64,
    pub slab_reclaimable: u64,
    pub slab_unreclaimable: u64,
    pub kernel_stack: u64,
    pub page_tables: u64,
    pub secondary_page_tables: u64,
    pub percpu: u64,
    pub vmalloc_used: u64,
    pub mlocked: u64,
    pub unevictable: u64,
    pub anon_huge_pages: u64,
    pub huge_pages_total: u64,
    pub huge_pages_free: u64,
    pub huge_pages_reserved: u64,
    pub huge_pages_surplus: u64,
    pub huge_page_size: u64,
}

impl MemInfo {
    pub fn parse(buffer: &[u8]) -> Self {
        let mut info = Self::default();
        for (key, value) in lines(buffer).filter_map(key_value) {
            let bytes = kib_value(value).unwrap_or(0);
            let count = || parse_u64(value).unwrap_or(0);
            match key {
                b"MemTotal" => info.total = bytes,
                b"MemFree" => info.free = bytes,
                b"MemAvailable" => info.available = bytes,
                b"Buffers" => info.buffers = bytes,
                b"Cached" => info.cached = bytes,
                b"SwapTotal" => info.swap_total = bytes,
                b"SwapFree" => info.swap_free = bytes,
                b"Zswap" => info.zswap = Some(bytes),
                b"Zswapped" => info.zswapped = Some(bytes),
                b"Dirty" => info.dirty = bytes,
                b"Writeback" => info.writeback = bytes,
                b"AnonPages" => info.anon_pages = bytes,
                b"Shmem" => info.shmem = bytes,
                b"SReclaimable" => info.slab_reclaimable = bytes,
                b"SUnreclaim" => info.slab_unreclaimable = bytes,
                b"KernelStack" => info.kernel_stack = bytes,
                b"PageTables" => info.page_tables = bytes,
                b"SecPageTables" => info.secondary_page_tables = bytes,
                b"Percpu" => info.percpu = bytes,
                b"VmallocUsed" => info.vmalloc_used = bytes,
                b"Mlocked" => info.mlocked = bytes,
                b"Unevictable" => info.unevictable = bytes,
                b"AnonHugePages" => info.anon_huge_pages = bytes,
                b"HugePages_Total" => info.huge_pages_total = count(),
                b"HugePages_Free" => info.huge_pages_free = count(),
                b"HugePages_Rsvd" => info.huge_pages_reserved = count(),
                b"HugePages_Surp" => info.huge_pages_surplus = count(),
                b"Hugepagesize" => info.huge_page_size = bytes,
                _ => {}
            }
        }
        info
    }

    pub fn used(&self) -> Bytes {
        Bytes::new(self.total.saturating_sub(self.available))
    }

    /// The ring categories of plan §2.4. `compressed` is RAM held by zram
    /// and the zswap pool, which no other category contains.
    pub fn categories(&self, compressed: Bytes) -> MemoryCategories {
        let apps = Bytes::new(self.anon_pages);
        let shared = Bytes::new(self.shmem);
        let cache = Bytes::new(
            self.cached.saturating_sub(self.shmem) + self.buffers + self.slab_reclaimable,
        );
        let kernel = Bytes::new(
            self.slab_unreclaimable
                + self.kernel_stack
                + self.page_tables
                + self.secondary_page_tables
                + self.percpu,
        );
        let free = Bytes::new(self.free);
        let shown = apps + shared + cache + kernel + compressed + free;
        MemoryCategories {
            apps,
            shared,
            cache,
            kernel,
            compressed,
            free,
            other: Bytes::new(self.total).saturating_sub(shown),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MEMINFO: &[u8] = b"MemTotal:       15520692 kB\n\
MemFree:         5861000 kB\n\
MemAvailable:    9121384 kB\n\
Buffers:          294312 kB\n\
Cached:          3155432 kB\n\
SwapCached:         1000 kB\n\
Unevictable:      195696 kB\n\
Mlocked:              16 kB\n\
SwapTotal:       4194300 kB\n\
SwapFree:        1263136 kB\n\
Zswap:                 0 kB\n\
Zswapped:              0 kB\n\
Dirty:              5320 kB\n\
Writeback:             0 kB\n\
AnonPages:       3961644 kB\n\
Mapped:           519684 kB\n\
Shmem:             45840 kB\n\
KReclaimable:     393740 kB\n\
Slab:             547244 kB\n\
SReclaimable:     336268 kB\n\
SUnreclaim:       210976 kB\n\
KernelStack:       22768 kB\n\
PageTables:       117232 kB\n\
SecPageTables:      4652 kB\n\
VmallocUsed:      149796 kB\n\
Percpu:            16192 kB\n\
AnonHugePages:    212992 kB\n\
HugePages_Total:       0\n\
HugePages_Free:        0\n\
HugePages_Rsvd:        0\n\
HugePages_Surp:        0\n\
Hugepagesize:       2048 kB\n";

    #[test]
    fn parses_real_meminfo() {
        let info = MemInfo::parse(MEMINFO);
        assert_eq!(info.total, 15_520_692 * 1024);
        assert_eq!(info.used().get(), (15_520_692 - 9_121_384) * 1024);
        assert_eq!(info.zswap, Some(0));
        assert_eq!(info.huge_page_size, 2048 * 1024);
        assert_eq!(info.mlocked, 16 * 1024);
    }

    #[test]
    fn categories_sum_to_total() {
        let info = MemInfo::parse(MEMINFO);
        let compressed = Bytes::new(737_804_288);
        let categories = info.categories(compressed);
        assert_eq!(
            categories.cache.get(),
            (3_155_432 - 45_840 + 294_312 + 336_268) * 1024
        );
        assert_eq!(
            categories.kernel.get(),
            (210_976 + 22_768 + 117_232 + 4652 + 16_192) * 1024
        );
        assert_eq!(categories.sum(), Bytes::new(info.total));
        assert!(categories.other.get() > 0);
    }
}
