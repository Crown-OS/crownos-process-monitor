//! PCIe link state from PCI sysfs, the same for every vendor.

use std::path::Path;

use monitor_core::util::parse;
use monitor_core::util::sysfs::PersistentFile;

use super::snapshot::{PcieLink, PcieSpeed};
use crate::util::attribute;

#[derive(Debug, Default)]
pub(crate) struct PcieFiles {
    speed: Option<PersistentFile>,
    width: Option<PersistentFile>,
    max_speed: Option<PersistentFile>,
    max_width: Option<PersistentFile>,
    maximum: Option<(Option<PcieSpeed>, Option<u8>)>,
}

impl PcieFiles {
    pub(crate) fn open(device_directory: &Path) -> Self {
        Self {
            speed: attribute::open(device_directory.join("current_link_speed")),
            width: attribute::open(device_directory.join("current_link_width")),
            max_speed: attribute::open(device_directory.join("max_link_speed")),
            max_width: attribute::open(device_directory.join("max_link_width")),
            maximum: None,
        }
    }

    pub(crate) fn sample(&mut self, link: &mut PcieLink, buffer: &mut Vec<u8>) {
        link.speed = attribute::read_bytes(self.speed.as_ref(), buffer).and_then(parse_speed);
        link.width =
            attribute::read_u64(self.width.as_ref()).and_then(|width| u8::try_from(width).ok());
        let (max_speed, max_width) = *self.maximum.get_or_insert_with(|| {
            (
                attribute::read_bytes(self.max_speed.as_ref(), buffer).and_then(parse_speed),
                attribute::read_u64(self.max_width.as_ref())
                    .and_then(|width| u8::try_from(width).ok()),
            )
        });
        link.max_speed = max_speed;
        link.max_width = max_width;
    }
}

/// Parses `16.0 GT/s PCIe`; `Unknown` yields `None`.
pub(crate) fn parse_speed(text: &[u8]) -> Option<PcieSpeed> {
    let rate = parse::fields(text).next().and_then(parse::parse_f64)?;
    (rate > 0.0).then(|| PcieSpeed::from_gigatransfers(rate as f32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_link_speed() {
        assert_eq!(
            parse_speed(b"16.0 GT/s PCIe"),
            Some(PcieSpeed::from_gigatransfers(16.0))
        );
        assert_eq!(
            parse_speed(b"2.5 GT/s PCIe"),
            Some(PcieSpeed::from_gigatransfers(2.5))
        );
        assert_eq!(parse_speed(b"Unknown"), None);
    }
}
