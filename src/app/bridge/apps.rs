use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use crownui::kit::icons;
use monitor_core::{AppKey, AppKind};
use monitor_engine::system::{AppRow, EndedProcess, ProcessRow, ProcessTable, user_names};

use super::row_facts::{AppFacts, ENDED_GROUP_KEY, ENDED_GROUP_NAME, ProcessFacts, category_of};
use crate::app::live::{AppEntry, AppList, ProcessEntry};
use crate::components::app_table::{AppCategory, RowMark};
use crate::settings::CpuScale;

/// Refills the table's app list in place: entries and their process
/// vectors are reused, so a steady tick allocates nothing.
pub struct AppListFiller {
    users: HashMap<u32, Arc<str>>,
    ended_group_name: Arc<str>,
    no_user: Arc<str>,
}

impl AppListFiller {
    pub fn new() -> Self {
        Self {
            users: user_names(),
            ended_group_name: ENDED_GROUP_NAME.into(),
            no_user: Arc::default(),
        }
    }

    /// Apps in table order, then the Ended recently group while it holds anything.
    pub fn refill(&mut self, list: &mut AppList, table: &ProcessTable, cpu_scale: CpuScale) {
        let app_count = table.apps.len();
        list.apps
            .truncate(app_count + usize::from(!table.ended.is_empty()));
        for (index, app) in table.apps.iter().enumerate() {
            match list.apps.get_mut(index) {
                Some(entry) => describe_app(entry, app),
                None => list.apps.push(new_entry(app)),
            }
        }
        for process in &table.processes {
            let user = self.user(process.uid.get());
            if let Some(app) = list.apps.get_mut(process.app) {
                app.processes.push(process_entry(process, user, cpu_scale));
            }
        }
        for app in list.apps.iter_mut().take(app_count) {
            if !app.processes.is_empty()
                && app
                    .processes
                    .iter()
                    .all(|process| process.mark == RowMark::New)
            {
                app.mark = RowMark::New;
            }
        }
        self.refill_ended(list, app_count, &table.ended);
    }

    /// The group entry is reused like any other, so a steady list allocates nothing.
    fn refill_ended(&self, list: &mut AppList, app_count: usize, ended: &[EndedProcess]) {
        if ended.is_empty() {
            return;
        }
        if list.apps.len() == app_count {
            list.apps.push(AppEntry::default());
        }
        let Some(group) = list.apps.get_mut(app_count) else {
            return;
        };
        group.key = ENDED_GROUP_KEY;
        if !Arc::ptr_eq(&group.name, &self.ended_group_name) {
            group.name = Arc::clone(&self.ended_group_name);
        }
        group.icon = icons::HISTORY;
        group.mark = RowMark::Ended;
        group.facts = AppFacts {
            category: AppCategory::Ended,
            ..AppFacts::default()
        };
        group.processes.clear();
        group
            .processes
            .extend(ended.iter().map(|process| self.ended_entry(process)));
    }

    fn ended_entry(&self, process: &EndedProcess) -> ProcessEntry {
        ProcessEntry {
            pid: process.key.pid.get(),
            name: Arc::clone(&process.name),
            user: Arc::clone(&self.no_user),
            cpu_percent: 0.0,
            memory_bytes: 0,
            threads: 0,
            mark: RowMark::Ended,
            facts: ProcessFacts {
                start_ticks: process.key.start_ticks,
                command_line: Arc::clone(&self.no_user),
                started_unix: process.started_unix,
                ended_unix: process.ended_unix,
                peak_memory: process.peak_memory.get(),
                ..ProcessFacts::default()
            },
        }
    }

    fn user(&mut self, uid: u32) -> Arc<str> {
        Arc::clone(
            self.users
                .entry(uid)
                .or_insert_with(|| uid.to_string().into()),
        )
    }
}

fn new_entry(app: &AppRow) -> AppEntry {
    let mut entry = AppEntry {
        name: Arc::clone(&app.name),
        ..AppEntry::default()
    };
    describe_app(&mut entry, app);
    entry
}

fn describe_app(entry: &mut AppEntry, app: &AppRow) {
    entry.key = stable_key(&app.key);
    if !Arc::ptr_eq(&entry.name, &app.name) {
        entry.name = Arc::clone(&app.name);
    }
    entry.icon = kind_icon(app.kind);
    entry.mark = RowMark::None;
    entry.processes.clear();
    let facts = &mut entry.facts;
    if facts.app_key.as_ref() != Some(&app.key) {
        facts.app_key = Some(app.key.clone());
    }
    facts.category = category_of(app.kind);
    facts.pausable = app.scope.is_some();
    facts.frozen = app.frozen;
    facts.roles = app.roles;
    facts.started_unix = app.started_unix;
}

fn process_entry(process: &ProcessRow, user: Arc<str>, cpu_scale: CpuScale) -> ProcessEntry {
    let cpu = match cpu_scale {
        CpuScale::PerCore => process.cpu,
        CpuScale::PerMachine => process.cpu_machine,
    };
    ProcessEntry {
        pid: process.key.pid.get(),
        name: Arc::clone(&process.name),
        user,
        cpu_percent: cpu.get(),
        memory_bytes: process.memory().get(),
        threads: process.threads,
        mark: if process.is_new {
            RowMark::New
        } else {
            RowMark::None
        },
        facts: ProcessFacts {
            start_ticks: process.key.start_ticks,
            parent: process.parent.get(),
            uid: process.uid.get(),
            command_line: Arc::clone(&process.command_line),
            path: process.executable.clone(),
            role: process.role,
            state: process.state,
            nice: process.nice,
            started_unix: process.started_unix,
            disk_read: process.disk_read.get() as f32,
            disk_write: process.disk_write.get() as f32,
            download: process.external.network_receive.get() as f32,
            upload: process.external.network_transmit.get() as f32,
            read_total: process.read_since_start.map_or(0, |read| read.get()),
            written_total: process
                .written_since_start
                .map_or(0, |written| written.get()),
            received_total: process.external.network_received_total.get(),
            sent_total: process.external.network_transmitted_total.get(),
            gpu_percent: process.external.gpu.get(),
            kernel_thread: process.kernel_thread,
            ended_unix: 0,
            peak_memory: 0,
        },
    }
}

/// The same app keeps the same row key across ticks, so sorting,
/// expansion and selection survive refills.
fn stable_key(key: &AppKey) -> u64 {
    let mut hasher = DefaultHasher::new();
    key.as_str().hash(&mut hasher);
    hasher.finish()
}

const fn kind_icon(kind: AppKind) -> &'static str {
    match kind {
        AppKind::Desktop => icons::APP_WINDOW,
        AppKind::Flatpak | AppKind::Snap => icons::PACKAGE,
        AppKind::Terminal => icons::SQUARE_TERMINAL,
        AppKind::Container => icons::CONTAINER,
        AppKind::Service => icons::COG,
        AppKind::Kernel => icons::CPU,
        AppKind::OtherUser => icons::USERS,
        AppKind::Process => icons::BOX,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_keys_are_stable() {
        let firefox = AppKey::new(AppKind::Desktop, "firefox");
        assert_eq!(stable_key(&firefox), stable_key(&firefox.clone()));
        assert_ne!(
            stable_key(&firefox),
            stable_key(&AppKey::new(AppKind::Flatpak, "firefox"))
        );
    }
}
