use crownui::kit::use_theme;
use crownui::prelude::{IntoProp, Role, Semantic, Styled, View, bind, component, hstack, text};

use crate::util::spacing;

const FACT_SIZE: f32 = 12.0;
const LABEL_WIDTH: f32 = 140.0;

/// A muted label beside a value that may change every tick; named after the label.
pub fn fact_row(label: &'static str, value: impl IntoProp<String> + 'static) -> impl View {
    component(move |cx| {
        let theme = use_theme(cx);
        hstack((
            text(label)
                .text_size(FACT_SIZE)
                .text_color(bind(move |runtime| {
                    theme.with(runtime, |theme| theme.text.muted)
                }))
                .w(LABEL_WIDTH)
                .shrink(0.0),
            text(value).text_size(FACT_SIZE).flex_1().min_w(0.0),
        ))
        .gap(spacing::COMFORTABLE)
        .items_start()
        .w_full()
        .role(Role::Group)
        .label(label)
    })
}
