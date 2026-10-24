//! The Compare key, V (D2; AGENT-3; track 3 of the agents design): V undoes the newest step and
//! keeps what it compares, V again redoes it. While the pill is open every agent's write is busy,
//! so no agent's step can clear redo and take "After" away. Any other change to the history, the
//! page's close, its socket closing, the song closing, or an outside batch ends it.

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

/// The `compare` events among `events`: why each ended it.
fn ended(events: &[Value]) -> Vec<String> {
    events.iter().filter(|e| e["t"] == "compare").map(|e| e["why"].as_str().unwrap_or("?").to_string()).collect()
}

// AGENT-3, D2: V's undo of the agent's newest step opens the pill, Before; V again, After. While it
// is open, every agent's write is busy, naming what is compared, and changes nothing: an undo, a
// Tx, a fix. Esc closes it, and the agent's write lands.
#[test]
fn while_comparing_an_agents_write_is_busy() {
    let (fixture, graph) = first_song();
    let mut told = steps_of(&graph);
    let t = token(&graph);
    let before = fixture.read("data/bass/notes.json");
    let (status, body) = agent(|| graph.tx_commit(Some(t), &notes_tx(&fixture, "a1"), None, &[])).unwrap();
    assert_eq!(status, 200, "{body}");
    let step = body["step"].as_u64().unwrap();
    let after = fixture.read("data/bass/notes.json");
    // V: Before.
    let v = graph.compare(t, step, false).unwrap();
    assert_eq!(v["step"], json!(step), "{v}");
    assert_eq!(fixture.read("data/bass/notes.json"), before, "the agent's change is undone");
    let c = graph.comparing();
    assert_eq!((&c["step"], &c["after"], &c["name"]), (&json!(step), &json!(false), &json!("Claude: brighter hats")), "{c}");
    assert_eq!(graph.state().unwrap()["comparing"], c, "the state says it");
    // An agent's writes are busy, naming it, and change nothing.
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
    assert_eq!(graph.state().unwrap()["redo_name"]["step"], json!(step), "redo still has it");
    // V again: After.
    graph.compare(t, step, true).unwrap();
    assert_eq!(fixture.read("data/bass/notes.json"), after, "the agent's change is back");
    assert_eq!(graph.comparing()["after"], json!(true));
    assert_eq!(agent(|| graph.tx_commit(Some(t), &pan, None, &[])).unwrap().0, 423, "still busy on After");
    // Undo and redo said what V did.
    let flipped = |e: &Value| e["t"] == "undo" || e["t"] == "redo";
    let events = heard(&mut told, |e| e.iter().filter(|e| flipped(e)).count() == 2);
    let flips: Vec<(&str, bool, &Value)> =
        events.iter().filter(|e| flipped(e)).map(|e| (e["t"].as_str().unwrap(), e["compare"] == json!(true), &e["by"])).collect();
    assert_eq!(flips, [("undo", true, &json!("Claude")), ("redo", true, &json!("Claude"))], "{events:?}");
    // Esc: closed, heard, and the agent's write lands.
    graph.close_compare(t).unwrap();
    assert_eq!(graph.comparing(), Value::Null);
    assert_eq!(ended(&heard(&mut told, |e| !ended(e).is_empty())), ["closed"]);
    let (status, body) = agent(|| graph.tx_commit(Some(t), &pan, None, &[])).unwrap();
    assert_eq!(status, 200, "{body}");
    assert!(graph.close_compare(t + 1).is_err(), "another song's");
}

// AGENT-3, D2: a file written directly can land while the pill is open. The batch the watcher takes
// in ends Compare, saying a file changed outside the app, and agents aren't busy any more.
#[test]
fn an_outside_batch_ends_compare() {
    let (fixture, graph) = first_song();
    let mut told = steps_of(&graph);
    let t = token(&graph);
    until(&graph, 400, |_| false);
    let (status, body) = agent(|| graph.tx_commit(Some(t), &notes_tx(&fixture, "a1"), None, &[])).unwrap();
    assert_eq!(status, 200, "{body}");
    graph.compare(t, body["step"].as_u64().unwrap(), false).unwrap();
    assert!(graph.comparing().is_object());
    heard(&mut told, |e| e.iter().any(|e| e["t"] == "undo"));
    fixture.put("tracks/hats.ts", &fixture.read("tracks/hats.ts").replacen("track(", "// changed outside\ntrack(", 1));
    until(&graph, 3000, |g| g.comparing().is_null());
    assert_eq!(graph.comparing(), Value::Null, "the batch ended it");
    assert_eq!(ended(&heard(&mut told, |e| !ended(e).is_empty())), ["outside"]);
    let pan = values_tx(&fixture, "song.ts", "pan: 0.3", "0.3", "0.4");
    assert_eq!(agent(|| graph.tx_commit(Some(t), &pan, None, &[])).unwrap().0, 200, "agents aren't busy any more");
}

// AGENT-3, D2: any other change to the history ends it (a write with the page key, an undo not V's),
// and so do the page's socket closing and the song closing. What plays stays.
#[test]
fn any_other_change_ends_compare() {
    let (fixture, graph) = first_song();
    let mut told = steps_of(&graph);
    let t = token(&graph);
    let open = |graph: &super::Graph| {
        let (status, body) = agent(|| graph.tx_commit(Some(token(graph)), &notes_tx(&fixture, "a1"), None, &[])).unwrap();
        assert_eq!(status, 200, "{body}");
        graph.compare(token(graph), body["step"].as_u64().unwrap(), false).unwrap();
        assert!(graph.comparing().is_object());
    };
    // A knob's release, with the page's key.
    open(&graph);
    set(&graph, 0.3, 0.4);
    assert_eq!(graph.comparing(), Value::Null, "a page write ends it");
    // ⌘Z, not V's: it takes back the knob's release, under the step compared.
    open(&graph);
    assert!(fixture.read("song.ts").contains("pan: 0.4"));
    graph.undo(t).unwrap();
    assert_eq!(graph.comparing(), Value::Null, "an undo ends it");
    assert!(fixture.read("song.ts").contains("pan: 0.3"), "and is done");
    // The page's socket closes.
    open(&graph);
    graph.page_gone();
    assert_eq!(graph.comparing(), Value::Null, "the page went away");
    // The song closes.
    open(&graph);
    graph.close();
    assert_eq!(graph.comparing(), Value::Null, "no song, nothing compared");
    let why = ended(&heard(&mut told, |e| ended(e).len() == 4));
    assert_eq!(why, ["changed", "changed", "page", "song"], "each end is heard, with why");
}

// D2: V compares the newest change an agent or another program made. On the musician's own newest
// change it is refused, saying so and naming it, and nothing changes; with nothing to undo, too.
#[test]
fn v_compares_only_a_change_made_elsewhere() {
    let (fixture, graph) = first_song();
    let t = token(&graph);
    let e = graph.compare(t, 0, false).unwrap_err();
    assert!(e.to_string().contains("nothing to undo"), "{e}");
    set(&graph, 0.3, 0.4);
    let mine = graph.state().unwrap()["undo_name"]["step"].as_u64().unwrap();
    let e = graph.compare(t, mine, false).unwrap_err();
    let refused = e.downcast_ref::<super::super::steps::NotElsewhere>().expect("refused as the musician's own");
    assert_eq!(refused.newest["step"], json!(mine));
    assert!(e.to_string().contains("The newest change is yours"), "{e}");
    assert!(fixture.read("song.ts").contains("pan: 0.4"), "nothing undone");
    assert_eq!(graph.comparing(), Value::Null);
}
