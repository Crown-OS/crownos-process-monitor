//! What the app table knows about each row beyond its shared columns: identity for actions and
//! the inspector, the Columns-menu details, browser roles and the Ended recently group.

use std::fmt::Write;
use std::sync::Arc;

use monitor_core::{AppKey, AppKind, Pid, ProcessKey};
use monitor_engine::system::{ProcessRole, ProcessState, RoleBreakdown};

use crate::components::app_table::AppCategory;
use crate::util::format::write_bytes;
use crate::util::time::write_clock;

/// The key of the Ended recently group; real apps hash to other values.
pub const ENDED_GROUP_KEY: u64 = u64::MAX;
pub const ENDED_GROUP_NAME: &str = "Ended recently";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProcessFacts {
    /// With the pid, the key that survives pid reuse.
    pub start_ticks: u64,
    pub parent: u32,
    pub uid: u32,
    pub command_line: Arc<str>,
    pub path: Option<Arc<str>>,
    pub role: ProcessRole,
    pub state: ProcessState,
    pub nice: i32,
    pub started_unix: u64,
    /// Bytes per second.
    pub disk_read: f32,
    pub disk_write: f32,
    pub download: f32,
    pub upload: f32,
    /// Bytes since the process started: storage while I/O is sampled, TCP since first seen.
    pub read_total: u64,
    pub written_total: u64,
    pub received_total: u64,
    pub sent_total: u64,
    pub gpu_percent: f32,
    pub kernel_thread: bool,
    /// Set for rows of the Ended recently group.
    pub ended_unix: u64,
    pub peak_memory: u64,
}

impl ProcessFacts {
    pub fn key(&self, pid: u32) -> ProcessKey {
        ProcessKey {
            pid: Pid::new(pid),
            start_ticks: self.start_ticks,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AppFacts {
    /// `None` for the Ended recently group.
    pub app_key: Option<AppKey>,
    pub category: AppCategory,
    /// The app owns a cgroup scope, so it can be frozen as a whole.
    pub pausable: bool,
    /// That scope is frozen right now.
    pub frozen: bool,
    pub roles: RoleBreakdown,
    pub started_unix: u64,
}

pub const fn category_of(kind: AppKind) -> AppCategory {
    match kind {
        AppKind::Desktop
        | AppKind::Flatpak
        | AppKind::Snap
        | AppKind::Terminal
        | AppKind::Process => AppCategory::Mine,
        AppKind::Container => AppCategory::Container,
        AppKind::Service | AppKind::Kernel | AppKind::OtherUser => AppCategory::System,
    }
}

/// The browser roles worth naming; Main and Other stay unlabelled.
const NAMED_ROLES: [(ProcessRole, &str, &str); 4] = [
    (ProcessRole::Tab, "tab", "tabs"),
    (ProcessRole::Extension, "extension", "extensions"),
    (ProcessRole::Graphics, "graphics", "graphics"),
    (ProcessRole::Utility, "utility", "utility"),
];

pub const fn role_label(role: ProcessRole) -> &'static str {
    match role {
        ProcessRole::Tab => "Tab",
        ProcessRole::Extension => "Extension",
        ProcessRole::Graphics => "Graphics",
        ProcessRole::Utility => "Utility",
        ProcessRole::Main | ProcessRole::Other => "",
    }
}

/// "12 tabs · 3 extensions · 1 graphics" for apps that split into browser roles.
pub fn write_roles(out: &mut String, roles: &RoleBreakdown) {
    for (role, one, many) in NAMED_ROLES {
        let count = roles.count(role);
        if count == 0 {
            continue;
        }
        if !out.is_empty() {
            out.push_str(" · ");
        }
        let noun = if count == 1 { one } else { many };
        let _ = write!(out, "{count} {noun}");
    }
}

/// "14:02–14:09 · 84 MB peak" for a process of the Ended recently group.
pub fn write_ended(out: &mut String, facts: &ProcessFacts, utc_offset_seconds: i32) {
    write_clock(out, facts.started_unix as i64, utc_offset_seconds);
    out.push('–');
    write_clock(out, facts.ended_unix as i64, utc_offset_seconds);
    if facts.peak_memory > 0 {
        out.push_str(" · ");
        write_bytes(out, facts.peak_memory);
        out.push_str(" peak");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_read_as_counts() {
        let mut roles = RoleBreakdown::default();
        for role in [
            ProcessRole::Tab,
            ProcessRole::Tab,
            ProcessRole::Extension,
            ProcessRole::Main,
        ] {
            roles.add(role);
        }
        let mut out = String::new();
        write_roles(&mut out, &roles);
        assert_eq!(out, "2 tabs · 1 extension");
    }

    #[test]
    fn ended_rows_show_their_lifetime_and_peak() {
        let facts = ProcessFacts {
            started_unix: 3_600,
            ended_unix: 3_600 + 9 * 60,
            peak_memory: 84 * 1024 * 1024,
            ..ProcessFacts::default()
        };
        let mut out = String::new();
        write_ended(&mut out, &facts, 0);
        assert_eq!(out, "01:00–01:09 · 84.00 MB peak");
    }

    #[test]
    fn kinds_fall_into_chip_categories() {
        assert_eq!(category_of(AppKind::Desktop), AppCategory::Mine);
        assert_eq!(category_of(AppKind::Kernel), AppCategory::System);
        assert_eq!(category_of(AppKind::Container), AppCategory::Container);
    }
}
