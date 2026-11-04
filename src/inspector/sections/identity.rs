use crownui::prelude::AnyView;
use monitor_core::util::time::unix_now;
use monitor_engine::system::inspect::{ProcessDetails, Sandbox};
use monitor_engine::system::{ProcessRole, ProcessState};

use crate::inspector::fields::{Fields, field_section};
use crate::util::format::duration;
use crate::util::time::moment;

pub const fn state_name(state: ProcessState) -> &'static str {
    match state {
        ProcessState::Running => "R · running",
        ProcessState::Sleeping => "S · sleeping",
        ProcessState::DiskSleep => "D · waiting on I/O",
        ProcessState::Stopped => "T · stopped",
        ProcessState::TracingStop => "t · stopped by a debugger",
        ProcessState::Zombie => "Z · zombie",
        ProcessState::Dead => "X · dead",
        ProcessState::Idle => "I · idle kernel thread",
        ProcessState::Unknown => "?",
    }
}

fn kind(details: &ProcessDetails) -> &'static str {
    if details.identity.kernel_thread {
        return "Kernel thread";
    }
    match (&details.security.sandbox, details.identity.role) {
        (Sandbox::Container { .. }, _) => "Container",
        (_, ProcessRole::Tab) => "Browser tab",
        (_, ProcessRole::Extension) => "Browser extension",
        (_, ProcessRole::Graphics) => "GPU process",
        (_, ProcessRole::Utility) => "Utility process",
        (_, ProcessRole::Other) => "Helper process",
        (_, ProcessRole::Main) => "Main process",
    }
}

pub fn view(details: &ProcessDetails, utc_offset_seconds: i32) -> Option<AnyView> {
    let identity = &details.identity;
    let mut fields = Fields::default();
    let user = identity.user.as_deref().unwrap_or("?");
    fields
        .add("PID", identity.key.pid.get().to_string())
        .add(
            "App",
            format!("{} ({})", identity.app_name, identity.app.as_str()),
        )
        .add("Kind", kind(details))
        .add(
            "User",
            format!("{user} · UID {} · GID {}", identity.uid.get(), identity.gid),
        )
        .add("State", state_name(identity.state))
        .add(
            "Started",
            moment(identity.started_unix as i64, unix_now(), utc_offset_seconds),
        )
        .add("Running for", duration(identity.running_for.as_secs()))
        .add_some("Terminal", identity.tty.clone())
        .add(
            "Session",
            format!(
                "session {} · group {}",
                identity.session, identity.process_group
            ),
        );
    field_section("Identity", fields)
}
