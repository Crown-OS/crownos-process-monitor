use monitor_core::util::parse::{fields, parse_i64, parse_u32, parse_u64};

/// `PF_KTHREAD` in the stat `flags` field.
const KERNEL_THREAD_FLAG: u32 = 0x0020_0000;

/// The `/proc/<pid>/stat` fields the monitor uses (numbering as in proc(5)).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StatFields {
    pub state: u8,
    pub parent: u32,
    pub process_group: u32,
    pub session: u32,
    pub tty: u32,
    pub flags: u32,
    pub minor_faults: u64,
    pub major_faults: u64,
    pub user_ticks: u64,
    pub system_ticks: u64,
    pub children_user_ticks: u64,
    pub children_system_ticks: u64,
    pub priority: i64,
    pub nice: i64,
    pub threads: u32,
    pub start_ticks: u64,
    pub resident_pages: u64,
    pub policy: u32,
}

impl StatFields {
    pub fn is_kernel_thread(&self) -> bool {
        self.flags & KERNEL_THREAD_FLAG != 0
    }

    pub fn cpu_ticks(&self) -> u64 {
        self.user_ticks + self.system_ticks
    }
}

/// Splits a stat line into `comm` and the numeric fields. `comm` is taken
/// between the first `(` and the last `)`, since it may contain both.
pub fn parse_stat(buffer: &[u8]) -> Option<(&[u8], StatFields)> {
    let open = memchr::memchr(b'(', buffer)?;
    let close = memchr::memrchr(b')', buffer)?;
    let comm = buffer.get(open + 1..close)?;
    let mut stat = StatFields::default();
    for (index, column) in fields(buffer.get(close + 1..)?).enumerate() {
        let unsigned = || parse_u64(column).unwrap_or(0);
        let small = || parse_u32(column).unwrap_or(0);
        match index {
            0 => stat.state = column.first().copied().unwrap_or(b'?'),
            1 => stat.parent = small(),
            2 => stat.process_group = small(),
            3 => stat.session = small(),
            4 => stat.tty = small(),
            6 => stat.flags = small(),
            7 => stat.minor_faults = unsigned(),
            9 => stat.major_faults = unsigned(),
            11 => stat.user_ticks = unsigned(),
            12 => stat.system_ticks = unsigned(),
            13 => stat.children_user_ticks = parse_i64(column).unwrap_or(0).max(0).unsigned_abs(),
            14 => stat.children_system_ticks = parse_i64(column).unwrap_or(0).max(0).unsigned_abs(),
            15 => stat.priority = parse_i64(column).unwrap_or(0),
            16 => stat.nice = parse_i64(column).unwrap_or(0),
            17 => stat.threads = small(),
            19 => stat.start_ticks = unsigned(),
            21 => stat.resident_pages = parse_i64(column).unwrap_or(0).max(0).unsigned_abs(),
            38 => {
                stat.policy = small();
                break;
            }
            _ => {}
        }
    }
    Some((comm, stat))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAT_STAT: &[u8] = b"46474 (cat) R 46473 46474 46473 0 -1 4194304 95 0 0 0 0 0 0 0 20 0 1 0 122934 6635520 1006 18446744073709551615 94618629423104 94618629463473 140725544344224 0 0 0 0 0 0 0 0 0 17 9 0 0 0 0 0 94618629478896 94618629480740 94619312640000 140725544352132 140725544352152 140725544352152 140725544357867 0\n";
    const TRICKY_STAT: &[u8] = b"4242 (Web Content (x) )) S 2258 2258 1693 34816 -1 4194560 2512 0 12 0 345 67 0 0 20 -5 31 0 98765 6635520 20480 18446744073709551615 1 1 0 0 0 0 0 0 0 0 0 0 17 3 0 0 0 0 0\n";
    const KTHREADD: &[u8] = b"2 (kthreadd) S 0 0 0 0 -1 2129984 0 0 0 0 0 0 0 0 20 0 1 0 8 0 0 18446744073709551615 0 0 0 0 0 0 0 2147483647 0 0 0 0 0 12 0 0 0 0 0 0 0 0 0 0 0 0 0\n";

    #[test]
    fn parses_real_stat() {
        let Some((comm, stat)) = parse_stat(CAT_STAT) else {
            panic!("stat line rejected");
        };
        assert_eq!(comm, b"cat");
        assert_eq!(stat.state, b'R');
        assert_eq!(stat.parent, 46_473);
        assert_eq!(stat.session, 46_473);
        assert_eq!(stat.minor_faults, 95);
        assert_eq!(stat.priority, 20);
        assert_eq!(stat.threads, 1);
        assert_eq!(stat.start_ticks, 122_934);
        assert_eq!(stat.resident_pages, 1006);
        assert_eq!(stat.policy, 0);
        assert!(!stat.is_kernel_thread());
    }

    #[test]
    fn keeps_spaces_and_parentheses_in_comm() {
        let Some((comm, stat)) = parse_stat(TRICKY_STAT) else {
            panic!("stat line rejected");
        };
        assert_eq!(comm, b"Web Content (x) )");
        assert_eq!(stat.state, b'S');
        assert_eq!(stat.parent, 2258);
        assert_eq!(stat.tty, 34_816);
        assert_eq!(stat.major_faults, 12);
        assert_eq!(stat.cpu_ticks(), 345 + 67);
        assert_eq!(stat.nice, -5);
        assert_eq!(stat.threads, 31);
        assert_eq!(stat.start_ticks, 98_765);
    }

    #[test]
    fn flags_kernel_threads() {
        let Some((comm, stat)) = parse_stat(KTHREADD) else {
            panic!("stat line rejected");
        };
        assert_eq!(comm, b"kthreadd");
        assert!(stat.is_kernel_thread());
        assert!(parse_stat(b"garbage").is_none());
    }
}
