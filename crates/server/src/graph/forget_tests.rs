//! Track builders and monitoring mutes (Q7 of the 8b back ends): a track deleted with the builder
//! takes its mute and solo with it, and one made under a key a gone track's entry was kept for
//! starts heard. Entries of tracks that are only gone (commented out) stay.

use std::path::Path;

use serde_json::json;

use super::overlay::Change;
use super::tests::Fixture;

/// examples/first-song, as a project to add and delete tracks in.
fn first_song(fixture: &Fixture) {
    let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song");
    let mut stack = vec![from.clone()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                fixture.put(p.strip_prefix(&from).unwrap().to_str().unwrap(), &std::fs::read_to_string(&p).unwrap());
            }
        }
    }
}

#[test]
fn a_deleted_track_forgets_its_mute_and_a_new_one_starts_heard() {
    let fixture = Fixture::new();
    first_song(&fixture);
    // A mute kept for a track that isn't in the song now (MON-4: it would come back as it was).
    fixture.put(".music2/overlays.json", r#"{"mute": ["synth-pluck"], "solo": []}"#);
    let graph = fixture.open();
    let token = graph.state().unwrap()["session"].as_u64().unwrap();
    let change = Change { mute: [("hats".to_string(), true)].into(), solo: [("keys".to_string(), true)].into(), ..Change::default() };
    let now = graph.overlay(token, change).unwrap();
    assert_eq!((&now["mute"], &now["solo"]), (&json!(["hats", "synth-pluck"]), &json!(["keys"])));

    let (status, _) = graph.delete_track(token, "hats", false).unwrap();
    assert_eq!(status, 200);
    let o = &graph.state().unwrap()["overlays"];
    assert_eq!((&o["mute"], &o["solo"]), (&json!(["synth-pluck"]), &json!(["keys"])), "hats' mute went with it; the gone track's stays");

    let (status, _) = graph.delete_track(token, "keys", false).unwrap();
    assert_eq!(status, 200);
    assert_eq!(graph.state().unwrap()["overlays"]["solo"], json!([]), "a solo goes too");

    let added = graph.add_track(token, "@m2/synths:synth-pluck", None, None, None).unwrap();
    assert_eq!(added["module"], json!("synth-pluck"));
    let o = &graph.state().unwrap()["overlays"];
    assert_eq!((&o["mute"], &o["heard"]["synth-pluck"]), (&json!([]), &json!("heard")), "the new track isn't muted by the old entry");

    // Undone, the deleted track comes back heard: listening mutes are not undo steps.
    graph.undo(token).unwrap();
    graph.undo(token).unwrap();
    graph.undo(token).unwrap();
    let o = &graph.state().unwrap()["overlays"];
    assert_eq!((&o["mute"], &o["heard"]["hats"]), (&json!([]), &json!("heard")));
}
