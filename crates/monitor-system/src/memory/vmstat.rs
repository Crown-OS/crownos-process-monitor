use monitor_core::util::parse::{fields, lines, parse_u64};

/// The cumulative `/proc/vmstat` counters the memory page turns into rates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct VmCounters {
    pub swap_in_pages: u64,
    pub swap_out_pages: u64,
    pub page_faults: u64,
    pub major_faults: u64,
    pub oom_kills: u64,
}

impl VmCounters {
    pub fn parse(buffer: &[u8]) -> Self {
        let mut counters = Self::default();
        for line in lines(buffer) {
            let mut columns = fields(line);
            let (Some(key), Some(value)) = (columns.next(), columns.next().and_then(parse_u64))
            else {
                continue;
            };
            match key {
                b"pswpin" => counters.swap_in_pages = value,
                b"pswpout" => counters.swap_out_pages = value,
                b"pgfault" => counters.page_faults = value,
                b"pgmajfault" => counters.major_faults = value,
                b"oom_kill" => counters.oom_kills = value,
                _ => {}
            }
        }
        counters
    }

    pub fn minor_faults(&self) -> u64 {
        self.page_faults.saturating_sub(self.major_faults)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_vmstat() {
        let vmstat = b"nr_free_pages 1465250\npgpgin 9462124\npswpin 40666\npswpout 761451\n\
pgfault 28900548\npgmajfault 99200\noom_kill 0\n";
        let counters = VmCounters::parse(vmstat);
        assert_eq!(counters.swap_in_pages, 40_666);
        assert_eq!(counters.swap_out_pages, 761_451);
        assert_eq!(counters.minor_faults(), 28_900_548 - 99_200);
        assert_eq!(counters.oom_kills, 0);
    }
}
