use monitor_core::Pid;
use monitor_core::util::procfs::PidName;

const CAPACITY: usize = 64;

/// `<pid>/<file>` built on the stack, for `openat` relative to `/proc`.
pub(crate) struct PidPath {
    bytes: [u8; CAPACITY],
    length: usize,
}

impl PidPath {
    pub(crate) fn new(pid: Pid, file: &str) -> Self {
        let mut bytes = [0_u8; CAPACITY];
        let mut length = 0;
        let mut name = PidName::new(pid);
        for part in [name.as_str().as_bytes(), b"/", file.as_bytes()] {
            let end = (length + part.len()).min(CAPACITY);
            bytes[length..end].copy_from_slice(&part[..end - length]);
            length = end;
        }
        if file.is_empty() {
            length -= 1;
        }
        Self { bytes, length }
    }

    pub(crate) fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..self.length]).unwrap_or(".")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_pid_and_file() {
        assert_eq!(PidPath::new(Pid::new(42), "stat").as_str(), "42/stat");
        assert_eq!(PidPath::new(Pid::new(7), "").as_str(), "7");
    }
}
