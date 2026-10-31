//! Lane leases (WRITE-22a, 8b.7): a drag on notes or automation is heard where it is before it
//! is let go, written once on release, and put back when it ends any other way; outside writes
//! and new plans carry it, and nothing waits for the session.

use std::path::Path;
use std::time::Duration;

use m2_editor::clips::{ClipRef, Gesture};
use m2_play::{Driver, Player};
use m2_runtime::TraceKind;
use m2_song::{LaneAddr, Note};

use super::super::tests::{Fixture, eventually, revs};
use super::*;

/// A bass track playing its `notes` lane.
const SONG: &str = "import { song, track } from '@m2/core';\nimport { bass } from '@m2/synths';\n\
                    const b = track('bass', ({ lane }) => bass(lane.notes('notes')));\nexport default song(() => b.out);\n";

/// Clip `a`: a long C2 from beat 0 to 12, and an E2 at beat 4. Clip `b`, from beat 16: a G2 at 16.
const NOTES: &str = r#"{"clips": [
  {"id": "a", "start": 0, "length": 16, "notes": [{"pitch": "C2", "start": 0, "length": 12}, {"pitch": "E2", "start": 4, "length": 1}]},
  {"id": "b", "start": 16, "length": 8, "notes": [{"pitch": "G2", "start": 0, "length": 1}]}
]}"#;

const C2: f32 = 36.0;
const E2: f32 = 40.0;
const G2: f32 = 43.0;
/// Frames in a beat at 120 BPM and 48 kHz.
const BEAT: u64 = 24_000;

/// The song open in a driven player, playing, its renderer traced. The test's thread is the audio
/// thread: it fills.
struct Drag {
    fixture: Fixture,
    graph: Graph,
    driver: Driver,
    token: u64,
}

impl Drag {
    fn new() -> Drag {
        Drag::with(SONG, NOTES)
    }

    fn with(song: &str, notes: &str) -> Drag {
        let fixture = Fixture::new();
        fixture.put("song.ts", song);
        fixture.put("data/bass/notes.json", notes);
        let (p, driver) = Player::driven(48_000);
        let graph = Graph::with_player(p);
        graph.open(&fixture.0).unwrap();
        let token = graph.state().unwrap()["session"].as_u64().unwrap();
        graph.transport("play").unwrap();
        let mut d = Drag { fixture, graph, driver, token };
        d.fill(2);
        d.trace();
        d
    }

    /// Trace what the renderer that plays now turns on and off.
    fn trace(&mut self) {
        self.driver.engine().unwrap().current_mut().unwrap().trace(4096);
    }

    fn fill(&mut self, n: usize) {
        for _ in 0..n {
            let mut buf = [0.0f32; 256];
            self.driver.fill(&mut buf, 2);
            drop(self.driver.drain_retired());
        }
    }

    /// Fill until the transport passes `beat`.
    fn until(&mut self, beat: f64) {
        let mut n = 0;
        while self.graph.status().beat < beat {
            self.fill(1);
            n += 1;
            assert!(n < 100_000, "the transport never reached beat {beat}");
        }
    }

    /// The Ons the renderer played, by transport frame and pitch, and the Offs by frame.
    fn heard(&mut self) -> (Vec<(u64, f32)>, Vec<u64>) {
        let entries = self.driver.engine().unwrap().current().unwrap().trace_entries();
        let ons =
            entries.iter().filter_map(|e| if let TraceKind::On { pitch } = e.kind { Some((e.transport, pitch)) } else { None }).collect();
        let offs = entries.iter().filter(|e| e.kind == TraceKind::Off).map(|e| e.transport).collect();
        (ons, offs)
    }

    /// The Ons of `pitch`, by beat.
    fn ons(&mut self, pitch: f32) -> Vec<f64> {
        self.heard().0.into_iter().filter(|(_, p)| *p == pitch).map(|(f, _)| f as f64 / BEAT as f64).collect()
    }

    fn sounding(&mut self) -> usize {
        self.driver.engine().unwrap().current().unwrap().sounding_notes()
    }

    fn begin(&self, g: Gesture) -> LaneBegun {
        self.graph.lane_begin(self.token, g, Some("w1".into()), None).unwrap().unwrap()
    }

    fn revision(&self) -> u64 {
        self.graph.state().unwrap()["view"]["revision"].as_u64().unwrap()
    }
}

fn addr() -> LaneAddr {
    LaneAddr::new("bass", "notes").unwrap()
}

fn clip(id: &str) -> ClipRef {
    ClipRef { lane: addr(), id: Some(id.into()), at: None, sha: None }
}

fn note(pitch: f64, start: f64, length: f64) -> Note {
    Note { pitch, start, length, velocity: 0.8 }
}

/// Clip `a`'s notes with the E2 at `e` (beats into the clip).
fn e_at(e: f64) -> Gesture {
    Gesture::Notes { clip: clip("a"), notes: vec![note(36.0, 0.0, 12.0), note(40.0, e, 1.0)], length: None }
}

/// Clip `id` moved by `by` beats.
fn moved(id: &str, by: f64) -> Gesture {
    Gesture::Move { clips: vec![clip(id)], by, to: None, copy: false }
}

// WRITE-22a: while playing, a note dragged a beat later is heard there before it is let go, and
// nothing is written: no revision, no data revision, the lane's file as it was.
#[test]
fn a_lane_lease_previews_with_no_revision() {
    let mut d = Drag::new();
    let (before, file) = (revs(&d.graph), d.fixture.read("data/bass/notes.json"));
    let begun = d.begin(e_at(4.0));
    assert!(begun.previewable && begun.heard && begun.lease.is_some(), "{begun:?}");
    d.graph.lane_update(begun.lease.unwrap(), Some(2), e_at(5.0)).unwrap().expect("a swap was sent");
    d.until(6.0);
    assert_eq!(d.ons(E2), vec![5.0], "heard where it was dragged, not where it was");
    assert_eq!(revs(&d.graph), before, "no revision");
    assert_eq!(d.fixture.read("data/bass/notes.json"), file);
    let listed = d.graph.lane_leases();
    assert_eq!((listed.len(), listed[0].kind, listed[0].seq), (1, "lane", 2));
}

// WRITE-22a, RENDER-26a: the commit writes what was heard, one revision and one undo step, and
// nothing heard changes: the C2 sounding through it isn't cut, and the E2 plays once, where it was
// dragged. Undo takes it back.
#[test]
fn the_commit_changes_nothing_heard() {
    let mut d = Drag::new();
    let before = d.revision();
    let lease = d.begin(e_at(5.0)).lease.unwrap();
    d.until(1.0);
    match d.graph.lane_commit(lease, e_at(5.0)).unwrap() {
        Answer::Done(v) => assert_eq!(v["revision"].as_u64(), Some(before + 1)),
        Answer::Changed(e) => panic!("{e}"),
    }
    assert!(d.graph.lane_leases().is_empty(), "the lease ended");
    d.until(6.0);
    assert_eq!(d.ons(E2), vec![5.0]);
    assert_eq!(d.ons(C2), Vec::<f64>::new(), "the long note sounding since beat 0 wasn't struck again");
    let (_, offs) = d.heard();
    assert!(offs.iter().all(|f| *f >= 5 * BEAT), "nothing ended at the commit: {offs:?}");
    assert_eq!(d.revision(), before + 1);
    assert!(d.fixture.read("data/bass/notes.json").contains("\"start\": 5"));
    d.graph.undo(d.token).unwrap();
    assert!(d.fixture.read("data/bass/notes.json").contains("\"start\": 4"), "one undo step takes it back");
}

// WRITE-22a: cancelled, the accepted notes play again, and nothing is written.
#[test]
fn cancel_puts_the_accepted_notes_back() {
    let mut d = Drag::new();
    let before = d.revision();
    let lease = d.begin(e_at(6.0)).lease.unwrap();
    d.fill(2);
    d.graph.lane_cancel(lease).unwrap();
    d.until(7.0);
    assert_eq!(d.ons(E2), vec![4.0]);
    assert_eq!(d.revision(), before);
    assert!(d.graph.lane_leases().is_empty());
    assert!(d.graph.lane_update(lease, Some(3), e_at(6.0)).unwrap_err().to_string().contains("cancelled"));
}

// WRITE-22a, C6: the window that began a drag closes: its lease ends and the accepted notes play.
#[test]
fn a_closed_window_cancels_its_lane_lease() {
    let mut d = Drag::new();
    d.begin(e_at(6.0));
    d.graph.client_gone("w2");
    assert_eq!(d.graph.lane_leases().len(), 1, "another window's closing leaves it");
    d.graph.client_gone("w1");
    assert!(d.graph.lane_leases().is_empty());
    d.until(7.0);
    assert_eq!(d.ons(E2), vec![4.0]);
}

// WRITE-22a: a lease nothing was heard from for its TTL ends, the accepted notes playing; a
// keepalive keeps another.
#[test]
fn an_idle_lane_lease_expires() {
    let mut d = Drag::new();
    let idle = d.graph.lane_begin(d.token, e_at(6.0), None, Some(Duration::from_millis(100))).unwrap().unwrap().lease.unwrap();
    let kept = d.graph.lane_begin(d.token, moved("b", 1.0), None, Some(Duration::from_millis(100))).unwrap().unwrap().lease.unwrap();
    for _ in 0..4 {
        std::thread::sleep(Duration::from_millis(50));
        d.graph.lane_keepalive(kept).unwrap();
        d.graph.expire();
    }
    let live: Vec<u64> = d.graph.lane_leases().iter().map(|l| l.lease).collect();
    assert_eq!(live, vec![kept]);
    assert!(d.graph.lane_keepalive(idle).unwrap_err().to_string().contains("nothing was heard from it"));
    d.until(7.0);
    assert_eq!(d.ons(E2), vec![4.0]);
}

// WRITE-22a, WRITE-3: an outside write to another clip of the dragged lane is taken in, and the drag
// plays on, applied on it: both are heard.
#[test]
fn an_outside_write_to_another_clip_keeps_the_preview() {
    let mut d = Drag::new();
    let lease = d.begin(e_at(5.0)).lease.unwrap();
    d.fill(2);
    d.fixture.put("data/bass/notes.json", &NOTES.replace(r#""pitch": "G2", "start": 0"#, r#""pitch": "G2", "start": 2"#));
    assert!(d.graph.poll());
    assert_eq!(d.graph.lane_leases().len(), 1, "the drag goes on");
    d.until(19.0);
    assert_eq!(d.ons(E2), vec![5.0], "the drag still heard");
    assert_eq!(d.ons(G2), vec![18.0], "the outside write heard");
    assert!(d.graph.lane_update(lease, Some(3), e_at(6.0)).is_ok());
}

// WRITE-22a, WRITE-12: an outside write to the dragged clip ends the drag: the new notes play, and
// the window hears why at its next request.
#[test]
fn an_outside_write_to_the_dragged_clip_ends_the_lease() {
    let mut d = Drag::new();
    let lease = d.begin(e_at(5.0)).lease.unwrap();
    d.fill(2);
    d.fixture.put("data/bass/notes.json", &NOTES.replace(r#""pitch": "E2", "start": 4"#, r#""pitch": "E2", "start": 7"#));
    assert!(d.graph.poll());
    assert!(d.graph.lane_leases().is_empty(), "the drag ended");
    let said = d.graph.lane_update(lease, Some(3), e_at(6.0)).unwrap_err().to_string();
    assert_eq!(said, "Bass changed elsewhere: your drag was dropped.");
    d.until(8.0);
    assert_eq!(d.ons(E2), vec![7.0], "the outside write plays");
}

// WRITE-22a: a code edit elsewhere during the drag makes a new plan, which plays the drag from its
// first block or the one after.
#[test]
fn a_new_plan_carries_the_preview() {
    let mut d = Drag::new();
    d.begin(e_at(5.0));
    d.fill(2);
    let first = d.driver.engine().unwrap().current().unwrap().generation();
    // A new node: no renderer can reach it, so a plan is prepared and activated.
    let song = SONG.replace("bass(lane.notes('notes'))", "vca(bass(lane.notes('notes')), 0.9)");
    d.fixture.put("song.ts", &format!("import {{ vca }} from 'std/dsp';\n{song}"));
    assert!(d.graph.poll());
    d.fill(2);
    assert_ne!(d.driver.engine().unwrap().current().unwrap().generation(), first, "a new plan plays");
    d.trace();
    d.until(6.0);
    assert_eq!(d.ons(E2), vec![5.0], "the new plan plays the drag");
    assert_eq!(d.graph.lane_leases().len(), 1);
}

// WRITE-22a: let go while a change elsewhere waits on the actor: a fresh engine it makes (another
// sample rate, as a new player or a refused activation does: nothing is kept from what played)
// plays the note where it was let go, not where it was, until the commit lands (NX-S3).
#[test]
fn a_fresh_engine_before_the_commit_plays_what_was_let_go() {
    let mut d = Drag::new();
    let lease = d.begin(e_at(5.0)).lease.unwrap();
    d.fill(2);
    // The actor pauses after the first settle's sends (the rate change's), before the commit's Tx.
    let (paused_tx, paused) = std::sync::mpsc::channel::<()>();
    let (go, go_rx) = std::sync::mpsc::channel::<()>();
    let mut once = true;
    d.graph.inspect(move |a| {
        a.between_phases = Some(Box::new(move |_| {
            if std::mem::take(&mut once) {
                paused_tx.send(()).unwrap();
                let _ = go_rx.recv();
            }
        }))
    });
    let held = d.graph.hold();
    d.fixture.put("music2.toml", &std::fs::read_to_string(d.fixture.0.join("music2.toml")).unwrap().replace("48000", "44100"));
    let (graph, driver) = (&d.graph, &mut d.driver);
    let fill = |driver: &mut Driver| {
        let mut buf = [0.0f32; 256];
        driver.fill(&mut buf, 2);
        drop(driver.drain_retired());
    };
    let ons = std::thread::scope(|s| {
        let polled = s.spawn(|| graph.poll());
        std::thread::sleep(Duration::from_millis(100));
        let commit = s.spawn(|| graph.lane_commit(lease, e_at(5.0)));
        eventually("the commit is on its way", || graph.lane_leases().first().is_some_and(|l| l.state == "committing"));
        std::thread::sleep(Duration::from_millis(50));
        drop(held);
        paused.recv().unwrap();
        // The fresh engine was sent, and the commit waits: what plays now?
        fill(driver);
        fill(driver);
        assert_eq!(driver.engine().unwrap().rate(), 44_100, "the fresh engine plays");
        driver.engine().unwrap().current_mut().unwrap().trace(4096);
        while graph.status().beat < 6.0 {
            fill(driver);
        }
        let entries = driver.engine().unwrap().current().unwrap().trace_entries();
        // At 44.1 kHz, 22050 frames a beat.
        let ons: Vec<f64> =
            entries.iter().filter(|e| e.kind == TraceKind::On { pitch: E2 }).map(|e| e.transport as f64 / 22_050.0).collect();
        go.send(()).unwrap();
        assert!(polled.join().unwrap());
        assert!(matches!(commit.join().unwrap().unwrap(), Answer::Done(_)));
        ons
    });
    assert_eq!(ons, vec![5.0], "the fresh engine plays the note where it was let go");
    assert!(d.fixture.read("data/bass/notes.json").contains("\"start\": 5"));
}

// WRITE-22a: a preview that needs more room than its lane's scheduler has isn't heard until the
// commit, whose new plan grows the lane.
#[test]
fn a_preview_too_big_for_its_room_waits_for_the_commit() {
    let mut d = Drag::new();
    // 300 notes at once, past the 256 every scheduler holds.
    let chord: Vec<Note> = (0..300).map(|i| note(40.0 + (i % 40) as f64, 6.0 + i as f64 * 1e-3, 2.0)).collect();
    let mut notes = vec![note(36.0, 0.0, 12.0), note(40.0, 4.0, 1.0)];
    notes.extend(chord);
    let big = Gesture::Notes { clip: clip("a"), notes, length: None };
    let lease = d.begin(big.clone()).lease.unwrap();
    let seq = d.graph.lane_update(lease, Some(2), big.clone()).unwrap();
    assert_eq!(seq, None, "nothing sent");
    d.until(5.0);
    assert_eq!(d.ons(E2), vec![4.0], "the accepted notes play on");
    let first = d.driver.engine().unwrap().current().unwrap().generation();
    assert!(matches!(d.graph.lane_commit(lease, big).unwrap(), Answer::Done(_)));
    d.fill(2);
    assert_ne!(d.driver.engine().unwrap().current().unwrap().generation(), first, "the commit grew the lane in a new plan");
    d.trace();
    d.until(6.5);
    assert!(d.heard().0.len() >= 256, "the chord plays");
}

// RENDER-29: begin and update never wait for the session, which a compile holds.
#[test]
fn lane_previews_never_wait_for_a_compile() {
    let mut d = Drag::new();
    let held = d.graph.hold();
    let t = Instant::now();
    let lease = d.begin(e_at(5.0)).lease.unwrap();
    d.graph.lane_update(lease, Some(2), e_at(6.0)).unwrap();
    assert!(t.elapsed() < Duration::from_millis(500), "{:?}", t.elapsed());
    d.until(7.0);
    assert_eq!(d.ons(E2), vec![6.0], "heard while the session is busy");
    drop(held);
}

// RENDER-26a: a sounding note dragged away from the playhead ends at once.
#[test]
fn a_note_dragged_away_from_the_playhead_is_ended() {
    let mut d = Drag::new();
    d.until(1.0);
    assert_eq!(d.sounding(), 1, "the C2 sounds");
    let away = Gesture::Notes { clip: clip("a"), notes: vec![note(36.0, 8.0, 4.0), note(40.0, 4.0, 1.0)], length: None };
    d.begin(away);
    d.fill(2);
    assert_eq!(d.sounding(), 0, "it ended");
}

// Q21, RENDER-26a: a clip copied while the song plays inside the copy has the same id in the
// preview and the commit, so the note sounding in it isn't cut at release.
#[test]
fn a_copied_clip_keeps_its_id_from_preview_to_commit() {
    let mut d = Drag::new();
    d.graph.seek(31.0).unwrap();
    d.fill(2);
    // From here: the seek ended the C2 sounding from beat 0.
    d.trace();
    let copy = Gesture::Move { clips: vec![clip("a")], by: 32.0, to: None, copy: true };
    let lease = d.begin(copy.clone()).lease.unwrap();
    d.until(33.0);
    assert_eq!(d.sounding(), 1, "the copy's C2 sounds");
    let Answer::Done(done) = d.graph.lane_commit(lease, copy).unwrap() else { panic!() };
    d.until(35.0);
    assert_eq!(d.sounding(), 1, "still sounding after the commit");
    let (ons, offs) = d.heard();
    assert_eq!(ons.iter().filter(|(_, p)| *p == C2).count(), 1, "struck once: {ons:?}");
    assert!(offs.is_empty(), "nothing ended: {offs:?}");
    let id = done["made"][0]["id"].as_str().unwrap();
    assert!(d.fixture.read("data/bass/notes.json").contains(&format!("\"id\": \"{id}\"")));
}

// design-8b §9.1: a gesture on an audio lane needs renders: it isn't previewed, and begins nothing.
#[test]
fn an_audio_lane_gesture_is_not_previewable() {
    fn copy(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap().flatten() {
            let (f, t) = (e.path(), to.join(e.file_name()));
            if f.is_dir() { copy(&f, &t) } else { std::fs::copy(&f, &t).map(drop).unwrap() }
        }
    }
    let fixture = Fixture::new();
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/resample"), &fixture.0);
    let graph = fixture.open();
    let state = graph.state().unwrap();
    let token = state["session"].as_u64().unwrap();
    let tracks = state["timeline"]["tracks"].as_array().unwrap();
    let (module, lane, id) = tracks
        .iter()
        .find_map(|t| {
            let l = t["lanes"].as_array()?.iter().find(|l| l["kind"] == "audio" && l["clips"].as_array().is_some_and(|c| !c.is_empty()))?;
            Some((t["module"].as_str()?.to_string(), l["key"].as_str()?.to_string(), l["clips"][0]["id"].as_str()?.to_string()))
        })
        .expect("an audio clip");
    let g = Gesture::Move {
        clips: vec![ClipRef { lane: LaneAddr::new(&module, &lane).unwrap(), id: Some(id), at: None, sha: None }],
        by: 4.0,
        to: None,
        copy: false,
    };
    let begun = graph.lane_begin(token, g, None, None).unwrap().unwrap();
    assert_eq!((begun.previewable, begun.lease), (false, None));
    assert!(graph.lane_leases().is_empty());
}

// RENDER-1, RENDER-3: an export renders the accepted revision: a drag in progress never reaches it.
#[test]
fn an_export_during_a_lane_lease_has_the_accepted_notes() {
    let d = Drag::new();
    let render = |name: &str| {
        let (_, _, r, _) = d.graph.render(m2_editor::render::Length::Seconds(4.0)).unwrap();
        let out = d.fixture.0.join(name);
        r.write(&out, |_| {}).unwrap();
        std::fs::read(out).unwrap()
    };
    let lease = d.begin(e_at(1.0)).lease.unwrap();
    let during = render("during.wav");
    d.graph.lane_cancel(lease).unwrap();
    assert_eq!(during, render("after.wav"), "the same bytes");
}

// WRITE-22a: an automation point dragged while playing is heard as a control lane swap.
#[test]
fn an_automation_drag_is_heard_before_it_is_written() {
    let song = "import { song, track } from '@m2/core';\nimport { osc, vca } from 'std/dsp';\nimport { hz } from '@m2/core';\n\
                const t = track('bass', ({ lane }) => vca(osc.square(hz(0)), lane.control('g', 'gain', 1)));\nexport default song(() => t.out);\n";
    let mut d = Drag::with(song, NOTES);
    d.fixture.put("data/bass/g.json", r#"{"unit": "gain", "clips": [{"id": "g1", "start": 0, "length": 64, "points": [[0, 0.5]]}]}"#);
    assert!(d.graph.poll());
    d.fill(4);
    let level = |d: &mut Drag| {
        let mut buf = [0.0f32; 256];
        d.driver.fill(&mut buf, 2);
        buf[254]
    };
    assert!((level(&mut d) - 0.5).abs() < 1e-3);
    let before = d.revision();
    let g1 = ClipRef { lane: LaneAddr::new("bass", "g").unwrap(), id: Some("g1".into()), at: None, sha: None };
    let lease = d.begin(Gesture::Points { clip: g1.clone(), points: vec![(0.0, 0.25)] }).lease.unwrap();
    d.fill(4);
    assert!((level(&mut d) - 0.25).abs() < 1e-3, "heard");
    assert_eq!(d.revision(), before);
    assert!(matches!(d.graph.lane_commit(lease, Gesture::Points { clip: g1, points: vec![(0.0, 0.25)] }).unwrap(), Answer::Done(_)));
    d.fill(4);
    assert!((level(&mut d) - 0.25).abs() < 1e-3, "kept");
    assert_eq!(d.revision(), before + 1);
    eventually("the lease ended", || d.graph.lane_leases().is_empty());
}

// AGENT-3, WRITE-22a (the review's F1): an agent's change to the code that would end the musician's
// clip drag is taken back at its settle, though it touches no lane file: here song.ts, renaming the
// track whose clip is dragged (`m2 edit` makes it so). The drag goes on, and one that leaves it be
// lands.
#[test]
fn an_agents_rename_that_would_end_a_clip_drag_is_taken_back() {
    use m2_song::tx::{Base, Edit, Expect, Op, Pos, Tx, TxPath};
    use serde_json::json;
    let mut d = Drag::new();
    let lease = d.begin(e_at(4.0)).lease.expect("a lane lease");
    d.fill(2);
    let agent = || crate::caller::Caller::agent(Some("Claude"), Some("rename the bass"));
    // On the code as it is now, committed on the hash its check shows, as `m2 edit` commits it.
    let seen = |d: &Drag, old: &str, new: &str| {
        let text = d.fixture.read("song.ts");
        let at = text.find(old).unwrap();
        let path = TxPath::new("song.ts").unwrap();
        let edits =
            vec![Edit { start: Pos::Byte(at as u32), end: Pos::Byte((at + old.len()) as u32), expected: old.into(), text: new.into() }];
        let mut tx = Tx::new(
            "tx-agent",
            ("a".into(), "b".into()),
            vec![Op::TextEdit { file: path.clone(), edits }],
            Expect::Seen(m2_song::Sha::of(b"")),
        );
        tx.base.insert(path, Base::text(&text));
        let mut json = tx.to_json();
        let (status, checked) = d.graph.tx_check(Some(d.token), &json).unwrap();
        assert_eq!(status, 200, "{checked}");
        json["expect"] = json!({"seen": checked["checked"]["hash"]});
        json
    };
    // A change that leaves the drag be: a comment.
    let tx = seen(&d, "export default", "// the bass\nexport default");
    let (status, body) = crate::caller::as_caller(agent(), || d.graph.tx_commit(Some(d.token), &tx, None, &[])).unwrap();
    assert_eq!(status, 200, "{body}");
    d.fill(2);
    assert!(d.graph.lane_leases().iter().any(|l| l.lease == lease), "the drag goes on");
    // The rename would drop it: taken back.
    let song = d.fixture.read("song.ts");
    let tx = seen(&d, "track('bass'", "track('sub'");
    let (status, body) = crate::caller::as_caller(agent(), || d.graph.tx_commit(Some(d.token), &tx, None, &[])).unwrap();
    assert_eq!((status, &body["busy"]["what"], &body["busy"]["taken_back"]), (423, &json!("lane"), &json!(true)), "{body}");
    assert!(body["busy"]["words"].as_str().unwrap().contains("dragging a clip on"), "{body}");
    assert_eq!(d.fixture.read("song.ts"), song, "taken back");
    d.fill(4);
    assert!(d.graph.lane_leases().iter().any(|l| l.lease == lease), "the drag goes on");
    d.graph.lane_update(lease, Some(3), e_at(5.0)).unwrap();
}
