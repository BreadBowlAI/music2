//! Audition on the server (MON-7, 8b.6): a window's notes reach the plan last sent and end by
//! their Offs, the window closing, or the watchdog; a lane nothing reads says so; bad notes are
//! refused in words; it works read-only, after an edit, and while the session compiles.

use super::super::tests::{Fixture, eventually};
use super::*;

/// A kick track that plays its `hits` lane, and declares a `spare` lane it never reads.
const SONG: &str = "import { song, track } from '@m2/core';\nimport { kick } from '@m2/synths';\n\
                    const k = track('kick', ({ lane }) => { lane.notes('spare'); return kick(lane.notes('hits')); });\n\
                    export default song(() => k.out);\n";

fn opened(song: &str) -> (Fixture, Graph) {
    let fixture = Fixture::new();
    fixture.put("song.ts", song);
    let graph = fixture.open();
    (fixture, graph)
}

fn on(lane: &str, h: u32, pitch: f64) -> Audition {
    Audition { module: Some("kick".into()), lane: Some(lane.into()), on: vec![Strike { h, pitch, velocity: 0.8 }], ..Audition::default() }
}

fn off(h: u32) -> Audition {
    Audition { off: vec![h], ..Audition::default() }
}

/// Live notes sounding, as `/api/graph/status` says.
fn live(graph: &Graph) -> u32 {
    graph.activity().live
}

/// Make the window `client` look as if it last spoke `secs` seconds earlier than it did.
fn age(graph: &Graph, client: &str, secs: u64) {
    graph.inner.control.lock().audition.by_client.get_mut(client).unwrap().heard -= Duration::from_secs(secs);
}

// MON-7: a window's note sounds in what plays until its Off; the status and the state say how many.
#[test]
fn a_note_sounds_until_it_is_let_go() {
    let (_f, graph) = opened(SONG);
    assert_eq!(graph.audition("w1", on("hits", 7, 36.0)).unwrap(), None);
    eventually("the note sounds", || live(&graph) == 1);
    assert_eq!(graph.state().unwrap()["live"], json!(1));
    // A pitch drag: the old pitch let go and the new one struck, in one message.
    let drag = Audition { off: vec![7], ..on("hits", 8, 38.0) };
    graph.audition("w1", drag).unwrap();
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(live(&graph), 1);
    graph.audition("w1", off(8)).unwrap();
    eventually("it ends", || live(&graph) == 0);
}

// MON-7: a handle struck again on another lane is let go on the first: nothing is left on.
#[test]
fn a_handle_struck_again_elsewhere_lets_go_first() {
    let two = "import { song, track } from '@m2/core';\nimport { kick } from '@m2/synths';\nimport { sum } from 'std/math';\n\
               const k = track('kick', ({ lane }) => sum(kick(lane.notes('hits')), kick(lane.notes('more'))));\n\
               export default song(() => k.out);\n";
    let (_f, graph) = opened(two);
    graph.audition("w1", on("hits", 1, 36.0)).unwrap();
    eventually("the note sounds", || live(&graph) == 1);
    graph.audition("w1", on("more", 1, 38.0)).unwrap();
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(live(&graph), 1, "the first one ended");
    graph.audition("w1", off(1)).unwrap();
    eventually("nothing is left on", || live(&graph) == 0);
}

// MON-7: a lane nothing plays, or one the song doesn't have, answers in words, and holds nothing.
#[test]
fn a_lane_nothing_reads_says_so() {
    let (_f, graph) = opened(SONG);
    for lane in ["spare", "nope"] {
        let answer = graph.audition("w1", on(lane, 1, 36.0)).unwrap();
        assert_eq!(answer, Some(json!({"t": "audition", "module": "kick", "lane": lane, "silent": "Nothing plays this lane"})));
    }
    assert!(graph.inner.control.lock().audition.by_client["w1"].held.is_empty());
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(live(&graph), 0);
}

// MON-7, R2: a pitch or velocity out of range, or notes with no lane, are refused in words, and
// nothing is sent.
#[test]
fn bad_notes_are_refused_in_words() {
    let (_f, graph) = opened(SONG);
    let said = |a: Audition| graph.audition("w1", a).unwrap_err().to_string();
    assert!(said(on("hits", 1, 128.0)).contains("pitch is a number from 0 to 127"));
    let loud = Audition { on: vec![Strike { h: 1, pitch: 36.0, velocity: 1.5 }], ..on("hits", 1, 36.0) };
    assert!(said(loud).contains("velocity is a number from 0 to 1"));
    assert!(said(Audition { module: None, ..on("hits", 1, 36.0) }).contains("Say which lane"));
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(live(&graph), 0);
}

// MON-7: a window that closes lets go of its notes at once, and only its own.
#[test]
fn a_closed_window_releases_its_notes() {
    let (_f, graph) = opened(SONG);
    graph.audition("w1", on("hits", 1, 36.0)).unwrap();
    graph.audition("w1", on("hits", 2, 38.0)).unwrap();
    graph.audition("w2", on("hits", 1, 40.0)).unwrap();
    eventually("three notes sound", || live(&graph) == 3);
    graph.client_gone("w1");
    eventually("the other window's note sounds on", || live(&graph) == 1);
    graph.audition("w2", off(1)).unwrap();
    eventually("it ends", || live(&graph) == 0);
}

// MON-7: a window that held notes and said nothing for 10 s is let go; a keepalive keeps another.
#[test]
fn the_watchdog_releases_a_quiet_window() {
    let (_f, graph) = opened(SONG);
    graph.audition("w1", on("hits", 1, 36.0)).unwrap();
    graph.audition("w1", on("hits", 2, 38.0)).unwrap();
    graph.audition("w2", on("hits", 1, 40.0)).unwrap();
    eventually("three notes sound", || live(&graph) == 3);
    graph.expire_auditions();
    age(&graph, "w1", 11);
    age(&graph, "w2", 9);
    graph.expire_auditions();
    eventually("the quiet window is let go", || live(&graph) == 1);
    graph.audition("w2", Audition { keep: vec![1], ..Audition::default() }).unwrap();
    age(&graph, "w2", 9);
    graph.expire_auditions();
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(live(&graph), 1, "the keepalive kept it");
    age(&graph, "w2", 2);
    graph.expire_auditions();
    eventually("then it went quiet too", || live(&graph) == 0);
    assert!(graph.inner.control.lock().audition.by_client.is_empty(), "and both are forgotten");
}

// MON-7, WRITE-17: audition is no write: a window open read-only plays notes too.
#[test]
fn audition_works_read_only() {
    let fixture = Fixture::new();
    fixture.put("song.ts", SONG);
    let held = m2_session::lock::acquire(&fixture.0, "m2", m2_session::lock::RETRY).unwrap();
    let graph = fixture.open();
    assert!(graph.read_only().is_some());
    graph.audition("w1", on("hits", 1, 36.0)).unwrap();
    eventually("the note sounds", || live(&graph) == 1);
    drop(held);
}

// MON-7, RENDER-26a: an edit that moves the lane to another slot keeps the note sounding, and its
// Off finds the lane where it is now.
#[test]
fn an_off_after_an_edit_finds_its_lane() {
    let (fixture, graph) = opened(SONG);
    let slot = |g: &Graph| {
        g.inspect(|a| {
            let p = a.open.as_ref().unwrap().plays.as_ref().unwrap();
            p.info.lanes.iter().position(|l| l.lane.module == "kick" && l.lane.lane == "hits").unwrap()
        })
    };
    let before = slot(&graph);
    graph.audition("w1", on("hits", 1, 36.0)).unwrap();
    eventually("the note sounds", || live(&graph) == 1);
    fixture.put(
        "song.ts",
        "import { song, track } from '@m2/core';\nimport { kick } from '@m2/synths';\nimport { sum } from 'std/math';\n\
         const s = track('snare', ({ lane }) => kick(lane.notes('hits')));\n\
         const k = track('kick', ({ lane }) => { lane.notes('spare'); return kick(lane.notes('hits')); });\n\
         export default song(() => sum(s.out, k.out));\n",
    );
    assert!(graph.poll());
    assert_ne!(slot(&graph), before, "the kick's lane moved");
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(live(&graph), 1, "the note moved with its lane");
    graph.audition("w1", off(1)).unwrap();
    eventually("its Off found it", || live(&graph) == 0);
}

// MON-7, RENDER-29, design-8b §8.5: while the session compiles a slow save, a struck note is heard
// before the compile ends: audition never takes the session.
#[test]
fn audition_never_waits_for_a_compile() {
    let slow = |n: u64| {
        format!(
            "import {{ hz, track }} from '@m2/core';\nimport {{ osc }} from 'std/dsp';\nfor (let i = 0; i < {n}; i++) {{}}\n\
             export const b = track('b', () => osc.tri(hz(300)));\n"
        )
    };
    let fixture = Fixture::new();
    fixture.put("tracks/b.ts", &slow(0));
    fixture.put(
        "song.ts",
        "import { song, track } from '@m2/core';\nimport { kick } from '@m2/synths';\nimport { sum } from 'std/math';\n\
         import { b } from './tracks/b';\nconst k = track('kick', ({ lane }) => kick(lane.notes('hits')));\n\
         export default song(() => sum(k.out, b.out));\n",
    );
    let graph = std::sync::Arc::new(fixture.open());
    let saver = {
        let (g, dir) = (graph.clone(), fixture.0.clone());
        std::thread::spawn(move || {
            std::fs::write(dir.join("tracks/b.ts"), slow(100_000_000)).unwrap();
            g.poll()
        })
    };
    eventually("the session compiles", || graph.busy().is_some());
    let busy_from = graph.busy().unwrap();
    let t = Instant::now();
    graph.audition("w1", on("hits", 1, 36.0)).unwrap();
    let sent_ms = t.elapsed().as_secs_f64() * 1e3;
    eventually("the note sounds", || live(&graph) == 1);
    let heard = graph.now_ns();
    assert!(saver.join().unwrap(), "the slow save was taken in");
    let (start, end) = *graph.busy_spans().iter().rev().find(|s| s.0 == busy_from).expect("the compile's span");
    let compile_ms = (end - start) as f64 / 1e6;
    if compile_ms < 500.0 {
        eprintln!("void: the compile took only {compile_ms:.0} ms");
        return;
    }
    eprintln!("audition: sent in {sent_ms:.2} ms, heard {:.0} ms before a {compile_ms:.0} ms compile ended", (end - heard) as f64 / 1e6);
    assert!(sent_ms < 20.0, "sending took {sent_ms:.1} ms");
    assert!(heard < end, "heard only after the compile ended");
}
