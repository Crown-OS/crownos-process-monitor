use crownui::kit::{Theme, badge, use_theme};
use crownui::prelude::{Color, Signal, View, bind, component, keyed, vstack};
use monitor_core::Coverage;
use monitor_engine::Snapshot;
use monitor_engine::helper::Capability;

use crate::app::live::LiveSnapshot;

/// Why a reading is partial, shown as a small badge next to it, or that the privileged helper
/// made it complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverageMark {
    TcpOnly,
    OwnProcesses,
    Estimated,
    /// The helper read what this user cannot: every process, every protocol.
    Full,
}

impl CoverageMark {
    pub const fn label(self) -> &'static str {
        match self {
            Self::TcpOnly => "TCP only",
            Self::OwnProcesses => "Your processes only",
            Self::Estimated => "Estimated",
            Self::Full => "All processes",
        }
    }

    fn color(self, theme: &Theme) -> Color {
        match self {
            Self::TcpOnly | Self::OwnProcesses => theme.status.warning,
            Self::Estimated => theme.status.neutral,
            Self::Full => theme.status.success,
        }
    }

    /// The badge for `coverage`: `partial` while incomplete, [`Self::Full`] once complete
    /// thanks to the helper, nothing when complete anyway or not measured.
    pub const fn for_coverage(coverage: Coverage, partial: Self, upgraded: bool) -> Option<Self> {
        match coverage {
            Coverage::Full if upgraded => Some(Self::Full),
            Coverage::Full | Coverage::Unavailable => None,
            Coverage::OwnProcesses | Coverage::TcpOnly | Coverage::Estimated => Some(partial),
        }
    }
}

pub fn coverage_badge(mark: CoverageMark) -> impl View {
    component(move |cx| {
        let theme = use_theme(cx);
        badge(
            mark.label(),
            bind(move |runtime| theme.with(runtime, |theme| mark.color(theme))),
        )
    })
}

/// What a live coverage badge reads from the tick.
#[derive(Clone, Copy)]
pub struct LiveCoverage {
    pub coverage: fn(&Snapshot) -> Coverage,
    pub partial: CoverageMark,
    /// The helper read that fills the gap.
    pub capability: Capability,
}

/// The coverage badge of the latest tick, rebuilt only when its mark changes.
pub fn live_coverage_badge(live: Signal<LiveSnapshot>, source: LiveCoverage) -> impl View {
    keyed(
        move |runtime| {
            live.with(runtime, |snapshot| {
                let tick = &snapshot.tick;
                CoverageMark::for_coverage(
                    (source.coverage)(tick),
                    source.partial,
                    tick.helper.serves(source.capability),
                )
            })
        },
        |_, mark| match mark {
            Some(mark) => coverage_badge(mark).boxed(),
            None => vstack(()).boxed(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_helper_turns_partial_badges_into_full() {
        let own = CoverageMark::OwnProcesses;
        assert_eq!(
            CoverageMark::for_coverage(Coverage::OwnProcesses, own, false),
            Some(own)
        );
        assert_eq!(CoverageMark::for_coverage(Coverage::Full, own, false), None);
        assert_eq!(
            CoverageMark::for_coverage(Coverage::Full, own, true),
            Some(CoverageMark::Full)
        );
        assert_eq!(
            CoverageMark::for_coverage(Coverage::Unavailable, CoverageMark::TcpOnly, false),
            None
        );
    }
}
