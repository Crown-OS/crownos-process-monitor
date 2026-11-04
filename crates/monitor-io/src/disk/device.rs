//! Static facts about whole physical disks from `/sys/block`.

use std::path::Path;
use std::sync::Arc;

use monitor_core::util::sysfs;
use monitor_core::{Bytes, DeviceKey};

use super::stacking::{self, StackedDevice};
use crate::util::text::{file_name, read_attribute};

const SYS_BLOCK: &str = "/sys/block";
const SYS_DEVICES: &str = "/sys/devices";
const VIRTUAL_DEVICES: &str = "/sys/devices/virtual/";
const SECTOR_BYTES: u64 = 512;
const VIRTUAL_PREFIXES: [&str; 8] = ["loop", "ram", "zram", "dm-", "md", "bcache", "nbd", "drbd"];
const VPD_HEADER: usize = 4;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Transport {
    Nvme,
    Sata,
    /// SAS and other SCSI hosts.
    Scsi,
    Usb,
    Virtio,
    Mmc,
    #[default]
    Other,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MediaKind {
    Ssd,
    Hdd,
    #[default]
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Kernel name, e.g. `nvme0n1`.
    pub key: DeviceKey,
    pub model: Option<Arc<str>>,
    pub serial: Option<Arc<str>>,
    pub firmware: Option<Arc<str>>,
    pub size: Bytes,
    pub media: MediaKind,
    /// The kernel flags the medium removable, or the disk hangs off USB.
    pub removable: bool,
    pub transport: Transport,
    pub logical_sector_size: u32,
    pub physical_sector_size: u32,
    /// The active I/O scheduler (`none`, `mq-deadline`, `kyber`, `bfq`).
    pub scheduler: Option<Arc<str>>,
    pub partitions: Vec<Arc<str>>,
    pub stack: Vec<StackedDevice>,
}

/// Whole physical disks currently present, sorted by name.
pub(crate) fn physical_disks() -> Vec<Arc<str>> {
    let Ok(entries) = std::fs::read_dir(SYS_BLOCK) else {
        return Vec::new();
    };
    let mut names: Vec<Arc<str>> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            is_physical(&name).then(|| Arc::from(name))
        })
        .collect();
    names.sort_unstable();
    names
}

fn is_physical(name: &str) -> bool {
    if VIRTUAL_PREFIXES
        .iter()
        .any(|prefix| name.starts_with(prefix))
    {
        return false;
    }
    std::fs::canonicalize(Path::new(SYS_BLOCK).join(name))
        .is_ok_and(|real| real.starts_with(SYS_DEVICES) && !real.starts_with(VIRTUAL_DEVICES))
}

pub(crate) fn describe(name: &str) -> DeviceInfo {
    let directory = Path::new(SYS_BLOCK).join(name);
    let device = directory.join("device");
    let queue = directory.join("queue");
    let transport = transport(&directory);
    let partitions = partitions(&directory);
    let mut stack = Vec::new();
    stacking::collect(name, &partitions, &mut stack);
    DeviceInfo {
        key: DeviceKey::new(name),
        model: read_attribute(device.join("model")).or_else(|| read_attribute(device.join("name"))),
        serial: read_attribute(device.join("serial")).or_else(|| vpd_serial(&device)),
        firmware: ["firmware_rev", "rev", "fwrev"]
            .iter()
            .find_map(|attribute| read_attribute(device.join(attribute))),
        size: Bytes::new(
            sysfs::read_u64(directory.join("size"))
                .map_or(0, |sectors| sectors.saturating_mul(SECTOR_BYTES)),
        ),
        media: match sysfs::read_u64(queue.join("rotational")) {
            Ok(0) => MediaKind::Ssd,
            Ok(_) => MediaKind::Hdd,
            Err(_) => MediaKind::Unknown,
        },
        removable: transport == Transport::Usb || removable_flag(&directory),
        transport,
        logical_sector_size: sector_size(&queue, "logical_block_size"),
        physical_sector_size: sector_size(&queue, "physical_block_size"),
        scheduler: read_attribute(queue.join("scheduler")).and_then(|line| active_scheduler(&line)),
        partitions,
        stack,
    }
}

/// The kernel flags the medium removable, or the disk hangs off USB.
pub(crate) fn is_removable(disk: &str) -> bool {
    let directory = Path::new(SYS_BLOCK).join(disk);
    removable_flag(&directory) || transport(&directory) == Transport::Usb
}

fn removable_flag(directory: &Path) -> bool {
    sysfs::read_u64(directory.join("removable")).is_ok_and(|flag| flag == 1)
}

fn sector_size(queue: &Path, attribute: &str) -> u32 {
    sysfs::read_u64(queue.join(attribute))
        .ok()
        .and_then(|size| u32::try_from(size).ok())
        .unwrap_or(SECTOR_BYTES as u32)
}

fn partitions(directory: &Path) -> Vec<Arc<str>> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut names: Vec<Arc<str>> = entries
        .flatten()
        .filter(|entry| entry.path().join("partition").exists())
        .filter_map(|entry| entry.file_name().into_string().ok().map(Arc::from))
        .collect();
    names.sort_unstable();
    names
}

/// `none mq-deadline [kyber] bfq` → `kyber`.
fn active_scheduler(line: &str) -> Option<Arc<str>> {
    let start = line.find('[')?;
    let end = line[start..].find(']')? + start;
    Some(Arc::from(&line[start + 1..end]))
}

/// SATA and SCSI disks expose their serial only in VPD page 0x80.
fn vpd_serial(device: &Path) -> Option<Arc<str>> {
    let page = std::fs::read(device.join("vpd_pg80")).ok()?;
    let serial = std::str::from_utf8(page.get(VPD_HEADER..)?)
        .ok()?
        .trim_matches(|c: char| c.is_whitespace() || c == '\0');
    (!serial.is_empty()).then(|| Arc::from(serial))
}

/// Walks the device's ancestors and reads their `subsystem` links; the first
/// bus that identifies the transport wins (USB bridges hide behind SCSI).
fn transport(directory: &Path) -> Transport {
    let Ok(real) = std::fs::canonicalize(directory) else {
        return Transport::Other;
    };
    let mut behind_scsi = false;
    for ancestor in real
        .ancestors()
        .take_while(|path| path.starts_with(SYS_DEVICES))
    {
        let Ok(link) = std::fs::read_link(ancestor.join("subsystem")) else {
            continue;
        };
        match file_name(&link) {
            Some("usb") => return Transport::Usb,
            Some("nvme" | "nvme-subsystem") => return Transport::Nvme,
            Some("virtio") => return Transport::Virtio,
            Some("mmc") => return Transport::Mmc,
            Some("scsi") => behind_scsi = true,
            _ => {}
        }
    }
    let through_ata = real
        .components()
        .any(|component| component.as_os_str().as_encoded_bytes().starts_with(b"ata"));
    match (behind_scsi, through_ata) {
        (true, true) => Transport::Sata,
        (true, false) => Transport::Scsi,
        (false, _) => Transport::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_active_scheduler() {
        assert_eq!(
            active_scheduler("none mq-deadline [kyber] bfq").as_deref(),
            Some("kyber")
        );
        assert_eq!(
            active_scheduler("[none] mq-deadline").as_deref(),
            Some("none")
        );
        assert_eq!(active_scheduler("none"), None);
    }

    #[test]
    #[ignore = "expects the block devices of the development machine"]
    fn finds_both_nvme_drives_as_physical() {
        let disks = physical_disks();
        assert!(
            !disks
                .iter()
                .any(|name| name.starts_with("zram") || name.starts_with("loop"))
        );
        if !Path::new("/sys/block/nvme0n1").exists() {
            return;
        }
        assert!(disks.iter().any(|name| &**name == "nvme0n1"));
        assert!(disks.iter().any(|name| &**name == "nvme1n1"));
        let info = describe("nvme0n1");
        assert_eq!(info.transport, Transport::Nvme);
        assert_eq!(info.media, MediaKind::Ssd);
        assert!(info.model.is_some());
        assert!(info.size.get() > 0);
        assert!(
            info.partitions
                .iter()
                .any(|partition| &**partition == "nvme0n1p3")
        );
        assert!(!info.removable);
    }
}
