use crownui::kit::{Toasts, icons};
use crownui::prelude::{AnyView, Cx, Runtime, Semantic, Styled, View, bind, keyed, text, vstack};
use monitor_engine::hardware::sensors::FanControlSupport;

use super::super::sources::{names_key, windowed_many};
use super::fan_control::fan_control;
use crate::app::AppState;
use crate::app::live::{LiveSnapshot, TYPICAL_TJMAX_CELSIUS, cpu_sensors};
use crate::components::chart::{Series, YRange, bar_chart, line_chart};
use crate::components::empty_state::{NOT_AVAILABLE, empty_state};
use crate::components::fact_grid::{Fact, fact_grid};
use crate::util::format::{count, temperature};
use crate::util::theme::Subsystem;

const CHART_FLOOR_CELSIUS: f32 = 20.0;
const CHART_HEADROOM_CELSIUS: f32 = 10.0;
const TEMPERATURE_HEIGHT: f32 = 130.0;
const FAN_HEIGHT: f32 = 70.0;
const NOTE_SIZE: f32 = 12.0;
const SECTION_GAP: f32 = 12.0;

fn sensor_keys(snapshot: &LiveSnapshot) -> u64 {
    names_key(cpu_sensors(&snapshot.tick.sensors).map(|reading| &*reading.identity.key))
}

fn fan_keys(snapshot: &LiveSnapshot) -> u64 {
    names_key(
        snapshot
            .tick
            .sensors
            .fans
            .iter()
            .map(|fan| &*fan.identity.key),
    )
}

fn temperature_chart(cx: &mut Cx, state: AppState) -> impl View + use<> {
    let theme = state.theme;
    let danger = cx.memo(move |runtime| theme.with(runtime, |theme| theme.status.danger));
    let series = vec![
        Series {
            label: "Package",
            color: state.tint(cx, Subsystem::Temperature),
        },
        Series {
            label: "Hottest core",
            color: state.tint(cx, Subsystem::Power),
        },
        Series {
            label: "Tjmax",
            color: danger,
        },
    ];
    let limit = state
        .live
        .with_untracked(cx, |snapshot| snapshot.cpu.tjmax_celsius);
    let ceiling = if limit.is_finite() {
        limit
    } else {
        TYPICAL_TJMAX_CELSIUS
    } + CHART_HEADROOM_CELSIUS;
    line_chart(
        windowed_many(
            state,
            [
                |snapshot| &snapshot.cpu.package_temperature,
                |snapshot| &snapshot.cpu.hottest_core_temperature,
                |snapshot| &snapshot.cpu.tjmax_line,
            ],
        ),
        series,
    )
    .range(YRange::Fixed {
        min: CHART_FLOOR_CELSIUS,
        max: ceiling,
    })
    .height(TEMPERATURE_HEIGHT)
}

/// Every CPU sensor's reading in the temperature unit setting.
fn sensor_readings(cx: &mut Cx, state: AppState, sensors: usize) -> impl View + use<> {
    let live = state.live;
    let settings = state.settings;
    let facts = (0..sensors)
        .map(|index| {
            let label = live.with_untracked(cx, |snapshot| {
                cpu_sensors(&snapshot.tick.sensors)
                    .nth(index)
                    .map_or_else(String::new, |reading| {
                        format!("{} {}", reading.identity.chip, reading.identity.label)
                    })
            });
            Fact::new(
                label,
                bind(move |runtime: &mut Runtime| {
                    let scale = settings.get(runtime).temperature;
                    live.with(runtime, |snapshot| {
                        let value = cpu_sensors(&snapshot.tick.sensors)
                            .nth(index)
                            .and_then(|reading| reading.value);
                        temperature(value.map_or(f32::NAN, |celsius| celsius.get()), scale)
                    })
                }),
            )
        })
        .collect();
    fact_grid(facts)
}

fn throttle_fact(state: AppState) -> Fact {
    let live = state.live;
    Fact::new(
        "Throttle events",
        bind(move |runtime: &mut Runtime| {
            live.with(runtime, |snapshot| {
                snapshot
                    .tick
                    .sensors
                    .throttle_events
                    .map_or_else(|| "Not counted on this CPU".to_owned(), count)
            })
        }),
    )
}

/// Fan speeds as bars, and the controls when a fan takes PWM.
fn fans(cx: &mut Cx, state: AppState, fans: usize, toasts: Toasts) -> AnyView {
    if fans == 0 {
        return text("No fan speeds are reported by this machine.")
            .text_size(NOTE_SIZE)
            .boxed();
    }
    let live = state.live;
    let tint = state.tint(cx, Subsystem::Temperature);
    let speeds = move |runtime: &mut Runtime, sink: &mut dyn FnMut(&[f32])| {
        live.with(runtime, |snapshot| sink(&snapshot.cpu.fan_rpm));
    };
    let bars = bar_chart(speeds, tint)
        .height(FAN_HEIGHT)
        .readout(move |runtime, index, rpm| {
            let label = live.with_untracked(runtime, |snapshot| {
                snapshot
                    .tick
                    .sensors
                    .fans
                    .get(index)
                    .map_or_else(String::new, |fan| fan.identity.label.to_string())
            });
            format!("{label} · {} rpm", count(rpm.max(0.0) as u64))
        });
    let controllable = live.with_untracked(cx, |snapshot| {
        snapshot
            .tick
            .sensors
            .fans
            .iter()
            .any(|fan| fan.identity.control == FanControlSupport::Pwm)
    });
    let control = controllable.then(|| fan_control(cx, live, toasts));
    vstack((bars, control)).gap(SECTION_GAP).w_full().boxed()
}

/// Temperatures with Tjmax, per-sensor readings, throttle counters and fans.
pub fn sensors_panel(state: AppState, toasts: Toasts) -> impl View + use<> {
    let live = state.live;
    let temperatures = keyed(
        move |runtime| live.with(runtime, sensor_keys),
        move |cx, _| -> AnyView {
            let sensors =
                live.with_untracked(cx, |snapshot| cpu_sensors(&snapshot.tick.sensors).count());
            if sensors == 0 {
                return empty_state(
                    icons::THERMOMETER,
                    NOT_AVAILABLE,
                    "No CPU temperature sensor was found under /sys/class/hwmon.",
                )
                .boxed();
            }
            vstack((
                temperature_chart(cx, state),
                sensor_readings(cx, state, sensors),
            ))
            .gap(SECTION_GAP)
            .w_full()
            .boxed()
        },
    );
    let fan_section = keyed(
        move |runtime| live.with(runtime, fan_keys),
        move |cx, _| {
            let count = live.with_untracked(cx, |snapshot| snapshot.tick.sensors.fans.len());
            fans(cx, state, count, toasts)
        },
    );
    vstack((
        temperatures,
        fact_grid(vec![throttle_fact(state)]),
        fan_section,
    ))
    .gap(SECTION_GAP)
    .w_full()
    .label("Sensors and fans")
}
