//! Where exports land: `$XDG_DOWNLOAD_DIR`, else the user-dirs entry, else `~/Downloads`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

const DOWNLOAD_KEY: &str = "XDG_DOWNLOAD_DIR";
const USER_DIRS_FILE: &str = "user-dirs.dirs";
const DEFAULT_FOLDER: &str = "Downloads";
const HOME_VARIABLE: &str = "$HOME";
const MAX_NAME_CHARS: usize = 48;

/// The download folder from what the environment provides; pure so it can be tested.
pub fn resolve_download_dir(
    variable: Option<OsString>,
    user_dirs: Option<&str>,
    home: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(path) = variable.filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(path));
    }
    let configured = user_dirs.and_then(|text| {
        text.lines()
            .filter_map(|line| line.trim().strip_prefix(DOWNLOAD_KEY))
            .filter_map(|rest| rest.trim_start().strip_prefix('='))
            .map(|value| value.trim().trim_matches('"'))
            .find(|value| !value.is_empty())
    });
    match (configured, home) {
        (Some(value), Some(home)) => Some(match value.strip_prefix(HOME_VARIABLE) {
            Some(rest) => home.join(rest.trim_start_matches('/')),
            None => PathBuf::from(value),
        }),
        (Some(value), None) if !value.starts_with(HOME_VARIABLE) => Some(PathBuf::from(value)),
        (_, home) => home.map(|home| home.join(DEFAULT_FOLDER)),
    }
}

/// Reads the environment and `~/.config/user-dirs.dirs`; touches the disk, so call it off the
/// UI thread.
pub fn download_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| home.as_ref().map(|home| home.join(".config")));
    let user_dirs =
        config.and_then(|config| std::fs::read_to_string(config.join(USER_DIRS_FILE)).ok());
    resolve_download_dir(
        std::env::var_os(DOWNLOAD_KEY),
        user_dirs.as_deref(),
        home.as_deref(),
    )
}

/// `firefox-4242.csv`: the name cut to safe file-name characters.
pub fn export_file_name(name: &str, pid: u32, extension: &str) -> String {
    let safe: String = name
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .take(MAX_NAME_CHARS)
        .collect();
    let stem = safe.trim_matches(|character| character == '.' || character == '_');
    let stem = if stem.is_empty() { "process" } else { stem };
    format!("{stem}-{pid}.{extension}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "/home/marvel";

    #[test]
    fn the_variable_wins() {
        let resolved = resolve_download_dir(
            Some("/data/dl".into()),
            Some("XDG_DOWNLOAD_DIR=\"$HOME/Other\""),
            Some(Path::new(HOME)),
        );
        assert_eq!(resolved, Some(PathBuf::from("/data/dl")));
    }

    #[test]
    fn user_dirs_expand_home() {
        let file = "# written by xdg-user-dirs-update\nXDG_DESKTOP_DIR=\"$HOME/Desktop\"\nXDG_DOWNLOAD_DIR=\"$HOME/Downloads/Mine\"\n";
        let resolved = resolve_download_dir(None, Some(file), Some(Path::new(HOME)));
        assert_eq!(resolved, Some(PathBuf::from("/home/marvel/Downloads/Mine")));
        let absolute = resolve_download_dir(
            Some(OsString::new()),
            Some("XDG_DOWNLOAD_DIR=\"/srv/drop\""),
            None,
        );
        assert_eq!(absolute, Some(PathBuf::from("/srv/drop")));
    }

    #[test]
    fn falls_back_to_home_downloads() {
        let resolved = resolve_download_dir(
            None,
            Some("XDG_MUSIC_DIR=\"$HOME/Music\""),
            Some(Path::new(HOME)),
        );
        assert_eq!(resolved, Some(PathBuf::from("/home/marvel/Downloads")));
        assert_eq!(resolve_download_dir(None, None, None), None);
    }

    #[test]
    fn export_names_are_safe() {
        assert_eq!(
            export_file_name("Web Content", 42, "csv"),
            "Web_Content-42.csv"
        );
        assert_eq!(export_file_name("../..", 7, "csv"), "process-7.csv");
        assert_eq!(
            export_file_name("kworker/0:1", 9, "csv"),
            "kworker_0_1-9.csv"
        );
    }
}
