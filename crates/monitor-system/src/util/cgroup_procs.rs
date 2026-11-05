use std::fs;
use std::path::{Path, PathBuf};

const PROCS_FILE: &str = "cgroup.procs";

/// How many processes `root` and every cgroup below it hold, which is what
/// `cgroup.kill` and `cgroup.freeze` reach. `None` when `root` has no readable
/// `cgroup.procs`. A descendant that cannot be read, such as a threaded
/// cgroup whose processes belong to its thread root, counts as empty.
pub(crate) fn subtree_process_count(root: &Path) -> Option<usize> {
    let mut total = process_count(root)?;
    let mut pending = Vec::new();
    push_children(root, &mut pending);
    while let Some(cgroup) = pending.pop() {
        total += process_count(&cgroup).unwrap_or_default();
        push_children(&cgroup, &mut pending);
    }
    Some(total)
}

fn process_count(cgroup: &Path) -> Option<usize> {
    let procs = fs::read(cgroup.join(PROCS_FILE)).ok()?;
    Some(memchr::memchr_iter(b'\n', &procs).count())
}

fn push_children(cgroup: &Path, pending: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(cgroup) else {
        return;
    };
    pending.extend(
        entries
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .map(|entry| entry.path()),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_every_descendant_like_cgroup_kill() -> std::io::Result<()> {
        let root = std::env::temp_dir().join(format!("monitor-cgroup-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("system/uid_10071/pid_812"))?;
        fs::create_dir_all(root.join("threaded"))?;
        fs::write(root.join(PROCS_FILE), "1\n")?;
        fs::write(root.join("system").join(PROCS_FILE), "")?;
        fs::write(root.join("system/uid_10071").join(PROCS_FILE), "77\n78\n")?;
        fs::write(
            root.join("system/uid_10071/pid_812").join(PROCS_FILE),
            "812\n",
        )?;

        assert_eq!(subtree_process_count(&root), Some(4));
        assert_eq!(subtree_process_count(&root.join("threaded")), None);
        assert_eq!(subtree_process_count(&root.join("missing")), None);

        fs::remove_dir_all(&root)
    }
}
