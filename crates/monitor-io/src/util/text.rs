use std::path::Path;
use std::sync::Arc;

use monitor_core::util::sysfs;

/// A kept text attribute; empty files read as `None`.
pub(crate) fn read_attribute(path: impl AsRef<Path>) -> Option<Arc<str>> {
    let text = sysfs::read_string(path).ok()?;
    (!text.is_empty()).then(|| Arc::from(text))
}

/// Undoes the octal escapes of `/proc/self/mountinfo` (`\040` is a space).
pub(crate) fn unescape_octal(field: &[u8]) -> String {
    let mut decoded = Vec::with_capacity(field.len());
    let mut rest = field;
    while let Some((&byte, tail)) = rest.split_first() {
        match (byte, tail) {
            (b'\\', [high, middle, low, after @ ..])
                if [high, middle, low]
                    .iter()
                    .all(|digit| (b'0'..=b'7').contains(digit)) =>
            {
                decoded.push(((high - b'0') << 6) | ((middle - b'0') << 3) | (low - b'0'));
                rest = after;
            }
            _ => {
                decoded.push(byte);
                rest = tail;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

/// Undoes udev's `\xHH` escapes in `/dev/disk/by-label` names.
pub(crate) fn unescape_udev(name: &str) -> String {
    let bytes = name.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut rest = bytes;
    while let Some((&byte, tail)) = rest.split_first() {
        let escaped = match tail {
            [b'x', high, low, after @ ..] if byte == b'\\' => std::str::from_utf8(&[*high, *low])
                .ok()
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
                .map(|value| (value, after)),
            _ => None,
        };
        match escaped {
            Some((value, after)) => {
                decoded.push(value);
                rest = after;
            }
            None => {
                decoded.push(byte);
                rest = tail;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

/// The last path component of `path`, as UTF-8.
pub(crate) fn file_name(path: &Path) -> Option<&str> {
    path.file_name()?.to_str()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unescapes_mountinfo_fields() {
        assert_eq!(
            unescape_octal(br"/run/media/u/MY\040STICK"),
            "/run/media/u/MY STICK"
        );
        assert_eq!(unescape_octal(br"/plain"), "/plain");
        assert_eq!(unescape_octal(br"/trailing\04"), r"/trailing\04");
    }

    #[test]
    fn unescapes_udev_labels() {
        assert_eq!(unescape_udev(r"My\x20Disk"), "My Disk");
        assert_eq!(unescape_udev("Workspace"), "Workspace");
        assert_eq!(unescape_udev(r"bad\xZZ"), r"bad\xZZ");
    }
}
