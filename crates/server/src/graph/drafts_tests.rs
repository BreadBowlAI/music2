//! The code editor's unsaved text (WRITE-27): kept in `.music2/drafts/`, never in the song's
//! files; offered back when the song opens again, as changed when its file changed since; dropped
//! when it is the file's text; kept by no read-only window. That a killed server leaves it is
//! `e2e/tr-trust.mjs`'s.

use serde_json::{Value, json};

use super::super::steps::tests::first_song_files;

fn token(graph: &super::super::Graph) -> u64 {
    graph.state().unwrap()["session"].as_u64().unwrap()
}

fn drafts_dir(fixture: &super::super::tests::Fixture) -> std::path::PathBuf {
    fixture.0.join(".music2/drafts")
}

// WRITE-27: a draft is kept as it is typed, never in the song's files; the song opened again offers
// it back, in the state (its file) and in the code editor's files (its text, unchanged); Restore,
// which keeps it again, or Discard answer it.
#[test]
fn a_draft_is_kept_and_offered_back_when_the_song_opens() {
    let fixture = first_song_files();
    let graph = fixture.open();
    let song = fixture.read("song.ts");
    let typed = format!("{song}\nconst half = ");
    graph.set_draft(token(&graph), "song.ts", &song, Some(&typed)).unwrap();
    assert_eq!(fixture.read("song.ts"), song, "never the song's file");
    assert_eq!(graph.state().unwrap()["drafts"], json!([]), "a draft of this window's isn't left");
    graph.close();

    graph.open(&fixture.0).unwrap();
    assert_eq!(graph.state().unwrap()["drafts"], json!(["song.ts"]));
    assert_eq!(graph.files().unwrap()["left"], json!([{"file": "song.ts", "text": typed, "changed": false}]));
    // Restored: the editor keeps it again, so it is no longer left.
    graph.set_draft(token(&graph), "song.ts", &song, Some(&typed)).unwrap();
    assert_eq!((graph.state().unwrap()["drafts"].clone(), graph.files().unwrap()["left"].clone()), (json!([]), json!([])));
    graph.close();

    graph.open(&fixture.0).unwrap();
    assert_eq!(graph.state().unwrap()["drafts"], json!(["song.ts"]), "still there, as it isn't saved");
    // Discarded.
    graph.set_draft(token(&graph), "song.ts", &song, None).unwrap();
    assert_eq!(graph.state().unwrap()["drafts"], json!([]));
    assert_eq!(std::fs::read_dir(drafts_dir(&fixture)).unwrap().count(), 0, "gone from disk");
    graph.close();
    graph.open(&fixture.0).unwrap();
    assert_eq!(graph.files().unwrap()["left"], json!([]));
}

// WRITE-27: a draft whose file changed since it was based on it, or is gone, is offered as changed,
// never applied; one whose text is the file's now is dropped without a word.
#[test]
fn a_draft_whose_file_changed_is_offered_as_changed() {
    let fixture = first_song_files();
    let graph = fixture.open();
    let (song, bass) = (fixture.read("song.ts"), fixture.read("tracks/bass.ts"));
    let t = token(&graph);
    graph.set_draft(t, "song.ts", &song, Some(&format!("{song}// mine\n"))).unwrap();
    graph.set_draft(t, "tracks/bass.ts", &bass, Some(&format!("{bass}// mine\n"))).unwrap();
    graph.set_draft(t, "tracks/keys.ts", &fixture.read("tracks/keys.ts"), Some("// all mine\n")).unwrap();
    graph.close();
    fixture.put("song.ts", &format!("{song}// theirs\n"));
    std::fs::remove_file(fixture.0.join("tracks/keys.ts")).unwrap();
    fixture.put("tracks/bass.ts", &format!("{bass}// mine\n"));

    graph.open(&fixture.0).unwrap();
    let left = graph.files().unwrap()["left"].clone();
    let said: Vec<(Value, Value)> = left.as_array().unwrap().iter().map(|d| (d["file"].clone(), d["changed"].clone())).collect();
    assert_eq!(said, [(json!("song.ts"), json!(true)), (json!("tracks/keys.ts"), json!(true))], "{left}");
    assert_eq!(fixture.read("song.ts"), format!("{song}// theirs\n"), "never applied");
    assert_eq!(std::fs::read_dir(drafts_dir(&fixture)).unwrap().count(), 2, "the draft that is the file's went");
}

// WRITE-27: only a code file of the song has a draft; a read-only window keeps none and offers none.
#[test]
fn a_read_only_window_keeps_no_draft() {
    let fixture = first_song_files();
    let graph = fixture.open();
    let song = fixture.read("song.ts");
    assert!(graph.set_draft(token(&graph), "data/bass/notes.json", "", Some("{}")).is_err(), "not code");
    assert!(graph.set_draft(token(&graph), "../song.ts", "", Some("x")).is_err(), "not the song's");
    graph.set_draft(token(&graph), "song.ts", &song, Some("// mine\n")).unwrap();
    graph.close();
    let held = m2_session::lock::acquire(&fixture.0, "m2", m2_session::lock::RETRY).unwrap();
    graph.open(&fixture.0).unwrap();
    assert!(!graph.state().unwrap()["read_only"].is_null());
    assert_eq!((graph.state().unwrap()["drafts"].clone(), graph.files().unwrap()["left"].clone()), (json!([]), json!([])));
    graph.set_draft(token(&graph), "song.ts", &song, None).unwrap();
    drop(held);
    assert_eq!(std::fs::read_dir(drafts_dir(&fixture)).unwrap().count(), 1, "the writer's draft stays");
}
