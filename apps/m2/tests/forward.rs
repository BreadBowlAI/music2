//! `m2` while the app has the song open (AGENT-1, AGENT-3, AGENT-4, AGENT-10; track 2 of the
//! agents design): a write is handed to the app, as one undo step there named by `--as` and
//! `--why`; what the musician holds makes it busy (exit 3), and the busy answer says not to write
//! the file directly instead; an app that can't be reached, or that quits on the way, is said in
//! words; `m2 undo` takes back only the agent's own step. This process stands for the app: its
//! server has the song open, so it holds the song's lock as `music2`.

mod app;

use std::path::Path;
use std::time::{Duration, Instant};

use app::{App, Temp, copy, example, fake, m2_in, m2_started, packages, publish, said, until_said};
use serde_json::{Value, json};

/// The id of the step on top of the app's undo.
fn step(a: &App) -> u64 {
    a.top()["step"].as_u64().unwrap_or(0)
}

/// Run `m2 args…` with the app open, which must succeed: what it said.
fn ok(a: &App, args: &[&str]) -> String {
    let o = a.m2(args);
    assert!(o.status.success(), "{args:?}: {}", said(&o));
    said(&o)
}

/// A mono 16-bit WAV of `seconds` at 48 kHz, at `path`.
fn wav(path: &Path, seconds: f64) {
    let spec = hound::WavSpec { channels: 1, sample_rate: 48_000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for i in 0..(seconds * 48_000.0) as usize {
        w.write_sample(((i as f32 * 0.03).sin() * 8_000.0) as i16).unwrap();
    }
    w.finalize().unwrap();
}

// AGENT-1: with the app holding the song's lock, `m2 set` builds and checks its edit, and the app
// commits it: the file changes, the app's undo has one step, the agent's, and `m2` prints what a
// local write prints, then that it is one undo step in the open app.
#[test]
fn a_write_while_the_app_holds_the_lock_lands_in_the_app() {
    let a = App::open("first-song", |_| {});
    let out = ok(&a, &["set", a.dir(), "song.ts#2/0.hats.pan", "0.4", "--as", "Claude", "--why", "wider hats"]);
    assert!(out.contains("song.ts: `0.3` → `0.4`"), "as a local write prints: {out}");
    assert!(out.contains("in the open app: one undo step, Claude: wider hats"), "{out}");
    assert!(a.read("song.ts").contains("pan: 0.4"));
    let top = a.top();
    assert_eq!((&top["source"], &top["by"], &top["name"]), (&json!("agent"), &json!("Claude"), &json!("wider hats")), "{top}");
    assert_eq!(a.running.state.graph.state().unwrap()["view"]["revision"], json!(2), "the app's revision");
}

// AGENT-1: every command that writes a song works with the app open, each as one undo step of the
// agent's in the app (5.2's list, and the new `edit` and `write`).
#[test]
fn every_write_command_forwards() {
    let a = App::open("first-song", |song| {
        std::fs::write(song.join("data/kick/old.json"), "{\"clips\": []}\n").unwrap();
    });
    let dir = a.dir();
    let files = a.temp.0.join("files");
    std::fs::create_dir_all(&files).unwrap();
    wav(&files.join("tone.wav"), 0.5);
    let tx = files.join("tx.json");
    let gesture = files.join("gesture.json");
    std::fs::write(&gesture, r#"{"op": "move", "clips": [{"module": "kick", "lane": "hits", "id": "my53vvkc"}], "by": 1}"#).unwrap();
    let text = files.join("lead.ts");
    std::fs::write(&text, "// a new file\n").unwrap();
    // Calls are counted in their file, so a gesture that adds or takes out a call comes after
    // those that name the calls after it.
    let rows: Vec<Vec<String>> = [
        vec!["connect", dir, "tracks/bass.ts#3", "tracks/bass.ts#5/@phase"],
        vec!["disconnect", dir, "tracks/bass.ts#5/1.phase"],
        vec!["set", dir, "song.ts#2/0.hats.pan", "0.4"],
        vec!["automate", dir, "tracks/bass.ts#5/1.period"],
        vec!["unautomate", dir, "bass/cutoff", "--at", "3"],
        vec!["label", dir, "keys", "Lead Keys"],
        vec!["add-track", dir, "synth.pluck", "--name", "Lead Vox"],
        vec!["import", dir, files.join("tone.wav").to_str().unwrap(), "--at", "3"],
        vec!["mute-in-song", dir, "kick", "on"],
        vec!["clips", dir, "move", "kick/hits:my53vvkc", "--by", "32"],
        vec!["clips", dir, "json", gesture.to_str().unwrap()],
        vec!["duplicate-track", dir, "bass"],
        vec!["delete-track", dir, "lead-vox"],
        vec!["layout", dir, "colour", "hats", "violet"],
        vec!["tempo", dir, "128"],
        vec!["meter", dir, "7/8", "--at", "9"],
        vec!["title", dir, "Night Drive"],
        vec!["add-effect", dir, "tracks/keys.ts#1", "std:delay"],
        vec!["remove-effect", dir, "tracks/keys.ts#1"],
        vec!["delete-data", dir, "data/kick/old.json"],
        vec!["edit", dir, "tracks/hats.ts", "--old", "track(\"hats\"", "--new", "track(\"hat2\""],
        vec!["relink", dir, "data/hats", "data/hat2"],
        vec!["plugin-state", dir, "keys/lead", "clap:m2.probe"],
        vec!["write", dir, "tracks/lead.ts", text.to_str().unwrap()],
        vec!["tx", dir, tx.to_str().unwrap()],
    ]
    .iter()
    .map(|r| r.iter().map(|s| s.to_string()).collect())
    .collect();
    for row in &rows {
        if row[0] == "tx" {
            // An agent's Tx on the hats' pan, based on the file as it is now.
            let song = a.read("song.ts");
            let at = song.find("pan: 0.4").unwrap() + 5;
            let tx_json = json!({"id": "tx-cli", "base": {"song.ts": {"sha": format!("sha256:{}", m2_song::Sha::of(song.as_bytes()).hex()), "text": song}},
                "label": ["0.4", "0.45"], "ops": [{"op": "text_edit", "file": "song.ts", "edits": [{"start": at, "end": at + 3, "expected": "0.4", "text": "0.45"}]}],
                "expect": {"values": [[{"written": {"file": "song.ts", "range": [at, at + 3]}}, 0.45]]}});
            std::fs::write(&tx, tx_json.to_string()).unwrap();
        }
        let before = step(&a);
        let args: Vec<&str> = row.iter().map(String::as_str).chain(["--as", "Claude"]).collect();
        let o = a.m2_with(&args, &[("M2_PROBE", "1")]);
        assert!(o.status.success(), "{row:?}: {}", said(&o));
        if row[0] == "tx" {
            let v: Value = serde_json::from_slice(&o.stdout).expect("m2 tx answers in JSON alone");
            assert!(v["committed"]["hash"].is_string(), "{v}");
        }
        assert!(said(&o).contains("in the open app: one undo step"), "{row:?}: {}", said(&o));
        let top = a.top();
        assert!(step(&a) > before, "{row:?}: a new step: {top}");
        assert_eq!((&top["source"], &top["by"]), (&json!("agent"), &json!("Claude")), "{row:?}: {top}");
    }
}

// AGENT-10: the step is named by `--why`, its author by `--as`, else `M2_AS`, else `agent`, which
// `m2` says once, on stderr. Both are plain text, at most 80 characters.
#[test]
fn the_step_is_named_by_as_and_why() {
    let a = App::open("first-song", |_| {});
    let o = a.m2_with(&["set", a.dir(), "song.ts#2/0.hats.pan", "0.4", "--why", "wider hats"], &[("M2_AS", "Codex")]);
    assert!(o.status.success(), "{}", said(&o));
    assert_eq!((&a.top()["by"], &a.top()["name"]), (&json!("Codex"), &json!("wider hats")));
    let o = a.m2_with(&["set", a.dir(), "song.ts#2/0.hats.pan", "0.45", "--as", "Claude 2"], &[("M2_AS", "Codex")]);
    assert!(o.status.success(), "{}", said(&o));
    assert_eq!((&a.top()["by"], &a.top()["given"]), (&json!("Claude 2"), &json!(false)), "--as wins; no words: its plain name");
    let o = a.m2(&["set", a.dir(), "song.ts#2/0.hats.pan", "0.5"]);
    assert!(o.status.success(), "{}", said(&o));
    assert_eq!(a.top()["by"], json!("agent"));
    let err = String::from_utf8_lossy(&o.stderr);
    assert_eq!(err.matches("--as").count(), 1, "said once, on stderr: {err}");
    let long = "x".repeat(100);
    let o = a.m2(&["set", a.dir(), "song.ts#2/0.hats.pan", "0.55", "--as", "Claude", "--why", &long]);
    assert!(o.status.success(), "{}", said(&o));
    assert_eq!(a.top()["name"].as_str().unwrap().chars().count(), 80, "cut to 80");
}

// AGENT-1, the review's m8: a forwarded write doesn't wait for the lock the app holds. Timed:
// it reaches the app well before a writer's wait for a held lock (250 ms) would have ended.
#[test]
fn a_forwarded_write_doesnt_wait_for_the_lock() {
    let temp = Temp::new("fwd-nowait");
    let song = temp.0.join("song");
    copy(&example("first-song"), &song);
    let session = m2_session::Session::open(&song, packages()).unwrap();
    let state = json!({"session": 1, "dir": song.canonicalize().unwrap(), "problems": []});
    let started = std::sync::Arc::new(std::sync::Mutex::new(None::<Instant>));
    let seen = started.clone();
    let (url, asked) = fake(state, move |_| {
        seen.lock().unwrap().get_or_insert_with(Instant::now);
        Some((503, json!({"error": "not now"})))
    });
    let home = temp.0.join("home");
    publish(&home, &url, "tools", std::process::id());
    let mut fastest = Duration::MAX;
    for _ in 0..3 {
        *started.lock().unwrap() = None;
        let t = Instant::now();
        let o = m2_in(&home, &["title", song.to_str().unwrap(), "Night Drive"], &[]);
        assert!(!o.status.success());
        let reached = started.lock().unwrap().expect("it reached the app").duration_since(t);
        fastest = fastest.min(reached);
    }
    assert!(asked.lock().unwrap().iter().any(|a| a.1 == "/api/tx/commit"));
    // Starting m2, reading std and building the song take their time; a wait for the lock adds
    // 250 ms before any of it.
    let local = {
        let copy2 = temp.0.join("song2");
        copy(&example("first-song"), &copy2);
        let t = Instant::now();
        assert!(m2_in(&home, &["title", copy2.to_str().unwrap(), "Night Drive"], &[]).status.success());
        t.elapsed()
    };
    assert!(fastest < local + Duration::from_millis(200), "reached the app after {fastest:?}; a local write takes {local:?}");
    drop(session);
}

// AGENT-1, WRITE-18: a song music2 holds that `m2` can't reach (no session.json, or one naming
// another process) fails before changing anything, naming the app and its pid.
#[test]
fn a_song_held_by_an_app_m2_cant_reach_fails_naming_it() {
    let temp = Temp::new("fwd-unreached");
    let song = temp.0.join("song");
    copy(&example("first-song"), &song);
    let held = m2_session::lock::acquire(&song, "music2", m2_session::lock::RETRY).unwrap();
    let home = temp.0.join("home");
    let before = std::fs::read_to_string(song.join("song.ts")).unwrap();
    for publish_one in [false, true] {
        if publish_one {
            publish(&home, "http://127.0.0.1:9", "tools", std::process::id() + 1);
        }
        let o = m2_in(&home, &["set", song.to_str().unwrap(), "song.ts#2/0.hats.pan", "0.4"], &[]);
        let err = String::from_utf8_lossy(&o.stderr);
        assert_eq!(o.status.code(), Some(1), "{err}");
        let pid = std::process::id().to_string();
        assert!(
            err.contains("nothing was changed") && err.contains("open in music2") && err.contains(&pid) && err.contains("can't reach"),
            "{err}"
        );
    }
    assert_eq!(std::fs::read_to_string(song.join("song.ts")).unwrap(), before);
    drop(held);
}

// AGENT-3: a write on a file the musician drags a knob in waits, saying so, while the drag goes
// on; once the drag ends, it lands. The drag ends only once `m2` said it waits: on the Mac's first
// full check `m2` reached the app only after 1.5 s, when the drag had ended, and said nothing.
#[test]
fn a_write_on_a_file_with_a_knob_drag_is_busy_and_the_drag_goes_on() {
    let a = App::open("first-song", |_| {});
    let graph = &a.running.state.graph;
    let revision = graph.state().unwrap()["view"]["revision"].as_u64().unwrap();
    let lease = graph.begin(a.session, revision, "song.ts#2/0.keys.pan", Default::default()).unwrap().lease;
    let (m2, err) = m2_started(&a.home, &["set", a.dir(), "song.ts#2/0.hats.pan", "0.4", "--as", "Claude"], &[]);
    let said = until_said(&err, "waiting: you're dragging");
    // Several of its tries, each one busy.
    std::thread::sleep(Duration::from_millis(1000));
    assert!(graph.leases().iter().any(|l| l.lease == lease), "the drag goes on while the agent waits");
    graph.update(lease, Some(1), -0.3).unwrap();
    assert!(a.read("song.ts").contains("pan: 0.3"), "nothing landed meanwhile");
    graph.commit(lease, -0.3, None).unwrap();
    let o = m2.wait_with_output().unwrap();
    assert!(o.status.success(), "{said:?} {:?}", err.try_iter().collect::<Vec<_>>());
    let song = a.read("song.ts");
    assert!(song.contains("pan: 0.4") && song.contains("pan: -0.3"), "both landed: {song}");
}

// AGENT-3: a write to a file with unsaved text in the app's code editor is busy at once.
#[test]
fn a_write_to_a_file_with_unsaved_editor_text_is_busy() {
    let a = App::open("first-song", |_| {});
    a.running.state.graph.set_unsaved(a.session, vec!["song.ts".into()]).unwrap();
    let t = Instant::now();
    let o = a.m2(&["set", a.dir(), "song.ts#2/0.hats.pan", "0.4"]);
    assert_eq!(o.status.code(), Some(3), "{}", said(&o));
    assert!(t.elapsed() < Duration::from_secs(10), "at once");
    assert!(said(&o).contains("song.ts has unsaved changes in the app's code editor"), "{}", said(&o));
    assert!(a.read("song.ts").contains("pan: 0.3"));
}

// AGENT-3: busy exits 3 once it gives up, and says not to write the file directly instead.
#[test]
fn busy_exits_3_and_says_not_to_write_the_file() {
    let a = App::open("first-song", |_| {});
    let graph = &a.running.state.graph;
    let revision = graph.state().unwrap()["view"]["revision"].as_u64().unwrap();
    let lease = graph.begin(a.session, revision, "song.ts#2/0.keys.pan", Default::default()).unwrap().lease;
    let o = a.m2_with(&["set", a.dir(), "song.ts#2/0.hats.pan", "0.4"], &[("M2_BUSY_WAIT", "1")]);
    assert_eq!(o.status.code(), Some(3), "{}", said(&o));
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("you're dragging") && err.contains("Don't write the file directly instead"), "{err}");
    assert!(graph.leases().iter().any(|l| l.lease == lease), "the drag goes on");
    assert!(a.read("song.ts").contains("pan: 0.3"));
    graph.cancel(lease).unwrap();
}

// AGENT-1, the review's m1: `m2 fix` with the app open applies the fix the app offers.
#[test]
fn m2_fix_with_the_app_open_applies_the_apps_fix() {
    let a = App::open("first-song", |song| {
        let hits = std::fs::read_to_string(song.join("data/kick/hits.json")).unwrap();
        std::fs::write(song.join("data/kick/hits.json"), hits.replacen("\"id\": \"my53vvkc\", ", "", 1)).unwrap();
    });
    let before = step(&a);
    let out = ok(&a, &["fix", a.dir(), "data/kick/hits.json", "assign-clip-ids", "--as", "Claude"]);
    assert!(out.contains("in the open app: one undo step"), "{out}");
    assert!(step(&a) > before && a.top()["by"] == json!("Claude"), "{}", a.top());
    assert!(a.read("data/kick/hits.json").contains("\"id\""), "the clip has an id");
}

// AGENT-1, the review's m8: `bounce --place` with the app open renders, then reads the song and
// places the clip through the app.
#[test]
fn bounce_place_with_the_app_open_reads_the_song_after_its_render() {
    let a = App::open("resample", |_| {});
    let before = step(&a);
    let out =
        ok(&a, &["bounce", a.dir(), "k-placed", "--track", "keys", "--seconds", "0.1", "--place", "chops.clips@16", "--as", "Claude"]);
    assert!(out.contains("placed it on chops.clips") && out.contains("in the open app: one undo step"), "{out}");
    assert!(a.song.join("assets/k-placed.wav").exists());
    assert!(step(&a) > before && a.top()["by"] == json!("Claude"), "{}", a.top());
    assert!(a.read("data/chops/clips.json").contains("k-placed.wav"));
}

/// A song held by a fake app, which answers the commit as `answer` says, given a writer session of
/// the song (its lock is the app's) and the Tx's JSON: the song, `m2`'s home and the temp folder.
fn held_by_fake(
    name: &str,
    answer: impl FnMut(&mut Option<m2_session::Session>, &Value) -> Option<(u16, Value)> + Send + 'static,
) -> (Temp, String, std::path::PathBuf) {
    let temp = Temp::new(name);
    let song = temp.0.join("song");
    copy(&example("first-song"), &song);
    let mut writer = Some(m2_session::Session::open(&song, packages()).unwrap());
    let state = json!({"session": 1, "dir": song.canonicalize().unwrap(), "problems": []});
    let mut answer = answer;
    let (url, _) =
        fake(state, move |(_, path, body)| if path == "/api/tx/commit" { answer(&mut writer, body) } else { Some((404, json!({}))) });
    let home = temp.0.join("home");
    publish(&home, &url, "tools", std::process::id());
    let dir = song.to_str().unwrap().to_string();
    (temp, dir, home)
}

// AGENT-1: the app quits before it commits: nothing changed, and `m2` says to run it again.
#[test]
fn the_app_quitting_before_the_commit_changes_nothing() {
    let (temp, dir, home) = held_by_fake("fwd-quit-before", |writer, _| {
        writer.take();
        None
    });
    let o = m2_in(&home, &["set", &dir, "song.ts#2/0.hats.pan", "0.4"], &[]);
    assert_eq!(o.status.code(), Some(1), "{}", said(&o));
    assert!(
        said(&o).contains("nothing was changed: the app closed first. Run this again; it will write the files itself."),
        "{}",
        said(&o)
    );
    assert!(std::fs::read_to_string(temp.0.join("song/song.ts")).unwrap().contains("pan: 0.3"));
}

/// The fake app's answer to a commit when it is killed half way through: the journal of a commit
/// of `tx` to `song.ts` with none of its changes in the files, its staged file, and no answer.
fn killed_mid_commit(writer: &mut Option<m2_session::Session>, body: &Value) -> Option<(u16, Value)> {
    let s = writer.as_mut().unwrap();
    let tx = m2_song::Tx::from_json(&body["tx"]).unwrap();
    let checked = s.check(&tx).unwrap();
    let after = checked.candidate().snapshot.files[Path::new("song.ts")].to_string();
    let dir = s.accepted().project.dir.clone();
    let before = std::fs::read_to_string(dir.join("song.ts")).unwrap();
    let jid = "1791935974533-64110-0";
    let j = m2_session::tx::journal::Journal {
        id: jid.into(),
        tx: tx.id.clone(),
        host: "here".into(),
        host_id: m2_session::lock::host_id(),
        pid: 1,
        program: "music2".into(),
        label: tx.label.clone(),
        targets: vec![m2_session::tx::journal::JTarget::File {
            path: "song.ts".into(),
            before: Some(m2_song::Sha::of(before.as_bytes())),
            after: Some(m2_song::Sha::of(after.as_bytes())),
            text: Some(before),
            blob: false,
            staged: Some(0),
        }],
    };
    std::fs::create_dir_all(dir.join(".music2/tx").join(jid)).unwrap();
    std::fs::write(dir.join(format!(".music2/tx/{jid}.json")), j.to_json()).unwrap();
    std::fs::write(dir.join(".music2/tx").join(jid).join("0"), &after).unwrap();
    writer.take();
    None
}

// AGENT-1, WRITE-16: the app is killed half way through the commit, leaving its journal: `m2`
// names the fixes, as every command does.
#[test]
fn the_app_killed_mid_commit_names_its_fixes() {
    let (temp, dir, home) = held_by_fake("fwd-killed", killed_mid_commit);
    let o = m2_in(&home, &["set", &dir, "song.ts#2/0.hats.pan", "0.4"], &[]);
    assert_eq!(o.status.code(), Some(1), "{}", said(&o));
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains(&format!("m2 fix {dir} .music2/tx/1791935974533-64110-0.json finish-tx")) && err.contains("discard-tx"), "{err}");
    drop(temp);
}

// AGENT-1, DIAG-4: the fixes name the song as it was named, as every command's do, even when that
// path goes through a symbolic link: on a Mac the temp folder's `/var` is one, to `/private/var`,
// and the fixes once named the folder the link leads to.
#[test]
fn the_fixes_name_the_song_as_it_was_named() {
    let (temp, _, home) = held_by_fake("fwd-killed-linked", killed_mid_commit);
    let link = temp.0.join("linked");
    std::os::unix::fs::symlink(&temp.0, &link).unwrap();
    let dir = link.join("song").to_str().unwrap().to_string();
    let o = m2_in(&home, &["set", &dir, "song.ts#2/0.hats.pan", "0.4"], &[]);
    assert_eq!(o.status.code(), Some(1), "{}", said(&o));
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains(&format!("m2 fix {dir} .music2/tx/1791935974533-64110-0.json finish-tx")) && err.contains("discard-tx"), "{err}");
}

// AGENT-1: the app quits once it committed, before it answered: `m2` finds every file as its check
// made it, says it is saved but its undo step went with the app, and exits 0.
#[test]
fn the_app_quitting_after_the_commit_says_it_landed() {
    let (temp, dir, home) = held_by_fake("fwd-quit-after", |writer, body| {
        let mut s = writer.take().unwrap();
        let tx = m2_song::Tx::from_json(&body["tx"]).unwrap();
        s.commit(tx).unwrap();
        None
    });
    let o = m2_in(&home, &["set", &dir, "song.ts#2/0.hats.pan", "0.4"], &[]);
    assert!(o.status.success(), "{}", said(&o));
    assert!(said(&o).contains("it is saved, but its undo step went with the app"), "{}", said(&o));
    assert!(std::fs::read_to_string(temp.0.join("song/song.ts")).unwrap().contains("pan: 0.4"));
}

// AGENT-1: the app quits after something else changed the file: `m2` can't tell, and says so.
#[test]
fn the_app_quitting_after_another_change_says_it_cant_tell() {
    let (temp, dir, home) = held_by_fake("fwd-quit-other", |writer, _| {
        let s = writer.take().unwrap();
        let path = s.accepted().project.dir.join("song.ts");
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, text.replacen("pan: 0.3", "pan: 0.35", 1)).unwrap();
        None
    });
    let o = m2_in(&home, &["set", &dir, "song.ts#2/0.hats.pan", "0.4"], &[]);
    assert_eq!(o.status.code(), Some(1), "{}", said(&o));
    assert!(
        said(&o).contains("m2 can't tell whether this landed: the files changed meanwhile. Read the song again before you retry."),
        "{}",
        said(&o)
    );
    drop(temp);
}

// AGENT-4: `m2 undo` takes back the newest step only when it is the agent's own, by name; another
// name's, or the musician's, is refused, naming it.
#[test]
fn m2_undo_takes_back_only_its_own_step() {
    let a = App::open("first-song", |_| {});
    ok(&a, &["set", a.dir(), "song.ts#2/0.hats.pan", "0.4", "--as", "Claude", "--why", "wider hats"]);
    let o = a.m2(&["undo", a.dir(), "--as", "Codex"]);
    assert_eq!(o.status.code(), Some(1), "{}", said(&o));
    assert!(said(&o).contains("the newest change is Claude's (wider hats), not Codex's"), "{}", said(&o));
    let out = ok(&a, &["undo", a.dir(), "--as", "Claude"]);
    assert!(out.contains("wider hats"), "{out}");
    assert!(a.read("song.ts").contains("pan: 0.3"), "undone");
    ok(&a, &["set", a.dir(), "song.ts#2/0.hats.pan", "0.4", "--as", "Claude"]);
    let graph = &a.running.state.graph;
    let revision = graph.state().unwrap()["view"]["revision"].as_u64().unwrap();
    graph.set(a.session, revision, "song.ts#2/0.keys.pan", "set", -0.5, None).unwrap();
    let o = a.m2(&["undo", a.dir(), "--as", "Claude"]);
    assert_eq!(o.status.code(), Some(1), "{}", said(&o));
    assert!(said(&o).contains("nothing was undone: the newest change is yours"), "{}", said(&o));
    assert!(a.read("song.ts").contains("pan: -0.5"));
}

// AGENT-4: with no name, `m2 undo` is `agent`'s, which never takes back the musician's step.
#[test]
fn m2_undo_with_no_name_never_takes_back_the_musicians_step() {
    let a = App::open("first-song", |_| {});
    let graph = &a.running.state.graph;
    let revision = graph.state().unwrap()["view"]["revision"].as_u64().unwrap();
    graph.set(a.session, revision, "song.ts#2/0.hats.pan", "set", 0.4, None).unwrap();
    let o = a.m2(&["undo", a.dir()]);
    assert_eq!(o.status.code(), Some(1), "{}", said(&o));
    assert!(said(&o).contains("the newest change is yours"), "{}", said(&o));
    assert!(a.read("song.ts").contains("pan: 0.4"));
}

// AGENT-4: undo lives in the open app; with it closed, `m2 undo` says so.
#[test]
fn m2_undo_with_the_app_closed_says_undo_lives_in_the_app() {
    let temp = Temp::new("fwd-undo-closed");
    let song = temp.0.join("song");
    copy(&example("first-song"), &song);
    let o = m2_in(&temp.0.join("home"), &["undo", song.to_str().unwrap()], &[]);
    assert_eq!(o.status.code(), Some(1), "{}", said(&o));
    assert!(said(&o).contains("nothing to undo: undo lives in the open app"), "{}", said(&o));
}

// AGENT-1 (the review's F6): every Tx carries its bases' whole text, so a change to a big file is a
// big request: one of a few megabytes lands in the app as a small one does.
#[test]
fn a_change_to_a_big_file_lands_in_the_app() {
    let a = App::open("first-song", |song| {
        let text = std::fs::read_to_string(song.join("song.ts")).unwrap();
        std::fs::write(song.join("song.ts"), format!("{text}\n// {}\n", "x".repeat(2_200_000))).unwrap();
    });
    ok(&a, &["set", a.dir(), "song.ts#2/0.hats.pan", "0.4", "--as", "Claude"]);
    assert!(a.read("song.ts").contains("pan: 0.4"), "landed");
    let big = format!("{}// {}\n", a.read("song.ts"), "y".repeat(3_000_000));
    let from = a.temp.0.join("big.ts");
    std::fs::write(&from, &big).unwrap();
    ok(&a, &["write", a.dir(), "song.ts", from.to_str().unwrap(), "--as", "Claude"]);
    assert_eq!(a.read("song.ts"), big, "landed");
    assert_eq!(a.top()["by"], json!("Claude"));
}

// AGENT-1 (the review's F6): an answer `m2` can't read is said with its status and words, as a
// refusal: never "null", and never as a write that landed.
#[test]
fn an_answer_that_isnt_json_is_said_with_its_status() {
    let (temp, dir, home) =
        held_by_fake("fwd-not-json", |_, _| Some((413, json!({"__raw": "Failed to buffer the request body: length limit exceeded"}))));
    let o = m2_in(&home, &["set", &dir, "song.ts#2/0.hats.pan", "0.4"], &[]);
    assert_eq!(o.status.code(), Some(1), "{}", said(&o));
    let err = said(&o);
    assert!(err.contains("nothing was changed") && err.contains("413") && err.contains("length limit exceeded"), "{err}");
    assert!(!err.contains("null"), "{err}");
    assert!(std::fs::read_to_string(temp.0.join("song/song.ts")).unwrap().contains("pan: 0.3"));
}

/// What the song's `assets/` holds, hidden part files too (empty when there is none).
fn assets(a: &App) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(a.song.join("assets"))
        .map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    names.sort();
    names
}

// AGENT-1, AGENT-3 (the review of ag-routes' F2): `m2 import` while the musician compares hands its
// file to the app's import, which is busy: `m2` waits, then exits 3, and nothing reaches the song,
// no file in `assets/`, and the pill stays open. Once it closes, the import lands as one step of the
// agent's, named by the import (F10: not after `layout.json`).
#[test]
fn m2_import_while_comparing_copies_nothing() {
    let a = App::open("first-song", |_| {});
    let files = a.temp.0.join("files");
    std::fs::create_dir_all(&files).unwrap();
    let tone = files.join("tone.wav");
    wav(&tone, 0.5);
    let tone = tone.to_str().unwrap();
    ok(&a, &["title", a.dir(), "Night Drive", "--as", "Claude"]);
    let step = step(&a);
    let graph = &a.running.state.graph;
    graph.compare(a.session, step, true, Some("w1".into())).unwrap();
    let before = assets(&a);
    let o = a.m2_with(&["import", a.dir(), tone, "--as", "Claude"], &[("M2_BUSY_WAIT", "1")]);
    assert_eq!(o.status.code(), Some(3), "{}", said(&o));
    assert!(said(&o).contains("comparing"), "{}", said(&o));
    assert_eq!(assets(&a), before, "nothing was copied into the song");
    let t = Instant::now();
    while t.elapsed() < Duration::from_millis(2500) {
        assert_eq!(graph.comparing()["step"], json!(step), "the pill stays open");
        std::thread::sleep(Duration::from_millis(50));
    }
    graph.close_compare(a.session, false).unwrap();
    let out = ok(&a, &["import", a.dir(), tone, "--as", "Claude"]);
    assert!(out.contains("imported tone.wav as assets/tone.wav"), "{out}");
    assert!(out.contains("onto a new track Tone"), "{out}");
    assert!(out.contains("in the open app: one undo step, Claude: importing tone.wav"), "{out}");
    let top = a.top();
    assert_eq!((&top["by"], &top["name"]), (&json!("Claude"), &json!("importing tone.wav")), "{top}");
    assert_eq!(assets(&a), ["tone.wav"]);
}
