use crownui::kit::{Segment, SegmentedBarOptions, Theme, segmented_bar, tag_chip};
use crownui::prelude::{
    Color, Cx, Memo, Runtime, Signal, Styled, View, bind, hstack, text, vstack,
};
use monitor_engine::hardware::GpuDevice;

use super::super::sources::window_mean_peak;
use super::describe::{
    NOT_REPORTED, clock, describe_link, describe_performance, describe_throttle, fan, memory_line,
    memory_weights, power_draw, power_state_label, temperatures,
};
use super::selection::GpuChoice;
use crate::app::AppState;
use crate::app::live::LiveSnapshot;
use crate::components::fact_grid::{Fact, fact_grid};
use crate::util::format::{percent, rate};
use crate::util::theme::Subsystem;

const BAR_HEIGHT: f32 = 10.0;
const LEGEND_SIZE: f32 = 12.0;
const LEGEND_GAP: f32 = 6.0;
const BAR_GAP: f32 = 8.0;
/// How far free VRAM fades from the GPU tint towards the track.
const FREE_VRAM_FADE: f32 = 0.75;

/// Which figure of the chart window a fact shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WindowFigure {
    Mean,
    Peak,
}

fn device_fact(
    state: AppState,
    choice: GpuChoice,
    label: &'static str,
    describe: fn(&GpuDevice, &mut Runtime, AppState) -> String,
) -> Fact {
    let live = state.live;
    Fact::new(
        label,
        bind(move |runtime: &mut Runtime| {
            let device = choice.with(runtime, live, |device| device.cloned());
            device.map_or_else(
                || NOT_REPORTED.to_owned(),
                |device| describe(&device, runtime, state),
            )
        }),
    )
}

fn window_fact(
    state: AppState,
    choice: GpuChoice,
    label: &'static str,
    figure: WindowFigure,
) -> Fact {
    let (live, window) = (state.live, state.chart_window);
    Fact::new(
        label,
        bind(move |runtime: &mut Runtime| {
            let samples = window.get(runtime).seconds();
            let index = choice.index.get(runtime);
            live.with(runtime, |snapshot: &LiveSnapshot| {
                let (mean, highest) = snapshot
                    .gpu
                    .devices
                    .get(index)
                    .map_or((f32::NAN, f32::NAN), |history| {
                        window_mean_peak(&history.busy, samples)
                    });
                percent(match figure {
                    WindowFigure::Mean => mean,
                    WindowFigure::Peak => highest,
                })
            })
        }),
    )
}

/// Clocks, temperatures, power, fan, PCIe link, performance level and throttle reasons, with
/// the window's average and peak.
pub fn live_facts(_: &mut Cx, state: AppState, choice: GpuChoice) -> impl View + use<> {
    fact_grid(vec![
        device_fact(state, choice, "Utilization", |device, _, _| {
            device
                .metrics
                .utilization
                .map_or_else(|| NOT_REPORTED.to_owned(), |busy| percent(busy.get()))
        }),
        window_fact(state, choice, "Average in window", WindowFigure::Mean),
        window_fact(state, choice, "Peak in window", WindowFigure::Peak),
        device_fact(state, choice, "Core clock", |device, _, _| {
            let clocks = device.metrics.clocks;
            clock(clocks.core, clocks.core_max)
        }),
        device_fact(state, choice, "Memory clock", |device, _, _| {
            let clocks = device.metrics.clocks;
            clock(clocks.memory, clocks.memory_max)
        }),
        device_fact(
            state,
            choice,
            "Edge · junction · memory",
            |device, runtime, state| temperatures(device, state.settings.get(runtime).temperature),
        ),
        device_fact(state, choice, "Power", |device, _, _| power_draw(device)),
        device_fact(state, choice, "Fan", |device, _, _| fan(device)),
        device_fact(state, choice, "PCIe link", |device, _, _| {
            describe_link(&device.metrics.pcie)
        }),
        device_fact(
            state,
            choice,
            "PCIe transmit · receive",
            |device, runtime, state| {
                let link = device.metrics.pcie;
                let unit = state.settings.get(runtime).rates;
                match (link.transmit, link.receive) {
                    (Some(transmit), Some(receive)) => {
                        format!(
                            "{} · {}",
                            rate(transmit.get(), unit),
                            rate(receive.get(), unit)
                        )
                    }
                    _ => NOT_REPORTED.to_owned(),
                }
            },
        ),
        device_fact(state, choice, "Performance level", |device, _, _| {
            describe_performance(device.metrics.performance)
        }),
        device_fact(state, choice, "Clock limits", |device, _, _| {
            describe_throttle(device.metrics.clock_events)
        }),
        device_fact(state, choice, "Power state", |device, _, _| {
            power_state_label(device.metrics.power_state).to_owned()
        }),
    ])
}

/// VRAM and GTT in one stacked bar, with the exact figures below.
pub fn memory_bar(cx: &mut Cx, state: AppState, choice: GpuChoice) -> impl View + use<> {
    let live = state.live;
    let theme: Signal<Theme> = state.theme;
    let vram: Memo<Color> = state.tint(cx, Subsystem::Gpu);
    let gtt: Memo<Color> = state.tint(cx, Subsystem::Memory);
    let bar = segmented_bar([])
        .h(BAR_HEIGHT)
        .segments_from(move |runtime| {
            let colors = [vram.get(runtime), gtt.get(runtime)];
            let track = theme.with(runtime, |theme| theme.control.track);
            let weights = choice.with(runtime, live, |device| {
                device.map_or([0.0; 4], |device| memory_weights(&device.metrics.memory))
            });
            vec![
                Segment::new(weights[0], colors[0]),
                Segment::new(weights[1], colors[0].lerp(track, FREE_VRAM_FADE)),
                Segment::new(weights[2], colors[1]),
                Segment::new(weights[3], track),
            ]
        });
    let figures = text(bind(move |runtime| {
        choice.with(runtime, live, |device| {
            device.map_or_else(String::new, |device| memory_line(&device.metrics.memory))
        })
    }))
    .text_size(LEGEND_SIZE);
    vstack((
        bar,
        hstack((tag_chip("VRAM", vram), tag_chip("GTT", gtt))).gap(LEGEND_GAP),
        figures,
    ))
    .gap(BAR_GAP)
    .w_full()
}

/// Name, vendor and driver.
pub fn hardware_facts(_: &mut Cx, state: AppState, choice: GpuChoice) -> impl View + use<> {
    fact_grid(vec![
        device_fact(state, choice, "Name", |device, _, _| {
            device.identity.name.to_string()
        }),
        device_fact(state, choice, "Vendor", |device, _, _| {
            device
                .identity
                .vendor
                .short_name()
                .unwrap_or(NOT_REPORTED)
                .to_owned()
        }),
        device_fact(state, choice, "Driver", |device, _, _| {
            let identity = &device.identity;
            match &identity.driver_version {
                Some(version) => format!("{} {version}", identity.driver),
                None => identity.driver.to_string(),
            }
        }),
    ])
}
