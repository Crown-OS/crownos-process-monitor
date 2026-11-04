//! The Columns menu's choices, saved per table in their own config section, read and written
//! off the UI thread.

use std::collections::BTreeMap;
use std::sync::Mutex;

use crownui::prelude::{Cx, Signal};
use serde::{Deserialize, Serialize};

use crate::components::app_table::{Column, ColumnSet, Visibility};

/// Stored as `~/.config/crownos/process-monitor-tables.ron`.
const SECTION: &str = "process-monitor-tables";
/// Headless tests never touch the user's config.
const PERSISTS: bool = !cfg!(test);
/// Serialises read-modify-write cycles of tables saving at once.
static SAVING: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TablePreferences {
    /// Table label → ids of the optional columns shown.
    pub columns: BTreeMap<String, Vec<String>>,
}

/// The shown optional columns as ids, so a reordered column list keeps its choices.
pub fn shown_ids<D: ?Sized>(columns: &[Column<D>], shown: ColumnSet) -> Vec<String> {
    columns
        .iter()
        .enumerate()
        .filter(|(index, column)| {
            column.visibility == Visibility::Optional && shown.contains(*index)
        })
        .map(|(_, column)| column.id.to_owned())
        .collect()
}

pub fn set_from_ids<D: ?Sized>(columns: &[Column<D>], ids: &[String]) -> ColumnSet {
    columns
        .iter()
        .enumerate()
        .filter(|(_, column)| ids.iter().any(|id| id == column.id))
        .fold(ColumnSet::NONE, |set, (index, _)| set.with(index))
}

/// Loads `label`'s saved columns into `shown`, then saves every later change.
pub fn persist_columns<D: ?Sized + 'static>(
    cx: &mut Cx,
    label: &'static str,
    columns: &'static [Column<D>],
    shown: Signal<ColumnSet>,
) {
    if !PERSISTS {
        return;
    }
    let load = async move {
        tokio::task::spawn_blocking(move || {
            let saved: TablePreferences = crownconfig::load_section(SECTION);
            saved.columns.get(label).cloned()
        })
        .await
        .ok()
        .flatten()
    };
    cx.spawn(load, move |cx, saved| {
        if let Some(ids) = saved {
            shown.set(cx, set_from_ids(columns, &ids));
        }
        let mut last = shown.with_untracked(cx, |set| *set);
        cx.effect(move |runtime| {
            let current = shown.get(runtime);
            if current != last {
                last = current;
                save(label, shown_ids(columns, current));
            }
        });
    });
}

/// A column toggle is rare, so each save gets a short-lived thread of its own.
fn save(label: &'static str, ids: Vec<String>) {
    let saving = std::thread::Builder::new()
        .name("monitor-table-prefs".into())
        .spawn(move || {
            let Ok(_guard) = SAVING.lock() else {
                return;
            };
            let mut preferences: TablePreferences = crownconfig::load_section(SECTION);
            preferences.columns.insert(label.to_owned(), ids);
            if let Err(error) = crownconfig::save_section(SECTION, &preferences) {
                eprintln!("process-monitor: {error}");
            }
        });
    if let Err(error) = saving {
        eprintln!("process-monitor: cannot save the table columns: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::app_table::RowRef;

    fn blank(_: &(), _: RowRef, _: &mut String) {}

    const COLUMNS: [Column<()>; 3] = [
        Column::detail("pid", "PID", 60.0, blank),
        Column::detail("user", "User", 60.0, blank),
        Column::detail("path", "Path", 60.0, blank),
    ];

    #[test]
    fn columns_round_trip_through_their_ids() {
        let shown = ColumnSet::NONE.with(0).with(2);
        let ids = shown_ids(&COLUMNS, shown);
        assert_eq!(ids, ["pid", "path"]);
        assert_eq!(set_from_ids(&COLUMNS, &ids), shown);
        assert_eq!(
            set_from_ids(&COLUMNS, &["gone".to_owned()]),
            ColumnSet::NONE
        );
    }
}
