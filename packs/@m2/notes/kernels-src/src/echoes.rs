//! `@m2/notes:echo`, Note echo (PACK-32): each note as it comes, then `repeats` echoes, `time`
//! apart, each (1 − `fall`) times as hard as the last and `pitch` semitones higher, each as long as
//! the note. Echoes are placed from the note's start on the song's beat while it plays, and on a
//! clock of its own at `time.tempo` while it is stopped; Play, a Stop, a seek and a loop's wrap end
//! those sounding and forget those to come. A note's echoes keep the values it started with.

use m2_kernel::{Cfg, Event, Give, Io, Kernel};

use crate::key::TOP;
use crate::time::{self, RATES, Song};

/// The most notes it echoes at once (PACK-32): a note past that plays alone.
pub const ROOM: usize = 256;
/// The most echoes of a note: its `fan`.
pub const REPEATS: usize = 8;

// Its inputs, by place (as m2pkg.toml lists them).
const NOTES: usize = 0;
const BEATS: usize = 1;
const PLAYING: usize = 2;
const TEMPO: usize = 3;
const TIME: usize = 4;
const REPEAT: usize = 5;
const FALL: usize = 6;
const PITCH: usize = 7;

/// A note it echoes: its id, pitch, velocity and slide, where it started and (once let go) ended,
/// how far apart its echoes are, how many, how much quieter and higher each is, and which of them
/// have started and ended (bit k for echo k).
#[derive(Clone, Copy, Default)]
struct Echoed {
    id: u32,
    pitch: f32,
    velocity: f32,
    from_pitch: Option<f32>,
    start: f64,
    end: Option<f64>,
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

/// The id of echo `k` of the note `id` (the note itself is echo 0): nine to a note.
fn id_of(id: u32, k: usize) -> u32 {
    id.wrapping_mul(REPEATS as u32 + 1).wrapping_add(k as u32)
}

pub struct Echo {
    sample_rate: f32,
    notes: [Echoed; ROOM],
    n: usize,
    last: Option<Song>,
    /// Its own clock while the song is stopped, in beats.
    clock: f64,
}

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
}

impl Kernel for Echo {
    const INPUTS: usize = 8;
    const OUTPUTS: usize = 1;
    const NOTES: &'static [usize] = &[NOTES, 8];

    fn new(cfg: &Cfg) -> Self {
        Echo { sample_rate: cfg.sample_rate, notes: [Echoed::default(); ROOM], n: 0, last: None, clock: 0.0 }
    }

    fn process(&mut self, io: &mut Io) {
        let mut give = io.give(0);
        let mut events = io.events(NOTES).peekable();
        for f in 0..io.frames {
            let song = Song::at(io.input(PLAYING)[f], io.input(BEATS)[f]);
            let per_frame = time::beats_per_frame(io.input(TEMPO)[f], self.sample_rate);
            if song.moved(self.last) {
                self.end_all(&mut give, f);
                self.clock = 0.0;
            }
            self.last = Some(song);
            let pos = if song.playing { song.beats as f64 } else { self.clock };
            // Judged at the frame nearest each place, as the Arpeggiator judges its steps.
            let at = pos + 0.5 * per_frame;
            while let Some(e) = events.next_if(|e| e.frame() <= f) {
                match e {
                    Event::On { id, pitch, velocity, from_pitch, .. } => {
                        give.push(Event::On { frame: f, id: id_of(id, 0), pitch, velocity, from_pitch });
                        let repeats = time::within(io.input(REPEAT)[f], 1.0, REPEATS as f32, 3.0).round() as usize;
                        if self.n < ROOM {
                            self.notes[self.n] = Echoed {
                                id,
                                pitch,
                                velocity,
                                from_pitch,
                                start: pos,
                                end: None,
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
                            e.end = Some(pos);
                        }
                    }
                    Event::Midi { .. } => {
                        give.push(e);
                    }
                    _ => {}
                }
            }
            // Each echo starts k × its time after its note starts, and ends as long after it ends.
            let mut i = 0;
            while i < self.n {
                let e = &mut self.notes[i];
                for k in 1..=e.repeats {
                    let bit = 1 << k;
                    if e.started & bit == 0 && at >= e.start + k as f64 * e.every {
                        e.started |= bit;
                        match e.echo(k) {
                            Some((pitch, velocity)) => {
                                let from_pitch = e.from_pitch.map(|x| x + pitch - e.pitch).filter(|x| (0.0..=TOP as f32).contains(x));
                                give.push(Event::On { frame: f, id: id_of(e.id, k), pitch, velocity, from_pitch });
                            }
                            // One that would be silent or past 0 or 127 is left out, as if over.
                            None => e.ended |= bit,
                        }
                    }
                    if let Some(end) = e.end
                        && e.started & !e.ended & bit != 0
                        && at >= end + k as f64 * e.every
                    {
                        e.ended |= bit;
                        give.push(Event::Off { frame: f, id: id_of(e.id, k), velocity: 0.0 });
                    }
                }
                let all = ((1u16 << (e.repeats + 1)) - 1) & !1;
                if e.end.is_some() && e.ended & all == all {
                    self.notes.copy_within(i + 1..self.n, i);
                    self.n -= 1;
                } else {
                    i += 1;
                }
            }
            if !song.playing {
                self.clock += per_frame;
            }
        }
    }

    fn active(&self) -> bool {
        self.n > 0
    }
}
