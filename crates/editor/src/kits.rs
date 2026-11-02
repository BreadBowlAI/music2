//! A song's drum kits (KIT-1): a bus and the Drum tracks whose channels go to it. Worked out from
//! the song every time, never stored: the song's view, the beat grid and `m2 graph` read
//! [`drum_kits`]. Here too what the beat grid writes that the clip gestures don't: the pitch a new
//! hit plays at (KIT-14) and + Drum (KIT-15). The Drum itself, its General MIDI table and the kits
//! the packs name are [`crate::drums`]'.
//!
//! A Drum track plays one `@m2/drums` Drum (`drums.drum`) from a notes lane of its own: the voices
//! its call made read that lane, and nothing else. Its sound is the constant its `drum` control
//! plays, as written or by default; a lane or a signal has none to read. [`drum`] is the one place
//! that says so: KIT-1's rows and a MIDI file's drums going out (KIT-9) both read it.

use std::collections::BTreeSet;

use m2_ir::{Module, Node, Op, split_ref};
use m2_script::component::{Insert, Spelled};
use m2_session::{Revision, Session};
use serde::Serialize;

use crate::controls::walk;
use crate::drums::{self, DRUM};
use crate::mix::{self, Mixing};
use crate::tracks::{self, Goes, Made, NewTrack, Play};
use crate::{EditError, bus, kind, layout};

/// A drum kit of the song (KIT-1): a bus and the Drum tracks that go to it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DrumKit {
    /// The bus, by its key.
    pub bus: String,
    /// The kit's name: its bus's label.
    pub label: String,
    /// Its Drum tracks, in the timeline's order: the beat grid's rows.
    pub rows: Vec<DrumRow>,
}

/// A Drum track of a kit: a row of the beat grid.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DrumRow {
    /// The track, by its key.
    pub track: String,
    /// The notes lane its Drum plays, by its key.
    pub lane: String,
    /// The sound its Drum plays (`snare`): its `drum` as written, or the default when the call
    /// leaves it out; none when the code works it out.
    pub sound: Option<String>,
}

/// The song's drum kits, in the timeline's order of their buses.
pub fn drum_kits(s: &Session) -> Vec<DrumKit> {
    let rev = s.accepted();
    let Mixing::One(m) = mix::mixer(rev) else { return vec![] };
    let order = layout::view(s).order;
    let place = |key: &str| order.iter().position(|k| k == key).unwrap_or(usize::MAX);
    // Each bus channel's bus, and each channel's output.
    let buses = bus::bus_channels(rev, &m);
    let routes = bus::routing(rev, &m);
    let mut kits: Vec<DrumKit> = buses.values().map(|b| DrumKit { bus: b.clone(), label: bus::label_of(rev, b), rows: vec![] }).collect();
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

/// Add a Drum track to the kit of bus `bus` (KIT-15): playing `sound` (one of the Drum's), its
/// `drum` written, named as a kit's track of it is (KIT-6's words: Snare, Toms), its channel going to
/// the bus, in the timeline after the kit's last row. Add a track's one Tx, one undo step.
pub fn add_drum(s: &mut Session, bus: &str, sound: &str) -> Result<Made, EditError> {
    let value = s.catalog().components.get(DRUM).and_then(|d| sound_value(d, sound));
    let Some(value) = value else {
        let sounds: Vec<&str> = drums::GM.iter().map(|g| g.sound).collect();
        return Err(EditError::Rejected(format!("A Drum plays {}, not {sound:?}.", kind::one_of(&sounds))));
    };
    let rev = s.accepted();
    let Some(kit) = drum_kits(s).into_iter().find(|k| k.bus == bus) else {
        return Err(EditError::Rejected(format!("{} is no drum kit's bus: no Drum track goes to it.", bus::label_of(rev, bus))));
    };
    let Mixing::One(m) = mix::mixer(rev) else { unreachable!("a kit is a mixer's") };
    let channel = bus::bus_channels(rev, &m).into_iter().find(|(_, b)| *b == kit.bus).map(|(c, _)| c).expect("a kit's bus has a channel");
    let label = drums::GM.iter().find(|g| g.sound == sound).map_or(sound, |g| g.name);
    let drum = NewTrack {
        play: Play::Instrument(DRUM.into()),
        label: Some(label.into()),
        values: vec![value],
        to: Some(Goes::Bus(channel)),
        ..NewTrack::default()
    };
    let after = kit.rows.last().map(|r| r.track.clone());
    Ok(tracks::add_tracks(s, &[drum], after.as_deref(), None)?.remove(0))
}

/// The value a Drum of descriptor `d` plays `sound` by: its `drum` menu at that sound's place, as an
/// insert carries it; none for a sound the menu hasn't got. + Drum writes it (KIT-15), and the call
/// form it makes is checked (PKG-11, `planned_call_forms_are_current`).
pub(crate) fn sound_value(d: &m2_script::Descriptor, sound: &str) -> Option<Insert> {
    let (_, options) = kind::of_unit(d.control("drum")?.unit);
    let place = options.iter().position(|o| o.value == sound)? as f64;
    Some(Insert { slot: "drum".into(), value: Spelled { value: place, spelled: None, written: place } })
}

/// Track `module` as a row, when it is a Drum track: one Drum, from a notes lane of its own.
pub(crate) fn drum(rev: &Revision, module: &str) -> Option<DrumRow> {
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
    Some(DrumRow { track: module.to_string(), lane, sound })
}

/// The pitch a new hit plays at on a row whose notes play `pitches` and whose Drum plays `sound`
/// (KIT-14): the pitch most of them play (of a tie, the lowest), else the sound's own note.
pub(crate) fn pitch(pitches: impl IntoIterator<Item = f64>, sound: Option<&str>) -> f64 {
    let mut count: Vec<(f64, usize)> = vec![];
    for p in pitches {
        match count.iter_mut().find(|(q, _)| *q == p) {
            Some(c) => c.1 += 1,
            None => count.push((p, 1)),
        }
    }
    // A row with no notes plays its sound's own note (KIT-6); a sound the code works out, A2, which
    // plays every sound as itself.
    let own = || sound.and_then(drums::own_note).map_or(45.0, f64::from);
    count.into_iter().max_by(|a, b| a.1.cmp(&b.1).then(b.0.total_cmp(&a.0))).map_or_else(own, |(p, _)| p)
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
