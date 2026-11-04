use std::path::Path;

use monitor_core::util::sysfs::{self, PersistentFile};

use crate::EnergyZone;

const POWERCAP: &str = "/sys/class/powercap";
const ZONE_PREFIX: &str = "intel-rapl:";

struct RaplCounter {
    zone: String,
    energy: PersistentFile,
    max_range_microjoules: u64,
}

/// RAPL counters (`energy_uj` is root-only), opened on first use and kept
/// open for `pread`.
#[derive(Default)]
pub(crate) struct EnergyReader {
    counters: Option<Vec<RaplCounter>>,
}

impl EnergyReader {
    pub(crate) fn read(&mut self) -> Vec<EnergyZone> {
        self.counters
            .get_or_insert_with(|| discover(Path::new(POWERCAP)))
            .iter()
            .filter_map(|counter| {
                Some(EnergyZone {
                    zone: counter.zone.clone(),
                    energy_microjoules: counter.energy.read_u64().ok()?,
                    max_range_microjoules: counter.max_range_microjoules,
                })
            })
            .collect()
    }
}

fn discover(powercap: &Path) -> Vec<RaplCounter> {
    let mut directories: Vec<String> = std::fs::read_dir(powercap)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| name.starts_with(ZONE_PREFIX))
        .collect();
    directories.sort_unstable();
    let zone_name =
        |directory: &str| sysfs::read_string(powercap.join(directory).join("name")).ok();
    directories
        .iter()
        .filter_map(|directory| {
            let name = zone_name(directory)?;
            let zone = match parent_zone(directory).and_then(zone_name) {
                Some(parent) => format!("{parent}/{name}"),
                None => name,
            };
            let zone_directory = powercap.join(directory);
            Some(RaplCounter {
                zone,
                energy: PersistentFile::open(zone_directory.join("energy_uj")).ok()?,
                max_range_microjoules: sysfs::read_u64(zone_directory.join("max_energy_range_uj"))
                    .unwrap_or(u64::MAX),
            })
        })
        .collect()
}

/// `intel-rapl:0` for the sub-zone `intel-rapl:0:1`; packages have none.
fn parent_zone(directory: &str) -> Option<&str> {
    let suffix = directory.strip_prefix(ZONE_PREFIX)?;
    let (package, _) = suffix.split_once(':')?;
    directory.get(..ZONE_PREFIX.len() + package.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_parent_zones() {
        assert_eq!(parent_zone("intel-rapl:0:1"), Some("intel-rapl:0"));
        assert_eq!(parent_zone("intel-rapl:12:0"), Some("intel-rapl:12"));
        assert_eq!(parent_zone("intel-rapl:0"), None);
        assert_eq!(parent_zone("dtpm"), None);
    }

    #[test]
    fn reads_only_what_is_permitted() {
        let zones = EnergyReader::default().read();
        let readable = std::fs::File::open(format!("{POWERCAP}/{ZONE_PREFIX}0/energy_uj")).is_ok();
        if !readable {
            assert!(zones.is_empty(), "unreadable zones are left out");
        }
        assert!(zones.iter().all(|zone| !zone.zone.is_empty()));
    }
}
