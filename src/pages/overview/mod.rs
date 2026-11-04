//! The Overview: every subsystem at a glance.

mod cards;
#[cfg(test)]
pub(super) mod fixtures;
mod hardware;
mod readout;
#[cfg(test)]
mod tests;

use crownui::prelude::{Cx, Styled, View, keyed, vstack};

use self::cards::{cpu_card, disk_card, memory_card, network_card};
use super::frame::page_frame;
use crate::app::history_query::history_totals;
use crate::app::{AppState, Navigation};

const GRID_GAP: f32 = 12.0;
const CARD_COLUMNS: u16 = 3;

pub fn page(cx: &mut Cx, state: AppState, navigation: Navigation) -> impl View + use<> {
    let live = state.live;
    let (totals, totals_driver) = history_totals(cx);
    let cards = keyed(
        move |runtime| {
            live.with(runtime, |snapshot| {
                let gpu = &snapshot.tick.gpu;
                (gpu.devices.len(), gpu.accelerators.len())
            })
        },
        move |cx, (gpus, accelerators)| {
            let mut cards = vec![
                cpu_card(cx, state, navigation),
                memory_card(cx, state, navigation),
                disk_card(cx, state, navigation, totals),
                network_card(cx, state, navigation, totals),
            ];
            cards.extend(hardware::hardware_cards(
                cx,
                state,
                navigation,
                gpus,
                accelerators,
            ));
            vstack(cards)
                .grid()
                .grid_cols(CARD_COLUMNS)
                .gap(GRID_GAP)
                .w_full()
        },
    );
    page_frame(cx, state, "Overview", (cards, totals_driver))
}
