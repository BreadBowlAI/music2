//! The timed gates as the app meets them: a reference project open in the server's own
//! [`Graph`], playing in real time on the silent player (`M2_AUDIO=none`), driven through the
//! calls the UI makes (`seek`, `write_notes`, `add_effect`), timed on the wall clock.
//!
//! What only the server shows: a seek that waits behind a compile holding the project
//! (RENDER-29), and a playhead that jumps back after a rebuild (RENDER-8). Each change is timed as
//! step 3a's design (§15) counts it: to when it is heard, which is the start of the audio callback
//! that took it ([`Graph::applied`]), plus the frame its block starts at in that callback, plus one
//! device buffer to the speaker. A seek counts from when it was asked for; an edit from when the
//! server's session had done its work, its compile included ([`Graph::last_edit`]), which the
//! contract's budgets leave out.
//!
//! These time what the app does; they can't listen. The silent player renders into nothing and
//! the server keeps no note trace, so what is heard after a seek or an edit is judged on the
//! engine rig ([`crate::gates`]), which plays the same engine through the same player callback.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use m2_server::graph::{EditTiming, Graph};
use m2_song::NoteClip;
use m2_song::lane::{LaneData, LaneKind};

use crate::gates::Opts;
use crate::report::Outcome;
use crate::signal::{Rng, Summary};
use crate::{Lab, Song, songs};

/// The silent player's buffer, in seconds: counted in every latency, as a device's would be.
fn period(rate: u32) -> f64 {
    let frames: f64 = std::env::var("M2_AUDIO_FRAMES").ok().and_then(|v| v.parse().ok()).unwrap_or(480.0);
    frames / rate as f64
}

/// The project in `dir`, open and playing from 1:00 in a fresh [`Graph`].
fn open(dir: &Path, song: &Song) -> Result<Arc<Graph>> {
    if std::env::var("M2_AUDIO").as_deref() != Ok("none") {
        bail!("server gates run on the silent player only: set M2_AUDIO=none");
    }
    let graph = Arc::new(Graph::default());
    graph.open(dir)?;
    graph.seek(song.beat(song.rate as u64 * 60))?;
    graph.transport("play")?;
    std::thread::sleep(Duration::from_secs(1));
    Ok(graph)
}

fn session_and_revision(graph: &Graph) -> Result<(u64, u64)> {
    let state = graph.state().context("no project is open")?;
    let token = state["session"].as_u64().context("no session")?;
    let revision = state["view"]["revision"].as_u64().context("no revision")?;
    Ok((token, revision))
}

/// Why a server gate leaves what is heard to the engine rig.
const UNHEARD: &str = "the silent player keeps no output and the server no note trace: judged on the engine rig";

/// Wait until `ready`, polling every millisecond, for `limit` at most.
fn wait(limit: Duration, mut ready: impl FnMut() -> bool) -> Option<()> {
    let t = Instant::now();
    while t.elapsed() < limit {
        if ready() {
            return Some(());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    None
}

/// When the change `t` carried was heard, in milliseconds after `from_ns` (on the player's clock):
/// the start of the callback that took its message, plus the frame its block starts at in that
/// callback, plus one device buffer. Waits for the engine to take it.
fn heard_ms(graph: &Graph, t: EditTiming, from_ns: u64, rate: u32) -> Result<f64> {
    Ok(taken_ms(graph, t, from_ns, rate)? + period(rate) * 1e3)
}

/// When the engine rendered the first block with the change `t` carried, in milliseconds after
/// `from_ns`: [`heard_ms`] without the device buffer to the speaker.
fn taken_ms(graph: &Graph, t: EditTiming, from_ns: u64, rate: u32) -> Result<f64> {
    let seq = t.seq.context("the change sent the engine nothing")?;
    let mut applied = None;
    wait(Duration::from_secs(5), || {
        applied = graph.applied(seq);
        applied.is_some()
    })
    .ok_or_else(|| anyhow!("the engine never took message {seq}"))?;
    let a = applied.expect("waited for it");
    let taken = a.callback_ns as f64 + a.frame as f64 * 1e9 / rate as f64;
    Ok((taken - from_ns as f64) / 1e6)
}

/// Save a code change to `tracks/t01.ts` the way the code editor does: it compiles while holding
/// the project.
fn touch(graph: &Graph, dir: &Path, n: usize) -> Result<()> {
    let path = "tracks/t01.ts";
    let base = std::fs::read_to_string(dir.join(path))?;
    let text = match base.find("\n// gate ") {
        Some(i) => base[..i + 1].to_string(),
        None => format!("{base}\n// gate {n}\n"),
    };
    graph.write_file(None, path, &base, &text)?;
    Ok(())
}

/// G1 through the server: seeks from 1:00, a third of them while a code save compiles holding the
/// project. A seek must not wait for it (RENDER-29), and must be heard within 50 ms of the call.
pub fn seek(lab: &mut Lab, o: &Opts) -> Result<Outcome> {
    let dir = lab.reference(&o.reference)?;
    let song = lab.compile(&dir)?;
    let graph = open(&dir, &song)?;
    let mut out = Outcome::new("G1 seek, through the server", "RENDER-8, RENDER-29", &format!("server on {}", o.reference.name()));
    let bps = 1.0 / song.data.tempo().seconds_at(1.0);
    let (mut free, mut busy, mut misplaced) = (vec![], vec![], vec![]);
    let mut rng = Rng::new(o.seed);
    for i in 0..o.seeks {
        let target = rng.range(0.0, song.data.end());
        let holder = if i % 3 == 2 {
            let (g, d) = (graph.clone(), dir.clone());
            let started = Arc::new(AtomicBool::new(false));
            let s = started.clone();
            let h = std::thread::spawn(move || {
                s.store(true, Ordering::Relaxed);
                touch(&g, &d, i)
            });
            while !started.load(Ordering::Relaxed) {
                std::hint::spin_loop();
            }
            // Into the compile, which holds the project.
            std::thread::sleep(Duration::from_millis(20));
            Some(h)
        } else {
            None
        };
        let t = Instant::now();
        graph.seek(target)?;
        let timing = graph.last_edit().context("the seek left no timing")?;
        let ms = heard_ms(&graph, timing, timing.requested_ns, song.rate)?;
        // Once a later callback has published it, the playhead is the target, plus what played since.
        std::thread::sleep(Duration::from_millis(30));
        let at = graph.status().beat;
        if !(at >= target - 1e-6 && at <= target + bps * (t.elapsed().as_secs_f64() + 0.1)) {
            misplaced.push(format!("beat {target:.2}: the playhead at {at:.2}"));
        }
        if let Some(h) = holder {
            h.join().map_err(|_| anyhow!("the code save panicked"))??;
            busy.push(ms);
        } else {
            free.push(ms);
        }
        std::thread::sleep(Duration::from_secs_f64(rng.range(0.3, 1.0)));
    }
    let (f, b) = (Summary::of(&free), Summary::of(&busy));
    out.check("audible within 50 ms", f.max <= 50.0, f);
    out.check("audible within 50 ms while a compile holds the project", b.max <= 50.0 || b.n == 0, b);
    out.check(
        "lands on its target",
        misplaced.is_empty(),
        misplaced.first().map_or("every seek".into(), |m| format!("{} wrong; {m}", misplaced.len())),
    );
    out.skip("every note from the target on is heard, nothing chased (RENDER-11)", UNHEARD);
    Ok(out)
}

/// The playhead, sampled every 2 ms from another thread while something runs.
struct Sampler {
    stop: Arc<AtomicBool>,
    thread: std::thread::JoinHandle<Vec<f64>>,
}

impl Sampler {
    fn start(graph: &Arc<Graph>) -> Sampler {
        let (g, stop) = (graph.clone(), Arc::new(AtomicBool::new(false)));
        let s = stop.clone();
        let thread = std::thread::spawn(move || {
            let mut beats = vec![];
            while !s.load(Ordering::Relaxed) {
                beats.push(g.status().beat);
                std::thread::sleep(Duration::from_millis(2));
            }
            beats
        });
        Sampler { stop, thread }
    }

    /// The largest step back the playhead took, in beats.
    fn finish(self) -> f64 {
        self.stop.store(true, Ordering::Relaxed);
        let beats = self.thread.join().unwrap_or_default();
        beats.windows(2).map(|w| w[0] - w[1]).fold(0.0, f64::max)
    }
}

/// What edits through the server took: until heard, past the session's work (what is counted);
/// the session's work; and of what is counted, the server's part before it sent the engine its
/// message (lane data prepared, or a renderer built), then the wait for the callback that took it.
/// One device buffer to the speaker makes up the rest.
#[derive(Default)]
struct Timed {
    heard: Vec<f64>,
    session: Vec<f64>,
    prepare: Vec<f64>,
    taken: Vec<f64>,
}

impl Timed {
    fn add(&mut self, graph: &Graph, rate: u32) -> Result<()> {
        let t = graph.last_edit().context("the edit left no timing")?;
        self.heard.push(heard_ms(graph, t, t.compiled_ns, rate)?);
        self.taken.push(taken_ms(graph, t, t.sent_ns, rate)?);
        self.session.push((t.compiled_ns - t.requested_ns) as f64 / 1e6);
        self.prepare.push((t.sent_ns - t.compiled_ns) as f64 / 1e6);
        Ok(())
    }

    fn report(&self, out: &mut Outcome, budget: f64) {
        let heard = Summary::of(&self.heard);
        out.info("the session's own work, its compile included (not counted)", Summary::of(&self.session));
        out.info("of what is counted, the server's before it sends", Summary::of(&self.prepare));
        out.info("then waiting for the next callback to take it (one device buffer at most)", Summary::of(&self.taken));
        out.check(format!("audible within compile + {budget:.0} ms"), heard.max <= budget, format!("past the session's work: {heard}"));
    }
}

/// G3 through the server: `write_notes` adds a note half a beat ahead of the playhead, from 1:00,
/// `o.edits` times.
pub fn note_edit(lab: &mut Lab, o: &Opts) -> Result<Outcome> {
    let dir = lab.reference(&o.reference)?;
    let song = lab.compile(&dir)?;
    let graph = open(&dir, &song)?;
    let mut out = Outcome::new("G3 note edit, through the server", "RENDER-26, PLANE-1", &format!("server on {}", o.reference.name()));
    let mut rng = Rng::new(o.seed);
    let mut timed = Timed::default();
    let sampler = Sampler::start(&graph);
    for _ in 0..o.edits {
        let text = std::fs::read_to_string(dir.join("data/t01/notes.json"))?;
        let LaneData::Notes(mut clips) = LaneData::read(&text, &LaneKind::Notes).map_err(|e| anyhow!("{e:?}"))? else { bail!("not notes") };
        // Half a beat ahead, or where the next clip starts when one ends there.
        let ahead = graph.status().beat + 0.5;
        let holds = |c: &NoteClip, b: f64| c.start <= b && b + 0.5 < c.start + c.length;
        let beat = clips.iter().map(|c| c.start.max(ahead)).filter(|b| clips.iter().any(|c| holds(c, *b))).min_by(f64::total_cmp);
        let beat = beat.context("no clip at or after the playhead")?;
        let c = clips.iter_mut().find(|c| holds(c, beat)).expect("found");
        c.notes.push(songs::note(72.0, beat - c.start, 0.5));
        let (token, revision) = session_and_revision(&graph)?;
        graph.write_notes(token, revision, "t01", "notes", &clips)?;
        timed.add(&graph, song.rate)?;
        std::thread::sleep(Duration::from_secs_f64(rng.range(0.3, 1.0)));
    }
    let back = sampler.finish();
    timed.report(&mut out, 20.0);
    out.check("the playhead never jumps back", back <= 1e-9, format!("largest step back {back:.3} beats"));
    out.skip("the new note is heard where it was written", UNHEARD);
    Ok(out)
}

/// G5 through the server: `add_effect` puts a delay after a track's `pluck`, from 1:00, on up to
/// `o.edits` tracks one after another. The playhead, sampled every 2 ms meanwhile, must never jump
/// back (RENDER-8, RENDER-25a).
pub fn add_effect(lab: &mut Lab, o: &Opts) -> Result<Outcome> {
    let dir = lab.reference(&o.reference)?;
    let song = lab.compile(&dir)?;
    let graph = open(&dir, &song)?;
    let mut out =
        Outcome::new("G5 add effect, through the server", "RENDER-8, RENDER-25, RENDER-25a", &format!("server on {}", o.reference.name()));
    let calls: Vec<String> = {
        let s = lab.session(&dir)?;
        let sites = s.accepted().compiled.sites.iter().filter(|x| x.in_project && x.file.starts_with("tracks/") && x.callee == "pluck");
        sites.map(|x| format!("{}#{}", x.file, x.ordinal)).take(o.edits).collect()
    };
    if calls.is_empty() {
        bail!("no pluck in the reference project's tracks");
    }
    let mut rng = Rng::new(o.seed);
    let mut timed = Timed::default();
    let sampler = Sampler::start(&graph);
    for call in &calls {
        let (token, revision) = session_and_revision(&graph)?;
        graph.add_effect(token, revision, call, "std/fx", "delay")?;
        timed.add(&graph, song.rate)?;
        std::thread::sleep(Duration::from_secs_f64(rng.range(0.3, 1.0)));
    }
    let back = sampler.finish();
    timed.report(&mut out, 100.0);
    out.check(
        "no jump back",
        back <= 1e-9,
        format!("largest step back {back:.3} beats ({:.0} ms)", back * song.data.tempo().seconds_at(1.0) * 1e3),
    );
    out.skip("the added effect is heard (RENDER-25)", UNHEARD);
    Ok(out)
}
