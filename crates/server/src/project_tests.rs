//! Save As and Rename Song (polish C3), on a small song in a temporary folder, opened by a server
//! state that keeps its recent list in that folder too.

use std::sync::atomic::{AtomicU64, Ordering};

use super::*;
use crate::graph::Graph;

const MANIFEST: &str =
    "[project]\nname = \"Night Drive\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.5\"\n";
const SONG: &str = "import { hz, song } from '@m2/core';\nimport { osc } from 'std/dsp';\nexport default song(() => osc.sine(hz(800)));\n";

/// A temporary folder holding `songs/Night Drive` (with a `.music2/` and a `.git` of its own) and
/// the recent list's file; removed after the test.
struct Songs(PathBuf);

impl Songs {
    fn new() -> Songs {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!("m2-save-as-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&root);
        let song = root.join("songs/Night Drive");
        for (file, text) in [
            ("music2.toml", MANIFEST),
            ("song.ts", SONG),
            ("data/arrangement.json", "{\n  \"tempo\": [\n    {\"beat\": 0, \"bpm\": 124}\n  ],\n  \"meter\": []\n}\n"),
            (".music2/view.json", "{\"zoom\": 48}\n"),
            (".git/HEAD", "ref: refs/heads/main\n"),
            (".gitignore", ".music2/\n"),
        ] {
            let p = song.join(file);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        Songs(root.canonicalize().unwrap())
    }

    fn song(&self) -> PathBuf {
        self.0.join("songs/Night Drive")
    }

    /// A server state with the song open, keeping its recent list here.
    fn open(&self) -> AppState {
        let state = AppState::new(String::new(), Graph::silent());
        state.keep_recent(self.0.join("recent.json"));
        state.open_song(&self.song()).unwrap();
        state
    }
}

impl Drop for Songs {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn token(state: &AppState) -> u64 {
    state.graph.state().unwrap()["session"].as_u64().unwrap()
}

fn read(dir: &Path, file: &str) -> String {
    std::fs::read_to_string(dir.join(file)).unwrap_or_default()
}

fn names(state: &AppState) -> Vec<String> {
    state.recent().unwrap().list().into_iter().map(|s| s.name).collect()
}

#[test]
fn save_as_copies_the_song_without_its_local_folder() {
    let songs = Songs::new();
    let state = songs.open();
    let start = save_as_start(&state.graph).unwrap();
    assert_eq!((&start["name"], &start["dir"]), (&json!("Night Drive 2"), &json!(songs.0.join("songs"))));
    save_as(&state, &SaveAs { session: token(&state), name: "Night Drive 2".into(), dir: None }).unwrap();
    let copy = songs.0.join("songs/Night Drive 2");
    assert_eq!(read(&copy, "song.ts"), SONG);
    assert_eq!(read(&copy, "data/arrangement.json"), read(&songs.song(), "data/arrangement.json"));
    assert_eq!(manifest_name(&copy), "Night Drive 2");
    assert!(!copy.join(".music2/view.json").exists() && !copy.join(".git").exists(), "no .music2/ and no .git");
    assert_eq!(read(&copy, ".gitignore"), ".music2/\n");
    // Save As copies the song as it is: no AGENTS.md or CLAUDE.md it hadn't (FMT-23).
    assert!(!copy.join("AGENTS.md").exists() && !copy.join("CLAUDE.md").exists(), "Save As adds nothing");
    // The next copy counts on.
    assert_eq!(save_as_start(&state.graph).unwrap()["name"], json!("Night Drive 3"));
}

// FMT-23: a song's own AGENTS.md and CLAUDE.md are copied as they are.
#[test]
fn save_as_copies_the_songs_agents_md_as_it_is() {
    let songs = Songs::new();
    std::fs::write(songs.song().join("AGENTS.md"), "# Notes of ours\n").unwrap();
    let state = songs.open();
    save_as(&state, &SaveAs { session: token(&state), name: "Ours".into(), dir: None }).unwrap();
    let copy = songs.0.join("songs/Ours");
    assert_eq!(read(&copy, "AGENTS.md"), "# Notes of ours\n");
    assert!(!copy.join("CLAUDE.md").exists());
}

#[test]
fn save_as_opens_the_copy_and_leaves_the_original() {
    let songs = Songs::new();
    let state = songs.open();
    let before: Vec<String> =
        ["music2.toml", "song.ts", "data/arrangement.json", ".music2/view.json"].iter().map(|f| read(&songs.song(), f)).collect();
    let opened = save_as(&state, &SaveAs { session: token(&state), name: "Summer".into(), dir: None }).unwrap();
    assert_eq!(opened["name"], json!("Summer"));
    assert_eq!(state.graph.dir(), Some(songs.0.join("songs/Summer")), "the copy is the open song");
    let after: Vec<String> =
        ["music2.toml", "song.ts", "data/arrangement.json", ".music2/view.json"].iter().map(|f| read(&songs.song(), f)).collect();
    assert_eq!(after, before, "the original is as it was");
    assert_eq!(names(&state), ["Summer", "Night Drive"], "both in the recent list, the copy first");
    // Another folder; and a name whose folder is there is refused, making nothing.
    let other = songs.0.join("elsewhere");
    std::fs::create_dir_all(&other).unwrap();
    save_as(&state, &SaveAs { session: token(&state), name: "Autumn".into(), dir: Some(other.clone()) }).unwrap();
    assert_eq!(state.graph.dir(), Some(other.join("Autumn")));
    let e = save_as(&state, &SaveAs { session: token(&state), name: "night drive".into(), dir: Some(songs.0.join("songs")) })
        .unwrap_err()
        .to_string();
    assert_eq!(e, "A folder called night drive is already there. Pick another name.");
    let e = save_as(&state, &SaveAs { session: token(&state) + 1, name: "Winter".into(), dir: None }).unwrap_err().to_string();
    assert!(e.contains("open song changed"), "{e}");
    assert!(!other.join("Winter").exists());
}

// The copy can't go inside the song's own folder: it would copy itself, deeper each time, until
// the path got too long (review PL-R2). Refused before anything is made.
#[test]
fn save_as_refuses_a_folder_inside_the_song() {
    let songs = Songs::new();
    let state = songs.open();
    let song = songs.song();
    for dir in [song.clone(), song.join("data"), songs.0.join("songs/./Night Drive/data")] {
        let e = save_as(&state, &SaveAs { session: token(&state), name: "Copy".into(), dir: Some(dir.clone()) }).unwrap_err().to_string();
        assert_eq!(e, "Pick a folder outside Night Drive's own folder.", "{}", dir.display());
    }
    assert!(!song.join("Copy").exists() && !song.join("data/Copy").exists(), "nothing was made");
    assert_eq!(state.graph.dir(), Some(song.clone()), "the song stays open");
    // A folder beside it whose name starts the same is fine.
    let beside = songs.0.join("songs/Night Drive B");
    std::fs::create_dir_all(&beside).unwrap();
    save_as(&state, &SaveAs { session: token(&state), name: "Copy".into(), dir: Some(beside.clone()) }).unwrap();
    assert_eq!(state.graph.dir(), Some(beside.join("Copy")));
}

#[test]
fn rename_moves_the_folder_and_the_name() {
    let songs = Songs::new();
    let state = songs.open();
    // The name only: one undo step, the folder stays.
    let r = rename(&state, &Rename { session: token(&state), name: "Night Ride".into(), folder: false, anyway: false }).unwrap();
    assert_eq!((&r["name"], &r["dir"]), (&json!("Night Ride"), &json!(songs.song())));
    assert_eq!(manifest_name(&songs.song()), "Night Ride");
    assert_eq!(state.graph.state().unwrap()["can_undo"], json!(true));
    // With the folder: the song opens again from the new one, its .music2/ with it.
    let r = rename(&state, &Rename { session: token(&state), name: "Summer".into(), folder: true, anyway: false }).unwrap();
    let to = songs.0.join("songs/Summer");
    assert_eq!((&r["dir"], &r["warning"]), (&json!(to), &Value::Null));
    assert!(!songs.song().exists());
    assert_eq!(manifest_name(&to), "Summer");
    assert_eq!(read(&to, ".music2/view.json"), "{\"zoom\": 48}\n");
    assert_eq!(state.graph.dir(), Some(to.clone()));
    assert_eq!(state.graph.name().as_deref(), Some("Summer"));
    assert_eq!(state.recent().unwrap().paths(), std::slice::from_ref(&to), "the recent list follows");
    // Only its capitals.
    rename(&state, &Rename { session: token(&state), name: "SUMMER".into(), folder: true, anyway: false }).unwrap();
    assert_eq!(state.graph.dir(), Some(songs.0.join("songs/SUMMER")));
}

#[test]
fn rename_refuses_a_taken_folder() {
    let songs = Songs::new();
    std::fs::create_dir_all(songs.0.join("songs/summer")).unwrap();
    let state = songs.open();
    let e = rename(&state, &Rename { session: token(&state), name: "Summer".into(), folder: true, anyway: false }).unwrap_err().to_string();
    assert_eq!(e, "A folder called Summer is already there. Pick another name.");
    assert_eq!(manifest_name(&songs.song()), "Night Drive", "nothing was written");
    assert_eq!(state.graph.dir(), Some(songs.song()));
    let e = rename(&state, &Rename { session: token(&state), name: " ".into(), folder: true, anyway: false }).unwrap_err().to_string();
    assert_eq!(e, "A song needs a name");
}

// Renaming the folder closes the song, and with it the code editor's unsaved edits: refused while
// there are any, unless the request says to go ahead (review PL-R1). The name alone closes nothing.
#[test]
fn rename_with_its_folder_asks_about_unsaved_code() {
    let songs = Songs::new();
    let state = songs.open();
    state.graph.set_unsaved(token(&state), vec!["song.ts".into()]).unwrap();
    let e = rename(&state, &Rename { session: token(&state), name: "Summer".into(), folder: true, anyway: false }).unwrap_err().to_string();
    assert_eq!(
        e,
        "song.ts has unsaved edits in the code editor. Renaming the folder closes the song without saving them. Save first, or rename anyway."
    );
    assert_eq!(manifest_name(&songs.song()), "Night Drive", "nothing was written");
    assert_eq!(state.graph.dir(), Some(songs.song()));
    state.graph.set_unsaved(token(&state), vec!["song.ts".into(), "tracks/bass.ts".into()]).unwrap();
    let e = rename(&state, &Rename { session: token(&state), name: "Summer".into(), folder: true, anyway: false }).unwrap_err().to_string();
    assert!(e.starts_with("2 files have unsaved edits in the code editor."), "{e}");
    // The name only: nothing closes, so nothing is lost.
    rename(&state, &Rename { session: token(&state), name: "Night Ride".into(), folder: true, anyway: false }).unwrap_err();
    rename(&state, &Rename { session: token(&state), name: "Night Ride".into(), folder: false, anyway: false }).unwrap();
    assert_eq!(manifest_name(&songs.song()), "Night Ride");
    // Asked to go ahead: it does.
    rename(&state, &Rename { session: token(&state), name: "Summer".into(), folder: true, anyway: true }).unwrap();
    assert_eq!(state.graph.dir(), Some(songs.0.join("songs/Summer")));
    assert!(state.graph.unsaved().is_empty(), "the song opened again: a new editor");
}

#[test]
fn rename_in_a_read_only_window_is_refused() {
    let songs = Songs::new();
    let held = m2_session::lock::acquire(&songs.song(), "m2", m2_session::lock::RETRY).unwrap();
    let state = songs.open();
    assert!(state.graph.read_only().is_some());
    let e = rename(&state, &Rename { session: token(&state), name: "Summer".into(), folder: true, anyway: false }).unwrap_err().to_string();
    assert!(e.contains("open for writing"), "{e}");
    assert_eq!(manifest_name(&songs.song()), "Night Drive");
    assert!(!songs.0.join("songs/Summer").exists());
    // Save As only reads the song: a read-only window has it.
    save_as(&state, &SaveAs { session: token(&state), name: "Summer".into(), dir: None }).unwrap();
    assert_eq!(state.graph.dir(), Some(songs.0.join("songs/Summer")));
    assert!(state.graph.read_only().is_none(), "the copy is this window's to write");
    drop(held);
}

fn manifest_name(dir: &Path) -> String {
    let doc: toml::Table = toml::from_str(&std::fs::read_to_string(dir.join("music2.toml")).unwrap()).unwrap();
    doc["project"]["name"].as_str().unwrap().to_string()
}
