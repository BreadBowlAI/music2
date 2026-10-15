//! The timed gates as the app meets them: a reference project open in the server's own
//! [`Graph`], playing in real time on the silent player (`M2_AUDIO=none`), driven through the
//! calls the UI makes (`seek`, `write_notes`, `add_effect`, a knob drag's lease), timed on the wall
//! clock.
//!
//! What only the server shows: a seek or a knob drag that waits behind a compile on the session
//! (RENDER-29, WRITE-22), a drag that an outside save ends, and a playhead that jumps back after a
//! rebuild (RENDER-8). Each change is timed as
//! step 3a's design (§15) counts it: to when it is heard, which is the start of the audio callback
//! that took it ([`Graph::applied`]), plus the frame its block starts at in that callback, plus one
//! device buffer to the speaker. A seek counts from when it was asked for; an edit from when the
//! server's session had done its work before the engine could be told, its compile included
//! ([`Graph::last_edit`]'s `ready_ns`; for a data Tx heard as its preview, its checks, PLANE-5),
//! which the contract's budgets leave out. G11 counts a data Tx from its request.
//!
//! These time what the app does; they can't listen. The silent player renders into nothing and
//! the server keeps no note trace, so what is heard after a seek or an edit is judged on the
//! engine rig ([`crate::gates`]), which plays the same engine through the same player callback.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use m2_editor::Gestures;
use m2_runtime::Count;
use m2_server::graph::{EditTiming, Graph};
use m2_song::NoteClip;
use m2_song::lane::{LaneData, LaneKind};

use crate::gates::Opts;
use crate::report::Outcome;
use crate::signal::{Rng, Summary, db, max_step, span};
use crate::{Lab, Song, songs};

mod watch;
pub use watch::watcher;

/// The silent player's buffer, in seconds: counted in every latency, as a device's would be.
fn period(rate: u32) -> f64 {
    let frames: f64 = std::env::var("M2_AUDIO_FRAMES").ok().and_then(|v| v.parse().ok()).unwrap_or(480.0);
    frames / rate as f64
}

/// The project in `dir`, open and playing from 1:00 in a fresh [`Graph`].
fn open(dir: &Path, song: &Song) -> Result<Arc<Graph>> {
    open_with(Arc::new(Graph::default()), dir, song)
}

/// [`open`] in `graph`.
fn open_with(graph: Arc<Graph>, dir: &Path, song: &Song) -> Result<Arc<Graph>> {
    open_at(graph, dir, song, 60.0)
}

/// [`open_with`], playing from `seconds`.
fn open_at(graph: Arc<Graph>, dir: &Path, song: &Song, seconds: f64) -> Result<Arc<Graph>> {
    if std::env::var("M2_AUDIO").as_deref() != Ok("none") {
        bail!("server gates run on the silent player only: set M2_AUDIO=none");
    }
    graph.open(dir)?;
    graph.seek(song.beat((seconds * song.rate as f64) as u64))?;
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

/// Save a code change to `tracks/t01.ts` the way the code editor does: it compiles on the session's
/// actor, which the transport must not wait for.
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

/// G1 through the server: seeks from 1:00, a third of them while a code save compiles on the
/// session. A seek must not wait for it (RENDER-29), and must be heard within 50 ms of the call.
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
            // Into the compile.
            std::thread::sleep(Duration::from_millis(20));
            Some(h)
        } else {
            None
        };
        // Timed from the call, on the engine's clock read without a lock: a seek that waits for a
        // lock the session holds over a compile is counted, which the seek's own stamp, taken
        // inside, wouldn't show (RENDER-29).
        let (t, from) = (Instant::now(), graph.now_ns());
        graph.seek(target)?;
        let timing = graph.last_edit().context("the seek left no timing")?;
        let ms = heard_ms(&graph, timing, from, song.rate)?;
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
    out.check("audible within 50 ms while the session compiles", b.max <= 50.0 || b.n == 0, b);
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

/// What edits through the server took: until heard, past the session's work before the engine
/// could be told (what is counted: its compile included, and for a data Tx heard as its preview,
/// its checks; PLANE-5); that work; and of what is counted, the server's part before it sent the
/// engine its message (lane data prepared, or a renderer built), then the wait for the callback
/// that took it. One device buffer to the speaker makes up the rest. A previewed edit's commit is
/// made durable beside it: until then is said apart.
#[derive(Default)]
struct Timed {
    heard: Vec<f64>,
    session: Vec<f64>,
    prepare: Vec<f64>,
    taken: Vec<f64>,
    durable: Vec<f64>,
}

impl Timed {
    fn add(&mut self, graph: &Graph, rate: u32) -> Result<()> {
        let t = graph.last_edit().context("the edit left no timing")?;
        self.heard.push(heard_ms(graph, t, t.ready_ns, rate)?);
        self.taken.push(taken_ms(graph, t, t.sent_ns, rate)?);
        self.session.push((t.ready_ns - t.requested_ns) as f64 / 1e6);
        self.prepare.push((t.sent_ns - t.ready_ns) as f64 / 1e6);
        if t.previewed {
            self.durable.push((t.compiled_ns - t.requested_ns) as f64 / 1e6);
        }
        Ok(())
    }

    /// What was timed, each line named after `on` (empty for the reference project).
    fn report(&self, out: &mut Outcome, on: &str, budget: f64) {
        let heard = Summary::of(&self.heard);
        out.info(format!("{on}the session's own work, its compile included (not counted)"), Summary::of(&self.session));
        out.info(format!("{on}of what is counted, the server's before it sends"), Summary::of(&self.prepare));
        out.info(format!("{on}then waiting for the next callback to take it (one device buffer at most)"), Summary::of(&self.taken));
        if !self.durable.is_empty() {
            out.info(format!("{on}heard as its preview (PLANE-5): from the request until its commit is durable"), Summary::of(&self.durable));
        }
        out.check(format!("{on}audible within compile + {budget:.0} ms"), heard.max <= budget, format!("past the session's work: {heard}"));
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
    timed.report(&mut out, "", 20.0);
    out.check("the playhead never jumps back", back <= 1e-9, format!("largest step back {back:.3} beats"));
    out.skip("the new note is heard where it was written", UNHEARD);
    Ok(out)
}

/// G5 and G7a through the server, from `o.at` (1:00; G7a's recorded run is p50 from 3:00): `o.edits`
/// edits through the app, alternately a delay added after t02's `pluck` (`add_effect`) and taken
/// out again (`undo`), each a code edit the engine hears as an activation. Each must be heard within
/// compile + 100 ms (G5) and within 200 ms of its request, its compile, check and verify included
/// (G7a, which is stricter than the contract's "after the compile finishes"); the playhead, sampled
/// every 2 ms meanwhile, must never jump back (RENDER-8, RENDER-25a).
pub fn add_effect(lab: &mut Lab, o: &Opts) -> Result<Outcome> {
    let dir = lab.reference(&o.reference)?;
    let song = lab.compile(&dir)?;
    let graph = open_at(Arc::new(Graph::default()), &dir, &song, o.at)?;
    let rules = "RENDER-8, RENDER-25, RENDER-25a";
    let mut out = Outcome::new("G5 and G7a add effect, through the server", rules, &format!("server on {} from {:.0} s", o.reference.name(), o.at));
    let call = {
        let s = lab.session(&dir)?;
        let site = s.accepted().compiled.sites.iter().find(|x| x.in_project && x.file == "tracks/t02.ts" && x.callee == "pluck").cloned();
        site.map(|x| format!("{}#{}", x.file, x.ordinal)).context("no pluck in the reference project's t02")?
    };
    let mut rng = Rng::new(o.seed);
    let mut timed = Timed::default();
    let (mut asked, mut since_compile, mut cont, mut prepare, mut fresh) = (vec![], vec![], vec![], vec![], vec![]);
    let sampler = Sampler::start(&graph);
    for k in 0..o.edits {
        let (token, revision) = session_and_revision(&graph)?;
        if k % 2 == 0 {
            graph.add_effect(token, revision, &call, "std:delay")?;
        } else {
            graph.undo(token)?;
        }
        timed.add(&graph, song.rate)?;
        let t = graph.last_edit().context("the edit left no timing")?;
        asked.push(heard_ms(&graph, t, t.requested_ns, song.rate)?);
        since_compile.push(heard_ms(&graph, t, t.compiled_ns, song.rate)?);
        cont.push(t.continues_ns as f64 / 1e6);
        prepare.push(t.prepare_ns as f64 / 1e6);
        fresh.push(t.fresh);
        std::thread::sleep(Duration::from_secs_f64(rng.range(0.3, 1.0)));
    }
    let back = sampler.finish();
    timed.report(&mut out, "", 100.0);
    out.info("G7a: of the server's part, naming what continues", Summary::of(&cont));
    out.info("G7a: and preparing the plan", Summary::of(&prepare));
    out.info("G7a: heard after the compile finished (the contract's own start)", Summary::of(&since_compile));
    let heard = Summary::of(&asked);
    out.check("G7a: heard within 200 ms of the request, its compile included", heard.max <= 200.0, heard);
    let (ok, made) = made_new(&fresh);
    out.check("G7a: at most 3 instances made new per edit, the pluck's voice pool never", ok, made);
    out.check(
        "no jump back",
        back <= 1e-9,
        format!("largest step back {back:.3} beats ({:.0} ms)", back * song.data.tempo().seconds_at(1.0) * 1e3),
    );
    out.skip("the added effect is heard (RENDER-25)", UNHEARD);
    match with_plugins(lab, "with-plugins-add-effect")? {
        Err(why) => out.skip("with-plugins", why),
        Ok((dir, graph)) => {
            let measured = add_effect_with_plugins(&mut out, lab, o, &dir, &graph);
            // Its plugin instances are destroyed before the program ends, whatever happened.
            graph.close();
            measured?;
        }
    }
    Ok(out)
}

/// G7a's rule on what each edit made new (design §12.1, ID-13): the delay's own instances only, at
/// most 3, and no voice pool, so the pluck it wraps, and everything else, goes on. The plan counts
/// a whole voice pool as one, so "at most 3" alone would pass a pluck that starts again.
fn made_new(fresh: &[Count]) -> (bool, String) {
    let ok = !fresh.is_empty() && fresh.iter().all(|c| c.polys == 0 && c.total() <= 3);
    let each = fresh.iter().map(|c| {
        let kinds = [(c.kernels, "kernel"), (c.polys, "voice pool"), (c.plugins, "plugin"), (c.rings, "feedback ring")];
        let named: Vec<String> =
            kinds.iter().filter(|(n, _)| *n > 0).map(|(n, what)| format!("{n} {what}{}", if *n > 1 { "s" } else { "" })).collect();
        if named.is_empty() { "none".to_string() } else { named.join(" and ") }
    });
    (ok, format!("made new by the {} edits: {}", fresh.len(), each.collect::<Vec<_>>().join("; ")))
}

/// G5 on `examples/with-plugins`, playing from its start: a delay after the kick, then after the
/// lead's last plugin (Airwindows). Each activation must keep Surge XT and Airwindows (HOST-5,
/// RENDER-25), so preparing the new plan creates no plugin instance and the server's part before it
/// sends costs no plugin load; and the playhead never jumps back.
fn add_effect_with_plugins(out: &mut Outcome, lab: &Lab, o: &Opts, dir: &Path, graph: &Arc<Graph>) -> Result<()> {
    let rate = graph.state().context("no project is open")?["sample_rate"].as_u64().context("no rate")? as u32;
    let calls: Vec<String> = {
        let s = lab.session(dir)?;
        let sites = &s.accepted().compiled.sites;
        let kick = sites.iter().filter(|x| x.in_project && x.file == "tracks/kick.ts" && x.callee == "kick");
        let lead = sites.iter().filter(|x| x.in_project && x.file == "tracks/lead.ts" && x.callee == "plugin").max_by_key(|x| x.ordinal);
        kick.take(1).chain(lead).map(|x| format!("{}#{}", x.file, x.ordinal)).collect()
    };
    if calls.len() != 2 {
        bail!("no kick or lead plugin in with-plugins: {calls:?}");
    }
    let made = created();
    let mut rng = Rng::new(o.seed);
    let mut timed = Timed::default();
    let sampler = Sampler::start(graph);
    for call in &calls {
        let (token, revision) = session_and_revision(graph)?;
        graph.add_effect(token, revision, call, "std:delay")?;
        timed.add(graph, rate)?;
        std::thread::sleep(Duration::from_secs_f64(rng.range(0.3, 1.0)));
    }
    let back = sampler.finish();
    timed.report(out, "with-plugins: ", 100.0);
    out.check("with-plugins: no jump back", back <= 1e-9, format!("largest step back {back:.3} beats"));
    out.check(
        "with-plugins: each activation kept every plugin instance, creating none",
        created() == made,
        format!("{} created over {} activations", created() - made, calls.len()),
    );
    Ok(())
}

/// The ids of the knobs written in `file` with `literal`, in the order of their lines (the
/// reference project's channel levels in song.ts: t01 first). Read by a session of its own: control
/// ids are `file#ordinal/arg`, the same in the server's for the same code.
fn file_knobs(lab: &Lab, dir: &Path, file: &str, literal: &str) -> Result<Vec<String>> {
    let s = lab.session(dir)?;
    let mut found: Vec<(u32, String)> =
        s.controls().into_iter().filter(|c| c.file == file && c.literal == literal).map(|c| (c.line, c.id)).collect();
    found.sort();
    if found.is_empty() {
        bail!("no knob written {literal} in {file}");
    }
    Ok(found.into_iter().map(|(_, id)| id).collect())
}

/// When update `seq` was heard, in milliseconds after `from_ns`: the start of the callback that
/// took it, plus its block's frame, plus one device buffer. Each stamp is read before the next
/// update is sent (the engine keeps only the last 16), and one that never comes fails the gate.
fn update_heard_ms(graph: &Graph, seq: u64, from_ns: u64, rate: u32) -> Option<f64> {
    let t = Instant::now();
    loop {
        if let Some(a) = graph.applied(seq) {
            let taken = a.callback_ns as f64 + a.frame as f64 * 1e9 / rate as f64;
            return Some((taken - from_ns as f64) / 1e6 + period(rate) * 1e3);
        }
        if t.elapsed() > Duration::from_millis(50) {
            return None;
        }
        std::thread::sleep(Duration::from_micros(200));
    }
}

/// tracks/t02.ts with a counted busy loop ahead of its track: QuickJS spins about 1e8 empty
/// iterations a second, and `Date` is gone from the build, so a loop is the only way to make a
/// build slow.
/// with-plugins' `tracks/kick.ts` with the kick wrapped in a delay: a structural save.
fn delayed_kick(kick: &str) -> String {
    kick.replace("import { kick }", "import { delay } from \"std/fx\";\nimport { kick }")
        .replace("const out = kick(lane.notes(\"hits\"));", "const out = delay(kick(lane.notes(\"hits\")));")
}

fn slow(base: &str, n: u64) -> String {
    format!("for (let i = 0; i < {n}; i++) {{}}\n{base}")
}

/// G8 through the server: a knob drag's updates while the session compiles a slow save (WRITE-22,
/// RENDER-29). The drag is a lease on t01's channel level, updated every 10 ms, each heard and
/// timed from its call; meanwhile another thread saves tracks/t02.ts with a busy loop that makes
/// its compile last over a second, then saves it back. Updates during the compile must be heard as
/// fast as the others. Gestures begun one data revision and one code change behind resolve without
/// the session; one forced through it waits for the compile (the negative control).
pub fn previews(lab: &mut Lab, o: &Opts) -> Result<Outcome> {
    // Four knobs: the drag, two begins behind, and the negative control (`--quick`'s p3 has three).
    let reference = m2_session::reference::Reference { tracks: o.reference.tracks.max(4), ..o.reference };
    let dir = lab.reference(&reference)?;
    let song = lab.compile(&dir)?;
    let graph = open(&dir, &song)?;
    let mut out = Outcome::new("G8 previews while compiling", "WRITE-22, RENDER-29", &format!("server on {}", reference.name()));
    let knobs = file_knobs(lab, &dir, "song.ts", "-12")?;
    if knobs.len() < 4 {
        bail!("the reference project needs 4 tracks for G8");
    }
    let (token, r0) = session_and_revision(&graph)?;
    // A code revision (a comment in tracks/t04.ts), then a data revision (t03's lane).
    let t04 = std::fs::read_to_string(dir.join("tracks/t04.ts"))?;
    graph.write_file(None, "tracks/t04.ts", &t04, &format!("// a comment\n{t04}"))?;
    let (_, r1) = session_and_revision(&graph)?;
    let lane = std::fs::read_to_string(dir.join("data/t03/notes.json"))?;
    let LaneData::Notes(clips) = LaneData::read(&lane, &LaneKind::Notes).map_err(|e| anyhow!("{e:?}"))? else { bail!("not notes") };
    graph.write_notes(token, r1, "t03", "notes", &clips[..clips.len() - 1])?;
    let (_, r2) = session_and_revision(&graph)?;
    let lease = graph.begin(token, r2, &knobs[0], Default::default())?.lease;

    let base = std::fs::read_to_string(dir.join("tracks/t02.ts"))?;
    let saver = {
        let (g, base) = (graph.clone(), base.clone());
        std::thread::spawn(move || -> Result<Vec<f64>> {
            std::thread::sleep(Duration::from_millis(500));
            let mut took = vec![];
            for (from, to) in [(base.clone(), slow(&base, 150_000_000)), (slow(&base, 150_000_000), base)] {
                let t = Instant::now();
                g.write_file(None, "tracks/t02.ts", &from, &to)?;
                took.push(t.elapsed().as_secs_f64() * 1e3);
            }
            Ok(took)
        })
    };
    let (mut during, mut free, mut missing, mut begins) = (vec![], vec![], 0, vec![]);
    let mut slow_begin = None;
    let mut db = -12.0;
    for _ in 0..300 {
        let tick = Instant::now();
        db = if db > -6.0 { -12.0 } else { db + 0.1 };
        let busy = graph.busy().is_some();
        // From the call: the clock takes no lock, so a wait for one inside `update` is counted.
        let from = graph.now_ns();
        match graph.update(lease, None, (db * 10.0f64).round() / 10.0)? {
            Some(seq) => match update_heard_ms(&graph, seq, from, song.rate) {
                Some(ms) if busy => during.push(ms),
                Some(ms) => free.push(ms),
                None => missing += 1,
            },
            None => missing += 1,
        }
        if busy && begins.is_empty() {
            // One data revision behind (the ring), and one code change behind (the aliases).
            let t = Instant::now();
            graph.begin(token, r1, &knobs[1], Default::default())?;
            begins.push(t.elapsed().as_secs_f64() * 1e3);
            let t = Instant::now();
            graph.begin(token, r0, &knobs[2], Default::default())?;
            begins.push(t.elapsed().as_secs_f64() * 1e3);
            let (g, id) = (graph.clone(), knobs[3].clone());
            slow_begin = Some(std::thread::spawn(move || {
                let t = Instant::now();
                g.begin_via_actor(token, r2, &id, Default::default()).map(|_| t.elapsed().as_secs_f64() * 1e3)
            }));
        }
        if saver.is_finished() && !free.is_empty() && !during.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10).saturating_sub(tick.elapsed()));
    }
    let took = saver.join().map_err(|_| anyhow!("the saver panicked"))??;
    let compiles: Vec<f64> = graph.busy_spans().iter().map(|(a, b)| (b - a) as f64 / 1e6).filter(|ms| *ms > 100.0).collect();
    out.info("the saves (slow, then back), until they replied", format!("{:.0?} ms; the session's long requests {compiles:.0?} ms", took));
    let (d, f) = (Summary::of(&during), Summary::of(&free));
    if compiles.iter().all(|ms| *ms < 1000.0) {
        out.skip("updates during the compile", format!("void: no compile lasted a second ({compiles:.0?} ms)"));
    } else {
        out.check("every update's stamp arrived", missing == 0, format!("{missing} missing"));
        out.check("during a compile: heard within 50 ms", d.max <= 50.0 && d.n >= 20, d);
        out.check("during a compile: p95 within 2 ms of the free p95", d.p95 <= f.p95 + 2.0, format!("free: {f}"));
    }
    out.check(
        "begins one data revision and one code change behind return within 20 ms",
        begins.len() == 2 && begins.iter().all(|ms| *ms < 20.0),
        format!("{begins:.2?} ms"),
    );
    match slow_begin.map(|h| h.join()) {
        Some(Ok(Ok(ms))) => out.info("negative control: a begin through the session, during the compile", format!("{ms:.0} ms")),
        _ => out.info("negative control", "didn't run"),
    }
    match with_plugins(lab, "with-plugins-previews")? {
        Err(why) => out.skip("with-plugins", why),
        Ok((dir, graph)) => {
            let measured = previews_with_plugins(&mut out, lab, &dir, &graph);
            // Its plugin instances are destroyed before the program ends, whatever happened.
            graph.close();
            measured?;
        }
    }
    Ok(out)
}

/// `examples/with-plugins` copied into the lab and playing from its start in a fresh [`Graph`], or
/// why not: its plugins aren't on the CLAP search path (`CLAP_PATH`, never the user's own folder
/// in CI).
fn with_plugins(lab: &Lab, name: &str) -> Result<std::result::Result<(std::path::PathBuf, Arc<Graph>), String>> {
    let dir = lab.copy(&crate::fixture("examples/with-plugins"), name)?;
    let missing = lab.session(&dir)?.problems().iter().find(|d| d.code == "plugin.missing").map(|d| d.to_string());
    if let Some(d) = missing {
        return Ok(Err(format!("{d} (CLAP_PATH says where Surge XT and Airwindows are)")));
    }
    let graph = Arc::new(Graph::default());
    graph.open(&dir)?;
    graph.transport("play")?;
    std::thread::sleep(Duration::from_millis(500));
    Ok(Ok((dir, graph)))
}

/// Plugin instances this process has created so far (`m2_plugins::InProcess`).
fn created() -> u64 {
    m2_plugins::InProcess::counts().0
}

/// G8 on `examples/with-plugins`: a lease on the lead's channel level in song.ts, updated every
/// 10 ms, while a save to tracks/kick.ts compiles for over a second and changes the kick's
/// envelope, then is saved back. Both saves activate a new plan, which must keep Surge XT and
/// Airwindows (HOST-5): the drag's updates are heard as fast during the compile, and it aims at the
/// newest renderer after.
fn previews_with_plugins(out: &mut Outcome, lab: &Lab, dir: &Path, graph: &Arc<Graph>) -> Result<()> {
    let rate = graph.state().context("no project is open")?["sample_rate"].as_u64().context("no rate")? as u32;
    let knob = file_knobs(lab, dir, "song.ts", "-4")?.remove(0);
    let (token, revision) = session_and_revision(graph)?;
    let lease = graph.begin(token, revision, &knob, Default::default())?.lease;
    let generation = graph.leases().first().and_then(|l| l.generation);
    let made = created();
    let kick = std::fs::read_to_string(dir.join("tracks/kick.ts"))?;
    // Slow, and structural (the kick wrapped in a delay): from step 6 a save that changes only
    // values is written into the cells of what plays, with no new plan to keep instances in.
    let slow_kick = slow(&delayed_kick(&kick), 150_000_000);
    let saver = {
        let (g, kick) = (graph.clone(), kick.clone());
        std::thread::spawn(move || -> Result<Vec<f64>> {
            std::thread::sleep(Duration::from_millis(500));
            let mut took = vec![];
            for (from, to) in [(kick.clone(), slow_kick.clone()), (slow_kick, kick)] {
                let t = Instant::now();
                g.write_file(None, "tracks/kick.ts", &from, &to)?;
                took.push(t.elapsed().as_secs_f64() * 1e3);
            }
            Ok(took)
        })
    };
    let (mut during, mut free, mut missing) = (vec![], vec![], 0);
    let mut db = -4.0;
    for _ in 0..400 {
        let tick = Instant::now();
        db = if db > 2.0 { -4.0 } else { db + 0.1 };
        let busy = graph.busy().is_some();
        let from = graph.now_ns();
        match graph.update(lease, None, (db * 10.0f64).round() / 10.0)? {
            Some(seq) => match update_heard_ms(graph, seq, from, rate) {
                Some(ms) if busy => during.push(ms),
                Some(ms) => free.push(ms),
                None => missing += 1,
            },
            None => missing += 1,
        }
        if saver.is_finished() && !free.is_empty() && !during.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10).saturating_sub(tick.elapsed()));
    }
    let took = saver.join().map_err(|_| anyhow!("the saver panicked"))??;
    let compiles: Vec<f64> = graph.busy_spans().iter().map(|(a, b)| (b - a) as f64 / 1e6).filter(|ms| *ms > 100.0).collect();
    out.info(
        "with-plugins: the saves (slow, then back), until they replied",
        format!("{took:.0?} ms; the session's long requests {compiles:.0?} ms"),
    );
    let (d, f) = (Summary::of(&during), Summary::of(&free));
    if compiles.iter().all(|ms| *ms < 1000.0) {
        out.skip("with-plugins: updates during the compile", format!("void: no compile lasted a second ({compiles:.0?} ms)"));
    } else {
        out.check("with-plugins: every update's stamp arrived", missing == 0, format!("{missing} missing"));
        out.check("with-plugins: during a compile: heard within 50 ms", d.max <= 50.0 && d.n >= 20, d);
        out.check("with-plugins: during a compile: p95 within 2 ms of the free p95", d.p95 <= f.p95 + 2.0, format!("free: {f}"));
    }
    let now = graph.leases().into_iter().find(|l| l.lease == lease).and_then(|l| l.generation);
    out.check(
        "with-plugins: both saves activated plans that kept every plugin instance, and the drag aims at the newest",
        created() == made && now.is_some() && now != generation,
        format!("{} instances created; the drag's renderer {generation:?} then {now:?}", created() - made),
    );
    Ok(())
}

/// G9 through the server: a knob drag through an agent's saves (WRITE-22). On p16, a lease on t01's
/// channel level in song.ts is updated every 10 ms for 10 s, each stamp read as it comes, while an
/// "agent" saves tracks/t05.ts every 700 ms (a comment, an added `delay`, its removal), and once
/// inserts a comment at the top of song.ts and a line at its bottom. Every save compiles; the
/// structural ones activate a new renderer, and from step 6 a comment builds none (the delta from
/// what plays is empty). The drag must go on through all of them, aimed at whichever renderer
/// plays, and its commit must write song.ts alone, leaving every outside edit on disk. The same
/// runs on `examples/with-plugins` when its plugins are installed: the drag is the lead's channel
/// level in song.ts and the agent edits tracks/kick.ts (a comment, the kick wrapped in a delay, and
/// back), so the structural saves activate plans that keep the plugin instances (HOST-5). (A plugin parameter written in code, Airwindows' dry/wet, isn't a
/// knob the session offers yet, so it can't be dragged.)
pub fn drag(lab: &mut Lab, o: &Opts) -> Result<Outcome> {
    let mut out = Outcome::new("G9 drag through outside saves", "WRITE-22, WRITE-12, HOST-5", &format!("server on {}", o.reference.name()));
    let dir = lab.reference(&o.reference)?;
    let song = lab.compile(&dir)?;
    let graph = open(&dir, &song)?;
    let knob = file_knobs(lab, &dir, "song.ts", "-12")?.remove(0);
    let t05 = std::fs::read_to_string(dir.join("tracks/t05.ts"))?;
    let delayed = t05.replace("=> ", "=> delay(").replacen(");\n", "));\n", 1);
    let saves = [format!("// the agent was here\n{t05}"), delayed, t05.clone()];
    drag_through(
        &mut out,
        &o.reference.name(),
        &graph,
        &dir,
        (&knob, "song.ts", |v| format!("db({v})")),
        ("tracks/t05.ts", &saves),
        -12.0,
        6.0,
    )?;

    let (dir, graph) = match with_plugins(lab, "with-plugins-drag")? {
        Ok(open) => open,
        Err(why) => {
            out.skip("with-plugins", why);
            return Ok(out);
        }
    };
    let knob = file_knobs(lab, &dir, "song.ts", "-4")?.remove(0);
    let kick = std::fs::read_to_string(dir.join("tracks/kick.ts"))?;
    // A comment, then the kick wrapped in a delay (structural: a new renderer, which carries the
    // plugin instances), then back.
    let saves = [format!("// the agent was here\n{kick}"), delayed_kick(&kick), kick.clone()];
    let dragged = drag_through(
        &mut out,
        "with-plugins",
        &graph,
        &dir,
        (&knob, "song.ts", |v| format!("db({v})")),
        ("tracks/kick.ts", &saves),
        -4.0,
        3.0,
    );
    // Its plugin instances are destroyed before the program ends, whatever happened.
    graph.close();
    dragged?;
    Ok(out)
}

/// After a save: the renderer the drag aims at, and the one that plays.
type Aim = (Option<u64>, Option<u64>);

/// A knob on `song`'s files: its id, its file, and how its value is spelled there.
type Knob<'a> = (&'a str, &'a str, fn(f64) -> String);

/// G9's drag: lease `knob`, update it every 10 ms for 10 s from `from` over `span`, while an agent
/// saves `other`'s texts in turn every 700 ms, and once the knob's own file with a line above
/// everything and one below; then commit it.
#[allow(clippy::too_many_arguments)]
fn drag_through(
    out: &mut Outcome,
    on: &str,
    graph: &Arc<Graph>,
    dir: &Path,
    (knob, file, spelled): Knob,
    (other, saves): (&str, &[String]),
    from: f64,
    span: f64,
) -> Result<()> {
    let (token, revision) = session_and_revision(graph)?;
    let lease = graph.begin(token, revision, knob, Default::default())?.lease;
    let stop = Arc::new(AtomicBool::new(false));
    let agent = {
        let (g, d, stop, saves, other, own) =
            (graph.clone(), dir.to_path_buf(), stop.clone(), saves.to_vec(), other.to_string(), file.to_string());
        std::thread::spawn(move || -> Result<(Vec<f64>, Vec<Aim>)> {
            let (mut took, mut generations) = (vec![], vec![]);
            let mut n = 0;
            while !stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(700));
                // Once, in the dragged knob's own file: lines above and below its call.
                let (path, text) = if n == 6 {
                    let s = std::fs::read_to_string(d.join(&own))?;
                    (own.as_str(), format!("// the agent was here too\n{s}// and here\n"))
                } else {
                    (other.as_str(), saves[n % saves.len()].clone())
                };
                std::fs::write(d.join(path), text)?;
                let t = Instant::now();
                g.poll();
                took.push(t.elapsed().as_secs_f64() * 1e3);
                // Where the drag aims, and the renderer that plays once the engine took what was
                // sent (an activation lands at a later block than the lease moves to it).
                let aimed = g.leases().first().and_then(|l| l.generation);
                let _ = wait(Duration::from_millis(500), || g.generation() == aimed);
                generations.push((aimed, g.generation()));
                n += 1;
            }
            Ok((took, generations))
        })
    };
    let rate = graph.state().context("no project is open")?["sample_rate"].as_u64().context("no rate")? as u32;
    let (mut sent, mut failed, mut missing) = (0, vec![], 0);
    let mut value = from;
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(10) {
        let tick = Instant::now();
        value = if value >= from + span { from } else { ((value + span / 60.0) * 100.0).round() / 100.0 };
        let at = graph.now_ns();
        match graph.update(lease, None, value) {
            Ok(Some(seq)) => {
                sent += 1;
                if update_heard_ms(graph, seq, at, rate).is_none() {
                    missing += 1;
                }
            }
            Ok(None) => missing += 1,
            Err(e) => failed.push(format!("{e:#}")),
        }
        std::thread::sleep(Duration::from_millis(10).saturating_sub(tick.elapsed()));
    }
    stop.store(true, Ordering::Relaxed);
    let (took, generations) = agent.join().map_err(|_| anyhow!("the agent panicked"))??;
    out.info(format!("{on}: each save, taken in (compile, phase 1 and phase 2)"), Summary::of(&took));
    out.check(
        format!("{on}: no update failed, and every one was heard"),
        failed.is_empty() && missing == 0,
        format!(
            "{sent} heard, {missing} missing; {}",
            failed.first().map_or("no failure".into(), |f| format!("{} failed: {f}", failed.len()))
        ),
    );
    // From step 6 a save that changes nothing that plays (a comment) or only values builds no
    // renderer (the step 6 design's 5.5): the drag must aim at whichever plays, and follow each
    // new one the structural saves build.
    let aimed = generations.iter().filter(|(lease, plays)| lease.is_some() && lease == plays).count();
    let built = generations.windows(2).filter(|w| w[0].1 != w[1].1).count();
    out.check(
        format!("{on}: after each save the drag aims at the renderer that plays, new ones included"),
        aimed == generations.len() && built > 0,
        format!(
            "{} saves, aimed at what plays after {aimed}; {built} new renderers followed; (aimed, plays): {generations:?}",
            generations.len()
        ),
    );
    let alive = graph.leases().iter().any(|l| l.lease == lease);
    out.check(format!("{on}: the drag is alive at the end"), alive, if alive { "yes" } else { "it ended" });
    let before = std::fs::read_to_string(dir.join(other))?;
    let committed = graph.commit(lease, value, None)?;
    let written = match committed {
        m2_server::graph::Committed::Written(v) => v["file"].as_str().unwrap_or_default().to_string(),
        m2_server::graph::Committed::Pending { .. } => "pending".into(),
    };
    let own = std::fs::read_to_string(dir.join(file))?;
    let kept = own.contains("// the agent was here too") && std::fs::read_to_string(dir.join(other))? == before;
    out.check(
        format!("{on}: the commit writes {file} alone with the last value, and every outside edit stays"),
        written == file && kept && own.contains(&spelled(value)),
        format!("wrote {written}; outside edits kept: {kept}"),
    );
    Ok(())
}

/// A disk kept busy, as an export or a bounce keeps it: a thread writing a 1 GB file in 4 MB chunks,
/// each synced, round and round, until dropped.
struct Busy {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Busy {
    fn start(file: &Path) -> Result<Busy> {
        use std::io::{Seek, Write};
        let mut f = std::fs::File::create(file)?;
        let stop = Arc::new(AtomicBool::new(false));
        let s = stop.clone();
        let thread = std::thread::spawn(move || {
            let chunk = vec![0x5au8; 4 << 20];
            let mut written = 0u64;
            while !s.load(Ordering::Relaxed) {
                if f.write_all(&chunk).and_then(|_| f.sync_data()).is_err() {
                    return;
                }
                written += chunk.len() as u64;
                if written >= 1 << 30 {
                    written = 0;
                    let _ = f.rewind();
                }
            }
        });
        Ok(Busy { stop, thread: Some(thread) })
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// The durable sync a commit's barrier makes (`m2_os::fs::barrier`: `F_FULLFSYNC` on macOS,
/// `fsync` on Linux), timed `n` times on a small journal-sized file in the project's `.music2/tx/`.
fn barriers(dir: &Path, n: usize) -> Result<Vec<f64>> {
    use std::io::Write;
    let txd = dir.join(".music2/tx");
    std::fs::create_dir_all(&txd)?;
    let file = txd.join(".gate-barrier");
    let mut out = vec![];
    for i in 0..n {
        let mut f = std::fs::File::create(&file)?;
        f.write_all(format!("{{\"journal\": 1, \"n\": {i}}}").as_bytes())?;
        let t = Instant::now();
        m2_os::fs::barrier(&f)?;
        out.push(t.elapsed().as_secs_f64() * 1e3);
    }
    std::fs::remove_file(&file)?;
    Ok(out)
}

/// The disk G11 writes on.
#[derive(Clone, Copy, PartialEq)]
enum Disking {
    Idle,
    /// Another writer syncing 4 MB chunks.
    Busy,
    /// Idle, with 5,000 files in `assets/` that no lane names.
    Assets,
}

/// The t01 lane as the file has it now, with its first note moved a sixteenth, back and forth by
/// `i`'s parity.
fn moved(dir: &Path, i: usize) -> Result<Vec<NoteClip>> {
    let text = std::fs::read_to_string(dir.join("data/t01/notes.json"))?;
    let LaneData::Notes(mut clips) = LaneData::read(&text, &LaneKind::Notes).map_err(|e| anyhow!("{e:?}"))? else { bail!("not notes") };
    let c = clips.iter_mut().find(|c| !c.notes.is_empty()).context("no notes in t01")?;
    c.notes[0].start += if i.is_multiple_of(2) { 0.25 } else { -0.25 };
    Ok(clips)
}

/// G11 (PLANE-5, the data plane's 20 ms; the step 6 design's 19 calls it G10, which the range
/// render already is): piano-roll writes through the server, each one note of `t01` moved (a
/// `Clips` Tx, journaled and synced), from 1:00 playing. Each is heard as its commit's preview,
/// sent once its checks pass, while the session makes it durable: from the request to the first
/// block that plays it, and apart, until the commit is durable, and the barrier's durable sync
/// alone. Run on an idle disk, while another writer keeps it busy, and with 5,000 files in
/// `assets/`. On the idle and the busy disk also: a held arrow key's nudges, written as the piano
/// roll writes them (the newest waiting for the write before); and on the idle one, a write the
/// disk refuses, taken back. The watcher has a gate of its own ([`watcher`]).
pub fn tx_data(lab: &mut Lab, o: &Opts) -> Result<Outcome> {
    let dir = lab.reference(&o.reference)?;
    let song = lab.compile(&dir)?;
    let mut out = Outcome::new("G11 tx-data", "PLANE-5, PLANE-1, WRITE-15", &format!("server on {}", o.reference.name()));
    let writes = if o.edits <= 2 { 20 } else { 200 };
    let ms = |a: u64, b: u64| b.saturating_sub(a) as f64 / 1e6;
    for disking in [Disking::Idle, Disking::Busy, Disking::Assets] {
        let on = match disking {
            Disking::Idle => "idle disk: ",
            Disking::Busy => "with another writer syncing 4 MB chunks: ",
            Disking::Assets => "idle disk, 5,000 files in assets/: ",
        };
        if disking == Disking::Assets {
            let assets = dir.join("assets");
            std::fs::create_dir_all(&assets)?;
            for i in 0..5000 {
                std::fs::write(assets.join(format!("take-{i:04}.wav")), b"")?;
            }
        }
        let busy = if disking == Disking::Busy { Some(Busy::start(&lab.dir("busy.bin"))?) } else { None };
        let graph = open(&dir, &song)?;
        let first = graph.generation();
        let mut rng = Rng::new(o.seed);
        let (mut audible, mut heard, mut ready, mut waited, mut durable, mut after) = (vec![], vec![], vec![], vec![], vec![], vec![]);
        let (mut previewed, mut back, mut swapped) = (0, 0, 0);
        let mut last = vec![];
        for i in 0..writes {
            let clips = moved(&dir, i)?;
            let (token, revision) = session_and_revision(&graph)?;
            graph.write_notes(token, revision, "t01", "notes", &clips)?;
            last = clips;
            let t = graph.last_edit().context("the edit left no timing")?;
            audible.push(taken_ms(&graph, t, t.requested_ns, song.rate)?);
            heard.push(heard_ms(&graph, t, t.requested_ns, song.rate)?);
            waited.push(taken_ms(&graph, t, t.sent_ns, song.rate)?);
            ready.push(ms(t.requested_ns, t.ready_ns));
            durable.push(ms(t.requested_ns, t.compiled_ns));
            after.push(ms(t.sent_ns, t.compiled_ns));
            previewed += usize::from(t.previewed);
            back += usize::from(t.taken_back.is_some());
            swapped += usize::from(graph.generation() == first);
            std::thread::sleep(Duration::from_secs_f64(rng.range(0.05, 0.2)));
        }
        if disking != Disking::Assets {
            last = burst(&mut out, on, &graph, &dir, song.rate)?;
        }
        if disking == Disking::Idle {
            refused(&mut out, &graph, &dir, song.rate)?;
        }
        graph.close();
        let syncs = barriers(&dir, writes)?;
        drop(busy);
        // As the file has it: the canonical text of what was sent, its notes in order (FMT-8a).
        let text = std::fs::read_to_string(dir.join("data/t01/notes.json"))?;
        let mut sent = LaneData::Notes(last.clone());
        sent.sort();
        let on_disk = text == m2_song::lane::canonical(&sent);
        let left = std::fs::read_dir(dir.join(".music2/tx"))?
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "json") && !e.file_name().to_string_lossy().starts_with('.'))
            .count();
        let (a, r) = (Summary::of(&audible), Summary::of(&ready));
        out.info(format!("{on}the barrier's durable sync alone (F_FULLFSYNC on macOS, fsync on Linux)"), Summary::of(&syncs));
        out.check(format!("{on}the session's part before the preview (plan, check, pre-check), under 5 ms at p95"), r.p95 <= 5.0, r);
        out.info(format!("{on}then waiting for the next callback to take the preview (one device buffer at most)"), Summary::of(&waited));
        out.check(format!("{on}from the request to the first block that plays it (its preview), under 20 ms at p95"), a.p95 <= 20.0, a);
        out.info(format!("{on}to the speaker, one device buffer later"), Summary::of(&heard));
        out.info(format!("{on}from the request until the commit is durable and accepted (journal, sync, renames)"), Summary::of(&durable));
        out.info(format!("{on}of that, the durable write after the preview was sent"), Summary::of(&after));
        out.check(
            format!("{on}every write was a preview lane swap, never a new renderer, and none was taken back"),
            previewed == writes && swapped == writes && back == 0,
            format!("{previewed} previewed, {swapped} swapped in the same renderer, {back} taken back, of {writes}"),
        );
        out.check(
            format!("{on}the last write is on disk as sent, and no journal is left after close"),
            on_disk && left == 0,
            format!("on disk as sent: {on_disk}; {left} journals left"),
        );
    }
    Ok(out)
}

/// G11's nudges: the piano roll writes each nudge of a held arrow key (`NoteView`'s `nudge`), the
/// newest waiting while a write is on its way (`NoteView.send`), so each write waits for the one
/// before to be durable. A hand makes a shape every 16 to 30 ms for 3 s (as fast as a key
/// repeats), each stamped as it is made (timed from the gesture, as a seek is from its call); each
/// shape that is written is timed from its stamp to the block that plays the write carrying it.
/// Like the piano roll, the sender keeps the lane it shows and the revision the last write
/// answered, asking the server for nothing else.
fn burst(out: &mut Outcome, on: &str, graph: &Arc<Graph>, dir: &Path, rate: u32) -> Result<Vec<NoteClip>> {
    let (shapes, made) = std::sync::mpsc::channel::<(usize, u64)>();
    let hand = {
        let g = graph.clone();
        std::thread::spawn(move || {
            let mut rng = Rng::new(7);
            let t = Instant::now();
            let mut k = 0;
            while t.elapsed() < Duration::from_secs(3) {
                if shapes.send((k, g.now_ns())).is_err() {
                    break;
                }
                k += 1;
                std::thread::sleep(Duration::from_secs_f64(rng.range(0.016, 0.030)));
            }
            k
        })
    };
    let base = moved(dir, 0)?;
    let at = |clips: &[NoteClip]| clips.iter().find(|c| !c.notes.is_empty()).map(|c| c.notes[0].start);
    let (token, mut revision) = session_and_revision(graph)?;
    let (mut timed, mut last, mut now) = (vec![], vec![], at(&base).map(|s| s - 0.25));
    // Each write takes the newest shape made meanwhile; the ones it passed over are never sent.
    while let Ok(mut shape) = made.recv() {
        while let Ok(newer) = made.try_recv() {
            shape = newer;
        }
        let (k, stamp) = shape;
        let mut clips = base.clone();
        let c = clips.iter_mut().find(|c| !c.notes.is_empty()).context("no notes in t01")?;
        // Somewhere new each time, so every write changes the lane.
        let mut start = c.notes[0].start.floor() + 0.0625 * (k % 15 + 1) as f64;
        if Some(start) == now {
            start += 0.03125;
        }
        c.notes[0].start = start.min(c.length - c.notes[0].length);
        now = Some(c.notes[0].start);
        let answer = graph.write_notes(token, revision, "t01", "notes", &clips)?;
        revision = answer["revision"].as_u64().context("the write answered no revision")?;
        last = clips;
        let t = graph.last_edit().context("the edit left no timing")?;
        timed.push(taken_ms(graph, t, stamp, rate)?);
    }
    let made = hand.join().map_err(|_| anyhow!("the hand panicked"))?;
    out.info(
        format!("{on}a held arrow key's nudges, each written once the write before replied ({} of {made} written): from the nudge to the block that plays it", timed.len()),
        Summary::of(&timed),
    );
    Ok(last)
}

/// G11's refused write: `.music2/tx/` made read-only for one write, as a full or failing disk
/// refuses a commit's journal. It is heard as its preview, then taken back (PLANE-5): its error says
/// so, the swap back follows the preview, and nothing is accepted.
fn refused(out: &mut Outcome, graph: &Arc<Graph>, dir: &Path, rate: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let name = "idle disk: a write the disk refuses is taken back: its error says so, nothing is accepted, the accepted lane plays again";
    let tx = dir.join(".music2/tx");
    let (token, revision) = session_and_revision(graph)?;
    let before = std::fs::read_to_string(dir.join("data/t01/notes.json"))?;
    let generation = graph.generation();
    std::fs::set_permissions(&tx, std::fs::Permissions::from_mode(0o500))?;
    if std::fs::write(tx.join(".gate-probe"), b"").is_ok() {
        let _ = std::fs::remove_file(tx.join(".gate-probe"));
        std::fs::set_permissions(&tx, std::fs::Permissions::from_mode(0o755))?;
        out.skip(name, "`.music2/tx/` is writable when read-only (run as root)");
        return Ok(());
    }
    let written = graph.write_notes(token, revision, "t01", "notes", &moved(dir, 0)?);
    std::fs::set_permissions(&tx, std::fs::Permissions::from_mode(0o755))?;
    let t = graph.last_edit().context("the edit left no timing")?;
    let said = written.as_ref().err().map(|e| e.to_string()).unwrap_or_default();
    let (seq, back) = (t.seq.unwrap_or(0), t.taken_back.unwrap_or(0));
    let heard = t.previewed && back > seq;
    let span = if heard { taken_ms(graph, EditTiming { seq: Some(back), ..t }, 0, rate)? - taken_ms(graph, t, 0, rate)? } else { f64::NAN };
    let kept = session_and_revision(graph)?.1 == revision && std::fs::read_to_string(dir.join("data/t01/notes.json"))? == before;
    out.check(
        name,
        heard && said.ends_with("It played for a moment and was taken back.") && kept && graph.generation() == generation,
        format!("previewed {}, taken back {}; kept {kept}; \"{said}\"", t.previewed, t.taken_back.is_some()),
    );
    let one = if span == 0.0 { " (one block took both: none of it was rendered)" } else { "" };
    out.info("idle disk: from the refused write's preview to the block that swapped it back", format!("{span:.2} ms{one}"));
    Ok(())
}

// ---------------------------------------------------------------- G7e (step 7)

/// A driven player whose callback a thread of the gate's calls in real time, a device buffer at a
/// time, keeping what it played: the silent player with a tape, so a server gate can listen.
struct Tape {
    played: Arc<parking_lot::Mutex<Played>>,
    stop: Arc<AtomicBool>,
}

#[derive(Default)]
struct Played {
    frames: Vec<[f32; 2]>,
    /// Each callback: when it started ([`m2_play::now_ns`]) and where its frames start.
    calls: Vec<(u64, usize)>,
}

impl Tape {
    fn start(rate: u32, frames: usize) -> Result<(m2_play::Player, Tape)> {
        let (player, mut driver) = m2_play::Player::driven(rate);
        let (played, stop) = (Arc::new(parking_lot::Mutex::new(Played::default())), Arc::new(AtomicBool::new(false)));
        let (p, s) = (played.clone(), stop.clone());
        std::thread::Builder::new().name("m2-gates-tape".into()).spawn(move || {
            let mut buf = vec![0.0f32; frames * 2];
            let period = Duration::from_secs_f64(frames as f64 / rate as f64);
            let mut next = Instant::now();
            while !s.load(Ordering::Relaxed) {
                let ns = m2_play::now_ns();
                driver.fill(&mut buf, 2);
                {
                    let mut p = p.lock();
                    let at = p.frames.len();
                    p.calls.push((ns, at));
                    p.frames.extend(buf.chunks(2).map(|f| [f[0], f[1]]));
                }
                drop(driver.drain_retired());
                next += period;
                next = next.max(Instant::now().checked_sub(period).unwrap_or(next));
                std::thread::sleep(next.saturating_duration_since(Instant::now()));
            }
        })?;
        Ok((player, Tape { played, stop }))
    }

    /// Where on the tape the block an [`m2_play::Applied`] stamp names starts.
    fn at(&self, a: m2_play::Applied) -> Option<usize> {
        let p = self.played.lock();
        p.calls.iter().rev().find(|(ns, _)| *ns <= a.callback_ns).map(|(_, at)| at + a.frame as usize)
    }

    fn frames(&self) -> Vec<[f32; 2]> {
        self.played.lock().frames.clone()
    }
}

impl Drop for Tape {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// A 24-bit stereo WAV file's frames, as an export writes them.
fn read_wav(path: &Path) -> Result<Vec<[f32; 2]>> {
    let b = std::fs::read(path)?;
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        bail!("{} isn't a WAV file", path.display());
    }
    let (mut at, mut fmt, mut data) = (12, None, None);
    while at + 8 <= b.len() {
        let size = u32::from_le_bytes(b[at + 4..at + 8].try_into()?) as usize;
        let body = &b[at + 8..(at + 8 + size).min(b.len())];
        match &b[at..at + 4] {
            b"fmt " => fmt = Some((u16::from_le_bytes(body[2..4].try_into()?), u16::from_le_bytes(body[14..16].try_into()?))),
            b"data" => data = Some(body),
            _ => {}
        }
        at += 8 + size + (size & 1);
    }
    let (Some((2, 24)), Some(data)) = (fmt, data) else { bail!("{} isn't 24-bit stereo: {fmt:?}", path.display()) };
    let sample = |s: &[u8]| (i32::from_le_bytes([0, s[0], s[1], s[2]]) >> 8) as f32 / 8_388_608.0;
    Ok(data.as_chunks::<6>().0.iter().map(|f| [sample(&f[0..3]), sample(&f[3..6])]).collect())
}

/// G7e (step 7 design §12.5, RENDER-1, RENDER-6a, HOME-4, HOME-6): an export of
/// `examples/with-plugins` pauses playback, reuses its plugin instances, and playback resumes where
/// it was with their state intact. Through the server's own export (`m2_server::export::start`),
/// the song playing from 0:02 in real time on a player whose output the gate keeps (the silent
/// player's buffer), the export started about a second later, just after a kick's attack; skipped
/// without Surge XT and Airwindows on `CLAP_PATH`.
/// - Pauses: `paused: "exporting"` was published while it ran; the engine clock and the transport
///   didn't move meanwhile, and the output was silence. That the engine held before the export's
///   first host call is printed: the probe test proves the order.
/// - Reuses: both instances borrowed, none created, and the plan's instances before and after are
///   the same.
/// - State intact: each instance's saved state before the export equals the one after the first
///   resumed block. The gate's copy holds the lead's cutoff lane still, so the code's values are
///   the same at both saves. Surge's saved state is committed, as an autosave will, so the
///   instance holds its committed state and the export loads nothing: the usual path.
/// - Resumes: the engine resumes at the clock it held, within two device buffers of the instances
///   being given back, with no step at the fade out or the fade in over G6's bound: twice the
///   music's own largest step over the 200 ms on each side of the pause.
/// - The file: as long as `m2 render`'s within a second, its level in each second of the song
///   within 1 dB of it, its release tail and the lead between the kicks within 6 dB, and its
///   onsets within 10 ms (plugins aren't bit-exact from one instance to the next).
pub fn export(lab: &mut Lab, _o: &Opts) -> Result<Outcome> {
    let mut out = Outcome::new("G7e export", "RENDER-1, RENDER-6a, HOME-4, HOME-6", "server on examples/with-plugins, real time");
    let dir = lab.copy(&crate::fixture("examples/with-plugins"), "export")?;
    let missing = lab.session(&dir)?.problems().iter().find(|d| d.code == "plugin.missing").map(|d| d.to_string());
    if let Some(d) = missing {
        out.skip("every check", format!("{d} (CLAP_PATH says where Surge XT and Airwindows are)"));
        return Ok(out);
    }
    std::fs::write(
        dir.join("data/lead/cutoff.json"),
        r#"{"unit": "scalar", "clips": [{"id": "rt0wbb3b", "start": 0, "length": 16, "points": [[0, 0.6], [16, 0.6]]}]}"#,
    )?;
    let song = lab.compile(&dir)?;
    let frames: usize = std::env::var("M2_AUDIO_FRAMES").ok().and_then(|v| v.parse().ok()).unwrap_or(512);
    let (player, tape) = Tape::start(song.rate, frames)?;
    let state = Arc::new(m2_server::AppState::new(String::new(), Graph::with_player(player)));
    let graph = &state.graph;
    graph.open(&dir)?;
    // Stopped, the engine still runs (RENDER-6a): the code's values reach the instances at once.
    std::thread::sleep(Duration::from_millis(200));
    let handles = graph.plugin_handles();
    if handles.len() != 2 {
        bail!("with-plugins plays {} plugin instances, not 2", handles.len());
    }
    let host = m2_plugins::host();
    let save = |h| host.save_state(h).recv().map_err(|e| anyhow!("{e}"))?.map_err(|e| anyhow!("{e}")).map(|b| b.bytes);
    let before: Vec<Vec<u8>> = handles.iter().map(|&h| save(h)).collect::<Result<_>>()?;
    // A save leaves Surge holding bytes that aren't committed, so the export would load the committed
    // ones, which ends every voice in Surge XT. Committed as an autosave will commit them (HOME-6), it
    // holds its committed state, as an instance does in the app: the export loads nothing, the usual
    // path (step 7 design §10.5), with the notes playback left sounding still in it.
    let surge = handles
        .iter()
        .position(
            |&h| matches!(host.main_call(h, m2_plugins::MainCall::Info), Ok(m2_plugins::Reply::Info(i)) if i.found.id.contains("surge")),
        )
        .context("with-plugins plays no Surge XT")?;
    std::fs::write(dir.join("data/lead/lead.state"), &before[surge])?;
    graph.poll();
    graph.seek(song.beat(2 * song.rate as u64))?;
    graph.transport("play")?;
    std::thread::sleep(Duration::from_secs(1));
    // Started 30 to 50 ms after a kick's onset, the hold a callback or two later lands on its body,
    // past its attack: a kick's attack inside a fade would be the music's own step, not the fade's.
    // The kick (a std voice, kept by the engine) still sounds at the resume, so the fade in is
    // heard: the export resets Surge XT's voices, so the lead's chord doesn't sound there.
    wait(Duration::from_secs(2), || (0.06..0.1).contains(&graph.status().beat.fract())).context("the playhead never reached a kick")?;
    let made = created();

    // The export, with the playhead and the engine sampled every millisecond meanwhile.
    let path = dir.join("export.wav");
    let t = Instant::now();
    m2_server::export::start(&state, Some(path.clone()))?;
    let mut paused = vec![];
    while state.exports.running() {
        if t.elapsed() > Duration::from_secs(120) {
            bail!("the export took over two minutes");
        }
        if graph.activity().paused == Some("exporting") {
            paused.push((graph.engine_clock().unwrap_or(0), graph.status().seconds));
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let took = t.elapsed();
    let b = state.exports.borrowed().context("the export borrowed no instance")?;
    let applied = |seq: u64| wait(Duration::from_secs(5), || graph.applied(seq).is_some()).and_then(|()| graph.applied(seq));
    let (held, resumed) = (applied(b.hold_seq), applied(b.resume_seq));
    // The first resumed block, and a device buffer for its saves to see.
    std::thread::sleep(Duration::from_millis(50));
    let after: Vec<Vec<u8>> = handles.iter().map(|&h| save(h)).collect::<Result<_>>()?;
    std::thread::sleep(Duration::from_millis(300));
    let tape_frames = tape.frames();
    out.info(
        "the export, until it was done",
        format!("{:.0} ms; paused for {:.0} ms", took.as_secs_f64() * 1e3, b.given_back_ns.saturating_sub(b.held_ns) as f64 / 1e6),
    );

    // Pauses. The export's own thread stamps both times, the hold's once it saw the engine hold, so
    // their order is printed, not judged: the call order is proved by the probe plugin's log in
    // `an_export_borrows_and_gives_back_the_live_instances` (crates/server/src/graph/tests.rs).
    out.info(
        "pauses: the engine said it held before the export's first host call",
        format!("held at {} ns, first call {} ns later", b.held_ns, b.first_call_ns.saturating_sub(b.held_ns)),
    );
    out.check(
        "reuses: both plugin instances borrowed, none made anew for the export",
        b.held_ns > 0 && b.taken == 2 && b.anew == 0,
        format!("{} taken, {} anew", b.taken, b.anew),
    );
    let still = paused.windows(2).all(|w| w[0] == w[1]);
    out.check(
        "pauses: paused: \"exporting\" published, the engine clock and the transport still meanwhile",
        paused.len() >= 3 && still,
        format!("{} samples while paused, all the same: {still} ({:?})", paused.len(), paused.first()),
    );
    let (Some(held), Some(resumed)) = (held, resumed) else {
        out.check("resumes", false, "the hold or the resume was never applied");
        return Ok(out);
    };
    let (Some(hold_at), Some(resume_at)) = (tape.at(held), tape.at(resumed)) else {
        out.check("resumes", false, "the hold or the resume isn't on the tape");
        return Ok(out);
    };
    let fade = (0.005 * song.rate as f64).round() as usize;
    let quiet = span(&tape_frames, (hold_at + 2 * fade) as u64, resume_at as u64);
    out.check(
        "pauses: silence while held",
        !quiet.is_empty() && quiet.iter().all(|f| *f == [0.0; 2]),
        format!("{} frames between the fade out and the resume, peak {:.1} dBFS", quiet.len(), db(crate::signal::peak(quiet))),
    );

    // Reuses.
    let now = graph.plugin_handles();
    out.check(
        "reuses: the plan's instances before and after are the same, none created",
        now == handles && created() == made,
        format!("{handles:?} before, {now:?} after; {} created", created() - made),
    );
    // State intact.
    let same: Vec<bool> = before.iter().zip(&after).map(|(a, b)| a == b).collect();
    out.check(
        "state intact: each instance's saved state after the first resumed block is the one before",
        same.iter().all(|s| *s),
        format!("{same:?} ({} and {} bytes)", before[0].len(), before.last().map_or(0, Vec::len)),
    );

    // Resumes.
    let period_ns = (frames as f64 / song.rate as f64 * 1e9) as u64;
    let late = resumed.callback_ns.saturating_sub(b.given_back_ns);
    out.check(
        "resumes: at the engine clock it held, within two device buffers of the instances given back",
        resumed.clock == paused.first().map_or(held.clock, |p| p.0) && late <= 2 * period_ns,
        format!(
            "held at clock {}, resumed at {}; {:.1} ms after they were back (bound {:.1})",
            paused.first().map_or(0, |p| p.0),
            resumed.clock,
            late as f64 / 1e6,
            2.0 * period_ns as f64 / 1e6
        ),
    );
    // The music's own largest step on both sides of the pause: 200 ms before the fade out, and
    // 200 ms after the fade in.
    let around = song.rate as usize / 5;
    let own = max_step(span(&tape_frames, hold_at.saturating_sub(around) as u64, hold_at as u64))
        .0
        .max(max_step(span(&tape_frames, (resume_at + fade) as u64, (resume_at + fade + around) as u64)).0);
    let down = max_step(span(&tape_frames, hold_at.saturating_sub(1) as u64, (hold_at + fade + 1) as u64)).0;
    let up = max_step(span(&tape_frames, resume_at.saturating_sub(1) as u64, (resume_at + fade + 1) as u64)).0;
    let beat = paused.first().map_or(0.0, |p| song.beat((p.1 * song.rate as f64).round() as u64));
    // Sound right after the fade in, or a missing fade there would go unheard.
    let heard = db(crate::signal::rms(span(&tape_frames, (resume_at + fade) as u64, (resume_at + 2 * fade) as u64)));
    out.check(
        "resumes: no step at the fade out or the fade in over G6's bound",
        own > 0.0 && heard > -60.0 && down <= 2.0 * own && up <= 2.0 * own,
        format!(
            "{down:.4} out, {up:.4} in; twice the music's own over the 200 ms each side: {:.4} (held at beat {beat:.2}, {:.0} ms after a kick; {heard:.1} dBFS just after the fade in)",
            2.0 * own,
            beat.fract() * song.data.tempo().seconds_at(1.0) * 1e3
        ),
    );

    // The file, against `m2 render`'s (new instances).
    let (_, _, fresh, _) = graph.render(m2_editor::render::Length::Song)?;
    let fresh_path = dir.join("render.wav");
    fresh.write(&fresh_path, |_| {})?;
    let (got, want) = (read_wav(&path)?, read_wav(&fresh_path)?);
    let sec = song.rate as usize;
    out.check(
        "the file: as long as m2 render's, within a second, and the song's length at least",
        got.len().abs_diff(want.len()) <= sec && got.len() as u64 >= song.end(),
        format!(
            "{:.2} s against {:.2} s; the song is {:.2} s",
            got.len() as f64 / sec as f64,
            want.len() as f64 / sec as f64,
            song.end() as f64 / sec as f64
        ),
    );
    // Over the song's seconds: its release tail after them differs from one instance to the next by
    // more than the bound (Surge's random phases), so there it is judged more loosely, below.
    let level = |x: &[[f32; 2]]| crate::signal::levels(x, sec).into_iter().map(db).collect::<Vec<f64>>();
    let (lg, lw) = (level(&got), level(&want));
    let song_secs = (song.end() as usize).div_ceil(sec);
    let worst = |a: &[f64], b: &[f64], from: usize, to: usize| {
        let pairs = a.iter().zip(b).take(to).skip(from);
        pairs.filter(|(a, b)| a.max(**b) > -60.0).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max)
    };
    let w = worst(&lg, &lw, 0, song_secs);
    out.check(
        "the file: its level in each second of the song within 1 dB of m2 render's",
        w <= 1.0 && lg.len() >= song_secs,
        format!("worst {w:.2} dB over {song_secs} seconds"),
    );
    let (_, _, twin, _) = graph.render(m2_editor::render::Length::Song)?;
    let twin_path = dir.join("render-twin.wav");
    twin.write(&twin_path, |_| {})?;
    let lt = level(&read_wav(&twin_path)?);
    // Two m2 renders' tails differ by up to about 1.5 dB (Surge's random phases), so within 6 dB, as
    // the lead below: a note that playback left sounding rings on in the export 60 dB over it.
    let (tail, twins) = (worst(&lg, &lw, song_secs, usize::MAX), worst(&lt, &lw, song_secs, usize::MAX));
    out.check(
        "the file: its release tail, each second above -60 dBFS, within 6 dB of m2 render's",
        tail <= 6.0 && lg.len() > song_secs,
        format!(
            "worst {tail:.2} dB over {} seconds; two m2 renders' own tails differ by {twins:.2} dB",
            lg.len().saturating_sub(song_secs)
        ),
    );
    // The lead alone, where the kick that hits every beat is quiet: from 60% to 90% of each beat.
    // Two m2 renders differ there by up to about 3 dB (Surge's random phases); a lead that a
    // loaded state left silent, or with the wrong filter, misses by 6 to 50 dB.
    let lead = |x: &[[f32; 2]]| -> Vec<f64> {
        let beats = song.beat(song.end()).floor() as usize;
        (0..beats)
            .map(|b| {
                let (from, to) = (song.frame(b as f64 + 0.6), song.frame(b as f64 + 0.9));
                db(crate::signal::rms(span(x, from, to)))
            })
            .collect()
    };
    let (ag, aw, at) = (lead(&got), lead(&want), lead(&read_wav(&twin_path)?));
    let off = |a: &[f64], b: &[f64]| a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0, f64::max);
    out.check(
        "the file: the lead between the kicks, beat by beat, within 6 dB of m2 render's",
        ag.len() >= 16 && off(&ag, &aw) <= 6.0,
        format!("worst {:.2} dB over {} beats; two m2 renders' own differ by {:.2} dB", off(&ag, &aw), ag.len(), off(&at, &aw)),
    );
    // The kick's hits, over the lead's held chord.
    let onsets = |x: &[[f32; 2]]| crate::signal::rises(x, sec / 200, 10, 6.0, -40.0, sec / 10);
    let (og, ow) = (onsets(&got), onsets(&want));
    let near = og.len() == ow.len() && og.iter().zip(&ow).all(|(a, b)| a.abs_diff(*b) <= sec as u64 / 100);
    out.check(
        "the file: its onsets within 10 ms of m2 render's",
        near && og.len() >= 15,
        format!("{} onsets against {}; the first {:?}", og.len(), ow.len(), &og[..og.len().min(4)]),
    );
    graph.close();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    // G7a (design §12.1), ID-13: an added delay makes its one kernel new and nothing else. Slice 2's
    // names, which started the wrapped pluck again, also made its voice pool new: that must fail,
    // though it is still "at most 3" as the plan counts (a whole voice pool is one).
    #[test]
    fn a_pluck_made_new_fails_g7a() {
        let delay = Count { kernels: 1, ..Count::default() };
        assert!(made_new(&[delay, Count::default(), delay]).0);
        let restarted = Count { kernels: 1, polys: 1, ..Count::default() };
        let (ok, said) = made_new(&[delay, restarted]);
        assert!(!ok && restarted.total() <= 3, "{said}");
        assert!(said.ends_with("1 kernel; 1 kernel and 1 voice pool"), "{said}");
        assert!(!made_new(&[Count { kernels: 4, ..Count::default() }]).0, "more than 3");
        assert!(!made_new(&[]).0, "no edit");
    }
}
