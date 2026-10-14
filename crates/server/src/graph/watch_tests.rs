//! The watcher with real file system events (WRITE-19): what an idle tick does, the session's own
//! writes, outside writes events name as one folder, a case-only rename, the watch lock never held
//! over a walk, and events that miss changes giving way to walks.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use super::tests::{Fixture, KICK, hits_of, kick_clips, revs, watching};
use super::*;

/// What the accepted revision's snapshot has at `path`: its text, or its binary as stored.
fn accepted_at(graph: &Graph, path: &str) -> (Option<String>, Option<m2_song::Stored>) {
    let p = PathBuf::from(path);
    graph.inspect(move |a| {
        let s = &a.open.as_ref().unwrap().session.accepted().snapshot;
        (s.files.get(&p).map(|t| t.to_string()), s.binaries.get(&p).cloned())
    })
}

fn stats(graph: &Graph) -> WatchStats {
    graph.watch_stats().expect("a project is open")
}

/// Tick until the first walk of everything is done.
fn walked_once(graph: &Graph) {
    watching(graph, 2000, |g| stats(g).walks > 0);
    assert!(stats(graph).walks > 0, "{:?}", stats(graph));
}

// WRITE-19, the step 6 design's 24: with events, a project nobody writes to is watched with no stat
// at all between the walks of everything, which come every 5 s.
#[test]
fn an_idle_project_is_watched_without_a_stat() {
    let fixture = Fixture::new();
    fixture.write(KICK);
    let graph = fixture.open();
    assert!(stats(&graph).events, "a temp folder is on a local volume");
    walked_once(&graph);
    let before = stats(&graph);
    for _ in 0..20 {
        graph.watch();
        std::thread::sleep(Duration::from_millis(20));
    }
    let after = stats(&graph);
    assert_eq!((after.walks, after.scoped, after.idle), (before.walks, before.scoped, before.idle + 20), "{after:?}");
}

// R12, WRITE-19: the session's own commits make no batch with events either: their events name the
// files, which then read as the session wrote them.
#[test]
fn own_commits_make_no_batch_with_events() {
    let fixture = Fixture::new();
    fixture.write(KICK);
    let graph = fixture.open();
    walked_once(&graph);
    let token = graph.state().unwrap()["session"].as_u64().unwrap();
    for (i, pitches) in [["C1", "D1"], ["E1", "F1"], ["G1", "A1"]].iter().enumerate() {
        let revision = revs(&graph).0;
        graph.write_notes(token, revision, "kick", "hits", &kick_clips(pitches)).unwrap();
        watching(&graph, 100 + 50 * i as u64, |_| false);
    }
    let mine = revs(&graph);
    watching(&graph, 600, |_| false);
    assert_eq!(revs(&graph), mine, "nothing more to take in");
    let s = stats(&graph);
    assert!(s.events && s.scoped > 0, "the commits' files were looked at: {s:?}");
    assert!(!graph.inner.watch.lock().as_ref().unwrap().settle.settling(), "no batch is open");
}

// R12, WRITE-19: an outside program that replaces a file right after the session wrote it, before
// the actor told the watcher, is still taken in: the tick that saw the replace opened a batch the
// session's own write then closed, and the next tick looks at the file again, named or not.
#[test]
fn an_outside_replace_right_after_an_own_write_is_taken_in() {
    let fixture = Fixture::new();
    fixture.write(KICK);
    fixture.put("data/kick/hits.json", &hits_of(4));
    let graph = fixture.open();
    walked_once(&graph);
    let (dir, theirs) = (fixture.0.clone(), hits_of(6));
    let replace = theirs.clone();
    graph.inspect(move |a| {
        let mut once = true;
        a.between_phases = Some(Box::new(move |inner: &Inner| {
            if std::mem::take(&mut once) {
                // A formatter reacting to the commit: a new file renamed over the session's.
                std::fs::write(dir.join(".fmt.tmp"), &replace).unwrap();
                std::fs::rename(dir.join(".fmt.tmp"), dir.join("data/kick/hits.json")).unwrap();
                std::thread::sleep(Duration::from_millis(100));
                assert!(inner.tick().is_none(), "the replace opens a batch");
                assert!(inner.watch.lock().as_ref().unwrap().settle.settling());
            }
        }));
    });
    let token = graph.state().unwrap()["session"].as_u64().unwrap();
    graph.write_notes(token, revs(&graph).0, "kick", "hits", &kick_clips(&["C1", "D1"])).unwrap();
    graph.inspect(|a| a.between_phases = None);
    // Well before the 5 s walk of everything.
    watching(&graph, 1500, |g| accepted_at(g, "data/kick/hits.json").0.as_deref() == Some(theirs.as_str()));
    assert_eq!(accepted_at(&graph, "data/kick/hits.json").0.as_deref(), Some(theirs.as_str()), "the outside version is taken in");
}

// WRITE-19: a folder moved into the project, or one replaced by a rename, is one event for the
// folder, and is walked: its files are taken in, well before the 5 s walk of everything would.
#[test]
fn a_folder_moved_into_the_project_is_taken_in() {
    let fixture = Fixture::new();
    fixture.write(KICK);
    fixture.put("data/kick/hits.json", &hits_of(4));
    let graph = fixture.open();
    walked_once(&graph);
    let outside = fixture.0.with_extension("outside");
    std::fs::create_dir_all(outside.join("kick")).unwrap();
    std::fs::create_dir_all(outside.join("tracks")).unwrap();
    std::fs::write(outside.join("kick/hits.json"), hits_of(8)).unwrap();
    std::fs::write(outside.join("tracks/extra.ts"), "export const extra = 1;\n").unwrap();
    // The lane's folder replaced by a rename, and a code folder moved in.
    std::fs::rename(fixture.0.join("data/kick"), outside.join("kick-old")).unwrap();
    std::fs::rename(outside.join("kick"), fixture.0.join("data/kick")).unwrap();
    std::fs::rename(outside.join("tracks"), fixture.0.join("tracks")).unwrap();
    let taken = |g: &Graph| {
        accepted_at(g, "data/kick/hits.json").0.as_deref() == Some(hits_of(8).as_str()) && accepted_at(g, "tracks/extra.ts").0.is_some()
    };
    watching(&graph, 1500, taken);
    assert!(taken(&graph), "{:?}", (accepted_at(&graph, "data/kick/hits.json"), accepted_at(&graph, "tracks/extra.ts")));
    std::fs::remove_dir_all(&outside).ok();
}

// WRITE-19: `assets/` replaced by a rename (a sample folder swapped in Finder) is taken in from its
// one event: the asset a lane plays has its new bytes' hash, and a file the new folder lacks is gone.
#[test]
fn an_assets_folder_replaced_by_a_rename_is_taken_in() {
    let fixture = Fixture::new();
    let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/resample");
    for rel in
        ["music2.toml", "song.ts", "tracks/kick.ts", "tracks/keys.ts", "tracks/chops.ts", "data/arrangement.json", "data/kick/hits.json"]
            .into_iter()
            .chain(["data/chops/clips.json", "data/keys/chords.json", "assets/keys-bounce.wav", "assets/keys-bounce.wav.json"])
    {
        let to = fixture.0.join(rel);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(from.join(rel), to).unwrap();
    }
    let graph = fixture.open();
    walked_once(&graph);
    let bytes = std::fs::read(fixture.0.join("assets/keys-bounce.wav")).unwrap();
    let shorter = &bytes[..bytes.len() / 2];
    let outside = fixture.0.with_extension("outside");
    std::fs::create_dir_all(outside.join("assets")).unwrap();
    std::fs::write(outside.join("assets/keys-bounce.wav"), shorter).unwrap();
    std::fs::rename(fixture.0.join("assets"), outside.join("assets-old")).unwrap();
    std::fs::rename(outside.join("assets"), fixture.0.join("assets")).unwrap();
    let want = Some(m2_song::Stored::Hash(m2_song::Sha::of(shorter)));
    let taken = |g: &Graph| accepted_at(g, "assets/keys-bounce.wav").1 == want && accepted_at(g, "assets/keys-bounce.wav.json").1.is_none();
    watching(&graph, 1500, taken);
    assert!(taken(&graph), "{:?}", accepted_at(&graph, "assets/keys-bounce.wav"));
    std::fs::remove_dir_all(&outside).ok();
}

// WRITE-19, WRITE-3a: a rename that changes only capitals settles as one path, the disk's spelling:
// the old name, which the disk still opens, is never stat'd into a second key.
#[test]
fn a_case_only_rename_settles_as_one_path() {
    let fixture = Fixture::new();
    fixture.write(KICK);
    fixture.put("tracks/Extra.ts", "export const extra = 1;\n");
    let graph = fixture.open();
    walked_once(&graph);
    std::fs::rename(fixture.0.join("tracks/Extra.ts"), fixture.0.join("tracks/extra.ts")).unwrap();
    let taken = |g: &Graph| accepted_at(g, "tracks/extra.ts").0.is_some();
    watching(&graph, 1500, taken);
    assert!(taken(&graph));
    assert_eq!(accepted_at(&graph, "tracks/Extra.ts").0, None, "one key, spelled as the disk spells it");
}

// The step 6 design's C.6: a tick never holds the watch lock over a stat: while a walk of everything
// is held up, the actor still tells the watcher what a commit wrote, and serves the next request.
#[test]
fn the_watch_lock_is_never_held_over_a_stat() {
    let fixture = Fixture::new();
    fixture.write(KICK);
    let graph = Arc::new(Graph::silent());
    graph.watch_by_walking(true);
    graph.open(&fixture.0).unwrap();
    assert!(!stats(&graph).events);
    let (entered, held) = (std::sync::mpsc::channel::<()>(), std::sync::mpsc::channel::<()>());
    let (tell, wait) = (entered.0, std::sync::Mutex::new(held.1));
    graph.inner.watch.lock().as_mut().unwrap().looking = Some(Arc::new(move || {
        let _ = tell.send(());
        let _ = wait.lock().unwrap().recv_timeout(Duration::from_secs(10));
    }));
    let g = graph.clone();
    let ticking = std::thread::spawn(move || g.watch());
    entered.1.recv_timeout(Duration::from_secs(5)).expect("the tick looks");
    let (done, finished) = std::sync::mpsc::channel();
    let g = graph.clone();
    std::thread::spawn(move || {
        let token = g.state().unwrap()["session"].as_u64().unwrap();
        g.write_notes(token, revs(&g).0, "kick", "hits", &kick_clips(&["C1"])).unwrap();
        // Served after the commit's job, and its telling the watcher what it wrote.
        g.inspect(|_| ());
        let _ = done.send(());
    });
    let served = finished.recv_timeout(Duration::from_secs(5));
    let _ = held.0.send(());
    ticking.join().unwrap();
    assert!(served.is_ok(), "the actor waited for the tick's walk");
    graph.inner.watch.lock().as_mut().unwrap().looking = None;
}

// WRITE-19: on a volume whose events miss changes, each is still taken in by the walk of
// everything; once two were missed within 10 minutes, the watcher walks every tick of a batch and
// as often as its walks allow while idle, until the project is opened again.
#[test]
fn events_that_miss_changes_give_way_to_walks() {
    let fixture = Fixture::new();
    fixture.write(KICK);
    fixture.put("data/kick/hits.json", &hits_of(4));
    let graph = fixture.open();
    walked_once(&graph);
    {
        let mut w = graph.inner.watch.lock();
        let w = w.as_mut().unwrap();
        (w.deaf, w.backstop, w.grace, w.next_walk) =
            (true, Duration::from_millis(100), Duration::from_millis(300), std::time::Instant::now());
    }
    for n in [5, 6, 7] {
        let events = stats(&graph).events;
        fixture.put("data/kick/hits.json", &hits_of(n));
        let taken = |g: &Graph| accepted_at(g, "data/kick/hits.json").0 == Some(hits_of(n));
        watching(&graph, 2000, taken);
        assert!(taken(&graph), "{n} notes taken in");
        // Past the grace, the change no event named is a miss.
        watching(&graph, 500, |_| false);
        let s = stats(&graph);
        if n == 5 {
            assert!(s.events && s.misses == 1, "{s:?}");
        } else {
            assert!(!s.events && s.misses == 2, "{s:?}");
        }
        let _ = events;
    }
}

// WRITE-19: the server's watcher thread waits for its next tick no longer than it must: an event
// naming a change wakes it at once while no batch is open, and the open batch wakes it when its
// 150 ms of quiet end, when the poll that sees everything takes it in.
#[test]
fn an_event_wakes_an_idle_watcher_and_a_batch_wakes_it_when_due() {
    let fixture = Fixture::new();
    fixture.write(KICK);
    fixture.put("data/kick/hits.json", &hits_of(4));
    let graph = fixture.open();
    walked_once(&graph);
    // Nothing pending: the wait lasts.
    let t = std::time::Instant::now();
    graph.watch_wait(Duration::from_millis(300));
    assert!(t.elapsed() >= Duration::from_millis(290), "{:?}", t.elapsed());
    let number = revs(&graph).0;
    fixture.put("data/kick/hits.json", &hits_of(5));
    let t = std::time::Instant::now();
    graph.watch_wait(Duration::from_secs(5));
    assert!(t.elapsed() < Duration::from_secs(1), "woken by the event: {:?}", t.elapsed());
    graph.watch();
    assert!(graph.inner.watch.lock().as_ref().unwrap().settle.settling(), "the batch opens");
    let t = std::time::Instant::now();
    graph.watch_wait(Duration::from_secs(5));
    let waited = t.elapsed();
    assert!(waited >= Duration::from_millis(100) && waited < Duration::from_secs(1), "woken when due: {waited:?}");
    assert!(graph.watch(), "taken in by the tick it was woken for");
    assert_eq!(revs(&graph).0, number + 1);
}
