//! L0 — music theory: keys, scales, chords and note names, in MIDI pitches so this
//! crate stays independent of the project model.

mod rng;

pub use rng::Rng;

use serde::{Deserialize, Serialize};

pub const NOTE_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Scale {
    #[default]
    Major,
    Minor,
    Dorian,
    Mixolydian,
    Lydian,
    Phrygian,
    HarmonicMinor,
    PentatonicMajor,
    PentatonicMinor,
    Chromatic,
}

impl Scale {
    pub const ALL: [Scale; 10] = [
        Scale::Major,
        Scale::Minor,
        Scale::Dorian,
        Scale::Mixolydian,
        Scale::Lydian,
        Scale::Phrygian,
        Scale::HarmonicMinor,
        Scale::PentatonicMajor,
        Scale::PentatonicMinor,
        Scale::Chromatic,
    ];

    pub fn intervals(self) -> &'static [u8] {
        match self {
            Scale::Major => &[0, 2, 4, 5, 7, 9, 11],
            Scale::Minor => &[0, 2, 3, 5, 7, 8, 10],
            Scale::Dorian => &[0, 2, 3, 5, 7, 9, 10],
            Scale::Mixolydian => &[0, 2, 4, 5, 7, 9, 10],
            Scale::Lydian => &[0, 2, 4, 6, 7, 9, 11],
            Scale::Phrygian => &[0, 1, 3, 5, 7, 8, 10],
            Scale::HarmonicMinor => &[0, 2, 3, 5, 7, 8, 11],
            Scale::PentatonicMajor => &[0, 2, 4, 7, 9],
            Scale::PentatonicMinor => &[0, 3, 5, 7, 10],
            Scale::Chromatic => &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Scale::Major => "major",
            Scale::Minor => "minor",
            Scale::Dorian => "dorian",
            Scale::Mixolydian => "mixolydian",
            Scale::Lydian => "lydian",
            Scale::Phrygian => "phrygian",
            Scale::HarmonicMinor => "harmonic minor",
            Scale::PentatonicMajor => "major pentatonic",
            Scale::PentatonicMinor => "minor pentatonic",
            Scale::Chromatic => "chromatic",
        }
    }

    pub fn parse(s: &str) -> Option<Scale> {
        let s = s.trim().to_ascii_lowercase().replace(['_', '-'], " ");
        Some(match s.as_str() {
            "major" | "maj" | "ionian" => Scale::Major,
            "minor" | "min" | "aeolian" | "natural minor" => Scale::Minor,
            "dorian" => Scale::Dorian,
            "mixolydian" => Scale::Mixolydian,
            "lydian" => Scale::Lydian,
            "phrygian" => Scale::Phrygian,
            "harmonic minor" => Scale::HarmonicMinor,
            "major pentatonic" | "pentatonic major" | "pentatonic" => Scale::PentatonicMajor,
            "minor pentatonic" | "pentatonic minor" => Scale::PentatonicMinor,
            "chromatic" => Scale::Chromatic,
            _ => return None,
        })
    }

    /// Seven-note scales support diatonic chords; pentatonic/chromatic borrow from major/minor.
    pub fn harmonic_parent(self) -> Scale {
        match self {
            Scale::PentatonicMajor | Scale::Chromatic => Scale::Major,
            Scale::PentatonicMinor => Scale::Minor,
            s => s,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Key {
    /// Pitch class of the tonic, 0 = C.
    pub root: u8,
    pub scale: Scale,
}

impl Default for Key {
    fn default() -> Self {
        // A minor: all white keys, a very common dance-pop key.
        Self { root: 9, scale: Scale::Minor }
    }
}

impl Key {
    pub fn new(root: u8, scale: Scale) -> Self {
        Self { root: root % 12, scale }
    }

    pub fn name(&self) -> String {
        format!("{} {}", NOTE_NAMES[self.root as usize % 12], self.scale.name())
    }

    /// Parse "A minor", "F# major", "Bb", "c#m".
    pub fn parse(s: &str) -> Option<Key> {
        let s = s.trim();
        let (root, rest) = parse_pitch_class(s)?;
        let rest = rest.trim();
        let scale = if rest.is_empty() {
            Scale::Major
        } else if rest == "m" {
            Scale::Minor
        } else {
            Scale::parse(rest)?
        };
        Some(Key::new(root, scale))
    }

    pub fn contains(&self, pitch: u8) -> bool {
        let pc = (pitch as i32 - self.root as i32).rem_euclid(12) as u8;
        self.scale.intervals().contains(&pc)
    }

    /// All MIDI pitches in `lo..=hi` that belong to the key, ascending.
    pub fn pitches_in_range(&self, lo: u8, hi: u8) -> Vec<u8> {
        (lo..=hi).filter(|p| self.contains(*p)).collect()
    }

    /// Nearest in-key pitch (ties resolve downward).
    pub fn snap(&self, pitch: u8) -> u8 {
        for d in 0..12i32 {
            for cand in [pitch as i32 - d, pitch as i32 + d] {
                if (0..=127).contains(&cand) && self.contains(cand as u8) {
                    return cand as u8;
                }
            }
        }
        pitch
    }

    /// Scale degree (0-based, may exceed 6 or be negative) → MIDI pitch, anchored at `octave_base`
    /// which should be the MIDI note of the tonic in the desired octave.
    pub fn degree_pitch(&self, degree: i32, octave_base: i32) -> i32 {
        let iv = self.scale.harmonic_parent().intervals();
        let n = iv.len() as i32;
        let oct = degree.div_euclid(n);
        let idx = degree.rem_euclid(n) as usize;
        octave_base + oct * 12 + iv[idx] as i32
    }

    /// MIDI note of the tonic in the octave starting at `octave_c` (e.g. 48 = C3).
    pub fn tonic_at(&self, octave_c: i32) -> i32 {
        octave_c + self.root as i32
    }

    /// Diatonic triad (or seventh) on a 0-based degree, as MIDI pitches rooted near `octave_c`.
    pub fn chord(&self, degree: i32, octave_c: i32, seventh: bool) -> Vec<i32> {
        let base = self.tonic_at(octave_c);
        let mut v = vec![self.degree_pitch(degree, base), self.degree_pitch(degree + 2, base), self.degree_pitch(degree + 4, base)];
        if seventh {
            v.push(self.degree_pitch(degree + 6, base));
        }
        v
    }
}

/// Parse a pitch class prefix like "C", "c#", "Bb", "F♯" and return (pc, remainder).
fn parse_pitch_class(s: &str) -> Option<(u8, &str)> {
    let mut chars = s.char_indices();
    let (_, c) = chars.next()?;
    let base: i32 = match c.to_ascii_uppercase() {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let mut pc = base;
    let mut rest_at = c.len_utf8();
    for (i, ch) in chars {
        match ch {
            '#' | '♯' => pc += 1,
            'b' | '♭' if i == rest_at => pc -= 1,
            _ => break,
        }
        rest_at = i + ch.len_utf8();
    }
    Some((pc.rem_euclid(12) as u8, &s[rest_at..]))
}

/// "C4" → 60 (C4 = middle C). Also accepts plain numbers.
pub fn parse_note(s: &str) -> Option<u8> {
    let s = s.trim();
    if let Ok(n) = s.parse::<i32>() {
        return (0..=127).contains(&n).then_some(n as u8);
    }
    let (pc, rest) = parse_pitch_class(s)?;
    let oct: i32 = rest.trim().parse().ok()?;
    let n = (oct + 1) * 12 + pc as i32;
    (0..=127).contains(&n).then_some(n as u8)
}

pub fn note_name(p: u8) -> String {
    format!("{}{}", NOTE_NAMES[p as usize % 12], p as i32 / 12 - 1)
}

pub fn midi_to_hz(p: f32) -> f32 {
    440.0 * 2f32.powf((p - 69.0) / 12.0)
}

/// Roman numeral for a 0-based degree in a key (uppercase = major triad).
pub fn roman(key: &Key, degree: i32) -> String {
    const R: [&str; 7] = ["I", "II", "III", "IV", "V", "VI", "VII"];
    let c = key.chord(degree, 60, false);
    let third = (c[1] - c[0]).rem_euclid(12);
    let fifth = (c[2] - c[0]).rem_euclid(12);
    let r = R[degree.rem_euclid(7) as usize];
    match (third, fifth) {
        (4, _) => r.to_string(),
        (3, 6) => format!("{}°", r.to_lowercase()),
        _ => r.to_lowercase(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsing() {
        assert_eq!(parse_note("C4"), Some(60));
        assert_eq!(parse_note("A4"), Some(69));
        assert_eq!(parse_note("Bb3"), Some(58));
        assert_eq!(Key::parse("F# minor"), Some(Key::new(6, Scale::Minor)));
        assert_eq!(Key::parse("Bb"), Some(Key::new(10, Scale::Major)));
        assert_eq!(Key::parse("c#m"), Some(Key::new(1, Scale::Minor)));
    }

    #[test]
    fn chords() {
        let c = Key::new(0, Scale::Major);
        assert_eq!(c.chord(0, 60, false), vec![60, 64, 67]);
        assert_eq!(c.chord(5, 48, false), vec![57, 60, 64]);
        assert_eq!(roman(&c, 5), "vi");
        assert_eq!(roman(&c, 4), "V");
        assert!(c.contains(62) && !c.contains(61));
        assert_eq!(c.snap(61), 60);
    }
}
