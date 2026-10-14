//! `layout.json` (FMT-19): the tracks' order and colours, and later markers. Shared presentation:
//! never part of a revision and never read by the build (PLANE-3), written by a Tx that is an undo
//! step of its own. The palette's names and taste.md §2.7's default order are UI vocabulary, so
//! they live here and not in core (P10).
//!
//! Reading keeps everything: a key that names no track (one commented out in `song.ts` for a
//! while) is ignored when read and written back in its place, with its colour, and unknown keys
//! are written back as read, so a newer version's layout survives an older writer. A file that
//! doesn't parse (a git merge left markers in it) is never overwritten by a track builder; only
//! Reset layout writes over it, as one undo step.

use std::collections::BTreeMap;
use std::path::Path;

use m2_session::Session;
use m2_song::tx::{Base, Expect, Op, Payload, Tx, TxPath};
use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};

use crate::EditError;
use crate::plan;

/// The file, as a Tx names it.
pub const FILE: &str = m2_song::path::LAYOUT;

/// taste.md §2.7 rule 2: the default order, each colour far in hue from the two before it.
pub const DEFAULT_ORDER: [&str; 12] =
    ["rose", "teal", "amber", "indigo", "green", "plum", "olive", "blue", "coral", "cyan", "violet", "lime"];

/// Each palette colour's OKLCH hue, for reading a `#rrggbb` as the nearest name.
const HUES: [(&str, f64); 12] = [
    ("rose", 10.0),
    ("coral", 38.0),
    ("amber", 72.0),
    ("olive", 100.0),
    ("lime", 128.0),
    ("green", 152.0),
    ("teal", 185.0),
    ("cyan", 215.0),
    ("blue", 248.0),
    ("indigo", 278.0),
    ("violet", 305.0),
    ("plum", 340.0),
];

/// A JSON value with its objects' keys in the order read, so a write keeps a file's own order.
#[derive(Clone, Debug, PartialEq)]
pub enum J {
    Null,
    Bool(bool),
    Num(serde_json::Number),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

impl<'de> Deserialize<'de> for J {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<J, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = J;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("JSON")
            }
            fn visit_unit<E>(self) -> Result<J, E> {
                Ok(J::Null)
            }
            fn visit_bool<E>(self, b: bool) -> Result<J, E> {
                Ok(J::Bool(b))
            }
            fn visit_i64<E>(self, n: i64) -> Result<J, E> {
                Ok(J::Num(n.into()))
            }
            fn visit_u64<E>(self, n: u64) -> Result<J, E> {
                Ok(J::Num(n.into()))
            }
            fn visit_f64<E: serde::de::Error>(self, n: f64) -> Result<J, E> {
                serde_json::Number::from_f64(n).map(J::Num).ok_or_else(|| E::custom("a number that isn't finite"))
            }
            fn visit_str<E>(self, s: &str) -> Result<J, E> {
                Ok(J::Str(s.to_string()))
            }
            fn visit_string<E>(self, s: String) -> Result<J, E> {
                Ok(J::Str(s))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<J, A::Error> {
                let mut out = vec![];
                while let Some(v) = a.next_element()? {
                    out.push(v);
                }
                Ok(J::Arr(out))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<J, A::Error> {
                let mut out: Vec<(String, J)> = vec![];
                while let Some((k, v)) = a.next_entry::<String, J>()? {
                    // A repeated key: the last one wins, as JSON readers take it, in its first place.
                    match out.iter_mut().find(|(x, _)| *x == k) {
                        Some(e) => e.1 = v,
                        None => out.push((k, v)),
                    }
                }
                Ok(J::Obj(out))
            }
        }
        d.deserialize_any(V)
    }
}

impl J {
    fn scalar(&self) -> bool {
        !matches!(self, J::Arr(_) | J::Obj(_))
    }

    /// Written in the canonical form: two spaces a level, a list of plain values or an object of
    /// them on one line (below the top), anything else a line per item.
    fn write(&self, out: &mut String, depth: usize) {
        let pad = |n: usize| "  ".repeat(n);
        match self {
            J::Null => out.push_str("null"),
            J::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            J::Num(n) => out.push_str(&n.to_string()),
            J::Str(s) => out.push_str(&serde_json::to_string(s).expect("a string serialises")),
            J::Arr(items) if items.is_empty() => out.push_str("[]"),
            J::Arr(items) if items.iter().all(J::scalar) => {
                out.push('[');
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    v.write(out, depth);
                }
                out.push(']');
            }
            J::Arr(items) => {
                out.push_str("[\n");
                for (i, v) in items.iter().enumerate() {
                    out.push_str(&pad(depth + 1));
                    v.write(out, depth + 1);
                    out.push_str(if i + 1 < items.len() { ",\n" } else { "\n" });
                }
                out.push_str(&pad(depth));
                out.push(']');
            }
            J::Obj(fields) if fields.is_empty() => out.push_str("{}"),
            J::Obj(fields) if depth > 0 && fields.iter().all(|(_, v)| v.scalar()) => {
                out.push_str("{ ");
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&serde_json::to_string(k).expect("a string serialises"));
                    out.push_str(": ");
                    v.write(out, depth);
                }
                out.push_str(" }");
            }
            J::Obj(fields) => {
                out.push_str("{\n");
                for (i, (k, v)) in fields.iter().enumerate() {
                    out.push_str(&pad(depth + 1));
                    out.push_str(&serde_json::to_string(k).expect("a string serialises"));
                    out.push_str(": ");
                    v.write(out, depth + 1);
                    out.push_str(if i + 1 < fields.len() { ",\n" } else { "\n" });
                }
                out.push_str(&pad(depth));
                out.push('}');
            }
        }
    }
}

/// `layout.json` as read: its known keys, and every other key as it was.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    /// Track keys, top to bottom; keys that name no track now included.
    pub order: Vec<String>,
    /// `tracks`: each key's fields as read, `colour` among them.
    pub tracks: Vec<(String, Vec<(String, J)>)>,
    /// `markers`, kept as read (8b.5 reads them).
    pub markers: Option<J>,
    /// Every other key, in the order read.
    pub other: Vec<(String, J)>,
}

/// What the file says: nothing there, a layout, or why it doesn't read.
#[derive(Clone, Debug, PartialEq)]
pub enum Read {
    Absent,
    Valid(Layout),
    Invalid(String),
}

impl Layout {
    /// Read the file's text. Readers ignore keys they don't know (FMT-19); a known key of the
    /// wrong shape makes the file unreadable rather than half read.
    pub fn read(text: &str) -> Result<Layout, String> {
        let j: J = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let J::Obj(fields) = j else { return Err("it isn't a JSON object".into()) };
        let mut layout = Layout::default();
        for (k, v) in fields {
            match (k.as_str(), v) {
                ("order", J::Arr(items)) => {
                    for i in items {
                        match i {
                            J::Str(s) if !layout.order.contains(&s) => layout.order.push(s),
                            J::Str(_) => {}
                            _ => return Err("its order isn't a list of track IDs".into()),
                        }
                    }
                }
                ("order", _) => return Err("its order isn't a list of track IDs".into()),
                ("tracks", J::Obj(entries)) => {
                    for (key, entry) in entries {
                        match entry {
                            J::Obj(f) => layout.tracks.push((key, f)),
                            _ => return Err(format!("its entry for {key} isn't an object")),
                        }
                    }
                }
                ("tracks", _) => return Err("its tracks aren't an object".into()),
                ("markers", v) => layout.markers = Some(v),
                (_, v) => layout.other.push((k, v)),
            }
        }
        Ok(layout)
    }

    /// The colour stored for `key`, by name: a palette name, or a `#rrggbb` read as the nearest
    /// one by hue (rewritten as a name when that track's colour is next written).
    pub fn colour(&self, key: &str) -> Option<&'static str> {
        let (_, fields) = self.tracks.iter().find(|(k, _)| k == key)?;
        match fields.iter().find(|(k, _)| k == "colour")?.1 {
            J::Str(ref s) => named(s),
            _ => None,
        }
    }

    /// The canonical text (FMT-19): two-space JSON with `order`, `tracks` and `markers` first, then
    /// every other key in the order read.
    pub fn canonical(&self) -> String {
        let mut fields = vec![("order".to_string(), J::Arr(self.order.iter().cloned().map(J::Str).collect()))];
        // The tracks in the order's order; any it doesn't list after, as read.
        let mut tracks = self.tracks.clone();
        tracks.sort_by_key(|(k, _)| self.order.iter().position(|o| o == k).unwrap_or(usize::MAX));
        fields.push(("tracks".into(), J::Obj(tracks.into_iter().map(|(k, f)| (k, J::Obj(f))).collect())));
        if let Some(m) = &self.markers {
            fields.push(("markers".into(), m.clone()));
        }
        fields.extend(self.other.iter().cloned());
        let mut out = String::new();
        J::Obj(fields).write(&mut out, 0);
        out.push('\n');
        out
    }

    /// Set `key`'s colour, keeping its other fields.
    fn set_colour(&mut self, key: &str, colour: &str) {
        let value = J::Str(colour.to_string());
        match self.tracks.iter_mut().find(|(k, _)| k == key) {
            Some((_, f)) => match f.iter_mut().find(|(k, _)| k == "colour") {
                Some(c) => c.1 = value,
                None => f.insert(0, ("colour".into(), value)),
            },
            None => self.tracks.push((key.to_string(), vec![("colour".into(), value)])),
        }
    }
}

/// A palette name, or the one nearest a `#rrggbb` by hue.
pub fn named(s: &str) -> Option<&'static str> {
    if let Some(n) = DEFAULT_ORDER.iter().find(|n| **n == s) {
        return Some(n);
    }
    let hex = s.strip_prefix('#').filter(|h| h.len() == 6)?;
    let c = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok().map(|v| v as f64 / 255.0);
    let hue = oklch_hue(c(0)?, c(2)?, c(4)?);
    let dist = |h: f64| {
        let d = (hue - h).abs() % 360.0;
        d.min(360.0 - d)
    };
    HUES.iter().min_by(|a, b| dist(a.1).total_cmp(&dist(b.1))).map(|(n, _)| *n)
}

/// The OKLCH hue of an sRGB colour, in degrees.
fn oklch_hue(r: f64, g: f64, b: f64) -> f64 {
    let lin = |c: f64| if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
    let (r, g, b) = (lin(r), lin(g), lin(b));
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    let a = 1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s;
    let bb = 0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s;
    bb.atan2(a).to_degrees().rem_euclid(360.0)
}

/// `layout.json` in the project at `dir`, as the disk has it now.
pub fn read_disk(dir: &Path) -> Read {
    match std::fs::read(dir.join(FILE)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Read::Absent,
        Err(e) => Read::Invalid(e.to_string()),
        Ok(bytes) => match String::from_utf8(bytes) {
            Err(_) => Read::Invalid("it isn't UTF-8 text".into()),
            Ok(text) => match Layout::read(&text) {
                Ok(l) => Read::Valid(l),
                Err(e) => Read::Invalid(e),
            },
        },
    }
}

/// The order and colours the app shows (taste.md §3.4, §2.7).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayoutView {
    /// Every track, top to bottom.
    pub order: Vec<String>,
    /// Every track's colour, by name.
    pub colours: BTreeMap<String, &'static str>,
    /// Some colour is stored for a track there is: the first layout write froze them (rule 4).
    pub stored: bool,
    /// Why `layout.json` doesn't read (`layout.invalid`): the defaults show meanwhile.
    pub invalid: Option<String>,
}

/// The first colour in the default order that none of `used` is; when all twelve are, the least
/// used, ties going to the default order (rule 5).
pub fn unused<'a>(used: impl IntoIterator<Item = &'a str>) -> &'static str {
    let mut count: BTreeMap<&str, usize> = DEFAULT_ORDER.iter().map(|n| (*n, 0)).collect();
    for u in used {
        if let Some(c) = count.get_mut(u) {
            *c += 1;
        }
    }
    let least = DEFAULT_ORDER.iter().map(|n| count[n]).min().unwrap_or(0);
    DEFAULT_ORDER.iter().find(|n| count[*n] == least).expect("twelve colours")
}

/// The effective layout of `tracks`, which come in their default order (the mixer's channel
/// order, then import order: never alphabetical, D8): the stored order of the tracks there are,
/// then the rest; each track's stored colour, else the first one no track uses, or with nothing
/// stored at all, colours by place.
pub fn effective(tracks: &[String], read: &Read) -> LayoutView {
    let (layout, invalid) = match read {
        Read::Valid(l) => (Some(l), None),
        Read::Absent => (None, None),
        Read::Invalid(e) => (None, Some(e.clone())),
    };
    let mut order: Vec<String> = layout.map_or(vec![], |l| l.order.iter().filter(|k| tracks.contains(k)).cloned().collect());
    let rest: Vec<String> = tracks.iter().filter(|k| !order.contains(k)).cloned().collect();
    order.extend(rest);
    let stored: BTreeMap<String, &'static str> =
        order.iter().filter_map(|k| layout.and_then(|l| l.colour(k)).map(|c| (k.clone(), c))).collect();
    let mut colours = BTreeMap::new();
    if stored.is_empty() {
        for (i, k) in order.iter().enumerate() {
            colours.insert(k.clone(), DEFAULT_ORDER[i % DEFAULT_ORDER.len()]);
        }
    } else {
        colours.extend(stored.iter().map(|(k, c)| (k.clone(), *c)));
        for k in &order {
            if !colours.contains_key(k) {
                let c = unused(colours.values().copied());
                colours.insert(k.clone(), c);
            }
        }
    }
    LayoutView { order, colours, stored: !stored.is_empty(), invalid }
}

/// `order` (the tracks there are, top to bottom) merged into what was stored: a stored key that
/// names no track keeps its place, after the key before it.
fn merged(stored: &[String], order: &[String], gone: Option<&str>) -> Vec<String> {
    // Each absent key, by the nearest key before it that is still listed (None: the top).
    let mut after: Vec<(Option<String>, String)> = vec![];
    let mut anchor: Option<String> = None;
    for k in stored {
        if Some(k.as_str()) == gone {
            continue;
        }
        if order.contains(k) {
            anchor = Some(k.clone());
        } else {
            after.push((anchor.clone(), k.clone()));
        }
    }
    let mut out: Vec<String> = after.iter().filter(|(a, _)| a.is_none()).map(|(_, k)| k.clone()).collect();
    for k in order {
        out.push(k.clone());
        out.extend(after.iter().filter(|(a, _)| a.as_deref() == Some(k.as_str())).map(|(_, x)| x.clone()));
    }
    out
}

/// A change to the layout, as a builder writes it.
pub struct Change<'a> {
    /// The tracks there will be, top to bottom.
    pub order: Vec<String>,
    /// Their colours: every one is written, so the first write freezes them all (rule 4).
    pub colours: &'a BTreeMap<String, &'static str>,
    /// A track deleted: its key leaves `order` and `tracks` (the only way one does).
    pub gone: Option<&'a str>,
}

/// The text `layout.json` is to have after `change`, from what it says now (`read`); None when it
/// doesn't read, which only Reset layout writes over.
pub fn text_after(read: &Read, change: &Change) -> Option<String> {
    let mut l = match read {
        Read::Valid(l) => l.clone(),
        Read::Absent => Layout::default(),
        Read::Invalid(_) => return None,
    };
    l.order = merged(&l.order, &change.order, change.gone);
    if let Some(g) = change.gone {
        l.tracks.retain(|(k, _)| k != g);
    }
    for k in &change.order {
        if let Some(c) = change.colours.get(k) {
            l.set_colour(k, c);
        }
    }
    Some(l.canonical())
}

/// The op that writes `layout.json`, and its base read from the disk (a snapshot never holds it,
/// PLANE-3): what the file says now, or Absent.
pub fn write_op(s: &Session, text: String) -> (Op, TxPath, Base) {
    let file = TxPath::new(FILE).expect("layout.json is a Tx path");
    let base = plan::base_from_disk(s, FILE);
    (Op::Write { file: file.clone(), bytes: Payload::Text(text.into()) }, file, base)
}

/// The tracks of the accepted revision in their default order: the mixer's channel order
/// (ID-18), then the rest in the order the build made them.
pub fn default_order(s: &Session) -> Vec<String> {
    let rev = s.accepted();
    let tracks: Vec<String> = rev.compiled.ir.modules.iter().filter(|m| m.track).map(|m| m.key.clone()).collect();
    let mut out: Vec<String> = vec![];
    if let crate::mix::Mixing::One(mixer) = crate::mix::mixer(rev) {
        for c in &mixer.channels {
            for t in &c.tracks {
                if tracks.contains(t) && !out.contains(t) {
                    out.push(t.clone());
                }
            }
        }
    }
    let rest: Vec<String> = tracks.iter().filter(|t| !out.contains(t)).cloned().collect();
    out.extend(rest);
    out
}

/// The accepted revision's layout, as the disk has it now.
pub fn view(s: &Session) -> LayoutView {
    effective(&default_order(s), &read_disk(&s.accepted().project.dir))
}

/// Refused while the file doesn't read: what reorder and recolour say (R15).
fn unreadable() -> EditError {
    EditError::Rejected("layout.json doesn't read: fix it, or reset the layout".into())
}

/// A Tx that writes only `layout.json`: no revision (PLANE-3), one undo step.
fn layout_tx(s: &Session, label: (String, String), text: String) -> Tx {
    let (op, file, base) = write_op(s, text);
    let mut tx = plan::planned(s, label, vec![op], Expect::Identical);
    tx.base.insert(file, base);
    tx
}

/// Commit a layout-only Tx, saying what was written.
fn commit(s: &mut Session, tx: Tx) -> Result<crate::Edited, EditError> {
    let label = tx.label.clone();
    let done = plan::run(s, tx, |_, _| Ok(()))?;
    Ok(crate::Edited { revision: done.number, file: FILE.into(), before: label.0, after: label.1 })
}

/// Reorder the tracks: `order` must be every track there is, once each.
pub fn reorder(s: &mut Session, order: &[String]) -> Result<crate::Edited, EditError> {
    let read = read_disk(&s.accepted().project.dir);
    if matches!(read, Read::Invalid(_)) {
        return Err(unreadable());
    }
    let view = effective(&default_order(s), &read);
    let mut want = order.to_vec();
    want.sort();
    let mut have = view.order.clone();
    have.sort();
    if want != have {
        return Err(EditError::Rejected("a new order lists every track once".into()));
    }
    // Named by the track that moved: the first whose place changed.
    let label = |k: &str| s.accepted().compiled.ir.modules.iter().find(|m| m.key == k).map_or(k.to_string(), |m| m.label().to_string());
    let moved = order.iter().zip(&view.order).find(|(a, b)| a != b).map_or("tracks".to_string(), |(a, _)| label(a));
    let text = text_after(&read, &Change { order: order.to_vec(), colours: &view.colours, gone: None }).ok_or_else(unreadable)?;
    commit(s, layout_tx(s, (moved.clone(), moved), text))
}

/// Recolour track `module` with palette colour `colour`.
pub fn recolour(s: &mut Session, module: &str, colour: &str) -> Result<crate::Edited, EditError> {
    let Some(colour) = DEFAULT_ORDER.iter().find(|n| **n == colour).copied() else {
        return Err(EditError::Rejected(format!("{colour} isn't a track colour: {}", DEFAULT_ORDER.join(", "))));
    };
    let read = read_disk(&s.accepted().project.dir);
    if matches!(read, Read::Invalid(_)) {
        return Err(unreadable());
    }
    let mut view = effective(&default_order(s), &read);
    let Some(was) = view.colours.get(module).copied() else { return Err(EditError::NotFound(module.to_string())) };
    view.colours.insert(module.to_string(), colour);
    let text = text_after(&read, &Change { order: view.order.clone(), colours: &view.colours, gone: None }).ok_or_else(unreadable)?;
    commit(s, layout_tx(s, (was.to_string(), colour.to_string()), text))
}

/// Reset layout (`layout.invalid`'s fix): the defaults written over what is there, based on it, so
/// undo brings the text back.
pub fn reset(s: &mut Session) -> Result<crate::Edited, EditError> {
    let view = effective(&default_order(s), &Read::Absent);
    let text = text_after(&Read::Absent, &Change { order: view.order.clone(), colours: &view.colours, gone: None }).expect("the defaults");
    commit(s, layout_tx(s, ("layout".into(), "the default layout".into()), text))
}

/// What needs attention in the layout: `layout.invalid` when the file doesn't read (DIAG-2), with
/// Reset layout, whose Tx is based on the text there, so undo brings it back.
pub fn problems(s: &Session) -> Vec<m2_song::Diagnostic> {
    let Read::Invalid(why) = read_disk(&s.accepted().project.dir) else { return vec![] };
    let view = effective(&default_order(s), &Read::Absent);
    let text = text_after(&Read::Absent, &Change { order: view.order.clone(), colours: &view.colours, gone: None }).expect("the defaults");
    let (op, file, base) = write_op(s, text);
    let fix = m2_song::Fix::new(
        "reset-layout",
        "Reset layout",
        true,
        ("layout".into(), "the default layout".into()),
        [(file, base)].into(),
        vec![op],
        Expect::Identical,
    );
    let message =
        format!("layout.json doesn't read ({why}), so the tracks show in their own order and colours. Fix it, or reset the layout.");
    vec![m2_song::Diagnostic::notice("layout.invalid", message).file(FILE).fix(fix)]
}

#[cfg(test)]
#[path = "layout_tests.rs"]
mod tests;
