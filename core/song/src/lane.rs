//! Lane files: `data/<module>/<lane>.json`, one lane's clips (FMT-10, FMT-11).
//!
//! ```json
//! {"clips": [
//!   {"id": "k3f9q2zt", "start": 0, "length": 16, "notes": [
//!     {"pitch": "A1", "start": 0, "length": 0.5, "velocity": 0.9}
//!   ]}
//! ]}
//! {"unit": "hz", "clips": [
//!   {"id": "q7m2c8xa", "start": 32, "length": 8, "points": [[0, 500], [8, 2500]]}
//! ]}
//! {"clips": [
//!   {"id": "z1b4r9td", "start": 64, "length": 4, "asset": "vox/lead.wav", "hash": "9f2c3a1be07d44c1", "offset": 0, …}
//! ]}
//! {"unit": "choice", "clips": [
//!   {"id": "m4k8w2qe", "start": 0, "length": 8, "points": [[0, "saw"], [4, "square"]]}
//! ]}
//! ```
//!
//! A file carries no kind: it is read as the kind its lane declares, and notes and audio clips
//! fail as the wrong shape. A file with no clips fits any kind. Times are in beats; a clip's
//! notes and points are relative to its start.
//!
//! Every reader puts clips and notes in one order before use, and [`canonical`] writes them in
//! it (FMT-8a), so the order of an array never changes the sound.

use std::cmp::Ordering;
use std::fmt::Write as _;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[cfg(test)]
mod oracle;
#[cfg(test)]
mod oracle_tests;

/// What a note written without a velocity plays at.
pub const VELOCITY: f64 = 0.8;

const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NoteClip {
    /// The clip's id as written, if any (FMT-12). One that breaks the id rule is kept as
    /// written and read as anonymous.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// An id written as anything but a string (`5`, `true`): kept as written, so a rewrite keeps
    /// it, and read as anonymous (FMT-12). Never sent or received.
    #[serde(skip)]
    pub other_id: Option<Value>,
    pub start: f64,
    pub length: f64,
    pub notes: Vec<Note>,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Note {
    /// Semitones (60 = middle C), or a name like `"A1"` or `"C#4"`.
    #[serde(deserialize_with = "pitch")]
    pub pitch: f64,
    pub start: f64,
    pub length: f64,
    #[serde(default = "default_velocity")]
    pub velocity: f64,
}

fn default_velocity() -> f64 {
    VELOCITY
}

/// A number, or a name read by [`parse_pitch`]. A visitor rather than an untagged enum, which
/// tried the number first and made an error for every named pitch; what it says for anything else
/// is that enum's message, so a file's errors read as they did.
fn pitch<'de, D: serde::Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    struct Pitch;
    /// What the untagged enum said for a value that is neither.
    const NEITHER: &str = "data did not match any variant of untagged enum P";
    impl<'de> serde::de::Visitor<'de> for Pitch {
        type Value = f64;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a pitch: 60, or \"C4\"")
        }
        fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<f64, E> {
            Ok(v)
        }
        fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<f64, E> {
            Ok(v as f64)
        }
        fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<f64, E> {
            Ok(v as f64)
        }
        fn visit_str<E: serde::de::Error>(self, s: &str) -> Result<f64, E> {
            parse_pitch(s).ok_or_else(|| E::custom(format!("bad pitch `{s}` (try 60 or \"C4\")")))
        }
        fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<f64, E> {
            Err(E::custom(NEITHER))
        }
        fn visit_unit<E: serde::de::Error>(self) -> Result<f64, E> {
            Err(E::custom(NEITHER))
        }
        fn visit_none<E: serde::de::Error>(self) -> Result<f64, E> {
            Err(E::custom(NEITHER))
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, _: A) -> Result<f64, A::Error> {
            Err(serde::de::Error::custom(NEITHER))
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(self, _: A) -> Result<f64, A::Error> {
            Err(serde::de::Error::custom(NEITHER))
        }
    }
    d.deserialize_any(Pitch)
}

/// `"C4"` → 60, `"A#1"` → 34, `"Bb3"` → 58.
pub fn parse_pitch(s: &str) -> Option<f64> {
    let s = s.trim();
    let mut chars = s.chars();
    let base = match chars.next()?.to_ascii_uppercase() {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let rest = chars.as_str();
    let (acc, oct) = if let Some(r) = rest.strip_prefix('#') {
        (1, r)
    } else if let Some(r) = rest.strip_prefix('b') {
        (-1, r)
    } else {
        (0, rest)
    };
    let octave: i32 = oct.parse().ok()?;
    Some(((octave + 1) * 12 + base + acc) as f64)
}

/// A pitch's name, `33` → `A1`, if it is a whole MIDI note.
pub fn pitch_name(p: f64) -> Option<String> {
    name_of(p).map(str::to_string)
}

/// Every whole MIDI note's name, made once: what the writer puts for each note.
static PITCH_NAMES: LazyLock<Vec<String>> = LazyLock::new(|| (0..128).map(|p| format!("{}{}", NAMES[p % 12], p as i32 / 12 - 1)).collect());

fn name_of(p: f64) -> Option<&'static str> {
    (p.fract() == 0.0 && (0.0..=127.0).contains(&p)).then(|| PITCH_NAMES[p as usize].as_str())
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ControlClip {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// As [`NoteClip::other_id`].
    #[serde(skip)]
    pub other_id: Option<Value>,
    pub start: f64,
    pub length: f64,
    /// `[beat, value]` pairs, linear in between, held flat before the first and after the last.
    pub points: Vec<(f64, f64)>,
}

/// A point's value on a lane of names (CARD-15): an option's name, or on or off, as written.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Step {
    Name(String),
    Switch(bool),
}

impl<'de> Deserialize<'de> for Step {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Step, D::Error> {
        match Value::deserialize(d)? {
            Value::String(s) => Ok(Step::Name(s)),
            Value::Bool(b) => Ok(Step::Switch(b)),
            v => Err(serde::de::Error::custom(format!("a point holds an option's name, or true or false, not {v}"))),
        }
    }
}

/// A clip on a lane of names: a choice's or a switch's (CARD-15).
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StepClip {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// As [`NoteClip::other_id`].
    #[serde(skip)]
    pub other_id: Option<Value>,
    pub start: f64,
    pub length: f64,
    /// `[beat, value]` pairs: each value holds until the next, the first from the clip's start.
    pub points: Vec<(f64, Step)>,
}

/// A clip on an audio lane: part of an asset, placed on the timeline.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AudioClip {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// As [`NoteClip::other_id`].
    #[serde(skip)]
    pub other_id: Option<Value>,
    /// Position and length on the timeline, in beats.
    pub start: f64,
    pub length: f64,
    /// A file in the project's `assets/` folder (WAV), by its exact name (FMT-14).
    pub asset: String,
    /// The first 16 hex digits of the SHA-256 of the asset's bytes (FMT-15).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    /// Where in the asset the clip starts, in seconds.
    #[serde(default)]
    pub offset: f64,
    #[serde(default)]
    pub gain_db: f64,
    /// Fade lengths in seconds.
    #[serde(default)]
    pub fade_in: f64,
    #[serde(default)]
    pub fade_out: f64,
    /// Play the clip backwards (the audio as placed, reversed).
    #[serde(default)]
    pub reverse: bool,
    /// Transpose in semitones. Without warp markers this is varispeed (faster and
    /// higher together); with warp markers, timing follows the markers and only the
    /// pitch changes.
    #[serde(default)]
    pub pitch: f64,
    /// Warp markers `[source_seconds, beat]`, beats relative to the clip start. The
    /// audio between markers is time-stretched (keeping its pitch) so each marked
    /// moment lands on its beat, following the tempo map.
    #[serde(default)]
    pub warp: Vec<(f64, f64)>,
}

/// The kind a lane is declared with: how its file is read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LaneKind {
    Notes,
    /// `unit` is the declared unit's spelling; `None` accepts any.
    Control {
        unit: Option<String>,
    },
    /// A lane that feeds a choice (CARD-15): its points hold names, read against `options`.
    Choice {
        options: Vec<String>,
    },
    /// A lane that feeds a switch: its points hold true or false.
    Switch,
    Audio,
}

/// One lane's clips, read as its kind and in canonical order.
#[derive(Clone, Debug, PartialEq)]
pub enum LaneData {
    Notes(Vec<NoteClip>),
    Control { unit: Option<String>, clips: Vec<ControlClip> },
    /// A lane of names (CARD-15): a choice's, read against `options`, or a switch's (`None`).
    /// Its file records the unit `choice` or `switch`.
    Steps { options: Option<Vec<String>>, clips: Vec<StepClip> },
    Audio(Vec<AudioClip>),
}

/// Why a lane file can't be played.
#[derive(Clone, Debug, PartialEq)]
pub enum LaneError {
    /// It doesn't parse, or doesn't fit its lane: `lane.invalid`. Names the field.
    Invalid(String),
    /// A control lane's unit isn't the declared one: `lane.unit-mismatch`.
    Unit { found: Option<String>, declared: String },
}

impl LaneData {
    pub fn empty(kind: &LaneKind) -> LaneData {
        match kind {
            LaneKind::Notes => LaneData::Notes(vec![]),
            LaneKind::Control { unit } => LaneData::Control { unit: unit.clone(), clips: vec![] },
            LaneKind::Choice { options } => LaneData::Steps { options: Some(options.clone()), clips: vec![] },
            LaneKind::Switch => LaneData::Steps { options: None, clips: vec![] },
            LaneKind::Audio => LaneData::Audio(vec![]),
        }
    }

    /// Whether this data was read as `kind` (and, for a control lane, in its unit).
    pub fn fits(&self, kind: &LaneKind) -> bool {
        match (self, kind) {
            (LaneData::Notes(_), LaneKind::Notes) | (LaneData::Audio(_), LaneKind::Audio) => true,
            (LaneData::Control { unit, .. }, LaneKind::Control { unit: want }) => want.is_none() || unit == want,
            (LaneData::Steps { options: Some(o), .. }, LaneKind::Choice { options }) => o == options,
            (LaneData::Steps { options: None, .. }, LaneKind::Switch) => true,
            _ => false,
        }
    }

    /// Read a lane file as `kind` (FMT-8, FMT-10). Clips come back in canonical order.
    pub fn read(text: &str, kind: &LaneKind) -> Result<LaneData, LaneError> {
        let invalid = LaneError::Invalid;
        let v: Value = serde_json::from_str(text).map_err(|e| invalid(e.to_string()))?;
        let Value::Object(top) = v else { return Err(invalid("a lane file is an object: {\"clips\": [...]}".into())) };
        let clips = match top.get("clips") {
            Some(Value::Array(c)) => c,
            Some(_) => return Err(invalid("`clips` must be a list".into())),
            None => return Err(invalid("missing field `clips`".into())),
        };
        let unit = match top.get("unit") {
            None => None,
            Some(Value::String(u)) => Some(u.clone()),
            Some(_) => return Err(invalid("`unit` must be a unit's name, such as \"hz\"".into())),
        };
        if let Some(k) = top.keys().find(|k| !matches!(k.as_str(), "clips" | "unit")) {
            return Err(invalid(format!("unknown field `{k}`, expected `clips` or `unit`")));
        }
        // A file with no clips fits any kind (FMT-10).
        if clips.is_empty() {
            return Ok(match kind {
                LaneKind::Control { unit: declared } => LaneData::Control { unit: unit.or_else(|| declared.clone()), clips: vec![] },
                _ => LaneData::empty(kind),
            });
        }
        match kind {
            LaneKind::Choice { .. } | LaneKind::Switch => {
                let (declared, options) = match kind {
                    LaneKind::Choice { options } => ("choice", Some(options.clone())),
                    _ => ("switch", None),
                };
                if unit.as_deref() != Some(declared) {
                    return Err(LaneError::Unit { found: unit, declared: declared.into() });
                }
                let mut clips = each::<StepClip>(clips, kind)?;
                for (i, c) in clips.iter_mut().enumerate() {
                    let wrong = c.points.iter().position(|(_, p)| matches!(p, Step::Name(_)) != options.is_some());
                    if let Some(j) = wrong {
                        let what = if options.is_some() { "an option's name, as [4, \"square\"]" } else { "true or false, as [4, true]" };
                        return Err(invalid(format!("{}: point {}: a point of {} holds {what}", at(i, c.start), j + 1, noun(kind))));
                    }
                    c.points.sort_by(|a, b| a.0.total_cmp(&b.0));
                }
                Ok(LaneData::Steps { options, clips: sorted(clips, |c| (c.start, c.id.as_deref())) })
            }
            LaneKind::Notes | LaneKind::Audio if unit.is_some() => {
                Err(invalid(format!("unknown field `unit`: only automation lanes have a unit, and this is {}", noun(kind))))
            }
            LaneKind::Notes => {
                let mut clips = each::<NoteClip>(clips, kind)?;
                for c in &mut clips {
                    c.notes.sort_by(note_order);
                }
                Ok(LaneData::Notes(sorted(clips, |c| (c.start, c.id.as_deref()))))
            }
            LaneKind::Control { unit: declared } => {
                if let Some(d) = declared
                    && unit.as_ref() != Some(d)
                {
                    return Err(LaneError::Unit { found: unit, declared: d.clone() });
                }
                let mut clips = each::<ControlClip>(clips, kind)?;
                for c in &mut clips {
                    // Stable: two points at one beat are a step, in the order written.
                    c.points.sort_by(|a, b| a.0.total_cmp(&b.0));
                }
                Ok(LaneData::Control { unit, clips: sorted(clips, |c| (c.start, c.id.as_deref())) })
            }
            LaneKind::Audio => {
                let mut clips = each::<AudioClip>(clips, kind)?;
                for (i, c) in clips.iter_mut().enumerate() {
                    let at = at(i, c.start);
                    if c.length <= 0.0 || c.offset < 0.0 || c.fade_in < 0.0 || c.fade_out < 0.0 {
                        return Err(invalid(format!(
                            "{at}: `length` must be above 0, and `offset`, `fade_in` and `fade_out` can't be negative"
                        )));
                    }
                    c.warp.sort_by(|a, b| a.1.total_cmp(&b.1));
                    if c.warp.windows(2).any(|w| w[1].0 <= w[0].0 || w[1].1 <= w[0].1) {
                        return Err(invalid(format!("{at}: `warp` markers must move forward in both source time and beats")));
                    }
                }
                Ok(LaneData::Audio(sorted(clips, |c| (c.start, c.id.as_deref()))))
            }
        }
    }

    /// The ids written as strings, in canonical clip order.
    pub fn ids(&self) -> Vec<Option<&str>> {
        match self {
            LaneData::Notes(c) => c.iter().map(|c| c.id.as_deref()).collect(),
            LaneData::Control { clips, .. } => clips.iter().map(|c| c.id.as_deref()).collect(),
            LaneData::Steps { clips, .. } => clips.iter().map(|c| c.id.as_deref()).collect(),
            LaneData::Audio(c) => c.iter().map(|c| c.id.as_deref()).collect(),
        }
    }

    /// The ids written as anything but a string, in canonical clip order (FMT-12).
    pub fn other_ids(&self) -> Vec<Option<&Value>> {
        match self {
            LaneData::Notes(c) => c.iter().map(|c| c.other_id.as_ref()).collect(),
            LaneData::Control { clips, .. } => clips.iter().map(|c| c.other_id.as_ref()).collect(),
            LaneData::Steps { clips, .. } => clips.iter().map(|c| c.other_id.as_ref()).collect(),
            LaneData::Audio(c) => c.iter().map(|c| c.other_id.as_ref()).collect(),
        }
    }

    /// Give clip `i` (in canonical order) the id `id`, in place of any it had.
    pub fn set_id(&mut self, i: usize, id: String) {
        let (slot, other) = match self {
            LaneData::Notes(c) => {
                let c = &mut c[i];
                (&mut c.id, &mut c.other_id)
            }
            LaneData::Control { clips, .. } => {
                let c = &mut clips[i];
                (&mut c.id, &mut c.other_id)
            }
            LaneData::Steps { clips, .. } => {
                let c = &mut clips[i];
                (&mut c.id, &mut c.other_id)
            }
            LaneData::Audio(c) => {
                let c = &mut c[i];
                (&mut c.id, &mut c.other_id)
            }
        };
        (*slot, *other) = (Some(id), None);
    }

    pub fn is_empty(&self) -> bool {
        self.ids().is_empty()
    }

    /// The beat where the last clip ends.
    pub fn end(&self) -> f64 {
        let ends: Vec<f64> = match self {
            LaneData::Notes(c) => c.iter().map(|c| c.start + c.length).collect(),
            LaneData::Control { clips, .. } => clips.iter().map(|c| c.start + c.length).collect(),
            LaneData::Steps { clips, .. } => clips.iter().map(|c| c.start + c.length).collect(),
            LaneData::Audio(c) => c.iter().map(|c| c.start + c.length).collect(),
        };
        ends.into_iter().fold(0.0, f64::max)
    }

    /// The kind this data was read as.
    pub fn kind(&self) -> LaneKind {
        match self {
            LaneData::Notes(_) => LaneKind::Notes,
            LaneData::Control { unit, .. } => LaneKind::Control { unit: unit.clone() },
            LaneData::Steps { options: Some(options), .. } => LaneKind::Choice { options: options.clone() },
            LaneData::Steps { options: None, .. } => LaneKind::Switch,
            LaneData::Audio(_) => LaneKind::Audio,
        }
    }

    /// Each clip's id as written (a string, valid or not) and its line in the canonical form, in
    /// canonical order: what the `Clips` op compares byte for byte (WRITE-3).
    pub fn clip_lines(&self) -> Vec<(Option<&str>, String)> {
        match self {
            LaneData::Notes(c) => c.iter().map(|c| (c.id.as_deref(), c.line())).collect(),
            LaneData::Control { clips, .. } => clips.iter().map(|c| (c.id.as_deref(), c.line())).collect(),
            LaneData::Steps { clips, .. } => clips.iter().map(|c| (c.id.as_deref(), c.line())).collect(),
            LaneData::Audio(c) => c.iter().map(|c| (c.id.as_deref(), c.line())).collect(),
        }
    }

    /// The canonical lines of the clips whose id, as written, is `id`: [`LaneData::clip_lines`]
    /// for those clips only.
    pub fn lines_of(&self, id: &str) -> Vec<String> {
        fn of<T: Clip>(clips: &[T], ids: Vec<Option<&str>>, id: &str) -> Vec<String> {
            clips.iter().zip(ids).filter(|(_, i)| *i == Some(id)).map(|(c, _)| c.line()).collect()
        }
        match self {
            LaneData::Notes(c) => of(c, self.ids(), id),
            LaneData::Control { clips, .. } => of(clips, self.ids(), id),
            LaneData::Steps { clips, .. } => of(clips, self.ids(), id),
            LaneData::Audio(c) => of(c, self.ids(), id),
        }
    }

    /// How many clips have `id`, as written.
    pub fn clips_with(&self, id: &str) -> usize {
        self.ids().into_iter().filter(|i| *i == Some(id)).count()
    }

    /// The clip whose id is `id`, as its JSON in the lane format's schema: the last, if two have
    /// it, as a map of the clips by id keeps it.
    pub fn clip_value(&self, id: &str) -> Option<Value> {
        let i = self.ids().iter().rposition(|i| *i == Some(id))?;
        let v = match self {
            LaneData::Notes(c) => serde_json::to_value(&c[i]),
            LaneData::Control { clips, .. } => serde_json::to_value(&clips[i]),
            LaneData::Steps { clips, .. } => serde_json::to_value(&clips[i]),
            LaneData::Audio(c) => serde_json::to_value(&c[i]),
        };
        Some(v.expect("a clip serialises"))
    }

    /// Whether a read of this data's canonical text as `kind` gives exactly this data, every
    /// number to the bit (FMT-8a): what lets a commit's candidate play the data its plan made
    /// instead of parsing the text it wrote (PLANE-1). Data read from a file, or made from such
    /// data by [`LaneData::put_clips`], reads back unless a number is `-0` (written `0`) or a
    /// control lane's unit isn't one a read of it as `kind` keeps; data with a number JSON can't
    /// hold (infinite, not a number) never does.
    pub fn reads_back(&self, kind: &LaneKind) -> bool {
        let unit = match (self, kind) {
            (LaneData::Control { unit, clips }, LaneKind::Control { unit: declared }) => match clips.is_empty() {
                // No clips: a read takes the file's unit, else the declared one.
                true => unit.is_some() || declared.is_none(),
                false => declared.is_none() || unit == declared,
            },
            _ => self.fits(kind),
        };
        unit && !self.has_unwritten()
    }

    /// Whether any number isn't written as it is: `-0`, which the canonical form writes as `0`,
    /// or one JSON can't hold.
    fn has_unwritten(&self) -> bool {
        let neg = |x: &f64| !x.is_finite() || (*x == 0.0 && x.is_sign_negative());
        let pairs = |ps: &[(f64, f64)]| ps.iter().any(|(a, b)| neg(a) || neg(b));
        match self {
            LaneData::Notes(c) => c.iter().any(|c| {
                neg(&c.start) || neg(&c.length) || c.notes.iter().any(|n| [n.pitch, n.start, n.length, n.velocity].iter().any(neg))
            }),
            LaneData::Control { clips, .. } => clips.iter().any(|c| neg(&c.start) || neg(&c.length) || pairs(&c.points)),
            LaneData::Steps { clips, .. } => clips.iter().any(|c| neg(&c.start) || neg(&c.length) || c.points.iter().any(|(b, _)| neg(b))),
            LaneData::Audio(c) => {
                c.iter().any(|c| [c.start, c.length, c.offset, c.gain_db, c.fade_in, c.fade_out, c.pitch].iter().any(neg) || pairs(&c.warp))
            }
        }
    }

    /// Whether every clip can be addressed by its id: each has a valid id, written as a string,
    /// and no id repeats (FMT-12). A lane that isn't is written whole.
    pub fn addressable(&self) -> bool {
        let mut seen = std::collections::BTreeSet::new();
        let mut usable = |id: &Option<&str>| id.is_some_and(|i| crate::ClipId::valid(i) && seen.insert(i.to_string()));
        self.ids().iter().zip(self.other_ids()).all(|(id, other)| other.is_none() && usable(id))
    }

    /// The `Clips` op on this data (WRITE-3): each clip of `put` (whole, in the lane format's
    /// schema) replaces the clip with its id, or is added; each id of `delete` goes, and one that
    /// isn't here is fine. Clips without a valid id are kept as written (FMT-12). Why not, naming
    /// the clip, when a clip of `put` doesn't read as this lane's kind.
    pub fn put_clips(&mut self, put: &[Value], delete: &[String]) -> Result<(), String> {
        let mut top = Map::new();
        if let Some(u) = self.unit() {
            top.insert("unit".into(), Value::String(u.to_string()));
        }
        top.insert("clips".into(), Value::Array(put.to_vec()));
        let new = LaneData::read(&Value::Object(top).to_string(), &self.kind()).map_err(|e| match e {
            LaneError::Invalid(why) => why,
            LaneError::Unit { declared, .. } => format!("the clips must be automation in `{declared}`"),
        })?;
        let gone: std::collections::BTreeSet<String> =
            new.ids().into_iter().flatten().map(str::to_string).chain(delete.iter().cloned()).collect();
        let keep = |id: &Option<String>| !id.as_ref().is_some_and(|i| gone.contains(i));
        match (&mut *self, new) {
            (LaneData::Notes(a), LaneData::Notes(b)) => {
                a.retain(|c| keep(&c.id));
                a.extend(b);
            }
            (LaneData::Control { clips: a, .. }, LaneData::Control { clips: b, .. }) => {
                a.retain(|c| keep(&c.id));
                a.extend(b);
            }
            (LaneData::Steps { clips: a, .. }, LaneData::Steps { clips: b, .. }) => {
                a.retain(|c| keep(&c.id));
                a.extend(b);
            }
            (LaneData::Audio(a), LaneData::Audio(b)) => {
                a.retain(|c| keep(&c.id));
                a.extend(b);
            }
            _ => unreachable!("read as this data's own kind"),
        }
        self.sort();
        Ok(())
    }

    /// Put clips back in canonical order, after an edit.
    pub fn sort(&mut self) {
        match self {
            LaneData::Notes(c) => {
                for c in c.iter_mut() {
                    c.notes.sort_by(note_order);
                }
                *c = sorted(std::mem::take(c), |c| (c.start, c.id.as_deref()));
            }
            LaneData::Control { clips, .. } => *clips = sorted(std::mem::take(clips), |c| (c.start, c.id.as_deref())),
            LaneData::Steps { clips, .. } => *clips = sorted(std::mem::take(clips), |c| (c.start, c.id.as_deref())),
            LaneData::Audio(c) => *c = sorted(std::mem::take(c), |c| (c.start, c.id.as_deref())),
        }
    }

    /// The unit its file records: an automation lane's, or `choice` or `switch` (FMT-10, CARD-15).
    pub fn unit(&self) -> Option<&str> {
        match self {
            LaneData::Control { unit, .. } => unit.as_deref(),
            LaneData::Steps { options, .. } => Some(if options.is_some() { "choice" } else { "switch" }),
            LaneData::Notes(_) | LaneData::Audio(_) => None,
        }
    }
}

fn noun(kind: &LaneKind) -> &'static str {
    match kind {
        LaneKind::Notes => "a notes lane",
        LaneKind::Control { .. } => "an automation lane",
        LaneKind::Choice { .. } => "a choice's lane",
        LaneKind::Switch => "a switch's lane",
        LaneKind::Audio => "an audio lane",
    }
}

/// `clip 3 (at beat 16)`.
fn at(i: usize, start: f64) -> String {
    format!("clip {} (at beat {})", i + 1, number(start))
}

/// Each clip parsed on its own, so an error names the clip, and for a note, the note. An id
/// written as anything but a string is kept aside as written, and the clip read as anonymous:
/// one hand edit must never make a lane unreadable (FMT-12).
fn each<T: for<'de> Deserialize<'de> + Clip>(clips: &[Value], kind: &LaneKind) -> Result<Vec<T>, LaneError> {
    let mut out = Vec::with_capacity(clips.len());
    for (i, c) in clips.iter().enumerate() {
        // Read from the parsed value in place; only a clip whose id is set aside is copied.
        let other = c.as_object().and_then(|o| o.get("id")).filter(|id| !id.is_string() && !id.is_null()).cloned();
        let without;
        let c = match (&other, c) {
            (Some(_), Value::Object(o)) => {
                let mut o = o.clone();
                o.remove("id");
                without = Value::Object(o);
                &without
            }
            _ => c,
        };
        let parsed = T::deserialize(c).map(|mut t| {
            *t.other_id() = other;
            t
        });
        let parsed = parsed.map_err(|e| {
            let start = c.get("start").and_then(Value::as_f64);
            let at = start.map_or_else(|| format!("clip {}", i + 1), |s| at(i, s));
            // Name the note when a note is what's wrong.
            let note = c
                .get("notes")
                .and_then(Value::as_array)
                .and_then(|ns| ns.iter().enumerate().find_map(|(j, n)| Note::deserialize(n).err().map(|e| format!("note {}: {e}", j + 1))));
            let shape = shape_hint(c, kind);
            LaneError::Invalid(format!("{at}: {}{shape}", note.unwrap_or_else(|| e.to_string())))
        })?;
        out.push(parsed);
    }
    Ok(out)
}

/// When a clip has another kind's shape, say so.
fn shape_hint(c: &Value, kind: &LaneKind) -> &'static str {
    let has = |k: &str| c.get(k).is_some();
    let other = if has("notes") {
        "notes"
    } else if has("points") {
        "automation points"
    } else if has("asset") {
        "audio"
    } else {
        return "";
    };
    match (kind, other) {
        (LaneKind::Notes, "notes") | (LaneKind::Control { .. }, "automation points") | (LaneKind::Audio, "audio") => "",
        (LaneKind::Choice { .. } | LaneKind::Switch, "automation points") => "",
        (LaneKind::Choice { .. } | LaneKind::Switch, "notes") => " (it holds notes, but this is a lane of names)",
        (LaneKind::Choice { .. } | LaneKind::Switch, _) => " (it holds audio, but this is a lane of names)",
        (LaneKind::Notes, "automation points") => " (it holds automation points, but this is a notes lane)",
        (LaneKind::Notes, _) => " (it holds audio, but this is a notes lane)",
        (LaneKind::Control { .. }, "notes") => " (it holds notes, but this is an automation lane)",
        (LaneKind::Control { .. }, _) => " (it holds audio, but this is an automation lane)",
        (LaneKind::Audio, "notes") => " (it holds notes, but this is an audio lane)",
        (LaneKind::Audio, _) => " (it holds automation points, but this is an audio lane)",
    }
}

/// Notes by start, then pitch, then length and velocity: a total order, so no two notes'
/// order is left to the file.
fn note_order(a: &Note, b: &Note) -> Ordering {
    a.start
        .total_cmp(&b.start)
        .then(a.pitch.total_cmp(&b.pitch))
        .then(a.length.total_cmp(&b.length))
        .then(a.velocity.total_cmp(&b.velocity))
}

/// Clips by `(start, id)`, anonymous first; clips equal in both by their canonical text, so
/// the order is total. Stable, so it is the one order a sort by all three keys gives, with the
/// text made only for clips that tie on the first two (anonymous clips that start together).
fn sorted<T: Clip>(mut clips: Vec<T>, key: impl Fn(&T) -> (f64, Option<&str>)) -> Vec<T> {
    let order = |a: &T, b: &T| {
        let ((sa, ia), (sb, ib)) = (key(a), key(b));
        sa.total_cmp(&sb).then(ia.cmp(&ib))
    };
    clips.sort_by(order);
    let mut i = 0;
    while i < clips.len() {
        let ties = clips[i + 1..].iter().take_while(|c| order(&clips[i], c).is_eq()).count();
        if ties > 0 {
            clips[i..=i + ties].sort_by_cached_key(|c| c.line());
        }
        i += ties + 1;
    }
    clips
}

/// A clip's line in the canonical form, and its id written as anything but a string.
trait Clip {
    /// The line, written onto `out`.
    fn write_line(&self, out: &mut String);
    fn other_id(&mut self) -> &mut Option<Value>;

    fn line(&self) -> String {
        let mut out = String::new();
        self.write_line(&mut out);
        out
    }
}

/// The id as written: a string, or another JSON value kept as it was (FMT-12).
fn id_field(out: &mut String, id: &Option<String>, other: &Option<Value>) {
    match (id, other) {
        (Some(id), _) => {
            out.push_str("\"id\": ");
            write_string(out, id);
            out.push_str(", ");
        }
        (None, Some(v)) => {
            let _ = write!(out, "\"id\": {v}, ");
        }
        (None, None) => {}
    }
}

/// `"start": 0, "length": 4`, as every clip opens after its id.
fn span(out: &mut String, start: f64, length: f64) {
    out.push_str("\"start\": ");
    write_number(out, start);
    out.push_str(", \"length\": ");
    write_number(out, length);
}

/// `[0, 500], [8, 2500]`.
fn pairs(out: &mut String, pairs: &[(f64, f64)]) {
    for (i, (a, b)) in pairs.iter().enumerate() {
        out.push_str(if i == 0 { "[" } else { ", [" });
        write_number(out, *a);
        out.push_str(", ");
        write_number(out, *b);
        out.push(']');
    }
}

impl Clip for NoteClip {
    /// The clip's opening line and one line per note.
    fn write_line(&self, out: &mut String) {
        out.push('{');
        id_field(out, &self.id, &self.other_id);
        span(out, self.start, self.length);
        out.push_str(", \"notes\": [");
        if self.notes.is_empty() {
            out.push_str("]}");
            return;
        }
        for (i, n) in self.notes.iter().enumerate() {
            out.push_str(if i == 0 { "\n    " } else { ",\n    " });
            note(out, n);
        }
        out.push_str("\n  ]}");
    }

    fn other_id(&mut self) -> &mut Option<Value> {
        &mut self.other_id
    }
}

fn note(out: &mut String, n: &Note) {
    out.push_str("{\"pitch\": ");
    match name_of(n.pitch) {
        // A name is letters, `#` and digits: nothing to escape.
        Some(name) => {
            out.push('"');
            out.push_str(name);
            out.push('"');
        }
        None => write_number(out, n.pitch),
    }
    out.push_str(", \"start\": ");
    write_number(out, n.start);
    out.push_str(", \"length\": ");
    write_number(out, n.length);
    out.push_str(", \"velocity\": ");
    write_number(out, n.velocity);
    out.push('}');
}

impl Clip for ControlClip {
    fn write_line(&self, out: &mut String) {
        out.push('{');
        id_field(out, &self.id, &self.other_id);
        span(out, self.start, self.length);
        out.push_str(", \"points\": [");
        pairs(out, &self.points);
        out.push_str("]}");
    }

    fn other_id(&mut self) -> &mut Option<Value> {
        &mut self.other_id
    }
}

impl Clip for StepClip {
    fn write_line(&self, out: &mut String) {
        out.push('{');
        id_field(out, &self.id, &self.other_id);
        span(out, self.start, self.length);
        out.push_str(", \"points\": [");
        for (i, (b, p)) in self.points.iter().enumerate() {
            out.push_str(if i == 0 { "[" } else { ", [" });
            write_number(out, *b);
            out.push_str(", ");
            match p {
                Step::Name(n) => write_string(out, n),
                Step::Switch(on) => out.push_str(if *on { "true" } else { "false" }),
            }
            out.push(']');
        }
        out.push_str("]}");
    }

    fn other_id(&mut self) -> &mut Option<Value> {
        &mut self.other_id
    }
}

impl Clip for AudioClip {
    fn write_line(&self, out: &mut String) {
        out.push('{');
        id_field(out, &self.id, &self.other_id);
        span(out, self.start, self.length);
        out.push_str(", \"asset\": ");
        write_string(out, &self.asset);
        out.push_str(", ");
        if let Some(h) = &self.hash {
            out.push_str("\"hash\": ");
            write_string(out, h);
            out.push_str(", ");
        }
        for (i, (key, x)) in [("offset", self.offset), ("gain_db", self.gain_db), ("fade_in", self.fade_in), ("fade_out", self.fade_out)]
            .into_iter()
            .enumerate()
        {
            let _ = write!(out, "{}\"{key}\": ", if i == 0 { "" } else { ", " });
            write_number(out, x);
        }
        let _ = write!(out, ", \"reverse\": {}, \"pitch\": ", self.reverse);
        write_number(out, self.pitch);
        out.push_str(", \"warp\": [");
        pairs(out, &self.warp);
        out.push_str("]}");
    }

    fn other_id(&mut self) -> &mut Option<Value> {
        &mut self.other_id
    }
}

/// The canonical form of a lane file (FMT-8a): a fixed key order per schema, numbers in their
/// shortest round-trip form and never `-0`, one clip per line and one note per line, clips by
/// `(start, id)` and notes by `(start, pitch)`, and a final newline. A control lane records its
/// unit. Reading a canonical file and writing it again gives the same bytes. Written into one
/// buffer, with nothing made per note.
pub fn canonical(data: &LaneData) -> String {
    let mut data = data.clone();
    data.sort();
    let mut out = String::with_capacity(256);
    out.push('{');
    if let Some(u) = data.unit() {
        out.push_str("\"unit\": ");
        write_string(&mut out, u);
        out.push_str(", ");
    }
    out.push_str("\"clips\": [");
    fn lines<T: Clip>(out: &mut String, clips: &[T]) {
        for (i, c) in clips.iter().enumerate() {
            out.push_str(if i == 0 { "\n  " } else { ",\n  " });
            c.write_line(out);
        }
        if !clips.is_empty() {
            out.push('\n');
        }
    }
    match &data {
        LaneData::Notes(c) => lines(&mut out, c),
        LaneData::Control { clips, .. } => lines(&mut out, clips),
        LaneData::Steps { clips, .. } => lines(&mut out, clips),
        LaneData::Audio(c) => lines(&mut out, c),
    }
    out.push_str("]}\n");
    out
}

/// A number as data files write it: Rust's shortest round-trip form, never `-0`.
pub fn number(x: f64) -> String {
    if x == 0.0 { "0".into() } else { x.to_string() }
}

/// [`number`], written onto `out`.
fn write_number(out: &mut String, x: f64) {
    if x == 0.0 {
        out.push('0');
    } else {
        let _ = write!(out, "{x}");
    }
}

/// A JSON string, written onto `out` as serde_json writes it.
fn write_string(out: &mut String, s: &str) {
    out.push_str(&string(s));
}

fn string(s: &str) -> String {
    serde_json::to_string(s).expect("a string serialises")
}

/// What a lane file holds, judged by the shape of its clips: which kind of lane can read it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Holds {
    Notes,
    /// Points: automation.
    Control,
    /// Points holding names: a choice's lane (CARD-15).
    Choice,
    /// Points holding true or false: a switch's lane.
    Switch,
    Audio,
    /// No clips, which any lane reads.
    Empty,
    /// Clips no lane can read.
    Unknown,
}

impl Holds {
    pub fn of(text: &str) -> Holds {
        let kinds = [(LaneKind::Notes, Holds::Notes), (LaneKind::Control { unit: None }, Holds::Control), (LaneKind::Audio, Holds::Audio)];
        // A lane of names is known by its unit, which its points need.
        let unit = serde_json::from_str::<Map<String, Value>>(text).ok().and_then(|m| m.get("unit").and_then(Value::as_str).map(str::to_string));
        for (u, kind, holds) in [("choice", LaneKind::Choice { options: vec![] }, Holds::Choice), ("switch", LaneKind::Switch, Holds::Switch)] {
            if unit.as_deref() == Some(u) && LaneData::read(text, &kind).is_ok() {
                return holds;
            }
        }
        // A file whose clips are all gone fits any lane.
        if serde_json::from_str::<Map<String, Value>>(text).ok().and_then(|m| m.get("clips").and_then(|c| c.as_array().map(Vec::is_empty)))
            == Some(true)
        {
            return Holds::Empty;
        }
        // The shape alone: the unit is checked against a lane, not here.
        let without_unit = match serde_json::from_str::<Map<String, Value>>(text) {
            Ok(mut m) => {
                m.remove("unit");
                Value::Object(m).to_string()
            }
            Err(_) => return Holds::Unknown,
        };
        kinds.into_iter().find(|(k, _)| LaneData::read(&without_unit, k).is_ok()).map_or(Holds::Unknown, |(_, h)| h)
    }

    /// Whether a lane of `kind` can read a file that holds this.
    pub fn fits(self, kind: &LaneKind) -> bool {
        match self {
            Holds::Empty => true,
            Holds::Notes => *kind == LaneKind::Notes,
            Holds::Control => matches!(kind, LaneKind::Control { .. }),
            Holds::Choice => matches!(kind, LaneKind::Choice { .. }),
            Holds::Switch => *kind == LaneKind::Switch,
            Holds::Audio => *kind == LaneKind::Audio,
            Holds::Unknown => false,
        }
    }
}

/// A note in song time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AbsNote {
    pub start: f64,
    pub end: f64,
    pub pitch: f64,
    pub velocity: f64,
}

/// A notes lane's notes in song time, cut to their clips, in a total order: by start, pitch,
/// end and velocity.
pub fn notes(clips: &[NoteClip]) -> Vec<AbsNote> {
    notes_keyed(clips).into_iter().map(|(n, _)| n).collect()
}

/// [`notes`], each with the index in `clips` of the clip it comes from: the live engine names a
/// sounding note by its clip (RENDER-26a). One function, so the two orders can't drift apart.
pub fn notes_keyed(clips: &[NoteClip]) -> Vec<(AbsNote, usize)> {
    let mut out = vec![];
    for (i, c) in clips.iter().enumerate() {
        for n in &c.notes {
            let (s, e) = (n.start.max(0.0), (n.start + n.length).min(c.length));
            if e > s {
                out.push((AbsNote { start: c.start + s, end: c.start + e, pitch: n.pitch, velocity: n.velocity.clamp(0.0, 1.0) }, i));
            }
        }
    }
    // Stable: equal notes keep their clips' order, as before.
    out.sort_by(|(a, _), (b, _)| {
        a.start.total_cmp(&b.start).then(a.pitch.total_cmp(&b.pitch)).then(a.end.total_cmp(&b.end)).then(a.velocity.total_cmp(&b.velocity))
    });
    out
}

/// A control lane's clips, in canonical order. Where clips overlap, the one that starts later
/// wins; of two that start together, the one with the greater id (FMT-8a).
/// A lane of names plays as one of these too, its points made places ([`ControlLane::steps`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ControlLane {
    clips: Vec<ControlClip>,
    /// Each point holds until the next, with no line between (CARD-15).
    step: bool,
}

impl ControlLane {
    /// From clips in canonical order ([`LaneData::read`]).
    pub fn new(clips: Vec<ControlClip>) -> ControlLane {
        ControlLane { clips, step: false }
    }

    /// A lane of names, as the places it plays (CARD-15): an option's place among `options`, or a
    /// switch's 1 or 0 (`options` is `None`). A name the choice doesn't have plays the lane's
    /// fallback from its point to the next. Made off the audio thread, when the lane is prepared.
    pub fn steps(clips: &[StepClip], options: Option<&[String]>) -> ControlLane {
        let place = |p: &Step| match (p, options) {
            (Step::Switch(on), None) => f64::from(u8::from(*on)),
            (Step::Name(n), Some(o)) => o.iter().position(|x| x == n).map_or(f64::NAN, |i| i as f64),
            _ => f64::NAN,
        };
        let clips = clips
            .iter()
            .map(|c| ControlClip {
                id: c.id.clone(),
                other_id: None,
                start: c.start,
                length: c.length,
                points: c.points.iter().map(|(b, p)| (*b, place(p))).collect(),
            })
            .collect();
        ControlLane { clips, step: true }
    }

    pub fn value_at(&self, beat: f64, default: f64) -> f64 {
        // `max_by` keeps the last of equals: in canonical order, the greater id.
        let clip = self.clips.iter().filter(|c| beat >= c.start && beat < c.start + c.length).max_by(|a, b| a.start.total_cmp(&b.start));
        let Some(c) = clip else { return default };
        let t = beat - c.start;
        let p = &c.points;
        if self.step {
            // The last point at or before `t`, else the first; a place no option has (NaN) is the fallback.
            let held = p.iter().rev().find(|(b, _)| *b <= t).or(p.first()).map_or(default, |x| x.1);
            return if held.is_nan() { default } else { held };
        }
        match p.iter().position(|(b, _)| *b > t) {
            None => p.last().map_or(default, |x| x.1),
            Some(0) => p[0].1,
            Some(i) => {
                let ((b0, v0), (b1, v1)) = (p[i - 1], p[i]);
                if b1 > b0 { v0 + (v1 - v0) * (t - b0) / (b1 - b0) } else { v1 }
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.clips.is_empty()
    }

    pub fn clips(&self) -> &[ControlClip] {
        &self.clips
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notes_kind() -> LaneKind {
        LaneKind::Notes
    }

    fn hz() -> LaneKind {
        LaneKind::Control { unit: Some("hz".into()) }
    }

    fn invalid(text: &str, kind: &LaneKind) -> String {
        match LaneData::read(text, kind) {
            Err(LaneError::Invalid(m)) => m,
            other => panic!("expected lane.invalid for {text}, got {other:?}"),
        }
    }

    #[test]
    fn pitches() {
        assert_eq!(parse_pitch("C4"), Some(60.0));
        assert_eq!(parse_pitch("A1"), Some(33.0));
        assert_eq!(parse_pitch("Bb3"), Some(58.0));
        assert_eq!(parse_pitch("C#-1"), Some(1.0));
        assert_eq!(parse_pitch("H2"), None);
        assert_eq!(pitch_name(33.0).as_deref(), Some("A1"));
        assert_eq!(pitch_name(61.0).as_deref(), Some("C#4"));
        assert_eq!(pitch_name(60.5), None);
        assert_eq!(pitch_name(128.0), None);
    }

    // FMT-8: lane files, clips and notes reject unknown fields, naming them.
    #[test]
    fn strict_fields() {
        let m = invalid(r#"{"clips": [], "kind": "notes"}"#, &notes_kind());
        assert!(m.contains("unknown field `kind`"), "{m}");
        let m = invalid(r#"{"clips": [{"start": 0, "length": 4, "colour": 1, "notes": []}]}"#, &notes_kind());
        assert!(m.contains("clip 1 (at beat 0)") && m.contains("unknown field `colour`"), "{m}");
        let m = invalid(
            r#"{"clips": [{"start": 0, "length": 4, "notes": [{"pitch": "C4", "start": 0, "length": 1}, {"pitch": "C4", "start": 1, "length": 1, "vel": 1}]}]}"#,
            &notes_kind(),
        );
        assert!(m.contains("clip 1 (at beat 0): note 2") && m.contains("unknown field `vel`"), "{m}");
        let m = invalid(r#"{"unit": "hz", "clips": [{"start": 8, "length": 4, "points": [], "curve": "exp"}]}"#, &hz());
        assert!(m.contains("clip 1 (at beat 8)") && m.contains("`curve`"), "{m}");
        let m = invalid(r#"{"clips": [{"start": 0, "length": 1, "asset": "a.wav", "loop": true}]}"#, &LaneKind::Audio);
        assert!(m.contains("`loop`"), "{m}");
        let m = invalid(r#"{"clips": [{"start": 0, "length": 4, "notes": [{"pitch": "H9", "start": 0, "length": 1}]}]}"#, &notes_kind());
        assert!(m.contains("bad pitch `H9`"), "{m}");
        assert!(invalid("{\"clips\": [", &notes_kind()).contains("line 1"));
        assert!(
            invalid(r#"{"clip": []}"#, &notes_kind()).contains("`clip`") || invalid(r#"{"clip": []}"#, &notes_kind()).contains("clips")
        );
    }

    // FMT-10: no kind in the file; the wrong shape fails, and no clips fits any kind.
    #[test]
    fn kinds_by_shape() {
        let points = r#"{"clips": [{"start": 0, "length": 4, "points": [[0, 1]]}]}"#;
        assert!(invalid(points, &notes_kind()).contains("it holds automation points, but this is a notes lane"));
        let notes = r#"{"clips": [{"start": 0, "length": 4, "notes": []}]}"#;
        assert!(invalid(notes, &LaneKind::Audio).contains("it holds notes, but this is an audio lane"));
        for kind in [notes_kind(), hz(), LaneKind::Audio] {
            assert!(LaneData::read(r#"{"clips": []}"#, &kind).unwrap().is_empty());
            assert!(LaneData::read(r#"{"unit": "hz", "clips": []}"#, &kind).is_ok());
        }
        assert!(
            invalid(r#"{"unit": "hz", "clips": [{"start": 0, "length": 4, "notes": []}]}"#, &notes_kind()).contains("unknown field `unit`")
        );
    }

    // WRITE-3: the `Clips` op puts whole clips by id and deletes by id; a clip with no usable id
    // is kept as written, and a put clip that isn't the lane's kind is refused, naming it.
    #[test]
    fn clips_are_put_and_deleted_by_id() {
        let text = r#"{"clips": [{"id": "a", "start": 0, "length": 4, "notes": []}, {"id": "b", "start": 4, "length": 4, "notes": []},
                                 {"start": 8, "length": 4, "notes": []}]}"#;
        let mut data = LaneData::read(text, &notes_kind()).unwrap();
        assert!(!data.addressable(), "a clip has no id");
        let lines = data.clip_lines();
        assert_eq!(lines.iter().map(|l| l.0).collect::<Vec<_>>(), [Some("a"), Some("b"), None]);
        assert_eq!(lines[0].1, r#"{"id": "a", "start": 0, "length": 4, "notes": []}"#);
        let put = serde_json::json!({"id": "a", "start": 2, "length": 1, "notes": [{"pitch": "C4", "start": 0, "length": 1}]});
        data.put_clips(&[put, serde_json::json!({"id": "c", "start": 16, "length": 1, "notes": []})], &["b".into(), "zz".into()]).unwrap();
        assert_eq!(data.ids(), [Some("a"), None, Some("c")], "in order of start");
        let line = "{\"id\": \"a\", \"start\": 2, \"length\": 1, \"notes\": [\n    \
                    {\"pitch\": \"C4\", \"start\": 0, \"length\": 1, \"velocity\": 0.8}\n  ]}";
        assert_eq!(data.clip_lines()[0].1, line);
        let wrong = data.put_clips(&[serde_json::json!({"id": "d", "start": 0, "length": 1, "points": []})], &[]).unwrap_err();
        assert!(wrong.contains("clip 1") && wrong.contains("automation points"), "{wrong}");

        let ok = LaneData::read(r#"{"clips": [{"id": "a", "start": 0, "length": 4, "notes": []}]}"#, &notes_kind()).unwrap();
        assert!(ok.addressable());
        for not in [r#"{"id": "A", "start": 0, "length": 4, "notes": []}"#, r#"{"id": 5, "start": 0, "length": 4, "notes": []}"#] {
            let text = format!(r#"{{"clips": [{not}]}}"#);
            assert!(!LaneData::read(&text, &notes_kind()).unwrap().addressable(), "{not}");
        }
        let twice = r#"{"clips": [{"id": "a", "start": 0, "length": 4, "notes": []}, {"id": "a", "start": 8, "length": 4, "notes": []}]}"#;
        assert!(!LaneData::read(twice, &notes_kind()).unwrap().addressable());
        // A control lane's puts are read in its unit.
        let mut control = LaneData::read(r#"{"unit": "hz", "clips": []}"#, &hz()).unwrap();
        control.put_clips(&[serde_json::json!({"id": "p", "start": 0, "length": 4, "points": [[0, 1]]})], &[]).unwrap();
        let lane = "{\"unit\": \"hz\", \"clips\": [\n  {\"id\": \"p\", \"start\": 0, \"length\": 4, \"points\": [[0, 1]]}\n]}\n";
        assert_eq!(canonical(&control), lane);
    }

    // FMT-10: a control lane records its unit, and another unit is lane.unit-mismatch.
    #[test]
    fn units() {
        let lane = |unit: &str| format!(r#"{{{unit}"clips": [{{"start": 0, "length": 4, "points": [[0, 500]]}}]}}"#);
        assert!(matches!(LaneData::read(&lane("\"unit\": \"hz\", "), &hz()), Ok(LaneData::Control { unit: Some(u), .. }) if u == "hz"));
        assert_eq!(
            LaneData::read(&lane("\"unit\": \"db\", "), &hz()),
            Err(LaneError::Unit { found: Some("db".into()), declared: "hz".into() })
        );
        assert_eq!(LaneData::read(&lane(""), &hz()), Err(LaneError::Unit { found: None, declared: "hz".into() }));
        assert!(LaneData::read(&lane(""), &LaneKind::Control { unit: None }).is_ok(), "an untyped lane takes any unit");
        let empty = LaneData::read(r#"{"clips": []}"#, &hz()).unwrap();
        assert_eq!(empty, LaneData::Control { unit: Some("hz".into()), clips: vec![] });
        assert!(empty.fits(&hz()) && !empty.fits(&LaneKind::Control { unit: Some("db".into()) }) && !empty.fits(&notes_kind()));
    }

    #[test]
    fn audio_clips_parse_with_defaults_and_validate() {
        let data = LaneData::read(
            r#"{"clips": [{"start": 4, "length": 2, "asset": "vox.wav", "warp": [[1.0, 1], [0.0, 0]]},
                          {"start": 0, "length": 1, "asset": "vox.wav", "reverse": true, "pitch": -12}]}"#,
            &LaneKind::Audio,
        )
        .unwrap();
        let LaneData::Audio(c) = data else { panic!() };
        assert_eq!(c[0].start, 0.0);
        assert!(c[0].reverse && c[0].pitch == -12.0 && c[0].gain_db == 0.0 && c[0].hash.is_none());
        assert_eq!(c[1].warp, vec![(0.0, 0.0), (1.0, 1.0)]);
        assert!(invalid(r#"{"clips": [{"start": 0, "length": 0, "asset": "a.wav"}]}"#, &LaneKind::Audio).contains("clip 1 (at beat 0)"));
        assert!(
            invalid(r#"{"clips": [{"start": 0, "length": 1, "asset": "a.wav", "warp": [[0, 0], [1, 0]]}]}"#, &LaneKind::Audio)
                .contains("warp")
        );
    }

    // FMT-8a: readers order clips by (start, id) and notes by (start, pitch), so the order of an
    // array never changes what plays.
    #[test]
    fn order_never_comes_from_the_file() {
        let a = r#"{"clips": [{"id": "b", "start": 0, "length": 4, "notes": [{"pitch": 62, "start": 0, "length": 1}, {"pitch": 60, "start": 0, "length": 1}]},
                              {"id": "a", "start": 0, "length": 4, "notes": []}, {"start": 0, "length": 2, "notes": []}]}"#;
        let b = r#"{"clips": [{"start": 0, "length": 2, "notes": []}, {"id": "a", "start": 0, "length": 4, "notes": []},
                              {"id": "b", "start": 0, "length": 4, "notes": [{"pitch": 60, "start": 0, "length": 1}, {"pitch": 62, "start": 0, "length": 1}]}]}"#;
        let (a, b) = (LaneData::read(a, &notes_kind()).unwrap(), LaneData::read(b, &notes_kind()).unwrap());
        assert_eq!(a, b);
        assert_eq!(a.ids(), [None, Some("a"), Some("b")]);
        // Overlapping control clips that start together: the greater id wins, whatever the file order.
        let lane = |first: &str, second: &str| {
            let text = format!(r#"{{"unit": "hz", "clips": [{first}, {second}]}}"#);
            let LaneData::Control { clips, .. } = LaneData::read(&text, &hz()).unwrap() else { panic!() };
            ControlLane::new(clips).value_at(1.0, 0.0)
        };
        let (x, y) =
            (r#"{"id": "x", "start": 0, "length": 4, "points": [[0, 1]]}"#, r#"{"id": "y", "start": 0, "length": 4, "points": [[0, 2]]}"#);
        assert_eq!((lane(x, y), lane(y, x)), (2.0, 2.0));
    }

    #[test]
    fn control_lane_interpolates_and_later_clip_wins() {
        let lane = ControlLane::new(vec![
            ControlClip { id: None, other_id: None, start: 0.0, length: 8.0, points: vec![(0.0, 0.0), (4.0, 1.0)] },
            ControlClip { id: None, other_id: None, start: 6.0, length: 2.0, points: vec![(0.0, 5.0)] },
        ]);
        assert_eq!(lane.value_at(2.0, 9.0), 0.5);
        assert_eq!(lane.value_at(5.0, 9.0), 1.0);
        assert_eq!(lane.value_at(7.0, 9.0), 5.0);
        assert_eq!(lane.value_at(8.0, 9.0), 9.0);
    }

    // FMT-8a: the canonical form, and reading then writing it gives the same bytes.
    #[test]
    fn canonical_form() {
        let text = r#"{"clips": [{"start": 16.0, "length": 16, "notes": [{"pitch": 45, "start": 0.5, "length": 0.25}, {"pitch": "A1", "start": -0.0, "length": 0.5, "velocity": 0.9}]},
                                  {"id": "k3f9q2zt", "start": 0, "length": 16, "notes": []}]}"#;
        let data = LaneData::read(text, &notes_kind()).unwrap();
        let out = canonical(&data);
        assert_eq!(
            out,
            "{\"clips\": [\n  {\"id\": \"k3f9q2zt\", \"start\": 0, \"length\": 16, \"notes\": []},\n  {\"start\": 16, \"length\": 16, \"notes\": [\n    \
             {\"pitch\": \"A1\", \"start\": 0, \"length\": 0.5, \"velocity\": 0.9},\n    {\"pitch\": \"A2\", \"start\": 0.5, \"length\": 0.25, \
             \"velocity\": 0.8}\n  ]}\n]}\n"
        );
        assert_eq!(canonical(&LaneData::read(&out, &notes_kind()).unwrap()), out);

        let control = r#"{"clips": [{"id": "q7m2c8xa", "points": [[8, 2500], [0, 500]], "length": 8, "start": 32}], "unit": "hz"}"#;
        let out = canonical(&LaneData::read(control, &hz()).unwrap());
        assert_eq!(
            out,
            "{\"unit\": \"hz\", \"clips\": [\n  {\"id\": \"q7m2c8xa\", \"start\": 32, \"length\": 8, \"points\": [[0, 500], [8, 2500]]}\n]}\n"
        );
        assert_eq!(canonical(&LaneData::read(&out, &hz()).unwrap()), out);

        let audio = r#"{"clips": [{"id": "z1b4r9td", "start": 64, "length": 4, "asset": "vox/lead \"take\".wav", "hash": "9f2c3a1be07d44c1", "gain_db": -3, "fade_in": 0.01, "fade_out": 0.05}]}"#;
        let out = canonical(&LaneData::read(audio, &LaneKind::Audio).unwrap());
        assert_eq!(
            out,
            "{\"clips\": [\n  {\"id\": \"z1b4r9td\", \"start\": 64, \"length\": 4, \"asset\": \"vox/lead \\\"take\\\".wav\", \"hash\": \"9f2c3a1be07d44c1\", \
             \"offset\": 0, \"gain_db\": -3, \"fade_in\": 0.01, \"fade_out\": 0.05, \"reverse\": false, \"pitch\": 0, \"warp\": []}\n]}\n"
        );
        assert_eq!(canonical(&LaneData::read(&out, &LaneKind::Audio).unwrap()), out);

        assert_eq!(canonical(&LaneData::empty(&notes_kind())), "{\"clips\": []}\n");
        assert_eq!(
            canonical(&LaneData::empty(&LaneKind::Control { unit: Some("bipolar".into()) })),
            "{\"unit\": \"bipolar\", \"clips\": []}\n"
        );
        assert_eq!(number(-0.0), "0");
        assert_eq!(number(0.1 + 0.2), "0.30000000000000004");
        assert_eq!(number(1e21), "1000000000000000000000");
    }

    // FMT-8a: a number written with all its digits reads back as the same number, so writing a
    // note at any beat (an agent's, or one half a beat ahead of the playhead) reads back as written.
    #[test]
    fn numbers_with_all_their_digits_read_back_as_written() {
        let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
        for i in 0..20_000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            // A beat in 0..512, and the one the default float parser misread.
            let start = if i == 0 { 10.292177777777795 } else { (x >> 11) as f64 / (1u64 << 53) as f64 * 512.0 };
            let text = format!(
                r#"{{"clips": [{{"start": 0, "length": 512, "notes": [{{"pitch": 60, "start": {}, "length": 0.5}}]}}]}}"#,
                number(start)
            );
            let LaneData::Notes(clips) = LaneData::read(&text, &notes_kind()).unwrap() else { unreachable!() };
            assert_eq!(clips[0].notes[0].start.to_bits(), start.to_bits(), "{start} read back as {}", clips[0].notes[0].start);
            let out = canonical(&LaneData::Notes(clips));
            assert_eq!(canonical(&LaneData::read(&out, &notes_kind()).unwrap()), out);
        }
    }

    fn shapes() -> LaneKind {
        LaneKind::Choice { options: ["saw", "square", "sine"].map(String::from).to_vec() }
    }

    // CARD-15: a lane of names records `choice` (or `switch`) and holds names (or true and false),
    // each kept as written, known or not, and written back canonically, byte for byte.
    #[test]
    fn lanes_of_names_read_and_write_canonically() {
        let text = "{\"unit\": \"choice\", \"clips\": [\n  {\"id\": \"a1\", \"start\": 4, \"length\": 8, \"points\": [[0, \"square\"], [2, \"tri\"], [4, \"saw\"]]}\n]}\n";
        let d = LaneData::read(text, &shapes()).unwrap();
        let LaneData::Steps { options, clips } = &d else { panic!("{d:?}") };
        assert_eq!(options.as_deref(), Some(&["saw".to_string(), "square".into(), "sine".into()][..]));
        assert_eq!(clips[0].points[1], (2.0, Step::Name("tri".into())), "a name the choice doesn't have is kept");
        assert_eq!(canonical(&d), text);
        assert!(d.fits(&shapes()) && !d.fits(&LaneKind::Switch) && !d.fits(&hz()));
        let switch = "{\"unit\": \"switch\", \"clips\": [\n  {\"start\": 0, \"length\": 4, \"points\": [[0, true], [1, false]]}\n]}\n";
        let d = LaneData::read(switch, &LaneKind::Switch).unwrap();
        assert_eq!(canonical(&d), switch);
        assert_eq!(canonical(&LaneData::empty(&shapes())), "{\"unit\": \"choice\", \"clips\": []}\n");
        // A point of another kind, or another unit, isn't this lane's.
        let m = invalid(r#"{"unit": "choice", "clips": [{"start": 0, "length": 4, "points": [[0, 0.5]]}]}"#, &shapes());
        assert!(m.contains("clip 1 (at beat 0)") && m.contains("an option's name"), "{m}");
        let m = invalid(r#"{"unit": "switch", "clips": [{"start": 0, "length": 4, "points": [[0, "on"]]}]}"#, &LaneKind::Switch);
        assert!(m.contains("true or false"), "{m}");
        let hz_file = r#"{"unit": "hz", "clips": [{"start": 0, "length": 4, "points": [[0, "saw"]]}]}"#;
        assert_eq!(LaneData::read(hz_file, &shapes()), Err(LaneError::Unit { found: Some("hz".into()), declared: "choice".into() }));
        // An automation lane doesn't read names.
        assert!(LaneData::read(text, &hz()).is_err());
        assert_eq!(Holds::of(text), Holds::Choice);
        assert_eq!(Holds::of(switch), Holds::Switch);
        assert!(Holds::Choice.fits(&shapes()) && !Holds::Choice.fits(&hz()));
    }

    // CARD-15: a lane of names steps: each point holds until the next, the first from its clip's
    // start; a name the choice doesn't have plays the fallback from there to the next point.
    #[test]
    fn a_lane_of_names_steps() {
        let text = r#"{"unit": "choice", "clips": [{"start": 4, "length": 8, "points": [[1, "square"], [2, "tri"], [4, "sine"]]}]}"#;
        let LaneData::Steps { clips, options } = LaneData::read(text, &shapes()).unwrap() else { panic!() };
        let lane = ControlLane::steps(&clips, options.as_deref());
        let at = |b: f64| lane.value_at(b, 9.0);
        assert_eq!([at(0.0), at(4.0), at(5.5), at(5.99), at(6.0), at(7.9), at(8.0), at(11.9), at(12.0)], [9.0, 1.0, 1.0, 1.0, 9.0, 9.0, 2.0, 2.0, 9.0]);
        let switch = r#"{"unit": "switch", "clips": [{"start": 0, "length": 4, "points": [[0, true], [1, false]]}]}"#;
        let LaneData::Steps { clips, options } = LaneData::read(switch, &LaneKind::Switch).unwrap() else { panic!() };
        let lane = ControlLane::steps(&clips, options.as_deref());
        assert_eq!([lane.value_at(0.5, 0.0), lane.value_at(1.5, 0.0)], [1.0, 0.0]);
    }

    #[test]
    fn what_a_file_holds() {
        assert_eq!(
            Holds::of(r#"{"clips": [{"start": 0, "length": 4, "notes": [{"pitch": "C1", "start": 0, "length": 1}]}]}"#),
            Holds::Notes
        );
        assert_eq!(Holds::of(r#"{"unit": "unit01", "clips": [{"start": 0, "length": 4, "points": [[0, 1]]}]}"#), Holds::Control);
        assert_eq!(Holds::of(r#"{"clips": [{"start": 0, "length": 4, "points": [[0, 1]]}]}"#), Holds::Control);
        assert_eq!(Holds::of(r#"{"clips": [{"start": 0, "length": 4, "asset": "vox.wav"}]}"#), Holds::Audio);
        assert_eq!(Holds::of(r#"{"unit": "hz", "clips": []}"#), Holds::Empty);
        assert_eq!(Holds::of(r#"{"clips": [{"start": 0}]}"#), Holds::Unknown);
        assert_eq!(Holds::of("{"), Holds::Unknown);
        assert!(Holds::Empty.fits(&notes_kind()) && Holds::Control.fits(&hz()) && !Holds::Notes.fits(&hz()));
    }

    #[test]
    fn notes_are_cut_to_their_clip() {
        let c = NoteClip {
            id: None,
            other_id: None,
            start: 4.0,
            length: 2.0,
            notes: vec![
                Note { pitch: 60.0, start: 1.5, length: 1.0, velocity: 2.0 },
                Note { pitch: 62.0, start: 3.0, length: 1.0, velocity: 0.5 },
            ],
        };
        assert_eq!(notes(&[c]), [AbsNote { start: 5.5, end: 6.0, pitch: 60.0, velocity: 1.0 }]);
    }
}
