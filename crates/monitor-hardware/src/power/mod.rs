//! Batteries and AC, system power draw, RAPL, platform and daemon power
//! profiles, and logind sleep inhibitors.

mod battery;
mod dbus;
mod external;
mod profile;
pub mod rapl;
mod snapshot;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use monitor_core::util::deferred::DeferredReader;
use monitor_core::{Collector, Coverage, SampleContext, SourceResult, Watts};

pub use external::EnergyCounter;
pub use rapl::RaplReader;
pub use snapshot::{
    BatteryIdentity, BatteryReading, BatteryScope, BatteryStatus, InhibitMode, Inhibitor,
    PlatformProfile, PowerSnapshot, PowerSource, RaplPower, SystemDraw, WattHours,
};

use battery::{Supply, SupplyReadings};
use dbus::{DbusState, DbusWatcher};
use external::ExternalRapl;

use crate::gpu::{GpuKind, GpuSnapshot};
use crate::util::directory;
use monitor_core::util::cadence::Cadence;

const DISCOVERY_INTERVAL: Duration = Duration::from_secs(10);
const PROFILE_INTERVAL: Duration = Duration::from_secs(10);
/// Batteries refresh their figures every few seconds at best…
const SUPPLY_INTERVAL: Duration = Duration::from_secs(5);
/// …and nobody watches them while the window is minimized.
const MINIMIZED_SUPPLY_INTERVAL: Duration = Duration::from_secs(15);
const SUPPLY_THREAD: &str = "monitor-power-supplies";

type SupplyReader = DeferredReader<SupplyReadings>;

/// Collects [`PowerSnapshot`]s. D-Bus figures arrive from a background
/// thread refreshed every 10 s.
#[derive(Debug)]
pub struct PowerCollector {
    sysfs: PathBuf,
    supplies: Option<SupplyReader>,
    readings: SupplyReadings,
    signature: u64,
    discovery: Cadence,
    supply_reads: Cadence,
    profile_cadence: Cadence,
    platform_profile: Option<PlatformProfile>,
    rapl: RaplReader,
    external_rapl: ExternalRapl,
    dbus: DbusWatcher,
    dbus_state: DbusState,
}

impl Default for PowerCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerCollector {
    /// Spawns the D-Bus thread.
    pub fn new() -> Self {
        let sysfs = PathBuf::from("/sys");
        let supplies_root = sysfs.join("class/power_supply");
        let (supplies, readings) = start_supplies(battery::discover(&supplies_root));
        Self {
            signature: directory::signature(&supplies_root, |_| true),
            supplies,
            readings,
            supply_reads: Cadence::default(),
            rapl: RaplReader::discover_at(&sysfs.join("class/powercap")),
            external_rapl: ExternalRapl::default(),
            sysfs,
            discovery: Cadence::default(),
            profile_cadence: Cadence::default(),
            platform_profile: None,
            dbus: DbusWatcher::spawn(),
            dbus_state: DbusState {
                coverage: Coverage::Unavailable,
                ..DbusState::default()
            },
        }
    }

    /// RAPL counters the privileged helper read at `now`. While they keep
    /// arriving, samples report RAPL power from them with full coverage.
    pub fn use_external_energy<'a>(
        &mut self,
        counters: impl IntoIterator<Item = EnergyCounter<'a>>,
        now: Duration,
    ) {
        self.external_rapl.update(counters, now);
    }

    fn refresh_supplies(&mut self, now: Duration) {
        if !self.discovery.due(now, DISCOVERY_INTERVAL) {
            return;
        }
        let root = self.sysfs.join("class/power_supply");
        let signature = directory::signature(&root, |_| true);
        if signature != self.signature {
            self.signature = signature;
            (self.supplies, self.readings) = start_supplies(battery::discover(&root));
            self.supply_reads.reset();
        }
    }

    /// Republishes the latest supply readings and asks the worker for the
    /// next ones when due.
    fn read_supplies(&mut self, now: Duration, interval: Duration, output: &mut PowerSnapshot) {
        if let Some(supplies) = &self.supplies {
            let readings = &mut self.readings;
            supplies.take_fresh(|fresh| readings.clone_from(fresh));
            if self.supply_reads.due(now, interval) {
                supplies.request();
            }
        }
        output.batteries.clone_from(&self.readings.batteries);
        let system_battery = output.system_battery();
        output.source = match (self.readings.external_online, system_battery) {
            (Some(true), _) => PowerSource::Ac,
            (_, Some(battery)) if battery.status == BatteryStatus::Discharging => {
                PowerSource::Battery
            }
            (Some(false), Some(_)) => PowerSource::Battery,
            _ => PowerSource::Unknown,
        };
    }
}

/// Reads once on this thread, so the first tick has figures, then hands
/// the supplies to their worker.
fn start_supplies(mut supplies: Vec<Supply>) -> (Option<SupplyReader>, SupplyReadings) {
    let mut readings = SupplyReadings::default();
    let mut buffer = Vec::new();
    battery::read_supplies(&mut supplies, &mut buffer, &mut readings);
    let reader = (!supplies.is_empty())
        .then(|| {
            DeferredReader::spawn(
                SUPPLY_THREAD,
                (supplies, buffer),
                |(supplies, buffer): &mut (Vec<Supply>, Vec<u8>), out: &mut SupplyReadings| {
                    battery::read_supplies(supplies, buffer, out);
                },
            )
        })
        .flatten();
    (reader, readings)
}

impl Collector for PowerCollector {
    type Output = PowerSnapshot;

    fn sample(
        &mut self,
        context: &SampleContext<'_>,
        output: &mut PowerSnapshot,
    ) -> SourceResult<()> {
        let now = context.tick.monotonic();
        self.refresh_supplies(now);
        let interval = if context.interest.is_visible() {
            SUPPLY_INTERVAL
        } else {
            MINIMIZED_SUPPLY_INTERVAL
        };
        self.read_supplies(now, interval, output);
        self.rapl.sample(now, &mut output.rapl);
        output.rapl_coverage = self.rapl.coverage();
        if let Some(readings) = self.external_rapl.readings(now) {
            output.rapl.clear();
            output.rapl.extend_from_slice(readings);
            output.rapl_coverage = Coverage::Full;
        }
        if self.profile_cadence.due(now, PROFILE_INTERVAL) {
            self.platform_profile = profile::read(&self.sysfs);
        }
        output.platform_profile.clone_from(&self.platform_profile);
        if let Some(state) = self.dbus.latest() {
            self.dbus_state = state;
        }
        output
            .daemon_profile
            .clone_from(&self.dbus_state.daemon_profile);
        output.inhibitors = Arc::clone(&self.dbus_state.inhibitors);
        output.dbus_coverage = self.dbus_state.coverage;
        output.system_draw = measured_draw(output);
        Ok(())
    }
}

/// Battery discharge on battery power; otherwise the RAPL package figure.
fn measured_draw(power: &PowerSnapshot) -> Option<SystemDraw> {
    if power.source == PowerSource::Battery
        && let Some(watts) = power.system_battery().and_then(|battery| battery.power)
    {
        return Some(SystemDraw {
            watts,
            coverage: Coverage::Full,
        });
    }
    RaplReader::package_power(&power.rapl).map(|watts| SystemDraw {
        watts,
        coverage: Coverage::Estimated,
    })
}

/// System draw for the Overview: battery discharge when on battery, else
/// RAPL package plus discrete GPU power. Integrated GPUs are left out because
/// their power is part of the package (amdgpu APUs even report the package).
pub fn estimate_system_draw(power: &PowerSnapshot, gpus: &GpuSnapshot) -> Option<SystemDraw> {
    let measured = power.system_draw?;
    if measured.coverage == Coverage::Full {
        return Some(measured);
    }
    let discrete: Watts = gpus
        .devices
        .iter()
        .filter(|device| device.identity.kind == GpuKind::Discrete)
        .filter_map(|device| device.metrics.power_draw)
        .fold(Watts::new(0.0), |sum, watts| sum + watts);
    Some(SystemDraw {
        watts: measured.watts + discrete,
        coverage: Coverage::Estimated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use monitor_core::{Elapsed, FdIndex, Interest, Tick};

    #[test]
    fn live_reads_power_state() {
        let mut collector = PowerCollector::new();
        let interest = Interest::default();
        let fds = FdIndex::default();
        let mut snapshot = PowerSnapshot::default();
        let mut sample = |snapshot: &mut PowerSnapshot| {
            let context = SampleContext {
                tick: Tick::now(),
                elapsed: Elapsed::default(),
                interest: &interest,
                fds: &fds,
            };
            assert!(collector.sample(&context, snapshot).is_ok());
        };
        sample(&mut snapshot);
        for _ in 0..30 {
            std::thread::sleep(Duration::from_millis(100));
            sample(&mut snapshot);
            if snapshot.dbus_coverage == Coverage::Full {
                break;
            }
        }
        println!(
            "source {:?}, draw {:?}",
            snapshot.source, snapshot.system_draw
        );
        for battery in &snapshot.batteries {
            println!(
                "{} {:?} {:?} power {:?} health {:?} cycles {:?} empty {:?} full {:?}",
                battery.identity.name,
                battery.status,
                battery.capacity,
                battery.power,
                battery.health,
                battery.cycle_count,
                battery.time_to_empty,
                battery.time_to_full
            );
        }
        println!(
            "rapl {:?}, platform {:?}, daemon {:?}, dbus {:?}",
            snapshot.rapl_coverage,
            snapshot.platform_profile,
            snapshot.daemon_profile,
            snapshot.dbus_coverage
        );
        for inhibitor in snapshot.inhibitors.iter() {
            println!(
                "  inhibitor {} {:?} {} ({})",
                inhibitor.what, inhibitor.mode, inhibitor.who, inhibitor.why
            );
        }
        if std::path::Path::new("/sys/class/power_supply/BAT1").exists() {
            let battery = snapshot
                .batteries
                .iter()
                .find(|battery| &*battery.identity.name == "BAT1");
            assert!(
                battery
                    .is_some_and(|battery| battery.capacity.is_some() && battery.health.is_some())
            );
        }
    }
}
