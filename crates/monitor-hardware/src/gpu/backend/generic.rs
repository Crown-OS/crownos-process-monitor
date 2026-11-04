//! Generic DRM drivers (panfrost, panthor, msm, v3d, etnaviv…): devfreq for
//! load and clocks; engines come from fdinfo.

use std::path::Path;

use monitor_core::util::parse;
use monitor_core::util::sysfs::PersistentFile;
use monitor_core::{Megahertz, Percent};

use super::DeviceSample;
use crate::util::{attribute, directory};

#[derive(Debug, Default)]
pub(crate) struct DevfreqBackend {
    current_frequency: Option<PersistentFile>,
    maximum_frequency: Option<PersistentFile>,
    load: Option<PersistentFile>,
}

impl DevfreqBackend {
    pub(crate) fn open(device: &Path) -> Self {
        let root = device.join("devfreq");
        let Some(node) = directory::entries(&root, |_| true).into_iter().next() else {
            return Self::default();
        };
        let node = root.join(node);
        Self {
            current_frequency: attribute::open(node.join("cur_freq")),
            maximum_frequency: attribute::open(node.join("max_freq")),
            load: attribute::open(node.join("load")),
        }
    }

    pub(crate) fn sample(&mut self, sample: &mut DeviceSample<'_>) {
        let metrics = &mut *sample.metrics;
        let hertz = |file: &Option<PersistentFile>| {
            attribute::read_u64(file.as_ref()).map(Megahertz::from_hz)
        };
        metrics.clocks.core = hertz(&self.current_frequency);
        metrics.clocks.core_max = hertz(&self.maximum_frequency);
        metrics.utilization =
            attribute::read_bytes(self.load.as_ref(), sample.buffer).and_then(parse_load);
    }
}

/// devfreq `load` reads `37@500000000Hz` or a bare percentage.
fn parse_load(text: &[u8]) -> Option<Percent> {
    let digits = text.split(|&byte| byte == b'@').next()?;
    parse::parse_u32(digits).map(|load| Percent::clamped(load as f32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_devfreq_load() {
        assert_eq!(parse_load(b"37@500000000Hz"), Some(Percent::new(37.0)));
        assert_eq!(parse_load(b"12"), Some(Percent::new(12.0)));
        assert_eq!(parse_load(b"busy"), None);
    }
}
