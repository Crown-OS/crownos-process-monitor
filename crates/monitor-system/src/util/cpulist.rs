use monitor_core::util::parse::parse_u32;

/// Expands a kernel CPU list such as `0-3,8,10-11`.
pub(crate) fn cpu_list(text: &[u8]) -> impl Iterator<Item = u32> + '_ {
    text.trim_ascii()
        .split(|&byte| byte == b',')
        .filter(|range| !range.is_empty())
        .flat_map(|range| {
            let (start, end) = match memchr::memchr(b'-', range) {
                Some(dash) => (parse_u32(&range[..dash]), parse_u32(&range[dash + 1..])),
                None => (parse_u32(range), parse_u32(range)),
            };
            match (start, end) {
                (Some(start), Some(end)) => start..end.saturating_add(1),
                _ => 0..0,
            }
        })
}

const ONLINE_CPUS: &str = "/sys/devices/system/cpu/online";

/// Online logical CPUs, at least one.
pub(crate) fn online_cpu_count() -> u32 {
    let mut bytes = [0_u8; 256];
    let length = rustix::fs::open(
        ONLINE_CPUS,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .and_then(|fd| rustix::io::read(&fd, &mut bytes[..]))
    .unwrap_or(0);
    u32::try_from(cpu_list(&bytes[..length]).count())
        .unwrap_or(u32::MAX)
        .max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_ranges() {
        let cpus: Vec<_> = cpu_list(b"0-3,8,10-11\n").collect();
        assert_eq!(cpus, [0, 1, 2, 3, 8, 10, 11]);
        assert_eq!(cpu_list(b"").count(), 0);
        assert_eq!(cpu_list(b"5-2").count(), 0);
        assert!(online_cpu_count() >= 1);
    }
}
