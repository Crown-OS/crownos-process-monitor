use std::rc::Rc;

use crownui::kit::Theme;
use crownui::prelude::{
    AnyView, Cx, Listeners, Point, Role, Semantic, Signal, Store, Styled, View, bind, hstack,
    keyed, text, vstack,
};

use super::column::{Alignment, Column, ColumnSet};
use super::data::{RowId, TableRows};
use super::header::{CELL_GAP, ROW_PADDING};
use super::model::RowModel;
use super::row_leading::{RowKind, leading, row_name};
use super::row_source::RowSource;
use super::state::TableState;

const TEXT_SIZE: f32 = 13.0;
const ROW_RADIUS: f32 = 6.0;
const SELECTED_ALPHA: f32 = 0.18;

pub(super) type Activate = Rc<dyn Fn(&mut Cx, RowId)>;

/// Everything a row needs to build itself from the model, shared by every row.
pub(super) struct RowContext<S: 'static, D: ?Sized + 'static> {
    pub data: Signal<S>,
    pub project: fn(&S) -> &D,
    pub columns: &'static [Column<D>],
    pub model: Store<RowModel>,
    /// Ticks whenever the model was rebuilt; rows re-read what they show on it.
    pub figures: Signal<u64>,
    pub state: TableState,
    pub theme: Signal<Theme>,
    pub row_height: f32,
    pub menu_at: Signal<Option<Point>>,
    pub menu_target: Signal<Option<RowId>>,
    pub pointer: Signal<Point>,
    pub activate: Option<Activate>,
    pub delete: Option<Activate>,
}

impl<S: 'static, D: ?Sized + 'static> Clone for RowContext<S, D> {
    fn clone(&self) -> Self {
        Self {
            activate: self.activate.clone(),
            delete: self.delete.clone(),
            ..*self
        }
    }
}

impl<S: 'static, D: ?Sized + 'static> RowContext<S, D> {
    fn source(&self, index: usize) -> RowSource<S, D> {
        RowSource {
            data: self.data,
            project: self.project,
            model: self.model,
            figures: self.figures,
            index,
        }
    }
}

fn metric_cells<S, D>(
    context: &RowContext<S, D>,
    source: RowSource<S, D>,
    visible: ColumnSet,
) -> Vec<AnyView>
where
    S: 'static,
    D: TableRows + ?Sized + 'static,
{
    context
        .columns
        .iter()
        .enumerate()
        .filter(|(position, column)| column.is_shown(*position, visible))
        .map(|(_, column)| {
            let figure = bind(move |runtime| {
                source.read(runtime, String::new(), |data, row| {
                    let mut cell = String::new();
                    (column.text)(data, row, &mut cell);
                    cell
                })
            });
            let cell = text(figure)
                .text_size(TEXT_SIZE)
                .w(column.width)
                .shrink(0.0);
            match column.alignment {
                Alignment::Start => cell.truncate().boxed(),
                Alignment::End => cell.text_right().boxed(),
            }
        })
        .collect()
}

/// The row at `index`, refreshed in place on every model rebuild and built again only when
/// an app row and a process row trade places there.
pub(super) fn row_view<S, D>(context: &RowContext<S, D>, index: usize) -> impl View + use<S, D>
where
    S: 'static,
    D: TableRows + ?Sized + 'static,
{
    let source = context.source(index);
    let rows = context.clone();
    keyed(
        move |runtime| source.read(runtime, None, |_, row| Some(RowKind::of(row))),
        move |cx, kind| match kind {
            Some(kind) => build_row(cx, &rows, source, kind),
            None => vstack(()).boxed(),
        },
    )
}

fn build_row<S, D>(
    cx: &mut Cx,
    context: &RowContext<S, D>,
    source: RowSource<S, D>,
    kind: RowKind,
) -> AnyView
where
    S: 'static,
    D: TableRows + ?Sized + 'static,
{
    let (state, theme) = (context.state, context.theme);
    let visible = state.columns.with_untracked(cx, |set| *set);
    let name = row_name(cx, source);
    let cells = metric_cells(context, source, visible);
    let leading = leading(cx, source, kind, name, state, theme);
    let selected = state.selected;
    let selected_fill = bind(move |runtime| {
        let chosen = selected
            .get(runtime)
            .is_some_and(|id| source.id(runtime) == Some(id));
        theme.with(runtime, |theme| {
            if chosen {
                theme.accent.start.with_alpha_factor(SELECTED_ALPHA)
            } else {
                crownui::prelude::TRANSPARENT
            }
        })
    });
    let on_double = context.activate.clone();
    let (menu_at, menu_target, pointer) = (context.menu_at, context.menu_target, context.pointer);
    hstack((leading, cells))
        .gap(CELL_GAP)
        .px(ROW_PADDING)
        .h(context.row_height)
        .w_full()
        .items_center()
        .rounded(ROW_RADIUS)
        .bg(selected_fill)
        .role(Role::ListItem)
        .label(name)
        .on_click(move |cx, _| {
            if let Some(id) = source.id(cx) {
                selected.set(cx, Some(id));
            }
        })
        .on_double_click(move |cx, _| {
            let Some(id) = source.id(cx) else {
                return;
            };
            selected.set(cx, Some(id));
            if let Some(activate) = &on_double {
                activate(cx, id);
            }
        })
        .on_context_menu(move |cx, _| {
            let Some(id) = source.id(cx) else {
                return;
            };
            selected.set(cx, Some(id));
            menu_target.set(cx, Some(id));
            let at = pointer.get(cx);
            menu_at.set(cx, Some(at));
        })
        .boxed()
}
