use super::filter::AppCategory;

/// A row of the flattened table: an app, or one of its processes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RowRef {
    App(usize),
    Process(usize, usize),
}

impl RowRef {
    pub const fn app(self) -> usize {
        match self {
            Self::App(app) | Self::Process(app, _) => app,
        }
    }
}

/// A row's identity across samples and re-sorts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RowId {
    pub app: u64,
    pub process: Option<u64>,
}

/// "New" for a process younger than ten seconds, "Ended" for one gone in the last ten minutes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RowMark {
    #[default]
    None,
    New,
    Ended,
}

/// The rows a table shows: apps, each with its processes.
pub trait TableRows {
    fn app_count(&self) -> usize;
    fn process_count(&self, app: usize) -> usize;
    fn app_key(&self, app: usize) -> u64;
    fn process_key(&self, app: usize, process: usize) -> u64;
    fn name(&self, row: RowRef) -> &str;
    /// A Lucide icon body for the app's icon slot.
    fn icon(&self, app: usize) -> &'static str;
    fn mark(&self, row: RowRef) -> RowMark;

    /// Which filter chip shows the app; `Ended` sorts it below every other app.
    fn category(&self, _app: usize) -> AppCategory {
        AppCategory::Mine
    }

    /// Muted text after the name, such as a browser role or an end time.
    fn detail(&self, _row: RowRef, _out: &mut String) {}

    fn row_id(&self, row: RowRef) -> RowId {
        match row {
            RowRef::App(app) => RowId {
                app: self.app_key(app),
                process: None,
            },
            RowRef::Process(app, process) => RowId {
                app: self.app_key(app),
                process: Some(self.process_key(app, process)),
            },
        }
    }
}
