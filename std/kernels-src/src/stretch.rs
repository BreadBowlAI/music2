//! `stretch`, std's first offline kernel (kernel ABI v1, flags bit 0): a clip's audio laid out as its
//! time map says, keeping its pitch, then transposed by `values[0]` semitones and brought to the
//! song's rate. The host runs it off the audio thread to make a warped clip's render
//! (`core/runtime/src/offline.rs`); song code can't call it (`kernel.offline`). Port order must match
//! `std/m2pkg.toml`: no inputs, one output.
//!
//! **What it reads.** Its source is asset 0, read through `m2.asset_read` a window at a time, never
//! whole. Its time map is pairs of (source seconds, output seconds from the output's start), in
//! order ([`Cfg::time_map`]): time runs in a straight line between two pairs, and before the first
//! and after the last the nearest line goes on. One pair is speed 1 through it; none, the source as
//! it is. **What it writes:** `out`, with the source's channels, from its first frame on, each call
//! the next frames, as many calls as the host wants. Past the source's end it is silence.
//!
//! **The method** is WSOLA, as the runtime had it before (track sx, D6). Frames of 2048 source
//! frames, Hann-windowed, are laid every 1024 frames, so their windows add up to 1. Each frame is
//! taken from near where the map says, at the place within 392 frames whose start sounds most like
//! the natural continuation of the frame before, so waveforms join without a jump. The channels
//! share one place (their sum is matched), so a stereo image stays put. The match is normalized by
//! the candidate's loudness, so a loud spot can't win over one that is only similar. A drum hit
//! stays sharp, where a phase vocoder would smear it.
//!
//! **Pitch**: the stretch is made 2^(semitones / 12) times as long, then read back that much faster
//! with the cubic Hermite formula clips use (`core/runtime/src/stream.rs`), which also takes it from
//! the source's rate to the song's.
//!
//! Its memory is taken at birth: a window of the source, a window of the stretch, and the frame's
//! window. A reset clears them in place. Its maths is the same on every machine: only `+ - × ÷`,
//! `sqrt` and `floor`, which WebAssembly defines bit for bit, and `exp2` and `cos`, which std.wasm
//! carries its own copies of.

use m2_kernel::{Asset, Cfg, Io, Kernel, export};

/// Frames in one WSOLA frame, at the source's rate.
const FRAME: usize = 2048;
/// Frames from one frame's start to the next's: half a frame, so the windows add up to 1.
const HOP: usize = FRAME / 2;
/// How far from where the map says a frame may be taken, either way.
const SEARCH: i64 = 384;
/// The coarse search's step; the fine search looks this far either side of the coarse best.
const COARSE: usize = 8;
/// Frames of the source held at once: more than one frame and its search, so a frame that follows
/// on at speed 2 or less reads only the frames it adds.
const WINDOW: usize = 8192;
/// Frames of the stretch held at once: one frame, and the few the Hermite read looks back.
const HELD: usize = FRAME + 16;
/// The widest transposition, either way, in semitones: 5 octaves.
const MOST_SEMITONES: f64 = 60.0;

/// Inputs: none (its source is asset 0). Output: the stretch, with the source's channels.
pub struct Stretch {
    /// The source; none when it is missing.
    src: Option<Asset>,
    channels: usize,
    map: &'static [[f64; 2]],
    src_rate: f64,
    /// Frames of the stretch a second of output: the source's rate, times the transposition.
    z_rate: f64,
    /// Frames of the stretch an output frame reads past the one before.
    step: f64,
    /// The Hann window.
    window: Box<[f32]>,
    /// The source's frames from `s_at` on, `s_len` of them, a channel each, and their sum.
    s: [Box<[f32]>; 2],
    sum: Box<[f32]>,
    s_at: i64,
    s_len: usize,
    /// The stretch's frames from `z_at` on, a channel each: its frames below `done()` are whole.
    z: [Box<[f32]>; 2],
    z_at: i64,
    /// The next WSOLA frame's number: frame `k` lands at `k × HOP − HOP` of the stretch.
    next: i64,
    /// Where the frame before was taken from, in source frames.
    prev: Option<i64>,
    /// The sum of the frame before's natural continuation (its second half), every second frame.
    cont: Box<[f32]>,
    /// The next output frame.
    out: u64,
}

impl Stretch {
    /// Output seconds `o` as source seconds, through the time map.
    fn source_at(&self, o: f64) -> f64 {
        source_at(self.map, o)
    }

    /// Where frame `k` should be taken from, in source frames: its middle lands at `k × HOP` of the
    /// stretch, so its start at the map's place for that, less half a frame.
    fn nominal(&self, k: i64) -> i64 {
        let middle = (k * HOP as i64) as f64 / self.z_rate;
        let at = (self.source_at(middle) * self.src_rate).floor();
        // Any map, however wild, keeps the arithmetic below in range.
        at.clamp(-((1i64 << 40) as f64), (1i64 << 40) as f64) as i64 - HOP as i64
    }

    /// Frames of the stretch below this are whole: no frame yet to come adds to them.
    fn done(&self) -> i64 {
        (self.next - 1) * HOP as i64
    }

    /// Make sure the source window holds frames `lo..hi` (at most [`WINDOW`]): what it holds of them
    /// stays, the rest is read, and it is filled on past `hi` as far as it goes, so the next frames
    /// find theirs. Before the file's start is silence, as is its end and on.
    fn hold(&mut self, src: Asset, lo: i64, hi: i64) {
        let end = self.s_at + self.s_len as i64;
        if lo >= self.s_at && hi <= end {
            return;
        }
        let kept = if lo >= self.s_at && lo < end { (end - lo) as usize } else { 0 };
        if kept > 0 {
            let from = (lo - self.s_at) as usize;
            for c in 0..self.channels {
                self.s[c].copy_within(from..self.s_len, 0);
            }
            self.sum.copy_within(from..self.s_len, 0);
        }
        self.s_at = lo;
        let first = lo + kept as i64;
        for c in 0..self.channels {
            let buf = &mut self.s[c][kept..];
            let before = (-first).clamp(0, buf.len() as i64) as usize;
            buf[..before].fill(0.0);
            let from = first + before as i64;
            if from < src.frames as i64 {
                src.read(c, from as u32, &mut buf[before..]);
            } else {
                buf[before..].fill(0.0);
            }
        }
        for i in kept..WINDOW {
            self.sum[i] = match self.channels {
                1 => self.s[0][i],
                _ => self.s[0][i] + self.s[1][i],
            };
        }
        self.s_len = WINDOW;
    }

    /// How much the source from `cand` on sounds like the natural continuation: their correlation
    /// over half a frame, every second frame, over the candidate's loudness.
    fn score(&self, cand: i64) -> f32 {
        let at = (cand - self.s_at) as usize;
        let (mut dot, mut energy) = (0.0f32, 0.0f32);
        for (j, c) in self.cont.iter().enumerate() {
            let x = self.sum[at + 2 * j];
            dot += c * x;
            energy += x * x;
        }
        dot / (energy + 1e-12).sqrt()
    }

    /// Lay the next frame into the stretch. `keep` is the first frame of the stretch still to be
    /// read: what is before it may go.
    fn place(&mut self, src: Asset, keep: i64) {
        let k = self.next;
        let t = k * HOP as i64 - HOP as i64;
        let nominal = self.nominal(k);
        let pos = match self.prev {
            None => {
                self.hold(src, nominal, nominal + FRAME as i64);
                nominal
            }
            Some(_) => {
                let reach = SEARCH + COARSE as i64;
                self.hold(src, nominal - reach, nominal + reach + FRAME as i64);
                // Near where the map says, the place most like the natural continuation; on a tie,
                // the nearer to the map.
                let mut best = (nominal, self.score(nominal));
                for d in (-SEARCH..=SEARCH).step_by(COARSE) {
                    let s = self.score(nominal + d);
                    if s > best.1 {
                        best = (nominal + d, s);
                    }
                }
                let coarse = best.0;
                for d in -(COARSE as i64)..=COARSE as i64 {
                    let s = self.score(coarse + d);
                    if s > best.1 {
                        best = (coarse + d, s);
                    }
                }
                best.0
            }
        };
        // Room in the stretch for the frame: what is before both it and `keep` goes.
        if t + FRAME as i64 > self.z_at + HELD as i64 {
            let to = t.min(keep).max(self.z_at);
            let by = (to - self.z_at) as usize;
            for z in &mut self.z[..self.channels] {
                z.copy_within(by.., 0);
                z[HELD - by..].fill(0.0);
            }
            self.z_at = to;
        }
        let (at, from) = ((t - self.z_at) as usize, (pos - self.s_at) as usize);
        for c in 0..self.channels {
            let (z, s) = (&mut self.z[c][at..at + FRAME], &self.s[c][from..from + FRAME]);
            for ((z, &x), &w) in z.iter_mut().zip(s).zip(self.window.iter()) {
                *z += x * w;
            }
        }
        for (j, c) in self.cont.iter_mut().enumerate() {
            *c = self.sum[from + HOP + 2 * j];
        }
        self.prev = Some(pos);
        self.next += 1;
    }

    /// Channel `c` of the stretch at frame `i`: silence before its start.
    fn z(&self, c: usize, i: i64) -> f32 {
        if i < 0 { 0.0 } else { self.z[c][(i - self.z_at) as usize] }
    }
}

/// Output seconds `o` as source seconds, through `map` ([`Cfg::time_map`]).
fn source_at(map: &[[f64; 2]], o: f64) -> f64 {
    match map {
        [] => o,
        [p] => p[0] + (o - p[1]),
        _ => {
            let i = map.partition_point(|p| p[1] <= o).clamp(1, map.len() - 1);
            let (a, z) = (map[i - 1], map[i]);
            if z[1] > a[1] { a[0] + (o - a[1]) * (z[0] - a[0]) / (z[1] - a[1]) } else { a[0] }
        }
    }
}

impl Kernel for Stretch {
    const INPUTS: usize = 0;
    const OUTPUTS: usize = 1;

    fn new(cfg: &Cfg) -> Self {
        let src = cfg.assets.first().copied().filter(|a| !a.missing() && a.sample_rate > 0.0);
        let semitones = cfg.values.first().copied().filter(|v| v.is_finite()).unwrap_or(0.0).clamp(-MOST_SEMITONES, MOST_SEMITONES);
        let ratio = (semitones / 12.0).exp2();
        let src_rate = src.map_or(1.0, |a| a.sample_rate as f64);
        let window = (0..FRAME).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / FRAME as f32).cos()).collect();
        let channels = src.map_or(1, |a| (a.channels as usize).clamp(1, 2));
        let mut s = Stretch {
            src,
            channels,
            map: cfg.time_map(),
            src_rate,
            z_rate: src_rate * ratio,
            step: src_rate * ratio / cfg.sample_rate as f64,
            window,
            s: [vec![0.0; WINDOW].into(), vec![0.0; WINDOW].into()],
            sum: vec![0.0; WINDOW].into(),
            s_at: 0,
            s_len: 0,
            z: [vec![0.0; HELD].into(), vec![0.0; HELD].into()],
            z_at: 0,
            next: 0,
            prev: None,
            cont: vec![0.0; HOP / 2].into(),
            out: 0,
        };
        s.reset(cfg);
        s
    }

    fn reset(&mut self, _cfg: &Cfg) {
        for z in &mut self.z {
            z.fill(0.0);
        }
        self.cont.fill(0.0);
        (self.z_at, self.next, self.prev, self.s_at, self.s_len, self.out) = (-(HOP as i64), 0, None, 0, 0, 0);
    }

    fn process(&mut self, io: &mut Io) {
        let ([], [l, r]) = io.ports([], [(0, 0), (0, 1)]);
        let Some(src) = self.src else {
            l.fill(0.0);
            r.fill(0.0);
            return;
        };
        let last = self.channels - 1;
        for (l, r) in l.iter_mut().zip(r.iter_mut()) {
            // Where this frame reads the stretch: the four frames around it must be whole.
            let p = self.out as f64 * self.step;
            let i = p.floor() as i64;
            while self.done() <= i + 2 {
                self.place(src, i - 1);
            }
            let t = (p - i as f64) as f32;
            let read = |c: usize| {
                let (y0, y1, y2, y3) = (self.z(c, i - 1), self.z(c, i), self.z(c, i + 1), self.z(c, i + 2));
                let c1 = 0.5 * (y2 - y0);
                let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
                let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
                ((c3 * t + c2) * t + c1) * t + y1
            };
            (*l, *r) = (read(0), read(last));
            self.out += 1;
        }
    }
}

export!(stretch, Stretch);

#[cfg(test)]
mod tests;
