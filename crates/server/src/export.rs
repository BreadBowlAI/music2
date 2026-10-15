//! Export Audio: the open song rendered offline to a WAV file, on a thread of its own.
//!
//! It renders the accepted revision, which is what the app plays. [`m2_editor::render`] does
//! the rendering, as it does for `m2 render`: both write the same file. A pure song renders with
//! a renderer of its own, so playback and every other request carry on meanwhile. A song whose
//! plugins play borrows their live instances (RENDER-1): on the export's thread, playback pauses
//! ([`Lent`]), each instance renders offline from its committed state, and playback resumes where
//! it was once the file is written. One export runs at a time. Clients hear how it goes as `export`
//! events, each naming the `song` and its project folder (`dir`), and whether playback is `paused`
//! for it: `progress` (0..1, at most ten a second), then `done` with the file's `path`, `seconds`,
//! `peak_db` and the `problems` it carries (a plugin that isn't installed, FMT-18a; one it couldn't
//! borrow, `plugin.not-borrowed`), or an `error`.

use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use m2_editor::render::{Length, Render};
use m2_play::Player;
use m2_plugins::{Blob, Found, RenderMode};
use m2_runtime::{BLOCK, PluginInstance};
use m2_song::Diagnostic;
use parking_lot::Mutex;
use serde_json::json;

use crate::AppState;
use crate::graph::Export;

/// Progress goes out at most this often.
const PROGRESS_EVERY: Duration = Duration::from_millis(100);
/// How long an export waits for playback to pause before it renders beside it.
const HOLD_WAIT: Duration = Duration::from_secs(2);
/// The songs folder's folder for exports. No song may take its name there.
pub const EXPORTS: &str = "Exports";

/// The export that is running, if one is.
#[derive(Default)]
pub struct Exports {
    running: AtomicBool,
    /// Set to stop it (the app is quitting).
    stop: Arc<AtomicBool>,
    /// The song being exported.
    song: Mutex<Option<String>>,
    /// How the last export that borrowed live instances went.
    borrowed: Mutex<Option<Borrowing>>,
}

/// How an export that borrowed the live plugin instances went, its times on the player's clock
/// ([`m2_play::now_ns`]): what G7e reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Borrowing {
    /// The hold's message, and when the engine said it held it (0: it never did).
    pub hold_seq: u64,
    pub held_ns: u64,
    /// Its first host call on a borrowed instance (0: none).
    pub first_call_ns: u64,
    /// When every instance was back, and the resume's message (0: none was sent).
    pub given_back_ns: u64,
    pub resume_seq: u64,
    /// Instances taken offline, and nodes that played a new one in the export.
    pub taken: usize,
    pub anew: usize,
}

impl Exports {
    /// How the last export that borrowed live instances went, once it has given them back.
    pub fn borrowed(&self) -> Option<Borrowing> {
        *self.borrowed.lock()
    }

    pub fn running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// The name of the song being exported, if one is.
    pub fn song(&self) -> Option<String> {
        self.song.lock().clone().filter(|_| self.running())
    }

    /// Stop the running export, if there is one, and wait up to `wait` for it to remove its
    /// unfinished file. True once nothing is running.
    pub fn stop(&self, wait: Duration) -> bool {
        self.stop.store(true, Ordering::SeqCst);
        let start = Instant::now();
        while self.running() && start.elapsed() < wait {
            std::thread::sleep(Duration::from_millis(10));
        }
        !self.running()
    }
}

/// Where exports go unless the user picks a file: the songs folder's `Exports`.
pub fn exports_dir() -> Result<PathBuf> {
    Ok(crate::project::songs_dir()?.join(EXPORTS))
}

/// `<dir>/<song>.wav`, `<song> 2.wav`, `<song> 3.wav`…
fn numbered(dir: &Path, song: &str) -> impl Iterator<Item = PathBuf> + use<> {
    let (dir, base) = (dir.to_path_buf(), crate::project::folder_name(song));
    (1..).map(move |n| dir.join(if n == 1 { format!("{base}.wav") } else { format!("{base} {n}.wav") }))
}

/// The first of `<dir>/<song>.wav`, `<song> 2.wav`… that isn't taken: an export never
/// replaces a file the user didn't pick.
pub fn free_path(dir: &Path, song: &str) -> PathBuf {
    // A link to nowhere is taken too: the export would replace it.
    numbered(dir, song).find(|p| p.symlink_metadata().is_err()).expect("some number is free")
}

/// Where the open song is exported unless the user picks a file (the save dialog starts
/// here). The `Exports` folder is made if it isn't there.
pub fn default_path(state: &AppState) -> Result<PathBuf> {
    new_file(&state.graph.name().context("no project is open")?)
}

fn new_file(song: &str) -> Result<PathBuf> {
    let dir = exports_dir()?;
    // Synced in the songs folder as it is made, as a new song's folder is (WRITE-1).
    m2_os::fs::make_dirs(&dir).with_context(|| format!("making {}", dir.display()))?;
    Ok(free_path(&dir, song))
}

/// Start exporting the open song to `path` (a file the user picked, replaced if it is there,
/// unless it is in the project's `assets/`, where it is only made new), or to a new file in the
/// songs folder's `Exports`. Returns where it goes; the render runs on. A new file's name is
/// settled when the render is done, so if another file took it meanwhile, the `done` event
/// names the next free one.
pub fn start(state: &Arc<AppState>, path: Option<PathBuf>) -> Result<PathBuf> {
    if state.exports.running.swap(true, Ordering::SeqCst) {
        bail!("A song is already being exported. Wait for it to finish, then try again.");
    }
    state.exports.stop.store(false, Ordering::SeqCst);
    let begun = (|| {
        let song = state.graph.name().context("no project is open")?;
        let (path, fresh) = match path {
            Some(p) => (picked(p)?, None),
            None => {
                let p = new_file(&song)?;
                let names = numbered(p.parent().context("an exports folder")?, &song);
                (p, Some(names))
            }
        };
        // From here the session prepares nothing for a song whose plugins play: until `lent` drops.
        let Export { song, dir, render, problems, lent } = state.graph.export(Length::Song)?;
        let render = render.stop_when(state.exports.stop.clone());
        *state.exports.song.lock() = Some(song.clone());
        let thread_state = state.clone();
        let thread_path = path.clone();
        std::thread::Builder::new().name("music2-export".into()).spawn(move || {
            let (state, mut render, mut carried, mut lent) = (thread_state, render, problems, lent);
            // Playback held and the live instances taken offline here, off the session's actor
            // (RENDER-1). One that can't be taken plays a new instance in the export.
            let taken = match lent.as_mut() {
                Some(l) => std::panic::catch_unwind(AssertUnwindSafe(|| l.take(&mut render, &mut carried)))
                    .unwrap_or_else(|_| Err(anyhow!("music2 hit a bug while pausing playback"))),
                None => Ok(()),
            };
            let paused = lent.as_ref().is_some_and(Lent::held);
            if lent.is_some() && !paused && taken.is_ok() {
                state.toast("Playback couldn't pause for the export, so it renders beside playback, with plugins of its own.");
            }
            #[cfg(test)]
            if let Some(l) = lent.as_ref() {
                l.inside();
            }
            let about = json!({"t": "export", "song": song, "dir": dir, "paused": paused});
            let mut last: Option<Instant> = None;
            let progress = |progress: f64| {
                if progress < 1.0 && last.is_none_or(|t| t.elapsed() >= PROGRESS_EVERY) {
                    last = Some(Instant::now());
                    let _ = state.events.send(with(&about, json!({"progress": progress})).to_string());
                }
            };
            let written = match taken {
                // A bug in the render mustn't leave every later export refused as "already running".
                Ok(()) => std::panic::catch_unwind(AssertUnwindSafe(|| match fresh {
                    Some(names) => render.write_new(names, progress),
                    // A file picked in the project's assets/ is made new, never written over (WRITE-1).
                    None => render.write_for(&dir, &thread_path, progress).map(|r| (thread_path.clone(), r)),
                }))
                .unwrap_or_else(|_| Err(anyhow!("music2 hit a bug while exporting"))),
                Err(e) => {
                    drop(render);
                    Err(e)
                }
            }
            // Running out of memory says so plainly, with the detail after.
            .map_err(crate::graph::plainly);
            // The instances back, and playback resumed, before it says it's done (RENDER-1).
            if let Some(l) = lent.as_mut() {
                l.give_back();
                *state.exports.borrowed.lock() = Some(l.record);
            }
            drop(lent);
            // Free before saying so: a client may export again as soon as it hears.
            state.exports.running.store(false, Ordering::SeqCst);
            let event = match written {
                // What the export carries: a plugin that isn't installed is silent in it (FMT-18a),
                // and one it couldn't borrow played anew (RENDER-1).
                Ok((path, r)) => {
                    json!({"done": true, "path": path, "seconds": r.seconds(), "peak_db": r.peak_db, "problems": carried})
                }
                Err(e) => json!({"error": format!("{e:#}"), "path": thread_path}),
            };
            state.announce(with(&about, event).to_string());
        })?;
        Ok(path)
    })();
    if begun.is_err() {
        state.exports.running.store(false, Ordering::SeqCst);
    }
    begun
}

/// One live plugin instance an export borrows (RENDER-1), and what makes a new one in its place.
pub(crate) struct Borrow {
    /// The export plan's node it plays.
    pub(crate) node: usize,
    pub(crate) inst: Arc<PluginInstance>,
    /// Its committed state, read.
    pub(crate) blob: Option<Arc<Blob>>,
    pub(crate) plugin: Found,
    /// How the user knows it: "Surge XT (lead)".
    pub(crate) who: String,
}

/// Tests: what an export does inside its window, and an export with no hold (the negative control
/// of the G7e probe test, `an_export_borrows_and_gives_back_the_live_instances`).
#[cfg(test)]
#[derive(Default)]
pub(crate) struct Hooks {
    pub(crate) no_hold: AtomicBool,
    pub(crate) inside: Mutex<Option<Box<dyn FnMut() + Send>>>,
    /// The session's job fails once it has made the export (`Actor::export`).
    pub(crate) panic_after: AtomicBool,
}

/// The live plugin instances an export borrows, and the engine held while it does (RENDER-1).
/// Made on the session's actor, which prepares nothing for the engine while it lives, and taken on
/// the export's thread ([`Lent::take`]). Given back when it drops, on every path (done, failed,
/// stopped, a panic, quitting, never taken): each instance it took offline is deactivated, set to
/// realtime and activated again, holding the committed blob, with the parameters its code owns
/// sent again at its next block (HOME-4); then the engine resumes where it held, and the session
/// plays what waited for the export. One that doesn't come back active is marked failed first, so
/// its node plays silence until the session replaces it (HOST-5).
pub struct Lent {
    player: Player,
    rate: u32,
    /// Still to be taken.
    borrows: Vec<Borrow>,
    /// Taken offline: given back when it drops.
    taken: Vec<Arc<PluginInstance>>,
    /// The engine was told to hold: told to resume when it drops.
    holding: bool,
    /// It said it held.
    held: bool,
    /// How it went.
    pub(crate) record: Borrowing,
    /// What tells the session it is over.
    pub(crate) done: Option<Box<dyn FnOnce() + Send>>,
    #[cfg(test)]
    pub(crate) hooks: Option<Arc<Hooks>>,
}

impl Lent {
    pub(crate) fn new(player: Player, borrows: Vec<Borrow>, rate: u32) -> Lent {
        Lent {
            player,
            rate,
            taken: Vec::with_capacity(borrows.len()),
            borrows,
            holding: false,
            held: false,
            record: Borrowing::default(),
            done: None,
            #[cfg(test)]
            hooks: None,
        }
    }

    /// Whether the engine said it held: playback is paused for the export.
    pub fn held(&self) -> bool {
        self.held
    }

    /// On the export's thread: hold the engine, then take each instance offline with its committed
    /// blob loaded (skipped where it holds it, HOME-6), for `render`. The engine says it is held only
    /// after the callback that held it returned, and only once it has taken this hold, so from then
    /// on nothing it plays calls these until the resume. An instance whose host calls fail is given
    /// back at once, and a new one plays its node in the export, as `problems` says
    /// (`plugin.not-borrowed`). When the engine doesn't hold within 2 s (its device stalled),
    /// playback resumes and every node plays a new instance. Fails only when a new one can't be made.
    pub(crate) fn take(&mut self, render: &mut Render, problems: &mut Vec<Diagnostic>) -> Result<()> {
        let borrows = std::mem::take(&mut self.borrows);
        if !self.hold() {
            // Nothing was taken: the session may play what waits now.
            self.give_back();
            for b in borrows {
                render.unlend(b.node, b.inst.another(&b.plugin, b.blob, self.rate)?);
                self.record.anew += 1;
            }
            return Ok(());
        }
        for b in borrows {
            // Given back on every path from here, its first call included.
            self.taken.push(b.inst.clone());
            if self.record.first_call_ns == 0 {
                self.record.first_call_ns = m2_play::now_ns();
            }
            self.record.taken += 1;
            if let Err(e) = offline(&b, self.rate) {
                let inst = self.taken.pop().expect("pushed above");
                self.give(&inst);
                (self.record.taken, self.record.anew) = (self.record.taken - 1, self.record.anew + 1);
                render.unlend(b.node, b.inst.another(&b.plugin, b.blob, self.rate)?);
                problems.push(Diagnostic::warning(
                    "plugin.not-borrowed",
                    format!(
                        "{} couldn't be lent to the export, so a new one played it there, from its saved settings. (In detail: {e:#})",
                        b.who
                    ),
                ));
            }
        }
        Ok(())
    }

    /// Tell the engine to hold, and wait up to 2 s for it to say it holds this hold.
    fn hold(&mut self) -> bool {
        #[cfg(test)]
        if self.hooks.as_ref().is_some_and(|h| h.no_hold.load(Ordering::SeqCst)) {
            // The negative control: the instances are taken while the engine plays on.
            return true;
        }
        let Ok(seq) = self.player.hold(true) else { return false };
        (self.holding, self.record.hold_seq) = (true, seq);
        let (t, mut applied) = (Instant::now(), false);
        // `held` alone could still say an earlier hold's, until this one is applied. Its stamp is
        // kept among the last few only, so once seen it is remembered.
        while !(applied && self.player.held()) {
            if t.elapsed() > HOLD_WAIT {
                return false;
            }
            applied |= self.player.applied(seq).is_some();
            if !applied || !self.player.held() {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        (self.held, self.record.held_ns) = (true, m2_play::now_ns());
        true
    }

    /// One instance back to realtime and active, holding its committed blob, its code-owned
    /// parameters sent again at its next block.
    fn give(&self, inst: &PluginInstance) {
        let (host, h) = (inst.host(), inst.handle());
        host.deactivate(h);
        let _ = host.set_render_mode(h, RenderMode::Realtime);
        // One that won't come back active is marked failed before playback resumes, so its node
        // plays silence and never calls it; the session replaces it from its committed state
        // (HOST-5) and says so.
        if let Err(e) = host.activate(h, self.rate, BLOCK as u32) {
            eprintln!("music2: a plugin didn't come back after the export: {e:#}");
            inst.kill();
        }
        inst.mark_reloaded();
    }

    /// Every instance taken back, playback resumed, and the session told. Once.
    pub(crate) fn give_back(&mut self) {
        let back = !self.taken.is_empty();
        for inst in std::mem::take(&mut self.taken) {
            self.give(&inst);
        }
        if back {
            self.record.given_back_ns = m2_play::now_ns();
        }
        if std::mem::take(&mut self.holding) {
            self.record.resume_seq = self.player.hold(false).unwrap_or(0);
        }
        self.held = false;
        if let Some(done) = self.done.take() {
            done();
        }
    }

    /// Tests: what runs inside the export's window, after the instances were taken.
    #[cfg(test)]
    fn inside(&self) {
        if let Some(f) = self.hooks.as_ref().and_then(|h| h.inside.lock().take()) {
            let mut f = f;
            f();
        }
    }
}

/// Take `b`'s instance offline for the export: deactivated, its committed blob loaded (skipped
/// where it holds it), set to offline mode and activated. Then one call on this thread, before the
/// render, ends every note playback left sounding: a choke and a NoteOff for every key, as a panic
/// sends them, since CLAP's `reset` doesn't end them in every plugin (Surge XT keeps its voices,
/// and its NoteOffs match the note's id, which the export doesn't know). The same call lets a
/// plugin that applies a loaded state at its next call, ending every voice and setting every
/// parameter (Surge XT does), apply it here and not over the export's first notes and the code's
/// values. The render resets it at its first call. Marked as one whose notes the export ended.
fn offline(b: &Borrow, rate: u32) -> Result<()> {
    let (host, h) = (b.inst.host(), b.inst.handle());
    host.deactivate(h);
    if let Some(blob) = &b.blob {
        host.load_state(h, blob.clone())?;
    }
    host.set_render_mode(h, RenderMode::Offline)?;
    host.activate(h, rate, BLOCK as u32)?;
    let mut end = m2_plugins::Events::with_capacity(2 * 128);
    for key in 0..128 {
        end.push(m2_plugins::Event::NoteChoke { time: 0, id: -1, key });
        end.push(m2_plugins::Event::NoteOff { time: 0, id: -1, key });
    }
    let (mut l, mut r, silence) = ([0.0f32; BLOCK], [0.0f32; BLOCK], [0.0f32; BLOCK]);
    let mut io =
        m2_plugins::SharedBuffers { inputs: [&silence, &silence], n_in: 0, outputs: [&mut l, &mut r], n_out: 2, frames: BLOCK as u32 };
    let mut out = m2_plugins::Events::with_capacity(64);
    if host.process(h, &mut io, &end, &mut out, &m2_plugins::Transport::default()) == m2_plugins::Status::Error {
        bail!("it reported a processing error before the export");
    }
    // Its notes ended here: they start again at the resume, however far the export gets (RENDER-1).
    b.inst.mark_lent();
    Ok(())
}

impl Drop for Lent {
    fn drop(&mut self) {
        self.give_back();
    }
}

/// `about`'s fields and `more`'s.
fn with(about: &serde_json::Value, more: serde_json::Value) -> serde_json::Value {
    let mut ev = about.clone();
    if let (Some(ev), serde_json::Value::Object(more)) = (ev.as_object_mut(), more) {
        ev.extend(more);
    }
    ev
}

/// A file the user picked: a full path, in a folder that is there, ending in `.wav`.
fn picked(path: PathBuf) -> Result<PathBuf> {
    if !path.is_absolute() {
        bail!("export to a full path, like /Users/you/Music/song.wav (not {})", path.display());
    }
    let path = if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav")) {
        path
    } else {
        let mut named = path.into_os_string();
        named.push(".wav");
        PathBuf::from(named)
    };
    let parent = path.parent().context("export to a file, not a whole disk")?;
    if !parent.is_dir() {
        bail!("there's no folder {} to export into", parent.display());
    }
    if path.is_dir() {
        bail!("{} is a folder", path.display());
    }
    Ok(path)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::atomic::AtomicU64;

    use serde_json::Value;
    use tokio::sync::broadcast;

    use super::*;
    use crate::graph::Graph;

    struct Temp(PathBuf);

    impl Temp {
        fn new() -> Temp {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let dir = std::env::temp_dir().join(format!("m2-export-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            std::fs::create_dir_all(&dir).unwrap();
            Temp(dir)
        }

        fn write(&self, name: &str, text: &str) {
            let p = self.0.join(name);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const SINE: &str =
        "import { hz, song } from '@m2/core';\nimport { osc } from 'std/dsp';\nexport default song(() => osc.sine(hz(440)));\n";

    /// A minute of a held chord: an export long enough to be still rendering when asked.
    const CHORD: &str = "import { song, track } from '@m2/core';\nimport { pad } from 'std/instruments';\n\
                         const p = track('pad', ({ lane }) => pad(lane.notes('chords')));\nexport default song(() => p.out);\n";

    /// The app's state with a project of `song.ts` (and the chord's clip) open.
    fn open(project: &Temp, song: &str) -> Arc<AppState> {
        project.write(
            "music2.toml",
            "[project]\nname = \"untitled\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.1\"\n",
        );
        project.write("song.ts", song);
        let notes: Vec<String> = [45, 52, 57, 60, 64].iter().map(|p| format!(r#"{{"pitch": {p}, "start": 0, "length": 120}}"#)).collect();
        project.write(
            "data/pad/chords.json",
            &format!(r#"{{"clips": [{{"id": "a", "start": 0, "length": 120, "notes": [{}]}}]}}"#, notes.join(", ")),
        );
        project.write("data/arrangement.json", r#"{"tempo": [{"beat": 0, "bpm": 120}]}"#);
        let state = Arc::new(AppState::new(String::new(), Graph::silent()));
        state.graph.open(&project.0).unwrap();
        state
    }

    /// Events until the export says it's finished; the last one is that.
    pub(crate) fn until_finished(events: &mut broadcast::Receiver<String>) -> Vec<Value> {
        let start = Instant::now();
        let mut seen = vec![];
        while start.elapsed() < Duration::from_secs(30) {
            match events.try_recv() {
                Ok(text) => {
                    let ev: Value = serde_json::from_str(&text).unwrap();
                    if ev["t"] != "export" {
                        continue;
                    }
                    let finished = ev.get("progress").is_none();
                    seen.push(ev);
                    if finished {
                        return seen;
                    }
                }
                Err(_) => std::thread::sleep(Duration::from_millis(5)),
            }
        }
        panic!("the export didn't finish: {seen:?}");
    }

    #[test]
    fn an_export_renders_on_its_own_while_a_second_is_refused() {
        let project = Temp::new();
        let state = open(&project, CHORD);
        let out = project.0.join("out.wav");
        let mut events = state.events.subscribe();
        assert_eq!(start(&state, Some(project.0.join("out"))).unwrap(), out, "`.wav` is added");
        assert!(state.exports.running());
        let e = start(&state, Some(project.0.join("other.wav"))).unwrap_err().to_string();
        assert!(e.contains("already being exported"), "{e}");
        // The project and playback aren't held while it renders.
        let t = Instant::now();
        assert!(state.graph.state().is_some());
        state.graph.transport("play").unwrap();
        state.graph.transport("stop").unwrap();
        assert!(t.elapsed() < Duration::from_millis(250), "answered in {:?}", t.elapsed());
        assert!(state.exports.running(), "still rendering");

        let seen = until_finished(&mut events);
        let done = seen.last().unwrap();
        assert_eq!((&done["done"], done["path"].as_str()), (&json!(true), out.to_str()));
        // Every event says which song it's about, even after another song is opened.
        let dir = project.0.canonicalize().unwrap();
        assert!(seen.iter().all(|e| e["song"] == "untitled" && e["dir"].as_str() == dir.to_str()), "{seen:?}");
        // RENDER-1 (step 7 design §12.5): a pure song exports beside playback, never paused for it.
        assert!(seen.iter().all(|e| e["paused"] == false) && state.exports.borrowed().is_none(), "{seen:?}");
        let seconds = done["seconds"].as_f64().unwrap();
        assert!(seconds > 60.5 && seconds < 70.0, "the minute, and the chord's release: {seconds}");
        let progress: Vec<f64> = seen[..seen.len() - 1].iter().map(|e| e["progress"].as_f64().unwrap()).collect();
        assert!(
            !progress.is_empty() && progress.windows(2).all(|w| w[0] <= w[1]) && progress.iter().all(|p| (0.0..1.0).contains(p)),
            "{progress:?}"
        );
        let wav = hound::WavReader::open(&out).unwrap();
        assert_eq!((wav.spec().channels, wav.spec().sample_rate, wav.spec().bits_per_sample), (2, 48_000, 24));
        assert_eq!(wav.duration() as f64, seconds * 48_000.0);
        assert!(!state.exports.running());
        assert!(!std::fs::read_dir(&project.0).unwrap().any(|e| e.unwrap().file_name().to_string_lossy().ends_with(".part")));
    }

    #[test]
    fn stopping_an_export_leaves_no_file_and_frees_the_next() {
        let project = Temp::new();
        let state = open(&project, CHORD);
        let out = project.0.join("chord.wav");
        let mut events = state.events.subscribe();
        start(&state, Some(out.clone())).unwrap();
        assert_eq!(state.exports.song().as_deref(), Some("untitled"));
        assert!(state.exports.stop(Duration::from_secs(10)), "it stopped");
        assert!(!state.exports.running() && state.exports.song().is_none());
        let end = until_finished(&mut events).pop().unwrap();
        assert!(end["error"].as_str().unwrap().contains("stopped"), "{end}");
        let names: Vec<String> = std::fs::read_dir(&project.0).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into()).collect();
        assert!(!out.exists() && !names.iter().any(|n| n.ends_with(".part")), "{names:?}");
        // Stopping doesn't stick: the next export runs to the end.
        start(&state, Some(out.clone())).unwrap();
        assert_eq!(until_finished(&mut events).pop().unwrap()["done"], true);
        assert!(out.is_file());
        // With nothing running, stop answers at once.
        assert!(state.exports.stop(Duration::ZERO));
    }

    #[test]
    fn a_song_that_never_falls_silent_exports_ten_seconds_past_its_end_and_replaces_a_picked_file() {
        let project = Temp::new();
        let state = open(&project, SINE);
        let out = project.0.join("sine.wav");
        std::fs::write(&out, "an older export").unwrap();
        let mut events = state.events.subscribe();
        start(&state, Some(out.clone())).unwrap();
        let done = until_finished(&mut events).pop().unwrap();
        // Its chord lane isn't used: the song ends at beat 120 (a minute) all the same.
        assert_eq!(done["seconds"], 70.0, "{done}");
        assert!(done["peak_db"].as_f64().unwrap().abs() < 0.01, "a full-scale sine: {}", done["peak_db"]);
        assert_eq!(hound::WavReader::open(&out).unwrap().duration(), 70 * 48_000);
    }

    #[test]
    fn an_export_that_cant_start_says_why_and_leaves_the_next_free() {
        let project = Temp::new();
        let state = open(&project, SINE);
        let folder = project.0.join("folder.wav");
        std::fs::create_dir_all(&folder).unwrap();
        for (path, why) in [
            (PathBuf::from("relative.wav"), "full path"),
            (PathBuf::from("/no/such/folder/song.wav"), "no folder /no/such/folder"),
            (folder, "is a folder"),
        ] {
            let e = start(&state, Some(path)).unwrap_err().to_string();
            assert!(e.contains(why), "{e}");
            assert!(!state.exports.running());
        }
        state.graph.close();
        assert!(start(&state, Some(project.0.join("x.wav"))).unwrap_err().to_string().contains("no project is open"));
        assert!(!state.exports.running());
    }

    #[test]
    fn the_default_file_is_named_after_the_song_and_never_one_that_is_there() {
        // As in a home folder's Music/music2/Exports.
        let home = Temp::new();
        let exports = home.0.join("Music/music2/Exports");
        std::fs::create_dir_all(&exports).unwrap();
        assert_eq!(free_path(&exports, "first-song"), exports.join("first-song.wav"));
        std::fs::write(exports.join("first-song.wav"), "").unwrap();
        assert_eq!(free_path(&exports, "first-song"), exports.join("first-song 2.wav"));
        std::fs::write(exports.join("first-song 2.wav"), "").unwrap();
        std::os::unix::fs::symlink(home.0.join("nowhere"), exports.join("first-song 3.wav")).unwrap();
        assert_eq!(free_path(&exports, "first-song"), exports.join("first-song 4.wav"));
        // A name is a file name, never a path.
        assert_eq!(free_path(&exports, "AC/DC"), exports.join("AC-DC.wav"));
        assert_eq!(free_path(&exports, "../up"), exports.join("-up.wav"));
        assert_eq!(free_path(&exports, ""), exports.join("My song.wav"));
    }
}
