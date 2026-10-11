//! L0 — musical time.
//!
//! Positions are integer ticks at [`PPQ`] ticks per quarter note. 960 is divisible by
//! 2, 3, 4, 5, 6 and 8, so straight, triplet and quintuplet grids are all exact.
//! Commands speak bars (1-based) and beats; conversion happens at the boundary.

use serde::{Deserialize, Serialize};

/// Ticks per quarter note (one beat in x/4 time).
pub const PPQ: i64 = 960;

pub type Tick = i64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeSig {
    pub num: u8,
    pub den: u8,
}

impl Default for TimeSig {
    fn default() -> Self {
        Self { num: 4, den: 4 }
    }
}

impl TimeSig {
    /// Ticks in one beat of this signature (a quarter in x/4, an eighth in x/8).
    pub fn ticks_per_beat(self) -> Tick {
        PPQ * 4 / self.den.max(1) as i64
    }

    pub fn ticks_per_bar(self) -> Tick {
        self.ticks_per_beat() * self.num.max(1) as i64
    }
}

/// Constant-tempo conversion between ticks, seconds and samples.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tempo {
    pub bpm: f64,
    pub sig: TimeSig,
}

impl Tempo {
    pub fn new(bpm: f64, sig: TimeSig) -> Self {
        Self { bpm: bpm.clamp(20.0, 400.0), sig }
    }

    pub fn seconds_per_tick(self) -> f64 {
        60.0 / (self.bpm * PPQ as f64)
    }

    pub fn tick_to_seconds(self, t: Tick) -> f64 {
        t as f64 * self.seconds_per_tick()
    }

    pub fn seconds_to_tick(self, s: f64) -> Tick {
        (s / self.seconds_per_tick()).round() as Tick
    }

    pub fn tick_to_sample(self, t: Tick, sr: f64) -> i64 {
        (self.tick_to_seconds(t) * sr).round() as i64
    }

    /// Fractional tick at a sample position (used by the renderer to schedule events).
    pub fn sample_to_tick_f(self, s: i64, sr: f64) -> f64 {
        s as f64 / sr / self.seconds_per_tick()
    }

    pub fn sample_to_tick(self, s: i64, sr: f64) -> Tick {
        self.sample_to_tick_f(s, sr).floor() as Tick
    }

    /// 1-based bar number → tick at the start of that bar.
    pub fn bar_to_tick(self, bar: f64) -> Tick {
        ((bar - 1.0) * self.sig.ticks_per_bar() as f64).round() as Tick
    }

    pub fn beats_to_ticks(self, beats: f64) -> Tick {
        (beats * self.sig.ticks_per_beat() as f64).round() as Tick
    }

    pub fn ticks_to_beats(self, t: Tick) -> f64 {
        t as f64 / self.sig.ticks_per_beat() as f64
    }

    pub fn ticks_to_bars(self, t: Tick) -> f64 {
        t as f64 / self.sig.ticks_per_bar() as f64
    }

    /// "bar.beat" display, both 1-based, e.g. `3.2`.
    pub fn format_position(self, t: Tick) -> String {
        let bar = t.div_euclid(self.sig.ticks_per_bar()) + 1;
        let beat = t.rem_euclid(self.sig.ticks_per_bar()) / self.sig.ticks_per_beat() + 1;
        format!("{bar}.{beat}")
    }
}

/// Snap `t` to the nearest multiple of `grid` (no-op for grid <= 0).
pub fn snap(t: Tick, grid: Tick) -> Tick {
    if grid <= 0 {
        return t;
    }
    ((t as f64 / grid as f64).round() as Tick) * grid
}

pub fn snap_floor(t: Tick, grid: Tick) -> Tick {
    if grid <= 0 {
        return t;
    }
    t.div_euclid(grid) * grid
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let t = Tempo::new(120.0, TimeSig::default());
        assert_eq!(t.bar_to_tick(1.0), 0);
        assert_eq!(t.bar_to_tick(2.0), 4 * PPQ);
        assert_eq!(t.tick_to_seconds(PPQ), 0.5);
        assert_eq!(t.tick_to_sample(PPQ, 48_000.0), 24_000);
        assert_eq!(t.sample_to_tick(24_000, 48_000.0), PPQ);
        assert_eq!(t.format_position(4 * PPQ + PPQ), "2.2");
        assert_eq!(snap(1000, 240), 960);
    }
}
