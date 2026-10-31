//! Notes as signals, for what `poly` can't do on its own: `drumhit` tells drum voices when
//! another hit chokes them, and `hitcount` counts a lane's hits, so each gets a seed of its
//! own. Port order must match `std/m2pkg.toml`. (std's one note at a time, `mono()`, is the
//! `poly` op's `"legato"` mode since std 0.3.0: `std:notecv` and `std:mono` are gone.)

use m2_dsp::kit::{Kind, kind_of};
use m2_kernel::{Cfg, Io, Kernel, export};

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

    fn new(_: &Cfg) -> Self {
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

    fn new(_: &Cfg) -> Self {
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
