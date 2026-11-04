use std::cell::OnceCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use monitor_core::{AppKey, AppKind, Uid};

use super::cgroup::{CgroupIdentity, parse_cgroup, service_display_name};
use super::desktop::{DesktopEntry, DesktopIndex};
use crate::util::passwd;
use crate::util::text::basename;

const KERNEL_APP_ID: &str = "kernel";
const KERNEL_APP_NAME: &str = "Kernel";
const USER_UNIT_PREFIX: &str = "user:";

/// The app a process belongs to, with what the table shows for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AppIdentity {
    pub key: AppKey,
    pub name: Arc<str>,
    pub icon: Option<Arc<str>>,
    /// Children found through ancestry may join this app.
    pub inheritable: bool,
}

impl AppIdentity {
    fn new(kind: AppKind, id: &str, name: Arc<str>, icon: Option<Arc<str>>) -> Self {
        Self {
            key: AppKey::new(kind, id),
            name,
            icon,
            inheritable: matches!(
                kind,
                AppKind::Desktop
                    | AppKind::Flatpak
                    | AppKind::Snap
                    | AppKind::Terminal
                    | AppKind::Container
            ),
        }
    }

    pub fn from_entry(kind: AppKind, entry: &DesktopEntry) -> Self {
        Self::new(kind, &entry.id, entry.name.clone(), entry.icon.clone())
    }
}

/// What the resolver needs to know about one process.
pub(crate) struct ProcessFacts<'a> {
    pub uid: Uid,
    pub name: &'a str,
    pub executable: Option<&'a str>,
    pub command_line: &'a str,
    pub cgroup: &'a str,
    pub kernel_thread: bool,
}

impl ProcessFacts<'_> {
    fn program_names(&self) -> [&str; 3] {
        let argv0 = self.command_line.split(' ').next().map_or("", basename);
        [self.executable.map_or("", basename), self.name, argv0]
    }
}

/// A direct resolution and whether the process sits in the app's own scope.
pub(crate) struct Resolved {
    pub identity: AppIdentity,
    pub owns_scope: bool,
}

/// Maps processes to apps using plan §3.2: kernel threads, other users,
/// cgroup names, then desktop entries. Ancestry is the caller's fallback.
pub(crate) struct AppResolver {
    desktop: DesktopIndex,
    own_uid: Uid,
    user_names: OnceCell<HashMap<u32, Arc<str>>>,
    kernel: AppIdentity,
}

impl AppResolver {
    pub fn new(desktop: DesktopIndex, own_uid: Uid) -> Self {
        Self {
            desktop,
            own_uid,
            user_names: OnceCell::new(),
            kernel: AppIdentity::new(AppKind::Kernel, KERNEL_APP_ID, KERNEL_APP_NAME.into(), None),
        }
    }

    pub fn desktop(&self) -> &DesktopIndex {
        &self.desktop
    }

    pub fn refresh_desktop(&mut self, now: Duration) -> bool {
        self.desktop.refresh_if_changed(now)
    }

    /// Resolves without looking at other processes, or `None` when only
    /// ancestry can tell.
    pub fn direct(&self, facts: &ProcessFacts<'_>) -> Option<Resolved> {
        if facts.kernel_thread {
            return Some(Resolved {
                identity: self.kernel.clone(),
                owns_scope: false,
            });
        }
        let cgroup = parse_cgroup(facts.cgroup);
        let owns_scope = cgroup.owns_scope();
        let identity = match cgroup {
            CgroupIdentity::Container(id) => {
                Some(AppIdentity::new(AppKind::Container, id, id.into(), None))
            }
            CgroupIdentity::Service(unit) => Some(self.service(&unit, facts)),
            _ if facts.uid != self.own_uid => Some(self.other_user(facts.uid)),
            CgroupIdentity::App { full, tail } => Some(self.desktop_app(&full, &tail)),
            CgroupIdentity::Flatpak(id) => Some(self.sandboxed(AppKind::Flatpak, &id)),
            CgroupIdentity::Snap(name) => Some(self.sandboxed(AppKind::Snap, name)),
            CgroupIdentity::Terminal(name) => Some(self.terminal(name)),
            CgroupIdentity::Session | CgroupIdentity::Unknown => {
                return self.desktop_match(facts).map(|identity| Resolved {
                    identity,
                    owns_scope: false,
                });
            }
        };
        identity.map(|identity| Resolved {
            identity,
            owns_scope,
        })
    }

    /// A desktop entry for the process itself, by program name.
    pub fn desktop_match(&self, facts: &ProcessFacts<'_>) -> Option<AppIdentity> {
        self.desktop
            .find_program(facts.program_names())
            .map(|entry| AppIdentity::from_entry(AppKind::Desktop, entry))
    }

    /// The last resort: the process name is the app.
    pub fn by_name(&self, facts: &ProcessFacts<'_>) -> AppIdentity {
        AppIdentity::new(AppKind::Process, facts.name, facts.name.into(), None)
    }

    fn desktop_app(&self, full: &str, tail: &str) -> AppIdentity {
        match self
            .desktop
            .find_app_id(full)
            .or_else(|| self.desktop.find_app_id(tail))
        {
            Some(entry) => AppIdentity::from_entry(AppKind::Desktop, entry),
            None => AppIdentity::new(AppKind::Desktop, tail, tail.into(), None),
        }
    }

    fn sandboxed(&self, kind: AppKind, id: &str) -> AppIdentity {
        let entry = self
            .desktop
            .find_app_id(id)
            .or_else(|| self.desktop.find_program([id]));
        match entry {
            Some(entry) => AppIdentity::new(kind, id, entry.name.clone(), entry.icon.clone()),
            None => AppIdentity::new(kind, id, id.into(), None),
        }
    }

    fn terminal(&self, name: &str) -> AppIdentity {
        match self.desktop.find_program([name]) {
            Some(entry) => AppIdentity::new(
                AppKind::Terminal,
                name,
                entry.name.clone(),
                entry.icon.clone(),
            ),
            None => AppIdentity::new(AppKind::Terminal, name, name.into(), None),
        }
    }

    /// System units keep their name as id; the user's own units get a
    /// `user:` prefix, since both managers may run a unit of the same name.
    fn service(&self, unit: &str, facts: &ProcessFacts<'_>) -> AppIdentity {
        let own = facts.uid == self.own_uid;
        let entry = own
            .then(|| self.desktop.find_program(facts.program_names()))
            .flatten();
        let name = entry.map_or_else(
            || service_display_name(unit).into(),
            |entry| entry.name.clone(),
        );
        let icon = entry.and_then(|entry| entry.icon.clone());
        if own {
            AppIdentity::new(
                AppKind::Service,
                &format!("{USER_UNIT_PREFIX}{unit}"),
                name,
                icon,
            )
        } else {
            AppIdentity::new(AppKind::Service, unit, name, icon)
        }
    }

    fn other_user(&self, uid: Uid) -> AppIdentity {
        let names = self.user_names.get_or_init(passwd::user_names);
        let name = names
            .get(&uid.get())
            .cloned()
            .unwrap_or_else(|| uid.get().to_string().into());
        AppIdentity::new(AppKind::OtherUser, &name, name.clone(), None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolver() -> AppResolver {
        AppResolver::new(DesktopIndex::default(), Uid::new(1000))
    }

    fn facts<'a>(uid: u32, name: &'a str, cgroup: &'a str) -> ProcessFacts<'a> {
        ProcessFacts {
            uid: Uid::new(uid),
            name,
            executable: None,
            command_line: name,
            cgroup,
            kernel_thread: false,
        }
    }

    #[test]
    fn resolves_by_cgroup() {
        let resolver = resolver();
        let resolve = |resolver: &AppResolver, process: ProcessFacts<'_>| {
            resolver.direct(&process).map(|resolved| {
                (
                    resolved.identity.key.as_str().to_owned(),
                    resolved.owns_scope,
                )
            })
        };
        assert_eq!(
            resolve(
                &resolver,
                facts(
                    1000,
                    "orca-ide",
                    "/user.slice/user-1000.slice/user@1000.service/app.slice/app-orca-2258.scope"
                )
            ),
            Some(("desktop:orca".to_owned(), true))
        );
        assert_eq!(
            resolve(&resolver, facts(0, "sshd", "/system.slice/sshd.service")),
            Some(("service:sshd.service".to_owned(), true))
        );
        assert_eq!(
            resolve(
                &resolver,
                facts(0, "Xwayland", "/user.slice/user-1000.slice/session-2.scope")
            )
            .map(|(key, _)| key),
            Some("user:root".to_owned())
        );
        assert_eq!(
            resolve(
                &resolver,
                facts(
                    1000,
                    "pipewire",
                    "/user.slice/user-1000.slice/user@1000.service/session.slice/pipewire.service"
                )
            ),
            Some(("service:user:pipewire.service".to_owned(), true))
        );
        assert_eq!(
            resolve(
                &resolver,
                facts(
                    1000,
                    "crownbar",
                    "/user.slice/user-1000.slice/session-2.scope"
                )
            ),
            None
        );
        assert_eq!(
            resolver.by_name(&facts(1000, "crownbar", "/")).key.as_str(),
            "process:crownbar"
        );
    }

    #[test]
    fn groups_kernel_threads() {
        let resolver = resolver();
        let mut kworker = facts(0, "kworker/0:1", "/");
        kworker.kernel_thread = true;
        let resolved = resolver.direct(&kworker).map(|resolved| resolved.identity);
        assert_eq!(
            resolved.map(|identity| identity.key.kind()),
            Some(AppKind::Kernel)
        );
    }
}
