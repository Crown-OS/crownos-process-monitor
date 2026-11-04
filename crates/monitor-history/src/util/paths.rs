use std::ffi::OsString;
use std::path::PathBuf;

const APP_DIRECTORY: &str = "crownos-process-monitor";
const DATABASE_FILE: &str = "history.sqlite";
const DOWNLOAD_KEY: &str = "XDG_DOWNLOAD_DIR=";
const HOME_VARIABLE: &str = "$HOME";

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
}

fn xdg_dir(variable: &str, fallback_below_home: &str) -> Option<PathBuf> {
    std::env::var_os(variable)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| home().map(|home| home.join(fallback_below_home)))
}

/// `$XDG_STATE_HOME/crownos-process-monitor/history.sqlite`, defaulting to
/// `~/.local/state/…`.
pub fn default_database_path() -> Option<PathBuf> {
    xdg_dir("XDG_STATE_HOME", ".local/state")
        .map(|state| state.join(APP_DIRECTORY).join(DATABASE_FILE))
}

/// `XDG_DOWNLOAD_DIR` from `user-dirs.dirs`, or `~/Downloads`.
pub fn download_directory() -> Option<PathBuf> {
    let home = home()?;
    let configured = xdg_dir("XDG_CONFIG_HOME", ".config")
        .and_then(|config| std::fs::read_to_string(config.join("user-dirs.dirs")).ok())
        .and_then(|contents| parse_download_dir(&contents, &home));
    Some(configured.unwrap_or_else(|| home.join("Downloads")))
}

fn parse_download_dir(contents: &str, home: &std::path::Path) -> Option<PathBuf> {
    let value = contents
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix(DOWNLOAD_KEY))?
        .trim_matches('"');
    match value.strip_prefix(HOME_VARIABLE) {
        Some(below_home) => {
            let mut path = OsString::from(home);
            path.push(below_home);
            Some(PathBuf::from(path))
        }
        None if value.starts_with('/') => Some(PathBuf::from(value)),
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn parses_user_dirs() {
        let home = Path::new("/home/ada");
        let contents =
            "# comment\nXDG_DESKTOP_DIR=\"$HOME/Desktop\"\nXDG_DOWNLOAD_DIR=\"$HOME/Fetched\"\n";
        assert_eq!(
            parse_download_dir(contents, home),
            Some(PathBuf::from("/home/ada/Fetched"))
        );
        assert_eq!(
            parse_download_dir("XDG_DOWNLOAD_DIR=\"/data/dl\"", home),
            Some(PathBuf::from("/data/dl"))
        );
        assert_eq!(parse_download_dir("XDG_MUSIC_DIR=\"$HOME/M\"", home), None);
    }
}
