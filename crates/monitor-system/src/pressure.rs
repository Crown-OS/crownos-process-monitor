use monitor_core::Percent;

/// Share of wall time some (or all) tasks were stalled, averaged over 10 s,
/// 60 s and 300 s, from `/proc/pressure/*`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PressureAverages {
    pub avg10: Percent,
    pub avg60: Percent,
    pub avg300: Percent,
}
