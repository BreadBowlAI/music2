//! A voice's notes (VOX-43, VOX-44): a song of one Vox track at 120 bpm (a beat is 0.5 s), its clip
//! playing 2 s of a 4 s file from its second 1. The measure is given as the server would give it.

use std::sync::Arc;

use m2_runtime::measure::Measured;
use m2_session::Session;
use m2_song::{AudioClip, LaneAddr, LaneData, Sung};

use crate::clips::{ClipRef, Gesture, edit};
use crate::testing::{Fixture, MANIFEST};

const RATE: u32 = 48_000;

fn song(clip: &str) -> Fixture {
    let f = Fixture::new();
    f.write("music2.toml", MANIFEST);
    f.write(
        "song.ts",
        "import { song, track } from \"@m2/core\";\nconst vox = track(\"vox\", { name: \"Vox\" }, ({ lane }) => lane.audio(\"clips\"));\n\
         export default song(() => vox.out);\n",
    );
    f.write("data/arrangement.json", "{\"tempo\": [{\"beat\": 0, \"bpm\": 120}], \"meter\": []}\n");
    f.write("data/vox/clips.json", format!("{{\"clips\": [{clip}]}}\n"));
    let path = f.0.join("assets/vox.wav");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    m2_runtime::audio::write_wav(&path, &[vec![0.1; 4 * RATE as usize]], RATE).unwrap();
    f
}

/// The clip: 4 beats (2 s) at beat 2, from second 1 of its file.
const CLIP: &str = "{\"id\": \"vox00001\", \"start\": 2, \"length\": 4, \"asset\": \"vox.wav\", \"offset\": 1}";

fn vox() -> LaneAddr {
    LaneAddr::new("vox", "clips").unwrap()
}

fn the_clip(s: &Session) -> AudioClip {
    match s.accepted().compiled.data.lane("vox", "clips").map(|d| (**d).clone()) {
        Some(LaneData::Audio(c)) => c[0].clone(),
        other => panic!("an audio lane: {other:?}"),
    }
}

/// A measure of the 4 s file, a hop of 256 frames: each run `(from, to, centre)` in hops a note.
fn measured(runs: &[(usize, usize, f32)]) -> Arc<Measured> {
    let hops = (4 * RATE as usize).div_ceil(256);
    let (mut note, mut centre) = (vec![0.0; hops], vec![0.0; hops]);
    for (n, &(a, b, c)) in runs.iter().enumerate() {
        note[a..b].fill((n + 1) as f32);
        centre[a..b].fill(c);
    }
    let names = ["hz", "clarity", "level", "note", "centre"].map(String::from).to_vec();
    Arc::new(Measured {
        hop: 256,
        rate: RATE as f64,
        outputs: names,
        values: vec![vec![0.0; hops], vec![0.0; hops], vec![0.0; hops], note, centre],
    })
}

fn find(found: Arc<Measured>) -> Gesture {
    let clip = ClipRef { lane: vox(), id: Some("vox00001".into()), at: None, sha: None };
    Gesture::FindNotes { clip, measure: "@m2/vocals:notes".into(), render: "@m2/vocals:voice".into(), found: Some(found) }
}

/// Hops 200 to 300 (1.0667 s to 1.6 s) and 400 to 500 (2.1333 s to 2.6667 s) are in the part the
/// clip plays; 50 to 100 is before it and 600 to 700 after.
const RUNS: [(usize, usize, f32); 4] = [(50, 100, 60.0), (200, 300, 64.3137), (400, 500, 66.9), (600, 700, 59.0)];

// VOX-43: Find notes writes the notes of each run that reaches the part of the file the clip plays,
// rounded to 0.1 ms and a cent, unedited, and the renderer: one step, undone in one.
#[test]
fn find_notes_writes_the_notes_the_measure_found() {
    let f = song(CLIP);
    let mut s = f.open();
    let before = the_clip(&s);
    edit(&mut s, &find(measured(&RUNS))).unwrap();
    let c = the_clip(&s);
    assert_eq!(c.render.as_deref(), Some("@m2/vocals:voice"));
    assert_eq!(
        c.sung,
        [
            Sung { at: 1.0667, length: 0.5333, pitch: 64.31, move_by: 0.0, straighten: 0.0 },
            Sung { at: 2.1333, length: 0.5334, pitch: 66.9, move_by: 0.0, straighten: 0.0 },
        ]
    );
    s.undo().unwrap();
    assert_eq!(the_clip(&s), before, "one undo step takes it back");
}

// VOX-43, D8: run again on a clip with notes, each edited note's move and straighten go to the new
// note it overlaps by more than half; one that overlaps no new note by half gives nothing.
#[test]
fn find_notes_again_keeps_the_edits() {
    let clip = r#"{"id": "vox00001", "start": 2, "length": 4, "asset": "vox.wav", "offset": 1, "render": "@m2/vocals:voice",
        "sung": [{"at": 1.1, "length": 0.4, "pitch": 64.3, "move": -0.31, "straighten": 0.5},
                 {"at": 1.95, "length": 0.3, "pitch": 66.9, "move": 1, "straighten": 0},
                 {"at": 2.9, "length": 0.05, "pitch": 66.9, "move": 2, "straighten": 0}]}"#;
    let f = song(clip);
    let mut s = f.open();
    edit(&mut s, &find(measured(&RUNS))).unwrap();
    let c = the_clip(&s);
    assert_eq!((c.sung[0].move_by, c.sung[0].straighten), (-0.31, 0.5), "overlaps it by 0.4 of 0.4 s");
    assert_eq!((c.sung[1].move_by, c.sung[1].straighten), (0.0, 0.0), "overlaps 1.95..2.25 by 0.1167 s: not half");
    assert_eq!(c.sung.len(), 2);
}

// VOX-43: refused in words, with no change: a clip played backwards or at another pitch, a measure
// that finds no notes there, or isn't one for Find notes, a render that isn't a kernel, a gesture
// with no measure given.
#[test]
fn find_notes_refuses_what_it_cant_hear() {
    let refused = |clip: &str, g: Gesture| {
        let f = song(clip);
        let mut s = f.open();
        let before = f.read("data/vox/clips.json");
        let why = edit(&mut s, &g).map(|_| ()).unwrap_err().to_string();
        assert_eq!(f.read("data/vox/clips.json"), before);
        why
    };
    let reversed = CLIP.replace("\"offset\": 1", "\"offset\": 1, \"reverse\": true");
    assert!(refused(&reversed, find(measured(&RUNS))).contains("turn Reverse off first"));
    let pitched = CLIP.replace("\"offset\": 1", "\"offset\": 1, \"pitch\": 2");
    assert!(refused(&pitched, find(measured(&RUNS))).contains("set the clip's pitch back to 0"));
    assert!(refused(CLIP, find(measured(&[(50, 100, 60.0)]))).contains("No sung notes were found"));
    let mut other = (*measured(&RUNS)).clone();
    other.outputs[3] = "onset".into();
    assert!(refused(CLIP, find(Arc::new(other))).contains("needs a measure that writes `note` and `centre`"));
    let Gesture::FindNotes { clip, measure, .. } = find(measured(&RUNS)) else { unreachable!() };
    let g = Gesture::FindNotes { clip: clip.clone(), measure: measure.clone(), render: "voice".into(), found: Some(measured(&RUNS)) };
    assert!(refused(CLIP, g).contains("isn't a kernel"));
    assert!(refused(CLIP, Gesture::FindNotes { clip, measure, render: "@m2/vocals:voice".into(), found: None }).contains("ask the server"));
}

// VOX-43, PACK-10: on a song that doesn't list @m2/vocals, Find notes adds its dependency and lock
// entry in the same step; the packages it measures with hold the pack's measure and renderer, and
// refuse a kernel that isn't one.
#[test]
fn find_notes_adds_the_pack_the_song_lacks() {
    let f = song(CLIP);
    let mut s = f.open();
    assert!(!f.read("music2.toml").contains("@m2/vocals"));
    let set = crate::sung::packages_for(&s, "@m2/vocals:notes", "@m2/vocals:voice").unwrap();
    assert_eq!(set.offline_decl("@m2/vocals:notes").map(|d| d.hop), Some(256));
    let wrong = |m: &str, r: &str| crate::sung::packages_for(&s, m, r).map(drop).unwrap_err().to_string();
    assert!(wrong("@m2/vocals:voice", "@m2/vocals:voice").contains("isn't a measure"));
    assert!(wrong("@m2/vocals:notes", "@m2/vocals:notes").contains("doesn't play edited notes"));
    assert!(wrong("@nobody/none:notes", "@m2/vocals:voice").contains("isn't installed"));
    edit(&mut s, &find(measured(&RUNS))).unwrap();
    assert!(f.read("music2.toml").contains("\"@m2/vocals\" = \"1.1\""), "{}", f.read("music2.toml"));
    assert!(f.read("music2.lock").contains("@m2/vocals"));
    assert!(s.problems().iter().all(|d| d.code == "asset.unpinned"), "{:?}", s.problems());
    s.undo().unwrap();
    assert!(!f.read("music2.toml").contains("@m2/vocals") && the_clip(&s).sung.is_empty(), "one undo takes both back");
}

/// The clip with notes at 1.2 s (0.4 long, across file second 1.5, beat 3 of the clip's 2 to 6)
/// and at 2.5 s.
const SUNG: &str = r#"{"id": "vox00001", "start": 2, "length": 4, "asset": "vox.wav", "offset": 1, "render": "@m2/vocals:voice",
    "sung": [{"at": 1.2, "length": 0.4, "pitch": 64, "move": 1, "straighten": 0},
             {"at": 2.5, "length": 0.3, "pitch": 66, "move": 0, "straighten": 0}]}"#;

// VOX-44: a split leaves each half the notes that reach the part of the file it plays; the note
// across the cut stays in both.
#[test]
fn a_split_keeps_the_notes_each_half_plays() {
    let f = song(SUNG);
    let mut s = f.open();
    let clip = ClipRef { lane: vox(), id: Some("vox00001".into()), at: None, sha: None };
    // Beat 3 is 0.5 s into the clip: file second 1.5.
    edit(&mut s, &Gesture::Split { clips: vec![clip], at: 3.0 }).unwrap();
    let LaneData::Audio(c) = (**s.accepted().compiled.data.lane("vox", "clips").unwrap()).clone() else { panic!() };
    let ats = |c: &AudioClip| c.sung.iter().map(|n| n.at).collect::<Vec<_>>();
    assert_eq!((ats(&c[0]), ats(&c[1])), (vec![1.2], vec![1.2, 2.5]));
    assert!(c.iter().all(|c| c.render.is_some()));
}

// VOX-44: a resize keeps the notes the part it plays now reaches.
#[test]
fn a_resize_keeps_the_notes_it_plays() {
    let f = song(SUNG);
    let mut s = f.open();
    let clip = ClipRef { lane: vox(), id: Some("vox00001".into()), at: None, sha: None };
    // Its end to beat 4: it plays file seconds 1 to 2.
    edit(&mut s, &Gesture::Resize { clip, start: 2.0, length: 2.0 }).unwrap();
    assert_eq!(the_clip(&s).sung.iter().map(|n| n.at).collect::<Vec<_>>(), [1.2]);
}

// RENDER-30b (the review's S8): Reverse on a clip whose notes are edited is refused in words,
// since played backwards they would play as sung; on one whose notes aren't edited it is fine.
#[test]
fn reverse_is_refused_on_edited_notes() {
    let clip = ClipRef { lane: vox(), id: Some("vox00001".into()), at: None, sha: None };
    let reverse = Gesture::Audio { clip, gain_db: None, fade_in: None, fade_out: None, reverse: Some(true), follow: None, offset: None };
    let f = song(SUNG);
    let mut s = f.open();
    let why = edit(&mut s, &reverse).map(drop).unwrap_err().to_string();
    assert!(why.contains("notes are edited") && !the_clip(&s).reverse, "{why}");
    let f = song(&SUNG.replace("\"move\": 1", "\"move\": 0"));
    let mut s = f.open();
    edit(&mut s, &reverse).unwrap();
    assert!(the_clip(&s).reverse);
}

// VOX-43 (the review's S8): a song that plays a version of @m2/vocals with no measure (one made
// before 1.1, here a copy of the pack from a folder without `notes`) is told which version it
// plays and how to update it.
#[test]
fn find_notes_on_an_older_vocals_says_to_update() {
    let f = song(CLIP);
    let pack = f.0.join("old-vocals");
    let from = m2_song::pack::source_packs().join("@m2/vocals");
    for rel in ["m2pkg.toml", "README.md", "src/index.ts", "kernels/vocals.wasm"] {
        let to = pack.join(rel);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(from.join(rel), &to).unwrap();
    }
    for p in std::fs::read_dir(from.join("presets/tune")).unwrap() {
        let p = p.unwrap().path();
        let to = pack.join("presets/tune").join(p.file_name().unwrap());
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(&p, to).unwrap();
    }
    let manifest = std::fs::read_to_string(pack.join("m2pkg.toml")).unwrap();
    let cut = manifest.find("[[kernel]]\nname = \"notes\"").unwrap();
    std::fs::write(pack.join("m2pkg.toml"), manifest[..cut].replace("version = \"1.1.0\"", "version = \"1.0.0\"")).unwrap();
    f.write("music2.toml", format!("{MANIFEST}\"@m2/vocals\" = {{ path = \"old-vocals\" }}\n"));
    let s = f.open();
    let why = crate::sung::packages_for(&s, "@m2/vocals:notes", "@m2/vocals:voice").map(drop).unwrap_err().to_string();
    assert!(why.contains("@m2/vocals 1.0.0") && why.contains("m2 pack update @m2/vocals"), "{why}");
}

// VOX-44 (the review's S12): a comp leaves each piece the notes that reach the part of the file
// it plays. A loop's three takes of 4 s in one file, the clip playing the third (file seconds 8 to
// 12) with a note in each half; Take 2 comped over the second half: the first piece keeps the
// first note, the second plays file seconds 6 to 8 and keeps none.
#[test]
fn a_comp_keeps_each_pieces_notes() {
    let clip = r#"{"id": "vox00001", "start": 0, "length": 8, "asset": "vox.wav", "offset": 8, "render": "@m2/vocals:voice",
        "sung": [{"at": 8.5, "length": 0.5, "pitch": 64, "move": 1, "straighten": 0},
                 {"at": 10.5, "length": 0.5, "pitch": 66, "move": 0, "straighten": 0}]}"#;
    let f = song(clip);
    let path = f.0.join("assets/vox.wav");
    m2_runtime::audio::write_wav(&path.with_file_name("vox12.wav"), &[vec![0.1; 12 * RATE as usize]], RATE).unwrap();
    std::fs::rename(path.with_file_name("vox12.wav"), &path).unwrap();
    f.write("assets/vox.wav.json", "{\"takes\": [0, 4, 8]}\n");
    let mut s = f.open();
    let at = ClipRef { lane: vox(), id: Some("vox00001".into()), at: None, sha: None };
    edit(&mut s, &Gesture::Comp { clip: at, takes: vec![0.0, 4.0, 8.0], take: 1, from: 4.0, to: 8.0 }).unwrap();
    let LaneData::Audio(c) = (**s.accepted().compiled.data.lane("vox", "clips").unwrap()).clone() else { panic!() };
    assert_eq!(c.len(), 2, "{c:?}");
    let ats = |c: &AudioClip| c.sung.iter().map(|n| n.at).collect::<Vec<_>>();
    assert_eq!((ats(&c[0]), ats(&c[1])), (vec![8.5], vec![]));
}
