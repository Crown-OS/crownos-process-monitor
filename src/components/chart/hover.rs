use crownui::kit::{Theme, use_theme};
use crownui::prelude::{
    CornerRadii, Cx, Rect, Runtime, Signal, Styled, Vector, View, bind, canvas, component, text,
};

use crate::components::pointer::Hover;

const READOUT_TEXT_SIZE: f32 = 12.0;
const READOUT_WIDTH_ESTIMATE: f32 = 96.0;
const READOUT_OFFSET: f32 = 8.0;
const CROSSHAIR_WIDTH: f32 = 1.0;

/// A vertical hairline under the pointer.
pub fn crosshair(theme: Signal<Theme>, hover: Signal<Option<Hover>>) -> impl View {
    canvas(move |runtime, size, list| {
        let Some(hover) = hover.get(runtime) else {
            return;
        };
        let color = theme.with(runtime, |theme| theme.text.muted);
        let x = hover.position.x.clamp(0.0, size.width) - CROSSHAIR_WIDTH / 2.0;
        list.fill(
            Rect::from_xywh(x, 0.0, CROSSHAIR_WIDTH, size.height),
            CornerRadii::ZERO,
            color,
        );
    })
    .absolute()
    .inset(0.0)
    .pointer_events_none()
}

/// A small label that follows the pointer along the top edge, showing `describe(hover)`.
pub fn readout_label(
    hover: Signal<Option<Hover>>,
    mut describe: impl FnMut(&mut Runtime, Hover) -> String + 'static,
) -> impl View {
    component(move |cx: &mut Cx| {
        let theme = use_theme(cx);
        text(bind(move |runtime| {
            hover
                .get(runtime)
                .map(|hover| describe(runtime, hover))
                .unwrap_or_default()
        }))
        .text_size(READOUT_TEXT_SIZE)
        .px(6.0)
        .py(2.0)
        .rounded(6.0)
        .bg(bind(move |runtime| {
            theme.with(runtime, |theme| theme.popover.bg)
        }))
        .text_color(bind(move |runtime| {
            theme.with(runtime, |theme| theme.popover.text)
        }))
        .absolute()
        .top(0.0)
        .left(0.0)
        .translate(bind(move |runtime| {
            hover
                .get(runtime)
                .map_or(Vector::ZERO, |hover| Vector::new(readout_x(hover), 0.0))
        }))
        .hidden(bind(move |runtime| hover.with(runtime, Option::is_none)))
        .pointer_events_none()
    })
}

fn readout_x(hover: Hover) -> f32 {
    let x = hover.position.x;
    if x + READOUT_OFFSET + READOUT_WIDTH_ESTIMATE > hover.size.width {
        (x - READOUT_OFFSET - READOUT_WIDTH_ESTIMATE).max(0.0)
    } else {
        x + READOUT_OFFSET
    }
}
