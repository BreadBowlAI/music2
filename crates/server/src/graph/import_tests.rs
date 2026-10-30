//! Bringing audio in through the open song (A2): what it imports plays, a path and an upload land
//! alike, a read-only window refuses, and the peaks are ready when it answers.

use std::path::{Path, PathBuf};

use m2_editor::import::{Source, Target};
use m2_editor::render::Length;
use serde_json::{Value, json};

use crate::graph::Graph;
use crate::peaks::Store;

const SONG: &str = "import { song, track } from '@m2/core';\n\
                    const vox = track('vox', ({ lane }) => lane.audio('clips'));\nexport default song(() => vox.out);\n";

/// A song whose output is one audio track's lane, in a folder of its own.
fn project(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("m2-graph-import-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("data/vox")).unwrap();
    let manifest = "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.5\"\n";
    std::fs::write(dir.join("music2.toml"), manifest).unwrap();
    std::fs::write(dir.join("song.ts"), SONG).unwrap();
    std::fs::write(dir.join("data/vox/clips.json"), "{\"clips\": []}\n").unwrap();
    dir
}

/// A mono 32-bit float WAV at 48 kHz: sample `i` is `k / 8,388,607` for a whole `k`, so a 24-bit
/// render of it at unity gives back each `k` exactly.
fn float_wav(frames: usize) -> (Vec<u8>, Vec<i32>) {
    let ks: Vec<i32> = (0..frames).map(|i| ((i as i64 * 7919 % 2_000_001) - 1_000_000) as i32).collect();
    let spec = hound::WavSpec { channels: 1, sample_rate: 48_000, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let mut out = std::io::Cursor::new(vec![]);
    let mut w = hound::WavWriter::new(&mut out, spec).unwrap();
    for k in &ks {
        w.write_sample(*k as f32 / 8_388_607.0).unwrap();
    }
    w.finalize().unwrap();
    (out.into_inner(), ks)
}

fn opened(dir: &Path) -> (Graph, u64) {
    let graph = Graph::silent();
    let session = graph.open(dir).unwrap()["session"].as_u64().unwrap();
    (graph, session)
}

fn onto_vox(beat: f64) -> Target {
    Target::Lane { module: "vox".into(), lane: "clips".into(), beat }
}

fn bytes(name: &str, b: &[u8]) -> Source {
    Source::Bytes { name: name.into(), bytes: b.to_vec() }
}

/// The lane file's clips, without their ids.
fn clips(dir: &Path) -> Vec<Value> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("data/vox/clips.json")).unwrap()).unwrap();
    v["clips"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            let mut c = c.clone();
            c.as_object_mut().unwrap().remove("id");
            c
        })
        .collect()
}

// A2: an offline render right after the import has the file's samples from the beat it was
// dropped on, exactly: the file was taken in before the Tx named it (D1), at unity gain.
#[test]
fn an_imported_clip_plays_its_file() {
    let dir = project("plays");
    let (graph, session) = opened(&dir);
    let (wav, ks) = float_wav(12_000);
    let done = graph.import_audio(session, vec![bytes("tone.wav", &wav)], onto_vox(4.0)).unwrap();
    assert_eq!(done["name"], "importing tone.wav", "{done:#}");
    assert_eq!(done["made"].as_array().unwrap().len(), 1, "{done:#}");
    let (_, _, render, _) = graph.render(Length::Seconds(2.5)).unwrap();
    let out = dir.join("out.wav");
    render.write(&out, |_| {}).unwrap();
    let got: Vec<i32> = hound::WavReader::open(&out).unwrap().samples::<i32>().map(Result::unwrap).collect();
    // Beat 4 at 120 BPM is 2 s: frame 96,000; two channels a frame.
    let at = 96_000;
    assert!(got[..at * 2].iter().all(|s| *s == 0), "silence before the clip");
    for (i, k) in ks.iter().enumerate() {
        assert_eq!((got[(at + i) * 2], got[(at + i) * 2 + 1]), (*k, *k), "frame {i} of the file");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_path_and_an_upload_import_alike() {
    let (a, b) = (project("path"), project("upload"));
    let (wav, _) = float_wav(4_800);
    let file = a.join("outside vox.wav");
    std::fs::write(&file, &wav).unwrap();
    let (ga, sa) = opened(&a);
    let (gb, sb) = opened(&b);
    ga.import_audio(sa, vec![Source::Path(file.clone())], onto_vox(2.0)).unwrap();
    gb.import_audio(sb, vec![bytes("outside vox.wav", &wav)], onto_vox(2.0)).unwrap();
    assert_eq!(clips(&a), clips(&b));
    assert_eq!(clips(&a)[0]["asset"], "outside vox.wav");
    assert_eq!(std::fs::read(a.join("assets/outside vox.wav")).unwrap(), std::fs::read(b.join("assets/outside vox.wav")).unwrap());
    for d in [a, b] {
        let _ = std::fs::remove_dir_all(&d);
    }
}

// WRITE-17: a window open read-only imports nothing, in WRITE-18's words, and copies nothing.
#[test]
fn a_read_only_window_refuses_an_import() {
    let dir = project("ro");
    let held = m2_session::lock::acquire(&dir, "m2", m2_session::lock::RETRY).unwrap();
    let (graph, session) = opened(&dir);
    let (wav, _) = float_wav(480);
    let why = graph.import_audio(session, vec![bytes("vox.wav", &wav)], onto_vox(0.0)).unwrap_err().to_string();
    assert!(why.contains("nothing was changed"), "{why}");
    assert!(!dir.join("assets").exists(), "nothing copied");
    drop(held);
    let _ = std::fs::remove_dir_all(&dir);
}

// FMT-14, FMT-24 (review F2): one import of `kick.flac` (it comes in as `kick.wav`) and `kick.wav`
// brings both in, one numbered, as one step: each staged file has a part of its own.
#[test]
fn one_import_of_a_flac_and_a_wav_of_one_name_lands_both() {
    let dir = project("one-name");
    let (graph, session) = opened(&dir);
    let flac = include_bytes!("../../../../core/runtime/tests/fixtures/flac/16bit-mono.flac");
    let done = graph.import_audio(session, vec![bytes("kick.flac", flac), bytes("kick.wav", &float_wav(480).0)], onto_vox(0.0)).unwrap();
    assert_eq!(done["assets"], json!(["kick.wav", "kick 2.wav"]), "{done:#}");
    assert!(dir.join("assets/kick.wav").exists() && dir.join("assets/kick 2.wav").exists());
}

// Refused before anything is copied (D16): not an audio lane, not a WAV.
#[test]
fn a_refused_import_adds_nothing() {
    let dir = project("refused");
    let (graph, session) = opened(&dir);
    let why = graph.import_audio(
        session,
        vec![bytes("x.wav", &float_wav(480).0)],
        Target::Lane { module: "vox".into(), lane: "nope".into(), beat: 0.0 },
    );
    assert!(why.unwrap_err().to_string().contains("no lane `nope`"));
    let why = graph.import_audio(session, vec![bytes("song.mp3", b"ID3 not a wav")], onto_vox(0.0)).unwrap_err().to_string();
    assert!(why.starts_with("song.mp3 isn't a WAV or FLAC file. music2 reads WAV and FLAC files."), "{why}");
    assert!(std::fs::read_dir(dir.join("assets")).map_or(true, |d| d.count() == 0));
    assert_eq!(clips(&dir), Vec::<Value>::new());
    let _ = std::fs::remove_dir_all(&dir);
}

// D31: the peaks come from the import's own decode, ready when it answers.
#[test]
fn the_peaks_are_ready_when_it_answers() {
    let dir = project("peaks");
    let (graph, session) = opened(&dir);
    let (wav, _) = float_wav(2_560);
    let sha = m2_song::Sha::of(&wav);
    assert!(!Store::global().holds(sha));
    graph.import_audio(session, vec![bytes("p.wav", &wav)], onto_vox(0.0)).unwrap();
    assert!(Store::global().holds(sha), "kept by the import, from its own decode");
    assert_eq!(Store::global().of(&dir.join("assets/p.wav")).unwrap().frames, 2_560);
    let _ = std::fs::remove_dir_all(&dir);
}

// Q16, Q4: one undo takes the import back and names it; the file stays.
#[test]
fn one_undo_takes_an_import_back() {
    let dir = project("undo");
    let (graph, session) = opened(&dir);
    let (a, _) = float_wav(480);
    let mut b = a.clone();
    *b.last_mut().unwrap() ^= 1;
    let done = graph.import_audio(session, vec![bytes("a.wav", &a), bytes("b.wav", &b)], onto_vox(0.0)).unwrap();
    assert_eq!(done["name"], "importing 2 files");
    assert_eq!(done["undo_name"]["name"], "importing 2 files", "{done:#}");
    assert_eq!(clips(&dir).len(), 2);
    graph.undo(session).unwrap();
    assert_eq!(clips(&dir), Vec::<Value>::new());
    assert!(dir.join("assets/a.wav").exists() && dir.join("assets/b.wav").exists(), "music2 never deletes an asset");
    let _ = std::fs::remove_dir_all(&dir);
}

// Q7 of the 8b back ends: a track an import makes under a key a gone track's mute was kept for
// starts heard, as + Track's do.
#[test]
fn a_track_an_import_makes_starts_heard() {
    let fixture = crate::graph::tests::Fixture::new();
    let from = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song");
    for f in ["music2.toml", "song.ts", "tracks/kick.ts", "tracks/hats.ts", "tracks/bass.ts", "tracks/keys.ts", "data/arrangement.json"] {
        fixture.put(f, &std::fs::read_to_string(from.join(f)).unwrap());
    }
    fixture.put(".music2/overlays.json", r#"{"mute": ["vox"], "solo": []}"#);
    let graph = fixture.open();
    let token = graph.state().unwrap()["session"].as_u64().unwrap();
    let (wav, _) = float_wav(480);
    let done = graph.import_audio(token, vec![bytes("vox.wav", &wav)], Target::Below { beat: 0.0 }).unwrap();
    assert_eq!(done["tracks"][0]["module"], "vox", "{done:#}");
    let o = &graph.state().unwrap()["overlays"];
    assert_eq!((&o["mute"], &o["heard"]["vox"]), (&serde_json::json!([]), &serde_json::json!("heard")));
}

// AGENT-2 (the review of ag-routes' F4): an agent's import is named by the words it gives, in the
// same job as the change: the step, its event and the answer all say them. With no words, the step
// has the import's own name, "importing vox.wav", as the page's has. (An audio track's, through
// `m2 call`: `apps/m2/tests/call.rs`.)
#[test]
fn an_agents_step_is_named_by_its_words_in_the_same_job() {
    use crate::caller::{Caller, as_caller};
    use crate::graph::steps::tests::{heard, steps_of};
    let dir = project("words");
    let (graph, session) = opened(&dir);
    let mut steps = steps_of(&graph);
    let (wav, _) = float_wav(4_800);
    let named = |words: Option<&str>| Caller::agent(Some("Claude"), words);
    let done = as_caller(named(Some("a vox")), || graph.import_audio(session, vec![bytes("vox.wav", &wav)], onto_vox(0.0))).unwrap();
    assert_eq!((&done["name"], &done["undo_name"]["name"]), (&json!("a vox"), &json!("a vox")), "{done:#}");
    assert_eq!(graph.state().unwrap()["undo_name"]["name"], json!("a vox"));
    // The step's event, once worked out off the session's thread.
    let told = |steps: &mut tokio::sync::broadcast::Receiver<String>| {
        let t = std::time::Instant::now();
        loop {
            let names: Vec<Value> = heard(steps).into_iter().map(|e| e["name"].clone()).collect();
            if !names.is_empty() || t.elapsed() > std::time::Duration::from_secs(10) {
                return names;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    };
    assert_eq!(told(&mut steps), [json!("a vox")], "its event says them too");
    let done =
        as_caller(named(None), || graph.import_audio(session, vec![bytes("drums.wav", &float_wav(2_400).0)], onto_vox(8.0))).unwrap();
    assert_eq!(done["undo_name"]["name"], json!("importing drums.wav"), "{done:#}");
    assert_eq!(told(&mut steps), [json!("importing drums.wav")]);
    let done = graph.import_audio(session, vec![bytes("bells.wav", &float_wav(1_200).0)], onto_vox(12.0)).unwrap();
    assert_eq!(done["undo_name"]["name"], json!("importing bells.wav"), "the page's: {done:#}");
    let _ = std::fs::remove_dir_all(&dir);
}
