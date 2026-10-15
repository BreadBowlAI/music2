//! G11's watcher (WRITE-19; the step 6 design's 24: over 2 ms per poll, file-system events with a
//! slow walk as the fallback): p16 with 5,000 files in `assets/`, playing, its watcher ticked as
//! the server ticks it.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use m2_server::graph::Graph;
use m2_song::lane::LaneData;

use super::{moved, open_with, taken_ms};
use crate::Lab;
use crate::gates::Opts;
use crate::report::Outcome;
use crate::signal::{Rng, Summary};

/// The CPU time the calling thread has used, in seconds.
fn thread_cpu() -> f64 {
    m2_os::thread::cpu_time().as_secs_f64()
}

/// One tick of the watcher: when it began, how long it took, and the CPU time it used.
#[derive(Clone, Copy)]
struct Tick {
    at: Instant,
    ms: f64,
    cpu: f64,
}

/// The server's watcher loop (`spawn_graph_watch`): a tick 50 ms after the last, sooner when an
/// event names a change while none is settling or a batch's quiet ends; on a thread of its own,
/// recording each tick.
struct Ticker {
    stop: Arc<AtomicBool>,
    ticks: Arc<parking_lot::Mutex<Vec<Tick>>>,
    thread: Option<JoinHandle<()>>,
}

impl Ticker {
    fn start(graph: &Arc<Graph>) -> Ticker {
        let (stop, ticks) = (Arc::new(AtomicBool::new(false)), Arc::new(parking_lot::Mutex::new(vec![])));
        let (g, s, t) = (graph.clone(), stop.clone(), ticks.clone());
        let thread = std::thread::spawn(move || {
            while !s.load(Ordering::Relaxed) {
                g.watch_wait(Duration::from_millis(50));
                let (at, cpu) = (Instant::now(), thread_cpu());
                g.watch();
                t.lock().push(Tick { at, ms: at.elapsed().as_secs_f64() * 1e3, cpu: thread_cpu() - cpu });
            }
        });
        Ticker { stop, ticks, thread: Some(thread) }
    }

    /// The ticks that began in `from..to`.
    fn between(&self, from: Instant, to: Instant) -> Vec<Tick> {
        self.ticks.lock().iter().filter(|t| t.at >= from && t.at < to).copied().collect()
    }
}

impl Drop for Ticker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// `on`'s idle lines: 10 s of ticks with nothing written.
fn idle(out: &mut Outcome, on: &str, ticker: &Ticker, check: bool) {
    let from = Instant::now();
    std::thread::sleep(Duration::from_secs(10));
    let to = Instant::now();
    let ticks = ticker.between(from, to);
    let ms: Vec<f64> = ticks.iter().map(|t| t.ms).collect();
    let share = ticks.iter().map(|t| t.cpu).sum::<f64>() / to.duration_since(from).as_secs_f64() * 100.0;
    out.info(format!("{on}idle: each tick, its walks included"), Summary::of(&ms));
    let used = format!("{share:.2}% of a core ({} ticks in 10 s)", ticks.len());
    if check {
        out.check(format!("{on}idle: the CPU its ticks used over 10 s, under 1% of a core"), share < 1.0, used);
    } else {
        out.info(format!("{on}idle: the CPU its ticks used over 10 s"), used);
    }
}

/// `on`'s outside writes: `n` writes of `data/t01/notes.json`, one note moved each time, half
/// written in place and half as an editor's atomic save (a new file renamed over it), each from
/// its last byte to the first block that plays it.
fn outside(out: &mut Outcome, on: &str, graph: &Arc<Graph>, dir: &Path, rate: u32, n: usize) -> Result<()> {
    let (mut place, mut renamed) = (vec![], vec![]);
    let first = graph.generation();
    let mut swapped = 0;
    let mut rng = Rng::new(11);
    for i in 0..n {
        std::thread::sleep(Duration::from_secs_f64(rng.range(0.2, 0.5)));
        let mut lane = LaneData::Notes(moved(dir, i)?);
        lane.sort();
        let text = m2_song::lane::canonical(&lane);
        let before = graph.last_taken_in();
        let file = dir.join("data/t01/notes.json");
        if i.is_multiple_of(2) {
            std::fs::write(&file, &text)?;
        } else {
            let tmp = dir.join(".m2-gate-save.tmp");
            std::fs::write(&tmp, &text)?;
            std::fs::rename(&tmp, &file)?;
        }
        let stamp = graph.now_ns();
        let t0 = Instant::now();
        let t = loop {
            if let Some(t) = graph.last_taken_in().filter(|t| Some(*t) != before && t.seq.is_some()) {
                break t;
            }
            if t0.elapsed() > Duration::from_secs(10) {
                return Err(anyhow!("{on}outside write {i} was never taken in"));
            }
            std::thread::sleep(Duration::from_millis(1));
        };
        let ms = taken_ms(graph, t, stamp, rate)?;
        if i.is_multiple_of(2) {
            place.push(ms)
        } else {
            renamed.push(ms)
        }
        swapped += usize::from(graph.generation() == first);
    }
    out.info(format!("{on}an outside lane write in place: from its last byte to the first block that plays it"), Summary::of(&place));
    out.info(format!("{on}an outside lane write as an atomic save (renamed over it): the same"), Summary::of(&renamed));
    out.check(
        format!("{on}every outside write was taken in, as a lane swap in the same renderer"),
        swapped == n,
        format!("{swapped} of {n} swapped in the same renderer"),
    );
    Ok(())
}

/// G11's watcher lines: with file system events, as the app watches a project on a local volume,
/// then by walks alone, as on a network volume.
pub fn watcher(lab: &mut Lab, o: &Opts) -> Result<Outcome> {
    let dir = lab.reference(&o.reference)?;
    let song = lab.compile(&dir)?;
    let mut out = Outcome::new("G11 watcher", "WRITE-19", &format!("server on {} with 5,000 files in assets/", o.reference.name()));
    let assets = dir.join("assets");
    std::fs::create_dir_all(&assets)?;
    for i in 0..5000 {
        std::fs::write(assets.join(format!("take-{i:04}.wav")), b"")?;
    }
    let writes = if o.edits <= 2 { 6 } else { 20 };
    for walk in [false, true] {
        let on = if walk { "by walks alone (M2_WATCH=walk, a network volume): " } else { "" };
        let graph = Arc::new(Graph::default());
        graph.watch_by_walking(walk);
        let graph = open_with(graph, &dir, &song)?;
        let stats = graph.watch_stats().context("no watcher")?;
        if !walk {
            out.check("watched by file-system events (a local volume)", stats.events, if stats.events { "events" } else { "walks alone" });
        }
        let ticker = Ticker::start(&graph);
        let before = graph.watch_stats().context("no watcher")?;
        idle(&mut out, on, &ticker, !walk);
        let after = graph.watch_stats().context("no watcher")?;
        let largest = ticker.ticks.lock().iter().map(|t| t.ms).fold(0.0, f64::max);
        out.info(
            format!("{on}idle: what the ticks did over 10 s"),
            format!(
                "{} walks of everything (the last {:.1} ms), {} ticks that stat'd some paths, {} that looked at nothing; the largest tick since open {largest:.1} ms",
                after.walks - before.walks,
                after.last_walk_ms,
                after.scoped - before.scoped,
                after.idle - before.idle,
            ),
        );
        outside(&mut out, on, &graph, &dir, song.rate, if walk { writes / 2 } else { writes })?;
        drop(ticker);
        graph.close();
    }
    // The full walk itself: with no asset named, and with an audio lane naming 1,000 of them
    // (each then stat'd, as the assets a lane plays are).
    let named: BTreeSet<String> = (0..1000).map(|i| format!("assets/take-{:04}.wav", i * 5)).collect();
    for (what, named) in [("no asset named", BTreeSet::new()), ("1,000 of them named by a lane", named)] {
        let mut walks = vec![];
        for _ in 0..20 {
            let t = Instant::now();
            std::hint::black_box(m2_session::watch::listing(&dir, &named));
            walks.push(t.elapsed().as_secs_f64() * 1e3);
        }
        out.info(format!("the full stat walk, {what}"), Summary::of(&walks));
    }
    let _ = std::fs::remove_dir_all(&dir).context("removing the project");
    Ok(out)
}
