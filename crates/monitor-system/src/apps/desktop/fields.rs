use crate::util::text::basename;

const DESKTOP_GROUP: &str = "[Desktop Entry]";

/// The `[Desktop Entry]` keys the monitor uses.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct DesktopFields {
    pub kind: Option<String>,
    pub name: Option<String>,
    pub generic_name: Option<String>,
    pub icon: Option<String>,
    pub exec: Option<String>,
    pub try_exec: Option<String>,
    pub startup_wm_class: Option<String>,
    pub categories: Option<String>,
    pub no_display: bool,
    pub hidden: bool,
}

pub(crate) fn parse_desktop_fields(text: &str) -> DesktopFields {
    let mut fields = DesktopFields::default();
    let mut in_group = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_group = line == DESKTOP_GROUP;
            continue;
        }
        let Some((key, value)) = line.split_once('=').filter(|_| in_group) else {
            continue;
        };
        let value = value.trim();
        let set = |slot: &mut Option<String>| {
            if slot.is_none() {
                *slot = Some(value.to_owned());
            }
        };
        match key.trim() {
            "Type" => set(&mut fields.kind),
            "Name" => set(&mut fields.name),
            "GenericName" => set(&mut fields.generic_name),
            "Icon" => set(&mut fields.icon),
            "Exec" => set(&mut fields.exec),
            "TryExec" => set(&mut fields.try_exec),
            "StartupWMClass" => set(&mut fields.startup_wm_class),
            "Categories" => set(&mut fields.categories),
            "NoDisplay" => fields.no_display = value == "true",
            "Hidden" => fields.hidden = value == "true",
            _ => {}
        }
    }
    fields
}

/// The program an `Exec=` line runs, past `env VAR=…` wrappers.
pub(crate) fn exec_program(exec: &str) -> Option<&str> {
    exec_parts(exec).map(|(program, _)| program)
}

/// The program and whether real arguments (not `%f`-style field codes)
/// follow it.
pub(super) fn exec_parts(exec: &str) -> Option<(&str, bool)> {
    let mut rest = exec.trim();
    while !rest.is_empty() {
        let (token, remainder) = match rest.strip_prefix('"') {
            Some(quoted) => quoted.split_once('"').unwrap_or((quoted, "")),
            None => rest.split_once(char::is_whitespace).unwrap_or((rest, "")),
        };
        rest = remainder.trim_start();
        let is_wrapper = token == "env" || (token.contains('=') && !token.starts_with('/'));
        if !is_wrapper {
            let program = basename(token);
            let has_arguments = rest
                .split_whitespace()
                .any(|argument| !argument.starts_with('%'));
            return (!program.is_empty() && !program.starts_with('%'))
                .then_some((program, has_arguments));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::desktop::fixtures::{KITTY, ORCA};

    #[test]
    fn parses_desktop_entries() {
        let fields = parse_desktop_fields(KITTY);
        assert_eq!(fields.name.as_deref(), Some("kitty"));
        assert_eq!(fields.try_exec.as_deref(), Some("kitty"));
        assert_eq!(
            fields.categories.as_deref(),
            Some("System;TerminalEmulator;")
        );
        let orca = parse_desktop_fields(ORCA);
        assert_eq!(
            orca.name.as_deref(),
            Some("Orca"),
            "actions do not override the entry"
        );
        assert_eq!(orca.startup_wm_class.as_deref(), Some("orca"));
    }

    #[test]
    fn finds_exec_program() {
        assert_eq!(exec_program("stably-orca %U"), Some("stably-orca"));
        assert_eq!(
            exec_program("env GDK_BACKEND=x11 /usr/bin/blender %f"),
            Some("blender")
        );
        assert_eq!(exec_program("\"/opt/app/My App\" %U"), Some("My App"));
        assert_eq!(exec_program("%U"), None);
        assert_eq!(exec_parts("stably-orca %U"), Some(("stably-orca", false)));
        assert_eq!(exec_parts("kitty /vm/start.sh"), Some(("kitty", true)));
    }
}
