use std::sync::Arc;
use std::time::Duration;

use monitor_core::util::parse::find_value;
use monitor_core::util::time::wall_unix_seconds;
use monitor_core::{AppKey, AppKind, Coverage, Pid, ProcessKey, SourceError, SourceResult, Uid};

use super::details::{DesktopEntryDetails, Identity, ProcessDetails, Startup};
use super::security::tty_name;
use super::startup::{find_autostart, find_unit_file, unit_description, unit_name};
use crate::apps::ProcessRole;
use crate::apps::cgroup::unified_path;
use crate::apps::desktop::{DesktopEntry, DesktopIndex, parse_desktop_fields};
use crate::apps::resolver::{AppResolver, ProcessFacts};
use crate::process::{ProcessState, parse_stat};
use crate::util::text::basename;
use crate::util::{clock, passwd};
use executable::describe_executable;
use procfile::{Restrictions, gone, process_file, read_link, read_text};
use resources::{children_of, describe_resources, read_environment};
use sandbox::describe_security;

mod executable;
mod procfile;
mod resources;
mod sandbox;
#[cfg(test)]
mod tests;

/// What the caller already knows about the process.
pub(crate) struct InspectContext<'a> {
    /// The app the process table put it in, when known.
    pub app: Option<(AppKey, Arc<str>)>,
    pub resolver: &'a AppResolver,
}

/// Gathers inspector details for `key` on demand. Builds a desktop index,
/// so prefer [`crate::ProcessCollector::inspect`] when a collector exists.
pub fn inspect(key: ProcessKey) -> SourceResult<ProcessDetails> {
    let resolver = AppResolver::new(DesktopIndex::load(), Uid::current());
    inspect_with(
        key,
        &InspectContext {
            app: None,
            resolver: &resolver,
        },
    )
}

pub(crate) fn inspect_with(
    key: ProcessKey,
    context: &InspectContext<'_>,
) -> SourceResult<ProcessDetails> {
    let pid = key.pid;
    let stat_text = std::fs::read(process_file(pid, "stat"))
        .map_err(|error| SourceError::io(process_file(pid, "stat"), error))?;
    let (comm, stat) = parse_stat(&stat_text).ok_or_else(|| gone(pid))?;
    if stat.start_ticks != key.start_ticks {
        return Err(gone(pid));
    }
    let name = String::from_utf8_lossy(comm).into_owned();
    let status = read_text(pid, "status").unwrap_or_default();
    let status_value = |field: &str| {
        find_value(status.as_bytes(), field.as_bytes())
            .and_then(|value| std::str::from_utf8(value).ok())
    };
    let uid = Uid::new(status_value("Uid").and_then(first_number).unwrap_or(0));
    let raw_command_line = std::fs::read(process_file(pid, "cmdline")).unwrap_or_default();
    let command_line: Vec<String> = crate::util::text::arguments(&raw_command_line)
        .map(|argument| String::from_utf8_lossy(argument).into_owned())
        .collect();
    let cgroup = unified_path(&read_text(pid, "cgroup").unwrap_or_default()).to_owned();

    let mut executable_restrictions = Restrictions::default();
    let executable_link = executable_restrictions.note(read_link(pid, "exe"));
    let executable =
        describe_executable(pid, executable_link, &command_line, executable_restrictions);
    let mut restrictions = Restrictions::default();
    let working_directory = restrictions.note(read_link(pid, "cwd"));

    let (app, app_name) = match &context.app {
        Some((app, name)) => (app.clone(), name.to_string()),
        None => resolve_app(
            context.resolver,
            &ProcessFacts {
                uid,
                name: &name,
                executable: executable.path.as_deref(),
                command_line: &command_line.join(" "),
                cgroup: &cgroup,
                kernel_thread: stat.is_kernel_thread(),
            },
        ),
    };
    let desktop_entry = find_desktop_entry(
        context.resolver.desktop(),
        &app,
        &name,
        executable.path.as_deref(),
    )
    .map(describe_desktop_entry);

    let program = executable.path.as_deref().map_or(name.as_str(), basename);
    let startup = describe_startup(&cgroup, uid, program);
    let security = describe_security(pid, &stat, &status, &cgroup, executable.path.as_deref());
    let resources = describe_resources(pid, &stat, &status);
    let own = uid == Uid::current();
    let environment = own.then(|| read_environment(pid)).flatten();
    let now = wall_unix_seconds();
    let started_unix = clock::start_unix_seconds(stat.start_ticks, clock::clock_ticks_per_second());
    let identity = Identity {
        key,
        parent: Pid::new(stat.parent),
        role: ProcessRole::from_command_line(&raw_command_line),
        name,
        app,
        app_name,
        kernel_thread: stat.is_kernel_thread(),
        uid,
        user: passwd::user_name(uid.get()),
        gid: status_value("Gid").and_then(first_number).unwrap_or(0),
        state: ProcessState::from_code(stat.state),
        started_unix,
        running_for: Duration::from_secs(now.saturating_sub(started_unix)),
        tty: tty_name(stat.tty),
        session: stat.session,
        process_group: stat.process_group,
    };
    let sections_restricted = [
        executable.coverage,
        security.coverage,
        resources.coverage,
        restrictions.coverage(),
    ]
    .contains(&Coverage::OwnProcesses);
    Ok(ProcessDetails {
        identity,
        executable,
        command_line,
        working_directory,
        security,
        startup,
        desktop_entry,
        resources,
        children: children_of(pid),
        environment,
        coverage: if sections_restricted || !own {
            Coverage::OwnProcesses
        } else {
            Coverage::Full
        },
    })
}

fn resolve_app(resolver: &AppResolver, facts: &ProcessFacts<'_>) -> (AppKey, String) {
    let identity = resolver
        .direct(facts)
        .map(|resolved| resolved.identity)
        .or_else(|| resolver.desktop_match(facts))
        .unwrap_or_else(|| resolver.by_name(facts));
    (identity.key, identity.name.to_string())
}

fn first_number(value: &str) -> Option<u32> {
    value
        .split_whitespace()
        .next()
        .and_then(|number| number.parse().ok())
}

fn find_desktop_entry<'a>(
    desktop: &'a DesktopIndex,
    app: &AppKey,
    name: &str,
    executable: Option<&str>,
) -> Option<&'a DesktopEntry> {
    match app.kind() {
        AppKind::Desktop | AppKind::Flatpak | AppKind::Snap => desktop
            .find_app_id(app.id())
            .or_else(|| desktop.find_program([app.id()])),
        AppKind::Terminal | AppKind::Process | AppKind::Service => {
            desktop.find_program([executable.map_or("", basename), name])
        }
        AppKind::Container | AppKind::Kernel | AppKind::OtherUser => None,
    }
}

fn describe_desktop_entry(entry: &DesktopEntry) -> DesktopEntryDetails {
    let text = std::fs::read_to_string(&entry.path).unwrap_or_default();
    let fields = parse_desktop_fields(&text);
    DesktopEntryDetails {
        id: entry.id.to_string(),
        path: entry.path.clone(),
        name: fields.name,
        generic_name: fields.generic_name,
        icon: fields.icon,
        categories: fields.categories,
        exec: fields.exec,
        startup_wm_class: fields.startup_wm_class,
        text,
    }
}

fn describe_startup(cgroup: &str, uid: Uid, program: &str) -> Startup {
    let unit = unit_name(cgroup);
    let unit_file = unit.and_then(|unit| find_unit_file(unit, cgroup, uid.get()));
    let description = unit_file
        .as_ref()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| unit_description(&text));
    Startup {
        unit: unit.map(str::to_owned),
        unit_file,
        description,
        autostart: find_autostart(program),
    }
}
