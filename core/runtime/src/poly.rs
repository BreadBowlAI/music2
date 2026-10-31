//! The `poly` op's allocator (RENDER-16): which voice a note gets, as `steal` says, and in
//! `"mono"` and `"legato"` the one voice's pitch, gate and velocity at every frame, as std's
//! `std:mono` kernel played them, kept by note id; and the lane's wheels, as its voices hear them.
//! Fixed sizes: nothing here allocates once built.

use m2_ir::{Alloc, Mode, Steal};

use crate::event::Event;
use crate::{BLOCK, VoiceSlot};

/// Notes remembered under the one sounding: more than any line holds down at once (std's depth).
const HELD: usize = 8;

/// The longest slide, in seconds.
const GLIDE_MAX: f32 = 10.0;

/// The bend at the pitch wheel's ends, in semitones (General MIDI's).
pub(crate) const BEND_RANGE: f32 = 2.0;

/// A lane's pitch wheel and mod wheel as its voices hear them (RENDER-14's `Midi`, D8 of track pm):
/// the bend in semitones, added to every voice's pitch, and the mod wheel, 0 to 1, a note field.
/// Kept in the pool, and reset with it.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Wheels {
    pub(crate) bend: f32,
    pub(crate) wheel: f32,
}

impl Wheels {
    /// Take a block's wheel events: the latest of each holds for the whole block, as they come at
    /// its first frame. A bend at either end is exactly [`BEND_RANGE`]; centred, exactly 0.
    pub(crate) fn read(&mut self, events: &[Event]) {
        for e in events {
            let Event::Midi { bytes: [status, d1, d2], .. } = *e else { continue };
            match (status & 0xF0, d1) {
                (0xE0, _) => {
                    let v = (u16::from(d1 & 0x7F) | u16::from(d2 & 0x7F) << 7) as f32 - 8192.0;
                    self.bend = BEND_RANGE * v / if v < 0.0 { 8192.0 } else { 8191.0 };
                }
                (0xB0, 1) => self.wheel = f32::from(d2 & 0x7F) / 127.0,
                (0xB0, 121) => *self = Wheels::default(),
                _ => {}
            }
        }
    }
}

/// How many notes have started on each key of a lane, as a pool counts them (RENDER-39): the key is
/// the pitch rounded, 0 to 127, and notes that start on one sample on one key count once. Counted
/// from the notes alone, so never by engine time; kept in the pool, and reset with it.
pub(crate) struct KeyCounts {
    n: [u32; 128],
    /// The engine sample each key last counted a start at.
    at: [u64; 128],
}

/// Where a count wraps: the integers below it are exact in f32, as `std:hitcount`'s.
const COUNT_WRAP: u32 = 1 << 24;

impl KeyCounts {
    pub(crate) const NONE: KeyCounts = KeyCounts { n: [0; 128], at: [u64::MAX; 128] };

    /// Count a note on `pitch` starting at engine sample `t`: its key's count, itself included.
    pub(crate) fn count(&mut self, t: u64, pitch: f32) -> u32 {
        let key = key_of(pitch);
        if self.at[key] != t {
            self.at[key] = t;
            self.n[key] = self.n[key] % (COUNT_WRAP - 1) + 1;
        }
        self.n[key]
    }
}

/// The key of a note on `pitch`: its pitch rounded, 0 to 127.
fn key_of(pitch: f32) -> usize {
    if pitch.is_nan() { 0 } else { pitch.round().clamp(0.0, 127.0) as usize }
}

/// The sustain pedal's state after `e`, if `e` moves it (RENDER-35): down at 64 or more.
pub(crate) fn pedal(e: &Event) -> Option<bool> {
    match *e {
        Event::Midi { bytes: [status, 64, v], .. } if status & 0xF0 == 0xB0 => Some(v >= 64),
        Event::Midi { bytes: [status, 121, _], .. } if status & 0xF0 == 0xB0 => Some(false),
        _ => None,
    }
}

/// What the pedal does to one note (RENDER-38): its id, its pitch, the sample its Off came at
/// (`u64::MAX` until it does), and whether the pedal holds it.
pub(crate) trait Pedalled {
    fn id(&self) -> u32;
    fn pitch(&self) -> f32;
    fn end(&mut self, at: u64);
    fn ended(&self) -> bool;
    fn held(&mut self) -> &mut bool;
}

/// A walk over notes: it calls what it is given on each one.
pub(crate) type EachNote<'a, N> = dyn FnMut(&mut dyn FnMut(&mut N)) + 'a;

/// One block's `events` from engine sample `start`, the pedal `down` as the block starts, over the
/// notes `each` visits (RENDER-38): a note's Off ends it at its frame, or, the pedal down there,
/// holds it; the pedal going up ends every note it holds at its frame, and a key struck again ends
/// the one held on that key. Returns the pedal as the block ends. Never allocates.
pub(crate) fn hold<N: Pedalled>(events: &[Event], mut down: bool, start: u64, each: &mut EachNote<N>) -> bool {
    for e in events {
        let at = start + e.frame() as u64;
        if let Some(now) = pedal(e) {
            if down && !now {
                each(&mut |n: &mut N| {
                    if std::mem::take(n.held()) {
                        n.end(at);
                    }
                });
            }
            down = now;
            continue;
        }
        match *e {
            Event::Off { id, .. } => each(&mut |n: &mut N| {
                if n.id() == id && !n.ended() && !*n.held() {
                    match down {
                        true => *n.held() = true,
                        false => n.end(at),
                    }
                }
            }),
            Event::On { pitch, .. } => {
                let key = key_of(pitch);
                each(&mut |n: &mut N| {
                    if *n.held() && key_of(n.pitch()) == key {
                        *n.held() = false;
                        n.end(at);
                    }
                });
            }
            _ => {}
        }
    }
    down
}

/// The voice a note starting at engine sample `t` gets, or none (`steal: "none"` and every voice
/// busy): a free voice, else the one `steal` names. A voice given a note this block is taken last,
/// and one whose note only the pedal holds first (RENDER-38): its key is up, as a piano's damper
/// would be.
pub(crate) fn pick(voices: &[VoiceSlot], steal: Steal) -> Option<usize> {
    if let Some(i) = voices.iter().position(|v| v.note.is_none() && !v.taken) {
        return Some(i);
    }
    let all = 0..voices.len();
    let kept = |i: usize| !voices[i].note.is_some_and(|n| n.held);
    match steal {
        Steal::Oldest => all.min_by_key(|&i| (kept(i), voices[i].born)),
        // The lowest peak over its last block; a tie goes to the oldest.
        Steal::Quietest => all.min_by(|&a, &b| {
            let key = |i: usize| (voices[i].taken, kept(i), voices[i].last_peak, voices[i].born);
            let ((ta, ka, pa, ba), (tb, kb, pb, bb)) = (key(a), key(b));
            ta.cmp(&tb).then(ka.cmp(&kb)).then(pa.total_cmp(&pb)).then(ba.cmp(&bb))
        }),
        Steal::None => None,
    }
}

/// A note held, oldest first. `kept`: the one of the notes that started on its frame that sounds
/// (the last); the others count as held, and never sound.
#[derive(Clone, Copy, Debug)]
struct Held {
    id: u32,
    pitch: f32,
    velocity: f32,
    /// Its key's count (RENDER-39).
    count: f32,
    kept: bool,
    /// Let go while the pedal is down: it stays until the pedal lifts (RENDER-38).
    pedalled: bool,
}

/// One block's pitch, gate and velocity, by frame: what a one-note voice's note fields read.
pub(crate) struct Cv {
    pub(crate) pitch: [f32; BLOCK],
    pub(crate) gate: [f32; BLOCK],
    pub(crate) velocity: [f32; BLOCK],
    /// The key count of the note it plays (RENDER-39).
    pub(crate) count: [f32; BLOCK],
}

/// A `"mono"` or `"legato"` poly's one note at a time: the notes held, the slide, and this
/// block's [`Cv`]. Std's rules, by note id: the latest note wins; when it ends while older ones
/// are held, the newest of them sounds again; notes that start on one frame count as one, the
/// last.
pub(crate) struct Mono {
    pub(crate) cv: Cv,
    /// The frames of this block where the voice starts again (`retrigger`: it is reset there).
    pub(crate) starts: [u16; BLOCK],
    pub(crate) n_starts: usize,
    /// A note has started: from then on the voice runs every block, as an instrument outside a
    /// poly does (it never sleeps).
    pub(crate) started: bool,
    /// The frame of the block the first note started on.
    pub(crate) first: u16,
    held: [Held; HELD],
    count: usize,
    gate: bool,
    pitch: f32,
    velocity: f32,
    key_count: f32,
    target: f32,
    step: f32,
    /// Frames of the slide left.
    left: u32,
    sr: f32,
}

impl Mono {
    pub(crate) fn new(sample_rate: f64) -> Mono {
        let none = Held { id: 0, pitch: 0.0, velocity: 0.0, count: 0.0, kept: false, pedalled: false };
        Mono {
            cv: Cv { pitch: [60.0; BLOCK], gate: [0.0; BLOCK], velocity: [0.0; BLOCK], count: [0.0; BLOCK] },
            starts: [0; BLOCK],
            n_starts: 0,
            started: false,
            first: 0,
            held: [none; HELD],
            count: 0,
            gate: false,
            pitch: 60.0,
            velocity: 0.0,
            key_count: 0.0,
            target: 60.0,
            step: 0.0,
            left: 0,
            sr: (sample_rate as f32).max(1.0),
        }
    }

    /// Back to the state right after building (RENDER-10).
    pub(crate) fn reset(&mut self) {
        *self = Mono::new(self.sr as f64);
    }

    /// Let go of every note held (RENDER-26a): the gate falls at the next frame, and no older note
    /// comes back.
    pub(crate) fn release(&mut self) {
        self.count = 0;
    }

    /// The note sounding at the end of the block, while the gate is open: its id, and the pitch
    /// and velocity the voice plays.
    pub(crate) fn sounding(&self) -> Option<(u32, f32, f32)> {
        self.top().filter(|_| self.gate).map(|t| (t.id, self.pitch, self.velocity))
    }

    /// The note sounding: the newest kept.
    fn top(&self) -> Option<Held> {
        self.held[..self.count].iter().rev().find(|h| h.kept).copied()
    }

    fn remove(&mut self, id: u32) {
        if let Some(i) = self.held[..self.count].iter().position(|h| h.id == id) {
            self.held.copy_within(i + 1..self.count, i);
            self.count -= 1;
        }
    }

    /// Hold a note, forgetting the oldest when 8 are held.
    fn push(&mut self, h: Held) {
        if self.count == HELD {
            self.held.copy_within(1.., 0);
            self.count -= 1;
        }
        self.held[self.count] = h;
        self.count += 1;
    }

    /// Head for `pitch`: over `glide` seconds, or at once.
    fn slide(&mut self, pitch: f32, glide: f32) {
        let glide = if glide.is_nan() { 0.0 } else { glide.clamp(0.0, GLIDE_MAX) };
        let n = (glide * self.sr).round() as u32;
        self.target = pitch;
        self.left = n;
        if n == 0 {
            self.pitch = pitch;
        } else {
            self.step = (pitch - self.pitch) / n as f32;
        }
    }

    fn jump(&mut self, pitch: f32) {
        self.pitch = pitch;
        self.target = pitch;
        self.left = 0;
    }

    /// One block of `events`, starting at engine sample `start_at`, `glide` the slide's time in seconds
    /// at each frame (none: 0): fills [`Mono::cv`] and [`Mono::starts`]. Each note is counted on its
    /// key in `keys` (RENDER-39). While `pedal` is down, a note let go stays held until it lifts,
    /// or until its key is struck again (RENDER-38).
    pub(crate) fn block(
        &mut self,
        events: &[Event],
        start_at: u64,
        keys: &mut KeyCounts,
        pedal: &mut bool,
        glide: Option<&[f32]>,
        alloc: Alloc,
    ) {
        self.n_starts = 0;
        let mut e = 0;
        for i in 0..BLOCK {
            let sounding = self.top();
            // This frame's events: the controllers, then Offs, then Ons (RENDER-14). An On whose Off
            // came on the same frame is no note, unless the pedal holds it.
            let mut start = None;
            while let Some(ev) = events.get(e).filter(|ev| ev.frame() as usize <= i) {
                if let Some(down) = self::pedal(ev) {
                    if *pedal && !down {
                        while let Some(k) = self.held[..self.count].iter().position(|h| h.pedalled) {
                            let id = self.held[k].id;
                            self.remove(id);
                        }
                    }
                    *pedal = down;
                }
                match *ev {
                    Event::Off { id, .. } if *pedal => {
                        if let Some(h) = self.held[..self.count].iter_mut().find(|h| h.id == id) {
                            h.pedalled = true;
                        }
                    }
                    Event::Off { id, .. } => self.remove(id),
                    Event::On { id, pitch, velocity, frame, .. } => {
                        let count = keys.count(start_at + frame as u64, pitch) as f32;
                        // A key struck again lets go of the note the pedal held on it.
                        while let Some(k) = self.held[..self.count].iter().position(|h| h.pedalled && key_of(h.pitch) == key_of(pitch)) {
                            let id = self.held[k].id;
                            self.remove(id);
                        }
                        // Let go on its own frame: no note, unless the pedal holds it.
                        let ended = events[..e].iter().any(|x| matches!(*x, Event::Off { id: o, frame: f, .. } if o == id && f == frame));
                        if !ended || *pedal {
                            self.push(Held { id, pitch, velocity, count, kept: false, pedalled: ended });
                            start = Some(self.count - 1);
                        }
                    }
                    _ => {}
                }
                e += 1;
            }
            // Of the notes that started on this frame, the last sounds.
            let start = start.map(|k| {
                self.held[k].kept = true;
                self.held[k]
            });
            let glide = glide.map_or(0.0, |g| g[i]);
            let mut gate = self.held[..self.count].iter().any(|h| h.kept);
            let mut again = false;
            if let Some(s) = start {
                // Legato: another note is held, and the gate was open.
                if alloc.mode == Mode::Legato && self.gate && self.count >= 2 {
                    self.slide(s.pitch, glide);
                } else {
                    if alloc.mode == Mode::Mono && self.started {
                        self.slide(s.pitch, glide)
                    } else {
                        self.jump(s.pitch)
                    }
                    again = true;
                }
                (self.velocity, self.key_count) = (s.velocity, s.count);
                if !self.started {
                    (self.started, self.first) = (true, i as u16);
                }
            } else if let Some(t) = self.top()
                && sounding.is_none_or(|s| s.id != t.id && (s.pitch, s.velocity) != (t.pitch, t.velocity))
            {
                // The note sounding ended and an older one is held: back to it.
                self.slide(t.pitch, glide);
                (self.velocity, self.key_count) = (t.velocity, t.count);
                again = alloc.mode == Mode::Mono;
            } else if self.left > 0 {
                self.left -= 1;
                self.pitch = if self.left == 0 { self.target } else { self.pitch + self.step };
            }
            if again {
                if alloc.retrigger {
                    self.starts[self.n_starts] = i as u16;
                    self.n_starts += 1;
                } else if self.gate {
                    // The gate falls for one frame, so envelopes start again from where they are.
                    gate = false;
                }
            }
            self.gate = gate;
            self.cv.gate[i] = if gate { 1.0 } else { 0.0 };
            self.cv.pitch[i] = self.pitch;
            self.cv.velocity[i] = self.velocity;
            self.cv.count[i] = self.key_count;
        }
    }
}
