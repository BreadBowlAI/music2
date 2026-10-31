//! `m2 sections` (ARR-9, WRITE-18): section gestures from the terminal, by bars or by a marker, each
//! one edit; and a refusal that changes nothing while another process holds the project.

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
    Command::new(env!("CARGO_BIN_EXE_m2")).env("HOME", std::env::temp_dir().join("m2-tests-no-app")).args(args).output().unwrap()
}

fn said(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

/// Run `m2 sections <dir> args…`, which must succeed: what it said.
fn ok(dir: &str, args: &[&str]) -> String {
    let o = m2(&[&["sections", dir][..], args].concat());
    assert!(o.status.success(), "{args:?}: {}", said(&o));
    said(&o)
}

#[test]
fn section_gestures_from_the_terminal() {
    let temp = Temp(std::env::temp_dir().join(format!("m2-sections-cli-{}", std::process::id())));
    let song = temp.0.join("first-song");
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song"), &song);
    std::fs::write(song.join("layout.json"), r#"{"markers": [{"beat": 0, "name": "Intro"}, {"beat": 32, "name": "Chorus"}]}"#).unwrap();
    let (dir, hits) = (song.to_str().unwrap(), song.join("data/kick/hits.json"));
    let read = || std::fs::read_to_string(&hits).unwrap();
    let layout = || std::fs::read_to_string(song.join("layout.json")).unwrap();

    // Bars 1 to 8 are beats 0 to 32 in 4/4: the copy lands on bar 9, on every track.
    let out = ok(dir, &["duplicate", "1-8"]);
    assert!(out.contains("bars 9–16 → a copy of bars 1–8"), "{out}");
    let made = out.lines().find_map(|l| l.trim().strip_prefix("made kick/hits:")).unwrap_or_else(|| panic!("{out}")).to_string();
    assert!(read().contains(&format!("{{\"id\": \"{made}\", \"start\": 32, \"length\": 32")), "{}", read());
    assert!(out.contains("made hats/hits:") && out.contains("made bass/notes:"), "{out}");
    // A marker names the bars from it to the next: the Intro's bars go, and the rest moves up.
    let out = ok(dir, &["delete-bars", "Intro"]);
    assert!(out.contains("bars 1–8 → bars 1–8 taken out"), "{out}");
    assert!(read().contains(&format!("{{\"id\": \"{made}\", \"start\": 0, \"length\": 32")) && !read().contains("my53vvkc"), "{}", read());
    assert!(layout().contains("\"name\": \"Chorus\"") && layout().contains("\"beat\": 0") && !layout().contains("Intro"), "{}", layout());
    // The last marker's bars run to the end of the last bar that plays.
    let out = ok(dir, &["insert-bars", "Chorus"]);
    assert!(out.contains("bar 1 → 8 empty bars at bar 1"), "{out}");
    assert!(read().contains(&format!("{{\"id\": \"{made}\", \"start\": 32, \"length\": 32")), "{}", read());
    let out = ok(dir, &["move", "9-16", "--to", "17"]);
    assert!(out.contains("bars 9–16 → bars 17–24"), "{out}");
    assert!(read().contains(&format!("{{\"id\": \"{made}\", \"start\": 64, \"length\": 32")), "the moved clip keeps its id: {}", read());
    let gesture = song.join("gesture.json");
    std::fs::write(&gesture, r#"{"op": "delete", "start": 64, "end": 96}"#).unwrap();
    let out = ok(dir, &["json", gesture.to_str().unwrap()]);
    assert!(out.contains("bars 17–24 → empty bars 17–24"), "{out}");
    assert_eq!(read(), "{\"clips\": []}\n", "{}", read());

    // Refused in words, and nothing changes.
    let now = (read(), layout());
    for (args, says) in [
        (&["delete", "Bridge"][..], "`Bridge` isn't bars or a marker"),
        (&["delete", "16-9"][..], "the last bar comes before the first"),
        (&["move", "1-8"][..], "needs `--to`"),
        (&["move", "1-8", "--to", "9.2"][..], "The bars go at the start of a bar, not at bar 9 beat 2."),
        (&["fold", "1-8"][..], "duplicate, move, delete"),
        (&["delete", "1-8", "--to", "9"][..], "`--to` isn't a flag of"),
    ] {
        let o = m2(&[&["sections", dir][..], args].concat());
        assert!(!o.status.success() && said(&o).contains(says), "{args:?}: {}", said(&o));
    }
    assert_eq!((read(), layout()), now);

    // WRITE-18: another process holds the project: refused, naming it, and nothing changes.
    let held = m2_session::lock::acquire(&song, "music2", m2_session::lock::RETRY).unwrap();
    let refused = m2(&["sections", dir, "delete", "1-8"]);
    assert!(!refused.status.success() && said(&refused).contains("music2"), "{}", said(&refused));
    assert_eq!((read(), layout()), now);
    drop(held);
}
