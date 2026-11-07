//! `@m2/notes:chord`, Chord (PACK-31): each note plays a chord, at once, as hard as it, and its Off
//! ends them all. In key, while the song has one, the note first goes to the key's nearest note (a
//! tie going down, NOTE-3), then the chord stacks the key's own notes from it, counted as a chord
//! stamp counts them (NOTE-4); else each shape's notes are fixed. It keeps nothing between notes:
//! the notes of a note's chord are known by its id (four to a note), so its Off ends all four.

use m2_kernel::{Cfg, Event, Io, Kernel, LIVE};

use crate::key::{Key, TOP};
use crate::time;

// Its inputs, by place (as m2pkg.toml lists them).
const NOTES: usize = 0;
const KEY: usize = 1;
const SHAPE: usize = 2;
const IN_KEY: usize = 3;
const INVERSION: usize = 4;

/// The most notes a chord has: its `fan`.
pub const FAN: usize = 4;

/// Each shape (octave, fifth, triad, 7th, sus2, sus4, add9) in the key's notes from the root (0):
/// the stamps' counting, 1-3-5 being 0, 2 and 4; an octave is 12 semitones in any key (`None`).
const IN_KEY_STEPS: [&[Option<i32>]; 7] = [
    &[Some(0), None],
    &[Some(0), Some(4)],
    &[Some(0), Some(2), Some(4)],
    &[Some(0), Some(2), Some(4), Some(6)],
    &[Some(0), Some(1), Some(4)],
    &[Some(0), Some(3), Some(4)],
    &[Some(0), Some(2), Some(4), Some(8)],
];

/// Each shape's fixed notes, in semitones above the note: a major triad, a dominant 7th.
const FIXED: [&[f32]; 7] =
    [&[0.0, 12.0], &[0.0, 7.0], &[0.0, 4.0, 7.0], &[0.0, 4.0, 7.0, 10.0], &[0.0, 2.0, 7.0], &[0.0, 5.0, 7.0], &[0.0, 4.0, 7.0, 14.0]];

/// The notes of the chord of `shape` on `pitch`, `inversion` times turned over, each with its place
/// in the shape: in `key`, or fixed with none. A pitch it already has is played once, and none past
/// 0 or 127 is played. Low to high.
pub fn chord(pitch: f32, key: Option<Key>, shape: usize, inversion: usize) -> ([(usize, f32); FAN], usize) {
    let mut notes = [(0, 0.0f32); FAN];
    let n = match key {
        Some(k) => {
            let root = k.snap(pitch);
            for (j, s) in IN_KEY_STEPS[shape].iter().enumerate() {
                notes[j] = (j, s.map_or(root + 12, |s| k.step(root, s)) as f32);
            }
            IN_KEY_STEPS[shape].len()
        }
        None => {
            for (j, o) in FIXED[shape].iter().enumerate() {
                notes[j] = (j, pitch + o);
            }
            FIXED[shape].len()
        }
    };
    // Inversion k: k times, the lowest note goes up an octave (the stamps' rule, NOTE-4).
    for _ in 0..inversion {
        let low = (0..n).min_by(|&a, &b| notes[a].1.total_cmp(&notes[b].1)).expect("a chord has notes");
        notes[low].1 += 12.0;
    }
    notes[..n].sort_by(|a, b| a.1.total_cmp(&b.1));
    let mut kept = 0;
    for i in 0..n {
        let (j, p) = notes[i];
        if (0.0..=TOP as f32).contains(&p) && (kept == 0 || notes[kept - 1].1 != p) {
            notes[kept] = (j, p);
            kept += 1;
        }
    }
    (notes, kept)
}

/// The id of note `j` of the chord of the note `id`: four to a note, so its Off finds them, each
/// with the note's live bit (ABI-15).
fn id_of(id: u32, j: usize) -> u32 {
    ((id & !LIVE).wrapping_mul(FAN as u32).wrapping_add(j as u32) & !LIVE) | (id & LIVE)
}

pub struct Chord;

impl Kernel for Chord {
    const INPUTS: usize = 5;
    const OUTPUTS: usize = 1;
    const NOTES: &'static [usize] = &[NOTES, 5];

    fn new(_: &Cfg) -> Self {
        Chord
    }

    fn process(&mut self, io: &mut Io) {
        let mut give = io.give(0);
        for e in io.events(NOTES) {
            match e {
                Event::On { frame, id, pitch, velocity, from_pitch } => {
                    let in_key = io.input(IN_KEY)[frame] >= 0.5 || io.input(IN_KEY)[frame].is_nan();
                    let key = if in_key { Key::of(io.input(KEY)[frame]) } else { None };
                    let shape = time::place(io.input(SHAPE)[frame], FIXED.len(), 2);
                    let inversion = time::within(io.input(INVERSION)[frame], 0.0, 3.0, 0.0).round() as usize;
                    let (notes, n) = chord(pitch, key, shape, inversion);
                    for &(j, p) in &notes[..n] {
                        // Its slide moves with it, and goes where it would leave 0 to 127.
                        let from = from_pitch.map(|f| f + p - pitch).filter(|f| (0.0..=TOP as f32).contains(f));
                        give.push(Event::On { frame, id: id_of(id, j), pitch: p, velocity, from_pitch: from });
                    }
                }
                Event::Off { frame, id, velocity } => {
                    for j in 0..FAN {
                        give.push(Event::Off { frame, id: id_of(id, j), velocity });
                    }
                }
                Event::Midi { .. } => {
                    give.push(e);
                }
                _ => {}
            }
        }
    }
}
