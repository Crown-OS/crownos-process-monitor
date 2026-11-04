//! Per-cgroup network bytes from two `cgroup_skb` programs
//! (`cgroup_traffic.bpf.c`, compiled by `build.rs`) attached to the cgroup
//! v2 root on first use and detached when the helper exits.

mod cgroup_paths;

use std::fs::File;

use aya::Ebpf;
use aya::maps::PerCpuHashMap;
use aya::programs::{CgroupAttachMode, CgroupSkb, CgroupSkbAttachType};
use cgroup_paths::CgroupPaths;

use crate::{CgroupTraffic, HelperError};

const CGROUP_ROOT: &str = "/sys/fs/cgroup";
const TRAFFIC_MAP: &str = "CGROUP_TRAFFIC";
const PROGRAMS: [(&str, CgroupSkbAttachType); 2] = [
    ("count_ingress", CgroupSkbAttachType::Ingress),
    ("count_egress", CgroupSkbAttachType::Egress),
];

/// `struct traffic` of the program: received, then transmitted bytes.
type Traffic = [u64; 2];

fn program_object() -> &'static [u8] {
    aya::include_bytes_aligned!(concat!(env!("OUT_DIR"), "/cgroup_traffic.bpf.o"))
}

enum Programs {
    NotLoaded,
    Attached(Box<Ebpf>),
    Failed(String),
}

pub(crate) struct CgroupTrafficMeter {
    programs: Programs,
    paths: CgroupPaths,
}

impl Default for CgroupTrafficMeter {
    fn default() -> Self {
        Self {
            programs: Programs::NotLoaded,
            paths: CgroupPaths::new(CGROUP_ROOT),
        }
    }
}

impl CgroupTrafficMeter {
    /// Bytes per cgroup since the programs were attached. Counters of
    /// cgroups that no longer exist are dropped.
    pub(crate) fn read(&mut self) -> Result<Vec<CgroupTraffic>, HelperError> {
        if matches!(self.programs, Programs::NotLoaded) {
            self.programs =
                attach().map_or_else(Programs::Failed, |ebpf| Programs::Attached(Box::new(ebpf)));
        }
        let ebpf = match &mut self.programs {
            Programs::Attached(ebpf) => ebpf,
            Programs::Failed(reason) => return Err(HelperError::NotSupported(reason.clone())),
            Programs::NotLoaded => return Err(HelperError::failed("eBPF programs not loaded")),
        };
        let map = ebpf
            .map_mut(TRAFFIC_MAP)
            .ok_or_else(|| HelperError::failed("the traffic map is missing"))?;
        let mut counters: PerCpuHashMap<_, u64, Traffic> =
            PerCpuHashMap::try_from(map).map_err(HelperError::failed)?;
        let mut traffic = Vec::new();
        let mut gone = Vec::new();
        self.paths.begin();
        for entry in counters.iter() {
            let (cgroup_id, per_cpu) = entry.map_err(HelperError::failed)?;
            let [received_bytes, transmitted_bytes] = per_cpu.iter().fold([0, 0], sum);
            let path = self.paths.path_of(cgroup_id).map(str::to_owned);
            if path.is_none() && self.paths.is_fresh() {
                gone.push(cgroup_id);
                continue;
            }
            traffic.push(CgroupTraffic {
                cgroup_id,
                cgroup_path: path.unwrap_or_default(),
                received_bytes,
                transmitted_bytes,
            });
        }
        for cgroup_id in gone {
            let _ = counters.remove(&cgroup_id);
        }
        Ok(traffic)
    }
}

fn sum(total: Traffic, per_cpu: &Traffic) -> Traffic {
    [
        total[0].wrapping_add(per_cpu[0]),
        total[1].wrapping_add(per_cpu[1]),
    ]
}

fn attach() -> Result<Ebpf, String> {
    let mut ebpf = Ebpf::load(program_object())
        .map_err(|error| format!("cannot load the eBPF programs: {error}"))?;
    let cgroup_root =
        File::open(CGROUP_ROOT).map_err(|error| format!("cannot open {CGROUP_ROOT}: {error}"))?;
    for (name, attach_type) in PROGRAMS {
        let program: &mut CgroupSkb = ebpf
            .program_mut(name)
            .ok_or_else(|| format!("program {name} is missing"))?
            .try_into()
            .map_err(|error| format!("program {name}: {error}"))?;
        program
            .load()
            .map_err(|error| format!("the kernel rejected {name}: {error}"))?;
        program
            .attach(&cgroup_root, attach_type, CgroupAttachMode::AllowMultiple)
            .map_err(|error| format!("cannot attach {name}: {error}"))?;
    }
    Ok(ebpf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums_per_cpu_counters() {
        let per_cpu = [[10, 1], [5, 2], [0, 3]];
        assert_eq!(per_cpu.iter().fold([0, 0], sum), [15, 6]);
    }

    #[test]
    fn object_parses_and_loads_with_privileges() {
        match Ebpf::load(program_object()) {
            Ok(ebpf) => {
                assert!(ebpf.map(TRAFFIC_MAP).is_some());
                assert!(
                    PROGRAMS
                        .iter()
                        .all(|(name, _)| ebpf.program(name).is_some())
                );
            }
            Err(error) => assert!(
                !matches!(
                    error,
                    aya::EbpfError::ParseError(_) | aya::EbpfError::BtfRelocationError(_)
                ),
                "the object itself is broken: {error}"
            ),
        }
    }

    #[test]
    fn unprivileged_meter_reports_not_supported() {
        if rustix::process::geteuid().is_root() {
            return;
        }
        let mut meter = CgroupTrafficMeter::default();
        assert!(matches!(meter.read(), Err(HelperError::NotSupported(_))));
        assert!(matches!(meter.read(), Err(HelperError::NotSupported(_))));
    }
}
