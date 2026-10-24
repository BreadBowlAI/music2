//! What an agent sees beside the musician (AGENT-6; track 3 of the agents design): `m2 focus` says
//! what the page looks at, `m2 watch` follows every event as one JSON line, until the app quits,
//! and `m2 point` shows a note at a track, a clip, a lane, a knob or bars. None of them writes.
//! This process stands for the app, as in `forward.rs`.

mod app;

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use app::{App, Temp, copy, example, fake, publish, said};
use serde_json::{Value, json};

/// `m2 watch <song> args…` running in `home`, and its lines as they come.
fn watch(home: &std::path::Path, song: &str, args: &[&str]) -> (Child, mpsc::Receiver<String>) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_m2"))
        .arg("watch")
        .arg(song)
        .args(args)
        .env("HOME", home)
        .env("M2_AUDIO", "none")
        .env("M2_MIDI", "none")
        .env_remove("M2_AS")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let out = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                return;
            }
        }
    });
    (child, rx)
}

/// Lines from `rx`, until `done` holds of them, or `secs` pass.
fn lines_until(rx: &mpsc::Receiver<String>, secs: u64, mut done: impl FnMut(&[String]) -> bool) -> Vec<String> {
    let (mut got, t) = (vec![], Instant::now());
    while !done(&got) && t.elapsed() < Duration::from_secs(secs) {
        if let Ok(line) = rx.recv_timeout(Duration::from_millis(100)) {
            got.push(line);
        }
    }
    got
}

fn parsed(lines: &[String]) -> Vec<Value> {
    lines.iter().map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("not JSON ({e}): {l}"))).collect()
}

// AGENT-6: `m2 watch` prints one JSON object a line, `t` first, for each event the page hears: a
// step (an agent's, by its name), the musician's focus, a toast, and the transport, which it reads
// itself. Meters only with --meters.
#[test]
fn m2_watch_prints_one_json_line_an_event() {
    let a = App::open("first-song", |_| {});
    let (mut child, rx) = watch(&a.home, a.dir(), &[]);
    // Its first line: the transport as it is, so the socket is open by then.
    let first = lines_until(&rx, 20, |l| !l.is_empty());
    assert!(first.first().is_some_and(|l| l.starts_with(r#"{"t":"transport""#)), "{first:?}");
    std::thread::sleep(Duration::from_millis(300));
    let focus = json!({"session": a.session, "view": "mix", "track": "bass"});
    a.running.state.graph.post_focus(serde_json::from_value(focus).unwrap()).unwrap();
    let o = a.m2(&["set", a.dir(), "song.ts#2/0.hats.pan", "0.4", "--as", "Claude", "--why", "wider hats"]);
    assert!(o.status.success(), "{}", said(&o));
    a.running.state.toast("hello from the app");
    a.running.state.graph.transport("play").unwrap();
    let has = |l: &[String], t: &str| l.iter().any(|l| l.starts_with(&format!(r#"{{"t":"{t}""#)));
    let lines = lines_until(&rx, 20, |l| {
        has(l, "focus")
            && has(l, "step")
            && has(l, "toast")
            && l.iter().any(|l| l.contains(r#""t":"transport""#) && l.contains(r#""playing":true"#))
    });
    let _ = child.kill();
    let events = parsed(&lines);
    for l in &lines {
        assert!(l.starts_with(r#"{"t":""#) && !l.contains('\n'), "one object a line, t first: {l}");
    }
    let of = |t: &str| events.iter().find(|e| e["t"] == t).cloned().unwrap_or_else(|| panic!("no {t} event: {lines:?}"));
    assert_eq!((&of("focus")["focus"]["view"], &of("focus")["focus"]["track"]), (&json!("mix"), &json!("bass")));
    let step = of("step");
    assert_eq!((&step["source"], &step["by"], &step["name"]), (&json!("agent"), &json!("Claude"), &json!("wider hats")), "{step}");
    assert_eq!(of("toast")["text"], json!("hello from the app"));
    assert!(events.iter().any(|e| e["t"] == "transport" && e["playing"] == json!(true)), "{lines:?}");
    assert!(!events.iter().any(|e| e["t"] == "meters"), "no meters without --meters");
}

// AGENT-6: when the app goes away (its socket drops, with no close frame, as when it quits), `m2
// watch` prints what it heard, then `closed`, and exits 0.
#[test]
fn m2_watch_ends_when_the_app_quits() {
    let temp = Temp::new("watch-quits");
    let song = temp.0.join("song");
    copy(&example("first-song"), &song);
    let held = m2_session::lock::acquire(&song, "music2", m2_session::lock::RETRY).unwrap();
    let state = json!({"session": 1, "dir": song.canonicalize().unwrap(), "problems": []});
    let (url, _) = fake(state, |asked| match asked.1.as_str() {
        "/api/ws" => Some((101, json!({"__ws": [r#"{"t":"toast","text":"bye"}"#]}))),
        "/api/graph/status" => Some((200, json!({"playing": false, "beat": 0.0, "bpm": 120.0, "loop": null}))),
        _ => Some((404, json!({"error": "no such API"}))),
    });
    let home = temp.0.join("home");
    publish(&home, &url, "tools", std::process::id());
    let t = Instant::now();
    let (mut child, rx) = watch(&home, song.to_str().unwrap(), &[]);
    let lines = lines_until(&rx, 20, |l| l.iter().any(|l| l.contains("closed")));
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        assert!(t.elapsed() < Duration::from_secs(20), "it ends: {lines:?}");
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(status.success(), "exit 0: {status:?}");
    let events = parsed(&lines);
    assert!(events.iter().any(|e| e["t"] == "toast" && e["text"] == "bye"), "what it heard first: {lines:?}");
    assert_eq!(events.last().map(|e| e["t"].clone()), Some(json!("closed")), "{lines:?}");
    drop(held);
}

// AGENT-6: `m2 focus` says what the page posted, by key and by note, with where playback is, the
// loop, unsaved code, Compare and the MIDI target. A song the app hasn't open is said in words.
#[test]
fn m2_focus_reads_what_the_page_posted() {
    let a = App::open("first-song", |_| {});
    let focus = json!({
        "session": a.session,
        "view": "notes",
        "track": "bass",
        "clips": [{"track": "bass", "lane": "notes", "id": "db9f9t5s"}],
        "notes": {"clip": {"track": "bass", "lane": "notes", "id": "db9f9t5s"}, "notes": [
            {"pitch": 33, "start": 0.0, "length": 0.5},
            {"pitch": 36, "start": 1.5, "length": 0.5},
        ]},
        "seen": [0.0, 32.0],
    });
    a.running.state.graph.post_focus(serde_json::from_value(focus.clone()).unwrap()).unwrap();
    let o = a.m2(&["focus", a.dir()]);
    assert!(o.status.success(), "{}", said(&o));
    let v: Value = serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("JSON ({e}): {}", said(&o)));
    for k in ["view", "track", "clips", "notes", "seen"] {
        assert_eq!(v[k], focus[k], "{k}: {v}");
    }
    assert_eq!((&v["playing"], &v["unsaved"], &v["comparing"]), (&json!(false), &json!([]), &Value::Null), "{v}");
    assert!(v["beat"].is_number() && v.get("loop").is_some() && v.get("midi").is_some(), "{v}");
    // A song music2 doesn't have open.
    let other = a.temp.0.join("other");
    copy(&example("first-song"), &other);
    let o = a.m2(&["focus", other.to_str().unwrap()]);
    assert!(!o.status.success());
    assert!(said(&o).contains("doesn't have this song open"), "{}", said(&o));
}

// AGENT-6: `m2 point` shows a note at a target the song has, by the agent's name; one it hasn't is
// refused in words; --clear takes it away. It writes nothing.
#[test]
fn m2_point_shows_a_note_and_clears_it() {
    let a = App::open("first-song", |_| {});
    let before = a.read("song.ts");
    let o = a.m2(&["point", a.dir(), "bars:9-16", "listen to the hats here", "--as", "Claude"]);
    assert!(o.status.success(), "{}", said(&o));
    assert!(said(&o).contains("bars 9 to 16"), "{}", said(&o));
    let p = a.running.state.graph.pointer();
    assert_eq!((&p["target"], &p["words"], &p["by"]), (&json!("bars:9-16"), &json!("listen to the hats here"), &json!("Claude")), "{p}");
    let o = a.m2(&["point", a.dir(), "track:nope", "here"]);
    assert!(!o.status.success());
    assert!(said(&o).contains("no track nope"), "{}", said(&o));
    assert_eq!(a.running.state.graph.pointer()["target"], json!("bars:9-16"), "it stays");
    let o = a.m2(&["point", a.dir(), "--clear"]);
    assert!(o.status.success(), "{}", said(&o));
    assert_eq!(a.running.state.graph.pointer(), Value::Null);
    assert_eq!(a.read("song.ts"), before, "nothing written");
}
