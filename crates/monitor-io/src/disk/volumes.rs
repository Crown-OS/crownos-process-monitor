//! Mounted filesystems, one entry per filesystem: bind mounts and btrfs
//! subvolumes of the same superblock share a device number and collapse
//! into one volume with several mount points.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use monitor_core::util::parse;
use monitor_core::{Bytes, Coverage};

use super::device;
use super::mountinfo::{self, MountClass, MountEntry};
use crate::util::text::{file_name, unescape_octal, unescape_udev};

const BY_LABEL: &str = "/dev/disk/by-label";
const SYS_DEV_BLOCK: &str = "/sys/dev/block";
const SYS_CLASS_BLOCK: &str = "/sys/class/block";
const APPROXIMATE_FILESYSTEMS: [&str; 3] = ["btrfs", "bcachefs", "zfs"];
const FILESYSTEM_ROOT: &str = "/";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DeviceNumber {
    pub major: u32,
    pub minor: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum VolumeRole {
    #[default]
    User,
    /// Boot and firmware partitions; the UI may hide them behind a switch.
    System,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum VolumeLocation {
    #[default]
    Local,
    /// Network filesystems are never `statvfs`ed on the sampler thread: a
    /// stale server would hang it.
    Remote,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MountPoint {
    pub path: Arc<str>,
    /// The directory of the filesystem mounted here (`/@home` for a btrfs
    /// subvolume, a subdirectory for a bind mount, `/` otherwise).
    pub root: Arc<str>,
    pub read_only: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Capacity {
    pub size: Bytes,
    pub used: Bytes,
    pub free: Bytes,
    /// Free space usable by unprivileged users.
    pub available: Bytes,
    /// `Estimated` for filesystems whose `statvfs` is approximate (btrfs).
    pub coverage: Coverage,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Volume {
    pub device: DeviceNumber,
    pub source: Arc<str>,
    pub fs_type: Arc<str>,
    pub label: Option<Arc<str>>,
    /// Kernel name of the block device, e.g. `nvme0n1p3` or `dm-0`.
    pub block_device: Option<Arc<str>>,
    /// The whole disk the block device is a partition of.
    pub disk: Option<Arc<str>>,
    /// The primary mount point comes first.
    pub mount_points: Vec<MountPoint>,
    pub read_only: bool,
    pub removable: bool,
    pub role: VolumeRole,
    pub location: VolumeLocation,
    pub capacity: Option<Capacity>,
}

impl Volume {
    pub fn primary_mount_point(&self) -> Option<&MountPoint> {
        self.mount_points.first()
    }
}

/// Builds the volume list from mountinfo text. Allocates; runs only when
/// the mount table changed.
pub(crate) fn build(mountinfo: &[u8]) -> Vec<Volume> {
    let labels = labels();
    let mut volumes: Vec<Volume> = Vec::new();
    for entry in parse::lines(mountinfo).filter_map(mountinfo::parse_line) {
        let class = entry.class();
        if class == MountClass::Hidden {
            continue;
        }
        let device = DeviceNumber {
            major: entry.major,
            minor: entry.minor,
        };
        let mount_point = MountPoint {
            path: Arc::from(unescape_octal(entry.mount_point)),
            root: Arc::from(unescape_octal(entry.root)),
            read_only: entry.read_only,
        };
        match volumes.iter_mut().find(|volume| volume.device == device) {
            Some(volume) => add_mount_point(volume, mount_point),
            None => volumes.push(new_volume(&entry, class, device, mount_point, &labels)),
        }
    }
    volumes
}

fn add_mount_point(volume: &mut Volume, mount_point: MountPoint) {
    if volume
        .mount_points
        .iter()
        .any(|known| known.path == mount_point.path)
    {
        return;
    }
    volume.mount_points.push(mount_point);
    volume
        .mount_points
        .sort_by_key(|point| (&*point.root != FILESYSTEM_ROOT, point.path.len()));
    volume.read_only = volume.mount_points.iter().all(|point| point.read_only);
}

fn new_volume(
    entry: &MountEntry<'_>,
    class: MountClass,
    device: DeviceNumber,
    mount_point: MountPoint,
    labels: &HashMap<String, String>,
) -> Volume {
    let source = unescape_octal(entry.source);
    let location = if class == MountClass::Remote {
        VolumeLocation::Remote
    } else {
        VolumeLocation::Local
    };
    let block_device = (location == VolumeLocation::Local)
        .then(|| block_device(device, &source))
        .flatten();
    let disk = block_device.as_deref().and_then(parent_disk);
    let label = block_device
        .as_deref()
        .and_then(|name| labels.get(name))
        .map(|label| Arc::from(label.as_str()));
    let removable = disk
        .as_deref()
        .or(block_device.as_deref())
        .is_some_and(device::is_removable);
    Volume {
        device,
        source: Arc::from(source),
        fs_type: Arc::from(String::from_utf8_lossy(entry.fs_type).as_ref()),
        label,
        block_device: block_device.map(Arc::from),
        disk: disk.map(Arc::from),
        read_only: mount_point.read_only,
        mount_points: vec![mount_point],
        removable,
        role: if class == MountClass::System {
            VolumeRole::System
        } else {
            VolumeRole::User
        },
        location,
        capacity: None,
    }
}

/// `/sys/dev/block/MAJ:MIN` names the device; btrfs and other anonymous
/// superblocks (major 0) fall back to resolving the source path.
fn block_device(device: DeviceNumber, source: &str) -> Option<String> {
    let link = PathBuf::from(format!("{SYS_DEV_BLOCK}/{}:{}", device.major, device.minor));
    if let Ok(target) = std::fs::read_link(link) {
        return file_name(&target).map(str::to_owned);
    }
    source
        .starts_with("/dev/")
        .then(|| std::fs::canonicalize(source).ok())
        .flatten()
        .and_then(|real| file_name(&real).map(str::to_owned))
}

fn parent_disk(block_device: &str) -> Option<String> {
    let class_entry = Path::new(SYS_CLASS_BLOCK).join(block_device);
    if !class_entry.join("partition").exists() {
        return Some(block_device.to_owned());
    }
    let real = std::fs::canonicalize(class_entry).ok()?;
    real.parent().and_then(file_name).map(str::to_owned)
}

/// Kernel device name → filesystem label, from udev's by-label symlinks.
fn labels() -> HashMap<String, String> {
    let Ok(entries) = std::fs::read_dir(BY_LABEL) else {
        return HashMap::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let target = std::fs::read_link(entry.path()).ok()?;
            let device = file_name(&target)?.to_owned();
            let label = unescape_udev(entry.file_name().to_str()?);
            Some((device, label))
        })
        .collect()
}

/// Refreshes `statvfs` figures of local volumes. Remote ones keep `None`.
pub(crate) fn refresh_capacity(volumes: &mut [Volume]) {
    for volume in volumes
        .iter_mut()
        .filter(|volume| volume.location == VolumeLocation::Local)
    {
        let coverage = if APPROXIMATE_FILESYSTEMS.contains(&&*volume.fs_type) {
            Coverage::Estimated
        } else {
            Coverage::Full
        };
        volume.capacity = volume
            .primary_mount_point()
            .and_then(|point| rustix::fs::statvfs(&*point.path).ok())
            .map(|stats| {
                let block = stats.f_frsize.max(1);
                let size = stats.f_blocks.saturating_mul(block);
                let free = stats.f_bfree.saturating_mul(block);
                Capacity {
                    size: Bytes::new(size),
                    used: Bytes::new(size.saturating_sub(free)),
                    free: Bytes::new(free),
                    available: Bytes::new(stats.f_bavail.saturating_mul(block)),
                    coverage,
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOUNTINFO: &[u8] = b"40 1 259:2 / / rw,relatime shared:1 - ext4 /dev/nvme1n1p6 rw
51 40 0:28 / /run rw,nosuid,nodev shared:12 - tmpfs tmpfs rw,size=3104140k,nr_inodes=819200,mode=755,inode64,huge=advise
57 40 259:1 / /boot rw,relatime shared:115 - vfat /dev/nvme1n1p5 rw,fmask=0022,dmask=0022,codepage=437,iocharset=ascii,shortname=mixed,utf8,errors=remount-ro
61 40 259:4 / /e rw,noatime shared:119 - ext4 /dev/nvme0n1p3 rw
72 40 259:4 /projects /srv/projects ro,noatime shared:119 - ext4 /dev/nvme0n1p3 rw
70 40 0:45 /@home /home rw,relatime shared:60 - btrfs /dev/sdz2 rw,ssd,space_cache=v2,subvolid=257,subvol=/@home
71 40 0:45 /@ /mnt/root rw,relatime shared:61 - btrfs /dev/sdz2 rw,ssd,space_cache=v2,subvolid=256,subvol=/@
81 40 0:70 / /home/u/remote rw,nosuid,nodev,relatime shared:71 - fuse.sshfs u@host:/srv rw,user_id=1000
298 68 0:63 / /run/user/1000/doc rw,nosuid,nodev,relatime shared:444 - fuse.portal portal rw,user_id=1000,group_id=1000
";

    #[test]
    fn dedupes_bind_mounts_and_subvolumes() {
        let volumes = build(MOUNTINFO);
        let summary: Vec<_> = volumes
            .iter()
            .map(|volume| {
                let paths: Vec<_> = volume
                    .mount_points
                    .iter()
                    .map(|point| &*point.path)
                    .collect();
                (&*volume.fs_type, paths, volume.role, volume.location)
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("ext4", vec!["/"], VolumeRole::User, VolumeLocation::Local),
                (
                    "vfat",
                    vec!["/boot"],
                    VolumeRole::System,
                    VolumeLocation::Local
                ),
                (
                    "ext4",
                    vec!["/e", "/srv/projects"],
                    VolumeRole::User,
                    VolumeLocation::Local
                ),
                (
                    "btrfs",
                    vec!["/home", "/mnt/root"],
                    VolumeRole::User,
                    VolumeLocation::Local
                ),
                (
                    "fuse.sshfs",
                    vec!["/home/u/remote"],
                    VolumeRole::User,
                    VolumeLocation::Remote
                ),
            ]
        );
        assert!(
            !volumes[2].read_only,
            "one writable mount keeps the volume writable"
        );
        assert_eq!(&*volumes[3].mount_points[0].root, "/@home");
    }

    #[test]
    fn never_measures_remote_volumes() {
        let mut volumes = build(MOUNTINFO);
        refresh_capacity(&mut volumes);
        let remote = volumes
            .iter()
            .find(|volume| volume.location == VolumeLocation::Remote);
        assert!(remote.is_some_and(|volume| volume.capacity.is_none()));
        let root = volumes.first();
        assert!(root.is_some_and(|volume| {
            volume
                .capacity
                .is_some_and(|capacity| capacity.size.get() > 0)
        }));
    }

    #[test]
    fn reads_the_live_mount_table() {
        let mut buffer = Vec::new();
        let Ok(text) = monitor_core::util::sysfs::read_into("/proc/self/mountinfo", &mut buffer)
        else {
            return;
        };
        let volumes = build(text);
        let root = volumes
            .iter()
            .find(|volume| volume.mount_points.iter().any(|point| &*point.path == "/"));
        assert!(root.is_some());
        if let Some(workspace) = volumes
            .iter()
            .find(|volume| volume.block_device.as_deref() == Some("nvme0n1p3"))
        {
            assert_eq!(workspace.disk.as_deref(), Some("nvme0n1"));
            assert_eq!(workspace.label.as_deref(), Some("Workspace"));
        }
    }
}
