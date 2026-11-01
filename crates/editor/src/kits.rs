//! Drum kits (KIT-1): a bus and the Drum tracks whose channels go to it. Worked out from the song
//! every time, never stored: the song's view, the beat grid and `m2 graph` read this one function.
//!
//! A Drum track plays one `@m2/drums` Drum (`drums.drum`) from a notes lane of its own: the voices
//! its call made read that lane, and nothing else. Its sound is the constant its `drum` control
//! plays, as written or by default; a lane or a signal has none to read.

use std::collections::BTreeSet;

use m2_ir::{Module, Node, Op, split_ref};
use m2_session::{Revision, Session};
use serde::Serialize;

use crate::controls::walk;
use crate::mix::{self, Mixing};
use crate::{bus, kind, layout};

/// The Drum, by its ComponentId: one drum on a track.
pub const DRUM: &str = "@m2/drums:drum";

/// A drum kit: a bus and the Drum tracks that go to it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Kit {
    /// The bus, by its key.
    pub bus: String,
    /// The kit's name: its bus's label.
    pub label: String,
    /// Its Drum tracks, in the timeline's order: the beat grid's rows.
    pub rows: Vec<Row>,
}

/// A Drum track of a kit: a row of the beat grid.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Row {
    /// The track, by its key.
    pub track: String,
    /// The notes lane its Drum plays, by its key.
    pub lane: String,
    /// The sound its Drum plays (`snare`): its `drum` as written, or the default when the call
    /// leaves it out; none when the code works it out.
    pub sound: Option<String>,
}

/// The song's drum kits, in the timeline's order of their buses.
pub fn kits(s: &Session) -> Vec<Kit> {
    let rev = s.accepted();
    let Mixing::One(m) = mix::mixer(rev) else { return vec![] };
    let order = layout::view(s).order;
    let place = |key: &str| order.iter().position(|k| k == key).unwrap_or(usize::MAX);
    // Each bus channel's bus, and each channel's output.
    let buses = bus::bus_channels(rev, &m);
    let routes = bus::routing(rev, &m);
    let mut kits: Vec<Kit> = buses.values().map(|b| Kit { bus: b.clone(), label: bus::label_of(rev, b), rows: vec![] }).collect();
    for c in &m.channels {
        let Some(to) = routes.get(&c.key).and_then(|r| r.to.as_ref()).and_then(|to| buses.get(to)) else { continue };
        let kit = kits.iter_mut().find(|k| k.bus == *to).expect("a bus channel's kit");
        kit.rows.extend(c.tracks.iter().filter_map(|t| drum(rev, t)));
    }
    kits.retain(|k| !k.rows.is_empty());
    for k in &mut kits {
        k.rows.sort_by_key(|r| place(&r.track));
    }
    kits.sort_by_key(|k| place(&k.bus));
    kits
}

/// Track `module` as a row, when it is a Drum track: one Drum, from a notes lane of its own.
pub(crate) fn drum(rev: &Revision, module: &str) -> Option<Row> {
    let m = rev.compiled.ir.modules.iter().find(|m| m.key == module && m.track)?;
    let frames: Vec<u32> = (0..m.frames.len() as u32).filter(|i| m.frames[*i as usize].component.as_deref() == Some(DRUM)).collect();
    let [frame] = frames[..] else { return None };
    // What the Drum's voices read their notes from: one lane, or it isn't one of the track's.
    let mut read: BTreeSet<Option<&str>> = BTreeSet::new();
    let mut sounds: BTreeSet<Option<String>> = BTreeSet::new();
    walk(&m.nodes, &mut |n: &Node| match (&n.op, &n.origin) {
        (Op::Poly { events, .. }, _) if within(m, n.frame, frame) => {
            read.insert(lane_of(m, events));
        }
        (Op::Const { value, unit }, Some(o)) if o.control.as_ref().is_some_and(|c| c.frame == frame && c.slot == "drum") => {
            sounds.insert(kind::said(*unit, *value).and_then(|v| v.as_str().map(str::to_string)));
        }
        _ => {}
    });
    let lane = match read.into_iter().collect::<Vec<_>>()[..] {
        [Some(lane)] => lane.to_string(),
        _ => return None,
    };
    let sound = match sounds.into_iter().collect::<Vec<_>>()[..] {
        [Some(ref s)] => Some(s.clone()),
        _ => None,
    };
    Some(Row { track: module.to_string(), lane, sound })
}

/// Whether frame `at` of `m` is frame `frame` or opened inside it.
fn within(m: &Module, at: Option<u32>, frame: u32) -> bool {
    let mut at = at;
    while let Some(i) = at {
        if i == frame {
            return true;
        }
        // A parent is always an earlier frame, so this ends.
        at = m.frames.get(i as usize).and_then(|f| f.parent).filter(|p| *p < i);
    }
    false
}

/// The lane of `m` that reference `r` reads, when it is a lane's node.
fn lane_of<'a>(m: &'a Module, r: &str) -> Option<&'a str> {
    let id = split_ref(r).0;
    m.nodes.iter().find(|n| n.id == id).and_then(|n| match &n.op {
        Op::Lane { lane } => Some(lane.as_str()),
        _ => None,
    })
}

#[cfg(test)]
#[path = "kits_tests.rs"]
mod tests;
