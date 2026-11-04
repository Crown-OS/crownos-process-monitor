use std::path::{Path, PathBuf};

use super::entry::DesktopEntry;
use super::fields::parse_desktop_fields;

const DESKTOP_SUFFIX: &str = ".desktop";
const DEFAULT_DATA_DIRS: &str = "/usr/local/share:/usr/share";
const EXTRA_APPLICATION_DIRS: [&str; 2] = [
    "/var/lib/flatpak/exports/share/applications",
    "/var/lib/snapd/desktop/applications",
];

pub(super) fn scan_directory(directory: &Path, prefix: &str, entries: &mut Vec<DesktopEntry>) {
    let Ok(listing) = std::fs::read_dir(directory) else {
        return;
    };
    let mut paths: Vec<PathBuf> = listing.flatten().map(|entry| entry.path()).collect();
    paths.sort();
    for path in paths {
        let Some(file_name) = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
        else {
            continue;
        };
        if path.is_dir() {
            if prefix.is_empty() {
                scan_directory(&path, &format!("{file_name}-"), entries);
            }
            continue;
        }
        let Some(stem) = file_name.strip_suffix(DESKTOP_SUFFIX) else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let id = format!("{prefix}{stem}");
        if let Some(entry) = DesktopEntry::from_fields(&id, path, parse_desktop_fields(&text)) {
            entries.push(entry);
        }
    }
}

pub(super) fn modification_stamp(directory: &Path) -> Option<(i64, u64)> {
    rustix::fs::stat(directory)
        .ok()
        .map(|stat| (stat.st_mtime, stat.st_mtime_nsec))
}

/// `$XDG_DATA_HOME/applications`, each `$XDG_DATA_DIRS/applications`, and the
/// Flatpak and Snap export directories, most specific first.
pub(crate) fn application_directories() -> Vec<PathBuf> {
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")));
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|dirs| !dirs.is_empty())
        .unwrap_or_else(|| DEFAULT_DATA_DIRS.to_owned());
    let mut directories: Vec<PathBuf> = Vec::new();
    if let Some(home) = &data_home {
        directories.push(home.join("applications"));
        directories.push(home.join("flatpak/exports/share/applications"));
    }
    directories.extend(
        data_dirs
            .split(':')
            .filter(|dir| !dir.is_empty())
            .map(|dir| Path::new(dir).join("applications")),
    );
    directories.extend(EXTRA_APPLICATION_DIRS.iter().map(PathBuf::from));
    let mut unique = Vec::with_capacity(directories.len());
    for directory in directories {
        let normalized: PathBuf = directory.components().collect();
        if !unique.contains(&normalized) {
            unique.push(normalized);
        }
    }
    unique
}
