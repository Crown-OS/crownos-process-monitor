use std::collections::VecDeque;

use super::row::EndedProcess;

/// How long an exited process stays listed.
const RETENTION_SECONDS: u64 = 10 * 60;
const CAPACITY: usize = 256;

/// Processes that exited recently, newest first, capped in count and age.
#[derive(Debug)]
pub(crate) struct EndedRing {
    entries: VecDeque<EndedProcess>,
}

impl Default for EndedRing {
    fn default() -> Self {
        Self {
            entries: VecDeque::with_capacity(CAPACITY),
        }
    }
}

impl EndedRing {
    pub fn push(&mut self, ended: EndedProcess) {
        if self.entries.len() == CAPACITY {
            self.entries.pop_back();
        }
        self.entries.push_front(ended);
    }

    pub fn prune(&mut self, now_unix: u64) {
        let cutoff = now_unix.saturating_sub(RETENTION_SECONDS);
        while self
            .entries
            .back()
            .is_some_and(|ended| ended.ended_unix < cutoff)
        {
            self.entries.pop_back();
        }
    }

    pub fn copy_into(&self, out: &mut Vec<EndedProcess>) {
        out.clear();
        out.extend(self.entries.iter().cloned());
    }
}

#[cfg(test)]
mod tests {
    use monitor_core::{AppKey, AppKind, Bytes, Pid, ProcessKey};

    use super::*;

    fn ended(pid: u32, ended_unix: u64) -> EndedProcess {
        EndedProcess {
            key: ProcessKey {
                pid: Pid::new(pid),
                start_ticks: 1,
            },
            name: "sleep".into(),
            app: AppKey::new(AppKind::Process, "sleep"),
            app_name: "sleep".into(),
            started_unix: 0,
            ended_unix,
            peak_memory: Bytes::ZERO,
        }
    }

    #[test]
    fn caps_and_expires() {
        let mut ring = EndedRing::default();
        for pid in 0..300 {
            ring.push(ended(pid, 1000 + u64::from(pid)));
        }
        let mut listed = Vec::new();
        ring.copy_into(&mut listed);
        assert_eq!(listed.len(), CAPACITY);
        assert_eq!(listed[0].key.pid, Pid::new(299), "newest first");
        ring.prune(1000 + 299 + RETENTION_SECONDS - 10);
        ring.copy_into(&mut listed);
        assert_eq!(listed.len(), 11);
    }
}
