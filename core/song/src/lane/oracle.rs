//! The lane reader and writer as they were before the G11 fix (A.3, A.9), kept word for word as
//! the oracle the fast ones are tested against: the same data, the same errors, the same bytes.

use serde::Deserialize;
use serde_json::Value;

use super::{AudioClip, ControlClip, LaneData, LaneError, LaneKind, Note, NoteClip, at, noun, note_order, number, shape_hint, string};

/// [`Note`] with the pitch read through the untagged enum it had.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OldNote {
    #[serde(deserialize_with = "pitch")]
    pitch: f64,
    start: f64,
    length: f64,
    #[serde(default = "super::default_velocity")]
    velocity: f64,
}

/// [`NoteClip`] made of [`OldNote`]s.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OldNoteClip {
    #[serde(default)]
    id: Option<String>,
    start: f64,
    length: f64,
    notes: Vec<OldNote>,
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
        P::S(s) => super::parse_pitch(&s).ok_or_else(|| serde::de::Error::custom(format!("bad pitch `{s}` (try 60 or \"C4\")"))),
    }
}

/// A clip type as the oracle reads and writes it.
trait Old: Sized {
    fn parse(v: Value) -> Result<Self, serde_json::Error>;
    fn line(&self) -> String;
    fn other_id(&mut self) -> &mut Option<Value>;
}

impl Old for NoteClip {
    fn parse(v: Value) -> Result<NoteClip, serde_json::Error> {
        let c = serde_json::from_value::<OldNoteClip>(v)?;
        let notes = c.notes.into_iter().map(|n| Note { pitch: n.pitch, start: n.start, length: n.length, velocity: n.velocity }).collect();
        Ok(NoteClip { id: c.id, other_id: None, start: c.start, length: c.length, notes })
    }

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

impl Old for ControlClip {
    fn parse(v: Value) -> Result<ControlClip, serde_json::Error> {
        serde_json::from_value(v)
    }

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

impl Old for AudioClip {
    fn parse(v: Value) -> Result<AudioClip, serde_json::Error> {
        serde_json::from_value(v)
    }

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

fn id_field(id: &Option<String>, other: &Option<Value>) -> String {
    match (id, other) {
        (Some(id), _) => format!("\"id\": {}, ", string(id)),
        (None, Some(v)) => format!("\"id\": {v}, "),
        (None, None) => String::new(),
    }
}

fn note(n: &Note) -> String {
    let pitch = super::pitch_name(n.pitch).map_or_else(|| number(n.pitch), |p| string(&p));
    format!(
        "{{\"pitch\": {pitch}, \"start\": {}, \"length\": {}, \"velocity\": {}}}",
        number(n.start),
        number(n.length),
        number(n.velocity)
    )
}

fn sorted<T: Old>(clips: Vec<T>, key: impl Fn(&T) -> (f64, Option<&str>)) -> Vec<T> {
    let mut keyed: Vec<(String, T)> = clips.into_iter().map(|c| (c.line(), c)).collect();
    keyed.sort_by(|(la, a), (lb, b)| {
        let ((sa, ia), (sb, ib)) = (key(a), key(b));
        sa.total_cmp(&sb).then(ia.cmp(&ib)).then(la.cmp(lb))
    });
    keyed.into_iter().map(|(_, c)| c).collect()
}

fn each<T: Old>(clips: &[Value], kind: &LaneKind) -> Result<Vec<T>, LaneError> {
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
        let parsed = T::parse(c.clone()).map(|mut t| {
            *t.other_id() = other;
            t
        });
        let parsed = parsed.map_err(|e| {
            let note = c.get("notes").and_then(Value::as_array).and_then(|ns| {
                ns.iter()
                    .enumerate()
                    .find_map(|(j, n)| serde_json::from_value::<OldNote>(n.clone()).err().map(|e| format!("note {}: {e}", j + 1)))
            });
            let shape = shape_hint(c, kind);
            LaneError::Invalid(format!("{at}: {}{shape}", note.unwrap_or_else(|| e.to_string())))
        })?;
        out.push(parsed);
    }
    Ok(out)
}

/// `LaneData::read` as it was.
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
                c.points.sort_by(|a, b| a.0.total_cmp(&b.0));
            }
            Ok(LaneData::Control { unit, clips: sorted(clips, |c| (c.start, c.id.as_deref())) })
        }
        LaneKind::Audio => {
            let mut clips = each::<AudioClip>(clips, kind)?;
            for (i, c) in clips.iter_mut().enumerate() {
                let at = at(i, c.start);
                if c.length <= 0.0 || c.offset < 0.0 || c.fade_in < 0.0 || c.fade_out < 0.0 {
                    return Err(invalid(format!("{at}: `length` must be above 0, and `offset`, `fade_in` and `fade_out` can't be negative")));
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

/// `canonical` as it was, with its own sort.
pub fn canonical(data: &LaneData) -> String {
    let data = sort(data.clone());
    let (unit, lines): (Option<&String>, Vec<String>) = match &data {
        LaneData::Notes(c) => (None, c.iter().map(Old::line).collect()),
        LaneData::Control { unit, clips } => (unit.as_ref(), clips.iter().map(Old::line).collect()),
        LaneData::Audio(c) => (None, c.iter().map(Old::line).collect()),
    };
    let unit = unit.map_or_else(String::new, |u| format!("\"unit\": {}, ", string(u)));
    if lines.is_empty() {
        return format!("{{{unit}\"clips\": []}}\n");
    }
    format!("{{{unit}\"clips\": [\n  {}\n]}}\n", lines.join(",\n  "))
}

/// `LaneData::sort` as it was.
pub fn sort(mut data: LaneData) -> LaneData {
    match &mut data {
        LaneData::Notes(c) => {
            for c in c.iter_mut() {
                c.notes.sort_by(note_order);
            }
            *c = sorted(std::mem::take(c), |c| (c.start, c.id.as_deref()));
        }
        LaneData::Control { clips, .. } => *clips = sorted(std::mem::take(clips), |c| (c.start, c.id.as_deref())),
        LaneData::Audio(c) => *c = sorted(std::mem::take(c), |c| (c.start, c.id.as_deref())),
    }
    data
}

/// Each clip's line as it was.
pub fn clip_lines(d: &LaneData) -> Vec<(Option<&str>, String)> {
    match d {
        LaneData::Notes(c) => c.iter().map(|c| (c.id.as_deref(), Old::line(c))).collect(),
        LaneData::Control { clips, .. } => clips.iter().map(|c| (c.id.as_deref(), Old::line(c))).collect(),
        LaneData::Audio(c) => c.iter().map(|c| (c.id.as_deref(), Old::line(c))).collect(),
    }
}
