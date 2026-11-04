//! RAPL energy counters from `/sys/class/powercap/intel-rapl:*` (also used
//! by AMD Zen). `energy_uj` is root-only, so an unprivileged reader reports
//! [`Coverage::Unavailable`]; the privileged helper reuses this reader.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use monitor_core::util::sysfs::PersistentFile;
use monitor_core::{Coverage, Watts};

use super::snapshot::RaplPower;
use crate::util::{attribute, directory};

const POWERCAP: &str = "/sys/class/powercap";
const ZONE_PREFIX: &str = "intel-rapl:";
const MICROJOULES_PER_JOULE: f64 = 1_000_000.0;

#[derive(Debug)]
struct RaplZone {
    name: Arc<str>,
    is_package: bool,
    energy: PersistentFile,
    wrap_range: u64,
    previous: Option<(u64, Duration)>,
}

#[derive(Debug, Default)]
pub struct RaplReader {
    zones: Vec<RaplZone>,
    coverage: Coverage,
}

impl RaplReader {
    pub fn discover() -> Self {
        Self::discover_at(Path::new(POWERCAP))
    }

    pub fn discover_at(powercap: &Path) -> Self {
        let zones: Vec<RaplZone> =
            directory::entries(powercap, |name| name.starts_with(ZONE_PREFIX))
                .into_iter()
                .filter_map(|zone| {
                    let directory = powercap.join(&zone);
                    Some(RaplZone {
                        name: attribute::read_text(directory.join("name"))?,
                        is_package: zone.matches(':').count() == 1,
                        energy: attribute::open(directory.join("energy_uj"))?,
                        wrap_range: attribute::read_number(directory.join("max_energy_range_uj"))
                            .unwrap_or(u64::MAX),
                        previous: None,
                    })
                })
                .collect();
        let coverage = if zones.is_empty() {
            Coverage::Unavailable
        } else {
            Coverage::Full
        };
        Self { zones, coverage }
    }

    /// `Unavailable` when no zone exists or none could be opened.
    pub fn coverage(&self) -> Coverage {
        self.coverage
    }

    /// Average power of every zone since the previous call; the first call
    /// only primes the counters.
    pub fn sample(&mut self, now: Duration, out: &mut Vec<RaplPower>) {
        out.clear();
        for zone in &mut self.zones {
            let Ok(energy) = zone.energy.read_u64() else {
                continue;
            };
            let Some((previous, then)) = zone.previous.replace((energy, now)) else {
                continue;
            };
            let seconds = now.saturating_sub(then).as_secs_f64();
            if seconds <= 0.0 {
                continue;
            }
            let consumed = if energy >= previous {
                energy - previous
            } else {
                zone.wrap_range
                    .saturating_sub(previous)
                    .saturating_add(energy)
            };
            out.push(RaplPower {
                name: Arc::clone(&zone.name),
                is_package: zone.is_package,
                watts: Watts::new((consumed as f64 / MICROJOULES_PER_JOULE / seconds) as f32),
            });
        }
    }

    /// Sum of the `package-*` zones, the CPU package draw.
    pub fn package_power(readings: &[RaplPower]) -> Option<Watts> {
        readings
            .iter()
            .filter(|reading| reading.is_package && reading.name.starts_with("package"))
            .map(|reading| reading.watts)
            .reduce(|sum, watts| sum + watts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unprivileged_reader_is_unavailable_or_reads() {
        let mut reader = RaplReader::discover();
        let mut readings = Vec::new();
        reader.sample(Duration::from_secs(1), &mut readings);
        assert!(readings.is_empty(), "the first sample only primes counters");
        println!("rapl coverage {:?}", reader.coverage());
        if rustix::process::geteuid().is_root() {
            return;
        }
        if Path::new("/sys/class/powercap/intel-rapl:0/energy_uj").exists() {
            assert_eq!(reader.coverage(), Coverage::Unavailable);
        }
    }

    #[test]
    fn sums_package_zones() {
        let reading = |name: &str, is_package: bool, watts: f32| RaplPower {
            name: Arc::from(name),
            is_package,
            watts: Watts::new(watts),
        };
        let readings = [
            reading("package-0", true, 10.0),
            reading("core", false, 6.0),
            reading("package-1", true, 5.0),
            reading("psys", true, 30.0),
        ];
        assert_eq!(RaplReader::package_power(&readings), Some(Watts::new(15.0)));
    }
}
