//! `@m2/notes:arp`, the Arpeggiator (PACK-30): the notes held, one at a time, in a pattern, over and
//! over. Each rule is a function of where the song is and the notes held, never of what it played
//! before (the events note's §5): so a bar sounds the same live, after a Stop or a seek, on each lap
//! of a loop, and in the export.
//!
//! Where it is: the song's beat while it plays, and its own clock, at `time.tempo`, from the first
//! key while the song is stopped. Each frame it applies the frame's events (Offs, then Ons), then:
//! a note that joins restarts the pattern (the catch plays the step at once, else it waits for the
//! next); a step that starts plays the (n − n₀) mod L th note of the order (Random, the hash of the
//! seed and n − n₀); and a note whose gate is over ends. Positions are judged at the frame nearest
//! them (half a frame on from the frame's own), so a grid line or a gate's end that falls on a
//! frame is that frame, whatever the 32-bit beat rounds to.

use m2_kernel::{Cfg, Event, Give, Io, Kernel};

use crate::time::{self, RATES, Song};

/// The most notes it holds at once (PACK-30): a note past that is left out.
pub const HELD: usize = 64;
/// The most of its own notes sounding at once: a step's, the steps' before it while a gate over 1
/// overlaps them, and one the catch ends. Past that, the oldest ends.
const SOUNDING: usize = 8;
/// The longest order: every note held, in 4 octaves, up and back down.
const ORDER: usize = 2 * 4 * HELD;

// Its inputs, by place (as m2pkg.toml lists them).
const NOTES: usize = 0;
const BEATS: usize = 1;
const PLAYING: usize = 2;
const TEMPO: usize = 3;
const STYLE: usize = 4;
const RATE: usize = 5;
const OCTAVES: usize = 6;
const GATE: usize = 7;
const HOLD: usize = 8;
const SWING: usize = 9;

/// The order the held notes play in (`style`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Style {
    Up,
    Down,
    UpDown,
    Played,
    Random,
}

impl Style {
    fn of(v: f32) -> Style {
        [Style::Up, Style::Down, Style::UpDown, Style::Played, Style::Random][time::place(v, 5, 0)]
    }
}

/// A note held: its id, pitch and velocity, the frame it started on (counted from its birth), and
/// whether its key is down (else Hold latched it).
#[derive(Clone, Copy, Default)]
struct Held {
    id: u32,
    pitch: f32,
    velocity: f32,
    at: u64,
    down: bool,
}

/// A note it plays: its id, the step it plays, and where its gate ends.
#[derive(Clone, Copy, Default)]
struct Sounding {
    id: u32,
    step: i64,
    end: f64,
}

pub struct Arp {
    sample_rate: f32,
    seed: u64,
    held: [Held; HELD],
    n_held: usize,
    sounding: [Sounding; SOUNDING],
    n_sounding: usize,
    /// The order, worked out as each step starts: each note's pitch and velocity.
    order: [(f32, f32); ORDER],
    /// The held notes by place, as the order sorts them.
    sorted: [u8; HELD],
    /// The song at the frame before.
    last: Option<Song>,
    /// Its own clock while the song is stopped, in beats; none while it isn't needed.
    clock: Option<f64>,
    /// The step the pattern restarted on (n₀), the furthest step it has reached since, and the
    /// place of the rate it counts steps by.
    n0: i64,
    step: Option<i64>,
    rate: usize,
    /// Frames since its birth: when each held note started.
    frames: u64,
    next_id: u32,
    /// Where the steps picture draws the step playing, in beats; −1 while no note is held.
    meter: f32,
}

/// The step a position `at` is in, on a grid of steps `rate` beats long from beat 0, the second of
/// each pair moved later by `swing` (PACK-30): 0.5 is straight.
fn step_at(at: f64, rate: f64, swing: f64) -> i64 {
    let pair = (at / (2.0 * rate)).floor();
    let into = at - pair * (2.0 * rate);
    2 * pair as i64 + (into >= 2.0 * swing * rate) as i64
}

/// Where step `n` starts, and how long it lasts until the next.
fn step_of(n: i64, rate: f64, swing: f64) -> (f64, f64) {
    let pair = n.div_euclid(2) as f64 * (2.0 * rate);
    match n.rem_euclid(2) {
        0 => (pair, 2.0 * swing * rate),
        _ => (pair + 2.0 * swing * rate, (2.0 - 2.0 * swing) * rate),
    }
}

/// Where the steps picture draws step `k` of a turn, in beats from the turn's start: the middle of
/// its box, which starts where the step does and is `gate` × the step long (one long, past 1).
pub fn place(k: usize, rate: f64, swing: f64, gate: f64) -> f64 {
    let (start, len) = step_of(k as i64, rate, swing);
    start + 0.5 * gate.min(1.0) * len
}

/// The place in an order of `len` notes that Random plays at step `k` (n − n₀): SplitMix64 of the
/// seed and the step, so the same step plays the same note every time (PACK-30).
pub fn pick(seed: u64, k: i64, len: usize) -> usize {
    let mut z = seed.wrapping_add((k as u64).wrapping_add(1).wrapping_mul(0x9E37_79B9_7F4A_7C15));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    ((z ^ (z >> 31)) % len.max(1) as u64) as usize
}

impl Arp {
    /// A key pressed: it joins the held notes (and restarts the pattern), unless 64 are held. With
    /// Hold latching, a key pressed while none is down starts a new chord.
    fn press(&mut self, id: u32, pitch: f32, velocity: f32) -> bool {
        if !(0.0..=127.0).contains(&pitch) {
            return false;
        }
        if self.n_held > 0 && !self.held[..self.n_held].iter().any(|h| h.down) {
            self.n_held = 0;
        }
        self.drop_held(|h| h.id == id);
        if self.n_held == HELD {
            return false;
        }
        self.held[self.n_held] = Held { id, pitch, velocity: velocity.clamp(0.0, 1.0), at: self.frames, down: true };
        self.n_held += 1;
        true
    }

    /// A key let go: its note leaves the held notes, or stays latched while `hold`.
    fn release(&mut self, id: u32, hold: bool) {
        match hold {
            true => self.held[..self.n_held].iter_mut().filter(|h| h.id == id).for_each(|h| h.down = false),
            false => self.drop_held(|h| h.id == id),
        }
    }

    /// Hold lets go: every latched note leaves.
    fn let_go(&mut self) {
        self.drop_held(|h| !h.down);
    }

    fn drop_held(&mut self, which: impl Fn(&Held) -> bool) {
        let mut kept = 0;
        for i in 0..self.n_held {
            if !which(&self.held[i]) {
                self.held[kept] = self.held[i];
                kept += 1;
            }
        }
        self.n_held = kept;
    }

    /// The order the held notes play in, worked out into `order`: its length.
    fn make_order(&mut self, style: Style, octaves: usize) -> usize {
        let n = self.n_held;
        for i in 0..n {
            self.sorted[i] = i as u8;
        }
        let held = &self.held;
        let key = |i: u8| {
            let h = &held[i as usize];
            if style == Style::Played { (h.at as f64, h.pitch, h.id) } else { (h.pitch as f64, 0.0, h.id) }
        };
        // An insertion sort of 64 at most: no room taken.
        for i in 1..n {
            let mut j = i;
            while j > 0 && {
                let (a, b) = (key(self.sorted[j - 1]), key(self.sorted[j]));
                a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.cmp(&b.2)).is_gt()
            } {
                self.sorted.swap(j - 1, j);
                j -= 1;
            }
        }
        let mut len = 0;
        for o in 0..octaves {
            for i in 0..n {
                let h = self.held[self.sorted[i] as usize];
                let p = h.pitch + 12.0 * o as f32;
                if p <= 127.0 {
                    self.order[len] = (p, h.velocity);
                    len += 1;
                }
            }
        }
        match style {
            Style::Down => self.order[..len].reverse(),
            Style::UpDown => {
                let up = len;
                for i in (1..up.saturating_sub(1)).rev() {
                    self.order[len] = self.order[i];
                    len += 1;
                }
            }
            _ => {}
        }
        len
    }

    /// Step `n` plays at frame `f`, the song at `pos`: the note the step was playing ends first (the
    /// catch plays a step again), then the step's note of the order starts.
    #[allow(clippy::too_many_arguments)]
    fn play(&mut self, give: &mut Give, io: &Io, f: usize, n: i64, pos: f64, per_frame: f64, rate: f64, swing: f64) {
        let style = Style::of(io.input(STYLE)[f]);
        let octaves = time::place(io.input(OCTAVES)[f], 4, 0) + 1;
        let gate = time::within(io.input(GATE)[f], 0.1, 2.0, 0.8) as f64;
        self.end_where(give, f, |s| s.step == n);
        let len = self.make_order(style, octaves);
        if len == 0 {
            return;
        }
        let k = match style {
            Style::Random => pick(self.seed, n - self.n0, len),
            _ => (n - self.n0).rem_euclid(len as i64) as usize,
        };
        let (pitch, velocity) = self.order[k];
        let (start, steps) = step_of(n, rate, swing);
        // Its gate's end, from its step's start; never before the frame after it starts.
        let end = (start + gate * steps).max(pos + per_frame);
        if self.n_sounding == SOUNDING {
            let oldest = self.sounding[0].id;
            self.end_where(give, f, |s| s.id == oldest);
        }
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        if give.push(Event::On { frame: f, id, pitch, velocity, from_pitch: None }) {
            self.sounding[self.n_sounding] = Sounding { id, step: n, end };
            self.n_sounding += 1;
        }
        self.meter = place(k, rate, swing, gate) as f32;
    }

    /// The notes it plays that `which` picks end at frame `f`.
    fn end_where(&mut self, give: &mut Give, f: usize, which: impl Fn(&Sounding) -> bool) {
        let mut kept = 0;
        for i in 0..self.n_sounding {
            let s = self.sounding[i];
            if which(&s) {
                give.push(Event::Off { frame: f, id: s.id, velocity: 0.0 });
            } else {
                self.sounding[kept] = s;
                kept += 1;
            }
        }
        self.n_sounding = kept;
    }
}

impl Kernel for Arp {
    const INPUTS: usize = 10;
    const OUTPUTS: usize = 2;
    const NOTES: &'static [usize] = &[NOTES, 10];

    fn new(cfg: &Cfg) -> Self {
        let seed = cfg.values.first().copied().filter(|s| s.is_finite()).unwrap_or(1.0);
        Arp {
            sample_rate: cfg.sample_rate,
            seed: seed as i64 as u64,
            held: [Held::default(); HELD],
            n_held: 0,
            sounding: [Sounding::default(); SOUNDING],
            n_sounding: 0,
            order: [(0.0, 0.0); ORDER],
            sorted: [0; HELD],
            last: None,
            clock: None,
            n0: 0,
            step: None,
            rate: usize::MAX,
            frames: 0,
            next_id: 1,
            meter: -1.0,
        }
    }

    fn process(&mut self, io: &mut Io) {
        let mut give = io.give(0);
        let mut events = io.events(NOTES).peekable();
        for f in 0..io.frames {
            let song = Song::at(io.input(PLAYING)[f], io.input(BEATS)[f]);
            let per_frame = time::beats_per_frame(io.input(TEMPO)[f], self.sample_rate);
            // Play, a Stop, a seek, a loop's wrap: the notes it plays end, Hold lets go, and the
            // pattern restarts here.
            let mut restart = false;
            if song.moved(self.last) {
                self.end_where(&mut give, f, |_| true);
                self.let_go();
                self.clock = None;
                restart = true;
            }
            self.last = Some(song);
            // Hold latches only while the song is stopped.
            let hold = io.input(HOLD)[f] >= 0.5 && !song.playing;
            if !hold {
                self.let_go();
            }
            let was_held = self.n_held;
            while let Some(e) = events.next_if(|e| e.frame() <= f) {
                match e {
                    Event::On { id, pitch, velocity, .. } => restart |= self.press(id, pitch, velocity),
                    Event::Off { id, .. } => self.release(id, hold),
                    Event::Midi { .. } => {
                        give.push(e);
                    }
                    _ => {}
                }
            }
            self.frames += 1;
            if self.n_held == 0 {
                self.step = None;
                self.meter = -1.0;
            }
            // Where it is: the song's beat, or, stopped, its own clock, which starts again at 0 with
            // the first key and runs while a note is held or sounding.
            let pos = if song.playing {
                song.beats as f64
            } else {
                if self.clock.is_none() || (was_held == 0 && self.n_held > 0) {
                    let from = self.clock.unwrap_or(0.0);
                    self.sounding[..self.n_sounding].iter_mut().for_each(|s| s.end -= from);
                    self.clock = Some(0.0);
                }
                if self.n_held == 0 && self.n_sounding == 0 {
                    self.clock = None;
                }
                match self.clock {
                    Some(c) => c,
                    None => continue,
                }
            };
            let at = pos + 0.5 * per_frame;
            self.end_where(&mut give, f, |s| at >= s.end);
            if self.n_held > 0 {
                let place = time::place(io.input(RATE)[f], RATES.len(), 3);
                let (rate, swing) = (RATES[place], time::within(io.input(SWING)[f], 0.5, 0.71, 0.5));
                let n = step_at(at, rate, swing as f64);
                if restart {
                    let (start, len) = step_of(n, rate, swing as f64);
                    let catch = (0.25 * len).max(2.0 * per_frame);
                    self.n0 = if at - start <= catch { n } else { n + 1 };
                    if self.n0 == n {
                        self.play(&mut give, io, f, n, pos, per_frame, rate, swing as f64);
                    }
                    self.step = Some(n);
                    self.rate = place;
                } else if self.rate != place {
                    // A new rate: the pattern starts again at its next step.
                    self.n0 = n + 1;
                    self.step = Some(n);
                    self.rate = place;
                } else if self.step.is_none_or(|s| n > s) {
                    // A step starts. (A swing that moves while it plays never starts one twice.)
                    self.step = Some(n);
                    if n >= self.n0 {
                        self.play(&mut give, io, f, n, pos, per_frame, rate, swing as f64);
                    }
                }
            }
            if let Some(c) = self.clock.as_mut()
                && !song.playing
            {
                *c += per_frame;
            }
        }
        drop(events);
        let meter = if self.n_held == 0 { -1.0 } else { self.meter };
        io.meter(1, meter);
    }

    fn active(&self) -> bool {
        self.n_held > 0 || self.n_sounding > 0
    }
}
