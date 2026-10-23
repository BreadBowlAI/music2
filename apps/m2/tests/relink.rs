//! Data a rename left behind: every command that reads the project says so, with the commands
//! that fix it, and `m2 relink` / `m2 delete-data` fix it (DIAG-4, DIAG-6). Bounces name tracks
//! by key (ID-11), and renders run at the project's rate (RENDER-31a).

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

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

// DIAG-4, DIAG-6, ID-17: a track key renamed by hand leaves its folder unclaimed; every command
// that opens the project prints it with the commands that fix it.
#[test]
fn every_command_says_what_a_rename_left_behind_and_relink_puts_it_back() {
    let temp = Temp(std::env::temp_dir().join(format!("m2-relink-cli-{}", std::process::id())));
    let song = temp.0.join("first-song");
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song"), &song);
    let kick = std::fs::read_to_string(song.join("tracks/kick.ts")).unwrap();
    // The key changes by hand; the label stays "Kick" (ID-17).
    std::fs::write(song.join("tracks/kick.ts"), kick.replacen("track(\"kick\"", "track(\"big-kick\"", 1)).unwrap();
    let notes = std::fs::read_to_string(song.join("data/kick/hits.json")).unwrap();
    let dir = song.to_str().unwrap();

    // A command run fresh knows only the key the data is filed under.
    let warning = "warning: Data filed under kick isn't playing. There's no track called kick any more, so nothing plays what \
                   data/kick holds: notes in hits. Did you rename it? Move its data to Kick (big-kick) to hear it again, or delete \
                   it. [data.unclaimed]";
    let folder = format!("  move it:   m2 relink {dir} data/kick data/big-kick");
    let lane = format!("  move it:   m2 relink {dir} data/kick/hits.json data/big-kick/hits.json");
    let delete = format!("  delete it: m2 delete-data {dir} data/kick");
    let wav = temp.0.join("out.wav");
    let wav = wav.to_str().unwrap();
    let commands: [&[&str]; 11] = [
        &["check", dir],
        &["render", dir, wav, "--seconds", "0.1"],
        &["bounce", dir, "kick-bounce", "--seconds", "0.1"],
        &["graph", dir],
        &["controls", dir],
        &["set", dir, "song.ts#0/0", "1"],
        &["cables", dir],
        &["connect", dir, "nothing", "here"],
        &["disconnect", dir, "nothing/here"],
        &["effects", dir],
        &["remove-effect", dir, "nothing"],
    ];
    for args in commands {
        let err = stderr(&m2(args));
        assert!(err.contains(warning) && err.contains(&folder) && err.contains(&lane) && err.contains(&delete), "m2 {}:\n{err}", args[0]);
    }

    // The whole folder, given as a path from here as well as one in the project.
    let out = m2(&["relink", dir, song.join("data/kick").to_str().unwrap(), "data/big-kick"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("data/kick: `unused data` → `Kick (big-kick)` (revision 2"));
    assert!(!stderr(&out).contains("warning"), "{}", stderr(&out));
    assert_eq!(std::fs::read_to_string(song.join("data/big-kick/hits.json")).unwrap(), notes);
    assert!(!song.join("data/kick").exists());
    let out = m2(&["check", dir]);
    assert!(out.status.success() && !stderr(&out).contains("warning"), "{}", stderr(&out));

    // Again: nothing to move, and nothing is touched.
    let out = m2(&["relink", dir, "data/kick", "data/big-kick"]);
    assert!(!out.status.success() && stderr(&out).contains("edit rejected: the project has no data/kick"), "{}", stderr(&out));

    // Data no lane can take is deleted on request only, and claimed data never.
    std::fs::create_dir_all(song.join("data/snare")).unwrap();
    std::fs::write(song.join("data/snare/hits.json"), &notes).unwrap();
    let err = stderr(&m2(&["check", dir]));
    assert!(err.contains("warning: data/snare/hits.json: Notes on snare › hits aren't playing.") && !err.contains("m2 relink"), "{err}");
    let out = m2(&["delete-data", dir, "data/big-kick/hits.json"]);
    assert!(!out.status.success() && song.join("data/big-kick/hits.json").exists(), "{}", stderr(&out));
    let out = m2(&["delete-data", dir, "data/snare/hits.json"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("data/snare/hits.json: `unused notes` → `deleted`"));
    assert!(!song.join("data/snare/hits.json").exists() && !stderr(&out).contains("warning"));

    // A data problem's fix, by its code (DIAG-1): a clip without an id gets one.
    let anonymous = notes.replacen(&format!("\"id\": \"{}\", ", id(&notes)), "", 1);
    std::fs::write(song.join("data/big-kick/hits.json"), &anonymous).unwrap();
    let err = stderr(&m2(&["check", dir]));
    let fix = format!("  fix it:    m2 fix {dir} data/big-kick/hits.json assign-clip-ids   (Give every clip an id)");
    assert!(err.contains("notice: data/big-kick/hits.json: A clip has no id") && err.contains(&fix), "{err}");
    let out = m2(&["fix", dir, "data/big-kick/hits.json", "assign-clip-ids"]);
    assert!(out.status.success() && !stderr(&out).contains("notice"), "{}", stderr(&out));
}

/// The first clip id in a lane file.
fn id(lane: &str) -> &str {
    let at = lane.find("\"id\": \"").unwrap() + 7;
    &lane[at..at + lane[at..].find('"').unwrap()]
}

// ID-11, RENDER-2: `m2 bounce --track` takes a track's key, never its label, and its
// provenance records the key and every package file the build and the runtime read.
#[test]
fn bounce_names_a_track_by_its_key() {
    let temp = Temp(std::env::temp_dir().join(format!("m2-bounce-cli-{}", std::process::id())));
    let song = temp.0.join("first-song");
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song"), &song);
    let dir = song.to_str().unwrap();
    let out = m2(&["bounce", dir, "kick-bounce", "--track", "Kick", "--seconds", "0.1"]);
    assert!(!out.status.success() && stderr(&out).contains("--track takes a track's key, and there's no track `Kick`"), "{}", stderr(&out));
    let out = m2(&["bounce", dir, "kick-bounce", "--track", "kick", "--seconds", "0.1"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let provenance: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(song.join("assets/kick-bounce.wav.json")).unwrap()).unwrap();
    assert_eq!(provenance["bounced_from"], serde_json::json!({"module": "kick", "export": "out"}));
    let files = provenance["packages"]["std"]["files"].as_object().unwrap();
    assert!(files.keys().any(|f| f.starts_with("std/")) && files.values().all(|h| h.as_str().is_some_and(|h| h.len() == 64)), "{files:?}");
    // What the runtime ran: the kernels' wasm and the manifest that declares them.
    assert!(["std/m2pkg.toml", "std/kernels/std.wasm", "std/src/instruments.ts"].iter().all(|f| files.contains_key(*f)), "{files:?}");
    // Never over an asset that exists (FMT-14).
    let out = m2(&["bounce", dir, "kick-bounce", "--track", "kick", "--seconds", "0.1"]);
    assert!(!out.status.success() && stderr(&out).contains("assets are never replaced"), "{}", stderr(&out));
}

// RENDER-31a: offline renders run at the project's sample_rate, and never run the DSP at another.
#[test]
fn renders_run_at_the_projects_sample_rate() {
    let temp = Temp(std::env::temp_dir().join(format!("m2-render-rate-cli-{}", std::process::id())));
    let song = temp.0.join("starter");
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../templates/starter"), &song);
    let manifest = std::fs::read_to_string(song.join("music2.toml")).unwrap();
    std::fs::write(song.join("music2.toml"), manifest.replace("sample_rate = 48000", "sample_rate = 44100")).unwrap();
    let (dir, wav) = (song.to_str().unwrap(), temp.0.join("out.wav"));
    let out = m2(&["render", dir, wav.to_str().unwrap(), "--seconds", "0.5"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let spec = hound::WavReader::open(&wav).unwrap().spec();
    assert_eq!(spec.sample_rate, 44_100);
    let out = m2(&["render", dir, wav.to_str().unwrap(), "--seconds", "0.5", "--sr", "48000"]);
    assert!(!out.status.success() && stderr(&out).contains("renders at its own sample_rate (44100 Hz"), "{}", stderr(&out));
    let out = m2(&["bounce", dir, "kick-44k", "--track", "kick", "--seconds", "0.1"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(hound::WavReader::open(song.join("assets/kick-44k.wav")).unwrap().spec().sample_rate, 44_100);
}

// FMT-14, WRITE-18: a bounce creates its asset and its provenance create-only. An asset name
// may have folders, and a provenance left where the new one goes is never replaced.
#[test]
fn bounce_creates_its_asset_and_provenance_create_only() {
    let temp = Temp(std::env::temp_dir().join(format!("m2-bounce-new-cli-{}", std::process::id())));
    let song = temp.0.join("first-song");
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song"), &song);
    let dir = song.to_str().unwrap();
    std::fs::create_dir_all(song.join("assets")).unwrap();
    std::fs::write(song.join("assets/b.wav.json"), "{\"someone\": \"else wrote this\"}\n").unwrap();
    let out = m2(&["bounce", dir, "b", "--seconds", "0.1"]);
    assert!(!out.status.success() && stderr(&out).contains("b.wav.json exists, and assets are never replaced"), "{}", stderr(&out));
    assert_eq!(std::fs::read_to_string(song.join("assets/b.wav.json")).unwrap(), "{\"someone\": \"else wrote this\"}\n");
    assert!(!song.join("assets/b.wav").exists(), "nothing is rendered over or beside it");

    // In a folder: the provenance goes beside the asset, in that folder.
    let out = m2(&["bounce", dir, "vox/keys", "--track", "keys", "--seconds", "0.1"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(song.join("assets/vox/keys.wav").is_file() && song.join("assets/vox/keys.wav.json").is_file());
    let provenance: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(song.join("assets/vox/keys.wav.json")).unwrap()).unwrap();
    assert_eq!(provenance["bounced_from"]["module"], "keys");
}

// FMT-14, ID-11: `--place` is checked whole before anything is rendered, so a label where a
// key belongs leaves no asset behind to block the retry.
#[test]
fn bounce_checks_where_it_is_placed_before_writing() {
    let temp = Temp(std::env::temp_dir().join(format!("m2-bounce-place-cli-{}", std::process::id())));
    let song = temp.0.join("resample");
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/resample"), &song);
    let dir = song.to_str().unwrap();
    let lane = std::fs::read_to_string(song.join("data/chops/clips.json")).unwrap();
    for (place, why) in [
        ("Chops.clips@16", "`Chops` is the label of `chops`"),
        ("chops.cips@16", "`chops` has no audio lane `cips`: its audio lanes are clips"),
        ("keys.chords@16", "`keys` has no audio lane `chords`: its audio lanes are none"),
        ("chops.clips@x", "--place beat"),
        ("chops@16", "--place takes track.lane@beat"),
    ] {
        let out = m2(&["bounce", dir, "k6", "--track", "keys", "--seconds", "0.1", "--place", place]);
        assert!(!out.status.success() && stderr(&out).contains(why), "{place}: {}", stderr(&out));
        assert!(!song.join("assets/k6.wav").exists() && !song.join("assets/k6.wav.json").exists(), "{place}: nothing written");
    }
    assert_eq!(std::fs::read_to_string(song.join("data/chops/clips.json")).unwrap(), lane);
    let out = m2(&["bounce", dir, "k6", "--track", "keys", "--seconds", "0.1", "--place", "chops.clips@16"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(String::from_utf8_lossy(&out.stdout).contains("placed it on chops.clips at beat 16"));
    assert!(std::fs::read_to_string(song.join("data/chops/clips.json")).unwrap().contains("\"asset\": \"k6.wav\""));
}

// DIAG-1, DIAG-4: a problem that stops the open or the build is printed with its severity and
// its code, as every other problem is.
#[test]
fn blocking_problems_print_their_code() {
    let temp = Temp(std::env::temp_dir().join(format!("m2-codes-cli-{}", std::process::id())));
    let song = temp.0.join("first-song");
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song"), &song);
    let dir = song.to_str().unwrap();
    let manifest = std::fs::read_to_string(song.join("music2.toml")).unwrap();
    std::fs::write(song.join("music2.toml"), manifest.replace("format = 1", "format = 2\nwhat = 1")).unwrap();
    let err = stderr(&m2(&["check", dir]));
    assert!(err.contains("error: music2.toml: this project is in format 2") && err.contains("[format.newer]"), "{err}");
    std::fs::write(song.join("music2.toml"), manifest).unwrap();
    let kick = std::fs::read_to_string(song.join("tracks/kick.ts")).unwrap();
    std::fs::write(song.join("tracks/kick.ts"), kick.replacen("track(\"kick\"", "track(\"Kick\"", 1)).unwrap();
    let out = m2(&["check", dir]);
    assert!(
        !out.status.success() && stderr(&out).contains("error: tracks/kick.ts:") && stderr(&out).contains("[key.invalid]"),
        "{}",
        stderr(&out)
    );
}
