use crownui::kit::{icon_muted, use_theme};
use crownui::prelude::{IntoProp, Styled, View, bind, component, text, vstack};

use crate::util::spacing;

const ICON_SIZE: f32 = 28.0;
const TITLE_SIZE: f32 = 15.0;
const REASON_SIZE: f32 = 12.0;
const PADDING: f32 = 24.0;

pub const NOT_AVAILABLE: &str = "Not available on this hardware";

/// A centred placeholder for data this machine cannot provide, with the reason below.
pub fn empty_state(
    icon: &'static str,
    title: &'static str,
    reason: impl IntoProp<String> + 'static,
) -> impl View {
    component(move |cx| {
        let theme = use_theme(cx);
        vstack((
            icon_muted(icon).size(ICON_SIZE),
            text(title).text_size(TITLE_SIZE).font_semibold(),
            text(reason)
                .text_size(REASON_SIZE)
                .text_color(bind(move |runtime| {
                    theme.with(runtime, |theme| theme.text.muted)
                }))
                .text_center(),
        ))
        .gap(spacing::COMFORTABLE)
        .p(PADDING)
        .items_center()
        .w_full()
    })
}
