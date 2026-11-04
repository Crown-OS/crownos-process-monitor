use std::collections::HashMap;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Walking the tree costs about a millisecond; an unknown id triggers at
/// most one walk per interval.
const REWALK_INTERVAL: Duration = Duration::from_secs(10);

/// Maps cgroup v2 ids (the inode numbers of cgroupfs directories) to paths
/// relative to the cgroup root.
#[derive(Debug)]
pub(crate) struct CgroupPaths {
    root: PathBuf,
    by_id: HashMap<u64, Box<str>>,
    walked_at: Option<Instant>,
    walked_this_round: bool,
}

impl CgroupPaths {
    pub(crate) fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            by_id: HashMap::new(),
            walked_at: None,
            walked_this_round: false,
        }
    }

    /// Starts a round of lookups.
    pub(crate) fn begin(&mut self) {
        self.walked_this_round = false;
    }

    /// Whether an id missing now is known to be gone, not just new.
    pub(crate) fn is_fresh(&self) -> bool {
        self.walked_this_round
    }

    pub(crate) fn path_of(&mut self, cgroup_id: u64) -> Option<&str> {
        let may_rewalk = self
            .walked_at
            .is_none_or(|walked| walked.elapsed() >= REWALK_INTERVAL);
        if !self.by_id.contains_key(&cgroup_id) && may_rewalk {
            self.by_id = walk(&self.root);
            self.walked_at = Some(Instant::now());
            self.walked_this_round = true;
        }
        self.by_id.get(&cgroup_id).map(AsRef::as_ref)
    }
}

fn walk(root: &Path) -> HashMap<u64, Box<str>> {
    let mut by_id = HashMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let Ok(metadata) = std::fs::metadata(&directory) else {
            continue;
        };
        let relative = directory.strip_prefix(root).unwrap_or(Path::new(""));
        by_id.insert(
            metadata.ino(),
            format!("/{}", relative.display()).into_boxed_str(),
        );
        pending.extend(
            std::fs::read_dir(&directory)
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
                .map(|entry| entry.path()),
        );
    }
    by_id
}

#[cfg(test)]
mod tests {
    use super::*;

    const CGROUP_ROOT: &str = "/sys/fs/cgroup";
    const ROOT_CGROUP_ID: u64 = 1;

    #[test]
    fn resolves_the_live_hierarchy() {
        if !Path::new(CGROUP_ROOT).join("cgroup.controllers").exists() {
            return;
        }
        let mut paths = CgroupPaths::new(CGROUP_ROOT);
        paths.begin();
        assert_eq!(paths.path_of(ROOT_CGROUP_ID), Some("/"));
        assert!(paths.is_fresh());
        let own = std::fs::read_to_string("/proc/self/cgroup").unwrap_or_default();
        let own = own.trim().trim_start_matches("0::");
        let own_id = std::fs::metadata(format!("{CGROUP_ROOT}{own}")).map(|meta| meta.ino());
        if let Ok(own_id) = own_id {
            paths.begin();
            assert_eq!(paths.path_of(own_id), Some(own));
            assert!(!paths.is_fresh(), "known ids need no walk");
        }
        assert_eq!(paths.path_of(u64::MAX), None);
        assert!(!paths.is_fresh(), "rewalks are rate-limited");
    }
}
