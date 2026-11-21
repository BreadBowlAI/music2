//! `m2 versions` (VER-5): a notes clip's versions from the terminal: listed, and each act of the
//! `versions` gesture (VER-3), one undo step each; with the app open, handed to the app as one step
//! by the agent's name (AGENT-1), and busy (status 3) while the musician holds the lane (AGENT-3).

mod app;

use std::io::Write;
use std::process::{Command, Output, Stdio};

use app::{App, Temp, copy, example, said};
use serde_json::{Value, json};

/// The first song's bass clip.
const CLIP: &str = "bass/notes:db9f9t5s";
const BUSY: &str = r#"[{"pitch": "A2", "start": 4, "length": 0.25}, {"pitch": "A1", "start": 0, "length": 0.25, "velocity": 0.5}]"#;

/// Run `m2 args…` in `home`, with `stdin` as its standard input.
fn m2(home: &std::path::Path, args: &[&str], stdin: &str, env: &[(&str, &str)]) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_m2"));
    c.env("HOME", home).env_remove("M2_AS").envs(env.iter().copied()).args(args);
    let mut child = c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    child.wait_with_output().unwrap()
}

// VER-5: with the app closed, every act writes the lane's file as one step that `m2 undo` takes
// back; `list` shows the versions, and `--json` their notes; a refusal changes nothing.
#[test]
fn the_acts_with_the_app_closed() {
    let temp = Temp::new("versions-cli");
    let song = temp.0.join("song");
    copy(&example("first-song"), &song);
    let (home, dir) = (m2_song::pack::test_home(), song.to_str().unwrap().to_string());
    let read = || std::fs::read_to_string(song.join("data/bass/notes.json")).unwrap();
    let before = read();
    let run = |args: &[&str], stdin: &str| m2(&home, &[&["versions", &dir][..], args].concat(), stdin, &[]);
    let ok = |args: &[&str], stdin: &str| {
        let o = run(args, stdin);
        assert!(o.status.success(), "{args:?}: {}", said(&o));
        said(&o)
    };
    // What `--json` prints, alone on standard output.
    let json = |args: &[&str]| -> Value { serde_json::from_slice(&run(args, "").stdout).unwrap_or_else(|e| panic!("{args:?}: {e}")) };

    assert!(ok(&["list"], "").contains("No clip has versions"));
    let listed = ok(&["list", CLIP], "");
    assert!(listed.contains("no versions") && listed.contains("m2 versions"), "says how to add one: {listed}");
    let v = json(&["list", CLIP, "--json"]);
    assert_eq!((v["clip"]["id"].as_str(), v["versions"].as_array().map(Vec::len)), (Some("db9f9t5s"), Some(0)), "{v}");
    assert_eq!(v["notes"].as_array().unwrap().len(), 48, "what plays: {v}");

    // Add from standard input; the name `--as` gives is who added it.
    let out = m2(&home, &["versions", &dir, "add", CLIP, "busy", "-", "--as", "Claude"], BUSY, &[]);
    assert!(out.status.success() && said(&out).contains("no versions → 2 versions, `original` plays"), "{}", said(&out));
    assert!(read().contains("{\"name\": \"busy\", \"by\": \"Claude\", \"notes\": ["), "{}", read());
    // From a file, and by the clip's place.
    let file = temp.0.join("sparse.json");
    std::fs::write(&file, r#"[{"pitch": 33, "start": 0, "length": 4}]"#).unwrap();
    ok(&["add", "bass/notes@1", "sparse", file.to_str().unwrap()], "");

    let all = ok(&["list"], "");
    assert!(all.contains(CLIP) && all.contains("3 versions, `original` plays"), "{all}");
    ok(&["star", CLIP, "busy"], "");
    let listed = ok(&["list", CLIP], "");
    let line = |name: &str| listed.lines().find(|l| l.contains(name)).unwrap_or_else(|| panic!("{listed}")).to_string();
    assert!(line("original").contains("plays") && line("busy").contains("waits") && line("busy").contains("by Claude"), "{listed}");
    assert!(line("busy").trim_start().starts_with('*') && !line("sparse").contains('*'), "the star: {listed}");
    let v = json(&["list", CLIP, "--json"]);
    let names: Vec<&str> = v["versions"].as_array().unwrap().iter().map(|x| x["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["original", "busy", "sparse"]);
    assert_eq!(
        v["versions"][1],
        json!({"name": "busy", "plays": false, "star": true, "by": "Claude", "notes": [
            {"pitch": "A1", "start": 0.0, "length": 0.25, "velocity": 0.5}, {"pitch": "A2", "start": 4.0, "length": 0.25, "velocity": 0.8}]})
    );
    assert_eq!(
        (&v["versions"][0]["plays"], &v["versions"][0]["by"], &v["versions"][0]["notes"]),
        (&json!(true), &Value::Null, &v["notes"])
    );
    let v = json(&["list", "--json"]);
    assert_eq!(
        (v["clips"][0]["clip"].as_str(), v["clips"][0]["plays"].as_str(), v["clips"][0]["versions"].as_array().map(Vec::len)),
        (Some(CLIP), Some("original"), Some(3)),
        "{v}"
    );

    assert!(ok(&["unstar", CLIP, "busy"], "").contains("`busy`, starred → `busy`, no star"));
    assert!(ok(&["unstar", CLIP, "busy"], "").contains("nothing to write"));
    assert!(ok(&["rename", CLIP, "busy", "busier"], "").contains("version `busy` → version `busier`"));
    assert!(ok(&["play", CLIP, "busier"], "").contains("`busier` plays"));
    assert!(read().contains("{\"name\": \"original\", \"notes\": ["), "the first notes wait: {}", read());

    // Refused in words, nothing written.
    let at = read();
    for (args, stdin, says) in [
        (&["play", CLIP, "nope"][..], "", "no version `nope`. It has `original`, `busier` and `sparse`"),
        (&["remove", CLIP, "busier"], "", "`busier` is the version that plays"),
        (&["add", CLIP, "sparse", "-"], "[]", "has a version `sparse` already"),
        (&["add", CLIP, "odd", "-"], "{\"notes\": 1}", "the notes don't read"),
        (&["list", "bass/cutoff@1"], "", "Only a notes clip has versions"),
        (&["list", "bass/notes:nope"], "", "nope"),
    ] {
        let o = run(args, stdin);
        assert!(!o.status.success() && said(&o).contains(says), "{args:?}: {}", said(&o));
    }
    assert_eq!(read(), at);

    ok(&["remove", CLIP, "sparse"], "");
    let o = m2(&home, &["undo", &dir], "", &[]);
    assert!(o.status.success(), "{}", said(&o));
    assert_eq!(read(), at, "one step, taken back");
    ok(&["play", CLIP, "original"], "");
    assert!(ok(&["keep", CLIP], "").contains("no versions"));
    assert_eq!(read(), before, "kept: today's clip");
}

// VER-5, AGENT-1: with the app open, an add is handed to the app and lands as one undo step by
// the agent's name, which the version carries too. AGENT-3: while the musician drags a clip of the
// lane, it is busy: status 3, and nothing changes.
#[test]
fn add_with_the_app_open_is_one_step_by_the_agent() {
    let a = App::open("first-song", |_| {});
    let graph = &a.running.state.graph;
    let before = a.read("data/bass/notes.json");
    let step = |a: &App| a.top()["step"].as_u64().unwrap_or(0);
    let was = step(&a);
    let args = ["versions", a.dir(), "add", CLIP, "busy", "-", "--as", "Claude", "--why", "a busier bass"];

    // The musician drags the clip: busy.
    let clip: m2_editor::clips::ClipRef = serde_json::from_value(json!({"module": "bass", "lane": "notes", "id": "db9f9t5s"})).unwrap();
    let drag = m2_editor::clips::Gesture::Move { clips: vec![clip], by: 4.0, to: None, copy: false };
    let lease = graph.lane_begin(a.session, drag, None, None).unwrap().expect("a drag begins").lease.expect("previewed");
    let o = m2(&a.home, &args, BUSY, &[("M2_BUSY_WAIT", "1")]);
    assert_eq!(o.status.code(), Some(3), "{}", said(&o));
    assert!(String::from_utf8_lossy(&o.stderr).contains("Don't write the file directly instead"), "{}", said(&o));
    assert_eq!((a.read("data/bass/notes.json"), step(&a)), (before.clone(), was), "nothing landed");
    graph.lane_cancel(lease).unwrap();

    let o = m2(&a.home, &args, BUSY, &[]);
    assert!(o.status.success(), "{}", said(&o));
    assert!(said(&o).contains("in the open app: one undo step, Claude: a busier bass"), "{}", said(&o));
    let top = a.top();
    assert_eq!((&top["source"], &top["by"], &top["name"]), (&json!("agent"), &json!("Claude"), &json!("a busier bass")), "{top}");
    let now = a.read("data/bass/notes.json");
    assert!(now.contains("{\"name\": \"original\"},") && now.contains("{\"name\": \"busy\", \"by\": \"Claude\", \"notes\": ["), "{now}");
    // One step: the app's undo takes the versions away again.
    let o = m2(&a.home, &["undo", a.dir(), "--as", "Claude"], "", &[]);
    assert!(o.status.success(), "{}", said(&o));
    assert_eq!(a.read("data/bass/notes.json"), before);
}
