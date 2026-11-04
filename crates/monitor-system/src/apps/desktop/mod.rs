use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use monitor_core::util::cadence::Cadence;

use directories::{application_directories, modification_stamp, scan_directory};
use entry::GENERIC_LAUNCHERS;

pub(crate) use entry::DesktopEntry;
pub(crate) use fields::{exec_program, parse_desktop_fields};

mod directories;
mod entry;
mod fields;
#[cfg(test)]
mod fixtures;

const CHANGE_CHECK_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Debug, Default)]
struct Lookup(HashMap<String, usize>);

impl Lookup {
    /// Visible entries without extra arguments win; otherwise first wins.
    fn insert(&mut self, key: &str, index: usize, entries: &[DesktopEntry]) {
        let key = key.to_lowercase();
        match self.0.get(&key) {
            Some(&existing) if entries[existing].rank() <= entries[index].rank() => {}
            _ => {
                self.0.insert(key, index);
            }
        }
    }

    fn get(&self, key: &str) -> Option<usize> {
        self.0.get(&key.to_lowercase()).copied()
    }
}

/// Every installed `.desktop` application, indexed by id, window class and
/// program name. Rebuilt when an applications directory changes.
#[derive(Debug, Default)]
pub(crate) struct DesktopIndex {
    entries: Vec<DesktopEntry>,
    by_id: Lookup,
    by_id_tail: Lookup,
    by_wm_class: Lookup,
    by_program: Lookup,
    directories: Vec<(PathBuf, Option<(i64, u64)>)>,
    change_checks: Cadence,
}

impl DesktopIndex {
    pub fn load() -> Self {
        Self::load_from(application_directories())
    }

    pub fn load_from(directories: Vec<PathBuf>) -> Self {
        let mut index = Self::default();
        for directory in &directories {
            scan_directory(directory, "", &mut index.entries);
        }
        let mut seen = HashSet::new();
        index.entries.retain(|entry| seen.insert(entry.id.clone()));
        index.build_lookups();
        index.directories = directories
            .into_iter()
            .map(|directory| {
                let stamp = modification_stamp(&directory);
                (directory, stamp)
            })
            .collect();
        index
    }

    fn build_lookups(&mut self) {
        let entries = &self.entries;
        for (index, entry) in entries.iter().enumerate() {
            self.by_id.insert(&entry.id, index, entries);
            if let Some(tail) = entry
                .id
                .rsplit('.')
                .next()
                .filter(|tail| *tail != &*entry.id)
            {
                self.by_id_tail.insert(tail, index, entries);
            }
            if let Some(class) = &entry.wm_class {
                self.by_wm_class.insert(class, index, entries);
            }
            for program in [&entry.program, &entry.try_exec].into_iter().flatten() {
                self.by_program.insert(program, index, entries);
            }
        }
    }

    /// Re-stats the directories every [`CHANGE_CHECK_INTERVAL`] and rebuilds
    /// when one changed. Returns whether it rebuilt.
    pub fn refresh_if_changed(&mut self, now: Duration) -> bool {
        if !self.change_checks.due(now, CHANGE_CHECK_INTERVAL) {
            return false;
        }
        let changed = self
            .directories
            .iter()
            .any(|(directory, stamp)| modification_stamp(directory) != *stamp);
        if changed {
            let directories = std::mem::take(&mut self.directories)
                .into_iter()
                .map(|(directory, _)| directory)
                .collect();
            let change_checks = self.change_checks;
            *self = Self::load_from(directories);
            self.change_checks = change_checks;
        }
        changed
    }

    /// An app id from a cgroup or sandbox: matched against file ids, then
    /// window classes.
    pub fn find_app_id(&self, id: &str) -> Option<&DesktopEntry> {
        self.by_id
            .get(id)
            .or_else(|| self.by_wm_class.get(id))
            .map(|index| &self.entries[index])
    }

    /// A process by the names it goes by (comm, executable basename, argv0).
    /// Hidden helper entries (URL handlers and the like) never claim a
    /// program.
    pub fn find_program<'a>(
        &self,
        names: impl IntoIterator<Item = &'a str> + Clone,
    ) -> Option<&DesktopEntry> {
        let lookups = [
            &self.by_id,
            &self.by_wm_class,
            &self.by_program,
            &self.by_id_tail,
        ];
        lookups
            .iter()
            .find_map(|lookup| {
                names
                    .clone()
                    .into_iter()
                    .filter(|name| !name.is_empty() && !GENERIC_LAUNCHERS.contains(name))
                    .filter_map(|name| lookup.get(name))
                    .find(|&index| !self.entries[index].no_display)
            })
            .map(|index| &self.entries[index])
    }
}

#[cfg(test)]
mod tests {
    use super::fields::parse_desktop_fields;
    use super::fixtures::{KITTY, ORCA, URL_HANDLER, VM_LAUNCHER};
    use super::*;

    fn fixture_index() -> DesktopIndex {
        let entries = [
            ("macos-vm-sonoma", VM_LAUNCHER),
            ("kitty", KITTY),
            ("stably-orca", ORCA),
            ("claude-code-url-handler", URL_HANDLER),
        ]
        .into_iter()
        .filter_map(|(id, text)| {
            DesktopEntry::from_fields(id, PathBuf::from(id), parse_desktop_fields(text))
        })
        .collect();
        let mut index = DesktopIndex {
            entries,
            ..DesktopIndex::default()
        };
        index.build_lookups();
        index
    }

    #[test]
    fn matches_apps_and_programs() {
        let index = fixture_index();
        assert_eq!(
            index.find_app_id("orca").map(|entry| &*entry.id),
            Some("stably-orca")
        );
        assert_eq!(
            index.find_app_id("Kitty").map(|entry| &*entry.name),
            Some("kitty")
        );
        assert_eq!(
            index.find_program(["kitty"]).map(|entry| &*entry.id),
            Some("kitty")
        );
        assert_eq!(
            index
                .find_program(["orca-ide", "orca"])
                .map(|entry| &*entry.id),
            Some("stably-orca")
        );
        assert!(index.find_program(["bash"]).is_none());
        assert!(
            index.find_program(["claude"]).is_none(),
            "hidden handlers never claim a program"
        );
        assert!(index.find_app_id("claude-code-url-handler").is_some());
    }

    #[test]
    fn loads_live_index() {
        let mut index = DesktopIndex::load();
        let changed_at_start = index.refresh_if_changed(Duration::from_secs(1));
        assert!(!changed_at_start || !index.entries.is_empty());
        assert!(
            !index.refresh_if_changed(Duration::from_secs(2)),
            "checked recently"
        );
    }
}
