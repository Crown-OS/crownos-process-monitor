use crownui::kit::{
    ButtonOptions, ButtonVariant, MenuRowOptions, Theme, button, context_menu_at, icon_muted,
    icons, menu_item, menu_item_selected,
};
use crownui::prelude::{
    AnyView, Cx, Listeners, Point, Role, Semantic, Signal, Styled, View, bind, hstack, keyed, show,
    text, vstack,
};

use super::column::{Alignment, Column, ColumnSet, Visibility};
use super::sort::{SortColumn, SortDirection, SortOrder};
use super::state::TableState;
use crate::util::spacing;

pub(super) const HEADER_HEIGHT: f32 = 30.0;
pub(super) const ROW_PADDING: f32 = 12.0;
pub(super) const CELL_GAP: f32 = 8.0;
const HEADER_TEXT_SIZE: f32 = 12.0;
const INDICATOR_SIZE: f32 = 12.0;

fn sort_value(order: SortOrder, column: SortColumn) -> String {
    match (order.column == column, order.direction) {
        (false, _) => String::new(),
        (true, SortDirection::Ascending) => "ascending".to_owned(),
        (true, SortDirection::Descending) => "descending".to_owned(),
    }
}

fn indicator(order: SortOrder, column: SortColumn) -> AnyView {
    if order.column != column {
        return vstack(()).w(INDICATOR_SIZE).boxed();
    }
    let arrow = match order.direction {
        SortDirection::Ascending => icons::ARROW_UP,
        SortDirection::Descending => icons::ARROW_DOWN,
    };
    icon_muted(arrow).size(INDICATOR_SIZE).boxed()
}

#[derive(Clone, Copy)]
enum CellWidth {
    Flex,
    Fixed(f32),
}

fn header_cell(
    title: &'static str,
    column: SortColumn,
    (alignment, width): (Alignment, CellWidth),
    state: TableState,
    theme: Signal<Theme>,
) -> AnyView {
    let sort = state.sort;
    let label = text(title)
        .text_size(HEADER_TEXT_SIZE)
        .font_medium()
        .text_color(bind(move |runtime| {
            theme.with(runtime, |theme| theme.text.muted)
        }));
    let arrow = keyed(
        move |runtime| sort.get(runtime),
        move |_, order| indicator(order, column),
    );
    let cell = match alignment {
        Alignment::Start => hstack((label.boxed(), arrow.boxed())),
        Alignment::End => hstack((arrow.boxed(), label.boxed())).justify_end(),
    };
    let cell = cell
        .gap(spacing::TIGHT)
        .items_center()
        .role(Role::Button)
        .label(format!("Sort by {title}"))
        .value(bind(move |runtime| sort_value(sort.get(runtime), column)))
        .on_click(move |cx, _| state.sort_by(cx, column));
    match width {
        CellWidth::Flex => cell.flex_1().min_w(0.0).boxed(),
        CellWidth::Fixed(width) => cell.w(width).shrink(0.0).boxed(),
    }
}

/// The sticky header: Name, then each shown column; sortable ones toggle on click.
pub(super) fn header_row<D: ?Sized + 'static>(
    columns: &'static [Column<D>],
    visible: ColumnSet,
    state: TableState,
    theme: Signal<Theme>,
) -> impl View {
    let mut cells = vec![header_cell(
        "Name",
        SortColumn::Name,
        (Alignment::Start, CellWidth::Flex),
        state,
        theme,
    )];
    cells.extend(
        columns
            .iter()
            .enumerate()
            .filter(|(index, column)| column.is_shown(*index, visible))
            .map(|(index, column)| match column.sort_key {
                Some(_) => header_cell(
                    column.title,
                    SortColumn::Metric(index),
                    (column.alignment, CellWidth::Fixed(column.width)),
                    state,
                    theme,
                ),
                None => text(column.title)
                    .text_size(HEADER_TEXT_SIZE)
                    .font_medium()
                    .w(column.width)
                    .shrink(0.0)
                    .boxed(),
            }),
    );
    hstack(cells)
        .gap(CELL_GAP)
        .px(ROW_PADDING)
        .h(HEADER_HEIGHT)
        .items_center()
        .shrink(0.0)
}

/// The Columns button and the menu that shows or hides each optional column.
pub(super) fn columns_menu<D: ?Sized + 'static>(
    columns: &'static [Column<D>],
    state: TableState,
    menu_at: Signal<Option<Point>>,
    pointer: Signal<Point>,
) -> impl View {
    let trigger = button("Columns")
        .leading_icon(icons::COLUMNS_3)
        .variant(ButtonVariant::Ghost)
        .on_click(move |cx, _| {
            let at = pointer.get(cx);
            menu_at.set(cx, Some(at));
        });
    let menu = context_menu_at(menu_at, move || {
        let rows: Vec<AnyView> = columns
            .iter()
            .enumerate()
            .filter(|(_, column)| column.visibility == Visibility::Optional)
            .map(|(index, column)| {
                let title = column.title;
                let shown = state.columns;
                let toggle = move |cx: &mut Cx| {
                    shown.update(cx, |set| *set = set.toggled(index));
                };
                show(
                    bind(move |runtime| shown.get(runtime).contains(index)),
                    move || {
                        menu_item_selected(title)
                            .leading_icon(icons::CHECK)
                            .on_click(move |cx, _| toggle(cx))
                    },
                    move || menu_item(title).on_click(move |cx, _| toggle(cx)),
                )
                .flex_col()
                .items_stretch()
                .boxed()
            })
            .collect();
        vstack(rows).items_stretch()
    });
    hstack((trigger, menu)).items_center()
}
