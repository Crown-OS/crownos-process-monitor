use std::time::Duration;

use crate::{AppKey, DeviceKey, ProcessKey};

const VISIBLE_INTERVAL: Duration = Duration::from_secs(1);
const MINIMIZED_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Page {
    #[default]
    Overview,
    Cpu,
    Gpu,
    Memory,
    Disk,
    Network,
}

impl Page {
    pub const ALL: [Self; 6] = [
        Self::Overview,
        Self::Cpu,
        Self::Gpu,
        Self::Memory,
        Self::Disk,
        Self::Network,
    ];

    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Cpu => "CPU",
            Self::Gpu => "GPU",
            Self::Memory => "Memory",
            Self::Disk => "Disk",
            Self::Network => "Network",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WindowState {
    #[default]
    Visible,
    Minimized,
}

/// What the user is looking at. Collectors use it to skip expensive work
/// nobody would see.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Interest {
    pub page: Page,
    pub window: WindowState,
    pub inspected: Option<ProcessKey>,
    pub expanded_app: Option<AppKey>,
    pub selected_gpu: Option<DeviceKey>,
    /// User-chosen interval while visible; `None` keeps the default.
    pub visible_interval: Option<Duration>,
}

impl Interest {
    #[must_use]
    pub fn is_visible(&self) -> bool {
        self.window == WindowState::Visible
    }

    /// True when `page` is on screen right now.
    #[must_use]
    pub fn shows(&self, page: Page) -> bool {
        self.is_visible() && self.page == page
    }

    #[must_use]
    pub fn sample_interval(&self) -> Duration {
        match self.window {
            WindowState::Visible => self.visible_interval.unwrap_or(VISIBLE_INTERVAL),
            WindowState::Minimized => MINIMIZED_INTERVAL,
        }
    }
}
