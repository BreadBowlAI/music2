//! Notes as signals, for what `poly` can't do on its own yet: `notecv` and `mono` play one
//! note at a time with legato glide (std's `mono()`), `drumhit` tells drum voices when
//! another hit chokes them, and `hitcount` counts a lane's hits, so each gets a seed of its
//! own. Port order must match `std/m2pkg.toml`.
//!
//! A note's start travels as a code on one sample, and its end as the code negated: an
//! integer below 2^21, so it is exact in f32, and so are the sums a `poly` makes of eight of
//! them and of its 0/1 gates.

use m2_kernel::{Io, Kernel, export};

use crate::drums::{Kind, kind_of};
use crate::voice::{clampf, pitch_of, sane};

/// Code steps per semitone, and the code's velocity field (above 128 × 64 pitch steps).
const PITCH_STEPS: f32 = 64.0;
const VELOCITY_FIELD: f32 = 16_384.0;

/// The code for a note: 1 + pitch in 64ths of a semitone (0..127.98) + 16384 × velocity in
/// 127ths. Never 0.
pub(crate) fn note_code(pitch: f32, velocity: f32) -> f32 {
    let p = (clampf(pitch_of(pitch), 0.0, 127.98) * PITCH_STEPS).round();
    let v = (clampf(velocity, 0.0, 1.0) * 127.0).round();
    1.0 + p + VELOCITY_FIELD * v
}

/// The pitch and velocity in a code, if `x` is one: a positive integer in range.
pub(crate) fn decode(x: f32) -> Option<(f32, f32)> {
    if !((1.0..128.0 * VELOCITY_FIELD).contains(&x) && x == x.floor()) {
        return None;
    }
    let c = x - 1.0;
    let v = (c / VELOCITY_FIELD).floor();
    let p = c - v * VELOCITY_FIELD;
    (p < 128.0 * PITCH_STEPS).then_some((p / PITCH_STEPS, v / 127.0))
}

/// A note as audio. Inputs: pitch, gate, velocity. Output: the gate (exactly 0 or 1) on the
/// left; on the right, the note's code on its first sample, the code negated on the sample
/// its gate falls, else 0.
pub struct NoteCv {
    first: bool,
    on: bool,
    code: f32,
}

impl Kernel for NoteCv {
    const INPUTS: usize = 3;
    const OUTPUTS: usize = 1;

    fn new(_: f32, _: usize) -> Self {
        NoteCv { first: true, on: false, code: 0.0 }
    }

    fn process(&mut self, io: &mut Io) {
        let ([pitch, gate, velocity], [l, r]) = io.ports([(0, 0), (1, 0), (2, 0)], [(0, 0), (0, 1)]);
        for i in 0..l.len() {
            let on = gate[i] > 0.5;
            l[i] = if on { 1.0 } else { 0.0 };
            r[i] = if std::mem::take(&mut self.first) {
                self.code = note_code(pitch[i], velocity[i]);
                self.code
            } else if self.on && !on {
                -self.code
            } else {
                0.0
            };
            self.on = on;
        }
    }
}

export!(notecv, NoteCv);

/// Notes `mono` remembers under the one sounding: more than any line holds down at once.
const HELD: usize = 8;

/// One note at a time, decoded from two `notecv` polys: `last` (one voice: the latest note)
/// and `held` (eight voices: its left side counts the notes held, its right side sums their
/// starts and ends). Inputs: last, held, glide (seconds). Outputs: gate, pitch, velocity.
///
/// The latest note wins. One that starts while the gate is on and another note is still
/// held is legato: the pitch slides to it over `glide` and the gate stays on. Any other note
/// jumps, and if the gate was on it drops for one sample, so envelopes start again from
/// where they are. Two notes that only touch never slide: at the sample one ends and the
/// next starts, `held` counts one. When the sounding note ends while others are still held,
/// the newest of them sounds again, sliding back to it as legato (the classic monosynth's
/// note memory). Notes that start on the same sample count as one, the last of them.
pub struct Mono {
    sr: f32,
    gate: bool,
    pitch: f32,
    velocity: f32,
    target: f32,
    step: f32,
    /// Samples of glide left.
    left: u32,
    /// The codes of the notes held, oldest first: the last one is sounding.
    held: [f32; HELD],
    count: usize,
}

impl Mono {
    /// The code of the note sounding, if any.
    fn top(&self) -> Option<f32> {
        self.count.checked_sub(1).map(|i| self.held[i])
    }

    fn push(&mut self, code: f32) {
        if self.count == HELD {
            self.drop_oldest();
        }
        self.held[self.count] = code;
        self.count += 1;
    }

    fn drop_oldest(&mut self) {
        self.held.copy_within(1..self.count, 0);
        self.count -= 1;
    }

    /// Forget the held notes whose codes add up to `sum` (the notes that just ended): the
    /// newest one alone if it matches, else the first set of them that does. Codes that
    /// match none (notes that started with others on one sample) are ignored.
    fn forget(&mut self, sum: f32) {
        let n = self.count;
        if let Some(i) = (0..n).rev().find(|&i| self.held[i] == sum) {
            self.remove(1 << i);
            return;
        }
        // Codes and their sums are integers below 2^24: exact.
        let members = |m: u32| (0..n).filter(move |i| (m >> i) & 1 == 1);
        if let Some(mask) = (1u32..1 << n).find(|&m| members(m).map(|i| self.held[i]).sum::<f32>() == sum) {
            self.remove(mask);
        }
    }

    fn remove(&mut self, mask: u32) {
        let mut kept = 0;
        for i in 0..self.count {
            if (mask >> i) & 1 == 0 {
                self.held[kept] = self.held[i];
                kept += 1;
            }
        }
        self.count = kept;
    }

    /// Head for `pitch`: over `glide` seconds, or at once.
    fn slide(&mut self, pitch: f32, glide: f32) {
        let n = (clampf(glide, 0.0, 10.0) * self.sr).round() as u32;
        self.target = pitch;
        self.left = n;
        if n == 0 {
            self.pitch = pitch;
        } else {
            self.step = (pitch - self.pitch) / n as f32;
        }
    }

    fn jump(&mut self, pitch: f32) {
        self.pitch = pitch;
        self.target = pitch;
        self.left = 0;
    }
}

/// A finite sample, or 0: codes are far beyond `sane`'s range, so they are checked this way.
fn finite(x: f32) -> f32 {
    if x.is_finite() { x } else { 0.0 }
}

impl Kernel for Mono {
    const INPUTS: usize = 3;
    const OUTPUTS: usize = 3;

    fn new(sample_rate: f32, _: usize) -> Self {
        Mono {
            sr: sample_rate.max(1.0),
            gate: false,
            pitch: 60.0,
            velocity: 0.0,
            target: 60.0,
            step: 0.0,
            left: 0,
            held: [0.0; HELD],
            count: 0,
        }
    }

    fn process(&mut self, io: &mut Io) {
        let ([code, held, changes, glide], [gate_out, pitch_out, velocity_out]) =
            io.ports([(0, 1), (1, 0), (1, 1), (2, 0)], [(0, 0), (1, 0), (2, 0)]);
        for i in 0..code.len() {
            let sounding = self.top();
            let start = decode(code[i]);
            // What `held` says ended: its sum of starts and ends, less the start `last` names.
            let ended = finite(changes[i]) - if start.is_some() { code[i] } else { 0.0 };
            if ended < 0.0 {
                self.forget(-ended);
            }
            if start.is_some() {
                self.push(code[i]);
            }
            // Never more notes remembered than are held (a note a full `held` stole never ends).
            let count = sane(held[i]).max(0.0).round() as usize;
            while self.count > count {
                self.drop_oldest();
            }
            let mut gate = self.count > 0;
            if let Some((p, v)) = start {
                if self.gate && count >= 2 {
                    self.slide(p, glide[i]);
                } else {
                    self.jump(p);
                    if self.gate {
                        gate = false;
                    }
                }
                self.velocity = v;
            } else if self.top() != sounding
                && let Some((p, v)) = self.top().and_then(decode)
            {
                // The note sounding ended and an older one is still held: back to it.
                self.slide(p, glide[i]);
                self.velocity = v;
            } else if self.left > 0 {
                self.left -= 1;
                self.pitch = if self.left == 0 { self.target } else { self.pitch + self.step };
            }
            self.gate = gate;
            gate_out[i] = if gate { 1.0 } else { 0.0 };
            pitch_out[i] = self.pitch;
            velocity_out[i] = self.velocity;
        }
    }
}

export!(mono, Mono);

/// Where `drumhit` marks every hit, above the five choke groups' fields: 16^5.
pub(crate) const HIT_FIELD: u32 = 1 << 20;

/// Marks a drum hit, on the note's first sample, else 0: 16^group for its drum's choke group
/// (by General MIDI pitch, or `model`), which the drum voices read, plus [`HIT_FIELD`] for
/// every hit, which `hitcount` counts. Each field has 4 bits, so up to 15 hits on one sample
/// can't carry into the next (std's polys of it have 8 voices at most), and every sum is an
/// integer below 2^24: exact in f32. Inputs: pitch, model. Output: out.
pub struct DrumHit {
    first: bool,
}

impl Kernel for DrumHit {
    const INPUTS: usize = 2;
    const OUTPUTS: usize = 1;

    fn new(_: f32, _: usize) -> Self {
        DrumHit { first: true }
    }

    fn process(&mut self, io: &mut Io) {
        let ([pitch, model], [out]) = io.ports([(0, 0), (1, 0)], [(0, 0)]);
        for i in 0..out.len() {
            out[i] = 0.0;
            if std::mem::take(&mut self.first) {
                let kind: Kind = kind_of(model[i], pitch[i]).0;
                out[i] = (HIT_FIELD + kind.group().map_or(0, |g| 16u32.pow(g))) as f32;
            }
        }
    }
}

export!(drumhit, DrumHit);

/// How many hits a lane has played: the hits `drumhit` marks (the sum of a poly of it),
/// counted from 0, this sample's included, wrapping at 2^24 so every value is exact. A drum
/// reads it on its first sample for its seed. It counts notes, never samples: an engine that
/// ran a while before playing (stopped, its tails decaying) gives every hit the seed it has in
/// the export, from transport 0. Input: hits. Output: out.
pub struct HitCount {
    n: u32,
}

/// Where the count wraps: the integers below it are exact in f32.
const COUNT_WRAP: u32 = 1 << 24;

/// The hits `drumhit` marked on one sample of `hits`: its hit field, if `hits` is a sum of
/// marks (an integer in 0..2^24), else none.
fn hits_in(hits: f32) -> u32 {
    if (0.0..COUNT_WRAP as f32).contains(&hits) && hits == hits.floor() { (hits as u32 / HIT_FIELD) & 15 } else { 0 }
}

impl Kernel for HitCount {
    const INPUTS: usize = 1;
    const OUTPUTS: usize = 1;

    fn new(_: f32, _: usize) -> Self {
        HitCount { n: 0 }
    }

    fn process(&mut self, io: &mut Io) {
        let ([hits], [out]) = io.ports([(0, 0)], [(0, 0)]);
        for i in 0..out.len() {
            self.n = (self.n + hits_in(hits[i])) % COUNT_WRAP;
            out[i] = self.n as f32;
        }
    }
}

export!(hitcount, HitCount);

#[cfg(test)]
mod tests;
