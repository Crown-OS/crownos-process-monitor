//! Zero-copy parsing and classification of `/proc/self/mountinfo` lines,
//! after the logic of `crownos-files/src/fs/mounts.rs`.

use monitor_core::util::parse;

const PSEUDO_FILESYSTEMS: &[&[u8]] = &[
    b"autofs",
    b"binfmt_misc",
    b"bpf",
    b"cgroup",
    b"cgroup2",
    b"configfs",
    b"debugfs",
    b"devpts",
    b"devtmpfs",
    b"efivarfs",
    b"fusectl",
    b"hugetlbfs",
    b"mqueue",
    b"nsfs",
    b"proc",
    b"pstore",
    b"ramfs",
    b"rpc_pipefs",
    b"securityfs",
    b"selinuxfs",
    b"squashfs",
    b"sysfs",
    b"tmpfs",
    b"tracefs",
    b"overlay",
    b"erofs",
    b"nfsd",
];
const HIDDEN_ROOTS: &[&[u8]] = &[
    b"/proc",
    b"/sys",
    b"/dev",
    b"/run/user",
    b"/run/credentials",
    b"/run/netns",
    b"/var/lib/snapd",
    b"/snap",
    b"/var/lib/docker",
    b"/var/lib/containers",
    b"/var/lib/waydroid",
];
const SYSTEM_ROOTS: &[&[u8]] = &[b"/boot", b"/efi", b"/recovery"];
const REMOTE_FILESYSTEMS: &[&[u8]] = &[
    b"nfs",
    b"nfs4",
    b"cifs",
    b"smb3",
    b"smbfs",
    b"9p",
    b"afs",
    b"ceph",
    b"glusterfs",
    b"davfs",
    b"sshfs",
    b"fuse.sshfs",
    b"fuse.rclone",
    b"fuse.s3fs",
    b"fuse.gcsfuse",
];
const FUSE_SUBTYPE: &[u8] = b"fuse.";
const LOOP_SOURCE: &[u8] = b"/dev/loop";
const DEVICE_SOURCE: &[u8] = b"/dev/";
const SEPARATOR: &[u8] = b" - ";

/// One mountinfo line, borrowed from the read buffer. Paths are still
/// octal-escaped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MountEntry<'a> {
    pub major: u32,
    pub minor: u32,
    pub root: &'a [u8],
    pub mount_point: &'a [u8],
    pub read_only: bool,
    pub fs_type: &'a [u8],
    pub source: &'a [u8],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MountClass {
    /// Kernel, container, snap and per-user internals: never listed.
    Hidden,
    /// Real but rarely interesting (`/boot`, `/efi`).
    System,
    User,
    Remote,
}

pub(crate) fn parse_line(line: &[u8]) -> Option<MountEntry<'_>> {
    let separator = memchr::memmem::find(line, SEPARATOR)?;
    let mut front = line[..separator].split(|&byte| byte == b' ');
    let mut back = line[separator + SEPARATOR.len()..].split(|&byte| byte == b' ');
    let _id = front.next()?;
    let _parent = front.next()?;
    let device = front.next()?;
    let colon = memchr::memchr(b':', device)?;
    let root = front.next()?;
    let mount_point = front.next()?;
    let options = front.next()?;
    Some(MountEntry {
        major: parse::parse_u32(&device[..colon])?,
        minor: parse::parse_u32(&device[colon + 1..])?,
        root,
        mount_point,
        read_only: options
            .split(|&byte| byte == b',')
            .any(|option| option == b"ro"),
        fs_type: back.next()?,
        source: back.next()?,
    })
}

impl MountEntry<'_> {
    pub(crate) fn class(&self) -> MountClass {
        if REMOTE_FILESYSTEMS.contains(&self.fs_type) {
            MountClass::Remote
        } else if self.is_hidden() {
            MountClass::Hidden
        } else if SYSTEM_ROOTS
            .iter()
            .any(|root| under(self.mount_point, root))
        {
            MountClass::System
        } else {
            MountClass::User
        }
    }

    fn is_hidden(&self) -> bool {
        PSEUDO_FILESYSTEMS.contains(&self.fs_type)
            || self.source.starts_with(LOOP_SOURCE)
            || (self.fs_type.starts_with(FUSE_SUBTYPE) && !self.source.starts_with(DEVICE_SOURCE))
            || HIDDEN_ROOTS
                .iter()
                .any(|root| under(self.mount_point, root))
    }
}

/// `path` is `root` or lies below it (`/bootstrap` is not under `/boot`).
fn under(path: &[u8], root: &[u8]) -> bool {
    path.strip_prefix(root)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(b"/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOUNTINFO: &[u8] = b"40 1 259:2 / / rw,relatime shared:1 - ext4 /dev/nvme1n1p6 rw
38 40 0:7 / /dev rw,nosuid shared:2 - devtmpfs devtmpfs rw,size=7624108k,nr_inodes=1906027,mode=755,inode64,huge=advise
44 40 0:25 / /sys rw,nosuid,nodev,noexec,relatime shared:5 - sysfs sysfs rw
46 44 0:29 / /sys/fs/cgroup rw,nosuid,nodev,noexec,relatime shared:7 - cgroup2 cgroup2 rw,nsdelegate,memory_recursiveprot,memory_hugetlb_accounting
51 40 0:28 / /run rw,nosuid,nodev shared:12 - tmpfs tmpfs rw,size=3104140k,nr_inodes=819200,mode=755,inode64,huge=advise
53 40 0:44 / /tmp rw,nosuid,nodev shared:111 - tmpfs tmpfs rw,size=7760348k,nr_inodes=1048576,inode64,huge=advise,usrquota
57 40 259:1 / /boot rw,relatime shared:115 - vfat /dev/nvme1n1p5 rw,fmask=0022,dmask=0022,codepage=437,iocharset=ascii,shortname=mixed,utf8,errors=remount-ro
61 40 259:4 / /e rw,noatime shared:119 - ext4 /dev/nvme0n1p3 rw
289 68 0:62 / /run/user/1000/gvfs rw,nosuid,nodev,relatime shared:435 - fuse.gvfsd-fuse gvfsd-fuse rw,user_id=1000,group_id=1000
298 68 0:63 / /run/user/1000/doc rw,nosuid,nodev,relatime shared:444 - fuse.portal portal rw,user_id=1000,group_id=1000
70 40 0:45 /@home /home rw,relatime shared:60 - btrfs /dev/sda2 rw,ssd,space_cache=v2,subvolid=257,subvol=/@home
71 40 0:45 /@ /mnt/root rw,relatime shared:61 - btrfs /dev/sda2 rw,ssd,space_cache=v2,subvolid=256,subvol=/@
72 40 259:4 /projects /srv/projects ro,noatime shared:119 - ext4 /dev/nvme0n1p3 rw
80 40 7:3 / /snap/core22/1380 ro,nodev,relatime shared:70 - squashfs /dev/loop3 ro
81 40 0:70 / /home/u/remote rw,nosuid,nodev,relatime shared:71 - fuse.sshfs u@host:/srv rw,user_id=1000
82 40 8:17 / /run/media/u/MY\\040STICK rw,nosuid,nodev,relatime shared:72 - exfat /dev/sdb1 rw
";

    fn entries() -> Vec<MountEntry<'static>> {
        parse::lines(MOUNTINFO).filter_map(parse_line).collect()
    }

    #[test]
    fn parses_every_line() {
        let entries = entries();
        assert_eq!(entries.len(), 16);
        let home = entries[10];
        assert_eq!((home.major, home.minor), (0, 45));
        assert_eq!(home.root, b"/@home");
        assert_eq!(home.fs_type, b"btrfs");
        assert!(entries[12].read_only);
    }

    #[test]
    fn classifies_mounts() {
        let classes: Vec<_> = entries().iter().map(MountEntry::class).collect();
        use MountClass::{Hidden, Remote, System, User};
        assert_eq!(
            classes,
            [
                User, Hidden, Hidden, Hidden, Hidden, Hidden, System, User, Hidden, Hidden, User,
                User, User, Hidden, Remote, User,
            ]
        );
    }

    #[test]
    fn matches_whole_path_components() {
        assert!(under(b"/boot/efi", b"/boot"));
        assert!(under(b"/boot", b"/boot"));
        assert!(!under(b"/bootstrap", b"/boot"));
    }
}
