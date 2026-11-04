//! Devices stacked on a disk (LUKS, LVM, MD, bcache), found through the
//! `holders` links of the disk and of its partitions.

use std::path::Path;
use std::sync::Arc;

use crate::util::text::{file_name, read_attribute};

const SYS_BLOCK: &str = "/sys/block";
const MAX_DEPTH: u8 = 4;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum StackKind {
    Luks,
    Lvm,
    Md,
    Bcache,
    DeviceMapper,
    #[default]
    Other,
}

/// A virtual block device that sits on a disk or one of its partitions. Its
/// I/O is never added to totals: the physical disk already counts it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StackedDevice {
    /// Kernel name, e.g. `dm-0` or `md127`.
    pub name: Arc<str>,
    /// Device-mapper or MD name, e.g. `luks-3f2a…` or `vg-root`.
    pub mapped_name: Option<Arc<str>>,
    pub kind: StackKind,
    /// The partition, disk or stacked device directly underneath.
    pub parent: Arc<str>,
    /// 0 for devices directly on the disk or a partition.
    pub depth: u8,
}

/// Appends everything stacked on `disk` and on its `partitions`.
pub(crate) fn collect(disk: &str, partitions: &[Arc<str>], out: &mut Vec<StackedDevice>) {
    let disk_directory = Path::new(SYS_BLOCK).join(disk);
    walk(&disk_directory, Arc::from(disk), 0, out);
    for partition in partitions {
        walk(
            &disk_directory.join(&**partition),
            Arc::clone(partition),
            0,
            out,
        );
    }
}

fn walk(directory: &Path, parent: Arc<str>, depth: u8, out: &mut Vec<StackedDevice>) {
    if depth >= MAX_DEPTH {
        return;
    }
    let Ok(holders) = std::fs::read_dir(directory.join("holders")) else {
        return;
    };
    for holder in holders.flatten() {
        let path = holder.path();
        let Some(name) = file_name(&path).map(Arc::<str>::from) else {
            continue;
        };
        let holder_directory = Path::new(SYS_BLOCK).join(&*name);
        let (kind, mapped_name) = classify(&name, &holder_directory);
        out.push(StackedDevice {
            name: Arc::clone(&name),
            mapped_name,
            kind,
            parent: Arc::clone(&parent),
            depth,
        });
        walk(&holder_directory, name, depth + 1, out);
    }
}

fn classify(name: &str, directory: &Path) -> (StackKind, Option<Arc<str>>) {
    if name.starts_with("dm-") {
        let uuid = read_attribute(directory.join("dm/uuid"));
        let kind = uuid.as_deref().map_or(StackKind::DeviceMapper, dm_kind);
        (kind, read_attribute(directory.join("dm/name")))
    } else if name.starts_with("md") {
        (StackKind::Md, read_attribute(directory.join("md/level")))
    } else if name.starts_with("bcache") {
        (StackKind::Bcache, None)
    } else {
        (StackKind::Other, None)
    }
}

fn dm_kind(uuid: &str) -> StackKind {
    if uuid.starts_with("CRYPT-") {
        StackKind::Luks
    } else if uuid.starts_with("LVM-") {
        StackKind::Lvm
    } else {
        StackKind::DeviceMapper
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_device_mapper_uuids() {
        assert_eq!(dm_kind("CRYPT-LUKS2-3f2a9c-luks-3f2a9c"), StackKind::Luks);
        assert_eq!(dm_kind("LVM-abcdefgh"), StackKind::Lvm);
        assert_eq!(dm_kind("mpath-360"), StackKind::DeviceMapper);
    }
}
