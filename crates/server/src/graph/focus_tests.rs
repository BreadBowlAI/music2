//! The focus record and pointers (AGENT-6; track 3 of the agents design): what the musician looks
//! at and what an agent points at are attention, not song. Memory only: no file, revision or undo
//! step; they go with the song.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::super::steps::tests::{first_song, steps_of, token};
use super::*;
use crate::caller::{Caller, as_caller};

/// Every file of the song but its `.music2/` (the lock, the app's own state): what a write changes.
fn files(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.file_name().is_some_and(|n| n == ".music2") {
                continue;
            }
            if p.is_dir() {
                stack.push(p);
            } else {
                out.insert(p.strip_prefix(dir).unwrap().to_path_buf(), std::fs::read(&p).unwrap());
            }
        }
    }
    out
}

/// Every event heard so far, as JSON.
fn events(from: &mut tokio::sync::broadcast::Receiver<String>) -> Vec<Value> {
    let mut out = vec![];
    while let Ok(e) = from.try_recv() {
        out.push(serde_json::from_str(&e).unwrap());
    }
    out
}

/// What the page would post with Bass selected and two of its notes picked in the roll.
fn record(token: u64, revision: u64) -> Value {
    json!({
        "session": token,
        "revision": revision,
        "view": "notes",
        "track": "bass",
        "clips": [{"track": "bass", "lane": "notes", "id": "b1"}],
        "range": null,
        "notes": {"clip": {"track": "bass", "lane": "notes", "id": "b1"}, "notes": [
            {"pitch": 40, "start": 0.0, "length": 0.25},
            {"pitch": 43, "start": 1.0, "length": 0.5},
        ]},
        "knob": null,
        "seen": [0.0, 32.0],
        "code": null,
    })
}

// AGENT-6: the page's focus record is kept in memory and served with what the server knows (where
// playback is, the loop, unsaved code, Compare, the MIDI target); every window and agent hears it.
// It writes no file, makes no revision and no step, and goes when the song closes or another opens.
#[test]
fn focus_is_memory_only_and_goes_with_the_song() {
    let (fixture, graph) = first_song();
    let mut told = steps_of(&graph);
    let t = token(&graph);
    let revision = graph.state().unwrap()["view"]["revision"].as_u64().unwrap();
    let before = files(&fixture.0);
    let posted = record(t, revision);
    let served = graph.post_focus(serde_json::from_value(posted.clone()).unwrap()).unwrap();
    for k in ["session", "revision", "view", "track", "clips", "notes", "seen"] {
        assert_eq!(served[k], posted[k], "{k}: {served}");
    }
    assert_eq!((&served["playing"], &served["unsaved"], &served["comparing"]), (&json!(false), &json!([]), &Value::Null), "{served}");
    assert!(served["beat"].is_number() && served.get("loop").is_some() && served.get("midi").is_some(), "the server's fields: {served}");
    assert_eq!(graph.focus(), served, "GET /api/focus says it");
    let heard: Vec<Value> = events(&mut told).into_iter().filter(|e| e["t"] == "focus").collect();
    assert_eq!(heard.len(), 1, "one event: {heard:?}");
    assert_eq!(heard[0]["focus"], served);
    // Attention, not song.
    assert_eq!(files(&fixture.0), before, "no file changed");
    assert_eq!(graph.state().unwrap()["view"]["revision"], json!(revision), "no revision");
    assert_eq!(graph.state().unwrap()["can_undo"], json!(false), "no undo step");
    // The newest post wins.
    let mut kick = posted.clone();
    kick["track"] = json!("kick");
    graph.post_focus(serde_json::from_value(kick).unwrap()).unwrap();
    assert_eq!(graph.focus()["track"], json!("kick"));
    // The song closes: it goes. Opened again, there is none until the page posts.
    graph.close();
    assert_eq!(graph.focus(), Value::Null, "no song, no focus");
    graph.open(&fixture.0).unwrap();
    let again = graph.focus();
    assert_eq!((&again["session"], &again["track"], &again["view"]), (&json!(token(&graph)), &Value::Null, &Value::Null), "{again}");
    assert_eq!(files(&fixture.0), before, "still no file changed");
}

// AGENT-6: a focus for another song than the open one is refused, and changes nothing; the tools
// key's posts are refused at the route (`routes_see_tests`).
#[test]
fn a_focus_for_another_song_is_refused() {
    let (_fixture, graph) = first_song();
    let mut told = steps_of(&graph);
    let t = token(&graph);
    graph.post_focus(serde_json::from_value(record(t, 1)).unwrap()).unwrap();
    let kept = graph.focus();
    events(&mut told);
    let mut other = record(t + 1, 1);
    other["track"] = json!("kick");
    let e = graph.post_focus(serde_json::from_value(other).unwrap()).unwrap_err();
    assert!(e.to_string().contains("open project changed"), "{e}");
    assert_eq!(graph.focus(), kept, "the focus stays as it was");
    assert!(events(&mut told).iter().all(|e| e["t"] != "focus"), "and nobody hears of it");
}

// AGENT-6: a pointer is not a write. It works in a read-only window; it changes no file, revision
// or step; its target is checked against the song (a track, a clip, a lane, a knob or bars); its
// words are an agent's (AGENT-10); there is one at a time, the page's OK clears it, and it goes
// with the song.
#[test]
fn a_point_is_not_a_write() {
    let (fixture, graph) = first_song();
    // Read-only: another program holds the lock.
    graph.close();
    let held = m2_session::lock::acquire(&fixture.0, "m2", m2_session::lock::RETRY).unwrap();
    graph.open(&fixture.0).unwrap();
    assert!(graph.read_only().is_some(), "the window is read-only");
    let mut told = steps_of(&graph);
    let t = token(&graph);
    let revision = graph.state().unwrap()["view"]["revision"].clone();
    let before = files(&fixture.0);
    let claude = |f: &dyn Fn() -> Result<Value>| as_caller(Caller::agent(Some("Claude"), None), f);
    let p = claude(&|| graph.point(t, Some(("bars:9-16", "listen to the\u{7} hats here")))).unwrap();
    assert_eq!(
        (&p["target"], &p["kind"], &p["beats"], &p["words"], &p["by"]),
        (&json!("bars:9-16"), &json!("bars"), &json!([32.0, 64.0]), &json!("listen to the hats here"), &json!("Claude")),
        "{p}"
    );
    assert_eq!(graph.pointer(), p);
    assert_eq!(graph.state().unwrap()["point"], p, "the state holds it for a page that loads later");
    let heard: Vec<Value> = events(&mut told).into_iter().filter(|e| e["t"] == "point").collect();
    assert_eq!((heard.len(), &heard[0]["point"]), (1, &p), "{heard:?}");
    // Each kind of target the song has.
    let bass = graph.state().unwrap()["timeline"]["tracks"].as_array().unwrap().iter().find(|t| t["module"] == "bass").cloned().unwrap();
    let notes = bass["lanes"].as_array().unwrap().iter().find(|l| l["key"] == "notes").cloned().unwrap();
    let clip = notes["clips"].as_array().unwrap().iter().find_map(|c| c["id"].as_str()).unwrap().to_string();
    let knob = graph.state().unwrap()["view"]["modules"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|m| m["nodes"].as_array().unwrap().iter())
        .flat_map(|n| n["knobs"].as_array().into_iter().flatten())
        .find_map(|k| k["id"].as_str().map(str::to_string))
        .unwrap();
    for (target, kind) in [
        ("track:bass".to_string(), "track"),
        (format!("clip:bass/notes:{clip}"), "clip"),
        ("lane:bass/notes".to_string(), "lane"),
        (format!("knob:{knob}"), "knob"),
        ("bars:3".to_string(), "bars"),
    ] {
        let p = claude(&|| graph.point(t, Some((&target, "here")))).unwrap();
        assert_eq!((&p["target"], &p["kind"]), (&json!(target), &json!(kind)), "{p}");
    }
    assert_eq!(graph.pointer()["beats"], json!([8.0, 12.0]), "bar 3, in 4/4");
    // What the song hasn't, or what isn't a target, is refused, and the pointer stays.
    let kept = graph.pointer();
    for bad in ["track:nope", "clip:bass/notes:nope", "lane:bass/nope", "knob:nope", "bars:0-2", "bars:5-3", "elsewhere:x", "bass"] {
        assert!(claude(&|| graph.point(t, Some((bad, "here")))).is_err(), "{bad} is refused");
    }
    assert!(claude(&|| graph.point(t + 1, Some(("track:bass", "here")))).is_err(), "another song's");
    assert_eq!(graph.pointer(), kept);
    // Words cut to 80.
    let long = "x".repeat(100);
    let p = claude(&|| graph.point(t, Some(("track:bass", &long)))).unwrap();
    assert_eq!(p["words"].as_str().unwrap().chars().count(), 80);
    // Not song.
    assert_eq!(files(&fixture.0), before, "no file changed");
    assert_eq!(graph.state().unwrap()["view"]["revision"], revision, "no revision");
    assert_eq!(graph.state().unwrap()["can_undo"], json!(false), "no undo step");
    // The page's OK clears it, and that is heard too.
    events(&mut told);
    assert_eq!(graph.point(t, None).unwrap(), Value::Null);
    assert_eq!(graph.pointer(), Value::Null);
    let heard: Vec<Value> = events(&mut told).into_iter().filter(|e| e["t"] == "point").collect();
    assert_eq!((heard.len(), &heard[0]["point"]), (1, &Value::Null), "{heard:?}");
    // It goes with the song.
    claude(&|| graph.point(t, Some(("track:bass", "here")))).unwrap();
    graph.close();
    assert_eq!(graph.pointer(), Value::Null);
    drop(held);
}
