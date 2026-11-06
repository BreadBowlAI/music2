//! The song's key: a root and a scale of the table ([`crate::scales`], NOTE-1). It is music, like the
//! tempo, so it lives in `data/arrangement.json` (FMT-13, VOX-10) as `{"root": "A", "scale":
//! "minor"}`, and sound reads its notes as `time.key`, a [`keyset`](SongKey::keyset) (VOX-12).

use serde::{Deserialize, Serialize};

use crate::scales::Scale;

/// The roots as files write them, by pitch class from C. The app spells them with ♯ and ♭ (Q32);
/// the file keeps to ASCII, so a hand or a script can write it.
pub const ROOTS: [&str; 12] = ["C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"];

/// The keyset of no key: every one of the twelve notes allowed (VOX-12).
pub const EVERY_NOTE: u16 = 0xfff;

/// A key: its root (one of [`ROOTS`]) and its scale, one of the table's (NOTE-1). Only
/// [`SongKey::new`] makes one, and a request's is read through it, so every key is one that reads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Written", into = "Written")]
pub struct SongKey {
    root: &'static str,
    scale: Scale,
}

/// A key as a file or a request writes it, before it is read: what `POST /api/graph/song-key`
/// takes, so a key that doesn't read is refused as an edit, in words; and what the arrangement
/// keeps of a key music2 doesn't know (VOX-11).
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Written {
    pub root: String,
    pub scale: String,
}

impl TryFrom<Written> for SongKey {
    type Error = String;

    fn try_from(w: Written) -> Result<SongKey, String> {
        SongKey::new(&w.root, &w.scale)
    }
}

impl From<SongKey> for Written {
    fn from(k: SongKey) -> Written {
        k.written()
    }
}

impl SongKey {
    /// A key from what a file or a request says: the root by any of its names (`C#`, `C♯`, `Db`,
    /// `D♭`), the scale by its name in any case (`Harmonic Minor`). The root is kept in [`ROOTS`]'s
    /// spelling.
    pub fn new(root: &str, scale: &str) -> Result<SongKey, String> {
        let pc = pitch_class(root).ok_or_else(|| format!("{root} isn't a key's root: one of {}", ROOTS.join(", ")))?;
        let scale = Scale::try_from(scale.to_string())?;
        Ok(SongKey { root: ROOTS[pc], scale })
    }

    /// Its root, as [`ROOTS`] spells it: `C#`, `Bb`.
    pub fn root(&self) -> &str {
        self.root
    }

    /// Its scale.
    pub fn scale(&self) -> Scale {
        self.scale
    }

    /// As a file writes it: the root and the scale by their names.
    pub fn written(&self) -> Written {
        Written { root: self.root.to_string(), scale: self.scale.name().to_string() }
    }

    /// Its name, as the app says it: `E♭ major`, `F♯ minor`, `D Dorian`, `A harmonic minor`. A
    /// mode's name is a name, so it keeps its capital.
    pub fn name(&self) -> String {
        let mode = matches!(self.scale, Scale::Dorian | Scale::Phrygian | Scale::Lydian | Scale::Mixolydian | Scale::Locrian);
        let scale = if mode { self.scale.label().to_string() } else { self.scale.label().to_lowercase() };
        format!("{} {scale}", crate::scales::pc_name(self.pc()))
    }

    /// Its notes as `time.key` gives them (VOX-12): bit n is on when pitch class n (0 is C) is in
    /// the key.
    pub fn keyset(&self) -> u16 {
        self.notes().iter().fold(0, |set, &pc| set | 1 << pc)
    }
}

/// The pitch class of a root's name, from C: a letter, then `#`, `♯`, `b` or `♭`, or nothing.
fn pitch_class(name: &str) -> Option<usize> {
    let mut chars = name.trim().chars();
    let letter = match chars.next()?.to_ascii_uppercase() {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let shift = match (chars.next(), chars.next()) {
        (None, _) => 0,
        (Some('#' | '♯'), None) => 1,
        (Some('b' | '♭'), None) => 11,
        _ => return None,
    };
    Some((letter + shift) % 12)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scales::TABLE;

    // VOX-12: a key's keyset holds exactly its notes, from every root, in every scale of the table.
    #[test]
    fn every_key_has_its_keyset() {
        let key = |root: &str, scale: &str| SongKey::new(root, scale).unwrap().keyset();
        // C major: C D E F G A B.
        assert_eq!(key("C", "major"), 0b1010_1011_0101);
        // A minor has C major's notes.
        assert_eq!(key("A", "minor"), key("C", "major"));
        // E♭ major pentatonic: E♭ F G B♭ C.
        assert_eq!(key("Eb", "major-pentatonic"), 1 << 3 | 1 << 5 | 1 << 7 | 1 << 10 | 1);
        assert_eq!(key("F#", "chromatic"), EVERY_NOTE);
        for row in TABLE.iter() {
            for (pc, root) in ROOTS.iter().enumerate() {
                let set = key(root, row.name);
                let want = row.steps.iter().fold(0u16, |s, &st| s | 1 << ((pc + st as usize) % 12));
                assert_eq!(set, want, "{root} {}", row.name);
                assert_eq!(set.count_ones() as usize, row.steps.len());
            }
        }
    }
}
