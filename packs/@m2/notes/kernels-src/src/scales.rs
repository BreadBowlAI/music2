//! `@m2/notes:lock`, Lock to key (PACK-33): each note moved to the key's nearest note, a tie going
//! down, as the roll's Lock to key moves one (NOTE-3), then `shift` notes of the key up (down, below 0). With no key,
//! or a chromatic one (4095, which `time.key` gives for both), every note plays as it is. A note
//! keeps its id, so its Off ends it; one moved past 0 or 127 is left out.

use m2_kernel::{Cfg, Event, Io, Kernel};

use crate::key::{Key, TOP};
use crate::time;

// Its inputs, by place (as m2pkg.toml lists them).
const NOTES: usize = 0;
const KEY: usize = 1;
const SHIFT: usize = 2;

/// Where Lock to key moves `pitch` in `key`, `shift` notes of the key on: none past 0 or 127, nor for a
/// note given past them, with no key too (PACK-29).
pub fn moved(pitch: f32, key: Option<Key>, shift: i32) -> Option<f32> {
    let Some(k) = key else { return (0.0..=TOP as f32).contains(&pitch).then_some(pitch) };
    let p = k.step(k.snap(pitch), shift);
    (0..=TOP).contains(&p).then_some(p as f32)
}

pub struct LockToKey;

impl Kernel for LockToKey {
    const INPUTS: usize = 3;
    const OUTPUTS: usize = 1;
    const NOTES: &'static [usize] = &[NOTES, 3];

    fn new(_: &Cfg) -> Self {
        LockToKey
    }

    fn process(&mut self, io: &mut Io) {
        let mut give = io.give(0);
        for e in io.events(NOTES) {
            match e {
                Event::On { frame, id, pitch, velocity, from_pitch } => {
                    let shift = time::within(io.input(SHIFT)[frame], -7.0, 7.0, 0.0).round() as i32;
                    if let Some(p) = moved(pitch, Key::of(io.input(KEY)[frame]), shift) {
                        let from = from_pitch.map(|f| f + p - pitch).filter(|f| (0.0..=TOP as f32).contains(f));
                        give.push(Event::On { frame, id, pitch: p, velocity, from_pitch: from });
                    }
                }
                Event::Off { .. } | Event::Midi { .. } => {
                    give.push(e);
                }
                _ => {}
            }
        }
    }
}
