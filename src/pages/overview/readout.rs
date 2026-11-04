//! Reactive text over the live snapshot in the units the settings ask for.

use crownui::ext::Prop;
use crownui::prelude::{IntoProp, Runtime, bind};
use monitor_core::PressureLevel;

use crate::app::AppState;
use crate::app::live::{LiveSnapshot, recent};
use crate::util::alert_text::AlertUnits;

/// Text that follows the live snapshot and the unit settings.
pub fn reading(
    state: AppState,
    read: impl Fn(&LiveSnapshot, AlertUnits) -> String + 'static,
) -> Prop<String> {
    let (live, settings) = (state.live, state.settings);
    bind(move |runtime: &mut Runtime| {
        let current = settings.get(runtime);
        let units = AlertUnits {
            rates: current.rates,
            temperature: current.temperature,
        };
        live.with(runtime, |snapshot| read(snapshot, units))
    })
    .into_prop()
}

/// A flag that follows the live snapshot, e.g. a reading under strain.
pub fn live_flag(state: AppState, read: fn(&LiveSnapshot) -> bool) -> Prop<bool> {
    let live = state.live;
    bind(move |runtime: &mut Runtime| live.with(runtime, read)).into_prop()
}

pub const fn pressure_label(level: PressureLevel) -> &'static str {
    match level {
        PressureLevel::Normal => "Normal",
        PressureLevel::Elevated => "Elevated",
        PressureLevel::Critical => "Critical",
    }
}

/// Mean and peak of the recorded samples of the newest `window` seconds; NaN when none.
pub fn window_mean_and_peak(history: &[f32], window: usize) -> (f32, f32) {
    let (sum, count, peak) = recent(history, window)
        .iter()
        .filter(|value| value.is_finite())
        .fold((0.0, 0_u32, f32::NAN), |(sum, count, peak), value| {
            (sum + value, count + 1, peak.max(*value))
        });
    if count == 0 {
        (f32::NAN, f32::NAN)
    } else {
        (sum / count as f32, peak)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_statistics_skip_gaps() {
        let history = [90.0, f32::NAN, 10.0, 30.0];
        assert_eq!(window_mean_and_peak(&history, 3), (20.0, 30.0));
        let (mean, peak) = window_mean_and_peak(&[f32::NAN], 1);
        assert!(mean.is_nan() && peak.is_nan());
    }
}
