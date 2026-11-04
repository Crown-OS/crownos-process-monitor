use monitor_core::Bytes;

/// udev's database record for the DMI device, where `udev-builtin-dmi_memory_id`
/// stores the SMBIOS memory table readable without privileges.
const DMI_UDEV_RECORD: &str = "/run/udev/data/+dmi:id";
const PROPERTY_PREFIX: &str = "E:MEMORY_DEVICE_";

/// One populated memory slot.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryModule {
    pub index: u32,
    /// Slot name, e.g. `DIMM 0`.
    pub locator: String,
    /// Bank or channel, e.g. `P0 CHANNEL A`.
    pub bank: Option<String>,
    pub size: Bytes,
    /// Memory type, e.g. `DDR5`.
    pub kind: Option<String>,
    pub form_factor: Option<String>,
    /// Rated speed in MT/s.
    pub speed_mts: Option<u32>,
    /// Configured speed in MT/s.
    pub configured_speed_mts: Option<u32>,
    pub manufacturer: Option<String>,
    pub part_number: Option<String>,
    pub rank: Option<u32>,
}

/// DIMMs from the udev database. Empty when udev has no record.
pub fn memory_modules() -> Vec<MemoryModule> {
    std::fs::read_to_string(DMI_UDEV_RECORD)
        .map(|record| parse_modules(&record))
        .unwrap_or_default()
}

fn parse_modules(record: &str) -> Vec<MemoryModule> {
    let mut modules: Vec<MemoryModule> = Vec::new();
    for line in record.lines() {
        let Some((key, value)) = line
            .strip_prefix(PROPERTY_PREFIX)
            .and_then(|rest| rest.split_once('='))
        else {
            continue;
        };
        let Some((index, field)) = key.split_once('_') else {
            continue;
        };
        let Ok(index) = index.parse::<u32>() else {
            continue;
        };
        let module = match modules.iter().position(|module| module.index == index) {
            Some(position) => &mut modules[position],
            None => {
                modules.push(MemoryModule {
                    index,
                    ..MemoryModule::default()
                });
                let last = modules.len() - 1;
                &mut modules[last]
            }
        };
        let text = || {
            Some(value.trim().to_owned())
                .filter(|text| !text.is_empty() && text != "Not Specified" && text != "Unknown")
        };
        match field {
            "LOCATOR" => module.locator = value.trim().to_owned(),
            "BANK_LOCATOR" => module.bank = text(),
            "SIZE" => module.size = Bytes::new(value.parse().unwrap_or(0)),
            "TYPE" => module.kind = text(),
            "FORM_FACTOR" => module.form_factor = text(),
            "SPEED_MTS" => module.speed_mts = value.parse().ok(),
            "CONFIGURED_SPEED_MTS" => module.configured_speed_mts = value.parse().ok(),
            "MANUFACTURER" => module.manufacturer = text(),
            "PART_NUMBER" => module.part_number = text(),
            "RANK" => module.rank = value.parse().ok(),
            _ => {}
        }
    }
    modules.retain(|module| module.size.get() > 0);
    modules.sort_by_key(|module| module.index);
    modules
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECORD: &str = "I:4774388\n\
E:ID_VENDOR=LENOVO\n\
E:MEMORY_ARRAY_LOCATION=System Board Or Motherboard\n\
E:MEMORY_DEVICE_0_SIZE=8589934592\n\
E:MEMORY_DEVICE_0_FORM_FACTOR=SODIMM\n\
E:MEMORY_DEVICE_0_LOCATOR=DIMM 0\n\
E:MEMORY_DEVICE_0_BANK_LOCATOR=P0 CHANNEL A\n\
E:MEMORY_DEVICE_0_TYPE=DDR5\n\
E:MEMORY_DEVICE_0_SPEED_MTS=5600\n\
E:MEMORY_DEVICE_0_MANUFACTURER=Hynix\n\
E:MEMORY_DEVICE_0_ASSET_TAG=Not Specified\n\
E:MEMORY_DEVICE_0_PART_NUMBER=HMCG66AGBSA095N\n\
E:MEMORY_DEVICE_0_RANK=1\n\
E:MEMORY_DEVICE_0_CONFIGURED_SPEED_MTS=5600\n\
E:MEMORY_DEVICE_1_SIZE=8589934592\n\
E:MEMORY_DEVICE_1_LOCATOR=DIMM 0\n\
E:MEMORY_DEVICE_1_BANK_LOCATOR=P0 CHANNEL B\n\
E:MEMORY_DEVICE_1_TYPE=DDR5\n\
E:MEMORY_DEVICE_2_LOCATOR=DIMM 1\n\
E:MEMORY_ARRAY_NUM_DEVICES=3\n\
G:systemd\n";

    #[test]
    fn parses_udev_record() {
        let modules = parse_modules(RECORD);
        assert_eq!(modules.len(), 2, "empty slot dropped");
        let first = &modules[0];
        assert_eq!(first.size, Bytes::new(8_589_934_592));
        assert_eq!(first.kind.as_deref(), Some("DDR5"));
        assert_eq!(first.speed_mts, Some(5600));
        assert_eq!(first.form_factor.as_deref(), Some("SODIMM"));
        assert_eq!(first.manufacturer.as_deref(), Some("Hynix"));
        assert_eq!(first.part_number.as_deref(), Some("HMCG66AGBSA095N"));
        assert_eq!(first.bank.as_deref(), Some("P0 CHANNEL A"));
        assert_eq!(modules[1].bank.as_deref(), Some("P0 CHANNEL B"));
    }

    #[test]
    fn reads_live_modules() {
        let modules = memory_modules();
        assert!(modules.iter().all(|module| module.size.get() > 0));
    }
}
