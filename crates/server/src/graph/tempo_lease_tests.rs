//! The tempo lease (RENDER-27a, 8b.7): the tempo dragged is heard at the first block after its
//! plan is prepared, one prepare in flight with the newest value next; an edit meanwhile plays at
//! the dragged tempo; the commit is one Tx, a cancel or a closed window goes back; seeks and
//! exports keep to their maps; a song with audio hears it on release (Q23's fallback).

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use m2_play::{Driver, Player};
use m2_runtime::TraceKind;

use super::super::tests::{Fixture, eventually};
use super::*;

/// A bass track playing its `notes` lane.
const SONG: &str = "import { song, track } from '@m2/core';\nimport { bass } from 'std/instruments';\n\
                    const b = track('bass', ({ lane }) => bass(lane.notes('notes')));\nexport default song(() => b.out);\n";

/// An E2 at beat 4, and one at beat 12.
const NOTES: &str = r#"{"clips": [{"id": "a", "start": 0, "length": 16, "notes": [{"pitch": "E2", "start": 4, "length": 1}, {"pitch": "E2", "start": 12, "length": 1}]}]}"#;
const E2: f32 = 40.0;

/// The song open in a driven player at 120 BPM, playing, its renderer traced. The test's thread is
/// the audio thread: it fills.
struct Drag {
    fixture: Fixture,
    graph: Graph,
    driver: Driver,
    token: u64,
}

impl Drag {
    fn new() -> Drag {
        let fixture = Fixture::new();
        fixture.put("song.ts", SONG);
        fixture.put("data/bass/notes.json", NOTES);
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

    fn until(&mut self, beat: f64) {
        let mut n = 0;
        while self.graph.status().beat < beat {
            self.fill(1);
            n += 1;
            assert!(n < 100_000, "the transport never reached beat {beat}");
        }
    }

    /// The transport frames E2 was struck at, since the trace began.
    fn ons(&mut self) -> Vec<u64> {
        let entries = self.driver.engine().unwrap().current().unwrap().trace_entries();
        entries.iter().filter(|e| e.kind == TraceKind::On { pitch: E2 }).map(|e| e.transport).collect()
    }

    fn begin(&self) -> TempoBegun {
        self.graph.tempo_begin(self.token, 0.0, Some("w1".into()), None).unwrap()
    }

    /// Wait until the drag's value `bpm` is heard: its plan sent, and taken by the engine.
    fn heard(&mut self, bpm: f64) {
        eventually("the value is heard", || self.graph.tempo_leases().first().and_then(|l| l.heard) == Some(bpm));
        self.fill(2);
    }

    /// The tempo of the arrangement the plan that plays was built with in place of the accepted one's.
    fn leased(&self) -> Option<f64> {
        self.graph.inspect(|a| a.open.as_ref().unwrap().plays.as_ref().unwrap().tempo.as_ref().map(|t| t.tempo[0].bpm))
    }

    fn revision(&self) -> u64 {
        self.graph.state().unwrap()["view"]["revision"].as_u64().unwrap()
    }
}

// RENDER-27a: a value dragged is heard at the first block after its plan is prepared, playing:
// the status says the new tempo, and a note plays at its beat in the new map (beat 4 at 240 BPM is
// frame 48000, not 96000). Nothing is written.
#[test]
fn the_tempo_is_heard_at_the_first_block_after_its_prepare() {
    let mut d = Drag::new();
    let (revision, file) = (d.revision(), d.fixture.0.join("data/arrangement.json").exists());
    let begun = d.begin();
    assert_eq!((begun.at, begun.bpm, begun.heard), (0.0, 120.0, true));
    d.graph.tempo_update(begun.lease, Some(1), 240.0).unwrap();
    d.heard(240.0);
    assert_eq!(d.graph.status().bpm, Some(240.0), "the status follows");
    assert_eq!(d.leased(), Some(240.0));
    d.trace();
    d.until(5.0);
    assert_eq!(d.ons(), vec![48_000], "struck at beat 4 in the dragged map");
    assert_eq!((d.revision(), d.fixture.0.join("data/arrangement.json").exists()), (revision, file), "nothing written");
}

// RENDER-27a, Q22, X24: one prepare in flight. Fifty updates while the first prepare is held: an
// update that finds the drag's job queued or running queues none, and the job runs again while a
// newer value waits. So a few jobs run, not fifty, and the last plays the 50th value. The prepare
// is held until the 50th update is sent, not for a time: a sleep lasts what the OS makes it (on
// GitHub's Mac, about four times what was asked), so timed updates could outlast timed prepares.
#[test]
fn one_tempo_prepare_in_flight() {
    let mut d = Drag::new();
    let lease = d.begin().lease;
    let jobs = Arc::new(AtomicU32::new(0));
    let ran = jobs.clone();
    let (sent, all_sent) = std::sync::mpsc::channel::<()>();
    d.graph.inspect(move |a| {
        a.between_phases = Some(Box::new(move |_| {
            if ran.fetch_add(1, Ordering::Relaxed) == 0 {
                all_sent.recv_timeout(Duration::from_secs(10)).expect("the 50th update was sent");
            }
        }))
    });
    let graph = &d.graph;
    std::thread::scope(|s| {
        s.spawn(|| {
            for i in 1..=50 {
                graph.tempo_update(lease, Some(i), 120.0 + i as f64).unwrap();
            }
            sent.send(()).unwrap();
        });
    });
    d.heard(170.0);
    std::thread::sleep(Duration::from_millis(200));
    let l = &d.graph.tempo_leases()[0];
    let ran = jobs.load(Ordering::Relaxed);
    assert!(
        ran <= 4 && l.previews <= 4,
        "{ran} jobs ran and {} plans played: one prepare in flight, the values between skipped",
        l.previews
    );
    assert_eq!(l.heard, Some(170.0), "the last plan plays the 50th value");
    assert_eq!(d.graph.status().bpm, Some(170.0));
}

// RENDER-27a, Q22: fifty updates while the session is busy queue one job, which plays the newest
// value once.
#[test]
fn updates_while_the_session_is_busy_play_the_newest_once() {
    let mut d = Drag::new();
    let lease = d.begin().lease;
    let held = d.graph.hold();
    for i in 1..=50 {
        d.graph.tempo_update(lease, Some(i), 120.0 + i as f64).unwrap();
    }
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(d.graph.tempo_leases()[0].previews, 0, "nothing heard while the session is busy");
    drop(held);
    d.heard(170.0);
    std::thread::sleep(Duration::from_millis(100));
    let l = &d.graph.tempo_leases()[0];
    assert_eq!((l.previews, l.heard), (1, Some(170.0)), "one activation, carrying the 50th value");
    assert_eq!(d.graph.status().bpm, Some(170.0));
}

// RENDER-27a (design-8b §9.5): what a tempo preview costs to prepare, p50, for the build notes.
// Information only: Linux timings vary.
#[test]
fn the_tempo_prepare_is_measured() {
    let mut d = Drag::new();
    let lease = d.begin().lease;
    let mut ns = vec![];
    for i in 1..=21 {
        let bpm = 120.0 + i as f64;
        d.graph.tempo_update(lease, Some(i), bpm).unwrap();
        d.heard(bpm);
        ns.push(d.graph.inspect(|a| a.open.as_ref().unwrap().prepared.prepare_ns));
    }
    ns.sort_unstable();
    eprintln!("tempo prepare p50: {:.2} ms (of {} prepares)", ns[ns.len() / 2] as f64 / 1e6, ns.len());
    assert!(ns.iter().all(|n| *n > 0));
}

// RENDER-27a: an edit while the drag lives plays at the dragged tempo (its plan prepared with the
// drag's map), and the drag goes on from it.
#[test]
fn a_stale_tempo_prepare_is_done_again() {
    let mut d = Drag::new();
    let lease = d.begin().lease;
    d.graph.tempo_update(lease, Some(1), 240.0).unwrap();
    d.heard(240.0);
    let first = d.driver.engine().unwrap().current().unwrap().generation();
    // A new node: a new plan.
    let song = SONG.replace("bass(lane.notes('notes'))", "vca(bass(lane.notes('notes')), 0.9)");
    d.fixture.put("song.ts", &format!("import {{ vca }} from 'std/dsp';\n{song}"));
    assert!(d.graph.poll());
    d.fill(2);
    assert_ne!(d.driver.engine().unwrap().current().unwrap().generation(), first, "the edit's plan plays");
    assert_eq!((d.leased(), d.graph.status().bpm), (Some(240.0), Some(240.0)), "at the dragged tempo");
    d.graph.tempo_update(lease, Some(2), 180.0).unwrap();
    d.heard(180.0);
    assert_eq!((d.leased(), d.graph.status().bpm), (Some(180.0), Some(180.0)), "and the drag goes on");
}

// RENDER-27a, design-8b §4.1: the commit is one Tx writing data/arrangement.json, one undo step;
// the lease ends, and the accepted arrangement plays (no drag's in its place).
#[test]
fn a_tempo_commit_is_one_tx() {
    let mut d = Drag::new();
    let before = d.revision();
    let lease = d.begin().lease;
    d.graph.tempo_update(lease, Some(1), 150.0).unwrap();
    d.heard(150.0);
    let TempoCommitted::Done(v) = d.graph.tempo_commit(lease, 150.0).unwrap() else { panic!("refused") };
    assert_eq!(v["revision"].as_u64(), Some(before + 1));
    assert!(d.fixture.read("data/arrangement.json").contains("150"), "{}", d.fixture.read("data/arrangement.json"));
    assert!(d.graph.tempo_leases().is_empty());
    d.fill(2);
    assert_eq!((d.leased(), d.graph.status().bpm), (None, Some(150.0)));
    d.graph.undo(d.token).unwrap();
    d.fill(2);
    assert_eq!(d.graph.status().bpm, Some(120.0), "one undo step back");
}

// RENDER-27a: let go while another job waits on the actor: every settle until the commit lands
// plays the value let go, never the old tempo, and the commit's own plays it too (NX-S3).
#[test]
fn a_settle_before_the_commit_plays_the_value_let_go() {
    let mut d = Drag::new();
    let lease = d.begin().lease;
    d.graph.tempo_update(lease, Some(1), 150.0).unwrap();
    d.heard(150.0);
    let played: Arc<parking_lot::Mutex<Vec<Option<f64>>>> = Arc::default();
    let seen = played.clone();
    d.graph.inspect(move |a| a.between_phases = Some(Box::new(move |inner: &Inner| seen.lock().push(inner.status().bpm))));
    let held = d.graph.hold();
    // Dragged on to 160: its job waits behind the busy session.
    d.graph.tempo_update(lease, Some(2), 160.0).unwrap();
    let graph = &d.graph;
    std::thread::scope(|s| {
        // Let go: the commit's Tx waits behind the drag's job.
        let commit = s.spawn(|| graph.tempo_commit(lease, 160.0));
        eventually("the commit is on its way", || graph.tempo_leases().first().is_some_and(|l| l.state == "committing"));
        std::thread::sleep(Duration::from_millis(50));
        drop(held);
        assert!(matches!(commit.join().unwrap().unwrap(), TempoCommitted::Done(_)));
    });
    let played = played.lock().clone();
    assert!(played.len() >= 2 && played.iter().all(|b| *b == Some(160.0)), "each settle played the value let go: {played:?}");
    assert!(d.fixture.read("data/arrangement.json").contains("160"));
}

// RENDER-27a: a commit the disk refuses ends the drag, and the accepted tempo plays again.
#[test]
fn a_tempo_commit_that_fails_goes_back() {
    use std::os::unix::fs::PermissionsExt;
    let mut d = Drag::new();
    let lease = d.begin().lease;
    d.graph.tempo_update(lease, Some(1), 150.0).unwrap();
    d.heard(150.0);
    // `.music2/tx/` read-only, as a full or failing disk refuses a commit's journal.
    let tx = d.fixture.0.join(".music2/tx");
    std::fs::create_dir_all(&tx).unwrap();
    std::fs::set_permissions(&tx, std::fs::Permissions::from_mode(0o500)).unwrap();
    let failed = d.graph.tempo_commit(lease, 150.0).is_err();
    std::fs::set_permissions(&tx, std::fs::Permissions::from_mode(0o755)).unwrap();
    if !failed {
        eprintln!("skipped: .music2/tx/ is writable read-only (root)");
        return;
    }
    assert!(d.graph.tempo_leases().is_empty());
    eventually("the accepted tempo plays", || d.graph.status().bpm == Some(120.0));
    d.fill(2);
    assert_eq!(d.leased(), None);
}

// RENDER-27a: cancelled, the accepted tempo plays again, and nothing is written.
#[test]
fn a_tempo_cancel_goes_back() {
    let mut d = Drag::new();
    let lease = d.begin().lease;
    d.graph.tempo_update(lease, Some(1), 200.0).unwrap();
    d.heard(200.0);
    d.graph.tempo_cancel(lease).unwrap();
    eventually("the accepted tempo plays", || d.graph.status().bpm == Some(120.0));
    d.fill(2);
    assert_eq!(d.leased(), None);
    assert!(!d.fixture.0.join("data/arrangement.json").exists(), "nothing written");
    assert!(d.graph.tempo_update(lease, Some(2), 210.0).unwrap_err().to_string().contains("cancelled"));
}

// RENDER-27a, C6: the window that began the drag closes: it ends, and the accepted tempo plays.
// Meanwhile another window can't drag the tempo.
#[test]
fn a_closed_window_ends_its_tempo_lease() {
    let mut d = Drag::new();
    let lease = d.begin().lease;
    d.graph.tempo_update(lease, Some(1), 200.0).unwrap();
    d.heard(200.0);
    assert!(d.graph.tempo_begin(d.token, 0.0, Some("w2".into()), None).unwrap_err().to_string().contains("another window"));
    d.graph.client_gone("w1");
    assert!(d.graph.tempo_leases().is_empty());
    eventually("the accepted tempo plays", || d.graph.status().bpm == Some(120.0));
}

// RENDER-27a: an idle drag ends after its TTL; a keepalive keeps one.
#[test]
fn an_idle_tempo_lease_expires() {
    let d = Drag::new();
    let lease = d.graph.tempo_begin(d.token, 0.0, None, Some(Duration::from_millis(100))).unwrap().lease;
    for _ in 0..4 {
        std::thread::sleep(Duration::from_millis(50));
        d.graph.tempo_keepalive(lease).unwrap();
        d.graph.expire();
    }
    assert_eq!(d.graph.tempo_leases().len(), 1, "kept alive");
    std::thread::sleep(Duration::from_millis(150));
    d.graph.expire();
    assert!(d.graph.tempo_leases().is_empty());
    assert!(d.graph.tempo_keepalive(lease).unwrap_err().to_string().contains("nothing was heard from it"));
}

// RENDER-27a: a seek while the drag plays lands on its beat in the dragged map: beat 8 at 240 BPM
// is frame 96000.
#[test]
fn a_seek_during_a_tempo_lease_lands_on_its_beat() {
    let mut d = Drag::new();
    let lease = d.begin().lease;
    d.graph.tempo_update(lease, Some(1), 240.0).unwrap();
    d.heard(240.0);
    d.graph.seek(8.0).unwrap();
    d.fill(1);
    let at = d.driver.engine().unwrap().current().unwrap().transport().pos();
    assert!((96_000..96_000 + 256).contains(&at), "frame {at}");
    assert!((8.0..8.03).contains(&d.graph.status().beat), "a block past beat 8: {}", d.graph.status().beat);
}

/// Clip `a` with its first E2 at `e` (beats into the clip), the second at 12: a drag of a note.
fn e_at(e: f64) -> m2_editor::clips::Gesture {
    let clip =
        m2_editor::clips::ClipRef { lane: m2_song::LaneAddr::new("bass", "notes").unwrap(), id: Some("a".into()), at: None, sha: None };
    let note = |start: f64| m2_song::Note { pitch: 40.0, start, length: 1.0, velocity: 0.8 };
    m2_editor::clips::Gesture::Notes { clip, notes: vec![note(e), note(12.0)], length: None }
}

// RENDER-27a, WRITE-22a: a note dragged while a tempo drag plays is heard in the dragged map:
// beat 5 at 240 BPM is frame 60000, not 120000 (NX-S4).
#[test]
fn a_note_dragged_during_a_tempo_drag_plays_in_its_map() {
    let mut d = Drag::new();
    let lease = d.begin().lease;
    d.graph.tempo_update(lease, Some(1), 240.0).unwrap();
    d.heard(240.0);
    let begun = d.graph.lane_begin(d.token, e_at(5.0), Some("w2".into()), None).unwrap().unwrap();
    assert!(begun.previewable && begun.heard, "{begun:?}");
    d.fill(2);
    d.trace();
    d.until(6.0);
    assert_eq!(d.ons(), vec![60_000], "struck at beat 5 in the dragged map");
}

// RENDER-27a, WRITE-22a: the tempo dragged while a note drag plays: the tempo's plan carries the
// note drag, timed in its own map (NX-S4).
#[test]
fn a_tempo_drag_carries_a_note_drag_in_its_map() {
    let mut d = Drag::new();
    let note = d.graph.lane_begin(d.token, e_at(5.0), Some("w2".into()), None).unwrap().unwrap();
    assert!(note.previewable && note.heard, "{note:?}");
    let lease = d.begin().lease;
    d.graph.tempo_update(lease, Some(1), 240.0).unwrap();
    d.heard(240.0);
    d.trace();
    d.until(6.0);
    assert_eq!(d.ons(), vec![60_000], "struck at beat 5 in the dragged map");
}

// RENDER-1, RENDER-3: an export renders the accepted tempo: a drag in progress never reaches it.
#[test]
fn an_export_during_a_tempo_lease_has_the_accepted_tempo() {
    let mut d = Drag::new();
    let render = |d: &Drag, name: &str| {
        let (_, _, r, _) = d.graph.render(m2_editor::render::Length::Seconds(4.0)).unwrap();
        let out = d.fixture.0.join(name);
        r.write(&out, |_| {}).unwrap();
        std::fs::read(out).unwrap()
    };
    let lease = d.begin().lease;
    d.graph.tempo_update(lease, Some(1), 240.0).unwrap();
    d.heard(240.0);
    let during = render(&d, "during.wav");
    d.graph.tempo_cancel(lease).unwrap();
    assert_eq!(during, render(&d, "after.wav"), "the same bytes");
}

// X24: a commit after another writer changed the tempo is refused in words, and writes nothing.
#[test]
fn a_tempo_commit_after_another_change_is_refused() {
    let mut d = Drag::new();
    let lease = d.begin().lease;
    d.graph.tempo_update(lease, Some(1), 150.0).unwrap();
    d.heard(150.0);
    let outside = "{\"tempo\": [{\"beat\": 0, \"bpm\": 90}], \"meter\": []}\n";
    d.fixture.put("data/arrangement.json", outside);
    assert!(d.graph.poll());
    match d.graph.tempo_commit(lease, 150.0).unwrap() {
        TempoCommitted::Changed(why) => assert_eq!(why, "The tempo changed elsewhere: your drag was dropped."),
        TempoCommitted::Done(v) => panic!("written: {v}"),
    }
    assert_eq!(d.fixture.read("data/arrangement.json"), outside);
    d.fill(2);
    eventually("the outside tempo plays", || d.graph.status().bpm == Some(90.0));
}

// Q23's fallback (X3): in a song whose plan plays audio, the drag previews nothing (its renders are
// timed in the accepted map), and the release is heard as before.
#[test]
fn a_song_with_audio_hears_the_tempo_on_release() {
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
    let token = graph.state().unwrap()["session"].as_u64().unwrap();
    let bpm = graph.status().bpm.unwrap();
    let begun = graph.tempo_begin(token, 0.0, None, None).unwrap();
    assert!(!begun.heard, "not heard as it goes");
    graph.tempo_update(begun.lease, Some(1), bpm + 10.0).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!((graph.tempo_leases()[0].previews, graph.status().bpm), (0, Some(bpm)), "nothing previewed");
    assert!(matches!(graph.tempo_commit(begun.lease, bpm + 10.0).unwrap(), TempoCommitted::Done(_)));
    eventually("heard once written", || graph.status().bpm == Some(bpm + 10.0));
}
