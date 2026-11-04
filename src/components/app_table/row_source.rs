use crownui::prelude::{Runtime, Signal, Store};

use super::data::{RowId, RowRef, TableRows};
use super::model::RowModel;

/// Whatever row sits at `index` of the model right now. A mounted row reads everything it
/// shows through this, so a tick that re-sorts the rows refreshes them in place.
pub(super) struct RowSource<S: 'static, D: ?Sized + 'static> {
    pub data: Signal<S>,
    pub project: fn(&S) -> &D,
    pub model: Store<RowModel>,
    /// Ticks whenever the model was rebuilt.
    pub figures: Signal<u64>,
    pub index: usize,
}

impl<S: 'static, D: ?Sized + 'static> Clone for RowSource<S, D> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<S: 'static, D: ?Sized + 'static> Copy for RowSource<S, D> {}

impl<S: 'static, D: TableRows + ?Sized + 'static> RowSource<S, D> {
    /// `read` of the current row, or `fallback` once the index is past the end; re-runs the
    /// calling observer after every model rebuild.
    pub fn read<T>(
        self,
        runtime: &mut Runtime,
        fallback: T,
        read: impl FnOnce(&D, RowRef) -> T,
    ) -> T {
        self.figures.get(runtime);
        let Some(row) = self
            .model
            .with(runtime, |model| model.rows().get(self.index).copied())
        else {
            return fallback;
        };
        let project = self.project;
        self.data
            .with_untracked(runtime, |source| read(project(source), row))
    }

    pub fn id(self, runtime: &mut Runtime) -> Option<RowId> {
        self.read(runtime, None, |data, row| Some(data.row_id(row)))
    }
}
