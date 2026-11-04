//! The GPU (one card per device), Power, Sensors and Accelerator cards.

use crownui::kit::icons;
use crownui::prelude::{AnyView, Cx, View, bind};
use monitor_core::Coverage;
use monitor_engine::Snapshot;
use monitor_engine::hardware::{BatteryStatus, GpuPower, PowerSource};

use super::cards::open;
use super::readout::{live_flag, reading, window_mean_and_peak};
use crate::app::live::{LiveSnapshot, recent};
use crate::app::{AppState, Navigation, Page};
use crate::components::chart::SeriesSource;
use crate::components::stat_card::{StatCardContent, stat_card};
use crate::components::stat_strip::Figure;
use crate::util::format::{bytes, count, duration, percent, power, temperature};
use crate::util::theme::Subsystem;

const GPU_TITLES: [&str; 4] = ["GPU", "GPU 2", "GPU 3", "GPU 4"];
const ACCELERATOR_TITLES: [&str; 2] = ["Accelerator", "Accelerator 2"];
pub const MAX_GPU_CARDS: usize = GPU_TITLES.len();
pub const MAX_ACCELERATOR_CARDS: usize = ACCELERATOR_TITLES.len();
const UNKNOWN: &str = "–";

/// A sparkline over `pick`'s history, cut to the chart window.
fn sparkline(
    state: AppState,
    pick: impl Fn(&LiveSnapshot) -> Option<&[f32]> + Clone + 'static,
) -> impl SeriesSource {
    let (live, window) = (state.live, state.chart_window);
    move |runtime, sink| {
        let samples = window.get(runtime).seconds();
        live.with(runtime, |snapshot| {
            sink(pick(snapshot).map_or(&[][..], |history| recent(history, samples)));
        });
    }
}

fn gpu_busy(snapshot: &LiveSnapshot, index: usize) -> Option<&[f32]> {
    snapshot
        .gpu
        .devices
        .get(index)
        .map(|device| device.busy.as_slice())
}

pub fn gpu_card(cx: &mut Cx, state: AppState, navigation: Navigation, index: usize) -> AnyView {
    let name = state.live.with_untracked(cx, |snapshot| {
        snapshot.tick.gpu.devices.get(index).map_or_else(
            || "GPU".to_owned(),
            |device| device.identity.name.to_string(),
        )
    });
    let (live, window) = (state.live, state.chart_window);
    let statistic = move |pick_peak: bool| {
        bind(move |runtime| {
            let samples = window.get(runtime).seconds();
            live.with(runtime, |snapshot| {
                let (mean, peak) = gpu_busy(snapshot, index).map_or((f32::NAN, f32::NAN), |busy| {
                    window_mean_and_peak(busy, samples)
                });
                percent(if pick_peak { peak } else { mean })
            })
        })
    };
    let content = StatCardContent::new(
        GPU_TITLES[index.min(MAX_GPU_CARDS - 1)],
        icons::GPU,
        state.tint(cx, Subsystem::Gpu),
        reading(state, move |snapshot, _| {
            let Some(device) = snapshot.tick.gpu.devices.get(index) else {
                return UNKNOWN.to_owned();
            };
            match (device.metrics.power_state, device.metrics.utilization) {
                (GpuPower::Asleep, _) => "Asleep".to_owned(),
                (_, Some(busy)) => percent(busy.get()),
                (_, None) => UNKNOWN.to_owned(),
            }
        }),
    )
    .figure(
        Figure::new(
            name,
            reading(state, move |snapshot, _| {
                let memory = snapshot
                    .tick
                    .gpu
                    .devices
                    .get(index)
                    .map(|device| device.metrics.memory);
                memory.and_then(|memory| memory.vram_used).map_or_else(
                    || UNKNOWN.to_owned(),
                    |used| format!("{} VRAM", bytes(used.get())),
                )
            }),
        )
        .strain(live_flag(state, |snapshot| snapshot.gpu.strained)),
    )
    .figure(Figure::new("Average", statistic(false)))
    .figure(Figure::new("Peak", statistic(true)));
    stat_card(
        content,
        sparkline(state, move |snapshot| gpu_busy(snapshot, index)),
        open(navigation, Page::Gpu),
    )
    .boxed()
}

/// Battery discharge, else RAPL plus GPUs, with "est." when estimated.
fn draw_text(snapshot: &Snapshot) -> String {
    match snapshot.system_draw.or(snapshot.power.system_draw) {
        Some(draw) if draw.coverage == Coverage::Estimated => {
            format!("{} est.", power(draw.watts.get()))
        }
        Some(draw) => power(draw.watts.get()),
        None => "Unknown".to_owned(),
    }
}

fn source_text(snapshot: &Snapshot) -> &'static str {
    let charging = snapshot
        .power
        .system_battery()
        .map(|battery| battery.status);
    match (snapshot.power.source, charging) {
        (_, Some(BatteryStatus::Charging)) => "Charging",
        (PowerSource::Ac, _) => "On AC",
        (PowerSource::Battery, _) => "On battery",
        (PowerSource::Unknown, _) => UNKNOWN,
    }
}

fn remaining_text(snapshot: &Snapshot) -> String {
    let battery = snapshot.power.system_battery();
    let remaining = battery.and_then(|battery| match battery.status {
        BatteryStatus::Charging => battery.time_to_full,
        _ => battery.time_to_empty,
    });
    remaining.map_or_else(|| UNKNOWN.to_owned(), |left| duration(left.as_secs()))
}

pub fn power_card(cx: &mut Cx, state: AppState, navigation: Navigation) -> AnyView {
    let battery_figure =
        |read: fn(&Snapshot) -> String| reading(state, move |snapshot, _| read(&snapshot.tick));
    let content = StatCardContent::new(
        "Power",
        icons::ZAP,
        state.tint(cx, Subsystem::Power),
        reading(state, |snapshot, _| draw_text(&snapshot.tick)),
    )
    .figure(Figure::new(
        "Battery",
        battery_figure(|snapshot| {
            snapshot
                .power
                .system_battery()
                .and_then(|battery| battery.capacity)
                .map_or_else(|| "None".to_owned(), |capacity| percent(capacity.get()))
        }),
    ))
    .figure(Figure::new("Remaining", battery_figure(remaining_text)))
    .figure(Figure::new(
        "Health",
        battery_figure(|snapshot| {
            snapshot
                .power
                .system_battery()
                .and_then(|battery| battery.health)
                .map_or_else(|| UNKNOWN.to_owned(), |health| percent(health.get()))
        }),
    ))
    .figure(Figure::new(
        "Source",
        battery_figure(|snapshot| source_text(snapshot).to_owned()),
    ));
    stat_card(
        content,
        sparkline(state, |snapshot| {
            Some(snapshot.overview.power_draw.as_slice())
        }),
        open(navigation, Page::Cpu),
    )
    .boxed()
}

fn gpu_celsius(snapshot: &Snapshot) -> f32 {
    snapshot
        .gpu
        .devices
        .iter()
        .filter_map(|device| device.metrics.temperatures.primary())
        .map(|celsius| celsius.get())
        .fold(f32::NAN, f32::max)
}

fn fastest_fan(snapshot: &Snapshot) -> Option<u32> {
    let board = snapshot.sensors.fans.iter().filter_map(|fan| fan.rpm);
    let gpus = snapshot
        .gpu
        .devices
        .iter()
        .filter_map(|device| device.metrics.fan.rpm);
    board.chain(gpus).map(|rpm| rpm.get()).max()
}

pub fn sensors_card(cx: &mut Cx, state: AppState, navigation: Navigation) -> AnyView {
    let content = StatCardContent::new(
        "Sensors",
        icons::THERMOMETER,
        state.tint(cx, Subsystem::Temperature),
        reading(state, |snapshot, units| {
            let hottest = snapshot
                .tick
                .sensors
                .hottest_sensor()
                .and_then(|reading| reading.value);
            hottest.map_or_else(
                || UNKNOWN.to_owned(),
                |celsius| temperature(celsius.get(), units.temperature),
            )
        }),
    )
    .figure(
        Figure::new(
            "Hottest",
            reading(state, |snapshot, _| {
                snapshot.tick.sensors.hottest_sensor().map_or_else(
                    || UNKNOWN.to_owned(),
                    |reading| format!("{} {}", reading.identity.chip, reading.identity.label),
                )
            }),
        )
        .strain(live_flag(state, |snapshot| {
            snapshot.tick.strain.temperature.is_strained()
        })),
    )
    .figure(Figure::new(
        "CPU",
        reading(state, |snapshot, units| {
            let celsius = snapshot
                .tick
                .sensors
                .cpu_temperature
                .map_or(f32::NAN, |celsius| celsius.get());
            temperature(celsius, units.temperature)
        }),
    ))
    .figure(Figure::new(
        "GPU",
        reading(state, |snapshot, units| {
            temperature(gpu_celsius(&snapshot.tick), units.temperature)
        }),
    ))
    .figure(Figure::new(
        "Fan",
        reading(state, |snapshot, _| {
            fastest_fan(&snapshot.tick).map_or_else(
                || UNKNOWN.to_owned(),
                |rpm| format!("{} rpm", count(u64::from(rpm))),
            )
        }),
    ));
    stat_card(
        content,
        sparkline(state, |snapshot| Some(snapshot.overview.hottest.as_slice())),
        open(navigation, Page::Cpu),
    )
    .boxed()
}

pub fn accelerator_card(
    cx: &mut Cx,
    state: AppState,
    navigation: Navigation,
    index: usize,
) -> AnyView {
    let (name, driver) = state.live.with_untracked(cx, |snapshot| {
        snapshot.tick.gpu.accelerators.get(index).map_or_else(
            || ("NPU".to_owned(), String::new()),
            |accelerator| (accelerator.name.to_string(), accelerator.driver.to_string()),
        )
    });
    let content = StatCardContent::new(
        ACCELERATOR_TITLES[index.min(MAX_ACCELERATOR_CARDS - 1)],
        icons::BRAIN,
        state.tint(cx, Subsystem::Gpu),
        reading(state, move |snapshot, _| {
            let Some(accelerator) = snapshot.tick.gpu.accelerators.get(index) else {
                return UNKNOWN.to_owned();
            };
            match (accelerator.power_state, accelerator.busy) {
                (GpuPower::Asleep, _) => "Asleep".to_owned(),
                (_, Some(busy)) => percent(busy.get()),
                (_, None) => "Idle".to_owned(),
            }
        }),
    )
    .figure(Figure::new(name, driver));
    stat_card(
        content,
        sparkline(state, move |snapshot| {
            snapshot
                .overview
                .accelerators
                .get(index)
                .map(|history| history.busy.as_slice())
        }),
        open(navigation, Page::Gpu),
    )
    .boxed()
}

/// Cards for the hardware this machine has, in grid order.
pub fn hardware_cards(
    cx: &mut Cx,
    state: AppState,
    navigation: Navigation,
    gpus: usize,
    accelerators: usize,
) -> Vec<AnyView> {
    let mut cards: Vec<AnyView> = (0..gpus.min(MAX_GPU_CARDS))
        .map(|index| gpu_card(cx, state, navigation, index))
        .collect();
    cards.push(power_card(cx, state, navigation));
    cards.push(sensors_card(cx, state, navigation));
    cards.extend(
        (0..accelerators.min(MAX_ACCELERATOR_CARDS))
            .map(|index| accelerator_card(cx, state, navigation, index)),
    );
    cards
}

#[cfg(test)]
mod tests {
    use monitor_core::Watts;
    use monitor_engine::hardware::SystemDraw;

    use super::*;

    #[test]
    fn power_draw_says_when_it_is_estimated() {
        let mut snapshot = Snapshot::default();
        assert_eq!(draw_text(&snapshot), "Unknown");
        snapshot.system_draw = Some(SystemDraw {
            watts: Watts::new(14.0),
            coverage: Coverage::Estimated,
        });
        assert_eq!(draw_text(&snapshot), "14 W est.");
        assert_eq!(source_text(&snapshot), UNKNOWN);
        assert_eq!(remaining_text(&snapshot), UNKNOWN);
    }
}
