//! Allocation-free parsers for the text formats of `/proc` and `/sys`.

const KIB: u64 = 1024;

#[must_use]
pub fn parse_u64(bytes: &[u8]) -> Option<u64> {
    let digits = bytes.trim_ascii();
    if digits.is_empty() {
        return None;
    }
    digits.iter().try_fold(0_u64, |value, &byte| {
        let digit = byte.checked_sub(b'0').filter(|digit| *digit < 10)?;
        value.checked_mul(10)?.checked_add(u64::from(digit))
    })
}

#[must_use]
pub fn parse_i64(bytes: &[u8]) -> Option<i64> {
    let trimmed = bytes.trim_ascii();
    match trimmed.split_first() {
        Some((b'-', magnitude)) => parse_u64(magnitude)
            .and_then(|value| i64::try_from(value).ok())
            .map(|value| -value),
        _ => parse_u64(trimmed).and_then(|value| i64::try_from(value).ok()),
    }
}

#[must_use]
pub fn parse_u32(bytes: &[u8]) -> Option<u32> {
    parse_u64(bytes).and_then(|value| u32::try_from(value).ok())
}

#[must_use]
pub fn parse_f64(bytes: &[u8]) -> Option<f64> {
    std::str::from_utf8(bytes.trim_ascii()).ok()?.parse().ok()
}

/// Parses `0x1002` or `1002` as hexadecimal.
#[must_use]
pub fn parse_hex_u32(bytes: &[u8]) -> Option<u32> {
    let trimmed = bytes.trim_ascii();
    let digits = trimmed
        .strip_prefix(b"0x")
        .or_else(|| trimmed.strip_prefix(b"0X"))
        .unwrap_or(trimmed);
    u32::from_str_radix(std::str::from_utf8(digits).ok()?, 16).ok()
}

pub fn lines(buffer: &[u8]) -> impl Iterator<Item = &[u8]> {
    buffer
        .split(|&byte| byte == b'\n')
        .filter(|line| !line.is_empty())
}

pub fn fields(line: &[u8]) -> impl Iterator<Item = &[u8]> {
    line.split(u8::is_ascii_whitespace)
        .filter(|field| !field.is_empty())
}

/// Splits `Key: value` into a trimmed pair.
#[must_use]
pub fn key_value(line: &[u8]) -> Option<(&[u8], &[u8])> {
    let colon = memchr::memchr(b':', line)?;
    Some((line[..colon].trim_ascii(), line[colon + 1..].trim_ascii()))
}

/// Parses the value of a `/proc/meminfo`-style line (`123456 kB`) into bytes.
#[must_use]
pub fn kib_value(value: &[u8]) -> Option<u64> {
    let number = fields(value).next()?;
    parse_u64(number).map(|kib| kib.saturating_mul(KIB))
}

/// Finds the value of `key` in a `Key: value` file.
pub fn find_value<'a>(buffer: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    lines(buffer)
        .filter_map(key_value)
        .find_map(|(name, value)| (name == key).then_some(value))
}

/// Returns `text` as UTF-8, replacing nothing: invalid input yields `None`.
#[must_use]
pub fn as_str(bytes: &[u8]) -> Option<&str> {
    std::str::from_utf8(bytes.trim_ascii()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_numbers() {
        assert_eq!(parse_u64(b" 42\n"), Some(42));
        assert_eq!(parse_u64(b""), None);
        assert_eq!(parse_u64(b"4x"), None);
        assert_eq!(parse_u64(b"99999999999999999999999"), None);
        assert_eq!(parse_i64(b"-273150"), Some(-273_150));
        assert_eq!(parse_hex_u32(b"0x10de\n"), Some(0x10de));
        assert_eq!(parse_f64(b"3.86"), Some(3.86));
    }

    #[test]
    fn parses_meminfo_lines() {
        let meminfo = b"MemTotal:       15520700 kB\nMemAvailable:    3292032 kB\n";
        let available = find_value(meminfo, b"MemAvailable").and_then(kib_value);
        assert_eq!(available, Some(3_292_032 * 1024));
        assert_eq!(find_value(meminfo, b"Missing"), None);
    }

    #[test]
    fn splits_fields() {
        let collected: Vec<_> = fields(b"cpu  608313 1439\t128793").collect();
        assert_eq!(collected, [&b"cpu"[..], b"608313", b"1439", b"128793"]);
    }
}
