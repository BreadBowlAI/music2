//! The `poly` op's allocator (RENDER-16): which voice a note gets, as `steal` says, and in
//! `"mono"` and `"legato"` the one voice's pitch, gate and velocity at every frame, as std's
//! `std:mono` kernel played them, kept by note id. Fixed sizes: nothing here allocates once built.

use m2_ir::{Alloc, Mode, Steal};

use crate::event::Event;
use crate::{BLOCK, VoiceSlot};

/// Notes remembered under the one sounding: more than any line holds down at once (std's depth).
const HELD: usize = 8;

/// The longest slide, in seconds.
const GLIDE_MAX: f32 = 10.0;

/// The voice a note starting at engine sample `t` gets, or none (`steal: "none"` and every voice
/// busy): a free voice, else the one `steal` names. A voice given a note this block is taken last.
pub(crate) fn pick(voices: &[VoiceSlot], steal: Steal) -> Option<usize> {
    if let Some(i) = voices.iter().position(|v| v.note.is_none() && !v.taken) {
        return Some(i);
    }
    let all = 0..voices.len();
    match steal {
        Steal::Oldest => all.min_by_key(|&i| voices[i].born),
        // The lowest peak over its last block; a tie goes to the oldest.
        Steal::Quietest => all.min_by(|&a, &b| {
            let key = |i: usize| (voices[i].taken, voices[i].last_peak, voices[i].born);
            let ((ta, pa, ba), (tb, pb, bb)) = (key(a), key(b));
            ta.cmp(&tb).then(pa.total_cmp(&pb)).then(ba.cmp(&bb))
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
    kept: bool,
}

/// One block's pitch, gate and velocity, by frame: what a one-note voice's note fields read.
pub(crate) struct Cv {
    pub(crate) pitch: [f32; BLOCK],
    pub(crate) gate: [f32; BLOCK],
    pub(crate) velocity: [f32; BLOCK],
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
    target: f32,
    step: f32,
    /// Frames of the slide left.
    left: u32,
    sr: f32,
}

impl Mono {
    pub(crate) fn new(sample_rate: f64) -> Mono {
        let none = Held { id: 0, pitch: 0.0, velocity: 0.0, kept: false };
        Mono {
            cv: Cv { pitch: [60.0; BLOCK], gate: [0.0; BLOCK], velocity: [0.0; BLOCK] },
            starts: [0; BLOCK],
            n_starts: 0,
            started: false,
            first: 0,
            held: [none; HELD],
            count: 0,
            gate: false,
            pitch: 60.0,
            velocity: 0.0,
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

    /// One block of `events`, `glide` the slide's time in seconds at each frame (none: 0): fills
    /// [`Mono::cv`] and [`Mono::starts`].
    pub(crate) fn block(&mut self, events: &[Event], glide: Option<&[f32]>, alloc: Alloc) {
        self.n_starts = 0;
        let mut e = 0;
        for i in 0..BLOCK {
            let sounding = self.top();
            // This frame's events: Offs first, then Ons (RENDER-14). An On whose Off came on the
            // same frame is no note.
            let mut start = None;
            while let Some(ev) = events.get(e).filter(|ev| ev.frame() as usize <= i) {
                match *ev {
                    Event::Off { id, .. } => self.remove(id),
                    Event::On { id, pitch, velocity, frame, .. } => {
                        let ended = events[..e].iter().any(|x| matches!(*x, Event::Off { id: o, frame: f, .. } if o == id && f == frame));
                        if !ended {
                            self.push(Held { id, pitch, velocity, kept: false });
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
                    if alloc.mode == Mode::Mono && self.started { self.slide(s.pitch, glide) } else { self.jump(s.pitch) }
                    again = true;
                }
                self.velocity = s.velocity;
                if !self.started {
                    (self.started, self.first) = (true, i as u16);
                }
            } else if let Some(t) = self.top()
                && sounding.is_none_or(|s| s.id != t.id && (s.pitch, s.velocity) != (t.pitch, t.velocity))
            {
                // The note sounding ended and an older one is held: back to it.
                self.slide(t.pitch, glide);
                self.velocity = t.velocity;
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
        }
    }
}
