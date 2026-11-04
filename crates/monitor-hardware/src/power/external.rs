//! RAPL counters read by the privileged helper (`energy_uj` is root-only).

use std::sync::Arc;
use std::time::Duration;

use monitor_core::Watts;

use super::snapshot::RaplPower;

const MICROJOULES_PER_JOULE: f64 = 1_000_000.0;
/// Readings older than this are no longer published.
const STALE_AFTER: Duration = Duration::from_secs(12);
const SUB_ZONE_SEPARATOR: char = '/';

/// One powercap counter as the helper reports it: `zone` is `package-0`, or
/// `package-0/core` for a sub-zone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnergyCounter<'a> {
    pub zone: &'a str,
    pub microjoules: u64,
    pub max_range_microjoules: u64,
}

#[derive(Debug)]
struct Zone {
    path: Arc<str>,
    name: Arc<str>,
    previous: u64,
    read_at: Duration,
}

#[derive(Debug, Default)]
pub(crate) struct ExternalRapl {
    zones: Vec<Zone>,
    readings: Vec<RaplPower>,
    updated_at: Option<Duration>,
}

impl ExternalRapl {
    pub(crate) fn update<'a>(
        &mut self,
        counters: impl IntoIterator<Item = EnergyCounter<'a>>,
        now: Duration,
    ) {
        self.readings.clear();
        for counter in counters {
            let Some(zone) = self
                .zones
                .iter_mut()
                .find(|zone| &*zone.path == counter.zone)
            else {
                let name = counter
                    .zone
                    .rsplit(SUB_ZONE_SEPARATOR)
                    .next()
                    .unwrap_or(counter.zone);
                self.zones.push(Zone {
                    path: Arc::from(counter.zone),
                    name: Arc::from(name),
                    previous: counter.microjoules,
                    read_at: now,
                });
                continue;
            };
            let seconds = now.saturating_sub(zone.read_at).as_secs_f64();
            let previous = std::mem::replace(&mut zone.previous, counter.microjoules);
            zone.read_at = now;
            if seconds <= 0.0 {
                continue;
            }
            let consumed = if counter.microjoules >= previous {
                counter.microjoules - previous
            } else {
                counter
                    .max_range_microjoules
                    .saturating_sub(previous)
                    .saturating_add(counter.microjoules)
            };
            self.readings.push(RaplPower {
                name: Arc::clone(&zone.name),
                is_package: !zone.path.contains(SUB_ZONE_SEPARATOR),
                watts: Watts::new((consumed as f64 / MICROJOULES_PER_JOULE / seconds) as f32),
            });
        }
        self.updated_at = Some(now);
    }

    /// The latest readings, unless the helper stopped answering.
    pub(crate) fn readings(&self, now: Duration) -> Option<&[RaplPower]> {
        self.updated_at
            .filter(|updated| now.saturating_sub(*updated) < STALE_AFTER)
            .map(|_| self.readings.as_slice())
            .filter(|readings| !readings.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counter(zone: &str, microjoules: u64) -> EnergyCounter<'_> {
        EnergyCounter {
            zone,
            microjoules,
            max_range_microjoules: 1_000_000_000,
        }
    }

    #[test]
    fn turns_counters_into_power_across_wraps() {
        let mut rapl = ExternalRapl::default();
        rapl.update(
            [
                counter("package-0", 999_000_000),
                counter("package-0/core", 0),
            ],
            Duration::from_secs(10),
        );
        assert!(rapl.readings(Duration::from_secs(10)).is_none());
        rapl.update(
            [
                counter("package-0", 4_000_000),
                counter("package-0/core", 2_000_000),
            ],
            Duration::from_secs(11),
        );
        let Some(readings) = rapl.readings(Duration::from_secs(11)) else {
            panic!("no readings after two updates");
        };
        assert_eq!(readings[0].watts, Watts::new(5.0));
        assert!(readings[0].is_package);
        assert_eq!(&*readings[1].name, "core");
        assert!(!readings[1].is_package);
        assert!(rapl.readings(Duration::from_secs(60)).is_none());
    }
}
