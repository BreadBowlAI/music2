//! Sections (gap #23, ARR-1 to ARR-9): a span of whole bars across every lane (a verse, a chorus),
//! and what can be done to it at once on every track: paste a copy of one, duplicate it, move it,
//! empty it, insert bars and delete bars.
//!
//! A gesture is pure first ([`apply`]): what it makes of every lane, of the arrangement and of the
//! markers, planned on the accepted revision. [`edit`] makes it one Tx: a `Clips` op for each lane
//! that changes (as a clip gesture writes it), and `data/arrangement.json` and `layout.json` written
//! whole when they change; `Confined { those lanes }`, which admits the arrangement it writes. No code
//! changes, so a song whose draft doesn't build still takes it (WRITE-14).
//!
//! The rules every gesture keeps:
//! - **edges:** a clip that crosses an edge of a span is split there, as Split splits (ARR-3).
//! - **placing:** what a section puts in bars replaces what they held, lane by lane (ARR-4).
//! - **time:** only insert and delete bars move the markers and the tempo and time signature
//!   changes (ARR-6 to ARR-8).

use std::collections::BTreeMap;

use m2_session::{Revision, Session};
use m2_song::tx::{Confined, Expect, Op, Payload};
use m2_song::{Arrangement, DataPath, LaneAddr, LaneKind, Meter, TempoMap};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::clips::{self, Clip, ClipsEdited, LaneEdit};
use crate::layout::{self, Layout, Read};
use crate::markers::{self, Marker};
use crate::plan;
use crate::{EditError, Edited};

/// The most bars one Insert bars opens.
pub const BARS_MAX: u32 = 999;

/// How close two beats are to be one: a bar line to the 960th of a beat's millionth.
const EPS: f64 = 1e-9;

/// A copied section (ARR-5): its span, and for every lane of the song the clips that overlap it,
/// whole, as the lane format writes them. A paste cuts them to the span.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Section {
    pub start: f64,
    pub end: f64,
    pub lanes: Vec<SectionLane>,
}

/// One lane of a copied section.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SectionLane {
    #[serde(flatten)]
    pub lane: LaneAddr,
    pub clips: Vec<Value>,
}

/// What a producer does to a section (ARR-2). Beats from the song's start, each on a bar line.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub enum Gesture {
    /// A copied section at bar line `at`, in place of what was there.
    Paste { section: Section, at: f64 },
    /// A copy of the section at bar line `to`, or right after itself.
    Duplicate {
        start: f64,
        end: f64,
        #[serde(default)]
        to: Option<f64>,
    },
    /// The section at bar line `to`, a gap where it was.
    Move { start: f64, end: f64, to: f64 },
    /// The section emptied: a gap.
    Delete { start: f64, end: f64 },
    /// `bars` empty bars at bar line `at`: everything from there on moves later.
    InsertBars { at: f64, bars: u32 },
    /// The bars taken out: everything after moves earlier.
    DeleteBars { start: f64, end: f64 },
}

/// What a gesture makes of the song: each lane that changes, and the arrangement and the markers
/// when they change; with its words for history.
#[derive(Clone, Debug, PartialEq)]
pub struct Planned {
    pub lanes: Vec<LaneEdit>,
    pub arrangement: Option<Arrangement>,
    pub markers: Option<Vec<Marker>>,
    pub label: (String, String),
}

/// A lane of the song as a gesture finds it: its clips, and why its file can't be written, when it
/// doesn't read (it plays the clips it had before, FMT-9, and writing those would lose its text).
struct Now {
    kind: LaneKind,
    was: Vec<Clip>,
    broken: Option<String>,
}

/// Every lane the accepted revision declares, by address.
fn lanes(rev: &Revision) -> BTreeMap<LaneAddr, Now> {
    let mut out = BTreeMap::new();
    for (addr, kind) in &rev.compiled.decls.lanes {
        let shown = DataPath::lane(addr).to_string();
        let broken = rev.compiled.problems.iter().find(|d| d.src.as_deref() == Some(&shown) && d.code.starts_with("lane."));
        let broken = broken.map(|d| format!("{shown} doesn't read as the lane right now ({}): fix the file first", d.code));
        let was = match rev.compiled.data.lane(addr.module.as_str(), addr.lane.as_str()) {
            Some(d) if d.fits(kind) => clips::clips_of(d),
            _ => vec![],
        };
        out.insert(addr.clone(), Now { kind: kind.clone(), was, broken });
    }
    out
}

/// The clips of `clips` cut at the span `a` to `b` (ARR-3): those outside it, whole or split off,
/// and those inside. A clip that crosses an edge is split there, as Split splits.
fn cut(clips: &[Clip], a: f64, b: f64, tempo: &TempoMap) -> Result<(Vec<Clip>, Vec<Clip>), EditError> {
    let (mut outside, mut inside) = (vec![], vec![]);
    for c in clips {
        if c.end() <= a || c.start() >= b {
            outside.push(c.clone());
            continue;
        }
        let mut rest = c.clone();
        if rest.start() < a {
            let (before, from) = rest.split(a, tempo)?;
            outside.push(before);
            rest = from;
        }
        if rest.end() > b {
            let (within, after) = rest.split(b, tempo)?;
            inside.push(within);
            outside.push(after);
        } else {
            inside.push(rest);
        }
    }
    Ok((outside, inside))
}

/// `clips` moved by `by` beats, each a copy with no id (it gets a fresh one) unless `keep`.
fn moved(clips: Vec<Clip>, by: f64, keep: bool) -> impl Iterator<Item = Clip> {
    clips.into_iter().map(move |mut c| {
        c.set_start(c.start() + by);
        if !keep {
            c.set_id(None);
        }
        c
    })
}

/// `beat`, which must be a bar line of the song (ARR-1): `what` says which in the refusal.
fn bar_line(arr: &Arrangement, beat: f64, what: &str) -> Result<f64, EditError> {
    if !(beat.is_finite() && beat >= 0.0) {
        return Err(EditError::Rejected(format!("{what} is a beat from the song's start on, not {beat}.")));
    }
    let (bar, into) = arr.bar_at(beat);
    match into.abs() < EPS || (arr.meter_at(beat).bar() - into).abs() < EPS {
        true => Ok(beat),
        false => Err(EditError::Rejected(format!(
            "{what} must be on a bar line: beat {} is inside bar {}.",
            m2_song::lane::number(beat),
            bar + 1
        ))),
    }
}

/// A section's span: both ends on bar lines, the end after the start (ARR-1).
fn span(arr: &Arrangement, start: f64, end: f64) -> Result<(f64, f64), EditError> {
    let (a, b) = (bar_line(arr, start, "A section's start")?, bar_line(arr, end, "A section's end")?);
    match b > a {
        true => Ok((a, b)),
        false => Err(EditError::Rejected("A section ends after it starts: pick one bar or more.".into())),
    }
}

/// `bars 9–16`, or `bar 9`: the bars a span covers, counted from 1 in the song's meter.
pub fn bars_words(arr: &Arrangement, a: f64, b: f64) -> String {
    let first = arr.bar_at(a).0 + 1;
    let last = arr.bar_at((b - EPS).max(a)).0 + 1;
    match first == last {
        true => format!("bar {first}"),
        false => format!("bars {first}–{last}"),
    }
}

/// The time signature the bars inserted at bar line `at` take: the one just before it, or at the
/// song's start, its own (ARR-6).
fn meter_before(arr: &Arrangement, at: f64) -> Meter {
    match at > 0.0 {
        true => arr.meter.iter().rev().find(|m| m.beat < at).copied().unwrap_or(Meter::COMMON),
        false => arr.meter.first().copied().unwrap_or(Meter::COMMON),
    }
}

/// `arr` with `len` beats opened at `at`: every change at or after it later by them, but the song's
/// own first tempo and time signature, at beat 0 (ARR-6).
fn inserted(arr: &Arrangement, at: f64, len: f64) -> Arrangement {
    let mut a = arr.clone();
    for t in a.tempo.iter_mut().skip(1).filter(|t| t.beat >= at) {
        t.beat += len;
    }
    for m in a.meter.iter_mut().skip(1).filter(|m| m.beat >= at) {
        m.beat += len;
    }
    a
}

/// `entries` (changes in effect until the next, by beat) with the span `a` to `b` taken out
/// (ARR-7): those before it stay, those at or after its end move earlier by its length, and of those
/// inside it only the one in effect at its end stays, at its start. One that lands at the start
/// saying what `same` says plays there already is left out.
fn closed<T: Clone>(
    entries: &[T],
    beat: impl Fn(&T) -> f64,
    set: impl Fn(&mut T, f64),
    same: impl Fn(&T, &T) -> bool,
    a: f64,
    b: f64,
) -> Vec<T> {
    let mut out: Vec<T> = entries.iter().filter(|e| beat(e) < a).cloned().collect();
    let land = |mut e: T, at: f64, out: &mut Vec<T>| {
        set(&mut e, at);
        if !(at == a && out.last().is_some_and(|p| same(p, &e))) {
            out.push(e);
        }
    };
    if !entries.iter().any(|e| (beat(e) - b).abs() < EPS)
        && let Some(last) = entries.iter().filter(|e| beat(e) >= a && beat(e) < b).last()
    {
        land(last.clone(), a, &mut out);
    }
    for e in entries.iter().filter(|e| beat(e) >= b - EPS) {
        let at = (beat(e) - (b - a)).max(a);
        land(e.clone(), at, &mut out);
    }
    out
}

/// What a copied clip is, by its shape: the kind of lane it came from.
fn shape(v: &Value) -> Option<&'static str> {
    let has = |k: &str| v.get(k).is_some();
    match () {
        _ if has("notes") => Some("notes"),
        _ if has("asset") => Some("audio"),
        _ if has("points") => Some("automation"),
        _ => None,
    }
}

/// The kind of clip a lane of `kind` holds, in the same words.
fn holds(kind: &LaneKind) -> &'static str {
    match kind {
        LaneKind::Notes => "notes",
        LaneKind::Audio => "audio",
        LaneKind::Control { .. } => "automation",
    }
}

/// The clips of `lane` in a copied section, read as clips of the lane of the same keys now (ARR-5):
/// a lane the song doesn't declare now, or one of another kind, refuses the paste.
fn copied(rev: &Revision, now: &BTreeMap<LaneAddr, Now>, lane: &SectionLane) -> Result<Vec<Clip>, EditError> {
    let name = || {
        let m = rev.compiled.ir.modules.iter().find(|m| m.key == lane.lane.module.as_str());
        let l = m.and_then(|m| m.lanes.iter().find(|l| l.key == lane.lane.lane.as_str()));
        match (m, l) {
            (Some(m), Some(l)) => format!("{}'s {} lane", m.label(), l.label()),
            _ => format!("{}'s {} lane", lane.lane.module, lane.lane.lane),
        }
    };
    let Some(n) = now.get(&lane.lane) else {
        return Err(EditError::Missing(format!("The copy has {}, which the song doesn't have now; copy the bars again.", name())));
    };
    lane.clips
        .iter()
        .map(|v| match shape(v) {
            Some(s) if s == holds(&n.kind) => clips::parse(v, &n.kind),
            Some(s) => Err(EditError::Rejected(format!("The copy has {s} clips for {}, which holds {} now.", name(), holds(&n.kind)))),
            None => Err(EditError::Rejected("A copied clip holds notes, points or audio.".into())),
        })
        .collect()
}

/// The section `start` to `end` of `rev`, as a copy of it is kept (ARR-5): every lane of the song,
/// each with the clips that overlap the span, whole. The window makes it from the timeline it draws.
pub fn section(rev: &Revision, start: f64, end: f64) -> Result<Section, EditError> {
    let (a, b) = span(&rev.project.arrangement, start, end)?;
    let lanes = lanes(rev)
        .into_iter()
        .map(|(lane, n)| {
            let over = n.was.iter().filter(|c| c.start() < b && c.end() > a);
            let clips = over.map(|c| serde_json::from_str(&c.line(&n.kind)).expect("a canonical line is JSON")).collect();
            SectionLane { lane, clips }
        })
        .collect();
    Ok(Section { start: a, end: b, lanes })
}

/// What `g` makes of the song, planned on `rev` with its markers `markers` (from `layout.json`):
/// only lanes that change. `mint` gives a fresh id for a clip on a lane, as often as asked. Pure:
/// the same revision, markers, gesture and ids give the same plan.
pub fn apply(rev: &Revision, markers: &[Marker], g: &Gesture, mint: &mut dyn FnMut(&LaneAddr) -> String) -> Result<Planned, EditError> {
    let arr = &rev.project.arrangement;
    let tempo = rev.compiled.data.tempo();
    let now = lanes(rev);
    // Each lane's clips after the gesture, each with whether the gesture put it there (to select).
    let mut after: BTreeMap<LaneAddr, Vec<(Clip, bool)>> = BTreeMap::new();
    let (mut arrangement, mut moved_markers) = (None, None);
    let kept = |cs: Vec<Clip>| cs.into_iter().map(|c| (c, false)).collect::<Vec<_>>();
    let label = match g {
        Gesture::Paste { section, at } => {
            let at = bar_line(arr, *at, "A paste")?;
            let (a, b) = (section.start, section.end);
            if !(a.is_finite() && b.is_finite() && a >= 0.0 && b > a) {
                return Err(EditError::Rejected("The copy's bars don't read: copy them again.".into()));
            }
            if section.lanes.is_empty() {
                return Err(EditError::Rejected("Nothing to paste.".into()));
            }
            for lane in &section.lanes {
                let (_, inside) = cut(&copied(rev, &now, lane)?, a, b, tempo)?;
                let (outside, _) = cut(&now[&lane.lane].was, at, at + (b - a), tempo)?;
                let mut out = kept(outside);
                out.extend(moved(inside, at - a, false).map(|c| (c, true)));
                after.insert(lane.lane.clone(), out);
            }
            (bars_words(arr, at, at + (b - a)), format!("a copy of {}", bars_words(arr, a, b)))
        }
        Gesture::Duplicate { start, end, to } => {
            let (a, b) = span(arr, *start, *end)?;
            let to = match to {
                Some(t) => bar_line(arr, *t, "Where the copy goes")?,
                None => b,
            };
            for (lane, n) in &now {
                let (_, inside) = cut(&n.was, a, b, tempo)?;
                let (outside, _) = cut(&n.was, to, to + (b - a), tempo)?;
                let mut out = kept(outside);
                out.extend(moved(inside, to - a, false).map(|c| (c, true)));
                after.insert(lane.clone(), out);
            }
            (bars_words(arr, to, to + (b - a)), format!("a copy of {}", bars_words(arr, a, b)))
        }
        Gesture::Move { start, end, to } => {
            let (a, b) = span(arr, *start, *end)?;
            let to = bar_line(arr, *to, "Where the section goes")?;
            for (lane, n) in &now {
                let (rest, inside) = cut(&n.was, a, b, tempo)?;
                let (outside, _) = cut(&rest, to, to + (b - a), tempo)?;
                let mut out = kept(outside);
                out.extend(moved(inside, to - a, true).map(|c| (c, true)));
                after.insert(lane.clone(), out);
            }
            (bars_words(arr, a, b), bars_words(arr, to, to + (b - a)))
        }
        Gesture::Delete { start, end } => {
            let (a, b) = span(arr, *start, *end)?;
            for (lane, n) in &now {
                after.insert(lane.clone(), kept(cut(&n.was, a, b, tempo)?.0));
            }
            (bars_words(arr, a, b), format!("empty {}", bars_words(arr, a, b)))
        }
        Gesture::InsertBars { at, bars } => {
            let at = bar_line(arr, *at, "New bars")?;
            if !(1..=BARS_MAX).contains(bars) {
                return Err(EditError::Rejected(format!("Insert 1 to {BARS_MAX} bars.")));
            }
            let len = *bars as f64 * meter_before(arr, at).bar();
            for (lane, n) in &now {
                let mut out = vec![];
                for c in &n.was {
                    if c.end() <= at {
                        out.push((c.clone(), false));
                    } else if c.start() >= at {
                        out.extend(moved(vec![c.clone()], len, true).map(|c| (c, false)));
                    } else {
                        let (before, from) = c.split(at, tempo)?;
                        out.push((before, false));
                        out.extend(moved(vec![from], len, true).map(|c| (c, false)));
                    }
                }
                after.insert(lane.clone(), out);
            }
            arrangement = Some(inserted(arr, at, len));
            moved_markers =
                Some(markers.iter().map(|m| Marker { beat: if m.beat >= at { m.beat + len } else { m.beat }, ..m.clone() }).collect());
            let n = arr.bar_at(at).0 + 1;
            let words = if *bars == 1 { "1 empty bar".to_string() } else { format!("{bars} empty bars") };
            (format!("bar {n}"), format!("{words} at bar {n}"))
        }
        Gesture::DeleteBars { start, end } => {
            let (a, b) = span(arr, *start, *end)?;
            for (lane, n) in &now {
                let (outside, _) = cut(&n.was, a, b, tempo)?;
                let out = outside.into_iter().map(|c| if c.start() >= b { moved(vec![c], a - b, true).next().expect("one") } else { c });
                after.insert(lane.clone(), kept(out.collect()));
            }
            let mut arr2 = arr.clone();
            arr2.tempo = closed(&arr.tempo, |t| t.beat, |t, x| t.beat = x, |p, e| p.bpm == e.bpm, a, b);
            arr2.meter = closed(&arr.meter, |m| m.beat, |m, x| m.beat = x, |p, e| (p.beats, p.unit) == (e.beats, e.unit), a, b);
            arrangement = Some(arr2);
            moved_markers = Some(closed(markers, |m| m.beat, |m, x| m.beat = x, |_, _| false, a, b));
            (bars_words(arr, a, b), format!("{} taken out", bars_words(arr, a, b)))
        }
    };
    let mut edits = vec![];
    for (lane, out) in after {
        let n = &now[&lane];
        let same = clips::lane_data(&n.kind, out.iter().map(|(c, _)| c.clone()).collect()) == clips::lane_data(&n.kind, n.was.clone());
        if same {
            continue;
        }
        if let Some(why) = &n.broken {
            return Err(EditError::Rejected(why.clone()));
        }
        edits.push(clips::finished(lane, &n.kind, out, mint));
    }
    Ok(Planned {
        lanes: edits,
        arrangement: arrangement.filter(|a| a != arr),
        markers: moved_markers.filter(|m: &Vec<Marker>| m.as_slice() != markers),
        label,
    })
}

/// [`apply`] as [`edit_as`] with Tx id `tx_id` applies it: the ids it mints are the ones that Tx
/// writes (FMT-11).
pub fn apply_as(rev: &Revision, markers: &[Marker], g: &Gesture, tx_id: &str) -> Result<Planned, EditError> {
    let mut n = 0;
    let mut mint = |lane: &LaneAddr| {
        n += 1;
        m2_song::ClipId::derive(tx_id, lane, n - 1)
    };
    apply(rev, markers, g, &mut mint)
}

/// Make `g` one Tx on the session, one undo step (ARR-2).
pub fn edit(s: &mut Session, g: &Gesture) -> Result<ClipsEdited, EditError> {
    edit_as(s, g, &m2_song::tx::random_id())
}

/// [`edit`] as the Tx with id `tx_id`.
pub fn edit_as(s: &mut Session, g: &Gesture, tx_id: &str) -> Result<ClipsEdited, EditError> {
    let rev = s.accepted();
    // A data Tx needs the lanes an accepted build declared; a draft that doesn't build since is no
    // bar (WRITE-14).
    if !rev.has_code() {
        return Err(EditError::DraftInvalid(s.draft_errors.clone()));
    }
    let read = layout::read_disk(&rev.project.dir);
    let marks = match &read {
        Read::Valid(l) => markers::read(l),
        _ => vec![],
    };
    if matches!(read, Read::Invalid(_)) && matches!(g, Gesture::InsertBars { .. } | Gesture::DeleteBars { .. }) {
        return Err(EditError::Rejected(
            "layout.json doesn't read, so its markers can't move with the bars: fix it, or reset the layout".into(),
        ));
    }
    let planned = apply_as(rev, &marks, g, tx_id)?;
    let made: Vec<(LaneAddr, String)> = planned.lanes.iter().flat_map(|e| e.made.iter().map(|id| (e.lane.clone(), id.clone()))).collect();
    let (mut ops, wrote) = clips::lane_ops(rev, &planned.lanes)?;
    let mut shown: Vec<String> = wrote.iter().map(|w| DataPath::lane(&w.lane).to_string()).collect();
    if let Some(a) = &planned.arrangement {
        let text = a.canonical();
        // The reader's own words, should a change land off a bar line (it can't: FMT-13 holds).
        Arrangement::read(&text).map_err(|d| EditError::Rejected(d.message))?;
        ops.push(Op::Write { file: plan::path(crate::song::FILE)?, bytes: Payload::Text(text.into()) });
        shown.push(crate::song::FILE.into());
    }
    let mut layout_base = None;
    if let Some(m) = &planned.markers {
        let mut l = match read {
            Read::Valid(l) => l,
            Read::Absent => Layout::default(),
            Read::Invalid(_) => return Err(layout::unreadable()),
        };
        markers::put(&mut l, m);
        let (op, file, base) = layout::write_op(s, l.canonical());
        ops.push(op);
        layout_base = Some((file, base));
        shown.push(layout::FILE.into());
    }
    let file = shown.first().cloned().unwrap_or_else(|| crate::song::FILE.into());
    if ops.is_empty() {
        let same = "no change".to_string();
        return Ok(ClipsEdited { edited: Edited { revision: rev.number, file, before: same.clone(), after: same }, made: vec![] });
    }
    let lanes = wrote.iter().map(|w| w.lane.clone()).collect();
    let label = planned.label.clone();
    let mut tx = plan::planned(s, label.clone(), ops, Expect::Confined(Confined { lanes, ..Confined::default() }));
    tx.id = tx_id.to_string();
    if let Some((f, b)) = layout_base {
        tx.base.insert(f, b);
    }
    let want = planned.arrangement;
    let done = plan::run(s, tx, |_, checked| {
        clips::read_as_written(checked, &wrote)?;
        match &want {
            Some(a) if checked.candidate().project.arrangement != *a => {
                Err(EditError::Rejected("the arrangement wouldn't be what was asked".into()))
            }
            _ => Ok(()),
        }
    })?;
    Ok(ClipsEdited { edited: Edited { revision: done.number, file, before: label.0, after: label.1 }, made })
}

#[cfg(test)]
#[path = "sections_tests.rs"]
mod tests;
