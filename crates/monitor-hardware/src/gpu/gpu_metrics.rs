//! Parser for amdgpu's binary `gpu_metrics` table.
//!
//! Layouts follow `struct gpu_metrics_v1_1..v1_3` (dGPU) and
//! `gpu_metrics_v2_1..v2_4` (APU) of the kernel's `kgd_pp_interface.h`; each
//! later content revision only appends fields, so one offset table serves a
//! whole family. Other revisions return `None` and the caller falls back to
//! the plain sysfs files.

use monitor_core::{Celsius, Megahertz, Percent, Rpm, Watts};

const HEADER_SIZE: usize = 4;
const UNAVAILABLE: u16 = u16::MAX;
const CENTI: f32 = 100.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MetricsFamily {
    Discrete,
    Apu,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MetricsHeader {
    pub(crate) structure_size: u16,
    pub(crate) format_revision: u8,
    pub(crate) content_revision: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct AmdGpuMetrics {
    pub(crate) edge: Option<Celsius>,
    pub(crate) hotspot: Option<Celsius>,
    pub(crate) memory_temperature: Option<Celsius>,
    pub(crate) gfx_activity: Option<Percent>,
    pub(crate) media_activity: Option<Percent>,
    pub(crate) socket_power: Option<Watts>,
    pub(crate) gfx_clock: Option<Megahertz>,
    pub(crate) memory_clock: Option<Megahertz>,
    pub(crate) fan: Option<Rpm>,
}

/// Byte offsets of the fields we use, and how to scale them.
struct Layout {
    minimum_size: usize,
    edge: usize,
    hotspot: Option<usize>,
    memory_temperature: Option<usize>,
    gfx_activity: usize,
    media_activity: usize,
    socket_power: usize,
    average_gfx_clock: usize,
    average_memory_clock: usize,
    current_gfx_clock: usize,
    current_memory_clock: usize,
    fan: Option<usize>,
    centi_units: bool,
}

const DISCRETE_V1: Layout = Layout {
    minimum_size: 74,
    edge: 4,
    hotspot: Some(6),
    memory_temperature: Some(8),
    gfx_activity: 16,
    media_activity: 20,
    socket_power: 22,
    average_gfx_clock: 40,
    average_memory_clock: 44,
    current_gfx_clock: 54,
    current_memory_clock: 58,
    fan: Some(72),
    centi_units: false,
};

const APU_V2: Layout = Layout {
    minimum_size: 88,
    edge: 4,
    hotspot: None,
    memory_temperature: None,
    gfx_activity: 28,
    media_activity: 30,
    socket_power: 40,
    average_gfx_clock: 64,
    average_memory_clock: 68,
    current_gfx_clock: 76,
    current_memory_clock: 80,
    fan: None,
    centi_units: true,
};

pub(crate) fn header(bytes: &[u8]) -> Option<MetricsHeader> {
    let header = bytes.get(..HEADER_SIZE)?;
    Some(MetricsHeader {
        structure_size: u16::from_le_bytes([header[0], header[1]]),
        format_revision: header[2],
        content_revision: header[3],
    })
}

pub(crate) fn family(header: MetricsHeader) -> Option<MetricsFamily> {
    match (header.format_revision, header.content_revision) {
        (1, 1..=3) => Some(MetricsFamily::Discrete),
        (2, 1..=4) => Some(MetricsFamily::Apu),
        _ => None,
    }
}

pub(crate) fn parse(bytes: &[u8]) -> Option<AmdGpuMetrics> {
    let layout = match family(header(bytes)?)? {
        MetricsFamily::Discrete => &DISCRETE_V1,
        MetricsFamily::Apu => &APU_V2,
    };
    if bytes.len() < layout.minimum_size {
        return None;
    }
    let field = |offset: usize| {
        let pair = bytes.get(offset..offset + 2)?;
        let value = u16::from_le_bytes([pair[0], pair[1]]);
        (value != UNAVAILABLE).then_some(value)
    };
    let scale = |value: u16| {
        if layout.centi_units {
            f32::from(value) / CENTI
        } else {
            f32::from(value)
        }
    };
    let temperature = |offset: usize| field(offset).map(|value| Celsius::new(scale(value)));
    let activity = |offset: usize| field(offset).map(|value| Percent::clamped(scale(value)));
    let clock = |current: usize, average: usize| {
        field(current)
            .or_else(|| field(average))
            .map(|mhz| Megahertz::new(u32::from(mhz)))
    };
    let socket_power = field(layout.socket_power).map(|value| {
        if layout.centi_units {
            Watts::from_milliwatts(u64::from(value))
        } else {
            Watts::new(f32::from(value))
        }
    });
    Some(AmdGpuMetrics {
        edge: temperature(layout.edge),
        hotspot: layout.hotspot.and_then(temperature),
        memory_temperature: layout.memory_temperature.and_then(temperature),
        gfx_activity: activity(layout.gfx_activity),
        media_activity: activity(layout.media_activity),
        socket_power,
        gfx_clock: clock(layout.current_gfx_clock, layout.average_gfx_clock),
        memory_clock: clock(layout.current_memory_clock, layout.average_memory_clock),
        fan: layout
            .fan
            .and_then(field)
            .map(|rpm| Rpm::new(u32::from(rpm))),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `/sys/class/drm/card1/device/gpu_metrics` of the Radeon 780M (v2.1).
    const PHOENIX_V2_1: [u8; 120] = [
        0x78, 0x00, 0x02, 0x01, 0x75, 0x12, 0xfe, 0x12, 0xa7, 0x12, 0x8e, 0x12, 0x24, 0x13, 0xa7,
        0x12, 0xe5, 0x12, 0x8e, 0x12, 0xfe, 0x12, 0x68, 0x12, 0xe5, 0x12, 0xff, 0xff, 0xf0, 0x01,
        0x00, 0x00, 0x40, 0xba, 0x33, 0x88, 0xac, 0x01, 0x00, 0x00, 0x44, 0x31, 0xff, 0xff, 0x4e,
        0x04, 0xeb, 0x16, 0x28, 0x00, 0xe1, 0x05, 0xa8, 0x04, 0xef, 0x04, 0xad, 0x02, 0xdc, 0x02,
        0x02, 0x0c, 0xa2, 0x03, 0x20, 0x03, 0x90, 0x01, 0xe8, 0x03, 0xe8, 0x03, 0x20, 0x03, 0x20,
        0x03, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x35, 0x05,
        0x60, 0x09, 0x35, 0x05, 0x60, 0x09, 0x60, 0x09, 0x35, 0x05, 0x60, 0x09, 0x35, 0x05, 0x60,
        0x09, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    ];

    #[test]
    fn parses_phoenix_apu_table() {
        let header = header(&PHOENIX_V2_1);
        assert_eq!(
            header,
            Some(MetricsHeader {
                structure_size: 120,
                format_revision: 2,
                content_revision: 1
            })
        );
        let Some(metrics) = parse(&PHOENIX_V2_1) else {
            panic!("v2.1 table must parse");
        };
        assert_eq!(metrics.edge, Some(Celsius::new(47.25)));
        assert_eq!(metrics.gfx_activity, Some(Percent::new(4.96)));
        assert_eq!(metrics.media_activity, Some(Percent::ZERO));
        assert_eq!(metrics.socket_power, Some(Watts::from_milliwatts(12_612)));
        assert_eq!(metrics.gfx_clock, Some(Megahertz::new(800)));
        assert_eq!(metrics.memory_clock, Some(Megahertz::new(1000)));
        assert_eq!(metrics.hotspot, None);
        assert_eq!(metrics.fan, None);
    }

    #[test]
    fn parses_discrete_table() {
        let mut table = [0xff_u8; 96];
        table[..4].copy_from_slice(&[96, 0, 1, 3]);
        let mut put = |offset: usize, value: u16| {
            table[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        };
        put(4, 52);
        put(6, 61);
        put(8, 58);
        put(16, 37);
        put(20, 4);
        put(22, 87);
        put(54, 2450);
        put(58, 1250);
        put(72, 1430);
        let Some(metrics) = parse(&table) else {
            panic!("v1.3 table must parse");
        };
        assert_eq!(metrics.edge, Some(Celsius::new(52.0)));
        assert_eq!(metrics.hotspot, Some(Celsius::new(61.0)));
        assert_eq!(metrics.memory_temperature, Some(Celsius::new(58.0)));
        assert_eq!(metrics.gfx_activity, Some(Percent::new(37.0)));
        assert_eq!(metrics.socket_power, Some(Watts::new(87.0)));
        assert_eq!(metrics.gfx_clock, Some(Megahertz::new(2450)));
        assert_eq!(metrics.memory_clock, Some(Megahertz::new(1250)));
        assert_eq!(metrics.fan, Some(Rpm::new(1430)));
    }

    #[test]
    fn rejects_unknown_revisions_and_short_tables() {
        assert_eq!(parse(&[0x78, 0, 3, 0, 0, 0]), None);
        assert_eq!(parse(&[0x78, 0, 1, 4]), None);
        assert_eq!(parse(&PHOENIX_V2_1[..60]), None);
        assert_eq!(parse(&[]), None);
    }
}
