//! SMART health as UDisks2 reports it, and its mapping from D-Bus properties.

use std::sync::Arc;
use std::time::Duration;

use monitor_core::{Bytes, Celsius, Percent};

use crate::util::dbus::{self, Properties};

const KELVIN_OFFSET: f64 = 273.15;
const NOT_AVAILABLE: i32 = -1;
const WORN_OUT_PERCENT: f32 = 100.0;

#[derive(Clone, Debug, Default, PartialEq)]
pub enum DriveHealth {
    /// UDisks2 has not answered yet.
    #[default]
    Pending,
    /// No UDisks2, or the drive hides SMART (typical for USB bridges).
    Unavailable,
    Ata(Arc<AtaHealth>),
    Nvme(Arc<NvmeHealth>),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum HealthVerdict {
    Good,
    /// Wear or spare thresholds crossed, or a past self-test failed.
    Warning,
    /// The drive predicts its own failure or raised a critical warning.
    Failing,
    #[default]
    Unknown,
}

impl DriveHealth {
    pub fn verdict(&self) -> HealthVerdict {
        match self {
            Self::Pending | Self::Unavailable => HealthVerdict::Unknown,
            Self::Ata(ata) => ata.verdict(),
            Self::Nvme(nvme) => nvme.verdict(),
        }
    }

    pub fn temperature(&self) -> Option<Celsius> {
        match self {
            Self::Pending | Self::Unavailable => None,
            Self::Ata(ata) => ata.temperature,
            Self::Nvme(nvme) => nvme.temperature,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AttributeUnit {
    #[default]
    Unknown,
    Dimensionless,
    Milliseconds,
    Sectors,
    Millikelvin,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AtaAttribute {
    pub id: u8,
    pub name: Arc<str>,
    pub flags: u16,
    /// Normalized values, `None` where the drive reports none.
    pub value: Option<u8>,
    pub worst: Option<u8>,
    pub threshold: Option<u8>,
    /// The decoded raw value in `unit`.
    pub pretty: i64,
    pub unit: AttributeUnit,
}

impl AtaAttribute {
    /// The normalized value is at or below its failure threshold.
    pub fn is_failing(&self) -> bool {
        matches!((self.value, self.threshold), (Some(value), Some(threshold)) if threshold > 0 && value <= threshold)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AtaHealth {
    pub supported: bool,
    pub enabled: bool,
    pub failing: bool,
    pub temperature: Option<Celsius>,
    pub power_on: Option<Duration>,
    pub bad_sectors: Option<u64>,
    pub selftest_status: Option<Arc<str>>,
    pub attributes: Vec<AtaAttribute>,
    pub updated_unix_seconds: u64,
}

impl AtaHealth {
    fn verdict(&self) -> HealthVerdict {
        if !self.supported || !self.enabled {
            HealthVerdict::Unknown
        } else if self.failing || self.attributes.iter().any(AtaAttribute::is_failing) {
            HealthVerdict::Failing
        } else if self.bad_sectors.is_some_and(|sectors| sectors > 0)
            || failed_selftest(self.selftest_status.as_deref())
        {
            HealthVerdict::Warning
        } else {
            HealthVerdict::Good
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NvmeHealth {
    /// `spare`, `temperature`, `degraded`, `readonly`, `volatile_mem`, `pmr_readonly`.
    pub critical_warnings: Vec<Arc<str>>,
    pub power_on_hours: Option<u64>,
    pub temperature: Option<Celsius>,
    pub percent_used: Option<Percent>,
    pub available_spare: Option<Percent>,
    pub spare_threshold: Option<Percent>,
    pub media_errors: Option<u64>,
    pub data_read: Option<Bytes>,
    pub data_written: Option<Bytes>,
    pub unsafe_shutdowns: Option<u64>,
    pub power_cycles: Option<u64>,
    pub selftest_status: Option<Arc<str>>,
    pub updated_unix_seconds: u64,
}

impl NvmeHealth {
    fn verdict(&self) -> HealthVerdict {
        let spare_low = matches!(
            (self.available_spare, self.spare_threshold),
            (Some(spare), Some(threshold)) if spare.get() < threshold.get()
        );
        if !self.critical_warnings.is_empty() {
            HealthVerdict::Failing
        } else if spare_low
            || self
                .percent_used
                .is_some_and(|used| used.get() >= WORN_OUT_PERCENT)
            || self.media_errors.is_some_and(|errors| errors > 0)
            || failed_selftest(self.selftest_status.as_deref())
        {
            HealthVerdict::Warning
        } else {
            HealthVerdict::Good
        }
    }
}

fn failed_selftest(status: Option<&str>) -> bool {
    status.is_some_and(|status| {
        status.contains("fatal") || status.contains("error") || status.starts_with("fail")
    })
}

/// One element of `Drive.Ata.SmartGetAttributes`: `(ysqiiixia{sv})`.
pub(crate) type RawAtaAttribute = (u8, String, u16, i32, i32, i32, i64, i32, Properties);

pub(crate) fn ata_health(properties: &Properties, attributes: Vec<RawAtaAttribute>) -> AtaHealth {
    AtaHealth {
        supported: dbus::value(properties, "SmartSupported").unwrap_or(false),
        enabled: dbus::value(properties, "SmartEnabled").unwrap_or(false),
        failing: dbus::value(properties, "SmartFailing").unwrap_or(false),
        temperature: dbus::value(properties, "SmartTemperature").and_then(from_kelvin),
        power_on: dbus::value(properties, "SmartPowerOnSeconds")
            .filter(|seconds: &u64| *seconds > 0)
            .map(Duration::from_secs),
        bad_sectors: dbus::value::<i64>(properties, "SmartNumBadSectors")
            .and_then(|count| u64::try_from(count).ok()),
        selftest_status: dbus::string(properties, "SmartSelftestStatus").map(Arc::from),
        attributes: attributes.into_iter().map(ata_attribute).collect(),
        updated_unix_seconds: dbus::value(properties, "SmartUpdated").unwrap_or(0),
    }
}

fn ata_attribute(
    (id, name, flags, value, worst, threshold, pretty, unit, _): RawAtaAttribute,
) -> AtaAttribute {
    let normalized = |raw: i32| {
        (raw != NOT_AVAILABLE)
            .then(|| u8::try_from(raw).ok())
            .flatten()
    };
    AtaAttribute {
        id,
        name: Arc::from(name),
        flags,
        value: normalized(value),
        worst: normalized(worst),
        threshold: normalized(threshold),
        pretty,
        unit: match unit {
            1 => AttributeUnit::Dimensionless,
            2 => AttributeUnit::Milliseconds,
            3 => AttributeUnit::Sectors,
            4 => AttributeUnit::Millikelvin,
            _ => AttributeUnit::Unknown,
        },
    }
}

/// Builds NVMe health from `NVMe.Controller` properties and the dictionary
/// `SmartGetAttributes` returns.
pub(crate) fn nvme_health(properties: &Properties, attributes: &Properties) -> NvmeHealth {
    let percent = |name: &str| {
        dbus::value::<u8>(attributes, name).map(|value| Percent::new(f32::from(value)))
    };
    NvmeHealth {
        critical_warnings: properties
            .get("SmartCriticalWarning")
            .map(|variant| dbus::strings(variant).into_iter().map(Arc::from).collect())
            .unwrap_or_default(),
        power_on_hours: dbus::value(properties, "SmartPowerOnHours")
            .or_else(|| dbus::value::<u64>(attributes, "power_on_hours")),
        temperature: dbus::value::<u16>(properties, "SmartTemperature")
            .and_then(|kelvin| from_kelvin(f64::from(kelvin))),
        percent_used: percent("percent_used"),
        available_spare: percent("avail_spare"),
        spare_threshold: percent("spare_thresh"),
        media_errors: dbus::value(attributes, "media_errors"),
        data_read: dbus::value(attributes, "total_data_read").map(Bytes::new),
        data_written: dbus::value(attributes, "total_data_written").map(Bytes::new),
        unsafe_shutdowns: dbus::value(attributes, "unsafe_shutdowns"),
        power_cycles: dbus::value(attributes, "power_cycles"),
        selftest_status: dbus::string(properties, "SmartSelftestStatus").map(Arc::from),
        updated_unix_seconds: dbus::value(properties, "SmartUpdated").unwrap_or(0),
    }
}

fn from_kelvin(kelvin: f64) -> Option<Celsius> {
    (kelvin > 0.0).then(|| Celsius::new((kelvin - KELVIN_OFFSET) as f32))
}

#[cfg(test)]
mod tests {
    use zbus::OwnedVariant;

    use super::*;

    #[test]
    fn maps_nvme_properties_and_attributes() {
        let mut properties = Properties::new();
        properties.insert("SmartPowerOnHours".into(), OwnedVariant::from(5487_u64));
        properties.insert("SmartTemperature".into(), OwnedVariant::from(315_u16));
        properties.insert("SmartUpdated".into(), OwnedVariant::from(1_793_800_692_u64));
        let mut attributes = Properties::new();
        attributes.insert("avail_spare".into(), OwnedVariant::from(100_u8));
        attributes.insert("spare_thresh".into(), OwnedVariant::from(5_u8));
        attributes.insert("percent_used".into(), OwnedVariant::from(6_u8));
        attributes.insert(
            "total_data_written".into(),
            OwnedVariant::from(8_872_455_168_000_u64),
        );
        attributes.insert("media_errors".into(), OwnedVariant::from(0_u64));
        attributes.insert("unsafe_shutdowns".into(), OwnedVariant::from(350_u64));
        let health = nvme_health(&properties, &attributes);
        assert_eq!(health.power_on_hours, Some(5487));
        assert!(
            health
                .temperature
                .is_some_and(|celsius| (celsius.get() - 41.85).abs() < 0.01)
        );
        assert_eq!(health.percent_used.map(Percent::get), Some(6.0));
        assert_eq!(health.data_written, Some(Bytes::new(8_872_455_168_000)));
        assert_eq!(health.unsafe_shutdowns, Some(350));
        assert_eq!(
            DriveHealth::Nvme(Arc::new(health)).verdict(),
            HealthVerdict::Good
        );
    }

    #[test]
    fn flags_failing_ata_attributes() {
        let mut properties = Properties::new();
        properties.insert("SmartSupported".into(), OwnedVariant::from(true));
        properties.insert("SmartEnabled".into(), OwnedVariant::from(true));
        properties.insert("SmartPowerOnSeconds".into(), OwnedVariant::from(7200_u64));
        let reallocated = (
            5,
            "reallocated-sector-count".to_owned(),
            0x33,
            9,
            9,
            10,
            1200,
            3,
            Properties::new(),
        );
        let health = ata_health(&properties, vec![reallocated]);
        assert_eq!(health.power_on, Some(Duration::from_secs(7200)));
        assert_eq!(health.attributes[0].unit, AttributeUnit::Sectors);
        assert!(health.attributes[0].is_failing());
        assert_eq!(
            DriveHealth::Ata(Arc::new(health)).verdict(),
            HealthVerdict::Failing
        );
    }
}
