use monitor_core::Bytes;

pub fn bytes_to_integer(bytes: Bytes) -> i64 {
    i64::try_from(bytes.get()).unwrap_or(i64::MAX)
}

pub fn integer_to_bytes(value: i64) -> Bytes {
    Bytes::new(u64::try_from(value).unwrap_or(0))
}

pub fn real_to_bytes(value: f64) -> Bytes {
    Bytes::new(if value.is_finite() && value > 0.0 {
        value as u64
    } else {
        0
    })
}
