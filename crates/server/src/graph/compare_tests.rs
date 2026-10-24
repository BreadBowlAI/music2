//! The Compare key, V (D2; AGENT-3; track 3 of the agents design and its review): V plays the song
//! before the newest change an agent or another program made, built in memory; V again plays it as
//! it is. Compare never writes the song: its files, undo and the view stay at After, so a quit or a
//! crash on Before leaves the song as it was. Esc keeps what plays, undoing the step on Before;
//! anything else ends it at After. While the pill is open every agent's write is busy, and a V that
//! names a stale step is refused.

use std::sync::Arc;

use serde_json::{Value, json};

use super::super::busy::tests::{agent, notes_tx, values_tx};
use super::super::steps::tests::{first_song, set, steps_of, token, until};

/// Every event heard, as JSON, once `done` holds of them, or after 3 s: undo, redo and compare
/// events follow the step events before them, from the thread that tells those.
fn heard(from: &mut tokio::sync::broadcast::Receiver<String>, mut done: impl FnMut(&[Value]) -> bool) -> Vec<Value> {
    let (mut out, t) = (vec![], std::time::Instant::now());
    loop {
        while let Ok(e) = from.try_recv() {
            out.push(serde_json::from_str(&e).unwrap());
        }
        if done(&out) || t.elapsed() > std::time::Duration::from_secs(3) {
            return out;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// The `compare` events among `events`.
fn compares(events: &[Value]) -> Vec<Value> {
    events.iter().filter(|e| e["t"] == "compare").cloned().collect()
}

/// The `compare` events among `events` that closed the pill: why each ended it.
fn ended(events: &[Value]) -> Vec<String> {
    compares(events).iter().filter(|e| e["open"] == json!(false)).map(|e| e["why"].as_str().unwrap_or("?").to_string()).collect()
}

/// What the engine plays: the revision its renderer was built from, or took since.
fn plays(graph: &super::Graph) -> Arc<m2_session::Revision> {
    graph.inspect(|a| a.open.as_ref().unwrap().plays.as_ref().unwrap().revision.clone())
}

/// Whether the bass notes of `rev` have the clip `id`.
fn holds(rev: &m2_session::Revision, id: &str) -> bool {
    rev.snapshot.files.get(std::path::Path::new("data/bass/notes.json")).is_some_and(|t| t.contains(&format!("\"{id}\"")))
}

/// An agent's step, Claude's: a bass clip `id`, saying `why`. Its id.
fn agent_step(fixture: &super::super::tests::Fixture, graph: &super::Graph, id: &str, why: &str) -> u64 {
    let mut tx = notes_tx(fixture, id);
    tx["id"] = json!(format!("tx-agent-{id}"));
    let (status, body) = crate::caller::as_caller(crate::caller::Caller::agent(Some("Claude"), Some(why)), || {
        graph.tx_commit(Some(token(graph)), &tx, None, &[])
    })
    .unwrap();
    assert_eq!(status, 200, "{body}");
    body["step"].as_u64().unwrap()
}

// AGENT-3, D2 (the review's F3): V plays Before from memory, and the song stays After: its file, undo
// and redo don't change. V again plays After. The app quitting on Before leaves the agent's change in
// the song, and the next launch has it.
#[test]
fn compare_plays_before_from_memory_and_never_writes_the_song() {
    let (fixture, graph) = first_song();
    let t = token(&graph);
    let step = agent_step(&fixture, &graph, "a1", "brighter hats");
    let after = fixture.read("data/bass/notes.json");
    assert!(holds(&plays(&graph), "a1"), "the agent's change plays");
    // V: Before.
    let v = graph.compare(t, step, true, None).unwrap();
    let c = &v["comparing"];
    assert_eq!((&c["step"], &c["after"], &c["name"]), (&json!(step), &json!(false), &json!("Claude: brighter hats")), "{v}");
    assert!(!holds(&plays(&graph), "a1"), "Before plays: the song without the agent's change");
    assert_eq!(fixture.read("data/bass/notes.json"), after, "the file stays as it is");
    let s = graph.state().unwrap();
    assert_eq!((&s["undo_name"]["step"], &s["redo_name"]), (&json!(step), &Value::Null), "undo and redo stay");
    assert_eq!(s["comparing"], graph.comparing(), "the state says what is compared");
    // V again: After.
    let v = graph.compare(t, step, false, None).unwrap();
    assert_eq!(v["comparing"]["after"], json!(true), "{v}");
    assert!(holds(&plays(&graph), "a1"), "After plays the song as it is");
    // Before again, then the app quits.
    graph.compare(t, step, true, None).unwrap();
    assert!(!holds(&plays(&graph), "a1"));
    graph.close();
    drop(graph);
    assert_eq!(fixture.read("data/bass/notes.json"), after, "quit on Before: the song is as it was");
    let again = fixture.open();
    assert_eq!(again.state().unwrap()["comparing"], Value::Null);
    assert!(holds(&plays(&again), "a1"), "and the next launch plays the agent's change");
}

// AGENT-3, D2 (F3): the newest step is an outside one, maybe the musician's own save in another editor.
// V on it never rewrites that file, and a quit on Before leaves it as saved.
#[test]
fn compare_never_rewrites_an_outside_save() {
    let (fixture, graph) = first_song();
    let t = token(&graph);
    until(&graph, 400, |_| false);
    let mine = fixture.read("tracks/bass.ts");
    let saved = mine.replacen("track(", "// my own edit, saved in my editor\ntrack(", 1);
    fixture.put("tracks/bass.ts", &saved);
    until(&graph, 3000, |g| g.state().unwrap()["undo_name"]["source"] == "outside");
    let step = graph.state().unwrap()["undo_name"]["step"].as_u64().unwrap();
    graph.compare(t, step, true, None).unwrap();
    let before = plays(&graph);
    let code = before.snapshot.files.get(std::path::Path::new("tracks/bass.ts")).map(|t| t.to_string());
    assert_eq!(code, Some(mine), "Before plays the code as it was");
    assert_eq!(fixture.read("tracks/bass.ts"), saved, "the saved file stays as saved");
    graph.close();
    drop(graph);
    assert_eq!(fixture.read("tracks/bass.ts"), saved, "quit on Before: still as saved");
}

// AGENT-3, D2 (F3, F5): Esc keeps what plays. On Before the step is undone then, an undo step Redo
// brings back, and the `compare` event says it closed on Before; on After nothing changes.
#[test]
fn esc_on_before_undoes_the_step_and_says_so() {
    let (fixture, graph) = first_song();
    let mut told = steps_of(&graph);
    let t = token(&graph);
    let before = fixture.read("data/bass/notes.json");
    let step = agent_step(&fixture, &graph, "a1", "brighter hats");
    let after = fixture.read("data/bass/notes.json");
    graph.compare(t, step, true, None).unwrap();
    let v = graph.close_compare(t, true).unwrap();
    assert_eq!(v["comparing"], Value::Null, "{v}");
    assert_eq!(fixture.read("data/bass/notes.json"), before, "Esc on Before undoes the agent's change");
    assert_eq!(graph.state().unwrap()["redo_name"]["step"], json!(step), "and Redo has it");
    assert!(!holds(&plays(&graph), "a1"), "what played plays on");
    let events = heard(&mut told, |e| !ended(e).is_empty());
    let undone = events.iter().position(|e| e["t"] == "undo" && e["step"] == json!(step));
    let closed = events.iter().position(|e| e["t"] == "compare" && e["open"] == json!(false));
    assert!(undone.is_some() && undone < closed, "the undo, then the pill closing: {events:?}");
    let closed = &events[closed.unwrap()];
    assert_eq!((&closed["after"], &closed["why"], &closed["step"]), (&json!(false), &json!("closed"), &json!(step)), "{closed}");
    // Redo brings it back; V, V, Esc on After changes nothing.
    graph.redo_step(t, step).unwrap();
    assert_eq!(fixture.read("data/bass/notes.json"), after);
    graph.compare(t, step, true, None).unwrap();
    graph.compare(t, step, false, None).unwrap();
    graph.close_compare(t, true).unwrap();
    assert_eq!(fixture.read("data/bass/notes.json"), after, "Esc on After keeps After");
    assert_eq!(graph.state().unwrap()["redo_name"], Value::Null);
    let closed = compares(&heard(&mut told, |e| ended(e).len() == 1));
    assert_eq!(closed.last().map(|e| (&e["after"], &e["why"])), Some((&json!(true), &json!("closed"))), "{closed:?}");
    assert!(graph.close_compare(t + 1, false).is_err(), "another song's");
}

// AGENT-3, D2: while the pill is open, every agent's write is busy, naming what is compared, and
// changes nothing: an undo, a Tx. Its close lets the agent's write land.
#[test]
fn while_comparing_an_agents_write_is_busy() {
    let (fixture, graph) = first_song();
    let t = token(&graph);
    let step = agent_step(&fixture, &graph, "a1", "brighter hats");
    graph.compare(t, step, true, None).unwrap();
    let pan = values_tx(&fixture, "song.ts", "pan: 0.3", "0.3", "0.4");
    let (status, busy) = agent(|| graph.tx_commit(Some(t), &pan, None, &[])).unwrap();
    assert_eq!(status, 423, "{busy}");
    assert_eq!(
        (&busy["busy"]["what"], &busy["busy"]["words"]),
        (&json!("compare"), &json!("you're comparing Claude: brighter hats")),
        "{busy}"
    );
    let e = agent(|| graph.undo(t)).unwrap_err();
    assert!(e.downcast_ref::<super::super::busy::Busy>().is_some(), "an agent's undo too: {e}");
    assert!(!fixture.read("song.ts").contains("pan: 0.4"), "nothing changed");
    graph.compare(t, step, false, None).unwrap();
    assert_eq!(agent(|| graph.tx_commit(Some(t), &pan, None, &[])).unwrap().0, 423, "still busy on After");
    graph.close_compare(t, false).unwrap();
    assert_eq!(graph.comparing(), Value::Null);
    let (status, body) = agent(|| graph.tx_commit(Some(t), &pan, None, &[])).unwrap();
    assert_eq!(status, 200, "{body}");
}

// AGENT-3, D2 (F3): everything but Esc ends Compare at After, writing nothing: a close that isn't
// Esc's (any other action), a write with the page key, ⌘Z, the page's socket closing, the song
// closing, and an outside batch, after which the agent's change is still there to undo.
#[test]
fn anything_else_ends_compare_at_after() {
    let (fixture, graph) = first_song();
    let mut told = steps_of(&graph);
    let t = token(&graph);
    until(&graph, 400, |_| false);
    let has = |id: &str| fixture.read("data/bass/notes.json").contains(&format!("\"{id}\""));
    let open = |graph: &super::Graph, id: &str| {
        let step = agent_step(&fixture, graph, id, "brighter hats");
        graph.compare(token(graph), step, true, None).unwrap();
        assert!(!holds(&plays(graph), id), "Before plays");
        step
    };
    // Another action: the page's close without Esc.
    open(&graph, "a1");
    graph.close_compare(t, false).unwrap();
    assert!(holds(&plays(&graph), "a1") && has("a1"), "After plays, and is the song");
    // A knob's release, with the page's key.
    open(&graph, "a2");
    set(&graph, 0.3, 0.4);
    assert_eq!(graph.comparing(), Value::Null, "a page write ends it");
    assert!(holds(&plays(&graph), "a2") && has("a2"), "at After");
    // ⌘Z, not Compare's: the musician's own undo of the step compared.
    let a3 = open(&graph, "a3");
    graph.undo(t).unwrap();
    assert_eq!(graph.comparing(), Value::Null, "an undo ends it");
    assert!(!has("a3"), "and is done");
    graph.redo_step(t, a3).unwrap();
    // The page's socket closes.
    open(&graph, "a4");
    graph.page_gone(None, false);
    assert_eq!(graph.comparing(), Value::Null, "the page went away");
    assert!(holds(&plays(&graph), "a4"), "at After");
    // A file written directly lands on Before.
    let a5 = open(&graph, "a5");
    fixture.put("tracks/hats.ts", &fixture.read("tracks/hats.ts").replacen("track(", "// an editor's autosave\ntrack(", 1));
    until(&graph, 3000, |g| g.comparing().is_null());
    assert_eq!(graph.comparing(), Value::Null, "the batch ended it");
    assert!(holds(&plays(&graph), "a5") && has("a5"), "at After");
    assert_eq!(graph.state().unwrap()["undo_name"]["source"], json!("outside"));
    graph.undo(t).unwrap();
    assert_eq!(graph.state().unwrap()["undo_name"]["step"], json!(a5), "the agent's change is still there to undo");
    // The song closes.
    open(&graph, "a6");
    graph.close();
    assert_eq!(graph.comparing(), Value::Null, "no song, nothing compared");
    assert!(has("a6"), "and the song is as it was");
    let events = heard(&mut told, |e| ended(e).len() == 6);
    assert_eq!(ended(&events), ["closed", "changed", "changed", "page", "outside", "song"], "each end is heard, with why");
    let closes: Vec<Value> = compares(&events).into_iter().filter(|e| e["open"] == json!(false)).collect();
    assert!(closes.iter().all(|e| e["after"] == json!(true)), "each at After: {closes:?}");
}

// D2 (F1, F2): a V names its step. While the pill is open, a V for another step is refused, naming
// what is compared, and a V for the side that plays already changes nothing; with the pill closed,
// V opens it only on the newest step, and never on After. Nothing refused changes the song.
#[test]
fn a_v_that_names_a_stale_step_is_refused() {
    let (fixture, graph) = first_song();
    let t = token(&graph);
    let older = agent_step(&fixture, &graph, "older1", "older bass hit");
    let newer = agent_step(&fixture, &graph, "newer1", "newer bass hit");
    let both = fixture.read("data/bass/notes.json");
    // Two Vs in flight, each naming the newest: the second plays Before as the first did.
    graph.compare(t, newer, true, None).unwrap();
    let v = graph.compare(t, newer, true, None).unwrap();
    assert_eq!((&v["comparing"]["step"], &v["comparing"]["after"]), (&json!(newer), &json!(false)), "{v}");
    // A V naming the older step is stale.
    let e = graph.compare(t, older, true, None).unwrap_err();
    let stale = e.downcast_ref::<super::Stale>().expect("refused as stale");
    assert_eq!(stale.comparing["step"], json!(newer), "{e}");
    assert!(e.to_string().contains("newer bass hit"), "{e}");
    // Two flips to After: both answer After, and the pill stays open.
    graph.compare(t, newer, false, None).unwrap();
    let v = graph.compare(t, newer, false, None).unwrap();
    assert_eq!(v["comparing"]["after"], json!(true), "{v}");
    assert_eq!(graph.comparing()["step"], json!(newer), "still open");
    graph.close_compare(t, false).unwrap();
    // Closed: V names the older step, or asks for After.
    let e = graph.compare(t, older, true, None).unwrap_err();
    let n = e.downcast_ref::<super::super::steps::NotNewest>().expect("not the newest");
    assert_eq!(n.newest["step"], json!(newer), "{e}");
    let e = graph.compare(t, newer, false, None).unwrap_err();
    assert!(e.downcast_ref::<super::Stale>().is_some_and(|s| s.comparing.is_null()), "After with the pill closed: {e}");
    assert_eq!(graph.comparing(), Value::Null);
    assert_eq!(fixture.read("data/bass/notes.json"), both, "nothing changed the song");
    assert_eq!(graph.state().unwrap()["undo_name"]["step"], json!(newer));
}

// D2 (F5): every change of the pill is a `compare` event: its session, its step, open or closed,
// Before or After, and a number that counts up, so a window keeps the newest it heard.
#[test]
fn every_change_of_the_pill_is_told() {
    let (fixture, graph) = first_song();
    let mut told = steps_of(&graph);
    let t = token(&graph);
    let step = agent_step(&fixture, &graph, "a1", "brighter hats");
    let opened = graph.compare(t, step, true, None).unwrap()["comparing"].clone();
    let flipped = graph.compare(t, step, false, None).unwrap()["comparing"].clone();
    graph.close_compare(t, false).unwrap();
    let events = compares(&heard(&mut told, |e| compares(e).len() == 3));
    let said: Vec<(Value, Value, Value, Value)> =
        events.iter().map(|e| (e["session"].clone(), e["step"].clone(), e["open"].clone(), e["after"].clone())).collect();
    let (s, st) = (json!(t), json!(step));
    let want = [
        (s.clone(), st.clone(), json!(true), json!(false)),
        (s.clone(), st.clone(), json!(true), json!(true)),
        (s, st, json!(false), json!(true)),
    ];
    assert_eq!(said, want, "{events:?}");
    let n: Vec<u64> = events.iter().map(|e| e["n"].as_u64().unwrap_or(0)).collect();
    assert!(n.windows(2).all(|w| w[0] < w[1]), "counting up: {n:?}");
    assert_eq!((&opened["n"], &flipped["n"]), (&json!(n[0]), &json!(n[1])), "the answers say the same");
    assert_eq!(events[0]["name"], json!("Claude: brighter hats"));
}

// AGENT-3 (F11): only the socket of the window that opened Compare ends it when it closes.
#[test]
fn only_the_window_that_opened_compare_ends_it() {
    let (fixture, graph) = first_song();
    let t = token(&graph);
    let step = agent_step(&fixture, &graph, "a1", "brighter hats");
    graph.compare(t, step, true, Some("w1".into())).unwrap();
    graph.page_gone(Some("w2"), true);
    assert_eq!(graph.comparing()["step"], json!(step), "another window's socket closing leaves it");
    graph.page_gone(Some("w1"), true);
    assert_eq!(graph.comparing(), Value::Null, "its own window's ends it");
    assert!(holds(&plays(&graph), "a1"), "at After");
}

// D2: V compares the newest change an agent or another program made. On the musician's own newest
// change it is refused, saying so and naming it, and nothing changes; with nothing to undo, too.
#[test]
fn v_compares_only_a_change_made_elsewhere() {
    let (fixture, graph) = first_song();
    let t = token(&graph);
    let e = graph.compare(t, 0, true, None).unwrap_err();
    assert!(e.to_string().contains("nothing to undo"), "{e}");
    set(&graph, 0.3, 0.4);
    let mine = graph.state().unwrap()["undo_name"]["step"].as_u64().unwrap();
    let e = graph.compare(t, mine, true, None).unwrap_err();
    let refused = e.downcast_ref::<super::super::steps::NotElsewhere>().expect("refused as the musician's own");
    assert_eq!(refused.newest["step"], json!(mine));
    assert!(e.to_string().contains("The newest change is yours"), "{e}");
    assert!(fixture.read("song.ts").contains("pan: 0.4"), "nothing undone");
    assert_eq!(graph.comparing(), Value::Null);
}
