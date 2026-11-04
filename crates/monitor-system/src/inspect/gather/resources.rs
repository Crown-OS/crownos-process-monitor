use monitor_core::util::parse::{find_value, kib_value, parse_u64};
use monitor_core::util::procfs::ProcRoot;
use monitor_core::util::sysfs::read_at;
use monitor_core::{Bytes, Pid, ProcessKey};

use super::procfile::{ReadFailure, Restrictions, process_file, read_text};
use crate::inspect::details::{ChildProcess, IoTotals, Resources, SchedulingPolicy};
use crate::process::{IoCounters, SmapsRollup, StatFields, parse_stat};
use crate::util::clock;
use crate::util::pid_path::PidPath;

pub(super) fn describe_resources(pid: Pid, stat: &StatFields, status: &str) -> Resources {
    let ticks_per_second = clock::clock_ticks_per_second();
    let duration = |ticks: u64| clock::ticks_to_duration(ticks, ticks_per_second);
    let value = |field: &str| find_value(status.as_bytes(), field.as_bytes());
    let bytes = |field: &str| value(field).and_then(kib_value).map(Bytes::new);
    let mut restrictions = Restrictions::default();
    let rollup = restrictions
        .note(read_text(pid, "smaps_rollup"))
        .map(|text| SmapsRollup::parse(text.as_bytes()));
    let io = restrictions.note(read_text(pid, "io")).map(|text| {
        let counters = IoCounters::parse(text.as_bytes());
        IoTotals {
            read_bytes: counters.read_bytes,
            write_bytes: counters.write_bytes,
            read_chars: counters.read_chars,
            write_chars: counters.write_chars,
        }
    });
    let open_files = restrictions
        .note(std::fs::read_dir(process_file(pid, "fd")).map_err(ReadFailure::from))
        .map(|entries| u32::try_from(entries.count()).unwrap_or(u32::MAX));
    let oom = |name: &str| {
        read_text(pid, name)
            .ok()
            .and_then(|text| text.trim().parse().ok())
    };
    Resources {
        user_time: duration(stat.user_ticks),
        system_time: duration(stat.system_ticks),
        children_user_time: duration(stat.children_user_ticks),
        children_system_time: duration(stat.children_system_ticks),
        resident: bytes("VmRSS").unwrap_or_default(),
        proportional: rollup.map(|rollup| rollup.proportional),
        unique: rollup.map(|rollup| rollup.unique),
        swap: rollup.map(|rollup| rollup.swap).or_else(|| bytes("VmSwap")),
        peak_resident: bytes("VmHWM"),
        threads: stat.threads,
        open_files,
        minor_faults: stat.minor_faults,
        major_faults: stat.major_faults,
        io,
        voluntary_switches: value("voluntary_ctxt_switches").and_then(parse_u64),
        involuntary_switches: value("nonvoluntary_ctxt_switches").and_then(parse_u64),
        nice: i32::try_from(stat.nice).unwrap_or(0),
        priority: i32::try_from(stat.priority).unwrap_or(0),
        policy: SchedulingPolicy::from_raw(stat.policy),
        cpus_allowed: value("Cpus_allowed_list")
            .and_then(|list| std::str::from_utf8(list).ok())
            .map(str::to_owned),
        oom_score: oom("oom_score"),
        oom_score_adjust: oom("oom_score_adj"),
        coverage: restrictions.coverage(),
    }
}

pub(super) fn read_environment(pid: Pid) -> Option<Vec<(String, String)>> {
    let raw = std::fs::read(process_file(pid, "environ")).ok()?;
    Some(
        crate::util::text::arguments(&raw)
            .filter_map(|pair| {
                let text = String::from_utf8_lossy(pair);
                let (name, value) = text.split_once('=')?;
                Some((name.to_owned(), value.to_owned()))
            })
            .collect(),
    )
}

pub(super) fn children_of(pid: Pid) -> Vec<ChildProcess> {
    let Ok(mut root) = ProcRoot::open() else {
        return Vec::new();
    };
    let mut pids = Vec::new();
    if root.list_pids(&mut pids).is_err() {
        return Vec::new();
    }
    let mut buffer = Vec::new();
    pids.into_iter()
        .filter_map(|child| {
            let path = PidPath::new(child, "stat");
            let (comm, stat) = parse_stat(read_at(root.as_fd(), path.as_str(), &mut buffer).ok()?)?;
            (stat.parent == pid.get()).then(|| ChildProcess {
                key: ProcessKey {
                    pid: child,
                    start_ticks: stat.start_ticks,
                },
                name: String::from_utf8_lossy(comm).into_owned(),
            })
        })
        .collect()
}
