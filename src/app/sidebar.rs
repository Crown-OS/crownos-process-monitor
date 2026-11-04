use crownui::kit::{SidebarItemOptions, sidebar_item, sidebar_section};
use crownui::prelude::{Cx, Listeners, Memo, View, bind};

use super::live::{LiveSnapshot, latest};
use super::navigation::{Navigation, Page};
use super::state::AppState;
use crate::util::format::{write_count, write_percent, write_rate};
use crate::util::motion::instant;
use crate::util::shortcut::ctrl_digit;

const SECTION_TITLE: &str = "Monitor";

/// The six page rows, each with its live figure trailing; Ctrl+1…6 opens them.
pub fn sidebar_menus(cx: &mut Cx, navigation: Navigation, state: AppState) -> impl View + use<> {
    let current = navigation.current();
    let items: Vec<_> = Page::ALL
        .into_iter()
        .map(|page| {
            let index = page.index();
            let item = sidebar_item(
                page.title(),
                page.icon(),
                bind(move |runtime| current.get(runtime) == index),
            )
            .trailing_text(trailing_value(cx, state, page))
            .on_click(move |cx, _| navigation.show(cx, page));
            match ctrl_digit(index) {
                Some(keybind) => item.shortcut(keybind),
                None => item,
            }
        })
        .collect();
    sidebar_section(SECTION_TITLE, items).transition(instant())
}

fn trailing_value(cx: &mut Cx, state: AppState, page: Page) -> Memo<String> {
    let live = state.live;
    let settings = state.settings;
    cx.memo(move |runtime| {
        let rates = settings.get(runtime).rates;
        live.with(runtime, |snapshot| describe(snapshot, page, rates))
    })
}

fn describe(snapshot: &LiveSnapshot, page: Page, rates: crate::util::format::RateUnit) -> String {
    let mut out = String::new();
    match page {
        Page::Overview => write_count(&mut out, snapshot.apps.apps.len() as u64),
        Page::Cpu => write_percent(&mut out, latest(&snapshot.cpu.total)),
        Page::Gpu => write_percent(&mut out, latest(&snapshot.gpu.busy)),
        Page::Memory => write_percent(&mut out, latest(&snapshot.memory.used_percent)),
        Page::Disk => {
            let total = latest(&snapshot.disk.read) + latest(&snapshot.disk.write);
            write_rate(&mut out, f64::from(total), rates);
        }
        Page::Network => write_rate(
            &mut out,
            f64::from(latest(&snapshot.network.download)),
            rates,
        ),
    }
    out
}
