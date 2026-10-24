//! A commit's preview (PLANE-5): a data Tx is heard once it passed its checks, while the session
//! makes it durable, and taken back when that fails; nothing is accepted before it is durable.

use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use m2_session::tx::stage::{Disk, How, Kind, Real};
use m2_song::tx::{Base, Confined, Expect, Op, TxPath};

use super::tests::{Fixture, KICK, eventually, generation, kick_clips, knob, player, synced};
use super::*;

/// What a test does at a disk call.
enum Act {
    Fail,
    /// Fail, and fail the next call that matches this too.
    FailThen(String),
    Panic,
    Sleep(Duration),
    /// Run something first: an outside writer, or a player replaced, at that moment.
    Before(Box<dyn Fn() + Send + Sync>),
}

/// A disk that does what the real one does, after doing what a test asked at the first call whose
/// description (`barrier <path>`, `rename <from> <to> <how>`, …) contains a pattern: once each.
#[derive(Clone, Default)]
struct Faulty {
    at: Arc<Mutex<Vec<(String, Act)>>>,
    /// Every call as it began, on the player's clock (`m2_play::now_ns`).
    calls: Arc<Mutex<Vec<(u64, String)>>>,
}

impl Faulty {
    fn at(&self, pattern: &str, act: Act) {
        self.at.lock().push((pattern.to_string(), act));
    }

    fn tick(&self, what: String) -> io::Result<()> {
        self.calls.lock().push((m2_play::now_ns(), what.clone()));
        let act = {
            let mut at = self.at.lock();
            at.iter().position(|(p, _)| what.contains(p.as_str())).map(|i| at.remove(i).1)
        };
        match act {
            Some(Act::Fail) => Err(io::Error::other("injected")),
            Some(Act::FailThen(next)) => {
                self.at(&next, Act::Fail);
                Err(io::Error::other("injected"))
            }
            Some(Act::Panic) => panic!("a bug at {what}"),
            Some(Act::Sleep(d)) => {
                std::thread::sleep(d);
                Ok(())
            }
            Some(Act::Before(f)) => {
                f();
                Ok(())
            }
            None => Ok(()),
        }
    }
}

impl Disk for Faulty {
    fn read(&mut self, p: &Path) -> io::Result<Vec<u8>> {
        self.tick(format!("read {}", p.display()))?;
        Real.read(p)
    }
    fn kind(&mut self, p: &Path) -> io::Result<Option<Kind>> {
        self.tick(format!("kind {}", p.display()))?;
        Real.kind(p)
    }
    fn create(&mut self, p: &Path, bytes: &[u8]) -> io::Result<()> {
        self.tick(format!("create {}", p.display()))?;
        Real.create(p, bytes)
    }
    fn mkdirs(&mut self, p: &Path) -> io::Result<()> {
        self.tick(format!("mkdirs {}", p.display()))?;
        Real.mkdirs(p)
    }
    fn sync_dir(&mut self, p: &Path) -> io::Result<()> {
        self.tick(format!("sync_dir {}", p.display()))?;
        Real.sync_dir(p)
    }
    fn barrier(&mut self, file: &Path) -> io::Result<()> {
        self.tick(format!("barrier {}", file.display()))?;
        Real.barrier(file)
    }
    fn rename(&mut self, from: &Path, to: &Path, how: How) -> io::Result<()> {
        self.tick(format!("rename {} {} {how:?}", from.display(), to.display()))?;
        Real.rename(from, to, how)
    }
    fn remove_file(&mut self, p: &Path) -> io::Result<()> {
        self.tick(format!("remove_file {}", p.display()))?;
        Real.remove_file(p)
    }
    fn remove_dir(&mut self, p: &Path) -> io::Result<()> {
        self.tick(format!("remove_dir {}", p.display()))?;
        Real.remove_dir(p)
    }
    fn remove_tree(&mut self, p: &Path) -> io::Result<()> {
        self.tick(format!("remove_tree {}", p.display()))?;
        Real.remove_tree(p)
    }
    fn clone_file(&mut self, from: &Path, to: &Path) -> io::Result<()> {
        self.tick(format!("clone_file {} {}", from.display(), to.display()))?;
        Real.clone_file(from, to)
    }
    fn list(&mut self, dir: &Path) -> io::Result<Vec<String>> {
        self.tick(format!("list {}", dir.display()))?;
        Real.list(dir)
    }
}

/// The kick project open and playing, its session committing through `disk`, with its lane file
/// written once (so the next write replaces it): the token and the revision.
fn kick(fixture: &Fixture, disk: Option<Faulty>) -> (Graph, u64, u64) {
    fixture.write(KICK);
    let graph = Graph::silent();
    if let Some(disk) = disk {
        graph.inspect(move |a| a.disk = Some(Box::new(move || Box::new(disk.clone()))));
    }
    graph.open(&fixture.0).unwrap();
    graph.transport("play").unwrap();
    eventually("it plays", || graph.status().playing);
    let (token, revision) = token_and_revision(&graph);
    graph.write_notes(token, revision, "kick", "hits", &kick_clips(&["C1"])).unwrap();
    let (token, revision) = token_and_revision(&graph);
    (graph, token, revision)
}

fn token_and_revision(graph: &Graph) -> (u64, u64) {
    let state = graph.state().unwrap();
    (state["session"].as_u64().unwrap(), state["view"]["revision"].as_u64().unwrap())
}

/// Whether what the engine plays is the accepted revision, by its own record: the same revision
/// (its lanes the same data), in the player there is.
fn plays_accepted(graph: &Graph) -> bool {
    let epoch = graph.inner.current_device().map(|d| d.epoch);
    graph.inspect(move |a| {
        let o = a.open.as_ref().unwrap();
        let p = o.plays.as_ref().unwrap();
        Arc::ptr_eq(&p.revision, &o.session.accepted_arc()) && o.playing == Some(o.session.accepted().number) && Some(p.epoch) == epoch
    })
}

/// The kick's notes in the accepted revision, by pitch.
fn accepted_pitches(graph: &Graph) -> Vec<f64> {
    graph.inspect(|a| {
        let data = &a.open.as_ref().unwrap().session.accepted().compiled.data;
        match data.lane("kick", "hits").map(|l| (**l).clone()) {
            Some(m2_song::LaneData::Notes(clips)) => clips.iter().flat_map(|c| c.notes.iter().map(|n| n.pitch)).collect(),
            _ => vec![],
        }
    })
}

/// When the engine took message `seq`, on the player's clock.
fn applied_ns(graph: &Graph, seq: u64) -> u64 {
    eventually("the engine takes it", || graph.applied(seq).is_some());
    graph.applied(seq).unwrap().callback_ns
}

/// `.music2/tx/` read-only while this lives, as a full or failing disk refuses a commit's journal.
struct Refusing(std::path::PathBuf);

impl Drop for Refusing {
    fn drop(&mut self) {
        let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
    }
}

/// [`Refusing`] on the fixture's `.music2/tx/`; None when it can still be written (as root).
fn refusing(fixture: &Fixture) -> Option<Refusing> {
    let tx = Refusing(fixture.0.join(".music2/tx"));
    std::fs::set_permissions(&tx.0, std::fs::Permissions::from_mode(0o500)).unwrap();
    if std::fs::write(tx.0.join("probe"), b"").is_ok() {
        let _ = std::fs::remove_file(tx.0.join("probe"));
        eprintln!("skipped: .music2/tx/ is writable read-only (root)");
        return None;
    }
    Some(tx)
}

// PLANE-5: a lane write is heard before it is durable: its preview is sent once its checks pass,
// and the engine takes it while the commit's barrier is still running (slowed here to 300 ms).
// The settle after the commit sends nothing more: the same renderer plays the accepted revision.
#[test]
fn a_lane_write_is_heard_before_it_is_durable() {
    let fixture = Fixture::new();
    let disk = Faulty::default();
    let (graph, token, revision) = kick(&fixture, Some(disk.clone()));
    let first = generation(&graph);
    disk.at("barrier", Act::Sleep(Duration::from_millis(300)));
    graph.write_notes(token, revision, "kick", "hits", &kick_clips(&["C1", "D1"])).unwrap();
    let t = graph.last_edit().unwrap();
    let seq = t.seq.unwrap();
    assert!(t.previewed && t.taken_back.is_none(), "{t:?}");
    assert!(t.requested_ns <= t.ready_ns && t.ready_ns <= t.sent_ns && t.sent_ns + 300_000_000 <= t.compiled_ns, "{t:?}");
    assert!(applied_ns(&graph, seq) < t.compiled_ns, "heard before it was durable");
    assert_eq!(graph.inspect(|a| a.open.as_ref().unwrap().sent), Some(seq), "the settle after it sent nothing");
    assert_eq!(generation(&graph), first, "a swap, no new renderer");
    assert!(plays_accepted(&graph));
    assert_eq!(accepted_pitches(&graph), [24.0, 26.0]);
    assert!(fixture.read("data/kick/hits.json").contains("\"D1\""));
}

// PLANE-5, WRITE-15: the preview never skips the sync. Each of several lane writes heard as its
// preview is made durable by exactly one barrier (`Real::barrier`: `F_FULLFSYNC` on macOS), on its
// own journal once that is in place, after the preview went out and before any rename into the
// project; while the barrier runs nothing is accepted (the view shown and the file are the write's
// base), and the revision moves once the commit returns.
#[test]
fn every_previewed_write_is_made_durable_by_its_own_barrier_before_it_is_accepted() {
    let fixture = Fixture::new();
    let disk = Faulty::default();
    let (graph, token, mut revision) = kick(&fixture, Some(disk.clone()));
    for p in ["D1", "E1", "F1", "G1", "A1"] {
        let before = fixture.read("data/kick/hits.json");
        let seen = Arc::new(Mutex::new(None));
        let (inner, s, file) = (graph.inner.clone(), seen.clone(), fixture.0.join("data/kick/hits.json"));
        disk.at(
            "barrier",
            Act::Before(Box::new(move || {
                let shown = inner.state().and_then(|v| v["view"]["revision"].as_u64());
                *s.lock() = Some((shown, std::fs::read_to_string(&file).ok()));
            })),
        );
        let from = disk.calls.lock().len();
        graph.write_notes(token, revision, "kick", "hits", &kick_clips(&["C1", p])).unwrap();
        let t = graph.last_edit().unwrap();
        assert!(t.previewed && t.taken_back.is_none(), "{p}: {t:?}");
        let calls = disk.calls.lock()[from..].to_vec();
        let barriers: Vec<usize> = (0..calls.len()).filter(|&k| calls[k].1.starts_with("barrier ")).collect();
        assert_eq!(barriers.len(), 1, "{p}: one barrier per commit: {calls:#?}");
        let (at_ns, journal) = (calls[barriers[0]].0, calls[barriers[0]].1["barrier ".len()..].to_string());
        assert!(journal.contains("/.music2/tx/") && journal.ends_with(".json"), "{p}: {journal}");
        let placed = calls[..barriers[0]].iter().any(|(_, c)| c.starts_with("rename ") && c.contains(&format!(" {journal} ")));
        assert!(placed, "{p}: the journal is in place before its barrier: {calls:#?}");
        assert!(t.sent_ns <= at_ns, "{p}: the preview went out first: {t:?}, barrier at {at_ns}");
        let into = calls.iter().position(|(_, c)| c.starts_with("rename ") && c.contains("/data/kick/hits.json "));
        assert!(into.is_some_and(|k| k > barriers[0]), "{p}: renamed into the project after the barrier: {calls:#?}");
        assert_eq!(*seen.lock(), Some((Some(revision), Some(before))), "{p}: nothing accepted while it ran");
        let (_, now) = token_and_revision(&graph);
        assert_eq!(now, revision + 1, "{p}: accepted once durable");
        assert!(fixture.read("data/kick/hits.json").contains(&format!("\"{p}\"")), "{p}");
        revision = now;
    }
}

// PLANE-5, RENDER-26a, WRITE-21: a write the disk refuses (here `.music2/tx/` read-only) is heard,
// then taken back: its error says what wasn't saved and that it was taken back, the swap back
// follows its preview, and nothing was accepted: the revision, undo and the file are as they
// were, and the accepted data plays. An agent's Tx is answered `taken_back` too. Once the disk
// takes writes again, the next one is heard and saved.
#[test]
fn a_durable_failure_swaps_the_accepted_data_back() {
    let fixture = Fixture::new();
    let (graph, token, revision) = kick(&fixture, None);
    let first = generation(&graph);
    let file = fixture.read("data/kick/hits.json");
    let can_undo = graph.state().unwrap()["can_undo"].clone();
    let Some(tx) = refusing(&fixture) else { return };
    let e = graph.write_notes(token, revision, "kick", "hits", &kick_clips(&["C1", "D1"])).unwrap_err().to_string();
    assert!(e.starts_with("data/kick/hits.json couldn't be saved: Permission denied"), "{e}");
    assert!(e.ends_with(". It played for a moment and was taken back."), "{e}");
    let t = graph.last_edit().unwrap();
    let (seq, back) = (t.seq.unwrap(), t.taken_back.expect("swapped back"));
    assert!(t.previewed && back > seq, "{t:?}");
    assert!(applied_ns(&graph, seq) <= applied_ns(&graph, back), "taken back after it was heard");
    assert_eq!(token_and_revision(&graph).1, revision, "no revision");
    assert_eq!(graph.state().unwrap()["can_undo"], can_undo);
    assert_eq!(fixture.read("data/kick/hits.json"), file);
    assert!(plays_accepted(&graph) && generation(&graph) == first);
    assert_eq!(accepted_pitches(&graph), [24.0]);

    // An agent's Tx through the route: refused as a disk failure, said the same way.
    let lane = m2_song::LaneAddr::new("kick", "hits").unwrap();
    let put = json!({"id": "a", "start": 0, "length": 4, "notes": [{"pitch": "E1", "start": 0, "length": 0.25}]});
    let mut agent = m2_song::Tx::new(
        "tx-agent",
        ("before".into(), "after".into()),
        vec![Op::Clips { lane: lane.clone(), put: vec![put], delete: vec![] }],
        Expect::Confined(Confined { lanes: vec![lane.clone()], ..Confined::default() }),
    );
    agent.base.insert(TxPath::lane(&lane), Base::Sha(m2_song::Sha::of(file.as_bytes())));
    let (status, answer) = graph.tx_commit(Some(token), &agent.to_json(), None, &[]).unwrap();
    assert_eq!((status, &answer["io"]["taken_back"]), (500, &json!(true)), "{answer}");
    assert!(answer["io"]["message"].as_str().unwrap().ends_with("taken back."), "{answer}");
    assert!(plays_accepted(&graph) && fixture.read("data/kick/hits.json") == file);

    drop(tx);
    graph.write_notes(token, revision, "kick", "hits", &kick_clips(&["C1", "D1"])).unwrap();
    let t = graph.last_edit().unwrap();
    assert!(t.previewed && t.taken_back.is_none(), "{t:?}");
    assert_eq!((token_and_revision(&graph).1, accepted_pitches(&graph)), (revision + 1, vec![24.0, 26.0]));
    assert!(plays_accepted(&graph));
}

// PLANE-5, WRITE-19, R9: another program replaces the lane file between the commit's pre-check
// and its swap: the swap's read-back catches it, the commit rolls back, and the preview is taken
// back. The outside version is the file's, and plays once the watcher takes it in.
#[test]
fn a_preview_whose_file_was_replaced_at_its_rename_is_taken_back_and_the_outside_version_plays() {
    let fixture = Fixture::new();
    let disk = Faulty::default();
    let (graph, token, revision) = kick(&fixture, Some(disk.clone()));
    let outside = r#"{"clips": [{"id": "z", "start": 0, "length": 4, "notes": [{"pitch": "G1", "start": 0, "length": 1}]}]}"#;
    let file = fixture.0.join("data/kick/hits.json");
    disk.at(
        "hits.json Swap",
        Act::Before(Box::new(move || {
            let tmp = file.with_extension("tmp");
            std::fs::write(&tmp, outside).unwrap();
            std::fs::rename(&tmp, &file).unwrap();
        })),
    );
    let e = graph.write_notes(token, revision, "kick", "hits", &kick_clips(&["C1", "D1"])).unwrap_err().to_string();
    assert!(e.contains("changed since the edit was planned") && e.ends_with("taken back."), "{e}");
    assert!(graph.last_edit().unwrap().taken_back.is_some());
    assert_eq!((fixture.read("data/kick/hits.json"), accepted_pitches(&graph)), (outside.to_string(), vec![24.0]));
    assert!(plays_accepted(&graph));
    assert!(graph.poll(), "the outside version is taken in");
    assert_eq!(accepted_pitches(&graph), [31.0]);
    assert!(plays_accepted(&graph));
}

// WRITE-21, PLANE-5: an undo sent right behind a lane write waits for it (one request at a
// time), undoes it, and is heard as its own preview.
#[test]
fn an_undo_right_after_a_lane_write_waits_for_it_and_is_previewed() {
    let fixture = Fixture::new();
    let (graph, token, revision) = kick(&fixture, None);
    let graph = Arc::new(graph);
    let held = graph.hold();
    let (g, w) = (
        graph.clone(),
        std::thread::spawn({
            let g = graph.clone();
            move || g.write_notes(token, revision, "kick", "hits", &kick_clips(&["C1", "D1"]))
        }),
    );
    // Queued behind the write.
    std::thread::sleep(Duration::from_millis(50));
    let u = std::thread::spawn(move || g.undo(token));
    std::thread::sleep(Duration::from_millis(50));
    drop(held);
    w.join().unwrap().unwrap();
    let undone = u.join().unwrap().unwrap();
    assert_eq!(undone["revision"].as_u64(), Some(revision + 2), "{undone}");
    let t = graph.last_edit().unwrap();
    assert!(t.previewed && t.taken_back.is_none(), "the undo was heard as its preview: {t:?}");
    assert_eq!(accepted_pitches(&graph), [24.0]);
    assert!(plays_accepted(&graph));
}

// PLANE-5: two lane writes sent at once, each changing its own clip, are served one after the
// other, each previewed on what the one before accepted: both are on disk, and what plays is the
// accepted revision holding both, in the same renderer.
#[test]
fn two_lane_writes_at_once_are_heard_in_order_each_once() {
    let fixture = Fixture::new();
    let (graph, token, revision) = kick(&fixture, None);
    let two = |a: &str, b: &str| -> Vec<NoteClip> {
        let clip = |id: &str, start: u32, pitch: &str| json!({"id": id, "start": start, "length": 4, "notes": [{"pitch": pitch, "start": 0, "length": 0.25}]});
        serde_json::from_value(json!([clip("a", 0, a), clip("b", 4, b)])).unwrap()
    };
    graph.write_notes(token, revision, "kick", "hits", &two("C1", "C1")).unwrap();
    let first = generation(&graph);
    let revision = revision + 1;
    let graph = Arc::new(graph);
    let held = graph.hold();
    let writes: Vec<_> = [two("D1", "C1"), two("C1", "E1")]
        .into_iter()
        .map(|clips| {
            let g = graph.clone();
            let w = std::thread::spawn(move || g.write_notes(token, revision, "kick", "hits", &clips));
            std::thread::sleep(Duration::from_millis(30));
            w
        })
        .collect();
    drop(held);
    for w in writes {
        w.join().unwrap().unwrap();
    }
    assert_eq!(token_and_revision(&graph).1, revision + 2);
    assert_eq!(accepted_pitches(&graph), [26.0, 28.0], "both, each merged on the one before");
    assert!(graph.last_edit().unwrap().previewed);
    assert!(plays_accepted(&graph) && generation(&graph) == first);
}

// PLANE-5, RENDER-25a: a code save sent while a lane write is being made durable waits for it
// (one request at a time), then builds a renderer from the accepted revision, which holds the
// write: what plays ends with both.
#[test]
fn a_code_save_behind_a_previewed_write_plays_both() {
    let fixture = Fixture::new();
    let (graph, token, revision) = kick(&fixture, None);
    let first = generation(&graph);
    let graph = Arc::new(graph);
    let held = graph.hold();
    let write = {
        let g = graph.clone();
        std::thread::spawn(move || g.write_notes(token, revision, "kick", "hits", &kick_clips(&["C1", "D1"])))
    };
    std::thread::sleep(Duration::from_millis(50));
    let song = fixture.read("song.ts");
    let structural = song.replace("export default", "const spare = track('spare', () => osc.sine(hz(3)));\nexport default");
    let save = {
        let (g, song) = (graph.clone(), song.clone());
        std::thread::spawn(move || g.write_file(Some(token), "song.ts", &song, &structural))
    };
    std::thread::sleep(Duration::from_millis(50));
    drop(held);
    write.join().unwrap().unwrap();
    save.join().unwrap().unwrap();
    assert_ne!(generation(&graph), first, "the save built a renderer");
    assert!(plays_accepted(&graph));
    assert_eq!(accepted_pitches(&graph), [24.0, 26.0], "holding the write");
    assert!(fixture.read("song.ts").contains("spare"));
}

// WRITE-22, WRITE-13, PLANE-5: a knob drag goes on through a lane write heard as its preview: the
// write changes no code, so the lease keeps its renderer, and its commit writes the value.
#[test]
fn a_drag_goes_on_through_a_previewed_lane_write() {
    let fixture = Fixture::new();
    let (graph, token, revision) = kick(&fixture, None);
    let first = generation(&graph);
    let (_, _, id) = knob(&graph, 0.3);
    graph.preview(token, revision, &id, "drag", 0.5).unwrap();
    graph.write_notes(token, revision, "kick", "hits", &kick_clips(&["C1", "D1"])).unwrap();
    assert!(graph.last_edit().unwrap().previewed);
    let leases = graph.leases();
    assert_eq!((leases.len(), leases[0].generation), (1, Some(first)), "the drag goes on, aimed at the same renderer");
    graph.preview(token, revision, &id, "drag", 0.6).unwrap();
    graph.set(token, revision, &id, "drag", 0.6, Some(0.3)).unwrap();
    assert!(fixture.read("song.ts").contains("secs(0.6)") && synced(&graph));
    assert_eq!(accepted_pitches(&graph), [24.0, 26.0]);
}

// WRITE-17, PLANE-5: a project another program holds is refused before any check: nothing is
// previewed, nothing sent.
#[test]
fn a_read_only_project_sends_no_preview() {
    let fixture = Fixture::new();
    fixture.write(KICK);
    let held = m2_session::lock::acquire(&fixture.0, "m2", m2_session::lock::RETRY).unwrap();
    let graph = fixture.open();
    let (token, revision) = token_and_revision(&graph);
    assert!(graph.write_notes(token, revision, "kick", "hits", &kick_clips(&["C1"])).is_err());
    let t = graph.last_edit().unwrap();
    assert!(!t.previewed && t.seq.is_none() && t.taken_back.is_none(), "{t:?}");
    drop(held);
}

// RENDER-8, PLANE-5: a player that is replaced while a previewed commit is being made durable
// (another thread let a stalled one go) heard the preview; the settle after the commit loads the
// accepted revision into the new one.
#[test]
fn a_player_replaced_during_a_commit_plays_the_accepted_revision() {
    let fixture = Fixture::new();
    let disk = Faulty::default();
    let (graph, token, revision) = kick(&fixture, Some(disk.clone()));
    let inner = graph.inner.clone();
    disk.at(
        "barrier",
        Act::Before(Box::new(move || {
            let player = Player::silent().unwrap();
            *inner.player.lock() = Some(Device { player, epoch: 99, resume: 0.0 });
        })),
    );
    graph.write_notes(token, revision, "kick", "hits", &kick_clips(&["C1", "D1"])).unwrap();
    assert!(graph.last_edit().unwrap().previewed, "heard in the player before");
    assert_eq!(graph.inspect(|a| a.open.as_ref().unwrap().plays.as_ref().unwrap().epoch), 99);
    assert!(plays_accepted(&graph));
    assert_eq!(accepted_pitches(&graph), [24.0, 26.0]);
}

/// A one-second render of the accepted revision, as Export renders it (RENDER-3): its bytes.
fn exported(graph: &Graph, fixture: &Fixture, name: &str) -> Vec<u8> {
    let (_, _, render, _) = graph.render(m2_editor::render::Length::Seconds(1.0)).unwrap();
    let out = fixture.0.join(format!("{name}.wav"));
    render.write(&out, |_| {}).unwrap();
    let bytes = std::fs::read(&out).unwrap();
    std::fs::remove_file(&out).unwrap();
    bytes
}

// RENDER-3, PLANE-5: an export after a write that was heard and taken back renders the accepted
// data: the same bytes as before the write.
#[test]
fn an_export_after_a_taken_back_write_renders_the_accepted_data() {
    let fixture = Fixture::new();
    let (graph, token, revision) = kick(&fixture, None);
    let before = exported(&graph, &fixture, "before");
    let Some(tx) = refusing(&fixture) else { return };
    assert!(graph.write_notes(token, revision, "kick", "hits", &kick_clips(&["C1", "C1", "C1", "C1"])).is_err());
    assert!(graph.last_edit().unwrap().taken_back.is_some());
    assert!(exported(&graph, &fixture, "after") == before, "the accepted data, not the preview's");
    drop(tx);
}

// RENDER-26, PLANE-5: a lane that needs more room than its scheduler has isn't previewed: it is
// heard once its commit is accepted, through a renderer sized for it.
#[test]
fn a_lane_that_outgrows_its_scheduler_is_not_previewed() {
    let fixture = Fixture::new();
    let (graph, token, revision) = kick(&fixture, None);
    let first = generation(&graph);
    let notes: Vec<Value> = (0..300).map(|i| json!({"pitch": 24 + i % 64, "start": 0, "length": 4})).collect();
    let clips: Vec<NoteClip> = serde_json::from_value(json!([{"id": "a", "start": 0, "length": 4, "notes": notes}])).unwrap();
    graph.write_notes(token, revision, "kick", "hits", &clips).unwrap();
    let t = graph.last_edit().unwrap();
    assert!(!t.previewed && t.ready_ns == t.compiled_ns && t.compiled_ns <= t.sent_ns, "{t:?}");
    assert_ne!(generation(&graph), first, "a renderer sized for it");
    assert!(plays_accepted(&graph));
}

// PLANE-5, RENDER-8: a bug that panics inside a commit after its preview went out leaves the
// engine on the accepted revision: what plays is swapped back in the same renderer, and the
// transport plays on, not reloaded from the start.
#[test]
fn a_panic_in_a_commit_after_its_preview_plays_the_accepted_revision() {
    let fixture = Fixture::new();
    let disk = Faulty::default();
    let (graph, token, revision) = kick(&fixture, Some(disk.clone()));
    graph.seek(8.0).unwrap();
    let first = generation(&graph);
    disk.at("barrier", Act::Panic);
    let e = graph.write_notes(token, revision, "kick", "hits", &kick_clips(&["C1", "D1"])).unwrap_err();
    assert!(e.to_string().contains("hit a bug"), "{e:#}");
    assert!(plays_accepted(&graph) && generation(&graph) == first, "swapped back, not reloaded");
    assert_eq!(accepted_pitches(&graph), [24.0]);
    let at = graph.status().beat;
    assert!(at >= 8.0, "the playhead went on from where it was: {at}");
    let p = player(&graph);
    eventually("it plays on", || p.status().beats > at);
}

// PLANE-5: a failed commit that changed no lane anything plays (a data file no lane reads) heard
// nothing, and its error says nothing of being taken back.
#[test]
fn a_failed_write_nothing_plays_says_nothing_of_a_preview() {
    let fixture = Fixture::new();
    let (graph, token, _) = kick(&fixture, None);
    let Some(tx) = refusing(&fixture) else { return };
    let path = TxPath::new("data/kick/spare.json").unwrap();
    let mut agent = m2_song::Tx::new(
        "tx-agent",
        ("before".into(), "after".into()),
        vec![Op::Write { file: path.clone(), bytes: m2_song::tx::Payload::Text("{\"clips\": []}\n".into()) }],
        Expect::Confined(Confined::default()),
    );
    agent.base.insert(path, Base::Absent);
    let (status, answer) = graph.tx_commit(Some(token), &agent.to_json(), None, &[]).unwrap();
    assert_eq!((status, answer["io"].get("taken_back")), (500, None), "{answer}");
    assert!(!answer["io"]["message"].as_str().unwrap().contains("taken back"), "{answer}");
    drop(tx);
    assert!(plays_accepted(&graph));
}

// PLANE-5: a commit heard as its preview that the disk is slow to make durable (2 s here, a disk
// that stopped answering) is said while it waits: it plays, but isn't saved yet; and once it is
// saved, that is said too. The watcher's tick doesn't wait behind the session meanwhile.
#[test]
fn a_save_the_disk_is_slow_to_make_is_said() {
    let fixture = Fixture::new();
    let disk = Faulty::default();
    let (graph, token, revision) = kick(&fixture, Some(disk.clone()));
    disk.at("barrier", Act::Sleep(Duration::from_millis(2600)));
    let graph = Arc::new(graph);
    let write = {
        let g = graph.clone();
        std::thread::spawn(move || g.write_notes(token, revision, "kick", "hits", &kick_clips(&["C1", "D1"])))
    };
    let t = Instant::now();
    let mut said = vec![];
    while said.is_empty() && t.elapsed() < Duration::from_secs(5) {
        assert!(!graph.catch_up(), "the watcher's catch-up doesn't wait for the session");
        said = graph.expire();
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(said.iter().any(|s| s.contains("isn't saved yet")) && !write.is_finished(), "said while it waits: {said:?}");
    assert!(t.elapsed() >= Duration::from_millis(1900), "not before 2 s: {:?}", t.elapsed());
    write.join().unwrap().unwrap();
    let after = graph.expire();
    assert!(after.iter().any(|s| s.starts_with("The last edit is saved now")), "{after:?}");
    assert!(graph.expire().is_empty(), "said once");
}

/// An agent's Tx putting `pitch` alone in the kick's clip, planned on its lane file as it is now.
fn agent_tx(fixture: &Fixture, pitch: &str) -> Value {
    let lane = m2_song::LaneAddr::new("kick", "hits").unwrap();
    let put = json!({"id": "a", "start": 0, "length": 4, "notes": [{"pitch": pitch, "start": 0, "length": 0.25}]});
    let mut tx = m2_song::Tx::new(
        "tx-agent",
        ("before".into(), "after".into()),
        vec![Op::Clips { lane: lane.clone(), put: vec![put], delete: vec![] }],
        Expect::Confined(Confined { lanes: vec![lane.clone()], ..Confined::default() }),
    );
    tx.base.insert(TxPath::lane(&lane), Base::Sha(m2_song::Sha::of(fixture.read("data/kick/hits.json").as_bytes())));
    tx.to_json()
}

/// What `expire` says once it says something, polled as the server does, while `busy` runs:
/// empty after 5 s.
fn said_while<T>(graph: &Graph, busy: &std::thread::JoinHandle<T>) -> Vec<String> {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(5) {
        let said = graph.expire();
        if !said.is_empty() {
            assert!(!busy.is_finished(), "said while it waits: {said:?}");
            return said;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    vec![]
}

/// The toasts the server sent since the last call.
fn toasts(events: &mut tokio::sync::broadcast::Receiver<String>) -> Vec<String> {
    std::iter::from_fn(|| events.try_recv().ok())
        .filter_map(|e| serde_json::from_str::<Value>(&e).ok())
        .filter(|e| e["t"] == "toast")
        .map(|e| e["text"].as_str().unwrap_or_default().to_string())
        .collect()
}

// PLANE-5, WRITE-19: through the server's own threads (`spawn_graph_watch`), a heard write the
// disk is slow to make durable is said while it waits, and its save once it is, though an outside
// save settled meanwhile and the watcher's tick waits for the session to take it in.
#[test]
fn the_server_says_a_slow_save_while_an_outside_save_waits_behind_it() {
    let fixture = Fixture::new();
    let disk = Faulty::default();
    let (graph, token, revision) = kick(&fixture, Some(disk.clone()));
    let state = Arc::new(crate::AppState::new(String::new(), graph));
    let (_, mut events, _page) = state.subscribe(true);
    crate::spawn_graph_watch(&state);
    disk.at("barrier", Act::Sleep(Duration::from_millis(3000)));
    let from = disk.calls.lock().len();
    let write = {
        let s = state.clone();
        std::thread::spawn(move || s.graph.write_notes(token, revision, "kick", "hits", &kick_clips(&["C1", "D1"])))
    };
    eventually("the commit's barrier runs", || disk.calls.lock()[from..].iter().any(|(_, c)| c.starts_with("barrier ")));
    // An editor saves song.ts: its batch settles, and the tick hands it to the session.
    let song = fixture.read("song.ts");
    fixture.put("song.ts", &format!("{song}// saved outside\n"));
    let settling = || state.graph.inner.watch.lock().as_ref().is_some_and(|w| w.settle.settling());
    eventually("its batch opens", settling);
    eventually("and settles", || !settling());
    assert!(!write.is_finished(), "settled while the save waits for the disk");
    let (t, mut said) = (Instant::now(), vec![]);
    while said.is_empty() && t.elapsed() < Duration::from_secs(5) {
        said = toasts(&mut events);
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!write.is_finished(), "said while it waits: {said:?}");
    assert_eq!(said, ["Saving is taking a while: the disk hasn't answered for 2 s. The last edit plays, but isn't saved yet."]);
    write.join().unwrap().unwrap();
    let (t, mut said) = (Instant::now(), vec![]);
    while said.is_empty() && t.elapsed() < Duration::from_secs(5) {
        said = toasts(&mut events);
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(said.len() == 1 && said[0].starts_with("The last edit is saved now: the disk took 3."), "{said:?}");
    eventually("the outside save is taken in", || token_and_revision(&state.graph).1 == revision + 2);
    assert!(state.graph.last_taken_in().is_some());
}

// PLANE-5: an agent's Tx heard as its preview, then taken back, is said in the app too, not only
// in the agent's answer: the listener heard it go. On a slow disk, the app told it isn't saved yet
// is told how it ended. The app's own edit is said to every window in its error's words (which
// the window that sent it shows once).
#[test]
fn an_agents_write_heard_and_taken_back_is_said_in_the_app() {
    let fixture = Fixture::new();
    let disk = Faulty::default();
    let (graph, token, revision) = kick(&fixture, Some(disk.clone()));
    let graph = Arc::new(graph);
    graph.expire();
    disk.at("hits.json Swap", Act::Fail);
    let (status, answer) = graph.tx_commit(Some(token), &agent_tx(&fixture, "E1"), None, &[]).unwrap();
    let said = answer["io"]["message"].as_str().unwrap_or_default().to_string();
    assert!(status == 500 && answer["io"]["taken_back"] == json!(true), "{answer}");
    assert!(said.starts_with("data/kick/hits.json couldn't be saved: "), "{said}");
    assert!(said.ends_with("It played for a moment and was taken back."), "{said}");
    assert_eq!(graph.expire(), [format!("An agent's edit: {said}")]);

    // Slow, then refused.
    disk.at("barrier", Act::Sleep(Duration::from_millis(2600)));
    disk.at("hits.json Swap", Act::Fail);
    let agent = {
        let (g, tx) = (graph.clone(), agent_tx(&fixture, "F1"));
        std::thread::spawn(move || g.tx_commit(Some(token), &tx, None, &[]))
    };
    let said = said_while(&graph, &agent);
    assert_eq!(said, ["Saving is taking a while: the disk hasn't answered for 2 s. The last edit plays, but isn't saved yet."]);
    let (status, answer) = agent.join().unwrap().unwrap();
    assert_eq!((status, &answer["io"]["taken_back"]), (500, &json!(true)), "{answer}");
    assert_eq!(graph.expire(), [format!("An agent's edit: {}", answer["io"]["message"].as_str().unwrap())]);

    // The app's own edit.
    disk.at("hits.json Swap", Act::Fail);
    let e = graph.write_notes(token, revision, "kick", "hits", &kick_clips(&["C1", "G1"])).unwrap_err().to_string();
    assert!(e.ends_with("taken back."), "{e}");
    assert_eq!(graph.expire(), [e]);
    assert!(plays_accepted(&graph));
    assert_eq!(accepted_pitches(&graph), [24.0]);
}

// PLANE-5: with the transport stopped nothing is heard, and no message says it was: a refused
// write's error says what wasn't saved, a slow save's message that it isn't saved yet, and an
// agent's refused Tx is answered `taken_back` (the engine was swapped back) and not said in the
// app.
#[test]
fn with_the_transport_stopped_no_message_says_the_edit_played() {
    let fixture = Fixture::new();
    let disk = Faulty::default();
    let (graph, token, revision) = kick(&fixture, Some(disk.clone()));
    graph.transport("stop").unwrap();
    eventually("it stops", || !graph.status().playing);
    graph.expire();
    disk.at("hits.json Swap", Act::Fail);
    let e = graph.write_notes(token, revision, "kick", "hits", &kick_clips(&["C1", "D1"])).unwrap_err().to_string();
    let t = graph.last_edit().unwrap();
    assert!(t.previewed && t.taken_back.is_some(), "{t:?}");
    assert_eq!(e, "data/kick/hits.json couldn't be saved: injected");
    disk.at("hits.json Swap", Act::Fail);
    let (status, answer) = graph.tx_commit(Some(token), &agent_tx(&fixture, "E1"), None, &[]).unwrap();
    assert_eq!((status, &answer["io"]["taken_back"]), (500, &json!(true)), "{answer}");
    assert_eq!(answer["io"]["message"], json!("data/kick/hits.json couldn't be saved: injected"));
    assert!(graph.expire().is_empty(), "nothing heard, nothing said in the app");
    assert!(plays_accepted(&graph));

    let graph = Arc::new(graph);
    disk.at("barrier", Act::Sleep(Duration::from_millis(2600)));
    let write = {
        let g = graph.clone();
        std::thread::spawn(move || g.write_notes(token, revision, "kick", "hits", &kick_clips(&["C1", "D1"])))
    };
    let said = said_while(&graph, &write);
    assert_eq!(said, ["Saving is taking a while: the disk hasn't answered for 2 s. The last edit isn't saved yet."]);
    write.join().unwrap().unwrap();
    let after = graph.expire();
    assert!(after.len() == 1 && after[0].starts_with("The last edit is saved now"), "{after:?}");
}

// PLANE-5, 6a.6: a commit whose rollback fails too leaves its journal, and part of it on disk: its
// error says so and points to Needs attention, not that it was taken back, and an agent's refusal
// isn't `taken_back`; the engine plays the accepted revision until the files are taken in.
#[test]
fn an_interrupted_commit_after_its_preview_points_to_needs_attention() {
    let fixture = Fixture::new();
    let disk = Faulty::default();
    let (graph, token, _) = kick(&fixture, Some(disk.clone()));
    let lane = m2_song::LaneAddr::new("kick", "hits").unwrap();
    let file = fixture.read("data/kick/hits.json");
    let put = json!({"id": "a", "start": 0, "length": 4, "notes": [{"pitch": "E1", "start": 0, "length": 0.25}]});
    let spare = TxPath::new("data/kick/spare.json").unwrap();
    let mut agent = m2_song::Tx::new(
        "tx-agent",
        ("before".into(), "after".into()),
        vec![
            Op::Clips { lane: lane.clone(), put: vec![put], delete: vec![] },
            Op::Write { file: spare.clone(), bytes: m2_song::tx::Payload::Text("{\"clips\": []}\n".into()) },
        ],
        Expect::Confined(Confined { lanes: vec![lane.clone()], ..Confined::default() }),
    );
    agent.base.insert(TxPath::lane(&lane), Base::Sha(m2_song::Sha::of(file.as_bytes())));
    agent.base.insert(spare, Base::Absent);
    // The second rename fails, then the rollback's swap of the lane file back.
    disk.at("spare.json Excl", Act::FailThen("hits.json Swap".into()));
    let (status, answer) = graph.tx_commit(Some(token), &agent.to_json(), None, &[]).unwrap();
    let said = answer["io"]["message"].as_str().unwrap_or_default();
    assert_eq!(status, 500, "{answer}");
    assert!(said.contains("couldn't be undone") && said.ends_with("Needs attention says how to finish or undo it."), "{said}");
    assert!(answer["io"].get("taken_back").is_none(), "{answer}");
    let problems = graph.state().unwrap()["problems"].clone();
    assert!(problems.as_array().unwrap().iter().any(|p| p["code"] == "tx.interrupted"), "{problems}");
    assert!(plays_accepted(&graph));
    assert_eq!(accepted_pitches(&graph), [24.0]);
}
