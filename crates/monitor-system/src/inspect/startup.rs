use std::path::{Path, PathBuf};

use crate::apps::cgroup::leaf;
use crate::apps::desktop::{exec_program, parse_desktop_fields};

const SYSTEM_UNIT_DIRECTORIES: [&str; 7] = [
    "/etc/systemd/system",
    "/run/systemd/system",
    "/run/systemd/transient",
    "/run/systemd/generator",
    "/usr/local/lib/systemd/system",
    "/usr/lib/systemd/system",
    "/lib/systemd/system",
];
const USER_UNIT_DIRECTORIES: [&str; 3] = [
    "/etc/systemd/user",
    "/usr/local/lib/systemd/user",
    "/usr/lib/systemd/user",
];
const AUTOSTART_DIRECTORY: &str = "/etc/xdg/autostart";

/// The unit named by the cgroup leaf, if it is a service or scope.
pub(crate) fn unit_name(cgroup: &str) -> Option<&str> {
    let unit = leaf(cgroup);
    (unit.ends_with(".service") || unit.ends_with(".scope")).then_some(unit)
}

fn unit_directories(cgroup: &str, uid: u32) -> Vec<PathBuf> {
    if !cgroup.contains("/user@") {
        return SYSTEM_UNIT_DIRECTORIES.iter().map(PathBuf::from).collect();
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let runtime = PathBuf::from(format!("/run/user/{uid}/systemd"));
    let mut directories = Vec::with_capacity(8);
    if let Some(home) = &home {
        directories.push(home.join(".config/systemd/user"));
    }
    directories.push(runtime.join("user"));
    directories.push(runtime.join("transient"));
    directories.push(runtime.join("generator"));
    if let Some(home) = &home {
        directories.push(home.join(".local/share/systemd/user"));
    }
    directories.extend(USER_UNIT_DIRECTORIES.iter().map(PathBuf::from));
    directories
}

/// Finds the unit file, trying `name@.service` for template instances.
pub(crate) fn find_unit_file(unit: &str, cgroup: &str, uid: u32) -> Option<PathBuf> {
    let template = unit.split_once('@').and_then(|(prefix, rest)| {
        rest.rsplit_once('.')
            .map(|(_, suffix)| format!("{prefix}@.{suffix}"))
    });
    let names: Vec<&str> = std::iter::once(unit).chain(template.as_deref()).collect();
    unit_directories(cgroup, uid)
        .into_iter()
        .flat_map(|directory| names.iter().map(move |name| directory.join(name)))
        .find(|path| path.is_file())
}

/// `Description=` from the `[Unit]` section.
pub(crate) fn unit_description(text: &str) -> Option<String> {
    let mut in_unit = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_unit = line == "[Unit]";
        } else if in_unit && let Some(description) = line.strip_prefix("Description=") {
            return Some(description.trim().to_owned());
        }
    }
    None
}

/// An autostart entry whose `Exec=` runs `program`.
pub(crate) fn find_autostart(program: &str) -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")));
    let directories = config
        .map(|config| config.join("autostart"))
        .into_iter()
        .chain(std::iter::once(PathBuf::from(AUTOSTART_DIRECTORY)));
    directories
        .filter_map(|directory| std::fs::read_dir(directory).ok())
        .flat_map(|entries| entries.flatten().map(|entry| entry.path()))
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "desktop")
        })
        .find(|path| launches(path, program))
}

fn launches(path: &Path, program: &str) -> bool {
    std::fs::read_to_string(path).is_ok_and(|text| {
        let fields = parse_desktop_fields(&text);
        !fields.hidden && fields.exec.as_deref().and_then(exec_program) == Some(program)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_units_from_cgroups() {
        assert_eq!(
            unit_name("/system.slice/sshd.service"),
            Some("sshd.service")
        );
        assert_eq!(
            unit_name(
                "/user.slice/user-1000.slice/user@1000.service/app.slice/app-orca-2258.scope"
            ),
            Some("app-orca-2258.scope")
        );
        assert_eq!(unit_name("/"), None);
    }

    #[test]
    fn parses_descriptions() {
        let unit = "[Unit]\nDescription=OpenSSH Daemon\nWants=sshdgenkeys.service\n\n[Service]\nExecStart=/usr/bin/sshd -D\n";
        assert_eq!(unit_description(unit).as_deref(), Some("OpenSSH Daemon"));
        assert_eq!(unit_description("[Service]\nDescription=x\n"), None);
    }

    #[test]
    fn finds_live_system_unit() {
        let found = find_unit_file(
            "systemd-journald.service",
            "/system.slice/systemd-journald.service",
            0,
        );
        if let Some(path) = found {
            assert!(path.ends_with("systemd-journald.service"));
        }
    }
}
