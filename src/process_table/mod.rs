//! The app table as every page uses it: its row menu (inspect, copy, pin, quit, force quit,
//! pause/resume, renice, CPU affinity, alerts), confirmations stating how many processes an action reaches,
//! actions run off the UI thread with a toast for the result, and saved column choices.

mod affinity;
mod alert;
mod assess;
mod escalate;
mod menu;
mod perform;
mod preferences;
mod prompt;
mod target;
#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;

use crownui::kit::{provide_toasts, toast_host};
use crownui::prelude::{Cx, Listeners, Point, Signal, Styled, View, component, vstack};
use monitor_engine::system::ProcessAction;

pub use alert::{AlertRequest, AlertWish};
pub use assess::NextStep;
pub use menu::{NICE_LEVELS, RowSummary};
pub use perform::RowAction;
pub use target::{ActionTarget, representative};

use self::affinity::AffinityDialog;
use self::menu::{MenuContext, renice_menu, row_menu};
use self::preferences::persist_columns;
use self::prompt::ActionPrompt;
use crate::app::AppState;
use crate::app::live::{AppList, LiveSnapshot};
use crate::components::app_table::{
    Column, ColumnSet, RowId, SortColumn, SortDirection, SortOrder, TableRows, TableState,
    app_table,
};

const TABLE_HEIGHT: f32 = 380.0;

/// What a page table's rows are read from: the app list, or the whole live snapshot for
/// columns that also read the per-row details or the latest tick.
pub trait TableSource: TableRows + 'static {
    fn rows(snapshot: &LiveSnapshot) -> &Self;
}

impl TableSource for AppList {
    fn rows(snapshot: &LiveSnapshot) -> &Self {
        &snapshot.apps
    }
}

impl TableSource for LiveSnapshot {
    fn rows(snapshot: &LiveSnapshot) -> &Self {
        snapshot
    }
}

/// The page's app table with its row menu, confirmations and toasts. Double-click, Enter or
/// Inspect writes `inspect`; Delete asks to quit the selected row.
pub fn apps_table<D: TableSource>(
    cx: &mut Cx,
    state: AppState,
    label: &'static str,
    columns: &'static [Column<D>],
    sort_column: usize,
    inspect: Signal<Option<RowId>>,
) -> impl View + use<D> {
    let visible = cx.signal(ColumnSet::NONE);
    persist_columns(cx, label, columns, visible);
    let table = TableState::new(
        cx,
        SortOrder::new(SortColumn::Metric(sort_column), SortDirection::Descending),
        visible,
    );
    let row_height = state.settings.current(cx).spacing.row_height();
    component(move |cx| {
        let toasts = provide_toasts(cx);
        let prompt = ActionPrompt::new(cx, state, toasts);
        let pointer = cx.signal(Point::ZERO);
        let context = MenuContext {
            state,
            table,
            prompt,
            inspect,
            pointer,
            renice_at: cx.signal(None),
            renice_row: cx.signal(None),
            affinity: AffinityDialog::new(cx),
        };
        toast_host(
            vstack((
                app_table(label, state.live, D::rows, columns, table)
                    .row_height(row_height)
                    .context_menu(row_menu(context))
                    .on_activate(move |cx, row| inspect.set(cx, Some(row)))
                    .on_delete(move |cx, row| {
                        prompt.request(cx, row, RowAction::Signal(ProcessAction::Quit));
                    }),
                prompt.view(),
                renice_menu(context),
                context.affinity.view(state, prompt),
            ))
            .flex_1()
            .min_h(0.0)
            .on_pointer_down(move |cx, event| pointer.set(cx, event.position)),
        )
        .h(TABLE_HEIGHT)
        .w_full()
    })
}
