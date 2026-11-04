use crate::util::text::basename;

/// Capability names by bit number, as in `linux/capability.h`.
const CAPABILITY_NAMES: [&str; 41] = [
    "chown",
    "dac_override",
    "dac_read_search",
    "fowner",
    "fsetid",
    "kill",
    "setgid",
    "setuid",
    "setpcap",
    "linux_immutable",
    "net_bind_service",
    "net_broadcast",
    "net_admin",
    "net_raw",
    "ipc_lock",
    "ipc_owner",
    "sys_module",
    "sys_rawio",
    "sys_chroot",
    "sys_ptrace",
    "sys_pacct",
    "sys_admin",
    "sys_boot",
    "sys_nice",
    "sys_resource",
    "sys_time",
    "sys_tty_config",
    "mknod",
    "lease",
    "audit_write",
    "audit_control",
    "setfcap",
    "mac_override",
    "mac_admin",
    "syslog",
    "wake_alarm",
    "block_suspend",
    "audit_read",
    "perfmon",
    "bpf",
    "checkpoint_restore",
];

/// A capability mask from `/proc/<pid>/status` with its names decoded.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CapabilitySet {
    pub raw: u64,
    pub names: Vec<&'static str>,
}

impl CapabilitySet {
    pub fn from_raw(raw: u64) -> Self {
        Self {
            raw,
            names: CAPABILITY_NAMES
                .iter()
                .enumerate()
                .filter(|(bit, _)| raw & (1_u64 << bit) != 0)
                .map(|(_, name)| *name)
                .collect(),
        }
    }

    pub fn parse_hex(text: &str) -> Self {
        Self::from_raw(u64::from_str_radix(text.trim(), 16).unwrap_or(0))
    }

    /// Every known capability is set (the usual bounding set).
    pub fn is_full(&self) -> bool {
        self.names.len() == CAPABILITY_NAMES.len()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SeccompMode {
    #[default]
    Disabled,
    Strict,
    Filter,
}

impl SeccompMode {
    pub fn parse(text: &str) -> Self {
        match text.trim() {
            "1" => Self::Strict,
            "2" => Self::Filter,
            _ => Self::Disabled,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Sandbox {
    #[default]
    None,
    Flatpak {
        app_id: Option<String>,
    },
    Snap {
        name: String,
    },
    Bubblewrap,
    Firejail,
    Container {
        id: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Emulation {
    Wine,
    Proton,
    Box64,
    Fex,
    /// A `binfmt_misc` handler runs the program, e.g. `qemu-aarch64`.
    Binfmt {
        handler: String,
        interpreter: String,
    },
}

const WINE_PROGRAMS: [&str; 5] = [
    "wine",
    "wine64",
    "wine-preloader",
    "wine64-preloader",
    "wineserver",
];

/// Emulation guessed from the executable and command line.
pub(crate) fn emulation_hint(executable: &str, command_line: &[String]) -> Option<Emulation> {
    let program = basename(executable);
    let mentions = |needle: &str| {
        command_line
            .iter()
            .any(|argument| argument.contains(needle))
    };
    if mentions("/Proton") || mentions("/proton") {
        Some(Emulation::Proton)
    } else if WINE_PROGRAMS.contains(&program)
        || command_line
            .first()
            .is_some_and(|argv0| argv0.ends_with(".exe"))
    {
        Some(Emulation::Wine)
    } else if program.starts_with("box64") || program.starts_with("box86") {
        Some(Emulation::Box64)
    } else if program.starts_with("FEX") {
        Some(Emulation::Fex)
    } else {
        None
    }
}

/// The `interpreter` line of a `/proc/sys/fs/binfmt_misc/<name>` entry.
pub(crate) fn binfmt_interpreter(entry: &str) -> Option<&str> {
    entry
        .lines()
        .find_map(|line| line.strip_prefix("interpreter "))
        .map(str::trim)
}

/// `name=` from the `[Application]` group of `.flatpak-info`.
pub(crate) fn flatpak_app_id(info: &str) -> Option<String> {
    let mut in_application = false;
    for line in info.lines().map(str::trim) {
        if line.starts_with('[') {
            in_application = line == "[Application]";
        } else if in_application && let Some(name) = line.strip_prefix("name=") {
            return Some(name.to_owned());
        }
    }
    None
}

/// Decodes `tty_nr` from stat into a device name.
pub(crate) fn tty_name(tty: u32) -> Option<String> {
    if tty == 0 {
        return None;
    }
    let major = (tty >> 8) & 0xfff;
    let minor = (tty & 0xff) | ((tty >> 12) & 0xfff00);
    Some(match major {
        136..=143 => format!("pts/{}", minor + (major - 136) * 256),
        4 if minor < 64 => format!("tty{minor}"),
        4 => format!("ttyS{}", minor - 64),
        _ => format!("{major}:{minor}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_capabilities() {
        let empty = CapabilitySet::parse_hex("0000000000000000");
        assert!(empty.names.is_empty());
        let bounding = CapabilitySet::parse_hex("000001ffffffffff");
        assert!(bounding.is_full());
        let ambient = CapabilitySet::parse_hex("0000000800000000");
        assert_eq!(ambient.names, ["wake_alarm"]);
        let mixed = CapabilitySet::from_raw((1 << 12) | (1 << 21));
        assert_eq!(mixed.names, ["net_admin", "sys_admin"]);
    }

    #[test]
    fn parses_status_security_lines() {
        assert_eq!(SeccompMode::parse("2"), SeccompMode::Filter);
        assert_eq!(SeccompMode::parse("0"), SeccompMode::Disabled);
    }

    #[test]
    fn reads_flatpak_info() {
        let info = "[Application]\nname=org.gnome.Maps\nruntime=runtime/org.gnome.Platform/x86_64/46\n\n[Instance]\ninstance-id=123\n";
        assert_eq!(flatpak_app_id(info).as_deref(), Some("org.gnome.Maps"));
        assert_eq!(flatpak_app_id("[Instance]\nname=x\n"), None);
    }

    #[test]
    fn guesses_emulation() {
        let arguments = |items: &[&str]| {
            items
                .iter()
                .map(|item| (*item).to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            emulation_hint("/usr/bin/wine64-preloader", &arguments(&["C:\\game.exe"])),
            Some(Emulation::Wine)
        );
        assert_eq!(
            emulation_hint(
                "/home/u/.steam/steamapps/common/Proton 9.0/files/bin/wine64",
                &arguments(&["/home/u/.steam/steamapps/common/Proton 9.0/proton", "run"])
            ),
            Some(Emulation::Proton)
        );
        assert_eq!(
            emulation_hint("/usr/bin/kitty", &arguments(&["kitty"])),
            None
        );
        let entry = "enabled\ninterpreter /usr/bin/qemu-aarch64-static\nflags: F\noffset 0\n";
        assert_eq!(
            binfmt_interpreter(entry),
            Some("/usr/bin/qemu-aarch64-static")
        );
    }

    #[test]
    fn names_terminals() {
        assert_eq!(tty_name(34_816).as_deref(), Some("pts/0"));
        assert_eq!(tty_name(34_819).as_deref(), Some("pts/3"));
        assert_eq!(tty_name(1025).as_deref(), Some("tty1"));
        assert_eq!(tty_name(0), None);
    }
}
