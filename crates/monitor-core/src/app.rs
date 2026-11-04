use std::fmt;
use std::sync::Arc;

/// Where an application identity came from. Ordered from most to least
/// specific so grouping can prefer the better source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AppKind {
    Desktop,
    Flatpak,
    Snap,
    Terminal,
    Container,
    Service,
    Kernel,
    OtherUser,
    Process,
}

impl AppKind {
    pub const ALL: [Self; 9] = [
        Self::Desktop,
        Self::Flatpak,
        Self::Snap,
        Self::Terminal,
        Self::Container,
        Self::Service,
        Self::Kernel,
        Self::OtherUser,
        Self::Process,
    ];

    #[must_use]
    pub const fn prefix(self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::Flatpak => "flatpak",
            Self::Snap => "snap",
            Self::Terminal => "terminal",
            Self::Container => "container",
            Self::Service => "service",
            Self::Kernel => "kernel",
            Self::OtherUser => "user",
            Self::Process => "process",
        }
    }

    #[must_use]
    pub fn from_prefix(prefix: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.prefix() == prefix)
    }
}

/// A stable, persistable application identifier such as
/// `desktop:org.mozilla.firefox` or `service:sshd.service`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AppKey(Arc<str>);

impl AppKey {
    const SEPARATOR: char = ':';

    #[must_use]
    pub fn new(kind: AppKind, id: &str) -> Self {
        let mut key = String::with_capacity(kind.prefix().len() + 1 + id.len());
        key.push_str(kind.prefix());
        key.push(Self::SEPARATOR);
        key.push_str(id);
        Self(key.into())
    }

    /// Parses a key previously produced by [`AppKey::new`], e.g. from history.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        let (prefix, id) = raw.split_once(Self::SEPARATOR)?;
        AppKind::from_prefix(prefix).map(|kind| Self::new(kind, id))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn kind(&self) -> AppKind {
        self.0
            .split_once(Self::SEPARATOR)
            .and_then(|(prefix, _)| AppKind::from_prefix(prefix))
            .unwrap_or(AppKind::Process)
    }

    #[must_use]
    pub fn id(&self) -> &str {
        self.0
            .split_once(Self::SEPARATOR)
            .map_or(&*self.0, |(_, id)| id)
    }
}

impl fmt::Display for AppKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_kind_and_id() {
        let key = AppKey::new(AppKind::Flatpak, "org.gnome.Maps");
        assert_eq!(key.as_str(), "flatpak:org.gnome.Maps");
        assert_eq!(key.kind(), AppKind::Flatpak);
        assert_eq!(key.id(), "org.gnome.Maps");
        assert_eq!(AppKey::parse(key.as_str()), Some(key));
    }

    #[test]
    fn ids_may_contain_separator() {
        let key = AppKey::new(AppKind::Container, "docker:abc");
        assert_eq!(key.id(), "docker:abc");
        assert_eq!(key.kind(), AppKind::Container);
    }

    #[test]
    fn rejects_unknown_prefix() {
        assert!(AppKey::parse("bogus:thing").is_none());
    }
}
