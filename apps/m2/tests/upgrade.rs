//! `m2 upgrade` (FMT-25, AGENT-14) on real old songs: the repository's own examples as they were
//! at older commits, kept in `old-songs/` (first-song at std 0.4 and 0.8, the sampler at 0.4, the
//! sounds demo at 0.5 and 0.8). Each builds and renders once upgraded; where the sounds didn't
//! change, the bits are today's.

mod app;

use std::path::{Path, PathBuf};

use app::{App, Temp, copy, example, m2_in, said};
use serde_json::json;

/// Old song `name`, as the repository had it.
fn old(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/old-songs").join(name)
}

/// Every file under `dir` but `.music2/`, by its path, as bytes.
fn files(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn walk(root: &Path, d: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
        for e in std::fs::read_dir(d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                if e.file_name() != ".music2" {
                    walk(root, &p, out);
                }
            } else {
                out.push((p.strip_prefix(root).unwrap().to_path_buf(), std::fs::read(&p).unwrap()));
            }
        }
    }
    let mut out = vec![];
    walk(dir, dir, &mut out);
    out.sort();
    out
}

/// A copy of old song `name` at `<temp>/<to>`, with today's example `recordings`' `assets/` when
/// it plays recordings (the old song's are the same files).
fn copy_old(temp: &Temp, name: &str, to: &str, recordings: Option<&str>) -> PathBuf {
    let song = temp.0.join(to);
    copy(&old(name), &song);
    if let Some(ex) = recordings {
        copy(&example(ex).join("assets"), &song.join("assets"));
    }
    song
}

/// `m2 render` of the song at `song`: the WAV's bytes.
fn render(home: &Path, song: &Path) -> Vec<u8> {
    let out = song.with_extension("wav");
    let o = m2_in(home, &["render", song.to_str().unwrap(), out.to_str().unwrap()], &[]);
    assert!(o.status.success(), "{}: {}", song.display(), said(&o));
    std::fs::read(&out).unwrap()
}

// FMT-25: each old song, upgraded, builds and renders, and is then made for this music2.
#[test]
fn each_old_song_builds_and_renders_once_upgraded() {
    let temp = Temp::new("upgrade-each");
    let home = temp.0.join("home");
    for (name, recordings) in [
        ("first-song-std-0.4", None),
        ("first-song-std-0.8", None),
        ("sampler-std-0.4", Some("sampler")),
        ("sounds-std-0.5", None),
        ("sounds-std-0.8", None),
    ] {
        let song = copy_old(&temp, name, name, recordings);
        let dir = song.to_str().unwrap();
        let o = m2_in(&home, &["check", dir], &[]);
        assert!(!o.status.success(), "{name} doesn't build as it was made: {}", said(&o));
        let o = m2_in(&home, &["upgrade", dir], &[]);
        assert!(o.status.success(), "{name}: {}", said(&o));
        assert!(said(&o).contains("upgraded to this music2 (revision"), "{name}: {}", said(&o));
        assert!(said(&o).contains("which music2 can undo once it opens the song"), "{name}: {}", said(&o));
        let o = m2_in(&home, &["check", dir], &[]);
        assert!(o.status.success(), "{name} builds: {}", said(&o));
        let wav = render(&home, &song);
        assert!(wav.len() > 48_000 * 2 * 3, "{name} renders: {} bytes", wav.len());
        let o = m2_in(&home, &["upgrade", dir], &[]);
        assert!(o.status.success() && said(&o).contains("made for this music2 already"), "{name}: {}", said(&o));
        // KIT-24: a song made for this music2 that plays the drum machine names it all the same.
        if name.starts_with("sounds") {
            assert!(said(&o).contains("to do when you choose (the upgrade doesn't):"), "{name}: {}", said(&o));
            assert!(said(&o).contains("m2 make-kit <song> drums"), "{name}: {}", said(&o));
        }
    }
}

/// The sounds demo, made for std 0.8, as the music2 it was made for rendered it (`m2 render`, built
/// from f93d901a, std 0.8 and `@m2/drums` 1): the SHA-256 of its WAV.
const SOUNDS_THEN: &str = "c38145830265b3980876d7d86b54193e84d3947e997d1fc088c6c5e4fcd59156";

// FMT-25 (the review's G3, G4, G17): an upgrade keeps the song's sound. The sampler example made
// for std 0.4 is today's example once upgraded, file for file, and renders today's bits. The
// sounds demo, every single drum converted with its knobs, upgraded from std 0.5 and from 0.8,
// renders the bits the music2 it was made for rendered.
#[test]
fn an_upgrade_keeps_the_songs_sound() {
    let temp = Temp::new("upgrade-bits");
    let home = temp.0.join("home");
    let old = copy_old(&temp, "sampler-std-0.4", "sampler-old", Some("sampler"));
    let today = temp.0.join("sampler-today");
    copy(&example("sampler"), &today);
    let o = m2_in(&home, &["upgrade", old.to_str().unwrap()], &[]);
    assert!(o.status.success(), "{}", said(&o));
    let today_files: Vec<(PathBuf, Vec<u8>)> =
        files(&today).into_iter().filter(|(p, _)| !matches!(p.to_str(), Some("example.toml" | ".gitignore"))).collect();
    assert_eq!(files(&old), today_files, "today's example, file for file");
    assert!(render(&home, &old) == render(&home, &today), "today's bits");

    let five = copy_old(&temp, "sounds-std-0.5", "sounds-0.5", None);
    let eight = copy_old(&temp, "sounds-std-0.8", "sounds-0.8", None);
    for song in [&five, &eight] {
        let o = m2_in(&home, &["upgrade", song.to_str().unwrap()], &[]);
        assert!(o.status.success(), "{}", said(&o));
        // KIT-24: its drum machine is named, with Make it a kit, and left as it is: the bits hold.
        assert!(said(&o).contains("tracks/drums.ts:11: `drums.kit` plays the drum machine"), "{}", said(&o));
        assert!(said(&o).contains("Make it a kit (the Drums track's menu), or run m2 make-kit <song> drums"), "{}", said(&o));
    }
    for song in [&five, &eight] {
        let then = m2_song::Sha::of(&render(&home, song)).hex();
        assert_eq!(then, SOUNDS_THEN, "{}: the old music2's bits", song.display());
    }
}

// FMT-25: `--check` says what it would change, and that the song then builds, and writes nothing;
// a song made for this music2 is left as it is.
#[test]
fn check_writes_nothing_and_a_current_song_is_left_alone() {
    let temp = Temp::new("upgrade-check");
    let home = temp.0.join("home");
    let song = copy_old(&temp, "first-song-std-0.4", "song", None);
    let before = files(&song);
    let o = m2_in(&home, &["upgrade", song.to_str().unwrap(), "--check"], &[]);
    assert!(o.status.success(), "{}", said(&o));
    let out = said(&o);
    for line in
        ["- std = \"0.4\"", "+ std = \"0.9\"", "- import { kick } from \"std/instruments\";", "+ import { drums } from \"@m2/drums\";"]
    {
        assert!(out.contains(line), "says {line}: {out}");
    }
    assert!(out.contains("--check: nothing was written; the song builds after it"), "{out}");
    assert_eq!(files(&song), before, "nothing written");
    let today = temp.0.join("today");
    copy(&example("first-song"), &today);
    let before = files(&today);
    let o = m2_in(&home, &["upgrade", today.to_str().unwrap()], &[]);
    assert!(o.status.success() && said(&o).contains("made for this music2 already"), "{}", said(&o));
    assert_eq!(files(&today), before);
    // A song made for a newer music2 can't be moved back: said, and nothing written.
    let newer = temp.0.join("newer");
    copy(&example("first-song"), &newer);
    let manifest = std::fs::read_to_string(newer.join("music2.toml")).unwrap().replace("\"@m2/core\" = \"2\"", "\"@m2/core\" = \"3\"");
    std::fs::write(newer.join("music2.toml"), manifest).unwrap();
    let before = files(&newer);
    let o = m2_in(&home, &["upgrade", newer.to_str().unwrap()], &[]);
    assert_eq!(o.status.code(), Some(1), "{}", said(&o));
    assert!(said(&o).contains("this song was made for a newer music2"), "{}", said(&o));
    assert_eq!(files(&newer), before);
}

// FMT-25 (G9): the musician's presets of sounds that moved are named, and never changed.
#[test]
fn presets_of_moved_sounds_are_named() {
    let temp = Temp::new("upgrade-presets");
    let home = temp.0.join("home");
    let preset = home.join("Music/music2/Presets/_m2_drums/drum-snare/Tight.json");
    std::fs::create_dir_all(preset.parent().unwrap()).unwrap();
    let text = "{\n  \"m2preset\": 1,\n  \"name\": \"Tight\",\n  \"sound\": \"@m2/drums:drum-snare\",\n  \"values\": {\"tune\": 200}\n}\n";
    std::fs::write(&preset, text).unwrap();
    let song = copy_old(&temp, "sounds-std-0.8", "song", None);
    let o = m2_in(&home, &["upgrade", song.to_str().unwrap()], &[]);
    assert!(o.status.success(), "{}", said(&o));
    assert!(said(&o).contains("your presets of sounds that moved, which the upgrade doesn't change"), "{}", said(&o));
    assert!(said(&o).contains("Presets/_m2_drums/drum-snare/Tight.json: \"Tight\" is for the single snare"), "{}", said(&o));
    assert_eq!(std::fs::read_to_string(&preset).unwrap(), text);
}

// FMT-25: a call it can't convert is left as it is and named by its file and line, with the line
// to write; the song wouldn't build, so nothing is written, and it exits 1.
#[test]
fn a_call_it_cant_convert_is_named_and_nothing_is_written() {
    let temp = Temp::new("upgrade-left");
    let home = temp.0.join("home");
    let song = copy_old(&temp, "sounds-std-0.8", "song", None);
    let perc = song.join("tracks/perc.ts");
    let text = std::fs::read_to_string(&perc).unwrap();
    let text = text.replace("drums.snare(lane.notes(\"roll\"), { snap: 0.75 })", "drums.snare(lane.notes(\"roll\"), rolled)");
    std::fs::write(&perc, format!("{text}const rolled = {{ snap: 0.75 }};\n")).unwrap();
    let before = files(&song);
    let o = m2_in(&home, &["upgrade", song.to_str().unwrap()], &[]);
    assert_eq!(o.status.code(), Some(1), "{}", said(&o));
    let out = said(&o);
    assert!(out.contains("tracks/perc.ts:10: `drums.snare`: its options are worked out by code (`rolled`)"), "{out}");
    assert!(out.contains("write: drums.drum(lane.notes(\"roll\"), { drum: \"snare\", … })"), "{out}");
    assert!(out.contains("nothing was written"), "{out}");
    assert_eq!(files(&song), before, "nothing written");
}

// AGENT-14: while the app has the song open, `m2 upgrade` asks the app, so it is one undo step
// there, the agent's; the app says the song needs it until then; `m2 undo` puts back every byte.
#[test]
fn with_the_app_open_it_is_one_undo_step_there() {
    let a = App::open("first-song", |song| {
        std::fs::remove_dir_all(song).unwrap();
        copy(&old("first-song-std-0.8"), song);
    });
    let before = files(&a.song);
    let state = || a.running.state.graph.state().unwrap();
    assert_eq!(state()["upgrade"], json!(true), "made for an older music2");
    assert_eq!(state()["has_code"], json!(false));
    let o = a.m2(&["upgrade", a.dir(), "--as", "Claude"]);
    assert!(o.status.success(), "{}", said(&o));
    assert!(said(&o).contains("in the open app: one undo step, Claude: Upgrade"), "{}", said(&o));
    assert_eq!((state()["upgrade"].clone(), state()["has_code"].clone()), (json!(false), json!(true)), "it plays now");
    let top = a.top();
    assert_eq!((&top["by"], &top["name"]), (&json!("Claude"), &json!("Upgrade")), "{top}");
    assert!(a.read("tracks/kick.ts").contains("drums.drum(lane.notes(\"hits\"))"));
    let upgraded = files(&a.song);
    let o = a.m2(&["undo", a.dir(), "--as", "Claude"]);
    assert!(o.status.success(), "{}", said(&o));
    assert_eq!(files(&a.song), before, "every file as it was");
    // The old files don't build, so the app plays the upgrade still; upgrading again works (G1).
    assert_eq!(state()["upgrade"], json!(true), "made for an older music2 again");
    let o = a.m2(&["upgrade", a.dir(), "--as", "Claude"]);
    assert!(o.status.success(), "{}", said(&o));
    assert_eq!(files(&a.song), upgraded, "the same upgrade again");
}
