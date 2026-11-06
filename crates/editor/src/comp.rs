//! Comping (VOX-1 to VOX-6): one take built from the best parts of several, recorded round a loop
//! into one file (MON-11). A comp is plain audio clips of that file in a row, keeping one pass's
//! timeline, each a **piece** playing one take by where it reads in the file; nothing new is saved.
//!
//! [`comp`] works on the comp's **segments**, the beats each piece plays alone, from the middle of
//! one join to the middle of the next, and touches only what its range reaches (VOX-5):
//! 1. Cut the segments at the range's edges, and point each one inside it at the take: where it
//!    reads in the file moves by the distance between the two takes' starts (VOX-3).
//! 2. Join segments side by side across the comp's own joins in the range, where they play on from
//!    each other at the same speed (VOX-5). A join the musician made is never merged.
//! 3. Make each segment a clip again. A join the range reaches is made again, an overlap of
//!    [`JOIN`] with fades that long (VOX-4); every other edge stays as it was, and a clip that
//!    comes out as it was is kept as it was, to the bit.

use m2_song::{AudioClip, TempoMap};

use crate::EditError;

/// How long a join's overlap is, and its fades: 10 ms (VOX-4).
pub const JOIN: f64 = 0.01;
/// The most two clips of a comp overlap, and the most their places in a pass differ, in seconds
/// (VOX-2).
const REACH: f64 = 0.05;
/// How far a piece may read past where its take has sound, in seconds: 1 ms, as an edge may reach
/// past its file (a pass is a whole number of samples, the loop's beats may not be).
const SLACK: f64 = 0.001;
/// Beats or seconds closer than this are one place.
const SAME: f64 = 1e-9;

/// What a comp makes of a piece (by its index): kept as it was, or made again from it.
#[derive(Debug, PartialEq)]
pub enum Piece {
    Same(usize),
    Made(usize, AudioClip),
}

/// What a comp needs to know of the file: where each take starts in it, and how long it is, in
/// seconds (none when its header can't be read).
pub struct File<'a> {
    pub takes: &'a [f64],
    pub seconds: Option<f64>,
}

/// Whether `c` can be a piece: it reads its file forwards at its own speed (VOX-2, VOX-6).
pub fn plays(c: &AudioClip) -> bool {
    !c.reverse && c.pitch == 0.0
}

/// Where in its pass clip `c` plays at song beat `beat`, in seconds.
fn place(c: &AudioClip, beat: f64, takes: &[f64], tempo: &TempoMap) -> f64 {
    let at = reads(c, beat, tempo);
    at - takes[take_at(takes, at)]
}

/// The comp that clip `named` is in, among a lane's clips of one file (`clips`, in the lane's
/// order): the indices of its pieces, in time order. Each plays forwards at its own pitch, starts
/// after the one before starts and ends after it ends, starts no later than it ends, overlapping it
/// by [`REACH`] at most, and plays the same place in its pass as the one before there, within
/// [`REACH`] (VOX-2).
pub fn pieces(clips: &[&AudioClip], named: usize, takes: &[f64], tempo: &TempoMap) -> Vec<usize> {
    let mut order: Vec<usize> = (0..clips.len()).collect();
    order.sort_by(|a, b| clips[*a].start.total_cmp(&clips[*b].start).then(a.cmp(b)));
    let links = |a: &AudioClip, b: &AudioClip| {
        let (a_end, b_end) = (a.start + a.length, b.start + b.length);
        plays(a)
            && plays(b)
            && b.start > a.start
            && b_end > a_end
            && b.start <= a_end + SAME
            && tempo.seconds_at(a_end) - tempo.seconds_at(b.start) <= REACH + SAME
            && (place(a, b.start, takes, tempo) - place(b, b.start, takes, tempo)).abs() <= REACH + SAME
    };
    let Some(at) = order.iter().position(|i| *i == named) else { return vec![] };
    let (mut lo, mut hi) = (at, at);
    while lo > 0 && links(clips[order[lo - 1]], clips[order[lo]]) {
        lo -= 1;
    }
    while hi + 1 < order.len() && links(clips[order[hi]], clips[order[hi + 1]]) {
        hi += 1;
    }
    order[lo..=hi].to_vec()
}

/// The beats a segment plays, the clip it reads by (pointed at its take; its span as it was), and
/// the pieces whose left and right edges it has.
#[derive(Clone, Debug)]
struct Seg {
    lo: f64,
    hi: f64,
    base: AudioClip,
    from: usize,
    last: usize,
    /// Cut, pointed at another take or joined by this comp.
    changed: bool,
}

/// The edge between two segments: where it is, and whether a comp made it (cut now, or a join a
/// comp made before: 10 ms of overlap with 10 ms fades).
#[derive(Clone, Copy, Debug)]
struct Edge {
    at: f64,
    ours: bool,
}

/// Where clip `c` reads in its file at song beat `beat`, in seconds: from its offset, or by its
/// warp markers as the engine reads them (straight between markers, on past the outer ones). It
/// plays forwards at its own pitch (VOX-6).
pub(crate) fn reads(c: &AudioClip, beat: f64, tempo: &TempoMap) -> f64 {
    let w = &c.warp;
    let b = beat - c.start;
    match w.len() {
        0 => c.offset + tempo.seconds_at(beat) - tempo.seconds_at(c.start),
        1 => w[0].0 + tempo.seconds_at(beat) - tempo.seconds_at(c.start + w[0].1),
        _ => {
            let i = w.partition_point(|m| m.1 <= b).clamp(1, w.len() - 1);
            let (a, z) = (w[i - 1], w[i]);
            a.0 + (b - a.1) * (z.0 - a.0) / (z.1 - a.1)
        }
    }
}

/// How fast `c` reads its file just before (`side` −1) or just after (+1) song beat `beat`:
/// seconds of file a second.
fn speed(c: &AudioClip, beat: f64, side: f64, tempo: &TempoMap) -> f64 {
    let d = 1e-3 * side;
    (reads(c, beat + d, tempo) - reads(c, beat, tempo)) / (tempo.seconds_at(beat + d) - tempo.seconds_at(beat))
}

/// `c` reading `by` seconds later in its file: its offset and its markers move, as the Takes menu
/// moves them (MON-11).
fn repoint(c: &mut AudioClip, by: f64) {
    c.offset += by;
    c.warp.iter_mut().for_each(|m| m.0 += by);
}

/// The take that holds `at`, seconds into the file: the last that starts at or before it.
fn take_at(takes: &[f64], at: f64) -> usize {
    takes.iter().rposition(|t| *t <= at + 1e-6).unwrap_or(0)
}

/// `x` to a billionth: no float noise in the lane file (review P13).
fn tidy(x: f64) -> f64 {
    (x * 1e9).round() / 1e9 + 0.0
}

/// Comp `pieces` (a comp, in time order, as [`pieces`] finds it): take `take` (from 0) of the
/// file's takes plays from beat `from` to `to`, the rest as before. `place` says a beat in the
/// window's words ("bar 2 beat 1"). What becomes of the pieces, in time order; a piece named by
/// none is gone, joined into the one before it.
pub fn comp(
    pieces: &[&AudioClip],
    file: &File,
    take: usize,
    (from, to): (f64, f64),
    tempo: &TempoMap,
    place: &dyn Fn(f64) -> String,
) -> Result<Vec<Piece>, EditError> {
    let no = |why: String| Err(EditError::Rejected(why));
    let takes = file.takes;
    if !(from.is_finite() && to.is_finite() && to > from) {
        return no("A comp's range is two beats, the second after the first.".into());
    }
    if takes.len() < 2 {
        return no("This clip's file has no takes to choose from.".into());
    }
    if take >= takes.len() {
        return no(format!("Its file has {} takes: choose 1 to {}.", takes.len(), takes.len()));
    }
    let (Some(first), Some(last)) = (pieces.first(), pieces.last()) else { return no("That clip isn't there any more.".into()) };
    for p in pieces {
        let why = match () {
            _ if p.reverse => "it plays backwards",
            _ if p.pitch != 0.0 => "its pitch is changed",
            _ => continue,
        };
        return no(format!("Comp cuts only clips that play forwards at their own pitch, and {why}."));
    }
    let secs = |b: f64| tempo.seconds_at(b);
    let name = format!("Take {}", take + 1);
    // Each piece's segment: from the middle of the join before it to the middle of the one after.
    let mut cuts = vec![first.start];
    let mut edges = vec![];
    for w in pieces.windows(2) {
        let (a, b) = (w[0], w[1]);
        let a_end = a.start + a.length;
        let at = if b.start < a_end { tempo.beat_at((secs(a_end) + secs(b.start)) / 2.0) } else { b.start };
        let ours = (secs(a_end) - secs(b.start) - JOIN).abs() < 1e-6 && (a.fade_out - JOIN).abs() < 1e-9 && (b.fade_in - JOIN).abs() < 1e-9;
        cuts.push(at);
        edges.push(Edge { at, ours });
    }
    cuts.push(last.start + last.length);
    let (start, end) = (cuts[0], cuts[cuts.len() - 1]);
    // The range, cut to the comp; a cut less than a join from a join or an edge moves onto it.
    let snap = |x: f64| cuts.iter().copied().find(|c| (secs(*c) - secs(x)).abs() < JOIN).unwrap_or(x);
    let (lo, hi) = (snap(from.max(start)), snap(to.min(end)));
    if hi <= lo + SAME {
        return no(format!("There's nothing of this comp from {} to {}.", place(from), place(to)));
    }
    if secs(hi) - secs(lo) < JOIN - SAME {
        return no("That part is too short to comp: swipe across more of it.".into());
    }
    let mut segs: Vec<Seg> = pieces
        .iter()
        .enumerate()
        .map(|(i, p)| Seg { lo: cuts[i], hi: cuts[i + 1], base: (*p).clone(), from: i, last: i, changed: false })
        .collect();
    // 1. Cut at the range's edges, never inside a piece's own fade; point what is inside at the take.
    for x in [lo, hi] {
        if let Some(i) = segs.iter().position(|s| s.lo + SAME < x && x < s.hi - SAME) {
            let p = pieces[segs[i].from];
            let into = secs(x) - secs(p.start);
            let left = secs(p.start + p.length) - secs(x);
            if into < p.fade_in + JOIN / 2.0 || left < p.fade_out + JOIN / 2.0 {
                return no("Comp can't cut inside a clip's fade: make the fade shorter first, or swipe outside it.".into());
            }
            let mut after = segs[i].clone();
            after.lo = x;
            segs[i].hi = x;
            segs[i].changed = true;
            after.changed = true;
            segs.insert(i + 1, after);
            edges.insert(i, Edge { at: x, ours: true });
        }
    }
    let inside = |s: &Seg| s.lo >= lo - SAME && s.hi <= hi + SAME;
    let sound = (takes[take], takes.get(take + 1).copied().or(file.seconds).unwrap_or(f64::INFINITY));
    for s in segs.iter_mut().filter(|s| inside(s)) {
        let mid = reads(&s.base, (s.lo + s.hi) / 2.0, tempo);
        let by = takes[take] - takes[take_at(takes, mid)];
        if by == 0.0 {
            continue;
        }
        repoint(&mut s.base, by);
        s.changed = true;
        // Where the take has sound: from its start to the next one's, or the file's end.
        if reads(&s.base, s.lo, tempo) < sound.0 - SLACK {
            return no(format!("{name} has no sound that early."));
        }
        if reads(&s.base, s.hi, tempo) > sound.1 + SLACK {
            let (mut a, mut b) = (s.lo, s.hi);
            for _ in 0..60 {
                let m = (a + b) / 2.0;
                if reads(&s.base, m, tempo) > sound.1 { b = m } else { a = m }
            }
            return no(format!("{name} stops at {}: swipe up to there.", place(a)));
        }
    }
    // 2. Join neighbours across the comp's own joins in the range, where they play on at one speed.
    let mut joined: Vec<Seg> = vec![];
    let mut kept_edges: Vec<Edge> = vec![];
    for (i, s) in segs.into_iter().enumerate() {
        if let Some(p) = joined.last_mut() {
            let e = edges[i - 1];
            let at = e.at;
            let merges = e.ours
                && at >= lo - SAME
                && at <= hi + SAME
                && p.base.gain_db == s.base.gain_db
                && p.base.warp.is_empty() == s.base.warp.is_empty()
                && (reads(&p.base, at, tempo) - reads(&s.base, at, tempo)).abs() < 1e-6
                && (speed(&p.base, at, -1.0, tempo) - speed(&s.base, at, 1.0, tempo)).abs() < 1e-6;
            if merges {
                if !p.base.warp.is_empty() {
                    // The first's markers before the join, one on it, the second's after: each side
                    // reads as it did.
                    let (j, by) = (at - p.base.start, s.base.start - p.base.start);
                    let mut w: Vec<(f64, f64)> = p.base.warp.iter().copied().filter(|m| m.1 < j - SAME).collect();
                    if w.is_empty() {
                        w.push((reads(&p.base, at - 1.0, tempo), j - 1.0));
                    }
                    w.push((reads(&p.base, at, tempo), j));
                    let after: Vec<(f64, f64)> = s.base.warp.iter().map(|m| (m.0, m.1 + by)).filter(|m| m.1 > j + SAME).collect();
                    match after.is_empty() {
                        true => w.push((reads(&s.base, at + 1.0, tempo), j + 1.0)),
                        false => w.extend(after),
                    }
                    p.base.warp = w;
                }
                p.hi = s.hi;
                p.last = s.last;
                p.changed = true;
                continue;
            }
            kept_edges.push(e);
        }
        joined.push(s);
    }
    // 3. Each segment a clip again: a join the range reaches made again, every other edge as it was.
    let remade = |e: &Edge| e.at >= lo - SAME && e.at <= hi + SAME;
    let n = joined.len();
    let mut out = vec![];
    for (i, s) in joined.into_iter().enumerate() {
        let left = (i > 0).then(|| kept_edges[i - 1]).filter(remade);
        let right = (i + 1 < n).then(|| kept_edges[i]).filter(remade);
        let (head, tail) = (pieces[s.from], pieces[s.last]);
        let from_beat = left.map_or(head.start, |_| tempo.beat_at(secs(s.lo) - JOIN / 2.0));
        let until = right.map_or(tail.start + tail.length, |_| tempo.beat_at(secs(s.hi) + JOIN / 2.0));
        let mut c = s.base;
        if c.warp.is_empty() {
            c.offset += secs(from_beat) - secs(c.start);
        } else {
            let by = c.start - from_beat;
            c.warp.iter_mut().for_each(|m| m.1 += by);
        }
        if c.offset < -1e-9 {
            return no(format!("{name} has no sound that early."));
        }
        c.offset = tidy(c.offset.max(0.0));
        c.warp.iter_mut().for_each(|m| *m = (tidy(m.0), tidy(m.1)));
        (c.start, c.length) = (tidy(from_beat), tidy(until - from_beat));
        if c.length < crate::clips::SHORTEST {
            return no("That part is too short to comp: swipe across more of it.".into());
        }
        c.fade_in = left.map_or(head.fade_in, |_| JOIN);
        c.fade_out = right.map_or(tail.fade_out, |_| JOIN);
        let was = pieces[s.from];
        let same = |a: f64, b: f64| (a - b).abs() < SAME;
        let kept = !s.changed && left.is_none() && right.is_none()
            || same(c.start, was.start)
                && same(c.length, was.length)
                && same(c.offset, was.offset)
                && same(c.fade_in, was.fade_in)
                && same(c.fade_out, was.fade_out)
                && c.warp.len() == was.warp.len()
                && c.warp.iter().zip(&was.warp).all(|(a, b)| same(a.0, b.0) && same(a.1, b.1));
        out.push(if kept { Piece::Same(s.from) } else { Piece::Made(s.from, c) });
    }
    Ok(out)
}

#[cfg(test)]
#[path = "comp_tests.rs"]
mod tests;
