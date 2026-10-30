//! `sampler` and `regions`, the voices behind std/sampler: they play audio files, read through
//! kernel ABI v1's asset import (`m2.asset_read`). Port order must match `std/m2pkg.toml`.
//!
//! `sampler` plays zones. Its settings say what it plays: its assets (the files), and in its values
//! a root, a lowest and a highest note for each file, in threes (left out: 60, 0 and 127). When a
//! note starts, it picks the first file whose range holds the note, and plays that file from its
//! start. A note no range holds, and a file that is missing, play silence.
//!
//! `regions` plays regions (architecture.md, PKG-15 and PKG-16). Its assets are the files, each
//! once; its values are how many values a region takes (15), then each region's, in this order:
//! file (which asset, from 0), root, the lowest and highest key, the lowest and highest velocity
//! (1 to 127), the round robin position and how many takes (`of`), trigger (0 attack, 1 release),
//! the loop's first and last frame (-1: no loop), tune (cents), volume (dB), pan (-1 to 1) and
//! offset (frames). A region with fewer values takes the defaults for the rest, and values past the
//! 15 it knows are skipped, so a later std may add fields at the end. A value that can't be what its
//! field holds (a file that isn't there, a range upside down) never breaks it: such a region never
//! plays, and a number out of its range is clamped.
//!
//! When a note starts, its velocity becomes a MIDI velocity, once: round(velocity × 127), 1 to 127
//! (halves round up). Every attack region whose keys hold the note's pitch (rounded), whose velocity
//! range holds that velocity, and whose round robin position is the note's turn plays, summed: up to
//! as many as one note can match (32 at most), the first in the list first. The turn comes from
//! `count`, how many times notes have started on the lane, counted outside the voices so that it is
//! the same in an export and live: ((count - 1) mod of) + 1. When the note ends, the release regions
//! that match it (its key, and the velocity and turn it started with) start, and play to their end.
//!
//! A file plays sped up by 2^((pitch + tune - root) / 12) × its rate / the song's rate (a region's
//! tune added, in semitones), read at that fractional place with the cubic Hermite formula clips use
//! (`core/runtime/src/audio.rs`). So at its root, at the song's rate, it plays the file's own
//! samples; an octave up, every second one. The speed follows pitch and tune at every sample, never
//! the host's blocks, so the output is the same however the host splits them. A block reads the file
//! once a channel, into a window taken at birth, and again only when the speed outruns it.
//!
//! A region with a loop, while its note is held, goes from the loop's last frame back to its first.
//! The last `fade` frames before the end (5 ms of the file, but no more than the frames before the
//! loop's start, nor half the loop) crossfade in a straight line into the same frames before the
//! start, read through a second window, so the jump doesn't click. Whether a pass loops is decided
//! when it reaches its crossfade: held, it loops, and that crossfade finishes even if the note ends
//! in it; let go, it plays on past the end to the file's end. A loop's last frame past the file is
//! the file's last, and a pass that loops never ends at the file's end, so a loop to the file's
//! last frame (the one SFZ files most often have) loops like any other.
//!
//! A held note that a release region matches says it is active, though nothing may sound yet: a
//! poly frees a silent voice that says it isn't when its note ends on a block line, before the
//! voice sees the gate fall (architecture.md, RENDER-16), and the release would be lost.
//!
//! `mode` 0 (one-shot) plays to the file's end, whatever the note does after it starts; 1 (gated)
//! fades out over `release` when the note ends. `attack` fades in from the note's start. Both are
//! straight lines, read when they start, and end in exact silence; release regions play to their
//! end at full level. The level is level × (1 - veltrack + veltrack × velocity), velocity read when
//! the note starts (`veltrack` 1, its default, is velocity × level), times a region's volume and its
//! pan: the far side × sin((1 - |pan|) × π/2), the near side as it is.
//!
//! Memory: the regions, and as many layers as one note can play, each with its windows, are taken
//! in `new`. With zones there is one layer, and it plays as the sampler did before regions, bit for
//! bit: its volume and pan are a gain of exactly 1, and nothing is summed.

use std::f32::consts::FRAC_PI_2;

use m2_kernel::{Asset, Cfg, Io, Kernel, db_to_gain, export};

use crate::voice::{Memo, clampf, pitch_of};

// Inputs, in manifest order: `sampler` has the first 9, `regions` all 10.
const PITCH: usize = 0;
const GATE: usize = 1;
const VELOCITY: usize = 2;
const TUNE: usize = 3;
const LEVEL: usize = 4;
const ATTACK: usize = 5;
const RELEASE: usize = 6;
const MODE: usize = 7;
const VELTRACK: usize = 8;
const COUNT: usize = 9;

/// Values a region takes in `regions`' layout.
pub(crate) const FIELDS: usize = 15;

/// The most regions one voice plays at once.
const LAYERS: usize = 32;

/// The longest crossfade at a loop's end, in seconds of the file.
const LOOP_FADE: f64 = 0.005;

/// The fastest a file plays: 6 octaves up. Faster is noise, and would need a read a sample.
const FASTEST: f64 = 64.0;

/// A loop: from its first frame to the frame after its last, and how many frames before that end
/// crossfade into the frames before the start.
#[derive(Clone, Copy, Debug)]
struct Loop {
    start: f64,
    end: f64,
    fade: f64,
}

/// One file and when it plays: a zone, or a region.
#[derive(Clone, Copy, Debug)]
struct Region {
    asset: Asset,
    root: f32,
    lo: f32,
    hi: f32,
    /// Velocities, 1 to 127.
    soft: u32,
    loud: u32,
    /// Round robin: take `position` of `of`, 1 ≤ position ≤ of.
    position: u32,
    of: u32,
    release: bool,
    looped: Option<Loop>,
    /// In semitones.
    tune: f32,
    /// Volume and pan, a channel each.
    gain: [f32; 2],
    offset: f64,
}

impl Region {
    /// A region on `lo..=hi`, at every velocity and turn, as it is: a zone.
    fn zone(asset: Asset, root: f32, lo: f32, hi: f32) -> Region {
        Region {
            asset,
            root,
            lo,
            hi,
            soft: 1,
            loud: 127,
            position: 1,
            of: 1,
            release: false,
            looped: None,
            tune: 0.0,
            gain: [1.0, 1.0],
            offset: 0.0,
        }
    }

    /// Whether it plays for a note on `key` at `velocity` (1 to 127) whose count is `count` (1 on).
    fn holds(&self, key: f32, velocity: u32, count: u32) -> bool {
        key >= self.lo && key <= self.hi && (self.soft..=self.loud).contains(&velocity) && (count - 1) % self.of + 1 == self.position
    }
}

/// The zones of a state's settings: each asset, with its three values (or their defaults).
fn zones(cfg: &Cfg) -> Box<[Region]> {
    let v = |i: usize, default: f32| cfg.values.get(i).map_or(default, |&x| if x.is_finite() { x as f32 } else { default });
    cfg.assets.iter().enumerate().map(|(i, &asset)| Region::zone(asset, v(3 * i, 60.0), v(3 * i + 1, 0.0), v(3 * i + 2, 127.0))).collect()
}

/// The regions of a state's settings, in `regions`' layout (the module doc). A region that names
/// no asset of the state's, or a missing one, is left out.
fn regions_of(cfg: &Cfg) -> Box<[Region]> {
    let Some((&fields, body)) = cfg.values.split_first() else { return Box::new([]) };
    if !(1.0..=256.0).contains(&fields) || fields.fract() != 0.0 {
        return Box::new([]);
    }
    body.chunks(fields as usize).filter_map(|r| region(cfg.assets, r)).collect()
}

/// One region from its values `r` (fewer than [`FIELDS`] take the defaults).
fn region(assets: &[Asset], r: &[f64]) -> Option<Region> {
    // Values past the ones this std knows are for a later one.
    let r = &r[..r.len().min(FIELDS)];
    let at = |k: usize, default: f64| r.get(k).copied().filter(|x| x.is_finite()).unwrap_or(default);
    let whole = |k: usize, default: f64, lo: f64, hi: f64| at(k, default).round().clamp(lo, hi);
    let file = at(0, -1.0);
    if !(file >= 0.0 && file.fract() == 0.0 && file < assets.len() as f64) {
        return None;
    }
    let asset = assets[file as usize];
    if asset.missing() {
        return None;
    }
    let of = whole(7, 1.0, 1.0, u32::MAX as f64) as u32;
    let frames = asset.frames as f64;
    // Both frames are played: `last`, then `start` again.
    let (start, last) = (at(9, -1.0).round(), at(10, -1.0).round().min(frames - 1.0));
    let looped = (start >= 0.0 && start < last).then(|| {
        let end = last + 1.0;
        let fade = (LOOP_FADE * asset.sample_rate as f64).floor().min(start).min(((end - start) / 2.0).floor());
        Loop { start, end, fade }
    });
    let volume = db_to_gain(clampf(at(12, 0.0) as f32, -120.0, 24.0));
    let pan = clampf(at(13, 0.0) as f32, -1.0, 1.0);
    let far = ((1.0 - pan.abs()) * FRAC_PI_2).sin();
    let side = if pan > 0.0 {
        [far, 1.0]
    } else if pan < 0.0 {
        [1.0, far]
    } else {
        [1.0, 1.0]
    };
    Some(Region {
        asset,
        root: at(1, 60.0) as f32,
        lo: at(2, 0.0) as f32,
        hi: at(3, 127.0) as f32,
        soft: whole(4, 1.0, 1.0, 127.0) as u32,
        loud: whole(5, 127.0, 1.0, 127.0) as u32,
        position: whole(6, 1.0, 1.0, of as f64) as u32,
        of,
        release: at(8, 0.0) >= 0.5,
        looped,
        tune: clampf(at(11, 0.0) as f32, -2400.0, 2400.0) / 100.0,
        gain: [volume * side[0], volume * side[1]],
        // Past the file's end, it plays nothing.
        offset: at(14, 0.0).round().clamp(0.0, frames),
    })
}

/// The most regions one note can play at once, by key and velocity: its attack regions, then its
/// release regions with them (turns aside, so it may be more than any one note plays).
fn most_at_once(regions: &[Region]) -> usize {
    let peak = |d: &[i32; 129]| {
        let (mut n, mut most) = (0, 0);
        for x in d {
            n += x;
            most = most.max(n);
        }
        most as usize
    };
    let mut most = 0;
    for key in -48..=160 {
        let key = key as f32;
        let mut by = [[0i32; 129]; 2];
        for r in regions.iter().filter(|r| key >= r.lo && key <= r.hi && r.soft <= r.loud) {
            let d = &mut by[r.release as usize];
            d[r.soft as usize] += 1;
            d[r.loud as usize + 1] -= 1;
        }
        most = most.max(peak(&by[0]) + peak(&by[1]));
    }
    most
}

/// A file's frames from `at` on, `len` of them, a channel each.
struct Window {
    ch: [Box<[f32]>; 2],
    at: i64,
    len: usize,
}

impl Window {
    fn new(len: usize) -> Window {
        Window { ch: [vec![0.0; len].into_boxed_slice(), vec![0.0; len].into_boxed_slice()], at: 0, len: 0 }
    }

    /// Frames `from..` of the file into the window, as many as the rest of the block needs at this
    /// speed (at least the 4 one read takes, at most the window): silence before the file's start.
    fn fill(&mut self, asset: &Asset, from: i64, want: usize) {
        let len = want.clamp(4, self.ch[0].len());
        let skip = (-from).clamp(0, len as i64) as usize;
        for ch in 0..2 {
            let w = &mut self.ch[ch][..len];
            w[..skip].fill(0.0);
            asset.read(ch, (from + skip as i64) as u32, &mut w[skip..]);
        }
        self.at = from;
        self.len = len;
    }

    /// The file at `pos`, both channels (cubic Hermite, as clips read): silence outside it. `step`
    /// and `left` say how far the rest of the block reads.
    fn read(&mut self, asset: &Asset, pos: f64, step: f64, left: usize) -> [f32; 2] {
        let i = pos.floor() as i64;
        if i - 1 < self.at || i + 3 > self.at + self.len as i64 {
            // The rest of the block, at this speed, from the frame before this one.
            let want = (pos + step * left as f64).floor() as i64 + 3 - (i - 1);
            self.fill(asset, i - 1, want.max(0) as usize);
        }
        let t = (pos - i as f64) as f32;
        let k = (i - 1 - self.at) as usize;
        let read = |w: &[f32]| {
            let (y0, y1, y2, y3) = (w[k], w[k + 1], w[k + 2], w[k + 3]);
            let c1 = 0.5 * (y2 - y0);
            let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
            let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
            ((c3 * t + c2) * t + c1) * t + y1
        };
        [read(&self.ch[0]), read(&self.ch[1])]
    }
}

/// What one region is doing in a note. A plain value: a reset writes a fresh one.
#[derive(Clone, Copy, Debug, Default)]
struct Play {
    /// The region it plays; none while silent.
    region: Option<usize>,
    /// Where in the file, in frames at its own rate.
    pos: f64,
    /// Frames of the file a sample, and the pitch and tune it was worked out from.
    step: f64,
    step_for: (f32, f32),
    /// The fade: its level now, its change a sample, and whether it is fading out.
    env: f32,
    slope: f32,
    releasing: bool,
    gated: bool,
    /// The velocity's part of the level.
    amp: f32,
    /// Its loop is still ahead of it; and this pass, held when it reached its crossfade, loops.
    armed: bool,
    wrap: bool,
}

impl Play {
    /// Region `region` from its offset, at full level unless an attack fades it in.
    fn of(region: usize, r: &Region, env: f32, slope: f32, gated: bool, amp: f32, armed: bool) -> Play {
        Play {
            region: Some(region),
            pos: r.offset,
            step: 0.0,
            step_for: (f32::NAN, f32::NAN),
            env,
            slope,
            gated,
            amp,
            armed,
            ..Play::default()
        }
    }
}

/// A region playing, with the windows it reads through: the second only for a loop's crossfade.
struct Layer {
    play: Play,
    main: Window,
    tail: Window,
}

impl Layer {
    fn start(&mut self, play: Play) {
        self.play = play;
        self.main.len = 0;
        self.tail.len = 0;
    }
}

/// Inputs: pitch, gate, velocity, tune, level, attack, release, mode, veltrack, and for `regions`
/// count. Output: stereo. `REGIONS`: whether its values are regions, else zones.
pub struct Sampler<const REGIONS: bool> {
    sr: f32,
    regions: Box<[Region]>,
    gate: bool,
    /// The note's key, MIDI velocity, count and velocity's part of the level, from its start: what
    /// its release regions match and play at.
    key: f32,
    velocity: u32,
    count: u32,
    amp: f32,
    /// A release region matches the note: its end starts a sound.
    to_release: bool,
    level: Memo,
    layers: Box<[Layer]>,
}

impl<const REGIONS: bool> Sampler<REGIONS> {
    /// Start a note: the attack regions it plays, from their offsets.
    fn start(&mut self, at: &dyn Fn(usize) -> f32) {
        let key = pitch_of(at(PITCH)).round();
        let attack = clampf(at(ATTACK), 0.0, 60.0) * self.sr;
        let (env, slope) = if attack >= 1.0 { (0.0, 1.0 / attack) } else { (1.0, 0.0) };
        let velocity = clampf(at(VELOCITY), 0.0, 1.0);
        let v = clampf(at(VELTRACK), 0.0, 1.0);
        self.key = key;
        self.velocity = clampf((velocity * 127.0).round(), 1.0, 127.0) as u32;
        // A float to int cast saturates; NaN is the first turn too. Zones have no turns.
        self.count = if REGIONS { at(COUNT).max(1.0) as u32 } else { 1 };
        self.amp = 1.0 - v + v * velocity;
        self.to_release = self.regions.iter().any(|r| r.release && r.holds(key, self.velocity, self.count));
        let gated = at(MODE) >= 0.5;
        for l in self.layers.iter_mut() {
            l.play = Play::default();
        }
        let mut free = self.layers.iter_mut();
        for (i, r) in self.regions.iter().enumerate() {
            if r.release || r.asset.missing() || !r.holds(key, self.velocity, self.count) {
                continue;
            }
            let Some(l) = free.next() else { break };
            let armed = r.looped.is_some_and(|lp| r.offset < lp.end);
            l.start(Play::of(i, r, env, slope, gated, self.amp, armed));
        }
    }

    /// The note ends: a gated region fades out over `release`, and the release regions start.
    fn stop(&mut self, release: f32) {
        let frames = clampf(release, 0.0, 60.0) * self.sr;
        for l in self.layers.iter_mut() {
            let p = &mut l.play;
            if p.region.is_none() || !p.gated || p.releasing {
                continue;
            }
            p.releasing = true;
            if frames < 1.0 || p.env <= 0.0 {
                p.region = None;
            } else {
                p.slope = -p.env / frames;
            }
        }
        let mut free = self.layers.iter_mut().filter(|l| l.play.region.is_none());
        for (i, r) in self.regions.iter().enumerate() {
            if !r.release || !r.holds(self.key, self.velocity, self.count) {
                continue;
            }
            let Some(l) = free.next() else { break };
            l.start(Play::of(i, r, 1.0, 0.0, false, self.amp, false));
        }
    }
}

impl<const REGIONS: bool> Kernel for Sampler<REGIONS> {
    const INPUTS: usize = if REGIONS { 10 } else { 9 };
    const OUTPUTS: usize = 1;

    fn new(cfg: &Cfg) -> Self {
        let regions = if REGIONS { regions_of(cfg) } else { zones(cfg) };
        // Zones play one at a time: the first that holds the note.
        let layers = if REGIONS { most_at_once(&regions).min(LAYERS) } else { 1 };
        let window = 4 * cfg.max_block.max(1) + 8;
        let tail = if regions.iter().any(|r| r.looped.is_some()) { window } else { 0 };
        Sampler {
            sr: cfg.sample_rate,
            regions,
            gate: false,
            key: 0.0,
            velocity: 1,
            count: 1,
            amp: 0.0,
            to_release: false,
            level: Memo::new(),
            layers: (0..layers).map(|_| Layer { play: Play::default(), main: Window::new(window), tail: Window::new(tail) }).collect(),
        }
    }

    /// Its regions and windows stay; a window is read again before it is used.
    fn reset(&mut self, _cfg: &Cfg) {
        self.gate = false;
        (self.key, self.velocity, self.count, self.amp, self.to_release) = (0.0, 1, 1, 0.0, false);
        self.level = Memo::new();
        for l in self.layers.iter_mut() {
            l.play = Play::default();
            (l.main.at, l.main.len, l.tail.at, l.tail.len) = (0, 0, 0, 0);
        }
    }

    fn process(&mut self, io: &mut Io) {
        // `sampler` has no count: its place reads the gate again, and nothing reads it.
        let count = if REGIONS { COUNT } else { GATE };
        let (ins, [l, r]) = io.ports(std::array::from_fn::<_, 10, _>(|p| (if p == COUNT { count } else { p }, 0)), [(0, 0), (0, 1)]);
        let n = l.len();
        for i in 0..n {
            let at = |p: usize| ins[p][i];
            let gate = at(GATE) > 0.5;
            if gate != self.gate {
                self.gate = gate;
                if gate {
                    self.start(&at);
                } else {
                    self.stop(at(RELEASE));
                }
            }
            let mut silent = true;
            for layer in self.layers.iter_mut() {
                let Some(ri) = layer.play.region else { continue };
                let region = self.regions[ri];
                let asset = region.asset;
                let p = &mut layer.play;
                let (pitch, tune) = (at(PITCH), at(TUNE));
                if (pitch, tune) != p.step_for {
                    let semis = pitch_of(pitch) - region.root + if tune.is_finite() { tune.clamp(-48.0, 48.0) } else { 0.0 } + region.tune;
                    let rate = asset.sample_rate as f64 / self.sr as f64;
                    p.step = ((semis as f64 / 12.0).exp2() * rate).min(FASTEST);
                    p.step_for = (pitch, tune);
                }
                // A loop: a pass that reaches its crossfade while the note is held goes back to the
                // start at the end; one that reaches it let go plays on, and no pass loops again.
                let mut fade = None;
                if let Some(lp) = region.looped {
                    let from = lp.end - lp.fade;
                    loop {
                        if p.armed && !p.wrap && p.pos >= from {
                            (p.wrap, p.armed) = (self.gate, self.gate);
                        }
                        if !(p.wrap && p.pos >= lp.end) {
                            break;
                        }
                        p.pos -= lp.end - lp.start;
                        p.wrap = false;
                    }
                    if p.wrap && p.pos >= from {
                        fade = Some((((p.pos - from) / lp.fade) as f32, p.pos - (lp.end - lp.start)));
                    }
                }
                let [mut a, mut b] = layer.main.read(&asset, p.pos, p.step, n - i);
                if let Some((t, back)) = fade {
                    let [c, d] = layer.tail.read(&asset, back, p.step, n - i);
                    a += (c - a) * t;
                    b += (d - b) * t;
                }
                let g = p.env * p.amp * self.level.get(clampf(at(LEVEL), -120.0, 24.0), db_to_gain);
                let (x, y) = (a * g * region.gain[0], b * g * region.gain[1]);
                if silent {
                    (l[i], r[i]) = (x, y);
                    silent = false;
                } else {
                    l[i] += x;
                    r[i] += y;
                }
                p.pos += p.step;
                // Past the file's last frame, the region is over, unless this pass goes back to its
                // loop's start at the next sample: a loop may end on the file's last frame.
                if p.pos > asset.frames as f64 - 1.0 && !(p.wrap || p.armed && self.gate) {
                    p.region = None;
                }
                if p.slope != 0.0 {
                    p.env += p.slope;
                    if p.releasing && p.env <= 0.0 {
                        p.region = None;
                    } else if !p.releasing && p.env >= 1.0 {
                        p.env = 1.0;
                        p.slope = 0.0;
                    }
                }
            }
            if silent {
                l[i] = 0.0;
                r[i] = 0.0;
            }
        }
    }

    /// While a file plays: past the note's end, a one-shot plays to the file's end, a gated one
    /// fades out, and a release region plays. And while a note is held that a release region
    /// matches, silent or not: a poly frees a silent voice that says it isn't active when its
    /// note's end lands on a block line, before the voice sees the gate fall (core/runtime's
    /// poly), and the release would be lost.
    fn active(&self) -> bool {
        self.gate && self.to_release || self.layers.iter().any(|l| l.play.region.is_some())
    }
}

export!(sampler, Sampler<false>);
export!(regions, Sampler<true>);

#[cfg(test)]
mod tests;
