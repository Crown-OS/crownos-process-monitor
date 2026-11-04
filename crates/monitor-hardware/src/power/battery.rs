//! Power supplies from `/sys/class/power_supply/*/uevent`, one read each.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use monitor_core::util::parse;
use monitor_core::util::sysfs::PersistentFile;
use monitor_core::{Percent, Watts};

use super::snapshot::{BatteryIdentity, BatteryReading, BatteryScope, BatteryStatus, WattHours};
use crate::util::{attribute, directory};

const MICRO: f64 = 1_000_000.0;
const SECONDS_PER_HOUR: f64 = 3600.0;

#[derive(Debug)]
pub(crate) enum Supply {
    Battery {
        uevent: PersistentFile,
        identity: Option<Arc<BatteryIdentity>>,
        name: Arc<str>,
    },
    External {
        online: PersistentFile,
    },
}

/// What one read of every supply found.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct SupplyReadings {
    pub(crate) batteries: Vec<BatteryReading>,
    /// `None` without AC or USB supplies.
    pub(crate) external_online: Option<bool>,
}

/// Reads every supply; battery uevents go through the ACPI EC and take
/// around half a millisecond each.
pub(crate) fn read_supplies(
    supplies: &mut [Supply],
    buffer: &mut Vec<u8>,
    out: &mut SupplyReadings,
) {
    out.batteries.clear();
    out.external_online = None;
    for supply in supplies {
        match supply {
            Supply::Battery {
                uevent,
                identity,
                name,
            } => {
                let Ok(content) = uevent.read(buffer) else {
                    continue;
                };
                let fields = parse_uevent(content);
                if fields.present == Some(false) {
                    continue;
                }
                let identity = identity.get_or_insert_with(|| Arc::new(fields.identity(name)));
                out.batteries.push(fields.reading(Arc::clone(identity)));
            }
            Supply::External { online } => {
                let is_online = online.read_u64().is_ok_and(|value| value == 1);
                out.external_online = Some(out.external_online.unwrap_or(false) || is_online);
            }
        }
    }
}

pub(crate) fn discover(root: &Path) -> Vec<Supply> {
    directory::entries(root, |_| true)
        .into_iter()
        .filter_map(|name| {
            let supply = root.join(&name);
            let kind = attribute::read_text(supply.join("type"))?;
            match &*kind {
                "Battery" => Some(Supply::Battery {
                    uevent: attribute::open(supply.join("uevent"))?,
                    identity: None,
                    name: Arc::from(name),
                }),
                "Mains" | "USB" => Some(Supply::External {
                    online: attribute::open(supply.join("online"))?,
                }),
                _ => None,
            }
        })
        .collect()
}

/// Raw `POWER_SUPPLY_*` values in the kernel's micro-units.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct BatteryFields<'a> {
    pub(crate) status: BatteryStatus,
    pub(crate) present: Option<bool>,
    pub(crate) scope_device: bool,
    pub(crate) capacity: Option<u64>,
    pub(crate) power_now: Option<i64>,
    pub(crate) current_now: Option<i64>,
    pub(crate) voltage_now: Option<u64>,
    pub(crate) voltage_min_design: Option<u64>,
    pub(crate) energy_now: Option<u64>,
    pub(crate) energy_full: Option<u64>,
    pub(crate) energy_full_design: Option<u64>,
    pub(crate) charge_now: Option<u64>,
    pub(crate) charge_full: Option<u64>,
    pub(crate) charge_full_design: Option<u64>,
    pub(crate) cycle_count: Option<u64>,
    pub(crate) model: Option<&'a [u8]>,
    pub(crate) manufacturer: Option<&'a [u8]>,
    pub(crate) technology: Option<&'a [u8]>,
}

pub(crate) fn parse_uevent(uevent: &[u8]) -> BatteryFields<'_> {
    let mut fields = BatteryFields::default();
    for line in parse::lines(uevent) {
        let Some(equals) = memchr::memchr(b'=', line) else {
            continue;
        };
        let Some(key) = line[..equals].strip_prefix(b"POWER_SUPPLY_") else {
            continue;
        };
        let value = line[equals + 1..].trim_ascii();
        let unsigned = || parse::parse_u64(value);
        let signed = || parse::parse_i64(value);
        match key {
            b"STATUS" => fields.status = BatteryStatus::parse(value),
            b"PRESENT" => fields.present = Some(value == b"1"),
            b"SCOPE" => fields.scope_device = value == b"Device",
            b"CAPACITY" => fields.capacity = unsigned(),
            b"POWER_NOW" => fields.power_now = signed(),
            b"CURRENT_NOW" => fields.current_now = signed(),
            b"VOLTAGE_NOW" => fields.voltage_now = unsigned(),
            b"VOLTAGE_MIN_DESIGN" => fields.voltage_min_design = unsigned(),
            b"ENERGY_NOW" => fields.energy_now = unsigned(),
            b"ENERGY_FULL" => fields.energy_full = unsigned(),
            b"ENERGY_FULL_DESIGN" => fields.energy_full_design = unsigned(),
            b"CHARGE_NOW" => fields.charge_now = unsigned(),
            b"CHARGE_FULL" => fields.charge_full = unsigned(),
            b"CHARGE_FULL_DESIGN" => fields.charge_full_design = unsigned(),
            b"CYCLE_COUNT" => fields.cycle_count = unsigned(),
            b"MODEL_NAME" => fields.model = Some(value),
            b"MANUFACTURER" => fields.manufacturer = Some(value),
            b"TECHNOLOGY" => fields.technology = Some(value),
            _ => {}
        }
    }
    fields
}

impl BatteryFields<'_> {
    pub(crate) fn identity(&self, name: &Arc<str>) -> BatteryIdentity {
        let text = |value: Option<&[u8]>| {
            value
                .and_then(parse::as_str)
                .filter(|text| !text.is_empty())
                .map(Arc::from)
        };
        BatteryIdentity {
            name: Arc::clone(name),
            model: text(self.model),
            manufacturer: text(self.manufacturer),
            technology: text(self.technology),
            scope: if self.scope_device {
                BatteryScope::Device
            } else {
                BatteryScope::System
            },
        }
    }

    fn power_watts(&self) -> Option<f64> {
        if let Some(micro) = self.power_now {
            return Some(micro.unsigned_abs() as f64 / MICRO);
        }
        let amps = self.current_now?.unsigned_abs() as f64 / MICRO;
        let volts = self.voltage_now? as f64 / MICRO;
        Some(amps * volts)
    }

    /// Energy in µWh, from `energy_*` or `charge_*` (µAh) times design voltage.
    fn energy(&self, energy: Option<u64>, charge: Option<u64>) -> Option<u64> {
        energy.or_else(|| {
            let volts = self.voltage_min_design.or(self.voltage_now)? as f64 / MICRO;
            Some((charge? as f64 * volts) as u64)
        })
    }

    pub(crate) fn reading(&self, identity: Arc<BatteryIdentity>) -> BatteryReading {
        let energy_now = self.energy(self.energy_now, self.charge_now);
        let energy_full = self.energy(self.energy_full, self.charge_full);
        let energy_full_design = self.energy(self.energy_full_design, self.charge_full_design);
        let watts = self.power_watts().filter(|&watts| watts > 0.0);
        let hours = |micro_watt_hours: u64| {
            watts.map(|watts| {
                Duration::from_secs_f64(micro_watt_hours as f64 / MICRO / watts * SECONDS_PER_HOUR)
            })
        };
        let health = match (energy_full, energy_full_design) {
            (Some(full), Some(design)) if design > 0 => {
                Some(Percent::from_ratio(full as f64 / design as f64))
            }
            _ => None,
        };
        BatteryReading {
            identity,
            status: self.status,
            capacity: self
                .capacity
                .map(|capacity| Percent::clamped(capacity as f32)),
            power: watts.map(|watts| Watts::new(watts as f32)),
            energy_now: energy_now.map(WattHours::from_microwatt_hours),
            energy_full: energy_full.map(WattHours::from_microwatt_hours),
            energy_full_design: energy_full_design.map(WattHours::from_microwatt_hours),
            health,
            cycle_count: self
                .cycle_count
                .and_then(|count| u32::try_from(count).ok())
                .filter(|&count| count > 0),
            time_to_empty: (self.status == BatteryStatus::Discharging)
                .then(|| energy_now.and_then(hours))
                .flatten(),
            time_to_full: (self.status == BatteryStatus::Charging)
                .then(|| {
                    let remaining = energy_full?.saturating_sub(energy_now?);
                    hours(remaining)
                })
                .flatten(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `/sys/class/power_supply/BAT1/uevent` of this machine (serial removed).
    const BAT1_UEVENT: &str = "DEVTYPE=power_supply
POWER_SUPPLY_NAME=BAT1
POWER_SUPPLY_TYPE=Battery
POWER_SUPPLY_STATUS=Charging
POWER_SUPPLY_CHARGE_TYPES=Fast
POWER_SUPPLY_PRESENT=1
POWER_SUPPLY_TECHNOLOGY=Li-poly
POWER_SUPPLY_CYCLE_COUNT=593
POWER_SUPPLY_VOLTAGE_MIN_DESIGN=15440000
POWER_SUPPLY_VOLTAGE_NOW=17348000
POWER_SUPPLY_POWER_NOW=6071000
POWER_SUPPLY_ENERGY_FULL_DESIGN=80000000
POWER_SUPPLY_ENERGY_FULL=66100000
POWER_SUPPLY_ENERGY_NOW=64950000
POWER_SUPPLY_CAPACITY=98
POWER_SUPPLY_CAPACITY_LEVEL=Normal
POWER_SUPPLY_TYPE=Battery
POWER_SUPPLY_MODEL_NAME=L22M4PC2
POWER_SUPPLY_MANUFACTURER=SMP
";

    #[test]
    fn parses_charging_battery() {
        let fields = parse_uevent(BAT1_UEVENT.as_bytes());
        let name: Arc<str> = Arc::from("BAT1");
        let identity = Arc::new(fields.identity(&name));
        assert_eq!(identity.model.as_deref(), Some("L22M4PC2"));
        assert_eq!(identity.scope, BatteryScope::System);
        let battery = fields.reading(identity);
        assert_eq!(battery.status, BatteryStatus::Charging);
        assert_eq!(battery.capacity, Some(Percent::new(98.0)));
        assert_eq!(battery.power, Some(Watts::new(6.071)));
        assert_eq!(battery.energy_full_design, Some(WattHours::new(80.0)));
        assert_eq!(
            battery.health.map(|health| (health.get() * 10.0).round()),
            Some(826.0)
        );
        assert_eq!(battery.cycle_count, Some(593));
        assert_eq!(battery.time_to_empty, None);
        let to_full = battery.time_to_full.map(|left| left.as_secs());
        assert_eq!(to_full, Some(681), "1.15 Wh at 6.071 W");
    }

    #[test]
    fn derives_power_and_energy_from_charge() {
        let uevent = b"POWER_SUPPLY_STATUS=Discharging
POWER_SUPPLY_CURRENT_NOW=-1000000
POWER_SUPPLY_VOLTAGE_NOW=12000000
POWER_SUPPLY_VOLTAGE_MIN_DESIGN=11000000
POWER_SUPPLY_CHARGE_NOW=2000000
POWER_SUPPLY_CHARGE_FULL=4000000
POWER_SUPPLY_CHARGE_FULL_DESIGN=5000000
POWER_SUPPLY_SCOPE=Device
";
        let fields = parse_uevent(uevent);
        let battery = fields.reading(Arc::new(fields.identity(&Arc::from("hid-mouse"))));
        assert_eq!(battery.identity.scope, BatteryScope::Device);
        assert_eq!(battery.power, Some(Watts::new(12.0)));
        assert_eq!(battery.energy_now, Some(WattHours::new(22.0)));
        assert_eq!(battery.health, Some(Percent::new(80.0)));
        assert_eq!(battery.time_to_empty.map(|left| left.as_secs()), Some(6600));
    }
}
