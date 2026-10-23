//! `m2 edit` and `m2 write` (AGENT-1, the agents design's 5.3): a change to a song file as a Tx with
//! a base, which the app can refuse, name and let the agent undo while the song is open, and which
//! `m2` writes itself while it is closed. `m2 edit` replaces text found exactly once (or every time,
//! with `--all`); `m2 write` writes a whole text file.

mod app;

use std::path::Path;

use app::{App, Temp, copy, example, fake, m2_in, packages, publish, said};
use serde_json::json;

// 5.3: text found once is replaced, as one undo step in the app, named by the agent's words.
#[test]
fn m2_edit_replaces_text_found_once_as_one_named_step() {
    let a = App::open("first-song", |_| {});
    let o = a.m2(&["edit", a.dir(), "tracks/kick.ts", "--old", "ms(150)", "--new", "ms(120)", "--as", "Claude", "--why", "tighter kick"]);
    assert!(o.status.success(), "{}", said(&o));
    assert!(said(&o).contains("in the open app: one undo step, Claude: tighter kick"), "{}", said(&o));
    assert!(a.read("tracks/kick.ts").contains("release: ms(120)"));
    let top = a.top();
    assert_eq!((&top["source"], &top["by"], &top["name"]), (&json!("agent"), &json!("Claude"), &json!("tighter kick")), "{top}");
    let o = a.m2(&["undo", a.dir(), "--as", "Claude"]);
    assert!(o.status.success(), "{}", said(&o));
    assert!(a.read("tracks/kick.ts").contains("release: ms(150)"), "the agent takes it back");
}

// 5.3: text found more than once is refused, saying how many times, unless `--all`; text not found
// is refused too. Nothing changes.
#[test]
fn m2_edit_refuses_text_found_twice() {
    let temp = Temp::new("edit-twice");
    let song = temp.0.join("song");
    copy(&example("first-song"), &song);
    let dir = song.to_str().unwrap();
    let home = temp.0.join("home");
    let before = std::fs::read_to_string(song.join("song.ts")).unwrap();
    let o = m2_in(&home, &["edit", dir, "song.ts", "--old", "pan:", "--new", "pan :"], &[]);
    assert_eq!(o.status.code(), Some(1), "{}", said(&o));
    assert!(said(&o).contains("found 2 times") && said(&o).contains("--all"), "{}", said(&o));
    let o = m2_in(&home, &["edit", dir, "song.ts", "--old", "no such text", "--new", "x"], &[]);
    assert_eq!(o.status.code(), Some(1), "{}", said(&o));
    assert!(said(&o).contains("isn't in song.ts"), "{}", said(&o));
    assert_eq!(std::fs::read_to_string(song.join("song.ts")).unwrap(), before, "nothing changed");
    let o = m2_in(&home, &["edit", dir, "song.ts", "--old", "pan: ", "--new", "pan:  ", "--all"], &[]);
    assert!(o.status.success(), "{}", said(&o));
    assert_eq!(std::fs::read_to_string(song.join("song.ts")).unwrap().matches("pan:  ").count(), 2, "every time");
}

// 5.3: a file that changed after `m2 edit` read it (409) is read again, and the edit tried once more,
// since "replace this text" still means the same.
#[test]
fn m2_edit_on_a_stale_file_reads_again_once() {
    let temp = Temp::new("edit-stale");
    let song = temp.0.join("song");
    copy(&example("first-song"), &song);
    let mut writer = Some(m2_session::Session::open(&song, packages()).unwrap());
    let state = json!({"session": 1, "dir": song.canonicalize().unwrap(), "problems": []});
    let file = song.join("tracks/kick.ts");
    let mut first = true;
    let (url, asked) = fake(state, move |(_, path, body)| {
        if path != "/api/tx/commit" {
            return Some((404, json!({})));
        }
        if std::mem::take(&mut first) {
            // Another writer changes the file elsewhere: this Tx's base no longer applies.
            let text = std::fs::read_to_string(&file).unwrap();
            std::fs::write(&file, format!("// a comment\n{text}")).unwrap();
            return Some((
                409,
                json!({"conflict": {"files": [{"file": "tracks/kick.ts", "current": null, "clip": null, "why": null}], "message": "changed"}}),
            ));
        }
        let s = writer.as_mut().unwrap();
        s.refresh();
        let tx = m2_song::Tx::from_json(&body["tx"]).unwrap();
        let c = s.commit(tx).unwrap();
        Some((200, json!({"committed": {"revision": c.number, "code_rev": c.code_rev, "data_rev": c.data_rev}, "step": s.last_step()})))
    });
    let home = temp.0.join("home");
    publish(&home, &url, "tools", std::process::id());
    let o = m2_in(&home, &["edit", song.to_str().unwrap(), "tracks/kick.ts", "--old", "ms(150)", "--new", "ms(120)"], &[]);
    assert!(o.status.success(), "{}", said(&o));
    let text = std::fs::read_to_string(song.join("tracks/kick.ts")).unwrap();
    assert!(text.starts_with("// a comment") && text.contains("ms(120)"), "both: {text}");
    assert_eq!(asked.lock().unwrap().iter().filter(|a| a.1 == "/api/tx/commit").count(), 2, "tried once more");
}

// 5.3: `m2 write` makes a new text file as one step in the app, and overwrites one that is there.
#[test]
fn m2_write_makes_a_new_file_as_one_step() {
    let a = App::open("first-song", |_| {});
    let from = a.temp.0.join("lead.ts");
    std::fs::write(&from, "// a lead, soon\n").unwrap();
    let o = a.m2(&["write", a.dir(), "tracks/lead.ts", from.to_str().unwrap(), "--as", "Claude"]);
    assert!(o.status.success(), "{}", said(&o));
    assert_eq!(a.read("tracks/lead.ts"), "// a lead, soon\n");
    assert_eq!((&a.top()["source"], &a.top()["by"]), (&json!("agent"), &json!("Claude")));
    let o = a.m2(&["undo", a.dir(), "--as", "Claude"]);
    assert!(o.status.success(), "{}", said(&o));
    assert!(!a.song.join("tracks/lead.ts").exists(), "its undo takes the new file away");
}

// 5.3: `m2 write` writes text: an asset or a plugin's state is refused, naming the command that
// writes it.
#[test]
fn m2_write_refuses_a_binary_file() {
    let temp = Temp::new("write-binary");
    let song = temp.0.join("song");
    copy(&example("first-song"), &song);
    let from = temp.0.join("x.txt");
    std::fs::write(&from, "text").unwrap();
    let home = temp.0.join("home");
    for (file, names) in [("assets/x.wav", "m2 import"), ("data/keys/lead.state", "m2 plugin-state")] {
        let o = m2_in(&home, &["write", song.to_str().unwrap(), file, from.to_str().unwrap()], &[]);
        assert_eq!(o.status.code(), Some(1), "{}", said(&o));
        assert!(said(&o).contains(names), "{file}: {}", said(&o));
        assert!(!song.join(file).exists());
    }
}

// 5.3: with the app closed, `m2 edit` writes the file itself, as every `m2` write does.
#[test]
fn m2_edit_works_with_the_app_closed() {
    let temp = Temp::new("edit-closed");
    let song = temp.0.join("song");
    copy(&example("first-song"), &song);
    let o = m2_in(&temp.0.join("home"), &["edit", song.to_str().unwrap(), "tracks/kick.ts", "--old", "ms(150)", "--new", "ms(120)"], &[]);
    assert!(o.status.success(), "{}", said(&o));
    assert!(!said(&o).contains("open app"), "{}", said(&o));
    assert!(std::fs::read_to_string(Path::new(&song).join("tracks/kick.ts")).unwrap().contains("ms(120)"));
}
