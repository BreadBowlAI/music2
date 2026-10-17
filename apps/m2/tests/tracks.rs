//! The track builders from the terminal (WRITE-10, WRITE-18): `m2 add-track`, `duplicate-track`,
//! `delete-track` and `layout` on a copy of first-song.

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
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn m2(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_m2")).args(args).output().unwrap()
}

fn ok(args: &[&str]) -> String {
    let out = m2(args);
    assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

// WRITE-10: each command is its builder, verified, and says what it did in words.
#[test]
fn tracks_are_added_duplicated_deleted_and_laid_out_from_the_terminal() {
    let temp = Temp(std::env::temp_dir().join(format!("m2-tracks-cli-{}", std::process::id())));
    let song = temp.0.join("first-song");
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song"), &song);
    let dir = song.to_str().unwrap();
    let read = |p: &str| std::fs::read_to_string(song.join(p)).unwrap();

    let said = ok(&["add-track", dir, "synth.pluck", "--name", "Lead Vox", "--after", "kick"]);
    assert!(said.contains("added Lead Vox (Synth pluck), ID lead-vox, in tracks/lead-vox.ts"), "{said}");
    assert!(read("tracks/lead-vox.ts").contains("import { synth } from \"std/synth\";") && read("tracks/lead-vox.ts").contains("synth.pluck(lane.notes(\"notes\"))"));
    assert!(read("song.ts").contains("\"lead-vox\": leadVox.out"));
    assert!(read("layout.json").contains("[\"kick\", \"lead-vox\", \"hats\""), "{}", read("layout.json"));
    // An instrument that isn't one says which there are.
    let out = m2(&["add-track", dir, "std:delay"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success() && err.contains("std:synth-pluck (Synth pluck)") && !err.contains("std:pluck"), "{err}");

    let said = ok(&["duplicate-track", dir, "bass"]);
    assert!(said.contains("as Bass 2, ID bass-2"), "{said}");
    assert!(song.join("data/bass-2/notes.json").exists());

    // The kick feeds two ducks now: listed, nothing changed, until --cascade.
    let before = read("song.ts");
    let out = m2(&["delete-track", dir, "kick"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success() && err.contains("Duck in Bass (song.ts") && err.contains("--cascade"), "{err}");
    assert_eq!(read("song.ts"), before);
    let said = ok(&["delete-track", dir, "kick", "--cascade"]);
    assert!(said.contains("deleted Kick") && said.contains("Duck"), "{said}");
    assert!(!song.join("tracks/kick.ts").exists() && !read("song.ts").contains("kick.") && !read("song.ts").contains("./tracks/kick"));

    let said = ok(&["layout", dir, "colour", "hats", "violet"]);
    assert!(said.contains("teal → violet") && said.contains("no revision"), "{said}");
    let out = m2(&["layout", dir, "colour", "hats", "red"]);
    assert!(!out.status.success());
    ok(&["layout", dir, "order", "keys", "bass-2", "bass", "hats", "lead-vox"]);
    assert!(read("layout.json").contains("[\"keys\", \"bass-2\", \"bass\", \"hats\", \"lead-vox\"]"));
    ok(&["check", dir]);
}
