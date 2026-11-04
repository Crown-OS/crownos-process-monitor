use crownui::kit::icons;
use crownui::prelude::{Cx, Signal};

/// The six sidebar pages, in sidebar order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
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

    pub const fn icon(self) -> &'static str {
        match self {
            Self::Overview => icons::LAYOUT_DASHBOARD,
            Self::Cpu => icons::CPU,
            Self::Gpu => icons::GPU,
            Self::Memory => icons::MEMORY_STICK,
            Self::Disk => icons::HARD_DRIVE,
            Self::Network => icons::NETWORK,
        }
    }

    pub const fn index(self) -> usize {
        self as usize
    }

    pub fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or(Self::Overview)
    }
}

/// Which page the content card shows.
#[derive(Debug, Clone, Copy)]
pub struct Navigation {
    page: Signal<usize>,
}

impl Navigation {
    pub fn new(cx: &mut Cx, page: Page) -> Self {
        Self {
            page: cx.signal(page.index()),
        }
    }

    /// The index of the page on screen, as `slide_transition` takes it.
    pub fn current(self) -> Signal<usize> {
        self.page
    }

    pub fn show(self, cx: &mut Cx, page: Page) {
        if self.page.with_untracked(cx, |current| *current) != page.index() {
            self.page.set(cx, page.index());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_round_trip_through_their_index() {
        for page in Page::ALL {
            assert_eq!(Page::from_index(page.index()), page);
        }
        assert_eq!(Page::from_index(42), Page::Overview);
    }
}
