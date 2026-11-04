/// GPU engine families as history stores them, one row per engine per bucket.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GpuEngine {
    Graphics,
    Compute,
    Copy,
    /// Video decode, or a codec engine that does both directions.
    Video,
    VideoEncode,
    VideoEnhance,
    Other,
}

impl GpuEngine {
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

    /// The `engine` column value.
    pub const fn stored_name(self) -> &'static str {
        match self {
            Self::Graphics => "graphics",
            Self::Compute => "compute",
            Self::Copy => "copy",
            Self::Video => "video",
            Self::VideoEncode => "video_encode",
            Self::VideoEnhance => "video_enhance",
            Self::Other => "other",
        }
    }

    pub fn from_stored_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|engine| engine.stored_name() == name)
    }

    const fn index(self) -> usize {
        self as usize
    }
}

/// Busy seconds of each engine during one tick; `None` for engines the GPU
/// does not have or did not report.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EngineBusy([Option<f64>; GpuEngine::COUNT]);

impl EngineBusy {
    pub fn set(&mut self, engine: GpuEngine, busy_seconds: f64) {
        self.0[engine.index()] = Some(busy_seconds);
    }

    pub fn get(&self, engine: GpuEngine) -> Option<f64> {
        self.0[engine.index()]
    }
}

/// One engine's recorded and busy seconds in a bucket.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EngineTime {
    pub seconds: f64,
    pub busy_seconds: f64,
}

/// [`EngineTime`] per engine, summed over a minute; only reported engines have one.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EngineTimes([Option<EngineTime>; GpuEngine::COUNT]);

impl EngineTimes {
    pub fn add(&mut self, busy: &EngineBusy, seconds: f64) {
        for (time, busy) in self.0.iter_mut().zip(busy.0) {
            if let Some(busy_seconds) = busy {
                let time = time.get_or_insert_default();
                time.seconds += seconds;
                time.busy_seconds += busy_seconds;
            }
        }
    }

    pub fn get(&self, engine: GpuEngine) -> Option<EngineTime> {
        self.0[engine.index()]
    }

    pub fn iter(&self) -> impl Iterator<Item = (GpuEngine, EngineTime)> + '_ {
        GpuEngine::ALL
            .into_iter()
            .zip(self.0)
            .filter_map(|(engine, time)| Some((engine, time?)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_and_only_reported_engines_accumulate() {
        for engine in GpuEngine::ALL {
            assert_eq!(
                GpuEngine::from_stored_name(engine.stored_name()),
                Some(engine)
            );
        }
        let mut busy = EngineBusy::default();
        busy.set(GpuEngine::Graphics, 0.25);
        let mut times = EngineTimes::default();
        times.add(&busy, 1.0);
        times.add(&busy, 1.0);
        assert_eq!(
            times.iter().collect::<Vec<_>>(),
            [(
                GpuEngine::Graphics,
                EngineTime {
                    seconds: 2.0,
                    busy_seconds: 0.5
                }
            )]
        );
        assert_eq!(times.get(GpuEngine::Compute), None);
    }
}
