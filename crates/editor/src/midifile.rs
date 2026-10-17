//! MIDI files and songs (studio lane C, gaps #36): the std sound a MIDI part plays and the program
//! a track is written with (Q33, one table for both ways), and a song or one clip written as a MIDI
//! file for other music apps (Q37).
//!
//! A song's file is format 1 at 960 ticks a beat. Its first track holds the song's name, its tempo
//! map and its time signatures. Then one track for each notes lane with notes, in the tracks'
//! order: named by its track ("Bass"; "Bass · lead" for a track's second lane), with a program
//! change from [`program_for`] when the table knows its sound, on channel 10 for a drum's lane and
//! on channels 1 to 9 and 11 to 16 in order for the rest (the 17th on share 16). Notes are cut at
//! their clips' edges, as they play; a velocity becomes `round(v × 127)` in 1 to 127, a tuned pitch
//! the nearest key. Two notes of one pitch that overlap: the first ends where the second starts
//! (D37). Tracks muted in the song are left out, as the audio export leaves them out.

use m2_ir::{Node, Op, split_ref};
use m2_script::component::PortKind;
use m2_session::{Revision, Session};
use m2_song::{Arrangement, NoteClip};

use crate::smf::{self, Event, Smf, SmfTrack};
use crate::timeline::{self, TimelineLane};
use crate::EditError;

/// Ticks a beat in the files music2 writes.
pub const TICKS: u16 = 960;
/// What a part on channel 10 plays: General MIDI's drums, as `std/drums` names its notes.
pub const DRUM_KIT: &str = "std:drum-kit";
/// Channel 10, as files count channels from 0.
const DRUM_CHANNEL: u8 = 9;

/// A std sound: the General MIDI programs that pick it (Q33), and the one it is written with.
struct Sound {
    component: &'static str,
    /// First and last, both in.
    picked_by: &'static [(u8, u8)],
    writes: u8,
}

/// Q33's table. A program picks the sound whose ranges hold it; a track is written with its
/// sound's own program, a fitting one of General MIDI's.
const SOUNDS: &[Sound] = &[
    // Electric Piano 1.
    Sound { component: "std:fm-keys", picked_by: &[(0, 7)], writes: 4 },
    // Tubular Bells.
    Sound { component: "std:fm-bell", picked_by: &[(8, 15)], writes: 14 },
    // Pad 2 (warm).
    Sound { component: "std:synth-pad", picked_by: &[(16, 23), (88, 103), (120, 127)], writes: 89 },
    // Acoustic Guitar (steel).
    Sound { component: "std:synth-pluck", picked_by: &[(24, 31), (104, 111)], writes: 25 },
    // Synth Bass 1.
    Sound { component: "std:synth-bass", picked_by: &[(32, 39)], writes: 38 },
    // String Ensemble 1.
    Sound { component: "std:synth-strings", picked_by: &[(40, 55)], writes: 48 },
    // Synth Brass 1.
    Sound { component: "std:synth-supersaw", picked_by: &[(56, 63)], writes: 62 },
    // Lead 2 (sawtooth).
    Sound { component: "std:synth-lead", picked_by: &[(64, 87)], writes: 81 },
    // Steel Drums.
    Sound { component: "std:fm-metal", picked_by: &[(112, 119)], writes: 114 },
    // std/instruments' simple sounds: written as their nearest, never picked.
    Sound { component: "std:bass", picked_by: &[], writes: 38 },
    Sound { component: "std:pluck", picked_by: &[], writes: 25 },
    Sound { component: "std:pad", picked_by: &[], writes: 89 },
    Sound { component: "std:synth-sub", picked_by: &[], writes: 38 },
];

/// std/instruments' drums, which have no category: their lanes are drums too.
const PLAIN_DRUMS: &[&str] = &["std:kick", "std:hat"];

/// The std sound a part plays (Q33): channel 10 plays the Drum kit; else its program's family;
/// with no program, Synth bass when its notes average below C3, else FM keys.
pub fn sound_for(drums: bool, program: Option<u8>, average_pitch: f64) -> &'static str {
    if drums {
        return DRUM_KIT;
    }
    match program {
        Some(p) => SOUNDS.iter().find(|s| s.picked_by.iter().any(|&(lo, hi)| (lo..=hi).contains(&p))).map_or("std:fm-keys", |s| s.component),
        None if average_pitch < 48.0 => "std:synth-bass",
        None => "std:fm-keys",
    }
}

/// The General MIDI program a track playing `component` is written with: none for a sound the
/// table doesn't know (D37).
pub fn program_for(component: &str) -> Option<u8> {
    SOUNDS.iter().find(|s| s.component == component).map(|s| s.writes)
}

/// Every node of `nodes`, voices included.
fn each<'a>(nodes: &'a [Node], out: &mut Vec<&'a Node>) {
    for n in nodes {
        out.push(n);
        if let Op::Poly { body, .. } = &n.op {
            each(&body.nodes, out);
        }
    }
}

/// The instrument that plays lane `lane` of `module`: the outermost component that takes notes and
/// makes sound, of the frames the lane's readers were made in. None when nothing reads it, or
/// when its readers are of different instruments.
pub fn player_of(rev: &Revision, module: &str, lane: &str) -> Option<String> {
    let compiled = &rev.compiled;
    let m = compiled.ir.modules.iter().find(|m| m.key == module)?;
    let lanes: Vec<&str> = m.nodes.iter().filter(|n| matches!(&n.op, Op::Lane { lane: l } if l == lane)).map(|n| n.id.as_str()).collect();
    let reads = |r: &str| lanes.contains(&split_ref(r).0);
    let instrument = |id: &str| {
        compiled.components.get(id).is_some_and(|d| {
            d.inputs.first().is_some_and(|i| i.kind == PortKind::Notes) && d.output.as_ref().is_some_and(|o| o.kind == PortKind::Audio)
        })
    };
    let mut nodes = vec![];
    each(&m.nodes, &mut nodes);
    let players: Vec<Option<&str>> = nodes
        .into_iter()
        .filter(|n| match &n.op {
            Op::Poly { events, .. } => reads(events),
            Op::Kernel { inputs, .. } | Op::Plugin { inputs, .. } => inputs.values().any(|r| reads(r)),
            _ => false,
        })
        .map(|n| m.chain(n.frame).into_iter().find_map(|f| f.component.as_deref().filter(|id| instrument(id))))
        .collect();
    match players.first() {
        Some(Some(id)) if players.iter().all(|p| p == &Some(*id)) => Some(id.to_string()),
        _ => None,
    }
}

/// Whether `component` plays drums: the Drum kit, any of std's Drums, std/instruments' kick and hat.
fn is_drums(rev: &Revision, component: &str) -> bool {
    PLAIN_DRUMS.contains(&component)
        || rev.compiled.components.get(component).is_some_and(|d| d.editor.category.as_deref() == Some("Drums"))
}

/// A beat as a tick of [`TICKS`].
fn tick(beat: f64) -> u64 {
    (beat * TICKS as f64).round().max(0.0) as u64
}

/// The tempo map and time signatures from `from` on, moved to start at tick 0: the tempo and the
/// meter there first, then each change after it.
fn tempo_events(a: &Arrangement, from: f64) -> Vec<(u64, Event)> {
    let mut out = vec![];
    let us = |bpm: f64| (60_000_000.0 / bpm).round() as u32;
    out.push((0, Event::Tempo(us(a.tempo_map().bpm_at(from)))));
    out.extend(a.tempo.iter().filter(|t| t.beat > from).map(|t| (tick(t.beat - from), Event::Tempo(us(t.bpm)))));
    let meter = |m: m2_song::Meter| Event::Meter { beats: m.beats.min(255) as u8, unit_pow: m.unit.trailing_zeros() as u8 };
    out.push((0, meter(a.meter_at(from))));
    out.extend(a.meter.iter().filter(|m| m.beat > from).map(|m| (tick(m.beat - from), meter(*m))));
    out
}

/// The MIDI events of `clips`' notes as they play, on `channel`, moved `from` beats earlier.
fn note_events(clips: &[NoteClip], channel: u8, from: f64) -> Vec<(u64, Event)> {
    let mut notes: Vec<(u8, u64, u64, u8)> = m2_song::lane::notes(clips)
        .into_iter()
        .map(|n| {
            let key = n.pitch.round().clamp(0.0, 127.0) as u8;
            let start = tick(n.start - from);
            let velocity = (n.velocity * 127.0).round().clamp(1.0, 127.0) as u8;
            (key, start, tick(n.end - from).max(start + 1), velocity)
        })
        .collect();
    // By key, then start: a note still sounding when its key is struck again ends there.
    notes.sort_by_key(|n| (n.0, n.1));
    let mut out = vec![];
    for (i, &(key, start, end, velocity)) in notes.iter().enumerate() {
        let end = match notes.get(i + 1) {
            Some(&(k, next, _, _)) if k == key && next < end => next,
            _ => end,
        };
        // A note struck twice at one tick sounds once.
        if end <= start {
            continue;
        }
        out.push((start, Event::NoteOn { channel, key, velocity }));
        out.push((end, Event::NoteOff { channel, key }));
    }
    out.sort_by_key(|e| e.0);
    out
}

/// The events of one lane's MIDI track: its program change, then its notes.
fn lane_track(name: String, program: Option<u8>, channel: u8, clips: &[NoteClip], from: f64) -> SmfTrack {
    let mut events: Vec<(u64, Event)> = program.map(|program| (0, Event::Program { channel, program })).into_iter().collect();
    events.extend(note_events(clips, channel, from));
    let end = events.last().map_or(0, |e| e.0);
    SmfTrack { name: Some(name), events, end }
}

/// The song's name, from its manifest.
fn song_name(rev: &Revision) -> String {
    rev.project.manifest.project.name.clone()
}

/// The open song as a MIDI file (Q37; see the module's doc). Refused in words when no lane has a
/// note to write.
pub fn export_song(s: &Session) -> Result<Vec<u8>, EditError> {
    let rev = s.accepted();
    let view = timeline::timeline(s);
    let muted = |module: &str| rev.compiled.ir.modules.iter().any(|m| m.key == module && m.muted);
    let mut tracks = vec![SmfTrack { name: Some(song_name(rev)), events: tempo_events(&rev.project.arrangement, 0.0), end: 0 }];
    let mut next = 0u8;
    for t in view.tracks.iter().filter(|t| !muted(&t.module)) {
        let mut first = true;
        for lane in &t.lanes {
            let TimelineLane::Notes { key, label, clips, .. } = lane else { continue };
            let name = if first { t.label.clone() } else { format!("{} · {label}", t.label) };
            first = false;
            if clips.iter().all(|c| c.notes.is_empty()) {
                continue;
            }
            let player = player_of(rev, &t.module, key);
            let drums = player.as_deref().is_some_and(|p| is_drums(rev, p));
            let channel = match drums {
                true => DRUM_CHANNEL,
                // 1 to 9, then 11 to 16; the 17th on share 16.
                false => {
                    let c = if next < DRUM_CHANNEL { next } else { (next + 1).min(15) };
                    next = next.saturating_add(1);
                    c
                }
            };
            let program = player.as_deref().filter(|_| !drums).and_then(program_for);
            let track = lane_track(name, program, channel, clips, 0.0);
            if track.events.iter().any(|e| matches!(e.1, Event::NoteOn { .. })) {
                tracks.push(track);
            }
        }
    }
    if tracks.len() == 1 {
        return Err(EditError::Rejected("The song has no notes to write as a MIDI file.".into()));
    }
    Ok(smf::write(&Smf { format: 1, ppq: TICKS, tracks }))
}

/// A clip, by its track, lane and id.
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
pub struct ClipAt {
    pub module: String,
    pub lane: String,
    pub id: String,
}

/// A clip as a MIDI file (Q37): its notes from tick 0, cut at its edges as they play, with the
/// tempo and meter where it starts and the changes inside it. Also says the track's name, for the
/// file's. Refused in words when the clip isn't a notes clip there.
pub fn export_clip(s: &Session, module: &str, lane: &str, id: &str) -> Result<(Vec<u8>, String), EditError> {
    let rev = s.accepted();
    let view = timeline::timeline(s);
    let missing = || EditError::Missing("That clip isn't there any more.".into());
    let t = view.tracks.iter().find(|t| t.module == module).ok_or_else(missing)?;
    let Some(TimelineLane::Notes { clips, .. }) = t.lanes.iter().find(|l| l.key() == lane) else { return Err(missing()) };
    let clip = clips.iter().rfind(|c| c.id.as_deref() == Some(id)).ok_or_else(missing)?;
    let player = player_of(rev, module, lane);
    let drums = player.as_deref().is_some_and(|p| is_drums(rev, p));
    let channel = if drums { DRUM_CHANNEL } else { 0 };
    let program = player.as_deref().filter(|_| !drums).and_then(program_for);
    let mut notes = lane_track(t.label.clone(), program, channel, std::slice::from_ref(clip), clip.start);
    notes.end = notes.end.max(tick(clip.length));
    let mut tempo = SmfTrack { name: Some(song_name(rev)), events: tempo_events(&rev.project.arrangement, clip.start), end: 0 };
    tempo.events.retain(|e| e.0 < tick(clip.length) || e.0 == 0);
    let bytes = smf::write(&Smf { format: 1, ppq: TICKS, tracks: vec![tempo, notes] });
    Ok((bytes, t.label.clone()))
}

#[cfg(test)]
#[path = "midifile_tests.rs"]
mod tests;
