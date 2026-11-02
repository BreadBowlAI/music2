//! `m2 clips` (8b.5, WRITE-18): clip gestures from the terminal, by id or by place, each one edit;
//! and a refusal that changes nothing while another process holds the project.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let (f, t) = (e.path(), to.join(e.file_name()));
        if f.is_dir() { copy(&f, &t) } else { std::fs::copy(&f, &t).map(drop).unwrap() }
    }
}

struct Temp(PathBuf);

impl Drop for Temp {
    fn drop(&mut self) {
        // Read-only files too: a pack m2 installed in a HOME here (PACK-16).
        let _ = m2_song::pack::remove_tree(&self.0);
    }
}

fn m2(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_m2")).env("HOME", m2_song::pack::test_home()).args(args).output().unwrap()
}

fn said(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

/// Run `m2 clips <dir> args…`, which must succeed: what it said.
fn ok(dir: &str, args: &[&str]) -> String {
    let o = m2(&[&["clips", dir][..], args].concat());
    assert!(o.status.success(), "{args:?}: {}", said(&o));
    said(&o)
}

#[test]
fn clip_gestures_from_the_terminal() {
    let temp = Temp(std::env::temp_dir().join(format!("m2-clips-cli-{}", std::process::id())));
    let song = temp.0.join("first-song");
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song"), &song);
    let (dir, hits) = (song.to_str().unwrap(), song.join("data/kick/hits.json"));
    let read = || std::fs::read_to_string(&hits).unwrap();
    let before = read();

    // By id, at a place: bar 9 is beat 32 in 4/4.
    let out = ok(dir, &["move", "kick/hits:my53vvkc", "--by", "32"]);
    assert!(out.contains("clip at bar 1 (8 bars) → clip at bar 9 (8 bars)"), "{out}");
    assert!(read().contains("{\"id\": \"my53vvkc\", \"start\": 32, \"length\": 32"));
    // By place: the clip that starts at bar 9; a copy made by duplicate lands at bar 17.
    let out = ok(dir, &["duplicate", "kick/hits@9"]);
    let made = out.lines().find_map(|l| l.trim().strip_prefix("made kick/hits:")).unwrap_or_else(|| panic!("{out}")).to_string();
    assert!(read().contains(&format!("{{\"id\": \"{made}\", \"start\": 64, \"length\": 32")), "{}", read());
    ok(dir, &["delete", &format!("kick/hits:{made}")]);
    ok(dir, &["move", "kick/hits:my53vvkc", "--by", "-32"]);
    assert_eq!(read(), before, "moved back");
    // The left edge to bar 1 beat 2, and back: the notes stay where they play.
    ok(dir, &["resize", "kick/hits:my53vvkc", "--start", "1.2", "--length", "31"]);
    assert!(read().contains("{\"id\": \"my53vvkc\", \"start\": 1, \"length\": 31"), "{}", read());
    ok(dir, &["resize", "kick/hits:my53vvkc", "--start", "1", "--length", "32"]);
    assert_eq!(read(), before, "trimmed back");
    let out = ok(dir, &["create", "kick/hits", "--at", "9.3.2"]);
    assert!(out.contains("clip at bar 9 (1 bar)"), "{out}");
    let out = ok(dir, &["create", "kick/hits", "--at", "40", "--beats", "--length", "2"]);
    assert!(out.contains("clip at bar 11 (0.5 bars)"), "{out}");
    let gesture = song.join("gesture.json");
    std::fs::write(&gesture, r#"{"op": "move", "clips": [{"module": "kick", "lane": "hits", "id": "my53vvkc"}], "by": 0, "to": {"module": "hats", "lane": "hits"}}"#).unwrap();
    ok(dir, &["json", gesture.to_str().unwrap()]);
    assert!(std::fs::read_to_string(song.join("data/hats/hits.json")).unwrap().contains("my53vvkc"));

    // Refused in words, and nothing changes.
    let now = read();
    for (args, says) in [
        // What isn't there: said whole, as the user reads it.
        (&["move", "kick/hits:nothere", "--by", "1"][..], "error: Kick's hits lane has no clip `nothere` now.\n"),
        (&["delete", "nosuch/notes:abc"][..], "error: There's no track `nosuch`.\n"),
        (&["delete", "bass/nolane:abc"][..], "error: Bass has no lane `nolane`.\n"),
        (&["move", "kick/hits@9.3.2", "--by", "1", "--to", "bass/cutoff"][..], "A notes clip can't go on an automation lane"),
        (&["create", "kick/hits", "--at", "9.5"][..], "bar 9 has 4 beats"),
        (&["delete", "kick/hits"][..], "isn't a clip"),
        (&["fold", "kick/hits:x"][..], "create, move, resize, split"),
        (&["delete", "kick/hits@9.3.2", "--copy"][..], "`--copy` isn't a flag of"),
    ] {
        let o = m2(&[&["clips", dir][..], args].concat());
        assert!(!o.status.success() && said(&o).contains(says), "{args:?}: {}", said(&o));
    }
    assert_eq!(read(), now);

    // WRITE-18: another process holds the project: refused, naming it, and nothing changes.
    let held = m2_session::lock::acquire(&song, "music2", m2_session::lock::RETRY).unwrap();
    let refused = m2(&["clips", dir, "delete", "kick/hits@9.3.2"]);
    assert!(!refused.status.success() && said(&refused).contains("music2"), "{}", said(&refused));
    assert_eq!(read(), now);
    drop(held);
}

// Split at a place, join back, quantize: each one edit, from the terminal.
#[test]
fn split_join_and_quantize_from_the_terminal() {
    let temp = Temp(std::env::temp_dir().join(format!("m2-clips-cli-sjq-{}", std::process::id())));
    let song = temp.0.join("first-song");
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song"), &song);
    let (dir, notes) = (song.to_str().unwrap(), song.join("data/bass/notes.json"));
    let read = || std::fs::read_to_string(&notes).unwrap();
    let before = read();
    let out = ok(dir, &["split", "bass/notes:db9f9t5s", "--at", "5"]);
    let second = out.lines().find_map(|l| l.trim().strip_prefix("made bass/notes:")).unwrap_or_else(|| panic!("{out}")).to_string();
    assert!(
        read().contains("{\"id\": \"db9f9t5s\", \"start\": 0, \"length\": 16")
            && read().contains(&format!("{{\"id\": \"{second}\", \"start\": 16"))
    );
    ok(dir, &["join", "bass/notes:db9f9t5s", &format!("bass/notes:{second}")]);
    assert_eq!(read(), before, "joined back, as it was");
    ok(dir, &["quantize", "bass/notes@1", "--grid", "1"]);
    assert!(!read().contains("\"start\": 0.75"), "on the beat now");
    let o = m2(&["clips", dir, "quantize", "bass/notes@1"]);
    assert!(!o.status.success() && said(&o).contains("--grid"), "{}", said(&o));
}

// HOME-2, G9: `m2 unautomate` puts what the lane plays at a place (else its fallback) back in the
// code and takes the lane file away; it takes the lock.
#[test]
fn unautomate_from_the_terminal() {
    let temp = Temp(std::env::temp_dir().join(format!("m2-unautomate-cli-{}", std::process::id())));
    let song = temp.0.join("first-song");
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song"), &song);
    let dir = song.to_str().unwrap();
    let bass = std::fs::read_to_string(song.join("tracks/bass.ts")).unwrap();
    let held = m2_session::lock::acquire(&song, "music2", m2_session::lock::RETRY).unwrap();
    let refused = m2(&["unautomate", dir, "bass/cutoff"]);
    assert!(!refused.status.success() && said(&refused).contains("music2"), "{}", said(&refused));
    drop(held);
    // Bar 3 is beat 8, half way up the cutoff's rise from 300 to 2200 Hz.
    let o = m2(&["unautomate", dir, "bass/cutoff", "--at", "3"]);
    assert!(o.status.success() && said(&o).contains("cutoff automated → cutoff hz(1250)"), "{}", said(&o));
    assert_eq!(
        std::fs::read_to_string(song.join("tracks/bass.ts")).unwrap(),
        bass.replace("lane.control(\"cutoff\", \"hz\", hz(500))", "hz(1250)")
    );
    assert!(!song.join("data/bass/cutoff.json").exists());
    let o = m2(&["unautomate", dir, "bass/drop"]);
    assert!(o.status.success(), "{}", said(&o));
    let o = m2(&["unautomate", dir, "bass/notes"]);
    assert!(!o.status.success() && said(&o).contains("isn't an automation lane"), "{}", said(&o));
    let o = m2(&["unautomate", dir, "nosuch/cutoff"]);
    assert!(!o.status.success() && said(&o).ends_with("error: There's no track `nosuch`.\n"), "said whole: {}", said(&o));
}
