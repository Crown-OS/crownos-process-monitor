use std::collections::HashSet;

use monitor_core::util::parse::parse_u64;
use monitor_core::util::sysfs::{read_into, read_string, read_u64};
use monitor_core::{Bytes, Megahertz};

use super::CPU_SYSFS;
use crate::util::cpulist::cpu_list;

/// Topology ids the kernel reports when a level does not exist.
const UNSET_IDS: [u64; 2] = [0xffff, u32::MAX as u64];
const INTEL_PERFORMANCE_CPUS: &str = "/sys/devices/cpu_core/cpus";
const INTEL_EFFICIENT_CPUS: &str = "/sys/devices/cpu_atom/cpus";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CoreType {
    /// Every core is alike, or the platform does not say.
    #[default]
    Uniform,
    Performance,
    Efficient,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CpuTopology {
    pub cpu: u32,
    pub online: bool,
    pub package: u32,
    pub die: Option<u32>,
    pub cluster: Option<u32>,
    /// Physical core id within the package; SMT siblings share it.
    pub core: u32,
    pub core_type: CoreType,
    /// Arm `cpu_capacity` (1024 = the biggest core).
    pub capacity: Option<u32>,
    pub max_frequency: Option<Megahertz>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CacheKind {
    Data,
    Instruction,
    #[default]
    Unified,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CacheLevel {
    pub level: u8,
    pub kind: CacheKind,
    /// Size of one instance.
    pub size: Bytes,
    /// How many separate instances exist (e.g. one L2 per core).
    pub instances: u32,
    pub line_size: Option<u32>,
    pub ways: Option<u32>,
}

pub(crate) fn read_cpu_list(path: &str) -> Vec<u32> {
    let mut buffer = Vec::new();
    read_into(path, &mut buffer).map_or_else(|_| Vec::new(), |bytes| cpu_list(bytes).collect())
}

fn topology_id(cpu: u32, name: &str) -> Option<u32> {
    read_u64(format!("{CPU_SYSFS}/cpu{cpu}/topology/{name}"))
        .ok()
        .filter(|id| !UNSET_IDS.contains(id))
        .and_then(|id| u32::try_from(id).ok())
}

pub(crate) fn read_topology() -> Vec<CpuTopology> {
    let online: HashSet<u32> = read_cpu_list(&format!("{CPU_SYSFS}/online"))
        .into_iter()
        .collect();
    let performance = read_cpu_list(INTEL_PERFORMANCE_CPUS);
    let efficient = read_cpu_list(INTEL_EFFICIENT_CPUS);
    let mut cpus: Vec<CpuTopology> = read_cpu_list(&format!("{CPU_SYSFS}/present"))
        .into_iter()
        .map(|cpu| CpuTopology {
            cpu,
            online: online.contains(&cpu),
            package: topology_id(cpu, "physical_package_id").unwrap_or(0),
            die: topology_id(cpu, "die_id"),
            cluster: topology_id(cpu, "cluster_id"),
            core: topology_id(cpu, "core_id").unwrap_or(cpu),
            core_type: if performance.contains(&cpu) {
                CoreType::Performance
            } else if efficient.contains(&cpu) {
                CoreType::Efficient
            } else {
                CoreType::Uniform
            },
            capacity: read_u64(format!("{CPU_SYSFS}/cpu{cpu}/cpu_capacity"))
                .ok()
                .and_then(|capacity| u32::try_from(capacity).ok()),
            max_frequency: read_u64(format!("{CPU_SYSFS}/cpu{cpu}/cpufreq/cpuinfo_max_freq"))
                .ok()
                .map(Megahertz::from_khz),
        })
        .collect();
    if cfg!(not(any(target_arch = "x86", target_arch = "x86_64"))) {
        classify_by_capacity(&mut cpus);
    }
    cpus
}

/// Arm big.LITTLE: the largest capacity is a performance core, any smaller
/// one an efficiency core. x86 capacities only encode preferred cores.
fn classify_by_capacity(cpus: &mut [CpuTopology]) {
    let capacities = || cpus.iter().filter_map(|cpu| cpu.capacity);
    let (Some(largest), Some(smallest)) = (capacities().max(), capacities().min()) else {
        return;
    };
    if largest == smallest {
        return;
    }
    for cpu in cpus
        .iter_mut()
        .filter(|cpu| cpu.core_type == CoreType::Uniform)
    {
        cpu.core_type = match cpu.capacity {
            Some(capacity) if capacity == largest => CoreType::Performance,
            Some(_) => CoreType::Efficient,
            None => CoreType::Uniform,
        };
    }
}

pub(crate) fn read_caches(cpus: &[CpuTopology]) -> Vec<CacheLevel> {
    let mut caches: Vec<CacheLevel> = Vec::new();
    let mut seen_instances: HashSet<(u8, CacheKind, String)> = HashSet::new();
    for cpu in cpus.iter().filter(|cpu| cpu.online) {
        let Ok(entries) = std::fs::read_dir(format!("{CPU_SYSFS}/cpu{}/cache", cpu.cpu)) else {
            continue;
        };
        for entry in entries
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("index"))
        {
            let path = entry.path();
            let attribute = |name: &str| read_string(path.join(name)).ok();
            let Some(level) = attribute("level").and_then(|level| level.parse::<u8>().ok()) else {
                continue;
            };
            let kind = match attribute("type").as_deref() {
                Some("Data") => CacheKind::Data,
                Some("Instruction") => CacheKind::Instruction,
                _ => CacheKind::Unified,
            };
            let shared = attribute("shared_cpu_list").unwrap_or_default();
            if !seen_instances.insert((level, kind, shared)) {
                continue;
            }
            match caches
                .iter_mut()
                .find(|cache| cache.level == level && cache.kind == kind)
            {
                Some(cache) => cache.instances += 1,
                None => caches.push(CacheLevel {
                    level,
                    kind,
                    size: attribute("size")
                        .and_then(|size| parse_cache_size(&size))
                        .unwrap_or_default(),
                    instances: 1,
                    line_size: attribute("coherency_line_size").and_then(|size| size.parse().ok()),
                    ways: attribute("ways_of_associativity").and_then(|ways| ways.parse().ok()),
                }),
            }
        }
    }
    caches.sort_by_key(|cache| (cache.level, cache.kind));
    caches
}

/// Parses sysfs cache sizes such as `32K` or `16384K`.
fn parse_cache_size(text: &str) -> Option<Bytes> {
    let text = text.trim();
    let (digits, multiplier) = match text.as_bytes().last() {
        Some(b'K') => (&text[..text.len() - 1], 1024),
        Some(b'M') => (&text[..text.len() - 1], 1024 * 1024),
        _ => (text, 1),
    };
    parse_u64(digits.as_bytes()).map(|value| Bytes::new(value * multiplier))
}

pub(crate) fn count_packages_and_cores(cpus: &[CpuTopology]) -> (u32, u32) {
    let packages: HashSet<u32> = cpus.iter().map(|cpu| cpu.package).collect();
    let cores: HashSet<(u32, Option<u32>, u32)> = cpus
        .iter()
        .map(|cpu| (cpu.package, cpu.die, cpu.core))
        .collect();
    let count = |length: usize| u32::try_from(length).unwrap_or(u32::MAX);
    (count(packages.len()), count(cores.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cache_sizes() {
        assert_eq!(parse_cache_size("32K"), Some(Bytes::new(32 * 1024)));
        assert_eq!(
            parse_cache_size("16384K\n"),
            Some(Bytes::new(16 * 1024 * 1024))
        );
        assert_eq!(parse_cache_size("2M"), Some(Bytes::new(2 * 1024 * 1024)));
        assert_eq!(parse_cache_size("x"), None);
    }

    #[test]
    fn classifies_big_little_by_capacity() {
        let cpu = |cpu, capacity| CpuTopology {
            cpu,
            capacity: Some(capacity),
            ..CpuTopology::default()
        };
        let mut cpus = [cpu(0, 446), cpu(1, 446), cpu(2, 1024)];
        classify_by_capacity(&mut cpus);
        assert_eq!(cpus[0].core_type, CoreType::Efficient);
        assert_eq!(cpus[2].core_type, CoreType::Performance);
        let mut uniform = [cpu(0, 1024), cpu(1, 1024)];
        classify_by_capacity(&mut uniform);
        assert_eq!(uniform[0].core_type, CoreType::Uniform);
    }

    #[test]
    fn reads_live_topology() {
        let cpus = read_topology();
        assert!(!cpus.is_empty());
        let (packages, cores) = count_packages_and_cores(&cpus);
        assert!(packages >= 1 && cores >= 1 && cores as usize <= cpus.len());
        let caches = read_caches(&cpus);
        assert!(
            caches
                .iter()
                .any(|cache| cache.level == 1 && cache.kind == CacheKind::Data)
        );
    }
}
