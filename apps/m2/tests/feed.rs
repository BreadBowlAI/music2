//! `m2 feed` (FEED-6): an agent plays a track live from the terminal, with the app open. This
//! process stands for the app, as in `forward.rs`; its player is silent and keeps the wall clock.

mod app;

use std::time::{Duration, Instant};

use app::{App, said};
use serde_json::{Value, json};

/// `m2 feed <song> args…` as the agent Claude.
fn feed(a: &App, args: &[&str]) -> std::process::Output {
    let mut all = vec!["feed", a.dir()];
    all.extend_from_slice(args);
    all.extend_from_slice(&["--as", "Claude"]);
    a.m2(&all)
}

/// Its one JSON line.
fn line(o: &std::process::Output) -> Value {
    assert!(o.status.success(), "{}", said(o));
    serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("not JSON ({e}): {}", said(o)))
}

/// `notes` written to a file of the test's own: its path.
fn notes(a: &App, name: &str, notes: Value) -> String {
    let path = a.temp.0.join(name);
    std::fs::write(&path, notes.to_string()).unwrap();
    path.to_str().unwrap().to_string()
}

// FEED-6, FEED-1, FEED-7: the agent's loop: open, put, wait for low (the musician plays), put the
// next bars from written_to, keep what played as one undo step by its name, and close.
#[test]
fn the_agents_loop_with_m2_feed() {
    let a = App::open("first-song", |_| {});
    let o = feed(&a, &["open", "keys", "--low", "1"]);
    assert!(o.status.success() && said(&o).contains("a feed is open on keys"), "{}", said(&o));
    let state = a.running.state.graph.state().unwrap();
    assert_eq!((&state["feeds"][0]["feed"], &state["feeds"][0]["writer"]), (&json!("keys"), &json!("Claude")), "the Live mark's writer");
    let first = notes(&a, "a.json", json!([{"pitch": "A4", "start": 0.5, "length": 0.5}, {"pitch": "C5", "start": 2, "length": 0.5}]));
    let put = line(&feed(&a, &["put", "keys", "0", "8", &first]));
    assert_eq!((&put["taken"], &put["late"], &put["written_to"]), (&json!(2), &json!(0), &json!(8.0)), "{put}");
    // The musician presses Play; the agent waits for the buffer to drop under a bar.
    a.running.state.graph.transport("play").unwrap();
    let t = Instant::now();
    let low = line(&feed(&a, &["wait", "keys", "--for", "low", "--timeout", "20"]));
    assert_eq!((&low["t"], &low["why"], &low["feed"]), (&json!("feed"), &json!("low"), &json!("keys")), "{low}");
    let beats = low["buffer"]["beats"].as_f64().unwrap();
    assert!(beats < 4.0 && beats > 2.5, "said as it dropped under the low mark: {low}");
    assert!(t.elapsed() < Duration::from_secs(20));
    // The next bars, from written_to.
    let from = low["written_to"].as_f64().unwrap();
    let next = notes(&a, "b.json", json!([{"pitch": "E5", "start": from + 0.5, "length": 0.5}]));
    let put = line(&feed(&a, &["put", "keys", &from.to_string(), &(from + 8.0).to_string(), &next]));
    assert_eq!((&put["taken"], &put["written_to"]), (&json!(1), &json!(from + 8.0)), "{put}");
    let status = feed(&a, &["status"]);
    assert!(status.status.success() && said(&status).contains("keys: playing"), "{}", said(&status));
    let json = line(&feed(&a, &["status", "--json"]));
    assert_eq!(json["feeds"][0]["writer"], "Claude");
    a.running.state.graph.transport("stop").unwrap();
    // Keep: the two notes that played, as a new clip, one undo step by the agent.
    let kept = feed(&a, &["keep", "keys"]);
    assert!(kept.status.success() && said(&kept).contains("kept 2 notes"), "{}", said(&kept));
    let top = a.top();
    assert_eq!(top["by"], "Claude", "{top}");
    assert!(top["name"].as_str().unwrap_or_default().contains("played on"), "{top}");
    let closed = feed(&a, &["close", "keys"]);
    assert!(closed.status.success(), "{}", said(&closed));
    assert!(a.running.state.graph.state().unwrap()["feeds"].as_array().unwrap().is_empty());
    let gone = feed(&a, &["put", "keys", "0", "8", &first]);
    assert!(!gone.status.success() && said(&gone).contains("no feed open"), "{}", said(&gone));
}

// FEED-6: `wait --for low` answers at once when the feed is already low (here empty, which is
// lower); every wait has a time limit, and one that passes exits 1, saying so.
#[test]
fn wait_answers_at_once_when_low_and_times_out() {
    let a = App::open("first-song", |_| {});
    assert!(feed(&a, &["open", "keys"]).status.success());
    let t = Instant::now();
    let now = line(&feed(&a, &["wait", "--for", "low", "--timeout", "10"]));
    assert_eq!((&now["why"], &now["feed"]), (&json!("empty"), &json!("keys")), "{now}");
    assert!(t.elapsed() < Duration::from_secs(5), "at once: {:?}", t.elapsed());
    let t = Instant::now();
    let none = feed(&a, &["wait", "keys", "--for", "jump", "--timeout", "1"]);
    assert_eq!(none.status.code(), Some(1), "{}", said(&none));
    assert!(said(&none).contains("no feed event came in 1 s"), "{}", said(&none));
    assert!(t.elapsed() >= Duration::from_secs(1) && t.elapsed() < Duration::from_secs(10), "{:?}", t.elapsed());
    // A seek is a jump.
    let (child, _) = app::m2_started(&a.home, &["feed", a.dir(), "wait", "--for", "jump", "--timeout", "20"], &[]);
    std::thread::sleep(Duration::from_millis(1500));
    a.running.state.graph.seek(8.0).unwrap();
    let out = child.wait_with_output().unwrap();
    let jump: Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", said(&out)));
    assert_eq!(jump["why"], "jump", "{jump}");
    // A time limit past a day is refused in words, never a panic.
    let huge = feed(&a, &["wait", "--for", "jump", "--timeout", "1e20"]);
    assert_eq!(huge.status.code(), Some(1), "{}", said(&huge));
    assert!(said(&huge).contains("86400"), "{}", said(&huge));
    // No timeout is refused before anything is asked.
    let bare = a.m2(&["feed", a.dir(), "wait"]);
    assert_eq!(bare.status.code(), Some(2), "{}", said(&bare));
    // With the app closed: feeds need it.
    let closed = app::m2_in(&a.temp.0.join("nobody"), &["feed", a.dir(), "status"], &[]);
    assert!(!closed.status.success() && said(&closed).contains("needs the app open"), "{}", said(&closed));
}

// FEED-3, AGENT-7: `m2 listen` renders the song's files: a feed playing in the app never reaches it.
#[test]
fn m2_listen_never_hears_a_feed() {
    let a = App::open("first-song", |_| {});
    let listen = |a: &App| {
        let o = a.m2(&["listen", a.dir(), "--bars", "1-2", "--tracks", "keys", "--json"]);
        assert!(o.status.success(), "{}", said(&o));
        let v: Value = serde_json::from_slice(&o.stdout).unwrap();
        v["tracks"].clone()
    };
    let before = listen(&a);
    assert!(feed(&a, &["open", "keys"]).status.success());
    let loud: Vec<Value> = (0..16).map(|k| json!({"pitch": 84 + k % 5, "start": k as f64 * 0.5, "length": 0.5, "velocity": 1})).collect();
    let path = notes(&a, "loud.json", json!(loud));
    line(&feed(&a, &["put", "keys", "0", "8", &path]));
    a.running.state.graph.transport("play").unwrap();
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(listen(&a), before, "the same measures: the feed isn't in the song");
    a.running.state.graph.transport("stop").unwrap();
    // The same notes in the song change what it measures: so it would hear a feed that reached a file.
    let kept = feed(&a, &["keep", "keys"]);
    assert!(kept.status.success(), "{}", said(&kept));
    assert_ne!(listen(&a), before, "kept, they are heard");
}

// FEED-5, FEED-6: on a loop shorter than the low mark, all written, `wait --for low` waits: the
// state's low mark is the loop's length, as the server judges it, so an agent never spins.
#[test]
fn wait_on_a_short_loop_all_written_waits() {
    let a = App::open("first-song", |_| {});
    a.running.state.graph.set_loop(Some((0.0, 8.0)), Some(true)).unwrap();
    assert!(feed(&a, &["open", "keys", "--low", "4"]).status.success());
    let all = notes(&a, "loop.json", json!([{"pitch": "A4", "start": 0, "length": 1}, {"pitch": "C5", "start": 4, "length": 1}]));
    line(&feed(&a, &["put", "keys", "0", "8", &all]));
    a.running.state.graph.transport("play").unwrap();
    let st = line(&feed(&a, &["status", "--json"]));
    assert_eq!(st["feeds"][0]["low_mark"]["beats"], 8.0, "{st}");
    let t = Instant::now();
    let waited = feed(&a, &["wait", "keys", "--for", "low", "--timeout", "2"]);
    assert_eq!(waited.status.code(), Some(1), "it waited, and nothing came: {}", said(&waited));
    assert!(t.elapsed() >= Duration::from_secs(2), "{:?}", t.elapsed());
    a.running.state.graph.transport("stop").unwrap();
}
