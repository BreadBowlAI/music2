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

// ---------------------------------------------------------------- bringing a MIDI file in

/// A track to make a file of: its name and its events.
type Track<'a> = (Option<&'a str>, Vec<(u64, Event)>);

/// A file of `tracks`, 96 ticks a beat.
fn file(format: u16, tracks: Vec<Track>) -> Smf {
    let tracks = tracks
        .into_iter()
        .map(|(name, events)| {
            let end = events.iter().map(|e| e.0).max().unwrap_or(0);
            SmfTrack { name: name.map(String::from), events, end }
        })
        .collect();
    Smf { format, ppq: 96, tracks }
}

/// A note on `channel`, key `key`, from tick `start` for `len` ticks, at velocity 100.
fn played(channel: u8, key: u8, start: u64, len: u64) -> Vec<(u64, Event)> {
    vec![(start, Event::NoteOn { channel, key, velocity: 100 }), (start + len, Event::NoteOff { channel, key })]
}

fn label(c: &str) -> String {
    c.rsplit(':').next().unwrap().to_string()
}

#[test]
fn each_track_and_channel_becomes_a_track() {
    let mut two = vec![(0, Event::Program { channel: 0, program: 33 }), (0, Event::Program { channel: 1, program: 0 })];
    two.extend(played(1, 72, 0, 96));
    two.extend(played(0, 40, 48, 96));
    let mut drums = played(9, 36, 0, 24);
    drums.extend(played(9, 38, 96, 24));
    let f = file(1, vec![(Some("Tempo"), vec![(0, Event::Tempo(468_750))]), (Some("Drums"), drums), (Some("Band"), two), (Some("Empty"), vec![])]);
    let b = bring(&f, "groove.mid", &label).unwrap();
    let got: Vec<(&str, &str, usize)> = b.parts.iter().map(|p| (p.label.as_str(), p.component.as_str(), p.notes.len())).collect();
    // The tempo track and the empty one have no notes; Band's two channels are two parts, named by
    // their sounds, in the order their first notes come.
    assert_eq!(got, [("Drums", DRUM_KIT, 2), ("fm-keys", "std:fm-keys", 1), ("synth-bass", "std:synth-bass", 1)]);
    let bass = &b.parts[2].notes[0];
    assert_eq!((bass.pitch, bass.start, bass.length), (40.0, 0.5, 1.0));
    assert_eq!(bass.velocity, 0.787, "100 / 127, to a thousandth");
    assert_eq!((bass.velocity * 127.0).round(), 100.0);
    assert_eq!(b.arrangement.tempo, [Tempo { beat: 0.0, bpm: 128.0 }]);
    // Format 0: one track, split by channel.
    let mut one = played(9, 36, 0, 24);
    one.extend(played(2, 50, 0, 96));
    let b = bring(&file(0, vec![(Some("Song"), one)]), "zero.mid", &label).unwrap();
    // Both start at once: channel 3 (D3 and up: FM keys) comes before channel 10.
    assert_eq!(b.parts.iter().map(|p| p.component.as_str()).collect::<Vec<_>>(), ["std:fm-keys", DRUM_KIT]);
    // A name twice is numbered.
    let b = bring(&file(1, vec![(Some("Piano"), played(0, 60, 0, 96)), (Some("piano"), played(1, 64, 0, 96))]), "two.mid", &label).unwrap();
    assert_eq!(b.parts.iter().map(|p| p.label.as_str()).collect::<Vec<_>>(), ["Piano", "piano 2"]);
}

#[test]
fn channel_10_plays_the_drum_kit() {
    // Whatever program it sets.
    let mut drums = vec![(0, Event::Program { channel: 9, program: 40 })];
    drums.extend(played(9, 42, 0, 12));
    let b = bring(&file(1, vec![(None, drums)]), "d.mid", &label).unwrap();
    assert_eq!((b.parts[0].component.as_str(), b.parts[0].label.as_str()), (DRUM_KIT, "drum-kit"));
}

#[test]
fn programs_pick_std_sounds() {
    let cases = [(0, "std:fm-keys"), (11, "std:fm-bell"), (19, "std:synth-pad"), (25, "std:synth-pluck"), (38, "std:synth-bass"), (48, "std:synth-strings"), (61, "std:synth-supersaw"), (80, "std:synth-lead"), (89, "std:synth-pad"), (105, "std:synth-pluck"), (114, "std:fm-metal"), (125, "std:synth-pad")];
    for (program, want) in cases {
        let mut t = vec![(0, Event::Program { channel: 3, program })];
        t.extend(played(3, 60, 0, 96));
        assert_eq!(bring(&file(1, vec![(None, t)]), "p.mid", &label).unwrap().parts[0].component, want, "program {program}");
    }
    // A program set on another track for the channel counts too.
    let f = file(1, vec![(None, vec![(0, Event::Program { channel: 3, program: 48 })]), (Some("Strings"), played(3, 60, 0, 96))]);
    assert_eq!(bring(&f, "p.mid", &label).unwrap().parts[0].component, "std:synth-strings");
}

#[test]
fn no_program_picks_by_pitch() {
    let low = file(1, vec![(None, [played(0, 36, 0, 96), played(0, 43, 96, 96)].concat())]);
    assert_eq!(bring(&low, "low.mid", &label).unwrap().parts[0].component, "std:synth-bass");
    let high = file(1, vec![(None, [played(0, 48, 0, 96), played(0, 60, 96, 96)].concat())]);
    assert_eq!(bring(&high, "high.mid", &label).unwrap().parts[0].component, "std:fm-keys");
}

#[test]
fn notes_shorter_than_a_64th_get_a_64th() {
    let tap = file(1, vec![(None, played(0, 60, 96, 0))]);
    let n = &bring(&tap, "t.mid", &label).unwrap().parts[0].notes[0];
    assert_eq!((n.start, n.length), (1.0, SHORTEST));
}

#[test]
fn odd_tempos_and_meters_stay_in_range() {
    let events = vec![
        // 10 bpm and 60,000,000 bpm: clamped.
        (0, Event::Tempo(6_000_000)),
        (96, Event::Tempo(1)),
        // 128 at one tick twice: the last wins.
        (192, Event::Tempo(500_000)),
        (192, Event::Tempo(468_750)),
        (0, Event::Meter { beats: 3, unit_pow: 2 }),
        // Not a meter music2 holds, and one not on a bar line of 3/4 (beat 4), then 6/8 on beat 6.
        (96 * 3, Event::Meter { beats: 0, unit_pow: 2 }),
        (96 * 3, Event::Meter { beats: 7, unit_pow: 9 }),
        (96 * 4, Event::Meter { beats: 5, unit_pow: 2 }),
        (96 * 6, Event::Meter { beats: 6, unit_pow: 3 }),
    ];
    let mut t = events;
    t.extend(played(0, 60, 0, 96 * 8));
    t.sort_by_key(|e| e.0);
    let b = bring(&file(1, vec![(None, t)]), "odd.mid", &label).unwrap();
    assert_eq!(b.arrangement.tempo, [Tempo { beat: 0.0, bpm: 20.0 }, Tempo { beat: 1.0, bpm: 999.0 }, Tempo { beat: 2.0, bpm: 128.0 }]);
    assert_eq!(b.arrangement.meter, [Meter { beat: 0.0, beats: 3, unit: 4 }, Meter { beat: 6.0, beats: 6, unit: 8 }]);
    // What it is, music2 can hold.
    assert_eq!(Arrangement::read(&b.arrangement.canonical()).unwrap(), b.arrangement);
    // No tempo: 120; a first one later: 120 until it; 4/4 alone is none written.
    let b = bring(&file(1, vec![(None, [vec![(96, Event::Tempo(400_000)), (0, Event::Meter { beats: 4, unit_pow: 2 })], played(0, 60, 0, 96)].concat())]), "late.mid", &label).unwrap();
    assert_eq!(b.arrangement.tempo, [Tempo { beat: 0.0, bpm: 120.0 }, Tempo { beat: 1.0, bpm: 150.0 }]);
    assert!(b.arrangement.meter.is_empty());
    let b = bring(&file(1, vec![(None, played(0, 60, 0, 96))]), "plain.mid", &label).unwrap();
    assert_eq!(b.arrangement, Arrangement::default());
    // A meter change first found later starts from 4/4.
    let b = bring(&file(1, vec![(None, [vec![(96 * 4, Event::Meter { beats: 3, unit_pow: 2 })], played(0, 60, 0, 96)].concat())]), "m.mid", &label).unwrap();
    assert_eq!(b.arrangement.meter, [Meter::COMMON, Meter { beat: 4.0, beats: 3, unit: 4 }]);
}

#[test]
fn more_than_32_parts_is_refused() {
    let tracks: Vec<Track> = (0..40).map(|i| (None, played((i % 16) as u8, 60, 0, 96))).collect();
    let e = bring(&file(1, tracks), "groove.mid", &label).unwrap_err();
    assert_eq!(e, "groove.mid has 40 parts: music2 brings in 32 at most.");
    let tracks: Vec<Track> = (0..32).map(|i| (None, played((i % 16) as u8, 60, 0, 96))).collect();
    assert_eq!(bring(&file(1, tracks), "groove.mid", &label).unwrap().parts.len(), 32);
    // No notes, and a note past bar 9,999 (4/4: beat 39,996 starts bar 10,000).
    assert_eq!(bring(&file(1, vec![(Some("Tempo"), vec![(0, Event::Tempo(500_000))])]), "groove.mid", &label).unwrap_err(), "groove.mid has no notes.");
    let far = file(1, vec![(None, played(0, 60, 96 * 39_996, 96))]);
    assert_eq!(bring(&far, "far.mid", &label).unwrap_err(), "far.mid has a note past bar 9,999: music2 brings in notes up to there.");
    let near = file(1, vec![(None, played(0, 60, 96 * 39_994, 96))]);
    assert!(bring(&near, "near.mid", &label).is_ok());
}

#[test]
fn an_exported_song_imports_as_the_same_notes() {
    let f = fixture("examples/first-song");
    let s = f.open();
    let smf = smf::read(&export_song(&s).unwrap()).unwrap();
    let b = bring(&smf, "first-song.mid", &label).unwrap();
    assert_eq!(b.parts.iter().map(|p| p.label.as_str()).collect::<Vec<_>>(), ["Kick", "Hats", "Bass", "Keys"]);
    // The sounds: drums on 10, then what the programs pick again.
    assert_eq!(b.parts.iter().map(|p| p.component.as_str()).collect::<Vec<_>>(), [DRUM_KIT, DRUM_KIT, "std:synth-bass", "std:synth-pluck"]);
    assert_eq!(b.arrangement.tempo, [Tempo { beat: 0.0, bpm: 124.0 }]);
    let data = &s.accepted().compiled.data;
    for (p, (module, lane)) in b.parts.iter().zip([("kick", "hits"), ("hats", "hits"), ("bass", "notes"), ("keys", "chords")]) {
        let want: Vec<(f64, f64, f64)> = data.notes(module, lane).iter().map(|n| (n.pitch, n.start, n.end - n.start)).collect();
        let mut got: Vec<(f64, f64, f64)> = p.notes.iter().map(|n| (n.pitch, n.start, n.length)).collect();
        got.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.total_cmp(&b.0)));
        assert_eq!(got.len(), want.len(), "{module}");
        for (g, w) in got.iter().zip(&want) {
            assert!(g.0 == w.0 && (g.1 - w.1).abs() < 1e-9 && (g.2 - w.2).abs() < 1e-9, "{module}: every note back, as beats: {g:?} {w:?}");
        }
    }
}

#[test]
fn an_empty_song_takes_the_files_tempo() {
    let song = Arrangement::default();
    let file = Arrangement { tempo: vec![Tempo { beat: 0.0, bpm: 128.0 }], meter: vec![Meter { beat: 0.0, beats: 3, unit: 4 }] };
    assert_eq!(tempo_plan(true, &song, &file, None), Ok(Some(file.clone())), "tempo and meter, with no question");
    assert_eq!(tempo_plan(true, &song, &song, None), Ok(None), "the same: nothing to write");
}

#[test]
fn a_song_with_clips_asks_then_keeps_or_takes_the_tempo() {
    let song = Arrangement { tempo: vec![Tempo { beat: 0.0, bpm: 120.0 }], meter: vec![Meter { beat: 0.0, beats: 6, unit: 8 }] };
    let file = Arrangement { tempo: vec![Tempo { beat: 0.0, bpm: 128.0 }, Tempo { beat: 64.0, bpm: 140.0 }], meter: vec![] };
    assert_eq!(tempo_plan(false, &song, &file, None), Err(Asks { file_bpm: 128.0, song_bpm: 120.0 }));
    assert_eq!(tempo_plan(false, &song, &file, Some(TempoChoice::Keep)), Ok(None));
    // Use: the file's tempo map; the song's time signatures stay.
    assert_eq!(tempo_plan(false, &song, &file, Some(TempoChoice::Use)), Ok(Some(Arrangement { tempo: file.tempo.clone(), meter: song.meter.clone() })));
    // The same tempos: no question.
    let same = Arrangement { tempo: song.tempo.clone(), meter: vec![] };
    assert_eq!(tempo_plan(false, &song, &same, None), Ok(None));
}

#[test]
fn a_one_track_file_dropped_on_a_notes_lane_goes_into_it() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let before = f.read("data/bass/notes.json");
    // Two notes, the last ending in the clip's second bar: the clip is two bars, from bar 9.
    let one = file(1, vec![(Some("Riff"), [played(0, 45, 0, 48), played(0, 52, 96 * 4, 96 * 2)].concat()), (None, vec![(0, Event::Tempo(400_000))])]);
    let done = import_into(&mut s, &one, "riff.mid", "bass", "notes", 32.0).unwrap();
    assert_eq!(done.made.len(), 1);
    let lane = s.accepted().compiled.data.lane("bass", "notes").unwrap().clone();
    let m2_song::LaneData::Notes(clips) = &*lane else { panic!("notes") };
    let clip = clips.iter().find(|c| c.id.as_deref() == Some(done.made[0].1.as_str())).unwrap();
    assert_eq!((clip.start, clip.length), (32.0, 8.0));
    let notes: Vec<(f64, f64, f64)> = clip.notes.iter().map(|n| (n.pitch, n.start, n.length)).collect();
    assert_eq!(notes, [(45.0, 0.0, 0.5), (52.0, 4.0, 2.0)], "by beats, the song's tempo kept");
    assert_eq!(s.accepted().project.arrangement.tempo[0].bpm, 124.0, "no tempo written");
    // One undo step takes it back, byte for byte.
    crate::undo(&mut s).unwrap();
    assert_eq!(f.read("data/bass/notes.json"), before);
    // A file of two parts, and a lane that isn't notes, are refused in words.
    let two = file(1, vec![(None, played(0, 45, 0, 48)), (None, played(1, 60, 0, 48))]);
    let e = import_into(&mut s, &two, "two.mid", "bass", "notes", 0.0).unwrap_err().to_string();
    assert!(e.contains("two.mid has 2 parts: drop it below the tracks to bring each in as a track of its own."), "{e}");
    let e = import_into(&mut s, &one, "riff.mid", "bass", "cutoff", 0.0).unwrap_err().to_string();
    assert!(e.contains("Bass has no notes lane cutoff to bring riff.mid into."), "{e}");
}
