//! The song's key as a note effect reads it, `time.key` (VOX-12): a set of the twelve pitch
//! classes, bit n for pitch class n (0 is C). What Chord and Scale share (PACK-31, PACK-33): the
//! key's nearest note, a tie going down, as Lock to key finds it (NOTE-3), and a walk along the
//! key's notes.

/// The highest pitch a note has; the lowest is 0.
pub const TOP: i32 = 127;

/// A key with fewer than all twelve notes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    set: u16,
}

impl Key {
    /// The key `time.key` says, or none for every note: no key, a chromatic one (4095), or a value
    /// that isn't a set of notes.
    pub fn of(v: f32) -> Option<Key> {
        if !v.is_finite() || !(1.0..4095.0).contains(&v) {
            return None;
        }
        let set = v as u16 & 0xfff;
        (set != 0 && set != 0xfff).then_some(Key { set })
    }

    /// Whether pitch `p` is a note of the key.
    pub fn has(self, p: i32) -> bool {
        (self.set >> p.rem_euclid(12)) & 1 == 1
    }

    /// The note of the key nearest pitch `p` (rounded), from 0 to 127: one exactly between two goes
    /// down to the lower (NOTE-3), as `SongKey::snap` finds it.
    pub fn snap(self, p: f32) -> i32 {
        let p = if p.is_finite() { (p.round() as i32).clamp(0, TOP) } else { 60 };
        let fits = |q: i32| (0..=TOP).contains(&q) && self.has(q);
        (0..=12).find_map(|d| [p - d, p + d].into_iter().find(|&q| fits(q))).unwrap_or(p)
    }

    /// The note `steps` notes of the key from `from`, a note of the key: up for `steps` above 0,
    /// down below. It may lie past 0 or 127.
    pub fn step(self, from: i32, steps: i32) -> i32 {
        let by = steps.signum();
        let mut p = from;
        for _ in 0..steps.unsigned_abs() {
            p += by;
            while !self.has(p) {
                p += by;
            }
        }
        p
    }
}
