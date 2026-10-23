//! Stems (C3, Q39, Q40): a folder of one file per track, as `m2 bounce --track` writes it; tracks
//! muted in the song left out; a song that plays plugins refused in words; one export at a time.

use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

use super::*;
use crate::export::tests::until_finished;
use crate::graph::Graph;

struct Temp(PathBuf);

impl Temp {
    fn new() -> Temp {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!("m2-stems-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        Temp(dir)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let (f, t) = (e.path(), to.join(e.file_name()));
        if f.is_dir() { copy(&f, &t) } else { std::fs::copy(&f, &t).map(drop).unwrap() }
    }
}

/// The app's state with a copy of first-song open in `temp`, changed by `edit`; and its session.
fn first_song(temp: &Temp, edit: impl FnOnce(&Path)) -> (Arc<AppState>, u64) {
    let song = temp.0.join("first-song");
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song"), &song);
    edit(&song);
    let state = Arc::new(AppState::new(String::new(), Graph::silent()));
    let token = state.graph.open(&song).unwrap()["session"].as_u64().unwrap();
    (state, token)
}

/// The names of the files in `dir`, sorted.
fn names(dir: &Path) -> Vec<String> {
    let mut n: Vec<String> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    n.sort();
    n
}

// Q39: one 32-bit float file per track, in the tracks' order, numbered, in a new folder in
// Exports; what each stem's progress says; a second export takes the next folder.
#[test]
fn stems_write_one_file_per_track_in_a_folder() {
    let temp = Temp::new();
    let (state, token) = first_song(&temp, |_| {});
    let exports = temp.0.join("Exports");
    let mut events = state.events.subscribe();
    let folder = start_in(&state, token, &exports).unwrap();
    assert_eq!(folder, exports.join("first-song stems"));
    assert!(state.exports.running() && state.exports.song().as_deref() == Some("first-song"));
    let seen = until_finished(&mut events);
    let done = seen.last().unwrap();
    assert_eq!((&done["done"], &done["count"], &done["stems"]), (&json!(true), &json!(4), &json!(true)), "{done}");
    assert_eq!(done["path"].as_str(), folder.to_str());
    assert_eq!(names(&folder), ["1 Kick.wav", "2 Hats.wav", "3 Bass.wav", "4 Keys.wav"]);
    // 32 beats at 124 bpm, and 2 s: as `m2 bounce` renders.
    let frames = ((32.0 * 60.0 / 124.0 + 2.0) * 48_000.0f64).round() as u32;
    for n in names(&folder) {
        let wav = hound::WavReader::open(folder.join(&n)).unwrap();
        let spec = wav.spec();
        assert_eq!(
            (spec.channels, spec.bits_per_sample, spec.sample_format, wav.duration()),
            (2, 32, hound::SampleFormat::Float, frames),
            "{n}"
        );
    }
    // Each stem says it started, in order, with how many there are; progress only goes forward.
    let started: Vec<(String, u64)> = seen
        .iter()
        .filter(|e| e.get("progress").is_some())
        .map(|e| (e["stem"].as_str().unwrap().to_string(), e["n"].as_u64().unwrap()))
        .fold(vec![], |mut v, s| {
            if v.last() != Some(&s) {
                v.push(s);
            }
            v
        });
    assert_eq!(started, [("Kick".into(), 1), ("Hats".into(), 2), ("Bass".into(), 3), ("Keys".into(), 4)]);
    assert!(seen.iter().filter(|e| e.get("progress").is_some()).all(|e| e["of"] == 4 && e["stems"] == true));
    let progress: Vec<f64> = seen.iter().filter_map(|e| e["progress"].as_f64()).collect();
    assert!(progress.windows(2).all(|w| w[0] <= w[1]) && progress.iter().all(|p| (0.0..1.0).contains(p)), "{progress:?}");
    assert!(!state.exports.running());
    // Again: a folder of its own.
    assert_eq!(start_in(&state, token, &exports).unwrap(), exports.join("first-song stems 2"));
    assert_eq!(until_finished(&mut events).pop().unwrap()["count"], 4);
}

// Q39: a track muted in the song has no stem, as the audio export leaves it out.
#[test]
fn stems_leave_out_tracks_muted_in_the_song() {
    let temp = Temp::new();
    let (state, token) = first_song(&temp, |song| {
        let hats = std::fs::read_to_string(song.join("tracks/hats.ts")).unwrap();
        std::fs::write(song.join("tracks/hats.ts"), hats.replace("{ name: \"Hats\" }", "{ name: \"Hats\", muted: true }")).unwrap();
    });
    let exports = temp.0.join("Exports");
    let mut events = state.events.subscribe();
    let folder = start_in(&state, token, &exports).unwrap();
    assert_eq!(until_finished(&mut events).pop().unwrap()["count"], 3);
    assert_eq!(names(&folder), ["1 Kick.wav", "2 Bass.wav", "3 Keys.wav"]);
}

// Q40: a song that plays plugins is refused in words, with nothing made and the slot free; the
// dialog's question answers the same words.
#[test]
fn stems_of_a_song_with_plugins_are_refused_in_words() {
    m2_plugins::probe::install();
    let temp = Temp::new();
    let (state, token) = first_song(&temp, |song| {
        std::fs::write(
            song.join("tracks/keys.ts"),
            "import { plugin, track } from \"@m2/core\";\n\n\
             export default track(\"keys\", { name: \"Keys\" }, ({ lane }) => plugin(\"clap:m2.probe\", { notes: lane.notes(\"chords\") }));\n",
        )
        .unwrap();
    });
    let exports = temp.0.join("Exports");
    let e = start_in(&state, token, &exports).unwrap_err().to_string();
    assert!(e.starts_with("Not yet for a song that plays plugins (") && e.ends_with(")."), "{e}");
    assert!(!exports.join("first-song stems").exists());
    assert!(!state.exports.running());
    assert_eq!(state.graph.stems_refused(token).unwrap().as_deref(), Some(e.as_str()));
    // first-song as it was: none.
    let temp = Temp::new();
    let (state, token) = first_song(&temp, |_| {});
    assert_eq!(state.graph.stems_refused(token).unwrap(), None);
}

// One export at a time, of either kind: stems while a song exports are refused, and the reverse;
// quitting stops stems and leaves nothing behind.
#[test]
fn one_export_at_a_time() {
    let temp = Temp::new();
    let (state, token) = first_song(&temp, |_| {});
    let exports = temp.0.join("Exports");
    let mut events = state.events.subscribe();
    crate::export::start(&state, Some(temp.0.join("song.wav"))).unwrap();
    let e = start_in(&state, token, &exports).unwrap_err().to_string();
    assert!(e.contains("already being exported"), "{e}");
    assert!(!exports.join("first-song stems").exists());
    until_finished(&mut events);
    start_in(&state, token, &exports).unwrap();
    let e = crate::export::start(&state, Some(temp.0.join("again.wav"))).unwrap_err().to_string();
    assert!(e.contains("already being exported"), "{e}");
    // Quitting stops them: no stem, no folder.
    assert!(state.exports.stop(Duration::from_secs(20)), "they stopped");
    let end: Value = until_finished(&mut events).pop().unwrap();
    assert!(end["error"].as_str().unwrap().contains("stopped"), "{end}");
    assert!(!exports.join("first-song stems").exists(), "{:?}", names(&exports));
    assert!(!state.exports.running());
    // Another session's window is refused, the slot left free.
    assert!(start_in(&state, token + 1, &exports).unwrap_err().to_string().contains("changed"));
    assert!(!state.exports.running());
}
