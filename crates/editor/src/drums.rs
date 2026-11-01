//! The Drum and the kits the packs name (KIT-2 to KIT-9; the design is `docs/designs/drums/design.md`).
//! Which kits a song has, a Drums bus and the Drum tracks that go to it, worked out from the song,
//! is [`crate::kits`]' (KIT-1).
//!
//! - **The General MIDI table** (KIT-6): which of the Drum's sounds answers to each drum note, one
//!   table for [`split`] (a drum machine's lane by sound, KIT-7), a MIDI file's drums coming in as
//!   a kit (KIT-8) and a Drum's notes going out as General MIDI drums (KIT-9).
//! - **The kits the packs name** (KIT-3, KIT-4): each instrument's `editor.kits`, as + Track's Drum
//!   kit lists them, and adding one (KIT-5): a Drums bus and the kit's tracks, each going to it, in
//!   one Tx, one undo step, through [`tracks::add_tracks`].

use std::collections::BTreeMap;

use m2_script::component::{Insert, Spelled};
use m2_session::Session;
use m2_song::Note;
use serde::Serialize;

use crate::EditError;
use crate::bus::BusKind;
use crate::tracks::{self, Goes, Made, NewTrack, Play};

/// The Drum, by its ComponentId: one drum on a track, which a kit's tracks play.
pub const DRUM: &str = "@m2/drums:drum";

/// What a kit's bus is called: a kit's name is its bus's label (KIT-1).
pub const BUS: &str = "Drums";

/// One of the Drum's sounds (KIT-6): its name on the Drum's menu, the General MIDI drum notes it
/// answers to, and its own, the one music2 writes for it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GmDrum {
    pub sound: &'static str,
    pub notes: &'static [u8],
    pub own: u8,
    /// What a kit's track of it is called: the presets' word (Toms).
    pub name: &'static str,
}

/// The General MIDI table (KIT-6): which of the Drum's sounds answers to each drum note, in its
/// menu's order, and what a kit's track of each is called. The drum machine plays at each note the
/// very drum of its sound (KIT-20), but the toms, which it spreads 1.5 semitones a step where the
/// Drum's tom is chromatic, and 52, 53 and 55, where it is silent (a MIDI file's Chinese cymbal and
/// splash are the Cymbal's, its ride bell the Ride's).
pub const GM: [GmDrum; 17] = [
    GmDrum { sound: "kick", notes: &[36], own: 36, name: "Kick" },
    GmDrum { sound: "snare", notes: &[38], own: 38, name: "Snare" },
    GmDrum { sound: "clap", notes: &[39], own: 39, name: "Clap" },
    GmDrum { sound: "hats", notes: &[42, 44, 46], own: 42, name: "Hats" },
    GmDrum { sound: "tom", notes: &[41, 43, 45, 47, 48, 50], own: 45, name: "Toms" },
    GmDrum { sound: "rim", notes: &[37], own: 37, name: "Rim" },
    GmDrum { sound: "cowbell", notes: &[56], own: 56, name: "Cowbell" },
    GmDrum { sound: "cymbal", notes: &[49, 52, 55], own: 49, name: "Cymbal" },
    GmDrum { sound: "shaker", notes: &[69], own: 69, name: "Shaker" },
    GmDrum { sound: "soft kick", notes: &[35], own: 35, name: "Soft kick" },
    GmDrum { sound: "snappy snare", notes: &[40], own: 40, name: "Snappy snare" },
    GmDrum { sound: "ride", notes: &[51, 53], own: 51, name: "Ride" },
    GmDrum { sound: "tambourine", notes: &[54], own: 54, name: "Tambourine" },
    GmDrum { sound: "high crash", notes: &[57], own: 57, name: "High crash" },
    GmDrum { sound: "high ride", notes: &[59], own: 59, name: "High ride" },
    GmDrum { sound: "high shaker", notes: &[70], own: 70, name: "High shaker" },
    GmDrum { sound: "claves", notes: &[75], own: 75, name: "Claves" },
];

/// The Drum's sound that answers to General MIDI drum note `note`, if any (KIT-6).
pub fn sound_of(note: u8) -> Option<&'static str> {
    GM.iter().find(|g| g.notes.contains(&note)).map(|g| g.sound)
}

/// The General MIDI drum notes `sound` answers to (KIT-6); none for a sound the Drum hasn't got.
pub fn notes_of(sound: &str) -> &'static [u8] {
    GM.iter().find(|g| g.sound == sound).map_or(&[], |g| g.notes)
}

/// The note music2 writes for `sound` (KIT-6).
pub fn own_note(sound: &str) -> Option<u8> {
    GM.iter().find(|g| g.sound == sound).map(|g| g.own)
}

/// A drum machine's notes, split by sound (KIT-7): a lane for each sound they use, in the Drum's
/// menu order, and those no sound answers to.
#[derive(Clone, Debug, PartialEq)]
pub struct Split {
    pub lanes: Vec<(&'static str, Vec<Note>)>,
    pub left: Vec<Note>,
}

/// The General MIDI key a note plays on the drum machine: its pitch, rounded.
fn key_of(n: &Note) -> u8 {
    n.pitch.round().clamp(0.0, 127.0) as u8
}

/// Split one lane of the drum machine's notes by the sound that answers to each (KIT-7), each note
/// as it was, so the hats keep closed, pedal and open, and the toms their pitches. Never drops one:
/// those no sound answers to come back apart, in order.
pub fn split(notes: &[Note]) -> Split {
    let lanes = GM
        .iter()
        .map(|g| (g.sound, notes.iter().filter(|n| g.notes.contains(&key_of(n))).cloned().collect::<Vec<_>>()))
        .filter(|(_, mine)| !mine.is_empty())
        .collect();
    Split { lanes, left: notes.iter().filter(|n| sound_of(key_of(n)).is_none()).cloned().collect() }
}

/// General MIDI's drums: its percussion notes, 35 to 81.
const GENERAL_MIDI: std::ops::RangeInclusive<u8> = 35..=81;

/// The General MIDI drum note a Drum playing `sound` writes for its note `key` (KIT-9): one of the
/// sound's notes as it is; a General MIDI drum no sound answers to as it is, already that drum (a
/// Percussion track's bongos); another sound's note, or one past General MIDI's drums, as the
/// sound's own, or for a tom the tom note nearest it (the lower at an equal distance).
pub fn general_midi(sound: &str, key: u8) -> u8 {
    let notes = notes_of(sound);
    match () {
        _ if notes.contains(&key) || (sound_of(key).is_none() && GENERAL_MIDI.contains(&key)) => key,
        _ if sound == "tom" => notes.iter().copied().min_by_key(|n| (n.abs_diff(key), *n)).unwrap_or(key),
        _ => own_note(sound).unwrap_or(key),
    }
}

/// A kit an instrument names (KIT-3), as + Track's Drum kit lists it: a preset a new kit is made
/// from, not a kit of the song ([`crate::kits::DrumKit`]).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct KitPreset {
    /// Its name: "808 kit".
    pub name: String,
    /// The instrument its tracks play, by ComponentId (`@m2/drums:drum`), and what it is called.
    pub component: String,
    pub instrument: String,
    /// Its tracks' names, in order.
    pub tracks: Vec<String>,
    /// The pack it is from, with its version; none for std's and the project's (PACK-11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pack: Option<crate::packs::From>,
}

/// Every kit the instruments + Track lists name, in the list's order, each in the order its
/// instrument names them.
pub fn presets(s: &Session) -> Vec<KitPreset> {
    let catalog = s.catalog();
    tracks::instruments(s)
        .into_iter()
        .filter_map(|i| catalog.components.get(&i.component).map(|d| (i, d)))
        .flat_map(|(i, d)| {
            d.editor.kits.iter().map(move |k| KitPreset {
                name: k.name.clone(),
                component: i.component.clone(),
                instrument: i.label.clone(),
                tracks: k.tracks.iter().map(|t| t.name.clone()).collect(),
                pack: i.pack.clone(),
            })
        })
        .collect()
}

/// Add kit `kit` of instrument `component` (KIT-5): a Drums bus (a Group, RENDER-34, its fader at the kit's `db`) and
/// the kit's tracks, each playing the instrument with the kit's values from an empty lane of its own and
/// going to the bus, after track `after` (else last), the bus first. One Tx, one undo step.
/// Refused in words when the lists offer no such instrument, or it names no such kit.
pub fn add_kit(s: &mut Session, component: &str, kit: &str, after: Option<&str>) -> Result<Vec<Made>, EditError> {
    let listed = tracks::instruments(s).into_iter().find(|i| i.component == component);
    let inst = listed.ok_or_else(|| EditError::NotFound(format!("{component}: no instrument the lists offer is called that")))?;
    let d = s.catalog().components[component].clone();
    let Some(k) = d.editor.kits.iter().find(|k| k.name == kit) else {
        let names: Vec<String> = d.editor.kits.iter().map(|k| k.name.clone()).collect();
        return Err(EditError::Rejected(match names.len() {
            0 => format!("{} has no kits.", inst.label),
            1 => format!("{} has no kit called {kit}: its kit is {}.", inst.label, names[0]),
            _ => format!("{} has no kit called {kit}: its kits are {}.", inst.label, crate::listed(&names)),
        }));
    };
    let made = k.tracks.iter().map(|t| NewTrack {
        play: Play::Instrument(component.to_string()),
        label: Some(t.name.clone()),
        values: t.values.clone(),
        to: Some(Goes::New(0)),
        ..NewTrack::default()
    });
    tracks::add_tracks(s, &std::iter::once(kit_bus(k.db.clone())).chain(made).collect::<Vec<_>>(), after, None)
}

/// A kit's bus (KIT-5): a Group called Drums, as Add bus writes one, its channel's fader at `db`
/// when the kit gives one (KIT-4).
pub(crate) fn kit_bus(db: Option<Spelled>) -> NewTrack {
    NewTrack { play: Play::Bus(BusKind::Group), label: Some(BUS.into()), db, ..NewTrack::default() }
}

/// A kit of the Drum for `notes`, a drum machine's (KIT-8): a Drums bus at 0 dB, as the drum
/// machine's channel was, at place `bus` of the list it goes in, and a Drum track for each sound
/// they use, by [`split`], named as the table words it and
/// going to the bus, with one clip of that sound's notes (`clip` makes it); the notes no sound
/// answers to on Percussion, which plays the tom, each at its own pitch.
pub(crate) fn kit_of(
    notes: &[Note],
    bus: usize,
    clip: impl Fn(Vec<Note>) -> Result<serde_json::Value, EditError>,
) -> Result<Vec<NewTrack>, EditError> {
    let Split { lanes, left } = split(notes);
    let mut out = vec![kit_bus(None)];
    let named = |sound: &'static str| GM.iter().find(|g| g.sound == sound).map_or(sound, |g| g.name);
    let lanes = lanes.into_iter().map(|(sound, mine)| (named(sound), sound, mine));
    let left = (!left.is_empty()).then_some(("Percussion", "tom", left));
    for (label, sound, mine) in lanes.chain(left) {
        let place = GM.iter().position(|g| g.sound == sound).expect("a sound of the table") as f64;
        let menu = Insert { slot: "drum".into(), value: Spelled { value: place, spelled: None, written: place } };
        out.push(NewTrack {
            play: Play::Instrument(DRUM.into()),
            label: Some(label.to_string()),
            clips: vec![clip(mine)?],
            values: vec![menu],
            to: Some(Goes::New(bus)),
            ..NewTrack::default()
        });
    }
    Ok(out)
}

/// The sound each track's Drum plays, by its key (KIT-9), as a song's kits say it
/// ([`crate::kits::drum`]): the name its menu holds, a value the code writes or leaves out. None for
/// a track whose Drum's sound the code works out (a lane, a signal), that plays no Drum, or two, or
/// whose Drum plays another track's lane.
pub(crate) fn sounds_played(s: &Session) -> BTreeMap<String, &'static str> {
    let rev = s.accepted();
    rev.compiled
        .ir
        .modules
        .iter()
        .filter_map(|m| crate::kits::drum(rev, &m.key))
        .filter_map(|r| Some((r.track, GM.iter().find(|g| Some(g.sound) == r.sound.as_deref())?.sound)))
        .collect()
}

#[cfg(test)]
#[path = "drums_tests.rs"]
mod tests;
