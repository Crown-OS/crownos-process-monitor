use monitor_core::Percent;

/// Engine families every driver's engine names are folded into.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EngineClass {
    Graphics,
    Compute,
    Copy,
    /// Video decode, or a codec engine that does both directions.
    Video,
    VideoEncode,
    VideoEnhance,
    Other,
}

impl EngineClass {
    pub const COUNT: usize = 7;
    pub const ALL: [Self; Self::COUNT] = [
        Self::Graphics,
        Self::Compute,
        Self::Copy,
        Self::Video,
        Self::VideoEncode,
        Self::VideoEnhance,
        Self::Other,
    ];

    pub const fn title(self) -> &'static str {
        match self {
            Self::Graphics => "3D",
            Self::Compute => "Compute",
            Self::Copy => "Copy",
            Self::Video => "Video",
            Self::VideoEncode => "Video Encode",
            Self::VideoEnhance => "Video Enhance",
            Self::Other => "Other",
        }
    }

    pub(crate) const fn index(self) -> usize {
        self as usize
    }

    /// Maps fdinfo engine names of amdgpu, i915, xe, msm, panfrost and v3d.
    pub fn from_driver_name(name: &[u8]) -> Self {
        match name {
            b"gfx" | b"render" | b"rcs" | b"gpu" | b"fragment" | b"vertex-tiler" | b"bin"
            | b"3d" => Self::Graphics,
            b"compute" | b"ccs" | b"csd" => Self::Compute,
            b"dma" | b"copy" | b"bcs" | b"blit" | b"tfu" | b"sdma" => Self::Copy,
            b"dec" | b"video" | b"vcs" | b"jpeg" | b"vcn" => Self::Video,
            b"enc" | b"enc_1" => Self::VideoEncode,
            b"video-enhance" | b"vecs" | b"vpe" => Self::VideoEnhance,
            _ => Self::Other,
        }
    }
}

/// Utilization per engine class; `None` when the device has no such engine
/// or the source does not report it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EngineUsage([Option<Percent>; EngineClass::COUNT]);

impl EngineUsage {
    pub fn get(&self, class: EngineClass) -> Option<Percent> {
        self.0[class.index()]
    }

    pub fn set(&mut self, class: EngineClass, value: Percent) {
        self.0[class.index()] = Some(value);
    }

    /// Adds `value` to the class, capping the sum at 100 %.
    pub fn accumulate(&mut self, class: EngineClass, value: Percent) {
        let sum = self.get(class).map_or(0.0, Percent::get) + value.get();
        self.0[class.index()] = Some(Percent::clamped(sum));
    }

    pub fn merge(&mut self, other: &Self) {
        for class in EngineClass::ALL {
            if let Some(value) = other.get(class) {
                self.accumulate(class, value);
            }
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (EngineClass, Percent)> + '_ {
        EngineClass::ALL
            .into_iter()
            .filter_map(|class| self.get(class).map(|value| (class, value)))
    }

    /// The busiest engine, which is what a single "GPU %" figure shows.
    pub fn busiest(&self) -> Option<Percent> {
        self.iter()
            .map(|(_, value)| value)
            .max_by(|left, right| left.get().total_cmp(&right.get()))
    }

    pub fn is_empty(&self) -> bool {
        self.0.iter().all(Option::is_none)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_driver_engine_names() {
        assert_eq!(EngineClass::from_driver_name(b"gfx"), EngineClass::Graphics);
        assert_eq!(
            EngineClass::from_driver_name(b"enc_1"),
            EngineClass::VideoEncode
        );
        assert_eq!(
            EngineClass::from_driver_name(b"video-enhance"),
            EngineClass::VideoEnhance
        );
        assert_eq!(EngineClass::from_driver_name(b"vcs"), EngineClass::Video);
        assert_eq!(EngineClass::from_driver_name(b"npu"), EngineClass::Other);
    }

    #[test]
    fn accumulates_and_caps() {
        let mut usage = EngineUsage::default();
        usage.accumulate(EngineClass::Graphics, Percent::new(70.0));
        usage.accumulate(EngineClass::Graphics, Percent::new(50.0));
        usage.set(EngineClass::Copy, Percent::new(5.0));
        assert_eq!(usage.get(EngineClass::Graphics), Some(Percent::FULL));
        assert_eq!(usage.busiest(), Some(Percent::FULL));
        assert_eq!(usage.iter().count(), 2);
    }
}
