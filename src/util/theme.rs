//! Subsystem tints for each palette, light/dark aware, plus the strain red.

use crownui::ext::Prop;
use crownui::kit::{Theme, ThemeMode};
use crownui::prelude::{Color, Cx, Memo, Signal, rgb};
use serde::{Deserialize, Serialize};

use super::prop::read_prop;

/// The colour family charts and cards are painted in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Palette {
    #[default]
    Vitals,
    Monochrome,
    ColorBlindSafe,
}

/// What a reading measures; each has its own tint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subsystem {
    Cpu,
    Memory,
    Gpu,
    Disk,
    Network,
    Power,
    Temperature,
}

impl Subsystem {
    pub const ALL: [Self; 7] = [
        Self::Cpu,
        Self::Memory,
        Self::Gpu,
        Self::Disk,
        Self::Network,
        Self::Power,
        Self::Temperature,
    ];
}

#[derive(Clone, Copy)]
struct Tint {
    light: u32,
    dark: u32,
}

const fn tint(light: u32, dark: u32) -> Tint {
    Tint { light, dark }
}

const VITALS: [Tint; 7] = [
    tint(0x007aff, 0x0a84ff),
    tint(0xaf52de, 0xbf5af2),
    tint(0xff2d55, 0xff375f),
    tint(0xff9500, 0xff9f0a),
    tint(0x30b0c7, 0x64d2ff),
    tint(0x34c759, 0x30d158),
    tint(0xd4a100, 0xffd60a),
];

const MONOCHROME: [Tint; 7] = [
    tint(0x1c1c1e, 0xf2f2f7),
    tint(0x3a3a3c, 0xd1d1d6),
    tint(0x48484a, 0xc7c7cc),
    tint(0x545456, 0xaeaeb2),
    tint(0x636366, 0xe5e5ea),
    tint(0x2c2c2e, 0x9a9aa0),
    tint(0x6e6e73, 0x8e8e93),
];

const OKABE_ITO: [Tint; 7] = [
    tint(0x0072b2, 0x56b4e9),
    tint(0xcc79a7, 0xcc79a7),
    tint(0x009e73, 0x009e73),
    tint(0xe69f00, 0xe69f00),
    tint(0x56b4e9, 0x0072b2),
    tint(0x8a7f00, 0xf0e442),
    tint(0xd55e00, 0xd55e00),
];

/// The tint of `subsystem` in `palette` for a light or dark surface.
pub fn subsystem_color(palette: Palette, mode: ThemeMode, subsystem: Subsystem) -> Color {
    let tints = match palette {
        Palette::Vitals => &VITALS,
        Palette::Monochrome => &MONOCHROME,
        Palette::ColorBlindSafe => &OKABE_ITO,
    };
    let tint = tints[subsystem as usize];
    rgb(if mode.is_dark() {
        tint.dark
    } else {
        tint.light
    })
}

/// A distinct colour for the `index`th series of a multi-series chart.
pub fn series_color(palette: Palette, mode: ThemeMode, index: usize) -> Color {
    subsystem_color(palette, mode, Subsystem::ALL[index % Subsystem::ALL.len()])
}

/// Readings under strain turn this red.
pub fn strain_color(theme: &Theme) -> Color {
    theme.status.danger
}

/// A colour that follows the theme's mode and the palette setting.
pub fn subsystem_tint(
    cx: &mut Cx,
    theme: Signal<Theme>,
    palette: Memo<Palette>,
    subsystem: Subsystem,
) -> Memo<Color> {
    cx.memo(move |runtime| {
        let mode = theme.with(runtime, |theme| theme.mode);
        subsystem_color(palette.get(runtime), mode, subsystem)
    })
}

/// The `index`th series colour, following the theme and the palette setting.
pub fn series_tint(
    cx: &mut Cx,
    theme: Signal<Theme>,
    palette: Memo<Palette>,
    index: usize,
) -> Memo<Color> {
    cx.memo(move |runtime| {
        let mode = theme.with(runtime, |theme| theme.mode);
        series_color(palette.get(runtime), mode, index)
    })
}

/// `normal` from the theme, or the strain red while `strained` holds.
pub fn strain_tint(
    theme: Signal<Theme>,
    mut strained: Prop<bool>,
    normal: fn(&Theme) -> Color,
) -> Prop<Color> {
    Prop::Dynamic(Box::new(move |runtime| {
        let strained = read_prop(&mut strained, runtime);
        theme.with(runtime, |theme| {
            if strained {
                strain_color(theme)
            } else {
                normal(theme)
            }
        })
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_palette_tints_every_subsystem_distinctly() {
        for palette in [
            Palette::Vitals,
            Palette::Monochrome,
            Palette::ColorBlindSafe,
        ] {
            for mode in [
                ThemeMode::from_dark_mode(false),
                ThemeMode::from_dark_mode(true),
            ] {
                let colors: Vec<_> = Subsystem::ALL
                    .iter()
                    .map(|subsystem| subsystem_color(palette, mode, *subsystem).to_rgba8())
                    .collect();
                let mut unique = colors.clone();
                unique.sort_unstable();
                unique.dedup();
                assert_eq!(unique.len(), colors.len(), "{palette:?} {mode:?}");
            }
        }
    }

    #[test]
    fn series_colours_cycle_through_the_palette() {
        let mode = ThemeMode::from_dark_mode(true);
        assert_eq!(
            series_color(Palette::Vitals, mode, 0),
            series_color(Palette::Vitals, mode, Subsystem::ALL.len())
        );
    }
}
