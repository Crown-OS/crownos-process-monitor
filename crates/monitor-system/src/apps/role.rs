use crate::util::text::switches;

/// What a member process does inside a browser or Electron app.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ProcessRole {
    #[default]
    Main,
    Tab,
    Extension,
    Graphics,
    Utility,
    Other,
}

impl ProcessRole {
    /// Reads Chromium/Electron `--type=` switches and Firefox `-contentproc`
    /// process types from a NUL-separated command line.
    pub(crate) fn from_command_line(raw: &[u8]) -> Self {
        let mut chromium_type = None;
        let mut extension = false;
        let mut firefox_content = false;
        let mut firefox_type = None;
        for argument in switches(raw) {
            if let Some(kind) = argument.strip_prefix(b"--type=") {
                chromium_type = Some(kind);
            } else if argument == b"--extension-process" {
                extension = true;
            } else if argument == b"-contentproc" {
                firefox_content = true;
            } else if argument.ends_with(b"ptype=crashpad-handler") {
                chromium_type = Some(b"crashpad-handler");
            } else if firefox_content {
                firefox_type = Some(argument)
                    .filter(|argument| Self::firefox_role(argument).is_some())
                    .or(firefox_type);
            }
        }
        match chromium_type {
            Some(b"renderer") if extension => Self::Extension,
            Some(b"renderer") => Self::Tab,
            Some(b"gpu-process") => Self::Graphics,
            Some(b"utility") => Self::Utility,
            Some(_) => Self::Other,
            None if firefox_content => firefox_type
                .and_then(Self::firefox_role)
                .unwrap_or(Self::Other),
            None => Self::Main,
        }
    }

    fn firefox_role(kind: &[u8]) -> Option<Self> {
        match kind {
            b"tab" => Some(Self::Tab),
            b"gpu" => Some(Self::Graphics),
            b"rdd" | b"socket" | b"utility" => Some(Self::Utility),
            b"forkserver" => Some(Self::Other),
            _ => None,
        }
    }
}

/// Member counts per role.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RoleBreakdown {
    pub main: u32,
    pub tab: u32,
    pub extension: u32,
    pub graphics: u32,
    pub utility: u32,
    pub other: u32,
}

impl RoleBreakdown {
    pub fn add(&mut self, role: ProcessRole) {
        *self.count_mut(role) += 1;
    }

    pub fn count(&self, role: ProcessRole) -> u32 {
        match role {
            ProcessRole::Main => self.main,
            ProcessRole::Tab => self.tab,
            ProcessRole::Extension => self.extension,
            ProcessRole::Graphics => self.graphics,
            ProcessRole::Utility => self.utility,
            ProcessRole::Other => self.other,
        }
    }

    fn count_mut(&mut self, role: ProcessRole) -> &mut u32 {
        match role {
            ProcessRole::Main => &mut self.main,
            ProcessRole::Tab => &mut self.tab,
            ProcessRole::Extension => &mut self.extension,
            ProcessRole::Graphics => &mut self.graphics,
            ProcessRole::Utility => &mut self.utility,
            ProcessRole::Other => &mut self.other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_chromium_processes() {
        let role = |line: &[u8]| ProcessRole::from_command_line(line);
        assert_eq!(
            role(b"/opt/stably-orca/orca-ide\0--type=renderer\0--crashpad-handler-pid=2296\0"),
            ProcessRole::Tab
        );
        assert_eq!(
            role(b"chrome\0--type=renderer\0--extension-process\0"),
            ProcessRole::Extension
        );
        assert_eq!(
            role(b"/opt/stably-orca/orca-ide\0--type=gpu-process\0--ozone-platform=x11\0"),
            ProcessRole::Graphics
        );
        assert_eq!(
            role(b"/opt/stably-orca/orca-ide\0--type=utility\0--utility-sub-type=network.mojom.NetworkService\0"),
            ProcessRole::Utility
        );
        assert_eq!(
            role(b"/opt/stably-orca/orca-ide\0--type=zygote\0"),
            ProcessRole::Other
        );
        assert_eq!(
            role(b"/opt/stably-orca/chrome_crashpad_handler\0--monitor-self-annotation=ptype=crashpad-handler\0"),
            ProcessRole::Other
        );
        assert_eq!(role(b"/opt/stably-orca/orca-ide\0"), ProcessRole::Main);
        assert_eq!(
            role(b"/opt/stably-orca/orca-ide --type=gpu-process --ozone-platform=x11 --crashpad-handler-pid=2296"),
            ProcessRole::Graphics,
            "rewritten command lines have no NUL separators"
        );
    }

    #[test]
    fn splits_firefox_processes() {
        let role = |line: &[u8]| ProcessRole::from_command_line(line);
        assert_eq!(
            role(b"/usr/lib/firefox/firefox\0-contentproc\0-childID\0" as &[u8]),
            ProcessRole::Other
        );
        assert_eq!(
            role(b"/usr/lib/firefox/firefox\0-contentproc\0-isForBrowser\0-prefsLen\x0031117\0{d1a8}\x002470\0tab\0"),
            ProcessRole::Tab
        );
        assert_eq!(
            role(b"firefox\0-contentproc\0{a}\x002470\0rdd\0"),
            ProcessRole::Utility
        );
        assert_eq!(
            role(b"firefox\0-contentproc\0{a}\x002470\0gpu\0"),
            ProcessRole::Graphics
        );
        assert_eq!(
            role(b"firefox\0-contentproc\0forkserver\0"),
            ProcessRole::Other
        );
    }

    #[test]
    fn counts_roles() {
        let mut breakdown = RoleBreakdown::default();
        breakdown.add(ProcessRole::Tab);
        breakdown.add(ProcessRole::Tab);
        breakdown.add(ProcessRole::Main);
        assert_eq!(breakdown.count(ProcessRole::Tab), 2);
        assert_eq!(breakdown.count(ProcessRole::Main), 1);
    }
}
