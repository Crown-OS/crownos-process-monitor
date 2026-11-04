use monitor_core::util::parse::{parse_hex_u32, parse_u32};
use monitor_core::util::sysfs::{read_string, read_u64};
use monitor_core::{Megahertz, SourceError, SourceResult};

use super::CPU_SYSFS;
use super::features::highlights_from_flags;
use super::topology::{
    CacheLevel, CpuTopology, count_packages_and_cores, read_caches, read_topology,
};
use super::vulnerabilities::{Vulnerability, read_vulnerabilities};

const CPUINFO: &str = "/proc/cpuinfo";
const AMD_FAMILY_ZEN: u32 = 0x17;
const AMD_FAMILY_ZEN3_ZEN4: u32 = 0x19;
const AMD_FAMILY_ZEN5: u32 = 0x1a;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SmtState {
    On,
    Off,
    ForceOff,
    NotSupported,
    NotImplemented,
    #[default]
    Unknown,
}

impl SmtState {
    fn parse(control: &str) -> Self {
        match control {
            "on" => Self::On,
            "off" => Self::Off,
            "forceoff" => Self::ForceOff,
            "notsupported" => Self::NotSupported,
            "notimplemented" => Self::NotImplemented,
            _ => Self::Unknown,
        }
    }
}

/// Static processor description for the hardware panel.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CpuInfo {
    pub vendor: String,
    pub model_name: String,
    pub microarchitecture: Option<&'static str>,
    pub family: u32,
    pub model: u32,
    pub stepping: u32,
    pub microcode: Option<String>,
    pub sockets: u32,
    pub cores: u32,
    pub threads: u32,
    pub smt: SmtState,
    /// Intel hybrid (P/E cores) or Arm big.LITTLE.
    pub hybrid: bool,
    pub cpus: Vec<CpuTopology>,
    pub caches: Vec<CacheLevel>,
    pub base_frequency: Option<Megahertz>,
    pub min_frequency: Option<Megahertz>,
    pub max_frequency: Option<Megahertz>,
    pub features: Vec<&'static str>,
    pub virtualization: Option<&'static str>,
    pub hypervisor: bool,
    pub vulnerabilities: Vec<Vulnerability>,
}

#[derive(Debug, Default, PartialEq)]
struct CpuinfoFields {
    vendor: Option<String>,
    model_name: Option<String>,
    family: Option<u32>,
    model: Option<u32>,
    stepping: Option<u32>,
    microcode: Option<String>,
    flags: String,
    arm_implementer: Option<u32>,
    arm_part: Option<u32>,
}

/// Reads the first processor block of `/proc/cpuinfo`.
fn parse_cpuinfo(text: &str) -> CpuinfoFields {
    let mut fields = CpuinfoFields::default();
    let first_block = text.split("\n\n").next().unwrap_or_default();
    let flags_line = text.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        matches!(key.trim(), "flags" | "Features").then(|| value.trim().to_owned())
    });
    fields.flags = flags_line.unwrap_or_default();
    for line in first_block.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        let number = || parse_u32(value.as_bytes());
        match key.trim() {
            "vendor_id" => fields.vendor = Some(value.to_owned()),
            "model name" => fields.model_name = Some(value.to_owned()),
            "cpu family" => fields.family = number(),
            "model" => fields.model = number(),
            "stepping" => fields.stepping = number(),
            "microcode" => fields.microcode = Some(value.to_owned()),
            "CPU implementer" => fields.arm_implementer = parse_hex_u32(value.as_bytes()),
            "CPU part" => fields.arm_part = parse_hex_u32(value.as_bytes()),
            "CPU variant" => fields.model = parse_hex_u32(value.as_bytes()),
            "CPU revision" => fields.stepping = number(),
            _ => {}
        }
    }
    fields
}

fn arm_vendor(implementer: u32) -> &'static str {
    match implementer {
        0x41 => "ARM",
        0x42 => "Broadcom",
        0x48 => "HiSilicon",
        0x4e => "NVIDIA",
        0x51 => "Qualcomm",
        0x53 => "Samsung",
        0x61 => "Apple",
        0x6d => "Microsoft",
        0xc0 => "Ampere",
        _ => "Arm",
    }
}

fn amd_microarchitecture(family: u32, model: u32) -> Option<&'static str> {
    match (family, model) {
        (AMD_FAMILY_ZEN, 0x00..=0x2f) => Some("Zen / Zen+"),
        (AMD_FAMILY_ZEN, _) => Some("Zen 2"),
        (AMD_FAMILY_ZEN3_ZEN4, 0x10..=0x1f | 0x60..=0x7f | 0xa0..=0xaf) => Some("Zen 4"),
        (AMD_FAMILY_ZEN3_ZEN4, _) => Some("Zen 3"),
        (AMD_FAMILY_ZEN5, _) => Some("Zen 5"),
        _ => None,
    }
}

fn smt_state() -> SmtState {
    read_string(format!("{CPU_SYSFS}/smt/control"))
        .map_or(SmtState::Unknown, |control| SmtState::parse(&control))
}

fn base_frequency() -> Option<Megahertz> {
    read_u64(format!("{CPU_SYSFS}/cpu0/cpufreq/base_frequency"))
        .map(Megahertz::from_khz)
        .or_else(|_| {
            read_u64(format!("{CPU_SYSFS}/cpu0/acpi_cppc/nominal_freq"))
                .map(|mhz| Megahertz::new(u32::try_from(mhz).unwrap_or(u32::MAX)))
        })
        .ok()
        .filter(|frequency| frequency.get() > 0)
}

struct ArchitectureFeatures {
    highlights: Vec<&'static str>,
    virtualization: Option<&'static str>,
    hypervisor: bool,
    hybrid: bool,
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn architecture_features(_flags: &str) -> ArchitectureFeatures {
    let detected = super::features::x86::detect();
    ArchitectureFeatures {
        highlights: detected.highlights,
        virtualization: detected.virtualization,
        hypervisor: detected.hypervisor,
        hybrid: detected.hybrid,
    }
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
fn architecture_features(flags: &str) -> ArchitectureFeatures {
    ArchitectureFeatures {
        highlights: highlights_from_flags(flags),
        virtualization: None,
        hypervisor: flags.split_whitespace().any(|flag| flag == "hypervisor"),
        hybrid: false,
    }
}

/// Reads the processor description once. Allocates; call it off the hot path.
pub fn cpu_info() -> SourceResult<CpuInfo> {
    let text = std::fs::read_to_string(CPUINFO).map_err(|error| SourceError::io(CPUINFO, error))?;
    let fields = parse_cpuinfo(&text);
    let cpus = read_topology();
    let caches = read_caches(&cpus);
    let (sockets, cores) = count_packages_and_cores(&cpus);
    let mut features = architecture_features(&fields.flags);
    if features.highlights.is_empty() {
        features.highlights = highlights_from_flags(&fields.flags);
    }
    let core_types_differ = cpus
        .windows(2)
        .any(|pair| pair[0].core_type != pair[1].core_type);
    let vendor = match (&fields.vendor, fields.arm_implementer) {
        (Some(vendor), _) => vendor.clone(),
        (None, Some(implementer)) => arm_vendor(implementer).to_owned(),
        (None, None) => String::new(),
    };
    let family = fields.family.unwrap_or(0);
    let model = fields.model.unwrap_or(0);
    let model_name = fields
        .model_name
        .clone()
        .unwrap_or_else(|| match fields.arm_part {
            Some(part) => format!("{vendor} part {part:#x}"),
            None => vendor.clone(),
        });
    Ok(CpuInfo {
        microarchitecture: (vendor == "AuthenticAMD")
            .then(|| amd_microarchitecture(family, model))
            .flatten(),
        vendor,
        model_name,
        family,
        model,
        stepping: fields.stepping.unwrap_or(0),
        microcode: fields.microcode,
        sockets,
        cores,
        threads: u32::try_from(cpus.len()).unwrap_or(u32::MAX),
        smt: smt_state(),
        hybrid: features.hybrid || core_types_differ,
        base_frequency: base_frequency(),
        min_frequency: read_u64(format!("{CPU_SYSFS}/cpu0/cpufreq/cpuinfo_min_freq"))
            .ok()
            .map(Megahertz::from_khz),
        max_frequency: cpus.iter().filter_map(|cpu| cpu.max_frequency).max(),
        caches,
        cpus,
        features: features.highlights,
        virtualization: features.virtualization,
        hypervisor: features.hypervisor,
        vulnerabilities: read_vulnerabilities(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const CPUINFO_X86: &str = "processor\t: 0\n\
vendor_id\t: AuthenticAMD\n\
cpu family\t: 25\n\
model\t\t: 116\n\
model name\t: AMD Ryzen 7 7840HS w/ Radeon 780M Graphics\n\
stepping\t: 1\n\
microcode\t: 0xa70410a\n\
cpu MHz\t\t: 4731.617\n\
flags\t\t: fpu vme sse4_2 avx avx2 aes avx512f sha_ni svm\n\
\n\
processor\t: 1\n\
vendor_id\t: AuthenticAMD\n";

    const CPUINFO_ARM: &str = "processor\t: 0\n\
BogoMIPS\t: 48.00\n\
Features\t: fp asimd evtstrm aes pmull sha1 sha2 crc32 atomics\n\
CPU implementer\t: 0x41\n\
CPU architecture: 8\n\
CPU variant\t: 0x4\n\
CPU part\t: 0xd0b\n\
CPU revision\t: 0\n";

    #[test]
    fn parses_x86_cpuinfo() {
        let fields = parse_cpuinfo(CPUINFO_X86);
        assert_eq!(fields.vendor.as_deref(), Some("AuthenticAMD"));
        assert_eq!(
            fields.model_name.as_deref(),
            Some("AMD Ryzen 7 7840HS w/ Radeon 780M Graphics")
        );
        assert_eq!(
            (fields.family, fields.model, fields.stepping),
            (Some(25), Some(116), Some(1))
        );
        assert_eq!(fields.microcode.as_deref(), Some("0xa70410a"));
        assert!(fields.flags.contains("avx512f"));
        assert_eq!(amd_microarchitecture(25, 116), Some("Zen 4"));
    }

    #[test]
    fn parses_arm_cpuinfo() {
        let fields = parse_cpuinfo(CPUINFO_ARM);
        assert_eq!(fields.arm_implementer, Some(0x41));
        assert_eq!(fields.arm_part, Some(0xd0b));
        assert_eq!(arm_vendor(0x41), "ARM");
        assert!(highlights_from_flags(&fields.flags).contains(&"NEON"));
    }

    #[test]
    fn reads_live_cpu_info() -> SourceResult<()> {
        let info = cpu_info()?;
        assert!(!info.model_name.is_empty());
        assert!(info.threads >= info.cores && info.cores >= 1);
        assert!(info.sockets >= 1);
        assert!(!info.caches.is_empty());
        Ok(())
    }
}
