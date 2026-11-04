use std::collections::HashSet;
use std::fmt::Write;
use std::sync::Arc;

use crownui::kit::icons;
use crownui::prelude::{AnyView, Cx, View, keyed};
use monitor_engine::system::{
    CacheKind, CacheLevel, CoreType, CpuInfo, SmtState, Vulnerability, VulnerabilityStatus,
};

use crate::app::AppState;
use crate::components::empty_state::{NOT_AVAILABLE, empty_state};
use crate::components::fact_grid::{Fact, fact_grid};
use crate::util::format::{bytes, frequency};

const NOT_REPORTED: &str = "–";

const fn smt_label(smt: SmtState) -> &'static str {
    match smt {
        SmtState::On => "On",
        SmtState::Off => "Off",
        SmtState::ForceOff => "Forced off",
        SmtState::NotSupported => "Not supported",
        SmtState::NotImplemented => "Not implemented",
        SmtState::Unknown => NOT_REPORTED,
    }
}

const fn cache_label(cache: &CacheLevel) -> &'static str {
    match (cache.level, cache.kind) {
        (1, CacheKind::Data) => "L1d",
        (1, CacheKind::Instruction) => "L1i",
        (1, CacheKind::Unified) => "L1",
        (2, _) => "L2",
        (3, _) => "L3",
        _ => "L4",
    }
}

/// "L1d 32 kB × 8 · L1i 32 kB × 8 · L2 1 MB × 8 · L3 16 MB".
pub fn cache_summary(caches: &[CacheLevel]) -> String {
    let mut out = String::new();
    for cache in caches {
        if !out.is_empty() {
            out.push_str(" · ");
        }
        let _ = write!(out, "{} {}", cache_label(cache), bytes(cache.size.get()));
        if cache.instances > 1 {
            let _ = write!(out, " × {}", cache.instances);
        }
    }
    out
}

/// "6 P + 8 E cores" on hybrid parts, "8 cores" otherwise; counts physical cores.
pub fn layout_summary(info: &CpuInfo) -> String {
    let physical = |wanted: CoreType| {
        info.cpus
            .iter()
            .filter(|cpu| cpu.core_type == wanted)
            .map(|cpu| (cpu.package, cpu.core))
            .collect::<HashSet<_>>()
            .len()
    };
    let (performance, efficient) = (
        physical(CoreType::Performance),
        physical(CoreType::Efficient),
    );
    if performance > 0 && efficient > 0 {
        format!("{performance} P + {efficient} E cores")
    } else {
        format!("{} cores, one type", info.cores)
    }
}

/// "2 vulnerable (spectre_v1, retbleed) · 9 mitigated · 14 not affected".
pub fn vulnerability_summary(vulnerabilities: &[Vulnerability]) -> String {
    if vulnerabilities.is_empty() {
        return NOT_REPORTED.to_owned();
    }
    let counted = |status| {
        vulnerabilities
            .iter()
            .filter(move |vulnerability| vulnerability.status == status)
    };
    let mut out = String::new();
    let vulnerable: Vec<_> = counted(VulnerabilityStatus::Vulnerable)
        .map(|vulnerability| vulnerability.name.as_str())
        .collect();
    if !vulnerable.is_empty() {
        let _ = write!(
            out,
            "{} vulnerable ({}) · ",
            vulnerable.len(),
            vulnerable.join(", ")
        );
    }
    let _ = write!(
        out,
        "{} mitigated · {} not affected",
        counted(VulnerabilityStatus::Mitigated).count(),
        counted(VulnerabilityStatus::NotAffected).count()
    );
    out
}

fn optional_megahertz(value: Option<monitor_core::Megahertz>) -> String {
    frequency(value.map_or(f32::NAN, |mhz| mhz.get() as f32))
}

fn facts(info: &CpuInfo) -> Vec<Fact> {
    let virtualization = match (info.virtualization, info.hypervisor) {
        (Some(name), true) => format!("{name} · running in a virtual machine"),
        (Some(name), false) => name.to_owned(),
        (None, true) => "Running in a virtual machine".to_owned(),
        (None, false) => "None".to_owned(),
    };
    vec![
        Fact::new("Vendor", info.vendor.clone()),
        Fact::new("Model", info.model_name.clone()),
        Fact::new(
            "Microarchitecture",
            info.microarchitecture.unwrap_or(NOT_REPORTED).to_owned(),
        ),
        Fact::new(
            "Family · model · stepping",
            format!("{:#x} · {:#x} · {}", info.family, info.model, info.stepping),
        ),
        Fact::new(
            "Sockets · cores · threads",
            format!("{} · {} · {}", info.sockets, info.cores, info.threads),
        ),
        Fact::new("SMT", smt_label(info.smt).to_owned()),
        Fact::new("Layout", layout_summary(info)),
        Fact::new("Caches", cache_summary(&info.caches)),
        Fact::new(
            "Base · max clock",
            format!(
                "{} · {}",
                optional_megahertz(info.base_frequency),
                optional_megahertz(info.max_frequency)
            ),
        ),
        Fact::new("Instruction sets", info.features.join(", ")),
        Fact::new("Virtualization", virtualization),
        Fact::new(
            "Vulnerabilities",
            vulnerability_summary(&info.vulnerabilities),
        ),
        Fact::new(
            "Microcode",
            info.microcode
                .clone()
                .unwrap_or_else(|| NOT_REPORTED.to_owned()),
        ),
    ]
}

/// The processor description, rebuilt only when the engine's `CpuInfo` changes.
pub fn hardware_panel(_: &mut Cx, state: AppState) -> impl View + use<> {
    let live = state.live;
    keyed(
        move |runtime| {
            live.with(runtime, |snapshot| {
                Arc::as_ptr(&snapshot.tick.cpu_info) as usize
            })
        },
        move |cx, _| -> AnyView {
            let info = live.with_untracked(cx, |snapshot| Arc::clone(&snapshot.tick.cpu_info));
            if info.model_name.is_empty() {
                return empty_state(
                    icons::CPU,
                    NOT_AVAILABLE,
                    "The processor did not describe itself in /proc/cpuinfo.",
                )
                .boxed();
            }
            fact_grid(facts(&info)).boxed()
        },
    )
}

#[cfg(test)]
mod tests {
    use monitor_core::Bytes;
    use monitor_engine::system::CpuTopology;

    use super::*;

    const KIB: u64 = 1024;

    #[test]
    fn caches_list_size_and_instances() {
        let caches = [
            CacheLevel {
                level: 1,
                kind: CacheKind::Data,
                size: Bytes::new(32 * KIB),
                instances: 8,
                ..CacheLevel::default()
            },
            CacheLevel {
                level: 3,
                kind: CacheKind::Unified,
                size: Bytes::new(16 * KIB * KIB),
                instances: 1,
                ..CacheLevel::default()
            },
        ];
        assert_eq!(cache_summary(&caches), "L1d 32 kB × 8 · L3 16.00 MB");
    }

    #[test]
    fn hybrid_layouts_count_physical_cores() {
        let cpu = |cpu, core, core_type| CpuTopology {
            cpu,
            core,
            core_type,
            ..CpuTopology::default()
        };
        let info = CpuInfo {
            cores: 3,
            cpus: vec![
                cpu(0, 0, CoreType::Performance),
                cpu(1, 0, CoreType::Performance),
                cpu(2, 8, CoreType::Efficient),
                cpu(3, 9, CoreType::Efficient),
            ],
            ..CpuInfo::default()
        };
        assert_eq!(layout_summary(&info), "1 P + 2 E cores");
        assert_eq!(
            layout_summary(&CpuInfo {
                cores: 8,
                ..CpuInfo::default()
            }),
            "8 cores, one type"
        );
    }

    #[test]
    fn vulnerabilities_name_what_is_exposed() {
        let entry = |name: &str, status| Vulnerability {
            name: name.to_owned(),
            status,
            detail: String::new(),
        };
        let summary = vulnerability_summary(&[
            entry("retbleed", VulnerabilityStatus::Vulnerable),
            entry("spectre_v2", VulnerabilityStatus::Mitigated),
            entry("meltdown", VulnerabilityStatus::NotAffected),
        ]);
        assert_eq!(
            summary,
            "1 vulnerable (retbleed) · 1 mitigated · 1 not affected"
        );
    }
}
