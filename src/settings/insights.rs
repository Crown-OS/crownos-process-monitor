use monitor_core::{AppKey, Bytes, Percent};
use monitor_engine::alerts::{AlertKind, AppLimit, CustomLimit, QuietScope, QuietUntil};
use serde::{Deserialize, Serialize};

/// Stored as `~/.config/crownos/process-monitor-insights.ron`: the list-shaped alert
/// settings, kept apart so `MonitorSettings` stays `Copy`.
pub const INSIGHTS_SECTION: &str = "process-monitor-insights";

/// Per-app limits and quieted alerts.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct InsightSettings {
    pub custom_limits: Vec<SavedLimit>,
    pub quieted: Vec<SavedQuiet>,
}

/// An app as the settings remember it: the engine's key and the name shown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedApp {
    pub key: String,
    pub name: String,
}

impl SavedApp {
    pub fn new(key: &AppKey, name: &str) -> Self {
        Self {
            key: key.as_str().to_owned(),
            name: name.to_owned(),
        }
    }

    pub fn app_key(&self) -> Option<AppKey> {
        AppKey::parse(&self.key)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum LimitValue {
    CpuPercent(f32),
    MemoryBytes(u64),
}

/// "Tell me when Firefox uses more than 4 GB for 5 min".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedLimit {
    pub app: SavedApp,
    pub value: LimitValue,
    pub minutes: u32,
}

impl SavedLimit {
    pub fn custom_limit(&self) -> Option<CustomLimit> {
        Some(CustomLimit {
            app: self.app.app_key()?,
            limit: match self.value {
                LimitValue::CpuPercent(percent) => AppLimit::Cpu(Percent::new(percent)),
                LimitValue::MemoryBytes(bytes) => AppLimit::Memory(Bytes::new(bytes)),
            },
            minutes: self.minutes,
        })
    }
}

/// What a quiet rule silences: one app, or one machine-wide kind by its stable code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuietTarget {
    App(SavedApp),
    Kind(String),
}

impl QuietTarget {
    pub fn kind(kind: AlertKind) -> Self {
        Self::Kind(kind.code().to_owned())
    }

    pub fn scope(&self) -> Option<QuietScope> {
        match self {
            Self::App(app) => app.app_key().map(QuietScope::App),
            Self::Kind(code) => AlertKind::from_code(code).map(QuietScope::Kind),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedQuiet {
    pub target: QuietTarget,
    /// `None` quiets for good.
    pub until_unix: Option<i64>,
}

impl SavedQuiet {
    pub fn until(&self) -> QuietUntil {
        self.until_unix
            .map_or(QuietUntil::Forever, |unix_seconds| QuietUntil::Time {
                unix_seconds,
            })
    }

    pub fn is_active(&self, now_unix: i64) -> bool {
        self.until_unix.is_none_or(|until| now_unix < until)
    }
}

impl InsightSettings {
    pub fn engine_limits(&self) -> Vec<CustomLimit> {
        self.custom_limits
            .iter()
            .filter_map(SavedLimit::custom_limit)
            .collect()
    }

    /// Replaces any rule for the same target.
    pub fn quiet(&mut self, target: QuietTarget, until_unix: Option<i64>) {
        self.unquiet(&target);
        self.quieted.push(SavedQuiet { target, until_unix });
    }

    pub fn unquiet(&mut self, target: &QuietTarget) {
        self.quieted.retain(|rule| &rule.target != target);
    }

    pub fn active_quiets(&self, now_unix: i64) -> impl Iterator<Item = &SavedQuiet> {
        self.quieted
            .iter()
            .filter(move |rule| rule.is_active(now_unix))
    }
}

#[cfg(test)]
mod tests {
    use monitor_core::AppKind;

    use super::*;

    fn firefox() -> SavedApp {
        SavedApp::new(&AppKey::new(AppKind::Desktop, "firefox"), "Firefox")
    }

    #[test]
    fn saved_limits_become_engine_limits() {
        let settings = InsightSettings {
            custom_limits: vec![
                SavedLimit {
                    app: firefox(),
                    value: LimitValue::MemoryBytes(4 << 30),
                    minutes: 5,
                },
                SavedLimit {
                    app: SavedApp {
                        key: "not a key".to_owned(),
                        name: "Broken".to_owned(),
                    },
                    value: LimitValue::CpuPercent(50.0),
                    minutes: 5,
                },
            ],
            ..InsightSettings::default()
        };
        let limits = settings.engine_limits();
        assert_eq!(limits.len(), 1, "unparsable keys are skipped");
        assert_eq!(limits[0].limit, AppLimit::Memory(Bytes::new(4 << 30)));
        assert_eq!(limits[0].app, AppKey::new(AppKind::Desktop, "firefox"));
    }

    #[test]
    fn quieting_replaces_and_expires() {
        let mut settings = InsightSettings::default();
        let target = QuietTarget::App(firefox());
        settings.quiet(target.clone(), Some(100));
        settings.quiet(target.clone(), None);
        settings.quiet(QuietTarget::kind(AlertKind::MachineHot), Some(100));
        assert_eq!(settings.quieted.len(), 2);
        assert_eq!(settings.active_quiets(200).count(), 1);
        assert_eq!(settings.quieted[0].until(), QuietUntil::Forever);
        assert_eq!(
            QuietTarget::kind(AlertKind::MachineHot).scope(),
            Some(QuietScope::Kind(AlertKind::MachineHot))
        );
        settings.unquiet(&target);
        assert_eq!(settings.active_quiets(0).count(), 1);
    }
}
