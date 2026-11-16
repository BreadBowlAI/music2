//! Undo kept with the song (WRITE-23): a change `m2` makes while the app is closed is a step of the
//! song's one history, the agent's, by the name it gives (AGENT-10), which the app reads back when
//! it opens the song, and undoes.

mod app;

use app::{App, m2_in, said};
use serde_json::json;

#[test]
fn a_change_m2_made_while_the_app_was_closed_is_undone_in_the_app() {
    let mut before = String::new();
    let a = App::open("first-song", |song| {
        before = std::fs::read_to_string(song.join("data/arrangement.json")).unwrap();
        let home = song.parent().unwrap().join("m2-home");
        let o = m2_in(&home, &["tempo", song.to_str().unwrap(), "128", "--as", "Claude", "--why", "faster"], &[]);
        assert!(o.status.success(), "{}", said(&o));
    });
    let top = a.top();
    assert_eq!((&top["source"], &top["by"], &top["name"]), (&json!("agent"), &json!("Claude"), &json!("faster")), "{top}");
    assert_ne!(a.read("data/arrangement.json"), before);
    let o = a.m2(&["undo", a.dir(), "--as", "Claude"]);
    assert!(o.status.success(), "{}", said(&o));
    assert_eq!(a.read("data/arrangement.json"), before, "undone in the app, byte for byte");
}

// WRITE-23 (review U6): what `m2` tells agents says so: a write with the app closed is an undo step
// of the song's history, never "undo isn't kept".
#[test]
fn m2s_notes_say_a_write_with_the_app_closed_is_kept_for_undo() {
    let home = std::env::temp_dir().join(format!("m2-undo-notes-{}", std::process::id()));
    let o = m2_in(&home, &["docs", "commands"], &[]);
    let notes = String::from_utf8_lossy(&o.stdout).replace('\n', " ");
    assert!(o.status.success(), "{}", said(&o));
    assert!(!notes.contains("undo isn't kept"), "the notes still say undo isn't kept");
    assert!(notes.contains("an undo step of the song's history"), "{notes}");
}

// AGENT-4, WRITE-23: with the app closed, `m2 undo` takes back the agent's own newest step from the
// song's history, by the rule the app keeps: another agent's step is refused, changing nothing.
#[test]
fn m2_undo_with_the_app_closed_takes_back_only_its_own_newest_step() {
    let home = std::env::temp_dir().join(format!("m2-undo-closed-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    let song = home.join("song");
    let dir = song.to_str().unwrap();
    let o = m2_in(&home, &["new", dir, "--example", "first-song"], &[]);
    assert!(o.status.success(), "{}", said(&o));
    let read = || std::fs::read_to_string(song.join("data/arrangement.json")).unwrap();
    let before = read();
    let o = m2_in(&home, &["tempo", dir, "128", "--as", "Claude", "--why", "faster"], &[]);
    assert!(o.status.success(), "{}", said(&o));
    let after = read();
    assert_ne!(after, before);

    let o = m2_in(&home, &["undo", dir, "--as", "Codex"], &[]);
    assert!(!o.status.success() && said(&o).contains("Claude's (faster), not Codex's"), "{}", said(&o));
    assert_eq!(read(), after, "another agent's step stays");

    let o = m2_in(&home, &["undo", dir, "--as", "Claude"], &[]);
    assert!(o.status.success(), "{}", said(&o));
    assert!(String::from_utf8_lossy(&o.stdout).contains("undid faster"), "{}", said(&o));
    assert_eq!(read(), before, "undone, byte for byte");

    let o = m2_in(&home, &["undo", dir, "--as", "Claude"], &[]);
    assert!(!o.status.success() && said(&o).contains("nothing to undo"), "{}", said(&o));
    let _ = m2_song::pack::remove_tree(&home);
}
