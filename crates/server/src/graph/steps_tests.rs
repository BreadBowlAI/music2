//! Undo's names on the server (gaps #29, plan C3 as X7 built it): the window's names follow undo
//! and redo, a step made elsewhere is named plainly, and a refused undo keeps them.

use std::path::Path;

use m2_song::tx::{Base, Confined, Expect, Op, TxPath};
use serde_json::json;

use super::super::tests::{Fixture, knob};
use super::*;

/// examples/first-song, open.
fn first_song() -> (Fixture, Graph) {
    let fixture = Fixture::new();
    let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song");
    let mut stack = vec![from.clone()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x != "wav") {
                fixture.put(p.strip_prefix(&from).unwrap().to_str().unwrap(), &std::fs::read_to_string(&p).unwrap());
            }
        }
    }
    let graph = fixture.open();
    (fixture, graph)
}

/// The state's names of Undo's and Redo's steps: their words, or "-" with none.
fn names(graph: &Graph) -> (String, String) {
    let s = graph.state().unwrap();
    let say = |v: &Value| v["name"].as_str().unwrap_or("-").to_string();
    (say(&s["undo_name"]), say(&s["redo_name"]))
}

/// Set the control whose value is `from` to `to`, as a knob's release does; the revision it made.
fn set(graph: &Graph, from: f64, to: f64) -> u64 {
    let (token, revision, id) = knob(graph, from);
    graph.set(token, revision, &id, "set", to, None).unwrap()["revision"].as_u64().unwrap()
}

fn token(graph: &Graph) -> u64 {
    graph.state().unwrap()["session"].as_u64().unwrap()
}

// gaps #29: the name a window gives its step is Undo's, then Redo's once undone, then Undo's
// again; the state and the app's menu say it; undo and redo answer with what they took back.
#[test]
fn undo_names_follow_undo_and_redo() {
    let (_f, graph) = first_song();
    assert_eq!(names(&graph), ("-".into(), "-".into()));
    assert_eq!(graph.undo_names(), (None, None));
    let made = set(&graph, 0.3, 0.4);
    assert_eq!(names(&graph), ("the code in song.ts".into(), "-".into()), "unnamed: by its file");
    assert_eq!(graph.state().unwrap()["undo_name"]["given"], json!(false));
    let named = graph.name_step(token(&graph), made, "Hats pan".into(), false).unwrap();
    assert_eq!(
        (&named["named"], &named["undo_name"]["name"], &named["undo_name"]["given"]),
        (&json!(true), &json!("Hats pan"), &json!(true))
    );
    assert_eq!(names(&graph), ("Hats pan".into(), "-".into()));
    assert_eq!(graph.undo_names(), (Some("Hats pan".into()), None), "the Edit menu's words");
    let undone = graph.undo(token(&graph)).unwrap();
    assert_eq!((&undone["name"], &undone["structural"]), (&json!("Hats pan"), &json!(false)));
    assert_eq!(names(&graph), ("-".into(), "Hats pan".into()));
    assert_eq!(graph.redo(token(&graph)).unwrap()["name"], json!("Hats pan"));
    assert_eq!(names(&graph), ("Hats pan".into(), "-".into()));
    // A revision no step made isn't named, and says so.
    let none = graph.name_step(token(&graph), made + 40, "nothing".into(), false).unwrap();
    assert_eq!(none["named"], json!(false));
    assert!(graph.name_step(token(&graph) + 1, made, "x".into(), false).is_err(), "another session's");
}

// X7: names go by the step, not the Tx id an undo makes anew each time, so a name lasts through
// undo, redo and undo; a structural one says so when it is undone.
#[test]
fn a_named_step_keeps_its_name_through_undo_redo_undo() {
    let (_f, graph) = first_song();
    let hats = set(&graph, 0.3, 0.4);
    let keys = set(&graph, -0.25, -0.5);
    graph.name_step(token(&graph), hats, "Hats pan".into(), false).unwrap();
    graph.name_step(token(&graph), keys, "adding Reverb".into(), true).unwrap();
    let undone = graph.undo(token(&graph)).unwrap();
    assert_eq!((&undone["name"], &undone["structural"]), (&json!("adding Reverb"), &json!(true)));
    assert_eq!(names(&graph), ("Hats pan".into(), "adding Reverb".into()));
    graph.undo(token(&graph)).unwrap();
    assert_eq!(names(&graph), ("-".into(), "Hats pan".into()));
    graph.redo(token(&graph)).unwrap();
    assert_eq!(names(&graph), ("Hats pan".into(), "adding Reverb".into()));
    graph.undo(token(&graph)).unwrap();
    assert_eq!(names(&graph), ("-".into(), "Hats pan".into()));
    graph.redo(token(&graph)).unwrap();
    let again = graph.redo(token(&graph)).unwrap();
    assert_eq!((&again["name"], &again["structural"]), (&json!("adding Reverb"), &json!(true)));
}

// Q19: a step made elsewhere (an agent's Tx) is named plainly by what it changed: Bass's notes,
// the arrangement, a file of code.
#[test]
fn a_step_made_elsewhere_gets_a_plain_name() {
    let (fixture, graph) = first_song();
    let lane = m2_song::LaneAddr::new("bass", "notes").unwrap();
    let put = json!({"id": "agent1", "start": 0, "length": 4, "notes": [{"pitch": "E1", "start": 0, "length": 0.25}]});
    let mut tx = m2_song::Tx::new(
        "tx-agent",
        ("before".into(), "after".into()),
        vec![Op::Clips { lane: lane.clone(), put: vec![put], delete: vec![] }],
        Expect::Confined(Confined { lanes: vec![lane.clone()], ..Confined::default() }),
    );
    tx.base.insert(TxPath::lane(&lane), Base::Sha(m2_song::Sha::of(fixture.read("data/bass/notes.json").as_bytes())));
    let (status, answer) = graph.tx_commit(None, &tx.to_json()).unwrap();
    assert_eq!(status, 200, "{answer}");
    assert_eq!(names(&graph).0, "Bass notes");
    assert_eq!(graph.undo(token(&graph)).unwrap()["name"], json!("Bass notes"));
    assert_eq!(names(&graph).1, "Bass notes");
    // The plain names of other files, by the timeline published.
    let t = graph.inner.control.lock().open.as_ref().unwrap().shown.timeline.clone();
    assert_eq!(plain("data/arrangement.json", &t), "the tempo or time signature");
    assert_eq!(plain("data/bass/cutoff.json", &t), "Bass cutoff automation");
    assert_eq!(plain("tracks/bass.ts", &t), "the code in tracks/bass.ts");
    assert_eq!(plain("layout.json", &t), "the track order, colours or markers");
}

// WRITE-21: an undo refused (what it would take back changed since) keeps both names as they were.
#[test]
fn a_refused_undo_keeps_the_names() {
    let (fixture, graph) = first_song();
    let made = set(&graph, 0.3, 0.4);
    graph.name_step(token(&graph), made, "Hats pan".into(), false).unwrap();
    // Another editor changes the same value meanwhile.
    fixture.put("song.ts", &fixture.read("song.ts").replacen("pan: 0.4", "pan: 0.45", 1));
    graph.poll();
    let refused = graph.undo(token(&graph));
    assert!(refused.is_err(), "{refused:?}");
    assert_eq!(names(&graph), ("Hats pan".into(), "-".into()));
}
