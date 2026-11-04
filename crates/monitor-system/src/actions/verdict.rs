use monitor_core::{Pid, ProcessKey};

use super::ProcessAction;
use crate::apps::cgroup::{leaf, unified_path};
use crate::process::{StatFields, parse_stat};

const INIT_PID: u32 = 1;
const KTHREADD_PID: u32 = 2;
/// The CrownOS compositor's project and binary name…
const COMPOSITOR: &str = "crownpositor";
/// …and the name its binary is built as.
const COMPOSITOR_BINARY: &str = "compositor";
const SERVICE_MANAGER: &str = "systemd";
/// `comm` is cut at 15 bytes, hence `dbus-broker-lau`.
const MESSAGE_BUSES: [&str; 3] = ["dbus-daemon", "dbus-broker", "dbus-broker-lau"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RefuseReason {
    Init,
    KernelThread,
    Monitor,
    Compositor,
    ServiceManager,
    SessionBus,
    SessionLeader,
}

impl RefuseReason {
    pub const fn explanation(self) -> &'static str {
        match self {
            Self::Init => "PID 1 runs the whole system; stopping it would crash the machine",
            Self::KernelThread => "kernel threads are part of the kernel and cannot be signalled",
            Self::Monitor => "this is the process monitor itself",
            Self::Compositor => {
                "the compositor draws the whole desktop; stopping it ends the session"
            }
            Self::ServiceManager => "systemd --user manages every app and service of the session",
            Self::SessionBus => {
                "the message bus connects the desktop; stopping it breaks every app"
            }
            Self::SessionLeader => "this process leads the login session; stopping it logs you out",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Safe,
    /// Show a confirmation dialog first.
    AskFirst,
    Refuse(RefuseReason),
}

pub(crate) struct ProcessProbe {
    pub stat: StatFields,
    pub comm: String,
    pub cgroup: String,
    /// `/proc/<pid>/exe`, `None` when unreadable.
    pub executable: Option<String>,
}

impl ProcessProbe {
    /// Reads what the refuse list needs, or `None` when `key` is gone.
    pub fn read(key: ProcessKey) -> Option<Self> {
        let text = std::fs::read(format!("/proc/{}/stat", key.pid)).ok()?;
        let (comm, stat) = parse_stat(&text)?;
        (stat.start_ticks == key.start_ticks).then(|| Self {
            stat,
            comm: String::from_utf8_lossy(comm).into_owned(),
            cgroup: std::fs::read_to_string(format!("/proc/{}/cgroup", key.pid))
                .map(|text| unified_path(&text).to_owned())
                .unwrap_or_default(),
            executable: std::fs::read_link(format!("/proc/{}/exe", key.pid))
                .ok()
                .map(|path| path.to_string_lossy().into_owned()),
        })
    }

    pub fn refuse_reason(&self, pid: Pid) -> Option<RefuseReason> {
        let raw = pid.get();
        if raw == INIT_PID {
            Some(RefuseReason::Init)
        } else if raw == KTHREADD_PID
            || self.stat.is_kernel_thread()
            || self.stat.parent == KTHREADD_PID
        {
            Some(RefuseReason::KernelThread)
        } else if raw == std::process::id() {
            Some(RefuseReason::Monitor)
        } else if self.is_compositor() {
            Some(RefuseReason::Compositor)
        } else if self.comm == SERVICE_MANAGER {
            Some(RefuseReason::ServiceManager)
        } else if MESSAGE_BUSES.contains(&self.comm.as_str()) {
            Some(RefuseReason::SessionBus)
        } else if self.stat.session == raw && leaf(&self.cgroup).starts_with("session-") {
            Some(RefuseReason::SessionLeader)
        } else {
            None
        }
    }
}

impl ProcessProbe {
    /// `crownpositor` or its `compositor` binary, judged by the executable
    /// path so an unrelated program named `compositor` is not protected.
    fn is_compositor(&self) -> bool {
        if self.comm != COMPOSITOR && self.comm != COMPOSITOR_BINARY {
            return false;
        }
        let Some(executable) = self.executable.as_deref() else {
            return self.comm == COMPOSITOR;
        };
        let path = std::path::Path::new(executable);
        path.file_name().is_some_and(|name| name == COMPOSITOR)
            || path
                .components()
                .any(|component| component.as_os_str() == COMPOSITOR)
    }
}

pub(crate) fn verdict_for(probe: &ProcessProbe, pid: Pid, action: ProcessAction) -> Verdict {
    match probe.refuse_reason(pid) {
        Some(reason) => Verdict::Refuse(reason),
        None if action == ProcessAction::Resume => Verdict::Safe,
        None => Verdict::AskFirst,
    }
}

/// Whether `action` on `key` may go ahead, and with a confirmation or not.
/// A process that already exited gets `AskFirst`; acting on it reports
/// [`super::ActionError::Gone`].
pub fn verdict(key: ProcessKey, action: ProcessAction) -> Verdict {
    ProcessProbe::read(key).map_or(Verdict::AskFirst, |probe| {
        verdict_for(&probe, key.pid, action)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe_with_exe(comm: &str, executable: &str) -> ProcessProbe {
        ProcessProbe {
            executable: Some(executable.to_owned()),
            ..probe(comm, 1, "/", 0)
        }
    }

    fn probe(comm: &str, session: u32, cgroup: &str, flags: u32) -> ProcessProbe {
        ProcessProbe {
            stat: StatFields {
                session,
                flags,
                parent: 1000,
                ..StatFields::default()
            },
            comm: comm.to_owned(),
            cgroup: cgroup.to_owned(),
            executable: None,
        }
    }

    #[test]
    fn recognises_the_compositor_by_its_executable() {
        let pid = Pid::new(4242);
        let compositor = Some(RefuseReason::Compositor);
        assert_eq!(
            probe_with_exe(
                "compositor",
                "/e/CrownOS/crownpositor/target/release/compositor"
            )
            .refuse_reason(pid),
            compositor
        );
        assert_eq!(
            probe_with_exe("crownpositor", "/usr/bin/crownpositor").refuse_reason(pid),
            compositor
        );
        assert_eq!(
            probe_with_exe("compositor", "/opt/other/bin/compositor").refuse_reason(pid),
            None
        );
        assert_eq!(probe("compositor", 1, "/", 0).refuse_reason(pid), None);
    }

    #[test]
    fn applies_refuse_list() {
        let app_scope =
            "/user.slice/user-1000.slice/user@1000.service/app.slice/app-orca-2258.scope";
        let session = "/user.slice/user-1000.slice/session-2.scope";
        let pid = Pid::new(4242);
        let reason = |probe: ProcessProbe, pid: Pid| probe.refuse_reason(pid);
        assert_eq!(
            reason(probe("systemd", 1, "/init.scope", 0), Pid::new(1)),
            Some(RefuseReason::Init)
        );
        assert_eq!(
            reason(probe("kworker/0:1", 0, "/", 0x0020_0000), pid),
            Some(RefuseReason::KernelThread)
        );
        assert_eq!(
            reason(probe("x", 0, "/", 0), Pid::new(std::process::id())),
            Some(RefuseReason::Monitor)
        );
        assert_eq!(
            reason(probe("crownpositor", 1, session, 0), pid),
            Some(RefuseReason::Compositor)
        );
        assert_eq!(
            reason(probe("systemd", 1258, app_scope, 0), pid),
            Some(RefuseReason::ServiceManager)
        );
        assert_eq!(
            reason(probe("dbus-broker", 1, app_scope, 0), pid),
            Some(RefuseReason::SessionBus)
        );
        assert_eq!(
            reason(probe("login", 4242, session, 0), pid),
            Some(RefuseReason::SessionLeader)
        );
        assert_eq!(
            reason(probe("fish", 4242, app_scope, 0), pid),
            None,
            "a terminal shell may be closed"
        );
        assert_eq!(
            verdict_for(
                &probe("orca-ide", 1, app_scope, 0),
                pid,
                ProcessAction::Quit
            ),
            Verdict::AskFirst
        );
        assert_eq!(
            verdict_for(
                &probe("orca-ide", 1, app_scope, 0),
                pid,
                ProcessAction::Resume
            ),
            Verdict::Safe
        );
    }

    #[test]
    fn refuses_live_init_and_self() {
        let Some(own) = std::fs::read("/proc/self/stat")
            .ok()
            .and_then(|text| parse_stat(&text).map(|(_, stat)| stat.start_ticks))
        else {
            return;
        };
        let key = ProcessKey {
            pid: Pid::new(std::process::id()),
            start_ticks: own,
        };
        assert_eq!(
            verdict(key, ProcessAction::Quit),
            Verdict::Refuse(RefuseReason::Monitor)
        );
    }
}
