use crownui::kit::{ButtonOptions, ButtonSize, ButtonVariant, button, icons, use_theme};
use crownui::prelude::{
    AnyView, Listeners, Signal, Styled, View, bind, component, show, text, vstack,
};

use crate::util::spacing;

const BODY_SIZE: f32 = 11.0;
const BODY_PADDING: f32 = 8.0;

/// A Show/Hide button over a block of monospace-ish text that stays folded until asked.
pub fn folded_text(label: &'static str, open: Signal<bool>, body: String) -> AnyView {
    let toggle = button(bind(move |runtime| {
        if open.get(runtime) {
            format!("Hide {label}")
        } else {
            format!("Show {label}")
        }
    }))
    .variant(ButtonVariant::Ghost)
    .control_size(ButtonSize::Small)
    .leading_icon(icons::CHEVRON_RIGHT)
    .on_click(move |cx, _| open.update(cx, |open| *open = !*open));
    let body = show(
        open,
        move || {
            let body = body.clone();
            component(move |cx| {
                let theme = use_theme(cx);
                text(body)
                    .text_size(BODY_SIZE)
                    .p(BODY_PADDING)
                    .rounded(6.0)
                    .w_full()
                    .bg(bind(move |runtime| {
                        theme.with(runtime, |theme| theme.surface.bg)
                    }))
            })
        },
        || vstack(()),
    );
    vstack((toggle, body))
        .gap(spacing::TIGHT)
        .items_start()
        .w_full()
        .boxed()
}
