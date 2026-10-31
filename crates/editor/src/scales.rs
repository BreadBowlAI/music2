//! The scales music2 knows, in one table (NOTE-1), and what a key means: its notes, the nearest of
//! them to any pitch (NOTE-3), and the chords built on them (NOTE-4). A scale is the set of notes
//! that sound right together; a key is a scale from a root.
//!
//! Everything that knows a scale reads it here: `layout.json`'s key (FMT-19), the key field and
//! agents (`GET /api/theory`), and the roll, through what the timeline publishes of the song's key
//! ([`KeyView`], NOTE-2). The page works out no scale, snap or chord of its own.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::songkey::{ROOTS, SongKey};

/// A scale (NOTE-1). Files and requests name it by [`Scale::name`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "&'static str")]
pub enum Scale {
    Major,
    Minor,
    Dorian,
    Phrygian,
    Lydian,
    Mixolydian,
    Locrian,
    HarmonicMinor,
    MelodicMinor,
    MajorPentatonic,
    MinorPentatonic,
    Blues,
    Chromatic,
}

/// One row of the table: a scale's name as files write it, its label, what it sounds like, and
/// its steps, semitones from the root.
pub struct Row {
    pub scale: Scale,
    pub name: &'static str,
    pub label: &'static str,
    pub words: &'static str,
    pub steps: &'static [u8],
}

/// The table (NOTE-1), in the order the key field lists it.
pub const TABLE: [Row; 13] = [
    Row { scale: Scale::Major, name: "major", label: "Major", words: "Bright and happy", steps: &[0, 2, 4, 5, 7, 9, 11] },
    Row { scale: Scale::Minor, name: "minor", label: "Minor", words: "Darker, sadder", steps: &[0, 2, 3, 5, 7, 8, 10] },
    Row {
        scale: Scale::Dorian,
        name: "dorian",
        label: "Dorian",
        words: "Minor with a brighter sixth: soulful, funky",
        steps: &[0, 2, 3, 5, 7, 9, 10],
    },
    Row {
        scale: Scale::Phrygian,
        name: "phrygian",
        label: "Phrygian",
        words: "Minor with a low second: dark, Spanish",
        steps: &[0, 1, 3, 5, 7, 8, 10],
    },
    Row {
        scale: Scale::Lydian,
        name: "lydian",
        label: "Lydian",
        words: "Major with a raised fourth: dreamy, floating",
        steps: &[0, 2, 4, 6, 7, 9, 11],
    },
    Row {
        scale: Scale::Mixolydian,
        name: "mixolydian",
        label: "Mixolydian",
        words: "Major with a low seventh: bluesy rock",
        steps: &[0, 2, 4, 5, 7, 9, 10],
    },
    Row {
        scale: Scale::Locrian,
        name: "locrian",
        label: "Locrian",
        words: "Dark and restless, with no settled home",
        steps: &[0, 1, 3, 5, 6, 8, 10],
    },
    Row {
        scale: Scale::HarmonicMinor,
        name: "harmonic-minor",
        label: "Harmonic minor",
        words: "Minor with a raised seventh: classical, Middle Eastern",
        steps: &[0, 2, 3, 5, 7, 8, 11],
    },
    Row {
        scale: Scale::MelodicMinor,
        name: "melodic-minor",
        label: "Melodic minor",
        words: "Minor with a raised sixth and seventh: jazzy",
        steps: &[0, 2, 3, 5, 7, 9, 11],
    },
    Row {
        scale: Scale::MajorPentatonic,
        name: "major-pentatonic",
        label: "Major pentatonic",
        words: "Five notes of major: hard to play a wrong note",
        steps: &[0, 2, 4, 7, 9],
    },
    Row {
        scale: Scale::MinorPentatonic,
        name: "minor-pentatonic",
        label: "Minor pentatonic",
        words: "Five notes of minor: rock and blues solos",
        steps: &[0, 3, 5, 7, 10],
    },
    Row {
        scale: Scale::Blues,
        name: "blues",
        label: "Blues",
        words: "Minor pentatonic with one more note, the blue note",
        steps: &[0, 3, 5, 6, 7, 10],
    },
    Row {
        scale: Scale::Chromatic,
        name: "chromatic",
        label: "Chromatic",
        words: "All twelve notes: nothing is out of the key",
        steps: &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
    },
];

impl Scale {
    fn row(self) -> &'static Row {
        let row = &TABLE[self as usize];
        debug_assert_eq!(row.scale, self, "the table is in the enum's order");
        row
    }

    /// Its name as files write it: `harmonic-minor`.
    pub fn name(self) -> &'static str {
        self.row().name
    }

    /// Its label, as the app shows it: `Harmonic minor`.
    pub fn label(self) -> &'static str {
        self.row().label
    }

    /// Its notes, as semitones from the root.
    pub fn steps(self) -> &'static [u8] {
        self.row().steps
    }

    /// The scale written in any case, with spaces or `_` for `-`: `Harmonic Minor`.
    pub fn parse(text: &str) -> Option<Scale> {
        let name = text.trim().to_lowercase().replace([' ', '_'], "-");
        TABLE.iter().find(|r| r.name == name).map(|r| r.scale)
    }

    /// The scale of seven notes its chords are built from (NOTE-4): its own, or a pentatonic's or
    /// the blues' major or minor. None for chromatic, which has no degrees.
    pub fn chords(self) -> Option<Scale> {
        match self {
            Scale::MajorPentatonic => Some(Scale::Major),
            Scale::MinorPentatonic | Scale::Blues => Some(Scale::Minor),
            Scale::Chromatic => None,
            s => Some(s),
        }
    }

    /// Whether its third is minor (the library's "fits my song": a key or its relative). None for
    /// chromatic, which has both.
    pub fn minor(self) -> Option<bool> {
        let s = self.steps();
        match (s.contains(&3), s.contains(&4)) {
            (true, false) => Some(true),
            (false, true) => Some(false),
            _ => None,
        }
    }
}

impl TryFrom<String> for Scale {
    type Error = String;

    fn try_from(text: String) -> Result<Scale, String> {
        Scale::parse(&text).ok_or_else(|| {
            let names: Vec<&str> = TABLE.iter().map(|r| r.name).collect();
            format!("a key's scale is one of {}, not {text}", names.join(", "))
        })
    }
}

impl From<Scale> for &'static str {
    fn from(s: Scale) -> &'static str {
        s.name()
    }
}

/// A chord a stamp places (NOTE-4), counted in the key's notes from the root.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Chord {
    Triad,
    Seventh,
    Sus2,
    Sus4,
    Add9,
}

impl Chord {
    pub const ALL: [Chord; 5] = [Chord::Triad, Chord::Seventh, Chord::Sus2, Chord::Sus4, Chord::Add9];

    /// Its name in a request: `seventh`.
    pub fn name(self) -> &'static str {
        match self {
            Chord::Triad => "triad",
            Chord::Seventh => "seventh",
            Chord::Sus2 => "sus2",
            Chord::Sus4 => "sus4",
            Chord::Add9 => "add9",
        }
    }

    /// As the app shows it: `7th`.
    pub fn label(self) -> &'static str {
        match self {
            Chord::Triad => "Triad",
            Chord::Seventh => "7th",
            Chord::Sus2 => "Sus2",
            Chord::Sus4 => "Sus4",
            Chord::Add9 => "Add9",
        }
    }

    /// What it is, in plain words.
    pub fn words(self) -> &'static str {
        match self {
            Chord::Triad => "Three notes: the root, a third and a fifth",
            Chord::Seventh => "Four notes: a triad and a seventh, richer",
            Chord::Sus2 => "The root, a second and a fifth: open",
            Chord::Sus4 => "The root, a fourth and a fifth: waiting to move on",
            Chord::Add9 => "A triad with a ninth on top: bright",
        }
    }

    /// Its notes, as steps up the key's scale from the root (0): 1-3-5 is 0, 2, 4.
    fn steps(self) -> &'static [usize] {
        match self {
            Chord::Triad => &[0, 2, 4],
            Chord::Seventh => &[0, 2, 4, 6],
            Chord::Sus2 => &[0, 1, 4],
            Chord::Sus4 => &[0, 3, 4],
            Chord::Add9 => &[0, 2, 4, 8],
        }
    }

    /// How many inversions it has: one for each of its notes below a ninth.
    pub fn inversions(self) -> usize {
        self.steps().iter().filter(|s| **s < 7).count()
    }
}

/// What the timeline publishes of the song's key (NOTE-2): the key as the file says it, and what
/// it means, from the table.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct KeyView {
    #[serde(flatten)]
    pub key: SongKey,
    /// As the app says it: `E♭ major`, `A harmonic minor`.
    pub name: String,
    /// Its pitch classes (0 is C), from the root up.
    pub notes: Vec<u8>,
    /// For each pitch class from C, the semitones to the nearest note of the key, a tie going
    /// down (NOTE-3): 0 for the key's own notes.
    pub snap: [i8; 12],
    /// One for each note of the key that has a chord (NOTE-4), from the root up.
    pub chords: Vec<Degree>,
}

/// The chords on one note of a key.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Degree {
    /// Its root's pitch class (0 is C).
    pub root: u8,
    /// Its place in the key: `IV`, `ii`, `vii°`.
    pub numeral: String,
    /// Each kind of chord on it.
    pub kinds: BTreeMap<Chord, Stamp>,
}

/// One chord: its name, and its notes for each inversion, in semitones from the root's row.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Stamp {
    pub name: String,
    pub shapes: Vec<Vec<u8>>,
}

/// The highest pitch a note has; the lowest is 0.
const TOP: i32 = 127;

/// A pitch class as the app spells it on its own, as a key's root is spelled: `C♯`, `E♭`.
pub fn pc_name(pc: u8) -> String {
    let r = ROOTS[pc as usize % 12];
    r.chars()
        .enumerate()
        .map(|(i, c)| {
            if i == 1 && c == '#' {
                '♯'
            } else if i == 1 && c == 'b' {
                '♭'
            } else {
                c
            }
        })
        .collect()
}

impl SongKey {
    /// Its root's pitch class (0 is C).
    pub fn pc(&self) -> u8 {
        ROOTS.iter().position(|r| *r == self.root()).expect("a key's root is one of ROOTS, as SongKey::new keeps it") as u8
    }

    /// Its pitch classes, from the root up.
    pub fn notes(&self) -> Vec<u8> {
        self.scale().steps().iter().map(|s| (self.pc() + s) % 12).collect()
    }

    /// Its notes' names, from the root up. A note of its chords' scale of seven notes gets that
    /// scale's own letter, as a chord chart writes it (`G♯` in A harmonic minor, `B♭` in F major);
    /// another (the blues' blue note, a chromatic key's) is spelled as on its own.
    pub fn names(&self) -> Vec<String> {
        self.notes().iter().map(|pc| self.spelled(*pc)).collect()
    }

    /// Pitch class `pc` spelled in the key ([`SongKey::names`]).
    fn spelled(&self, pc: u8) -> String {
        const LETTERS: &str = "CDEFGAB";
        const NATURAL: [u8; 7] = [0, 2, 4, 5, 7, 9, 11];
        let Some(scale) = self.scale().chords() else { return pc_name(pc) };
        let Some(i) = scale.steps().iter().position(|s| (self.pc() + s) % 12 == pc) else { return pc_name(pc) };
        let first = LETTERS.find(&self.root()[..1]).expect("a root starts with its letter");
        let letter = (first + i) % 7;
        let sign = match (pc + 12 - NATURAL[letter]) % 12 {
            0 => "",
            1 => "♯",
            2 => "𝄪",
            11 => "♭",
            10 => "𝄫",
            _ => return pc_name(pc),
        };
        format!("{}{sign}", &LETTERS[letter..=letter])
    }

    /// Whether pitch `p` is a note of the key.
    pub fn has(&self, p: i32) -> bool {
        self.notes().contains(&(p.rem_euclid(12) as u8))
    }

    /// The note of the key nearest pitch `p` (rounded), between 0 and 127: a pitch exactly between
    /// two of them goes down to the lower (NOTE-3).
    pub fn snap(&self, p: f64) -> f64 {
        let p = (p.round() as i32).clamp(0, TOP);
        let fits = |q: i32| (0..=TOP).contains(&q) && self.has(q);
        let to = (0..=12).find_map(|d| [p - d, p + d].into_iter().find(|q| fits(*q))).expect("a key has a note in every octave");
        to as f64
    }

    /// [`SongKey::snap`] as a table: for each pitch class from C, the semitones to its note.
    pub fn snap_table(&self) -> [i8; 12] {
        std::array::from_fn(|pc| (self.snap(60.0 + pc as f64) - (60.0 + pc as f64)) as i8)
    }

    /// The chords on the key's notes (NOTE-4), from the root up: none for a scale with no chords
    /// (chromatic), and none on a note its chords' scale doesn't have (the blues' blue note).
    pub fn chords(&self) -> Vec<Degree> {
        let Some(scale) = self.scale().chords() else { return vec![] };
        let steps = scale.steps();
        let n = steps.len();
        // The scale's kth note up from the root, in semitones; past the seventh, an octave up.
        let note = |k: usize| steps[k % n] as i32 + 12 * (k / n) as i32;
        let notes = self.notes();
        (0..n)
            .filter_map(|i| {
                let root = (self.pc() + steps[i]) % 12;
                if !notes.contains(&root) {
                    return None;
                }
                let kinds = Chord::ALL
                    .iter()
                    .map(|c| {
                        let offsets: Vec<u8> = c.steps().iter().map(|s| (note(i + s) - note(i)) as u8).collect();
                        let shapes = (0..c.inversions())
                            .map(|inv| {
                                let mut v: Vec<u8> = offsets.iter().enumerate().map(|(j, o)| if j < inv { o + 12 } else { *o }).collect();
                                v.sort_unstable();
                                v
                            })
                            .collect();
                        (*c, Stamp { name: format!("{}{}", self.spelled(root), suffix(*c, &offsets)), shapes })
                    })
                    .collect::<BTreeMap<_, _>>();
                let triad = &kinds[&Chord::Triad].shapes[0];
                Some(Degree { root, numeral: numeral(i + 1, triad[1], triad[2]), kinds })
            })
            .collect()
    }

    /// The notes of chord `chord` on root `root`, a note of the key with a chord, in inversion
    /// `inversion` (NOTE-4): what the `stamp` gesture places. Refused in words when the root has
    /// no chord or a note would be above the highest.
    pub fn stamp(&self, root: f64, chord: Chord, inversion: usize) -> Result<Vec<f64>, String> {
        let p = root.round() as i32;
        let pc = p.rem_euclid(12) as u8;
        let names = || self.names().join(", ");
        let Some(d) = self.chords().into_iter().find(|d| d.root == pc) else {
            return Err(match self.scale().chords() {
                None => format!("{} has no chords to stamp: pick a key with a scale of seven notes", self.name()),
                Some(_) if self.has(p) => format!("{} has no chord on {}, its blue note", self.name(), self.spelled(pc)),
                Some(_) => format!("{} isn't in {}: a chord is built on one of its notes ({})", pc_name(pc), self.name(), names()),
            });
        };
        let stamp = &d.kinds[&chord];
        let shape = stamp.shapes.get(inversion).ok_or_else(|| {
            format!("{} has {} inversions, 0 to {}, not {inversion}", stamp.name, stamp.shapes.len(), stamp.shapes.len() - 1)
        })?;
        let notes: Vec<f64> = shape.iter().map(|o| (p + *o as i32) as f64).collect();
        if notes.iter().any(|n| *n > TOP as f64) {
            return Err(format!("{} from {} goes above the highest note, G9", stamp.name, root));
        }
        Ok(notes)
    }

    /// What the timeline publishes of it (NOTE-2).
    pub fn view(&self) -> KeyView {
        KeyView { key: self.clone(), name: self.name(), notes: self.notes(), snap: self.snap_table(), chords: self.chords() }
    }
}

/// A chord's name after its root, from its notes in semitones from the root (`offsets`, in root
/// position) and its kind: `m`, `dim`, `7`, `maj7`, `m7♭5`, `sus2`, `add9`.
fn suffix(chord: Chord, offsets: &[u8]) -> String {
    let fifth = |f: u8| match f {
        6 => "♭5",
        8 => "♯5",
        _ => "",
    };
    // The triad's sound, by its third and its fifth.
    let triad = |third: u8, f: u8| -> String {
        match (third, f) {
            (4, 7) => "".into(),
            (3, 7) => "m".into(),
            (3, 6) => "dim".into(),
            (4, 8) => "aug".into(),
            (3, _) => format!("m({})", fifth(f)),
            _ => format!("({})", fifth(f)),
        }
    };
    match chord {
        Chord::Triad => triad(offsets[1], offsets[2]),
        Chord::Seventh => match (triad(offsets[1], offsets[2]).as_str(), offsets[3]) {
            ("", 11) => "maj7".into(),
            ("", 10) => "7".into(),
            ("m", 10) => "m7".into(),
            ("m", 11) => "m(maj7)".into(),
            ("dim", 10) => "m7♭5".into(),
            ("dim", 9) => "dim7".into(),
            ("aug", 11) => "aug(maj7)".into(),
            ("aug", 10) => "aug7".into(),
            (t, s) => format!("{t}({})", ["6", "♭7", "7"][(s as usize).clamp(9, 11) - 9]),
        },
        Chord::Sus2 | Chord::Sus4 => {
            let sus = match offsets[1] {
                1 => "sus♭2",
                2 => "sus2",
                5 => "sus4",
                _ => "sus♯4",
            };
            format!("{sus}{}", fifth(offsets[2]))
        }
        Chord::Add9 => {
            let nine = match offsets[3] {
                13 => "add♭9",
                15 => "add♯9",
                _ => "add9",
            };
            match triad(offsets[1], offsets[2]) {
                t if t.is_empty() => nine.into(),
                t => format!("{t}({nine})"),
            }
        }
    }
}

/// A chord's place in its key as a numeral: upper case for a major triad, lower for a minor one,
/// with ° for diminished and + for augmented.
fn numeral(degree: usize, third: u8, fifth: u8) -> String {
    let n = ["I", "II", "III", "IV", "V", "VI", "VII"][degree - 1];
    match (third, fifth) {
        (3, 6) => format!("{}°", n.to_lowercase()),
        (3, _) => n.to_lowercase(),
        (4, 8) => format!("{n}+"),
        _ => n.into(),
    }
}

#[cfg(test)]
#[path = "scales_tests.rs"]
mod tests;
