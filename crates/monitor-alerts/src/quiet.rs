use monitor_core::AppKey;
use monitor_core::util::time::{DAY_SECONDS, local_midnight};

use crate::alert::AlertKind;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum QuietScope {
    App(AppKey),
    Kind(AlertKind),
}

impl QuietScope {
    pub fn matches(&self, app: Option<&AppKey>, kind: AlertKind) -> bool {
        match self {
            Self::App(quiet_app) => app == Some(quiet_app),
            Self::Kind(quiet_kind) => *quiet_kind == kind,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum QuietUntil {
    Time { unix_seconds: i64 },
    Forever,
}

impl QuietUntil {
    /// Until the next local midnight.
    pub fn tomorrow(now_unix: i64, utc_offset_seconds: i32) -> Self {
        Self::Time {
            unix_seconds: local_midnight(now_unix, utc_offset_seconds) + DAY_SECONDS,
        }
    }

    const fn covers(self, now_unix: i64) -> bool {
        match self {
            Self::Time { unix_seconds } => now_unix < unix_seconds,
            Self::Forever => true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct QuietRule {
    pub scope: QuietScope,
    pub until: QuietUntil,
}

const APP_SCOPE: &str = "app";
const KIND_SCOPE: &str = "kind";
const FOREVER: &str = "forever";

impl QuietRule {
    /// One line of the persisted quiet list: `app\t<app key>\t<until>` or
    /// `kind\t<alert code>\t<until>`, where until is unix seconds or `forever`.
    pub fn to_line(&self) -> String {
        let (scope, id) = match &self.scope {
            QuietScope::App(app) => (APP_SCOPE, app.as_str()),
            QuietScope::Kind(kind) => (KIND_SCOPE, kind.code()),
        };
        match self.until {
            QuietUntil::Time { unix_seconds } => format!("{scope}\t{id}\t{unix_seconds}"),
            QuietUntil::Forever => format!("{scope}\t{id}\t{FOREVER}"),
        }
    }

    /// `None` for malformed lines and alert kinds this version does not know.
    pub fn parse_line(line: &str) -> Option<Self> {
        let mut fields = line.trim_end().split('\t');
        let (scope, id, until) = (fields.next()?, fields.next()?, fields.next()?);
        let scope = match scope {
            APP_SCOPE => QuietScope::App(AppKey::parse(id)?),
            KIND_SCOPE => QuietScope::Kind(AlertKind::from_code(id)?),
            _ => return None,
        };
        let until = match until {
            FOREVER => QuietUntil::Forever,
            seconds => QuietUntil::Time {
                unix_seconds: seconds.parse().ok()?,
            },
        };
        Some(Self { scope, until })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct QuietList {
    rules: Vec<QuietRule>,
}

impl QuietList {
    pub fn quiet(&mut self, scope: QuietScope, until: QuietUntil) {
        self.unquiet(&scope);
        self.rules.push(QuietRule { scope, until });
    }

    pub fn unquiet(&mut self, scope: &QuietScope) {
        self.rules.retain(|rule| &rule.scope != scope);
    }

    pub fn is_quiet(&self, app: Option<&AppKey>, kind: AlertKind, now_unix: i64) -> bool {
        self.rules
            .iter()
            .any(|rule| rule.until.covers(now_unix) && rule.scope.matches(app, kind))
    }

    pub fn prune(&mut self, now_unix: i64) {
        self.rules.retain(|rule| rule.until.covers(now_unix));
    }

    pub fn rules(&self) -> &[QuietRule] {
        &self.rules
    }
}

#[cfg(test)]
mod tests {
    use monitor_core::AppKind;

    use super::*;

    #[test]
    fn rules_round_trip_through_lines() {
        let rules = [
            QuietRule {
                scope: QuietScope::App(AppKey::new(AppKind::Desktop, "org.mozilla.firefox")),
                until: QuietUntil::Forever,
            },
            QuietRule {
                scope: QuietScope::Kind(AlertKind::GpuBusy),
                until: QuietUntil::Time {
                    unix_seconds: 1_700_000_000,
                },
            },
        ];
        for rule in rules {
            assert_eq!(QuietRule::parse_line(&rule.to_line()), Some(rule));
        }
        assert_eq!(QuietRule::parse_line("kind\tretired_rule\tforever"), None);
        assert_eq!(QuietRule::parse_line("app\tdesktop:x"), None);
        assert_eq!(QuietRule::parse_line(""), None);
    }
}
