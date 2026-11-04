use std::os::fd::{AsFd, OwnedFd};

use monitor_core::util::sysfs;
use monitor_system::parse_stat;
use zbus::fdo::DBusProxy;
use zbus::names::{BusName, OwnedUniqueName};

use crate::auth::AuthError;

/// Who is calling, as established by the bus daemon rather than claimed by
/// the client. Backends decide on this alone.
#[derive(Debug)]
pub struct Caller {
    pub bus_name: OwnedUniqueName,
    pub uid: u32,
    pub pid: u32,
    pub start_ticks: u64,
    pub supplementary_groups: Vec<u32>,
    /// Pins the process so a recycled PID cannot impersonate the caller.
    pub pidfd: Option<OwnedFd>,
}

impl Caller {
    pub async fn identify(bus: &DBusProxy<'_>, sender: OwnedUniqueName) -> Result<Self, AuthError> {
        let credentials = bus
            .get_connection_credentials(BusName::Unique(sender.as_ref()))
            .await?;
        let uid = credentials
            .unix_user_id()
            .ok_or(AuthError::Credentials("the bus reported no user id"))?;
        let pid = credentials
            .process_id()
            .ok_or(AuthError::Credentials("the bus reported no process id"))?;
        let supplementary_groups = credentials.unix_group_ids().cloned().unwrap_or_default();
        Ok(Self {
            bus_name: sender,
            uid,
            pid,
            start_ticks: process_start_ticks(pid)?,
            supplementary_groups,
            pidfd: credentials
                .process_fd()
                .and_then(|fd| fd.as_fd().try_clone_to_owned().ok()),
        })
    }
}

fn process_start_ticks(pid: u32) -> Result<u64, AuthError> {
    let mut buffer = Vec::new();
    let stat = sysfs::read_into(format!("/proc/{pid}/stat"), &mut buffer)
        .map_err(|_| AuthError::Credentials("the calling process has exited"))?;
    parse_stat(stat)
        .map(|(_, stat)| stat.start_ticks)
        .ok_or(AuthError::Credentials("unreadable process start time"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_own_start_time() {
        assert!(process_start_ticks(std::process::id()).is_ok_and(|ticks| ticks > 0));
    }
}
