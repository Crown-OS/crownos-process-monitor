use std::sync::Arc;

use monitor_core::util::parse::parse_u64;
use monitor_core::util::sysfs::PersistentFile;
use monitor_core::{Bytes, Percent};

pub(crate) const CGROUP_ROOT: &str = "/sys/fs/cgroup";
const MICROSECONDS_PER_SECOND: f64 = 1_000_000.0;

/// What one read of an app's own scope found.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ScopeReading {
    pub memory: Bytes,
    /// Share since the previous read; `None` on the first.
    pub cpu: Option<Percent>,
    /// `cgroup.events` says `frozen 1`.
    pub frozen: bool,
}

/// `memory.current`, `cpu.stat` and `cgroup.events` of an app's own scope, kept open.
pub(crate) struct ScopeUsage {
    pub path: Arc<str>,
    memory: PersistentFile,
    cpu: PersistentFile,
    events: Option<PersistentFile>,
    previous_usage: Option<u64>,
}

impl ScopeUsage {
    pub fn open(path: &Arc<str>) -> Option<Self> {
        let open = |file: &str| PersistentFile::open(format!("{CGROUP_ROOT}{path}/{file}")).ok();
        Some(Self {
            path: path.clone(),
            memory: open("memory.current")?,
            cpu: open("cpu.stat")?,
            events: open("cgroup.events"),
            previous_usage: None,
        })
    }

    pub fn read(&mut self, buffer: &mut Vec<u8>, seconds: Option<f64>) -> Option<ScopeReading> {
        let memory = Bytes::new(self.memory.read_u64().ok()?);
        let frozen = self
            .events
            .as_ref()
            .and_then(|events| events.read(buffer).ok())
            .and_then(|events| find_value_spaced(events, b"frozen"))
            == Some(1);
        let usage = self
            .cpu
            .read(buffer)
            .ok()
            .and_then(|stat| find_value_spaced(stat, b"usage_usec"));
        let previous = std::mem::replace(&mut self.previous_usage, usage);
        let cpu = match (previous, usage, seconds) {
            (Some(previous), Some(usage), Some(seconds)) if seconds > 0.0 => {
                Some(Percent::from_ratio(
                    usage.saturating_sub(previous) as f64 / MICROSECONDS_PER_SECOND / seconds,
                ))
            }
            _ => None,
        };
        Some(ScopeReading {
            memory,
            cpu,
            frozen,
        })
    }
}

/// `cpu.stat` uses `key value` lines rather than `key: value`.
fn find_value_spaced(buffer: &[u8], key: &[u8]) -> Option<u64> {
    buffer
        .split(|&byte| byte == b'\n')
        .find_map(|line| line.strip_prefix(key)?.strip_prefix(b" "))
        .and_then(parse_u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_frozen_events() {
        assert_eq!(
            find_value_spaced(b"populated 1\nfrozen 1\n", b"frozen"),
            Some(1)
        );
        assert_eq!(
            find_value_spaced(b"populated 1\nfrozen 0\n", b"frozen"),
            Some(0)
        );
    }

    #[test]
    fn parses_cpu_stat() {
        let stat =
            b"usage_usec 70431491\nuser_usec 45188520\nsystem_usec 25242970\nnice_usec 1534362\n";
        assert_eq!(find_value_spaced(stat, b"usage_usec"), Some(70_431_491));
        assert_eq!(find_value_spaced(stat, b"system_usec"), Some(25_242_970));
        assert_eq!(find_value_spaced(stat, b"missing"), None);
    }

    #[test]
    fn reads_own_scope() {
        let mut buffer = Vec::new();
        let Ok(text) = std::fs::read_to_string("/proc/self/cgroup") else {
            return;
        };
        let path: Arc<str> = crate::apps::cgroup::unified_path(&text).into();
        if let Some(mut usage) = ScopeUsage::open(&path) {
            let first = usage.read(&mut buffer, Some(1.0));
            assert!(first.is_some_and(|reading| {
                reading.memory.get() > 0 && reading.cpu.is_none() && !reading.frozen
            }));
            let second = usage.read(&mut buffer, Some(1.0));
            assert!(second.is_some_and(|reading| reading.cpu.is_some()));
        }
    }
}
