use std::time::Duration;

use monitor_core::Bytes;

use super::describe;
use super::fields::Fields;
use crate::inspect::details::{
    ChildProcess, DesktopEntryDetails, Identity, ProcessDetails, Resources, Security, Startup,
};

const YES: &str = "yes";

fn seconds(duration: Duration) -> String {
    format!("{:.2}", duration.as_secs_f64())
}

fn byte_count(value: Option<Bytes>) -> Option<String> {
    value.map(|bytes| bytes.get().to_string())
}

pub(super) fn identity<'a>(fields: &mut Fields<'a>, identity: &'a Identity) {
    fields
        .section("Identity")
        .add("Name", identity.name.as_str())
        .add("PID", identity.key.pid.to_string())
        .add("Parent", identity.parent.to_string())
        .add("App", format!("{} ({})", identity.app_name, identity.app))
        .add("Role", format!("{:?}", identity.role))
        .add(
            "User",
            format!(
                "{} ({})",
                identity.user.as_deref().unwrap_or_default(),
                identity.uid.get()
            ),
        )
        .add("GID", identity.gid.to_string())
        .add("State", format!("{:?}", identity.state))
        .add("Started (unix)", identity.started_unix.to_string())
        .add(
            "Running for (s)",
            identity.running_for.as_secs().to_string(),
        )
        .add_some("TTY", identity.tty.as_deref())
        .add("Session", identity.session.to_string())
        .add("Process group", identity.process_group.to_string());
}

pub(super) fn executable<'a>(fields: &mut Fields<'a>, details: &'a ProcessDetails) {
    let executable = &details.executable;
    fields
        .section("Executable")
        .add_some("Path", executable.path.as_deref())
        .add_some("Deleted", executable.deleted.then_some(YES))
        .add_some("Architecture", executable.elf.map(|elf| elf.architecture()))
        .add_some(
            "Emulation",
            executable.emulation.as_ref().map(describe::emulation),
        )
        .add("Command line", details.command_line.join(" "))
        .add_some("Working directory", details.working_directory.as_deref());
}

pub(super) fn security<'a>(fields: &mut Fields<'a>, security: &'a Security) {
    fields
        .section("Security")
        .add("Sandbox", describe::sandbox(&security.sandbox))
        .add("Namespaces", security.namespaces.join(", "))
        .add("Seccomp", format!("{:?}", security.seccomp))
        .add("NoNewPrivs", security.no_new_privileges.to_string())
        .add(
            "Effective capabilities",
            describe::capabilities(&security.effective),
        )
        .add(
            "Permitted capabilities",
            describe::capabilities(&security.permitted),
        )
        .add(
            "Bounding capabilities",
            describe::capabilities(&security.bounding),
        )
        .add_some("LSM label", security.lsm_label.as_deref());
}

pub(super) fn startup<'a>(fields: &mut Fields<'a>, startup: &'a Startup) {
    fields
        .section("Startup")
        .add_some("Unit", startup.unit.as_deref())
        .add_some(
            "Unit file",
            startup
                .unit_file
                .as_ref()
                .map(|path| path.to_string_lossy()),
        )
        .add_some("Description", startup.description.as_deref())
        .add_some(
            "Autostart",
            startup
                .autostart
                .as_ref()
                .map(|path| path.to_string_lossy()),
        );
}

pub(super) fn desktop_entry<'a>(fields: &mut Fields<'a>, entry: &'a DesktopEntryDetails) {
    fields
        .section("Desktop entry")
        .add("Id", entry.id.as_str())
        .add("File", entry.path.to_string_lossy())
        .add_some("Name", entry.name.as_deref())
        .add_some("Icon", entry.icon.as_deref())
        .add_some("Categories", entry.categories.as_deref())
        .add_some("Exec", entry.exec.as_deref())
        .add_some("StartupWMClass", entry.startup_wm_class.as_deref());
}

pub(super) fn resources<'a>(fields: &mut Fields<'a>, resources: &'a Resources) {
    fields
        .section("Resources")
        .add("User time (s)", seconds(resources.user_time))
        .add("System time (s)", seconds(resources.system_time))
        .add(
            "Children user time (s)",
            seconds(resources.children_user_time),
        )
        .add(
            "Children system time (s)",
            seconds(resources.children_system_time),
        )
        .add("RSS (bytes)", resources.resident.get().to_string())
        .add_some("PSS (bytes)", byte_count(resources.proportional))
        .add_some("USS (bytes)", byte_count(resources.unique))
        .add_some("Swap (bytes)", byte_count(resources.swap))
        .add_some("Peak RSS (bytes)", byte_count(resources.peak_resident))
        .add("Threads", resources.threads.to_string())
        .add_some(
            "Open files",
            resources.open_files.map(|count| count.to_string()),
        )
        .add("Minor faults", resources.minor_faults.to_string())
        .add("Major faults", resources.major_faults.to_string());
    if let Some(io) = resources.io {
        fields
            .add("Disk read (bytes)", io.read_bytes.to_string())
            .add("Disk written (bytes)", io.write_bytes.to_string());
    }
    fields
        .add_some(
            "Voluntary switches",
            resources.voluntary_switches.map(|count| count.to_string()),
        )
        .add_some(
            "Involuntary switches",
            resources
                .involuntary_switches
                .map(|count| count.to_string()),
        )
        .add("Nice", resources.nice.to_string())
        .add("Priority", resources.priority.to_string())
        .add("Scheduler", resources.policy.name())
        .add_some("CPU affinity", resources.cpus_allowed.as_deref())
        .add_some(
            "OOM score",
            resources.oom_score.map(|score| score.to_string()),
        )
        .add_some(
            "OOM score adjust",
            resources.oom_score_adjust.map(|score| score.to_string()),
        );
}

pub(super) fn children<'a>(fields: &mut Fields<'a>, children: &'a [ChildProcess]) {
    fields.section("Children");
    for child in children {
        fields.add(child.key.pid.to_string(), child.name.as_str());
    }
}

pub(super) fn environment<'a>(fields: &mut Fields<'a>, environment: &'a [(String, String)]) {
    fields.section("Environment");
    for (name, value) in environment {
        fields.add(name.as_str(), value.as_str());
    }
}
