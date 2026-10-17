//! Clip gestures, by id (8b.5, WRITE-10, FMT-12): the timeline as an editor.
//!
//! A gesture is the request: what it means is decided once, here, so the app, `m2 clips` and
//! agents get the same meaning (WRITE-9). [`apply`] is pure: what a gesture makes of the lanes it
//! touches, from the accepted revision. [`edit`] makes it one Tx: a `Clips` op for each lane
//! touched (or the whole lane, when its clips can't all be named by id), `Confined { lanes }`, and
//! the planner's assertion that the lanes read what was written.
//!
//! The rules every gesture keeps:
//! - **ids:** a moved clip keeps its id, unless the lane it lands on has it. A copy, a paste, a
//!   duplicate, a split's second half and a new clip get a fresh one (FMT-11, FMT-12). Every clip
//!   of a lane a gesture writes gets an id, so `clip.anonymous` clears.
//! - **kinds:** notes, automation and audio clips never cross. Automation crosses only between
//!   lanes of one unit.
//! - **seen:** a clip named with the `sha` the window drew it with is refused when it changed
//!   since (WRITE-12).

use std::collections::{BTreeMap, BTreeSet};

use m2_ir::LaneDecl;
use m2_session::{ConflictFile, Revision, Session};
use m2_song::tx::{Confined, Expect, Op, Payload, TxPath};
use m2_song::{AudioClip, ClipId, ControlClip, DataPath, LaneAddr, LaneData, LaneKind, Note, NoteClip, canonical};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::plan;
use crate::{EditError, Edited, ready};

/// The shortest a clip can be: 1/64 of a beat.
pub const SHORTEST: f64 = 1.0 / 64.0;

/// A clip a gesture names: by its id, or, for a clip with no id of its own, by where it starts
/// and its rank among the clips of its lane that start there (counted from 0, in the timeline's
/// order). `sha` is the clip's hash as the window drew it: a clip changed since is refused.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClipRef {
    #[serde(flatten)]
    pub lane: LaneAddr,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<(f64, u32)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha: Option<String>,
}

/// A clip on the clipboard: the lane it was copied from, and the clip as the lane format writes
/// it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Copied {
    #[serde(flatten)]
    pub from: LaneAddr,
    pub clip: Value,
}

/// What a producer does to clips. Times are in beats.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum Gesture {
    /// A new clip: empty notes, or automation holding the lane's value there. One bar long unless
    /// `length` says.
    Create {
        #[serde(flatten)]
        lane: LaneAddr,
        start: f64,
        #[serde(default)]
        length: Option<f64>,
    },
    /// A drag: by `by` beats, onto lane `to` when given; a copy when `copy` (⌥-drag).
    Move {
        clips: Vec<ClipRef>,
        by: f64,
        #[serde(default)]
        to: Option<LaneAddr>,
        #[serde(default)]
        copy: bool,
    },
    /// Either edge: a new start moves the left edge, and what is inside stays where it plays.
    Resize { clip: ClipRef, start: f64, length: f64 },
    Split { clips: Vec<ClipRef>, at: f64 },
    Join { clips: Vec<ClipRef> },
    /// Copies right after the selection, or after `range` (a marquee's, bar-snapped). `grid` is
    /// what a span shorter than a bar rounds up to; none rounds nothing. `times` copies (Repeat…,
    /// Q12: 1 to 64), each right after the one before; one when not given.
    Duplicate {
        clips: Vec<ClipRef>,
        #[serde(default)]
        range: Option<(f64, f64)>,
        #[serde(default)]
        grid: Option<f64>,
        #[serde(default)]
        times: Option<u32>,
    },
    /// Clips at `at`, keeping their offsets from the earliest: on lane `to` when it fits them all,
    /// else on the lanes they came from.
    Paste {
        clips: Vec<Copied>,
        #[serde(default)]
        to: Option<LaneAddr>,
        at: f64,
    },
    Delete { clips: Vec<ClipRef> },
    /// The piano roll's edit of one clip: its notes, and its length when given.
    Notes {
        clip: ClipRef,
        notes: Vec<Note>,
        #[serde(default)]
        length: Option<f64>,
    },
    /// Automation drawn in place: one clip's points.
    Points { clip: ClipRef, points: Vec<(f64, f64)> },
    /// Note starts to the nearest line of `grid` (beats), and ends too when `ends`. `notes` picks
    /// notes of one clip by their place in it.
    Quantize {
        clips: Vec<ClipRef>,
        #[serde(default)]
        notes: Option<Vec<u32>>,
        grid: f64,
        #[serde(default)]
        ends: bool,
    },
    /// An audio clip's settings (lane A's A3): its volume (−48 to +12 dB), its fades (seconds, at
    /// most the clip together) and its direction. Those not given stay.
    Audio {
        clip: ClipRef,
        #[serde(default)]
        gain_db: Option<f64>,
        #[serde(default)]
        fade_in: Option<f64>,
        #[serde(default)]
        fade_out: Option<f64>,
        #[serde(default)]
        reverse: Option<bool>,
    },
    /// The general form, for agents: whole clips put (each replacing the clip with its id, or
    /// added) and clips deleted by id.
    Set {
        #[serde(flatten)]
        lane: LaneAddr,
        #[serde(default)]
        put: Vec<Value>,
        #[serde(default)]
        delete: Vec<String>,
    },
}

/// What a gesture makes of one lane: all its clips, in canonical order, and the ids of the clips
/// the gesture put there (made, copied, pasted, split off, or moved from another lane), for the
/// app to select.
#[derive(Clone, Debug, PartialEq)]
pub struct LaneEdit {
    pub lane: LaneAddr,
    pub data: LaneData,
    pub made: Vec<String>,
}

/// An accepted gesture, and the clips it put, by lane.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipsEdited {
    pub edited: Edited,
    pub made: Vec<(LaneAddr, String)>,
}

/// The most copies one Repeat… makes (Q12).
pub const REPEATS: u32 = 64;

/// What `g` makes of the lanes it touches, planned on `rev`: only lanes that change. `mint` gives
/// a fresh id for a clip on a lane, as often as asked; one the lane has is asked again. Pure: the
/// same revision, gesture and ids give the same edits.
pub fn apply(rev: &Revision, g: &Gesture, mint: &mut dyn FnMut(&LaneAddr) -> String) -> Result<Vec<LaneEdit>, EditError> {
    let mut w = Work { rev, lanes: BTreeMap::new() };
    let arr = &rev.project.arrangement;
    match g {
        Gesture::Create { lane, start, length } => {
            starts(*start)?;
            let length = length.unwrap_or_else(|| arr.meter_at(*start).bar());
            lasts(length)?;
            let l = w.lane(lane)?;
            let clip = match &l.kind {
                LaneKind::Notes => Clip::Notes(NoteClip { id: None, other_id: None, start: *start, length, notes: vec![] }),
                LaneKind::Control { .. } => {
                    let v = l.value_at(*start);
                    Clip::Control(ControlClip { id: None, other_id: None, start: *start, length, points: vec![(0.0, v), (length, v)] })
                }
                LaneKind::Audio => {
                    return Err(EditError::Rejected(
                        "An audio clip plays a sound file. Drop a WAV file here, or use File ▸ Import Audio…".into(),
                    ));
                }
            };
            l.added.push((clip, true));
        }
        Gesture::Move { clips, by, to, copy } => {
            finite("the distance", *by)?;
            let refs = w.resolve_all(clips)?;
            if let Some(t) = to {
                w.lane(t)?;
            }
            for (from, i) in refs {
                let mut c = w.lanes[&from].was[i].clone();
                starts(c.start() + by)?;
                c.set_start(c.start() + by);
                let target = to.clone().unwrap_or_else(|| from.clone());
                if target != from {
                    w.fits(&from, &target)?;
                }
                match (*copy, target == from) {
                    (true, _) => {
                        c.set_id(None);
                        w.lanes.get_mut(&target).expect("loaded").added.push((c, true));
                    }
                    (false, true) => w.lanes.get_mut(&from).expect("loaded").keep[i] = Some(c),
                    (false, false) => {
                        w.lanes.get_mut(&from).expect("loaded").keep[i] = None;
                        w.lanes.get_mut(&target).expect("loaded").added.push((c, true));
                    }
                }
            }
        }
        Gesture::Resize { clip, start, length } => {
            starts(*start)?;
            lasts(*length)?;
            let (lane, i) = w.resolve(clip)?;
            let l = w.lanes.get_mut(&lane).expect("resolved");
            let mut c = l.was[i].clone();
            if let Clip::Audio(a) = &mut c {
                let file = rev.compiled.data.asset_path(&a.asset).and_then(|p| asset_seconds(&p));
                audio_span(a, *start, *length, rev.compiled.data.tempo(), file)?;
            } else {
                if *start != c.start() {
                    c.move_left_edge(*start, rev.compiled.data.tempo())?;
                }
                c.set_length(*length);
            }
            l.keep[i] = Some(c);
        }
        Gesture::Delete { clips } => {
            for (lane, i) in w.resolve_all(clips)? {
                w.lanes.get_mut(&lane).expect("resolved").keep[i] = None;
            }
        }
        Gesture::Duplicate { clips, range, grid, times } => {
            let times = times.unwrap_or(1);
            if !(1..=REPEATS).contains(&times) {
                return Err(EditError::Rejected(format!("Repeat 1 to {REPEATS} times")));
            }
            let refs = w.resolve_all(clips)?;
            let each = || refs.iter().map(|(l, i)| &w.lanes[l].was[*i]);
            let (lo, hi) = match range {
                Some(r) => *r,
                None => (each().map(Clip::start).fold(f64::INFINITY, f64::min), each().map(Clip::end).fold(f64::NEG_INFINITY, f64::max)),
            };
            if !(lo.is_finite() && hi > lo) {
                return Err(EditError::Rejected("Pick the clips, or the bars, to duplicate.".into()));
            }
            let shift = rounded_up(hi - lo, arr.meter_at(lo).bar(), *grid);
            for k in 1..=times {
                for (lane, i) in &refs {
                    let l = w.lanes.get_mut(lane).expect("resolved");
                    let mut c = l.was[*i].clone();
                    c.set_start(c.start() + shift * k as f64);
                    c.set_id(None);
                    l.added.push((c, true));
                }
            }
        }
        Gesture::Paste { clips, to, at } => {
            starts(*at)?;
            let mut kinds = vec![];
            let mut earliest = f64::INFINITY;
            for c in clips {
                kinds.push(w.copied_kind(c)?);
                let start = c.clip.get("start").and_then(Value::as_f64);
                earliest = earliest.min(start.ok_or_else(|| EditError::Rejected("A copied clip says where it starts.".into()))?);
            }
            let Some(first) = kinds.first() else { return Err(EditError::Rejected("Nothing to paste.".into())) };
            // Onto the lane named when the clips are of one kind; else each onto its own lane.
            let one = kinds.iter().all(|k| std::mem::discriminant(k) == std::mem::discriminant(first));
            for (c, kind) in clips.iter().zip(&kinds) {
                let target = to.clone().filter(|_| one).unwrap_or_else(|| c.from.clone());
                let into = w.lane(&target)?.kind.clone();
                fits(kind, &into).map_err(|why| EditError::Rejected(why.replace("{from}", &w.named(&c.from)).replace("{to}", &w.named(&target))))?;
                let mut clip = parse(&c.clip, &into)?;
                clip.set_start(at + (clip.start() - earliest));
                clip.set_id(None);
                w.lanes.get_mut(&target).expect("loaded").added.push((clip, true));
            }
        }
        Gesture::Audio { clip, gain_db, fade_in, fade_out, reverse } => {
            let (lane, i) = w.resolve(clip)?;
            let l = w.lanes.get_mut(&lane).expect("resolved");
            let Clip::Audio(mut c) = l.was[i].clone() else {
                return Err(EditError::Rejected("Only an audio clip has a volume, fades or a direction.".into()));
            };
            if let Some(g) = gain_db {
                if !(g.is_finite() && (GAIN.0..=GAIN.1).contains(g)) {
                    return Err(EditError::Rejected("A clip's volume goes from −48 to +12 dB.".into()));
                }
                c.gain_db = *g;
            }
            for (to, given) in [(&mut c.fade_in, fade_in), (&mut c.fade_out, fade_out)] {
                if let Some(f) = given {
                    if !(f.is_finite() && *f >= 0.0) {
                        return Err(EditError::Rejected("A fade is a number of seconds, 0 or more.".into()));
                    }
                    *to = *f;
                }
            }
            let tempo = rev.compiled.data.tempo();
            let seconds = tempo.seconds_at(c.start + c.length) - tempo.seconds_at(c.start);
            if c.fade_in + c.fade_out > seconds + SLACK {
                return Err(EditError::Rejected(format!("The fades together are at most the clip: {} seconds.", (seconds * 100.0).round() / 100.0)));
            }
            if let Some(r) = reverse {
                c.reverse = *r;
            }
            l.keep[i] = Some(Clip::Audio(c));
        }
        Gesture::Set { lane, put, delete } => {
            let l = w.lane(lane)?;
            for (i, c) in l.was.iter().enumerate() {
                if c.id().is_some_and(|id| delete.iter().any(|d| d == id)) {
                    l.keep[i] = None;
                }
            }
            for v in put {
                let c = parse(v, &l.kind)?;
                let same: Vec<usize> = (0..l.was.len()).filter(|i| c.id().is_some() && l.was[*i].id() == c.id()).collect();
                match same.split_first() {
                    Some((first, rest)) => {
                        l.keep[*first] = Some(c);
                        rest.iter().for_each(|i| l.keep[*i] = None);
                    }
                    None => l.added.push((c, true)),
                }
            }
        }
        Gesture::Split { clips, at } => {
            finite("the split", *at)?;
            let mut split = false;
            for (lane, i) in w.resolve_all(clips)? {
                let l = w.lanes.get_mut(&lane).expect("resolved");
                let c = &l.was[i];
                if !(c.start() < *at && *at < c.end()) {
                    continue;
                }
                let (first, second) = c.split(*at, rev.compiled.data.tempo())?;
                l.keep[i] = Some(first);
                l.added.push((second, true));
                split = true;
            }
            if !split {
                return Err(EditError::Rejected(format!("No clip picked crosses {}, so there is nothing to split.", crate::notes::place(arr, *at))));
            }
        }
        Gesture::Join { clips } => {
            let refs = w.resolve_all(clips)?;
            let lane = &refs[0].0;
            if refs.iter().any(|(l, _)| l != lane) {
                return Err(EditError::Rejected("Clips join only within one lane.".into()));
            }
            if refs.len() < 2 {
                return Err(EditError::Rejected("Pick two clips or more to join.".into()));
            }
            let l = w.lanes.get_mut(lane).expect("resolved");
            let mut picked: Vec<usize> = refs.iter().map(|(_, i)| *i).collect();
            // The earliest keeps its id: by start, then the lane's order.
            picked.sort_by(|a, b| l.was[*a].start().total_cmp(&l.was[*b].start()).then(a.cmp(b)));
            let notes: Vec<&NoteClip> = picked
                .iter()
                .map(|i| match &l.was[*i] {
                    Clip::Notes(c) => Ok(c),
                    Clip::Control(_) => Err("Automation clips can't be joined: the lane's fallback would stop playing in their gaps."),
                    Clip::Audio(_) => Err("Audio clips can't be joined yet: that needs a render of them as one sound."),
                })
                .collect::<Result<_, _>>()
                .map_err(|why| EditError::Rejected(why.into()))?;
            let joined = joined(&notes);
            for i in &picked[1..] {
                l.keep[*i] = None;
            }
            l.keep[picked[0]] = Some(Clip::Notes(joined));
        }
        Gesture::Notes { clip, notes, length } => {
            notes_fit(notes)?;
            if let Some(length) = length {
                lasts(*length)?;
            }
            let (lane, i) = w.resolve(clip)?;
            let l = w.lanes.get_mut(&lane).expect("resolved");
            let Clip::Notes(mut c) = l.was[i].clone() else {
                return Err(EditError::Rejected("Only a notes clip has notes to edit.".into()));
            };
            c.notes = notes.clone();
            c.length = length.unwrap_or(c.length);
            l.keep[i] = Some(Clip::Notes(c));
        }
        Gesture::Points { clip, points } => {
            if points.iter().any(|(b, v)| !b.is_finite() || !v.is_finite()) {
                return Err(EditError::Rejected("Each point is a beat and a value, both numbers.".into()));
            }
            let (lane, i) = w.resolve(clip)?;
            let l = w.lanes.get_mut(&lane).expect("resolved");
            let Clip::Control(mut c) = l.was[i].clone() else {
                return Err(EditError::Rejected("Only an automation clip has points to draw.".into()));
            };
            c.points = points.clone();
            // Stable: two points at one beat are a step, in the order drawn.
            c.points.sort_by(|a, b| a.0.total_cmp(&b.0));
            l.keep[i] = Some(Clip::Control(c));
        }
        Gesture::Quantize { clips, notes, grid, ends } => {
            if !(grid.is_finite() && *grid > 0.0) {
                return Err(EditError::Rejected("The grid is a number of beats above 0, such as 0.25 for sixteenths.".into()));
            }
            let refs = w.resolve_all(clips)?;
            if notes.is_some() && refs.len() > 1 {
                return Err(EditError::Rejected("Pick notes in one clip, or whole clips.".into()));
            }
            for (lane, i) in refs {
                let l = w.lanes.get_mut(&lane).expect("resolved");
                let Clip::Notes(mut c) = l.was[i].clone() else {
                    return Err(EditError::Rejected("Only notes can be quantized.".into()));
                };
                let picked: Vec<usize> = match notes {
                    Some(n) => n.iter().map(|&k| k as usize).collect(),
                    None => (0..c.notes.len()).collect(),
                };
                if let Some(k) = picked.iter().find(|k| **k >= c.notes.len()) {
                    return Err(EditError::Missing(format!("The clip has {} notes, so it has no note {}.", c.notes.len(), k + 1)));
                }
                for k in picked {
                    quantized(&mut c.notes[k], c.start, *grid, *ends);
                }
                l.keep[i] = Some(Clip::Notes(c));
            }
        }
    }
    Ok(w.finish(mint))
}

/// Notes as the lane format holds them: each a pitch, a start, a length above 0 and a velocity of 0
/// or more.
fn notes_fit(notes: &[Note]) -> Result<(), EditError> {
    match notes.iter().find(|n| ![n.pitch, n.start, n.length, n.velocity].iter().all(|x| x.is_finite()) || n.length <= 0.0 || n.velocity < 0.0) {
        Some(n) => Err(EditError::Rejected(format!("A note needs a pitch, a start, a length above 0 and a velocity of 0 or more, not {n:?}."))),
        None => Ok(()),
    }
}

/// `x` on the grid of `grid` beats from the song's start, as a place in a clip that starts at
/// `start`: exact multiples when the clip starts on the grid.
fn on_grid(x: f64, start: f64, grid: f64) -> f64 {
    let line = ((start + x) / grid).round();
    let k = (start / grid).round();
    match (k * grid - start).abs() < 1e-9 {
        true => (line - k) * grid,
        false => line * grid - start,
    }
}

/// Note `n` of a clip starting at `start`, its start (and end, with `ends`) on the nearest line of
/// `grid`. An end that would meet its start lands a line later.
fn quantized(n: &mut Note, start: f64, grid: f64, ends: bool) {
    let end = n.start + n.length;
    n.start = on_grid(n.start, start, grid);
    if ends {
        let mut e = on_grid(end, start, grid);
        if e <= n.start {
            e = n.start + grid;
        }
        n.length = e - n.start;
    }
}

/// Notes clips of one lane as one clip, from the earliest start to the latest end, with the first's
/// id (it is the earliest). Every note keeps its place in time. A note whose sound would change
/// (part of it hidden by its clip's edge, now inside) is cut to what played, and dropped when
/// nothing of it played; a note that played whole, or stays as hidden as it was, is kept whole.
fn joined(clips: &[&NoteClip]) -> NoteClip {
    let start = clips.iter().map(|c| c.start).fold(f64::INFINITY, f64::min);
    let end = clips.iter().map(|c| c.start + c.length).fold(f64::NEG_INFINITY, f64::max);
    let mut notes = vec![];
    for c in clips {
        for n in &c.notes {
            let (a, z) = (c.start + n.start, c.start + n.start + n.length);
            // What played, and what would play in the joined clip, in song time.
            let was = (a.max(c.start), z.min(c.start + c.length));
            let now = (a.max(start), z.min(end));
            let heard = |(x, y): (f64, f64)| (y > x).then_some((x, y));
            match (heard(was), heard(now)) {
                (w, n2) if w == n2 => notes.push(Note { start: a - start, ..n.clone() }),
                (Some((x, y)), _) => notes.push(Note { start: x - start, length: y - x, ..n.clone() }),
                (None, _) => {}
            }
        }
    }
    NoteClip { id: clips[0].id.clone(), other_id: None, start, length: end - start, notes }
}

/// A clip of any kind, as the lane format holds it.
#[derive(Clone, Debug, PartialEq)]
enum Clip {
    Notes(NoteClip),
    Control(ControlClip),
    Audio(AudioClip),
}

impl Clip {
    fn start(&self) -> f64 {
        match self {
            Clip::Notes(c) => c.start,
            Clip::Control(c) => c.start,
            Clip::Audio(c) => c.start,
        }
    }

    fn length(&self) -> f64 {
        match self {
            Clip::Notes(c) => c.length,
            Clip::Control(c) => c.length,
            Clip::Audio(c) => c.length,
        }
    }

    fn end(&self) -> f64 {
        self.start() + self.length()
    }

    fn id(&self) -> Option<&str> {
        match self {
            Clip::Notes(c) => c.id.as_deref(),
            Clip::Control(c) => c.id.as_deref(),
            Clip::Audio(c) => c.id.as_deref(),
        }
    }

    fn set_start(&mut self, x: f64) {
        match self {
            Clip::Notes(c) => c.start = x,
            Clip::Control(c) => c.start = x,
            Clip::Audio(c) => c.start = x,
        }
    }

    fn set_length(&mut self, x: f64) {
        match self {
            Clip::Notes(c) => c.length = x,
            Clip::Control(c) => c.length = x,
            Clip::Audio(c) => c.length = x,
        }
    }

    /// The id, in place of any it had (an id written as anything but a string too).
    fn set_id(&mut self, id: Option<String>) {
        let (slot, other) = match self {
            Clip::Notes(c) => (&mut c.id, &mut c.other_id),
            Clip::Control(c) => (&mut c.id, &mut c.other_id),
            Clip::Audio(c) => (&mut c.id, &mut c.other_id),
        };
        (*slot, *other) = (id, None);
    }

    /// Whether its id names it: valid, and written as a string (FMT-12).
    fn named(&self) -> Option<&str> {
        let other = match self {
            Clip::Notes(c) => c.other_id.is_some(),
            Clip::Control(c) => c.other_id.is_some(),
            Clip::Audio(c) => c.other_id.is_some(),
        };
        self.id().filter(|id| !other && ClipId::valid(id))
    }

    /// Move the left edge to `start`, keeping what is inside where it plays: notes and points
    /// shift the other way; an audio clip starts later or earlier in its asset, by the seconds the
    /// edge moved under `tempo`, and never before the asset's start.
    fn move_left_edge(&mut self, start: f64, tempo: &m2_song::TempoMap) -> Result<(), EditError> {
        let by = self.start() - start;
        match self {
            Clip::Notes(c) => c.notes.iter_mut().for_each(|n| n.start += by),
            Clip::Control(c) => c.points.iter_mut().for_each(|p| p.0 += by),
            Clip::Audio(c) => {
                audio_edge(c, "its left edge")?;
                if c.warp.is_empty() {
                    let offset = c.offset + tempo.seconds_at(start) - tempo.seconds_at(c.start);
                    if offset < -1e-9 {
                        return Err(EditError::Rejected("The clip starts at the start of its audio already.".into()));
                    }
                    c.offset = offset.max(0.0);
                } else {
                    // Warp markers place source moments on beats from the clip's start.
                    c.warp.iter_mut().for_each(|m| m.1 += by);
                }
            }
        }
        self.set_start(start);
        Ok(())
    }

    /// The clip cut at song beat `at`, inside it: the first part keeps the id and ends there; the
    /// second, with no id, starts there. Notes that cross are shortened to end at the cut (as
    /// Live does); a note at or after it goes to the second part. Automation takes the value at
    /// the cut as the first part's last point and the second's first, so the curve plays on. An
    /// audio clip's second part starts later in its asset; the first keeps the fade in, the
    /// second the fade out.
    fn split(&self, at: f64, tempo: &m2_song::TempoMap) -> Result<(Clip, Clip), EditError> {
        let cut = at - self.start();
        let (mut first, mut second) = (self.clone(), self.clone());
        first.set_length(cut);
        second.set_start(at);
        second.set_length(self.length() - cut);
        second.set_id(None);
        match (&mut first, &mut second) {
            (Clip::Notes(a), Clip::Notes(b)) => {
                a.notes.retain(|n| n.start < cut);
                for n in &mut a.notes {
                    n.length = n.length.min(cut - n.start);
                }
                b.notes.retain(|n| n.start >= cut);
                b.notes.iter_mut().for_each(|n| n.start -= cut);
            }
            (Clip::Control(a), Clip::Control(b)) => {
                let (left, right) = (value_before(&a.points, cut), value_at(&a.points, cut));
                let before: Vec<(f64, f64)> = a.points.iter().copied().filter(|p| p.0 < cut).collect();
                let after: Vec<(f64, f64)> = a.points.iter().filter(|p| p.0 > cut).map(|p| (p.0 - cut, p.1)).collect();
                if let (Some(l), Some(r)) = (left, right) {
                    a.points = before.into_iter().chain([(cut, l)]).collect();
                    b.points = [(0.0, r)].into_iter().chain(after).collect();
                }
            }
            (Clip::Audio(a), Clip::Audio(b)) => {
                audio_edge(a, "split")?;
                if a.warp.is_empty() && a.reverse {
                    // Backwards, the first part plays the later part of the span: it ends at the
                    // cut, so it starts later in the asset; the second keeps the offset (A3).
                    a.offset += tempo.seconds_at(self.end()) - tempo.seconds_at(at);
                } else if a.warp.is_empty() {
                    b.offset = a.offset + tempo.seconds_at(at) - tempo.seconds_at(a.start);
                } else {
                    b.warp.iter_mut().for_each(|m| m.1 -= cut);
                }
                a.fade_out = 0.0;
                b.fade_in = 0.0;
            }
            _ => unreachable!("both parts are this clip"),
        }
        Ok((first, second))
    }

    /// Its line in the canonical form, as `kind` writes it.
    fn line(&self, kind: &LaneKind) -> String {
        lane_data(kind, vec![self.clone()]).clip_lines().remove(0).1
    }
}

/// What automation `points` play at `t`, beats from the clip's start, as a control lane reads
/// them: linear between points, held flat before the first and after the last; of points at one
/// beat, the last. None with no points (the lane's fallback plays).
fn value_at(points: &[(f64, f64)], t: f64) -> Option<f64> {
    let last = points.last()?;
    Some(match points.iter().position(|(b, _)| *b > t) {
        None => last.1,
        Some(0) => points[0].1,
        Some(i) => {
            let ((b0, v0), (b1, v1)) = (points[i - 1], points[i]);
            if b1 > b0 { v0 + (v1 - v0) * (t - b0) / (b1 - b0) } else { v1 }
        }
    })
}

/// What `points` play just before `t`: where a step is at `t`, the value it steps from.
fn value_before(points: &[(f64, f64)], t: f64) -> Option<f64> {
    let first_at = points.iter().position(|(b, _)| *b >= t);
    match first_at {
        Some(i) if i > 0 && points[i].0 == t => {
            let ((b0, v0), (b1, v1)) = (points[i - 1], points[i]);
            Some(if b1 > b0 { v0 + (v1 - v0) * (t - b0) / (b1 - b0) } else { v1 })
        }
        Some(0) => points.first().map(|p| p.1),
        _ => value_at(points, t),
    }
}

/// An audio clip whose `what` moves inside it: refused for a pitched clip, whose audio isn't read
/// at its own speed from its offset (A8), and a reversed one with warp markers. A reversed clip's
/// edges and splits mirror the offset (A3).
fn audio_edge(c: &AudioClip, what: &str) -> Result<(), EditError> {
    let why = match () {
        _ if c.pitch != 0.0 => "its pitch is changed",
        _ if c.reverse && !c.warp.is_empty() => "it is reversed and warped",
        _ => return Ok(()),
    };
    Err(EditError::Rejected(format!("The audio clip's {what} can't move here: {why}. Change that in its lane file.")))
}

/// How far a clip's audio may reach past its file and still be taken: 1 ms, for the page's beats.
const SLACK: f64 = 0.001;
/// A clip's volume, in dB (Q11).
pub const GAIN: (f64, f64) = (-48.0, 12.0);

/// A WAV file's length in seconds, from its header alone (`hound`, no decode): what an audio clip's
/// edges stop at (Q13).
pub fn asset_seconds(path: &std::path::Path) -> Option<f64> {
    let r = hound::WavReader::open(path).ok()?;
    Some(r.duration() as f64 / r.spec().sample_rate as f64)
}

/// Give audio clip `c` the span `start` for `length` beats, what plays inside staying where it
/// plays (A3, D3): forwards, the left edge moves the offset and the right edge doesn't; backwards
/// (a reversed clip plays `offset..offset + seconds` from its end), the right edge moves it and the
/// left doesn't. The audio stops at its file: never before its start, nor past its end (`file`
/// seconds, from the header), with 1 ms of slack. Warp markers move with the left edge, as before.
fn audio_span(c: &mut AudioClip, start: f64, length: f64, tempo: &m2_song::TempoMap, file: Option<f64>) -> Result<(), EditError> {
    let (left, right) = (start != c.start, start + length != c.start + c.length);
    if !(left || right) {
        return Ok(());
    }
    if !c.warp.is_empty() {
        if c.reverse {
            audio_edge(c, "edge")?;
        }
        if left {
            let by = c.start - start;
            c.warp.iter_mut().for_each(|m| m.1 += by);
        }
        (c.start, c.length) = (start, length);
        return Ok(());
    }
    // The edge that moves the offset (forwards the left, backwards the right) is refused for a
    // pitched clip, as before.
    let moves = left && !c.reverse || right && c.reverse;
    if moves {
        audio_edge(c, if c.reverse { "right edge" } else { "left edge" })?;
    }
    let secs = |b: f64| tempo.seconds_at(b);
    let offset = match c.reverse {
        true => c.offset + secs(c.start + c.length) - secs(start + length),
        false => c.offset + secs(start) - secs(c.start),
    };
    let rate = 2f64.powf(c.pitch / 12.0);
    let reach = offset + (secs(start + length) - secs(start)) * rate;
    let name = &c.asset;
    // Only the edge that moves a limit is held to it: a clip already past its file's end keeps
    // its other edge free.
    let reaches = right && !c.reverse || left && c.reverse;
    if moves && offset < -SLACK {
        return Err(EditError::Rejected(match c.reverse {
            false => "The clip starts at the start of its audio already.".into(),
            true => format!("The clip ends where {name} starts: it plays backwards."),
        }));
    }
    if let Some(f) = file
        && reaches
        && reach > f + SLACK
    {
        return Err(EditError::Rejected(match c.reverse {
            false => format!("The clip ends where {name} ends."),
            true => format!("The clip starts where {name} ends: it plays backwards."),
        }));
    }
    // Unchanged where the edge that moves it stayed, to the bit.
    if moves {
        c.offset = offset.max(0.0);
    }
    (c.start, c.length) = (start, length);
    Ok(())
}

/// A lane's clips, one by one.
fn clips_of(d: &LaneData) -> Vec<Clip> {
    match d {
        LaneData::Notes(c) => c.iter().cloned().map(Clip::Notes).collect(),
        LaneData::Control { clips, .. } => clips.iter().cloned().map(Clip::Control).collect(),
        LaneData::Audio(c) => c.iter().cloned().map(Clip::Audio).collect(),
    }
}

/// A lane of `kind` holding `clips`, in canonical order. Clips are of the lane's kind.
fn lane_data(kind: &LaneKind, clips: Vec<Clip>) -> LaneData {
    let mut d = LaneData::empty(kind);
    for c in clips {
        match (&mut d, c) {
            (LaneData::Notes(v), Clip::Notes(c)) => v.push(c),
            (LaneData::Control { clips, .. }, Clip::Control(c)) => clips.push(c),
            (LaneData::Audio(v), Clip::Audio(c)) => v.push(c),
            _ => unreachable!("kinds are checked before a clip lands on a lane"),
        }
    }
    d.sort();
    d
}

/// A clip as the lane format writes it, read as a clip of a lane of `kind`.
fn parse(v: &Value, kind: &LaneKind) -> Result<Clip, EditError> {
    let mut top = serde_json::Map::new();
    if let LaneKind::Control { unit: Some(u) } = kind {
        top.insert("unit".into(), Value::String(u.clone()));
    }
    top.insert("clips".into(), Value::Array(vec![v.clone()]));
    match LaneData::read(&Value::Object(top).to_string(), kind) {
        Ok(d) => Ok(clips_of(&d).remove(0)),
        Err(m2_song::LaneError::Invalid(why)) => Err(EditError::Rejected(format!("The clip doesn't read: {why}"))),
        Err(m2_song::LaneError::Unit { declared, .. }) => Err(EditError::Rejected(format!("The clip must be automation in {declared}."))),
    }
}

/// Whether a clip of a lane of kind `from` may go on a lane of kind `to`: why not, with `{from}`
/// and `{to}` for the lanes' names.
fn fits(from: &LaneKind, to: &LaneKind) -> Result<(), String> {
    let noun = |k: &LaneKind| match k {
        LaneKind::Notes => ("A notes clip", "a notes lane"),
        LaneKind::Control { .. } => ("An automation clip", "an automation lane"),
        LaneKind::Audio => ("An audio clip", "an audio lane"),
    };
    match (from, to) {
        (LaneKind::Notes, LaneKind::Notes) | (LaneKind::Audio, LaneKind::Audio) => Ok(()),
        (LaneKind::Control { unit: Some(a) }, LaneKind::Control { unit: Some(b) }) if a != b => {
            Err(format!("{{from}} ({a}) and {{to}} ({b}) can't share clips: their units differ"))
        }
        (LaneKind::Control { .. }, LaneKind::Control { .. }) => Ok(()),
        _ => Err(format!("{} can't go on {}", noun(from).0, noun(to).1)),
    }
}

/// A lane a gesture touches: its accepted clips, each as it ends up (None: gone), and the clips
/// added to it, each with whether the gesture put it there (to select).
struct Lane {
    kind: LaneKind,
    /// The track, and the lane, as the app names them.
    track: String,
    label: String,
    default: f64,
    was: Vec<Clip>,
    keep: Vec<Option<Clip>>,
    added: Vec<(Clip, bool)>,
}

impl Lane {
    /// What the lane plays at `beat`: its clip there, else its fallback.
    fn value_at(&self, beat: f64) -> f64 {
        let clips = self.was.iter().filter_map(|c| if let Clip::Control(c) = c { Some(c.clone()) } else { None }).collect();
        m2_song::lane::ControlLane::new(clips).value_at(beat, self.default)
    }
}

/// The lanes a gesture touches, read from the accepted revision as it touches them.
struct Work<'a> {
    rev: &'a Revision,
    lanes: BTreeMap<LaneAddr, Lane>,
}

impl Work<'_> {
    /// Lane `addr`, read on first touch: refused when the code doesn't declare it, or its file
    /// doesn't read, since it then plays the clips it had before (FMT-9), and writing those would
    /// replace the file's newer text.
    fn lane(&mut self, addr: &LaneAddr) -> Result<&mut Lane, EditError> {
        if !self.lanes.contains_key(addr) {
            let rev = self.rev;
            let (module, key) = (addr.module.as_str(), addr.lane.as_str());
            let m = rev.compiled.ir.modules.iter().find(|m| m.key == module);
            let m = m.ok_or_else(|| EditError::Missing(format!("There's no track `{module}`.")))?;
            let decl: &LaneDecl = m.lanes.iter().find(|l| l.key == key).ok_or_else(|| EditError::Missing(format!("{} has no lane `{key}`.", m.label())))?;
            let shown = DataPath::lane(addr).to_string();
            if let Some(d) = rev.compiled.problems.iter().find(|d| d.src.as_deref() == Some(&shown) && d.code.starts_with("lane.")) {
                return Err(EditError::Rejected(format!(
                    "{shown} doesn't read as the lane right now, so the clips shown are the ones it played before; fix the file first ({})",
                    d.code
                )));
            }
            let kind = m2_runtime::lane_kind(decl.kind);
            let was = match rev.compiled.data.lane(module, key) {
                Some(d) if d.fits(&kind) => clips_of(d),
                _ => vec![],
            };
            let lane = Lane {
                kind,
                track: m.label().to_string(),
                label: decl.label().to_string(),
                default: decl.default.unwrap_or(0.0),
                keep: was.iter().cloned().map(Some).collect(),
                was,
                added: vec![],
            };
            self.lanes.insert(addr.clone(), lane);
        }
        Ok(self.lanes.get_mut(addr).expect("read"))
    }

    /// `Bass's notes lane`.
    fn named(&self, addr: &LaneAddr) -> String {
        match self.lanes.get(addr) {
            Some(l) => format!("{}'s {} lane", l.track, l.label),
            None => format!("{}'s {} lane", addr.module, addr.lane),
        }
    }

    /// The clip `r` names, as its index among its lane's accepted clips. A clip the window saw
    /// (its `sha`) that changed since, or is gone, is `Changed` (WRITE-12).
    fn resolve(&mut self, r: &ClipRef) -> Result<(LaneAddr, usize), EditError> {
        let rev = self.rev;
        let arr = &rev.project.arrangement;
        self.lane(&r.lane)?;
        let named = self.named(&r.lane);
        let l = &self.lanes[&r.lane];
        let found = match (r.at, &r.id) {
            (Some((start, rank)), id) => {
                let mut there = l.was.iter().enumerate().filter(|(_, c)| c.start() == start);
                let at = there.nth(rank as usize).map(|(i, _)| i).filter(|i| id.is_none() || l.was[*i].id() == id.as_deref());
                at.ok_or_else(|| format!("No clip starts at {} on {named} now.", crate::notes::place(arr, start)))
            }
            (None, Some(id)) => match l.was.iter().enumerate().filter(|(_, c)| c.id() == Some(id)).map(|(i, _)| i).collect::<Vec<_>>()[..] {
                [i] => Ok(i),
                [] => Err(format!("{named} has no clip `{id}` now.")),
                _ => {
                    return Err(EditError::Rejected(format!(
                        "Two clips on {named} have the id `{id}`; name the clip by where it starts."
                    )));
                }
            },
            (None, None) => return Err(EditError::Rejected("Name the clip by its id, or by where it starts.".into())),
        };
        let changed = |why: String, clip: Option<String>| {
            let file = DataPath::lane(&r.lane);
            let current = rev.snapshot.sha(&file.path());
            EditError::Changed(vec![ConflictFile { file: file.to_string(), current, clip, why: Some(why) }])
        };
        let i = match found {
            Ok(i) => i,
            Err(why) if r.sha.is_some() => return Err(changed(format!("{why} It was changed in another window or by an agent; try again."), r.id.clone())),
            Err(why) => return Err(EditError::Missing(why)),
        };
        let c = &l.was[i];
        if let Some(seen) = &r.sha
            && *seen != sha(&c.line(&l.kind))
        {
            let what = format!("the {} clip at bar {}", l.track, arr.bar_at(c.start()).0 + 1);
            let why = format!("is {what}, which was changed in another window or by an agent. It shows as it is now; try again");
            return Err(changed(why, c.id().map(str::to_string)));
        }
        Ok((r.lane.clone(), i))
    }

    /// Each clip `refs` name, once, in the order named.
    fn resolve_all(&mut self, refs: &[ClipRef]) -> Result<Vec<(LaneAddr, usize)>, EditError> {
        let mut out: Vec<(LaneAddr, usize)> = vec![];
        for r in refs {
            let at = self.resolve(r)?;
            if !out.contains(&at) {
                out.push(at);
            }
        }
        match out.is_empty() {
            true => Err(EditError::Rejected("Pick a clip first.".into())),
            false => Ok(out),
        }
    }

    /// Whether a clip of lane `from` may go on lane `to`, in words.
    fn fits(&self, from: &LaneAddr, to: &LaneAddr) -> Result<(), EditError> {
        fits(&self.lanes[from].kind, &self.lanes[to].kind)
            .map_err(|why| EditError::Rejected(why.replace("{from}", &self.named(from)).replace("{to}", &self.named(to))))
    }

    /// The kind of lane a copied clip came from: by its shape, and for automation, its lane's unit
    /// when that lane is still declared.
    fn copied_kind(&mut self, c: &Copied) -> Result<LaneKind, EditError> {
        let has = |k: &str| c.clip.get(k).is_some();
        Ok(match () {
            _ if has("notes") => LaneKind::Notes,
            _ if has("asset") => LaneKind::Audio,
            _ if has("points") => match self.lane(&c.from) {
                Ok(l) => l.kind.clone(),
                Err(_) => LaneKind::Control { unit: None },
            },
            _ => return Err(EditError::Rejected("A copied clip holds notes, points or audio.".into())),
        })
    }

    /// The lanes that change, each with every clip named by an id: a clip with none, one that
    /// breaks the id rule, or one whose id an earlier clip of the lane has, gets a fresh one.
    fn finish(self, mint: &mut dyn FnMut(&LaneAddr) -> String) -> Vec<LaneEdit> {
        let mut edits = vec![];
        for (addr, l) in self.lanes {
            let same = l.added.is_empty() && l.keep.iter().zip(&l.was).all(|(k, w)| k.as_ref() == Some(w));
            if same {
                continue;
            }
            let mut out: Vec<(Clip, bool)> = l.keep.into_iter().flatten().map(|c| (c, false)).chain(l.added).collect();
            let mut taken = BTreeSet::new();
            let need: Vec<usize> = (0..out.len()).filter(|i| !out[*i].0.named().is_some_and(|id| taken.insert(id.to_string()))).collect();
            for i in need {
                let id = loop {
                    let id = mint(&addr);
                    if ClipId::valid(&id) && taken.insert(id.clone()) {
                        break id;
                    }
                };
                out[i].0.set_id(Some(id));
            }
            let made = out.iter().filter(|(_, m)| *m).filter_map(|(c, _)| c.id().map(str::to_string)).collect();
            edits.push(LaneEdit { data: lane_data(&l.kind, out.into_iter().map(|(c, _)| c).collect()), lane: addr, made });
        }
        edits
    }
}

/// A clip's start: a number, and never before the song starts.
fn starts(x: f64) -> Result<(), EditError> {
    finite("a clip's start", x)?;
    match x >= 0.0 {
        true => Ok(()),
        false => Err(EditError::Rejected("A clip can't start before the song starts.".into())),
    }
}

/// A clip's length: at least 1/64 of a beat.
fn lasts(x: f64) -> Result<(), EditError> {
    finite("a clip's length", x)?;
    match x >= SHORTEST {
        true => Ok(()),
        false => Err(EditError::Rejected("A clip lasts at least 1/64 of a beat.".into())),
    }
}

fn finite(what: &str, x: f64) -> Result<(), EditError> {
    match x.is_finite() {
        true => Ok(()),
        false => Err(EditError::Rejected(format!("{what} must be a number of beats"))),
    }
}

/// How far a duplicate goes: a span of a bar or more, up to whole bars; a shorter one, up to the
/// grid; with no grid (snap off), the span exactly.
fn rounded_up(span: f64, bar: f64, grid: Option<f64>) -> f64 {
    const EPS: f64 = 1e-9;
    match grid.filter(|g| g.is_finite() && *g > 0.0) {
        _ if span >= bar - EPS => (span / bar - EPS).ceil() * bar,
        Some(g) => (span / g - EPS).ceil() * g,
        None => span,
    }
}

/// Make `g` one Tx on the session: a `Clips` op per lane touched (the whole lane when its clips
/// can't all be named by id), `Expect::Confined { lanes }`, and the assertion that each lane reads
/// every clip written as written and none deleted.
pub fn edit(s: &mut Session, g: &Gesture) -> Result<ClipsEdited, EditError> {
    edit_as(s, g, &m2_song::tx::random_id())
}

/// [`apply`] as [`edit_as`] with Tx id `tx_id` applies it: the ids it mints are the ones that Tx
/// writes. A lane lease previews with it, so a copied clip has the same id in the preview and in
/// the commit, and a note sounding in it isn't cut at release (WRITE-22a, RENDER-26a).
pub fn apply_as(rev: &Revision, g: &Gesture, tx_id: &str) -> Result<Vec<LaneEdit>, EditError> {
    let mut n = 0;
    let mut mint = |lane: &LaneAddr| {
        n += 1;
        ClipId::derive(tx_id, lane, n - 1)
    };
    apply(rev, g, &mut mint)
}

/// `g` with each clip it names pinned to its hash in `rev`, where the gesture carries none: what a
/// lane lease saw, so an outside change to a clip it drags ends the lease (WRITE-22a).
pub fn pinned(rev: &Revision, g: &Gesture) -> Result<Gesture, EditError> {
    let mut w = Work { rev, lanes: BTreeMap::new() };
    let mut g = g.clone();
    let mut pin = |r: &mut ClipRef| -> Result<(), EditError> {
        if r.sha.is_none() {
            let (lane, i) = w.resolve(r)?;
            let l = &w.lanes[&lane];
            r.sha = Some(sha(&l.was[i].line(&l.kind)));
        }
        Ok(())
    };
    match &mut g {
        Gesture::Move { clips, .. }
        | Gesture::Split { clips, .. }
        | Gesture::Join { clips }
        | Gesture::Duplicate { clips, .. }
        | Gesture::Delete { clips }
        | Gesture::Quantize { clips, .. } => clips.iter_mut().try_for_each(&mut pin)?,
        Gesture::Resize { clip, .. } | Gesture::Notes { clip, .. } | Gesture::Points { clip, .. } | Gesture::Audio { clip, .. } => pin(clip)?,
        Gesture::Create { .. } | Gesture::Paste { .. } | Gesture::Set { .. } => {}
    }
    Ok(g)
}

/// [`edit`] as the Tx with id `tx_id`: the clips it makes get the ids [`apply_as`] gives them.
pub fn edit_as(s: &mut Session, g: &Gesture, tx_id: &str) -> Result<ClipsEdited, EditError> {
    ready(s)?;
    let tx_id = tx_id.to_string();
    let rev = s.accepted();
    let edits = apply_as(rev, g, &tx_id)?;
    let made: Vec<(LaneAddr, String)> = edits.iter().flat_map(|e| e.made.iter().map(|id| (e.lane.clone(), id.clone()))).collect();
    let mut ops = vec![];
    // Per lane: each clip put, by id, with its line; and the ids gone.
    let mut wrote: Vec<(LaneAddr, BTreeMap<String, String>, Vec<String>)> = vec![];
    for e in &edits {
        let file = DataPath::lane(&e.lane).path();
        let now = rev.snapshot.files.get(&file);
        if now.is_none()
            && let Some(by) = rev.snapshot.taken(&file)
        {
            return Err(EditError::Rejected(crate::data::there_already(&file, &by)));
        }
        let was = rev.compiled.data.lane(e.lane.module.as_str(), e.lane.lane.as_str()).cloned();
        let old = was.as_deref().map(lines).unwrap_or_default();
        let new = lines(&e.data);
        let put: Vec<&String> = new.iter().filter(|(id, l)| old.get(*id) != Some(l)).map(|(id, _)| id).collect();
        let gone: Vec<String> = old.keys().filter(|id| !new.contains_key(*id)).cloned().collect();
        if put.is_empty() && gone.is_empty() && was.as_deref().is_some_and(|w| canonical(w) == canonical(&e.data)) {
            continue;
        }
        // Clip by clip where every clip has an address in the file as it is and as written; else
        // the lane whole.
        let by_id = was.as_deref().is_none_or(LaneData::addressable) && !rev.snapshot.not_utf8.contains_key(&file);
        ops.push(match by_id {
            true => Op::Clips {
                lane: e.lane.clone(),
                put: put.iter().map(|id| serde_json::from_str(&new[*id]).expect("a canonical line is JSON")).collect(),
                delete: gone.clone(),
            },
            false => Op::Write { file: TxPath::lane(&e.lane), bytes: Payload::Text(canonical(&e.data).into()) },
        });
        let put = put.into_iter().map(|id| (id.clone(), new[id].clone())).collect();
        wrote.push((e.lane.clone(), put, gone));
    }
    let shown = edits.first().map(|e| DataPath::lane(&e.lane).to_string()).unwrap_or_default();
    if ops.is_empty() {
        let same = "no change".to_string();
        let edited = Edited { revision: rev.number, file: shown, before: same.clone(), after: same };
        return Ok(ClipsEdited { edited, made: vec![] });
    }
    let label = describe(rev, &edits);
    let lanes = wrote.iter().map(|(l, ..)| l.clone()).collect();
    let mut tx = plan::planned(s, label.clone(), ops, Expect::Confined(Confined { lanes, ..Confined::default() }));
    tx.id = tx_id.clone();
    let done = plan::run(s, tx, |_, checked| {
        // The planner's assertion: each lane reads every clip put as written, and none deleted;
        // the rest of each lane is as the file has it, an outside edit to another clip merged
        // (WRITE-3), and nothing else changes (`Confined`).
        let data = &checked.candidate().compiled.data;
        for (lane, put, gone) in &wrote {
            let read = data.lane(lane.module.as_str(), lane.lane.as_str());
            let line = |id: &str| read.map(|d| d.lines_of(id)).unwrap_or_default();
            let kept = put.iter().all(|(id, l)| line(id) == [l.clone()]) && gone.iter().all(|id| line(id).is_empty());
            if !kept {
                return Err(EditError::Rejected(format!("writing {} would change more than its clips", DataPath::lane(lane))));
            }
        }
        Ok(())
    })?;
    Ok(ClipsEdited { edited: Edited { revision: done.number, file: shown, before: label.0, after: label.1 }, made })
}

/// Each clip's canonical line, by id, for the clips with an id.
fn lines(d: &LaneData) -> BTreeMap<String, String> {
    d.clip_lines().into_iter().filter_map(|(id, line)| Some((id?.to_string(), line))).collect()
}

/// What an edit of lanes changed, to show it and its undo: for one lane, the clips (or notes) only
/// one side has; for several, each lane's, named.
fn describe(rev: &Revision, edits: &[LaneEdit]) -> (String, String) {
    let arr = &rev.project.arrangement;
    let each: Vec<(String, (String, String))> = edits
        .iter()
        .map(|e| {
            let (module, key) = (e.lane.module.as_str(), e.lane.lane.as_str());
            let was = rev.compiled.data.lane(module, key).map(|d| (**d).clone()).unwrap_or_else(|| LaneData::empty(&e.data.kind()));
            let m = rev.compiled.ir.modules.iter().find(|m| m.key == module);
            let lane = m.and_then(|m| m.lanes.iter().find(|l| l.key == key)).map_or(key, |l| l.label());
            let name = format!("{} {lane}", m.map_or(module, |m| m.label()));
            let said = match (&was, &e.data) {
                (LaneData::Notes(a), LaneData::Notes(b)) => crate::notes::describe(a, b, arr),
                (a, b) => spans_described(a, b, arr),
            };
            (name, said)
        })
        .collect();
    match &each[..] {
        [(_, one)] => one.clone(),
        _ => {
            let side = |f: &dyn Fn(&(String, String)) -> &String| each.iter().map(|(n, s)| format!("{n}: {}", f(s))).collect::<Vec<_>>().join("; ");
            (side(&|s| &s.0), side(&|s| &s.1))
        }
    }
}

/// [`crate::notes::describe`] for automation and audio clips: the clips only one side has; with
/// the same clips, the one that changed.
fn spans_described(before: &LaneData, after: &LaneData, arr: &m2_song::Arrangement) -> (String, String) {
    let (a, b) = (clips_of(before), clips_of(after));
    let span = |c: &Clip| (c.start(), c.length());
    let only = |x: &[Clip], y: &[Clip]| -> Vec<Clip> { x.iter().filter(|c| !y.iter().any(|d| span(d) == span(c))).cloned().collect() };
    let words = |c: &Clip| {
        let bars = c.length() / arr.meter_at(c.start()).bar();
        let n = m2_song::lane::number(bars);
        format!("clip at bar {} ({n} {})", arr.bar_at(c.start()).0 + 1, if bars == 1.0 { "bar" } else { "bars" })
    };
    let listed = |cs: &[Clip]| match cs {
        [] => "no clip".to_string(),
        [c] => words(c),
        cs => format!("{} clips", cs.len()),
    };
    let (gone, new) = (only(&a, &b), only(&b, &a));
    if !gone.is_empty() || !new.is_empty() {
        return (listed(&gone), listed(&new));
    }
    // The same spans: the clips that changed inside.
    let inside = |c: &Clip| match c {
        Clip::Control(c) => format!("{}, {} points", words(&Clip::Control(c.clone())), c.points.len()),
        c => format!("{}, as it was", words(c)),
    };
    match a.iter().zip(&b).find(|(x, y)| x != y) {
        Some((x, y)) => (inside(x), match y {
            Clip::Audio(_) => format!("{}, changed", words(y)),
            y => inside(y),
        }),
        None => ("no change".into(), "no change".into()),
    }
}

/// A place in the song written as bar.beat.sixteenth, each counted from 1 in the song's meter
/// (`9`, `9.3`, `9.3.2`): how many beats from the start. Beats are quarter notes, as everywhere in
/// the timeline.
pub fn position(arr: &m2_song::Arrangement, text: &str) -> Result<f64, String> {
    let bad = || format!("`{text}` isn't a place in the song: write bar.beat.sixteenth, counted from 1, such as 9.3.1");
    let mut parts = text.trim().split('.').map(|p| p.parse::<u64>().ok().filter(|n| *n >= 1));
    let bar = parts.next().flatten().ok_or_else(bad)?;
    let beat = parts.next().unwrap_or(Some(1)).ok_or_else(bad)?;
    let sixteenth = parts.next().unwrap_or(Some(1)).ok_or_else(bad)?;
    if parts.next().is_some() || sixteenth > 4 {
        return Err(bad());
    }
    let start = bar_start(arr, bar - 1);
    let (beats, at) = (arr.meter_at(start).bar(), (beat - 1) as f64 + (sixteenth - 1) as f64 / 4.0);
    match at < beats {
        true => Ok(start + at),
        false => Err(format!("bar {bar} has {} beats, so it has no {text}", m2_song::lane::number(beats))),
    }
}

/// The beat bar `n` starts at (the first is 0), counted in the song's time signatures.
fn bar_start(arr: &m2_song::Arrangement, n: u64) -> f64 {
    let meters = if arr.meter.is_empty() { vec![m2_song::Meter::COMMON] } else { arr.meter.clone() };
    let mut before = 0;
    for (i, m) in meters.iter().enumerate() {
        let k = n - before;
        match meters.get(i + 1).map(|next| ((next.beat - m.beat) / m.bar()).round() as u64) {
            Some(bars) if k >= bars => before += bars,
            _ => return m.beat + k as f64 * m.bar(),
        }
    }
    unreachable!("the last time signature runs forever")
}

/// A timeline lane's JSON with each clip's [`sha`], from `data`, the lane it shows: none when the
/// clips aren't the data's, one for one.
pub fn with_sha(mut lane: Value, data: Option<&LaneData>) -> Value {
    let lines = data.map(LaneData::clip_lines).unwrap_or_default();
    if let Some(clips) = lane.get_mut("clips").and_then(Value::as_array_mut)
        && clips.len() == lines.len()
    {
        for (c, (id, line)) in clips.iter_mut().zip(&lines) {
            if let Some(c) = c.as_object_mut().filter(|c| c.get("id").and_then(Value::as_str) == *id) {
                c.insert("sha".into(), Value::String(sha(line)));
            }
        }
    }
    lane
}

/// A clip's hash, as the timeline gives it and a gesture names it by: the first 16 hex digits of
/// the SHA-256 of its canonical line.
pub fn sha(line: &str) -> String {
    m2_song::Sha::of(line.as_bytes()).hex()[..16].to_string()
}

#[cfg(test)]
#[path = "clips_tests.rs"]
mod tests;
