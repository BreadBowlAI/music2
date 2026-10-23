//! Clip gestures on the server (8b.5): the data path, the answer's new clips, and undo.

use m2_editor::clips::{ClipRef, Gesture};
use m2_song::LaneAddr;

use super::super::tests::{Fixture, KICK, eventually, generation, synced};
use super::*;

fn session(graph: &Graph) -> u64 {
    graph.state().unwrap()["session"].as_u64().unwrap()
}

fn done(a: Answer) -> Value {
    match a {
        Answer::Done(v) => v,
        Answer::Changed(why) => panic!("changed: {why}"),
    }
}

// PLANE-1, RENDER-26: a clip gesture is a data-only Tx: no QuickJS run, the same renderer plays
// on with the lane swapped in, and the answer names the clip it made. Undo goes the same way.
#[test]
fn a_clip_gesture_takes_the_data_path() {
    let fixture = Fixture::new();
    fixture.write(KICK);
    let graph = fixture.open();
    graph.transport("play").unwrap();
    eventually("it plays", || graph.status().playing);
    let first = generation(&graph);
    let builds = graph.inspect(|_| m2_script::builds());
    let token = session(&graph);
    let hits = LaneAddr::new("kick", "hits").unwrap();
    let made = done(graph.clips(token, Gesture::Create { lane: hits.clone(), start: 0.0, length: Some(4.0) }).unwrap());
    assert_eq!((made["file"].as_str(), made["after"].as_str()), (Some("data/kick/hits.json"), Some("clip at bar 1 (1 bar)")));
    let id = made["made"][0]["id"].as_str().unwrap().to_string();
    assert_eq!((made["made"][0]["module"].as_str(), made["made"][0]["lane"].as_str()), (Some("kick"), Some("hits")));
    let clip = ClipRef { lane: hits.clone(), id: Some(id.clone()), at: None, sha: None };
    let notes = vec![m2_song::Note { pitch: 24.0, start: 0.0, length: 0.25, velocity: 1.0 }];
    // Notes aren't drawn by a gesture yet (B7): a whole clip put is.
    let put = json!({"id": id, "start": 0, "length": 4, "notes": notes});
    done(graph.clips(token, Gesture::Set { lane: hits.clone(), put: vec![put], delete: vec![] }).unwrap());
    done(graph.clips(token, Gesture::Move { clips: vec![clip], by: 4.0, to: None, copy: true }).unwrap());
    let state = graph.state().unwrap();
    let shown = &state["timeline"]["tracks"][0]["lanes"][0]["clips"];
    assert_eq!((shown[0]["start"].as_f64(), shown[1]["start"].as_f64()), (Some(0.0), Some(4.0)), "{shown}");
    assert_eq!(graph.inspect(|_| m2_script::builds()), builds, "no QuickJS run");
    assert_eq!(generation(&graph), first, "no new renderer: the lane was swapped in");
    assert!(synced(&graph));
    graph.undo(token).unwrap();
    assert_eq!(generation(&graph), first);
    assert!(synced(&graph));
    // Another session's gesture is refused.
    assert!(graph.clips(token + 1, Gesture::Create { lane: hits, start: 8.0, length: None }).is_err());
}

// WRITE-12, G6: every clip in the published timeline carries its `sha`; a gesture that names a
// clip with the `sha` it was drawn with goes ahead, and one drawn before an outside change is
// answered 409's words, naming the clip, and changes nothing.
#[test]
fn the_timeline_carries_each_clips_sha_and_a_stale_one_is_refused() {
    let fixture = Fixture::new();
    fixture.write(KICK);
    fixture.put(
        "data/kick/hits.json",
        "{\"clips\": [{\"id\": \"a\", \"start\": 0, \"length\": 4, \"notes\": []}, {\"start\": 4, \"length\": 4, \"notes\": []}]}\n",
    );
    let graph = fixture.open();
    let token = session(&graph);
    let clips = |graph: &Graph| graph.state().unwrap()["timeline"]["tracks"][0]["lanes"][0]["clips"].clone();
    let shown = clips(&graph);
    let line = "{\"id\": \"a\", \"start\": 0, \"length\": 4, \"notes\": []}";
    assert_eq!(shown[0]["sha"], json!(m2_editor::clips::sha(line)), "{shown}");
    assert_eq!(shown[1]["sha"].as_str().map(str::len), Some(16), "an anonymous clip has one too");
    let hits = LaneAddr::new("kick", "hits").unwrap();
    let seen = |sha: &Value| ClipRef { lane: hits.clone(), id: Some("a".into()), at: None, sha: sha.as_str().map(str::to_string) };
    done(graph.clips(token, Gesture::Move { clips: vec![seen(&shown[0]["sha"])], by: 8.0, to: None, copy: false }).unwrap());
    let moved = clips(&graph);
    assert_ne!(moved[1]["sha"], shown[0]["sha"], "the moved clip's line, and its sha, changed");

    let text = fixture.read("data/kick/hits.json").replacen("\"start\": 8", "\"start\": 12", 1);
    fixture.put("data/kick/hits.json", &text);
    let stale = || Gesture::Delete { clips: vec![seen(&moved[1]["sha"])] };
    // Not taken in yet: the commit finds the clip changed on disk (WRITE-3).
    match graph.clips(token, stale()).unwrap() {
        Answer::Changed(why) => assert!(why.contains("clip `a`") && why.contains("changed since"), "{why}"),
        Answer::Done(v) => panic!("deleted a clip changed since: {v}"),
    }
    // Taken in: the window's `sha` is stale.
    assert!(graph.poll());
    match graph.clips(token, stale()).unwrap() {
        Answer::Changed(why) => assert!(why.contains("clip `a`") && why.contains("changed in another window"), "{why}"),
        Answer::Done(v) => panic!("deleted a clip changed since: {v}"),
    }
    assert_eq!(fixture.read("data/kick/hits.json"), text);
}
