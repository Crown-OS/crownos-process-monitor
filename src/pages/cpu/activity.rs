use crownui::kit::icons;
use crownui::prelude::{Color, Cx, Memo, Runtime, Signal, Styled, View, bind, vstack};
use monitor_core::{Coverage, Percent};
use monitor_engine::Snapshot;
use monitor_engine::system::PressureAverages;

use super::super::sources::windowed;
use crate::app::AppState;
use crate::app::live::LiveSnapshot;
use crate::components::fact_grid::{Fact, fact_grid};
use crate::components::stat_card::{StatCardContent, stat_tile};
use crate::components::stat_strip::Figure;
use crate::util::format::{count, duration, frequency, percent, power};
use crate::util::spacing;
use crate::util::theme::Subsystem;

const NOT_REPORTED: &str = "–";
const CARD_COLUMNS: u16 = 2;

fn tick_fact(
    live: Signal<LiveSnapshot>,
    label: &'static str,
    describe: fn(&Snapshot) -> String,
) -> Fact {
    Fact::new(
        label,
        bind(move |runtime: &mut Runtime| live.with(runtime, |snapshot| describe(&snapshot.tick))),
    )
}

fn text_or_dash(value: Option<&str>) -> String {
    value.unwrap_or(NOT_REPORTED).to_owned()
}

fn per_second(value: f32) -> String {
    format!("{}/s", count(value.max(0.0).round() as u64))
}

fn megahertz(value: Option<monitor_core::Megahertz>) -> String {
    frequency(value.map_or(f32::NAN, |mhz| mhz.get() as f32))
}

/// Governor, EPP, driver, boost and power profile, with the current clock spread.
pub fn scaling_facts(_: &mut Cx, state: AppState) -> impl View + use<> {
    let live = state.live;
    fact_grid(vec![
        tick_fact(live, "Governor", |tick| {
            text_or_dash(tick.cpu.policy.governor.as_deref())
        }),
        tick_fact(live, "Energy preference", |tick| {
            text_or_dash(tick.cpu.policy.energy_preference.as_deref())
        }),
        tick_fact(live, "Scaling driver", |tick| {
            text_or_dash(tick.cpu.policy.driver.as_deref())
        }),
        tick_fact(live, "Boost", |tick| {
            match tick.cpu.policy.boost {
                Some(true) => "On",
                Some(false) => "Off",
                None => NOT_REPORTED,
            }
            .to_owned()
        }),
        tick_fact(live, "Power profile", |tick| {
            let power = &tick.power;
            let profile = power.daemon_profile.as_deref().or_else(|| {
                power
                    .platform_profile
                    .as_ref()
                    .map(|profile| &*profile.active)
            });
            text_or_dash(profile)
        }),
        tick_fact(live, "Average clock", |tick| {
            megahertz(tick.cpu.frequency.average)
        }),
        tick_fact(live, "Slowest · fastest core", |tick| {
            let spread = &tick.cpu.frequency;
            format!("{} · {}", megahertz(spread.min), megahertz(spread.max))
        }),
        tick_fact(live, "Base · max clock", |tick| {
            let info = &tick.cpu_info;
            format!(
                "{} · {}",
                megahertz(info.base_frequency),
                megahertz(info.max_frequency)
            )
        }),
        tick_fact(live, "Package power", |tick| {
            let supply = &tick.power;
            let package = supply.rapl.iter().find(|zone| zone.is_package);
            match (package, supply.rapl_coverage) {
                (Some(zone), _) => power(zone.watts.get()),
                (None, Coverage::Unavailable) => "Needs the helper (RAPL is root-only)".to_owned(),
                (None, _) => NOT_REPORTED.to_owned(),
            }
        }),
    ])
}

fn live_text(
    live: Signal<LiveSnapshot>,
    describe: fn(&LiveSnapshot) -> String,
) -> impl Fn(&mut Runtime) -> String + 'static {
    move |runtime| live.with(runtime, describe)
}

fn load_card(state: AppState, tint: Memo<Color>) -> impl View {
    let live = state.live;
    let content = StatCardContent::new(
        "Load",
        icons::GAUGE,
        tint,
        bind(live_text(live, |snapshot| {
            format!("{:.2}", snapshot.cpu.load_average[0])
        })),
    )
    .figure(Figure::new(
        "5 min",
        bind(live_text(live, |snapshot| {
            format!("{:.2}", snapshot.cpu.load_average[1])
        })),
    ))
    .figure(Figure::new(
        "15 min",
        bind(live_text(live, |snapshot| {
            format!("{:.2}", snapshot.cpu.load_average[2])
        })),
    ))
    .figure(
        Figure::new(
            "Per CPU",
            bind(live_text(live, |snapshot| {
                format!("{:.2}", load_per_cpu(snapshot))
            })),
        )
        .strain(bind(move |runtime| {
            live.with(runtime, |snapshot| load_per_cpu(snapshot) > 1.0)
        })),
    );
    stat_tile(content, windowed(state, |snapshot| &snapshot.cpu.load))
}

fn load_per_cpu(snapshot: &LiveSnapshot) -> f32 {
    snapshot.cpu.load_average[0] / snapshot.tick.cpu.logical.len().max(1) as f32
}

fn pressure_card(state: AppState, tint: Memo<Color>) -> impl View {
    let live = state.live;
    let average = |pick: fn(&PressureAverages) -> Percent| {
        move |tick: &Snapshot| {
            tick.cpu.pressure.map_or_else(
                || NOT_REPORTED.to_owned(),
                |pressure| percent(pick(&pressure).get()),
            )
        }
    };
    let content = StatCardContent::new(
        "Pressure",
        icons::ACTIVITY,
        tint,
        bind(live_text(live, |snapshot| {
            snapshot.tick.cpu.pressure.map_or_else(
                || "PSI off".to_owned(),
                |pressure| percent(pressure.avg10.get()),
            )
        })),
    )
    .figure(tick_fact_figure(
        live,
        "60 s",
        average(|pressure| pressure.avg60),
    ))
    .figure(tick_fact_figure(
        live,
        "300 s",
        average(|pressure| pressure.avg300),
    ));
    stat_tile(content, windowed(state, |snapshot| &snapshot.cpu.pressure))
}

fn tick_fact_figure(
    live: Signal<LiveSnapshot>,
    label: &'static str,
    describe: impl Fn(&Snapshot) -> String + 'static,
) -> Figure {
    Figure::new(
        label,
        bind(move |runtime: &mut Runtime| live.with(runtime, |snapshot| describe(&snapshot.tick))),
    )
}

fn scheduler_card(state: AppState, tint: Memo<Color>) -> impl View {
    let live = state.live;
    let content = StatCardContent::new(
        "Context switches",
        icons::SHUFFLE,
        tint,
        bind(live_text(live, |snapshot| {
            per_second(snapshot.tick.cpu.events.context_switches)
        })),
    )
    .figure(tick_fact_figure(live, "Interrupts", |tick| {
        per_second(tick.cpu.events.interrupts)
    }))
    .figure(tick_fact_figure(live, "Soft interrupts", |tick| {
        per_second(tick.cpu.events.soft_interrupts)
    }))
    .figure(tick_fact_figure(live, "Forks", |tick| {
        per_second(tick.cpu.events.forks)
    }));
    stat_tile(
        content,
        windowed(state, |snapshot| &snapshot.cpu.context_switches_per_second),
    )
}

fn tasks_card(state: AppState, tint: Memo<Color>) -> impl View {
    let live = state.live;
    let content = StatCardContent::new(
        "Running tasks",
        icons::LIST_TREE,
        tint,
        bind(live_text(live, |snapshot| {
            count(u64::from(snapshot.tick.cpu.procs_running))
        })),
    )
    .figure(tick_fact_figure(live, "Blocked", |tick| {
        count(u64::from(tick.cpu.procs_blocked))
    }))
    .figure(tick_fact_figure(live, "Processes", |tick| {
        count(tick.processes.processes.len() as u64)
    }))
    .figure(tick_fact_figure(live, "Threads", |tick| {
        count(u64::from(tick.cpu.threads.max(tick.processes.thread_count)))
    }))
    .figure(tick_fact_figure(live, "Uptime", |tick| {
        duration(tick.cpu.uptime.as_secs())
    }));
    stat_tile(
        content,
        windowed(state, |snapshot| &snapshot.cpu.running_tasks),
    )
}

/// Load, pressure, scheduler rates and tasks as small cards with their recent history.
pub fn activity_cards(cx: &mut Cx, state: AppState) -> impl View + use<> {
    let tint = state.tint(cx, Subsystem::Cpu);
    vstack((
        load_card(state, tint),
        pressure_card(state, tint),
        scheduler_card(state, tint),
        tasks_card(state, tint),
    ))
    .grid()
    .grid_cols(CARD_COLUMNS)
    .gap(spacing::LOOSE)
    .w_full()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rates_round_and_never_go_negative() {
        assert_eq!(per_second(1234.4), "1,234/s");
        assert_eq!(per_second(-3.0), "0/s");
        assert_eq!(text_or_dash(None), NOT_REPORTED);
    }
}
