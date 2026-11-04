//! Readers for pseudo-files. Hot files stay open and are re-read with
//! `pread`, which makes the kernel regenerate their content without another
//! `open`/`close` pair per sample.

use std::os::fd::{AsFd, OwnedFd};
use std::path::Path;

use rustix::fs::{Mode, OFlags};

use super::parse;
use crate::{SourceError, SourceResult};

const INITIAL_BUFFER: usize = 4096;
const SMALL_FILE: usize = 128;

impl AsFd for PersistentFile {
    fn as_fd(&self) -> std::os::fd::BorrowedFd<'_> {
        self.fd.as_fd()
    }
}

fn read_flags() -> OFlags {
    OFlags::RDONLY | OFlags::CLOEXEC
}

/// A pseudo-file kept open across samples.
#[derive(Debug)]
pub struct PersistentFile {
    fd: OwnedFd,
}

impl PersistentFile {
    pub fn open(path: impl AsRef<Path>) -> SourceResult<Self> {
        let path = path.as_ref();
        rustix::fs::open(path, read_flags(), Mode::empty())
            .map(|fd| Self { fd })
            .map_err(|errno| SourceError::io(path.display().to_string(), errno))
    }

    /// Opens `name` relative to an already-open directory (e.g. `/proc/<pid>`).
    pub fn open_at(directory: impl AsFd, name: &str) -> SourceResult<Self> {
        rustix::fs::openat(directory, name, read_flags(), Mode::empty())
            .map(|fd| Self { fd })
            .map_err(|errno| SourceError::io(name.to_owned(), errno))
    }

    /// Reads the whole file into `buffer`, growing it only when the content
    /// no longer fits, and returns the bytes read.
    pub fn read<'b>(&self, buffer: &'b mut Vec<u8>) -> SourceResult<&'b [u8]> {
        read_fd(&self.fd, buffer)
    }

    pub fn read_u64(&self) -> SourceResult<u64> {
        let mut bytes = [0_u8; SMALL_FILE];
        let length = rustix::io::pread(&self.fd, &mut bytes[..], 0)
            .map_err(|errno| SourceError::io("persistent file", errno))?;
        parse::parse_u64(&bytes[..length]).ok_or(SourceError::parse("integer attribute"))
    }

    pub fn read_i64(&self) -> SourceResult<i64> {
        let mut bytes = [0_u8; SMALL_FILE];
        let length = rustix::io::pread(&self.fd, &mut bytes[..], 0)
            .map_err(|errno| SourceError::io("persistent file", errno))?;
        parse::parse_i64(&bytes[..length]).ok_or(SourceError::parse("integer attribute"))
    }
}

fn read_fd<'b>(fd: &OwnedFd, buffer: &'b mut Vec<u8>) -> SourceResult<&'b [u8]> {
    if buffer.len() < INITIAL_BUFFER {
        buffer.resize(INITIAL_BUFFER, 0);
    }
    let mut filled = 0;
    loop {
        let read = rustix::io::pread(fd, &mut buffer[filled..], filled as u64)
            .map_err(|errno| SourceError::io("pseudo file", errno))?;
        if read == 0 {
            break;
        }
        filled += read;
        if filled == buffer.len() {
            buffer.resize(buffer.len() * 2, 0);
        }
    }
    Ok(&buffer[..filled])
}

/// One-off read of a whole file into a reusable buffer.
pub fn read_into(path: impl AsRef<Path>, buffer: &mut Vec<u8>) -> SourceResult<&[u8]> {
    let path = path.as_ref();
    let fd = rustix::fs::open(path, read_flags(), Mode::empty())
        .map_err(|errno| SourceError::io(path.display().to_string(), errno))?;
    read_fd(&fd, buffer).map_err(|error| with_path(error, path))
}

/// Reads a whole file relative to an open directory into a reusable buffer.
pub fn read_at<'b>(
    directory: impl AsFd,
    name: &str,
    buffer: &'b mut Vec<u8>,
) -> SourceResult<&'b [u8]> {
    let fd = rustix::fs::openat(directory, name, read_flags(), Mode::empty())
        .map_err(|errno| SourceError::io(name.to_owned(), errno))?;
    read_fd(&fd, buffer)
}

/// Reads a small attribute (a number, a short word) without heap allocation
/// and hands the trimmed bytes to `parse`.
pub fn read_small<T>(
    path: impl AsRef<Path>,
    parse: impl FnOnce(&[u8]) -> Option<T>,
) -> SourceResult<T> {
    let path = path.as_ref();
    let fd = rustix::fs::open(path, read_flags(), Mode::empty())
        .map_err(|errno| SourceError::io(path.display().to_string(), errno))?;
    let mut bytes = [0_u8; SMALL_FILE];
    let length = rustix::io::pread(&fd, &mut bytes[..], 0)
        .map_err(|errno| SourceError::io(path.display().to_string(), errno))?;
    parse(bytes[..length].trim_ascii()).ok_or(SourceError::parse("small attribute"))
}

pub fn read_u64(path: impl AsRef<Path>) -> SourceResult<u64> {
    read_small(path, parse::parse_u64)
}

pub fn read_i64(path: impl AsRef<Path>) -> SourceResult<i64> {
    read_small(path, parse::parse_i64)
}

/// Reads a text attribute that is kept (model names, labels). Allocates.
pub fn read_string(path: impl AsRef<Path>) -> SourceResult<String> {
    let path = path.as_ref();
    std::fs::read_to_string(path)
        .map(|text| text.trim().to_owned())
        .map_err(|error| SourceError::io(path.display().to_string(), error))
}

/// The final path component of a symlink's target, e.g. the driver name
/// behind `/sys/class/drm/card0/device/driver`.
pub fn link_name(path: impl AsRef<Path>) -> Option<String> {
    let target = std::fs::read_link(path).ok()?;
    target.file_name()?.to_str().map(str::to_owned)
}

fn with_path(error: SourceError, path: &Path) -> SourceError {
    match error {
        SourceError::Io { source, .. } => SourceError::io(path.display().to_string(), source),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rereads_persistent_proc_file() {
        let file = PersistentFile::open("/proc/self/stat");
        assert!(file.is_ok());
        let Ok(file) = file else { return };
        let mut buffer = Vec::new();
        let first = file.read(&mut buffer).map(<[u8]>::len).unwrap_or(0);
        let second = file.read(&mut buffer).map(<[u8]>::len).unwrap_or(0);
        assert!(first > 0 && second > 0);
    }

    #[test]
    fn grows_buffer_for_large_files() {
        let mut buffer = Vec::new();
        let content = read_into("/proc/self/maps", &mut buffer).map(<[u8]>::to_vec);
        assert!(content.is_ok_and(|bytes| !bytes.is_empty()));
    }

    #[test]
    fn reads_small_numbers() {
        assert!(read_u64("/proc/sys/kernel/pid_max").is_ok_and(|max| max > 0));
        assert!(read_u64("/nonexistent").is_err());
    }
}
