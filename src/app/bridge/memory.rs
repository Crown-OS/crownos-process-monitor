//! Memory histories and the category breakdown; the Memory page reads the rest from the latest
//! engine snapshot.

use monitor_engine::Snapshot;
use monitor_engine::system::MemorySnapshot;

use crate::app::live::{HISTORY_CAPACITY, push_sample};

/// The breakdown ring and stacked chart, bottom first; they always add up to `MemTotal`.
pub const MEMORY_CATEGORIES: [&str; 8] = [
    "Apps",
    "Shared",
    "Cache",
    "Kernel",
    "Locked",
    "Compressed",
    "Other",
    "Free",
];
/// Every category but Free, the layers of the "used" chart.
pub const USED_CATEGORIES: usize = MEMORY_CATEGORIES.len() - 1;
pub const FREE_CATEGORY: usize = USED_CATEGORIES;

#[derive(Debug, Default)]
pub struct MemoryReadings {
    pub used_percent: Vec<f32>,
    /// Percent of `MemTotal` per used category, in [`MEMORY_CATEGORIES`] order.
    pub categories: [Vec<f32>; USED_CATEGORIES],
    pub swap_percent: Vec<f32>,
    pub total_bytes: u64,
    pub used_bytes: u64,
    /// Bytes per category, in [`MEMORY_CATEGORIES`] order.
    pub category_bytes: [u64; MEMORY_CATEGORIES.len()],
    pub strained: bool,
}

/// Splits `MemTotal` into the page's categories. Mlocked pages are mostly anonymous, so
/// Locked is carved out of Apps, which keeps the sum equal to the total.
pub fn category_bytes(memory: &MemorySnapshot) -> [u64; MEMORY_CATEGORIES.len()] {
    let categories = &memory.categories;
    let apps = categories.apps.get();
    let locked = memory.locked.get().min(apps);
    [
        apps - locked,
        categories.shared.get(),
        categories.cache.get(),
        categories.kernel.get(),
        locked,
        categories.compressed.get(),
        categories.other.get(),
        categories.free.get(),
    ]
}

fn percent_of(part: u64, total: u64) -> f32 {
    if total == 0 {
        f32::NAN
    } else {
        (part as f64 / total as f64 * 100.0) as f32
    }
}

impl MemoryReadings {
    pub(super) fn histories(&mut self) -> impl Iterator<Item = &mut Vec<f32>> {
        [&mut self.used_percent, &mut self.swap_percent]
            .into_iter()
            .chain(self.categories.iter_mut())
    }

    pub fn prepare(&mut self) {
        for history in self.histories() {
            history.clear();
            history.resize(HISTORY_CAPACITY, f32::NAN);
        }
    }

    pub fn push(&mut self, snapshot: &Snapshot) {
        let memory = &snapshot.memory;
        let total = memory.total.get();
        push_sample(&mut self.used_percent, percent_of(memory.used.get(), total));
        for (history, bytes) in self.categories.iter_mut().zip(category_bytes(memory)) {
            push_sample(history, percent_of(bytes, total));
        }
        push_sample(
            &mut self.swap_percent,
            percent_of(memory.swap_used.get(), memory.swap_total.get()),
        );
    }

    pub fn push_gap(&mut self) {
        for history in self.histories() {
            push_sample(history, f32::NAN);
        }
    }

    pub fn refresh(&mut self, snapshot: &Snapshot) {
        let memory = &snapshot.memory;
        self.total_bytes = memory.total.get();
        self.used_bytes = memory.used.get();
        self.category_bytes = category_bytes(memory);
        self.strained = snapshot.strain.memory.is_strained();
    }
}

#[cfg(test)]
mod tests {
    use monitor_core::Bytes;
    use monitor_engine::system::MemoryCategories;

    use super::*;

    const GIB: u64 = 1 << 30;

    fn sample() -> MemorySnapshot {
        MemorySnapshot {
            total: Bytes::new(16 * GIB),
            locked: Bytes::new(GIB / 2),
            categories: MemoryCategories {
                apps: Bytes::new(6 * GIB),
                shared: Bytes::new(GIB),
                cache: Bytes::new(4 * GIB),
                kernel: Bytes::new(GIB),
                compressed: Bytes::new(GIB / 2),
                free: Bytes::new(3 * GIB),
                other: Bytes::new(GIB / 2),
            },
            ..MemorySnapshot::default()
        }
    }

    #[test]
    fn categories_sum_to_the_total_with_locked_split_from_apps() {
        let memory = sample();
        let bytes = category_bytes(&memory);
        assert_eq!(bytes.iter().sum::<u64>(), memory.total.get());
        assert_eq!(bytes[0], 6 * GIB - GIB / 2);
        assert_eq!(bytes[4], GIB / 2);
        assert_eq!(bytes[FREE_CATEGORY], 3 * GIB);
    }

    #[test]
    fn locked_never_exceeds_apps() {
        let mut memory = sample();
        memory.locked = Bytes::new(64 * GIB);
        let bytes = category_bytes(&memory);
        assert_eq!(bytes[0], 0);
        assert_eq!(bytes[4], 6 * GIB);
    }

    #[test]
    fn percentages_of_an_empty_total_are_gaps() {
        assert!(percent_of(5, 0).is_nan());
        assert_eq!(percent_of(1, 4), 25.0);
    }
}
