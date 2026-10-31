//! The kernel of the pack `@m2/wavetable`, its own sound code (PACK-21, PACK-27): one voice of the
//! wavetable synth, called by the pack's id, `kernel("@m2/wavetable:voice", …)`. Port order must
//! match the pack's `m2pkg.toml`. Written with std's kit and the blocks std's kernels share
//! (`m2_dsp::voice`), built with std's kernels by `std/build.sh` into
//! `packs/@m2/wavetable/kernels/wavetable.wasm` (ABI-10); its crate's manifest is
//! `std/kernels-src/packs/@m2/wavetable/Cargo.toml`. This folder is no part of the pack (PACK-20).
//!
//! **A voice** is two oscillators, A and B, each reading a table: frames of 2048 samples, one cycle
//! each. Its `position` (0 to 1) slides through the frames, crossfading the two it falls between.
//! Each oscillator is a stack of 1 to 8 unison voices, spread evenly from `detune` semitones below
//! the note to `detune` above and panned out by `spread`. They run at twice the song's rate: each
//! unison voice reads the mip level whose top harmonic stays below that rate's Nyquist (`shelf`),
//! between its samples by a cubic through four, and m2-dsp's halfband brings the sum back to the
//! song's rate, cutting all above 20 kHz. So nothing folds back however high a note is. Then a filter (m2-dsp's TPT state-variable filter:
//! lowpass, bandpass, highpass or notch, a choice), whose cutoff follows the key, its envelope and
//! the LFO, and the amp envelope (m2-dsp's ADSR). The LFO, a sine from the note's start, also moves
//! both positions.
//!
//! **Its settings** (`values`): table A, table B (a built-in table's place, [`tables::BUILT_IN`]),
//! the file that is A's table and B's (its place among the assets; -1, missing or not there: the
//! built-in one plays), and the unison voices. The tables are made at the voice's birth, once for
//! every voice of the sound (`shelf`), never while it plays.
//!
//! The house rules are m2-dsp's: audio that isn't finite never comes out, controls are clamped
//! (NaN takes the low end) and smoothed, recursive state is flushed far below hearing, and controls
//! are read on a clock of the voice's own (a tick every 16 samples from its first), so the output
//! is the same however the host splits its blocks.

// A `new` here is the state its kind starts from (a birth's), not a default, as in m2-dsp.
#![allow(clippy::new_without_default)]

use m2_kernel::{Cfg, Io, Kernel, abi, db_to_gain, export};

use m2_dsp::voice::{Env, Filter, Glide, Halfband, Memo, Ramp, Stage, TICK, clampf, hash, hz, pan, pitch_of, prewarp, sin2pi};

pub mod fft;
pub mod shelf;
pub mod tables;

use shelf::{Found, LEN, LEVELS, Source, TOP, Table};

abi!();

// Inputs, in manifest order.
pub const PITCH: usize = 0;
pub const GATE: usize = 1;
pub const VELOCITY: usize = 2;
pub const A_POSITION: usize = 3;
pub const A_PITCH: usize = 4;
pub const A_LEVEL: usize = 5;
pub const B_POSITION: usize = 6;
pub const B_PITCH: usize = 7;
pub const B_LEVEL: usize = 8;
pub const DETUNE: usize = 9;
pub const SPREAD: usize = 10;
pub const FILTER: usize = 11;
pub const CUTOFF: usize = 12;
pub const Q: usize = 13;
pub const KEYTRACK: usize = 14;
pub const ENV: usize = 15;
pub const F_ATTACK: usize = 16;
pub const F_DECAY: usize = 17;
pub const F_SUSTAIN: usize = 18;
pub const F_RELEASE: usize = 19;
pub const ATTACK: usize = 20;
pub const DECAY: usize = 21;
pub const SUSTAIN: usize = 22;
pub const RELEASE: usize = 23;
pub const LFO_RATE: usize = 24;
pub const LFO_POSITION: usize = 25;
pub const LFO_CUTOFF: usize = 26;
pub const VEL_AMOUNT: usize = 27;
pub const LEVEL: usize = 28;
pub const INPUTS: usize = 29;

/// The most unison voices.
pub const UNISON: usize = 8;

/// Highest oscillator frequency: the top level's 2 harmonics stay under twice the rate's Nyquist.
const OSC_MAX: f32 = 20_000.0;
/// Output gain: a full-scale table on one oscillator, at velocity 0.8, peaks near -12 dBFS.
const OUT: f32 = 0.3;
/// Control smoothing time.
const SMOOTH: f32 = 0.01;

/// The continuous controls, smoothed at the tick rate.
const SMOOTHED: [usize; 13] =
    [A_POSITION, A_LEVEL, B_POSITION, B_LEVEL, DETUNE, SPREAD, CUTOFF, Q, KEYTRACK, ENV, LFO_POSITION, LFO_CUTOFF, LEVEL];

/// The mip level a unison voice moving `dt` cycles a sample (at twice the rate) reads: the first
/// whose top harmonic stays below Nyquist.
fn level_for(dt: f32) -> usize {
    let mut k = 0;
    while k + 1 < LEVELS && TOP[k] as f32 * dt > 0.5 {
        k += 1;
    }
    k
}

/// The wave `f` of the way from sample `i` of a level's copy to the next ([`Table::level`]'s
/// layout): a cubic (Catmull-Rom) through the four samples about it.
fn read(y: &[f32], i: usize, f: f32) -> f32 {
    let [before, a, b, after]: [f32; 4] = y[i..i + 4].try_into().unwrap_or([0.0; 4]);
    let c1 = 0.5 * (b - before);
    let c2 = before - 2.5 * a + 2.0 * b - 0.5 * after;
    let c3 = 0.5 * (after - before) + 1.5 * (a - b);
    ((c3 * f + c2) * f + c1) * f + a
}

/// The filter's mix for its shape (lowpass, bandpass, highpass, notch), with damping `k`: how much
/// of the input, the bandpass and the lowpass make it.
fn shape(place: usize, k: f32) -> [f32; 3] {
    match place {
        0 => [0.0, 0.0, 1.0],
        1 => [0.0, k, 0.0],
        2 => [1.0, -k, -1.0],
        _ => [1.0, -k, 0.0],
    }
}

/// One voice, with its two tables.
pub struct Wavetable {
    tables: [Table; 2],
    play: Play,
}

impl Wavetable {
    /// Where oscillator `o`'s table lies now: on the shelf it shares, or in its own memory.
    pub fn table_at(&self, o: usize) -> *const f32 {
        self.tables[o].find().map_or(std::ptr::null(), |t| t.at())
    }
}

/// All a voice changes as it plays: what a reset puts back, taking no memory.
struct Play {
    sr: f32,
    unison: usize,
    /// Each table's last frame, as a position (0 to 1) is turned into a frame.
    last: [f32; 2],
    gate: bool,
    /// Samples to the next tick.
    clock: u32,
    /// The next tick starts from the targets rather than ramping (a note from silence).
    jump: bool,
    velocity: f32,
    smooth: [Glide; SMOOTHED.len()],
    vel_gain: Glide,
    /// velocity^vel_amount, and the two it was worked out for.
    bright: f32,
    bright_for: (f32, f32),
    amp: Env,
    fenv: Env,
    /// The LFO's phase, in cycles from the note's start.
    lfo: f32,
    /// Each oscillator's unison voices: phase, and cycles a sample (at twice the rate).
    phase: [[f32; UNISON]; 2],
    dt: [[Ramp; UNISON]; 2],
    /// The mip level each reads.
    level: [[usize; UNISON]; 2],
    /// Each oscillator's place among its frames, and its level.
    pos: [Ramp; 2],
    gain: [Ramp; 2],
    /// Each unison voice's pitch, as a ratio, and its gain into each channel.
    ratios: [f32; UNISON],
    ratios_for: f32,
    pans: [[f32; 2]; UNISON],
    pans_for: f32,
    /// The oscillators' base increments for their pitches, and the gain for a level.
    memo: [Memo; 3],
    half: [Halfband; 2],
    /// The filter's prewarped gain and damping (1 / q), each ramped a sample at a time so a fast
    /// envelope or LFO never steps it, its shape, and each channel's state.
    fg: Ramp,
    fk: Ramp,
    shape: usize,
    svf: [[f32; 2]; 2],
    out: Ramp,
}

impl Play {
    fn new(sr: f32, unison: usize, frames: [usize; 2]) -> Self {
        let rate = sr / TICK as f32;
        Play {
            sr,
            unison,
            last: frames.map(|f| (f - 1) as f32),
            gate: false,
            clock: 0,
            jump: true,
            velocity: 0.0,
            smooth: std::array::from_fn(|_| Glide::new(SMOOTH, rate)),
            vel_gain: Glide::new(0.005, rate),
            bright: 0.0,
            bright_for: (f32::NAN, f32::NAN),
            amp: Env::new(),
            fenv: Env::new(),
            lfo: 0.0,
            phase: [[0.0; UNISON]; 2],
            dt: [[Ramp::default(); UNISON]; 2],
            level: [[0; UNISON]; 2],
            pos: [Ramp::default(); 2],
            gain: [Ramp::default(); 2],
            ratios: [1.0; UNISON],
            ratios_for: f32::NAN,
            pans: [[0.0; 2]; UNISON],
            pans_for: f32::NAN,
            memo: [Memo::new(); 3],
            half: [Halfband::default(); 2],
            fg: Ramp::default(),
            fk: Ramp::default(),
            shape: 0,
            svf: [[0.0; 2]; 2],
            out: Ramp::default(),
        }
    }

    fn start(&mut self, at: &dyn Fn(usize) -> f32) {
        let fresh = self.amp.value == 0.0;
        if fresh {
            // One voice starts at 0, upward, as every frame does; a stack starts its voices at
            // places that sound random but are the same for every note.
            for (k, p) in self.phase[0].iter_mut().enumerate() {
                *p = if self.unison == 1 { 0.0 } else { (hash(k as u32, 7) >> 8) as f32 / (1u32 << 24) as f32 };
            }
            self.phase[1] = self.phase[0];
            self.lfo = 0.0;
            self.half = [Halfband::default(); 2];
            self.svf = [[0.0; 2]; 2];
            // Controls start where they are, not where they were when the last note ended.
            self.smooth = std::array::from_fn(|_| Glide::new(SMOOTH, self.sr / TICK as f32));
            self.ratios_for = f32::NAN;
            self.pans_for = f32::NAN;
        }
        self.velocity = clampf(at(VELOCITY), 0.0, 1.0);
        self.amp.on();
        self.fenv.on();
        self.clock = 0;
        self.jump = fresh;
    }

    /// Read the controls and set every ramp heading for its new value.
    fn tick(&mut self, at: &dyn Fn(usize) -> f32) {
        let rate = self.sr / TICK as f32;
        let jump = std::mem::take(&mut self.jump);
        for h in &mut self.half {
            h.flush();
        }
        let range = |p: usize| -> (f32, f32) {
            match p {
                DETUNE => (0.0, 1.0),
                CUTOFF => (20.0, 20_000.0),
                Q => (0.1, 40.0),
                ENV => (-8.0, 8.0),
                LFO_CUTOFF => (0.0, 4.0),
                LEVEL => (-60.0, 12.0),
                _ => (0.0, 1.0),
            }
        };
        let mut c = [0.0; SMOOTHED.len()];
        for (i, &p) in SMOOTHED.iter().enumerate() {
            let (lo, hi) = range(p);
            c[i] = self.smooth[i].next(clampf(at(p), lo, hi));
        }
        let [a_position, a_level, b_position, b_level, detune, spread, cutoff, q, keytrack, env, lfo_position, lfo_cutoff, level] = c;

        // A legato note (one at a time) brings its own velocity.
        if self.gate {
            self.velocity = clampf(at(VELOCITY), 0.0, 1.0);
        }
        let pitch = pitch_of(at(PITCH));
        let lfo = sin2pi(self.lfo);
        self.lfo = (self.lfo + clampf(at(LFO_RATE), 0.0, 40.0) * TICK as f32 / self.sr).fract();

        // Unison: pitches and gains change only with their controls.
        let n = self.unison;
        if detune != self.ratios_for {
            self.ratios_for = detune;
            for (k, r) in self.ratios.iter_mut().enumerate().take(n) {
                let at = if n == 1 { 0.0 } else { 2.0 * k as f32 / (n - 1) as f32 - 1.0 };
                *r = (detune * at / 12.0).exp2();
            }
        }
        if spread != self.pans_for {
            self.pans_for = spread;
            self.set_pans(spread);
        }

        let sr = self.sr;
        let top = OSC_MAX / (2.0 * sr);
        for (o, (offset, position, gain)) in [(A_PITCH, a_position, a_level), (B_PITCH, b_position, b_level)].into_iter().enumerate() {
            let p = pitch + clampf(at(offset), -48.0, 48.0);
            let base = self.memo[o].get(p, |p| clampf(hz(p), 1.0, OSC_MAX) / (2.0 * sr));
            for k in 0..n {
                let dt = (base * self.ratios[k]).min(top);
                let ramp = &mut self.dt[o][k];
                ramp.to(dt, jump);
                // A glide's ramp moves within the tick: the level fits both its ends.
                self.level[o][k] = level_for(dt.max(ramp.value));
            }
            let place = clampf(position + lfo_position * lfo, 0.0, 1.0) * self.last[o];
            self.pos[o].to(place, jump);
            self.gain[o].to(gain, jump);
        }

        // Velocity: the level follows v^(2 × vel_amount); the filter's envelope its square root.
        let sens = clampf(at(VEL_AMOUNT), 0.0, 1.0);
        if (self.velocity, sens) != self.bright_for {
            self.bright_for = (self.velocity, sens);
            self.bright = self.velocity.powf(sens);
        }
        let bright = self.bright;
        // The filter: its cutoff follows the key, its envelope (scaled by velocity) and the LFO.
        self.fenv.set(at(F_ATTACK), at(F_DECAY), at(F_SUSTAIN), at(F_RELEASE), rate);
        let fe = self.fenv.next();
        let octaves = keytrack * (pitch - 60.0) / 12.0 + env * fe * bright + lfo_cutoff * lfo;
        let fc = clampf(cutoff * clampf(octaves, -20.0, 20.0).exp2(), 20.0, 0.45 * sr);
        self.fg.to(prewarp(fc, sr), jump);
        self.fk.to(1.0 / q, jump);
        self.shape = clampf(at(FILTER).round(), 0.0, 3.0) as usize;

        self.amp.set(at(ATTACK), at(DECAY), at(SUSTAIN), at(RELEASE), sr);
        if jump {
            self.vel_gain = Glide::new(0.005, rate);
        }
        let vel = self.vel_gain.next(bright * bright);
        self.out.to(OUT * self.memo[2].get(level, db_to_gain) * vel, jump);
    }

    /// Each unison voice's gains: equal-power pans, the outer voices widest, neighbours on opposite
    /// sides (so neither side gets all the high voices), the stack as loud as one voice.
    fn set_pans(&mut self, spread: f32) {
        let n = self.unison;
        let norm = 1.0 / (n as f32).sqrt();
        for k in 0..n {
            let at = if n == 1 { 0.0 } else { 2.0 * k as f32 / (n - 1) as f32 - 1.0 };
            // Mirror the low half's sides onto the high half.
            let low = k.min(n - 1 - k);
            let side = if low % 2 == 0 { -1.0 } else { 1.0 } * if k == low { 1.0 } else { -1.0 };
            let [l, r] = pan(spread * at.abs() * side);
            self.pans[k] = [l * norm, r * norm];
        }
    }

    /// One output sample, before the amp envelope.
    fn sample(&mut self, tables: &[Option<Found>; 2]) -> [f32; 2] {
        let n = self.unison;
        let gains = [self.gain[0].next(), self.gain[1].next()];
        let places = [self.pos[0].next(), self.pos[1].next()];
        let mut x = [[0.0f32; 2]; 2];
        for (o, table) in tables.iter().enumerate() {
            for d in &mut self.dt[o][..n] {
                d.next();
            }
            let g = gains[o];
            let table = match table {
                Some(t) if g != 0.0 => t,
                // Silent (or its table not on this sandbox's shelf), it keeps time, so turning it
                // up never starts it somewhere else.
                _ => {
                    for k in 0..n {
                        let p = self.phase[o][k] + 2.0 * self.dt[o][k].value;
                        self.phase[o][k] = p - p.floor();
                    }
                    continue;
                }
            };
            let place = places[o];
            let j = place as usize;
            let m = place - j as f32;
            for k in 0..n {
                let level = self.level[o][k];
                let (a, b) = (table.level(j, level), table.level(j + 1, level));
                let len = LEN[level];
                let dt = self.dt[o][k].value;
                let [gl, gr] = self.pans[k];
                for xs in &mut x {
                    let mut p = self.phase[o][k] + dt;
                    if p >= 1.0 {
                        p -= 1.0;
                    }
                    self.phase[o][k] = p;
                    let at = p * len as f32;
                    let i = (at as usize).min(len - 1);
                    let f = at - i as f32;
                    let (va, vb) = (read(a, i, f), read(b, i, f));
                    let v = g * (va + m * (vb - va));
                    xs[0] += v * gl;
                    xs[1] += v * gr;
                }
            }
        }
        let (g, k) = (self.fg.next(), self.fk.next());
        let filter = Filter::new(g, k, shape(self.shape, k));
        let mut y = [0.0; 2];
        for (c, y) in y.iter_mut().enumerate() {
            let h = self.half[c].tick(x[0][c], x[1][c]);
            *y = filter.tick(&mut self.svf[c], h);
        }
        y
    }
}

/// The table oscillator `o` plays: its file when it has one, else its built-in table.
fn table_of(cfg: &Cfg, o: usize) -> Table {
    let value = |i: usize, d: f64| cfg.values.get(i).copied().filter(|v| v.is_finite()).unwrap_or(d);
    let built_in = value(o, 0.0).round().clamp(0.0, (tables::BUILT_IN.len() - 1) as f64) as usize;
    let file = value(2 + o, -1.0).round();
    let file = (file >= 0.0).then(|| cfg.assets.get(file as usize)).flatten().filter(|f| !f.missing());
    match file {
        Some(&file) => shelf::table(Source::File(file.handle), tables::file_frames(&file), |out| {
            let mut reader = tables::Reader::new(file);
            tables::make(tables::file_frames(&file), out, |j, a, fft| reader.frame(j, a, fft))
        }),
        None => shelf::table(Source::Builtin(built_in), tables::frames(built_in), |out| {
            tables::make(tables::frames(built_in), out, |j, a, _| tables::built_in(built_in, j, a))
        }),
    }
}

/// The unison voices a voice's settings ask for.
fn unison_of(cfg: &Cfg) -> usize {
    cfg.values.get(4).copied().filter(|v| v.is_finite()).unwrap_or(1.0).round().clamp(1.0, UNISON as f64) as usize
}

impl Kernel for Wavetable {
    const INPUTS: usize = INPUTS;
    const OUTPUTS: usize = 1;

    fn new(cfg: &Cfg) -> Self {
        let tables = [table_of(cfg, 0), table_of(cfg, 1)];
        let play = Play::new(cfg.sample_rate.max(1.0), unison_of(cfg), tables.each_ref().map(Table::frames));
        Wavetable { tables, play }
    }

    /// Back to its birth, keeping its tables: no memory taken.
    fn reset(&mut self, cfg: &Cfg) {
        self.play = Play::new(cfg.sample_rate.max(1.0), unison_of(cfg), self.tables.each_ref().map(Table::frames));
    }

    fn process(&mut self, io: &mut Io) {
        let (ins, [ol, or]) = io.ports(std::array::from_fn::<_, INPUTS, _>(|p| (p, 0)), [(0, 0), (0, 1)]);
        // Each table, where it is for this call: found on the shelf by its settings (ABI-5).
        let found = [self.tables[0].find(), self.tables[1].find()];
        let play = &mut self.play;
        for i in 0..ol.len() {
            let at = |p: usize| ins[p][i];
            let gate = at(GATE) > 0.5;
            if gate != play.gate {
                play.gate = gate;
                if gate {
                    play.start(&at);
                } else {
                    play.amp.off();
                    play.fenv.off();
                }
            }
            // Silent and not playing: nothing to do.
            if play.amp.stage == Stage::Idle {
                ol[i] = 0.0;
                or[i] = 0.0;
                continue;
            }
            if play.clock == 0 {
                play.tick(&at);
            }
            play.clock = (play.clock + 1) % TICK;
            let a = play.amp.next();
            if a == 0.0 {
                ol[i] = 0.0;
                or[i] = 0.0;
                continue;
            }
            let [l, r] = play.sample(&found);
            let g = a * play.out.next();
            ol[i] = l * g;
            or[i] = r * g;
        }
    }

    /// Sound that started can still come out only in the release.
    fn active(&self) -> bool {
        self.play.amp.stage == Stage::Release
    }
}

export!(voice, Wavetable);

#[cfg(test)]
mod tests;
