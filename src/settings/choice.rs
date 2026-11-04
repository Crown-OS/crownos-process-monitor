use crate::util::format::{RateUnit, TemperatureScale};
use crate::util::theme::Palette;

use super::alerts::{
    CpuAlertLevel, GpuAlertLevel, GrowthAlertLevel, HeatAlertLevel, NotificationMode,
    SustainMinutes,
};
use super::model::{ChartWindow, CpuScale, HistoryRetention, SamplingInterval, Spacing};

/// A setting with a fixed list of options, shown as a segmented control.
pub trait Choice: Copy + PartialEq + Default + 'static {
    const ALL: &'static [Self];

    fn label(self) -> &'static str;

    fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|option| *option == self)
            .unwrap_or_default()
    }

    fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or_default()
    }
}

macro_rules! choices {
    ($($kind:ty { $($variant:ident => $label:literal),+ $(,)? })+) => {
        $(impl Choice for $kind {
            const ALL: &'static [Self] = &[$(<$kind>::$variant),+];

            fn label(self) -> &'static str {
                match self {
                    $(<$kind>::$variant => $label),+
                }
            }
        })+
    };
}

choices! {
    TemperatureScale { Celsius => "°C", Fahrenheit => "°F" }
    RateUnit { Bytes => "Bytes", Bits => "Bits" }
    CpuScale { PerCore => "Per core", PerMachine => "Per machine" }
    SamplingInterval { HalfSecond => "0.5 s", OneSecond => "1 s", TwoSeconds => "2 s" }
    HistoryRetention {
        ThirtyDays => "30 days",
        NinetyDays => "90 days",
        SixMonths => "6 months",
        OneYear => "1 year",
    }
    Palette { Vitals => "Vitals", Monochrome => "Monochrome", ColorBlindSafe => "Color-blind safe" }
    Spacing { Comfortable => "Comfortable", Compact => "Compact" }
    ChartWindow { FiveMinutes => "5 min", FifteenMinutes => "15 min", ThirtyMinutes => "30 min" }
    CpuAlertLevel { Fifty => "50%", Seventy => "70%", Ninety => "90%" }
    SustainMinutes { Five => "5 min", Ten => "10 min", Fifteen => "15 min", Thirty => "30 min" }
    GrowthAlertLevel {
        HalfGigabyte => "512 MB",
        OneGigabyte => "1 GB",
        TwoGigabytes => "2 GB",
        FourGigabytes => "4 GB",
    }
    HeatAlertLevel { Eighty => "80 °C", EightyFive => "85 °C", Ninety => "90 °C", NinetyFive => "95 °C" }
    GpuAlertLevel { Eighty => "80%", Ninety => "90%", NinetyFive => "95%" }
    NotificationMode { Desktop => "On", Off => "Off" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indices_round_trip_and_fall_back_to_the_default() {
        for option in Palette::ALL {
            assert_eq!(Palette::from_index(option.index()), *option);
        }
        assert_eq!(ChartWindow::from_index(99), ChartWindow::default());
    }
}
