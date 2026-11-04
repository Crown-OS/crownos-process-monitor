use rustix::thread::CpuSet;

use crate::util::cpulist::cpu_list;

/// The logical CPUs a process may run on, numbered as the kernel numbers them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct CpuMask(CpuSet);

impl CpuMask {
    /// The highest CPU number a mask can hold, plus one.
    pub const CAPACITY: usize = CpuSet::MAX_CPU;

    /// CPUs at or above [`Self::CAPACITY`] are ignored.
    pub fn from_cpus(cpus: impl IntoIterator<Item = usize>) -> Self {
        let mut mask = Self::default();
        cpus.into_iter().for_each(|cpu| mask.insert(cpu));
        mask
    }

    /// Parses a kernel CPU list such as `Cpus_allowed_list`'s `0-3,8`.
    pub fn parse_list(text: &str) -> Self {
        Self::from_cpus(cpu_list(text.as_bytes()).filter_map(|cpu| usize::try_from(cpu).ok()))
    }

    pub fn insert(&mut self, cpu: usize) {
        if cpu < Self::CAPACITY {
            self.0.set(cpu);
        }
    }

    pub fn remove(&mut self, cpu: usize) {
        if cpu < Self::CAPACITY {
            self.0.unset(cpu);
        }
    }

    pub fn contains(&self, cpu: usize) -> bool {
        cpu < Self::CAPACITY && self.0.is_set(cpu)
    }

    pub fn len(&self) -> usize {
        self.0.count() as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn cpus(&self) -> impl Iterator<Item = usize> + '_ {
        (0..Self::CAPACITY).filter(|&cpu| self.0.is_set(cpu))
    }

    pub(crate) const fn as_cpu_set(&self) -> &CpuSet {
        &self.0
    }

    pub(crate) const fn from_cpu_set(set: CpuSet) -> Self {
        Self(set)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_kernel_lists_and_ignores_out_of_range_cpus() {
        let mask = CpuMask::parse_list("0-3,8\n");
        assert_eq!(mask.cpus().collect::<Vec<_>>(), [0, 1, 2, 3, 8]);
        assert_eq!(mask.len(), 5);
        assert!(mask.contains(8) && !mask.contains(4));
        let mut edited = mask;
        edited.remove(0);
        edited.insert(CpuMask::CAPACITY);
        assert!(!edited.contains(0) && !edited.contains(CpuMask::CAPACITY));
        assert_ne!(edited, mask);
        assert!(CpuMask::parse_list("").is_empty());
    }
}
