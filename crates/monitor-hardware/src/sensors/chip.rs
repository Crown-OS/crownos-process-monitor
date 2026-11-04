//! Classification of hwmon chips and thermal zones by driver name.

use monitor_core::Celsius;

use super::snapshot::SensorCategory;

const MILLIDEGREES: i64 = 1000;
const LOWEST_PLAUSIBLE: i64 = -60 * MILLIDEGREES;
const HIGHEST_PLAUSIBLE: i64 = 150 * MILLIDEGREES;
const BOGUS_READINGS: [i64; 3] = [127 * MILLIDEGREES, 128 * MILLIDEGREES, 255 * MILLIDEGREES];

pub(crate) fn category(chip: &str) -> SensorCategory {
    const BOARD_PREFIXES: [&str; 12] = [
        "legion", "thinkpad", "dell_smm", "asus", "nct", "it87", "it86", "applesmc", "hp", "acer",
        "yoga", "f71",
    ];
    const WIRELESS_PREFIXES: [&str; 5] = ["mt7", "iwlwifi", "ath", "rtw", "brcm"];
    match chip {
        "k10temp" | "zenpower" | "coretemp" | "x86_pkg_temp" => SensorCategory::Cpu,
        "amdgpu" | "radeon" | "nouveau" | "i915" | "xe" => SensorCategory::Gpu,
        "nvme" | "drivetemp" => SensorCategory::Drive,
        "acpitz" => SensorCategory::Acpi,
        _ if chip.contains("cpu") => SensorCategory::Cpu,
        _ if chip.starts_with("BAT") || chip.contains("battery") => SensorCategory::Battery,
        _ if BOARD_PREFIXES.iter().any(|prefix| chip.starts_with(prefix)) => SensorCategory::Board,
        _ if WIRELESS_PREFIXES
            .iter()
            .any(|prefix| chip.starts_with(prefix)) =>
        {
            SensorCategory::Wireless
        }
        _ if chip.contains("gpu") => SensorCategory::Gpu,
        _ => SensorCategory::Other,
    }
}

/// How well a sensor stands for "the CPU temperature"; lower is better.
pub(crate) fn cpu_rank(chip: &str, label: &str, category: SensorCategory) -> Option<u8> {
    match (chip, label) {
        ("k10temp" | "zenpower", "Tdie") => Some(0),
        ("coretemp", package) if package.starts_with("Package id") => Some(0),
        ("k10temp" | "zenpower", "Tctl") => Some(1),
        ("x86_pkg_temp", _) => Some(1),
        _ if category == SensorCategory::Cpu
            && !label.starts_with("Tccd")
            && !label.starts_with("Core") =>
        {
            Some(2)
        }
        _ if category == SensorCategory::Board && label.to_ascii_lowercase().contains("cpu") => {
            Some(3)
        }
        _ if category == SensorCategory::Acpi => Some(4),
        _ => None,
    }
}

/// Drops the readings drivers use to mean "no sensor".
pub(crate) fn plausible_temperature(millidegrees: i64) -> Option<Celsius> {
    let bogus = millidegrees == 0
        || !(LOWEST_PLAUSIBLE..=HIGHEST_PLAUSIBLE).contains(&millidegrees)
        || BOGUS_READINGS.contains(&millidegrees);
    (!bogus).then(|| Celsius::from_millidegrees(millidegrees))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// hwmon `name`s of this machine.
    const MACHINE_CHIPS: [(&str, SensorCategory); 11] = [
        ("ACAD", SensorCategory::Other),
        ("acpitz", SensorCategory::Acpi),
        ("ucsi_source_psy_USBC000:002", SensorCategory::Other),
        ("mt7921_phy0", SensorCategory::Wireless),
        ("BAT1", SensorCategory::Battery),
        ("nvme", SensorCategory::Drive),
        ("amdgpu", SensorCategory::Gpu),
        ("legion_hwmon", SensorCategory::Board),
        ("k10temp", SensorCategory::Cpu),
        ("yogafan", SensorCategory::Board),
        ("cpu_thermal", SensorCategory::Cpu),
    ];

    #[test]
    fn classifies_machine_chips() {
        for (chip, expected) in MACHINE_CHIPS {
            assert_eq!(category(chip), expected, "{chip}");
        }
    }

    #[test]
    fn ranks_cpu_sensors() {
        let cpu = SensorCategory::Cpu;
        assert_eq!(cpu_rank("k10temp", "Tctl", cpu), Some(1));
        assert_eq!(cpu_rank("k10temp", "Tdie", cpu), Some(0));
        assert_eq!(cpu_rank("k10temp", "Tccd1", cpu), None);
        assert_eq!(cpu_rank("coretemp", "Package id 0", cpu), Some(0));
        assert_eq!(cpu_rank("coretemp", "Core 3", cpu), None);
        assert_eq!(
            cpu_rank("legion_hwmon", "CPU Temperature", SensorCategory::Board),
            Some(3)
        );
        assert_eq!(
            cpu_rank("legion_hwmon", "GPU Temperature", SensorCategory::Board),
            None
        );
        assert_eq!(cpu_rank("acpitz", "temp1", SensorCategory::Acpi), Some(4));
    }

    #[test]
    fn filters_sentinels() {
        assert_eq!(plausible_temperature(-273_150), None);
        assert_eq!(plausible_temperature(0), None);
        assert_eq!(plausible_temperature(127_000), None);
        assert_eq!(plausible_temperature(255_000), None);
        assert_eq!(plausible_temperature(71_000), Some(Celsius::new(71.0)));
        assert_eq!(plausible_temperature(-5_000), Some(Celsius::new(-5.0)));
    }
}
