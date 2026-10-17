//! The metronome (gaps #19): a click on each beat of the meter's unit (quarters in 4/4, eighths
//! in 6/8), a higher, louder one on each bar's first. The live engine mixes it into each block after
//! its plan renders it, before a hold's fade ([`crate::Engine::set_click`]).
//!
//! Only the live engine has one, and only `m2_play` makes an engine: offline renders and exports
//! never do, so a click never reaches an export (RENDER-1, RENDER-3). Meters read taps inside the
//! plan, so they never see it either. Plain fields: no allocation, no lock (RENDER-28). Off and not
//! ringing, it touches no sample, so the engine's output keeps every bit.

use std::f64::consts::TAU;

use crate::transport::frame_of;
use crate::{BLOCK, Renderer, Runs};

/// How long a blip lasts, in seconds.
const LEN: f64 = 0.030;
/// Its decay's time constant: e^-5 by its end, where a linear fade also takes it to 0.
const DECAY: f64 = 0.006;
/// A bar's first beat: 1,500 Hz at −12 dBFS.
const ACCENT: (f64, f64) = (1500.0, 0.251_188_643_150_958);
/// Every other beat: 1,000 Hz at −16 dBFS.
const BEAT: (f64, f64) = (1000.0, 0.158_489_319_246_111);
/// Blips one block can start: far more than the fastest grid makes (999 bpm in 4/32).
const STARTS: usize = 8;
/// How far a grid point may sit outside a run's beats and still be looked at: its frame decides.
const SLACK: f64 = 1e-6;

/// The metronome's state on the audio thread.
#[derive(Clone, Copy, Debug, Default)]
pub struct Click {
    on: bool,
    /// Frames into the blip that sounds, or none.
    ringing: Option<u32>,
    /// Its pitch, in radians per frame, and its peak.
    step: f64,
    gain: f64,
    /// A count-in (lane B): beats left to click, the frames between them, frames into the count
    /// (its first block's lead-in included), where its first click is, and frames in all (a
    /// whole number of blocks, so the bar ends on a block line). `count_end` is set for the block
    /// the bar ends with. Plain fields, as the rest (RENDER-28).
    count_beats: u32,
    count_step: f64,
    count_done: u64,
    count_pad: u64,
    count_total: u64,
    count_end: bool,
}

impl Click {
    /// Clicks on each beat from the next block on (true), or no new ones (false): a blip that
    /// sounds rings out.
    pub fn set(&mut self, on: bool) {
        self.on = on;
    }

    pub fn is_on(&self) -> bool {
        self.on
    }

    /// Count in one bar (lane B): `beats` clicks `frames_per_beat` apart, the first accented,
    /// whatever the transport does, from this block on. Click k comes `round(k × frames_per_beat)`
    /// frames after the first, and the bar is `round(beats × frames_per_beat)` frames, laid so it
    /// ends on a block line: the first click comes `ceil(bar / BLOCK) × BLOCK − bar` frames into
    /// this block. A count already running starts again.
    pub fn count_in(&mut self, beats: u32, frames_per_beat: f64) {
        let bar = (beats as f64 * frames_per_beat).round().max(1.0) as u64;
        let total = bar.div_ceil(BLOCK as u64) * BLOCK as u64;
        (self.count_beats, self.count_step, self.count_done) = (beats, frames_per_beat.max(0.0), 0);
        (self.count_pad, self.count_total, self.count_end) = (total - bar, total, false);
    }

    /// Whether a count-in runs: its clicks sound, and the engine never idles.
    pub fn counting(&self) -> bool {
        self.count_total > 0
    }

    /// No count-in any more: a Play, a Stop, a seek or a hold came first. A blip that sounds rings
    /// out.
    pub fn cancel_count(&mut self) {
        (self.count_total, self.count_end) = (0, false);
    }

    /// True once, after the block the count's bar ends with: the song plays from the next block.
    pub fn count_ended(&mut self) -> bool {
        std::mem::take(&mut self.count_end)
    }

    /// Mix this block's clicks into `frames`: a blip at each grid point whose frame is in a
    /// moving run, `[at, at + n)`, so a run that starts on a beat clicks it and one that ends on
    /// it doesn't. Stopped, nothing starts. Never allocates.
    pub fn mix(&mut self, frames: &mut [[f32; 2]; BLOCK], runs: &Runs, r: &Renderer) {
        if !self.on && self.ringing.is_none() && !self.counting() {
            return;
        }
        let rate = r.sample_rate() as f64;
        let mut starts = [(0u32, false); STARTS];
        let mut n = 0;
        // A count-in's blips come from its own frames: the transport is stopped meanwhile.
        if self.counting() {
            let (from, to) = (self.count_done, self.count_done + BLOCK as u64);
            for k in 0..self.count_beats {
                let at = self.count_pad + (k as f64 * self.count_step).round() as u64;
                if at >= to {
                    break;
                }
                if at >= from && n < STARTS {
                    starts[n] = ((at - from) as u32, k == 0);
                    n += 1;
                }
            }
            self.count_done = to;
            if to >= self.count_total {
                (self.count_total, self.count_end) = (0, true);
            }
        }
        if self.on {
            for run in runs.as_slice().iter().filter(|run| run.moving && run.n > 0) {
                grid(r, rate, run.at, run.at + run.n as u64, &mut |frame, accent| {
                    if n < STARTS {
                        starts[n] = (run.off + (frame - run.at) as u32, accent);
                        n += 1;
                    }
                });
            }
        }
        let len = (LEN * rate).round().max(1.0) as u32;
        let mut next = 0;
        for (f, out) in frames.iter_mut().enumerate() {
            while next < n && starts[next].0 as usize == f {
                let (hz, gain) = if starts[next].1 { ACCENT } else { BEAT };
                (self.ringing, self.step, self.gain) = (Some(0), TAU * hz / rate, gain);
                next += 1;
            }
            let Some(i) = self.ringing else { continue };
            // libm's, as all host math is (PLAT-4), though a click never reaches an export.
            let env = libm::exp(-(i as f64) / (DECAY * rate)) * (1.0 - i as f64 / len as f64);
            let s = (self.gain * env * libm::sin(self.step * i as f64)) as f32;
            *out = [out[0] + s, out[1] + s];
            self.ringing = (i + 1 < len).then_some(i + 1);
        }
    }
}

/// Each grid point of `r`'s meter whose frame is in `[a, b)`, in order: its frame, and whether it
/// starts a bar. A meter's grid is every `4 / unit` quarter notes from its start, and a meter
/// change falls on a bar line of the meter before it, which is one of its grid points, so walking
/// from point to point by the step of the meter at each meets every meter's grid.
fn grid(r: &Renderer, rate: f64, a: u64, b: u64, hit: &mut dyn FnMut(u64, bool)) {
    let (tempo, arr) = (r.tempo(), r.arrangement());
    let lo = tempo.beat_at(a as f64 / rate) - SLACK;
    let hi = tempo.beat_at(b as f64 / rate) + SLACK;
    let m = arr.meter_at(lo.max(0.0));
    let step = 4.0 / m.unit as f64;
    let mut g = m.beat + ((lo - m.beat) / step).ceil() * step;
    while g <= hi {
        let frame = frame_of(tempo, g, rate);
        if (a..b).contains(&frame) {
            hit(frame, arr.bar_at(g).1.abs() < 1e-9);
        }
        g += 4.0 / arr.meter_at(g).unit as f64;
    }
}

#[cfg(test)]
mod tests;
