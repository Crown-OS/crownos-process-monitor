//! Logical CPUs grouped by package, core type and physical core, the way the affinity dialog
//! lays them out.

use std::fmt::Write;

use monitor_engine::system::{CoreType, CpuInfo, CpuTopology};

/// One physical core: the positions of its logical CPUs in the source slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreCell {
    pub label: String,
    pub cpus: Vec<usize>,
}

/// Physical cores of one type ("Performance cores", "Efficient cores", "Cores").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreCluster {
    pub label: String,
    pub cells: Vec<CoreCell>,
}

/// The kind a core is shown as: the engine's type, or Arm capacity when the engine saw one
/// type but capacities differ.
fn shown_type(cpu: &CpuTopology, capacities_differ: bool, largest_capacity: u32) -> CoreType {
    match (cpu.core_type, cpu.capacity) {
        (CoreType::Uniform, Some(capacity)) if capacities_differ => {
            if capacity == largest_capacity {
                CoreType::Performance
            } else {
                CoreType::Efficient
            }
        }
        (core_type, _) => core_type,
    }
}

const fn type_label(core_type: CoreType) -> &'static str {
    match core_type {
        CoreType::Performance => "Performance cores",
        CoreType::Efficient => "Efficient cores",
        CoreType::Uniform => "Cores",
    }
}

fn ungrouped(logical: usize) -> Vec<CoreCluster> {
    vec![CoreCluster {
        label: type_label(CoreType::Uniform).to_owned(),
        cells: (0..logical)
            .map(|cpu| CoreCell {
                label: cpu.to_string(),
                cpus: vec![cpu],
            })
            .collect(),
    }]
}

/// Groups the `logical` CPUs the engine reports (online CPUs, ascending) by package and
/// core type, then by physical core so SMT siblings sit together. Falls back to one cell per
/// CPU when the topology does not match the usage list.
pub fn core_clusters(info: &CpuInfo, logical: usize) -> Vec<CoreCluster> {
    let mut online: Vec<&CpuTopology> = info.cpus.iter().filter(|cpu| cpu.online).collect();
    online.sort_by_key(|cpu| cpu.cpu);
    if online.len() != logical || logical == 0 {
        return ungrouped(logical);
    }
    let largest_capacity = online.iter().filter_map(|cpu| cpu.capacity).max();
    let capacities_differ = online
        .iter()
        .any(|cpu| cpu.capacity.is_some() && cpu.capacity != largest_capacity);
    let largest_capacity = largest_capacity.unwrap_or_default();
    let packages = online
        .iter()
        .map(|cpu| cpu.package)
        .max()
        .unwrap_or_default()
        > 0;

    let mut keyed: Vec<(u32, CoreType, u32, usize)> = online
        .iter()
        .enumerate()
        .map(|(position, cpu)| {
            let core_type = shown_type(cpu, capacities_differ, largest_capacity);
            (cpu.package, core_type, cpu.core, position)
        })
        .collect();
    keyed.sort_by_key(|(package, core_type, core, position)| {
        (*package, type_rank(*core_type), *core, *position)
    });

    let mut clusters: Vec<CoreCluster> = Vec::new();
    let mut current: Option<(u32, CoreType)> = None;
    let mut last_core = None;
    for (package, core_type, core, position) in keyed {
        if current != Some((package, core_type)) {
            current = Some((package, core_type));
            last_core = None;
            let label = if packages {
                format!("Socket {} · {}", package + 1, type_label(core_type))
            } else {
                type_label(core_type).to_owned()
            };
            clusters.push(CoreCluster {
                label,
                cells: Vec::new(),
            });
        }
        let Some(cluster) = clusters.last_mut() else {
            continue;
        };
        let cpu_number = online.get(position).map_or(0, |cpu| cpu.cpu);
        match cluster.cells.last_mut() {
            Some(cell) if last_core == Some(core) => {
                cell.cpus.push(position);
                let _ = write!(cell.label, " · {cpu_number}");
            }
            _ => cluster.cells.push(CoreCell {
                label: cpu_number.to_string(),
                cpus: vec![position],
            }),
        }
        last_core = Some(core);
    }
    clusters
}

const fn type_rank(core_type: CoreType) -> u8 {
    match core_type {
        CoreType::Performance => 0,
        CoreType::Uniform => 1,
        CoreType::Efficient => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cpu(number: u32, core: u32, core_type: CoreType) -> CpuTopology {
        CpuTopology {
            cpu: number,
            online: true,
            core,
            core_type,
            ..CpuTopology::default()
        }
    }

    fn labels(clusters: &[CoreCluster]) -> Vec<(&str, Vec<&str>)> {
        clusters
            .iter()
            .map(|cluster| {
                (
                    cluster.label.as_str(),
                    cluster
                        .cells
                        .iter()
                        .map(|cell| cell.label.as_str())
                        .collect(),
                )
            })
            .collect()
    }

    #[test]
    fn smt_siblings_share_a_cell_on_uniform_cpus() {
        let info = CpuInfo {
            cpus: (0..8).map(|n| cpu(n, n % 4, CoreType::Uniform)).collect(),
            ..CpuInfo::default()
        };
        let clusters = core_clusters(&info, 8);
        assert_eq!(
            labels(&clusters),
            [("Cores", vec!["0 · 4", "1 · 5", "2 · 6", "3 · 7"])]
        );
        assert_eq!(clusters[0].cells[1].cpus, [1, 5]);
    }

    #[test]
    fn hybrid_parts_split_by_core_type() {
        let mut cpus: Vec<_> = (0..4)
            .map(|n| cpu(n, n / 2, CoreType::Performance))
            .collect();
        cpus.extend((4..8).map(|n| cpu(n, n, CoreType::Efficient)));
        let info = CpuInfo {
            hybrid: true,
            cpus,
            ..CpuInfo::default()
        };
        assert_eq!(
            labels(&core_clusters(&info, 8)),
            [
                ("Performance cores", vec!["0 · 1", "2 · 3"]),
                ("Efficient cores", vec!["4", "5", "6", "7"]),
            ]
        );
    }

    #[test]
    fn arm_capacities_split_big_and_little() {
        let cpus = (0..4)
            .map(|n| CpuTopology {
                capacity: Some(if n < 2 { 1024 } else { 446 }),
                ..cpu(n, n, CoreType::Uniform)
            })
            .collect();
        let info = CpuInfo {
            cpus,
            ..CpuInfo::default()
        };
        let clusters = core_clusters(&info, 4);
        assert_eq!(clusters[0].label, "Performance cores");
        assert_eq!(clusters[1].label, "Efficient cores");
    }

    #[test]
    fn a_mismatched_topology_falls_back_to_one_cell_per_cpu() {
        let clusters = core_clusters(&CpuInfo::default(), 3);
        assert_eq!(labels(&clusters), [("Cores", vec!["0", "1", "2"])]);
        assert!(core_clusters(&CpuInfo::default(), 0)[0].cells.is_empty());
    }
}
