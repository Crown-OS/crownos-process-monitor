use std::borrow::Cow;
use std::sync::Arc;

/// Command lines longer than this are cut for display and search.
pub(crate) const COMMAND_LINE_LIMIT: usize = 4096;

pub(crate) fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Decodes systemd's `\xNN` escapes in unit names, borrowing when none occur.
pub(crate) fn unescape_unit(name: &str) -> Cow<'_, str> {
    if !name.contains("\\x") {
        return Cow::Borrowed(name);
    }
    let bytes = name.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let escaped = bytes
            .get(index..index + 4)
            .filter(|chunk| chunk.starts_with(b"\\x"))
            .and_then(|chunk| std::str::from_utf8(&chunk[2..]).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match escaped {
            Some(byte) => {
                decoded.push(byte);
                index += 4;
            }
            None => {
                decoded.push(bytes[index]);
                index += 1;
            }
        }
    }
    Cow::Owned(String::from_utf8_lossy(&decoded).into_owned())
}

/// Arguments of a NUL-separated `/proc/<pid>/cmdline`.
pub(crate) fn arguments(raw: &[u8]) -> impl Iterator<Item = &[u8]> {
    raw.split(|&byte| byte == 0)
        .filter(|argument| !argument.is_empty())
}

/// Like [`arguments`], but a command line rewritten into one NUL-free string
/// (Chromium and Electron helpers do this) is split on spaces.
pub(crate) fn switches(raw: &[u8]) -> impl Iterator<Item = &[u8]> {
    let separator = if arguments(raw).nth(1).is_none() {
        b' '
    } else {
        0
    };
    raw.split(move |&byte| byte == separator || byte == 0)
        .filter(|argument| !argument.is_empty())
}

/// The command line joined by spaces, cut at [`COMMAND_LINE_LIMIT`].
pub(crate) fn display_command_line(raw: &[u8]) -> Arc<str> {
    let limited = &raw[..raw.len().min(COMMAND_LINE_LIMIT)];
    let mut joined = String::from_utf8_lossy(limited).into_owned();
    let trimmed = joined.trim_end_matches('\0').len();
    joined.truncate(trimmed);
    joined.replace('\0', " ").into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unescapes_units() {
        assert_eq!(
            unescape_unit("app-dbus\\x2d:1.27\\x2dorg.a11y.atspi.Registry.slice"),
            "app-dbus-:1.27-org.a11y.atspi.Registry.slice"
        );
        assert!(matches!(unescape_unit("plain.service"), Cow::Borrowed(_)));
        assert_eq!(unescape_unit("bad\\x2"), "bad\\x2");
    }

    #[test]
    fn joins_command_lines() {
        let raw = b"/opt/stably-orca/orca-ide\0--type=zygote\0";
        assert_eq!(
            &*display_command_line(raw),
            "/opt/stably-orca/orca-ide --type=zygote"
        );
        assert_eq!(arguments(raw).count(), 2);
        assert_eq!(switches(raw).count(), 2);
        assert_eq!(
            switches(b"/opt/stably-orca/orca-ide --type=gpu-process --ozone-platform=x11").count(),
            3
        );
        assert_eq!(basename("/usr/bin/kitty"), "kitty");
        assert_eq!(basename("kitty"), "kitty");
    }
}
