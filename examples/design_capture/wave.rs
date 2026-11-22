//! Deterministic wandering readings: two sines and a little xorshift noise.

pub struct Wave {
    pub tick: u64,
    state: u64,
}

impl Default for Wave {
    fn default() -> Self {
        Self {
            tick: 0,
            state: 0x9e37_79b9_7f4a_7c15,
        }
    }
}

impl Wave {
    pub fn noise(&mut self) -> f32 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        (self.state >> 40) as f32 / (1_u64 << 24) as f32
    }

    pub fn at(&mut self, base: f32, amplitude: f32, period: f32) -> f32 {
        let phase = self.tick as f32 / period;
        let swing = phase.sin() * 0.6 + (phase * 2.7).sin() * 0.25;
        (base + amplitude * swing + amplitude * 0.3 * (self.noise() - 0.5)).max(0.0)
    }
}
