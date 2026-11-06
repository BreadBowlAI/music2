//! Comping (VOX-1 to VOX-6): one take built from the best parts of several, recorded round a loop
//! into one file (MON-11). A comp is plain audio clips of that file in a row, each a **piece**
//! playing one take by where it reads in the file; nothing new is saved.
//!
//! [`comp`] works on the comp's **segments**, the beats each piece plays alone, from the middle of
//! one join to the middle of the next, in three steps:
//! 1. Cut the segments at the range's edges, and point each one inside it at the take: where it
//!    reads in the file moves by the distance between the two takes' starts (VOX-3).
//! 2. Join segments side by side that play on from each other into one (VOX-5).
//! 3. Make each segment a clip again, overlapping the next by [`JOIN`], with fades that long
//!    (VOX-4). A clip that comes out as it was is kept as it was, to the bit.

use m2_song::{AudioClip, TempoMap};

use crate::EditError;

/// How long a join's overlap is, and its fades: 10 ms (VOX-4).
pub const JOIN: f64 = 0.01;
/// The most two clips of a comp overlap, in seconds (VOX-2).
const REACH: f64 = 0.05;
/// Beats closer than this are one place.
const SAME: f64 = 1e-9;

/// What a comp makes of a piece (by its index): kept as it was, or made again from it.
#[derive(Debug, PartialEq)]
pub enum Piece {
    Same(usize),
    Made(usize, AudioClip),
}

/// The comp that clip `named` is in, among a lane's clips of one file (`clips`, in the lane's
/// order): the indices of its pieces, in time order. Each starts after the one before starts and
/// ends after it ends, and starts no later than it ends, overlapping it by [`REACH`] at most
/// (VOX-2).
pub fn pieces(clips: &[&AudioClip], named: usize, tempo: &TempoMap) -> Vec<usize> {
    let mut order: Vec<usize> = (0..clips.len()).collect();
    order.sort_by(|a, b| clips[*a].start.total_cmp(&clips[*b].start).then(a.cmp(b)));
    let links = |a: &AudioClip, b: &AudioClip| {
        let (a_end, b_end) = (a.start + a.length, b.start + b.length);
        b.start > a.start
            && b_end > a_end
            && b.start <= a_end + SAME
            && tempo.seconds_at(a_end) - tempo.seconds_at(b.start) <= REACH + SAME
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
/// the piece it came from.
#[derive(Clone, Debug)]
struct Seg {
    lo: f64,
    hi: f64,
    base: AudioClip,
    from: usize,
}

/// Where clip `c` reads in its file at song beat `beat`, in seconds: from its offset, or by its
/// warp markers as the engine reads them (straight between markers, on past the outer ones). It
/// plays forwards at its own pitch (VOX-6).
fn reads(c: &AudioClip, beat: f64, tempo: &TempoMap) -> f64 {
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

/// Comp `pieces` (a comp, in time order, as [`pieces`] finds it): take `take` (from 0) of `takes`
/// plays from beat `from` to `to`, the rest as before. What becomes of the pieces, in time order;
/// a piece named by none is gone, joined into the one before it.
pub fn comp(pieces: &[&AudioClip], takes: &[f64], take: usize, (from, to): (f64, f64), tempo: &TempoMap) -> Result<Vec<Piece>, EditError> {
    let no = |why: String| Err(EditError::Rejected(why));
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
    // Each piece's segment: from the middle of the join before it to the middle of the one after.
    let mut cuts = vec![first.start];
    for w in pieces.windows(2) {
        let (a_end, b) = (w[0].start + w[0].length, w[1].start);
        cuts.push(if b < a_end { tempo.beat_at((secs(a_end) + secs(b)) / 2.0) } else { b });
    }
    cuts.push(last.start + last.length);
    let (start, end) = (cuts[0], cuts[cuts.len() - 1]);
    // The range, cut to the comp; a cut less than a join from a join or an edge moves onto it.
    let snap = |x: f64| cuts.iter().copied().find(|c| (secs(*c) - secs(x)).abs() < JOIN).unwrap_or(x);
    let (lo, hi) = (snap(from.max(start)), snap(to.min(end)));
    if hi <= lo + SAME {
        let beat = m2_song::lane::number;
        return no(format!("There's nothing of this comp from beat {} to beat {}.", beat(from), beat(to)));
    }
    let mut segs: Vec<Seg> =
        pieces.iter().enumerate().map(|(i, p)| Seg { lo: cuts[i], hi: cuts[i + 1], base: (*p).clone(), from: i }).collect();
    // 1. Cut at the range's edges; point what is inside at the take.
    for x in [lo, hi] {
        if let Some(i) = segs.iter().position(|s| s.lo + SAME < x && x < s.hi - SAME) {
            let mut after = segs[i].clone();
            after.lo = x;
            segs[i].hi = x;
            segs.insert(i + 1, after);
        }
    }
    for s in segs.iter_mut().filter(|s| s.lo >= lo - SAME && s.hi <= hi + SAME) {
        let mid = reads(&s.base, (s.lo + s.hi) / 2.0, tempo);
        repoint(&mut s.base, takes[take] - takes[take_at(takes, mid)]);
    }
    // 2. Join neighbours that play on from each other.
    let mut joined: Vec<Seg> = vec![];
    for s in segs {
        if let Some(p) = joined.last_mut()
            && p.base.gain_db == s.base.gain_db
            && p.base.warp.is_empty() == s.base.warp.is_empty()
            && (reads(&p.base, s.lo, tempo) - reads(&s.base, s.lo, tempo)).abs() < 1e-6
        {
            // The second's markers too, in the first's beats: it plays on what the second played.
            let by = s.base.start - p.base.start;
            for m in s.base.warp.iter().map(|m| (m.0, m.1 + by)) {
                if !p.base.warp.iter().any(|q| (q.1 - m.1).abs() < SAME) {
                    p.base.warp.push(m);
                }
            }
            p.base.warp.sort_by(|a, b| a.1.total_cmp(&b.1));
            p.hi = s.hi;
            continue;
        }
        joined.push(s);
    }
    // 3. Each segment a clip again, overlapping the next by a join, with its fades.
    let n = joined.len();
    let mut out = vec![];
    for (i, s) in joined.into_iter().enumerate() {
        let from = if i == 0 { start } else { tempo.beat_at(secs(s.lo) - JOIN / 2.0) };
        let until = if i + 1 == n { end } else { tempo.beat_at(secs(s.hi) + JOIN / 2.0) };
        let mut c = s.base;
        if c.warp.is_empty() {
            c.offset += secs(from) - secs(c.start);
        } else {
            let by = c.start - from;
            c.warp.iter_mut().for_each(|m| m.1 += by);
        }
        if c.offset < -1e-9 {
            return no("That take starts too late in its file to play there.".into());
        }
        c.offset = c.offset.max(0.0);
        (c.start, c.length) = (from, until - from);
        if c.length < crate::clips::SHORTEST {
            return no("That part is too short to comp: swipe across more of it.".into());
        }
        // The comp's own fades at its ends, where they fit beside a join's.
        let seconds = secs(until) - secs(from);
        let joins = |at_edge: bool| if at_edge { 0.0 } else { JOIN };
        c.fade_in = if i == 0 { first.fade_in.min(seconds - joins(n == 1)).max(0.0) } else { JOIN };
        c.fade_out = if i + 1 == n { last.fade_out.min(seconds - c.fade_in).max(0.0) } else { JOIN };
        let was = pieces[s.from];
        let same = |a: f64, b: f64| (a - b).abs() < SAME;
        let kept = same(c.start, was.start)
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
