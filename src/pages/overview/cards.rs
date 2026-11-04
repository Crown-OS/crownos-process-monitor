//! The CPU, Memory, Disk and Network cards.

use crownui::kit::icons;
use crownui::prelude::{AnyView, Cx, Runtime, Signal, View, bind};
use monitor_engine::history::Totals;

use super::super::sources::windowed;
use super::readout::{live_flag, pressure_label, reading};
use crate::app::history_query::{HistoryData, UsageTotals};
use crate::app::live::{LiveSnapshot, latest};
use crate::app::{AppState, Navigation, Page};
use crate::components::stat_card::{StatCardContent, stat_card};
use crate::components::stat_strip::Figure;
use crate::util::format::{bytes, percent, rate};
use crate::util::theme::Subsystem;

pub fn open(navigation: Navigation, page: Page) -> impl Fn(&mut Cx) + 'static {
    move |cx| navigation.show(cx, page)
}

/// "1.20 GB" of a history total, or "–" while history is loading or missing.
fn total_text(
    totals: Signal<HistoryData<UsageTotals>>,
    pick: fn(&UsageTotals) -> u64,
) -> impl Fn(&mut Runtime) -> String + 'static {
    move |runtime| {
        totals.with(runtime, |totals| {
            totals
                .ready()
                .map_or_else(|| "–".to_owned(), |totals| bytes(pick(totals)))
        })
    }
}

fn network_bytes(totals: &Totals) -> u64 {
    let combined = totals.combined();
    (combined.network_received + combined.network_transmitted).get()
}

pub fn cpu_card(cx: &mut Cx, state: AppState, navigation: Navigation) -> AnyView {
    let content = StatCardContent::new(
        "CPU",
        icons::CPU,
        state.tint(cx, Subsystem::Cpu),
        reading(state, |snapshot, _| percent(latest(&snapshot.cpu.total))),
    )
    .figure(
        Figure::new(
            "User",
            reading(state, |snapshot, _| percent(latest(&snapshot.cpu.user))),
        )
        .strain(live_flag(state, |snapshot| snapshot.cpu.busy_strained)),
    )
    .figure(Figure::new(
        "System",
        reading(state, |snapshot, _| percent(latest(&snapshot.cpu.system))),
    ))
    .figure(Figure::new(
        "Avg today",
        reading(state, |snapshot, _| {
            percent(snapshot.cpu.average_today.unwrap_or(f32::NAN))
        }),
    ));
    stat_card(
        content,
        windowed(state, |snapshot| &snapshot.cpu.total),
        open(navigation, Page::Cpu),
    )
    .boxed()
}

pub fn memory_card(cx: &mut Cx, state: AppState, navigation: Navigation) -> AnyView {
    let category =
        |pick: fn(&LiveSnapshot) -> u64| reading(state, move |snapshot, _| bytes(pick(snapshot)));
    let content = StatCardContent::new(
        "Memory",
        icons::MEMORY_STICK,
        state.tint(cx, Subsystem::Memory),
        reading(state, |snapshot, _| {
            format!(
                "{} of {}",
                bytes(snapshot.memory.used_bytes),
                bytes(snapshot.memory.total_bytes)
            )
        }),
    )
    .figure(
        Figure::new(
            "Pressure",
            reading(state, |snapshot, _| {
                pressure_label(snapshot.tick.memory.pressure_level).to_owned()
            }),
        )
        .strain(live_flag(state, |snapshot| snapshot.memory.strained)),
    )
    .figure(Figure::new(
        "Apps",
        category(|snapshot| {
            let categories = &snapshot.tick.memory.categories;
            (categories.apps + categories.shared).get()
        }),
    ))
    .figure(Figure::new(
        "Kernel",
        category(|snapshot| snapshot.tick.memory.categories.kernel.get()),
    ))
    .figure(Figure::new(
        "Compressed",
        category(|snapshot| snapshot.tick.memory.categories.compressed.get()),
    ));
    stat_card(
        content,
        windowed(state, |snapshot| &snapshot.memory.used_percent),
        open(navigation, Page::Memory),
    )
    .boxed()
}

pub fn disk_card(
    cx: &mut Cx,
    state: AppState,
    navigation: Navigation,
    totals: Signal<HistoryData<UsageTotals>>,
) -> AnyView {
    let content = StatCardContent::new(
        "Disk",
        icons::HARD_DRIVE,
        state.tint(cx, Subsystem::Disk),
        reading(state, |snapshot, _| {
            let disk = &snapshot.disk;
            format!(
                "{} free of {}",
                bytes(disk.total_bytes.saturating_sub(disk.used_bytes)),
                bytes(disk.total_bytes)
            )
        }),
    )
    .figure(
        Figure::new(
            "Read",
            reading(state, |snapshot, units| {
                rate(f64::from(latest(&snapshot.disk.read)), units.rates)
            }),
        )
        .strain(live_flag(state, |snapshot| {
            snapshot.tick.strain.disk.is_strained()
        })),
    )
    .figure(Figure::new(
        "Write",
        reading(state, |snapshot, units| {
            rate(f64::from(latest(&snapshot.disk.write)), units.rates)
        }),
    ))
    .figure(Figure::new(
        "Written today",
        bind(total_text(totals, |totals| {
            totals.today.combined().disk_written.get()
        })),
    ));
    stat_card(
        content,
        windowed(state, |snapshot| &snapshot.disk.write),
        open(navigation, Page::Disk),
    )
    .boxed()
}

pub fn network_card(
    cx: &mut Cx,
    state: AppState,
    navigation: Navigation,
    totals: Signal<HistoryData<UsageTotals>>,
) -> AnyView {
    let content = StatCardContent::new(
        "Network",
        icons::NETWORK,
        state.tint(cx, Subsystem::Network),
        reading(state, |snapshot, units| {
            rate(f64::from(latest(&snapshot.network.download)), units.rates)
        }),
    )
    .figure(
        Figure::new(
            "Up",
            reading(state, |snapshot, units| {
                rate(f64::from(latest(&snapshot.network.upload)), units.rates)
            }),
        )
        .strain(live_flag(state, |snapshot| {
            snapshot.tick.strain.network.is_strained()
        })),
    )
    .figure(Figure::new(
        "Today",
        bind(total_text(totals, |totals| network_bytes(&totals.today))),
    ))
    .figure(Figure::new(
        "7 days",
        bind(total_text(totals, |totals| {
            network_bytes(&totals.last_7_days)
        })),
    ));
    stat_card(
        content,
        windowed(state, |snapshot| &snapshot.network.download),
        open(navigation, Page::Network),
    )
    .boxed()
}
