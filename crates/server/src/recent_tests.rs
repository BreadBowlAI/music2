//! The recent songs list (polish C2), each test in a temporary folder of its own.

use std::sync::atomic::{AtomicU64, Ordering};

use super::*;

/// A temporary folder, removed after the test.
struct Temp(PathBuf);

impl Temp {
    fn new() -> Temp {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!("m2-recent-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        Temp(dir.canonicalize().unwrap())
    }

    /// A song folder called `name`, with its name and tempo.
    fn song(&self, name: &str, bpm: f64) -> PathBuf {
        let dir = self.0.join(name);
        std::fs::create_dir_all(dir.join("data")).unwrap();
        std::fs::write(dir.join("song.ts"), "").unwrap();
        std::fs::write(dir.join("music2.toml"), format!("[project]\nname = \"{name}\"\nformat = 1\n")).unwrap();
        let tempo = format!("{{\"tempo\": [{{\"beat\": 0, \"bpm\": {bpm}}}], \"meter\": []}}\n");
        std::fs::write(dir.join("data/arrangement.json"), tempo).unwrap();
        dir
    }

    fn recent(&self) -> Recent {
        Recent::new(self.0.join("home/.music2/recent.json"))
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn names(r: &Recent) -> Vec<String> {
    r.list().into_iter().map(|s| s.name).collect()
}

#[test]
fn opened_puts_it_first_once() {
    let t = Temp::new();
    let r = t.recent();
    assert!(r.list().is_empty(), "no file: an empty list");
    let (a, b, c) = (t.song("Night Drive", 124.0), t.song("Sunday", 90.0), t.song("Chorus Idea", 120.0));
    for dir in [&a, &b, &c, &a] {
        r.opened(dir).unwrap();
    }
    assert_eq!(names(&r), ["Night Drive", "Chorus Idea", "Sunday"]);
    let first = &r.list()[0];
    assert_eq!((&first.path, first.tempo, first.missing), (&a, Some(124.0), false));
    assert!(first.opened.ends_with('Z') && first.opened.len() == 20, "{}", first.opened);
    // The same folder by another way of writing it is the same song.
    r.opened(&t.0.join("home/../Sunday")).unwrap();
    assert_eq!(names(&r), ["Sunday", "Night Drive", "Chorus Idea"]);
    let file: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(r.file()).unwrap()).unwrap();
    assert_eq!(file["songs"][0]["path"], serde_json::json!(b));
    assert_eq!(file["open"], serde_json::json!(true));
}

#[test]
fn the_list_keeps_twenty() {
    let t = Temp::new();
    let r = t.recent();
    for n in 0..25 {
        r.opened(&t.song(&format!("Song {n}"), 120.0)).unwrap();
    }
    let list = names(&r);
    assert_eq!(list.len(), KEEP);
    assert_eq!((list[0].as_str(), list[19].as_str()), ("Song 24", "Song 5"));
}

#[test]
fn a_crash_while_reopening_is_not_tried_again() {
    let t = Temp::new();
    let r = t.recent();
    let a = t.song("Night Drive", 124.0);
    r.opened(&a).unwrap();
    // The app quit with it open: the next launch opens it.
    assert_eq!(r.to_reopen(), Reopen::Song(a.clone()));
    // That launch closed while opening it.
    r.reopening(&a).unwrap();
    let next = t.recent();
    assert_eq!(next.to_reopen(), Reopen::Crashed(a.clone()), "not tried again");
    assert_eq!(next.to_reopen(), Reopen::Nothing, "nor at the launch after");
    // A launch that opened it clears the guard.
    r.opened(&a).unwrap();
    r.reopening(&a).unwrap();
    r.reopened().unwrap();
    assert_eq!(r.to_reopen(), Reopen::Song(a));
}

#[test]
fn a_song_closed_before_quitting_doesnt_reopen() {
    let t = Temp::new();
    let r = t.recent();
    let a = t.song("Night Drive", 124.0);
    r.opened(&a).unwrap();
    r.closed().unwrap();
    assert_eq!(r.to_reopen(), Reopen::Nothing);
    assert_eq!(names(&r), ["Night Drive"], "still listed");
    // Nor one whose folder is gone.
    r.opened(&a).unwrap();
    std::fs::remove_dir_all(&a).unwrap();
    assert_eq!(r.to_reopen(), Reopen::Nothing);
}

#[test]
fn forget_and_clear() {
    let t = Temp::new();
    let r = t.recent();
    let (a, b) = (t.song("Night Drive", 124.0), t.song("Sunday", 90.0));
    r.opened(&a).unwrap();
    r.opened(&b).unwrap();
    // A folder that is gone: listed as missing, and forgotten by the path the list gave.
    std::fs::remove_dir_all(&a).unwrap();
    let gone = r.list().into_iter().find(|s| s.missing).expect("a missing song");
    assert_eq!((gone.name.as_str(), gone.tempo), ("Night Drive", None));
    r.forget(&gone.path).unwrap();
    assert_eq!(names(&r), ["Sunday"]);
    // Renamed: it keeps its place.
    let moved = t.0.join("Sunday Best");
    std::fs::rename(&b, &moved).unwrap();
    r.moved(&b, &moved).unwrap();
    assert_eq!(r.paths(), [moved]);
    r.clear().unwrap();
    assert!(r.list().is_empty());
    assert_eq!(r.to_reopen(), Reopen::Nothing);
}

#[test]
fn a_list_that_doesnt_read_is_empty() {
    let t = Temp::new();
    let r = t.recent();
    std::fs::create_dir_all(r.file().parent().unwrap()).unwrap();
    std::fs::write(r.file(), "{ not json").unwrap();
    assert!(r.list().is_empty());
    assert_eq!(r.to_reopen(), Reopen::Nothing);
    // The next open writes it over.
    let a = t.song("Night Drive", 124.0);
    r.opened(&a).unwrap();
    assert_eq!(names(&r), ["Night Drive"]);
}

#[test]
fn times_are_written_in_utc() {
    assert_eq!(utc(0), "1970-01-01T00:00:00Z");
    assert_eq!(utc(1_792_227_355), "2026-10-17T08:55:55Z");
    assert_eq!(utc(951_782_400), "2000-02-29T00:00:00Z");
}
