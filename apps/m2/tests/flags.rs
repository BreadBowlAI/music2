//! AGENT-13: `m2` reads each command's words against its usage in the one table, and refuses a
//! word the command doesn't take before anything is read or written: exit status 2, naming the
//! word and showing how the command is called. `--help` after a command prints its usage. (The
//! reader's own tests, which walk every form of the table, are in `src/words.rs`.)

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A scratch folder, removed after: a copy of first-song in `song/`, a HOME in `home/`, and `here/`,
/// the folder `m2` runs in, where a file named by a relative path would land.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("m2-flags-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song"), &dir.join("song"));
        for d in ["home", "here"] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        Self(dir)
    }

    fn song(&self) -> String {
        self.0.join("song").to_str().unwrap().to_string()
    }

    fn m2(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_m2"))
            .current_dir(self.0.join("here"))
            .env("HOME", self.0.join("home"))
            .env("M2_AUDIO", "none")
            .env("M2_MIDI", "none")
            .args(args)
            .output()
            .unwrap()
    }

    /// Every file under the scratch folder, with its bytes: what a refusal must leave as it was.
    fn files(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        fn walk(dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    out.insert(p.clone(), vec![]);
                    walk(&p, out);
                } else {
                    out.insert(p.clone(), std::fs::read(&p).unwrap());
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(&self.0, &mut out);
        out
    }

    /// Run `m2 args…`, which must be refused as a word it doesn't take: exit 2, nothing printed on
    /// standard output and nothing written. What it said.
    fn refused(&self, args: &[&str]) -> String {
        let before = self.files();
        let o = self.m2(args);
        let err = String::from_utf8_lossy(&o.stderr).into_owned();
        assert_eq!(o.status.code(), Some(2), "m2 {args:?}: {err}");
        assert!(o.stdout.is_empty(), "m2 {args:?} printed {}", String::from_utf8_lossy(&o.stdout));
        let after = self.files();
        let changed: Vec<_> = after.keys().chain(before.keys()).filter(|p| before.get(*p) != after.get(*p)).collect();
        assert!(changed.is_empty(), "m2 {args:?} wrote {changed:?}");
        // The review's W7: the words that say why are plain: no `[…]`, `|` or `…` of the usage's.
        let why = err.lines().next().unwrap_or_default();
        assert!(!why.contains(['[', ']', '|', '…']), "m2 {args:?} says why in the usage's notation: {why}");
        err
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let (f, t) = (e.path(), to.join(e.file_name()));
        if f.is_dir() { copy(&f, &t) } else { std::fs::copy(&f, &t).map(drop).unwrap() }
    }
}

const RENDER: &str = "m2 render <project> <out.wav> [--from S | --from-bar N] [--seconds N] [--preroll P] [--threads N]";

// AGENT-13, gap #50: `m2 render song -o mix.wav` wrote the render to a file named `-o` and ignored
// `mix.wav`. Now `-o` is refused, named, with the usage, and nothing is written.
#[test]
fn m2_render_refuses_a_flag_it_doesnt_know_and_writes_nothing() {
    let s = Scratch::new("render-o");
    let err = s.refused(&["render", &s.song(), "-o", "mix.wav"]);
    assert!(err.contains("`-o` isn't a flag of m2 render"), "{err}");
    assert!(err.contains(&format!("usage: {RENDER}")), "it shows how m2 render is called: {err}");
}

/// Each command `m2 docs commands` lists, with its usage forms: the page is made from the one table.
fn table(s: &Scratch) -> Vec<(String, Vec<String>)> {
    let o = s.m2(&["docs", "commands"]);
    assert!(o.status.success());
    let page = String::from_utf8(o.stdout).unwrap();
    let mut out: Vec<(String, Vec<String>)> = vec![];
    let mut lines = page.lines();
    while let Some(l) = lines.next() {
        if let Some(name) = l.strip_prefix("## m2 ") {
            assert_eq!(lines.next(), Some(""));
            assert_eq!(lines.next(), Some("```"));
            let forms = lines.by_ref().take_while(|l| *l != "```").map(str::to_string).collect();
            out.push((name.to_string(), forms));
        }
    }
    assert!(out.len() > 40, "{page}");
    out
}

/// `text` with each run of spaces and line breaks as one space.
fn flat(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

// AGENT-13: every command refuses a flag it doesn't take, the same way, with or without its other
// words; and `--help` after it prints its usage, writing nothing.
#[test]
fn every_command_refuses_a_flag_it_doesnt_know() {
    let s = Scratch::new("walk");
    let song = s.song();
    for (name, forms) in table(&s) {
        for args in [vec![name.as_str(), "--nope"], vec![name.as_str(), song.as_str(), "--nope", "x"]] {
            let err = s.refused(&args);
            assert!(err.contains(&format!("`--nope` isn't a flag of m2 {name}")), "m2 {args:?}: {err}");
            assert!(err.contains(&format!("usage: m2 {name}")), "m2 {args:?}: {err}");
        }
        let before = s.files();
        let o = s.m2(&[&name, "--help"]);
        assert!(o.status.success(), "m2 {name} --help: {}", String::from_utf8_lossy(&o.stderr));
        let help = flat(&String::from_utf8_lossy(&o.stdout));
        for f in &forms {
            assert!(help.contains(&flat(f)), "m2 {name} --help doesn't show {f}: {help}");
        }
        assert_eq!(s.files(), before, "m2 {name} --help wrote something");
    }
}

// AGENT-13: a flag that takes a value and has none is refused: at the end, or with another of the
// command's flags where its value goes.
#[test]
fn a_flag_without_its_value_is_refused() {
    let s = Scratch::new("value");
    let song = s.song();
    for (args, says) in [
        (&["render", &song, "out.wav", "--seconds"][..], "`--seconds` needs a value"),
        (&["render", &song, "out.wav", "--from", "--seconds", "2"], "`--from` needs a value"),
        (&["listen", &song, "--bars", "2", "--png", "--json"], "`--png` needs a value"),
        (&["edit", &song, "song.ts", "--old", "x", "--new"], "`--new` needs a value"),
        (&["clips", &song, "move", "kick/hits:my53vvkc", "--by"], "`--by` needs a value"),
        // The review's W2: --help where a value goes is no help, and exit 0 would say it was done.
        (&["render", &song, "out.wav", "--seconds", "--help"], "`--seconds` needs a value"),
        (&["render", &song, "out.wav", "--seconds", "-h"], "`--seconds` needs a value"),
    ] {
        let err = s.refused(args);
        assert!(err.contains(says), "m2 {args:?}: {err}");
        assert!(err.contains(&format!("usage: m2 {}", args[0])), "m2 {args:?}: {err}");
    }
}

// AGENT-13: a place word that starts with `-` is a flag, and refused as one; a lone `-` is standard
// input only where the command reads it. A flag named twice, or with its other, is refused too.
#[test]
fn a_word_that_starts_with_a_dash_is_refused_where_it_isnt_taken() {
    let s = Scratch::new("dash");
    let song = s.song();
    for (args, says) in [
        (&["new", "-x"][..], "`-x` isn't a flag of m2 new"),
        (&["render", &song, "-"], "`-` can't be <out.wav>"),
        (&["import", &song, "-"], "`-` can't be <file.wav or file.flac>"),
        (&["search", "-"], "`-` can't be <words>"),
        (&["render", &song, "a.wav", "--seconds", "1", "--seconds", "2"], "`--seconds` is given twice"),
        (&["render", &song, "a.wav", "--from", "1", "--from-bar", "2"], "`--from` and `--from-bar` are one or the other"),
        (&["new", "x", "--template", "empty", "--example", "first-song"], "`--template` and `--example` are one or the other"),
        // The review's W3: `-` as a flag's value is standard input only where the usage says so.
        (&["listen", &song, "--bars", "1", "--png", "-"], "`-` can't be the value of `--png`"),
        (&["bounce", &song, "name", "--export", "-"], "`-` can't be the value of `--export`"),
        (&["registry", "pending", "r", "--out", "-"], "`-` can't be the value of `--out`"),
    ] {
        let err = s.refused(args);
        assert!(err.contains(says), "m2 {args:?}: {err}");
    }
}

// AGENT-13: too many words, or too few, are refused, naming the word or what is missing; so is a
// word a command takes from a few (a pack's `new`, `check`, …) that isn't one of them.
#[test]
fn too_many_or_too_few_words_are_refused() {
    let s = Scratch::new("count");
    let song = s.song();
    for (args, says) in [
        (&["render", &song][..], "m2 render needs <out.wav>"),
        (&["render", &song, "a.wav", "b.wav"], "`b.wav` is one word too many for m2 render"),
        (&["point", &song, "bars:9", "listen", "here"], "`here` is one word too many for m2 point"),
        (&["mute-in-song", &song, "kick"], "m2 mute-in-song needs on or off"),
        (&["mute-in-song", &song, "kick", "hats"], "m2 mute-in-song ends with on or off, not `hats`"),
        (&["pack"], "m2 pack needs new, import-sfz, check, build, install, get, list, remove, lock or update"),
        (
            &["pack", "frob", "x"],
            "m2 pack takes new, import-sfz, check, build, install, get, list, remove, lock or update next, not `frob`",
        ),
        (&["clips"], "m2 clips needs <project> then create, move, resize, split, join, duplicate, quantize, delete or json"),
        (&["pack", "list", "x"], "`x` is one word too many for m2 pack list"),
        (&["pack", "new", "x"], "m2 pack new needs `--name`"),
        (
            &["clips", &song, "fold", "kick/hits:x"],
            "create, move, resize, split, join, duplicate, quantize, delete or json next, not `fold`",
        ),
        (&["clips", &song, "delete", "kick/hits@9.3.2", "--copy"], "`--copy` isn't a flag of m2 clips <project> delete"),
        (&["docs", "window", "shortcuts"], "`shortcuts` is one word too many for m2 docs"),
        // The review's W7: the command named as it was typed.
        (&["build", &song, "--nope"], "`--nope` isn't a flag of m2 build"),
        // The review's W6: the flag that doesn't go with the other words is the one named.
        (&["point", &song, "bars:9", "look", "--clear"], "`--clear` doesn't go with `bars:9`"),
    ] {
        let err = s.refused(args);
        assert!(err.contains(says), "m2 {args:?}: {err}");
    }
}

// AGENT-13: a negative number is a value, not a flag; and flags may come before the place words.
// (Text given to `m2 edit` that reads like `--why` is that text: `tests/edit.rs`.)
#[test]
fn numbers_and_flags_in_any_order_are_read() {
    let s = Scratch::new("read");
    let song = s.song();
    let o = s.m2(&["set", &song, "song.ts#2/0.hats.pan", "-0.3"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(std::fs::read_to_string(s.0.join("song/song.ts")).unwrap().contains("-0.3"));
    let out = s.0.join("here/out.wav");
    let o = s.m2(&["render", "--seconds", "0.25", &song, out.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(String::from_utf8_lossy(&o.stdout).starts_with("rendered "), "{}", String::from_utf8_lossy(&o.stdout));
    assert!(out.is_file());
}

// The review's W1: `--beats` says how every `@place` of a clip gesture reads, so each gesture that
// names clips by place takes it, as it did before AGENT-13. (`words.rs` asserts, in a debug build,
// that a command asks only for flags its form shows: tests/clips.rs runs each gesture.)
#[test]
fn every_clip_gesture_reads_places_in_beats() {
    let s = Scratch::new("beats");
    let song = s.song();
    let hits = || std::fs::read_to_string(s.0.join("song/data/kick/hits.json")).unwrap();
    assert!(hits().contains("my53vvkc"));
    let o = s.m2(&["clips", &song, "delete", "kick/hits@0", "--beats"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(!hits().contains("my53vvkc"), "deleted by its place in beats");
    for gesture in
        [&["join", "kick/hits@0", "kick/hits@64"][..], &["duplicate", "kick/hits@0"], &["quantize", "kick/hits@0", "--grid", "1"]]
    {
        let o = s.m2(&[&["clips", &song][..], gesture, &["--beats"]].concat());
        let err = String::from_utf8_lossy(&o.stderr);
        assert_ne!(o.status.code(), Some(2), "m2 clips {gesture:?} --beats: {err}");
        assert!(err.to_lowercase().contains("no clip"), "m2 clips {gesture:?} --beats reads its places, and finds none there: {err}");
    }
}

// The review's W2: `m2 edit`'s texts (`<text>`) take the next word whatever it is (review F9), the
// command's own flags and -h included.
#[test]
fn m2_edit_takes_any_text() {
    let s = Scratch::new("text");
    let song = s.song();
    let read = || std::fs::read_to_string(s.0.join("song/song.ts")).unwrap();
    for (old, new) in [("The root module", "--all"), ("--all", "-h"), ("-h", "--old"), ("--old", "The root module")] {
        let o = s.m2(&["edit", &song, "song.ts", "--old", old, "--new", new]);
        assert!(o.status.success(), "--old {old} --new {new}: {}", String::from_utf8_lossy(&o.stderr));
        assert!(String::from_utf8_lossy(&o.stdout).contains("replaced 1 place"), "{}", String::from_utf8_lossy(&o.stdout));
        assert!(read().starts_with(&format!("// {new}: whatever")), "{}", &read()[..40]);
    }
}

// The review's W3: `m2 listen --with -` reads the Tx from standard input, as its usage now says.
#[test]
fn listen_with_reads_standard_input() {
    let s = Scratch::new("with");
    let o = s.m2(&["listen", &s.song(), "--bars", "1", "--with", "-"]);
    let err = String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.status.code(), Some(1), "{err}");
    assert!(err.contains("--with - isn't a Tx"), "standard input was read, and it was empty: {err}");
}

// The review's W4: a word with a space in it is text, not a flag, though it starts with `-`; a new
// song's folder still can't start with `-` (`-5` reads as a number).
#[test]
fn text_that_starts_with_a_dash_and_a_new_folder() {
    let s = Scratch::new("text-dash");
    let o = s.m2(&["point", &s.song(), "bars:9", "- check the kick"]);
    let err = String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.status.code(), Some(1), "{err}");
    assert!(err.contains("music2 doesn't have this song open"), "taken, then refused for the app: {err}");
    // `-5` reads as a number, so m2 new's own words refuse it; `-.x` doesn't, so it is a flag.
    for (name, says) in [("-5", "can't start with `-`"), ("-.x", "`-.x` isn't a flag of m2 new")] {
        let before = s.files();
        let o = s.m2(&["new", name]);
        let err = String::from_utf8_lossy(&o.stderr);
        assert!(!o.status.success() && err.contains(says), "m2 new {name}: {err}");
        assert_eq!(s.files(), before, "m2 new {name} made something");
    }
}

// The review's W5: a flag that is gone reaches its command, with a value or none, given once or
// more, which says why it is gone.
#[test]
fn a_gone_flag_is_said_gone_however_it_is_given() {
    let s = Scratch::new("gone");
    let song = s.song();
    for args in [&["bounce", &song, "name", "--sr"][..], &["render", &song, "out.wav", "--sr", "1", "--sr", "2"]] {
        let o = s.m2(args);
        let err = String::from_utf8_lossy(&o.stderr);
        assert_eq!(o.status.code(), Some(1), "m2 {args:?}: {err}");
        assert!(err.contains("--sr is gone"), "m2 {args:?}: {err}");
    }
}

// The review's W6: `--as` where a flag goes, before the end, is said to go at the end, as anywhere
// else (AGENT-10): the flag before it takes no value.
#[test]
fn as_where_a_flag_goes_is_said_to_go_last() {
    let s = Scratch::new("as");
    let o = s.m2(&["listen", &s.song(), "--as-heard", "--as", "claude", "--json"]);
    let err = String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.status.code(), Some(1), "{err}");
    assert!(err.contains("--as and --why go at the end"), "{err}");
}
