//! The project lock from the terminal (WRITE-17, WRITE-18; the step 6 gate): a command that
//! writes a project while another process holds its lock fails, changes nothing and names the
//! holder; `m2 bounce` without `--place` only creates new assets and works all the same, as do the
//! commands that only read.

use std::collections::BTreeMap;
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
    Command::new(env!("CARGO_BIN_EXE_m2")).env("HOME", std::env::temp_dir().join("m2-tests-no-app")).args(args).output().unwrap()
}

/// Every file of the project but `.music2/`, with its bytes.
fn tree(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.ends_with(".music2") {
                continue;
            }
            if p.is_dir() {
                stack.push(p);
            } else {
                out.insert(p.strip_prefix(dir).unwrap().to_path_buf(), std::fs::read(&p).unwrap());
            }
        }
    }
    out
}

// WRITE-18, the step 6 gate.
#[test]
fn a_write_while_another_process_holds_the_lock_fails_and_changes_nothing() {
    let temp = Temp(std::env::temp_dir().join(format!("m2-lock-cli-{}", std::process::id())));
    let song = temp.0.join("resample");
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/resample"), &song);
    std::fs::write(song.join("data/chops/old.json"), "{\"clips\": []}\n").unwrap();
    let dir = song.to_str().unwrap();
    // The app, holding the project (this process stands for it: the lock is the process's).
    let held = m2_session::lock::acquire(&song, "music2", m2_session::lock::RETRY).unwrap();
    let pid = std::process::id().to_string();
    let before = tree(&song);
    let controls = String::from_utf8_lossy(&m2(&["controls", dir]).stdout).into_owned();
    let id = controls.lines().find(|l| l.contains('#')).and_then(|l| l.split_whitespace().next()).expect("a knob").to_string();
    // An agent's Tx: deleting the unread lane file, on its hash (WRITE-3).
    let sha = m2_song::Sha::of(&std::fs::read(song.join("data/chops/old.json")).unwrap()).prefixed();
    let tx = temp.0.join("tx.json");
    let json = format!(
        r#"{{"id": "tx-cli", "base": {{"data/chops/old.json": "{sha}"}}, "label": ["unused", "deleted"],
            "ops": [{{"op": "remove", "file": "data/chops/old.json"}}], "expect": "identical"}}"#
    );
    std::fs::write(&tx, json).unwrap();
    let tx = tx.to_str().unwrap();
    let writes: Vec<Vec<&str>> = vec![
        vec!["set", dir, &id, "0.123"],
        // The gesture commands refuse before they plan anything.
        vec!["connect", dir, "keys.out", &id],
        vec!["disconnect", dir, &id],
        vec!["add-effect", dir, &id, "@m2/effects:delay"],
        vec!["remove-effect", dir, &id],
        vec!["relink", dir, "data/chops/old.json", "data/keys/old.json"],
        vec!["delete-data", dir, "data/chops/old.json"],
        vec!["fix", dir, "data/chops/old.json", "delete-data"],
        vec!["plugin-state", dir, "keys/lead", "clap:org.example.none"],
        vec!["tx", dir, tx],
        vec!["bounce", dir, "k-placed", "--track", "keys", "--seconds", "0.1", "--place", "chops.clips@16"],
        // The first new builders (HOME-2, ID-10) are writers too.
        vec!["automate", dir, &id],
        vec!["label", dir, "keys", "Lead Keys"],
        // The track builders and the layout write too (8b.1).
        vec!["add-track", dir, "@m2/synths:synth-pluck"],
        vec!["duplicate-track", dir, "kick"],
        vec!["delete-track", dir, "keys"],
        vec!["layout", dir, "order", "chops", "kick", "keys"],
        vec!["layout", dir, "colour", "kick", "teal"],
        // The song bar's builders (8b.2).
        vec!["tempo", dir, "128"],
        vec!["meter", dir, "7/8", "--at", "9"],
        vec!["title", dir, "Night Drive"],
        // Studio lane A's A4.
        vec!["import", dir, "assets/keys-bounce.wav", "--track", "chops"],
    ];
    for args in &writes {
        let out = m2(args);
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{args:?} went ahead");
        assert!(err.contains("nothing was changed") && err.contains("music2") && err.contains(&pid), "{args:?}: {err}");
        assert_eq!(tree(&song), before, "{args:?} changed nothing");
    }
    let placing = writes.iter().find(|w| w.contains(&"--place")).unwrap();
    let place = String::from_utf8_lossy(&m2(placing).stderr).into_owned();
    assert!(place.contains("data/chops/clips.json"), "the lane file it would have written: {place}");
    // Creating a new asset can't race: it works while the app is open, as reading does.
    let out = m2(&["bounce", dir, "k", "--track", "keys", "--seconds", "0.1"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(song.join("assets/k.wav").exists() && song.join("assets/k.wav.json").exists());
    // WRITE-11: a check only reads, and prints the delta and its hash.
    let before = tree(&song);
    let checked = m2(&["tx", dir, tx, "--check"]);
    assert!(checked.status.success(), "{}", String::from_utf8_lossy(&checked.stderr));
    let said: serde_json::Value = serde_json::from_slice(&checked.stdout).unwrap();
    let said = &said["checked"];
    assert!(said["hash"].as_str().unwrap().starts_with("sha256:") && said["delta"].is_object(), "{said}");
    assert_eq!(tree(&song), before, "a check writes nothing");
    for args in [vec!["check", dir], vec!["graph", dir], vec!["controls", dir], vec!["cables", dir], vec!["effects", dir]] {
        let out = m2(&args);
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }
    let wav = temp.0.join("out.wav");
    assert!(m2(&["render", dir, wav.to_str().unwrap(), "--seconds", "0.1"]).status.success());
    // Let go, a write goes ahead; an agent's Tx commits on the hash its check showed (WRITE-11).
    drop(held);
    let out = m2(&["set", dir, &id, "0.123"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let hash = said["hash"].as_str().unwrap();
    let out = m2(&["tx", dir, tx, "--seen", hash]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let committed: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(committed["committed"]["hash"], hash, "{committed}");
    assert!(!song.join("data/chops/old.json").exists());
    // Again, its base is gone: a conflict, as JSON, exit 1.
    let out = m2(&["tx", dir, tx]);
    assert!(!out.status.success());
    let refused: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(refused["conflict"]["files"][0]["file"], "data/chops/old.json", "{refused}");
}
