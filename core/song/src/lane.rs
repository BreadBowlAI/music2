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
//! ```
//!
//! A file carries no kind: it is read as the kind its lane declares, and notes and audio clips
//! fail as the wrong shape. A file with no clips fits any kind. Times are in beats; a clip's
//! notes and points are relative to its start.
//!
//! Every reader puts clips and notes in one order before use, and [`canonical`] writes them in
//! it (FMT-8a), so the order of an array never changes the sound.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

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

fn pitch<'de, D: serde::Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum P {
        N(f64),
        S(String),
    }
    match P::deserialize(d)? {
        P::N(n) => Ok(n),
        P::S(s) => parse_pitch(&s).ok_or_else(|| serde::de::Error::custom(format!("bad pitch `{s}` (try 60 or \"C4\")"))),
    }
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
    (p.fract() == 0.0 && (0.0..=127.0).contains(&p)).then(|| format!("{}{}", NAMES[p as usize % 12], p as i32 / 12 - 1))
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
    Audio,
}

/// One lane's clips, read as its kind and in canonical order.
#[derive(Clone, Debug, PartialEq)]
pub enum LaneData {
    Notes(Vec<NoteClip>),
    Control { unit: Option<String>, clips: Vec<ControlClip> },
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
            LaneKind::Audio => LaneData::Audio(vec![]),
        }
    }

    /// Whether this data was read as `kind` (and, for a control lane, in its unit).
    pub fn fits(&self, kind: &LaneKind) -> bool {
        match (self, kind) {
            (LaneData::Notes(_), LaneKind::Notes) | (LaneData::Audio(_), LaneKind::Audio) => true,
            (LaneData::Control { unit, .. }, LaneKind::Control { unit: want }) => want.is_none() || unit == want,
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
            LaneData::Audio(c) => c.iter().map(|c| c.id.as_deref()).collect(),
        }
    }

    /// The ids written as anything but a string, in canonical clip order (FMT-12).
    pub fn other_ids(&self) -> Vec<Option<&Value>> {
        match self {
            LaneData::Notes(c) => c.iter().map(|c| c.other_id.as_ref()).collect(),
            LaneData::Control { clips, .. } => clips.iter().map(|c| c.other_id.as_ref()).collect(),
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
            LaneData::Audio(c) => c.iter().map(|c| c.start + c.length).collect(),
        };
        ends.into_iter().fold(0.0, f64::max)
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
            LaneData::Audio(c) => *c = sorted(std::mem::take(c), |c| (c.start, c.id.as_deref())),
        }
    }
}

fn noun(kind: &LaneKind) -> &'static str {
    match kind {
        LaneKind::Notes => "a notes lane",
        LaneKind::Control { .. } => "an automation lane",
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
        let start = c.get("start").and_then(Value::as_f64);
        let at = start.map_or_else(|| format!("clip {}", i + 1), |s| at(i, s));
        let mut c = c.clone();
        let other = match c.as_object_mut() {
            Some(o) if o.get("id").is_some_and(|id| !id.is_string() && !id.is_null()) => o.remove("id"),
            _ => None,
        };
        let c = &c;
        let parsed = serde_json::from_value::<T>(c.clone()).map(|mut t| {
            *t.other_id() = other;
            t
        });
        let parsed = parsed.map_err(|e| {
            // Name the note when a note is what's wrong.
            let note = c.get("notes").and_then(Value::as_array).and_then(|ns| {
                ns.iter()
                    .enumerate()
                    .find_map(|(j, n)| serde_json::from_value::<Note>(n.clone()).err().map(|e| format!("note {}: {e}", j + 1)))
            });
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
/// the order is total.
fn sorted<T: Clip>(clips: Vec<T>, key: impl Fn(&T) -> (f64, Option<&str>)) -> Vec<T> {
    let mut keyed: Vec<(String, T)> = clips.into_iter().map(|c| (c.line(), c)).collect();
    keyed.sort_by(|(la, a), (lb, b)| {
        let ((sa, ia), (sb, ib)) = (key(a), key(b));
        sa.total_cmp(&sb).then(ia.cmp(&ib)).then(la.cmp(lb))
    });
    keyed.into_iter().map(|(_, c)| c).collect()
}

/// A clip's line in the canonical form, and its id written as anything but a string.
trait Clip {
    fn line(&self) -> String;
    fn other_id(&mut self) -> &mut Option<Value>;
}

/// The id as written: a string, or another JSON value kept as it was (FMT-12).
fn id_field(id: &Option<String>, other: &Option<Value>) -> String {
    match (id, other) {
        (Some(id), _) => format!("\"id\": {}, ", string(id)),
        (None, Some(v)) => format!("\"id\": {v}, "),
        (None, None) => String::new(),
    }
}

impl Clip for NoteClip {
    /// The clip's opening line and one line per note.
    fn line(&self) -> String {
        let id = id_field(&self.id, &self.other_id);
        let head = format!("{{{id}\"start\": {}, \"length\": {}, \"notes\": [", number(self.start), number(self.length));
        if self.notes.is_empty() {
            return head + "]}";
        }
        let notes: Vec<String> = self.notes.iter().map(note).collect();
        format!("{head}\n    {}\n  ]}}", notes.join(",\n    "))
    }

    fn other_id(&mut self) -> &mut Option<Value> {
        &mut self.other_id
    }
}

fn note(n: &Note) -> String {
    let pitch = pitch_name(n.pitch).map_or_else(|| number(n.pitch), |p| string(&p));
    format!(
        "{{\"pitch\": {pitch}, \"start\": {}, \"length\": {}, \"velocity\": {}}}",
        number(n.start),
        number(n.length),
        number(n.velocity)
    )
}

impl Clip for ControlClip {
    fn line(&self) -> String {
        let points: Vec<String> = self.points.iter().map(|(b, v)| format!("[{}, {}]", number(*b), number(*v))).collect();
        format!(
            "{{{}\"start\": {}, \"length\": {}, \"points\": [{}]}}",
            id_field(&self.id, &self.other_id),
            number(self.start),
            number(self.length),
            points.join(", ")
        )
    }

    fn other_id(&mut self) -> &mut Option<Value> {
        &mut self.other_id
    }
}

impl Clip for AudioClip {
    fn line(&self) -> String {
        let hash = self.hash.as_ref().map_or_else(String::new, |h| format!("\"hash\": {}, ", string(h)));
        let warp: Vec<String> = self.warp.iter().map(|(s, b)| format!("[{}, {}]", number(*s), number(*b))).collect();
        format!(
            "{{{}\"start\": {}, \"length\": {}, \"asset\": {}, {hash}\"offset\": {}, \"gain_db\": {}, \"fade_in\": {}, \"fade_out\": {}, \
             \"reverse\": {}, \"pitch\": {}, \"warp\": [{}]}}",
            id_field(&self.id, &self.other_id),
            number(self.start),
            number(self.length),
            string(&self.asset),
            number(self.offset),
            number(self.gain_db),
            number(self.fade_in),
            number(self.fade_out),
            self.reverse,
            number(self.pitch),
            warp.join(", ")
        )
    }

    fn other_id(&mut self) -> &mut Option<Value> {
        &mut self.other_id
    }
}

/// The canonical form of a lane file (FMT-8a): a fixed key order per schema, numbers in their
/// shortest round-trip form and never `-0`, one clip per line and one note per line, clips by
/// `(start, id)` and notes by `(start, pitch)`, and a final newline. A control lane records its
/// unit. Reading a canonical file and writing it again gives the same bytes.
pub fn canonical(data: &LaneData) -> String {
    let mut data = data.clone();
    data.sort();
    let (unit, lines): (Option<&String>, Vec<String>) = match &data {
        LaneData::Notes(c) => (None, c.iter().map(Clip::line).collect()),
        LaneData::Control { unit, clips } => (unit.as_ref(), clips.iter().map(Clip::line).collect()),
        LaneData::Audio(c) => (None, c.iter().map(Clip::line).collect()),
    };
    let unit = unit.map_or_else(String::new, |u| format!("\"unit\": {}, ", string(u)));
    if lines.is_empty() {
        return format!("{{{unit}\"clips\": []}}\n");
    }
    format!("{{{unit}\"clips\": [\n  {}\n]}}\n", lines.join(",\n  "))
}

/// A number as data files write it: Rust's shortest round-trip form, never `-0`.
pub fn number(x: f64) -> String {
    if x == 0.0 { "0".into() } else { x.to_string() }
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
    Audio,
    /// No clips, which any lane reads.
    Empty,
    /// Clips no lane can read.
    Unknown,
}

impl Holds {
    pub fn of(text: &str) -> Holds {
        let kinds = [(LaneKind::Notes, Holds::Notes), (LaneKind::Control { unit: None }, Holds::Control), (LaneKind::Audio, Holds::Audio)];
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
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ControlLane {
    clips: Vec<ControlClip>,
}

impl ControlLane {
    /// From clips in canonical order ([`LaneData::read`]).
    pub fn new(clips: Vec<ControlClip>) -> ControlLane {
        ControlLane { clips }
    }

    pub fn value_at(&self, beat: f64, default: f64) -> f64 {
        // `max_by` keeps the last of equals: in canonical order, the greater id.
        let clip = self.clips.iter().filter(|c| beat >= c.start && beat < c.start + c.length).max_by(|a, b| a.start.total_cmp(&b.start));
        let Some(c) = clip else { return default };
        let t = beat - c.start;
        let p = &c.points;
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
