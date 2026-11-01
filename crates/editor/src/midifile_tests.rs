//! MIDI files out of songs (Q37) and the sound table (Q33), on the fixtures.

use std::collections::BTreeMap;

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

/// A song of one track, `pad`, playing `@m2/synths`' simple pad from lane `chords`, holding `clips`;
/// and any more files.
fn pad_song(clips: &str, more: &[(&str, &str)]) -> Fixture {
    let f = Fixture::new();
    f.write("music2.toml", MANIFEST);
    f.write(
        "song.ts",
        "import { song, track } from '@m2/core';\nimport { pad } from '@m2/synths';\n\
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
        "import { song, track } from '@m2/core';\nimport { pad } from '@m2/synths';\nimport { sum } from 'std/math';\n\
         const p = track('pad', { name: 'Pad' }, ({ lane }) => sum(pad(lane.notes('chords')), pad(lane.notes('lead', { name: 'Top' }))));\n\
         export default song(() => p.out);\n",
    );
    f.write("data/pad/lead.json", r#"{"clips": [{"start": 0, "length": 4, "notes": [{"pitch": 72, "start": 1, "length": 1}]}]}"#);
    let (_, parts) = written(&export_song(&f.open()).unwrap());
    let names: Vec<_> = parts.iter().map(|p| p.name.clone().unwrap()).collect();
    assert_eq!(names, ["t", "Pad", "Pad · Top"]);
    assert_eq!(parts[2].notes, [note(1, 72, 102, 960, 1920)], "channel 2");
}

// Q37: a drum machine's lane goes out on channel 10 as it plays, with no program, and the other
// parts still count 1, 2 past it. No list offers it now (KIT-2): its call is written in code.
#[test]
fn a_drum_machine_lane_is_channel_10() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let made = s.add_track("@m2/drums:drum", Some("Kit"), None, None).unwrap();
    f.write(&made.edited.file, f.read(&made.edited.file).replace("drums.drum(", "drums.kit("));
    f.write(
        &format!("data/{}/hits.json", made.module),
        r#"{"clips": [{"start": 0, "length": 4, "notes": [{"pitch": 38, "start": 1, "length": 0.25}]}]}"#,
    );
    let s = f.open();
    assert_eq!(player_of(s.accepted(), &made.module, "hits").as_deref(), Some(DRUM_MACHINE));
    let (_, parts) = written(&export_song(&s).unwrap());
    let kit = parts.iter().find(|p| p.name.as_deref() == Some("Kit")).unwrap();
    assert_eq!(kit.notes, [note(9, 38, 102, 960, 1200)]);
    assert!(kit.programs.is_empty(), "a drum part has no program");
    // The other parts still count 1, 2 past it.
    let keys = parts.iter().find(|p| p.name.as_deref() == Some("Keys")).unwrap();
    assert!(keys.notes.iter().all(|n| n.channel == 1));
}

// KIT-9: a kit's Drum tracks go out on channel 10, each note at a General MIDI note of its sound:
// one of them as it is (the hats' closed, pedal and open; a tom's), another sound's note to the
// sound's own (the Kick's D2, the snare's, as 36), or for a tom to the nearest tom note; a note no
// sound answers to as it is (C4). A Drum whose sound a lane plays keeps its notes' pitches; a clip
// of it written alone goes out as the song does.
#[test]
fn a_kits_tracks_go_out_as_general_midi_drums() {
    let f = fixture("templates/empty");
    let mut s = f.open();
    crate::drums::add_kit(&mut s, "@m2/drums:drum", "808 kit", None).unwrap();
    let hits = |pitches: &[u8]| {
        let notes: Vec<String> =
            pitches.iter().enumerate().map(|(i, p)| format!("{{\"pitch\": {p}, \"start\": {i}, \"length\": 0.25}}")).collect();
        format!("{{\"clips\": [{{\"id\": \"clip0000\", \"start\": 0, \"length\": 8, \"notes\": [{}]}}]}}", notes.join(", "))
    };
    f.write("data/kick/hits.json", hits(&[38, 35, 60]));
    f.write("data/hats/hits.json", hits(&[42, 44, 46, 36]));
    f.write("data/toms/hits.json", hits(&[45, 44, 60]));
    let s = f.open();
    let (_, parts) = written(&export_song(&s).unwrap());
    let keys = |name: &str| -> Vec<(u8, u8)> {
        parts.iter().find(|p| p.name.as_deref() == Some(name)).unwrap().notes.iter().map(|n| (n.channel, n.key)).collect()
    };
    assert_eq!(keys("Kick"), [(9, 36), (9, 35), (9, 60)]);
    assert_eq!(keys("Hats"), [(9, 42), (9, 44), (9, 46), (9, 42)]);
    assert_eq!(keys("Toms"), [(9, 45), (9, 43), (9, 60)]);
    let (clip, _) = export_clip(&s, "kick", "hits", "clip0000").unwrap();
    assert_eq!(
        written(&clip).1[1].notes.iter().map(|n| (n.channel, n.key)).collect::<Vec<_>>(),
        [(9, 36), (9, 35), (9, 60)],
        "a clip alone too"
    );
    // The Kick's sound from a lane of names: the code works it out, so its notes keep their pitches.
    f.write("tracks/kick.ts", f.read("tracks/kick.ts").replace("{ drum: \"kick\",", "{ drum: lane.control(\"sound\", \"choice\"),"));
    let s = f.open();
    assert!(s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    let (_, parts) = written(&export_song(&s).unwrap());
    let kick = parts.iter().find(|p| p.name.as_deref() == Some("Kick")).unwrap();
    assert_eq!(kick.notes.iter().map(|n| (n.channel, n.key)).collect::<Vec<_>>(), [(9, 38), (9, 35), (9, 60)]);
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
    let want: Vec<(f64, f64, f64)> =
        m2_song::lane::notes(std::slice::from_ref(clip)).iter().map(|n| (n.pitch, n.start - clip.start, n.end - clip.start)).collect();
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
    assert_eq!(sound_for(true, Some(0), 60.0), DRUM);
    assert_eq!(
        [0, 8, 16, 24, 33, 40, 56, 64, 88, 104, 112, 127].map(|p| sound_for(false, Some(p), 60.0)),
        [
            "@m2/synths:fm-keys",
            "@m2/synths:fm-bell",
            "@m2/synths:synth-pad",
            "@m2/synths:synth-pluck",
            "@m2/synths:synth-bass",
            "@m2/synths:synth-strings",
            "@m2/synths:synth-supersaw",
            "@m2/synths:synth-lead",
            "@m2/synths:synth-pad",
            "@m2/synths:synth-pluck",
            "@m2/synths:fm-metal",
            "@m2/synths:synth-pad"
        ]
    );
    assert_eq!(sound_for(false, None, 40.0), "@m2/synths:synth-bass");
    assert_eq!(sound_for(false, None, 48.0), "@m2/synths:fm-keys");
    // Each picked sound is written with a program that picks it again.
    for s in SOUNDS.iter().filter(|s| !s.picked_by.is_empty()) {
        assert_eq!(sound_for(false, program_for(s.component), 60.0), s.component);
    }
    assert_eq!(program_for("@m2/drums:drum-kit"), None);
    assert_eq!(program_for("@m2/synths:synth-custom"), None);
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
    let f = file(
        1,
        vec![(Some("Tempo"), vec![(0, Event::Tempo(468_750))]), (Some("Drums"), drums), (Some("Band"), two), (Some("Empty"), vec![])],
    );
    let b = bring(&f, "groove.mid", &label).unwrap();
    let got: Vec<(&str, &str, usize)> = b.parts.iter().map(|p| (p.label.as_str(), p.component.as_str(), p.notes.len())).collect();
    // The tempo track and the empty one have no notes; Band's two channels are two parts, named by
    // their sounds, in the order their first notes come; the drums are the Drum's (KIT-8).
    assert_eq!(got, [("Drums", DRUM, 2), ("fm-keys", "@m2/synths:fm-keys", 1), ("synth-bass", "@m2/synths:synth-bass", 1)]);
    assert_eq!(b.parts.iter().map(|p| p.drums).collect::<Vec<_>>(), [true, false, false]);
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
    assert_eq!(b.parts.iter().map(|p| p.component.as_str()).collect::<Vec<_>>(), ["@m2/synths:fm-keys", DRUM]);
    // A name twice is numbered.
    let b = bring(&file(1, vec![(Some("Piano"), played(0, 60, 0, 96)), (Some("piano"), played(1, 64, 0, 96))]), "two.mid", &label).unwrap();
    assert_eq!(b.parts.iter().map(|p| p.label.as_str()).collect::<Vec<_>>(), ["Piano", "piano 2"]);
}

// KIT-8: channel 10 is the drums, whatever program it sets: the Drum's, a kit's tracks; unnamed,
// they are the Drums.
#[test]
fn channel_10_is_the_drums() {
    let mut drums = vec![(0, Event::Program { channel: 9, program: 40 })];
    drums.extend(played(9, 42, 0, 12));
    let b = bring(&file(1, vec![(None, drums)]), "d.mid", &label).unwrap();
    assert_eq!((b.parts[0].component.as_str(), b.parts[0].label.as_str(), b.parts[0].drums), (DRUM, "Drums", true));
}

#[test]
fn programs_pick_std_sounds() {
    let cases = [
        (0, "@m2/synths:fm-keys"),
        (11, "@m2/synths:fm-bell"),
        (19, "@m2/synths:synth-pad"),
        (25, "@m2/synths:synth-pluck"),
        (38, "@m2/synths:synth-bass"),
        (48, "@m2/synths:synth-strings"),
        (61, "@m2/synths:synth-supersaw"),
        (80, "@m2/synths:synth-lead"),
        (89, "@m2/synths:synth-pad"),
        (105, "@m2/synths:synth-pluck"),
        (114, "@m2/synths:fm-metal"),
        (125, "@m2/synths:synth-pad"),
    ];
    for (program, want) in cases {
        let mut t = vec![(0, Event::Program { channel: 3, program })];
        t.extend(played(3, 60, 0, 96));
        assert_eq!(bring(&file(1, vec![(None, t)]), "p.mid", &label).unwrap().parts[0].component, want, "program {program}");
    }
    // A program set on another track for the channel counts too.
    let f = file(1, vec![(None, vec![(0, Event::Program { channel: 3, program: 48 })]), (Some("Strings"), played(3, 60, 0, 96))]);
    assert_eq!(bring(&f, "p.mid", &label).unwrap().parts[0].component, "@m2/synths:synth-strings");
}

#[test]
fn no_program_picks_by_pitch() {
    let low = file(1, vec![(None, [played(0, 36, 0, 96), played(0, 43, 96, 96)].concat())]);
    assert_eq!(bring(&low, "low.mid", &label).unwrap().parts[0].component, "@m2/synths:synth-bass");
    let high = file(1, vec![(None, [played(0, 48, 0, 96), played(0, 60, 96, 96)].concat())]);
    assert_eq!(bring(&high, "high.mid", &label).unwrap().parts[0].component, "@m2/synths:fm-keys");
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
    let b = bring(
        &file(
            1,
            vec![(None, [vec![(96, Event::Tempo(400_000)), (0, Event::Meter { beats: 4, unit_pow: 2 })], played(0, 60, 0, 96)].concat())],
        ),
        "late.mid",
        &label,
    )
    .unwrap();
    assert_eq!(b.arrangement.tempo, [Tempo { beat: 0.0, bpm: 120.0 }, Tempo { beat: 1.0, bpm: 150.0 }]);
    assert!(b.arrangement.meter.is_empty());
    let b = bring(&file(1, vec![(None, played(0, 60, 0, 96))]), "plain.mid", &label).unwrap();
    assert_eq!(b.arrangement, Arrangement::default());
    // A meter change first found later starts from 4/4.
    let b = bring(
        &file(1, vec![(None, [vec![(96 * 4, Event::Meter { beats: 3, unit_pow: 2 })], played(0, 60, 0, 96)].concat())]),
        "m.mid",
        &label,
    )
    .unwrap();
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
    assert_eq!(
        bring(&file(1, vec![(Some("Tempo"), vec![(0, Event::Tempo(500_000))])]), "groove.mid", &label).unwrap_err(),
        "groove.mid has no notes."
    );
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
    assert_eq!(
        b.parts.iter().map(|p| p.component.as_str()).collect::<Vec<_>>(),
        [DRUM, DRUM, "@m2/synths:synth-bass", "@m2/synths:synth-pluck"]
    );
    assert_eq!(b.arrangement.tempo, [Tempo { beat: 0.0, bpm: 124.0 }]);
    let data = &s.accepted().compiled.data;
    // The Kick's C1 went out as its sound's own note, C2 (KIT-9); the hats' F#2 is theirs.
    let general = |module: &str, pitch: f64| if module == "kick" { 36.0 } else { pitch };
    for (p, (module, lane)) in b.parts.iter().zip([("kick", "hits"), ("hats", "hits"), ("bass", "notes"), ("keys", "chords")]) {
        let want: Vec<(f64, f64, f64)> =
            data.notes(module, lane).iter().map(|n| (general(module, n.pitch), n.start, n.end - n.start)).collect();
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
    assert_eq!(tempo_plan(true, &song, &file, 0.0, None), Ok(Some(file.clone())), "tempo and meter, with no question");
    assert_eq!(tempo_plan(true, &song, &song, 0.0, None), Ok(None), "the same: nothing to write");
}

#[test]
fn a_song_with_clips_asks_then_keeps_or_takes_the_tempo() {
    let song = Arrangement { tempo: vec![Tempo { beat: 0.0, bpm: 120.0 }], meter: vec![Meter { beat: 0.0, beats: 6, unit: 8 }] };
    let file = Arrangement { tempo: vec![Tempo { beat: 0.0, bpm: 128.0 }, Tempo { beat: 64.0, bpm: 140.0 }], meter: vec![] };
    assert_eq!(tempo_plan(false, &song, &file, 0.0, None), Err(Asks { file_bpm: 128.0, song_bpm: 120.0 }));
    assert_eq!(tempo_plan(false, &song, &file, 0.0, Some(TempoChoice::Keep)), Ok(None));
    // Use: the file's tempo map; the song's time signatures stay.
    assert_eq!(
        tempo_plan(false, &song, &file, 0.0, Some(TempoChoice::Use)),
        Ok(Some(Arrangement { tempo: file.tempo.clone(), meter: song.meter.clone() }))
    );
    // The same tempos: no question.
    let same = Arrangement { tempo: song.tempo.clone(), meter: vec![] };
    assert_eq!(tempo_plan(false, &song, &same, 0.0, None), Ok(None));
}

#[test]
fn a_one_track_file_dropped_on_a_notes_lane_goes_into_it() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let before = f.read("data/bass/notes.json");
    // Two notes, the last ending in the clip's second bar: the clip is two bars, from bar 9.
    let one = file(
        1,
        vec![(Some("Riff"), [played(0, 45, 0, 48), played(0, 52, 96 * 4, 96 * 2)].concat()), (None, vec![(0, Event::Tempo(400_000))])],
    );
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

// ---------------------------------------------------------------- new tracks from a MIDI file (C2b)

/// groove.mid: a tempo track at `us` a beat; Drums on channel 10; Band with a bass (program 33,
/// channel 1, beats 0 to 3) and keys (program 0, channel 2, beats 4 to 5).
fn groove(us: u32) -> Smf {
    let mut band = vec![(0, Event::Program { channel: 0, program: 33 }), (0, Event::Program { channel: 1, program: 0 })];
    band.extend(played(0, 40, 0, 96 * 3));
    band.extend(played(1, 72, 96 * 4, 96));
    band.sort_by_key(|e| e.0);
    let drums = [played(9, 36, 0, 24), played(9, 38, 96, 24)].concat();
    file(1, vec![(Some("Tempo"), vec![(0, Event::Tempo(us))]), (Some("Drums"), drums), (Some("Band"), band)])
}

fn at(after: Option<&str>, beat: f64) -> Place {
    Place { after: after.map(String::from), at: beat }
}

fn made(i: Imported) -> TracksMade {
    match i {
        Imported::Tracks(t) => t,
        Imported::Asks(a) => panic!("asked {a:?}"),
    }
}

/// The tracks' keys, in the timeline's order.
fn order(s: &Session) -> Vec<String> {
    timeline::timeline(s).tracks.into_iter().filter(|t| t.track).map(|t| t.module).collect()
}

/// Every file of the project but the session's own, with its bytes.
fn files(f: &Fixture) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    let mut all: Vec<_> = crate::testing::files_under(&f.0)
        .into_iter()
        .filter(|p| !p.strip_prefix(&f.0).unwrap().starts_with(".music2"))
        .map(|p| (p.clone(), std::fs::read(&p).unwrap()))
        .collect();
    all.sort();
    all
}

// Q32, Q33, Q34, Q36, KIT-8: each part a new track with its sound and one clip, after the track
// asked for, from the bar asked for, the drums a kit: a Drums bus and a Drum track for each sound;
// at the song's own tempo there is no question.
#[test]
fn an_import_makes_a_track_for_each_part() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let number = s.accepted().number;
    // 483,871 µs a beat is the song's 124 BPM.
    let t = made(import_tracks(&mut s, &groove(483_871), "groove.mid", &at(Some("kick"), 8.0), None).unwrap());
    assert_eq!(s.accepted().number, number + 1, "one Tx");
    let names: Vec<&str> = t.tracks.iter().map(|t| t.1.as_str()).collect();
    assert_eq!(names, ["Drums", "Kick 2", "Snare", "Synth bass", "FM keys"]);
    let keys: Vec<String> = t.tracks.iter().map(|t| t.0.clone()).collect();
    assert_eq!(order(&s), ["kick", &keys[0], &keys[1], &keys[2], &keys[3], &keys[4], "hats", "bass", "keys"], "right after Kick");
    let keys = &keys[1..];
    let rev = s.accepted();
    let sounds: Vec<Option<String>> = keys.iter().zip(["hits", "hits", "notes", "notes"]).map(|(k, l)| player_of(rev, k, l)).collect();
    assert_eq!(
        sounds,
        [Some(DRUM.to_string()), Some(DRUM.to_string()), Some("@m2/synths:synth-bass".into()), Some("@m2/synths:fm-keys".into())]
    );
    // One clip each, from bar 3 to the bar line after its last note.
    assert_eq!(t.made.len(), 4);
    for ((k, l), len) in keys.iter().zip(["hits", "hits", "notes", "notes"]).zip([4.0, 4.0, 4.0, 8.0]) {
        let lane = rev.compiled.data.lane(k, l).unwrap().clone();
        let m2_song::LaneData::Notes(clips) = &*lane else { panic!("notes") };
        assert_eq!(clips.iter().map(|c| (c.start, c.length)).collect::<Vec<_>>(), [(8.0, len)], "{k}");
        assert!(t.made.iter().any(|(a, id)| a.module.as_str() == k && clips[0].id.as_deref() == Some(id.as_str())), "{k}");
    }
    let bass: Vec<(f64, f64, f64)> = rev.compiled.data.notes(&keys[2], "notes").iter().map(|n| (n.pitch, n.start, n.end)).collect();
    assert_eq!(bass, [(40.0, 8.0, 11.0)]);
    assert_eq!(rev.project.arrangement.tempo, [Tempo { beat: 0.0, bpm: 124.0 }], "no tempo written");
    // Below the tracks: at the end.
    let t = made(import_tracks(&mut s, &groove(483_871), "groove.mid", &at(None, 0.0), None).unwrap());
    assert_eq!(order(&s).last(), Some(&t.tracks[4].0));
    assert_eq!(t.tracks.iter().map(|t| t.1.as_str()).collect::<Vec<_>>(), ["Drums 2", "Kick 3", "Snare 2", "Synth bass 2", "FM keys 2"]);
}

// Q35: a song with no clips takes the file's tempo and time signatures, in the same Tx, with no
// question; the new clips end on its bar lines.
#[test]
fn an_import_into_an_empty_song_takes_the_files_tempo() {
    let f = fixture("templates/empty");
    let mut s = f.open();
    let mut g = groove(468_750);
    g.tracks[0].events.push((0, Event::Meter { beats: 3, unit_pow: 2 }));
    let t = made(import_tracks(&mut s, &g, "groove.mid", &at(None, 0.0), None).unwrap());
    let a = &s.accepted().project.arrangement;
    assert_eq!((a.tempo.clone(), a.meter.clone()), (vec![Tempo { beat: 0.0, bpm: 128.0 }], vec![Meter { beat: 0.0, beats: 3, unit: 4 }]));
    // Keys' last note ends at beat 5: two bars of 3/4.
    let lane = s.accepted().compiled.data.lane(&t.tracks[4].0, "notes").unwrap().clone();
    let m2_song::LaneData::Notes(clips) = &*lane else { panic!("notes") };
    assert_eq!((clips[0].start, clips[0].length), (0.0, 6.0));
    crate::undo(&mut s).unwrap();
    assert_eq!(s.accepted().project.arrangement.tempo[0].bpm, 120.0, "one undo takes the tempo back with the tracks");
}

// Q35: a song with clips asks first, writing nothing; Keep keeps its tempo, Use writes the file's.
#[test]
fn an_import_asks_then_keeps_or_uses_the_tempo() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let number = s.accepted().number;
    let asked = import_tracks(&mut s, &groove(468_750), "groove.mid", &at(None, 0.0), None).unwrap();
    assert_eq!(asked, Imported::Asks(Asks { file_bpm: 128.0, song_bpm: 124.0 }));
    assert_eq!(s.accepted().number, number, "nothing written");
    made(import_tracks(&mut s, &groove(468_750), "groove.mid", &at(None, 0.0), Some(TempoChoice::Keep)).unwrap());
    assert_eq!(s.accepted().project.arrangement.tempo, [Tempo { beat: 0.0, bpm: 124.0 }]);
    crate::undo(&mut s).unwrap();
    made(import_tracks(&mut s, &groove(468_750), "groove.mid", &at(None, 0.0), Some(TempoChoice::Use)).unwrap());
    assert_eq!(s.accepted().project.arrangement.tempo, [Tempo { beat: 0.0, bpm: 128.0 }]);
}

// D10: Use at a later bar is the file's tempo map from that bar on, the song's before it; the
// song's time signatures stay. Into an empty song, the file's time signatures from that bar too.
#[test]
fn use_at_a_later_bar_keeps_the_songs_tempo_before_it() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let mut g = groove(468_750);
    // 140 BPM from the file's beat 8.
    g.tracks[0].events.push((96 * 8, Event::Tempo(428_571)));
    let t = made(import_tracks(&mut s, &g, "groove.mid", &at(None, 16.0), Some(TempoChoice::Use)).unwrap());
    let a = &s.accepted().project.arrangement;
    assert_eq!(a.tempo, [Tempo { beat: 0.0, bpm: 124.0 }, Tempo { beat: 16.0, bpm: 128.0 }, Tempo { beat: 24.0, bpm: 140.0 }]);
    assert!(a.meter.is_empty());
    let lane = s.accepted().compiled.data.lane(&t.tracks[3].0, "notes").unwrap().clone();
    let m2_song::LaneData::Notes(clips) = &*lane else { panic!("notes") };
    assert_eq!(clips[0].start, 16.0);
    // The question at a later bar names the song's tempo there.
    let song = Arrangement { tempo: vec![Tempo { beat: 0.0, bpm: 120.0 }, Tempo { beat: 8.0, bpm: 90.0 }], meter: vec![] };
    let file = Arrangement { tempo: vec![Tempo { beat: 0.0, bpm: 128.0 }], meter: vec![Meter { beat: 0.0, beats: 3, unit: 4 }] };
    assert_eq!(tempo_plan(false, &song, &file, 16.0, None), Err(Asks { file_bpm: 128.0, song_bpm: 90.0 }));
    // The file's tempo where the song already has it: no question.
    let same = Arrangement { tempo: vec![Tempo { beat: 0.0, bpm: 90.0 }], meter: vec![] };
    assert_eq!(tempo_plan(false, &song, &same, 16.0, None), Ok(None));
    // An empty song: the song's before the bar, the file's tempo and meter from it.
    let plain = Arrangement::default();
    let want = Arrangement {
        tempo: vec![Tempo { beat: 0.0, bpm: 120.0 }, Tempo { beat: 8.0, bpm: 128.0 }],
        meter: vec![Meter::COMMON, Meter { beat: 8.0, beats: 3, unit: 4 }],
    };
    assert_eq!(tempo_plan(true, &plain, &file, 8.0, None), Ok(Some(want.clone())));
    assert_eq!(Arrangement::read(&want.canonical()).unwrap(), want, "one music2 can hold");
}

// WRITE-10, Q36: the tracks, their clips, the mixer's lines and the tempo are one undo step.
#[test]
fn one_undo_step_takes_the_import_back() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let before = files(&f);
    made(import_tracks(&mut s, &groove(468_750), "groove.mid", &at(Some("bass"), 4.0), Some(TempoChoice::Use)).unwrap());
    assert_ne!(files(&f), before);
    crate::undo(&mut s).unwrap();
    let after = files(&f);
    let names = |v: &[(std::path::PathBuf, Vec<u8>)]| v.iter().map(|x| x.0.display().to_string()).collect::<Vec<_>>();
    assert_eq!(names(&after), names(&before));
    assert!(after == before, "every byte back");
    assert_eq!(order(&s), ["kick", "hats", "bass", "keys"]);
}

// Q37 then Q32: a song written as a MIDI file comes back into an empty song as its tracks, with
// every note as beats, its sounds and its tempo; its Kick and Hats, two parts on channel 10, as one
// kit (KIT-8), the Kick's C1 at its sound's own note, C2 (KIT-9).
#[test]
fn an_exported_song_imports_back_as_tracks() {
    let from = fixture("examples/first-song");
    let song = from.open();
    let smf = smf::read(&export_song(&song).unwrap()).unwrap();
    let f = fixture("templates/empty");
    let mut s = f.open();
    let t = made(import_tracks(&mut s, &smf, "first-song.mid", &at(None, 0.0), None).unwrap());
    assert_eq!(t.tracks.iter().map(|t| t.1.as_str()).collect::<Vec<_>>(), ["Drums", "Kick", "Hats", "Bass", "Keys"]);
    assert_eq!(s.accepted().project.arrangement.tempo, [Tempo { beat: 0.0, bpm: 124.0 }]);
    let (a, b) = (&song.accepted().compiled.data, &s.accepted().compiled.data);
    let general = |old: &str, pitch: f64| if old == "kick" { 36.0 } else { pitch };
    for ((module, _), (old, lane)) in t.tracks[1..].iter().zip([("kick", "hits"), ("hats", "hits"), ("bass", "notes"), ("keys", "chords")])
    {
        let new_lane = if lane == "hits" { "hits" } else { "notes" };
        let want: Vec<(f64, f64, f64)> = a.notes(old, lane).iter().map(|n| (general(old, n.pitch), n.start, n.end)).collect();
        let got: Vec<(f64, f64, f64)> = b.notes(module, new_lane).iter().map(|n| (n.pitch, n.start, n.end)).collect();
        assert_eq!(got.len(), want.len(), "{old}");
        assert!(got.iter().zip(&want).all(|(g, w)| g.0 == w.0 && (g.1 - w.1).abs() < 1e-9 && (g.2 - w.2).abs() < 1e-9), "{old}: {got:?}");
    }
}

// KIT-8: a General MIDI drum file comes in as one kit, however many tracks hold its drums: a Drums
// bus and a Drum track for each sound its notes use, in the Drum's menu order, each with that
// sound's notes at their own pitches and velocities (the hats' closed, pedal and open; the toms'),
// each going to the bus, named as the table words it (Toms, review K9); the notes no sound answers
// to on Percussion, which plays its tom. The bus's fader is the master's 0 dB, as the drum machine's
// was. One undo step takes it all back.
#[test]
fn a_general_midi_drum_file_comes_in_as_one_kit() {
    let f = fixture("templates/empty");
    let mut s = f.open();
    let before = files(&f);
    let mut kicks = played(9, 36, 0, 24);
    kicks.extend(played(9, 36, 96 * 2, 24));
    let mut rest = vec![];
    for (i, key) in [42, 38, 46, 44, 41, 45, 49, 53, 62, 81].iter().enumerate() {
        rest.extend(played(9, *key, 48 * i as u64, 12));
    }
    rest.sort_by_key(|e| e.0);
    let drums = file(1, vec![(Some("Kick"), kicks), (Some("Kit"), rest), (Some("Bass"), played(0, 40, 0, 96))]);
    let t = made(import_tracks(&mut s, &drums, "beat.mid", &at(None, 0.0), None).unwrap());
    let got: Vec<&str> = t.tracks.iter().map(|t| t.1.as_str()).collect();
    assert_eq!(got, ["Drums", "Kick", "Snare", "Hats", "Toms", "Cymbal", "Percussion", "Bass"]);
    let rev = s.accepted();
    let bus = &t.tracks[0].0;
    assert!(rev.compiled.ir.modules.iter().any(|m| m.key == *bus && m.bus), "the Drums bus is a bus");
    let routes: BTreeMap<String, String> = rev.compiled.flat.routes.iter().cloned().collect();
    for (key, _) in &t.tracks[1..7] {
        assert_eq!(routes.get(key), Some(bus), "{key} goes to the bus");
        assert_eq!(player_of(rev, key, "hits").as_deref(), Some(DRUM), "{key} plays the Drum");
    }
    assert!(!routes.contains_key(&t.tracks[7].0), "the bass goes to the master");
    let notes = |k: usize| -> Vec<(f64, f64, f64)> {
        rev.compiled.data.notes(&t.tracks[k].0, "hits").iter().map(|n| (n.pitch, n.start, n.velocity)).collect()
    };
    let v = 0.787;
    assert_eq!(notes(1), [(36.0, 0.0, v), (36.0, 2.0, v)], "both tracks' kicks on one Kick");
    assert_eq!(notes(3), [(42.0, 0.0, v), (46.0, 1.0, v), (44.0, 1.5, v)], "the hats keep closed, open and pedal");
    assert_eq!(notes(4), [(41.0, 2.0, v), (45.0, 2.5, v)], "the toms their pitches");
    assert_eq!(notes(5), [(49.0, 3.0, v), (53.0, 3.5, v)], "the ride bell is the cymbal's");
    assert_eq!(notes(6), [(62.0, 4.0, v), (81.0, 4.5, v)], "none lost");
    let view = s.view();
    let sound = |k: usize| {
        let m = view.modules.iter().find(|m| m.key == t.tracks[k].0).unwrap();
        m.nodes.iter().flat_map(|n| &n.knobs).find(|k| k.slot.as_deref() == Some("drum")).map(|k| k.value.clone()).unwrap()
    };
    let sounds: Vec<serde_json::Value> = (1..7).map(sound).collect();
    assert_eq!(sounds, ["kick", "snare", "hats", "tom", "cymbal", "tom"].map(serde_json::Value::from));
    // One clip each, to the bar line after the drums' last note.
    for k in 1..7 {
        let lane = rev.compiled.data.lane(&t.tracks[k].0, "hits").unwrap().clone();
        let m2_song::LaneData::Notes(clips) = &*lane else { panic!("notes") };
        assert_eq!(clips.iter().map(|c| (c.start, c.length)).collect::<Vec<_>>(), [(0.0, 8.0)], "{}", t.tracks[k].1);
    }
    assert!(f.read("song.ts").contains(&format!("{bus}: drumsTrack.out,")), "no fader written: {}", f.read("song.ts"));
    crate::undo(&mut s).unwrap();
    assert!(files(&f) == before, "one undo takes it all back");
}

// KIT-8, KIT-9 (review K2): a kit with Percussion written as a MIDI file comes back as the same kit:
// Percussion's bongo and triangle keep their keys, out and in.
#[test]
fn a_percussion_track_round_trips() {
    let f = fixture("templates/empty");
    let mut s = f.open();
    let perc = file(1, vec![(Some("Perc"), [played(9, 36, 0, 24), played(9, 62, 48, 24), played(9, 81, 96, 24)].concat())]);
    let t = made(import_tracks(&mut s, &perc, "perc.mid", &at(None, 0.0), None).unwrap());
    assert_eq!(t.tracks.iter().map(|t| t.1.as_str()).collect::<Vec<_>>(), ["Drums", "Kick", "Percussion"]);
    let (_, parts) = written(&export_song(&s).unwrap());
    let keys: Vec<(u8, u8)> =
        parts.iter().find(|p| p.name.as_deref() == Some("Percussion")).unwrap().notes.iter().map(|n| (n.channel, n.key)).collect();
    assert_eq!(keys, [(9, 62), (9, 81)], "as they came in");
    let g = fixture("templates/empty");
    let mut back = g.open();
    let again = smf::read(&export_song(&s).unwrap()).unwrap();
    let t = made(import_tracks(&mut back, &again, "again.mid", &at(None, 0.0), None).unwrap());
    assert_eq!(t.tracks.iter().map(|t| t.1.as_str()).collect::<Vec<_>>(), ["Drums", "Kick", "Percussion"], "the same kit");
}

// KIT-8 (review K1): a one-part drum file dropped onto a Drum's lane comes in as a kit after that
// track, never into the lane whole, where every hit would play the lane's one drum. A part that
// isn't drums goes into any notes lane whole, and drums go whole into the drum machine's lane,
// which plays General MIDI.
#[test]
fn a_drum_part_dropped_on_a_drums_lane_is_a_kit() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let made_machine = s.add_track("@m2/drums:drum", Some("Machine"), None, None).unwrap();
    f.write(&made_machine.edited.file, f.read(&made_machine.edited.file).replace("drums.drum(", "drums.kit("));
    s.refresh();
    let beat = file(1, vec![(Some("Loop"), [played(9, 36, 0, 24), played(9, 42, 48, 24), played(9, 38, 96, 24)].concat())]);
    let riff = file(1, vec![(Some("Riff"), played(0, 45, 0, 48))]);
    assert!(!into_lane(&s, &beat, "kick", "hits"), "drums onto a Drum's lane: a kit");
    assert!(into_lane(&s, &beat, "machine", "hits"), "drums onto the drum machine's lane: into it");
    assert!(into_lane(&s, &riff, "kick", "hits") && into_lane(&s, &riff, "bass", "notes"), "a part that isn't drums: into it");
    assert!(!into_lane(&s, &groove(483_871), "bass", "notes"), "two parts: new tracks");
    let before = f.read("data/kick/hits.json");
    let e = import_into(&mut s, &beat, "loop.mid", "kick", "hits", 0.0).unwrap_err().to_string();
    assert!(e.contains("loop.mid is drums, which come in as a kit: drop it below the tracks or on a track's name."), "{e}");
    assert_eq!(f.read("data/kick/hits.json"), before, "nothing written");
}

// Q32, D10: refused in words, and nothing written: no notes, over 32 parts, a note past bar 9,999
// where it lands, the code doesn't compile.
#[test]
fn a_refused_file_writes_nothing() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let number = s.accepted().number;
    let before = files(&f);
    let refused = |s: &mut Session, file: &Smf, name: &str, place: &Place| {
        import_tracks(s, file, name, place, Some(TempoChoice::Use)).unwrap_err().to_string()
    };
    let none = file(1, vec![(Some("Tempo"), vec![(0, Event::Tempo(500_000))])]);
    assert!(refused(&mut s, &none, "groove.mid", &at(None, 0.0)).ends_with("groove.mid has no notes."));
    let many = file(1, (0..40).map(|i| (None, played((i % 16) as u8, 60, 0, 96))).collect());
    assert!(refused(&mut s, &many, "groove.mid", &at(None, 0.0)).ends_with("groove.mid has 40 parts: music2 brings in 32 at most."));
    // Bar 9,998 from the file's start, but past bar 9,999 from bar 11.
    let far = file(1, vec![(None, played(0, 60, 96 * 39_988, 96))]);
    let e = refused(&mut s, &far, "far.mid", &at(None, 40.0));
    assert!(e.ends_with("far.mid has a note past bar 9,999: music2 brings in notes up to there."), "{e}");
    assert_eq!((s.accepted().number, files(&f)), (number, before.clone()));
    // Code that doesn't compile.
    f.write("song.ts", "export default song(;\n");
    s.refresh();
    assert!(!s.draft_errors.is_empty());
    let e = refused(&mut s, &groove(468_750), "groove.mid", &at(None, 0.0));
    assert!(e.ends_with("The code doesn't compile, so no track can be added now. Fix the code first."), "{e}");
    assert_eq!(s.accepted().number, number);
}
