//! What the inspector reads about a sample process, answered without touching `/proc`.

use std::path::PathBuf;
use std::time::Duration;

use monitor_core::util::time::unix_now;
use monitor_core::{AppKey, Bytes, Coverage, Pid, ProcessKey, SourceError, SourceResult, Uid};
use monitor_engine::system::ProcessState;
use monitor_engine::system::inspect::{
    CapabilitySet, ChildProcess, DesktopEntryDetails, ElfClass, ElfInfo, Executable, Identity,
    IoTotals, ProcessDetails, Resources, Sandbox, SchedulingPolicy, SeccompMode, Security, Startup,
};

use crate::apps::{APPS, START_TICKS, USER_NAME, USER_UID};

const MIB: u64 = 1 << 20;
const X86_64: u16 = 62;
const RUNNING_FOR: Duration = Duration::from_secs(2 * 3600 + 47 * 60);

const ENVIRONMENT: [(&str, &str); 8] = [
    ("HOME", "/home/alex"),
    ("LANG", "en_US.UTF-8"),
    ("SHELL", "/usr/bin/fish"),
    ("USER", USER_NAME),
    ("WAYLAND_DISPLAY", "wayland-1"),
    ("XDG_CURRENT_DESKTOP", "CrownOS"),
    ("XDG_RUNTIME_DIR", "/run/user/1000"),
    ("XDG_SESSION_TYPE", "wayland"),
];

const DESKTOP_ENTRY: &str = "[Desktop Entry]\nName=Firefox\nGenericName=Web Browser\nExec=/usr/lib/firefox/firefox %u\nIcon=firefox\nType=Application\nCategories=Network;WebBrowser;\nStartupWMClass=firefox\n";

fn key(pid: u32) -> ProcessKey {
    ProcessKey {
        pid: Pid::new(pid),
        start_ticks: START_TICKS,
    }
}

/// Details for any sample pid; anything else reads as ended.
pub fn inspect(wanted: ProcessKey) -> SourceResult<ProcessDetails> {
    let (app, process) = APPS
        .iter()
        .find_map(|app| {
            app.processes
                .iter()
                .find(|process| key(process.pid) == wanted)
                .map(|process| (app, process))
        })
        .ok_or_else(|| SourceError::Io {
            path: "/proc".into(),
            source: std::io::ErrorKind::NotFound.into(),
        })?;
    let main = app.processes.first().map_or(1, |main| main.pid);
    let memory = Bytes::new(process.memory_mib * MIB);
    let started_unix = unix_now() as u64 - RUNNING_FOR.as_secs();
    let firefox = app.id == "firefox";
    Ok(ProcessDetails {
        identity: Identity {
            key: wanted,
            parent: Pid::new(if process.pid == main { 2104 } else { main }),
            name: process.name.to_owned(),
            app: AppKey::new(app.kind, app.id),
            app_name: app.name.to_owned(),
            role: process.role,
            kernel_thread: false,
            uid: Uid::new(USER_UID),
            user: Some(USER_NAME.to_owned()),
            gid: USER_UID,
            state: ProcessState::Sleeping,
            started_unix,
            running_for: RUNNING_FOR,
            tty: None,
            session: 2104,
            process_group: main,
        },
        executable: Executable {
            path: process.executable.map(str::to_owned),
            deleted: false,
            elf: Some(ElfInfo {
                class: ElfClass::Elf64,
                machine: X86_64,
            }),
            emulation: None,
            coverage: Coverage::Full,
        },
        command_line: process.command_line.split(' ').map(str::to_owned).collect(),
        working_directory: Some("/home/alex".to_owned()),
        security: Security {
            sandbox: Sandbox::None,
            namespaces: if firefox {
                vec!["user", "pid", "net"]
            } else {
                Vec::new()
            },
            seccomp: if firefox {
                SeccompMode::Filter
            } else {
                SeccompMode::Disabled
            },
            no_new_privileges: firefox,
            effective: CapabilitySet::from_raw(0),
            permitted: CapabilitySet::from_raw(0),
            bounding: CapabilitySet::from_raw(0x1ff_ffff_ffff),
            lsm_label: None,
            coverage: Coverage::Full,
        },
        startup: Startup {
            unit: Some(format!("app-crown-{}@4b1c.service", app.id)),
            unit_file: None,
            description: Some(format!("{} - launched by CrownOS", app.name)),
            autostart: None,
        },
        desktop_entry: firefox.then(|| DesktopEntryDetails {
            id: "firefox.desktop".to_owned(),
            path: PathBuf::from("/usr/share/applications/firefox.desktop"),
            name: Some("Firefox".to_owned()),
            generic_name: Some("Web Browser".to_owned()),
            icon: Some("firefox".to_owned()),
            categories: Some("Network;WebBrowser;".to_owned()),
            exec: Some("/usr/lib/firefox/firefox %u".to_owned()),
            startup_wm_class: Some("firefox".to_owned()),
            text: DESKTOP_ENTRY.to_owned(),
        }),
        resources: Resources {
            user_time: Duration::from_secs(1260),
            system_time: Duration::from_secs(214),
            resident: memory,
            proportional: Some(memory),
            unique: Some(Bytes::new(process.memory_mib * MIB * 3 / 4)),
            swap: Some(Bytes::new(process.memory_mib * MIB / 16)),
            peak_resident: Some(Bytes::new(process.memory_mib * MIB * 5 / 4)),
            threads: process.threads,
            open_files: Some(212),
            minor_faults: 1_284_330,
            major_faults: 412,
            io: Some(IoTotals {
                read_bytes: 412 * MIB,
                write_bytes: 96 * MIB,
                read_chars: 1900 * MIB,
                write_chars: 310 * MIB,
            }),
            voluntary_switches: Some(842_113),
            involuntary_switches: Some(12_904),
            nice: 0,
            priority: 20,
            policy: SchedulingPolicy::Normal,
            cpus_allowed: Some("0-15".to_owned()),
            oom_score: Some(712),
            oom_score_adjust: Some(0),
            coverage: Coverage::Full,
            ..Resources::default()
        },
        children: if process.pid == main {
            app.processes
                .iter()
                .skip(1)
                .map(|child| ChildProcess {
                    key: key(child.pid),
                    name: child.name.to_owned(),
                })
                .collect()
        } else {
            Vec::new()
        },
        environment: Some(
            ENVIRONMENT
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect(),
        ),
        coverage: Coverage::Full,
    })
}
