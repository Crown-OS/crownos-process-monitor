use std::borrow::Cow;

use crate::util::text::unescape_unit;

const CONTAINER_ID_LENGTH: usize = 12;
const SERVICE_MANAGER_NAME: &str = "systemd";

/// What a cgroup v2 path says about the app that owns a process, following
/// the systemd desktop-environment naming conventions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CgroupIdentity<'a> {
    /// `app-[<launcher>-]<app id>-<random>.scope` or `app-[<launcher>-]<app id>@<random>.service`.
    /// `full` keeps every dash-separated part, `tail` drops the launcher.
    App {
        full: Cow<'a, str>,
        tail: Cow<'a, str>,
    },
    Flatpak(Cow<'a, str>),
    Snap(&'a str),
    /// `<terminal>-<pid>-<n>.scope`, e.g. `kitty-1234-0.scope`.
    Terminal(&'a str),
    Container(&'a str),
    Service(Cow<'a, str>),
    /// A logind session scope, which holds whatever the session started.
    Session,
    Unknown,
}

impl CgroupIdentity<'_> {
    /// Whether the leaf cgroup belongs to this app alone, so its cgroup
    /// counters describe exactly the app.
    pub fn owns_scope(&self) -> bool {
        !matches!(self, Self::Session | Self::Unknown)
    }
}

/// The cgroup path from a `/proc/<pid>/cgroup` text (the v2 `0::` line).
pub(crate) fn unified_path(text: &str) -> &str {
    text.lines()
        .find_map(|line| line.strip_prefix("0::"))
        .unwrap_or("/")
        .trim()
}

pub(crate) fn leaf(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The innermost `.scope` or `.service` component and the path up to it.
/// Services may keep processes in sub-cgroups (`systemd-udevd.service/udev`).
pub(crate) fn unit_path(path: &str) -> (&str, &str) {
    let mut end = path.len();
    loop {
        let start = path[..end].rfind('/').map_or(0, |slash| slash + 1);
        let component = &path[start..end];
        if component.ends_with(".scope") || component.ends_with(".service") {
            return (&path[..end], component);
        }
        if start == 0 {
            return (path, leaf(path));
        }
        end = start - 1;
    }
}

pub(crate) fn parse_cgroup(path: &str) -> CgroupIdentity<'_> {
    let (_, leaf) = unit_path(path);
    if let Some(identity) = container(leaf) {
        return identity;
    }
    if let Some(body) = leaf
        .strip_prefix("snap.")
        .and_then(|body| body.strip_suffix(".scope"))
    {
        return CgroupIdentity::Snap(body.split('.').next().unwrap_or(body));
    }
    if let Some(identity) = leaf.strip_prefix("app-").and_then(app_unit) {
        return identity;
    }
    if let Some(scope) = leaf.strip_suffix(".scope") {
        if scope.starts_with("session-") {
            return CgroupIdentity::Session;
        }
        if leaf == "init.scope" {
            return CgroupIdentity::Service(Cow::Borrowed(leaf));
        }
        return terminal(scope).unwrap_or(CgroupIdentity::Unknown);
    }
    if leaf.ends_with(".service") {
        return CgroupIdentity::Service(unescape_unit(leaf));
    }
    CgroupIdentity::Unknown
}

fn container(leaf: &str) -> Option<CgroupIdentity<'_>> {
    let id = ["docker-", "libpod-conmon-", "libpod-"]
        .iter()
        .find_map(|prefix| leaf.strip_prefix(prefix))?
        .strip_suffix(".scope")?;
    Some(CgroupIdentity::Container(
        &id[..id.len().min(CONTAINER_ID_LENGTH)],
    ))
}

fn app_unit(body: &str) -> Option<CgroupIdentity<'_>> {
    let parts: Vec<&str> = if let Some(scope) = body.strip_suffix(".scope") {
        let mut parts: Vec<&str> = scope.split('-').collect();
        if parts.len() > 1 {
            parts.pop();
        }
        parts
    } else {
        let service = body.strip_suffix(".service")?;
        service
            .split('@')
            .next()
            .unwrap_or(service)
            .split('-')
            .collect()
    };
    let (&first, rest) = parts.split_first()?;
    if first == "flatpak" && !rest.is_empty() {
        return Some(CgroupIdentity::Flatpak(join_unescaped(rest)));
    }
    let full = join_unescaped(&parts);
    let tail = if rest.is_empty() {
        full.clone()
    } else {
        join_unescaped(rest)
    };
    Some(CgroupIdentity::App { full, tail })
}

fn join_unescaped<'a>(parts: &[&'a str]) -> Cow<'a, str> {
    match parts {
        [single] => unescape_unit(single),
        _ => Cow::Owned(unescape_unit(&parts.join("-")).into_owned()),
    }
}

fn terminal(scope: &str) -> Option<CgroupIdentity<'_>> {
    let mut parts = scope.rsplitn(3, '-');
    let instance = parts.next()?;
    let pid = parts.next()?;
    let name = parts.next()?;
    let numeric = |text: &str| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
    (numeric(instance) && numeric(pid) && !name.is_empty())
        .then_some(CgroupIdentity::Terminal(name))
}

/// A readable name for a unit: `pipewire.service` → `pipewire`,
/// `dbus-:1.27-org.a11y.atspi.Registry@0.service` → `org.a11y.atspi.Registry`.
pub(crate) fn service_display_name(unit: &str) -> &str {
    let name = unit
        .strip_suffix(".service")
        .or_else(|| unit.strip_suffix(".scope"))
        .unwrap_or(unit);
    let name = name.split('@').next().unwrap_or(name);
    if name == "init" {
        return SERVICE_MANAGER_NAME;
    }
    match name.strip_prefix("dbus-:") {
        Some(bus) => bus.split_once('-').map_or(bus, |(_, service)| service),
        None => name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(full: &str, tail: &str) -> CgroupIdentity<'static> {
        CgroupIdentity::App {
            full: Cow::Owned(full.to_owned()),
            tail: Cow::Owned(tail.to_owned()),
        }
    }

    #[test]
    fn parses_app_scopes() {
        assert_eq!(
            parse_cgroup(
                "/user.slice/user-1000.slice/user@1000.service/app.slice/app-orca-2258.scope"
            ),
            app("orca", "orca")
        );
        assert_eq!(
            parse_cgroup(
                "/user.slice/user-1000.slice/user@1000.service/app.slice/app-com.google.Chrome-77927.scope"
            ),
            app("com.google.Chrome", "com.google.Chrome")
        );
        assert_eq!(
            parse_cgroup(
                "/user.slice/user-1000.slice/user@1000.service/app.slice/app-gnome-org.gnome.Terminal-4021.scope"
            ),
            app("gnome-org.gnome.Terminal", "org.gnome.Terminal")
        );
        assert_eq!(
            parse_cgroup(
                "/user.slice/user-1000.slice/user@1000.service/app.slice/app-gnome-my\\x2dapp@1a2b.service"
            ),
            app("gnome-my-app", "my-app")
        );
    }

    #[test]
    fn parses_sandboxes_and_containers() {
        assert_eq!(
            parse_cgroup(
                "/user.slice/user-1000.slice/user@1000.service/app.slice/app-flatpak-org.gnome.Maps-12345.scope"
            ),
            CgroupIdentity::Flatpak(Cow::Borrowed("org.gnome.Maps"))
        );
        assert_eq!(
            parse_cgroup(
                "/user.slice/user-1000.slice/user@1000.service/app.slice/snap.firefox.firefox-0f1b5c7f-b5de-4765-b8e1-df45d82b6a9f.scope"
            ),
            CgroupIdentity::Snap("firefox")
        );
        assert_eq!(
            parse_cgroup(
                "/system.slice/docker-4f2a9c1d8e7b6a5f4e3d2c1b0a9f8e7d6c5b4a3f2e1d0c9b8a7f6e5d4c3b2a1f.scope"
            ),
            CgroupIdentity::Container("4f2a9c1d8e7b")
        );
        assert_eq!(
            parse_cgroup(
                "/user.slice/user-1000.slice/user@1000.service/user.slice/libpod-0123456789abcdef.scope"
            ),
            CgroupIdentity::Container("0123456789ab")
        );
    }

    #[test]
    fn parses_terminals_services_and_sessions() {
        assert_eq!(
            parse_cgroup(
                "/user.slice/user-1000.slice/user@1000.service/app.slice/kitty-8373-0.scope"
            ),
            CgroupIdentity::Terminal("kitty")
        );
        assert_eq!(
            parse_cgroup("/system.slice/NetworkManager.service"),
            CgroupIdentity::Service(Cow::Borrowed("NetworkManager.service"))
        );
        assert_eq!(
            parse_cgroup(
                "/user.slice/user-1000.slice/user@1000.service/session.slice/pipewire.service"
            ),
            CgroupIdentity::Service(Cow::Borrowed("pipewire.service"))
        );
        assert_eq!(
            parse_cgroup("/user.slice/user-1000.slice/session-2.scope"),
            CgroupIdentity::Session
        );
        assert_eq!(parse_cgroup("/"), CgroupIdentity::Unknown);
        assert_eq!(
            parse_cgroup(
                "/user.slice/user-1000.slice/user@1000.service/orca_ptys_orca_daemon_0f1b.slice/orca_pty_orca_daemon_0f1b_3.scope"
            ),
            CgroupIdentity::Unknown
        );
        assert!(CgroupIdentity::Terminal("kitty").owns_scope());
        assert!(!CgroupIdentity::Session.owns_scope());
    }

    #[test]
    fn unescapes_dbus_activated_services() {
        let path = "/user.slice/user-1000.slice/user@1000.service/app.slice/app-dbus\\x2d:1.27\\x2dorg.a11y.atspi.Registry.slice/dbus-:1.27-org.a11y.atspi.Registry@0.service";
        let CgroupIdentity::Service(unit) = parse_cgroup(path) else {
            panic!("expected a service");
        };
        assert_eq!(service_display_name(&unit), "org.a11y.atspi.Registry");
        assert_eq!(service_display_name("pipewire.service"), "pipewire");
        assert_eq!(service_display_name("init.scope"), "systemd");
    }

    #[test]
    fn finds_units_above_sub_cgroups() {
        assert_eq!(
            unit_path("/system.slice/systemd-udevd.service/udev"),
            (
                "/system.slice/systemd-udevd.service",
                "systemd-udevd.service"
            )
        );
        assert_eq!(
            parse_cgroup("/system.slice/systemd-udevd.service/udev"),
            CgroupIdentity::Service(Cow::Borrowed("systemd-udevd.service"))
        );
        assert_eq!(unit_path("/"), ("/", ""));
        assert_eq!(unit_path("/user.slice"), ("/user.slice", "user.slice"));
    }

    #[test]
    fn reads_unified_line() {
        assert_eq!(
            unified_path("0::/user.slice/user-1000.slice/session-2.scope\n"),
            "/user.slice/user-1000.slice/session-2.scope"
        );
        assert_eq!(unified_path("12:pids:/\n0::/init.scope\n"), "/init.scope");
        assert_eq!(unified_path(""), "/");
    }
}
