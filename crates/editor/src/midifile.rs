//! MIDI files and songs (studio lane C, gaps #36): the sound of music2's own packs (PACK-15) a MIDI
//! part plays and the program a track is written with (Q33, one table for both ways), a song or one
//! clip written as a MIDI
//! file for other music apps (Q37), and a MIDI file brought in as parts, its tempo map and its time
//! signatures (Q32–Q36, D10, D11).
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
use m2_song::{Arrangement, Meter, Note, NoteClip, Tempo};

use crate::EditError;
use crate::smf::{self, Event, Smf, SmfNote, SmfTrack};
use crate::timeline::{self, TimelineLane};

/// Ticks a beat in the files music2 writes.
pub const TICKS: u16 = 960;
/// What a part on channel 10 plays: General MIDI's drums, as `@m2/drums` names its notes.
pub const DRUM_KIT: &str = "@m2/drums:drum-kit";
/// Channel 10, as files count channels from 0.
const DRUM_CHANNEL: u8 = 9;

/// A sound of music2's own packs (PACK-15): the General MIDI programs that pick it (Q33), and the
/// one it is written with.
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
    Sound { component: "@m2/synths:fm-keys", picked_by: &[(0, 7)], writes: 4 },
    // Tubular Bells.
    Sound { component: "@m2/synths:fm-bell", picked_by: &[(8, 15)], writes: 14 },
    // Pad 2 (warm).
    Sound { component: "@m2/synths:synth-pad", picked_by: &[(16, 23), (88, 103), (120, 127)], writes: 89 },
    // Acoustic Guitar (steel).
    Sound { component: "@m2/synths:synth-pluck", picked_by: &[(24, 31), (104, 111)], writes: 25 },
    // Synth Bass 1.
    Sound { component: "@m2/synths:synth-bass", picked_by: &[(32, 39)], writes: 38 },
    // String Ensemble 1.
    Sound { component: "@m2/synths:synth-strings", picked_by: &[(40, 55)], writes: 48 },
    // Synth Brass 1.
    Sound { component: "@m2/synths:synth-supersaw", picked_by: &[(56, 63)], writes: 62 },
    // Lead 2 (sawtooth).
    Sound { component: "@m2/synths:synth-lead", picked_by: &[(64, 87)], writes: 81 },
    // Steel Drums.
    Sound { component: "@m2/synths:fm-metal", picked_by: &[(112, 119)], writes: 114 },
    // @m2/synths' simple sounds: written as their nearest, never picked.
    Sound { component: "@m2/synths:bass", picked_by: &[], writes: 38 },
    Sound { component: "@m2/synths:pluck", picked_by: &[], writes: 25 },
    Sound { component: "@m2/synths:pad", picked_by: &[], writes: 89 },
    Sound { component: "@m2/synths:synth-sub", picked_by: &[], writes: 38 },
];

/// @m2/synths' simple drums, which have no category: their lanes are drums too.
const PLAIN_DRUMS: &[&str] = &["@m2/synths:kick", "@m2/synths:hat"];

/// The sound a part plays (Q33): channel 10 plays the Drum kit; else its program's family;
/// with no program, Synth bass when its notes average below C3, else FM keys.
pub fn sound_for(drums: bool, program: Option<u8>, average_pitch: f64) -> &'static str {
    if drums {
        return DRUM_KIT;
    }
    match program {
        Some(p) => {
            SOUNDS.iter().find(|s| s.picked_by.iter().any(|&(lo, hi)| (lo..=hi).contains(&p))).map_or("@m2/synths:fm-keys", |s| s.component)
        }
        None if average_pitch < 48.0 => "@m2/synths:synth-bass",
        None => "@m2/synths:fm-keys",
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

/// Whether `component` plays drums: the Drum kit, any of the Drums group, @m2/synths' simple kick and hat.
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

// ---------------------------------------------------------------- bringing a MIDI file in

/// The most parts a file may bring in (Q32).
pub const MAX_PARTS: usize = 32;
/// The last bar a note may reach (D10).
pub const LAST_BAR: u64 = 9_999;
/// The shortest note brought in, in beats (D10).
pub const SHORTEST: f64 = 1.0 / 64.0;

/// A part of a MIDI file: one track's notes on one channel (Q32), what becomes a track.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    /// The track's name, or the sound's label when the track has none or makes several parts.
    pub label: String,
    /// The sound it plays (Q33).
    pub component: String,
    /// Its notes, in beats from the file's start, by start.
    pub notes: Vec<Note>,
}

impl Part {
    /// Where its last note ends, in beats from the file's start.
    pub fn end(&self) -> f64 {
        self.notes.iter().map(|n| n.start + n.length).fold(0.0, f64::max)
    }
}

/// A MIDI file as music2 brings it in: its parts, in file order, and its tempo map and time
/// signatures as an arrangement (in range, D10).
#[derive(Clone, Debug, PartialEq)]
pub struct Brought {
    pub parts: Vec<Part>,
    pub arrangement: Arrangement,
}

/// A tempo as the file says it, in range (D10) and to a hundredth of a BPM: 483871 µs a beat is
/// 124 BPM, as music2 wrote it.
fn bpm_of(us: u32) -> f64 {
    let bpm = 60_000_000.0 / us.max(1) as f64;
    ((bpm * 100.0).round() / 100.0).clamp(20.0, 999.0)
}

/// The file's tempo map and time signatures (Q35, D10): every track's, by tick, the last at one
/// tick winning. No tempo is 120 (SMF's); none at the start is 120 until the first. A meter
/// music2 can't hold (1 to 64 beats over 1 to 32) or not on a bar line of the one before is
/// skipped; with none at the start, 4/4.
fn arrangement_of(file: &Smf) -> Arrangement {
    let ppq = file.ppq.max(1) as f64;
    let mut tempos: Vec<(u64, f64)> = vec![];
    let mut meters: Vec<(u64, Meter)> = vec![];
    let mut events: Vec<(u64, Event)> = file.tracks.iter().flat_map(|t| t.events.iter().copied()).collect();
    events.sort_by_key(|e| e.0);
    for (tick, e) in events {
        match e {
            Event::Tempo(us) => match tempos.last_mut() {
                Some(last) if last.0 == tick => last.1 = bpm_of(us),
                _ => tempos.push((tick, bpm_of(us))),
            },
            Event::Meter { beats, unit_pow } if (1..=64).contains(&beats) && unit_pow <= 5 => {
                let m = Meter { beat: tick as f64 / ppq, beats: beats as u32, unit: 1 << unit_pow };
                match meters.last_mut() {
                    Some(last) if last.0 == tick => last.1 = m,
                    _ => meters.push((tick, m)),
                }
            }
            _ => {}
        }
    }
    let mut tempo: Vec<Tempo> = vec![];
    for (tick, bpm) in tempos {
        let beat = tick as f64 / ppq;
        if tempo.is_empty() && tick > 0 {
            tempo.push(Tempo { beat: 0.0, bpm: 120.0 });
        }
        if tempo.last().is_none_or(|t| t.bpm != bpm) {
            tempo.push(Tempo { beat, bpm });
        }
    }
    if tempo.is_empty() {
        tempo.push(Tempo { beat: 0.0, bpm: 120.0 });
    }
    let mut meter: Vec<Meter> = vec![];
    for (_, m) in meters {
        let prev = meter.last().copied().unwrap_or(Meter::COMMON);
        if meter.is_empty() && m.beat == 0.0 {
            meter.push(m);
            continue;
        }
        // On a bar line of the one before, put exactly there; else skipped.
        let bars = (m.beat - prev.beat) / prev.bar();
        if (bars - bars.round()).abs() > 1e-6 || bars.round() < 1.0 || (m.beats == prev.beats && m.unit == prev.unit) {
            continue;
        }
        if meter.is_empty() {
            meter.push(Meter::COMMON);
        }
        meter.push(Meter { beat: prev.beat + bars.round() * prev.bar(), ..m });
    }
    if meter.len() == 1 && meter[0] == Meter::COMMON {
        meter.clear();
    }
    Arrangement { tempo, meter }
}

/// Bring `file`, called `name` ("groove.mid"), in as parts (Q32–Q34, D10): every pair of a track
/// and a channel with notes, in file order (a format 0 file splits by channel). The name: the
/// track's, else the sound's label (`label_of` a component), a repeated one numbered. Notes: beats
/// by the file's division, velocity `v / 127` (to a thousandth), at least 1/64 beat. Refused in words: no notes, over
/// 32 parts, a note past bar 9,999.
pub fn bring(file: &Smf, name: &str, label_of: &dyn Fn(&str) -> String) -> Result<Brought, String> {
    let arrangement = arrangement_of(file);
    let ppq = file.ppq.max(1) as f64;
    let mut parts: Vec<Part> = vec![];
    for t in &file.tracks {
        let notes = t.notes();
        // The channels in the order their first notes come.
        let mut channels: Vec<u8> = vec![];
        for n in &notes {
            if !channels.contains(&n.channel) {
                channels.push(n.channel);
            }
        }
        for &ch in &channels {
            let mine: Vec<&SmfNote> = notes.iter().filter(|n| n.channel == ch).collect();
            // Its program: the first this track sets on the channel, else the first any track does.
            let program = |events: &[(u64, Event)]| {
                events.iter().find_map(|e| match e.1 {
                    Event::Program { channel, program } if channel == ch => Some(program),
                    _ => None,
                })
            };
            let program = program(&t.events).or_else(|| file.tracks.iter().find_map(|o| program(&o.events)));
            let average = mine.iter().map(|n| n.key as f64).sum::<f64>() / mine.len() as f64;
            let component = sound_for(ch == DRUM_CHANNEL, program, average).to_string();
            let named = t.name.clone().filter(|n| !n.is_empty() && channels.len() == 1);
            let notes = mine
                .iter()
                .map(|n| Note {
                    pitch: n.key as f64,
                    start: n.start as f64 / ppq,
                    length: ((n.end - n.start) as f64 / ppq).max(SHORTEST),
                    // To a thousandth: a tidy file, and `round(v × 127)` gives the same velocity back.
                    velocity: (n.velocity as f64 / 127.0 * 1000.0).round() / 1000.0,
                })
                .collect();
            parts.push(Part { label: named.unwrap_or_else(|| label_of(&component)), component, notes });
        }
    }
    if parts.is_empty() {
        return Err(format!("{name} has no notes."));
    }
    if parts.len() > MAX_PARTS {
        return Err(format!("{name} has {} parts: music2 brings in {MAX_PARTS} at most.", parts.len()));
    }
    if parts.iter().any(|p| arrangement.bar_at(p.end()).0 >= LAST_BAR) {
        return Err(format!("{name} has a note past bar {}: music2 brings in notes up to there.", "9,999"));
    }
    // A name twice: "Piano", "Piano 2".
    let mut seen: Vec<String> = vec![];
    for p in &mut parts {
        let base = p.label.clone();
        let mut n = 1;
        while seen.iter().any(|s| s.eq_ignore_ascii_case(&p.label)) {
            n += 1;
            p.label = format!("{base} {n}");
        }
        seen.push(p.label.clone());
    }
    Ok(Brought { parts, arrangement })
}

/// Where a part's clip ends, `from` its start at beat `at`: the bar line at or after its last note
/// (at least a bar on), by `a`'s time signatures.
pub fn clip_length(a: &Arrangement, at: f64, end: f64) -> f64 {
    let last = at + end;
    let (_, into) = a.bar_at(last);
    let line = if into <= 1e-9 { last - into } else { last - into + a.meter_at(last).bar() };
    (line - at).max(a.meter_at(at).bar())
}

/// What the tempo question answered (Q35): keep the song's tempo, or use the file's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TempoChoice {
    Keep,
    Use,
}

/// The tempo question (Q35): the file's first tempo and the song's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Asks {
    pub file_bpm: f64,
    pub song_bpm: f64,
}

/// What a MIDI import from beat `at` writes into `data/arrangement.json` (Q35): into an empty song
/// (no lane has a clip), the file's tempo map and time signatures from `at` on; else nothing when
/// the tempos are the same or the answer is Keep, the file's tempo map from `at` on (the song's
/// time signatures kept) when it is Use, and the question when there is no answer yet. Before
/// `at`, the song's tempo stays.
pub fn tempo_plan(
    song_empty: bool,
    song: &Arrangement,
    file: &Arrangement,
    at: f64,
    choice: Option<TempoChoice>,
) -> Result<Option<Arrangement>, Asks> {
    let moved = from_beat(song, file, at, song_empty);
    if song_empty {
        return Ok((moved != *song).then_some(moved));
    }
    if moved.tempo == song.tempo {
        return Ok(None);
    }
    match choice {
        None => Err(Asks { file_bpm: file.tempo[0].bpm, song_bpm: song.tempo_map().bpm_at(at) }),
        Some(TempoChoice::Keep) => Ok(None),
        Some(TempoChoice::Use) => Ok(Some(moved)),
    }
}

/// The song's tempo map before beat `at` and the file's from it on; with `meters`, the file's time
/// signatures from `at` too (when `at` is on one of the song's bar lines), else the song's.
fn from_beat(song: &Arrangement, file: &Arrangement, at: f64, meters: bool) -> Arrangement {
    let mut tempo: Vec<Tempo> = song.tempo.iter().filter(|t| t.beat < at).copied().collect();
    for t in &file.tempo {
        if tempo.last().is_none_or(|l| l.bpm != t.bpm) {
            tempo.push(Tempo { beat: t.beat + at, bpm: t.bpm });
        }
    }
    let on_line = song.bar_at(at).1.abs() < 1e-9;
    if !meters || !on_line {
        return Arrangement { tempo, meter: song.meter.clone() };
    }
    let mut meter: Vec<Meter> = song.meter.iter().filter(|m| m.beat < at).copied().collect();
    let theirs = if file.meter.is_empty() { vec![Meter::COMMON] } else { file.meter.clone() };
    for m in theirs {
        let m = Meter { beat: m.beat + at, ..m };
        let before = meter.last().copied().unwrap_or(Meter::COMMON);
        if (before.beats, before.unit) == (m.beats, m.unit) {
            continue;
        }
        if meter.is_empty() && m.beat > 0.0 {
            meter.push(Meter::COMMON);
        }
        meter.push(m);
    }
    if meter == [Meter::COMMON] {
        meter.clear();
    }
    Arrangement { tempo, meter }
}

/// The sounds' labels, as + Track lists them, by component.
fn labels(s: &Session) -> impl Fn(&str) -> String + use<> {
    let list: Vec<(String, String)> = crate::tracks::instruments(s).into_iter().map(|i| (i.component, i.label)).collect();
    move |c: &str| {
        list.iter()
            .find(|(id, _)| id == c)
            .map_or_else(|| m2_song::pack::split_id(c).map_or(c, |(_, id)| id).to_string(), |(_, l)| l.clone())
    }
}

/// Bring a one-part MIDI file in onto notes lane `lane` of track `module`, at beat `at` (Q36, D11):
/// one clip from `at` to the bar line after its last note, by the song's tempo and time signatures
/// (its notes go by beats; no question). One `Clips` Tx, one undo step. Refused in words when the
/// file has more than one part, or the lane isn't a notes lane.
pub fn import_into(
    s: &mut Session,
    file: &Smf,
    name: &str,
    module: &str,
    lane: &str,
    at: f64,
) -> Result<crate::clips::ClipsEdited, EditError> {
    if !(at >= 0.0 && at.is_finite()) {
        return Err(EditError::Rejected(format!("can't bring {name} in at beat {at}")));
    }
    let brought = bring(file, name, &labels(s)).map_err(EditError::Rejected)?;
    let view = timeline::timeline(s);
    let t = view.tracks.iter().find(|t| t.module == module).ok_or_else(|| EditError::Missing("That track isn't there any more.".into()))?;
    if !matches!(t.lanes.iter().find(|l| l.key() == lane), Some(TimelineLane::Notes { .. })) {
        return Err(EditError::Rejected(format!("{} has no notes lane {lane} to bring {name} into.", t.label)));
    }
    let [part] = brought.parts.as_slice() else {
        return Err(EditError::Rejected(format!(
            "{name} has {} parts: drop it below the tracks to bring each in as a track of its own.",
            brought.parts.len()
        )));
    };
    let length = clip_length(&s.accepted().project.arrangement, at, part.end());
    let clip = NoteClip { id: None, other_id: None, start: at, length, notes: part.notes.clone() };
    let lane = m2_song::LaneAddr {
        module: m2_song::Key::new(module).map_err(EditError::Rejected)?,
        lane: m2_song::Key::new(lane).map_err(EditError::Rejected)?,
    };
    let put = serde_json::to_value(&clip).map_err(|e| EditError::Io(e.to_string()))?;
    crate::clips::edit(s, &crate::clips::Gesture::Set { lane, put: vec![put], delete: vec![] })
}

/// Where a MIDI file's new tracks go (Q36): after track `after` (else at the end), their clips
/// from beat `at`.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub after: Option<String>,
    pub at: f64,
}

/// New tracks a MIDI file made: the edit, each track's key and name, and their clips (to select).
#[derive(Clone, Debug, PartialEq)]
pub struct TracksMade {
    pub edited: crate::Edited,
    pub tracks: Vec<(String, String)>,
    pub made: Vec<(m2_song::LaneAddr, String)>,
}

/// What [`import_tracks`] did: asked the tempo question (nothing written), or made the tracks.
#[derive(Clone, Debug, PartialEq)]
pub enum Imported {
    Asks(Asks),
    Tracks(TracksMade),
}

/// Bring a MIDI file in as new tracks at `place` (Q32 to Q36, D10): [`bring`], then
/// [`tempo_plan`] from the place's beat; each part a notes track playing its sound, with one clip
/// from that beat to the bar line after its last note; all of it one `tracks::add_tracks` Tx, one
/// undo step, the tempo in it. Answers the question, writing nothing, when it is due and `choice`
/// is None. Refused in words, nothing written: no notes, over 32 parts, a note past bar 9,999, the
/// code doesn't compile.
pub fn import_tracks(s: &mut Session, file: &Smf, name: &str, place: &Place, choice: Option<TempoChoice>) -> Result<Imported, EditError> {
    let at = place.at;
    if !(at >= 0.0 && at.is_finite()) {
        return Err(EditError::Rejected(format!("can't bring {name} in at beat {at}")));
    }
    if !s.draft_errors.is_empty() {
        return Err(EditError::Rejected("The code doesn't compile, so no track can be added now. Fix the code first.".into()));
    }
    let brought = bring(file, name, &labels(s)).map_err(EditError::Rejected)?;
    let rev = s.accepted();
    let song = &rev.project.arrangement;
    let empty = rev.compiled.data.lanes().all(|(_, d)| d.ids().is_empty());
    let arrangement = match tempo_plan(empty, song, &brought.arrangement, at, choice) {
        Ok(a) => a,
        Err(asks) => return Ok(Imported::Asks(asks)),
    };
    let timing = arrangement.as_ref().unwrap_or(song);
    if brought.parts.iter().any(|p| timing.bar_at(at + p.end()).0 >= LAST_BAR) {
        return Err(EditError::Rejected(format!("{name} has a note past bar 9,999: music2 brings in notes up to there.")));
    }
    let mut new = vec![];
    for p in &brought.parts {
        let clip = NoteClip { id: None, other_id: None, start: at, length: clip_length(timing, at, p.end()), notes: p.notes.clone() };
        let clip = serde_json::to_value(&clip).map_err(|e| EditError::Io(e.to_string()))?;
        new.push(crate::tracks::NewTrack {
            play: crate::tracks::Play::Instrument(p.component.clone()),
            label: Some(p.label.clone()),
            key: None,
            clips: vec![clip],
        });
    }
    let written = arrangement.map(|a| a.canonical());
    let made = crate::tracks::add_tracks(s, &new, place.after.as_deref(), written)?;
    let data = &s.accepted().compiled.data;
    let mut clips = vec![];
    for m in &made {
        for (addr, lane) in data.lanes().filter(|(a, _)| a.module.as_str() == m.module) {
            clips.extend(lane.ids().into_iter().flatten().map(|id| (addr.clone(), id.to_string())));
        }
    }
    let edited = made[0].edited.clone();
    Ok(Imported::Tracks(TracksMade { edited, tracks: made.into_iter().map(|m| (m.module, m.label)).collect(), made: clips }))
}

#[cfg(test)]
#[path = "midifile_tests.rs"]
mod tests;
