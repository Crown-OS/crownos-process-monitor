use crownui::ext::Prop;
use crownui::kit::{Theme, use_theme};
use crownui::prelude::{
    Cx, IntoProp, Semantic, Styled, View, bind, component, hstack, text, vstack,
};

use crate::util::theme::strain_tint;

const HEADLINE_SIZE: f32 = 34.0;
const FIGURE_SIZE: f32 = 17.0;
const LABEL_SIZE: f32 = 12.0;
const FIGURE_GAP: f32 = 28.0;
const LABEL_GAP: f32 = 2.0;

/// A labelled live value; turns the strain red while `strain` holds.
pub struct Figure {
    label: String,
    value: Prop<String>,
    strain: Prop<bool>,
}

impl Figure {
    pub fn new(label: impl Into<String>, value: impl IntoProp<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into_prop(),
            strain: Prop::Static(false),
        }
    }

    #[must_use]
    pub fn strain(mut self, strained: impl IntoProp<bool>) -> Self {
        self.strain = strained.into_prop();
        self
    }

    pub(super) fn view(self, cx: &mut Cx, value_size: f32) -> impl View + use<> {
        let theme = use_theme(cx);
        let muted = bind(move |runtime| theme.with(runtime, |theme| theme.text.muted));
        let tint = strain_tint(theme, self.strain, |theme: &Theme| theme.text.primary);
        let label = self.label.clone();
        vstack((
            text(self.label).text_size(LABEL_SIZE).text_color(muted),
            text(self.value)
                .text_size(value_size)
                .font_semibold()
                .text_color(tint)
                .label(label),
        ))
        .gap(LABEL_GAP)
    }
}

/// The hero strip of a page: the headline figure large, the secondary ones beside it.
pub fn stat_strip(headline: Figure, secondary: Vec<Figure>) -> impl View {
    component(move |cx| {
        let headline = headline.view(cx, HEADLINE_SIZE);
        let figures: Vec<_> = secondary
            .into_iter()
            .map(|figure| figure.view(cx, FIGURE_SIZE))
            .collect();
        hstack((headline, hstack(figures).gap(FIGURE_GAP).items_end()))
            .gap(FIGURE_GAP * 1.5)
            .items_end()
    })
}
