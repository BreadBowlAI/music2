//! The song bar's builders from the terminal (WRITE-10, WRITE-18): `m2 tempo`, `m2 meter` and
//! `m2 title` on a copy of first-song.

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

// WRITE-10, FMT-13: each command is its builder, verified, and says what it did in words; what
// FMT-13 refuses is refused, writing nothing.
#[test]
fn the_tempo_meter_and_title_are_set_from_the_terminal() {
    let temp = Temp(std::env::temp_dir().join(format!("m2-song-cli-{}", std::process::id())));
    let song = temp.0.join("first-song");
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song"), &song);
    let dir = song.to_str().unwrap();
    let read = |p: &str| std::fs::read_to_string(song.join(p)).unwrap();

    let said = ok(&["tempo", dir, "128"]);
    assert!(said.contains("data/arrangement.json: 124 BPM → 128 BPM"), "{said}");
    let said = ok(&["meter", dir, "7/8", "--at", "9"]);
    assert!(said.contains("4/4 → 7/8 from bar 9"), "{said}");
    let said = ok(&["tempo", dir, "100", "--at", "10"]);
    // Bar 10 is one bar of 7/8 after bar 9's beat 32.
    assert!(said.contains("128 BPM → 100 BPM from bar 10"), "{said}");
    assert_eq!(
        read("data/arrangement.json"),
        "{\n  \"tempo\": [\n    {\"beat\": 0, \"bpm\": 128},\n    {\"beat\": 35.5, \"bpm\": 100}\n  ],\n  \"meter\": [\n    \
         {\"beat\": 0, \"beats\": 4, \"unit\": 4},\n    {\"beat\": 32, \"beats\": 7, \"unit\": 8}\n  ]\n}\n"
    );
    let said = ok(&["title", dir, "Night Drive"]);
    assert!(said.contains("music2.toml: first-song → Night Drive"), "{said}");
    assert!(read("music2.toml").contains("name = \"Night Drive\""));

    let before = read("data/arrangement.json");
    for (args, says) in [
        (vec!["tempo", dir, "1000"], "outside 20 to 999"),
        (vec!["tempo", dir, "fast"], "a tempo is a number of BPM"),
        (vec!["meter", dir, "7/9"], "not 9"),
        (vec!["meter", dir, "4/4", "--at", "0"], "--at takes a bar"),
    ] {
        let out = m2(&args);
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success() && err.contains(says), "{args:?}: {err}");
    }
    assert_eq!(read("data/arrangement.json"), before, "nothing written");
}
