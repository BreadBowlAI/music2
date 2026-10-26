//! Undo's names on the server (gaps #29, plan C3 as X7 built it): the window's names follow undo
//! and redo, a step made elsewhere is named plainly, and a refused undo keeps them.

use std::path::Path;

use m2_song::tx::{Base, Confined, Expect, Op, TxPath};
use serde_json::json;

use super::super::tests::{Fixture, knob};
use super::*;

/// examples/first-song, open.
pub(in crate::graph) fn first_song() -> (Fixture, Graph) {
    let fixture = first_song_files();
    let graph = fixture.open();
    (fixture, graph)
}

/// examples/first-song's files, but its audio.
pub(in crate::graph) fn first_song_files() -> Fixture {
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
    fixture
}

/// The state's names of Undo's and Redo's steps: their words, or "-" with none.
fn names(graph: &Graph) -> (String, String) {
    let s = graph.state().unwrap();
    let say = |v: &Value| v["name"].as_str().unwrap_or("-").to_string();
    (say(&s["undo_name"]), say(&s["redo_name"]))
}

/// Set the control whose value is `from` to `to`, as a knob's release does; the revision it made.
pub(in crate::graph) fn set(graph: &Graph, from: f64, to: f64) -> u64 {
    let (token, revision, id) = knob(graph, from);
    graph.set(token, revision, &id, "set", to, None).unwrap()["revision"].as_u64().unwrap()
}

pub(in crate::graph) fn token(graph: &Graph) -> u64 {
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
    let (status, answer) = graph.tx_commit(None, &tx.to_json(), None, &[]).unwrap();
    assert_eq!(status, 200, "{answer}");
    assert_eq!(answer["step"], graph.state().unwrap()["undo_name"]["step"], "it answers with its step, as an edit does: {answer}");
    assert_eq!(names(&graph).0, "Bass notes");
    assert_eq!(graph.undo(token(&graph)).unwrap()["name"], json!("Bass notes"));
    assert_eq!(names(&graph).1, "Bass notes");
    // The plain names of other files, by the timeline published.
    let t = graph.inner.control.lock().open.as_ref().unwrap().shown.timeline.clone();
    assert_eq!(plain("data/arrangement.json", &t), "the tempo or time signature");
    assert_eq!(plain("data/bass/cutoff.json", &t), "Bass cutoff automation");
    assert_eq!(plain("tracks/bass.ts", &t), "the code in tracks/bass.ts");
    // The review of ag-routes' F10: the song's key lives there too.
    assert_eq!(plain("layout.json", &t), "the track order, colours, markers or the song's key");
}

// WRITE-21: an undo refused (what it would take back changed since) keeps both names as they were.
#[test]
fn a_refused_undo_keeps_the_names() {
    let (fixture, graph) = first_song();
    let made = set(&graph, 0.3, 0.4);
    graph.name_step(token(&graph), made, "Hats pan".into(), false).unwrap();
    // The same value changes meanwhile in a way that makes no step, as a git checkout does: `poll`
    // takes the folder in without the watcher, so it records none. (An editor's save would be a
    // step of its own, which undo takes back first, AGENT-2.)
    fixture.put("song.ts", &fixture.read("song.ts").replacen("pan: 0.4", "pan: 0.45", 1));
    graph.poll();
    let refused = graph.undo(token(&graph));
    assert!(refused.is_err(), "{refused:?}");
    assert_eq!(names(&graph), ("Hats pan".into(), "-".into()));
}

/// Every `step` event the graph sends from now on (AGENT-5).
pub(in crate::graph) fn steps_of(graph: &Graph) -> tokio::sync::broadcast::Receiver<String> {
    let (to, from) = tokio::sync::broadcast::channel(64);
    graph.tell_steps(to);
    from
}

/// The `step` events heard so far, as JSON.
pub(in crate::graph) fn heard(from: &mut tokio::sync::broadcast::Receiver<String>) -> Vec<Value> {
    let mut out = vec![];
    while let Ok(e) = from.try_recv() {
        let v: Value = serde_json::from_str(&e).unwrap();
        assert_eq!(v["t"], json!("step"), "{v}");
        out.push(v);
    }
    out
}

/// Watch for up to `ms`, until `done` holds.
pub(in crate::graph) fn until(graph: &Graph, ms: u64, done: impl FnMut(&Graph) -> bool) {
    super::super::tests::watching(graph, ms, done);
}

/// A mono 16-bit WAV of `frames` frames at 48 kHz.
pub(in crate::graph) fn wav(frames: usize) -> Vec<u8> {
    let spec = hound::WavSpec { channels: 1, sample_rate: 48_000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut out = std::io::Cursor::new(vec![]);
    let mut w = hound::WavWriter::new(&mut out, spec).unwrap();
    for i in 0..frames {
        w.write_sample(((i % 100) as i16 - 50) * 100).unwrap();
    }
    w.finalize().unwrap();
    out.into_inner()
}

// AGENT-2: an import writes its asset, takes it in for its own Tx, and makes that Tx's one step,
// the page's. The watcher's look at the files it wrote makes no outside step.
#[test]
fn an_import_in_the_app_makes_one_step_and_no_outside_step() {
    let (fixture, graph) = first_song();
    let mut steps = steps_of(&graph);
    until(&graph, 400, |_| false);
    let target = m2_editor::import::Target::Below { beat: 0.0 };
    let source = m2_editor::import::Source::Bytes { name: "tone.wav".into(), bytes: wav(4_800) };
    graph.import_audio(token(&graph), vec![source], target).unwrap();
    assert!(fixture.0.join("assets/tone.wav").exists());
    until(&graph, 1500, |_| false);
    let told = heard(&mut steps);
    assert_eq!(told.len(), 1, "one step: {told:?}");
    assert_eq!(told[0]["source"], json!("page"));
    let s = graph.state().unwrap();
    assert_eq!((&s["undo_name"]["source"], &s["undo_name"]["step"]), (&json!("page"), &told[0]["step"]));
    graph.undo(token(&graph)).unwrap();
    assert_eq!(graph.state().unwrap()["can_undo"], json!(false), "nothing else to undo");
}

// FMT-23: the song's files for agents are no part of it: editing them while it is open makes no
// build, no step and no event.
#[test]
fn editing_the_songs_agents_md_makes_no_build_and_no_step() {
    let (fixture, graph) = first_song();
    let mut steps = steps_of(&graph);
    until(&graph, 400, |_| false);
    let revision = knob(&graph, 0.3).1;
    fixture.put("AGENTS.md", "# Working on a music2 song\n");
    fixture.put("CLAUDE.md", "@AGENTS.md\n");
    until(&graph, 1500, |_| false);
    fixture.put("AGENTS.md", "# Working on a music2 song\n\nMore words.\n");
    until(&graph, 1500, |_| false);
    assert_eq!(heard(&mut steps), Vec::<Value>::new(), "no step");
    assert_eq!(knob(&graph, 0.3).1, revision, "no build");
    assert_eq!(graph.state().unwrap()["can_undo"], json!(false), "nothing to undo");
}

// AGENT-2: Undo's name says whose step is next: the page's by its name, an outside one as an
// outside change, by its file, in the state and in the Edit menu's words.
#[test]
fn undo_says_the_source_of_the_newest_step() {
    let (fixture, graph) = first_song();
    until(&graph, 400, |_| false);
    set(&graph, 0.3, 0.4);
    let mine = graph.state().unwrap()["undo_name"].clone();
    assert_eq!((&mine["source"], &mine["name"]), (&json!("page"), &json!("the code in song.ts")), "{mine}");
    let id = mine["step"].as_u64().expect("a step id");
    assert_eq!(graph.undo_names().0.as_deref(), Some("the code in song.ts"));
    fixture.put("tracks/bass.ts", &format!("{}\n// from another editor\n", fixture.read("tracks/bass.ts")));
    until(&graph, 3000, |g| g.state().unwrap()["undo_name"]["source"] == json!("outside"));
    let theirs = graph.state().unwrap()["undo_name"].clone();
    assert_eq!(
        (&theirs["source"], &theirs["name"], &theirs["given"]),
        (&json!("outside"), &json!("the code in tracks/bass.ts"), &json!(false)),
        "{theirs}"
    );
    assert!(theirs["step"].as_u64().unwrap() > id, "a new id: {theirs}");
    assert_eq!(graph.undo_names().0.as_deref(), Some("outside change: the code in tracks/bass.ts"), "the Edit menu's words");
    let undone = graph.undo(token(&graph)).unwrap();
    assert_eq!(undone["name"], json!("the code in tracks/bass.ts"));
    assert_eq!(graph.state().unwrap()["redo_name"]["source"], json!("outside"), "redo says it too");
    assert_eq!(graph.state().unwrap()["undo_name"]["step"], json!(id), "the page's step is next again");
}

// AGENT-2: an undo or a redo names its step by id, and is refused when that step isn't the
// newest (for redo, the newest undone): nothing changes, and the refusal names the newest.
#[test]
fn an_undo_for_a_step_that_is_no_longer_newest_changes_nothing() {
    let (fixture, graph) = first_song();
    until(&graph, 400, |_| false);
    fixture.put("tracks/bass.ts", &format!("{}\n// from another editor\n", fixture.read("tracks/bass.ts")));
    until(&graph, 3000, |g| g.state().unwrap()["undo_name"]["source"] == json!("outside"));
    let outside = graph.state().unwrap()["undo_name"]["step"].as_u64().expect("an outside step");
    set(&graph, 0.3, 0.4);
    let mine = graph.state().unwrap()["undo_name"]["step"].as_u64().unwrap();
    assert_ne!(mine, outside);
    let (song, bass) = (fixture.read("song.ts"), fixture.read("tracks/bass.ts"));
    let refused = graph.undo_step(token(&graph), outside).unwrap_err();
    let not = refused.downcast_ref::<NotNewest>().unwrap_or_else(|| panic!("not the newest: {refused:#}"));
    assert_eq!(not.newest["step"], json!(mine));
    assert!(refused.to_string().contains("The newest is the code in song.ts"), "{refused}");
    assert_eq!((fixture.read("song.ts"), fixture.read("tracks/bass.ts")), (song.clone(), bass.clone()), "nothing changed");
    assert!(graph.redo_step(token(&graph), mine).unwrap_err().downcast_ref::<NotNewest>().is_some(), "nothing to redo");
    let undone = graph.undo_step(token(&graph), mine).unwrap();
    assert_eq!(undone["step"], json!(mine), "it says the step it took back");
    assert_eq!((&undone["undo_name"]["step"], &undone["redo_name"]["step"]), (&json!(outside), &json!(mine)), "and the tops after it");
    assert!(graph.redo_step(token(&graph), outside).unwrap_err().downcast_ref::<NotNewest>().is_some(), "redo's newest is mine");
    graph.undo_step(token(&graph), outside).unwrap();
    assert!(!fixture.read("tracks/bass.ts").contains("another editor"), "the outside step undone, now it is the newest");
    graph.redo_step(token(&graph), outside).unwrap();
    assert_eq!(graph.state().unwrap()["undo_name"]["step"], json!(outside), "redone, with its id");
}

// AGENT-5: after each job that records a step, every window hears a `step` event: its id, the
// revision it made, its source, its name, its files, and the tracks, lanes and knobs its delta
// changed, by the view's names.
#[test]
fn a_step_event_names_what_its_delta_changed() {
    let (fixture, graph) = first_song();
    let mut steps = steps_of(&graph);
    until(&graph, 400, |_| false);
    fixture.put("song.ts", &fixture.read("song.ts").replacen("pan: 0.3", "pan: 0.45", 1));
    let notes = fixture.read("data/bass/notes.json");
    fixture.put("data/bass/notes.json", &notes.replacen("\"start\": 0", "\"start\": 0.5", 1));
    let mut told = vec![];
    until(&graph, 3000, |_| {
        told.extend(heard(&mut steps));
        !told.is_empty()
    });
    let (session, revision, pan) = knob(&graph, 0.45);
    let [e] = &told[..] else { panic!("one step event: {told:?}") };
    let state = graph.state().unwrap();
    assert_eq!((&e["session"], &e["made"], &e["step"]), (&json!(session), &json!(revision), &state["undo_name"]["step"]), "{e}");
    assert_eq!((&e["source"], &e["name"], &e["undoable"]), (&json!("outside"), &json!("2 files"), &json!(true)), "{e}");
    assert_eq!(e["files"], json!(["data/bass/notes.json", "song.ts"]), "{e}");
    assert_eq!(e["tracks"], json!(["bass", "song"]), "{e}");
    assert_eq!(e["lanes"], json!([["bass", "notes"]]), "{e}");
    assert_eq!(e["knobs"], json!([pan]), "{e}");
    // The page's own steps are told too, with their source.
    set(&graph, -0.25, -0.5);
    let mut mine = vec![];
    until(&graph, 2000, |_| {
        mine.extend(heard(&mut steps));
        !mine.is_empty()
    });
    assert_eq!(mine.len(), 1, "{mine:?}");
    assert_eq!((&mine[0]["source"], &mine[0]["tracks"]), (&json!("page"), &json!(["song"])), "{:?}", mine[0]);
    assert_eq!(mine[0]["knobs"].as_array().map(Vec::len), Some(1), "{:?}", mine[0]);
}

/// A server on first-song's files, with the test token: its url, the graph and the session.
fn serving() -> (Fixture, crate::Running, u64) {
    let fixture = first_song_files();
    let options = crate::ServeOptions { port: 0, web_dir: fixture.0.join("no-web"), token: "t00ls".into(), page_token: "t0k".into() };
    let running = crate::start_with(options, Graph::silent()).unwrap();
    let session = running.state.graph.open(&fixture.0).unwrap()["session"].as_u64().unwrap();
    (fixture, running, session)
}

// AGENT-2, as the page sees it: `POST /api/graph/undo` with a step that isn't the newest answers
// 409, with the newest in `newest` and words that name it, and changes nothing; so does the page's
// 0 (it names no step). With the newest, 200, and the step it took back.
#[test]
fn the_undo_route_answers_409_naming_the_newest_step() {
    let (fixture, running, session) = serving();
    let graph = &running.state.graph;
    set(graph, 0.3, 0.4);
    let first = graph.state().unwrap()["undo_name"]["step"].as_u64().unwrap();
    set(graph, -0.25, -0.5);
    let second = graph.state().unwrap()["undo_name"]["step"].as_u64().unwrap();
    let song = fixture.read("song.ts");
    let post =
        |step: u64| crate::routes_8b::tests::post(&running.url, "/api/graph/undo", Some("t0k"), &json!({"session": session, "step": step}));
    for stale in [first, 0] {
        let (status, body) = post(stale);
        assert_eq!((status, &body["newest"]["step"], &body["newest"]["source"]), (409, &json!(second), &json!("page")), "{body}");
        assert!(body["error"].as_str().unwrap().contains("The newest is the code in song.ts"), "{body}");
    }
    assert_eq!(fixture.read("song.ts"), song, "nothing changed");
    let (status, body) = post(second);
    assert_eq!((status, &body["step"], &body["undo_name"]["step"]), (200, &json!(second), &json!(first)), "{body}");
    assert_ne!(fixture.read("song.ts"), song, "undone");
}

// AGENT-2: an undo refused because its file changed since in a way that made no step (here a
// batch that also removed an audio file, which undo can't put back) answers 409 with `stuck`.
// Skipping it writes nothing, and Undo reaches the step before it; a step undo can take back
// isn't skipped.
#[test]
fn a_stuck_undo_answers_409_and_can_be_skipped() {
    let (fixture, running, session) = serving();
    let graph = &running.state.graph;
    let post =
        |path: &str, step: u64| crate::routes_8b::tests::post(&running.url, path, Some("t0k"), &json!({"session": session, "step": step}));
    set(graph, 0.3, 0.4);
    let mine = graph.state().unwrap()["undo_name"]["step"].as_u64().unwrap();
    std::fs::create_dir_all(fixture.0.join("assets")).unwrap();
    std::fs::write(fixture.0.join("assets/spare.wav"), wav(480)).unwrap();
    let bass = fixture.read("tracks/bass.ts");
    fixture.put("tracks/bass.ts", &format!("{bass}\n// from another editor\n"));
    let start = std::time::Instant::now();
    let outside = loop {
        let top = graph.state().unwrap()["undo_name"].clone();
        if top["source"] == json!("outside") {
            break top["step"].as_u64().unwrap();
        }
        assert!(start.elapsed().as_secs() < 5, "an outside step");
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    let (status, body) = post("/api/graph/undo/skip", outside);
    assert_eq!(status, 400, "undo can take it back: {body}");
    // One batch: the audio file goes, which makes no step, and the code changes again.
    std::fs::remove_file(fixture.0.join("assets/spare.wav")).unwrap();
    fixture.put("tracks/bass.ts", &format!("{bass}\n// changed again\n"));
    let start = std::time::Instant::now();
    while graph.state().unwrap()["undo_stuck"] != json!(true) {
        assert!(start.elapsed().as_secs() < 5, "stuck: {}", graph.state().unwrap()["undo_name"]);
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let (status, body) = post("/api/graph/undo", outside);
    assert_eq!((status, &body["stuck"]["step"]), (409, &json!(outside)), "{body}");
    assert_eq!(body["error"], json!("tracks/bass.ts changed since, so Undo can't take back that change."), "{body}");
    assert!(fixture.read("tracks/bass.ts").contains("changed again"), "nothing was written");
    assert_eq!(post("/api/graph/undo/skip", mine).0, 409, "only the newest is skipped");
    let (status, body) = post("/api/graph/undo/skip", outside);
    assert_eq!((status, &body["undo_name"]["step"]), (200, &json!(mine)), "{body}");
    assert!(fixture.read("tracks/bass.ts").contains("changed again"), "skipping writes nothing");
    assert_eq!(post("/api/graph/undo", mine).0, 200, "Undo reaches the step before it");
}
