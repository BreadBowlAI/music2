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

/// How close two beats are to count as one.
const EPS: f64 = 1e-9;

/// A copied section (ARR-5): its span, and for every lane of the song the clips that overlap it,
/// whole, as the lane format writes them. A paste cuts them to the span.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Section {
    pub start: f64,
    pub end: f64,
    pub lanes: Vec<SectionLane>,
}

/// One lane of a copied section: its address, its kind, and its clips.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SectionLane {
    #[serde(flatten)]
    pub lane: LaneAddr,
    #[serde(flatten)]
    pub kind: Holds,
    pub clips: Vec<Value>,
}

/// What a lane holds, as a copy carries it (`"kind": "control", "unit": "hz"`): a lane's kind, as
/// the lane format reads it (FMT-10).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Holds {
    Notes,
    Control {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unit: Option<String>,
    },
    Choice {
        options: Vec<String>,
    },
    Switch,
    Audio,
}

impl From<&LaneKind> for Holds {
    fn from(k: &LaneKind) -> Holds {
        match k {
            LaneKind::Notes => Holds::Notes,
            LaneKind::Control { unit } => Holds::Control { unit: unit.clone() },
            LaneKind::Choice { options } => Holds::Choice { options: options.clone() },
            LaneKind::Switch => Holds::Switch,
            LaneKind::Audio => Holds::Audio,
        }
    }
}

impl From<&Holds> for LaneKind {
    fn from(h: &Holds) -> LaneKind {
        match h {
            Holds::Notes => LaneKind::Notes,
            Holds::Control { unit } => LaneKind::Control { unit: unit.clone() },
            Holds::Choice { options } => LaneKind::Choice { options: options.clone() },
            Holds::Switch => LaneKind::Switch,
            Holds::Audio => LaneKind::Audio,
        }
    }
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

/// A lane of the song as a gesture finds it: its kind, its clips, what a person calls it (`Bass`,
/// `cutoff`), and the file it is read from, which may not read now: it then plays the clips it had
/// before (FMT-9), and its newer text can't move with the bars.
struct Now {
    kind: LaneKind,
    was: Vec<Clip>,
    track: String,
    label: String,
    file: String,
    broken: bool,
}

impl Now {
    /// `Bass's cutoff lane`.
    fn named(&self) -> String {
        format!("{}'s {} lane", self.track, self.label)
    }
}

/// Every lane the accepted revision declares, by address.
fn lanes(rev: &Revision) -> BTreeMap<LaneAddr, Now> {
    let mut out = BTreeMap::new();
    for (addr, kind) in &rev.compiled.decls.lanes {
        let file = DataPath::lane(addr).to_string();
        let broken = rev.compiled.problems.iter().any(|d| d.src.as_deref() == Some(&file) && d.code.starts_with("lane."));
        let was = match rev.compiled.data.lane(addr.module.as_str(), addr.lane.as_str()) {
            Some(d) if d.fits(kind) => clips::clips_of(d),
            _ => vec![],
        };
        let m = rev.compiled.ir.modules.iter().find(|m| m.key == addr.module.as_str());
        let track = m.map_or_else(|| addr.module.to_string(), |m| m.label().to_string());
        let label = m
            .and_then(|m| m.lanes.iter().find(|l| l.key == addr.lane.as_str()))
            .map_or_else(|| addr.lane.to_string(), |l| l.label().to_string());
        out.insert(addr.clone(), Now { kind: kind.clone(), was, track, label, file, broken });
    }
    out
}

/// Where clips are cut (ARR-3): the song's tempo, which an audio clip's later part is placed by, and
/// its bars, which a refusal counts in.
struct Edge<'a> {
    tempo: &'a TempoMap,
    arr: &'a Arrangement,
}

impl Edge<'_> {
    /// Clip `c` of track `track` cut at song beat `at`, as Split cuts it. An audio clip Split can't
    /// cut refuses in words: which clip, and what to change in the app (review R14).
    fn split(&self, c: &Clip, at: f64, track: &str) -> Result<(Clip, Clip), EditError> {
        if let Clip::Audio(a) = c {
            let (bar, edge) = (self.arr.bar_at(a.start).0 + 1, self.arr.bar_at(at).0 + 1);
            let why = match () {
                _ if a.pitch != 0.0 => format!("has its pitch changed, so it can't be cut at bar {edge}. Set its pitch back to 0"),
                _ if a.reverse && !a.warp.is_empty() => {
                    format!("plays backwards and follows the tempo, so it can't be cut at bar {edge}. Turn Reverse off")
                }
                _ => return c.split(at, self.tempo),
            };
            return Err(EditError::Rejected(format!("{track}'s audio clip at bar {bar} {why}, or move it off bar {edge}.")));
        }
        c.split(at, self.tempo)
    }

    /// The clips of `clips` (track `track`'s) cut at the span `a` to `b`: those outside it, whole or
    /// split off, and those inside. A clip that crosses an edge is split there.
    fn cut(&self, clips: &[Clip], a: f64, b: f64, track: &str) -> Result<(Vec<Clip>, Vec<Clip>), EditError> {
        let (mut outside, mut inside) = (vec![], vec![]);
        for c in clips {
            if c.end() <= a || c.start() >= b {
                outside.push(c.clone());
                continue;
            }
            let mut rest = c.clone();
            if rest.start() < a {
                let (before, from) = self.split(&rest, a, track)?;
                outside.push(before);
                rest = from;
            }
            if rest.end() > b {
                let (within, after) = self.split(&rest, b, track)?;
                inside.push(within);
                outside.push(after);
            } else {
                inside.push(rest);
            }
        }
        Ok((outside, inside))
    }
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

/// A clip's first part before bars opened at its cut, held over them (ARR-6): an automation or a
/// names clip goes on playing its value at the cut; notes and audio leave the new bars empty.
fn held(mut c: Clip, len: f64) -> Clip {
    match &mut c {
        Clip::Control(a) => {
            if let Some(&(_, v)) = a.points.last() {
                a.points.push((a.length + len, v));
            }
            a.length += len;
        }
        Clip::Steps(a) => a.length += len,
        Clip::Notes(_) | Clip::Audio(_) => {}
    }
    c
}

/// `beat`, which must be a bar line of the song (ARR-1), on it exactly. `head` says what goes there
/// ("A paste goes at the start of a bar"); the refusal counts bars and beats from 1.
fn bar_line(arr: &Arrangement, beat: f64, head: &str) -> Result<f64, EditError> {
    if !(beat.is_finite() && beat >= 0.0) {
        return Err(EditError::Rejected(format!("{head}, and {beat} isn't a place in the song.")));
    }
    let into = arr.bar_at(beat).1;
    let left = arr.meter_at(beat).bar() - into;
    match () {
        _ if into.abs() < EPS => Ok(beat - into),
        _ if left.abs() < EPS => Ok(beat + left),
        _ => Err(EditError::Rejected(format!("{head}, not at {}.", crate::notes::place(arr, beat)))),
    }
}

/// A section's span: both ends on bar lines, the end after the start (ARR-1).
fn span(arr: &Arrangement, start: f64, end: f64) -> Result<(f64, f64), EditError> {
    let a = bar_line(arr, start, "A section starts at the start of a bar")?;
    let b = bar_line(arr, end, "A section ends at the end of a bar")?;
    match b > a {
        true => Ok((a, b)),
        false => Err(EditError::Rejected("A section ends after it starts: pick one bar or more.".into())),
    }
}

/// Bars `a` to `b` put at bar line `to`: they must end at the end of a bar there (ARR-4), or the
/// time signatures differ and the section would half cover a bar.
fn lands(arr: &Arrangement, a: f64, b: f64, to: f64) -> Result<(), EditError> {
    let end = to + (b - a);
    let (bar, into) = arr.bar_at(end);
    match into.abs() < EPS || (arr.meter_at(end).bar() - into).abs() < EPS {
        true => Ok(()),
        false => Err(EditError::Rejected(format!(
            "At bar {}, {} would end inside bar {}, where the bars are another length: put them where they end at the end of a bar.",
            arr.bar_at(to).0 + 1,
            bars_words(arr, a, b),
            bar + 1
        ))),
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
/// saying what `same` says plays there already is left out. With nothing after the span to keep in
/// step (`follows` false), those inside go, and the song's own first entry stays.
fn closed<T: Clone>(
    entries: &[T],
    beat: impl Fn(&T) -> f64,
    set: impl Fn(&mut T, f64),
    same: impl Fn(&T, &T) -> bool,
    (a, b): (f64, f64),
    follows: bool,
) -> Vec<T> {
    if !follows {
        return entries.iter().enumerate().filter(|(i, e)| beat(e) < a || (*i == 0 && beat(e) == 0.0)).map(|(_, e)| e.clone()).collect();
    }
    let mut out: Vec<T> = entries.iter().filter(|e| beat(e) < a).cloned().collect();
    let land = |mut e: T, at: f64, out: &mut Vec<T>| {
        set(&mut e, at);
        if !(at == a && out.last().is_some_and(|p| same(p, &e))) {
            out.push(e);
        }
    };
    let after = |e: &&T| beat(e) >= b - EPS;
    if !entries.iter().any(|e| (beat(e) - b).abs() < EPS)
        && let Some(last) = entries.iter().filter(|e| beat(e) >= a && !after(e)).last()
    {
        land(last.clone(), a, &mut out);
    }
    for e in entries.iter().filter(after) {
        let at = (beat(e) - (b - a)).max(a);
        land(e.clone(), at, &mut out);
    }
    out
}

/// The clips of a copied section, read as clips of the lanes of the same keys now (ARR-5), by lane.
/// A lane the song doesn't declare now is skipped when the copy holds nothing for it; one it holds
/// clips for refuses the paste, naming every such lane. A lane that doesn't take the copy's kind of
/// clip refuses it, as a clip paste does.
fn copied(now: &BTreeMap<LaneAddr, Now>, section: &Section) -> Result<Vec<(LaneAddr, Vec<Clip>)>, EditError> {
    let mut lost: Vec<String> = vec![];
    for l in section.lanes.iter().filter(|l| !now.contains_key(&l.lane) && !l.clips.is_empty()) {
        let name = format!("{}'s {} lane", l.lane.module, l.lane.lane);
        if !lost.contains(&name) {
            lost.push(name);
        }
    }
    if let Some((last, rest)) = lost.split_last() {
        let names = if rest.is_empty() { last.clone() } else { format!("{} and {last}", rest.join(", ")) };
        return Err(EditError::Rejected(format!(
            "The copy has clips for {names}, which the song doesn't have now, so it can't be pasted."
        )));
    }
    let mut out = vec![];
    for l in &section.lanes {
        let Some(n) = now.get(&l.lane) else { continue };
        let refuse = |why: String| {
            let mut c = why.chars();
            let why: String = c.next().map(|f| f.to_lowercase().chain(c).collect()).unwrap_or_default();
            EditError::Rejected(format!("{}: {why}, so the copy can't be pasted.", n.named()))
        };
        clips::fits(&LaneKind::from(&l.kind), &n.kind)
            .map_err(|why| refuse(why.replace("{from}", "the copy's clips").replace("{to}", "this lane")))?;
        let clips = l
            .clips
            .iter()
            .map(|v| clips::parse(v, &n.kind).map_err(|e| refuse(format!("A clip in the copy doesn't read ({e})"))))
            .collect::<Result<_, _>>()?;
        out.push((l.lane.clone(), clips));
    }
    Ok(out)
}

/// The section `start` to `end` of `rev`, as a copy of it is kept (ARR-5): every lane of the song,
/// each with its kind and the clips that overlap the span, whole. The window's ⌘C asks for it
/// (`POST /api/graph/sections/copy`).
pub fn section(rev: &Revision, start: f64, end: f64) -> Result<Section, EditError> {
    let (a, b) = span(&rev.project.arrangement, start, end)?;
    let lanes = lanes(rev)
        .into_iter()
        .map(|(lane, n)| {
            let over = n.was.iter().filter(|c| c.start() < b && c.end() > a);
            let clips = over.map(|c| serde_json::from_str(&c.line(&n.kind)).expect("a canonical line is JSON")).collect();
            SectionLane { lane, kind: Holds::from(&n.kind), clips }
        })
        .collect();
    Ok(Section { start: a, end: b, lanes })
}

/// What `g` makes of the song, planned on `rev` with its markers `markers` (from `layout.json`):
/// only lanes that change. `mint` gives a fresh id for a clip on a lane, as often as asked. Pure:
/// the same revision, markers, gesture and ids give the same plan.
pub fn apply(rev: &Revision, markers: &[Marker], g: &Gesture, mint: &mut dyn FnMut(&LaneAddr) -> String) -> Result<Planned, EditError> {
    let arr = &rev.project.arrangement;
    let edge = Edge { tempo: rev.compiled.data.tempo(), arr };
    let now = lanes(rev);
    // A section reaches every lane: one whose newer text can't be read can't move with it (ARR-2).
    if let Some(n) = now.values().find(|n| n.broken) {
        return Err(EditError::Rejected(format!(
            "{} doesn't read right now, so its clips can't move with the bars: fix the file first.",
            n.file
        )));
    }
    let nothing = || Planned { lanes: vec![], arrangement: None, markers: None, label: ("no change".into(), "no change".into()) };
    // Each lane's clips after the gesture, each with whether the gesture put it there (to select).
    let mut after: BTreeMap<LaneAddr, Vec<(Clip, bool)>> = BTreeMap::new();
    let (mut arrangement, mut moved_markers) = (None, None);
    let kept = |cs: Vec<Clip>| cs.into_iter().map(|c| (c, false)).collect::<Vec<_>>();
    let label = match g {
        Gesture::Paste { section, at } => {
            let at = bar_line(arr, *at, "A paste goes at the start of a bar")?;
            if !(section.start.is_finite() && section.end.is_finite()) {
                return Err(EditError::Rejected("The copy's bars aren't a span of the song: copy them again.".into()));
            }
            let (a, b) = span(arr, section.start, section.end)?;
            lands(arr, a, b, at)?;
            for (lane, clips) in copied(&now, section)? {
                let n = &now[&lane];
                let (_, inside) = edge.cut(&clips, a, b, &n.track)?;
                let (outside, _) = edge.cut(&n.was, at, at + (b - a), &n.track)?;
                let mut out = kept(outside);
                out.extend(moved(inside, at - a, false).map(|c| (c, true)));
                after.insert(lane, out);
            }
            (bars_words(arr, at, at + (b - a)), format!("a copy of {}", bars_words(arr, a, b)))
        }
        Gesture::Duplicate { start, end, to } => {
            let (a, b) = span(arr, *start, *end)?;
            let to = match to {
                Some(t) => bar_line(arr, *t, "A copy goes at the start of a bar")?,
                None => b,
            };
            if to == a {
                return Ok(nothing());
            }
            lands(arr, a, b, to)?;
            for (lane, n) in &now {
                let (_, inside) = edge.cut(&n.was, a, b, &n.track)?;
                let (outside, _) = edge.cut(&n.was, to, to + (b - a), &n.track)?;
                let mut out = kept(outside);
                out.extend(moved(inside, to - a, false).map(|c| (c, true)));
                after.insert(lane.clone(), out);
            }
            (bars_words(arr, to, to + (b - a)), format!("a copy of {}", bars_words(arr, a, b)))
        }
        Gesture::Move { start, end, to } => {
            let (a, b) = span(arr, *start, *end)?;
            let to = bar_line(arr, *to, "The bars go at the start of a bar")?;
            if to == a {
                return Ok(nothing());
            }
            lands(arr, a, b, to)?;
            for (lane, n) in &now {
                let (rest, inside) = edge.cut(&n.was, a, b, &n.track)?;
                let (outside, _) = edge.cut(&rest, to, to + (b - a), &n.track)?;
                let mut out = kept(outside);
                out.extend(moved(inside, to - a, true).map(|c| (c, true)));
                after.insert(lane.clone(), out);
            }
            (bars_words(arr, a, b), bars_words(arr, to, to + (b - a)))
        }
        Gesture::Delete { start, end } => {
            let (a, b) = span(arr, *start, *end)?;
            for (lane, n) in &now {
                after.insert(lane.clone(), kept(edge.cut(&n.was, a, b, &n.track)?.0));
            }
            (bars_words(arr, a, b), format!("empty {}", bars_words(arr, a, b)))
        }
        Gesture::InsertBars { at, bars } => {
            let at = bar_line(arr, *at, "New bars go at the start of a bar")?;
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
                        let (before, from) = edge.split(c, at, &n.track)?;
                        out.push((held(before, len), false));
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
            // Whether anything after the bars must keep its tempo, time signature and name (ARR-7).
            let follows = now.values().any(|n| n.was.iter().any(|c| c.end() > b + EPS))
                || markers.iter().any(|m| m.beat >= b - EPS)
                || arr.tempo.iter().skip(1).any(|t| t.beat >= b - EPS)
                || arr.meter.iter().skip(1).any(|m| m.beat >= b - EPS);
            for (lane, n) in &now {
                let (outside, _) = edge.cut(&n.was, a, b, &n.track)?;
                let out = outside.into_iter().map(|c| if c.start() >= b { moved(vec![c], a - b, true).next().expect("one") } else { c });
                after.insert(lane.clone(), kept(out.collect()));
            }
            let mut arr2 = arr.clone();
            arr2.tempo = closed(&arr.tempo, |t| t.beat, |t, x| t.beat = x, |p, e| p.bpm == e.bpm, (a, b), follows);
            arr2.meter = closed(&arr.meter, |m| m.beat, |m, x| m.beat = x, |p, e| (p.beats, p.unit) == (e.beats, e.unit), (a, b), follows);
            arrangement = Some(arr2);
            moved_markers = Some(match follows {
                true => closed(markers, |m| m.beat, |m, x| m.beat = x, |_, _| false, (a, b), true),
                // A marker has no first one of the song's own: those in the bars go.
                false => markers.iter().filter(|m| m.beat < a).cloned().collect(),
            });
            (bars_words(arr, a, b), format!("{} taken out", bars_words(arr, a, b)))
        }
    };
    let mut edits = vec![];
    for (lane, out) in after {
        let n = &now[&lane];
        let same = clips::lane_data(&n.kind, out.iter().map(|(c, _)| c.clone()).collect()) == clips::lane_data(&n.kind, n.was.clone());
        if !same {
            edits.push(clips::finished(lane, &n.kind, out, mint));
        }
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
    edit_since(s, g, tx_id, None)
}

/// [`edit_as`] for a gesture that goes from revision `since` (AGENT-2): refused, saying who, when a
/// change after it touched the arrangement or any lane, since the bars it names by their beats
/// may be other bars now.
pub fn edit_since(s: &mut Session, g: &Gesture, tx_id: &str, since: Option<u64>) -> Result<ClipsEdited, EditError> {
    if let Some(since) = since {
        crate::beats_since(s, since, "where the song's bars are")?;
        crate::unchanged_since(s, since, "the song's bars", |p| p.starts_with("data/"))?;
    }
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
    // Insert and delete bars write the arrangement and the markers whole: while either file doesn't
    // read, its newer text couldn't move with the bars (ARR-6).
    if matches!(g, Gesture::InsertBars { .. } | Gesture::DeleteBars { .. }) {
        let arrangement = std::fs::read_to_string(rev.project.dir.join(crate::song::FILE));
        if arrangement.is_ok_and(|text| Arrangement::read(&text).is_err()) {
            return Err(EditError::Rejected(format!(
                "{} doesn't read right now, so its tempo and time signature changes can't move with the bars: fix the file first.",
                crate::song::FILE
            )));
        }
        if matches!(read, Read::Invalid(_)) {
            return Err(EditError::Rejected(
                "layout.json doesn't read right now, so its markers can't move with the bars: fix it, or reset the layout.".into(),
            ));
        }
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
    // Bars moved: where music sits in time (AGENT-2).
    tx.beats = true;
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
