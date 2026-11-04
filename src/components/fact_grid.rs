use crownui::ext::Prop;
use crownui::kit::use_theme;
use crownui::prelude::{IntoProp, Styled, View, bind, component, text, vstack};

use crate::util::spacing;

const COLUMNS: u16 = 3;
const LABEL_SIZE: f32 = 11.0;
const VALUE_SIZE: f32 = 13.0;
const ROW_GAP: f32 = 12.0;
const COLUMN_GAP: f32 = 18.0;

/// One labelled reading of a [`fact_grid`].
pub struct Fact {
    label: String,
    value: Prop<String>,
}

impl Fact {
    pub fn new(label: impl Into<String>, value: impl IntoProp<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into_prop(),
        }
    }
}

/// Small labelled values in three columns: hardware details, counters, link states.
pub fn fact_grid(facts: Vec<Fact>) -> impl View {
    component(move |cx| {
        let theme = use_theme(cx);
        let cells: Vec<_> = facts
            .into_iter()
            .map(|fact| {
                vstack((
                    text(fact.label)
                        .text_size(LABEL_SIZE)
                        .text_color(bind(move |runtime| {
                            theme.with(runtime, |theme| theme.text.muted)
                        })),
                    text(fact.value).text_size(VALUE_SIZE),
                ))
                .gap(spacing::HAIRLINE)
            })
            .collect();
        vstack(cells)
            .grid()
            .grid_cols(COLUMNS)
            .gap_y(ROW_GAP)
            .gap_x(COLUMN_GAP)
            .w_full()
    })
}
