use monitor_core::Pid;
use monitor_core::util::parse::find_value;

use super::procfile::{Restrictions, process_file, read_link, read_text};
use crate::apps::cgroup::{CgroupIdentity, parse_cgroup};
use crate::inspect::details::Security;
use crate::inspect::security::{CapabilitySet, Sandbox, SeccompMode, flatpak_app_id};
use crate::process::{StatFields, parse_stat};

const NAMESPACES: [&str; 8] = ["cgroup", "ipc", "mnt", "net", "pid", "time", "user", "uts"];
const SANDBOX_ANCESTRY_DEPTH: usize = 32;
const INIT_PID: u32 = 1;

pub(super) fn describe_security(
    pid: Pid,
    stat: &StatFields,
    status: &str,
    cgroup: &str,
    executable: Option<&str>,
) -> Security {
    let value = |field: &str| {
        find_value(status.as_bytes(), field.as_bytes())
            .and_then(|value| std::str::from_utf8(value).ok())
            .unwrap_or("")
    };
    let mut restrictions = Restrictions::default();
    let namespaces = differing_namespaces(pid, &mut restrictions);
    let lsm_label = ["attr/current", "attr/apparmor/current"]
        .iter()
        .find_map(|name| read_text(pid, name).ok())
        .map(|label| label.trim_end_matches(['\0', '\n']).to_owned())
        .filter(|label| !label.is_empty());
    Security {
        sandbox: detect_sandbox(pid, stat, cgroup, executable, &mut restrictions),
        namespaces,
        seccomp: SeccompMode::parse(value("Seccomp")),
        no_new_privileges: value("NoNewPrivs") == "1",
        effective: CapabilitySet::parse_hex(value("CapEff")),
        permitted: CapabilitySet::parse_hex(value("CapPrm")),
        bounding: CapabilitySet::parse_hex(value("CapBnd")),
        lsm_label,
        coverage: restrictions.coverage(),
    }
}

/// Namespaces that differ from PID 1's, or from the monitor's own when
/// PID 1's cannot be read.
fn differing_namespaces(pid: Pid, restrictions: &mut Restrictions) -> Vec<&'static str> {
    let reference = |kind: &str| {
        std::fs::read_link(format!("/proc/1/ns/{kind}"))
            .or_else(|_| std::fs::read_link(format!("/proc/self/ns/{kind}")))
            .ok()
    };
    NAMESPACES
        .into_iter()
        .filter(|kind| {
            let Some(target) = restrictions.note(read_link(pid, &format!("ns/{kind}"))) else {
                return false;
            };
            reference(kind).is_some_and(|reference| reference.to_string_lossy() != target)
        })
        .collect()
}

fn detect_sandbox(
    pid: Pid,
    stat: &StatFields,
    cgroup: &str,
    executable: Option<&str>,
    restrictions: &mut Restrictions,
) -> Sandbox {
    if let Some(info) = restrictions.note(read_text(pid, "root/.flatpak-info")) {
        return Sandbox::Flatpak {
            app_id: flatpak_app_id(&info),
        };
    }
    match parse_cgroup(cgroup) {
        CgroupIdentity::Flatpak(app_id) => {
            return Sandbox::Flatpak {
                app_id: Some(app_id.into_owned()),
            };
        }
        CgroupIdentity::Snap(name) => {
            return Sandbox::Snap {
                name: name.to_owned(),
            };
        }
        CgroupIdentity::Container(id) => return Sandbox::Container { id: id.to_owned() },
        _ => {}
    }
    if let Some(name) = executable
        .and_then(|path| path.strip_prefix("/snap/"))
        .and_then(|rest| rest.split('/').next())
    {
        return Sandbox::Snap {
            name: name.to_owned(),
        };
    }
    let mut parent = stat.parent;
    for _ in 0..SANDBOX_ANCESTRY_DEPTH {
        if parent <= INIT_PID {
            break;
        }
        let Some((comm, ancestor)) = std::fs::read(process_file(Pid::new(parent), "stat"))
            .ok()
            .and_then(|text| parse_stat(&text).map(|(comm, ancestor)| (comm.to_vec(), ancestor)))
        else {
            break;
        };
        match comm.as_slice() {
            b"bwrap" => return Sandbox::Bubblewrap,
            b"firejail" => return Sandbox::Firejail,
            _ => parent = ancestor.parent,
        }
    }
    Sandbox::None
}
