use std::sync::Arc;

use monitor_core::util::sysfs::PersistentFile;

use super::CPU_SYSFS;

/// Frequency scaling policy of CPU 0, which all CPUs share in practice.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CpuPolicy {
    pub driver: Option<Arc<str>>,
    pub governor: Option<Arc<str>>,
    /// `energy_performance_preference` (EPP) on amd-pstate and intel_pstate.
    pub energy_preference: Option<Arc<str>>,
    pub boost: Option<bool>,
}

enum BoostSource {
    Enabled(PersistentFile),
    NoTurbo(PersistentFile),
}

/// Policy attributes kept open; re-read on the slow cadence and replaced
/// only when their text changes.
pub(crate) struct PolicyReader {
    driver: Option<PersistentFile>,
    governor: Option<PersistentFile>,
    energy_preference: Option<PersistentFile>,
    boost: Option<BoostSource>,
    buffer: Vec<u8>,
}

impl PolicyReader {
    pub fn open() -> Self {
        let policy =
            |name: &str| PersistentFile::open(format!("{CPU_SYSFS}/cpu0/cpufreq/{name}")).ok();
        let boost = policy("boost")
            .or_else(|| PersistentFile::open(format!("{CPU_SYSFS}/cpufreq/boost")).ok())
            .map(BoostSource::Enabled)
            .or_else(|| {
                PersistentFile::open(format!("{CPU_SYSFS}/intel_pstate/no_turbo"))
                    .ok()
                    .map(BoostSource::NoTurbo)
            });
        Self {
            driver: policy("scaling_driver"),
            governor: policy("scaling_governor"),
            energy_preference: policy("energy_performance_preference"),
            boost,
            buffer: Vec::new(),
        }
    }

    pub fn refresh(&mut self, policy: &mut CpuPolicy) {
        refresh_text(self.driver.as_ref(), &mut self.buffer, &mut policy.driver);
        refresh_text(
            self.governor.as_ref(),
            &mut self.buffer,
            &mut policy.governor,
        );
        refresh_text(
            self.energy_preference.as_ref(),
            &mut self.buffer,
            &mut policy.energy_preference,
        );
        policy.boost = match &self.boost {
            Some(BoostSource::Enabled(file)) => file.read_u64().ok().map(|value| value != 0),
            Some(BoostSource::NoTurbo(file)) => file.read_u64().ok().map(|value| value == 0),
            None => None,
        };
    }
}

fn refresh_text(file: Option<&PersistentFile>, buffer: &mut Vec<u8>, slot: &mut Option<Arc<str>>) {
    let Some(text) = file
        .and_then(|file| file.read(buffer).ok())
        .and_then(|bytes| std::str::from_utf8(bytes.trim_ascii()).ok())
    else {
        *slot = None;
        return;
    };
    if slot.as_deref() != Some(text) {
        *slot = Some(Arc::from(text));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_live_policy_without_failing() {
        let mut reader = PolicyReader::open();
        let mut policy = CpuPolicy::default();
        reader.refresh(&mut policy);
        let first = policy.governor.clone();
        reader.refresh(&mut policy);
        assert_eq!(first, policy.governor);
        if let (Some(first), Some(second)) = (&first, &policy.governor) {
            assert!(
                Arc::ptr_eq(first, second),
                "unchanged text is not reallocated"
            );
        }
    }
}
