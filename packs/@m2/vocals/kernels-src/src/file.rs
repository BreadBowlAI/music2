//! A file an offline kernel reads (its asset 0, kernel ABI v1), a window of it at a time: its
//! channels made sane (m2-dsp's house rule: what isn't finite is silence, absurd levels capped),
//! and their mono sum, for finding a pitch and pulses. Every buffer is sized at birth; the window moves forward as it is asked for later
//! frames, keeping a little of what came before. Frames outside the file are silence.

use m2_dsp::fx::sane;
use m2_kernel::Asset;

pub struct File {
    asset: Asset,
    /// Frames the window holds, and how many before a request it keeps when it moves.
    cap: usize,
    back: usize,
    /// The file's frame the window starts at, and how many of its frames it holds.
    base: i64,
    held: usize,
    x: [Vec<f32>; 2],
    mono: Vec<f32>,
}

impl File {
    /// `asset`'s file, read `cap` frames at a time, keeping `back` before each request.
    pub fn new(asset: Asset, cap: usize, back: usize) -> File {
        let cap = cap.max(4 * back + 16);
        File { asset, cap, back, base: 0, held: 0, x: [vec![0.0; cap], vec![0.0; cap]], mono: vec![0.0; cap] }
    }

    /// The file's own rate (0 for one that is missing).
    pub fn rate(&self) -> f32 {
        self.asset.sample_rate
    }

    pub fn frames(&self) -> i64 {
        if self.asset.missing() { 0 } else { self.asset.frames as i64 }
    }

    /// The most frames one request may span.
    pub fn most(&self) -> usize {
        self.cap - self.back
    }

    /// Make the window hold frames `from..to` (at most [`Self::most`] of them).
    pub fn hold(&mut self, from: i64, to: i64) {
        if from >= self.base && to <= self.base + self.held as i64 {
            return;
        }
        self.base = from - self.back as i64;
        self.held = self.cap;
        // Frames before the file's start are silence; it is read from its first frame on (a mono
        // file's channel 1 is its channel 0).
        let skip = (-self.base).clamp(0, self.cap as i64) as usize;
        let (base, frames) = (self.base, self.frames());
        for (c, x) in self.x.iter_mut().enumerate() {
            x.fill(0.0);
            if skip < self.cap && base + (skip as i64) < frames {
                self.asset.read(c, (base + skip as i64) as u32, &mut x[skip..]);
            }
        }
        for x in &mut self.x {
            for v in x.iter_mut() {
                *v = sane(*v);
            }
        }
        for (m, (l, r)) in self.mono.iter_mut().zip(self.x[0].iter().zip(&self.x[1])) {
            *m = 0.5 * (l + r);
        }
    }

    /// The mono sum over `from..to`, which the window holds.
    pub fn mono(&self, from: i64, to: i64) -> &[f32] {
        &self.mono[(from - self.base) as usize..(to - self.base) as usize]
    }

    /// The mono sum at frame `f`, which the window holds.
    pub fn mono_at(&self, f: i64) -> f32 {
        self.mono[(f - self.base) as usize]
    }

    /// Channel `c` read at fractional frame `pos` as a clip's normal path reads its file (a cubic
    /// Hermite, `m2_runtime`'s `stream::read_in`): silence outside the file. The window holds
    /// `pos - 1 .. pos + 3`.
    pub fn read(&self, c: usize, pos: f64) -> f32 {
        let len = self.frames();
        if pos < 0.0 || pos > (len as f64 - 1.0) {
            return 0.0;
        }
        let i = pos.floor() as i64;
        let t = (pos - i as f64) as f32;
        let x = &self.x[c];
        let at = |k: i64| {
            if k < 0 || k >= len { 0.0 } else { usize::try_from(k - self.base).ok().and_then(|i| x.get(i)).copied().unwrap_or(0.0) }
        };
        let (y0, y1, y2, y3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
        let c1 = 0.5 * (y2 - y0);
        let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
        let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
        ((c3 * t + c2) * t + c1) * t + y1
    }
}
