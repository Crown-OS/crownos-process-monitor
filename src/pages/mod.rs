//! One module per sidebar page, plus the pieces they share.

mod apps;
mod columns;
mod cpu;
mod disk;
mod frame;
mod gpu;
mod history_switch;
mod memory;
mod network;
mod overview;
mod sources;

use crownui::prelude::{AnyView, Cx, View};

use crate::app::{AppState, Navigation, Page};

pub fn build(cx: &mut Cx, state: AppState, navigation: Navigation, page: Page) -> AnyView {
    match page {
        Page::Overview => overview::page(cx, state, navigation).boxed(),
        Page::Cpu => cpu::page(cx, state).boxed(),
        Page::Gpu => gpu::page(cx, state).boxed(),
        Page::Memory => memory::page(cx, state).boxed(),
        Page::Disk => disk::page(cx, state).boxed(),
        Page::Network => network::page(cx, state).boxed(),
    }
}
