use super::CPU_SYSFS;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VulnerabilityStatus {
    NotAffected,
    Mitigated,
    Vulnerable,
    #[default]
    Unknown,
}

impl VulnerabilityStatus {
    pub fn classify(text: &str) -> Self {
        if text.starts_with("Not affected") {
            Self::NotAffected
        } else if text.starts_with("Mitigation") {
            Self::Mitigated
        } else if text.starts_with("Vulnerable") {
            Self::Vulnerable
        } else {
            Self::Unknown
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Vulnerability {
    pub name: String,
    pub status: VulnerabilityStatus,
    /// The kernel's own wording, e.g. `Mitigation: Safe RET`.
    pub detail: String,
}

pub(crate) fn read_vulnerabilities() -> Vec<Vulnerability> {
    let Ok(entries) = std::fs::read_dir(format!("{CPU_SYSFS}/vulnerabilities")) else {
        return Vec::new();
    };
    let mut vulnerabilities: Vec<Vulnerability> = entries
        .flatten()
        .filter_map(|entry| {
            let detail = std::fs::read_to_string(entry.path())
                .ok()?
                .trim()
                .to_owned();
            Some(Vulnerability {
                name: entry.file_name().to_string_lossy().into_owned(),
                status: VulnerabilityStatus::classify(&detail),
                detail,
            })
        })
        .collect();
    vulnerabilities.sort_by(|left, right| left.name.cmp(&right.name));
    vulnerabilities
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_kernel_wording() {
        assert_eq!(
            VulnerabilityStatus::classify("Not affected"),
            VulnerabilityStatus::NotAffected
        );
        assert_eq!(
            VulnerabilityStatus::classify(
                "Mitigation: usercopy/swapgs barriers and __user pointer sanitization"
            ),
            VulnerabilityStatus::Mitigated
        );
        assert_eq!(
            VulnerabilityStatus::classify("Vulnerable: Clear CPU buffers attempted, no microcode"),
            VulnerabilityStatus::Vulnerable
        );
        assert_eq!(
            VulnerabilityStatus::classify("Unknown: Dependent on hypervisor status"),
            VulnerabilityStatus::Unknown
        );
    }
}
