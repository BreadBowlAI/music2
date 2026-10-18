use std::sync::atomic::AtomicU64;
use std::time::Duration;

use m2_play::{DeviceErrorKind, Driver};

use super::*;
use m2_editor::Gestures;

const MANIFEST: &str = "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.3\"\n";

pub(super) struct Fixture(pub(super) PathBuf);

impl Fixture {
    pub(super) fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!("m2-graph-server-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("music2.toml"), MANIFEST).unwrap();
        let fixture = Self(dir);
        fixture.write("export default song(() => osc.sine(hz(800)));\n");
        fixture
    }

    pub(super) fn write(&self, body: &str) {
        std::fs::write(self.0.join("song.ts"), format!("import {{ hz, song }} from '@m2/core';\nimport {{ osc }} from 'std/dsp';\n{body}"))
            .unwrap();
    }

    /// Write `text` to the project's file `path`, as an outside writer does.
    pub(super) fn put(&self, path: &str, text: &str) {
        let p = self.0.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    pub(super) fn read(&self, path: &str) -> String {
        std::fs::read_to_string(self.0.join(path)).unwrap()
    }

    pub(super) fn open(&self) -> Graph {
        let graph = Graph::silent();
        graph.open(&self.0).unwrap();
        graph
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// WRITE-20 (the step 6 design's 20.1): a project whose code doesn't compile opens, with nothing
// to show or play; why is the first diagnostic, placed in its file and line, and never names the
// project's folder.
#[test]
fn a_project_that_does_not_compile_says_where_without_its_folder() {
    let fixture = Fixture::new();
    fixture.write("export default song(() => osc.sine(hz(800)) oops);\n");
    let graph = Graph::silent();
    let state = graph.open(&fixture.0).unwrap();
    let first = &state["diagnostics"][0];
    assert_eq!((&first["file"], &first["line"]), (&json!("song.ts"), &json!(3)), "{state:#}");
    let said = first["message"].as_str().unwrap();
    assert!(!said.contains(&*fixture.0.canonicalize().unwrap().to_string_lossy()), "{said}");
    assert_eq!((&state["view"]["modules"], &state["timeline"]["tracks"]), (&json!([]), &json!([])));
    assert!(graph.inspect(|a| a.open.as_ref().unwrap().plays.is_none()), "nothing is loaded");
}

// WRITE-17, WRITE-18 (the step 6 design's 7.3): a project whose lock another program holds opens
// read-only: the state says why and who, and an edit is refused with WRITE-18's message, changing
// nothing; once the lock is let go, Retry makes the session a writer, and the edit goes ahead.
#[test]
fn a_project_another_program_holds_opens_read_only_until_retried() {
    let fixture = Fixture::new();
    fixture.write(KICK);
    let held = m2_session::lock::acquire(&fixture.0, "m2", m2_session::lock::RETRY).unwrap();
    let graph = fixture.open();
    let state = graph.state().unwrap();
    let ro = &state["read_only"];
    assert_eq!((&ro["why"], &ro["holder"]["program"], &ro["holder"]["pid"]), (&json!("held"), &json!("m2"), &json!(std::process::id())));
    let said = graph.read_only().unwrap();
    assert!(said.contains("nothing was changed") && said.contains("m2"), "{said}");
    let (token, revision) = (state["session"].as_u64().unwrap(), state["view"]["revision"].as_u64().unwrap());
    let song = fixture.read("song.ts");
    let e = graph.write_notes(token, revision, "kick", "hits", &kick_clips(&["C1"])).unwrap_err().to_string();
    assert!(e.contains("nothing was changed"), "{e}");
    assert!(!fixture.0.join("data/kick/hits.json").exists() && fixture.read("song.ts") == song, "nothing written");
    // Still held: Retry leaves it read-only.
    assert!(!graph.retry_lock(token).unwrap()["read_only"].is_null());
    drop(held);
    let state = graph.retry_lock(token).unwrap();
    assert!(state["read_only"].is_null() && graph.read_only().is_none(), "{state:#}");
    graph.write_notes(token, revision, "kick", "hits", &kick_clips(&["C1"])).unwrap();
    assert!(fixture.0.join("data/kick/hits.json").exists());
}

/// The open project's token, the accepted revision, and the id of the control whose value is
/// `value`.
pub(super) fn knob(graph: &Graph, value: f64) -> (u64, u64, String) {
    graph.inspect(move |a| {
        let o = a.open.as_ref().unwrap();
        let control = o.session.controls().into_iter().find(|c| c.value == value).unwrap();
        (o.token, o.session.accepted().number, control.id)
    })
}

fn control(graph: &Graph) -> (u64, u64, String) {
    knob(graph, 800.0)
}

/// What the player plays (0 while a preview plays), and the accepted revision.
pub(super) fn playing(graph: &Graph) -> (u64, u64) {
    graph.inspect(|a| a.open.as_ref().map(|o| (o.playing.unwrap_or(0), o.session.accepted().number)).unwrap())
}

pub(super) fn synced(graph: &Graph) -> bool {
    let (p, a) = playing(graph);
    p == a
}

pub(super) fn player(graph: &Graph) -> Player {
    graph.inner.current_device().expect("a player").player
}

// WRITE-22: a commit refused for a conflict, and an update the literal can't spell, end the
// gesture and put the code's value back.
#[test]
fn rejected_edits_and_previews_close_their_preview() {
    let fixture = Fixture::new();
    let graph = fixture.open();
    let (token, revision, id) = control(&graph);
    graph.preview(token, revision, &id, "drag", 900.0).unwrap();
    assert_eq!((graph.leases()[0].accepted, graph.leases()[0].value), (800.0, 900.0));
    assert!(graph.set(token, revision, &id, "drag", 950.0, Some(100.0)).is_err());
    assert!(graph.leases().is_empty());
    assert_eq!(control(&graph), (token, revision, id.clone()));

    graph.preview(token, revision, &id, "drag-2", 900.0).unwrap();
    assert!(graph.preview(token, revision, &id, "drag-2", f64::NAN).is_err());
    assert!(graph.leases().is_empty());
}

// WRITE-22: one gesture owns a control; another's preview, commit or cancel can't take it.
#[test]
fn cancellation_and_conflicts_respect_gesture_ownership() {
    let fixture = Fixture::new();
    let graph = fixture.open();
    let (token, revision, id) = control(&graph);
    graph.preview(token, revision, &id, "owner", 900.0).unwrap();
    graph.cancel_preview(token, &id, "old-gesture").unwrap();
    let e = graph.preview(token, revision, &id, "other", 1000.0).unwrap_err();
    assert!(e.to_string().contains("being edited by another gesture (for "), "{e}");
    assert!(graph.set(token, revision, &id, "other", 1000.0, Some(800.0)).is_err());
    assert_eq!(graph.leases().len(), 1);
    assert_eq!(graph.leases()[0].gesture.as_deref(), Some("owner"));
    graph.cancel_preview(token, &id, "owner").unwrap();
    assert!(graph.leases().is_empty());
    assert_eq!(control(&graph), (token, revision, id));
}

#[test]
fn old_project_requests_do_not_modify_the_new_project_or_its_preview() {
    let fixture = Fixture::new();
    let graph = fixture.open();
    let (old, revision, id) = control(&graph);
    graph.open(&fixture.0).unwrap();
    let (current, _, _) = control(&graph);
    assert!(current > old);
    graph.preview(current, revision, &id, "new", 900.0).unwrap();
    assert!(graph.preview(old, revision, &id, "old", 1000.0).is_err());
    assert!(graph.set(old, revision, &id, "old", 1000.0, Some(800.0)).is_err());
    graph.cancel_preview(old, &id, "new").unwrap();
    assert_eq!(graph.leases().len(), 1);
    assert_eq!(control(&graph), (current, revision, id));
}

#[test]
fn a_commit_carried_past_inserted_code_rebuilds_instead_of_patching_an_old_origin() {
    let fixture = Fixture::new();
    let graph = fixture.open();
    let (token, revision, id) = control(&graph);
    let old_origin = graph.inspect(|a| a.open.as_ref().unwrap().session.controls()[0].origin.clone());
    graph.preview(token, revision, &id, "drag", 900.0).unwrap();
    fixture.write("const first = hz(123);\nconst second = hz(456);\nexport default song(() => osc.sine(hz(800)));\n");
    // The inserted calls shift the control's position in the graph. The commit is carried past
    // them (its file's outside change rides along through the baseline, R2: one revision holds
    // both), and since the delta from what plays is more than values (WRITE-8), the player is
    // rebuilt rather than patched at the old origin.
    graph.set(token, revision, &id, "drag", 950.0, Some(800.0)).unwrap();
    let (playing, accepted) = playing(&graph);
    assert!(accepted > revision);
    assert_eq!(playing, accepted);
    assert!(graph.leases().is_empty());
    let current = graph.inspect(|a| a.open.as_ref().unwrap().session.controls().into_iter().find(|c| c.value == 950.0).unwrap());
    assert_ne!(current.origin, old_origin);
    let source = fixture.read("song.ts");
    assert!(source.contains("hz(123)") && source.contains("hz(456)") && source.contains("hz(950)"));
}

// WRITE-22: a save that doesn't touch the dragged call keeps the drag, and the commit writes it.
#[test]
fn an_unrelated_save_during_a_drag_keeps_the_drag_and_commits_it() {
    let fixture = Fixture::new();
    fixture.write("// notes\nexport default song(() => osc.sine(hz(800)));\n");
    let graph = fixture.open();
    let (token, revision, id) = control(&graph);
    graph.preview(token, revision, &id, "drag", 900.0).unwrap();
    fixture.write("// notes, edited elsewhere\nexport default song(() => osc.sine(hz(800)));\n");
    assert!(graph.poll());
    // The drag goes on, still named by the revision it started in.
    graph.preview(token, revision, &id, "drag", 920.0).unwrap();
    assert!(synced(&graph));
    let leases = graph.leases();
    assert_eq!((leases.len(), leases[0].accepted, leases[0].value), (1, 800.0, 920.0));
    // Another gesture can't take the carried control.
    assert!(graph.preview(token, revision + 1, &id, "other", 1000.0).is_err());
    let result = graph.set(token, revision, &id, "drag", 950.0, Some(800.0)).unwrap();
    assert_eq!(result["after"], "950");
    let source = fixture.read("song.ts");
    assert!(source.contains("// notes, edited elsewhere") && source.contains("hz(950)"));
    assert!(graph.leases().is_empty());
    assert!(synced(&graph));
}

// WRITE-22: a save that touches the dragged value ends the drag, saying why.
#[test]
fn a_save_that_touches_the_dragged_value_ends_the_drag() {
    let fixture = Fixture::new();
    let graph = fixture.open();
    let (token, revision, id) = control(&graph);
    graph.preview(token, revision, &id, "drag", 900.0).unwrap();
    fixture.write("export default song(() => osc.sine(hz(801)));\n");
    assert!(graph.poll());
    let e = graph.preview(token, revision, &id, "drag", 920.0).unwrap_err();
    assert!(e.to_string().contains("the code changed"), "{e}");
    assert!(graph.set(token, revision, &id, "drag", 950.0, Some(800.0)).is_err());
    assert!(fixture.read("song.ts").contains("hz(801)"));
    assert!(graph.leases().is_empty());
}

// WRITE-22: a refused edit changes no revision, so a drag goes on through it.
#[test]
fn a_refused_cable_edit_keeps_the_drag() {
    let fixture = Fixture::new();
    fixture.write("export default song(() => {\n  const tone = osc.saw(hz(110));\n  return osc.sine(hz(800));\n});\n");
    let graph = fixture.open();
    let (token, revision, id) = control(&graph);
    graph.preview(token, revision, &id, "drag", 900.0).unwrap();
    // An audio signal can't drive a frequency: refused.
    let from = "song.ts#0";
    assert!(graph.connect(token, revision, from, &id).is_err());
    assert_eq!(graph.leases().len(), 1);
    assert_eq!(playing(&graph), (revision, revision));
    graph.preview(token, revision, &id, "drag", 910.0).unwrap();
    assert!(graph.connect(token + 1, revision, from, &id).is_err(), "another session's request is refused");
}

#[test]
fn connecting_and_disconnecting_play_each_new_revision() {
    let fixture = Fixture::new();
    fixture.write(
        "import { lfo } from 'std/control';\nimport { xrange } from 'std/math';\nexport default song(({ time }) => {\n  const sweep = xrange(lfo(time.beats), hz(100), hz(200));\n  return osc.sine(hz(800));\n});\n",
    );
    let graph = fixture.open();
    let (token, revision, id) = control(&graph);
    let outlet = graph.inspect(|a| a.open.as_ref().unwrap().session.outlets()[0].id.clone());
    let connected = graph.connect(token, revision, &outlet, &id).unwrap();
    assert_eq!((connected["before"].as_str(), connected["after"].as_str()), (Some("hz(800)"), Some("sweep")));
    assert!(synced(&graph));
    let inlet = graph.inspect(|a| a.open.as_ref().unwrap().session.inlets()[0].id.clone());
    let revision = connected["revision"].as_u64().unwrap();
    let disconnected = graph.disconnect(token, revision, &inlet).unwrap();
    // std:sine declares `freq = 440`.
    assert_eq!(disconnected["after"], "hz(440)");
    assert!(synced(&graph));
    assert!(fixture.read("song.ts").contains("osc.sine(hz(440))"));
}

#[test]
fn undo_and_redo_play_each_revision_they_make() {
    let fixture = Fixture::new();
    let graph = fixture.open();
    let (token, revision, id) = control(&graph);
    assert_eq!(graph.state().unwrap()["can_undo"], false);
    graph.set(token, revision, &id, "drag", 950.0, Some(800.0)).unwrap();
    assert_eq!(graph.state().unwrap()["can_undo"], true);
    let undone = graph.undo(token).unwrap();
    assert_eq!((undone["before"].as_str(), undone["after"].as_str()), (Some("950"), Some("800")));
    let state = graph.state().unwrap();
    assert_eq!((&state["can_undo"], &state["can_redo"]), (&json!(false), &json!(true)));
    assert!(synced(&graph));
    assert_eq!(graph.inspect(|a| a.open.as_ref().unwrap().session.controls()[0].value), 800.0);
    graph.redo(token).unwrap();
    assert!(fixture.read("song.ts").contains("hz(950)"));
    assert!(graph.undo(token + 1).is_err(), "another session's request is refused");
}

// WRITE-4, WRITE-5, WRITE-11: an agent's Tx, as `/api/tx/*` takes it. A check writes nothing and
// answers with the delta's hash; a commit on that hash (`Seen`) is written, played and one undo
// step; the same Tx again is a conflict naming the file, in the editor's one JSON shape.
#[test]
fn an_agents_tx_is_checked_then_committed_on_the_hash_its_check_showed() {
    let fixture = Fixture::new();
    let graph = fixture.open();
    let (token, revision, _) = control(&graph);
    let song = fixture.read("song.ts");
    let at = song.find("800").unwrap();
    let edit = json!({"op": "text_edit", "file": "song.ts", "edits": [{"start": at, "end": at + 3, "expected": "800", "text": "900"}]});
    let anchor = json!({"written": {"file": "song.ts", "range": [at, at + 3]}});
    let base = json!({"song.ts": m2_song::Sha::of(song.as_bytes()).prefixed()});
    let tx = |expect: Value| json!({"id": "tx-agent", "base": base, "ops": [edit], "expect": expect});
    let (status, checked) = graph.tx_check(Some(token), &tx(json!({"values": [[anchor, 900.0]]}))).unwrap();
    assert_eq!(status, 200, "{checked}");
    let hash = checked["checked"]["hash"].as_str().unwrap().to_string();
    assert_eq!(checked["checked"]["delta"]["values"].as_array().map(Vec::len), Some(1), "{checked}");
    assert_eq!((fixture.read("song.ts"), playing(&graph)), (song.clone(), (revision, revision)), "a check writes nothing");

    let (status, committed) = graph.tx_commit(Some(token), &tx(json!({"seen": hash}))).unwrap();
    assert_eq!((status, committed["committed"]["hash"].as_str()), (200, Some(hash.as_str())), "{committed}");
    assert!(fixture.read("song.ts").contains("hz(900)") && synced(&graph));
    assert_eq!(knob(&graph, 900.0).1, revision + 1, "plays the revision it made");

    let (status, refused) = graph.tx_commit(Some(token), &tx(json!({"seen": hash}))).unwrap();
    assert_eq!((status, &refused["conflict"]["files"][0]["file"]), (409, &json!("song.ts")), "{refused}");
    graph.undo(token).unwrap();
    assert!(fixture.read("song.ts").contains("hz(800)") && synced(&graph));

    // The step 6 design's 1.2: a Tx that doesn't read is rejected (422) with its diagnostics, as
    // every refusal is answered, never a bare error.
    let mut extra = tx(json!("identical"));
    extra["extra"] = json!(1);
    let remove = json!([{"op": "remove", "file": "../x.json"}]);
    let outside = json!({"id": "tx-agent", "base": {"../x.json": null}, "ops": remove, "expect": "identical"});
    for (bad, code) in [(extra, "tx.invalid"), (outside, "tx.path")] {
        for answer in [graph.tx_check(Some(token), &bad).unwrap(), graph.tx_commit(Some(token), &bad).unwrap()] {
            assert_eq!((answer.0, &answer.1["rejected"]["diagnostics"][0]["code"]), (422, &json!(code)), "{}", answer.1);
        }
    }
    assert!(fixture.read("song.ts").contains("hz(800)"));
}

#[test]
fn effects_are_listed_added_and_removed_and_the_player_follows() {
    let fixture = Fixture::new();
    let graph = fixture.open();
    let state = graph.state().unwrap();
    let (token, revision) = (state["session"].as_u64().unwrap(), state["view"]["revision"].as_u64().unwrap());
    // PKG-2, PKG-11: listed from the descriptors, each by its ComponentId and where it is imported from.
    let delay = state["effects"].as_array().unwrap().iter().find(|e| e["component"] == "std:delay").cloned().unwrap();
    assert_eq!((&delay["name"], &delay["module"]), (&json!("delay"), &json!("std/fx")));
    assert!(delay["label"].as_str().is_some_and(|l| !l.is_empty()));
    let node = state["view"]["modules"][0]["nodes"].as_array().unwrap().iter().find(|n| n["audio"] == true).unwrap().clone();
    let call = node["id"].as_str().unwrap();
    let added = graph.add_effect(token, revision, call, "std:delay").unwrap();
    assert_eq!(added["after"], "delay(osc.sine(hz(800)), { time: 0.25, feedback: 0.3, mix: 0.3 })");
    let state = graph.state().unwrap();
    // The node is the effect by its component, named by its label.
    let delay = state["view"]["modules"][0]["nodes"].as_array().unwrap().iter().find(|n| n["component"] == "std:delay").unwrap().clone();
    assert_eq!(delay["effect"], state["effects"].as_array().unwrap().iter().find(|e| e["component"] == "std:delay").unwrap()["label"]);
    assert!(synced(&graph));
    let revision = state["view"]["revision"].as_u64().unwrap();
    graph.remove_effect(token, revision, delay["id"].as_str().unwrap()).unwrap();
    assert!(fixture.read("song.ts").contains("song(() => osc.sine(hz(800)))"));
}

#[test]
fn a_lane_write_plays_the_new_notes_and_undo_takes_them_back() {
    let fixture = Fixture::new();
    fixture.write(
        "import { track } from '@m2/core';\nimport { kick } from 'std/instruments';\n\
         const k = track('kick', ({ lane }) => kick(lane.notes('hits')));\nexport default song(() => k.out);\n",
    );
    let graph = fixture.open();
    let state = graph.state().unwrap();
    let (token, revision) = (state["session"].as_u64().unwrap(), state["view"]["revision"].as_u64().unwrap());
    let clips: Vec<NoteClip> =
        serde_json::from_value(json!([{"start": 0, "length": 4, "notes": [{"pitch": "C1", "start": 0, "length": 0.25}]}])).unwrap();
    let written = graph.write_notes(token, revision, "kick", "hits", &clips).unwrap();
    assert_eq!((written["file"].as_str(), written["after"].as_str()), (Some("data/kick/hits.json"), Some("clip at bar 1 (1 bar)")));
    let state = graph.state().unwrap();
    assert_eq!(state["timeline"]["tracks"][0]["lanes"][0]["clips"][0]["notes"][0]["pitch"], 24.0);
    assert!(synced(&graph), "the player plays the new notes");

    assert!(graph.write_notes(token, revision + 1, "kick", "nope", &clips).is_err());
    assert!(graph.write_notes(token + 1, revision + 1, "kick", "hits", &[]).is_err(), "another session's request is refused");
    graph.undo(token).unwrap();
    assert!(!fixture.0.join("data/kick/hits.json").exists(), "undo removes the file the write created");
    assert!(synced(&graph));
}

// The G11 fix's A.8: the timeline published after each change is the session's timeline, to the
// value, though each lane's JSON is made only when that lane changed: a write to one lane, an
// outside write to another, a rename of a track's label, and an undo.
#[test]
fn the_published_timeline_is_the_sessions_and_shares_what_didnt_change() {
    let fixture = Fixture::new();
    fixture.write(
        "import { track } from '@m2/core';\nimport { kick } from 'std/instruments';\nimport { sum } from 'std/math';\n\
         const k = track('kick', ({ lane }) => kick(lane.notes('hits')));\n\
         const s = track('snare', ({ lane }) => kick(lane.notes('hits')));\nexport default song(() => sum(k.out, s.out));\n",
    );
    fixture.put("data/snare/hits.json", "{\"clips\": [{\"id\": \"s\", \"start\": 0, \"length\": 4, \"notes\": []}]}\n");
    let graph = fixture.open();
    let same = |graph: &Graph| {
        let session = graph.inspect(|a| serde_json::to_value(a.open.as_ref().unwrap().session.timeline()).unwrap());
        // The published one also gives each clip its hash (8b.5).
        let mut published = graph.state().unwrap()["timeline"].clone();
        for c in published["tracks"].as_array_mut().unwrap().iter_mut().flat_map(|t| t["lanes"].as_array_mut().unwrap()) {
            c["clips"].as_array_mut().unwrap().iter_mut().for_each(|c| drop(c.as_object_mut().unwrap().remove("sha")));
        }
        assert_eq!(published, session, "the published timeline is the session's");
    };
    let lane = |graph: &Graph, module: &str| {
        let cp = graph.inner.control.lock();
        cp.open.as_ref().unwrap().shown.timeline.lane(module, "hits").map(|l| l.json.clone()).unwrap()
    };
    same(&graph);
    let state = graph.state().unwrap();
    let (token, revision) = (state["session"].as_u64().unwrap(), state["view"]["revision"].as_u64().unwrap());
    let snare = lane(&graph, "snare");
    let clips: Vec<NoteClip> =
        serde_json::from_value(json!([{"id": "k", "start": 0, "length": 4, "notes": [{"pitch": "C1", "start": 0, "length": 0.25}]}])).unwrap();
    graph.write_notes(token, revision, "kick", "hits", &clips).unwrap();
    same(&graph);
    assert!(Arc::ptr_eq(&snare, &lane(&graph, "snare")), "the lane not written is the same JSON");
    let kick = lane(&graph, "kick");

    fixture.put("data/snare/hits.json", "{\"clips\": [{\"id\": \"s\", \"start\": 8, \"length\": 4, \"notes\": []}]}\n");
    assert!(graph.poll());
    same(&graph);
    assert!(!Arc::ptr_eq(&snare, &lane(&graph, "snare")), "the lane written outside is made again");
    assert!(Arc::ptr_eq(&kick, &lane(&graph, "kick")));
    assert_eq!(graph.state().unwrap()["timeline"]["tracks"][1]["lanes"][0]["clips"][0]["start"], 8.0);

    graph.label(token, "kick", "Big Kick").unwrap();
    same(&graph);
    assert_eq!(graph.state().unwrap()["timeline"]["tracks"][0]["label"], "Big Kick");
    graph.undo(token).unwrap();
    graph.undo(token).unwrap();
    same(&graph);
}

// ID-10, HOME-2: the label and automate routes: a rename changes only what the timeline and the
// canvas show; an automate answers with the new lane, which the timeline shows empty at once, and
// the player plays each revision; one undo each takes them back.
#[test]
fn a_track_renamed_and_a_knob_automated_play_and_undo() {
    let fixture = Fixture::new();
    fixture.write(
        "import { track } from '@m2/core';\n\
         const t = track('tone', () => osc.sine(hz(800)));\nexport default song(() => t.out);\n",
    );
    let graph = fixture.open();
    let (token, revision, id) = control(&graph);
    let tracks = |graph: &Graph| graph.state().unwrap()["timeline"]["tracks"][0].clone();
    let renamed = graph.label(token, "tone", "Lead Tone").unwrap();
    assert_eq!((&renamed["before"], &renamed["after"], &renamed["file"]), (&json!("tone"), &json!("Lead Tone"), &json!("song.ts")));
    assert_eq!(tracks(&graph)["label"], "Lead Tone");
    assert_eq!(graph.state().unwrap()["view"]["modules"][1]["label"], "Lead Tone");
    assert!(graph.label(token + 1, "tone", "Nope").is_err(), "another session's request is refused");
    assert!(graph.label(token, "nope", "Nope").is_err());

    let revision = revision + 1;
    let automated = graph.automate(token, revision, &id).unwrap();
    assert_eq!((&automated["lane"], &automated["name"]), (&json!({"module": "tone", "lane": "freq"}), &json!("freq")));
    let written = "track('tone', { name: \"Lead Tone\" }, ({ lane }) => osc.sine(lane.control(\"freq\", \"hz\", hz(800))))";
    assert!(fixture.read("song.ts").contains(written));
    let lane = &tracks(&graph)["lanes"][0];
    let shown = (&lane["kind"], &lane["key"], &lane["default"], &lane["clips"]);
    assert_eq!(shown, (&json!("control"), &json!("freq"), &json!(800.0), &json!([])));
    assert!(synced(&graph), "the player plays the automated song");
    assert!(graph.automate(token, revision, &id).is_err(), "the knob is a lane now");

    graph.undo(token).unwrap();
    assert!(!fixture.0.join("data/tone/freq.json").exists() && synced(&graph));
    graph.undo(token).unwrap();
    assert_eq!(tracks(&graph)["label"], "tone");
    assert!(fixture.read("song.ts").contains("track('tone', () => osc.sine(hz(800)))"));
}

/// A Kick track renamed Kicks in its code, with its notes left under the old name.
fn renamed_kick(fixture: &Fixture) {
    fixture.write(
        "import { track } from '@m2/core';\nimport { kick } from 'std/instruments';\n\
         const k = track('kicks', ({ lane }) => kick(lane.notes('hits')));\nexport default song(() => k.out);\n",
    );
    std::fs::create_dir_all(fixture.0.join("data/kick")).unwrap();
    let notes = r#"{"clips": [{"id": "a", "start": 0, "length": 4, "notes": [{"pitch": "C1", "start": 0, "length": 1}]}]}"#;
    std::fs::write(fixture.0.join("data/kick/hits.json"), notes).unwrap();
}

// DIAG-1, DIAG-4, DIAG-6: data a rename left behind is a problem in the state and every
// graph event, with its fixes; applying one plays the data, and undo takes it back.
#[test]
fn data_a_rename_left_behind_is_a_problem_whose_fixes_play_and_undo() {
    let fixture = Fixture::new();
    renamed_kick(&fixture);
    let graph = fixture.open();
    let state = graph.state().unwrap();
    let (token, revision) = (state["session"].as_u64().unwrap(), state["view"]["revision"].as_u64().unwrap());
    let problem = &state["problems"][0];
    assert_eq!(
        (&problem["severity"], &problem["code"], &problem["src"], &problem["module"]),
        (&json!("warning"), &json!("data.unclaimed"), &json!("data/kick"), &json!("kick"))
    );
    assert!(problem["message"].as_str().unwrap().starts_with("Data filed under kick isn't playing."), "{problem}");
    let relink = problem["fixes"][0].clone();
    assert_eq!((&relink["code"], &relink["likely"]), (&json!("relink"), &json!(true)));
    // DIAG-1: the fix is a ready Tx, its JSON as `m2 tx` and agents read it.
    assert_eq!(relink["tx"]["ops"], json!([{"op": "rename", "from": "data/kick", "to": "data/kicks"}]));
    let news = graph.news();
    assert_eq!((&news["session"], &news["revision"], &news["problems"]), (&json!(token), &json!(revision), &state["problems"]));

    let clips = |graph: &Graph| graph.state().unwrap()["timeline"]["tracks"][0]["lanes"][0]["clips"].as_array().unwrap().len();
    assert!(graph.fix(token + 1, &relink).is_err(), "another session's request is refused");
    let moved = graph.fix(token, &relink).unwrap();
    assert_eq!((&moved["before"], &moved["after"]), (&json!("unused data"), &json!("kicks")));
    assert!(fixture.0.join("data/kicks/hits.json").exists() && !fixture.0.join("data/kick").exists());
    assert_eq!((graph.state().unwrap()["problems"].clone(), clips(&graph)), (json!([]), 1), "the track plays its notes");
    assert_eq!(graph.news()["problems"], json!([]));
    assert!(synced(&graph));
    assert!(graph.fix(token, &relink).is_err(), "its base is stale: a conflict");

    graph.undo(token).unwrap();
    assert!(fixture.0.join("data/kick/hits.json").exists() && synced(&graph));
    assert_eq!((graph.news()["problems"][0]["src"].clone(), clips(&graph)), (json!("data/kick"), 0));
    let revision = graph.state().unwrap()["view"]["revision"].as_u64().unwrap();
    let deleted = graph.delete_data(token, revision, "data/kick").unwrap();
    assert_eq!(deleted["after"], "deleted");
    assert!(!fixture.0.join("data/kick/hits.json").exists() && graph.news()["problems"] == json!([]));
    graph.undo(token).unwrap();
    assert!(fixture.0.join("data/kick/hits.json").exists() && synced(&graph));
    assert!(graph.delete_data(token, revision, "song.ts").is_err(), "only data no lane reads");
    // A lane file on its own goes to a lane, by its path.
    let revision = graph.state().unwrap()["view"]["revision"].as_u64().unwrap();
    graph.relink(token, revision, "data/kick/hits.json", "data/kicks/hits.json").unwrap();
    assert_eq!(clips(&graph), 1);
    graph.close();
    assert_eq!(graph.news(), json!({}));
}

#[test]
fn a_saved_file_is_undone_like_an_edit_made_here() {
    let fixture = Fixture::new();
    let graph = fixture.open();
    let (token, revision, id) = control(&graph);
    graph.set(token, revision, &id, "drag", 900.0, Some(800.0)).unwrap();
    let base = fixture.read("song.ts");
    let saved = base.replace("// ", "").replacen('\n', "\n// a comment\n", 1);
    let state = graph.write_file(Some(token), "song.ts", &base, &saved).unwrap();
    assert_eq!(state["can_undo"], true);
    let undone = graph.undo(token).unwrap();
    assert_eq!((undone["before"].as_str(), undone["after"].as_str()), (Some("// a comment"), Some("nothing")));
    assert_eq!(fixture.read("song.ts"), base);
    // Then the knob, underneath it.
    let undone = graph.undo(token).unwrap();
    assert_eq!((undone["before"].as_str(), undone["after"].as_str()), (Some("900"), Some("800")));
    let state = graph.state().unwrap();
    assert_eq!((&state["can_undo"], &state["can_redo"]), (&json!(false), &json!(true)));
}

#[test]
fn the_code_editors_unsaved_files_belong_to_the_project_it_said_them_for() {
    let fixture = Fixture::new();
    let graph = fixture.open();
    let (token, ..) = control(&graph);
    assert!(graph.unsaved().is_empty());
    graph.set_unsaved(token, vec!["song.ts".into()]).unwrap();
    assert_eq!(graph.unsaved(), ["song.ts"]);
    assert!(graph.set_unsaved(token + 1, vec![]).is_err(), "another project's editor is refused");
    // Opened again, it is a new project: its editor starts with nothing unsaved.
    graph.open(&fixture.0).unwrap();
    assert!(graph.unsaved().is_empty());
    graph.close();
    assert!(graph.unsaved().is_empty());
}

// WRITE-22: a draft that doesn't compile makes no revision, so the drag goes on through it; a
// renderer behind the accepted revision is still brought up to it by the next poll.
#[test]
fn a_broken_draft_keeps_the_drag_and_poll_syncs_already_refreshed_sources() {
    let fixture = Fixture::new();
    let graph = fixture.open();
    let (token, revision, id) = control(&graph);
    graph.preview(token, revision, &id, "drag", 900.0).unwrap();
    fixture.write("(");
    assert!(graph.poll());
    assert!(graph.inspect(|a| !a.open.as_ref().unwrap().session.draft_errors.is_empty()));
    assert_eq!(graph.leases().len(), 1);
    assert_eq!(playing(&graph), (revision, revision));
    graph.preview(token, revision, &id, "drag", 910.0).unwrap();
    fixture.write("export default song(() => osc.sine(hz(900)));\n");
    graph.inspect(|a| {
        let o = a.open.as_mut().unwrap();
        assert!(o.session.refresh());
        assert!(!o.session.refresh());
        assert_ne!(o.playing, Some(o.session.accepted().number));
    });
    assert!(graph.poll(), "a renderer behind the accepted revision still needs synchronization");
    assert!(synced(&graph));
    // The new code rewrote the dragged literal: that ended the drag.
    assert!(graph.leases().is_empty());
}

// RENDER-9, RENDER-29: going back to the start is a seek, not a rebuild: a drag goes on.
#[test]
fn a_commit_closes_previews_and_a_restart_keeps_them() {
    let fixture = Fixture::new();
    let graph = fixture.open();
    let (token, revision, id) = control(&graph);
    graph.preview(token, revision, &id, "drag", 900.0).unwrap();
    let result = graph.set(token, revision, &id, "drag", 950.0, Some(800.0)).unwrap();
    assert!(graph.leases().is_empty());
    assert_eq!(playing(&graph).0, result["revision"].as_u64().unwrap());
    assert_eq!(graph.inspect(|a| a.open.as_ref().unwrap().session.controls()[0].value), 950.0);
    let revision = result["revision"].as_u64().unwrap();
    graph.preview(token, revision, &id, "next", 1000.0).unwrap();
    graph.transport("restart").unwrap();
    let leases = graph.leases();
    assert_eq!((leases.len(), leases[0].gesture.as_deref()), (1, Some("next")));
    assert_eq!(playing(&graph).0, revision);
    assert_eq!(graph.inspect(|a| a.open.as_ref().unwrap().session.controls()[0].value), 950.0);
}

/// Wait for the player's thread to catch up.
pub(super) fn eventually(what: &str, done: impl Fn() -> bool) {
    let start = Instant::now();
    while !done() {
        assert!(start.elapsed() < Duration::from_secs(5), "timed out waiting until {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn seeking_keeps_playing_or_stopped_and_status_counts_beats_in_the_tempo() {
    let fixture = Fixture::new();
    std::fs::create_dir_all(fixture.0.join("data")).unwrap();
    std::fs::write(fixture.0.join("data/arrangement.json"), r#"{"tempo": [{"beat": 0, "bpm": 120}, {"beat": 4, "bpm": 60}]}"#).unwrap();
    let graph = fixture.open();
    let state = graph.state().unwrap();
    assert_eq!(state["timeline"]["tempo"], json!([{"beat": 0.0, "bpm": 120.0}, {"beat": 4.0, "bpm": 60.0}]));
    assert_eq!((&state["timeline"]["end"], &state["beat"]), (&json!(4.0), &json!(0.0)));

    // Stopped, the position moves at once and stays there, with the tempo there.
    let stopped = |seconds, beat, bpm| Status { playing: false, seconds, beat, bpm: Some(bpm), looping: None };
    graph.seek(2.0).unwrap();
    assert_eq!(graph.status(), stopped(1.0, 2.0, 120.0), "at 120 bpm, 1 second is 2 beats");
    graph.seek(8.0).unwrap();
    std::thread::sleep(Duration::from_millis(30));
    assert_eq!(graph.status(), stopped(6.0, 8.0, 60.0), "then 4 beats at 60 bpm");
    graph.seek(-3.0).unwrap();
    assert_eq!(graph.status().beat, 0.0);
    assert!(graph.seek(f64::NAN).is_err());

    // Playing, it goes on playing from there.
    graph.transport("play").unwrap();
    eventually("it plays", || graph.status().playing);
    graph.seek(4.0).unwrap();
    eventually("it plays on from beat 4", || graph.status().beat > 4.0);
    assert!(graph.status().playing);
    graph.transport("stop").unwrap();
    eventually("it stops", || !graph.status().playing);
    graph.seek(1.0).unwrap();
    std::thread::sleep(Duration::from_millis(30));
    assert_eq!(graph.status(), stopped(0.5, 1.0, 120.0));

    // RENDER-8: a seek replays nothing, so one far past the song lands there at once.
    let t = Instant::now();
    graph.seek(4.0 + 2000.0).unwrap();
    eventually("it lands far past the song", || graph.status() == stopped(2002.0, 2004.0, 60.0));
    assert!(t.elapsed() < Duration::from_secs(1), "{:?}", t.elapsed());

    let closed = Graph::silent();
    assert!(closed.seek(1.0).is_err(), "no project is open");
    assert_eq!(closed.status().beat, 0.0);
    graph.close();
    assert_eq!(graph.status().beat, 0.0);
}

// RENDER-31: the live engine runs at the project's sample_rate, whatever the device's (the
// silent player's is 48 kHz): positions count at the project's rate, and the transport too.
#[test]
fn the_engine_runs_at_the_projects_rate() {
    let fixture = Fixture::new();
    std::fs::write(fixture.0.join("music2.toml"), MANIFEST.replace("48000", "44100")).unwrap();
    let graph = fixture.open();
    assert_eq!(graph.state().unwrap()["sample_rate"], 44_100);
    graph.seek(2.0).unwrap();
    let p = player(&graph);
    eventually("the seek lands", || p.status().pending.is_none());
    assert_eq!((p.device_rate, p.rate(), p.status().pos), (48_000, 44_100, 44_100));
    let at = Status { playing: false, seconds: 1.0, beat: 2.0, bpm: Some(120.0), looping: None };
    assert_eq!(graph.status(), at, "at 120 bpm, 2 beats are 1 second");
    // A manifest edit to the rate plays on from the same beat, in a fresh engine at the new rate.
    std::fs::write(fixture.0.join("music2.toml"), MANIFEST.replace("48000", "96000")).unwrap();
    assert!(graph.poll());
    assert_eq!(graph.status(), at);
    let p = player(&graph);
    eventually("the new engine plays", || p.status().pending.is_none());
    assert_eq!((p.rate(), p.status().pos), (96_000, 96_000));
    assert_eq!(graph.status(), at);
}

// RENDER-9: a seek is a jump, never a replay: past a kernel that makes NaNs, it lands at once.
#[test]
fn seeking_past_a_nan_lands_at_once() {
    let fixture = Fixture::new();
    std::fs::create_dir_all(fixture.0.join("data/song")).unwrap();
    // 1e39 overflows to infinity: a kernel produces non-finite samples from beat 8.
    std::fs::write(
        fixture.0.join("data/song/g.json"),
        r#"{"unit": "gain", "clips": [{"id": "a", "start": 8, "length": 8, "points": [[0, 1e39]]}]}"#,
    )
    .unwrap();
    fixture.write(
        "import { vca } from 'std/dsp';\nexport default song(({ lane }) => vca(osc.sine(hz(440)), lane.control(\"g\", \"gain\", 1)));\n",
    );
    let graph = fixture.open();
    graph.seek(12.0).unwrap();
    assert_eq!(graph.status().beat, 12.0);
    graph.transport("play").unwrap();
    eventually("it plays on from there", || graph.status().beat > 12.0);
}

// RENDER-29: the status answers while the session is busy, as during a compile.
#[test]
fn status_does_not_wait_for_the_session() {
    let fixture = Fixture::new();
    let graph = &fixture.open();
    graph.seek(2.0).unwrap();
    let held = graph.hold();
    let (tx, rx) = mpsc::channel();
    std::thread::scope(|s| {
        s.spawn(move || tx.send(graph.status()).unwrap());
        let status = rx.recv_timeout(Duration::from_secs(2)).expect("status answers while the session is busy");
        assert_eq!(status.beat, 2.0);
        drop(held);
    });
}

// RENDER-9, RENDER-29: a seek moves the transport and nothing else: the drag goes on, and the
// revision the session took in meanwhile is played by the next poll, not by the seek.
#[test]
fn seeking_keeps_previews_and_leaves_playing_new_revisions_to_poll() {
    let fixture = Fixture::new();
    let graph = fixture.open();
    let (token, revision, id) = control(&graph);
    graph.preview(token, revision, &id, "drag", 900.0).unwrap();
    fixture.write("export default song(() => osc.sine(hz(700)));\n");
    graph.inspect(|a| {
        let o = a.open.as_mut().unwrap();
        assert!(o.session.refresh());
        assert_ne!(o.playing, Some(o.session.accepted().number));
    });
    graph.seek(4.0).unwrap();
    assert_eq!((graph.leases().len(), playing(&graph).0), (1, revision));
    assert!(graph.poll());
    assert!(graph.leases().is_empty(), "the new code rewrote the dragged value");
    assert!(synced(&graph));
    assert_eq!(graph.status().beat, 4.0, "where the seek put it");
}

#[test]
fn returning_a_drag_to_its_start_closes_the_preview_without_rewriting_source() {
    let fixture = Fixture::new();
    fixture.write("export default song(() => osc.sine(hz(8_00)));\n");
    let graph = fixture.open();
    let (token, revision, id) = control(&graph);
    let source = fixture.read("song.ts");
    graph.preview(token, revision, &id, "drag", 950.0).unwrap();
    let result = graph.set(token, revision, &id, "drag", 800.0, Some(800.0)).unwrap();
    assert_eq!(result["revision"].as_u64().unwrap(), revision);
    assert_eq!(fixture.read("song.ts"), source);
    assert!(graph.leases().is_empty());
    assert_eq!(playing(&graph).0, revision);
    assert_eq!(graph.inspect(|a| a.open.as_ref().unwrap().session.controls()[0].value), 800.0);
}

#[test]
fn a_saved_file_plays_at_once_and_a_save_over_a_change_is_refused() {
    let fixture = Fixture::new();
    let graph = fixture.open();
    let (token, revision, _) = control(&graph);
    let base = fixture.read("song.ts");
    let saved = base.replace("800", "900");
    let state = graph.write_file(Some(token), "song.ts", &base, &saved).unwrap();
    assert_eq!((&state["view"]["revision"], &state["diagnostics"]), (&json!(revision + 1), &json!([])));
    assert_eq!(playing(&graph), (revision + 1, revision + 1), "adopted and playing without waiting for the watcher");
    assert!(!graph.poll());

    // Saved from the old text: refused with what the file says now, and nothing written.
    let e = graph.write_file(Some(token), "song.ts", &base, "export default 1;\n").unwrap_err();
    assert_eq!(e.downcast_ref::<files::Conflict>().unwrap().current, saved);
    assert_eq!(fixture.read("song.ts"), saved);

    // Code that doesn't compile is saved; the last revision that did plays on.
    let state = graph.write_file(Some(token), "song.ts", &saved, "export default song(() => (\n").unwrap();
    assert_eq!(state["view"]["revision"], revision + 1);
    assert_eq!((&state["diagnostics"][0]["file"], &state["diagnostics"][0]["line"]), (&json!("song.ts"), &json!(2)));
    assert_eq!(playing(&graph), (revision + 1, revision + 1));
    assert!(fixture.read("song.ts").ends_with("song(() => (\n"));

    assert!(graph.write_file(Some(token + 1), "song.ts", "", "").is_err(), "another session's editor is refused");
    assert!(graph.write_file(Some(token), "../song.ts", "", "").is_err());
    graph.close();
    assert!(graph.files().is_err() && graph.write_file(None, "song.ts", "", "").is_err());
}

/// A kick track whose decay is a knob at 0.3 s.
pub(super) const KICK: &str = "import { secs, track } from '@m2/core';\nimport { kick } from 'std/instruments';\n\
                    const k = track('kick', ({ lane }) => kick(lane.notes('hits'), { decay: secs(0.3) }));\n\
                    export default song(() => k.out);\n";

pub(super) fn kick_clips(pitches: &[&str]) -> Vec<NoteClip> {
    let notes: Vec<Value> = pitches.iter().enumerate().map(|(i, p)| json!({"pitch": p, "start": i, "length": 0.25})).collect();
    serde_json::from_value(json!([{"id": "a", "start": 0, "length": 4, "notes": notes}])).unwrap()
}

/// The renderer the player plays, once it has taken everything sent.
pub(super) fn generation(graph: &Graph) -> u64 {
    let p = player(graph);
    eventually("the player takes what was sent", || p.status().pending.is_none());
    std::thread::sleep(Duration::from_millis(30));
    p.status().generation
}

// RENDER-26, PLANE-1, WRITE-22: a lane write is swapped into the playing renderer, whoever wrote
// it: the same renderer plays on, and a drag goes on. Undo, and a lane write after a knob commit,
// take the same way.
#[test]
fn a_lane_write_while_playing_is_swapped_into_the_playing_renderer() {
    let fixture = Fixture::new();
    fixture.write(KICK);
    let graph = fixture.open();
    graph.transport("play").unwrap();
    eventually("it plays", || graph.status().playing);
    let first = generation(&graph);
    let (token, revision, id) = knob(&graph, 0.3);
    graph.preview(token, revision, &id, "drag", 0.5).unwrap();
    // Written by an agent, mid-drag.
    let hits = r#"{"clips": [{"id": "a", "start": 0, "length": 4, "notes": [{"pitch": "C1", "start": 0, "length": 1}]}]}"#;
    fixture.put("data/kick/hits.json", hits);
    assert!(graph.poll());
    let (playing_now, after) = playing(&graph);
    assert!(after > revision && playing_now == after);
    let leases = graph.leases();
    assert_eq!((leases.len(), leases[0].revision), (1, after), "the drag goes on, in the new revision");
    assert_eq!(generation(&graph), first, "no new renderer");
    assert_eq!(graph.leases()[0].generation, Some(first), "its updates still go to it");
    assert!(graph.status().playing);
    graph.write_notes(token, after, "kick", "hits", &kick_clips(&["C1", "D1"])).unwrap();
    assert_eq!(generation(&graph), first, "the piano roll's write swaps too");

    // A knob commit leaves the renderer playing the committed graph: a lane edit still swaps.
    let after = playing(&graph).1;
    graph.preview(token, after, &id, "drag", 0.5).unwrap();
    graph.set(token, after, &id, "drag", 0.5, Some(0.3)).unwrap();
    let revision = graph.state().unwrap()["view"]["revision"].as_u64().unwrap();
    graph.write_notes(token, revision, "kick", "hits", &kick_clips(&["C1"])).unwrap();
    graph.undo(token).unwrap();
    assert_eq!(generation(&graph), first, "still no new renderer");
    assert!(synced(&graph));
    assert!(fixture.read("song.ts").contains("secs(0.5)"));
}

// Step 3's gates time an edit or a seek from when it was asked for (or the session was ready to
// tell the engine) to the callback that took it: what the server records says which message
// carried it, and the engine's stamp says when it applied. PLANE-5: a lane write's message is its
// preview, sent once its checks passed and before it is durable (`sent ≤ compiled`).
#[test]
fn edits_and_seeks_say_when_the_engine_took_them() {
    let fixture = Fixture::new();
    fixture.write(KICK);
    let graph = fixture.open();
    graph.transport("play").unwrap();
    let state = graph.state().unwrap();
    let (token, revision) = (state["session"].as_u64().unwrap(), state["view"]["revision"].as_u64().unwrap());
    let taken = |t: EditTiming| {
        let seq = t.seq.expect("a message was sent");
        eventually("the engine takes it", || graph.applied(seq).is_some());
        let a = graph.applied(seq).unwrap();
        assert!(t.requested_ns <= t.ready_ns && t.ready_ns <= t.sent_ns && t.sent_ns <= t.published_ns, "{t:?}");
        assert!(t.ready_ns <= t.compiled_ns && t.compiled_ns <= t.published_ns, "{t:?}");
        assert!(!t.previewed || t.sent_ns <= t.compiled_ns, "{t:?}");
        // The callback that took it started after it was queued, or was already running.
        assert!(t.sent_ns <= a.callback_ns + 20_000_000, "{t:?} then {a:?}");
        assert!(a.callback_ns <= graph.now_ns());
    };
    graph.write_notes(token, revision, "kick", "hits", &kick_clips(&["C1", "D1"])).unwrap();
    let t = graph.last_edit().unwrap();
    assert!(t.previewed && t.taken_back.is_none(), "{t:?}");
    taken(t);
    graph.seek(2.0).unwrap();
    let seek = graph.last_edit().unwrap();
    assert_eq!((seek.requested_ns, seek.ready_ns, seek.previewed), (seek.compiled_ns, seek.compiled_ns, false));
    taken(seek);
    // An edit that changes nothing sends nothing.
    let revision = graph.state().unwrap()["view"]["revision"].as_u64().unwrap();
    graph.write_notes(token, revision, "kick", "hits", &kick_clips(&["C1", "D1"])).unwrap();
    assert_eq!(graph.last_edit().unwrap().seq, None);
}

// RENDER-26, PLANE-1: an automation lane's file, written by hand, is swapped in too.
#[test]
fn a_control_lane_written_by_hand_is_swapped_into_the_playing_renderer() {
    let fixture = Fixture::new();
    let gain = |v: f64| format!(r#"{{"unit": "gain", "clips": [{{"id": "a", "start": 0, "length": 8, "points": [[0, {v}]]}}]}}"#);
    fixture.put("data/song/g.json", &gain(0.5));
    fixture.write(
        "import { vca } from 'std/dsp';\nexport default song(({ lane }) => vca(osc.sine(hz(440)), lane.control(\"g\", \"gain\", 1)));\n",
    );
    let graph = fixture.open();
    graph.transport("play").unwrap();
    let first = generation(&graph);
    fixture.put("data/song/g.json", &gain(0.25));
    assert!(graph.poll());
    assert_eq!(generation(&graph), first, "no new renderer");
    assert!(synced(&graph));
    assert!(graph.inspect(|a| {
        let o = a.open.as_ref().unwrap();
        o.plays.as_ref().unwrap().revision.compiled.data.control("song", "g") == o.session.accepted().compiled.data.control("song", "g")
    }));
}

/// Poll the watcher as the server does, every 50 ms, until `done` or `ms` pass; after each poll,
/// `after` sees the graph.
pub(super) fn watching(graph: &Graph, ms: u64, mut after: impl FnMut(&Graph) -> bool) {
    let t = Instant::now();
    while t.elapsed() < Duration::from_millis(ms) {
        graph.watch();
        if after(graph) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The accepted revision's number, code and data revisions.
pub(super) fn revs(graph: &Graph) -> (u64, u64, u64) {
    graph.inspect(|a| {
        let r = a.open.as_ref().unwrap().session.accepted();
        (r.number, r.code_rev, r.data_rev)
    })
}

// PLANE-1, RENDER-26, the step 6 gate: a lane file an outside writer changes is taken in by the
// watcher once it settles, without running QuickJS: the code revision stays, the data revision
// moves, and the playing renderer takes it as a lane swap.
#[test]
fn an_outside_lane_write_is_swapped() {
    let fixture = Fixture::new();
    fixture.write(KICK);
    let graph = fixture.open();
    graph.transport("play").unwrap();
    let first = generation(&graph);
    // QuickJS runs are counted per thread: the session's are the actor's.
    let builds = graph.inspect(|_| m2_script::builds());
    let (number, code_rev, data_rev) = revs(&graph);
    let hits = r#"{"clips": [{"id": "a", "start": 0, "length": 4, "notes": [{"pitch": "E1", "start": 0, "length": 1}]}]}"#;
    fixture.put("data/kick/hits.json", hits);
    watching(&graph, 3000, |g| revs(g).0 > number);
    let now = revs(&graph);
    assert_eq!(now, (number + 1, code_rev, number + 1), "a data revision, from {:?}", (number, code_rev, data_rev));
    assert_eq!(graph.inspect(|_| m2_script::builds()), builds, "no QuickJS run");
    assert_eq!(generation(&graph), first, "no new renderer");
    assert!(synced(&graph));
    let seq = graph.inspect(|a| a.open.as_ref().unwrap().sent).expect("a lane swap was sent");
    eventually("the engine applies the swap", || graph.applied(seq).is_some());
    // The session's own write makes no batch: a piano-roll edit is taken in once, by its commit.
    let state = graph.state().unwrap();
    let token = state["session"].as_u64().unwrap();
    graph.write_notes(token, number + 1, "kick", "hits", &kick_clips(&["C1", "D1"])).unwrap();
    let mine = revs(&graph);
    watching(&graph, 600, |_| false);
    assert_eq!(revs(&graph), mine, "nothing more to take in");
}

/// A notes lane of `n` notes in one clip, as an outside writer would write it.
pub(super) fn hits_of(n: usize) -> String {
    let notes: Vec<Value> = (0..n).map(|i| json!({"pitch": "C1", "start": i, "length": 0.25})).collect();
    serde_json::to_string_pretty(&json!({"clips": [{"id": "a", "start": 0, "length": 16, "notes": notes}]})).unwrap()
}

// WRITE-19, FMT-9, the step 6 gate: a lane file an outside writer writes in two halves never makes
// a revision in which that lane is empty. Watched by walks (a network volume): 60 ms apart, the
// halves settle as one batch, exactly one revision; 400 ms apart, longer than the quiet window, the
// first half settles alone: its revision keeps the lane's last data, with `lane.invalid`, and the
// second half makes the next. Watched by events: the same. inotify names each write; FSEvents names
// a file written while open only when it is opened and closed, so when the opening's event comes
// after the first half was written (fseventsd behind, as under a busy test run), the halves may
// settle as one.
#[test]
fn a_lane_written_in_two_halves_never_plays_empty() {
    for (walk, gap) in [(true, 60u64), (true, 400), (false, 60), (false, 400)] {
        let fixture = Fixture::new();
        fixture.write(KICK);
        fixture.put("data/kick/hits.json", &hits_of(4));
        let graph = Graph::silent();
        graph.watch_by_walking(walk);
        graph.open(&fixture.0).unwrap();
        assert_eq!(graph.watch_stats().unwrap().events, !walk);
        let start = revs(&graph).0;
        let new = hits_of(8);
        let (a, b) = new.split_at(new.len() / 2);
        let path = fixture.0.join("data/kick/hits.json");
        let (a, b) = (a.to_string(), b.to_string());
        let writer = std::thread::spawn(move || {
            use std::io::Write;
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(a.as_bytes()).unwrap();
            f.flush().unwrap();
            std::thread::sleep(Duration::from_millis(gap));
            f.write_all(b.as_bytes()).unwrap();
            f.flush().unwrap();
        });
        // Every revision the watcher takes in, as it plays the lane.
        let mut seen: Vec<(u64, usize, bool)> = vec![];
        watching(&graph, gap + 1500, |g| {
            let now = g.inspect(|a| {
                let r = a.open.as_ref().unwrap().session.accepted();
                let invalid = r.compiled.problems.iter().any(|p| p.code == "lane.invalid");
                (r.number, r.compiled.data.notes("kick", "hits").len(), invalid)
            });
            if seen.last().is_none_or(|l| l.0 != now.0) {
                seen.push(now);
            }
            now.1 == 8
        });
        writer.join().unwrap();
        assert!(seen.iter().all(|s| s.1 > 0), "{gap} ms: a revision played the lane empty: {seen:?}");
        let made: Vec<_> = seen.iter().filter(|s| s.0 > start).collect();
        if !walk && gap == 400 && made.len() == 1 {
            assert_eq!((made[0].1, made[0].2), (8, false), "events, {gap} ms: {seen:?}");
        } else if gap == 60 {
            assert_eq!(made.len(), 1, "{gap} ms: one batch, one revision: {seen:?}");
            assert_eq!((made[0].1, made[0].2), (8, false));
        } else {
            assert_eq!(made.len(), 2, "{gap} ms: {seen:?}");
            assert_eq!((made[0].1, made[0].2), (4, true), "{gap} ms: the half keeps the last data, and says so");
            assert_eq!((made[1].1, made[1].2), (8, false));
        }
    }
}

// RENDER-8, RENDER-25a: a structural edit is a new renderer that takes over where playback is:
// it never jumps back, and playing goes on.
#[test]
fn a_structural_edit_takes_over_where_playback_is() {
    let fixture = Fixture::new();
    fixture.write(KICK);
    let graph = fixture.open();
    let state = graph.state().unwrap();
    let (token, revision) = (state["session"].as_u64().unwrap(), state["view"]["revision"].as_u64().unwrap());
    graph.write_notes(token, revision, "kick", "hits", &kick_clips(&["C1", "C1", "C1", "C1"])).unwrap();
    graph.seek(8.0).unwrap();
    graph.transport("play").unwrap();
    eventually("it plays from beat 8", || graph.status().playing && graph.status().beat > 8.0);
    let first = generation(&graph);
    let state = graph.state().unwrap();
    let revision = state["view"]["revision"].as_u64().unwrap();
    let nodes = state["view"]["modules"].as_array().unwrap().iter().flat_map(|m| m["nodes"].as_array().unwrap().clone());
    let call = nodes.into_iter().find(|n| n["kind"] == "call" && n["audio"] == true && n["label"].as_str().unwrap().contains("kick"));
    let before = graph.status().beat;
    graph.add_effect(token, revision, call.unwrap()["id"].as_str().unwrap(), "std:delay").unwrap();
    let after = graph.status();
    assert!(after.playing && after.beat >= before, "no jump back: beat {before} then {}", after.beat);
    assert_ne!(generation(&graph), first, "a new renderer plays");
    let mut last = graph.status().beat;
    for _ in 0..20 {
        std::thread::sleep(Duration::from_millis(5));
        let now = graph.status().beat;
        assert!(now >= last, "the playhead never goes back: {last} then {now}");
        last = now;
    }
}

// RENDER-6a: stop is a transport event: the transport freezes, and the engine runs on so tails
// decay.
#[test]
fn stopping_freezes_the_transport_and_the_engine_runs_on() {
    let fixture = Fixture::new();
    let graph = fixture.open();
    graph.transport("play").unwrap();
    eventually("it plays", || graph.status().beat > 0.5);
    graph.transport("toggle").unwrap();
    // Stopped at once in the status; the engine takes it at its next block, and stays there.
    assert!(!graph.status().playing, "a stop sent is what the status says");
    eventually("the engine takes it", || {
        let beat = graph.status().beat;
        std::thread::sleep(Duration::from_millis(20));
        graph.status().beat == beat
    });
    let (beat, p) = (graph.status().beat, player(&graph));
    let clock = p.clock();
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(graph.status().beat, beat, "the transport holds");
    assert!(p.clock() > clock, "the engine clock goes on");
    assert!(graph.transport("rewind").is_err());
}

// RENDER-9a: a loop is beats of the accepted revision, refused when the engine couldn't play it;
// its range is kept while it is off, and playback wraps inside it. A project opened for the first
// time has none; opened again, it has the one it had (design-8b §2.2).
#[test]
fn the_loop_is_checked_kept_while_off_and_played() {
    let fixture = Fixture::new();
    let graph = fixture.open();
    assert_eq!((graph.status().looping, &graph.state().unwrap()["loop"]), (None, &Value::Null));
    assert!(graph.set_loop(None, Some(true)).is_err(), "no range to loop yet");
    for (start, end) in [(4.0, 2.0), (-1.0, 2.0), (f64::NAN, 2.0), (0.0, f64::INFINITY), (4.0, 4.005)] {
        assert!(graph.set_loop(Some((start, end)), None).is_err(), "{start}..{end}");
    }
    let on = |start, end, on| Some(Loop { start, end, on });
    assert_eq!(graph.set_loop(Some((4.0, 6.0)), None).unwrap().looping, on(4.0, 6.0, true));
    assert_eq!(graph.set_loop(None, Some(false)).unwrap().looping, on(4.0, 6.0, false));
    assert_eq!(graph.state().unwrap()["loop"], json!({"start": 4.0, "end": 6.0, "on": false}));
    assert_eq!(graph.set_loop(Some((4.0, 5.0)), Some(false)).unwrap().looping, on(4.0, 5.0, false));
    assert_eq!(graph.set_loop(None, Some(true)).unwrap().looping, on(4.0, 5.0, true));

    // At 120 bpm the loop is half a second: playing from inside it, it stays inside.
    graph.seek(4.5).unwrap();
    graph.transport("play").unwrap();
    eventually("it plays", || graph.status().playing);
    let t = Instant::now();
    let mut wrapped = false;
    while t.elapsed() < Duration::from_millis(1200) {
        let beat = graph.status().beat;
        assert!((4.0..5.0).contains(&beat), "inside the loop: {beat}");
        wrapped |= beat < 4.4;
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(wrapped, "it went round");
    graph.open(&fixture.0).unwrap();
    assert_eq!(graph.status().looping, on(4.0, 5.0, true));
}

// design-8b §2.2, §4.3: the loop is kept in `.music2/view.json`, local and never a Tx or an undo
// step: opened again, by this server or the next, it is where it was, on or off. Entries the file
// has that this version doesn't know are kept; a read-only window keeps its loop in memory only.
#[test]
fn the_loop_range_survives_reopening() {
    let fixture = Fixture::new();
    let graph = fixture.open();
    graph.set_loop(Some((8.0, 16.0)), Some(false)).unwrap();
    let file = fixture.0.join(".music2/view.json");
    let read = || serde_json::from_str::<Value>(&std::fs::read_to_string(&file).unwrap()).unwrap();
    let mut kept = read();
    assert_eq!(kept["loop"], json!({"start": 8.0, "end": 16.0, "on": false}));
    assert_eq!(graph.state().unwrap()["can_undo"], json!(false), "not an undo step");
    kept["zoom"] = json!(24);
    std::fs::write(&file, kept.to_string()).unwrap();
    graph.close();
    drop(graph);
    let graph = fixture.open();
    assert_eq!(graph.status().looping, Some(Loop { start: 8.0, end: 16.0, on: false }));
    assert_eq!(graph.state().unwrap()["loop"], json!({"start": 8.0, "end": 16.0, "on": false}));
    graph.set_loop(None, Some(true)).unwrap();
    assert_eq!((&read()["loop"]["on"], &read()["zoom"]), (&json!(true), &json!(24)));
    // Another window, read-only while this one writes: its loop isn't kept.
    let other = Graph::silent();
    assert!(!other.open(&fixture.0).unwrap()["read_only"].is_null());
    assert_eq!(other.status().looping, Some(Loop { start: 8.0, end: 16.0, on: true }), "it opens with the loop kept");
    other.set_loop(Some((1.0, 2.0)), None).unwrap();
    assert_eq!(read()["loop"], json!({"start": 8.0, "end": 16.0, "on": true}));
}

// RENDER-9a, design-8b §4.1: a tempo change that makes the loop too short to play turns it off,
// and the answer says so (the toast's "Loop turned off"); its range is kept. One that leaves it
// long enough doesn't, and with the loop off nothing is said of it.
#[test]
fn a_tempo_too_fast_for_the_loop_turns_it_off() {
    let fixture = Fixture::new();
    let graph = fixture.open();
    let token = graph.state().unwrap()["session"].as_u64().unwrap();
    // 0.012 beats: 6 ms at 120 BPM, just over the shortest loop at 48 kHz (two blocks, 256 frames).
    graph.set_loop(Some((0.0, 0.012)), None).unwrap();
    let v = graph.tempo(token, 0.0, Some(121.0), false, false, false).unwrap();
    assert_eq!((&v["loop_off"], &v["before"], &v["after"]), (&json!(false), &json!("120 BPM"), &json!("121 BPM")));
    assert_eq!(graph.status().looping, Some(Loop { start: 0.0, end: 0.012, on: true }));
    let v = graph.tempo(token, 0.0, Some(240.0), false, false, false).unwrap();
    assert_eq!(v["loop_off"], json!(true));
    assert_eq!(graph.status().looping, Some(Loop { start: 0.0, end: 0.012, on: false }));
    let v = graph.tempo(token, 0.0, Some(250.0), false, false, false).unwrap();
    assert!(v.get("loop_off").is_none(), "{v}");
    assert!(graph.set_loop(None, Some(true)).is_err(), "too short to turn on again at 250 BPM");
}

// taste.md §5.1, design-8b §4.3: Stop with `to` (the song bar's Space) stops and goes back to where
// play started, and answers where it stopped, for ⇧Space to continue from. The status says it
// stopped there at once, so no read after it sees it play on from there, or still where it
// stopped: the stop and the seek are both reported as done until the engine takes them. The beat
// it answers is where playback was as the stop was sent, a block before the engine took it at most.
#[test]
fn stop_with_to_goes_back_and_answers_where_it_stopped() {
    let fixture = Fixture::new();
    let graph = fixture.open();
    graph.seek(2.0).unwrap();
    graph.transport("play").unwrap();
    eventually("it plays past beat 3", || graph.status().beat > 3.0);
    let before = graph.status().beat;
    let at = graph.stop_to(2.0).unwrap();
    let s = graph.status();
    assert_eq!((s.playing, s.beat), (false, 2.0), "stopped back where play started, at once");
    // A block of 256 frames at 120 BPM and 48 kHz is 0.011 beats.
    assert!(at >= before && at < before + 0.25, "where it stopped: {at} (it was at {before})");
    eventually("the engine took both", || player(&graph).status().pending.is_none());
    std::thread::sleep(Duration::from_millis(50));
    let s = graph.status();
    assert!(!s.playing && (s.beat - 2.0).abs() < 1e-6, "and stays there: {s:?}");
    // ⇧Space: on from where it stopped.
    graph.seek(at).unwrap();
    graph.transport("play").unwrap();
    eventually("it plays on from there", || graph.status().beat > at + 0.1);
    assert!(graph.stop_to(f64::NAN).is_err());
}

/// A bass holding one note for 16 bars.
const HELD: &str = "import { song, track } from '@m2/core';\nimport { bass } from 'std/instruments';\n\
                    const b = track('bass', ({ lane }) => bass(lane.notes('notes')));\nexport default song(() => b.out);\n";

// RENDER-10, design-8b §4.3: Stop all sound ends a note held in the middle of its clip and resets
// every instance, and nothing plays the note again (the next block seeks without chasing,
// RENDER-11); the transport plays on. It answers while the session is busy, and changes nothing
// in it. With no player it opens none.
#[test]
fn panic_ends_every_note_and_never_touches_the_session() {
    let fixture = Fixture::new();
    fixture.put("song.ts", HELD);
    let clips: Vec<NoteClip> =
        serde_json::from_value(json!([{"id": "a", "start": 0, "length": 64, "notes": [{"pitch": "C2", "start": 0, "length": 64}]}])).unwrap();
    fixture.put("data/bass/notes.json", &m2_song::canonical(&m2_song::LaneData::Notes(clips)));
    let (p, mut driver) = Player::driven(48_000);
    let graph = Graph::with_player(p);
    graph.open(&fixture.0).unwrap();
    graph.transport("play").unwrap();
    // The loudest sample of `n` blocks.
    let mut loudest = |n: usize| {
        let mut most = 0f32;
        for _ in 0..n {
            let mut buf = [0.0f32; 256];
            driver.fill(&mut buf, 2);
            drop(driver.drain_retired());
            most = buf.iter().fold(most, |m, x| m.max(x.abs()));
        }
        most
    };
    assert!(loudest(40) > 0.01, "the note sounds");
    let before = (graph.state().unwrap(), fixture.read("song.ts"), fixture.read("data/bass/notes.json"));
    let held = graph.hold();
    graph.panic().unwrap();
    drop(held);
    loudest(8);
    assert!(loudest(40) < 1e-4, "silent: the note ended, and nothing played it again");
    assert!(graph.status().playing, "the transport plays on");
    let after = graph.state().unwrap();
    assert_eq!((&after["view"]["revision"], &after["can_undo"]), (&before.0["view"]["revision"], &json!(false)));
    assert_eq!((fixture.read("song.ts"), fixture.read("data/bass/notes.json")), (before.1, before.2));
    // With no player (nothing played yet, or a device let go), it does nothing and opens none.
    let idle = Graph::default();
    idle.panic().unwrap();
    assert!(idle.inner.current_device().is_none(), "no audio device opened to stop nothing");
}

// RENDER-29: seek, the loop and play never wait for the session, which an edit holds while it
// compiles.
#[test]
fn the_transport_does_not_wait_for_the_session() {
    let fixture = Fixture::new();
    let graph = &fixture.open();
    let held = graph.hold();
    let (tx, rx) = mpsc::channel();
    std::thread::scope(|s| {
        s.spawn(move || {
            graph.seek(3.0).unwrap();
            graph.set_loop(Some((2.0, 6.0)), None).unwrap();
            graph.transport("play").unwrap();
            tx.send(graph.status()).unwrap();
        });
        let status = rx.recv_timeout(Duration::from_secs(2)).expect("the transport answers while the session is busy");
        assert_eq!(status.looping, Some(Loop { start: 2.0, end: 6.0, on: true }));
        drop(held);
    });
    eventually("it plays from beat 3", || graph.status().playing && graph.status().beat >= 3.0);
}

// RENDER-29: a seek is timed from its call, so a wait for a lock (here the transport's, which a
// regression could hold over a compile) counts against it: the gate's G1 would see it.
#[test]
fn a_seek_is_timed_from_its_call() {
    let fixture = Fixture::new();
    let graph = &fixture.open();
    let asked = graph.now_ns();
    let held = graph.inner.timing.lock();
    std::thread::scope(|s| {
        let seek = s.spawn(|| graph.seek(1.0));
        std::thread::sleep(Duration::from_millis(50));
        drop(held);
        seek.join().unwrap().unwrap();
    });
    let t = graph.last_edit().unwrap();
    assert!(t.requested_ns < asked + 40_000_000 && t.sent_ns >= asked + 50_000_000, "asked at {asked}: {t:?}");
}

// A player that stops responding is let go, and the project is loaded into a new one where it
// was (step 3a design, §9.5).
#[test]
fn a_stalled_player_is_replaced_and_the_project_plays_on_where_it_was() {
    let fixture = Fixture::new();
    // A device that never calls back.
    let (p, driver) = Player::driven(48_000);
    let graph = Graph::with_player(p);
    graph.open(&fixture.0).unwrap();
    let t = Instant::now();
    let e = loop {
        if let Err(e) = graph.seek(3.0) {
            break e;
        }
        assert!(t.elapsed() < Duration::from_secs(5), "the queue never filled");
    };
    assert!(e.to_string().contains("stopped responding"), "{e:#}");
    assert!(t.elapsed() < Duration::from_secs(5), "a send gives up: {:?}", t.elapsed());
    assert!(graph.transport("play").is_err(), "the stalled player is let go");
    assert!(graph.poll(), "a new one plays the project");
    eventually("where it was", || graph.status() == Status { playing: false, seconds: 1.5, beat: 3.0, bpm: Some(120.0), looping: None });
    assert_eq!(player(&graph).device, "none");
    drop(driver);
}

// RENDER-28 (platform survey-later's bug 1): an xrun is counted and play goes on. No action
// fails and the same player plays the project; the counts are in the engine view and its status.
// Before, one xrun (a CoreAudio overload) let the player go at the next action, cutting every tail.
#[test]
fn an_xrun_is_counted_and_the_player_plays_on() {
    let mut dc = Dc::new();
    dc.graph.transport("play").unwrap();
    dc.blocks(2);
    for _ in 0..3 {
        dc.driver.error(DeviceErrorKind::Xrun.into());
    }
    dc.driver.error(DeviceErrorKind::BackendError.into());
    dc.graph.seek(1.0).unwrap();
    dc.graph.transport("play").unwrap();
    assert!(!dc.graph.poll(), "nothing to bring up to date");
    assert_eq!(dc.blocks(4), vec![0.5; 4], "the same player plays on");
    assert_eq!(dc.graph.inspect(|a| a.open.as_ref().unwrap().plays.as_ref().unwrap().epoch), 0, "never let go");
    let a = dc.graph.activity();
    assert_eq!((a.xruns, a.device_errors), (3, 1));
    let state = dc.graph.state().unwrap();
    assert_eq!((&state["xruns"], &state["device_errors"]), (&json!(3), &json!(1)), "in the engine view");
}

// RENDER-28, bug 1: a change of output device makes a new player, and only a device that is gone
// (its stream invalidated, or unavailable) stalls one. Either way the action that meets it says
// why, and the next settle loads the project into a new player, on the device that is the default
// then, where it was. The new player's counts start from nothing.
#[test]
fn a_device_change_or_loss_makes_a_new_player_where_it_was() {
    for (kind, why) in [
        (DeviceErrorKind::DeviceChanged, "the sound output moved to another device"),
        (DeviceErrorKind::StreamInvalidated, "the audio device stopped responding"),
        (DeviceErrorKind::DeviceNotAvailable, "the audio device stopped responding"),
    ] {
        let mut dc = Dc::new();
        dc.graph.seek(3.0).unwrap();
        dc.blocks(2);
        dc.driver.error(kind.into());
        assert_eq!(dc.graph.activity().device_errors, 1, "{kind:?}");
        let e = dc.graph.transport("play").unwrap_err();
        assert!(e.to_string().starts_with(why), "{kind:?}: {e:#}");
        assert!(dc.graph.poll(), "{kind:?}: a new player plays the project");
        assert_eq!(dc.graph.inspect(|a| a.open.as_ref().unwrap().plays.as_ref().unwrap().epoch), 1, "{kind:?}");
        eventually("where it was", || dc.graph.status().beat == 3.0);
        assert_eq!(player(&dc.graph).device, "none", "{kind:?}: a new player (silent in tests)");
        assert_eq!(dc.graph.activity(), Activity { output: Some("none".into()), ..Activity::default() }, "{kind:?}");
        dc.graph.transport("play").unwrap();
    }
}

// ---------------------------------------------------------------- leases (WRITE-22)

// WRITE-22: the explicit lease API: begin answers with the control, updates follow their order
// and drop older ones, a commit is one verified edit that ends the lease, a cancel puts the code's
// value back, and a lease that ended says why.
#[test]
fn a_lease_begins_updates_in_order_and_commits_or_cancels() {
    let fixture = Fixture::new();
    let graph = fixture.open();
    let (token, revision, id) = control(&graph);
    let begun = graph.begin(token, revision, &id, Owner::default()).unwrap();
    assert_eq!(
        (begun.revision, begun.id.as_str(), begun.value, begun.uses, begun.cells, begun.home, begun.heard),
        (revision, id.as_str(), 800.0, 1, 1, m2_session::Home::Code, true)
    );
    assert!(graph.update(begun.lease, Some(2), 900.0).unwrap().is_some());
    assert_eq!(graph.update(begun.lease, Some(1), 850.0).unwrap(), None, "older than one taken: dropped");
    assert_eq!(graph.leases()[0].value, 900.0);
    assert!(graph.begin(token + 1, revision, &id, Owner::default()).is_err(), "another session's request is refused");
    assert!(graph.begin(token, revision, &id, Owner::default()).is_err(), "one gesture at a time");
    graph.keepalive(begun.lease).unwrap();
    match graph.commit(begun.lease, 950.0, Some(800.0)).unwrap() {
        Committed::Written(v) => assert_eq!((&v["before"], &v["after"]), (&json!("800"), &json!("950"))),
        other => panic!("{other:?}"),
    }
    assert!(graph.leases().is_empty());
    let e = graph.update(begun.lease, None, 960.0).unwrap_err();
    assert!(e.to_string().contains("no gesture lease"), "a committed lease is gone: {e}");

    let (_, revision, id) = knob(&graph, 950.0);
    let again = graph.begin(token, revision, &id, Owner::default()).unwrap();
    graph.update(again.lease, None, 1000.0).unwrap();
    graph.cancel(again.lease).unwrap();
    let e = graph.update(again.lease, None, 1001.0).unwrap_err();
    assert!(e.to_string().contains("ended: cancelled"), "{e}");
    assert!(fixture.read("song.ts").contains("hz(950)"));
    graph.cancel(again.lease).unwrap();
}

// WRITE-22, C6: a lease idle past its TTL ends, and its value is put back; the news lists live
// leases. A window that closes ends its live leases, and keeps its pending ones.
#[test]
fn abandoned_leases_end_and_a_closed_window_ends_its_own() {
    let fixture = Fixture::new();
    fixture.write("import { sum } from 'std/math';\nexport default song(() => sum(osc.sine(hz(800)), osc.saw(hz(300))));\n");
    let graph = fixture.open();
    let (token, revision, id) = control(&graph);
    let owner = |client: &str, ttl_ms| Owner { client: Some(client.into()), gesture: None, ttl: Some(Duration::from_millis(ttl_ms)) };
    let short = graph.begin(token, revision, &id, owner("c1", 50)).unwrap();
    graph.update(short.lease, None, 900.0).unwrap();
    assert_eq!(graph.news()["leases"][0]["lease"], short.lease);
    assert!(graph.expire().is_empty(), "not yet");
    std::thread::sleep(Duration::from_millis(120));
    let said = graph.expire();
    assert!(said.len() == 1 && said[0].contains("abandoned"), "{said:?}");
    assert!(graph.leases().is_empty() && graph.news()["leases"] == json!([]));

    let a = graph.begin(token, revision, &id, owner("c1", 10_000)).unwrap();
    let (_, _, saw) = knob(&graph, 300.0);
    let b = graph.begin(token, revision, &saw, owner("c2", 10_000)).unwrap();
    graph.client_gone("c1");
    assert_eq!(graph.leases().iter().map(|l| l.lease).collect::<Vec<_>>(), [b.lease]);
    assert!(graph.update(a.lease, None, 900.0).unwrap_err().to_string().contains("window that owned it closed"));
}

// C5, WRITE-22: a commit refused only because another file doesn't compile waits, pending: the
// value plays on, a closed window keeps it, and once the draft compiles the actor writes it.
#[test]
fn a_commit_while_the_draft_is_broken_is_saved_once_it_compiles() {
    let fixture = Fixture::new();
    fixture.write("import './b';\nexport default song(() => osc.sine(hz(800)));\n");
    fixture.put("b.ts", "export const b = 1;\n");
    let graph = fixture.open();
    let (token, revision, id) = control(&graph);
    let owner = Owner { client: Some("c1".into()), gesture: Some("g".into()), ttl: None };
    let l = graph.begin(token, revision, &id, owner).unwrap();
    graph.update(l.lease, None, 900.0).unwrap();
    fixture.put("b.ts", "export const b = (;\n");
    assert!(graph.poll());
    match graph.commit(l.lease, 950.0, Some(800.0)).unwrap() {
        Committed::Pending { errors } => assert_eq!(errors[0]["file"], "b.ts"),
        other => panic!("{other:?}"),
    }
    assert_eq!((graph.leases()[0].state, graph.leases()[0].value), ("pending", 950.0));
    assert!(graph.update(l.lease, None, 960.0).unwrap_err().to_string().contains("waiting for the code to compile"));
    graph.client_gone("c1");
    assert_eq!(graph.leases().len(), 1, "a closed window keeps its pending commit");
    // A newer release replaces the value to save.
    assert!(matches!(graph.commit(l.lease, 970.0, Some(800.0)).unwrap(), Committed::Pending { .. }));
    assert!(fixture.read("song.ts").contains("hz(800)"));
    fixture.put("b.ts", "export const b = 2;\n");
    assert!(graph.poll());
    assert!(graph.leases().is_empty());
    assert!(fixture.read("song.ts").contains("hz(970)") && fixture.read("b.ts").contains("b = 2"));
    assert!(graph.expire().iter().any(|s| s.contains("saved once the code compiled")));
    assert!(graph.inspect(|a| a.open.as_ref().unwrap().session.can_undo()));
    let undone = graph.undo(token).unwrap();
    assert_eq!((&undone["before"], &undone["after"]), (&json!("970"), &json!("800")), "the last undo step is the knob");
}

// C5, C6: a pending commit is cancelled after 30 s; a new gesture from the same window takes a
// pending control over, and another window's is refused.
#[test]
fn a_pending_commit_is_taken_over_by_its_own_window_only() {
    let fixture = Fixture::new();
    fixture.write("import './b';\nexport default song(() => osc.sine(hz(800)));\n");
    fixture.put("b.ts", "export const b = 1;\n");
    let graph = fixture.open();
    let (token, revision, id) = control(&graph);
    let owner = |c: &str| Owner { client: Some(c.into()), gesture: None, ttl: None };
    let l = graph.begin(token, revision, &id, owner("c1")).unwrap();
    fixture.put("b.ts", "export const b = (;\n");
    assert!(graph.poll());
    assert!(matches!(graph.commit(l.lease, 950.0, None).unwrap(), Committed::Pending { .. }));
    assert!(graph.begin(token, revision, &id, owner("c2")).is_err(), "another window's is refused");
    let next = graph.begin(token, revision, &id, owner("c1")).unwrap();
    assert_eq!(graph.leases().iter().map(|l| (l.lease, l.state, l.value)).collect::<Vec<_>>(), [(next.lease, "live", 950.0)]);
    assert!(graph.update(l.lease, None, 1.0).unwrap_err().to_string().contains("superseded"));
    graph.cancel(next.lease).unwrap();
    assert!(fixture.read("song.ts").contains("hz(800)"));
}

/// A project whose output is the constant written in `tracks/a.ts` (a square at 0 Hz is 1), plus
/// `tracks/b.ts`, silent: what is heard is the value a knob drag plays, sample for sample.
struct Dc {
    fixture: Fixture,
    graph: Graph,
    driver: Driver,
}

const SONG_DC: &str = "import { song } from '@m2/core';\nimport { sum } from 'std/math';\nimport { a } from './tracks/a';\n\
                       import { b } from './tracks/b';\nexport default song(() => sum(a.out, b.out));\n";
const A_DC: &str = "import { hz, track } from '@m2/core';\nimport { osc, vca } from 'std/dsp';\n\
                    export const a = track('a', () => {\n  return vca(osc.square(hz(0)), 0.5);\n});\n";
const B_SILENT: &str = "import { hz, track } from '@m2/core';\nimport { osc, vca } from 'std/dsp';\n\
                        export const b = track('b', () => vca(osc.saw(hz(110)), 0));\n";

impl Dc {
    fn new() -> Dc {
        let mut dc = Dc::with(A_DC);
        assert_eq!(dc.blocks(4), vec![0.5; 4], "the song plays its constant");
        dc
    }

    /// The same, with `a` as `tracks/a.ts`.
    fn with(a: &str) -> Dc {
        let fixture = Fixture::new();
        fixture.put("song.ts", SONG_DC);
        fixture.put("tracks/a.ts", a);
        fixture.put("tracks/b.ts", B_SILENT);
        let (p, driver) = Player::driven(48_000);
        let graph = Graph::with_player(p);
        graph.open(&fixture.0).unwrap();
        Dc { fixture, graph, driver }
    }

    /// Fill `n` blocks of 128 frames: the first frame of each, which is every frame of it while
    /// nothing moves; a block whose frames differ is reported by its last.
    fn blocks(&mut self, n: usize) -> Vec<f32> {
        (0..n)
            .map(|_| {
                let mut buf = [0.0f32; 256];
                self.driver.fill(&mut buf, 2);
                drop(self.driver.drain_retired());
                let left: Vec<f32> = buf.iter().step_by(2).copied().collect();
                if left.iter().all(|x| *x == left[0]) { left[0] } else { left[127] }
            })
            .collect()
    }

    /// Every frame of `n` blocks.
    fn frames(&mut self, n: usize) -> Vec<f32> {
        let mut out = vec![];
        for _ in 0..n {
            let mut buf = [0.0f32; 256];
            self.driver.fill(&mut buf, 2);
            drop(self.driver.drain_retired());
            out.extend(buf.iter().step_by(2));
        }
        out
    }

    fn generation(&mut self) -> u64 {
        self.driver.engine().unwrap().current().unwrap().generation()
    }

    fn knob(&self) -> (u64, u64, String) {
        knob(&self.graph, 0.5)
    }
}

/// The origin of the accepted control whose value is `value`.
fn dc_origin(graph: &Graph, value: f64) -> m2_ir::Origin {
    graph.inspect(move |a| a.open.as_ref().unwrap().session.controls().into_iter().find(|c| c.value == value).unwrap().origin)
}

/// Whether `x` is `v` as the engine plays it (an f32), within a crossfade's rounding.
fn near(x: f32, v: f64) -> bool {
    (x as f64 - v).abs() < 1e-6
}

// WRITE-20, the step 6 gate: a project whose code doesn't compile opens with its problems and
// plays nothing; the code editor's save that still doesn't compile is written and stays silent;
// the one that compiles loads a renderer, and the song plays.
#[test]
fn a_broken_project_opens_and_plays_once_saved_fixed() {
    let fixture = Fixture::new();
    let broken = A_DC.replace("0.5);", "0.5) oops;");
    fixture.put("song.ts", SONG_DC);
    fixture.put("tracks/a.ts", &broken);
    fixture.put("tracks/b.ts", B_SILENT);
    let (p, driver) = Player::driven(48_000);
    let graph = Graph::with_player(p);
    let state = graph.open(&fixture.0).unwrap();
    assert_eq!(state["diagnostics"][0]["file"], json!("tracks/a.ts"), "{state:#}");
    let token = state["session"].as_u64().unwrap();
    let mut dc = Dc { fixture, graph, driver };
    assert!(dc.driver.engine().is_none(), "nothing is loaded into the player");
    assert_eq!(dc.blocks(4), vec![0.0; 4]);
    // The code editor lists the file as it is on disk, and saves over it.
    let files = dc.graph.files().unwrap();
    let a = files["files"].as_array().unwrap().iter().find(|f| f["path"] == "tracks/a.ts").unwrap().clone();
    assert_eq!(a["text"], json!(broken));
    let still = broken.replace("oops", "still oops");
    let saved = dc.graph.write_file(Some(token), "tracks/a.ts", &broken, &still).unwrap();
    assert_eq!(dc.fixture.read("tracks/a.ts"), still, "a broken save is written");
    assert!(saved["diagnostics"][0]["message"].as_str().is_some(), "{saved:#}");
    assert!(dc.driver.engine().is_none());
    assert_eq!(dc.blocks(4), vec![0.0; 4], "and plays nothing");
    let saved = dc.graph.write_file(Some(token), "tracks/a.ts", &still, A_DC).unwrap();
    assert_eq!((&saved["diagnostics"], saved["view"]["modules"].as_array().map(Vec::len)), (&json!([]), Some(3)), "{saved:#}");
    assert!(dc.blocks(8).iter().skip(4).all(|x| near(*x, 0.5)), "the song plays once it compiles");
    assert_eq!(playing(&dc.graph), (2, 2));
}

// WRITE-22, step 3's gate "an outside save to another file during a drag doesn't interrupt the
// drag": an agent saves tracks/b.ts while a knob in tracks/a.ts is dragged. The save compiles and
// a new plan takes over, keeping a.ts's instances; the drag's value plays on every frame of it,
// never the code's, and the next update reaches the new plan. The commit writes a.ts alone.
#[test]
fn an_outside_save_to_another_file_during_a_drag_does_not_interrupt_it() {
    let mut dc = Dc::new();
    let (token, revision, id) = dc.knob();
    let first = dc.generation();
    let l = dc.graph.begin(token, revision, &id, Owner::default()).unwrap();
    dc.graph.update(l.lease, None, 0.9).unwrap();
    assert_eq!(dc.blocks(2), [0.9, 0.9]);
    // The agent's save: a code change, so a new renderer.
    dc.fixture.put(
        "tracks/b.ts",
        &B_SILENT
            .replace("vca(osc.saw(hz(110)), 0)", "delay(vca(osc.saw(hz(110)), 0))")
            .replace("import { osc, vca } from 'std/dsp';", "import { osc, vca } from 'std/dsp';\nimport { delay } from 'std/fx';"),
    );
    assert!(dc.graph.poll());
    let mut heard = dc.frames(1);
    assert_ne!(dc.generation(), first, "a new renderer plays");
    assert!(dc.driver.engine().unwrap().in_window(), "b.ts's changed edges crossfading");
    // Mid-fade, the drag moves on: it reaches the new renderer.
    let lease = &dc.graph.leases()[0];
    assert_eq!((lease.lease, lease.generation), (l.lease, Some(dc.generation())), "the lease aims at the new renderer");
    assert!(dc.graph.update(l.lease, None, 0.92).unwrap().is_some());
    heard.extend(dc.frames(6));
    assert!(!dc.driver.engine().unwrap().in_window());
    // Never the code's 0.5, not for one frame: 0.9 until the update, then on to 0.92.
    assert!(heard.iter().all(|x| (0.9 - 1e-6..=0.92 + 1e-6).contains(&(*x as f64))), "{heard:?}");
    assert!(near(*heard.last().unwrap(), 0.92));
    assert!(near(heard[0], 0.9));

    match dc.graph.commit(l.lease, 0.95, Some(0.5)).unwrap() {
        Committed::Written(v) => assert_eq!((&v["file"], &v["after"]), (&json!("tracks/a.ts"), &json!("0.95"))),
        other => panic!("{other:?}"),
    }
    assert!(dc.blocks(3).iter().all(|x| near(*x, 0.95)));
    assert!(dc.fixture.read("tracks/a.ts").contains("0.95") && dc.fixture.read("tracks/b.ts").contains("delay("));
    let undone = dc.graph.undo(token).unwrap();
    assert_eq!((&undone["file"], &undone["before"], &undone["after"]), (&json!("tracks/a.ts"), &json!("0.95"), &json!("0.5")));
}

// WRITE-12, WRITE-22: saves to the dragged knob's own file that don't touch its call keep the drag:
// a comment above it, or an import at the top and a line at the bottom in one save (a line diff
// carries the call past both). A save that rewrites its value ends it, and the new code plays.
#[test]
fn saves_around_the_dragged_call_keep_it_and_one_that_rewrites_it_ends_it() {
    let mut dc = Dc::new();
    let (token, revision, id) = dc.knob();
    let l = dc.graph.begin(token, revision, &id, Owner::default()).unwrap();
    dc.graph.update(l.lease, None, 0.9).unwrap();
    for text in [
        A_DC.replace("export const a", "// a comment above\nexport const a"),
        format!("import {{ lfo }} from 'std/control';\n{A_DC}// and one below\n"),
    ] {
        dc.fixture.put("tracks/a.ts", &text);
        assert!(dc.graph.poll());
        assert!(dc.blocks(8).iter().all(|x| near(*x, 0.9)), "the drag plays on through the save");
        assert!(dc.graph.update(l.lease, None, 0.9).is_ok());
        assert_eq!(dc.graph.leases()[0].generation, Some(dc.generation()));
    }
    dc.fixture.put("tracks/a.ts", &A_DC.replace("0.5)", "0.6)"));
    assert!(dc.graph.poll());
    let heard = dc.blocks(8);
    assert!(near(*heard.last().unwrap(), 0.6), "the new code plays: {heard:?}");
    let e = dc.graph.update(l.lease, None, 0.7).unwrap_err();
    assert!(e.to_string().contains("the code changed under this control"), "{e}");
}

// C5, WRITE-22: a broken save to another file, then the release: the commit waits, pending, with
// the released value playing; the fix compiles, and the commit is written. Never the code's value
// in between.
#[test]
fn a_release_during_a_broken_save_plays_on_and_saves_once_fixed() {
    let mut dc = Dc::new();
    let (token, revision, id) = dc.knob();
    let l = dc.graph.begin(token, revision, &id, Owner::default()).unwrap();
    dc.graph.update(l.lease, None, 0.9).unwrap();
    dc.fixture.put("tracks/b.ts", "export const b = (;\n");
    assert!(dc.graph.poll());
    let mut heard = dc.blocks(2);
    assert!(matches!(dc.graph.commit(l.lease, 0.95, Some(0.5)).unwrap(), Committed::Pending { .. }));
    heard.extend(dc.blocks(4));
    dc.fixture.put("tracks/b.ts", &B_SILENT.replace(", 0))", ", 0.0))"));
    assert!(dc.graph.poll());
    heard.extend(dc.blocks(8));
    assert!(heard[..2].iter().all(|x| near(*x, 0.9)) && heard[2..].iter().all(|x| near(*x, 0.95)), "{heard:?}");
    assert!(dc.graph.leases().is_empty());
    assert!(dc.fixture.read("tracks/a.ts").contains("0.95"));
}

// WRITE-22 (critique 4): a renderer that fails to build leaves the old one playing: the lease's
// session side moves to the accepted revision, its engine side stays where it is heard, and a
// gesture begun meanwhile is heard there too. The next build that succeeds takes their values.
#[test]
fn a_renderer_that_fails_to_build_leaves_the_drag_where_it_is_heard() {
    let mut dc = Dc::new();
    let (token, revision, id) = dc.knob();
    let first = dc.generation();
    let l = dc.graph.begin(token, revision, &id, Owner::default()).unwrap();
    dc.graph.update(l.lease, None, 0.9).unwrap();
    dc.graph.inspect(|a| a.fail_builds = true);
    let before = dc_origin(&dc.graph, 0.5);
    // Inserted above a's call in its own file: the origins in the accepted revision move.
    let moved = A_DC.replace("  return", "  const extra = osc.tri(hz(5));\n  return");
    dc.fixture.put("tracks/a.ts", &moved);
    dc.fixture.put("tracks/b.ts", &format!("// b\n{B_SILENT}"));
    assert!(dc.graph.poll());
    assert_eq!(dc.generation(), first, "the old renderer plays on");
    let (playing_now, accepted) = playing(&dc.graph);
    assert!(playing_now < accepted, "the accepted revision didn't build");
    assert_ne!(dc_origin(&dc.graph, 0.5), before, "the accepted revision numbers the knob's call anew");
    let leases = dc.graph.leases();
    assert_eq!((leases[0].generation, leases[0].revision), (Some(first), playing(&dc.graph).1));
    assert!(dc.graph.update(l.lease, None, 0.93).unwrap().is_some());
    assert!(dc.blocks(2).iter().all(|x| near(*x, 0.93)));
    // A gesture on b's knob, whose origin moved in the accepted revision, is heard where the old
    // code has it.
    let (_, accepted, saw) = knob(&dc.graph, 110.0);
    let b = dc.graph.begin(token, accepted, &saw, Owner::default()).unwrap();
    assert!(b.heard);
    dc.graph.update(b.lease, None, 230.0).unwrap();
    dc.graph.inspect(|a| a.fail_builds = false);
    dc.fixture.put("tracks/b.ts", &format!("// b, again\n{B_SILENT}"));
    assert!(dc.graph.poll());
    let heard = dc.blocks(8);
    assert_ne!(dc.generation(), first);
    assert!(heard.iter().all(|x| near(*x, 0.93)), "the new renderer took the drag's value: {heard:?}");
    assert_eq!(dc.graph.leases().len(), 2);
}

// RENDER-26, R4 (the step 6 design's 5.5): the engine's path is the delta from what plays, never
// the last change's alone: after a structural save whose renderer failed to build, a lane edit
// builds a renderer that plays both, rather than swapping its lane into the stale one.
#[test]
fn a_failed_build_then_a_lane_edit_rebuilds() {
    let fixture = Fixture::new();
    fixture.write(KICK);
    let graph = fixture.open();
    graph.transport("play").unwrap();
    let first = generation(&graph);
    graph.inspect(|a| a.fail_builds = true);
    fixture.write(&KICK.replace("export default", "const spare = track('spare', () => osc.sine(hz(3)));\nexport default"));
    assert!(graph.poll());
    let (plays, accepted) = playing(&graph);
    assert!(plays < accepted, "the structural save didn't build");
    assert_eq!(generation(&graph), first, "what played plays on");
    graph.inspect(|a| a.fail_builds = false);
    let state = graph.state().unwrap();
    let (token, revision) = (state["session"].as_u64().unwrap(), state["view"]["revision"].as_u64().unwrap());
    graph.write_notes(token, revision, "kick", "hits", &kick_clips(&["C1", "D1"])).unwrap();
    // PLANE-5: what plays lags the accepted revision, so the write isn't previewed: it is heard
    // once accepted, through the renderer that plays both.
    assert!(!graph.last_edit().unwrap().previewed);
    assert_ne!(generation(&graph), first, "a new renderer, not a swap into the stale one");
    assert!(synced(&graph));
    let spare = graph.inspect(|a| {
        let p = a.open.as_ref().unwrap().plays.as_ref().unwrap();
        p.revision.compiled.flat.nodes.iter().any(|n| matches!(&n.op, m2_ir::FOp::Kernel { kernel, .. } if kernel == "std:sine"))
    });
    assert!(spare, "what plays has the structural change");
}

/// `tracks/a.ts` of the DC song with a second gain after the first, a line apart: what is heard is
/// their product. (Changes on adjacent lines are one hunk between two revisions, which names the
/// calls in it anew: the step 6 design's risk 1.)
const A_TWO_GAINS: &str = "import { hz, track } from '@m2/core';\nimport { osc, vca } from 'std/dsp';\n\
                           export const a = track('a', () => {\n  const first = vca(osc.square(hz(0)), 0.5);\n  \
                           // then the second gain\n  return vca(first, 1);\n});\n";

// RENDER-22, R4 (the step 6 design's 5.5): a knob commit whose file an outside writer also changed
// (another literal, not taken in yet) rides along (R2), and what plays takes the whole delta from
// itself as cell writes: both values are heard, with no new renderer.
#[test]
fn a_values_commit_writes_cells_from_what_plays() {
    let mut dc = Dc::with(A_TWO_GAINS);
    assert_eq!(dc.blocks(4), vec![0.5; 4]);
    let first = dc.generation();
    let (token, revision, id) = dc.knob();
    // An outside writer turns the second gain down; the session hasn't taken it in.
    dc.fixture.put("tracks/a.ts", &A_TWO_GAINS.replace("first, 1)", "first, 0.8)"));
    dc.graph.set(token, revision, &id, "drag", 0.6, Some(0.5)).unwrap();
    let a = dc.fixture.read("tracks/a.ts");
    assert!(a.contains("0.6);") && a.contains("first, 0.8)"), "both on disk: {a}");
    let heard = dc.blocks(8);
    assert!(heard[4..].iter().all(|x| near(*x, 0.6 * 0.8)), "both are heard: {heard:?}");
    assert_eq!(dc.generation(), first, "by cell writes, with no new renderer");
    assert!(synced(&dc.graph));
    // An outside value edit alone is written into the cells too.
    dc.fixture.put("tracks/a.ts", &a.replace("first, 0.8)", "first, 0.5)"));
    assert!(dc.graph.poll());
    let heard = dc.blocks(8);
    assert!(heard[4..].iter().all(|x| near(*x, 0.3)), "{heard:?}");
    assert_eq!(dc.generation(), first);
}

// WRITE-22, R2 (the step 6 design's 16): an outside edit in the dragged knob's own file, which
// compiles and isn't taken in yet, doesn't end the drag: its commit carries across it, is judged
// against the baseline that has it, and writes the knob's value beside it. Both are heard.
#[test]
fn a_knob_commit_after_an_outside_edit_in_its_file_keeps_both() {
    let mut dc = Dc::with(A_TWO_GAINS);
    let (token, revision, id) = dc.knob();
    let l = dc.graph.begin(token, revision, &id, Owner::default()).unwrap();
    dc.graph.update(l.lease, None, 0.9).unwrap();
    assert!(dc.blocks(4).iter().all(|x| near(*x, 0.9)));
    dc.fixture.put("tracks/a.ts", &A_TWO_GAINS.replace("first, 1)", "first, 0.8)"));
    assert!(matches!(dc.graph.commit(l.lease, 0.9, Some(0.5)).unwrap(), Committed::Written(_)));
    let a = dc.fixture.read("tracks/a.ts");
    assert!(a.contains("0.9);") && a.contains("first, 0.8)"), "both on disk: {a}");
    let heard = dc.blocks(8);
    assert!(heard[4..].iter().all(|x| near(*x, 0.9 * 0.8)), "both are heard: {heard:?}");
    assert!(dc.graph.leases().is_empty() && synced(&dc.graph));
}

// WRITE-22 (critique 13): a cancel while the commit is on the session answers "committing", and
// the commit decides: the committed value plays, never the code's in between.
#[test]
fn a_cancel_during_a_commit_leaves_it_to_the_commit() {
    let mut dc = Dc::new();
    let (token, revision, id) = dc.knob();
    let l = dc.graph.begin(token, revision, &id, Owner::default()).unwrap();
    dc.graph.update(l.lease, None, 0.9).unwrap();
    let held = dc.graph.hold();
    let graph = &dc.graph;
    let heard = std::thread::scope(|s| {
        let commit = s.spawn(move || graph.commit(l.lease, 0.95, Some(0.5)));
        eventually("the commit is on the session", || graph.leases()[0].state == "committing");
        let e = graph.cancel(l.lease).unwrap_err();
        assert!(e.to_string().contains("being committed"), "{e}");
        drop(held);
        commit.join().unwrap().unwrap()
    });
    assert!(matches!(heard, Committed::Written(_)));
    assert!(dc.blocks(4).iter().all(|x| near(*x, 0.95)));
    assert!(dc.fixture.read("tracks/a.ts").contains("0.95"));
}

/// A project with knobs in three track files and a notes lane, for the timing of G8.
fn three_tracks(fixture: &Fixture) {
    fixture.put(
        "song.ts",
        "import { hz, secs, song, track } from '@m2/core';\nimport { osc, vca } from 'std/dsp';\nimport { sum } from 'std/math';\n\
         import { kick } from 'std/instruments';\nimport { a } from './tracks/a';\nimport { b } from './tracks/b';\n\
         import { c } from './tracks/c';\nconst k = track('kick', ({ lane }) => kick(lane.notes('hits'), { decay: secs(0.3) }));\n\
         export default song(() => vca(sum(a.out, b.out, c.out, k.out), 0.25));\n",
    );
    for (key, call) in [("a", "osc.sine(hz(800))"), ("b", "osc.tri(hz(300))"), ("c", "osc.saw(hz(200))")] {
        fixture.put(
            &format!("tracks/{key}.ts"),
            &format!("import {{ hz, track }} from '@m2/core';\nimport {{ osc }} from 'std/dsp';\nexport const {key} = track('{key}', () => {call});\n"),
        );
    }
}

/// tracks/b.ts with a busy loop that holds its build for about a second (QuickJS spins about
/// 1e8 empty iterations a second).
fn slow_b(n: u64) -> String {
    format!(
        "import {{ hz, track }} from '@m2/core';\nimport {{ osc }} from 'std/dsp';\nfor (let i = 0; i < {n}; i++) {{}}\n\
         export const b = track('b', () => osc.tri(hz(300)));\n"
    )
}

/// How long `f` took, in milliseconds, and what it returned.
fn timed<T>(f: impl FnOnce() -> T) -> (f64, T) {
    let t = Instant::now();
    let r = f();
    (t.elapsed().as_secs_f64() * 1e3, r)
}

// WRITE-22, RENDER-29, step 3's gate "previews never wait on a compile": while the session compiles
// a slow save, a drag's updates return at once and the engine hears them before the compile ends;
// gestures begun one data revision and one code change behind are resolved without the session.
// The negative control, a begin forced through the session, waits for the compile.
#[test]
fn previews_never_wait_for_a_compile() {
    let fixture = Fixture::new();
    three_tracks(&fixture);
    let graph = Arc::new(fixture.open());
    graph.transport("play").unwrap();
    let r0 = playing(&graph).1;
    let (token, _, master) = knob(&graph, 0.25);
    // A code revision (a comment in tracks/c.ts), then a data revision (a lane file).
    fixture.put("tracks/c.ts", &format!("// c\n{}", fixture.read("tracks/c.ts")));
    assert!(graph.poll());
    let r1 = playing(&graph).1;
    let (_, _, saw) = knob(&graph, 200.0);
    fixture.put(
        "data/kick/hits.json",
        r#"{"clips": [{"id": "a", "start": 0, "length": 4, "notes": [{"pitch": "C1", "start": 0, "length": 1}]}]}"#,
    );
    assert!(graph.poll());
    let r2 = playing(&graph).1;
    assert!(r0 < r1 && r1 < r2);
    let (_, _, sine) = knob(&graph, 800.0);
    let lease = graph.begin(token, r2, &sine, Owner::default()).unwrap().lease;

    let compile = |n: u64| {
        let (g, dir) = (graph.clone(), fixture.0.clone());
        std::thread::spawn(move || {
            std::fs::write(dir.join("tracks/b.ts"), slow_b(n)).unwrap();
            g.poll()
        })
    };
    let saver = compile(100_000_000);
    eventually("the session compiles", || graph.busy().is_some());
    let busy_from = graph.busy().unwrap();
    let (mut sent, mut slowest, mut during) = (vec![], 0.0f64, 0);
    let mut begins = vec![];
    let mut value = 800.0;
    while !saver.is_finished() {
        let tick = Instant::now();
        value += 1.0;
        let (ms, seq) = timed(|| graph.update(lease, None, value).unwrap());
        slowest = slowest.max(ms);
        during += usize::from(graph.busy().is_some());
        let seq = seq.expect("heard");
        // Each stamp is read before the next update: the engine keeps only the last 16.
        let t = Instant::now();
        let stamp = loop {
            if let Some(a) = graph.applied(seq) {
                break a;
            }
            assert!(t.elapsed() < Duration::from_millis(50), "update {seq}'s stamp never came");
            std::thread::sleep(Duration::from_micros(200));
        };
        sent.push(stamp.callback_ns);
        if begins.is_empty() && sent.len() == 5 {
            // One data revision behind (the ring), and one code change behind (the aliases).
            begins.push(timed(|| graph.begin(token, r1, &saw, Owner::default()).unwrap()).0);
            begins.push(timed(|| graph.begin(token, r0, &master, Owner::default()).unwrap()).0);
        }
        std::thread::sleep(Duration::from_millis(5).saturating_sub(tick.elapsed()));
    }
    assert!(saver.join().unwrap(), "the slow save was taken in");
    // The actor keeps a request's span once its answer is sent: wait for it.
    eventually("the compile's span is kept", || graph.busy_spans().iter().any(|s| s.0 == busy_from));
    let (start, end) = *graph.busy_spans().iter().rev().find(|s| s.0 == busy_from).expect("the compile's span");
    let compile_ms = (end - start) as f64 / 1e6;
    if compile_ms < 500.0 {
        eprintln!("void: the compile took only {compile_ms:.0} ms");
        return;
    }
    assert!(during >= 20, "{during} updates were sent while the session compiled");
    assert!(slowest < 20.0, "an update took {slowest:.1} ms");
    assert_eq!(begins.len(), 2, "both begins ran during the compile");
    assert!(begins.iter().all(|ms| *ms < 20.0), "begins took {begins:?} ms");
    let heard_before = sent.iter().filter(|ns| **ns < end).count();
    eprintln!(
        "G8: compile {compile_ms:.0} ms; {during} updates while compiling, the slowest {slowest:.2} ms; {heard_before} heard before it \
         ended; begins {begins:.2?} ms"
    );
    assert!(heard_before >= 5, "only {heard_before} updates were heard before the compile ended");

    // The negative control: a begin through the session waits for the compile.
    let (_, r3, tri) = knob(&graph, 300.0);
    let saver = compile(100_000_001);
    eventually("the session compiles", || graph.busy().is_some());
    let asked = graph.now_ns();
    let (ms, begun) = timed(|| graph.begin_via_actor(token, r3, &tri, Owner::default()));
    assert!(saver.join().unwrap());
    // The compile's span is kept once the actor is idle again.
    eventually("the compile's span is kept", || graph.busy().is_none());
    let end = graph.busy_spans().iter().rev().find(|s| s.0 <= asked).map(|s| s.1).unwrap();
    let left = end.saturating_sub(asked) as f64 / 1e6;
    eprintln!("G8 negative control: a begin through the session took {ms:.0} ms, with {left:.0} ms of the compile left");
    assert!(ms >= 0.5 * left, "the session's begin took {ms:.1} ms during a compile");
    // Named at the revision before the save: carried by the session.
    begun.unwrap();
    fixture.put("tracks/b.ts", &slow_b(0));
    assert!(graph.poll());
}

// ---------------------------------------------------------------- plugins (HOST-5, RENDER-25)

/// A song with the probe plugin (`m2_plugins::probe`) playing a notes lane in track `p`, beside
/// the constant of `tracks/a.ts`.
fn with_probe() -> Dc {
    m2_plugins::probe::install();
    let fixture = Fixture::new();
    fixture.put(
        "song.ts",
        "import { plugin, song, track } from '@m2/core';\nimport { sum } from 'std/math';\nimport { a } from './tracks/a';\n\
         const p = track('p', ({ lane }) => plugin('clap:m2.probe', { notes: lane.notes('n'), gain_1: 0.5 }));\n\
         export default song(() => sum(a.out, p.out));\n",
    );
    fixture.put("tracks/a.ts", A_DC);
    fixture.put(
        "data/p/n.json",
        r#"{"clips": [{"id": "a", "start": 0, "length": 64, "notes": [{"pitch": "C4", "start": 0, "length": 60}]}]}"#,
    );
    let (p, driver) = Player::driven(48_000);
    let graph = Graph::with_player(p);
    graph.open(&fixture.0).unwrap();
    let mut dc = Dc { fixture, graph, driver };
    dc.graph.transport("play").unwrap();
    dc.frames(4);
    dc
}

impl Dc {
    /// The plugin instances the engine plays.
    fn handles(&mut self) -> Vec<m2_plugins::Handle> {
        self.driver.engine().unwrap().current().unwrap().plugin_handles()
    }
}

/// The plugin instances of the plan last sent, as the actor knows them.
fn plays_handles(graph: &Graph) -> Vec<m2_plugins::Handle> {
    graph.inspect(|a| a.open.as_ref().unwrap().plays.as_ref().unwrap().info.plugins.iter().map(|p| p.handle).collect())
}

// HOST-5, RENDER-25, WRITE-22: an outside save that changes the song's structure keeps its plugin
// instances in the new plan, and a drag through it goes on; a rate change, a new engine, creates
// them anew; closing the project destroys every one before it returns.
#[test]
fn activations_keep_the_plugin_instances_and_closing_destroys_them() {
    let mut dc = with_probe();
    let first = dc.handles();
    assert_eq!(first.len(), 1);
    let (token, revision, id) = dc.knob();
    let l = dc.graph.begin(token, revision, &id, Owner::default()).unwrap();
    dc.graph.update(l.lease, None, 0.9).unwrap();
    let generation = dc.generation();
    // A comment changes nothing that plays: the renderer adopts it (the step 6 design's 5.5).
    dc.fixture.put("tracks/a.ts", &A_DC.replace("  return", "  // a comment\n  return"));
    assert!(dc.graph.poll());
    dc.frames(8);
    assert_eq!(dc.generation(), generation, "no new renderer for a comment");
    // A new node elsewhere in the dragged knob's file is structural: a new renderer.
    dc.fixture.put("tracks/a.ts", &A_DC.replace("  return", "  // a comment\n  const spare = osc.sine(hz(1));\n  return"));
    assert!(dc.graph.poll());
    dc.frames(8);
    assert_ne!(dc.generation(), generation, "a new renderer");
    assert_eq!(dc.handles(), first, "with the same instance");
    assert_eq!(dc.graph.leases()[0].generation, Some(dc.generation()));
    assert_eq!(plays_handles(&dc.graph), first);

    // A new engine at another rate: never carried.
    dc.fixture.put("music2.toml", &MANIFEST.replace("48000", "44100"));
    assert!(dc.graph.poll());
    let fresh = plays_handles(&dc.graph);
    assert!(fresh.len() == 1 && fresh != first, "{fresh:?} after {first:?}");

    // Closing waits for the instances to be destroyed, while the device plays the fade out.
    let graph = &dc.graph;
    let teardown = std::thread::scope(|s| {
        let closing = s.spawn(move || graph.close());
        while !closing.is_finished() {
            let mut buf = [0.0f32; 256];
            dc.driver.fill(&mut buf, 2);
            drop(dc.driver.drain_retired());
            std::thread::sleep(Duration::from_millis(1));
        }
        closing.join().unwrap()
    });
    assert!(teardown.left == 0 && teardown.destroyed >= 1, "{teardown:?}");
    assert_eq!(dc.graph.inner.owner.live(), 0);
}

// HOST-5, RENDER-25: a plan that fails to prepare (a plugin added after the probe can't load its
// state) leaves what plays playing, and the plan after the fix still keeps the probe's instance:
// the failed prepare touched nothing that plays.
#[test]
fn a_failed_prepare_leaves_what_plays_untouched() {
    let mut dc = with_probe();
    let first = dc.handles();
    let generation = dc.generation();
    let song = dc.fixture.read("song.ts");
    // In the probe's track, after it: the failed build has taken the probe already.
    let probe = "plugin('clap:m2.probe', { notes: lane.notes('n'), gain_1: 0.5 })";
    let failing = song.replace(probe, &format!("plugin('clap:m2.probe.fails-state', {{ in: {probe}, key: 'bad' }})"));
    assert_ne!(failing, song);
    dc.fixture.put("song.ts", &failing);
    dc.fixture.put("data/p/bad.state.json", r#"{"plugin": "clap:m2.probe.fails-state", "plugin_version": "1.0.0"}"#);
    dc.fixture.put("data/p/bad.state", "fail: this blob doesn't load");
    dc.graph.poll();
    dc.frames(8);
    let (plays, accepted) = playing(&dc.graph);
    assert!(plays < accepted, "the new revision's renderer didn't build");
    assert_eq!((dc.generation(), dc.handles()), (generation, first.clone()), "what played plays on");
    // Fixed: the next plan keeps the probe (wrapped, by its name: its `(` is kept, ID-13), and
    // creates only the new plugin.
    dc.fixture.put("data/p/bad.state", "loads");
    assert!(dc.graph.poll());
    dc.frames(8);
    let now = dc.handles();
    assert_ne!(dc.generation(), generation, "a new renderer");
    assert!(now.len() == 2 && now.contains(&first[0]), "the probe was kept: {now:?} after {first:?}");
}

// HOST-5 (critique 17): a player that stopped responding is replaced, and the project loaded into
// the new one creates every instance anew: two audio threads never share one.
#[test]
fn a_new_player_after_a_stall_creates_every_instance_anew() {
    let dc = with_probe();
    let first = plays_handles(&dc.graph);
    let t = Instant::now();
    while dc.graph.seek(1.0).is_ok() {
        assert!(t.elapsed() < Duration::from_secs(5), "the queue never filled");
    }
    assert!(dc.graph.transport("play").is_err(), "the stalled player is let go");
    dc.fixture.put("tracks/a.ts", &A_DC.replace("  return", "  // a comment\n  return"));
    assert!(dc.graph.poll());
    let (epoch, now) = (dc.graph.inspect(|a| a.open.as_ref().unwrap().plays.as_ref().unwrap().epoch), plays_handles(&dc.graph));
    assert_eq!(epoch, 1, "loaded into the new player");
    assert!(now.len() == 1 && now != first, "{now:?} after {first:?}");
}

/// A sine and a saw summed, with `above` written before them in the same call: each call it
/// inserts renumbers theirs.
fn sine_and_saw(above: &str) -> String {
    format!("import {{ sum }} from 'std/math';\nexport default song(() => sum({above}osc.sine(hz(800)), osc.saw(hz(300))));\n")
}

// WRITE-22, WRITE-13: a settle publishes a new code revision's controls with the engine (its first
// phase), and only then carries the aliases to them (its second). In between, a client two code
// revisions behind must not resolve through the aliases of the code revision before, which name
// other calls once a call was inserted above: it waits for the session, which carries its id.
// After, the aliases take it to its own control, and a commit writes that control's literal.
#[test]
fn a_begin_between_a_settles_phases_never_resolves_through_stale_aliases() {
    let fixture = Fixture::new();
    fixture.write(&sine_and_saw(""));
    let graph = fixture.open();
    let (token, rev_a, saw) = knob(&graph, 300.0);
    fixture.write(&sine_and_saw("osc.tri(hz(1)), "));
    assert!(graph.poll());
    fixture.write(&sine_and_saw("osc.tri(hz(2)), osc.tri(hz(1)), "));
    let seen = Arc::new(Mutex::new(vec![]));
    let (s, id) = (seen.clone(), saw.clone());
    graph.inspect(move |a| {
        a.between_phases = Some(Box::new(move |inner: &Inner| {
            let found = match inner.control.lock().open.as_ref().unwrap().find(rev_a, &id) {
                Resolved::Control(c) => format!("{} = {}", c.id, c.value),
                Resolved::Missing => "missing".into(),
                Resolved::Slow => "slow".into(),
            };
            s.lock().push(found);
        }))
    });
    assert!(graph.poll());
    graph.inspect(|a| a.between_phases = None);
    assert_eq!(*seen.lock(), ["slow"], "between the phases, two code changes behind waits for the session");
    let b = graph.begin(token, rev_a, &saw, Owner::default()).unwrap();
    assert_eq!(b.value, 300.0, "after, the aliases take it to the saw");
    assert!(matches!(graph.commit(b.lease, 310.0, Some(300.0)).unwrap(), Committed::Written(_)));
    let src = fixture.read("song.ts");
    assert!(src.contains("osc.sine(hz(800)), osc.saw(hz(310))"), "{src}");
}

// WRITE-22 (README, "Scripts and agents"): a gesture string has a lease per control, as it had a
// preview per control before leases. A script that sends one gesture for every knob previews,
// commits and cancels each knob's own, never another's. A cancel it sends itself is forgotten, so
// it may set that knob next; a lease that ended otherwise refuses its gesture's late requests on
// that control, and only there.
#[test]
fn one_gesture_string_reaches_each_controls_own_lease() {
    let fixture = Fixture::new();
    fixture.write(&sine_and_saw(""));
    let graph = fixture.open();
    let (token, r0, sine) = knob(&graph, 800.0);
    let (_, _, saw) = knob(&graph, 300.0);
    let playing = || graph.leases().into_iter().map(|l| (l.id, l.value)).collect::<Vec<_>>();
    graph.preview(token, r0, &sine, "script", 900.0).unwrap();
    graph.preview(token, r0, &saw, "script", 310.0).unwrap();
    assert_eq!(playing(), [(sine.clone(), 900.0), (saw.clone(), 310.0)]);
    let set = graph.set(token, r0, &saw, "script", 320.0, Some(300.0)).unwrap();
    assert_eq!((&set["before"], &set["after"]), (&json!("300"), &json!("320")));
    assert!(fixture.read("song.ts").contains("osc.sine(hz(800)), osc.saw(hz(320))"));
    assert_eq!(playing(), [(sine.clone(), 900.0)], "the sine's drag goes on");

    graph.cancel_preview(token, &saw, "script").unwrap();
    assert_eq!(playing(), [(sine.clone(), 900.0)], "a cancel naming the saw leaves the sine's");
    graph.cancel_preview(token, &sine, "script").unwrap();
    assert!(playing().is_empty());
    let set = graph.set(token, r0, &sine, "script", 820.0, None).unwrap();
    assert_eq!((&set["before"], &set["after"]), (&json!("800"), &json!("820")), "its own cancel is forgotten");
    assert!(fixture.read("song.ts").contains("osc.sine(hz(820)), osc.saw(hz(320))"));

    // An agent cancels a drag through the lease routes: that gesture's late requests on that
    // control say why, and the same string begins anew on another.
    let (_, r1, saw) = knob(&graph, 320.0);
    let (_, _, sine) = knob(&graph, 820.0);
    graph.preview(token, r1, &saw, "drag", 330.0).unwrap();
    graph.cancel(graph.leases()[0].lease).unwrap();
    let e = graph.preview(token, r1, &saw, "drag", 340.0).unwrap_err();
    assert!(e.to_string().contains("this gesture ended: cancelled"), "{e}");
    assert!(graph.set(token, r1, &saw, "drag", 340.0, None).is_err());
    graph.preview(token, r1, &sine, "drag", 830.0).unwrap();
    assert_eq!(playing(), [(sine, 830.0)]);
}

// C5, WRITE-22: the page says a release made while the code doesn't compile is saved once it
// does. When that pending commit ends some other way (the project closes, another opens, an
// outside save changes its control, an agent cancels it), the user is told the value wasn't saved.
#[test]
fn a_pending_commit_that_ends_unsaved_says_so() {
    let fixture = Fixture::new();
    fixture.write("import './b';\nexport default song(() => osc.sine(hz(800)));\n");
    let other = Fixture::new();
    let pending = |graph: &Graph| {
        fixture.put("b.ts", "export const b = 1;\n");
        graph.open(&fixture.0).unwrap();
        let (token, revision, id) = control(graph);
        let l = graph.begin(token, revision, &id, Owner { client: Some("c1".into()), gesture: Some("g".into()), ttl: None }).unwrap();
        graph.update(l.lease, None, 900.0).unwrap();
        fixture.put("b.ts", "export const b = (;\n");
        assert!(graph.poll());
        assert!(matches!(graph.commit(l.lease, 950.0, Some(800.0)).unwrap(), Committed::Pending { .. }));
        assert!(graph.expire().is_empty());
        l.lease
    };
    let said = |graph: &Graph, why: &str| {
        let said = graph.expire();
        assert!(said.len() == 1 && said[0].starts_with("song.ts:4: the value wasn't saved: ") && said[0].ends_with(why), "{said:?}");
        assert!(graph.leases().is_empty());
    };
    let graph = Graph::silent();
    pending(&graph);
    graph.close();
    said(&graph, "the project was closed");

    pending(&graph);
    graph.open(&other.0).unwrap();
    said(&graph, "the project was closed");

    pending(&graph);
    fixture.write("import './b';\nexport default song(() => osc.sine(hz(810)));\n");
    fixture.put("b.ts", "export const b = 2;\n");
    assert!(graph.poll());
    said(&graph, "the code changed under this control");
    assert!(fixture.read("song.ts").contains("hz(810)"));

    fixture.write("import './b';\nexport default song(() => osc.sine(hz(800)));\n");
    let lease = pending(&graph);
    graph.cancel(lease).unwrap();
    said(&graph, "cancelled");
}

// WRITE-22, WRITE-12: a save that inserts a call above a dragged knob in its file renumbers it,
// and the drag goes on. The graph's state lists each gesture as its view names the control, so the
// page finds its drag's knob under its new id; between a settle's phases, the state still shows
// the view before, and the id it had there.
#[test]
fn the_state_names_each_drag_as_its_view_does() {
    let fixture = Fixture::new();
    fixture.write(&sine_and_saw(""));
    let graph = fixture.open();
    let (token, r0, saw) = knob(&graph, 300.0);
    graph.preview(token, r0, &saw, "g", 310.0).unwrap();
    let listed = |state: &Value| {
        let l = &state["leases"][0];
        (state["view"]["revision"].clone(), l["revision"].clone(), l["id"].clone(), l["gesture"].clone())
    };
    assert_eq!(listed(&graph.state().unwrap()), (json!(r0), json!(r0), json!(saw), json!("g")));
    let seen = Arc::new(Mutex::new(vec![]));
    let s = seen.clone();
    graph.inspect(move |a| a.between_phases = Some(Box::new(move |inner: &Inner| s.lock().push(inner.state().unwrap()))));
    fixture.write(&sine_and_saw("osc.tri(hz(1)), "));
    assert!(graph.poll());
    graph.inspect(|a| a.between_phases = None);
    assert_eq!(listed(&seen.lock()[0]), (json!(r0), json!(r0), json!(saw), json!("g")), "between the phases: the view before, its id");
    let (_, r1, moved) = knob(&graph, 300.0);
    assert_ne!(moved, saw, "renumbered");
    assert_eq!(listed(&graph.state().unwrap()), (json!(r1), json!(r1), json!(moved), json!("g")));
    assert_eq!(graph.leases()[0].state, "live", "the drag goes on");
    // The page goes on naming it as it began.
    graph.preview(token, r0, &saw, "g", 320.0).unwrap();
    graph.set(token, r0, &saw, "g", 330.0, Some(300.0)).unwrap();
    assert!(fixture.read("song.ts").contains("osc.tri(hz(1)), osc.sine(hz(800)), osc.saw(hz(330))"));
}

// WRITE-22, RENDER-22, PKG-3, D5: a knob a call leaves out (an inherited control, at its default)
// is dragged like any other: its updates are cell writes, heard with no new renderer and no
// message; its commit writes it into the call, and what plays takes that value edit as it is, by
// the same cells (the constant only moved from inherited to written).
#[test]
fn an_inherited_knob_is_dragged_and_its_commit_writes_it_in_with_no_new_renderer() {
    let a = "import { hz, track } from '@m2/core';\nimport { osc, vca } from 'std/dsp';\n\
             export const a = track('a', () => {\n  return vca(osc.square(hz(0)));\n});\n";
    let mut dc = Dc::with(a);
    assert_eq!(dc.blocks(4), vec![1.0; 4], "vca's gain plays its default, 1");
    let first = dc.generation();
    let (token, revision, id) = dc.graph.inspect(|a| {
        let o = a.open.as_ref().unwrap();
        let c = o.session.controls().into_iter().find(|c| !c.written && c.origin.slot() == Some("gain")).unwrap();
        (o.token, o.session.accepted().number, c.id)
    });
    assert!(id.ends_with("/@gain"), "{id}");
    let begun = dc.graph.begin(token, revision, &id, Owner::default()).unwrap();
    assert_eq!((begun.value, begun.cells, begun.home, begun.heard), (1.0, 1, m2_session::Home::Code, true));
    let p = player(&dc.graph);
    let seq = dc.graph.update(begun.lease, None, 0.4).unwrap().expect("a cell write");
    // A smooth control (PKG-14): one block ramps, then it holds.
    assert!(dc.blocks(3).iter().skip(1).all(|x| near(*x, 0.4)));
    assert!(dc.graph.applied(seq).is_some(), "a block heard it");
    assert_eq!(p.status().generation, first, "no new renderer for an update");
    match dc.graph.commit(begun.lease, 0.6, Some(1.0)).unwrap() {
        Committed::Written(v) => {
            let shown = (&v["file"], &v["before"], &v["after"]);
            assert_eq!(shown, (&json!("tracks/a.ts"), &json!("vca(osc.square(hz(0)))"), &json!("vca(osc.square(hz(0)), 0.6)")));
        }
        other => panic!("{other:?}"),
    }
    assert!(dc.fixture.read("tracks/a.ts").contains("vca(osc.square(hz(0)), 0.6)"));
    assert!(dc.blocks(3).iter().skip(1).all(|x| near(*x, 0.6)));
    assert_eq!(dc.generation(), first, "what plays took the first edit as a value edit");
    assert!(synced(&dc.graph));
    // The written knob is the same cell: a new drag of it plays at once.
    let (token, revision, id) = knob(&dc.graph, 0.6);
    let again = dc.graph.begin(token, revision, &id, Owner::default()).unwrap();
    dc.graph.update(again.lease, None, 0.2).unwrap();
    assert!(dc.blocks(3).iter().skip(1).all(|x| near(*x, 0.2)));
    dc.graph.cancel(again.lease).unwrap();
}

// RENDER-22, D10: one literal that reaches two slots is one control, and a lease on it writes
// every cell it reaches: both vcas here.
#[test]
fn a_lease_on_a_shared_literal_moves_every_cell() {
    let a = "import { hz, linear, track } from '@m2/core';\nimport { osc, vca } from 'std/dsp';\nimport { add } from 'std/math';\n\
             export const a = track('a', () => {\n  const g = linear(0.25);\n  return add(vca(osc.square(hz(0)), g), vca(osc.square(hz(0)), g));\n});\n";
    let mut dc = Dc::with(a);
    assert!(dc.blocks(2).iter().all(|x| near(*x, 0.5)));
    let (token, revision, id) = knob(&dc.graph, 0.25);
    let begun = dc.graph.begin(token, revision, &id, Owner::default()).unwrap();
    assert_eq!((begun.uses, begun.cells), (2, 2));
    dc.graph.update(begun.lease, None, 0.3).unwrap();
    assert!(dc.blocks(3).iter().skip(1).all(|x| near(*x, 0.6)), "both cells");
    assert_eq!(dc.graph.leases()[0].home, m2_session::Home::Code, "HOME-1");
    dc.graph.cancel(begun.lease).unwrap();
    assert!(dc.blocks(3).iter().skip(1).all(|x| near(*x, 0.5)), "put back in both");
}

/// A copy of the repository's project `rel`, open in a driven player and playing.
fn example(rel: &str) -> Dc {
    fn copy(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap().flatten() {
            let (f, t) = (e.path(), to.join(e.file_name()));
            if f.is_dir() { copy(&f, &t) } else { std::fs::copy(&f, &t).map(drop).unwrap() }
        }
    }
    let fixture = Fixture::new();
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel), &fixture.0);
    let (p, driver) = Player::driven(48_000);
    let graph = Graph::with_player(p);
    graph.open(&fixture.0).unwrap();
    let mut dc = Dc { fixture, graph, driver };
    dc.graph.transport("play").unwrap();
    dc.frames(40);
    dc
}

impl Dc {
    /// What the playing plan's activation kept and made.
    fn stats(&mut self) -> m2_runtime::Stats {
        self.driver.engine().unwrap().current().unwrap().stats().clone()
    }
}

// RENDER-27, RENDER-25 (§8.3): an edited audio clip is an activation of the same structure that
// keeps every instance: nothing is new, nothing fades but the lane, whose renders crossfade.
#[test]
fn an_audio_edit_is_an_activation_that_keeps_every_instance() {
    let mut dc = example("examples/resample");
    let first = dc.generation();
    let clips = dc.fixture.read("data/chops/clips.json");
    dc.fixture.put("data/chops/clips.json", &clips.replacen("\"gain_db\": 0", "\"gain_db\": -3", 1));
    assert!(dc.graph.poll());
    dc.frames(1);
    assert_ne!(dc.generation(), first, "an activation");
    assert!(dc.driver.engine().unwrap().in_window(), "the lane's renders crossfade");
    let s = dc.stats();
    assert_eq!((s.fresh.total(), s.ghosts, s.fades, s.resets), (0, 0, 0, 0), "{s:?}");
    assert!(s.kept.total() >= 5, "every kernel and voice pool: {s:?}");
}

// RENDER-27 (§8.4): a tempo change is an activation of the same structure: every instance kept,
// nothing fades, and the beat goes on.
#[test]
fn a_tempo_change_keeps_every_instance() {
    let mut dc = example("examples/resample");
    let before = player(&dc.graph).status().beats;
    dc.fixture.put("data/arrangement.json", "{\"tempo\": [{\"beat\": 0, \"bpm\": 100}], \"meter\": []}\n");
    assert!(dc.graph.poll());
    dc.frames(1);
    let s = dc.stats();
    assert_eq!((s.fresh.total(), s.ghosts, s.fades, s.resets), (0, 0, 0, 0), "{s:?}");
    let after = player(&dc.graph).status().beats;
    assert!(after > before && after - before < 0.1, "the beat goes on: {before} then {after}");
}

// HOST-5 (§10.3): a plugin that asks to be restarted is replaced at once, by the catch-up's
// activation: a new instance from its committed state, and the old one destroyed.
#[test]
fn a_restart_request_replaces_the_instance_from_its_committed_state() {
    let mut dc = with_probe();
    let first = dc.handles();
    m2_plugins::probe::stats(first[0]).unwrap().restart.store(true, Ordering::Relaxed);
    dc.frames(4);
    eventually("the restart is heard", || dc.graph.catch_up());
    dc.frames(8);
    let now = dc.handles();
    assert!(now.len() == 1 && now != first, "{now:?} after {first:?}");
    assert_eq!(dc.stats().fresh.plugins, 1);
    eventually("the old instance is destroyed", || dc.graph.inner.owner.live() == 1);
}

// RENDER-28 (§7.5): an activation the engine refuses (here, prepared against a plan the engine no
// longer plays: something else was activated behind the actor's back) leaves what plays playing,
// and the next settle recovers with a fresh engine at the same beat.
#[test]
fn a_refused_activation_recovers_by_replace() {
    let mut dc = Dc::new();
    // Behind the actor's back: a plan of what plays, prepared against it and activated.
    let other = dc.graph.inspect(|a| {
        let o = a.open.as_ref().unwrap();
        let p = o.plays.as_ref().unwrap();
        let (flat, data) = (&p.revision.compiled.flat, &p.revision.compiled.data);
        let restarts = std::collections::HashSet::new();
        let cont = m2_runtime::Continues::identity(flat);
        let keep = m2_runtime::Keep { old: flat, old_data: data, info: &p.info, cont: &cont, restarts: &restarts };
        let (mut cache, owner) = (m2_runtime::audio::Cache::default(), m2_plugins::Owner::discard());
        m2_runtime::Renderer::prepare(flat, data, &o.session.packages, 48_000, &mut cache, &owner, m2_runtime::Basis::Keep(keep)).unwrap()
    });
    player(&dc.graph).activate(other).unwrap();
    dc.frames(4);
    // The actor's next activation is prepared against the plan it sent last: refused.
    dc.fixture.put("tracks/b.ts", &B_SILENT.replace("vca(osc.saw(hz(110)), 0)", "vca(osc.saw(hz(220)), 0)"));
    dc.fixture.put("tracks/a.ts", &A_DC.replace("  return", "  const spare = osc.sine(hz(1));\n  return"));
    assert!(dc.graph.poll());
    dc.frames(4);
    assert_eq!(player(&dc.graph).refused(), 1);
    assert_eq!(dc.blocks(2), [0.5, 0.5], "what played plays on");
    // The catch-up waits while the actor is busy.
    eventually("a fresh engine replaces it", || dc.graph.catch_up());
    dc.frames(8);
    let (plays, generation, refused) = dc.graph.inspect(|a| {
        let o = a.open.as_ref().unwrap();
        (o.playing, o.plays.as_ref().unwrap().generation, o.plays.as_ref().unwrap().refused)
    });
    assert_eq!(dc.generation(), generation, "it plays what the actor sent");
    assert_eq!(plays, Some(playing(&dc.graph).1));
    assert_eq!(refused, player(&dc.graph).refused(), "and is in step");
    assert_eq!(dc.blocks(2), [0.5, 0.5]);
}

impl Fixture {
    /// Save `bytes` as the probe's state for key `key` of module `module`, with its sidecar
    /// (FMT-17), as the plugin's own save would.
    fn probe_state(&self, module: &str, key: &str, bytes: &[u8]) {
        self.put(&format!("data/{module}/{key}.state.json"), r#"{"plugin": "clap:m2.probe", "plugin_version": "1.0.0"}"#);
        std::fs::write(self.0.join(format!("data/{module}/{key}.state")), bytes).unwrap();
    }
}

/// The probe instance `h`'s state, saved now.
fn saved(h: m2_plugins::Handle) -> Vec<u8> {
    m2_plugins::host().save_state(h).recv().unwrap().unwrap().bytes
}

// HOST-5, ID-8, RENDER-26b (§3.2, §10.2): two keyed plugins, each with its own saved state, whose
// keys swap places in the code keep their instances, each following its key: none is created and
// none loads another's state, so each instance still saves its own key's state.
#[test]
fn swapping_two_plugin_keys_moves_the_instances() {
    m2_plugins::probe::install();
    let fixture = Fixture::new();
    let song = |first: &str, second: &str| {
        format!(
            "import {{ plugin, song }} from '@m2/core';\nimport {{ sum }} from 'std/math';\nexport default song(() => sum(\n  \
             plugin('clap:m2.probe', {{ key: '{first}' }}),\n  plugin('clap:m2.probe', {{ key: '{second}' }}),\n));\n"
        )
    };
    let (lead, pad) = (m2_plugins::probe::state("lead", 1.0), m2_plugins::probe::state("pad", 1.0));
    fixture.put("song.ts", &song("lead", "pad"));
    fixture.probe_state("song", "lead", &lead);
    fixture.probe_state("song", "pad", &pad);
    let (p, driver) = Player::driven(48_000);
    let graph = Graph::with_player(p);
    graph.open(&fixture.0).unwrap();
    let mut dc = Dc { fixture, graph, driver };
    dc.graph.transport("play").unwrap();
    dc.frames(4);
    let first = dc.handles();
    assert_eq!((saved(first[0]), saved(first[1])), (lead.clone(), pad.clone()), "each made from its key's state");
    // This session's own count: other tests' instances, made meanwhile, never add to it.
    let created = dc.graph.inner.owner.made();
    dc.fixture.put("song.ts", &song("pad", "lead"));
    assert!(dc.graph.poll());
    dc.frames(4);
    assert_eq!(dc.handles(), [first[1], first[0]], "each key kept its instance");
    assert_eq!(dc.graph.inner.owner.made(), created, "none created");
    assert_eq!(dc.stats().loads, 0, "none loaded another's state");
    assert_eq!((saved(first[0]), saved(first[1])), (lead, pad), "each holds its own key's state");
}

// RENDER-26b, HOST-5 (§10.2, §8.3): a plugin's state file changed from outside (an agent, an undo
// through the files) is an activation of the same structure that loads the new state into the
// instance that plays, on the plugin main thread: none is created. Its gain, which the code owns,
// is the code's again from the next block (HOME-4). The same file written again loads nothing.
#[test]
fn a_state_change_loads_into_the_running_instance() {
    m2_plugins::probe::install();
    let fixture = Fixture::new();
    fixture.put(
        "song.ts",
        "import { plugin, song, track } from '@m2/core';\n\
         const p = track('p', ({ lane }) => plugin('clap:m2.probe', { notes: lane.notes('n'), key: 'lead', gain_1: 0.5 }));\n\
         export default song(() => p.out);\n",
    );
    fixture.put(
        "data/p/n.json",
        r#"{"clips": [{"id": "a", "start": 0, "length": 64, "notes": [{"pitch": "C4", "start": 0, "length": 60}]}]}"#,
    );
    fixture.probe_state("p", "lead", &m2_plugins::probe::state("one", 0.25));
    let (p, driver) = Player::driven(48_000);
    let graph = Graph::with_player(p);
    graph.open(&fixture.0).unwrap();
    let mut dc = Dc { fixture, graph, driver };
    dc.graph.transport("play").unwrap();
    dc.frames(4);
    let first = dc.handles();
    let stats = m2_plugins::probe::stats(first[0]).unwrap();
    let tag = || stats.tag.lock().unwrap().clone();
    let gain = || match m2_plugins::host().main_call(first[0], m2_plugins::MainCall::Value(m2_plugins::probe::GAIN)) {
        Ok(m2_plugins::Reply::Value(Some(v))) => v,
        other => panic!("{other:?}"),
    };
    assert_eq!((tag(), gain()), ("one".to_string(), 0.5));
    let (made, generation) = (dc.graph.inner.owner.made(), dc.generation());
    let loads = stats.main_calls().iter().filter(|c| c.what == "load").count();
    dc.fixture.probe_state("p", "lead", &m2_plugins::probe::state("two", 0.75));
    assert!(dc.graph.poll());
    assert_eq!(tag(), "two", "loaded as the plan was prepared");
    dc.frames(4);
    assert_ne!(dc.generation(), generation, "an activation");
    assert_eq!(dc.handles(), first, "into the instance that plays");
    assert_eq!(dc.graph.inner.owner.made(), made, "none created");
    let s = dc.stats();
    assert_eq!((s.kept.plugins, s.fresh.total(), s.loads, s.ghosts), (1, 0, 1, 0), "{s:?}");
    let calls = stats.main_calls();
    let new: Vec<_> = calls.iter().filter(|c| c.what == "load").skip(loads).collect();
    assert!(new.len() == 1 && new[0].says_main && !new[0].says_audio, "one load, on the plugin main thread: {new:?}");
    assert_eq!(gain(), 0.5, "the code's gain again (HOME-4)");
    // The same bytes written again: nothing to load.
    let generation = dc.generation();
    dc.fixture.probe_state("p", "lead", &m2_plugins::probe::state("two", 0.75));
    dc.graph.poll();
    dc.frames(4);
    assert_eq!(stats.main_calls().iter().filter(|c| c.what == "load").count(), loads + 1);
    assert!(dc.generation() == generation || dc.stats().loads == 0);
}

// ---------------------------------------------------------------- exports that borrow (RENDER-1)

/// A song whose keyed probe (key `lead`, its state saved) plays two long notes, its gain read from
/// a control lane that holds 0.2 for 32 beats, then rises to 0.8 at beat 64, where its clip and
/// the song end: an export leaves the plugin at the lane's default, 0.5. Open in the app's state
/// on a driven player, playing, four blocks in. The test's thread is the audio thread: it fills.
struct Ex {
    fixture: Fixture,
    state: Arc<crate::AppState>,
    driver: Driver,
}

/// Its notes: two held from beat 0 to 60, so a later edit of the lane fits the room it has.
const EX_NOTES: &str = r#"{"clips": [{"id": "a", "start": 0, "length": 64, "notes": [{"pitch": "C4", "start": 0, "length": 60}, {"pitch": "G4", "start": 0, "length": 60}]}]}"#;

impl Ex {
    fn new() -> Ex {
        Ex::tagged("lead")
    }

    /// [`Ex::new`], its probe's saved state tagged `tag` ([`m2_plugins::probe::tagged`]).
    fn tagged(tag: &str) -> Ex {
        Ex::with(&Ex::song("p.out"), &m2_plugins::probe::state(tag, 1.0))
    }

    /// [`Ex::new`] with `song` as song.ts, and `state` as its probe's saved state.
    fn with(song: &str, state: &[u8]) -> Ex {
        m2_plugins::probe::install();
        let fixture = Fixture::new();
        fixture.put("song.ts", song);
        fixture.put("data/p/n.json", EX_NOTES);
        fixture.put(
            "data/p/g.json",
            r#"{"unit": "scalar", "clips": [{"id": "r", "start": 0, "length": 64, "points": [[0, 0.2], [32, 0.2], [64, 0.8]]}]}"#,
        );
        fixture.probe_state("p", "lead", state);
        let (p, driver) = Player::driven(48_000);
        let state = Arc::new(crate::AppState::new(String::new(), Graph::with_player(p)));
        state.graph.open(&fixture.0).unwrap();
        state.graph.transport("play").unwrap();
        let mut ex = Ex { fixture, state, driver };
        ex.fill(4);
        ex
    }

    /// song.ts, its output `out` (of `p.out`).
    fn song(out: &str) -> String {
        format!(
            "import {{ plugin, song, track }} from '@m2/core';\nimport {{ vca }} from 'std/dsp';\n\
             const p = track('p', ({{ lane }}) =>\n  plugin('clap:m2.probe', {{ notes: lane.notes('n'), key: 'lead', gain_1: lane.control('g', 'scalar', 0.5) }}),\n);\n\
             export default song(() => {out});\n"
        )
    }

    /// `n` device buffers of a block, as the audio thread fills them.
    fn fill(&mut self, n: usize) {
        for _ in 0..n {
            let mut buf = [0.0f32; 256];
            self.driver.fill(&mut buf, 2);
            drop(self.driver.drain_retired());
        }
    }

    fn handles(&mut self) -> Vec<m2_plugins::Handle> {
        self.driver.engine().unwrap().current().unwrap().plugin_handles()
    }

    fn engine(&mut self) -> &mut m2_runtime::Engine {
        self.driver.engine().unwrap()
    }

    fn hooks(&self) -> &crate::export::Hooks {
        &self.state.graph.inner.export_hooks
    }

    /// The export's thread stops inside its window, once it has taken the instances, until the
    /// sender returned is sent to; the receiver returned hears when it got there.
    fn stop_inside(&self) -> (mpsc::Sender<()>, mpsc::Receiver<()>) {
        let (to_test, at_test) = mpsc::channel();
        let (to_export, at_export) = mpsc::channel::<()>();
        *self.hooks().inside.lock() = Some(Box::new(move || {
            to_test.send(()).unwrap();
            let _ = at_export.recv_timeout(Duration::from_secs(30));
        }));
        (to_export, at_test)
    }

    /// Fill, as the device does, until `done`.
    fn fill_until(&mut self, what: &str, mut done: impl FnMut(&mut Ex) -> bool) {
        let t = Instant::now();
        while !done(self) {
            assert!(t.elapsed() < Duration::from_secs(30), "timed out waiting until {what}");
            self.fill(1);
            std::thread::sleep(Duration::from_micros(200));
        }
    }

    /// The probe's gain as the plugin reads it.
    fn gain(&self, h: m2_plugins::Handle) -> f64 {
        match m2_plugins::host().main_call(h, m2_plugins::MainCall::Value(m2_plugins::probe::GAIN)) {
            Ok(m2_plugins::Reply::Value(Some(v))) => v,
            other => panic!("{other:?}"),
        }
    }

    /// What the control lane `g` reads at `beat`.
    fn ramp(beat: f64) -> f64 {
        0.2 + 0.6 * ((beat - 32.0) / 32.0).clamp(0.0, 1.0)
    }
}

/// The calls among `calls` from `from` on that move an instance between the engine and an export.
fn moves(calls: &[m2_plugins::probe::Call], from: usize) -> Vec<&'static str> {
    let kinds = ["deactivate", "activate", "load", "save", "render_offline", "render_realtime"];
    calls[from..].iter().map(|c| c.what).filter(|w| kinds.contains(w)).collect()
}

// G7e (step 7 design §12.5), RENDER-1, RENDER-6a, HOME-4, HOME-6: an export of a song whose plugin
// plays borrows its live instance, on the export's thread. In order: the engine holds, and says
// so after its callback; the instance is deactivated, not loaded again (it holds its committed
// state, HOME-6), set to offline mode and activated, and reset on the export's thread at its first
// call; after the render it is deactivated, set to realtime and activated. No save, no second load.
// No `process` call comes from the audio thread between the first deactivate and the last
// activate, and the engine's clock doesn't move meanwhile. The export equals one made with new
// instances, bit for bit; the instance is the same and none is made; after the resume its gain
// reads the code's value where playback is (`reloaded`), not where the export ended. The negative
// control: with no hold, the audio thread calls `process` inside the window.
#[test]
fn an_export_borrows_and_gives_back_the_live_instances() {
    let mut ex = Ex::new();
    let first = ex.handles();
    let stats = m2_plugins::probe::stats(first[0]).unwrap();
    let (made, before) = (ex.state.graph.inner.owner.made(), stats.main_calls().len());
    let (audio, resets) = (m2_os::thread::token(), stats.resets.load(Ordering::Relaxed));
    let gain = ex.gain(first[0]);
    assert!((gain - Ex::ramp(player(&ex.state.graph).status().beats)).abs() < 0.01, "the lane's gain: {gain}");
    let borrowed = ex.fixture.0.join("borrowed.wav");
    let mut events = ex.state.events.subscribe();
    crate::export::start(&ex.state, Some(borrowed.clone())).unwrap();
    let (mut paused, mut clocks) = (false, vec![]);
    ex.fill_until("the export is done", |ex| {
        if ex.state.graph.activity().paused == Some("exporting") {
            paused = true;
            clocks.push(player(&ex.state.graph).clock());
        }
        !ex.state.exports.running()
    });
    let said = crate::export::tests::until_finished(&mut events);
    assert!(said.iter().all(|e| e["paused"] == true), "{said:?}");
    assert_eq!(said.last().unwrap()["done"], true, "{said:?}");
    ex.fill(4);

    // The hold, published, and nothing moved while it held.
    assert!(paused && clocks.windows(2).all(|w| w[0] == w[1]), "paused: {paused}; engine clocks while held {clocks:?}");
    // The instance, out and back, on the plugin main thread.
    let calls = stats.main_calls();
    assert_eq!(
        moves(&calls, before),
        ["deactivate", "render_offline", "activate", "deactivate", "render_realtime", "activate"],
        "{calls:?}"
    );
    assert!(calls[before..].iter().filter(|c| c.what != "stop_processing").all(|c| c.says_main && !c.says_audio), "{calls:?}");
    let tick = |what: &str, last: bool| {
        let mut it = calls[before..].iter().filter(|c| c.what == what).map(|c| c.tick);
        if last { it.next_back() } else { it.next() }.unwrap()
    };
    let (from, to) = (tick("deactivate", false), tick("activate", true));
    let inside: Vec<(u64, usize)> = stats.process_calls().into_iter().filter(|(t, _)| *t > from && *t < to).collect();
    assert!(!inside.is_empty() && inside.iter().all(|(_, th)| *th != audio), "the export processed it, the audio thread didn't");
    let (at, on) = (stats.reset_at.0.load(Ordering::Relaxed), stats.reset_at.1.load(Ordering::Relaxed));
    assert_eq!(stats.resets.load(Ordering::Relaxed), resets + 1, "reset once");
    assert!(at > tick("activate", false) && at < to && on == inside[0].1, "reset on the export's thread, at its first call");
    // The same instance, none made; back on the audio thread.
    assert_eq!((ex.handles(), ex.state.graph.inner.owner.made()), (first.clone(), made));
    assert!(stats.process_calls().last().unwrap().1 == audio, "played live again");
    // HOME-4: its gain is the code's again, where playback is (unchanged since before the export,
    // so sent again only because it was lent), not where the export ended.
    let gain = ex.gain(first[0]);
    let beat = player(&ex.state.graph).status().beats;
    assert!((gain - Ex::ramp(beat)).abs() < 1e-6 && gain < 0.5, "gain {gain} at beat {beat}");

    // The same file as an export with new instances.
    let (_, _, fresh, _) = ex.state.graph.render(Length::Song).unwrap();
    let fresh_wav = ex.fixture.0.join("fresh.wav");
    fresh.write(&fresh_wav, |_| {}).unwrap();
    let samples = |p: &Path| hound::WavReader::open(p).unwrap().into_samples::<i32>().map(Result::unwrap).collect::<Vec<i32>>();
    let (a, b) = (samples(&borrowed), samples(&fresh_wav));
    assert!(a.len() > 48_000 && a.iter().any(|x| *x != 0), "{} samples", a.len());
    assert!(a == b, "the borrowed export is the fresh one, bit for bit");

    // The negative control: no hold, and the audio thread plays one buffer inside the window
    // while the export's thread waits there. No two threads ever call the instance at once.
    let before = stats.main_calls().len();
    ex.hooks().no_hold.store(true, Ordering::SeqCst);
    let (go, there) = ex.stop_inside();
    crate::export::start(&ex.state, Some(ex.fixture.0.join("unheld.wav"))).unwrap();
    there.recv_timeout(Duration::from_secs(10)).expect("the export took the instance");
    ex.fill(1);
    go.send(()).unwrap();
    eventually("the unheld export is done", || !ex.state.exports.running());
    ex.hooks().no_hold.store(false, Ordering::SeqCst);
    let calls = stats.main_calls();
    let (from, to) = (
        calls[before..].iter().find(|c| c.what == "deactivate").unwrap().tick,
        calls[before..].iter().rfind(|c| c.what == "activate").unwrap().tick,
    );
    let audio_inside = stats.process_calls().iter().filter(|(t, th)| *t > from && *t < to && *th == audio).count();
    assert!(audio_inside > 0, "with no hold, the audio thread calls the instance inside the window");
}

// RENDER-1, R6 (step 7 design §10.5): while an export borrows the live instances, settles prepare
// nothing and send no activation, so no state load or other host call reaches them: a structural
// edit and a new state wait, while the engine holds. Once they are given back, one settle plays
// what waited: one activation, of the latest revision, keeping the instance and loading its state.
#[test]
fn settles_during_an_export_wait_then_apply_once() {
    let mut ex = Ex::new();
    let first = ex.handles();
    let stats = m2_plugins::probe::stats(first[0]).unwrap();
    let loads = || stats.main_calls().iter().filter(|c| c.what == "load").count();
    let (activations, generation, loaded) = (ex.engine().activations(), ex.engine().current().unwrap().generation(), loads());
    let (go, there) = ex.stop_inside();
    crate::export::start(&ex.state, Some(ex.fixture.0.join("out.wav"))).unwrap();
    ex.fill_until("the export took the instance", |_| there.try_recv().is_ok());

    // Inside the window: a structural edit and a new state, each taken in.
    ex.fixture.put("song.ts", &Ex::song("vca(p.out, 0.5)"));
    assert!(ex.state.graph.poll());
    ex.fixture.probe_state("p", "lead", &m2_plugins::probe::state("two", 1.0));
    ex.state.graph.poll();
    ex.fill(8);
    let (plays, accepted) = playing(&ex.state.graph);
    assert!(plays < accepted && ex.state.graph.inspect(|a| a.open.as_ref().unwrap().exporting), "{plays} plays, {accepted} accepted");
    assert_eq!((ex.engine().activations(), loads(), stats.tag.lock().unwrap().clone()), (activations, loaded, "lead".to_string()));
    assert!(ex.engine().is_held());

    go.send(()).unwrap();
    ex.fill_until("the export is done", |ex| !ex.state.exports.running());
    ex.fill_until("what waited plays", |ex| synced(&ex.state.graph));
    ex.fill(8);
    assert_eq!(ex.engine().activations(), activations + 1, "one activation");
    let cur = ex.engine().current().unwrap();
    assert!(cur.generation() != generation && cur.plugin_handles() == first, "the latest plan, with the same instance");
    assert_eq!((cur.stats().loads, stats.tag.lock().unwrap().clone()), (1, "two".to_string()), "its state loaded then");
    assert!(!ex.state.graph.inspect(|a| a.open.as_ref().unwrap().exporting));
}

// RENDER-1, RENDER-26, PLANE-5 (step 7 design §10.5): while an export holds playback, a notes edit's
// preview still reaches the engine: its `SwapLane` touches no plugin, so it applies while held,
// and the new notes play from the resume.
#[test]
fn lane_previews_play_during_an_export() {
    let mut ex = Ex::new();
    let (go, there) = ex.stop_inside();
    crate::export::start(&ex.state, Some(ex.fixture.0.join("out.wav"))).unwrap();
    ex.fill_until("the export took the instance", |_| there.try_recv().is_ok());
    let (generation, clock) = (ex.engine().current().unwrap().generation(), player(&ex.state.graph).clock());
    let (token, revision) = ex.state.graph.inspect(|a| {
        let o = a.open.as_ref().unwrap();
        (o.token, o.session.accepted().number)
    });
    // The G4 becomes an E4 from beat 1: the same room.
    let clips: Vec<NoteClip> =
        serde_json::from_str::<Value>(EX_NOTES).map(|v| serde_json::from_value(v["clips"].clone()).unwrap()).unwrap();
    let mut edited = clips.clone();
    edited[0].notes[1].pitch = 64.0;
    edited[0].notes[1].start = 1.0;
    edited[0].notes[1].length = 58.0;
    ex.state.graph.write_notes(token, revision, "p", "n", &edited).unwrap();
    let seq = ex.state.graph.last_edit().and_then(|t| t.seq).expect("sent to the engine");
    ex.fill(2);
    assert!(player(&ex.state.graph).applied(seq).is_some(), "the swap applied while held");
    assert!(ex.engine().is_held() && player(&ex.state.graph).clock() == clock, "still held");
    assert_eq!(ex.engine().current().unwrap().generation(), generation, "no new plan");

    go.send(()).unwrap();
    ex.fill_until("the export is done", |ex| !ex.state.exports.running());
    // Played on from beat 0.01 or so: at beat 1.5 the C4 and the new E4 sound.
    ex.fill_until("beat 1.5", |ex| player(&ex.state.graph).status().beats > 1.5);
    let keys: Vec<i16> = ex.engine().current().unwrap().plugins()[0].notes.iter().map(|n| n.1).collect();
    assert_eq!(keys, [60, 64], "the new notes play");
}

// RENDER-25 (step 7 design §10.4): while the plan for an edit creates a plugin instance (here a
// second probe, whose saved state takes 300 ms to load), the engine view and the status say which
// plugin is loading, and the old plan plays meanwhile; once it is made, nothing is.
#[test]
fn loading_is_published_while_a_plugin_is_created() {
    let mut dc = with_probe();
    let song = dc.fixture.read("song.ts");
    let probe = "plugin('clap:m2.probe', { notes: lane.notes('n'), gain_1: 0.5 })";
    let slow = song.replace(probe, &format!("plugin('clap:m2.probe', {{ in: {probe}, key: 'slow' }})"));
    assert_ne!(slow, song);
    dc.fixture.probe_state("p", "slow", b"slow: a big patch");
    dc.fixture.put("song.ts", &slow);
    let Dc { graph, driver, .. } = &mut dc;
    let (seen, viewed, heard) = std::thread::scope(|s| {
        let poll = s.spawn(|| graph.poll());
        let (mut seen, mut viewed, mut heard) = (vec![], Value::Null, 0);
        while !poll.is_finished() {
            let a = graph.activity();
            if !a.loading.is_empty() {
                // The load can end between the two reads: a pair is kept while both show it.
                let shown = graph.state().unwrap()["loading"].clone();
                if shown != json!([]) {
                    (seen, viewed) = (a.loading, shown);
                }
                // The device plays on meanwhile.
                let mut buf = [0.0f32; 256];
                driver.fill(&mut buf, 2);
                heard += 1;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(poll.join().unwrap());
        (seen, viewed, heard)
    });
    assert_eq!((seen, viewed), (vec!["m2 probe".to_string()], json!(["m2 probe"])));
    assert!(heard > 10, "the old plan played meanwhile: {heard} buffers");
    assert!(graph.activity().loading.is_empty() && graph.state().unwrap()["loading"] == json!([]));
}

// RENDER-1 (step 7 design §10.5, failures): a borrowed instance whose host calls fail (here, it
// refuses to activate for the export) is given back at once, and a new instance from the committed
// state plays its node in the export, which carries `plugin.not-borrowed`: the file is still the
// one new instances make, bit for bit. The live instance plays on after the resume.
#[test]
fn an_instance_that_cant_be_borrowed_plays_anew_in_the_export() {
    // A tag no other test's probe has: this test counts the instances made from it.
    let mut ex = Ex::tagged("anew");
    let first = ex.handles();
    let stats = m2_plugins::probe::stats(first[0]).unwrap();
    let (before, made) = (stats.main_calls().len(), m2_plugins::probe::tagged("anew").len());
    let out = ex.fixture.0.join("out.wav");
    let mut events = ex.state.events.subscribe();
    stats.refuse_activate.store(true, Ordering::Relaxed);
    crate::export::start(&ex.state, Some(out.clone())).unwrap();
    ex.fill_until("the export is done", |ex| !ex.state.exports.running());
    let done = crate::export::tests::until_finished(&mut events).pop().unwrap();
    assert_eq!(done["done"], true, "{done}");
    let problems: Vec<&str> = done["problems"].as_array().unwrap().iter().map(|p| p["code"].as_str().unwrap()).collect();
    assert_eq!(problems, ["plugin.not-borrowed"], "{done}");
    let calls = stats.main_calls();
    // The refused activate left it inactive: given back, it has nothing to deactivate.
    assert_eq!(
        moves(&calls, before),
        ["deactivate", "render_offline", "activate", "render_realtime", "activate"],
        "taken, refused, given back at once: {calls:?}"
    );
    let anew = m2_plugins::probe::tagged("anew");
    assert_eq!(anew.len(), made + 1, "one new instance for the export");
    let fresh = anew.last().unwrap();
    assert!(fresh.process.load(Ordering::Relaxed) > 0, "it played the export");
    eventually("the new instance is destroyed", || fresh.main_calls().iter().any(|c| c.what == "destroy"));
    let (_, _, own, _) = ex.state.graph.render(Length::Song).unwrap();
    let own_wav = ex.fixture.0.join("own.wav");
    own.write(&own_wav, |_| {}).unwrap();
    let samples = |p: &Path| hound::WavReader::open(p).unwrap().into_samples::<i32>().map(Result::unwrap).collect::<Vec<i32>>();
    assert!(samples(&out) == samples(&own_wav), "what new instances make, bit for bit");
    let played = stats.process.load(Ordering::Relaxed);
    ex.fill(4);
    assert_eq!((ex.handles(), stats.process.load(Ordering::Relaxed)), (first, played + 4), "the live instance plays on");
}

// RENDER-1 (step 7 design §10.5): when playback doesn't pause within 2 s (its device stopped
// calling back), the export touches no live instance: it renders beside playback with instances of
// its own, says so, and the session plays on. When the device calls back again, it isn't held.
#[test]
fn an_export_whose_playback_cant_pause_renders_beside_it() {
    let mut ex = Ex::new();
    let first = ex.handles();
    let stats = m2_plugins::probe::stats(first[0]).unwrap();
    let before = stats.main_calls().len();
    let mut events = ex.state.events.subscribe();
    let t = Instant::now();
    // Nothing fills: the device is stalled.
    crate::export::start(&ex.state, Some(ex.fixture.0.join("out.wav"))).unwrap();
    eventually("the export is done", || !ex.state.exports.running());
    assert!(t.elapsed() >= Duration::from_secs(2), "it waited 2 s for the pause");
    let mut said = vec![];
    let start = Instant::now();
    while said.last().is_none_or(|e: &Value| e["t"] != "export" || e.get("progress").is_some()) {
        assert!(start.elapsed() < Duration::from_secs(10), "{said:?}");
        if let Ok(text) = events.try_recv() {
            said.push(serde_json::from_str::<Value>(&text).unwrap());
        }
    }
    let done = said.last().unwrap();
    assert!(done["done"] == true && done["paused"] == false, "{done}");
    assert!(said.iter().any(|e| e["t"] == "toast" && e["text"].as_str().unwrap().contains("couldn't pause")), "{said:?}");
    assert!(moves(&stats.main_calls(), before).is_empty(), "the live instance was never touched");
    assert!(!ex.state.graph.inspect(|a| a.open.as_ref().unwrap().exporting), "the session plays on");
    ex.fill(8);
    assert!(!ex.engine().is_held() && !player(&ex.state.graph).held(), "the hold and the resume, in order");
    let played = stats.process.load(Ordering::Relaxed);
    ex.fill(4);
    assert_eq!(stats.process.load(Ordering::Relaxed), played + 4);
}

// RENDER-1: an export started right after another waits for its own hold. The first's resume has
// been sent but no callback has taken it, so the engine still says it is held, from the first's
// hold: the second takes nothing until the engine has taken its own hold too.
#[test]
fn a_second_export_waits_for_its_own_hold() {
    let mut ex = Ex::tagged("twice");
    let stats = m2_plugins::probe::stats(ex.handles()[0]).unwrap();
    crate::export::start(&ex.state, Some(ex.fixture.0.join("one.wav"))).unwrap();
    // Filled only until the engine holds: the first's resume is then sent, and no callback takes it.
    ex.fill_until("the engine holds", |ex| player(&ex.state.graph).held());
    eventually("the first export is done", || !ex.state.exports.running());
    assert!(player(&ex.state.graph).held(), "no callback has taken the resume yet");
    let before = stats.main_calls().len();
    crate::export::start(&ex.state, Some(ex.fixture.0.join("two.wav"))).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert!(moves(&stats.main_calls(), before).is_empty(), "nothing taken on the first's hold");
    ex.fill_until("the second export is done", |ex| !ex.state.exports.running());
    assert_eq!(moves(&stats.main_calls(), before).len(), 6, "taken and given back once the engine held again");
}

// RENDER-1, HOME-4: a borrowed instance that doesn't hold its committed state (here it saved its
// own since) has it loaded for the export; a plugin that applies a loaded state at its next call,
// ending its voices (the probe's `defer_load`, as Surge XT does), is given that call before the
// render, so the export's first notes and the code's values aren't lost: the export equals one
// made with new instances, bit for bit. After the resume, the code's gain wins again.
#[test]
fn an_export_that_loads_a_state_lets_the_plugin_apply_it_first() {
    let mut ex = Ex::tagged("applied");
    let first = ex.handles();
    let stats = m2_plugins::probe::stats(first[0]).unwrap();
    stats.defer_load.store(true, Ordering::Relaxed);
    m2_plugins::host().save_state(first[0]).recv().unwrap().unwrap();
    let before = stats.main_calls().len();
    let out = ex.fixture.0.join("out.wav");
    crate::export::start(&ex.state, Some(out.clone())).unwrap();
    ex.fill_until("the export is done", |ex| !ex.state.exports.running());
    ex.fill(4);
    assert_eq!(
        moves(&stats.main_calls(), before),
        ["deactivate", "load", "render_offline", "activate", "deactivate", "render_realtime", "activate"],
        "its committed state loaded"
    );
    let (_, _, fresh, _) = ex.state.graph.render(Length::Song).unwrap();
    let fresh_wav = ex.fixture.0.join("fresh.wav");
    fresh.write(&fresh_wav, |_| {}).unwrap();
    let samples = |p: &Path| hound::WavReader::open(p).unwrap().into_samples::<i32>().map(Result::unwrap).collect::<Vec<i32>>();
    let (a, b) = (samples(&out), samples(&fresh_wav));
    assert!(a.iter().any(|x| *x != 0) && a == b, "the export is the fresh one, bit for bit");
    let gain = ex.gain(first[0]);
    let beat = player(&ex.state.graph).status().beats;
    assert!((gain - Ex::ramp(beat)).abs() < 1e-6, "the code's gain again: {gain} at beat {beat}");
}

// RENDER-1: an export borrows an instance while playback's notes still sound in it. It holds its
// committed state, so nothing is loaded (HOME-6, the usual path), and CLAP's `reset` doesn't end
// the notes in every plugin (the probe's `reset_keeps_voices`, as Surge XT's keeps its voices), so a
// call before the render ends every key: none of playback's notes is in the file, which equals one
// made with new instances, bit for bit. Without that call, playback's two notes rang through the
// export and on after its end (with Surge XT, G7e's tail 60 dB over m2 render's).
#[test]
fn an_export_ends_the_notes_playback_left_sounding() {
    let mut ex = Ex::tagged("ringing");
    let first = ex.handles();
    let stats = m2_plugins::probe::stats(first[0]).unwrap();
    stats.reset_keeps_voices.store(true, Ordering::Relaxed);
    assert_eq!(stats.sounding.load(Ordering::Relaxed), 2, "playback's two notes sound");
    let before = stats.main_calls().len();
    let out = ex.fixture.0.join("out.wav");
    crate::export::start(&ex.state, Some(out.clone())).unwrap();
    ex.fill_until("the export is done", |ex| !ex.state.exports.running());
    assert!(!moves(&stats.main_calls(), before).contains(&"load"), "it held its committed state: nothing loaded");
    let (_, _, fresh, _) = ex.state.graph.render(Length::Song).unwrap();
    let fresh_wav = ex.fixture.0.join("fresh.wav");
    fresh.write(&fresh_wav, |_| {}).unwrap();
    let samples = |p: &Path| hound::WavReader::open(p).unwrap().into_samples::<i32>().map(Result::unwrap).collect::<Vec<i32>>();
    let (a, b) = (samples(&out), samples(&fresh_wav));
    assert!(a.iter().any(|x| *x != 0) && a == b, "the export is the fresh one, bit for bit");
}

// RENDER-1, RENDER-28 (bug 1): the output moves to another device while an export has playback's
// instance. The player is let go as any changed one is, and the action that met it says why; the
// export keeps what it borrowed. Nothing loads meanwhile: no instance is made and nothing else calls
// this one. The export finishes as one made with new instances, bit for bit, and gives the instance
// back; then the project loads into a new player where playback held, with an instance of its own,
// and the borrowed one is destroyed.
#[test]
fn a_device_change_during_an_export_loads_a_new_player_after_it() {
    // A tag no other test's probe has: this test counts the instances made from it.
    let mut ex = Ex::tagged("moved");
    let first = ex.handles();
    let stats = m2_plugins::probe::stats(first[0]).unwrap();
    let (before, audio) = (stats.main_calls().len(), m2_os::thread::token());
    let epoch = |ex: &Ex| ex.state.graph.inspect(|a| a.open.as_ref().unwrap().plays.as_ref().map(|p| p.epoch));
    let first_epoch = epoch(&ex).expect("it plays");
    let out = ex.fixture.0.join("out.wav");
    let (go, there) = ex.stop_inside();
    crate::export::start(&ex.state, Some(out.clone())).unwrap();
    ex.fill_until("the export took the instance", |_| there.try_recv().is_ok());
    let beat = player(&ex.state.graph).status().beats;
    ex.driver.error(DeviceErrorKind::DeviceChanged.into());
    let e = ex.state.graph.transport("stop").unwrap_err();
    assert!(e.to_string().starts_with("the sound output moved to another device"), "{e:#}");
    ex.state.graph.poll();
    ex.fill(4);
    assert_eq!(epoch(&ex), Some(first_epoch), "nothing loads while the export has the instance");
    assert_eq!(m2_plugins::probe::tagged("moved").len(), 1, "no instance made meanwhile");
    go.send(()).unwrap();
    ex.fill_until("the export is done", |ex| !ex.state.exports.running());

    // Given back, then destroyed: the player that played it is gone.
    eventually("the borrowed instance is destroyed", || stats.main_calls().iter().any(|c| c.what == "destroy"));
    let calls = stats.main_calls();
    assert_eq!(
        moves(&calls, before),
        ["deactivate", "render_offline", "activate", "deactivate", "render_realtime", "activate", "deactivate"],
        "taken, given back, then deactivated to be destroyed: {calls:?}"
    );
    let (from, to) = (
        calls[before..].iter().find(|c| c.what == "deactivate").unwrap().tick,
        calls[before..].iter().rfind(|c| c.what == "activate").unwrap().tick,
    );
    let inside: Vec<(u64, usize)> = stats.process_calls().into_iter().filter(|(t, _)| *t > from && *t < to).collect();
    assert!(!inside.is_empty() && inside.iter().all(|(_, th)| *th != audio), "only the export called it in its window");
    assert!(stats.process_calls().iter().all(|(t, _)| *t < to), "the player let go never played it again");
    // The new player, where playback held, stopped, with a new instance from the committed state.
    eventually("a new player plays the project", || epoch(&ex) == Some(first_epoch + 1));
    eventually("where playback held", || (ex.state.graph.status().beat - beat).abs() < 1e-3);
    assert_eq!(player(&ex.state.graph).device, "none", "a new player (silent in tests)");
    let made = m2_plugins::probe::tagged("moved");
    assert_eq!(made.len(), 2, "one new instance, for the new player");
    eventually("it plays", || made[1].process.load(Ordering::Relaxed) > 0);

    let (_, _, fresh, _) = ex.state.graph.render(Length::Song).unwrap();
    let fresh_wav = ex.fixture.0.join("fresh.wav");
    fresh.write(&fresh_wav, |_| {}).unwrap();
    let samples = |p: &Path| hound::WavReader::open(p).unwrap().into_samples::<i32>().map(Result::unwrap).collect::<Vec<i32>>();
    let (a, b) = (samples(&out), samples(&fresh_wav));
    assert!(a.iter().any(|x| *x != 0) && a == b, "the export is the fresh one, bit for bit");
}

// ---------------------------------------------------------------- the step 7 review's fixes

/// The samples of the WAV file at `p`.
fn wav(p: &Path) -> Vec<i32> {
    hound::WavReader::open(p).unwrap().into_samples::<i32>().map(Result::unwrap).collect()
}

/// The song the session exports now, as `m2 render` makes it: every instance new.
fn rendered(ex: &Ex) -> Vec<i32> {
    let (_, _, fresh, _) = ex.state.graph.render(Length::Song).unwrap();
    let at = ex.fixture.0.join("rendered.wav");
    fresh.write(&at, |_| {}).unwrap();
    wav(&at)
}

/// Export the open song to `out.wav` as the app does, filling meanwhile; its `done` or `error` event.
fn export_now(ex: &mut Ex) -> (PathBuf, Value) {
    let out = ex.fixture.0.join("out.wav");
    let mut events = ex.state.events.subscribe();
    crate::export::start(&ex.state, Some(out.clone())).unwrap();
    ex.fill_until("the export is done", |ex| !ex.state.exports.running());
    (out, crate::export::tests::until_finished(&mut events).pop().unwrap())
}

/// A probe parameter's value as the plugin reads it.
fn param(h: m2_plugins::Handle, id: u32) -> f64 {
    match m2_plugins::host().main_call(h, m2_plugins::MainCall::Value(id)) {
        Ok(m2_plugins::Reply::Value(Some(v))) => v,
        other => panic!("{other:?}"),
    }
}

// HOME-5a, RENDER-1 (plug-1 of the step 7 review): a plugin parameter the code stops setting keeps,
// live, the value it was last sent, which no file records. An export then doesn't borrow that
// instance: its node plays a new one from the committed state, so the file is the one `m2 render`
// makes, bit for bit, and the live instance keeps its value. Before, the export borrowed it and
// baked in the released value (the level 0.05, where `m2 render` plays the saved 0.4).
#[test]
fn an_export_after_a_released_parameter_is_what_m2_render_makes() {
    let song = |more: &str| {
        format!(
            "import {{ plugin, song, track }} from '@m2/core';\n\
             const p = track('p', ({{ lane }}) => plugin('clap:m2.probe', {{ notes: lane.notes('n'), key: 'lead'{more} }}));\n\
             export default song(() => p.out);\n"
        )
    };
    let mut ex = Ex::with(&song(", level_2: 0.05"), &m2_plugins::probe::state_with("released", 1.0, 0.4));
    let first = ex.handles();
    let stats = m2_plugins::probe::stats(first[0]).unwrap();
    assert_eq!(param(first[0], m2_plugins::probe::LEVEL), 0.05, "the code's level");
    ex.fixture.put("song.ts", &song(""));
    assert!(ex.state.graph.poll());
    ex.fill(4);
    assert!(synced(&ex.state.graph) && ex.handles() == first, "the same instance plays the edit");
    assert_eq!(param(first[0], m2_plugins::probe::LEVEL), 0.05, "it keeps the value it was last sent (HOME-5a)");
    let before = stats.main_calls().len();
    let (out, done) = export_now(&mut ex);
    assert!(done["done"] == true && done["paused"] == false, "{done}");
    assert!(moves(&stats.main_calls(), before).is_empty(), "the live instance wasn't borrowed");
    let (a, b) = (wav(&out), rendered(&ex));
    assert!(a.iter().any(|x| *x != 0) && a == b, "the export is what m2 render makes, bit for bit");
    assert_eq!(param(first[0], m2_plugins::probe::LEVEL), 0.05, "live keeps it");
}

// HOST-5, RENDER-1 (plug-2 of the step 7 review): a borrowed plugin that won't come back active
// after an export (here it refuses the activate) is marked failed before playback resumes: its node
// plays silence and never calls it, so playback goes on with no crash, the user is told, and the
// settle after the export replaces it from its committed state, which plays. Before, the inactive
// instance's error stopped the engine (a crash, the transport stopped), and broke its replacement.
#[test]
fn a_plugin_that_doesnt_come_back_after_an_export_is_replaced_as_playback_goes_on() {
    let mut ex = Ex::tagged("revived");
    let first = ex.handles();
    let stats = m2_plugins::probe::stats(first[0]).unwrap();
    let (go, there) = ex.stop_inside();
    crate::export::start(&ex.state, Some(ex.fixture.0.join("out.wav"))).unwrap();
    ex.fill_until("the export took the instance", |_| there.try_recv().is_ok());
    stats.refuse_activate.store(true, Ordering::Relaxed);
    go.send(()).unwrap();
    ex.fill_until("the export is done", |ex| !ex.state.exports.running());
    ex.fill_until("a new instance plays", |ex| synced(&ex.state.graph) && ex.handles() != first);
    ex.fill(8);
    assert!(ex.state.graph.status().playing && ex.engine().crashes() == 0, "playback went on, with no crash");
    let told = ex.state.graph.expire();
    assert!(told.iter().any(|t| t == "m2 probe (p) stopped working, so music2 started it again from its saved settings."), "{told:?}");
    // From the start its two notes sound in the new instance.
    ex.state.graph.seek(0.0).unwrap();
    let mut peak = 0.0f32;
    for _ in 0..16 {
        let mut buf = [0.0f32; 256];
        ex.driver.fill(&mut buf, 2);
        drop(ex.driver.drain_retired());
        peak = buf.iter().fold(peak, |m, x| m.max(x.abs()));
    }
    let fresh = m2_plugins::probe::stats(ex.handles()[0]).unwrap();
    assert!(fresh.sounding.load(Ordering::Relaxed) == 2 && peak > 0.01, "it plays: peak {peak}");
}

// RENDER-1 (plug-3 of the step 7 review): an export while what plays lags the accepted revision (the
// renderer for a new state didn't build) doesn't borrow the instance whose committed state changed:
// its node plays a new instance from the new state, and the live one holds what its plan says. So
// an undo back to that state loads nothing and plays what it says. Before, the export loaded the new
// state into the live instance, which then played it after the undo.
#[test]
fn an_export_never_changes_what_a_live_instance_holds() {
    let mut ex = Ex::tagged("held");
    let first = ex.handles();
    let stats = m2_plugins::probe::stats(first[0]).unwrap();
    let tag = || stats.tag.lock().unwrap().clone();
    ex.state.graph.inspect(|a| a.fail_builds = true);
    ex.fixture.probe_state("p", "lead", &m2_plugins::probe::state("newer", 1.0));
    ex.state.graph.poll();
    let (plays, accepted) = playing(&ex.state.graph);
    assert!(plays < accepted && tag() == "held", "what plays lags: {plays} plays, {accepted} accepted");
    let before = stats.main_calls().len();
    let (out, done) = export_now(&mut ex);
    assert_eq!(done["done"], true, "{done}");
    assert!(moves(&stats.main_calls(), before).is_empty() && tag() == "held", "the live instance wasn't borrowed");
    assert!(wav(&out) == rendered(&ex), "the export is the accepted revision's, bit for bit");
    // The state write undone, with builds working again: it plays what it says, as it held it.
    ex.state.graph.inspect(|a| a.fail_builds = false);
    ex.fixture.probe_state("p", "lead", &m2_plugins::probe::state("held", 1.0));
    ex.state.graph.poll();
    ex.fill(4);
    assert!(synced(&ex.state.graph) && ex.handles() == first && tag() == "held", "tag {}", tag());
}

// RENDER-1 (K3 of the step 7 review): the session's job fails after it made an export that borrows
// the live instances (here a test's panic). The export's guard, dropped with it, still tells the
// session it is over, so the edits after it play. Before, the session was told only once the job
// returned, and no edit played again until the project was opened again.
#[test]
fn an_export_whose_job_fails_still_frees_the_session() {
    let mut ex = Ex::tagged("panicked");
    ex.hooks().panic_after.store(true, Ordering::SeqCst);
    let e = crate::export::start(&ex.state, Some(ex.fixture.0.join("out.wav"))).unwrap_err();
    assert!(e.to_string().contains("hit a bug"), "{e:#}");
    assert!(!ex.state.exports.running());
    eventually("the session is told", || !ex.state.graph.inspect(|a| a.open.as_ref().unwrap().exporting));
    ex.fixture.put("song.ts", &Ex::song("vca(p.out, 0.5)"));
    assert!(ex.state.graph.poll());
    ex.fill(4);
    assert!(synced(&ex.state.graph), "an edit plays");
}

// HOST-5, RENDER-1 (K7 of the step 7 review): an instance that asked for a restart isn't borrowed by
// an export that starts before the session replaced it: the export makes its own, from the
// committed state, beside playback, and the file is what new instances make. The catch-up after
// replaces the live one.
#[test]
fn an_instance_waiting_for_a_restart_isnt_borrowed() {
    let mut ex = Ex::tagged("restarting");
    let first = ex.handles();
    let stats = m2_plugins::probe::stats(first[0]).unwrap();
    let h = first[0];
    ex.state.graph.inspect(move |a| a.open.as_mut().unwrap().restarts.insert(h));
    let before = stats.main_calls().len();
    let (out, done) = export_now(&mut ex);
    assert!(done["done"] == true && done["paused"] == false, "{done}");
    assert!(moves(&stats.main_calls(), before).is_empty(), "not borrowed");
    assert!(wav(&out) == rendered(&ex), "what new instances make, bit for bit");
    eventually("the catch-up replaces it", || ex.state.graph.catch_up());
    ex.fill(4);
    assert!(ex.handles() != first, "replaced");
}

// RENDER-1 (the owner's O4): an export that took a plugin's instance, and so ended the notes
// playback left in it, but failed before it rendered a block (here it can't write its file) still
// starts those notes again at the resume, as one that rendered does. Before, only the render's first
// call marked the instance, so the held notes stayed silent until their next note-on.
#[cfg(unix)]
#[test]
fn a_note_held_across_an_export_that_fails_at_once_plays_again() {
    use std::os::unix::fs::PermissionsExt;
    let mut ex = Ex::tagged("unwritten");
    ex.fill(4);
    let stats = m2_plugins::probe::stats(ex.handles()[0]).unwrap();
    assert_eq!(stats.sounding.load(Ordering::Relaxed), 2, "its two long notes sound");
    let shut = ex.fixture.0.join("shut");
    std::fs::create_dir(&shut).unwrap();
    std::fs::set_permissions(&shut, std::fs::Permissions::from_mode(0o555)).unwrap();
    let mut events = ex.state.events.subscribe();
    crate::export::start(&ex.state, Some(shut.join("out.wav"))).unwrap();
    ex.fill_until("the export is done", |ex| !ex.state.exports.running());
    std::fs::set_permissions(&shut, std::fs::Permissions::from_mode(0o755)).unwrap();
    let end = crate::export::tests::until_finished(&mut events).pop().unwrap();
    assert!(end["error"].as_str().is_some_and(|e| e.contains("couldn't write")) && end["paused"] == true, "{end}");
    ex.fill(4);
    assert_eq!(stats.sounding.load(Ordering::Relaxed), 2, "the held notes play again");
    assert!(ex.state.graph.status().playing && ex.engine().crashes() == 0, "playback goes on");
}

/// The loudest sample of the next `n` blocks the device takes.
fn loudest(ex: &mut Ex, n: usize) -> f32 {
    let mut most = 0f32;
    for _ in 0..n {
        let mut buf = [0.0f32; 256];
        ex.driver.fill(&mut buf, 2);
        drop(ex.driver.drain_retired());
        most = buf.iter().fold(most, |m, x| m.max(x.abs()));
    }
    most
}

// RENDER-10, RENDER-1 (the plan's risk 16, design-8b §4.3): Stop All Sound (⌘.) while an export
// holds playback waits for the resume, and applies after the plugins the export lent are told to
// start again the notes the song holds (O4): it wins, so nothing sounds after the export, and
// playback goes on. The negative control: an export with no Stop All Sound gives the notes back.
#[test]
fn stop_all_sound_during_an_export_stays_silent_after_it() {
    let mut ex = Ex::tagged("panicked");
    ex.fill(4);
    let stats = m2_plugins::probe::stats(ex.handles()[0]).unwrap();
    assert_eq!(stats.sounding.load(Ordering::Relaxed), 2, "its two long notes sound");
    assert!(loudest(&mut ex, 8) > 0.1, "and are heard");
    crate::export::start(&ex.state, Some(ex.fixture.0.join("one.wav"))).unwrap();
    ex.fill_until("the first export is done", |ex| !ex.state.exports.running());
    ex.fill(4);
    assert_eq!(stats.sounding.load(Ordering::Relaxed), 2, "with no Stop All Sound, they play again after an export");

    let (go, there) = ex.stop_inside();
    crate::export::start(&ex.state, Some(ex.fixture.0.join("two.wav"))).unwrap();
    ex.fill_until("the export took the instance", |_| there.try_recv().is_ok());
    ex.state.graph.panic().unwrap();
    ex.fill(4);
    go.send(()).unwrap();
    ex.fill_until("the second export is done", |ex| !ex.state.exports.running());
    ex.fill(4);
    assert_eq!(stats.sounding.load(Ordering::Relaxed), 0, "Stop All Sound wins: no note plays again");
    assert!(loudest(&mut ex, 40) < 1e-4, "silent after the export");
    assert!(ex.state.graph.status().playing && ex.engine().crashes() == 0, "playback goes on");
}

// The playhead's tempo (the attack's "the playhead after a tempo edit"): the status says the tempo
// the engine plays, which a tempo edit reaches at its switch. While an export holds playback the
// edit is accepted and the timeline says it, but the engine plays the tempo before until the export
// is done: a playhead moved at the timeline's would run ahead, then jump back at every poll.
// RENDER-9a, design-8b §4.1 (the attack's break 23): the loop is judged in the tempo the edit made,
// so the answer says "loop turned off" during the hold too, though the engine turns it off only at
// the switch. Before, it was judged in the tempo the engine played then, and nothing was said.
#[test]
fn during_an_export_a_tempo_edit_turns_the_loop_off_and_the_playhead_keeps_the_tempo_heard() {
    let mut ex = Ex::new();
    let token = ex.state.graph.state().unwrap()["session"].as_u64().unwrap();
    assert_eq!(ex.state.graph.status().bpm, Some(120.0));
    // 0.012 beats: 6 ms at 120 BPM, just over the shortest loop at 48 kHz.
    ex.state.graph.set_loop(Some((32.0, 32.012)), None).unwrap();
    let (go, there) = ex.stop_inside();
    crate::export::start(&ex.state, Some(ex.fixture.0.join("out.wav"))).unwrap();
    ex.fill_until("the export took the instance", |_| there.try_recv().is_ok());
    let graph = &ex.state.graph;
    let v = graph.tempo(token, 0.0, Some(240.0), false, false, false).unwrap();
    assert_eq!((&v["loop_off"], &v["after"]), (&json!(true), &json!("240 BPM")), "{v}");
    assert_eq!(graph.status().looping, Some(Loop { start: 32.0, end: 32.012, on: false }));
    assert_eq!(graph.state().unwrap()["timeline"]["tempo"], json!([{"beat": 0.0, "bpm": 240.0}]), "accepted");
    assert_eq!(graph.status().bpm, Some(120.0), "the engine plays the tempo before until the export is done");
    go.send(()).unwrap();
    ex.fill_until("the export is done", |ex| !ex.state.exports.running());
    ex.fill_until("the tempo edit plays", |ex| ex.state.graph.status().bpm == Some(240.0));
    ex.fill(4);
    assert_eq!(ex.state.graph.status().looping, Some(Loop { start: 32.0, end: 32.012, on: false }), "off, as it was said");
}

// HOST-5: a plugin that fails at every call is started again from its saved settings once. When
// the new instance fails too, soon after, it is left silent and the user is told once; the song
// plays on, and no new instance is made for it every 300 ms. One whose new instance failed long
// after it was started again is started again. Before, each catch-up made a new one, and said so.
#[test]
fn a_plugin_that_keeps_failing_is_started_again_once() {
    let mut ex = Ex::tagged("fails-process");
    let rounds = |ex: &mut Ex, n: usize| {
        for _ in 0..n {
            ex.fill(4);
            // On the actor, so a busy actor never skips a round, as the watcher's catch-up does.
            ex.state.graph.inspect(|a| a.catch_up());
        }
    };
    rounds(&mut ex, 1);
    // As if the instance it was started again with had played for a minute.
    ex.state.graph.inspect(|a| {
        let back = Instant::now() - Duration::from_secs(60);
        a.open.as_mut().unwrap().revived.values_mut().for_each(|t| *t = Some(back));
    });
    rounds(&mut ex, 20);
    let made = m2_plugins::probe::tagged("fails-process");
    let told: Vec<String> = ex.state.graph.expire().into_iter().filter(|t| t.starts_with("m2 probe (p)")).collect();
    // What was made, and what was said, if not.
    assert!(
        made.len() == 3 && made.iter().all(|s| s.process.load(Ordering::Relaxed) > 0),
        "two new instances, each failed: made {}, process {:?}, told {told:?}",
        made.len(),
        made.iter().map(|s| s.process.load(Ordering::Relaxed)).collect::<Vec<_>>()
    );
    let again = "m2 probe (p) stopped working, so music2 started it again from its saved settings.";
    let silent = "m2 probe (p) stopped working again, so music2 left it silent. Open the song again to give it another try.";
    assert_eq!(told, [again, again, silent]);
    assert!(ex.state.graph.status().playing && ex.engine().crashes() == 0, "the song plays on");
}

// RENDER-26b, HOME-6 (fit-2 and K6 of the step 7 review): a plugin that refuses a new state is named
// as the user knows it, by its plugin and its track, in plain words, once: a later edit neither tries
// the refused state again nor says so again. Its old state written back (an undo) loads, as the
// plugin may hold part of the refused one.
#[test]
fn a_refused_state_is_said_once_by_the_plugins_name() {
    m2_plugins::probe::install();
    let fixture = Fixture::new();
    fixture.put(
        "song.ts",
        "import { plugin, song, track } from '@m2/core';\nimport { sum } from 'std/math';\nimport { a } from './tracks/a';\n\
         const p = track('p', ({ lane }) => plugin('clap:m2.probe.fails-state', { notes: lane.notes('n'), key: 'lead' }));\n\
         export default song(() => sum(a.out, p.out));\n",
    );
    fixture.put("tracks/a.ts", A_DC);
    fixture.put(
        "data/p/n.json",
        r#"{"clips": [{"id": "a", "start": 0, "length": 64, "notes": [{"pitch": "C4", "start": 0, "length": 60}]}]}"#,
    );
    fixture.put("data/p/lead.state.json", r#"{"plugin": "clap:m2.probe.fails-state", "plugin_version": "1.0.0"}"#);
    std::fs::write(fixture.0.join("data/p/lead.state"), m2_plugins::probe::state("fine", 1.0)).unwrap();
    let (p, driver) = Player::driven(48_000);
    let graph = Graph::with_player(p);
    graph.open(&fixture.0).unwrap();
    let mut dc = Dc { fixture, graph, driver };
    dc.graph.transport("play").unwrap();
    dc.frames(4);
    let h = dc.handles()[0];
    let stats = m2_plugins::probe::stats(h).unwrap();
    let loads = || stats.main_calls().iter().filter(|c| c.what == "load").count();
    let refused = |graph: &Graph| graph.expire().into_iter().filter(|t| t.contains("couldn't load")).collect::<Vec<_>>();
    std::fs::write(dc.fixture.0.join("data/p/lead.state"), b"fail: refused").unwrap();
    assert!(dc.graph.poll());
    let told = refused(&dc.graph);
    assert!(
        told.len() == 1
            && told[0].starts_with("m2 probe (fails state) (p) couldn't load its new settings, so it keeps the sound it had. (In detail: ")
            && told[0].contains("it refused the saved state"),
        "{told:?}"
    );
    let tried = loads();
    // A structural edit elsewhere: the same instance plays it, and nothing is tried or said again.
    dc.fixture.put("tracks/a.ts", &A_DC.replace("  return", "  const spare = osc.sine(hz(1));\n  return"));
    assert!(dc.graph.poll());
    dc.frames(4);
    assert!(synced(&dc.graph) && dc.handles() == [h] && loads() == tried && refused(&dc.graph).is_empty());
    // The old state written back loads.
    std::fs::write(dc.fixture.0.join("data/p/lead.state"), m2_plugins::probe::state("fine", 1.0)).unwrap();
    assert!(dc.graph.poll());
    assert_eq!((loads(), refused(&dc.graph).len()), (tried + 1, 0));
}

// RENDER-28 (K2 of the step 7 review): a change whose renderer doesn't build is saved, and the
// version before it plays on; the user is told so in plain words, once, though the catch-up tries
// again every 300 ms. Once it builds, it plays; a later change that fails is told again.
#[test]
fn a_change_that_cant_get_ready_to_play_says_so_once() {
    let dc = Dc::new();
    let failing = |dc: &Dc| dc.graph.expire().into_iter().filter(|t| t.contains("keeps playing")).collect::<Vec<_>>();
    dc.graph.inspect(|a| a.fail_builds = true);
    dc.fixture.put("tracks/a.ts", &A_DC.replace("  return", "  const spare = osc.sine(hz(1));\n  return"));
    assert!(dc.graph.poll());
    // The catch-up's tries, made on the actor so none is skipped as busy.
    for _ in 0..3 {
        assert!(!dc.graph.inspect(|a| a.catch_up()));
    }
    let told = failing(&dc);
    let start = "Your last change is saved, but music2 couldn't get it ready to play, so the version before it keeps playing. Undo";
    assert!(
        told.len() == 1 && told[0].starts_with(start) && told[0].ends_with("(In detail: the renderer failed to build (a test))"),
        "{told:?}"
    );
    dc.graph.inspect(|a| a.fail_builds = false);
    eventually("it plays once it builds", || dc.graph.catch_up() && synced(&dc.graph));
    dc.graph.inspect(|a| a.fail_builds = true);
    dc.fixture.put("tracks/a.ts", &A_DC.replace("  return", "  const spare = osc.sine(hz(2));\n  return"));
    assert!(dc.graph.poll());
    assert_eq!(failing(&dc).len(), 1, "a later change that fails is told again");
}

// K2 of the step 7 review: running out of memory, as wasmtime says it ("mmap failed to reserve …"),
// reads in plain words, with its text as the detail, in an open, an export and a change that can't
// play; any other error is left in its own words.
#[test]
fn running_out_of_memory_is_said_plainly() {
    let e = || {
        anyhow!("mmap failed to reserve 0x104000000 bytes: Cannot allocate memory (os error 12)")
            .context("pad/n27: instantiating `std:adsr`")
    };
    let said = format!("{:#}", super::plainly(e()));
    let detail = "(In detail: pad/n27: instantiating `std:adsr`: mmap failed to reserve 0x104000000 bytes";
    assert!(
        said.starts_with("the computer ran out of memory for it. Close other apps or songs, then try again. ") && said.contains(detail),
        "{said}"
    );
    let change = super::actor::unbuilt(&e());
    assert!(change.starts_with("Your last change is saved, but the computer ran out of memory") && change.contains(detail), "{change}");
    assert_eq!(format!("{:#}", super::plainly(anyhow!("a plugin failed"))), "a plugin failed");
}

// A6 of the live plan's attack (D24): a sound over its own memory says so in its own words. The
// computer didn't run out, so the user is never told to close other apps for it.
#[test]
fn a_sound_over_its_memory_is_not_the_computer_running_out() {
    let e = || anyhow::Error::from(m2_runtime::OutOfMemory { node: "bass/delay".into() }).context("bass/delay: voice");
    let words = "bass/delay: voice: bass/delay ran out of memory: a sound may use up to 256 MB";
    assert_eq!(format!("{:#}", super::plainly(e())), words);
    let change = super::actor::unbuilt(&e());
    assert!(change.contains("couldn't get it ready to play") && change.contains(words) && !change.contains("computer"), "{change}");
}

// RENDER-28 (fit-1 of the step 7 review): the output device changes, or stops answering, while
// playing, and no action meets it first: the catch-up lets the player go, and the user is told in
// plain words that playback stopped, why, and to press Play. Stopped, nothing is said.
#[test]
fn playback_the_device_stopped_is_said() {
    for (kind, why, plays) in [
        (DeviceErrorKind::DeviceChanged, "the sound output moved to another device", true),
        (DeviceErrorKind::DeviceNotAvailable, "the audio device stopped responding", true),
        (DeviceErrorKind::DeviceChanged, "", false),
    ] {
        let mut dc = Dc::new();
        if plays {
            dc.graph.transport("play").unwrap();
        }
        dc.blocks(2);
        dc.driver.error(kind.into());
        // The catch-up waits while the actor is busy.
        eventually("a new player", || dc.graph.catch_up());
        let told: Vec<String> = dc.graph.expire().into_iter().filter(|t| t.starts_with("Playback stopped")).collect();
        match plays {
            true => assert_eq!(told, [format!("Playback stopped: {why}. Press Play to go on.")], "{kind:?}"),
            false => assert!(told.is_empty(), "{told:?}"),
        }
        assert_eq!(dc.graph.inspect(|a| a.open.as_ref().unwrap().plays.as_ref().unwrap().epoch), 1, "{kind:?}");
    }
}

/// first-song, mixed, as a project to add and delete tracks in.
fn mixed(fixture: &Fixture) {
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

/// The timeline's tracks, top to bottom, with their colours.
fn rows(graph: &Graph) -> Vec<(String, String)> {
    let state = graph.state().unwrap();
    state["timeline"]["tracks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["track"] == json!(true))
        .map(|t| (t["module"].as_str().unwrap().to_string(), t["colour"].as_str().unwrap().to_string()))
        .collect()
}

// WRITE-10, ID-18: a track added, duplicated and deleted through the server plays each revision;
// deleting the kick waits (409) with the duck that reads it, until the cascade.
#[test]
fn tracks_are_added_duplicated_and_deleted_and_the_player_follows() {
    let fixture = Fixture::new();
    mixed(&fixture);
    let graph = fixture.open();
    let token = graph.state().unwrap()["session"].as_u64().unwrap();
    let names: Vec<String> = graph.state().unwrap()["instruments"].as_array().unwrap().iter().map(|i| i["label"].as_str().unwrap().into()).collect();
    // std's ready-made sounds (T3): drums, instruments, the sampler, then the two with every knob.
    assert_eq!(names.len(), 23, "{names:?}");
    assert_eq!((names[0].as_str(), names[10].as_str(), names[11].as_str(), names[20].as_str(), names[22].as_str()), ("Drum kit", "FM metal", "FM keys", "Sampler", "Polysynth"));
    assert_eq!(graph.state().unwrap()["palette"][0], "rose");
    let added = graph.add_track(token, "std:synth-pluck", None, None, Some("kick".into())).unwrap();
    assert_eq!(
        (&added["module"], &added["label"], &added["file"]),
        (&json!("synth-pluck"), &json!("Synth pluck"), &json!("tracks/synth-pluck.ts"))
    );
    assert_eq!(rows(&graph)[1], ("synth-pluck".into(), "green".into()), "below the kick, in the first colour no track uses");
    assert!(synced(&graph));
    let copy = graph.duplicate_track(token, "synth-pluck").unwrap();
    assert_eq!((&copy["module"], &copy["new_channel"]), (&json!("synth-pluck-2"), &json!(false)));
    assert_eq!(rows(&graph)[2], ("synth-pluck-2".into(), "green".into()), "in its original's colour");
    let (status, asks) = graph.delete_track(token, "kick", false).unwrap();
    assert_eq!((status, &asks["blocked"], &asks["dependents"][0]["component"]), (409, &json!(false), &json!("Duck")), "{asks}");
    assert!(fixture.0.join("tracks/kick.ts").exists());
    let (status, done) = graph.delete_track(token, "kick", true).unwrap();
    assert_eq!((status, &done["before"]), (200, &json!("Kick")), "{done}");
    assert!(synced(&graph));
    assert_eq!(rows(&graph).iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), ["synth-pluck", "synth-pluck-2", "hats", "bass", "keys"]);
    graph.undo(token).unwrap();
    assert!(fixture.0.join("tracks/kick.ts").exists() && synced(&graph));
}

/// The largest step from one sample to the next.
fn max_step(x: &[f32]) -> f32 {
    x.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max)
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

// 8b.1 on step 7's engine (break 10 of the taste merge): adding a track while a pad's tail rings,
// then deleting another, are activations that keep it ringing. Before step 7 each made a new
// renderer and cut every tail. The delete takes out the first channel, so the pad's gain after it
// is renumbered (ID-14) and made new: allowed, as a crossfade between equal signals (design-7 Q9).
// Each shows no step in the output bigger than twice the tail's own, and the tail after it is as
// loud as the same song's with no edit, within 1 dB.
#[test]
fn adding_and_deleting_a_track_keeps_a_ringing_tail() {
    /// The song, open in a driven player and playing, 1.2 s in: the chord has ended and rings.
    fn ringing() -> (Dc, u64) {
        let fixture = Fixture::new();
        fixture.put(
            "song.ts",
            "import { song } from '@m2/core';\nimport { master, mixer } from 'std/mix';\nimport keys from './tracks/keys';\n\
             import pad from './tracks/pad';\n\
             export default song(() => master(mixer({ keys: { in: keys.out, db: -6 }, pad: { in: pad.out, db: -3 } })));\n",
        );
        fixture.put(
            "tracks/keys.ts",
            "import { track } from '@m2/core';\nimport { fm } from 'std/fm';\n\
             export default track('keys', { name: 'Keys' }, ({ lane }) => fm.keys(lane.notes('notes')));\n",
        );
        fixture.put(
            "tracks/pad.ts",
            "import { track } from '@m2/core';\nimport { synth } from 'std/synth';\n\
             export default track('pad', { name: 'Pad' }, ({ lane }) => synth.pad(lane.notes('notes')));\n",
        );
        fixture.put("data/keys/notes.json", "{\"clips\": []}\n");
        // A chord for the first 2 beats (1 s at 120 BPM); its release rings on for 1.6 s.
        let notes: Vec<String> =
            ["C4", "E4", "G4"].iter().map(|p| format!("{{\"pitch\": \"{p}\", \"start\": 0, \"length\": 2, \"velocity\": 0.8}}")).collect();
        fixture.put("data/pad/notes.json", &format!("{{\"clips\": [{{\"id\": \"chord\", \"start\": 0, \"length\": 8, \"notes\": [{}]}}]}}\n", notes.join(", ")));
        let (p, driver) = Player::driven(48_000);
        let graph = Graph::with_player(p);
        graph.open(&fixture.0).unwrap();
        let token = graph.state().unwrap()["session"].as_u64().unwrap();
        let mut dc = Dc { fixture, graph, driver };
        dc.graph.transport("play").unwrap();
        dc.frames(450);
        (dc, token)
    }
    let (mut dc, token) = ringing();
    let mut heard: Vec<f32> = vec![];
    let mut checked: Vec<(&str, std::ops::Range<usize>)> = vec![];
    let add = |g: &Graph| drop(g.add_track(token, "std:synth-pluck", None, None, None).unwrap());
    let delete = |g: &Graph| assert_eq!(g.delete_track(token, "keys", false).unwrap().0, 200);
    let edits: [&dyn Fn(&Graph); 2] = [&add, &delete];
    for (what, edit) in ["adding a track", "deleting the first channel's track"].into_iter().zip(edits) {
        let before = dc.frames(19);
        let own = max_step(&before);
        heard.extend(&before);
        let (from, generation) = (heard.len() - 1, dc.generation());
        edit(&dc.graph);
        // Until the activation lands, then the 100 ms after it, its crossfade included.
        for _ in 0..400 {
            heard.extend(dc.frames(1));
            if dc.generation() != generation {
                break;
            }
        }
        assert_ne!(dc.generation(), generation, "{what}: an activation");
        heard.extend(dc.frames(38));
        let step = max_step(&heard[from..]);
        assert!(step <= 2.0 * own, "{what}: a step of {step} against the tail's own {own}");
        checked.push((what, heard.len() - 19 * 128..heard.len()));
    }
    // The same song with no edit, for as long.
    let (mut plain, _) = ringing();
    let alone = plain.frames(heard.len() / 128);
    for (what, at) in checked {
        let (edited, unedited) = (rms(&heard[at.clone()]), rms(&alone[at]));
        assert!(unedited > 1e-3, "{what}: the tail still rings ({unedited})");
        assert!((20.0 * (edited / unedited).log10()).abs() < 1.0, "{what}: the tail rings on: {edited} against {unedited} with no edit");
    }
}

// design-8b §5.1, lead decision T5: in a mix that is mono all the way, the first pan on a channel
// turns the sum stereo. std's master is stereo from its input on, so its gain and its limiter keep
// their instances, and the limiter its envelope; before 8b.3 the limiter started again. Here the
// mix is loud enough that the limiter works all the time, and the edit is a hand-written `pan: 0`,
// as 8b.3's first pan writes it.
#[test]
fn a_first_pan_in_a_mono_mix_keeps_the_masters_gain_and_limiter() {
    let fixture = Fixture::new();
    fixture.put(
        "song.ts",
        "import { song } from '@m2/core';\nimport { master, mixer } from 'std/mix';\nimport { a } from './tracks/a';\n\
         import { b } from './tracks/b';\nexport default song(() => master(mixer({ a: a.out, b: b.out })));\n",
    );
    // A constant 2 (a square at 0 Hz is 1): over the limiter's −1 dB ceiling all the time.
    fixture.put("tracks/a.ts", &A_DC.replace("0.5)", "2)"));
    fixture.put("tracks/b.ts", B_SILENT);
    let (p, driver) = Player::driven(48_000);
    let graph = Graph::with_player(p);
    graph.open(&fixture.0).unwrap();
    let mut dc = Dc { fixture, graph, driver };
    dc.graph.transport("play").unwrap();
    let ceiling = 10f32.powf(-1.0 / 20.0);
    let before = dc.frames(20);
    assert!(before[128..].iter().all(|x| (x - ceiling).abs() < 1e-6), "the limiter holds the mix at its ceiling: {:?}", &before[128..132]);
    let generation = dc.generation();
    dc.fixture.put("song.ts", &dc.fixture.read("song.ts").replace("a: a.out", "a: { in: a.out, pan: 0 }"));
    assert!(dc.graph.poll());
    let mut after = vec![];
    for _ in 0..400 {
        after.extend(dc.frames(1));
        if dc.generation() != generation {
            break;
        }
    }
    assert_ne!(dc.generation(), generation, "an activation");
    after.extend(dc.frames(20));
    // What the playing plan made new, by kernel.
    let fresh = dc.stats().fresh_nodes;
    let made: Vec<String> = dc.graph.inspect(move |a| {
        let flat = &a.open.as_ref().unwrap().plays.as_ref().unwrap().revision.compiled.flat;
        fresh
            .iter()
            .map(|&i| match &flat.nodes[i].op {
                m2_ir::FOp::Kernel { kernel, .. } => kernel.clone(),
                _ => flat.nodes[i].name.clone(),
            })
            .collect()
    });
    assert!(made.iter().any(|k| k == "std:pan"), "the new panner is new: {made:?}");
    for k in ["std:stereo", "std:limit"] {
        assert!(!made.iter().any(|m| m == k), "the master's {k} is kept: {made:?}");
    }
    // The master's gain is the one gain that isn't on a channel's strip, which turns stereo.
    assert_eq!(made.iter().filter(|m| *m == "std:gain").count(), 1, "only channel a's gain is new: {made:?}");
    assert!(after.iter().all(|x| (x - ceiling).abs() < 1e-5), "the mix stays at the ceiling throughout: {:?}", max_step(&after));
}

// FMT-19, PLANE-3: an edit of layout.json from outside (an agent, git) is published with no
// revision once it holds still; a layout write planned before it conflicts rather than writing
// over it, since its base was read from the disk.
#[test]
fn an_outside_layout_edit_is_published_with_no_revision() {
    let fixture = Fixture::new();
    mixed(&fixture);
    let graph = fixture.open();
    let token = graph.state().unwrap()["session"].as_u64().unwrap();
    let revision = graph.state().unwrap()["view"]["revision"].clone();
    graph.watch();
    fixture.put("layout.json", "{\"order\": [\"keys\", \"bass\"], \"tracks\": {\"keys\": {\"colour\": \"lime\"}}}");
    eventually("the watcher publishes the new order", || {
        graph.watch();
        rows(&graph).first() == Some(&("keys".into(), "lime".into()))
    });
    assert_eq!(graph.state().unwrap()["view"]["revision"], revision, "no revision");
    assert_eq!(rows(&graph).iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), ["keys", "bass", "kick", "hats"]);
    // A file that doesn't read: the defaults show, and the problem offers Reset layout.
    fixture.put("layout.json", "{ oops");
    eventually("the unreadable layout is a problem", || {
        graph.watch();
        graph.state().unwrap()["problems"].as_array().unwrap().iter().any(|p| p["code"] == "layout.invalid")
    });
    assert!(graph.layout(token, None, Some(("kick".into(), "teal".into())), false).is_err());
    let reset = graph.layout(token, None, None, true).unwrap();
    assert_eq!(reset["file"], "layout.json");
    assert!(!graph.state().unwrap()["problems"].as_array().unwrap().iter().any(|p| p["code"] == "layout.invalid"));
}

// WRITE-12: a layout write's base is what the disk said when it was planned; an outside write
// after that is a conflict, never overwritten.
#[test]
fn a_layout_write_after_an_outside_edit_conflicts() {
    let fixture = Fixture::new();
    mixed(&fixture);
    let graph = fixture.open();
    graph.inspect(|a| {
        let s = &mut a.open.as_mut().unwrap().session;
        let read = m2_editor::layout::read_disk(&s.accepted().project.dir);
        let view = m2_editor::layout::effective(&m2_editor::layout::default_order(s), &read);
        let text = m2_editor::layout::text_after(&read, &m2_editor::layout::Change { order: view.order.clone(), colours: &view.colours, gone: None })
            .unwrap();
        let (op, file, base) = m2_editor::layout::write_op(s, text);
        let mut tx = m2_editor::plan::planned(s, ("a".into(), "b".into()), vec![op], m2_song::tx::Expect::Identical);
        tx.base.insert(file, base);
        std::fs::write(s.accepted().project.dir.join("layout.json"), "{\"order\": [\"bass\"]}").unwrap();
        assert!(matches!(s.commit(tx), Err(m2_session::Refusal::Conflict { .. })), "a conflict");
        assert_eq!(std::fs::read_to_string(s.accepted().project.dir.join("layout.json")).unwrap(), "{\"order\": [\"bass\"]}");
    });
}

// design-8b §5.4: the state carries the Mix view beside the view and the timeline: a strip for each
// track by the view's knob ids, and the master. A channel's first pan writes `pan: 0` into its
// entry and names the new knob, which the next state's strip has; undo takes it out. A window open
// read-only writes nothing.
#[test]
fn the_state_carries_the_mix_and_a_first_pan_names_its_knob() {
    let fixture = Fixture::new();
    mixed(&fixture);
    let graph = fixture.open();
    let state = graph.state().unwrap();
    let token = state["session"].as_u64().unwrap();
    let mix = &state["mix"];
    assert_eq!(mix["mixer"], "ok");
    let strips: Vec<(&str, &str)> =
        mix["strips"].as_array().unwrap().iter().map(|x| (x["channel"].as_str().unwrap(), x["label"].as_str().unwrap())).collect();
    assert_eq!(strips, [("kick", "Kick"), ("hats", "Hats"), ("bass", "Bass"), ("keys", "Keys")]);
    let kick = &mix["strips"][0];
    assert_eq!((&kick["pan"], &kick["pan_insert"]), (&Value::Null, &json!(true)));
    let knobs: Vec<Value> = state["view"]["modules"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|m| m["nodes"].as_array().unwrap())
        .flat_map(|n| n["knobs"].as_array().unwrap().iter().map(|k| k["id"].clone()))
        .collect();
    for id in [&kick["db"], &mix["master"]["db"], &mix["master"]["ceiling"]] {
        assert!(knobs.contains(id), "{id} is a knob");
    }
    let song = fixture.read("song.ts");
    let done = graph.mix_insert(token, "kick", "pan", 0.0).unwrap();
    assert_eq!((&done["before"], &done["after"]), (&json!("kick: kick.out"), &json!("kick: { in: kick.out, pan: 0 }")), "{done}");
    assert_eq!(fixture.read("song.ts"), song.replacen("kick: kick.out", "kick: { in: kick.out, pan: 0 }", 1));
    let now = graph.state().unwrap();
    assert_eq!((&now["mix"]["strips"][0]["pan"], &now["mix"]["strips"][0]["pan_insert"]), (&done["control"], &json!(false)));
    assert_eq!(now["view"]["revision"], done["revision"]);
    assert!(synced(&graph));
    graph.undo(token).unwrap();
    assert_eq!(fixture.read("song.ts"), song);
    graph.close();

    let held = m2_session::lock::acquire(&fixture.0, "m2", m2_session::lock::RETRY).unwrap();
    let graph = fixture.open();
    let token = graph.state().unwrap()["session"].as_u64().unwrap();
    let e = graph.mix_insert(token, "kick", "pan", 0.0).unwrap_err().to_string();
    assert!(e.contains("nothing was changed"), "{e}");
    assert_eq!(fixture.read("song.ts"), song);
    drop(held);
}

/// `n` blocks of 128 frames, both channels, interleaved.
fn stereo(dc: &mut Dc, n: usize) -> Vec<f32> {
    let mut out = vec![];
    for _ in 0..n {
        let mut buf = [0.0f32; 256];
        dc.driver.fill(&mut buf, 2);
        drop(dc.driver.drain_retired());
        out.extend(buf);
    }
    out
}

/// Play `dc` on through `edit` until the activation it sends lands, then `after` blocks: what was
/// heard from the edit on.
fn through(dc: &mut Dc, edit: impl FnOnce(&Graph), after: usize) -> Vec<f32> {
    let generation = dc.generation();
    edit(&dc.graph);
    let mut heard = vec![];
    for _ in 0..400 {
        heard.extend(stereo(dc, 1));
        if dc.generation() != generation {
            break;
        }
    }
    assert_ne!(dc.generation(), generation, "an activation");
    heard.extend(stereo(dc, after));
    heard
}

/// The largest difference between two takes, sample for sample.
fn apart(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len());
    a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max)
}

// design-8b §5.3, plan B4, lead decision T4: the first pan on first-song's Kick while it plays. Kick
// is the first channel, so the panners of Hats and Keys are renamed and made new (ID-14); the kick's
// strip turns stereo. It is heard as nothing: from the edit on, through the activation and its
// crossfades, the song plays as it does with no edit, within 1e-5 (−100 dBFS), so no step and
// every other track goes on unbroken.
#[test]
fn a_first_pan_while_playing_is_inaudible() {
    fn playing() -> (Dc, u64) {
        let mut dc = example("examples/first-song");
        let token = dc.graph.state().unwrap()["session"].as_u64().unwrap();
        // Into the song: a kick hit at beat 1.
        drop(stereo(&mut dc, 160));
        (dc, token)
    }
    let (mut dc, token) = playing();
    let heard = through(&mut dc, |g| drop(g.mix_insert(token, "kick", "pan", 0.0).unwrap()), 400);
    assert!(dc.fixture.read("song.ts").contains("kick: { in: kick.out, pan: 0 }"));
    let (mut plain, _) = playing();
    let alone = stereo(&mut plain, heard.len() / 256);
    assert!(rms(&alone) > 1e-2, "the song plays ({})", rms(&alone));
    let differs = apart(&heard, &alone);
    assert!(differs <= 1e-5, "the first pan is heard: {differs} from the song with no edit");
}

// design-8b §5.3, lead decision T5 (the attack's claim 6): in a mix that is mono all the way, the
// first pan turns the mix stereo: the sum's adds are made new, and std's master keeps its gain and
// limiter. Every instrument + Track offers is stereo since T3, so the two mono tracks are written in
// code, std's first bass and kick; their faders go up 12 dB through the Mix view's ids, written into
// their bare entries, so the limiter works. The first pan is heard as nothing: the song plays as it
// does with no edit, within 1e-5.
#[test]
fn a_first_pan_in_a_mono_mix_while_playing_is_inaudible() {
    fn playing() -> (Dc, u64) {
        let fixture = Fixture::new();
        fixture.put(
            "song.ts",
            "import { song } from '@m2/core';\nimport { master, mixer } from 'std/mix';\nimport low from './tracks/low';\n\
             import thump from './tracks/thump';\nexport default song(() => master(mixer({ low: low.out, thump: thump.out })));\n",
        );
        fixture.put(
            "tracks/low.ts",
            "import { track } from '@m2/core';\nimport { bass } from 'std/instruments';\n\
             export default track('low', { name: 'Low' }, ({ lane }) => bass(lane.notes('notes')));\n",
        );
        fixture.put(
            "tracks/thump.ts",
            "import { track } from '@m2/core';\nimport { kick } from 'std/instruments';\n\
             export default track('thump', { name: 'Thump' }, ({ lane }) => kick(lane.notes('hits')));\n",
        );
        fixture.put("data/low/notes.json", "{\"clips\": []}\n");
        fixture.put("data/thump/hits.json", "{\"clips\": []}\n");
        let (p, driver) = Player::driven(48_000);
        let graph = Graph::with_player(p);
        graph.open(&fixture.0).unwrap();
        let mut dc = Dc { fixture, graph, driver };
        let token = dc.graph.state().unwrap()["session"].as_u64().unwrap();
        for (module, lane, pitches) in [("low", "notes", ["C2", "G2", "C2", "G2"]), ("thump", "hits", ["C1", "C1", "C1", "C1"])] {
            let revision = dc.graph.state().unwrap()["view"]["revision"].as_u64().unwrap();
            dc.graph.write_notes(token, revision, module, lane, &kick_clips(&pitches)).unwrap();
            let state = dc.graph.state().unwrap();
            let strip = state["mix"]["strips"].as_array().unwrap().iter().find(|x| x["channel"] == json!(module)).unwrap().clone();
            let revision = state["view"]["revision"].as_u64().unwrap();
            dc.graph.set(token, revision, strip["db"].as_str().unwrap(), "fader", 12.0, None).unwrap();
        }
        let song = dc.fixture.read("song.ts");
        assert!(song.contains("mixer({ low: { in: low.out, db: 12 }, thump: { in: thump.out, db: 12 } })"), "{song}");
        dc.graph.transport("play").unwrap();
        drop(stereo(&mut dc, 200));
        (dc, token)
    }
    // Mono all the way to the master: every add of the mix has one channel.
    let adds = |dc: &Dc| -> Vec<u8> {
        dc.graph.inspect(|a| {
            let flat = &a.open.as_ref().unwrap().plays.as_ref().unwrap().revision.compiled.flat;
            flat.nodes
                .iter()
                .filter_map(|n| match &n.op {
                    m2_ir::FOp::Kernel { kernel, channels, .. } if n.module == "song" && kernel == "std:add" => Some(*channels),
                    _ => None,
                })
                .collect()
        })
    };
    let (mut dc, token) = playing();
    assert!(!adds(&dc).is_empty() && adds(&dc).iter().all(|c| *c == 1), "a mono mix: {:?}", adds(&dc));
    let heard = through(&mut dc, |g| drop(g.mix_insert(token, "low", "pan", 0.0).unwrap()), 400);
    assert!(dc.fixture.read("song.ts").contains("low: { in: low.out, db: 12, pan: 0 }"));
    assert!(adds(&dc).iter().all(|c| *c == 2), "a stereo mix now: {:?}", adds(&dc));
    let (mut plain, _) = playing();
    let alone = stereo(&mut plain, heard.len() / 256);
    let ceiling = 10f32.powf(-1.0 / 20.0);
    let peak = alone.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    assert!((peak - ceiling).abs() < 1e-3, "the limiter works: a peak of {peak}");
    let differs = apart(&heard, &alone);
    assert!(differs <= 1e-5, "the first pan is heard: {differs} from the song with no edit");
}
