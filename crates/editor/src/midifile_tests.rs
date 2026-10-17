//! MIDI files out of songs (Q37) and the sound table (Q33), on the fixtures.

use super::*;
use crate::Gestures;
use crate::smf::SmfNote;
use crate::testing::{Fixture, MANIFEST, fixture};

/// One written track as a test reads it: its name, the programs it sets, and its notes.
#[derive(Debug, PartialEq)]
struct Part {
    name: Option<String>,
    programs: Vec<(u8, u8)>,
    notes: Vec<SmfNote>,
}

fn written(bytes: &[u8]) -> (Smf, Vec<Part>) {
    let smf = smf::read(bytes).unwrap();
    assert_eq!((smf.format, smf.ppq), (1, TICKS));
    let parts = smf
        .tracks
        .iter()
        .map(|t| Part {
            name: t.name.clone(),
            programs: t
                .events
                .iter()
                .filter_map(|e| match e.1 {
                    Event::Program { channel, program } => Some((channel, program)),
                    _ => None,
                })
                .collect(),
            notes: t.notes(),
        })
        .collect();
    (smf, parts)
}

fn note(channel: u8, key: u8, velocity: u8, start: u64, end: u64) -> SmfNote {
    SmfNote { channel, key, velocity, start, end }
}

/// A song of one track, `pad`, playing `std/instruments`' pad from lane `chords`, holding `clips`;
/// and any more files.
fn pad_song(clips: &str, more: &[(&str, &str)]) -> Fixture {
    let f = Fixture::new();
    f.write("music2.toml", MANIFEST);
    f.write(
        "song.ts",
        "import { song, track } from '@m2/core';\nimport { pad } from 'std/instruments';\n\
         const p = track('pad', { name: 'Pad' }, ({ lane }) => pad(lane.notes('chords')));\nexport default song(() => p.out);\n",
    );
    f.write("data/pad/chords.json", clips);
    for (name, text) in more {
        f.write(name, text);
    }
    f
}

#[test]
fn the_song_has_a_tempo_track_and_a_track_per_lane() {
    let f = fixture("examples/first-song");
    let s = f.open();
    let (smf, parts) = written(&export_song(&s).unwrap());
    // The song's name, its tempo (124 bpm) and 4/4.
    assert_eq!(parts[0].name.as_deref(), Some("first-song"));
    assert_eq!(smf.tracks[0].events, [(0, Event::Tempo(483_871)), (0, Event::Meter { beats: 4, unit_pow: 2 })]);
    assert!(parts[0].notes.is_empty());
    // A track per notes lane with notes, in the tracks' order; bass's automation lanes aren't.
    let names: Vec<_> = parts[1..].iter().map(|p| p.name.clone().unwrap()).collect();
    assert_eq!(names, ["Kick", "Hats", "Bass", "Keys"]);
    // Kick and Hats are drums, on channel 10 with no program; Bass and Keys on 1 and 2, as their sounds.
    let channels: Vec<Vec<u8>> = parts[1..].iter().map(|p| p.notes.iter().map(|n| n.channel).collect()).collect();
    assert!(channels[0].iter().chain(&channels[1]).all(|&c| c == 9), "{channels:?}");
    assert!(channels[2].iter().all(|&c| c == 0) && channels[3].iter().all(|&c| c == 1));
    assert_eq!(parts[1..].iter().map(|p| p.programs.clone()).collect::<Vec<_>>(), [vec![], vec![], vec![(0, 38)], vec![(1, 25)]]);
    // Every note of the song, as it plays: the bass's first is A1 at 0.8 for half a beat.
    assert_eq!(parts[3].notes[0], note(0, 33, 102, 0, 480));
    let data = &s.accepted().compiled.data;
    let all: usize = data.lanes().map(|(a, _)| data.notes(a.module.as_str(), a.lane.as_str()).len()).sum();
    assert_eq!(parts.iter().map(|p| p.notes.len()).sum::<usize>(), all);
}

#[test]
fn a_second_lane_is_named_by_its_track_and_lane() {
    let f = pad_song(r#"{"clips": [{"start": 0, "length": 4, "notes": [{"pitch": 60, "start": 0, "length": 1}]}]}"#, &[]);
    f.write(
        "song.ts",
        "import { song, track } from '@m2/core';\nimport { pad } from 'std/instruments';\nimport { sum } from 'std/math';\n\
         const p = track('pad', { name: 'Pad' }, ({ lane }) => sum(pad(lane.notes('chords')), pad(lane.notes('lead', { name: 'Top' }))));\n\
         export default song(() => p.out);\n",
    );
    f.write("data/pad/lead.json", r#"{"clips": [{"start": 0, "length": 4, "notes": [{"pitch": 72, "start": 1, "length": 1}]}]}"#);
    let (_, parts) = written(&export_song(&f.open()).unwrap());
    let names: Vec<_> = parts.iter().map(|p| p.name.clone().unwrap()).collect();
    assert_eq!(names, ["t", "Pad", "Pad · Top"]);
    assert_eq!(parts[2].notes, [note(1, 72, 102, 960, 1920)], "channel 2");
}

#[test]
fn a_drum_kit_lane_is_channel_10() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let made = s.add_track("std:drum-kit", Some("Kit"), None, None).unwrap();
    f.write(
        &format!("data/{}/hits.json", made.module),
        r#"{"clips": [{"start": 0, "length": 4, "notes": [{"pitch": 38, "start": 1, "length": 0.25}]}]}"#,
    );
    let s = f.open();
    let (_, parts) = written(&export_song(&s).unwrap());
    let kit = parts.iter().find(|p| p.name.as_deref() == Some("Kit")).unwrap();
    assert_eq!(kit.notes, [note(9, 38, 102, 960, 1200)]);
    assert!(kit.programs.is_empty(), "a drum part has no program");
    // The other parts still count 1, 2 past it.
    let keys = parts.iter().find(|p| p.name.as_deref() == Some("Keys")).unwrap();
    assert!(keys.notes.iter().all(|n| n.channel == 1));
}

#[test]
fn notes_are_cut_at_their_clips_edges() {
    // One note starts before its clip does and one runs past its end: each sounds only inside.
    let f = pad_song(
        r#"{"clips": [{"start": 4, "length": 2, "notes": [
            {"pitch": 60, "start": -1, "length": 2},
            {"pitch": 64, "start": 1.5, "length": 4},
            {"pitch": 67, "start": 3, "length": 1}
        ]}]}"#,
        &[],
    );
    let (_, parts) = written(&export_song(&f.open()).unwrap());
    assert_eq!(parts[1].notes, [note(0, 60, 102, 3840, 4800), note(0, 64, 102, 5280, 5760)]);
    // Two clips laid over each other both play; where one pitch overlaps, the first ends where
    // the second starts.
    let f = pad_song(
        r#"{"clips": [
            {"start": 0, "length": 4, "notes": [{"pitch": 60, "start": 0, "length": 2}]},
            {"start": 1, "length": 4, "notes": [{"pitch": 60, "start": 0, "length": 2}]}
        ]}"#,
        &[],
    );
    let (_, parts) = written(&export_song(&f.open()).unwrap());
    assert_eq!(parts[1].notes, [note(0, 60, 102, 0, 960), note(0, 60, 102, 960, 2880)]);
}

#[test]
fn muted_in_song_tracks_are_left_out() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    crate::mute::mute_in_song(&mut s, &["bass".into(), "kick".into()], true).unwrap();
    let (_, parts) = written(&export_song(&s).unwrap());
    let names: Vec<_> = parts[1..].iter().map(|p| p.name.clone().unwrap()).collect();
    assert_eq!(names, ["Hats", "Keys"]);
    // Keys gets the first free channel now.
    assert!(parts[2].notes.iter().all(|n| n.channel == 0));
    // A song with no notes left to write says so.
    crate::mute::mute_in_song(&mut s, &["hats".into(), "keys".into()], true).unwrap();
    let e = export_song(&s).unwrap_err().to_string();
    assert!(e.contains("The song has no notes to write as a MIDI file."), "{e}");
}

#[test]
fn velocity_and_pitch_are_rounded_into_midi() {
    let f = pad_song(
        r#"{"clips": [{"start": 0, "length": 4, "notes": [
            {"pitch": 60.4, "start": 0, "length": 1, "velocity": 0.5},
            {"pitch": 61.6, "start": 0, "length": 1, "velocity": 0.001},
            {"pitch": 140, "start": 1, "length": 1, "velocity": 1},
            {"pitch": -3, "start": 2, "length": 0.0001, "velocity": 0.999}
        ]}]}"#,
        &[],
    );
    let (_, parts) = written(&export_song(&f.open()).unwrap());
    // 0.5 × 127 = 63.5, rounded; a whisper is 1, never a Note Off; a note too short for a tick has one.
    assert_eq!(
        parts[1].notes,
        [note(0, 60, 64, 0, 960), note(0, 62, 1, 0, 960), note(0, 127, 127, 960, 1920), note(0, 0, 127, 1920, 1921)]
    );
}

#[test]
fn a_clip_round_trips() {
    let f = fixture("examples/first-song");
    let s = f.open();
    let rev = s.accepted();
    let lane = rev.compiled.data.lane("bass", "notes").unwrap();
    let m2_song::LaneData::Notes(clips) = &**lane else { panic!("notes") };
    let clip = &clips[0];
    let (bytes, label) = export_clip(&s, "bass", "notes", clip.id.as_deref().unwrap()).unwrap();
    assert_eq!(label, "Bass");
    let (smf, parts) = written(&bytes);
    assert_eq!(smf.tracks[0].events[0], (0, Event::Tempo(483_871)));
    assert_eq!(parts[1].name.as_deref(), Some("Bass"));
    assert_eq!(parts[1].programs, [(0, 38)]);
    // Every note back, as beats from the clip's start.
    let want: Vec<(f64, f64, f64)> = m2_song::lane::notes(std::slice::from_ref(clip)).iter().map(|n| (n.pitch, n.start - clip.start, n.end - clip.start)).collect();
    let got: Vec<(f64, f64, f64)> =
        parts[1].notes.iter().map(|n| (n.key as f64, n.start as f64 / TICKS as f64, n.end as f64 / TICKS as f64)).collect();
    assert_eq!(got, want);
    // The file lasts as long as the clip.
    assert_eq!(smf.tracks[1].end, tick(clip.length));
    // A clip that isn't there, or isn't notes, says so.
    assert!(export_clip(&s, "bass", "notes", "nope0000").is_err());
    assert!(export_clip(&s, "bass", "cutoff", "nope0000").is_err());
}

#[test]
fn a_clip_later_in_the_song_takes_the_tempo_where_it_starts() {
    let f = pad_song(
        r#"{"clips": [{"id": "late0000", "start": 8, "length": 8, "notes": [{"pitch": 60, "start": 0, "length": 1}]}]}"#,
        &[(
            "data/arrangement.json",
            r#"{"tempo": [{"beat": 0, "bpm": 120}, {"beat": 4, "bpm": 100}, {"beat": 12, "bpm": 150}, {"beat": 40, "bpm": 90}], "meter": [{"beat": 0, "beats": 4, "unit": 4}, {"beat": 8, "beats": 3, "unit": 4}]}"#,
        )],
    );
    let (bytes, _) = export_clip(&f.open(), "pad", "chords", "late0000").unwrap();
    let smf = smf::read(&bytes).unwrap();
    assert_eq!(
        smf.tracks[0].events,
        [(0, Event::Tempo(600_000)), (0, Event::Meter { beats: 3, unit_pow: 2 }), (3840, Event::Tempo(400_000))]
    );
    assert_eq!(smf.tracks[1].notes(), [note(0, 60, 102, 0, 960)]);
}

#[test]
fn the_sound_table_picks_and_writes_one_way() {
    assert_eq!(sound_for(true, Some(0), 60.0), DRUM_KIT);
    assert_eq!(
        [0, 8, 16, 24, 33, 40, 56, 64, 88, 104, 112, 127].map(|p| sound_for(false, Some(p), 60.0)),
        [
            "std:fm-keys",
            "std:fm-bell",
            "std:synth-pad",
            "std:synth-pluck",
            "std:synth-bass",
            "std:synth-strings",
            "std:synth-supersaw",
            "std:synth-lead",
            "std:synth-pad",
            "std:synth-pluck",
            "std:fm-metal",
            "std:synth-pad"
        ]
    );
    assert_eq!(sound_for(false, None, 40.0), "std:synth-bass");
    assert_eq!(sound_for(false, None, 48.0), "std:fm-keys");
    // Each picked sound is written with a program that picks it again.
    for s in SOUNDS.iter().filter(|s| !s.picked_by.is_empty()) {
        assert_eq!(sound_for(false, program_for(s.component), 60.0), s.component);
    }
    assert_eq!(program_for("std:drum-kit"), None);
    assert_eq!(program_for("std:synth-custom"), None);
}
