use std::rc::Rc;

use crownui::ext::NodeId;
use crownui::kit::{
    ClearButton, TextInputOptions, ToggleItem, context_menu_at, divider, icons, text_input,
    toggle_group, use_theme,
};
use crownui::prelude::{
    AnyView, Cx, Listeners, Point, ScrollHandle, ScrollOptions, Semantic, Signal, Store, Styled,
    View, hstack, keyed, lazy_list, spacer, vstack,
};

use super::column::Column;
use super::data::{RowId, TableRows};
use super::filter::AppFilter;
use super::find::FindQuery;
use super::header::{CELL_GAP, columns_menu, header_row};
use super::keys::handle_key;
use super::model::{RowFilter, RowModel};
use super::row::{Activate, RowContext, row_view};
use super::state::TableState;
use crate::util::shortcut::ctrl;
use crate::util::spacing;

const DEFAULT_ROW_HEIGHT: f32 = 32.0;
const FIND_WIDTH: f32 = 240.0;

type MenuItems = Box<dyn FnMut(Signal<Option<RowId>>) -> Vec<AnyView>>;

/// The table over `project(data)`: `columns` after the built-in Name column, its state owned
/// by the caller.
pub struct AppTable<S: 'static, D: ?Sized + 'static> {
    label: &'static str,
    data: Signal<S>,
    project: fn(&S) -> &D,
    columns: &'static [Column<D>],
    state: TableState,
    row_height: f32,
    menu: Option<MenuItems>,
    activate: Option<Activate>,
    delete: Option<Activate>,
}

pub fn app_table<S, D>(
    label: &'static str,
    data: Signal<S>,
    project: fn(&S) -> &D,
    columns: &'static [Column<D>],
    state: TableState,
) -> AppTable<S, D>
where
    S: 'static,
    D: TableRows + ?Sized + 'static,
{
    AppTable {
        label,
        data,
        project,
        columns,
        state,
        row_height: DEFAULT_ROW_HEIGHT,
        menu: None,
        activate: None,
        delete: None,
    }
}

impl<S, D> AppTable<S, D>
where
    S: 'static,
    D: TableRows + ?Sized + 'static,
{
    #[must_use]
    pub fn row_height(mut self, height: f32) -> Self {
        self.row_height = height;
        self
    }

    /// Right-click menu rows; their handlers read the clicked row from the signal they get.
    #[must_use]
    pub fn context_menu(
        mut self,
        items: impl FnMut(Signal<Option<RowId>>) -> Vec<AnyView> + 'static,
    ) -> Self {
        self.menu = Some(Box::new(items));
        self
    }

    /// Runs on double-click or Enter on a row (e.g. to open the inspector).
    #[must_use]
    pub fn on_activate(mut self, activate: impl Fn(&mut Cx, RowId) + 'static) -> Self {
        self.activate = Some(Rc::new(activate));
        self
    }

    /// Runs on Delete over the selected row (e.g. to ask whether to quit it).
    #[must_use]
    pub fn on_delete(mut self, delete: impl Fn(&mut Cx, RowId) + 'static) -> Self {
        self.delete = Some(Rc::new(delete));
        self
    }
}

/// Builds `view` and remembers its node, so a shortcut can focus it later.
struct Captured<V> {
    view: V,
    node: Store<Option<NodeId>>,
}

impl<V: View> View for Captured<V> {
    fn build(self, cx: &mut Cx) -> NodeId {
        let node = self.view.build(cx);
        self.node.update(cx, |kept| *kept = Some(node));
        node
    }
}

/// The My apps / All / System / Containers chips, written into `filter`.
fn filter_chips(cx: &mut Cx, filter: Signal<AppFilter>) -> impl View + use<> {
    let initial = filter.with_untracked(cx, |filter| filter.index());
    let chosen = cx.signal(initial);
    cx.effect(move |runtime| {
        let picked = AppFilter::from_index(chosen.get(runtime));
        if filter.with_untracked(runtime, |current| *current != picked) {
            filter.set(runtime, picked);
        }
    });
    toggle_group(
        AppFilter::ALL
            .iter()
            .map(|filter| ToggleItem::text(filter.label())),
        chosen,
    )
}

fn watch_query(cx: &mut Cx, state: TableState) -> (Signal<u64>, Store<FindQuery>) {
    let filter = cx.store(FindQuery::Everything);
    let revision = cx.signal(0_u64);
    cx.effect(move |runtime| {
        let parsed = state.query.with(runtime, |query| FindQuery::parse(query));
        filter.update(runtime, |current| *current = parsed);
        revision.update(runtime, |revision| *revision += 1);
    });
    (revision, filter)
}

/// The row model and its length, rebuilt whenever the data, sort, filter, columns or
/// expansion change. `figures` ticks on every rebuild and mounted rows refresh in place on it;
/// `shape` ticks only when the visible columns change, which rebuilds the rows.
struct RowWatch {
    model: Store<RowModel>,
    count: Signal<usize>,
    figures: Signal<u64>,
    shape: Signal<u64>,
}

fn watch_rows<S, D>(
    cx: &mut Cx,
    data: Signal<S>,
    project: fn(&S) -> &D,
    columns: &'static [Column<D>],
    state: TableState,
) -> RowWatch
where
    S: 'static,
    D: TableRows + ?Sized + 'static,
{
    let model = cx.store(RowModel::default());
    let count = cx.signal(0_usize);
    let figures = cx.signal(0_u64);
    let shape = cx.signal(0_u64);
    let (filter_revision, filter) = watch_query(cx, state);
    let mut expanded = Vec::new();
    let mut pinned = Vec::new();
    let mut shown_columns = None;
    cx.effect(move |runtime| {
        let sort = state.sort.get(runtime);
        let chip = state.filter.get(runtime);
        filter_revision.get(runtime);
        let visible = state.columns.get(runtime);
        expanded.clear();
        state
            .expanded
            .with(runtime, |keys| expanded.extend_from_slice(keys));
        pinned.clear();
        state
            .pinned
            .with(runtime, |keys| pinned.extend_from_slice(keys));
        let mut rows = model.update(runtime, std::mem::take);
        let query = filter.update(runtime, std::mem::take);
        data.with(runtime, |source| {
            rows.rebuild(
                project(source),
                columns,
                &RowFilter {
                    query: &query,
                    sort,
                    expanded: &expanded,
                    pinned: &pinned,
                    chip,
                },
            );
        });
        let length = rows.rows().len();
        model.update(runtime, |model| *model = rows);
        filter.update(runtime, |filter| *filter = query);
        if count.with_untracked(runtime, |count| *count != length) {
            count.set(runtime, length);
        }
        figures.update(runtime, |figures| *figures += 1);
        if shown_columns.replace(visible) != Some(visible) {
            shape.update(runtime, |shape| *shape += 1);
        }
    });
    RowWatch {
        model,
        count,
        figures,
        shape,
    }
}

fn toolbar<D: ?Sized + 'static>(
    cx: &mut Cx,
    state: TableState,
    columns: &'static [Column<D>],
    columns_menu_at: Signal<Option<Point>>,
    pointer: Signal<Point>,
) -> impl View + use<D> {
    let find_field = cx.store(None);
    let find = Captured {
        view: text_input(state.query)
            .placeholder("Find")
            .leading_icon(icons::SEARCH)
            .clear_button(ClearButton::Visible)
            .width(FIND_WIDTH),
        node: find_field,
    };
    let jump_to_find = vstack(()).shortcut(ctrl('f')).on_click(move |cx, _| {
        if let Some(field) = find_field.with(cx, |field| *field) {
            cx.request_focus(field);
        }
    });
    hstack((
        find,
        jump_to_find,
        filter_chips(cx, state.filter),
        spacer(),
        columns_menu(columns, state, columns_menu_at, pointer),
    ))
    .gap(CELL_GAP)
    .items_center()
}

impl<S, D> View for AppTable<S, D>
where
    S: 'static,
    D: TableRows + ?Sized + 'static,
{
    fn build(self, cx: &mut Cx) -> NodeId {
        let theme = use_theme(cx);
        let Self {
            label,
            data,
            project,
            columns,
            state,
            row_height,
            menu,
            activate,
            delete,
        } = self;
        let RowWatch {
            model,
            count,
            figures,
            shape,
        } = watch_rows(cx, data, project, columns, state);

        let handle = ScrollHandle::new(cx);
        let menu_at = cx.signal(None);
        let columns_menu_at = cx.signal(None);
        let menu_target = cx.signal(None);
        let pointer = cx.signal(Point::ZERO);
        let context = RowContext {
            data,
            project,
            columns,
            model,
            figures,
            state,
            theme,
            row_height,
            menu_at,
            menu_target,
            pointer,
            activate,
            delete,
        };
        let keyboard = context.clone();
        let rows = context;
        let body = lazy_list(count, shape, row_height, move |_, index| {
            row_view(&rows, index)
        })
        .scroll_handle(handle)
        .flex_1();
        let toolbar = toolbar(cx, state, columns, columns_menu_at, pointer);
        let header = keyed(
            move |runtime| state.columns.get(runtime),
            move |_, visible| header_row(columns, visible, state, theme),
        );
        let row_menu = menu.map(|mut items| {
            context_menu_at(menu_at, move || vstack(items(menu_target)).items_stretch())
        });
        cx.build(
            vstack((toolbar, header, divider(), body, row_menu))
                .gap(spacing::TIGHT)
                .flex_1()
                .min_h(0.0)
                .focusable(true)
                .label(label)
                .on_pointer_down(move |cx, event| pointer.set(cx, event.position))
                .on_key_down(move |cx, event| {
                    if !event.is_default_prevented() && handle_key(cx, event, &keyboard, handle) {
                        event.prevent_default();
                    }
                }),
        )
    }
}
