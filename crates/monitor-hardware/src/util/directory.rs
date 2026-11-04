use std::collections::hash_map::DefaultHasher;
use std::ffi::OsString;
use std::hash::{Hash, Hasher};
use std::path::Path;

/// A cheap fingerprint of the entries of `directory` accepted by `keep`, used
/// to notice hotplug without rebuilding state on every check.
pub(crate) fn signature(directory: &Path, keep: impl Fn(&str) -> bool) -> u64 {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return 0;
    };
    let mut names: Vec<OsString> = entries
        .flatten()
        .map(|entry| entry.file_name())
        .filter(|name| name.to_str().is_some_and(&keep))
        .collect();
    names.sort_unstable();
    let mut hasher = DefaultHasher::new();
    names.hash(&mut hasher);
    hasher.finish()
}

/// Entries of `directory` whose names satisfy `keep`, sorted by name.
pub(crate) fn entries(directory: &Path, keep: impl Fn(&str) -> bool) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| keep(name))
        .collect();
    names.sort_unstable_by(|left, right| natural_order(left, right));
    names
}

/// Orders `hwmon10` after `hwmon9`.
fn natural_order(left: &str, right: &str) -> std::cmp::Ordering {
    let split = |name: &str| {
        let digits = name.len() - name.trim_end_matches(|c: char| c.is_ascii_digit()).len();
        let (stem, number) = name.split_at(name.len() - digits);
        (stem.to_owned(), number.parse::<u64>().unwrap_or(0))
    };
    split(left).cmp(&split(right))
}

/// True for `prefix` followed by one or more digits, like `card0` or `hwmon12`.
pub(crate) fn is_numbered(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix)
        .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_numbered_entries() {
        assert!(is_numbered("card0", "card"));
        assert!(is_numbered("hwmon12", "hwmon"));
        assert!(!is_numbered("card0-DP-1", "card"));
        assert!(!is_numbered("card", "card"));
        assert!(!is_numbered("renderD128", "card"));
    }

    #[test]
    fn sorts_naturally() {
        let mut names = ["hwmon10", "hwmon2", "hwmon1"];
        names.sort_by(|left, right| natural_order(left, right));
        assert_eq!(names, ["hwmon1", "hwmon2", "hwmon10"]);
    }
}
