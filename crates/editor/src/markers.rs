//! Markers on the ruler (Q13, design-8b §7.6): `layout.json`'s `markers`, a list of
//! `{ "beat": 64, "name": "Chorus" }` sorted by beat, one a beat, each name one line of 1 to 40
//! characters. Shared presentation, as the order and colours are (FMT-19): nothing that plays reads
//! them, and a write is a layout-only Tx, one undo step with no revision (PLANE-3).

use m2_session::Session;
use m2_song::Arrangement;
use serde::{Deserialize, Serialize};

use crate::layout::{self, J, Layout, Read};
use crate::{EditError, Edited};

/// The longest name, in characters.
pub const NAME_MAX: usize = 40;

/// A marker: where, in beats from the song's start, and what it is called.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    pub beat: f64,
    pub name: String,
}

/// The markers `layout` holds: its well-formed entries, by beat, the first of any two on one beat.
/// Others are left out, as a reader ignores what it doesn't know (FMT-19).
pub fn read(layout: &Layout) -> Vec<Marker> {
    let Some(J::Arr(items)) = &layout.markers else { return vec![] };
    let mut out: Vec<Marker> = items
        .iter()
        .filter_map(|item| {
            let J::Obj(fields) = item else { return None };
            let get = |k: &str| fields.iter().find(|(x, _)| x == k).map(|(_, v)| v);
            let beat = match get("beat")? {
                J::Num(n) => n.as_f64()?,
                _ => return None,
            };
            let name = match get("name")? {
                J::Str(s) => s.clone(),
                _ => return None,
            };
            (beat.is_finite() && beat >= 0.0).then_some(Marker { beat, name })
        })
        .collect();
    out.sort_by(|a, b| a.beat.total_cmp(&b.beat));
    out.dedup_by(|b, a| same_beat(a.beat, b.beat));
    out
}

/// The markers of the project at `dir`, as the disk has them: none while `layout.json` doesn't read.
pub fn on_disk(dir: &std::path::Path) -> Vec<Marker> {
    match layout::read_disk(dir) {
        Read::Valid(l) => read(&l),
        _ => vec![],
    }
}

fn same_beat(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

/// A beat as JSON writes it: a whole one without its `.0`.
fn number(beat: f64) -> J {
    if beat.fract() == 0.0 && beat < 1e15 {
        J::Num((beat as i64).into())
    } else {
        J::Num(serde_json::Number::from_f64(beat).expect("finite"))
    }
}

/// Set the song's markers to `markers`, the whole list (any order). Refused, in words, while
/// `layout.json` doesn't read, for two on one beat, a beat before the start, or a name that isn't
/// one line of 1 to 40 characters. Every other key of the file stays as it was.
pub fn set(s: &mut Session, mut markers: Vec<Marker>) -> Result<Edited, EditError> {
    let read = layout::read_disk(&s.accepted().project.dir);
    let mut l = match read {
        Read::Valid(l) => l,
        Read::Absent => Layout::default(),
        Read::Invalid(_) => return Err(layout::unreadable()),
    };
    let arrangement = s.accepted().project.arrangement.clone();
    let bar = |beat: f64| arrangement.bar_at(beat).0 + 1;
    let no = |why: String| Err(EditError::Rejected(why));
    for m in &markers {
        if !m.beat.is_finite() || m.beat < 0.0 {
            return no("A marker goes on a beat from the song's start on".into());
        }
        if m.name.trim().is_empty() {
            return no("A marker needs a name".into());
        }
        if m.name.contains(['\n', '\r']) {
            return no("A marker's name is one line".into());
        }
        if m.name.chars().count() > NAME_MAX {
            return no(format!("A marker's name is {NAME_MAX} characters at most"));
        }
    }
    markers.sort_by(|a, b| a.beat.total_cmp(&b.beat));
    if let Some(w) = markers.windows(2).find(|w| same_beat(w[0].beat, w[1].beat)) {
        return no(format!("A marker is at bar {} already", bar(w[1].beat)));
    }
    let before = self::read(&l);
    let label = label(&before, &markers, &arrangement);
    if before == markers {
        return Ok(Edited { revision: s.accepted().number, file: layout::FILE.into(), before: label.0, after: label.1 });
    }
    put(&mut l, &markers);
    let text = l.canonical();
    layout::commit(s, layout::layout_tx(s, label, text))
}

/// Write `markers` (sorted, one a beat) as `layout`'s list: none takes the key out.
pub(crate) fn put(l: &mut Layout, markers: &[Marker]) {
    l.markers = (!markers.is_empty()).then(|| {
        J::Arr(markers.iter().map(|m| J::Obj(vec![("beat".into(), number(m.beat)), ("name".into(), J::Str(m.name.clone()))])).collect())
    });
}

/// What a change to the list did, in words, as (before, after): one marker added ("", "Chorus at
/// bar 17"), deleted ("Chorus", ""), renamed ("Chorus", "Big Chorus") or moved ("Chorus at bar
/// 17", "Chorus at bar 25"; inside one bar with the beats, "Chorus at bar 9 beat 1", "Chorus at
/// bar 9 beat 2.5", so the two sides differ: the page names an undo step only when they do); any
/// other change ("markers", "markers").
fn label(before: &[Marker], after: &[Marker], arrangement: &Arrangement) -> (String, String) {
    let gone: Vec<&Marker> = before.iter().filter(|m| !after.contains(m)).collect();
    let new: Vec<&Marker> = after.iter().filter(|m| !before.contains(m)).collect();
    let bar = |m: &Marker| arrangement.bar_at(m.beat).0;
    let at = |m: &Marker| format!("{} at bar {}", m.name, bar(m) + 1);
    let place = |m: &Marker| format!("{} at {}", m.name, crate::notes::place(arrangement, m.beat));
    match (gone.as_slice(), new.as_slice()) {
        ([], [m]) => (String::new(), at(m)),
        ([m], []) => (m.name.clone(), String::new()),
        ([a], [b]) if same_beat(a.beat, b.beat) => (a.name.clone(), b.name.clone()),
        ([a], [b]) if bar(a) == bar(b) => (place(a), place(b)),
        ([a], [b]) => (at(a), at(b)),
        _ => ("markers".into(), "markers".into()),
    }
}

#[cfg(test)]
#[path = "markers_tests.rs"]
mod tests;
