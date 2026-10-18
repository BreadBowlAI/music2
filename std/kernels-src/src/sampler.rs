//! `sampler`, the voice behind std/sampler: plays one of its audio files, read through kernel ABI
//! v1's asset import (`m2.asset_read`). Port order must match `std/m2pkg.toml`.
//!
//! Its settings say what it plays: its assets (the files), and in its values a root, a lowest and
//! a highest note for each file, in threes (left out: 60, 0 and 127). When a note starts, it picks
//! the first file whose range holds the note, and plays that file from its start. A note no range
//! holds, and a file that is missing, play silence.
//!
//! The file plays sped up by 2^((pitch + tune − root) / 12) × its rate / the song's rate, read at
//! that fractional place with the cubic Hermite formula clips use (`core/runtime/src/audio.rs`).
//! So at its root, at the song's rate, it plays the file's own samples; an octave up, every second
//! one. The speed follows pitch and tune at every sample, never the host's blocks, so the output is
//! the same however the host splits them. A block reads the file once a channel, into a window
//! taken at birth, and again only when the speed outruns it.
//!
//! `mode` 0 (one-shot) plays to the file's end, whatever the note does after it starts; 1 (gated)
//! fades out over `release` when the note ends. `attack` fades in from the note's start. Both are
//! straight lines, read when they start, and end in exact silence. The level is `velocity` (read
//! when the note starts) times `level`.

use m2_kernel::{Asset, Cfg, Io, Kernel, db_to_gain, export};

use crate::voice::{Memo, clampf, pitch_of};

// Inputs, in manifest order.
const PITCH: usize = 0;
const GATE: usize = 1;
const VELOCITY: usize = 2;
const TUNE: usize = 3;
const LEVEL: usize = 4;
const ATTACK: usize = 5;
const RELEASE: usize = 6;
const MODE: usize = 7;
const INPUTS: usize = 8;

/// The fastest a file plays: 6 octaves up. Faster is noise, and would need a read a sample.
const FASTEST: f64 = 64.0;

/// One file and the notes that play it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Zone {
    asset: Asset,
    root: f32,
    lo: f32,
    hi: f32,
}

/// The zones of a state's settings: each asset, with its three values (or their defaults).
fn zones(cfg: &Cfg) -> Box<[Zone]> {
    let v = |i: usize, default: f32| cfg.values.get(i).map_or(default, |&x| if x.is_finite() { x as f32 } else { default });
    cfg.assets.iter().enumerate().map(|(i, &asset)| Zone { asset, root: v(3 * i, 60.0), lo: v(3 * i + 1, 0.0), hi: v(3 * i + 2, 127.0) }).collect()
}

/// What one note is doing. A plain value: a reset writes a fresh one.
#[derive(Clone, Copy, Debug, Default)]
struct Note {
    /// The zone it plays; none while silent.
    zone: Option<usize>,
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
    velocity: f32,
}

/// Inputs: pitch, gate, velocity, tune, level, attack, release, mode. Output: stereo.
pub struct Sampler {
    sr: f32,
    zones: Box<[Zone]>,
    gate: bool,
    note: Note,
    level: Memo,
    /// The file's frames from `win_at` on, `win_len` of them, a channel each.
    win: [Box<[f32]>; 2],
    win_at: i64,
    win_len: usize,
}

impl Sampler {
    /// Start a note at sample `i`: pick its zone, from the file's start.
    fn start(&mut self, at: &dyn Fn(usize) -> f32) {
        let key = pitch_of(at(PITCH)).round();
        let zone = self.zones.iter().position(|z| !z.asset.missing() && key >= z.lo && key <= z.hi);
        let attack = clampf(at(ATTACK), 0.0, 60.0) * self.sr;
        let (env, slope) = if attack >= 1.0 { (0.0, 1.0 / attack) } else { (1.0, 0.0) };
        let velocity = clampf(at(VELOCITY), 0.0, 1.0);
        self.note = Note { zone, pos: 0.0, step: 0.0, step_for: (f32::NAN, f32::NAN), env, slope, releasing: false, gated: at(MODE) >= 0.5, velocity };
        self.win_len = 0;
    }

    /// The note ends: a gated one fades out over `release`.
    fn stop(&mut self, release: f32) {
        let n = &mut self.note;
        if n.zone.is_none() || !n.gated || n.releasing {
            return;
        }
        let frames = clampf(release, 0.0, 60.0) * self.sr;
        n.releasing = true;
        if frames < 1.0 || n.env <= 0.0 {
            n.zone = None;
        } else {
            n.slope = -n.env / frames;
        }
    }

    /// Frames `from..` of the file into the window, as many as the rest of the block needs at this
    /// speed (at least the 4 one read takes, at most the window): silence before the file's start.
    fn fill(&mut self, asset: &Asset, from: i64, want: usize) {
        let len = want.clamp(4, self.win[0].len());
        let skip = (-from).clamp(0, len as i64) as usize;
        for ch in 0..2 {
            let w = &mut self.win[ch][..len];
            w[..skip].fill(0.0);
            asset.read(ch, (from + skip as i64) as u32, &mut w[skip..]);
        }
        self.win_at = from;
        self.win_len = len;
    }

    /// The file at `pos`, both channels (cubic Hermite, as clips read): silence outside it.
    fn at(&mut self, asset: &Asset, pos: f64, left: usize) -> [f32; 2] {
        let i = pos.floor() as i64;
        if i - 1 < self.win_at || i + 3 > self.win_at + self.win_len as i64 {
            // The rest of the block, at this speed, from the frame before this one.
            let want = (pos + self.note.step * left as f64).floor() as i64 + 3 - (i - 1);
            self.fill(asset, i - 1, want.max(0) as usize);
        }
        let t = (pos - i as f64) as f32;
        let k = (i - 1 - self.win_at) as usize;
        let read = |w: &[f32]| {
            let (y0, y1, y2, y3) = (w[k], w[k + 1], w[k + 2], w[k + 3]);
            let c1 = 0.5 * (y2 - y0);
            let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
            let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
            ((c3 * t + c2) * t + c1) * t + y1
        };
        [read(&self.win[0]), read(&self.win[1])]
    }
}

impl Kernel for Sampler {
    const INPUTS: usize = INPUTS;
    const OUTPUTS: usize = 1;

    fn new(cfg: &Cfg) -> Self {
        let win = || vec![0.0; 4 * cfg.max_block.max(1) + 8].into_boxed_slice();
        Sampler { sr: cfg.sample_rate, zones: zones(cfg), gate: false, note: Note::default(), level: Memo::new(), win: [win(), win()], win_at: 0, win_len: 0 }
    }

    /// Its zones and window stay; the window is read again before it is used.
    fn reset(&mut self, _cfg: &Cfg) {
        self.gate = false;
        self.note = Note::default();
        self.level = Memo::new();
        self.win_at = 0;
        self.win_len = 0;
    }

    fn process(&mut self, io: &mut Io) {
        let (ins, [l, r]) = io.ports(std::array::from_fn::<_, INPUTS, _>(|p| (p, 0)), [(0, 0), (0, 1)]);
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
            let Some(z) = self.note.zone else {
                l[i] = 0.0;
                r[i] = 0.0;
                continue;
            };
            let zone = self.zones[z];
            let asset = zone.asset;
            let (pitch, tune) = (at(PITCH), at(TUNE));
            if (pitch, tune) != self.note.step_for {
                let semis = pitch_of(pitch) - zone.root + if tune.is_finite() { tune.clamp(-48.0, 48.0) } else { 0.0 };
                let rate = asset.sample_rate as f64 / self.sr as f64;
                self.note.step = ((semis as f64 / 12.0).exp2() * rate).min(FASTEST);
                self.note.step_for = (pitch, tune);
            }
            let [a, b] = self.at(&asset, self.note.pos, n - i);
            let g = self.note.env * self.note.velocity * self.level.get(clampf(at(LEVEL), -120.0, 24.0), db_to_gain);
            l[i] = a * g;
            r[i] = b * g;
            self.note.pos += self.note.step;
            if self.note.pos > asset.frames as f64 - 1.0 {
                // Past the file's last frame: the note is over.
                self.note.zone = None;
            }
            if self.note.slope != 0.0 {
                self.note.env += self.note.slope;
                if self.note.releasing && self.note.env <= 0.0 {
                    self.note.zone = None;
                } else if !self.note.releasing && self.note.env >= 1.0 {
                    self.note.env = 1.0;
                    self.note.slope = 0.0;
                }
            }
        }
    }

    /// While a file plays: past the note's end, a one-shot plays to the file's end, and a gated
    /// one fades out.
    fn active(&self) -> bool {
        self.note.zone.is_some()
    }
}

export!(sampler, Sampler);

#[cfg(test)]
mod tests;
