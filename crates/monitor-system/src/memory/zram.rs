use std::mem::MaybeUninit;
use std::os::fd::OwnedFd;
use std::sync::Arc;

use monitor_core::Bytes;
use monitor_core::util::parse::{fields, parse_u64};
use monitor_core::util::sysfs::PersistentFile;
use rustix::fs::{Mode, OFlags, RawDir, SeekFrom};

use super::snapshot::ZramDevice;

const BLOCK_DEVICES: &str = "/sys/block";
const ZRAM_PREFIX: &[u8] = b"zram";
const DIRENT_BUFFER: usize = 4096;
const MAX_ZRAM_DEVICES: usize = 16;

struct ZramFiles {
    mm_stat: PersistentFile,
    disk_size: PersistentFile,
    algorithm: Option<PersistentFile>,
}

/// zram devices under `/sys/block`, rediscovered only when the set of
/// names changes.
pub(crate) struct ZramReader {
    directory: Option<OwnedFd>,
    dirent_buffer: Box<[MaybeUninit<u8>]>,
    files: Vec<ZramFiles>,
    names: Vec<[u8; 8]>,
    scanned: Vec<[u8; 8]>,
    buffer: Vec<u8>,
}

impl ZramReader {
    pub fn open() -> Self {
        Self {
            directory: rustix::fs::open(
                BLOCK_DEVICES,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .ok(),
            dirent_buffer: Box::new_uninit_slice(DIRENT_BUFFER),
            files: Vec::new(),
            names: Vec::new(),
            scanned: Vec::with_capacity(MAX_ZRAM_DEVICES),
            buffer: Vec::new(),
        }
    }

    /// Re-lists `/sys/block` and reopens the devices if they changed.
    pub fn rescan(&mut self, devices: &mut Vec<ZramDevice>) {
        let Some(directory) = &self.directory else {
            return;
        };
        self.scanned.clear();
        if rustix::fs::seek(directory, SeekFrom::Start(0)).is_err() {
            return;
        }
        let mut entries = RawDir::new(directory, &mut self.dirent_buffer);
        while let Some(Ok(entry)) = entries.next() {
            let name = entry.file_name().to_bytes();
            if name.starts_with(ZRAM_PREFIX)
                && name.len() <= 8
                && self.scanned.len() < MAX_ZRAM_DEVICES
            {
                let mut fixed = [0_u8; 8];
                fixed[..name.len()].copy_from_slice(name);
                self.scanned.push(fixed);
            }
        }
        self.scanned.sort_unstable();
        if self.scanned != self.names {
            self.reopen(devices);
        }
        self.refresh_algorithms(devices);
    }

    fn reopen(&mut self, devices: &mut Vec<ZramDevice>) {
        self.names.clone_from(&self.scanned);
        self.files.clear();
        devices.clear();
        for raw in &self.names {
            let name =
                String::from_utf8_lossy(raw.split(|&byte| byte == 0).next().unwrap_or_default())
                    .into_owned();
            let open = |attribute: &str| {
                PersistentFile::open(format!("{BLOCK_DEVICES}/{name}/{attribute}"))
            };
            let (Ok(mm_stat), Ok(disk_size)) = (open("mm_stat"), open("disksize")) else {
                continue;
            };
            self.files.push(ZramFiles {
                mm_stat,
                disk_size,
                algorithm: open("comp_algorithm").ok(),
            });
            devices.push(ZramDevice {
                name: name.into(),
                ..ZramDevice::default()
            });
        }
    }

    fn refresh_algorithms(&mut self, devices: &mut [ZramDevice]) {
        for (files, device) in self.files.iter().zip(devices) {
            let Some(active) = files
                .algorithm
                .as_ref()
                .and_then(|file| file.read(&mut self.buffer).ok())
                .and_then(active_algorithm)
            else {
                continue;
            };
            if device.algorithm.as_deref() != Some(active) {
                device.algorithm = Some(Arc::from(active));
            }
        }
    }

    pub fn read(&mut self, devices: &mut [ZramDevice]) {
        for (files, device) in self.files.iter().zip(devices) {
            device.disk_size = Bytes::new(files.disk_size.read_u64().unwrap_or(0));
            let Ok(stat) = files.mm_stat.read(&mut self.buffer) else {
                continue;
            };
            let mut columns = fields(stat).map(|column| Bytes::new(parse_u64(column).unwrap_or(0)));
            device.original = columns.next().unwrap_or_default();
            device.compressed = columns.next().unwrap_or_default();
            device.memory_used = columns.next().unwrap_or_default();
        }
    }
}

/// The bracketed entry of `comp_algorithm`, e.g. `zstd` in `lzo [zstd] lz4`.
fn active_algorithm(text: &[u8]) -> Option<&str> {
    let start = memchr::memchr(b'[', text)? + 1;
    let end = start + memchr::memchr(b']', &text[start..])?;
    std::str::from_utf8(&text[start..end]).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_active_algorithm() {
        assert_eq!(
            active_algorithm(b"lzo-rle lzo lz4 lz4hc [zstd] deflate 842 \n"),
            Some("zstd")
        );
        assert_eq!(active_algorithm(b"lzo lz4"), None);
    }

    #[test]
    fn parses_real_mm_stat() {
        let mm_stat = b"2915995648 712971381 737804288        0 738877440    19105        0      655      793\n";
        let values: Vec<_> = fields(mm_stat).filter_map(parse_u64).take(3).collect();
        assert_eq!(values, [2_915_995_648, 712_971_381, 737_804_288]);
        let device = ZramDevice {
            original: Bytes::new(values[0]),
            compressed: Bytes::new(values[1]),
            ..ZramDevice::default()
        };
        assert!((device.ratio() - 4.09).abs() < 0.01);
    }

    #[test]
    fn reads_live_zram_if_present() {
        let mut reader = ZramReader::open();
        let mut devices = Vec::new();
        reader.rescan(&mut devices);
        reader.read(&mut devices);
        for device in &devices {
            assert!(device.name.starts_with("zram"));
            assert!(device.original.get() == 0 || device.memory_used.get() > 0);
        }
    }
}
