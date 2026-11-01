//! `drum`, the voice behind @m2/drums: an 808/909-style machine in one kernel that picks
//! its drum when the note starts, either from the note's General MIDI pitch or from a
//! fixed `model` (`m2_dsp::kit`). `one`, the Drum's, is the same voice with its drum a menu (a
//! choice, read when a hit starts, its place read by [`SOUNDS`]) where the kit has `model`, cut by
//! any hit on its lane when its drum chokes. Port order must match the pack's `m2pkg.toml`.
//!
//! Every drum is a one-shot: it reads its settings on its first sample, starts from exactly
//! 0, and ends in exact silence at -90 dB. A hit in its choke group (from `drumhit`) fades
//! it out in a few milliseconds instead. Each hit's noise, and the metal's phases, are
//! seeded from its drum, its key and how many hits its lane has played (`hitcount`, read on
//! its first sample), so no two hits are quite the same (as on an analog machine) while
//! every render is, and so is live playback from 0, however long the engine sat stopped.

use m2_dsp::kit::{Kind, kind_of};
use m2_dsp::voice::{Clip, DcBlock, Decay, Drive, Noise, Square, Steps, Svf, T60, clampf, hash, pitch_of, sin2pi, soft};
use m2_kernel::{Cfg, Io, Kernel, db_to_gain, export};

// Inputs, in manifest order.
const PITCH: usize = 0;
const VELOCITY: usize = 1;
const HITS: usize = 2;
const COUNT: usize = 3;
const MODEL: usize = 4;
const KICK_TUNE: usize = 5;
const KICK_DECAY: usize = 6;
const PUNCH: usize = 7;
const SNARE_TUNE: usize = 8;
const SNAP: usize = 9;
const SNARE_DECAY: usize = 10;
const CLAP_TONE: usize = 11;
const CLAP_DECAY: usize = 12;
const HAT_TONE: usize = 13;
const HAT_DECAY: usize = 14;
const OPEN_DECAY: usize = 15;
const TOM_TUNE: usize = 16;
const TOM_DECAY: usize = 17;
const RIM_TONE: usize = 18;
const BELL_TUNE: usize = 19;
const CYMBAL_DECAY: usize = 20;
const SHAKE_DECAY: usize = 21;
const DRIVE: usize = 22;
const KICK_LEVEL: usize = 23;
const SNARE_LEVEL: usize = 24;
const HAT_LEVEL: usize = 25;
const TOM_LEVEL: usize = 26;
const PERC_LEVEL: usize = 27;
// The Drum's own (`drums.drum`), which move whatever drum it plays; the kit leaves them at their
// defaults, where they change no bit.
const TUNE: usize = 28;
const DECAY: usize = 29;
const PUSH: usize = 30;
const INPUTS: usize = 31;

/// What a place of the Drum's menu plays (KIT-20): one of the drum voice's own models (0 to 8),
/// or the drum the drum machine plays at a General MIDI note, at any note.
#[derive(Clone, Copy)]
enum Plays {
    Model(f32),
    Note(f32),
}

/// The Drum's menu, by place, grouped by family as `@m2/drums` declares it (`DRUMS`): kick, kick 2,
/// snare, snare 2, clap, hats, tom, rim, claves, cowbell, cymbal, cymbal 2, ride, ride 2, shaker,
/// shaker 2, tambourine. A song writes a sound by its name: its place reads it here.
const SOUNDS: [Plays; 17] = [
    Plays::Model(0.0),
    Plays::Note(35.0),
    Plays::Model(1.0),
    Plays::Note(40.0),
    Plays::Model(2.0),
    Plays::Model(3.0),
    Plays::Model(4.0),
    Plays::Model(5.0),
    Plays::Note(75.0),
    Plays::Model(6.0),
    Plays::Model(7.0),
    Plays::Note(57.0),
    Plays::Note(51.0),
    Plays::Note(59.0),
    Plays::Model(8.0),
    Plays::Note(70.0),
    Plays::Note(54.0),
];

/// What a note at `pitch` plays on the Drum whose menu is at `place` ([`SOUNDS`]); a place off the
/// menu as the drum machine's map reads it (NaN and below 0 its General MIDI kit, past it nothing).
fn sound_at(place: f32, pitch: f32) -> (Kind, f32, bool) {
    let at = place.round();
    match SOUNDS.get(at as usize).filter(|_| at >= 0.0) {
        Some(Plays::Model(m)) => kind_of(*m, pitch),
        Some(Plays::Note(n)) => kind_of(-1.0, *n),
        None if at >= 0.0 => (Kind::Silent, 0.0, false),
        None => kind_of(place, pitch),
    }
}

/// The input that sets drum `kind`'s level in the kit, by group: kick; snare, clap and rim; hats;
/// toms; cymbals and percussion.
fn level(kind: Kind) -> usize {
    match kind {
        Kind::Kick => KICK_LEVEL,
        Kind::Snare | Kind::Clap | Kind::Rim => SNARE_LEVEL,
        Kind::ClosedHat | Kind::PedalHat | Kind::OpenHat => HAT_LEVEL,
        Kind::Tom => TOM_LEVEL,
        _ => PERC_LEVEL,
    }
}

/// The 808's six metal oscillators, in Hz: square waves whose sum rings like a cymbal.
const METAL: [f32; 6] = [205.3, 304.4, 369.6, 522.7, 540.0, 800.0];

/// Each drum's gain, so that at velocity 0.8 its peak lands where a mix wants it (kick
/// near -6 dBFS, snare and clap -9, toms -9, rim and cowbell -12, hats -16, cymbals -18,
/// shaker -20). The kit's level knobs move a group from there.
const KICK_GAIN: f32 = 0.6;
const SNARE_GAIN: f32 = 0.43;
const CLAP_GAIN: f32 = 0.96;
const HAT_GAIN: f32 = 1.75;
const CYMBAL_GAIN: f32 = 0.67;
const TOM_GAIN: f32 = 0.47;
const RIM_GAIN: f32 = 3.8;
const CLAVES_GAIN: f32 = 13.5;
const BELL_GAIN: f32 = 0.32;
const SHAKER_GAIN: f32 = 0.4;

/// How long a choke takes to fade a drum out.
const CHOKE: f32 = 0.004;
const KICK_CHOKE: f32 = 0.003;

/// A burst of a clap: a linear rise over `rise` samples from `start`, then a decay.
#[derive(Clone, Copy)]
struct Burst {
    start: u32,
    rise: u32,
    decay: Decay,
}

impl Burst {
    fn at(&mut self, t: u32) -> f32 {
        if t < self.start {
            0.0
        } else if t - self.start < self.rise {
            self.decay.value * (t - self.start) as f32 / self.rise as f32
        } else {
            self.decay.next()
        }
    }
}

/// The sound of each drum, and the state it needs.
// `Metal` is the largest variant. Boxing it would allocate at a hit, on the audio thread; held in
// place, each `Drum` takes its room once, and a hit only writes a new sound over the old one.
#[allow(clippy::large_enum_variant)]
enum Sound {
    Silent,
    /// A sine swept down into its note, with a click on top.
    Kick {
        phase: f32,
        tune: f32,
        sweep: f32,
        drop: f32,
        drop_coef: f32,
        amp: Decay,
        click: Svf,
        click_len: u32,
        click_gain: f32,
        push: f32,
        /// A decaying sine that starts at phase 0 leaves a net offset (as do the snare's and
        /// the tom's): a 5 Hz highpass takes it out.
        dc: DcBlock,
    },
    /// Two swept sine modes and a filtered noise burst.
    Snare {
        phase: [f32; 2],
        freq: [f32; 2],
        bend: f32,
        bend_coef: f32,
        modes: [Decay; 2],
        body: f32,
        noise: Decay,
        hp: Svf,
        lp: Svf,
        rattle: f32,
        dc: DcBlock,
    },
    /// Band-passed noise in three quick bursts, then a longer one.
    Clap {
        bp: Svf,
        bursts: [Burst; 4],
    },
    /// The metal through a bandpass and a highpass (hats), or through two bands (cymbals).
    Metal {
        squares: [Square; 6],
        dt: [f32; 6],
        bus: Steps,
        bands: [Svf; 2],
        mix: [f32; 2],
        envs: [Decay; 2],
        hp: Svf,
        hold: u32,
        air: f32,
    },
    /// A sine with a small pitch drop and a stick noise.
    Tom {
        phase: f32,
        freq: f32,
        bend: f32,
        bend_coef: f32,
        amp: Decay,
        stick: Decay,
        bp: Svf,
        dc: DcBlock,
    },
    /// A short pulse into tuned resonators, highpassed and clipped.
    Rim {
        res: [Svf; 2],
        mix: [f32; 2],
        pulse: u32,
        hp: Svf,
        life: Decay,
    },
    /// Two squares through a broad bandpass, with a fast and a slow decay.
    Cowbell {
        squares: [Square; 2],
        dt: [f32; 2],
        bus: Steps,
        bp: Svf,
        envs: [Decay; 2],
    },
    /// Noise through a highpass and a bandpass.
    Shaker {
        hp: Svf,
        bp: Svf,
        amp: Decay,
    },
}

/// One drum hit. Inputs: pitch, velocity, hits (from drumhit), count (from hitcount),
/// model, then each drum's settings (see the manifest). Output: mono.
pub struct Drum {
    sr: f32,
    /// Samples since the hit.
    t: u32,
    kind: Kind,
    sound: Sound,
    /// The hit's seed, for its noise and its metal.
    seed: u32,
    noise: Noise,
    /// White noise's gain, so its level in hertz (what the filters pass) is the same at
    /// every sample rate.
    noise_gain: f32,
    gain: f32,
    /// Samples of the fade-in that keeps a hit's first sample at exactly 0.
    rise: u32,
    drive: Option<(Drive, Clip)>,
    /// Choke fade: where it is (0..1) and how far it moves each sample; 0 step = not choked.
    fade: f32,
    fade_step: f32,
    choke_len: f32,
    done: bool,
    dc_pole: f32,
    /// One drum's (`one`): every hit on its lane is of its drum, so any hit chokes a drum that chokes.
    one: bool,
    /// How much harder (or softer) the hit's first `boost_len` samples are: One drum's `punch` on a
    /// drum other than the kick and the snare. 0, as the kit leaves it: no bit changes.
    boost: f32,
    boost_len: u32,
}

impl Drum {
    fn start(&mut self, at: &dyn Fn(usize) -> f32) {
        let sr = self.sr;
        let (kind, semis, alt) = if self.one { sound_at(at(MODEL), at(PITCH)) } else { kind_of(at(MODEL), at(PITCH)) };
        self.kind = kind;
        // One drum's knobs: its pitch moved by `tune` semitones, its decays `decay` times as long
        // (the rim's ring and the cowbell's too), and `push` added to a kick's punch or a snare's
        // snap, or on any other drum, how much harder its first 15 ms are. At 0, 1 and 0 (the
        // kit's) each is left out, so no bit changes.
        let shift = if at(TUNE).is_finite() { clampf(at(TUNE), -48.0, 48.0) } else { 0.0 };
        let semis = if shift != 0.0 { semis + shift } else { semis };
        let ratio = if shift != 0.0 { (shift / 12.0).exp2() } else { 1.0 };
        let length = if at(DECAY).is_finite() { clampf(at(DECAY), 0.05, 20.0) } else { 1.0 };
        let longer = |t: f32| if length != 1.0 { t * length } else { t };
        let push = if at(PUSH).is_finite() { clampf(at(PUSH), -1.0, 1.0) } else { 0.0 };
        let pushed = |x: f32| if push != 0.0 { x + push } else { x };
        // A drum's own frequency, `ratio` times higher, inside its range.
        let moved = |f: f32, lo: f32, hi: f32| if ratio != 1.0 { clampf(f * ratio, lo, hi) } else { f };
        let velocity = clampf(at(VELOCITY), 0.0, 1.0);
        // About -2 dB at 0.8, -12 dB at 0.3, then the kit's level for the drum's group.
        self.gain = velocity.powf(1.1) * db_to_gain(clampf(at(level(kind)), -60.0, 12.0));
        // A seed of the hit's own: the same in every render, but not for every hit. The count
        // is of hits, never of samples, so it doesn't follow how long the engine has run (a
        // float to int cast saturates: NaN and negatives are 0).
        let key = (pitch_of(at(PITCH)) * 64.0) as i32 as u32;
        self.seed = hash(hash(kind as u32, key), at(COUNT) as u32);
        self.noise = Noise::new(self.seed);
        let drive = clampf(at(DRIVE), 0.0, 24.0);
        self.drive = (drive > 0.0).then(|| (Drive::new(drive), Clip::default()));
        let ms = |x: f32| ((x * 0.001 * sr) as u32).max(1);
        let tune = |p: usize, lo: f32, hi: f32| clampf(at(p), lo, hi) * (semis / 12.0).exp2();
        let time = |p: usize, lo: f32, hi: f32| longer(clampf(at(p), lo, hi));
        self.rise = ms(0.3);
        self.choke_len = CHOKE;
        let (boost, boost_len) = if push != 0.0 && !matches!(kind, Kind::Kick | Kind::Snare) { (push, ms(15.0)) } else { (0.0, 1) };
        (self.boost, self.boost_len) = (boost, boost_len);
        self.sound = match kind {
            Kind::Silent => Sound::Silent,
            Kind::Kick => {
                let punch = clampf(pushed(at(PUNCH)), 0.0, 1.0) * if alt { 0.6 } else { 1.0 };
                self.gain *= KICK_GAIN;
                self.choke_len = KICK_CHOKE;
                let push = 1.0 + 2.0 * punch;
                Sound::Kick {
                    phase: 0.0,
                    tune: tune(KICK_TUNE, 20.0, 200.0),
                    // 808 at punch 0 (the pitch starts an octave up), 909 at 1 (from 6×, faster).
                    sweep: 1.0 + 4.0 * punch,
                    drop: 1.0,
                    drop_coef: (-1.0 / ((0.020 - 0.012 * punch) * sr)).exp(),
                    amp: Decay::new(time(KICK_DECAY, 0.05, 5.0), sr),
                    click: Svf::highpass(sr, 1_500.0, 0.7),
                    click_len: ms(1.5),
                    click_gain: 0.3 * punch,
                    push,
                    dc: DcBlock::default(),
                }
            }
            Kind::Snare => {
                let snap = clampf(pushed(at(SNAP) + if alt { 0.2 } else { 0.0 }), 0.0, 1.0);
                let decay = time(SNARE_DECAY, 0.02, 2.0);
                let f = tune(SNARE_TUNE, 60.0, 1_000.0);
                self.gain *= SNARE_GAIN;
                Sound::Snare {
                    phase: [0.0; 2],
                    freq: [f, 1.8 * f],
                    bend: 0.15,
                    bend_coef: (-1.0 / (0.005 * sr)).exp(),
                    modes: [Decay::new(0.9 * decay, sr), Decay::new(0.55 * decay, sr)],
                    body: 1.0 - 0.55 * snap,
                    noise: Decay::new(decay, sr),
                    hp: Svf::highpass(sr, 1_000.0, 0.7),
                    // Harder hits are brighter.
                    lp: Svf::lowpass(sr, 2_500.0 + 9_000.0 * velocity, 0.7),
                    rattle: 0.2 + 0.9 * snap,
                    dc: DcBlock::default(),
                }
            }
            Kind::Clap => {
                let bp = Svf::bandpass(sr, moved(clampf(at(CLAP_TONE), 300.0, 8_000.0), 300.0, 8_000.0), 1.8);
                let short = |start: f32| Burst { start: ms(start), rise: ms(1.0), decay: Decay::new(0.017, sr) };
                self.gain *= CLAP_GAIN;
                self.rise = 1;
                Sound::Clap {
                    bp,
                    bursts: [
                        short(0.0),
                        short(10.0),
                        short(21.0),
                        Burst { start: ms(31.0), rise: ms(1.0), decay: Decay::new(time(CLAP_DECAY, 0.03, 3.0), sr) },
                    ],
                }
            }
            Kind::ClosedHat | Kind::PedalHat | Kind::OpenHat => {
                let tone = moved(clampf(at(HAT_TONE), 2_000.0, 16_000.0), 2_000.0, 16_000.0);
                let (decay, hold, level) = match kind {
                    Kind::ClosedHat => (time(HAT_DECAY, 0.005, 2.0), 0, 1.0),
                    Kind::PedalHat => (longer(0.025), 0, 1.0),
                    // Held open, it rings up louder than a closed hat.
                    _ => (time(OPEN_DECAY, 0.02, 5.0), ms(15.0), 0.55),
                };
                if kind == Kind::PedalHat {
                    self.rise = ms(2.0);
                }
                self.gain *= HAT_GAIN * level;
                self.metal(
                    1.0,
                    [Svf::bandpass(sr, tone * (0.85 + 0.25 * velocity), 1.2), Svf::bandpass(sr, tone, 1.0)],
                    [1.0, 0.0],
                    [decay, decay],
                    Svf::highpass(sr, 0.6 * tone, 0.7),
                    hold,
                )
            }
            Kind::Crash | Kind::Ride => {
                let decay = time(CYMBAL_DECAY, 0.1, 10.0);
                let ratio = (semis / 12.0).exp2();
                self.gain *= CYMBAL_GAIN;
                if kind == Kind::Crash {
                    self.rise = ms(1.0);
                    self.metal(
                        ratio,
                        [Svf::bandpass(sr, 3_400.0 * ratio, 1.5), Svf::bandpass(sr, 8_000.0 * ratio, 1.2)],
                        [0.5, 1.0],
                        [0.6 * decay, decay],
                        Svf::highpass(sr, 2_000.0, 0.7),
                        0,
                    )
                } else {
                    self.metal(
                        ratio,
                        [Svf::bandpass(sr, 5_000.0 * ratio, 2.0), Svf::bandpass(sr, 10_000.0 * ratio, 1.2)],
                        [0.7, 1.0],
                        [0.35 * decay, 0.7 * decay],
                        Svf::highpass(sr, 2_500.0, 0.7),
                        0,
                    )
                }
            }
            Kind::Tom => {
                let base = clampf(at(TOM_TUNE), 30.0, 600.0);
                let f = clampf(base * (semis / 12.0).exp2(), 20.0, 2_000.0);
                // Low toms ring longer.
                let decay = time(TOM_DECAY, 0.03, 5.0) * (base / f).sqrt();
                self.gain *= TOM_GAIN;
                Sound::Tom {
                    phase: 0.0,
                    freq: f,
                    bend: 0.25,
                    bend_coef: (-1.0 / (0.012 * sr)).exp(),
                    amp: Decay::new(decay, sr),
                    stick: Decay::from(0.15, 0.015, sr),
                    bp: Svf::bandpass(sr, 2.5 * f, 1.0),
                    dc: DcBlock::default(),
                }
            }
            Kind::Rim | Kind::Claves => {
                let (res, mix, life, level) = if kind == Kind::Rim {
                    // A click into resonators: `decay` lets them ring longer, and so narrower; a
                    // resonator's ring from a click is as loud as its band is wide, so the level
                    // follows the length, and the hit is as loud.
                    let f = moved(clampf(at(RIM_TONE), 300.0, 8_000.0), 300.0, 8_000.0);
                    let ring = longer(0.025);
                    ([resonator(sr, f, ring), resonator(sr, 0.27 * f, ring)], [1.0, 0.6], ring, longer(RIM_GAIN))
                } else {
                    let f = moved(2_500.0, 300.0, 8_000.0);
                    let ring = longer(0.06);
                    ([resonator(sr, f, ring), resonator(sr, f, ring)], [1.0, 0.0], ring, longer(CLAVES_GAIN))
                };
                self.gain *= level;
                Sound::Rim { res, mix, pulse: ms(0.25), hp: Svf::highpass(sr, 300.0, 0.7), life: Decay::new(life, sr) }
            }
            Kind::Cowbell => {
                let f = moved(clampf(at(BELL_TUNE), 100.0, 3_000.0), 100.0, 3_000.0);
                self.gain *= BELL_GAIN;
                Sound::Cowbell {
                    squares: free_squares(self.seed),
                    dt: [f / sr, 1.48 * f / sr],
                    bus: Steps::default(),
                    bp: Svf::bandpass(sr, 1.6 * f, 0.8),
                    envs: [Decay::from(0.7, longer(0.083), sr), Decay::from(0.3, longer(0.6), sr)],
                }
            }
            Kind::Shaker | Kind::Tambourine => {
                let decay = time(SHAKE_DECAY, 0.01, 2.0);
                let (hp, bp, decay, level) =
                    if kind == Kind::Shaker { (5_000.0, 9_000.0, decay, 1.0) } else { (7_000.0, 11_000.0, 2.5 * decay, 0.6) };
                let lift = (semis / 12.0).exp2() * (0.9 + 0.2 * velocity);
                self.gain *= SHAKER_GAIN * level;
                self.rise = ms(8.0);
                Sound::Shaker { hp: Svf::highpass(sr, hp, 0.7), bp: Svf::bandpass(sr, bp * lift, 1.0), amp: Decay::new(decay, sr) }
            }
        };
        if kind == Kind::Silent {
            self.done = true;
        }
    }

    /// The metal for hats and cymbals: six squares at `ratio` × the 808's, plus a little
    /// noise for air. The squares run free on a real machine, so each hit catches them at
    /// phases of its own.
    fn metal(&mut self, ratio: f32, bands: [Svf; 2], mix: [f32; 2], decays: [f32; 2], hp: Svf, hold: u32) -> Sound {
        let sr = self.sr;
        Sound::Metal {
            squares: free_squares(self.seed),
            dt: METAL.map(|f| f * ratio / sr),
            bus: Steps::default(),
            bands,
            mix,
            envs: decays.map(|d| Decay::new(d, sr)),
            hp,
            hold,
            air: 0.1,
        }
    }

    /// The next sample of the drum, before its level; None once it has ended.
    fn next(&mut self) -> Option<f32> {
        let sr = self.sr;
        let t = self.t;
        let pole = self.dc_pole;
        let (noise, gain) = (&mut self.noise, self.noise_gain);
        let mut white = || noise.next() * gain;
        Some(match &mut self.sound {
            Sound::Silent => return None,
            Sound::Kick { phase, tune, sweep, drop, drop_coef, amp, click, click_len, click_gain, push, dc } => {
                if amp.value == 0.0 {
                    return None;
                }
                let body = sin2pi(*phase) * amp.next();
                *phase = (*phase + *tune * (1.0 + *sweep * *drop) / sr).fract();
                *drop *= *drop_coef;
                let mut x = body;
                if t < *click_len {
                    // A half-sine window, 0 at both ends.
                    x += *click_gain * click.tick(white()) * sin2pi(0.5 * t as f32 / *click_len as f32);
                }
                dc.tick(soft(x * *push) / soft(*push), pole)
            }
            Sound::Snare { phase, freq, bend, bend_coef, modes, body, noise: env, hp, lp, rattle, dc } => {
                if env.value == 0.0 && modes[0].value == 0.0 && modes[1].value == 0.0 {
                    return None;
                }
                let tone = 0.65 * sin2pi(phase[0]) * modes[0].next() + 0.35 * sin2pi(phase[1]) * modes[1].next();
                for (p, f) in phase.iter_mut().zip(freq.iter()) {
                    *p = (*p + f * (1.0 + *bend) / sr).fract();
                }
                *bend *= *bend_coef;
                let n = lp.tick(hp.tick(white())) * env.next();
                dc.tick(*body * tone + *rattle * n, pole)
            }
            Sound::Clap { bp, bursts } => {
                let main = bursts[3];
                if t > main.start + main.rise && main.decay.value == 0.0 {
                    return None;
                }
                let e: f32 = bursts.iter_mut().map(|b| b.at(t)).sum();
                bp.tick(white()) * e * 2.0
            }
            Sound::Metal { squares, dt, bus, bands, mix, envs, hp, hold, air } => {
                if envs[0].value == 0.0 && envs[1].value == 0.0 {
                    return None;
                }
                let mut m = 0.0;
                for (sq, &d) in squares.iter_mut().zip(dt.iter()) {
                    m += sq.tick(d, 1.0 / 6.0, bus);
                }
                bus.add::<1>([m, 0.0]);
                let x = bus.next::<1>()[0] + *air * white();
                let held = t < *hold;
                let mut y = 0.0;
                for k in 0..2 {
                    let band = bands[k].tick(x);
                    let e = if held { envs[k].value } else { envs[k].next() };
                    y += mix[k] * band * e;
                }
                hp.tick(y)
            }
            Sound::Tom { phase, freq, bend, bend_coef, amp, stick, bp, dc } => {
                if amp.value == 0.0 {
                    return None;
                }
                let body = sin2pi(*phase) * amp.next();
                *phase = (*phase + *freq * (1.0 + *bend) / sr).fract();
                *bend *= *bend_coef;
                dc.tick(body + bp.tick(white()) * stick.next(), pole)
            }
            Sound::Rim { res, mix, pulse, hp, life } => {
                if life.value == 0.0 {
                    return None;
                }
                life.next();
                let x = if t < *pulse { sin2pi(0.5 * t as f32 / *pulse as f32) } else { 0.0 };
                let y = mix[0] * res[0].tick(x) + mix[1] * res[1].tick(x);
                soft(3.0 * hp.tick(y)) / 3.0
            }
            Sound::Cowbell { squares, dt, bus, bp, envs } => {
                if envs[1].value == 0.0 {
                    return None;
                }
                let m = squares[0].tick(dt[0], 0.5, bus) + squares[1].tick(dt[1], 0.5, bus);
                bus.add::<1>([m, 0.0]);
                bp.tick(bus.next::<1>()[0]) * (envs[0].next() + envs[1].next())
            }
            Sound::Shaker { hp, bp, amp } => {
                if amp.value == 0.0 {
                    return None;
                }
                bp.tick(hp.tick(white())) * amp.next()
            }
        })
    }
}

/// Squares at phases of their own, from `seed`.
fn free_squares<const N: usize>(seed: u32) -> [Square; N] {
    std::array::from_fn(|k| Square { phase: (hash(seed, 50 + k as u32) >> 8) as f32 / (1u32 << 24) as f32 })
}

/// A unit-peak bandpass at `f` Hz that rings for `t60` seconds.
fn resonator(sr: f32, f: f32, t60: f32) -> Svf {
    let q = (t60 * std::f32::consts::PI * f / T60).max(0.5);
    Svf::bandpass(sr, f, q)
}

impl Kernel for Drum {
    const INPUTS: usize = INPUTS;
    const OUTPUTS: usize = 1;

    fn new(cfg: &Cfg) -> Self {
        let sample_rate = cfg.sample_rate;
        Drum {
            sr: sample_rate.max(1.0),
            t: 0,
            kind: Kind::Silent,
            sound: Sound::Silent,
            seed: 0,
            noise: Noise::new(1),
            noise_gain: (sample_rate.max(1.0) / 48_000.0).sqrt(),
            gain: 0.0,
            rise: 1,
            drive: None,
            fade: 0.0,
            fade_step: 0.0,
            choke_len: CHOKE,
            done: false,
            dc_pole: DcBlock::pole(sample_rate.max(1.0)),
            one: false,
            boost: 0.0,
            boost_len: 1,
        }
    }

    fn process(&mut self, io: &mut Io) {
        let (ins, [out]) = io.ports(std::array::from_fn::<_, INPUTS, _>(|p| (p, 0)), [(0, 0)]);
        for i in 0..out.len() {
            if self.t == 0 {
                self.start(&|p: usize| ins[p][i]);
            } else if !self.done
                && self.fade_step == 0.0
                && if self.one { any_hit(self.kind, ins[HITS][i]) } else { choked(self.kind, ins[HITS][i]) }
            {
                self.fade_step = 1.0 / (self.choke_len * self.sr);
            }
            out[i] = 0.0;
            if !self.done {
                match self.next() {
                    None => self.done = true,
                    Some(mut y) => {
                        if self.t < self.rise {
                            y *= self.t as f32 / self.rise as f32;
                        }
                        if self.boost != 0.0 && self.t < self.boost_len {
                            y *= 1.0 + self.boost * (1.0 - self.t as f32 / self.boost_len as f32);
                        }
                        if let Some((d, clip)) = &mut self.drive {
                            y = clip.tick(y * d.into) * d.out;
                        }
                        if self.fade_step > 0.0 {
                            // A raised-cosine fade to exact 0.
                            y *= 0.5 + 0.5 * sin2pi(0.25 + 0.5 * self.fade);
                            self.fade += self.fade_step;
                            if self.fade >= 1.0 {
                                self.done = true;
                            }
                        }
                        out[i] = if self.done { 0.0 } else { y * self.gain };
                    }
                }
            }
            self.t = self.t.saturating_add(1);
        }
    }

    /// A drum keeps sounding until its decay (or a choke) ends it.
    fn active(&self) -> bool {
        self.t > 0 && !self.done
    }
}

/// Whether `hits` (a sum of drumhit's marks for the hits starting now) holds a hit in
/// `kind`'s group.
fn choked(kind: Kind, h: f32) -> bool {
    let Some(g) = kind.group() else { return false };
    (1.0..16_777_216.0).contains(&h) && h == h.floor() && (h as u32 >> (4 * g)) & 15 != 0
}

/// Whether `hits` holds a hit (drumhit's field for every hit, whatever its group) and `kind` chokes:
/// One drum's lane plays only its drum, so its next hit cuts it, as its single's did.
fn any_hit(kind: Kind, h: f32) -> bool {
    kind.group().is_some() && (1.0..16_777_216.0).contains(&h) && h == h.floor() && (h as u32 / m2_dsp::kit::HIT_FIELD) & 15 != 0
}

export!(drum, Drum);

/// The Drum's voice, `@m2/drums:one`: [`Drum`] with its drum a menu (its port 4, `drum`, a choice
/// whose places [`SOUNDS`] reads), cut by any hit on its lane when its drum chokes.
pub struct One(Drum);

impl Kernel for One {
    const INPUTS: usize = INPUTS;
    const OUTPUTS: usize = 1;

    fn new(cfg: &Cfg) -> Self {
        One(Drum { one: true, ..Drum::new(cfg) })
    }

    fn process(&mut self, io: &mut Io) {
        self.0.process(io)
    }

    fn active(&self) -> bool {
        self.0.active()
    }
}

export!(one, One);

#[cfg(test)]
mod tests;
