use crownui::kit::{icons, use_theme};
use crownui::prelude::{Signal, Styled, View, bind, component, hstack, text};

use crate::components::app_table::{Column, RowRef, TableRows};
use crate::util::icon::tinted_icon;
use crate::util::spacing;

const ICON_SIZE: f32 = 14.0;
const TEXT_SIZE: f32 = 13.0;

/// The app ranked highest by `column`'s sort key, or `None` when the column has no key or the
/// list is empty.
pub fn top_app_index<D: TableRows + ?Sized>(rows: &D, column: &Column<D>) -> Option<usize> {
    let key = column.sort_key?;
    (0..rows.app_count())
        .map(|app| (app, key(rows, RowRef::App(app))))
        .filter(|(_, value)| value.is_finite())
        .max_by(|left, right| left.1.total_cmp(&right.1))
        .map(|(app, _)| app)
}

/// "Top App: Firefox · 23%": the leading app for `column` and its formatted cell.
pub fn top_app<S, D>(data: Signal<S>, project: fn(&S) -> &D, column: Column<D>) -> impl View
where
    S: 'static,
    D: TableRows + ?Sized + 'static,
{
    component(move |cx| {
        let theme = use_theme(cx);
        let tint = bind(move |runtime| theme.with(runtime, |theme| theme.accent.end));
        let summary = bind(move |runtime| {
            data.with(runtime, |snapshot| {
                let rows = project(snapshot);
                let Some(app) = top_app_index(rows, &column) else {
                    return "No apps yet".to_owned();
                };
                let row = RowRef::App(app);
                let mut out = format!("{} · ", rows.name(row));
                (column.text)(rows, row, &mut out);
                out
            })
        });
        hstack((
            tinted_icon(icons::TROPHY, tint, ICON_SIZE),
            text("Top App").text_size(TEXT_SIZE).font_semibold(),
            text(summary).text_size(TEXT_SIZE),
        ))
        .gap(spacing::SNUG)
        .items_center()
    })
}
