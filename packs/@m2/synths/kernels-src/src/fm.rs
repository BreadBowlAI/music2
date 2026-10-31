//! `fm`, the FM voice behind @m2/synths' FM keys, bells and metal: two operator pairs, each a sine
//! modulator into a sine carrier, mixed, at 2× and decimated, so a bright patch's upper
//! sidebands fold into the decimator's stopband instead of back into the audio. Port order
//! must match the pack's `m2pkg.toml`.

use m2_dsp::voice::{DcBlock, Env, Glide, Halfband, Memo, Ramp, Stage, TICK, clampf, hash, hz, pitch_of, sin2pi};
use m2_kernel::{Cfg, Io, Kernel, db_to_gain, export};

// Inputs, in manifest order.
const PITCH: usize = 0;
const GATE: usize = 1;
const VELOCITY: usize = 2;
const RATIO_A: usize = 3;
const MOD_A: usize = 4;
const INDEX_A: usize = 5;
const FALL_A: usize = 6;
const RATIO_B: usize = 7;
const MOD_B: usize = 8;
const INDEX_B: usize = 9;
const FALL_B: usize = 10;
const MIX_B: usize = 11;
const FEEDBACK: usize = 12;
const DETUNE: usize = 13;
const TONE: usize = 14;
const ATTACK: usize = 15;
const DECAY: usize = 16;
const SUSTAIN: usize = 17;
const RELEASE: usize = 18;
const VEL_AMOUNT: usize = 19;
const LEVEL: usize = 20;
const DROP: usize = 21;
const DROP_TIME: usize = 22;
const INPUTS: usize = 23;

/// Where each modulator's index falls to, as a fraction of where it starts.
const FLOOR: [f32; 2] = [0.1, 0.0];
/// Modulator A's feedback at `feedback` = 1, in cycles of phase (about 1.6 radians, where
/// its sine has turned into a bright saw).
const FEEDBACK_MAX: f32 = 0.25;
/// Output gain: one note at velocity 0.8 peaks near -14 dBFS.
const OUT: f32 = 0.28;
/// Where the DC blockers cut: higher than the synth's, since a feedback carrier's DC part
/// moves as its index falls.
const DC_CUT: f32 = 10.0;
/// A pair whose sidebands would reach past this many times the sample rate is tamed. At 2×,
/// whatever passes the inner Nyquist (1×) folds back above it, into the decimator's stopband
/// (above 0.54×), as long as it started below about 1.46×.
const BAND_EDGE: f32 = 1.4;
/// Sidebands counted past the index: whatever lies beyond the fourth is too weak to hear
/// when it folds back.
const SIDEBANDS: f32 = 4.0;

/// The largest index (in radians) that keeps a pair's sidebands, out to (I + 4) × modulator
/// + carrier, under the band edge.
fn index_limit(carrier: f32, modulator: f32, sr: f32) -> f32 {
    (BAND_EDGE * sr - carrier) / modulator.max(1.0) - SIDEBANDS
}

/// `x` below `limit`, bending smoothly into it rather than clipping.
fn soft_min(x: f32, limit: f32) -> f32 {
    if limit <= 0.0 {
        return 0.0;
    }
    let r = x / limit;
    x / (1.0 + r * r * r * r).sqrt().sqrt()
}

/// A carrier's gain, faded out as it nears Nyquist.
fn carrier_gain(f: f32, sr: f32) -> f32 {
    clampf((0.45 * sr - f) / (0.05 * sr), 0.0, 1.0)
}

/// The continuous controls, smoothed at the tick rate.
const SMOOTHED: [usize; 4] = [MIX_B, FEEDBACK, TONE, LEVEL];

/// Inputs: pitch, gate, velocity, then the patch (see the manifest). Output: mono.
pub struct Fm {
    sr: f32,
    gate: bool,
    clock: u32,
    jump: bool,
    since: u32,
    velocity: f32,
    smooth: [Glide; SMOOTHED.len()],
    amp: Env,
    /// Two DC blockers in a row: feedback gives a carrier at the modulator's frequency a DC
    /// part, which drifts as the index falls.
    dc: [DcBlock; 2],
    half: Halfband,
    dc_pole: f32,
    /// Phases of carrier A, modulator A, carrier B, modulator B, stepping at 2×.
    phase: [f32; 4],
    dt: [Ramp; 4],
    index: [Ramp; 2],
    carrier: [Ramp; 2],
    feedback: Ramp,
    /// Modulator A's last two outputs (at 2×).
    last: [f32; 2],
    /// velocity^(2 × vel_amount), and the two it was worked out for.
    loud: f32,
    loud_for: (f32, f32),
    /// The gain for a level.
    level: Memo,
    out: Ramp,
}

impl Fm {
    fn tick(&mut self, at: &dyn Fn(usize) -> f32) {
        let sr = self.sr;
        let jump = std::mem::take(&mut self.jump);
        for d in &mut self.dc {
            d.flush();
        }
        self.half.flush();
        if jump {
            self.smooth = std::array::from_fn(|_| Glide::new(0.01, sr / TICK as f32));
        }
        let [mix, feedback, tone, level] = [0, 1, 2, 3].map(|i| {
            let (lo, hi) = if SMOOTHED[i] == LEVEL { (-60.0, 12.0) } else { (0.0, 1.0) };
            self.smooth[i].next(clampf(at(SMOOTHED[i]), lo, hi))
        });
        // A legato note (one at a time) brings its own velocity.
        if self.gate {
            self.velocity = clampf(at(VELOCITY), 0.0, 1.0);
        }
        let pitch = pitch_of(at(PITCH));
        let t = self.since as f32 / sr;
        self.since = self.since.saturating_add(TICK);
        // A drop into the note, for zaps and metal hits.
        let drop = clampf(at(DROP), 0.0, 48.0);
        let bend = if drop > 0.0 { drop * (-3.0 * t / clampf(at(DROP_TIME), 0.001, 10.0)).exp() } else { 0.0 };
        let f = hz(pitch + bend);
        let above = pitch - 60.0;
        // Higher notes decay sooner and are less bright, as on a piano.
        let shorter = clampf((-above / 24.0).exp2(), 0.125, 8.0);
        let darker = (-above.max(0.0) / 36.0).exp2();
        let vel = clampf(at(VEL_AMOUNT), 0.0, 1.0);
        // Velocity opens the index more than the level: harder is brighter.
        let bright = (1.0 - vel + vel * (self.velocity / 0.8) * (self.velocity / 0.8)) * ((tone - 0.5) * 4.0).exp2() * darker;
        let cents = clampf(at(DETUNE), 0.0, 100.0);
        let freqs = [
            clampf(at(RATIO_A), 0.0, 64.0) * f,
            clampf(at(MOD_A), 0.0, 64.0) * f,
            clampf(at(RATIO_B), 0.0, 64.0) * f * (cents / 1200.0).exp2(),
            clampf(at(MOD_B), 0.0, 64.0) * f,
        ];
        for (d, &hz) in self.dt.iter_mut().zip(&freqs) {
            d.to(clampf(hz, 0.0, sr) / (2.0 * sr), jump);
        }
        for k in 0..2 {
            let (index, fall) = if k == 0 { (at(INDEX_A), at(FALL_A)) } else { (at(INDEX_B), at(FALL_B)) };
            let fall = clampf(fall, 0.001, 60.0) * shorter;
            let env = FLOOR[k] + (1.0 - FLOOR[k]) * (-3.0 * t / fall).exp();
            let i = clampf(index, 0.0, 32.0) * env * bright;
            self.index[k].to(soft_min(i, index_limit(freqs[2 * k], freqs[2 * k + 1], sr)), jump);
            self.carrier[k].to(carrier_gain(freqs[2 * k], sr) * if k == 0 { 1.0 - mix } else { mix }, jump);
        }
        // Feedback makes a saw of modulator A: tame it as the saw's top nears Nyquist.
        let fb = feedback * FEEDBACK_MAX * clampf(index_limit(0.0, freqs[1], sr) / 8.0, 0.0, 1.0);
        self.feedback.to(fb, jump);
        self.amp.set(at(ATTACK), clampf(at(DECAY), 0.001, 60.0) * shorter, at(SUSTAIN), at(RELEASE), sr);
        // The level follows v^(2 × vel_amount): 0 is silent, and at 0.5 a note at 0.3 is 10 dB down.
        if (self.velocity, vel) != self.loud_for {
            self.loud_for = (self.velocity, vel);
            self.loud = self.velocity.powf(2.0 * vel);
        }
        self.out.to(OUT * self.level.get(level, db_to_gain) * self.loud, jump);
    }
}

impl Kernel for Fm {
    const INPUTS: usize = INPUTS;
    const OUTPUTS: usize = 1;

    fn new(cfg: &Cfg) -> Self {
        let sample_rate = cfg.sample_rate;
        let sr = sample_rate.max(1.0);
        Fm {
            sr,
            gate: false,
            clock: 0,
            jump: true,
            since: 0,
            velocity: 0.0,
            smooth: std::array::from_fn(|_| Glide::new(0.01, sr / TICK as f32)),
            amp: Env::new(),
            dc: [DcBlock::default(); 2],
            half: Halfband::default(),
            dc_pole: (-2.0 * std::f32::consts::PI * DC_CUT / sr).exp(),
            phase: [0.0; 4],
            dt: [Ramp::default(); 4],
            index: [Ramp::default(); 2],
            carrier: [Ramp::default(); 2],
            feedback: Ramp::default(),
            last: [0.0; 2],
            loud: 0.0,
            loud_for: (f32::NAN, f32::NAN),
            level: Memo::new(),
            out: Ramp::default(),
        }
    }

    fn process(&mut self, io: &mut Io) {
        let (ins, [out]) = io.ports(std::array::from_fn::<_, INPUTS, _>(|p| (p, 0)), [(0, 0)]);
        for i in 0..out.len() {
            let at = |p: usize| ins[p][i];
            let gate = at(GATE) > 0.5;
            if gate != self.gate {
                self.gate = gate;
                if gate {
                    let fresh = self.amp.value == 0.0;
                    if fresh {
                        // Every operator starts where it would be a fraction of a period into the
                        // note, the fraction set by the pitch: the same sound (a carrier's phase
                        // against its modulator's shapes the timbre), the same every time, but a
                        // chord's notes don't all start on a peak together. The attack starts the
                        // note from silence.
                        let u = (hash((pitch_of(at(PITCH)) * 64.0) as i32 as u32, 7) >> 8) as f32 / (1u32 << 24) as f32;
                        self.phase = [RATIO_A, MOD_A, RATIO_B, MOD_B].map(|p| (clampf(at(p), 0.0, 64.0) * u).fract());
                        self.last = [0.0; 2];
                        self.dc = [DcBlock::default(); 2];
                        self.half = Halfband::default();
                    }
                    self.velocity = clampf(at(VELOCITY), 0.0, 1.0);
                    self.since = 0;
                    self.clock = 0;
                    self.jump = fresh;
                    self.amp.on();
                } else {
                    self.amp.off();
                }
            }
            if self.amp.stage == Stage::Idle {
                out[i] = 0.0;
                continue;
            }
            if self.clock == 0 {
                self.tick(&at);
            }
            self.clock = (self.clock + 1) % TICK;
            let a = self.amp.next();
            if a == 0.0 {
                out[i] = 0.0;
                continue;
            }
            // An index in radians moves the carrier's phase by index / 2π cycles.
            let [ia, ib] = self.index.each_mut().map(|r| r.next() * std::f32::consts::FRAC_1_PI * 0.5);
            let [ga, gb] = self.carrier.each_mut().map(|r| r.next());
            let fb = self.feedback.next();
            let dt = self.dt.each_mut().map(|d| d.next());
            let mut two = [0.0; 2];
            for x in &mut two {
                // DX-style feedback, averaged over two samples so it doesn't hunt.
                let ma = sin2pi(self.phase[1] + fb * (self.last[0] + self.last[1]) * 0.5);
                self.last = [ma, self.last[0]];
                let mb = sin2pi(self.phase[3]);
                let ca = sin2pi(self.phase[0] + ia * ma);
                let cb = sin2pi(self.phase[2] + ib * mb);
                for (p, d) in self.phase.iter_mut().zip(dt) {
                    *p = (*p + d).fract();
                }
                *x = ga * ca + gb * cb;
            }
            let h = self.half.tick(two[0], two[1]);
            let [d0, d1] = &mut self.dc;
            let x = d1.tick(d0.tick(h, self.dc_pole), self.dc_pole);
            out[i] = x * a * self.out.next();
        }
    }

    fn active(&self) -> bool {
        self.amp.stage == Stage::Release
    }
}

export!(fm, Fm);

#[cfg(test)]
mod tests;
