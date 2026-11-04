//! Thin wrappers over `monitor_core::util::sysfs` for attributes that may be
//! missing on a given driver: absence is `None`, never an error.

use std::path::Path;
use std::sync::Arc;

use monitor_core::util::parse;
use monitor_core::util::sysfs::{self, PersistentFile};

pub(crate) fn open(path: impl AsRef<Path>) -> Option<PersistentFile> {
    PersistentFile::open(path).ok()
}

pub(crate) fn read_u64(file: Option<&PersistentFile>) -> Option<u64> {
    file?.read_u64().ok()
}

pub(crate) fn read_i64(file: Option<&PersistentFile>) -> Option<i64> {
    file?.read_i64().ok()
}

pub(crate) fn read_bytes<'b>(
    file: Option<&PersistentFile>,
    buffer: &'b mut Vec<u8>,
) -> Option<&'b [u8]> {
    file?.read(buffer).ok().map(<[u8]>::trim_ascii)
}

pub(crate) fn read_text(path: impl AsRef<Path>) -> Option<Arc<str>> {
    sysfs::read_string(path)
        .ok()
        .filter(|text| !text.is_empty())
        .map(Arc::from)
}

pub(crate) fn read_hex(path: impl AsRef<Path>) -> Option<u32> {
    sysfs::read_small(path, parse::parse_hex_u32).ok()
}

pub(crate) fn read_number(path: impl AsRef<Path>) -> Option<u64> {
    sysfs::read_u64(path).ok()
}
