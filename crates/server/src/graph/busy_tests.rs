//! Agents beside the musician on the server (AGENT-3, AGENT-4, AGENT-10; track 2 of the agents
//! design): the tools key makes a request an agent's; an agent's write is refused while the
//! musician holds what it touches, on arrival, and taken back at its settle when the musician
//! began to hold it meanwhile; it is never heard before that settle; its step is its own, judged
//! when undone, and `m2 undo` takes back only an agent's own.

use std::sync::mpsc;

use m2_song::tx::{Anchor, Base, Confined, Edit, Expect, Op, Pos, Range, Tx, TxPath};
use serde_json::{Value, json};

use super::super::steps::tests::{first_song, first_song_files, heard, set, steps_of, token, until, wav};
use super::super::tests::{kick_clips, knob};
use super::*;
use crate::caller::{Caller, as_caller};
use crate::graph::{Graph, Inner, Owner};

/// `f` as the agent Claude would send it, saying why.
fn agent<T>(f: impl FnOnce() -> T) -> T {
    as_caller(Caller::agent(Some("Claude"), Some("brighter hats")), f)
}

/// A `Values` Tx writing `new` over the literal `old` that `context` holds in `file`, based on the
/// file as it is now.
fn values_tx(f: &super::super::tests::Fixture, file: &str, context: &str, old: &str, new: &str) -> Value {
    let text = f.read(file);
    let at = text.find(context).unwrap_or_else(|| panic!("no `{context}`")) + context.find(old).unwrap();
    let (start, end) = (at as u32, (at + old.len()) as u32);
    let path = TxPath::new(file).unwrap();
    let op = Op::TextEdit {
        file: path.clone(),
        edits: vec![Edit { start: Pos::Byte(start), end: Pos::Byte(end), expected: old.into(), text: new.into() }],
    };
    let expect = Expect::Values(vec![(Anchor::Written { file: path.clone(), range: Range::bytes(start, end) }, new.parse().unwrap())]);
    let mut tx = Tx::new("tx-agent", (old.into(), new.into()), vec![op], expect);
    tx.base.insert(path, Base::text(&text));
    tx.to_json()
}

/// A `Clips` Tx putting one note clip on the bass's notes lane, confined to it.
fn notes_tx(f: &super::super::tests::Fixture, id: &str) -> Value {
    let lane = m2_song::LaneAddr::new("bass", "notes").unwrap();
    let put = json!({"id": id, "start": 0, "length": 4, "notes": [{"pitch": "E1", "start": 0, "length": 0.25}]});
    let mut tx = Tx::new(
        "tx-agent-notes",
        ("before".into(), "after".into()),
        vec![Op::Clips { lane: lane.clone(), put: vec![put], delete: vec![] }],
        Expect::Confined(Confined { lanes: vec![lane.clone()], ..Confined::default() }),
    );
    tx.base.insert(TxPath::lane(&lane), Base::text(&f.read("data/bass/notes.json")));
    tx.to_json()
}

/// A server on first-song's files with both keys: the page's `t0k`, and the tools key `t00ls`.
fn serving() -> (super::super::tests::Fixture, crate::Running, u64) {
    let fixture = first_song_files();
    let options = crate::ServeOptions { port: 0, web_dir: fixture.0.join("no-web"), token: "t00ls".into(), page_token: "t0k".into() };
    let running = crate::start_with(options, Graph::silent()).unwrap();
    let session = running.state.graph.open(&fixture.0).unwrap()["session"].as_u64().unwrap();
    (fixture, running, session)
}

/// `POST path` with `key`, and `headers`: the status and the JSON answer.
fn post(url: &str, path: &str, key: &str, headers: &[(&str, &str)], body: &Value) -> (u16, Value) {
    use std::io::{Read, Write};
    let addr = url.trim_start_matches("http://");
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    let body = body.to_string();
    let more: String = headers.iter().map(|(k, v)| format!("{k}: {v}\r\n")).collect();
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: {addr}\r\nx-m2-token: {key}\r\n{more}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).unwrap();
    let mut answer = String::new();
    s.read_to_string(&mut answer).unwrap();
    let status = answer[9..12].parse().unwrap();
    (status, answer.split_once("\r\n\r\n").map_or(Value::Null, |(_, b)| serde_json::from_str(b).unwrap_or(Value::Null)))
}

// AGENT-10: a request with the tools key is an agent's, by the name it gives in the body or the
// `x-m2-as` header, else `agent`; its step's source says so, and is named by its words, plain
// text of at most 80 characters. Any other key is refused.
#[test]
fn the_tools_key_makes_a_request_an_agents() {
    let (fixture, running, session) = serving();
    let url = &running.url;
    let graph = &running.state.graph;
    let tx = values_tx(&fixture, "song.ts", "pan: 0.3", "0.3", "0.4");
    let (status, body) =
        post(url, "/api/tx/commit", "t00ls", &[], &json!({"session": session, "tx": tx, "as": "Claude", "why": "brighter hats"}));
    assert_eq!(status, 200, "{body}");
    let top = graph.state().unwrap()["undo_name"].clone();
    assert_eq!(
        (&top["source"], &top["by"], &top["name"], &top["step"]),
        (&json!("agent"), &json!("Claude"), &json!("brighter hats"), &body["step"]),
        "{top}"
    );
    assert_eq!(graph.undo_names().0.as_deref(), Some("Claude: brighter hats"), "the Edit menu's words");
    // By the header, and words cleaned: no control characters, cut to 80 with "…".
    let long = format!("warmer\u{7} {}", "x".repeat(100));
    let tx = values_tx(&fixture, "song.ts", "pan: 0.4", "0.4", "0.45");
    let (status, _) = post(url, "/api/tx/commit", "t00ls", &[("x-m2-as", "Codex")], &json!({"session": session, "tx": tx, "why": long}));
    assert_eq!(status, 200);
    let top = graph.state().unwrap()["undo_name"].clone();
    let name = top["name"].as_str().unwrap();
    assert_eq!(top["by"], json!("Codex"));
    assert!(name.starts_with("warmer x") && name.ends_with('…') && name.chars().count() == 80, "{name}");
    // No name given: `agent`.
    let tx = values_tx(&fixture, "song.ts", "pan: 0.45", "0.45", "0.5");
    assert_eq!(post(url, "/api/tx/commit", "t00ls", &[], &json!({"session": session, "tx": tx})).0, 200);
    let top = graph.state().unwrap()["undo_name"].clone();
    assert_eq!((&top["source"], &top["by"], &top["given"]), (&json!("agent"), &json!("agent"), &json!(false)), "{top}");
    let (status, _) = post(url, "/api/graph/undo", "nope", &[], &json!({"session": session}));
    assert_eq!(status, 401, "another key");
}

// AGENT-10: the page's key is the musician's, whatever the request says about itself.
#[test]
fn a_write_with_the_page_key_is_the_musicians() {
    let (fixture, running, session) = serving();
    let graph = &running.state.graph;
    let tx = values_tx(&fixture, "song.ts", "pan: 0.3", "0.3", "0.4");
    let body = json!({"session": session, "tx": tx, "as": "Claude", "why": "brighter hats"});
    let (status, _) = post(&running.url, "/api/tx/commit", "t0k", &[("x-m2-as", "Claude")], &body);
    assert_eq!(status, 200);
    let top = graph.state().unwrap()["undo_name"].clone();
    assert_eq!((&top["source"], &top["by"]), (&json!("page"), &Value::Null), "{top}");
    // The page's write during its own drag of the same file goes ahead.
    let (t, revision, id) = knob(graph, -0.25);
    let lease = graph.begin(t, revision, &id, Owner::default()).unwrap().lease;
    let tx = values_tx(&fixture, "song.ts", "pan: 0.4", "0.4", "0.45");
    assert_eq!(post(&running.url, "/api/tx/commit", "t0k", &[], &json!({"session": session, "tx": tx})).0, 200);
    graph.cancel(lease).unwrap();
    // Agents never hold leases (AGENT-3).
    let (t, revision, id) = knob(graph, 0.45);
    let e = agent(|| graph.begin(t, revision, &id, Owner::default())).unwrap_err();
    assert!(e.to_string().contains("agents don't drag"), "{e:#}");
}

// AGENT-1: a forwarded Tx is committed only while its delta has the hash its sender checked:
// another hash is refused, 422, and nothing changes.
#[test]
fn a_forwarded_tx_whose_delta_isnt_seen_is_refused() {
    let (fixture, graph) = first_song();
    let tx = values_tx(&fixture, "song.ts", "pan: 0.3", "0.3", "0.4");
    let (status, checked) = graph.tx_check(Some(token(&graph)), &tx).unwrap();
    assert_eq!(status, 200);
    let hash = checked["checked"]["hash"].as_str().unwrap().to_string();
    let song = fixture.read("song.ts");
    let other = format!("sha256:{}", "0".repeat(64));
    let (status, body) = agent(|| graph.tx_commit(Some(token(&graph)), &tx, Some(&other), &[])).unwrap();
    assert_eq!(status, 422, "{body}");
    assert!(body.to_string().contains("changed while"), "{body}");
    assert_eq!(fixture.read("song.ts"), song, "nothing changed");
    assert_eq!(graph.state().unwrap()["can_undo"], json!(false), "no step");
    let (status, body) = agent(|| graph.tx_commit(Some(token(&graph)), &tx, Some(&hash), &[])).unwrap();
    assert_eq!((status, &body["committed"]["hash"]), (200, &json!(hash)), "{body}");
}

// AGENT-1, PLANE-5: an agent's step keeps its Tx's own expectation, so the page's undo of it is
// judged, and heard as its preview; one committed on a seen delta alone (`m2 tx --seen`) is raw.
#[test]
fn an_agents_step_is_judged_when_undone() {
    let (fixture, graph) = first_song();
    // Forwarded as `m2` forwards it: its own expectation, and the hash of its check beside it.
    let tx = notes_tx(&fixture, "agent1");
    let (_, checked) = graph.tx_check(Some(token(&graph)), &tx).unwrap();
    let hash = checked["checked"]["hash"].as_str().unwrap().to_string();
    let (status, _) = agent(|| graph.tx_commit(Some(token(&graph)), &tx, Some(&hash), &[])).unwrap();
    assert_eq!(status, 200);
    graph.undo(token(&graph)).unwrap();
    assert!(graph.last_edit().unwrap().previewed, "judged, so heard as its preview");
    // On a seen delta alone: raw, taken in as an outside write is when undone.
    let mut seen = notes_tx(&fixture, "agent2");
    let (_, checked) = graph.tx_check(Some(token(&graph)), &seen).unwrap();
    seen["expect"] = json!({"seen": checked["checked"]["hash"]});
    assert_eq!(agent(|| graph.tx_commit(Some(token(&graph)), &seen, None, &[])).unwrap().0, 200);
    graph.undo(token(&graph)).unwrap();
    assert!(!graph.last_edit().unwrap().previewed, "raw");
}

/// Run `f` on the actor once an agent's write arrived and passed its first check, before it is
/// planned: the actor waits for it.
fn after_arrival(graph: &Graph, f: impl FnOnce() + Send + 'static) {
    let (arrived, at) = mpsc::channel::<()>();
    let (done, wait) = mpsc::channel::<()>();
    graph.inspect(move |a| {
        a.after_arrival = Some(Box::new(move |_| {
            let _ = arrived.send(());
            let _ = wait.recv();
        }))
    });
    std::thread::spawn(move || {
        if at.recv().is_ok() {
            f();
            let _ = done.send(());
        }
    });
}

// AGENT-3, WRITE-22: a knob drag the musician begins while an agent's change to the same file
// compiles goes on, and the change is taken back at its settle: 423, `taken_back`; the file is as
// before, no step is recorded or told, and the drag's own commit lands.
#[test]
fn a_drag_begun_while_an_agents_change_compiles_goes_on_and_the_change_is_taken_back() {
    let (fixture, graph) = first_song();
    let graph = std::sync::Arc::new(graph);
    let mut steps = steps_of(&graph);
    let song = fixture.read("song.ts");
    let (t, revision, id) = knob(&graph, 0.3);
    let tx = values_tx(&fixture, "song.ts", "pan: -0.25", "-0.25", "-0.5");
    let (begun, lease) = mpsc::channel();
    let g = graph.clone();
    after_arrival(&graph, move || {
        let l = g.begin(t, revision, &id, Owner::default()).unwrap().lease;
        g.update(l, Some(1), 0.35).unwrap();
        begun.send(l).unwrap();
    });
    let (status, body) = agent(|| graph.tx_commit(Some(t), &tx, None, &[])).unwrap();
    let lease = lease.recv_timeout(std::time::Duration::from_secs(10)).expect("the drag began as the change arrived");
    assert_eq!((status, &body["busy"]["what"], &body["busy"]["taken_back"]), (423, &json!("knob"), &json!(true)), "{body}");
    assert!(body["busy"]["words"].as_str().unwrap().contains("dragging"), "{body}");
    assert_eq!(fixture.read("song.ts"), song, "taken back");
    assert_eq!(graph.state().unwrap()["can_undo"], json!(false), "no step");
    assert!(graph.leases().iter().any(|l| l.lease == lease), "the drag goes on");
    graph.update(lease, Some(2), 0.36).unwrap();
    graph.inspect(|a| a.after_arrival = None);
    until(&graph, 300, |_| false);
    assert!(heard(&mut steps).is_empty(), "no step is told");
    graph.commit(lease, 0.36, None).unwrap();
    assert!(fixture.read("song.ts").contains("pan: 0.36"), "the drag's commit lands");
}

// AGENT-3: unsaved text the code editor posts while an agent's change to that file lands takes the
// change back.
#[test]
fn unsaved_text_posted_while_an_agents_change_lands_takes_it_back() {
    let (fixture, graph) = first_song();
    let graph = std::sync::Arc::new(graph);
    let song = fixture.read("song.ts");
    let t = token(&graph);
    let tx = values_tx(&fixture, "song.ts", "pan: 0.3", "0.3", "0.4");
    let g = graph.clone();
    after_arrival(&graph, move || g.set_unsaved(t, vec!["song.ts".into()]).unwrap());
    let (status, body) = agent(|| graph.tx_commit(Some(t), &tx, None, &[])).unwrap();
    assert_eq!((status, &body["busy"]["what"], &body["busy"]["taken_back"]), (423, &json!("code"), &json!(true)), "{body}");
    assert_eq!(body["busy"]["words"], json!("song.ts has unsaved changes in the app's code editor"), "{body}");
    assert_eq!(fixture.read("song.ts"), song);
    graph.inspect(|a| a.after_arrival = None);
    // Posted before it arrives: refused at once, nothing planned.
    let (status, body) = agent(|| graph.tx_commit(Some(t), &tx, None, &[])).unwrap();
    assert_eq!((status, &body["busy"]["taken_back"]), (423, &json!(false)), "{body}");
    // Another file is free.
    let other = notes_tx(&fixture, "agent1");
    assert_eq!(agent(|| graph.tx_commit(Some(t), &other, None, &[])).unwrap().0, 200);
}

// AGENT-3, PLANE-5: an agent's data change is never previewed: the engine hears it at its settle,
// after the check there. The page's own is heard as its preview.
#[test]
fn an_agents_change_is_not_heard_before_its_settle() {
    let (_fixture, graph) = first_song();
    let t = token(&graph);
    let revision = |g: &Graph| g.state().unwrap()["view"]["revision"].as_u64().unwrap();
    let before = revision(&graph);
    let (tell, playing) = mpsc::channel();
    graph.inspect(move |a| {
        a.at_landing = Some(Box::new(move |inner: &Inner| {
            let _ = tell.send(inner.control.lock().open.as_ref().map(|p| p.engine.revision));
        }))
    });
    agent(|| graph.write_notes(t, before, "bass", "notes", &kick_clips(&["E1"]))).unwrap();
    let at = playing.recv_timeout(std::time::Duration::from_secs(10)).ok().flatten();
    assert_eq!(at, Some(before), "at its settle's check, the engine still had the revision before");
    assert!(!graph.last_edit().unwrap().previewed, "never heard as a preview");
    graph.inspect(|a| a.at_landing = None);
    graph.write_notes(t, revision(&graph), "bass", "notes", &kick_clips(&["F1"])).unwrap();
    assert!(graph.last_edit().unwrap().previewed, "the page's is");
}

// AGENT-3, AGENT-4: an agent's undo is checked as its writes are: refused while the musician drags
// a knob in the file it puts back, then taken back once the drag ended. It takes back only an
// agent's own newest step, of the same name.
#[test]
fn an_agents_undo_is_checked_like_its_writes() {
    let (fixture, graph) = first_song();
    let t = token(&graph);
    let tx = values_tx(&fixture, "song.ts", "pan: 0.3", "0.3", "0.4");
    assert_eq!(agent(|| graph.tx_commit(Some(t), &tx, None, &[])).unwrap().0, 200);
    let (_, revision, id) = knob(&graph, -0.25);
    let lease = graph.begin(t, revision, &id, Owner::default()).unwrap().lease;
    let e = agent(|| graph.undo(t)).unwrap_err();
    let busy = e.downcast_ref::<Busy>().unwrap_or_else(|| panic!("busy: {e:#}"));
    assert_eq!((busy.what, busy.taken_back), ("knob", false));
    assert!(fixture.read("song.ts").contains("pan: 0.4"), "nothing was undone");
    graph.cancel(lease).unwrap();
    // Another agent's name: not its step.
    let e = as_caller(Caller::agent(Some("Codex"), None), || graph.undo(t)).unwrap_err();
    assert!(e.to_string().contains("not Codex's"), "{e:#}");
    agent(|| graph.undo(t)).unwrap();
    assert!(fixture.read("song.ts").contains("pan: 0.3"), "undone");
    // The musician's own step is never an agent's to take back.
    set(&graph, 0.3, 0.35);
    let e = agent(|| graph.undo(t)).unwrap_err();
    assert!(e.to_string().contains("the newest change is yours"), "{e:#}");
    assert!(fixture.read("song.ts").contains("pan: 0.35"));
}

// AGENT-1, AGENT-2: a forwarded import names its new asset, which the app takes in before it
// commits: the clip plays it from the first block, with no `asset.missing`, and there is one step,
// the agent's, with no outside step after the watcher looked.
#[test]
fn a_forwarded_import_plays_its_asset_from_the_start_and_makes_one_step() {
    let fixture = super::super::tests::Fixture::new();
    fixture.put("song.ts", "import { song, track } from '@m2/core';\nconst chops = track('chops', ({ lane }) => lane.audio('clips'));\nexport default song(() => chops.out);\n");
    fixture.put("data/chops/clips.json", "{\"clips\": []}\n");
    let graph = fixture.open();
    let t = token(&graph);
    let mut steps = steps_of(&graph);
    until(&graph, 400, |_| false);
    // What `m2 import` does first: the file, made create-only under assets/.
    std::fs::create_dir_all(fixture.0.join("assets")).unwrap();
    let bytes = wav(4_800);
    std::fs::write(fixture.0.join("assets/tone.wav"), &bytes).unwrap();
    let hash = m2_song::Sha::of(&bytes).pin();
    let lane = m2_song::LaneAddr::new("chops", "clips").unwrap();
    let put = json!({"id": "tone1", "start": 0, "length": 1, "asset": "tone.wav", "hash": hash, "offset": 0, "gain_db": 0,
                     "fade_in": 0, "fade_out": 0, "reverse": false, "pitch": 0, "warp": []});
    let mut tx = Tx::new(
        "tx-agent-import",
        ("before".into(), "after".into()),
        vec![Op::Clips { lane: lane.clone(), put: vec![put], delete: vec![] }],
        Expect::Confined(Confined { lanes: vec![lane.clone()], ..Confined::default() }),
    );
    tx.base.insert(TxPath::lane(&lane), Base::text(&fixture.read("data/chops/clips.json")));
    let (status, body) = agent(|| graph.tx_commit(Some(t), &tx.to_json(), None, &["assets/tone.wav".into()])).unwrap();
    assert_eq!(status, 200, "{body}");
    let state = graph.state().unwrap();
    assert!(!state.to_string().contains("asset.missing"), "plays it from the start: {}", state["problems"]);
    until(&graph, 1500, |_| false);
    let told = heard(&mut steps);
    assert_eq!(told.len(), 1, "one step: {told:?}");
    assert_eq!((&told[0]["source"], &told[0]["by"]), (&json!("agent"), &json!("Claude")));
    // An asset path that isn't one is refused.
    let (status, _) = agent(|| graph.tx_commit(Some(t), &tx.to_json(), None, &["song.ts".into()])).unwrap();
    assert_eq!(status, 400);
}
