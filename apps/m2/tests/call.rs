//! `m2 routes` and `m2 call` (AGENT-8, AGENT-11; track 4 of the agents design): the app's one
//! route table, printed with no app running, and any route for agents sent to the app that has the
//! song open, as one undo step of the agent's: song changes with no command of their own among
//! them. The musician's controls are refused before anything is sent. This process stands for
//! the app, as in `forward.rs`.

mod app;

use app::{App, Temp, m2_in, said};
use serde_json::{Value, json};

/// The keys track of first-song, through a delay: an effect to turn off.
const KEYS: &str = "import { track } from \"@m2/core\";\nimport { pluck } from \"std/instruments\";\nimport { delay } from \"std/fx\";\n\n\
                    export default track(\"keys\", { name: \"Keys\" }, ({ lane }) => delay(pluck(lane.notes(\"chords\")), { mix: 0.2 }));\n";

/// What `m2` printed on its standard output, as JSON.
fn answer(o: &std::process::Output) -> Value {
    serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("not JSON ({e}): {}", said(o)))
}

// AGENT-8: `m2 routes --json` prints the whole table, with no app running: each route's method,
// path, whether it writes, whether agents may call it, its words and an example body. `m2 routes`
// says every route in words.
#[test]
fn m2_routes_lists_every_route() {
    let home = Temp::new("routes-home");
    let o = m2_in(&home.0, &["routes", "--json"], &[]);
    assert!(o.status.success(), "{}", said(&o));
    let listed: Vec<Value> = serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{e}: {}", said(&o)));
    let table = m2_server::api::all();
    assert!(table.len() > 100, "every route: {}", table.len());
    assert_eq!(listed, table.iter().map(|r| r.json()).collect::<Vec<_>>());
    let route = |method: &str, path: &str| listed.iter().find(|r| r["method"] == method && r["path"] == path).cloned().unwrap_or_default();
    let bypass = route("POST", "/api/graph/effect/bypass");
    assert_eq!((&bypass["writes"], &bypass["agents"]), (&json!(true), &json!(true)), "{bypass}");
    assert!(bypass["words"].as_str().is_some_and(|w| !w.is_empty()) && bypass["example"]["node"].is_string(), "{bypass}");
    let play = route("POST", "/api/graph/transport");
    assert_eq!((&play["writes"], &play["agents"]), (&json!(false), &json!(false)), "the musician's: {play}");
    let o = m2_in(&home.0, &["routes"], &[]);
    assert!(o.status.success(), "{}", said(&o));
    let text = String::from_utf8_lossy(&o.stdout);
    for r in &table {
        assert!(text.contains(&format!("{} {}", r.method, r.path)), "{} {} isn't listed:\n{text}", r.method, r.path);
    }
    let (agents, musicians) = (text.find("POST /api/graph/effect/bypass").unwrap(), text.find("POST /api/graph/transport").unwrap());
    assert!(agents < musicians, "the routes for agents first:\n{text}");
}

// AGENT-8: an action that has no `m2` command of its own, an effect's bypass, reached with `m2
// call`: the song's session is filled in, the app writes it as one undo step of the agent's, and
// `m2` prints the app's answer.
#[test]
fn m2_call_reaches_an_action_with_no_command() {
    let a = App::open("first-song", |song| std::fs::write(song.join("tracks/keys.ts"), KEYS).unwrap());
    let state = a.running.state.graph.state().unwrap();
    let revision = state["view"]["revision"].as_u64().unwrap();
    let delay = state["view"]["modules"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|m| m["nodes"].as_array().cloned().unwrap_or_default())
        .find(|n| n["component"] == "std:delay")
        .unwrap();
    let body = json!({"revision": revision, "node": delay["id"], "on": true}).to_string();
    let o = a.m2(&["call", a.dir(), "/api/graph/effect/bypass", &body, "--as", "Claude"]);
    assert!(o.status.success(), "{}", said(&o));
    assert_eq!(answer(&o)["file"], json!("tracks/keys.ts"), "{}", said(&o));
    assert!(a.read("tracks/keys.ts").contains("bypass: true"), "{}", a.read("tracks/keys.ts"));
    let top = a.top();
    assert_eq!((&top["source"], &top["by"]), (&json!("agent"), &json!("Claude")), "{top}");
}

// AGENT-11: `m2 call` refuses the musician's controls before it sends anything, with the words
// the app would answer: nothing plays, with the app open or not. A path the table hasn't got is
// refused too.
#[test]
fn m2_call_refuses_the_musicians_controls() {
    let a = App::open("first-song", |_| {});
    let o = a.m2(&["call", a.dir(), "/api/graph/transport", r#"{"action": "play"}"#]);
    assert_eq!(o.status.code(), Some(1), "{}", said(&o));
    let words = m2_server::api::find("POST", "/api/graph/transport").unwrap().refusal();
    assert!(said(&o).contains(&words), "{}", said(&o));
    assert_eq!(a.running.state.graph.state().unwrap()["playing"], json!(false), "nothing plays");
    // Before sending: with no app to reach, the same words.
    let home = Temp::new("call-no-app");
    let o = m2_in(&home.0, &["call", a.dir(), "/api/graph/seek", r#"{"beat": 8}"#], &[]);
    assert_eq!(o.status.code(), Some(1), "{}", said(&o));
    assert!(said(&o).contains(&m2_server::api::find("POST", "/api/graph/seek").unwrap().refusal()), "{}", said(&o));
    let o = a.m2(&["call", a.dir(), "/api/graph/nope", "{}"]);
    assert_eq!(o.status.code(), Some(1), "{}", said(&o));
    assert!(said(&o).contains("m2 routes"), "{}", said(&o));
}

// AGENT-8, AGENT-10: `--as` and `--why` name the step `m2 call` makes, in the `x-m2-as` and
// `x-m2-why` headers, percent-encoded, so a name that isn't plain ASCII is kept whole; `m2` says
// it is one undo step in the open app.
#[test]
fn m2_call_names_its_step() {
    let a = App::open("first-song", |_| {});
    let body = r#"{"markers": [{"beat": 16, "name": "Drop"}]}"#;
    let o = a.m2(&["call", a.dir(), "/api/graph/markers", body, "--as", "Clåude", "--why", "mark the drop"]);
    assert!(o.status.success(), "{}", said(&o));
    assert!(a.read("layout.json").contains("Drop"), "{}", a.read("layout.json"));
    let top = a.top();
    assert_eq!((&top["source"], &top["by"], &top["name"]), (&json!("agent"), &json!("Clåude"), &json!("mark the drop")), "{top}");
    assert!(String::from_utf8_lossy(&o.stderr).contains("in the open app: one undo step, Clåude: mark the drop"), "{}", said(&o));
}

// AGENT-8: a path with two methods takes the one `--method` names, and none without it; a GET's
// keys go in the query, with the song's session.
#[test]
fn m2_call_method_picks_one_of_two() {
    let a = App::open("first-song", |_| {});
    let o = a.m2(&["call", a.dir(), "/api/settings"]);
    assert_eq!(o.status.code(), Some(1), "{}", said(&o));
    assert!(said(&o).contains("GET or POST") && said(&o).contains("--method"), "{}", said(&o));
    let o = a.m2(&["call", a.dir(), "/api/settings", "--method", "GET"]);
    assert!(o.status.success(), "{}", said(&o));
    assert!(answer(&o)["outputs"].is_array(), "{}", said(&o));
    let o = a.m2(&["call", a.dir(), "/api/settings", r#"{"record": {"count_in": false}}"#, "--method", "post"]);
    assert_eq!(o.status.code(), Some(1), "{}", said(&o));
    assert!(said(&o).contains(&m2_server::api::find("POST", "/api/settings").unwrap().refusal()), "{}", said(&o));
    let o = a.m2(&["call", a.dir(), "/api/graph/presets", r#"{"node": "tracks/keys.ts#1"}"#]);
    assert!(o.status.success(), "{}", said(&o));
    assert!(answer(&o)["std"].is_array(), "{}", said(&o));
}

// AGENT-3, AGENT-8: a call the musician holds what it changes for is busy, as every `m2` write is:
// nothing changes, `m2` exits 3 and says not to write the file directly instead.
#[test]
fn m2_call_is_busy_as_every_write_is() {
    let a = App::open("first-song", |_| {});
    a.running.state.graph.set_unsaved(a.session, vec!["tracks/keys.ts".into()]).unwrap();
    let before = a.read("tracks/keys.ts");
    let body = r#"{"module": "keys", "label": "Piano"}"#;
    let o = a.m2_with(&["call", a.dir(), "/api/graph/label", body], &[("M2_BUSY_WAIT", "0")]);
    assert_eq!(o.status.code(), Some(3), "{}", said(&o));
    assert!(said(&o).contains("unsaved") && said(&o).contains("Don't write the file directly"), "{}", said(&o));
    assert_eq!(a.read("tracks/keys.ts"), before);
}
