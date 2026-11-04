use std::ops::{Add, AddAssign};

const KIB: u64 = 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Bytes(u64);

impl Bytes {
    pub const ZERO: Self = Self(0);

    #[must_use]
    pub const fn new(bytes: u64) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn from_kib(kib: u64) -> Self {
        Self(kib.saturating_mul(KIB))
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    #[must_use]
    pub const fn saturating_sub(self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }

    /// Share of `total`, or zero when `total` is empty.
    #[must_use]
    pub fn ratio_of(self, total: Self) -> f64 {
        if total.0 == 0 {
            0.0
        } else {
            self.0 as f64 / total.0 as f64
        }
    }
}

impl Add for Bytes {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }
}

impl AddAssign for Bytes {
    fn add_assign(&mut self, other: Self) {
        self.0 = self.0.saturating_add(other.0);
    }
}

impl std::iter::Sum for Bytes {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::ZERO, Add::add)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct BytesPerSecond(f64);

impl BytesPerSecond {
    pub const ZERO: Self = Self(0.0);

    #[must_use]
    pub const fn new(rate: f64) -> Self {
        Self(rate)
    }

    /// Rate between two counter readings; a counter that went backwards was
    /// reset and yields zero instead of a spike.
    #[must_use]
    pub fn between(earlier: u64, later: u64, seconds: f64) -> Self {
        if seconds <= 0.0 || later < earlier {
            Self::ZERO
        } else {
            Self((later - earlier) as f64 / seconds)
        }
    }

    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }
}

impl Add for BytesPerSecond {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }
}

impl AddAssign for BytesPerSecond {
    fn add_assign(&mut self, other: Self) {
        self.0 += other.0;
    }
}

/// A percentage in `0.0..=100.0` of whatever the owner documents
/// (one core, the whole machine, a device).
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct Percent(f32);

impl Percent {
    pub const ZERO: Self = Self(0.0);
    pub const FULL: Self = Self(100.0);

    #[must_use]
    pub fn new(percent: f32) -> Self {
        Self(if percent.is_finite() {
            percent.max(0.0)
        } else {
            0.0
        })
    }

    #[must_use]
    pub fn from_ratio(ratio: f64) -> Self {
        Self::new((ratio * 100.0) as f32)
    }

    /// Like [`Percent::new`] but capped at 100.
    #[must_use]
    pub fn clamped(percent: f32) -> Self {
        Self(Self::new(percent).0.min(100.0))
    }

    /// Like [`Percent::from_ratio`] but capped at 100.
    #[must_use]
    pub fn clamped_ratio(ratio: f64) -> Self {
        Self::clamped((ratio * 100.0) as f32)
    }

    /// The share as a ratio, capped at 1.
    #[must_use]
    pub fn as_ratio(self) -> f64 {
        f64::from(self.0.min(100.0)) / 100.0
    }

    #[must_use]
    pub const fn get(self) -> f32 {
        self.0
    }
}

impl Add for Percent {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }
}

impl AddAssign for Percent {
    fn add_assign(&mut self, other: Self) {
        self.0 += other.0;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct Celsius(f32);

impl Celsius {
    #[must_use]
    pub const fn new(celsius: f32) -> Self {
        Self(celsius)
    }

    /// hwmon reports millidegrees.
    #[must_use]
    pub fn from_millidegrees(milli: i64) -> Self {
        Self(milli as f32 / 1000.0)
    }

    #[must_use]
    pub const fn get(self) -> f32 {
        self.0
    }

    #[must_use]
    pub fn fahrenheit(self) -> f32 {
        self.0 * 9.0 / 5.0 + 32.0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct Watts(f32);

impl Watts {
    #[must_use]
    pub const fn new(watts: f32) -> Self {
        Self(watts)
    }

    #[must_use]
    pub fn from_microwatts(micro: u64) -> Self {
        Self(micro as f32 / 1_000_000.0)
    }

    #[must_use]
    pub fn from_milliwatts(milli: u64) -> Self {
        Self(milli as f32 / 1000.0)
    }

    #[must_use]
    pub const fn get(self) -> f32 {
        self.0
    }
}

impl Add for Watts {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Rpm(u32);

impl Rpm {
    #[must_use]
    pub const fn new(rpm: u32) -> Self {
        Self(rpm)
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Megahertz(u32);

impl Megahertz {
    #[must_use]
    pub const fn new(mhz: u32) -> Self {
        Self(mhz)
    }

    #[must_use]
    pub fn from_khz(khz: u64) -> Self {
        Self(u32::try_from(khz / 1000).unwrap_or(u32::MAX))
    }

    #[must_use]
    pub fn from_hz(hz: u64) -> Self {
        Self(u32::try_from(hz / 1_000_000).unwrap_or(u32::MAX))
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// How completely a figure covers the system, so the UI can badge partial data.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Coverage {
    #[default]
    Full,
    /// Only processes owned by the current user could be measured.
    OwnProcesses,
    /// Only TCP traffic is attributed (no UDP/QUIC) without the helper.
    TcpOnly,
    /// Derived from other figures rather than measured.
    Estimated,
    Unavailable,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_ignores_counter_reset() {
        assert_eq!(BytesPerSecond::between(100, 50, 1.0), BytesPerSecond::ZERO);
        assert_eq!(BytesPerSecond::between(100, 300, 2.0).get(), 100.0);
        assert_eq!(BytesPerSecond::between(0, 10, 0.0), BytesPerSecond::ZERO);
    }

    #[test]
    fn percent_rejects_nan_and_negative() {
        assert_eq!(Percent::new(f32::NAN).get(), 0.0);
        assert_eq!(Percent::new(-3.0).get(), 0.0);
        assert_eq!(Percent::clamped(140.0).get(), 100.0);
    }

    #[test]
    fn byte_sums_saturate() {
        assert_eq!((Bytes::new(u64::MAX) + Bytes::new(1)).get(), u64::MAX);
        assert_eq!(Bytes::from_kib(2).get(), 2048);
    }
}
