//! `synth`, the polysynth voice behind @m2/synths: a unison stack of saws and pulses with
//! a sub sine and noise, at 2× (band-limited steps, a drive, then a 4-pole ladder with its
//! own envelope, whose saturation would alias at 1×), decimated, then the amp envelope. Its
//! `shape`, a choice (CARD-12), says what the stack's main wave is: a saw (as it always was), a
//! square, a triangle or a sine.
//! Port order must match the pack's `m2pkg.toml`.

use m2_dsp::voice::{
    Clip, DcBlock, Drive, Env, Glide, Halfband, Ladder, LadderCoefs, Memo, Noise, Ramp, Stage, Steps, TICK, clampf, hash, hz, one_pole,
    pan, pitch_of, sin2pi,
};
use m2_kernel::{Cfg, Io, Kernel, db_to_gain, export};

// Inputs, in manifest order.
const PITCH: usize = 0;
const GATE: usize = 1;
const VELOCITY: usize = 2;
const SAW: usize = 3;
const SQUARE: usize = 4;
const WIDTH: usize = 5;
const SUB: usize = 6;
const SUB_OCTAVE: usize = 7;
const NOISE: usize = 8;
const UNISON: usize = 9;
const DETUNE: usize = 10;
const SPREAD: usize = 11;
const DROP: usize = 12;
const DROP_TIME: usize = 13;
const VIBRATO: usize = 14;
const CUTOFF: usize = 15;
const RESONANCE: usize = 16;
const KEYTRACK: usize = 17;
const ENV: usize = 18;
const F_ATTACK: usize = 19;
const F_DECAY: usize = 20;
const F_SUSTAIN: usize = 21;
const ATTACK: usize = 22;
const DECAY: usize = 23;
const SUSTAIN: usize = 24;
const RELEASE: usize = 25;
const DRIVE: usize = 26;
const VEL_AMOUNT: usize = 27;
const LEVEL: usize = 28;
const SEED: usize = 29;
const WHEEL: usize = 30;
const SHAPE: usize = 31;
const INPUTS: usize = 32;

/// The stack's main wave, by its place in `shape`'s choice: `saw|square|triangle|sine`.
const SQUARE_WAVE: u32 = 1;
const TRIANGLE_WAVE: u32 = 2;
const SINE_WAVE: u32 = 3;

/// Most unison voices.
const UNISON_MAX: usize = 7;

/// Each unison voice's detune, as a fraction of the outermost voice's, and its place in
/// the stereo field at full spread. Detunes are slightly uneven (after the JP-8000's, as
/// measured by Adam Szabo) so the voices never beat in step; pans alternate so both sides
/// get high and low voices.
const OFFSETS: [[f32; UNISON_MAX]; UNISON_MAX] = [
    [0.0; 7],
    [-1.0, 0.96, 0.0, 0.0, 0.0, 0.0, 0.0],
    [-1.0, 0.0, 0.98, 0.0, 0.0, 0.0, 0.0],
    [-1.0, -0.33, 0.34, 0.97, 0.0, 0.0, 0.0],
    [-1.0, -0.45, 0.0, 0.46, 0.98, 0.0, 0.0],
    [-1.0, -0.57, -0.18, 0.18, 0.56, 0.98, 0.0],
    [-1.0, -0.5716, -0.1775, 0.0, 0.181, 0.565, 0.9766],
];
const PANS: [[f32; UNISON_MAX]; UNISON_MAX] = [
    [0.0; 7],
    [-1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    [-1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0],
    [-1.0, 0.5, -0.5, 1.0, 0.0, 0.0, 0.0],
    [-1.0, 0.5, 0.0, -0.5, 1.0, 0.0, 0.0],
    [-1.0, 0.6, -0.2, 0.2, -0.6, 1.0, 0.0],
    [-1.0, 0.67, -0.33, 0.0, 0.33, -0.67, 1.0],
];

/// Highest oscillator frequency, so a stack's top voice stays below the decimator's band edge.
const OSC_MAX: f32 = 20_000.0;
/// The highest cutoff, as a fraction of the sample rate: the ladder runs at 2×, where a
/// cutoff this high leaves the audio band as open as an analog ladder at 2× the sample rate.
const CUTOFF_MAX: f32 = 0.8;
/// The vibrato's rate, and how long it takes to fade in after a note starts.
const VIBRATO_HZ: f32 = 5.5;
const VIBRATO_FADE: f32 = 0.3;
/// The vibrato the mod wheel adds at the top, in semitones: there at once, not faded in.
const WHEEL_DEPTH: f32 = 0.5;
/// The ladder's feedback for a resonance: rising evenly to 4 (where it starts to sing),
/// then a little past it at the top, so resonance 1 sings strongly as a sine.
fn feedback(resonance: f32) -> f32 {
    let r4 = resonance * resonance * resonance * resonance;
    resonance * 4.0 + 0.6 * r4 * r4
}
/// Output gain: the default patch, one voice at velocity 0.8, peaks near -12 dBFS.
const OUT: f32 = 0.3;
/// Control smoothing time.
const SMOOTH: f32 = 0.01;

/// Cents of detune for the outermost unison voice at `d` (0..1): gentle at first, then
/// wide (about ±2 semitones at 1, as on the JP-8000).
fn detune_cents(d: f32) -> f32 {
    1.0 + d * (30.0 + d * 60.0) + 100.0 * d * d * d * d * d
}

/// The continuous controls, smoothed at the tick rate.
const SMOOTHED: [usize; 14] = [SAW, SQUARE, WIDTH, SUB, NOISE, DETUNE, SPREAD, VIBRATO, CUTOFF, RESONANCE, KEYTRACK, ENV, DRIVE, LEVEL];

/// Inputs: pitch, gate, velocity, then the patch (see the manifest). Output: stereo.
pub struct Synth {
    sr: f32,
    gate: bool,
    /// Samples to the next tick.
    clock: u32,
    /// The next tick starts from the targets rather than ramping (a note from silence).
    jump: bool,
    /// White noise's gain, so its level in hertz is the same at every sample rate.
    noise_gain: f32,
    /// Samples since the note started.
    since: u32,
    velocity: f32,
    smooth: [Glide; SMOOTHED.len()],
    /// The velocity's gain, smoothed so a legato change of velocity doesn't step.
    vel_gain: Glide,
    /// velocity^vel_amount, and the two it was worked out for.
    bright: f32,
    bright_for: (f32, f32),
    amp: Env,
    fenv: Env,
    // Settings read when a note starts from silence.
    voices: usize,
    sub_octave: f32,
    stereo: bool,
    // Oscillators, at 2×.
    phase: [f32; UNISON_MAX],
    /// Whether each voice's pulse is high (its phase is below the width).
    high: [bool; UNISON_MAX],
    dt: [Ramp; UNISON_MAX],
    /// Each voice's gain into each channel.
    gains: [[f32; 2]; UNISON_MAX],
    /// The gains' sum per channel, for the constant parts of the saw and the pulse.
    gain_sum: [f32; 2],
    /// The detune and spread the gains were worked out for.
    gains_for: (f32, f32),
    ratios: [f32; UNISON_MAX],
    ratios_for: f32,
    /// The oscillators' base increment for a pitch, the ladder's G for a cutoff, the gain for a level.
    memo: [Memo; 3],
    sub_phase: f32,
    sub_dt: Ramp,
    vib_phase: f32,
    noise: Noise,
    /// The levels of the saw, the pulse, the sub, the noise, the triangle and the sine.
    levels: [Ramp; 6],
    width: Ramp,
    bus: Steps,
    // Shaping, per channel.
    drive_on: bool,
    drive: Drive,
    drive_for: f32,
    drive_in: Ramp,
    drive_out: Ramp,
    clip: [Clip; 2],
    half: [Halfband; 2],
    cutoff: Ramp,
    k: Ramp,
    ladder: [Ladder; 2],
    /// Two DC blockers in a row per channel: a driven, wide stack's slow beats make
    /// infrasonic difference tones, and those go too.
    dc: [[DcBlock; 2]; 2],
    dc_pole: f32,
    out: Ramp,
}

impl Synth {
    fn start(&mut self, at: &dyn Fn(usize) -> f32) {
        let fresh = self.amp.value == 0.0;
        if fresh {
            self.voices = clampf(at(UNISON).round(), 1.0, UNISON_MAX as f32) as usize;
            self.sub_octave = clampf(at(SUB_OCTAVE).round(), -2.0, 0.0);
            self.stereo = self.voices > 1 && at(SPREAD) > 0.0;
            // Start phases that sound random but are the same for the same note and seed.
            let seed = if at(SEED).is_finite() { at(SEED) as i32 as u32 } else { 0 };
            let key = hash((pitch_of(at(PITCH)) * 64.0) as i32 as u32, seed);
            for (k, p) in self.phase.iter_mut().enumerate() {
                *p = (hash(key, k as u32) >> 8) as f32 / (1u32 << 24) as f32;
            }
            self.high = self.phase.map(|p| p < 0.5);
            self.noise = Noise::new(hash(key, 99));
            // The sub locked to the centre voice, half a cycle on, where the saw's fundamental
            // (a falling sine) is: at the note's own pitch it reinforces it on every note
            // instead of cancelling some of it by chance.
            self.sub_phase = (self.phase[self.voices / 2] * self.sub_octave.exp2() + 0.5).fract();
            self.vib_phase = 0.0;
            self.bus = Steps::default();
            self.clip = [Clip::default(); 2];
            self.half = [Halfband::default(); 2];
            self.ladder = [Ladder::default(); 2];
            self.dc = [[DcBlock::default(); 2]; 2];
            // A whisper in the filter, so a self-oscillating ladder sings with nothing going in.
            self.ladder[0].nudge();
            self.ladder[1].nudge();
            // Controls start where they are, not where they were when the last note ended.
            self.smooth = std::array::from_fn(|_| Glide::new(SMOOTH, self.sr / TICK as f32));
            self.ratios_for = f32::NAN;
            self.gains_for = (f32::NAN, f32::NAN);
            self.drive_for = f32::NAN;
        }
        self.velocity = clampf(at(VELOCITY), 0.0, 1.0);
        self.since = 0;
        self.amp.on();
        self.fenv.on();
        self.clock = 0;
        self.jump = fresh;
    }

    /// Read the controls and set every ramp heading for its new value.
    fn tick(&mut self, at: &dyn Fn(usize) -> f32) {
        let rate = self.sr / TICK as f32;
        let jump = std::mem::take(&mut self.jump);
        for c in 0..2 {
            self.half[c].flush();
            self.ladder[c].flush();
            self.dc[c].iter_mut().for_each(DcBlock::flush);
        }
        let mut c = [0.0; SMOOTHED.len()];
        let range = |p: usize| -> (f32, f32) {
            match p {
                WIDTH => (0.05, 0.95),
                CUTOFF => (20.0, 20_000.0),
                ENV => (-8.0, 8.0),
                DRIVE => (0.0, 36.0),
                LEVEL => (-60.0, 12.0),
                VIBRATO => (0.0, 12.0),
                _ => (0.0, 1.0),
            }
        };
        for (i, &p) in SMOOTHED.iter().enumerate() {
            let (lo, hi) = range(p);
            c[i] = self.smooth[i].next(clampf(at(p), lo, hi));
        }
        let [saw, square, width, sub, noise, detune, spread, vibrato, cutoff, resonance, keytrack, env, drive, level] = c;

        // A legato note (one at a time) brings its own velocity.
        if self.gate {
            self.velocity = clampf(at(VELOCITY), 0.0, 1.0);
        }
        let pitch = pitch_of(at(PITCH));
        let t = self.since as f32 / self.sr;
        self.since = self.since.saturating_add(TICK);

        // Pitch: the note, a drop into it, a vibrato that fades in.
        let mut p = pitch;
        let drop = clampf(at(DROP), 0.0, 48.0);
        if drop > 0.0 {
            p += drop * (-3.0 * t / clampf(at(DROP_TIME), 0.001, 10.0)).exp();
        }
        // The mod wheel deepens it; at 0 it changes no bit.
        let wheel = clampf(at(WHEEL), 0.0, 1.0);
        if vibrato > 0.0 || wheel > 0.0 {
            let mut depth = vibrato * (t / VIBRATO_FADE).min(1.0);
            if wheel > 0.0 {
                depth += WHEEL_DEPTH * wheel;
            }
            p += depth * sin2pi(self.vib_phase);
        }
        self.vib_phase = (self.vib_phase + VIBRATO_HZ * TICK as f32 / self.sr).fract();
        let sr = self.sr;
        let base = self.memo[0].get(p, |p| clampf(hz(p), 1.0, OSC_MAX) / (2.0 * sr));

        // Unison: detune ratios and gains change only with their controls.
        let n = self.voices;
        if detune != self.ratios_for {
            self.ratios_for = detune;
            let cents = detune_cents(detune);
            for (r, o) in self.ratios.iter_mut().zip(&OFFSETS[n - 1]).take(n) {
                *r = (cents * o / 1200.0).exp2();
            }
        }
        if (detune, spread) != self.gains_for {
            self.gains_for = (detune, spread);
            self.set_gains(detune, spread);
        }
        let top = OSC_MAX / (2.0 * self.sr);
        for k in 0..n {
            self.dt[k].to((base * self.ratios[k]).min(top), jump);
        }
        self.sub_dt.to(base * self.sub_octave.exp2(), jump);
        // The shape says which wave the `saw` level plays on the stack's phases: the saw (as it
        // always was), the pulse (with its own level, at `width`), a triangle or a sine. A change
        // glides over a tick, as the levels do.
        let (saw, square, tri, sine) = match clampf(at(SHAPE).round(), 0.0, 3.0) as u32 {
            SQUARE_WAVE => (0.0, square + saw, 0.0, 0.0),
            TRIANGLE_WAVE => (0.0, square, saw, 0.0),
            SINE_WAVE => (0.0, square, 0.0, saw),
            _ => (saw, square, 0.0, 0.0),
        };
        for (r, v) in self.levels.iter_mut().zip([saw, square, sub, noise, tri, sine]) {
            r.to(v, jump);
        }
        self.width.to(width, jump);

        // Velocity: the level follows v^(2 × vel_amount), so 0 is silent and, at 0.5, a note at
        // 0.3 is 10 dB down; the filter envelope follows the square root of that.
        let sens = clampf(at(VEL_AMOUNT), 0.0, 1.0);
        if (self.velocity, sens) != self.bright_for {
            self.bright_for = (self.velocity, sens);
            self.bright = self.velocity.powf(sens);
        }
        let bright = self.bright;
        // Filter: the cutoff follows the key and the envelope (scaled by velocity).
        self.fenv.set(at(F_ATTACK), at(F_DECAY), at(F_SUSTAIN), at(RELEASE), rate);
        let fe = self.fenv.next();
        let octaves = keytrack * (pitch - 60.0) / 12.0 + env * fe * bright;
        let fc = clampf(cutoff * clampf(octaves, -20.0, 20.0).exp2(), 20.0, CUTOFF_MAX * self.sr);
        self.cutoff.to(self.memo[1].get(fc, |fc| one_pole(fc, 2.0 * sr)), jump);
        self.k.to(feedback(resonance), jump);

        // Drive, off at exactly 0 dB, so the default is clean.
        if drive != self.drive_for {
            self.drive_for = drive;
            self.drive = Drive::new(drive);
        }
        self.drive_on = drive > 0.0;
        self.drive_in.to(self.drive.into, jump || !self.drive_on);
        self.drive_out.to(self.drive.out, jump || !self.drive_on);

        self.amp.set(at(ATTACK), at(DECAY), at(SUSTAIN), at(RELEASE), self.sr);
        if jump {
            self.vel_gain = Glide::new(0.005, rate);
        }
        let vel = self.vel_gain.next(bright * bright);
        self.out.to(OUT * self.memo[2].get(level, db_to_gain) * vel, jump);

        // A mono stack that gets spread turns stereo: the right side starts as a copy of the left.
        if !self.stereo && n > 1 && spread > 0.0 {
            self.stereo = true;
            self.bus.copy_left();
            self.clip[1] = self.clip[0];
            self.half[1] = self.half[0];
            self.ladder[1] = self.ladder[0];
            self.dc[1] = self.dc[0];
        }
    }

    /// Centre and side gains crossfade with detune (Szabo's mix curve), normalised so the
    /// stack is as loud as one voice. Spread pans the side voices out (the inner ones a
    /// little less) and lets the centre voice give way to them, so a spread stack is wide
    /// rather than a mono voice with a halo.
    fn set_gains(&mut self, detune: f32, spread: f32) {
        let n = self.voices;
        let (centre, side) = if n % 2 == 1 {
            (if n > 1 { 1.0 - 0.6 * spread } else { 1.0 } * (0.99785 - 0.55366 * detune), 0.044372 + detune * (1.2841 - 0.73764 * detune))
        } else {
            (1.0, 1.0)
        };
        let g = |k: usize| if n % 2 == 1 && k == n / 2 { centre } else { side };
        let norm = 1.0 / (0..n).map(|k| g(k) * g(k)).sum::<f32>().sqrt();
        self.gain_sum = [0.0; 2];
        for (k, &at) in PANS[n - 1].iter().enumerate().take(n) {
            let p = pan(if at == 0.0 { 0.0 } else { spread * at.signum() * (0.85 + 0.15 * at.abs()) });
            self.gains[k] = [g(k) * norm * p[0], g(k) * norm * p[1]];
            self.gain_sum[0] += self.gains[k][0];
            self.gain_sum[1] += self.gains[k][1];
        }
    }

    /// One output sample of the voice before the amp envelope.
    fn sample(&mut self) -> [f32; 2] {
        if self.stereo {
            self.sample_in::<2>()
        } else {
            let [l, _] = self.sample_in::<1>();
            [l, l]
        }
    }

    /// One output sample on `CH` channels: a mono voice works out only the left.
    fn sample_in<const CH: usize>(&mut self) -> [f32; 2] {
        let n = self.voices;
        let [saw, square, sub, noise, tri, sine] = self.levels.each_mut().map(|r| r.next());
        // A triangle or a sine on the stack: neither steps, so neither needs a band-limited step,
        // and both are centred, so neither adds to `dc`. Off (the saw), not a bit changes.
        let shaped = tri != 0.0 || sine != 0.0;
        let w = self.width.next();
        let (saw2, sq2) = (2.0 * saw, 2.0 * square);
        let pulse = square != 0.0;
        for d in &mut self.dt[..n] {
            d.next();
        }
        let sub_dt = self.sub_dt.next();
        // The saw is 2t − 1 and the pulse (t < w) × 2 − 2w: the constant parts go in once.
        let dc = saw + sq2 * w;
        let mut x = [[0.0f32; 2]; 2];
        for xs in &mut x {
            let mut acc = [0.0f32; 2];
            for k in 0..n {
                let dt = self.dt[k].value;
                let gain = self.gains[k];
                let steps = |h: f32| [h * gain[0], h * gain[1]];
                let mut t = self.phase[k] + dt;
                if pulse {
                    // The pulse falls where the phase passes the width, and rises at the wrap.
                    if self.high[k] && t >= w {
                        self.bus.step::<CH>((t - w) / dt, steps(-sq2));
                        self.high[k] = false;
                    }
                    if t >= 1.0 {
                        t -= 1.0;
                        self.bus.step::<CH>(t / dt, steps(sq2 - saw2));
                        self.high[k] = true;
                        if t >= w {
                            self.bus.step::<CH>((t - w) / dt, steps(-sq2));
                            self.high[k] = false;
                        }
                    }
                    // The width moved past the phase: a step at mid-sample keeps the two in line.
                    if !self.high[k] && t < w {
                        self.bus.step::<CH>(0.5, steps(sq2));
                        self.high[k] = true;
                    }
                } else if t >= 1.0 {
                    t -= 1.0;
                    self.bus.step::<CH>(t / dt, steps(-saw2));
                    self.high[k] = true;
                }
                self.phase[k] = t;
                let mut v = saw2 * t + if self.high[k] && pulse { sq2 } else { 0.0 };
                if shaped {
                    v += tri * (1.0 - 4.0 * (t - 0.5).abs()) + sine * sin2pi(t);
                }
                for (a, g) in acc.iter_mut().zip(gain).take(CH) {
                    *a += g * v;
                }
            }
            let mut extra = 0.0;
            if sub != 0.0 {
                extra += sub * sin2pi(self.sub_phase);
            }
            self.sub_phase = (self.sub_phase + sub_dt).fract();
            if noise != 0.0 {
                extra += noise * self.noise_gain * self.noise.next();
            }
            for (a, g) in acc.iter_mut().zip(self.gain_sum).take(CH) {
                *a += extra - dc * g;
            }
            self.bus.add::<CH>(acc);
            *xs = self.bus.next::<CH>();
        }
        let (into, out) = (self.drive_in.next(), self.drive_out.next());
        let coefs = LadderCoefs::new(self.cutoff.next(), self.k.next());
        let mut y = [0.0; 2];
        for c in 0..CH {
            let (mut a, mut b) = (x[0][c], x[1][c]);
            if self.drive_on {
                a = self.clip[c].tick(a * into) * out;
                b = self.clip[c].tick(b * into) * out;
            }
            let ladder = &mut self.ladder[c];
            let (a, b) = (ladder.tick(a, &coefs), ladder.tick(b, &coefs));
            let h = self.half[c].tick(a, b);
            let [d0, d1] = &mut self.dc[c];
            y[c] = d1.tick(d0.tick(h, self.dc_pole), self.dc_pole);
        }
        y
    }
}

impl Kernel for Synth {
    const INPUTS: usize = INPUTS;
    const OUTPUTS: usize = 1;

    fn new(cfg: &Cfg) -> Self {
        let sample_rate = cfg.sample_rate;
        let sr = sample_rate.max(1.0);
        let rate = sr / TICK as f32;
        Synth {
            sr,
            gate: false,
            clock: 0,
            jump: true,
            noise_gain: (sr / 48_000.0).sqrt(),
            since: 0,
            velocity: 0.0,
            smooth: std::array::from_fn(|_| Glide::new(SMOOTH, rate)),
            vel_gain: Glide::new(0.005, rate),
            bright: 0.0,
            bright_for: (f32::NAN, f32::NAN),
            amp: Env::new(),
            fenv: Env::new(),
            voices: 1,
            sub_octave: -1.0,
            stereo: false,
            phase: [0.0; UNISON_MAX],
            high: [true; UNISON_MAX],
            dt: [Ramp::default(); UNISON_MAX],
            gains: [[0.0; 2]; UNISON_MAX],
            gain_sum: [0.0; 2],
            gains_for: (f32::NAN, f32::NAN),
            ratios: [1.0; UNISON_MAX],
            ratios_for: f32::NAN,
            memo: [Memo::new(); 3],
            sub_phase: 0.0,
            sub_dt: Ramp::default(),
            vib_phase: 0.0,
            noise: Noise::new(1),
            levels: [Ramp::default(); 6],
            width: Ramp::default(),
            bus: Steps::default(),
            drive_on: false,
            drive: Drive::new(0.0),
            drive_for: f32::NAN,
            drive_in: Ramp::default(),
            drive_out: Ramp::default(),
            clip: [Clip::default(); 2],
            half: [Halfband::default(); 2],
            cutoff: Ramp::default(),
            k: Ramp::default(),
            ladder: [Ladder::default(); 2],
            dc: [[DcBlock::default(); 2]; 2],
            dc_pole: DcBlock::pole(sr),
            out: Ramp::default(),
        }
    }

    fn process(&mut self, io: &mut Io) {
        let (ins, [ol, or]) = io.ports(std::array::from_fn::<_, INPUTS, _>(|p| (p, 0)), [(0, 0), (0, 1)]);
        for i in 0..ol.len() {
            let at = |p: usize| ins[p][i];
            let gate = at(GATE) > 0.5;
            if gate != self.gate {
                self.gate = gate;
                if gate {
                    self.start(&at);
                } else {
                    self.amp.off();
                    self.fenv.off();
                }
            }
            // Silent and not playing: nothing to do (a mono voice idles at the top level).
            if self.amp.stage == Stage::Idle {
                ol[i] = 0.0;
                or[i] = 0.0;
                continue;
            }
            if self.clock == 0 {
                self.tick(&at);
            }
            self.clock = (self.clock + 1) % TICK;
            let a = self.amp.next();
            if a == 0.0 {
                ol[i] = 0.0;
                or[i] = 0.0;
                continue;
            }
            let [l, r] = self.sample();
            let g = a * self.out.next();
            ol[i] = l * g;
            or[i] = r * g;
        }
    }

    /// Sound that started can still come out only in the release.
    fn active(&self) -> bool {
        self.amp.stage == Stage::Release
    }
}

export!(synth, Synth);

#[cfg(test)]
mod tests;
