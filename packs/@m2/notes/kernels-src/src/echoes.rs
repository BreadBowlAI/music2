//! `@m2/notes:echo`, Note echo (PACK-32): each note as it comes, then `repeats` echoes, `time`
//! apart, each (1 − `fall`) times as hard as the last and `pitch` semitones higher, each as long as
//! the note. Echoes are placed from the note's start on the song's beat while it plays, and on a
//! clock of its own at `time.tempo` while it is stopped; Play, a Stop, a seek and a loop's wrap end
//! those sounding and forget those to come. A note's echoes keep the values it started with.

use m2_kernel::{Cfg, Event, Give, Io, Kernel, LIVE};

use crate::key::TOP;
use crate::time::{self, RATES, Song};

/// The most notes it echoes at once (PACK-32): a note past that plays alone.
pub const ROOM: usize = 256;
/// The most echoes of a note: with the note itself, its `fan` of 8.
pub const REPEATS: usize = 7;

// Its inputs, by place (as m2pkg.toml lists them).
const NOTES: usize = 0;
const BEATS: usize = 1;
const PLAYING: usize = 2;
const TEMPO: usize = 3;
const TIME: usize = 4;
const REPEAT: usize = 5;
const FALL: usize = 6;
const PITCH: usize = 7;

/// A note it echoes: its id, pitch, velocity and slide, where it started and (once let go) ended
/// and at which frame since the kernel's first it was let go, how far apart its echoes are, how
/// many, how much quieter and higher each is, and which of them have started and ended (bit k for
/// echo k).
#[derive(Clone, Copy, Default)]
struct Echoed {
    id: u32,
    pitch: f32,
    velocity: f32,
    from_pitch: Option<f32>,
    start: f64,
    end: Option<f64>,
    closed: u64,
    every: f64,
    repeats: usize,
    fall: f32,
    climb: f32,
    started: u16,
    ended: u16,
}

impl Echoed {
    /// Echo `k`'s pitch and velocity, when it plays: none when it would be silent or past 0 or 127.
    fn echo(&self, k: usize) -> Option<(f32, f32)> {
        let (p, v) = (self.pitch + self.climb * k as f32, self.velocity * (1.0 - self.fall).powi(k as i32));
        ((0.0..=TOP as f32).contains(&p) && v > 0.0).then_some((p, v))
    }
}

/// The id of echo `k` of the note `id` (the note itself is echo 0): eight to a note, each with the
/// note's live bit (ABI-15).
fn id_of(id: u32, k: usize) -> u32 {
    ((id & !LIVE).wrapping_mul(REPEATS as u32 + 1).wrapping_add(k as u32) & !LIVE) | (id & LIVE)
}

pub struct Echo {
    sample_rate: f32,
    notes: [Echoed; ROOM],
    n: usize,
    last: Option<Song>,
    /// Its own clock while the song is stopped, in beats.
    clock: f64,
    /// The frames of its calls before this one.
    past: u64,
}

/// The frames one pass works out the places of: a block's, or a part of a longer call's.
const CHUNK: usize = 128;

impl Echo {
    /// Every echo sounding ends at frame `f`, and every one to come is forgotten.
    fn end_all(&mut self, give: &mut Give, f: usize) {
        for e in &self.notes[..self.n] {
            for k in 1..=e.repeats {
                if e.started & !e.ended & (1 << k) != 0 {
                    give.push(Event::Off { frame: f, id: id_of(e.id, k), velocity: 0.0 });
                }
            }
        }
        self.n = 0;
    }

    /// Each echo that starts or ends within frames `lo..hi` of a pass whose frames from `base` are
    /// at the places `at` (never going back within `lo..hi`, which no jump splits), at the first
    /// frame that reaches its place; then the notes let go before `hi` whose echoes are all over
    /// go, their places free, as if each frame had been checked in turn. A search over those frames
    /// for each echo, so a pass costs little however many notes it echoes.
    fn due(&mut self, give: &mut Give, at: &[f64], base: usize, lo: usize, hi: usize) {
        if lo >= hi {
            return;
        }
        let places = &at[lo - base..hi - base];
        let last = places[places.len() - 1];
        let frame = |target: f64| lo + places.partition_point(|&x| x < target);
        let mut i = 0;
        while i < self.n {
            let e = &mut self.notes[i];
            for k in 1..=e.repeats {
                let bit = 1 << k;
                let on = e.start + k as f64 * e.every;
                if e.started & bit == 0 && last >= on {
                    e.started |= bit;
                    match e.echo(k) {
                        Some((pitch, velocity)) => {
                            let from_pitch = e.from_pitch.map(|x| x + pitch - e.pitch).filter(|x| (0.0..=TOP as f32).contains(x));
                            give.push(Event::On { frame: frame(on), id: id_of(e.id, k), pitch, velocity, from_pitch });
                        }
                        // One that would be silent or past 0 or 127 is left out, as if over.
                        None => e.ended |= bit,
                    }
                }
                if let Some(end) = e.end
                    && e.started & !e.ended & bit != 0
                    && last >= end + k as f64 * e.every
                {
                    e.ended |= bit;
                    give.push(Event::Off { frame: frame(end + k as f64 * e.every), id: id_of(e.id, k), velocity: 0.0 });
                }
            }
            let all = ((1u16 << (e.repeats + 1)) - 1) & !1;
            if e.end.is_some() && e.closed < self.past + hi as u64 && e.ended & all == all {
                self.notes.copy_within(i + 1..self.n, i);
                self.n -= 1;
            } else {
                i += 1;
            }
        }
    }

    /// Frames `from..to` of a call (at most a [`CHUNK`]): each frame's transport and notes, then
    /// the echoes that fall in them.
    fn pass(&mut self, give: &mut Give, io: &Io, events: &mut std::iter::Peekable<m2_kernel::Events<'_>>, from: usize, to: usize) {
        let mut at = [0.0f64; CHUNK];
        let mut lo = from;
        for f in from..to {
            let song = Song::at(io.input(PLAYING)[f], io.input(BEATS)[f]);
            let per_frame = time::beats_per_frame(io.input(TEMPO)[f], self.sample_rate);
            if song.moved(self.last) {
                // The echoes due before the jump still play; then those sounding end.
                self.due(give, &at, from, lo, f);
                self.end_all(give, f);
                self.clock = 0.0;
                lo = f;
            }
            self.last = Some(song);
            let pos = if song.playing { song.beats as f64 } else { self.clock };
            // Judged at the frame nearest each place, as the Arpeggiator judges its steps. Held from
            // going back between two jumps (a tempo change can pull it back a hair), so the search
            // finds the first frame that reaches a place, as a check at every frame would.
            let here = pos + 0.5 * per_frame;
            at[f - from] = if f > lo { here.max(at[f - from - 1]) } else { here };
            while let Some(e) = events.next_if(|e| e.frame() <= f) {
                match e {
                    // A note given past 0 or 127 is left out, and so are its echoes (PACK-29).
                    Event::On { pitch, .. } if !(0.0..=TOP as f32).contains(&pitch) => {}
                    Event::On { id, pitch, velocity, from_pitch, .. } => {
                        give.push(Event::On { frame: f, id: id_of(id, 0), pitch, velocity, from_pitch });
                        if self.n == ROOM {
                            // A place may have come free in this pass, before this frame.
                            self.due(give, &at, from, lo, f);
                            lo = f;
                        }
                        let repeats = time::within(io.input(REPEAT)[f], 1.0, REPEATS as f32, 3.0).round() as usize;
                        if self.n < ROOM {
                            self.notes[self.n] = Echoed {
                                id,
                                pitch,
                                velocity,
                                from_pitch,
                                start: pos,
                                end: None,
                                closed: 0,
                                every: RATES[time::place(io.input(TIME)[f], RATES.len(), 1)],
                                repeats,
                                fall: time::within(io.input(FALL)[f], 0.0, 1.0, 0.3),
                                climb: time::within(io.input(PITCH)[f], -12.0, 12.0, 0.0).round(),
                                started: 0,
                                ended: 0,
                            };
                            self.n += 1;
                        }
                    }
                    Event::Off { id, velocity, .. } => {
                        give.push(Event::Off { frame: f, id: id_of(id, 0), velocity });
                        if let Some(e) = self.notes[..self.n].iter_mut().find(|e| e.id == id && e.end.is_none()) {
                            (e.end, e.closed) = (Some(pos), self.past + f as u64);
                        }
                    }
                    Event::Midi { .. } => {
                        give.push(e);
                    }
                    _ => {}
                }
            }
            if !song.playing {
                self.clock += per_frame;
            }
        }
        self.due(give, &at, from, lo, to);
    }
}

impl Kernel for Echo {
    const INPUTS: usize = 8;
    const OUTPUTS: usize = 1;
    const NOTES: &'static [usize] = &[NOTES, 8];

    fn new(cfg: &Cfg) -> Self {
        Echo { sample_rate: cfg.sample_rate, notes: [Echoed::default(); ROOM], n: 0, last: None, clock: 0.0, past: 0 }
    }

    fn process(&mut self, io: &mut Io) {
        let mut give = io.give(0);
        let mut events = io.events(NOTES).peekable();
        let mut from = 0;
        while from < io.frames {
            let to = (from + CHUNK).min(io.frames);
            self.pass(&mut give, io, &mut events, from, to);
            from = to;
        }
        self.past += io.frames as u64;
    }

    fn active(&self) -> bool {
        self.n > 0
    }
}
