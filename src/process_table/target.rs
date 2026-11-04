use std::sync::Arc;

use monitor_core::{AppKey, ProcessKey};

use crate::app::live::{AppEntry, AppList, ProcessEntry};
use crate::components::app_table::{AppCategory, RowId};

/// What a table row's actions reach: a whole app or one process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionTarget {
    App {
        key: AppKey,
        name: Arc<str>,
        /// Members the table listed; the scope may hold more.
        listed: usize,
        pausable: bool,
    },
    Process {
        key: ProcessKey,
        name: Arc<str>,
    },
}

impl ActionTarget {
    /// The target behind a row; `None` for the Ended recently group and rows gone since.
    pub fn resolve(apps: &AppList, row: RowId) -> Option<Self> {
        let app = apps.app_by_key(row.app)?;
        if app.facts.category == AppCategory::Ended {
            return None;
        }
        match row.process {
            None => Some(Self::App {
                key: app.facts.app_key.clone()?,
                name: Arc::clone(&app.name),
                listed: app.processes.len(),
                pausable: app.facts.pausable,
            }),
            Some(pid) => {
                let process = member(app, pid)?;
                Some(Self::Process {
                    key: process.facts.key(process.pid),
                    name: Arc::clone(&process.name),
                })
            }
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::App { name, .. } | Self::Process { name, .. } => name,
        }
    }

    pub const fn is_app(&self) -> bool {
        matches!(self, Self::App { .. })
    }
}

pub fn member(app: &AppEntry, pid: u64) -> Option<&ProcessEntry> {
    app.processes
        .iter()
        .find(|process| u64::from(process.pid) == pid)
}

/// The process a row stands for: itself, or an app's main process (its oldest member).
pub fn representative(apps: &AppList, row: RowId) -> Option<&ProcessEntry> {
    let app = apps.app_by_key(row.app)?;
    match row.process {
        Some(pid) => member(app, pid),
        None => app
            .processes
            .iter()
            .min_by_key(|process| (process.facts.started_unix, process.pid)),
    }
}

/// "1 process" / "96 processes".
pub fn process_count(count: usize) -> String {
    let noun = if count == 1 { "process" } else { "processes" };
    format!("{count} {noun}")
}
