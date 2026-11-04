use crownui::prelude::{Cx, Signal};

use super::column::ColumnSet;
use super::data::RowId;
use super::filter::AppFilter;
use super::sort::{SortColumn, SortDirection, SortOrder};

/// A table's interactive state, owned by the page so it can persist or act on it.
#[derive(Clone, Copy)]
pub struct TableState {
    pub sort: Signal<SortOrder>,
    pub expanded: Signal<Vec<u64>>,
    pub pinned: Signal<Vec<u64>>,
    pub selected: Signal<Option<RowId>>,
    pub query: Signal<String>,
    pub columns: Signal<ColumnSet>,
    pub filter: Signal<AppFilter>,
}

fn toggle_key(keys: &mut Vec<u64>, key: u64) {
    match keys.iter().position(|kept| *kept == key) {
        Some(index) => {
            keys.swap_remove(index);
        }
        None => keys.push(key),
    }
}

impl TableState {
    pub fn new(cx: &mut Cx, sort: SortOrder, columns: Signal<ColumnSet>) -> Self {
        Self {
            sort: cx.signal(sort),
            expanded: cx.signal(Vec::new()),
            pinned: cx.signal(Vec::new()),
            selected: cx.signal(None),
            query: cx.signal(String::new()),
            columns,
            filter: cx.signal(AppFilter::default()),
        }
    }

    pub fn toggle_expanded(self, cx: &mut Cx, app: u64) {
        self.expanded.update(cx, |keys| toggle_key(keys, app));
    }

    pub fn set_expanded(self, cx: &mut Cx, app: u64, expanded: bool) {
        let current = self.expanded.with_untracked(cx, |keys| keys.contains(&app));
        if current != expanded {
            self.toggle_expanded(cx, app);
        }
    }

    pub fn toggle_pinned(self, cx: &mut Cx, app: u64) {
        self.pinned.update(cx, |keys| toggle_key(keys, app));
    }

    /// Sorts by `column`, flipping the direction when it already is the sort column. Names
    /// start ascending, numbers descending.
    pub fn sort_by(self, cx: &mut Cx, column: SortColumn) {
        let current = self.sort.with_untracked(cx, |sort| *sort);
        let direction = if current.column == column {
            current.direction.flipped()
        } else if column == SortColumn::Name {
            SortDirection::Ascending
        } else {
            SortDirection::Descending
        };
        self.sort.set(cx, SortOrder::new(column, direction));
    }
}
