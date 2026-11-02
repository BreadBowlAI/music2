//! The code editor's unsaved text (WRITE-27): kept in `.music2/drafts/`, each window's its own,
//! never in the song's files; moved aside when the song opens again and offered back, as changed
//! when its file changed since, until Restore or Discard answers it, whatever is typed meanwhile;
//! dropped when it is the file's text; kept by no read-only window, and offered once one takes the
//! lock; never shown to an agent. That a killed server leaves it is `e2e/tr-trust.mjs`'s.

use serde_json::{Value, json};

use super::super::steps::tests::{first_song_files, serving};

fn token(graph: &super::super::Graph) -> u64 {
    graph.state().unwrap()["session"].as_u64().unwrap()
}

fn drafts_dir(fixture: &super::super::tests::Fixture) -> std::path::PathBuf {
    fixture.0.join(".music2/drafts")
}

/// The drafts the code editor is offered: each one's file and text.
fn left(graph: &super::super::Graph) -> Vec<(String, String)> {
    let files = graph.files().unwrap();
    files["left"].as_array().unwrap().iter().map(|d| (d["file"].as_str().unwrap().into(), d["text"].as_str().unwrap().into())).collect()
}

fn ids(graph: &super::super::Graph) -> Vec<String> {
    graph.files().unwrap()["left"].as_array().unwrap().iter().map(|d| d["id"].as_str().unwrap().to_string()).collect()
}

// WRITE-27: a draft is kept as it is typed, never in the song's files; the song opened again offers
// it back, in the state (its file) and in the code editor's files (its text, unchanged), until it is
// answered.
#[test]
fn a_draft_is_kept_and_offered_back_when_the_song_opens() {
    let fixture = first_song_files();
    let graph = fixture.open();
    let song = fixture.read("song.ts");
    let typed = format!("{song}\nconst half = ");
    graph.set_draft(token(&graph), "w1", "song.ts", &song, Some(&typed)).unwrap();
    assert_eq!(fixture.read("song.ts"), song, "never the song's file");
    assert_eq!(graph.state().unwrap()["drafts"], json!([]), "a draft of this window's isn't left");
    graph.close();

    graph.open(&fixture.0).unwrap();
    assert_eq!(graph.state().unwrap()["drafts"], json!(["song.ts"]));
    let files = graph.files().unwrap();
    assert_eq!(
        (&files["left"][0]["file"], &files["left"][0]["text"], &files["left"][0]["changed"]),
        (&json!("song.ts"), &json!(typed), &json!(false))
    );
    graph.close();
    graph.open(&fixture.0).unwrap();
    assert_eq!(left(&graph), [("song.ts".to_string(), typed.clone())], "still offered: nothing answered it");
    graph.answer_draft(token(&graph), &ids(&graph)[0]).unwrap();
    assert_eq!((graph.state().unwrap()["drafts"].clone(), left(&graph)), (json!([]), vec![]), "answered");
    graph.close();
    graph.open(&fixture.0).unwrap();
    assert_eq!(left(&graph), vec![], "for good");
}

// WRITE-27 (review U5): typing in a file whose draft is offered keeps the offer, and the draft it
// offers, on disk: what was typed is a draft of its own, offered too if the song closes unanswered.
#[test]
fn typing_over_an_offered_draft_never_throws_it_away() {
    let fixture = first_song_files();
    let graph = fixture.open();
    let song = fixture.read("song.ts");
    graph.set_draft(token(&graph), "w1", "song.ts", &song, Some(&format!("{song}// crash-saved\n"))).unwrap();
    graph.close();
    graph.open(&fixture.0).unwrap();
    graph.set_draft(token(&graph), "w2", "song.ts", &song, Some(&format!("{song}// typed since\n"))).unwrap();
    assert_eq!(left(&graph), [("song.ts".to_string(), format!("{song}// crash-saved\n"))], "the offer stays");
    graph.close();
    graph.open(&fixture.0).unwrap();
    let mut offered: Vec<String> = left(&graph).into_iter().map(|(_, t)| t).collect();
    offered.sort();
    assert_eq!(offered, [format!("{song}// crash-saved\n"), format!("{song}// typed since\n")], "both, unanswered");
}

// WRITE-27 (review U11): each window keeps its own draft of a file: one going clean leaves the
// other's.
#[test]
fn each_window_keeps_its_own_draft() {
    let fixture = first_song_files();
    let graph = fixture.open();
    let song = fixture.read("song.ts");
    graph.set_draft(token(&graph), "w1", "song.ts", &song, Some(&format!("{song}// one\n"))).unwrap();
    graph.set_draft(token(&graph), "w2", "song.ts", &song, Some(&format!("{song}// two\n"))).unwrap();
    graph.set_draft(token(&graph), "w2", "song.ts", &song, None).unwrap();
    graph.close();
    graph.open(&fixture.0).unwrap();
    assert_eq!(left(&graph), [("song.ts".to_string(), format!("{song}// one\n"))]);
}

// WRITE-27: a draft whose file changed since it was based on it, or is gone, is offered as changed,
// never applied; one whose text is the file's now is dropped without a word.
#[test]
fn a_draft_whose_file_changed_is_offered_as_changed() {
    let fixture = first_song_files();
    let graph = fixture.open();
    let (song, bass) = (fixture.read("song.ts"), fixture.read("tracks/bass.ts"));
    let t = token(&graph);
    graph.set_draft(t, "w1", "song.ts", &song, Some(&format!("{song}// mine\n"))).unwrap();
    graph.set_draft(t, "w1", "tracks/bass.ts", &bass, Some(&format!("{bass}// mine\n"))).unwrap();
    graph.set_draft(t, "w1", "tracks/keys.ts", &fixture.read("tracks/keys.ts"), Some("// all mine\n")).unwrap();
    graph.close();
    fixture.put("song.ts", &format!("{song}// theirs\n"));
    std::fs::remove_file(fixture.0.join("tracks/keys.ts")).unwrap();
    fixture.put("tracks/bass.ts", &format!("{bass}// mine\n"));

    graph.open(&fixture.0).unwrap();
    let left = graph.files().unwrap()["left"].clone();
    let said: Vec<(Value, Value)> = left.as_array().unwrap().iter().map(|d| (d["file"].clone(), d["changed"].clone())).collect();
    assert_eq!(said, [(json!("song.ts"), json!(true)), (json!("tracks/keys.ts"), json!(true))], "{left}");
    assert_eq!(fixture.read("song.ts"), format!("{song}// theirs\n"), "never applied");
    assert_eq!(std::fs::read_dir(drafts_dir(&fixture).join("left")).unwrap().count(), 2, "the draft that is the file's went");
}

// WRITE-27: only a code file of the song has a draft; a read-only window keeps none and offers none,
// and once Retry takes the lock, it offers what the song was left with (review U5).
#[test]
fn a_read_only_window_keeps_no_draft_until_it_takes_the_lock() {
    let fixture = first_song_files();
    let graph = fixture.open();
    let song = fixture.read("song.ts");
    assert!(graph.set_draft(token(&graph), "w1", "data/bass/notes.json", "", Some("{}")).is_err(), "not code");
    assert!(graph.set_draft(token(&graph), "w1", "../song.ts", "", Some("x")).is_err(), "not the song's");
    assert!(graph.answer_draft(token(&graph), "../../x").is_err(), "not a draft's id");
    graph.set_draft(token(&graph), "w1", "song.ts", &song, Some("// mine\n")).unwrap();
    graph.close();
    let held = m2_session::lock::acquire(&fixture.0, "m2", m2_session::lock::RETRY).unwrap();
    graph.open(&fixture.0).unwrap();
    assert!(!graph.state().unwrap()["read_only"].is_null());
    assert_eq!((graph.state().unwrap()["drafts"].clone(), left(&graph)), (json!([]), vec![]));
    graph.set_draft(token(&graph), "w1", "song.ts", &song, None).unwrap();
    assert_eq!(std::fs::read_dir(drafts_dir(&fixture)).unwrap().count(), 1, "the writer's draft stays");
    drop(held);
    graph.retry_lock(token(&graph)).unwrap();
    assert_eq!(left(&graph), [("song.ts".to_string(), "// mine\n".to_string())], "offered once the lock is taken");
    assert_eq!(graph.state().unwrap()["drafts"], json!(["song.ts"]));
}

// WRITE-27, AGENT-11: the text left unsaved is the musician's: the code editor's files leave it out
// for an agent's key.
#[test]
fn an_agent_never_reads_the_unsaved_code() {
    let (fixture, running, session) = serving();
    let graph = &running.state.graph;
    let song = fixture.read("song.ts");
    graph.set_draft(session, "w1", "song.ts", &song, Some("// mine\n")).unwrap();
    graph.close();
    graph.open(&fixture.0).unwrap();
    let page = crate::routes_8b::tests::get(&running.url, "/api/graph/files", "t0k");
    let agent = crate::routes_8b::tests::get(&running.url, "/api/graph/files", "t00ls");
    assert_eq!(page["left"][0]["text"], json!("// mine\n"), "{page}");
    assert!(agent.get("left").is_none() && agent["files"].is_array(), "an agent reads the files, not the draft");
}
