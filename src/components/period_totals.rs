use crownui::kit::use_theme;
use crownui::prelude::{
    Role, Runtime, Semantic, Styled, View, bind, component, hstack, text, vstack,
};

use crate::util::format::bytes;
use crate::util::spacing;

const VALUE_SIZE: f32 = 15.0;
const LABEL_SIZE: f32 = 12.0;
const COLUMN_GAP: f32 = 28.0;
pub const PERIOD_LABELS: [&str; 3] = ["Today", "7 days", "30 days"];

/// Two byte totals per period, e.g. read and written today, in 7 and in 30 days.
pub struct PeriodTotals<F> {
    pub directions: [&'static str; 2],
    /// `None` while history has not answered.
    pub totals: F,
}

pub fn period_totals<F>(view: PeriodTotals<F>) -> impl View
where
    F: Fn(&mut Runtime) -> Option<[[u64; 2]; 3]> + Clone + 'static,
{
    component(move |cx| {
        let theme = use_theme(cx);
        let muted = move || bind(move |runtime| theme.with(runtime, |theme| theme.text.muted));
        let columns: Vec<_> = PERIOD_LABELS
            .iter()
            .enumerate()
            .map(|(period, title)| {
                let rows: Vec<_> = view
                    .directions
                    .iter()
                    .enumerate()
                    .map(|(direction, label)| {
                        let totals = view.totals.clone();
                        hstack((
                            text(bind(move |runtime| {
                                totals(runtime).map_or_else(
                                    || "–".to_owned(),
                                    |totals| bytes(totals[period][direction]),
                                )
                            }))
                            .text_size(VALUE_SIZE)
                            .font_semibold(),
                            text(*label).text_size(LABEL_SIZE).text_color(muted()),
                        ))
                        .gap(spacing::SNUG)
                        .items_end()
                    })
                    .collect();
                vstack((
                    text(*title).text_size(LABEL_SIZE).text_color(muted()),
                    vstack(rows).gap(spacing::HAIRLINE),
                ))
                .gap(spacing::TIGHT)
                .role(Role::Group)
                .label(*title)
            })
            .collect();
        hstack(columns).gap(COLUMN_GAP).w_full()
    })
}
