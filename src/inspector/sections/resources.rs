use std::time::Duration;

use crownui::prelude::AnyView;
use monitor_engine::system::inspect::ProcessDetails;

use crate::inspector::fields::{Fields, field_section};
use crate::util::format::{bytes, count, percent};

fn seconds(time: Duration) -> String {
    format!("{:.1} s", time.as_secs_f64())
}

/// "412 MB · 64% of its 640 MB peak".
fn memory_now_and_peak(resident: u64, peak: Option<u64>) -> String {
    match peak.filter(|peak| *peak > 0) {
        Some(peak) => format!(
            "{} · {} of its {} peak",
            bytes(resident),
            percent((resident as f64 / peak as f64 * 100.0) as f32),
            bytes(peak)
        ),
        None => bytes(resident),
    }
}

pub fn view(details: &ProcessDetails) -> Option<AnyView> {
    let resources = &details.resources;
    let mut fields = Fields::default();
    fields
        .add(
            "CPU time",
            format!(
                "user {} · system {}",
                seconds(resources.user_time),
                seconds(resources.system_time)
            ),
        )
        .add(
            "Children CPU",
            format!(
                "user {} · system {}",
                seconds(resources.children_user_time),
                seconds(resources.children_system_time)
            ),
        )
        .add(
            "Resident",
            memory_now_and_peak(
                resources.resident.get(),
                resources.peak_resident.map(|peak| peak.get()),
            ),
        )
        .add_some(
            "Proportional",
            resources.proportional.map(|value| bytes(value.get())),
        )
        .add_some("Private", resources.unique.map(|value| bytes(value.get())))
        .add_some("Swapped", resources.swap.map(|value| bytes(value.get())))
        .add("Threads", count(u64::from(resources.threads)))
        .add_some(
            "Open files",
            resources.open_files.map(|files| count(u64::from(files))),
        )
        .add(
            "Page faults",
            format!(
                "{} major · {} minor",
                count(resources.major_faults),
                count(resources.minor_faults)
            ),
        )
        .add_some(
            "Disk I/O",
            resources.io.map(|io| {
                format!(
                    "{} read · {} written",
                    bytes(io.read_bytes),
                    bytes(io.write_bytes)
                )
            }),
        )
        .add_some(
            "Context switches",
            resources.voluntary_switches.map(|voluntary| {
                format!(
                    "{} voluntary · {} forced",
                    count(voluntary),
                    count(resources.involuntary_switches.unwrap_or(0))
                )
            }),
        )
        .add(
            "Priority",
            format!(
                "nice {} · priority {} · {}",
                resources.nice,
                resources.priority,
                resources.policy.name()
            ),
        )
        .add_some("CPU affinity", resources.cpus_allowed.clone())
        .add_some(
            "OOM score",
            resources.oom_score.map(|score| {
                format!(
                    "{score} (adjust {})",
                    resources.oom_score_adjust.unwrap_or(0)
                )
            }),
        );
    field_section("Resource usage", fields)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_reads_against_its_peak() {
        let mebibyte = 1024 * 1024;
        assert_eq!(
            memory_now_and_peak(64 * mebibyte, Some(128 * mebibyte)),
            "64.00 MB · 50% of its 128 MB peak"
        );
        assert_eq!(memory_now_and_peak(mebibyte, None), "1.00 MB");
    }
}
