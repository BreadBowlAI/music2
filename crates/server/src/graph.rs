//! The open song project in the app, shown as a graph.
//!
//! One editing session ([`m2_session::Session`]) and one player ([`m2_play::Player`]), whose live
//! engine runs while the project is open, playing or not: stop ends the notes and lets tails ring
//! out (RENDER-6a). Knob drags preview straight into the playing renderer; releasing commits the
//! value to code through the session. A value commit doesn't rebuild the renderer: the committed
//! graph equals the playing one with that value, which the preview already set.
//!
//! Every other accepted revision reaches the engine the way its files say (PLANE-1,
//! [`m2_session::plane`]): lane data that changed alone is swapped into the running renderer
//! (RENDER-26), and anything else is built into a new renderer that takes over where the engine
//! is, crossfaded, with no jump back (RENDER-25a). Nothing replays the song (RENDER-8). Edits made
//! elsewhere (a text editor, an agent, git) are picked up by [`Graph::poll`] and take the same way.
//!
//! The transport (play, stop, seek, the loop, and where playback is) never waits for the project,
//! which edits hold while they compile (RENDER-29).
//!
//! Every change is announced with what needs attention in the revision it leaves (see
//! [`Graph::news`]): its problems with their fixes (DIAG-1, DIAG-4), and why the draft doesn't
//! compile.
//!
//! The player runs the engine at the project's `sample_rate` (RENDER-31), so every frame here
//! counts at that rate.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result, anyhow, bail};
use m2_ir::{Flat, Kind, Origin};
use m2_play::Player;
use m2_runtime::transport::frame_of;
use m2_runtime::{Caps, Cmd, LaneRef, LaneSwap, PackageSet, Renderer, Transport, audio, notes_data};
use m2_session::render::{Length, Render};
use m2_session::{EditError, Edited, NoteClip, Session, plane};
use m2_song::{DataSet, Snapshot, TempoMap};
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::{Value, json};

use crate::files;

struct Open {
    token: u64,
    session: Session,
    /// The accepted revision the player is playing.
    playing: u64,
    /// What the engine plays, to tell lane data from structure. None until the project is loaded
    /// into the player.
    plays: Option<Plays>,
    previews: Vec<Preview>,
    /// Decoded assets and clip renders, shared by every renderer built for this project, so an
    /// activation never decodes or warps them again (RENDER-27).
    audio: audio::Cache,
    timing: Arc<Mutex<Option<Timing>>>,
    /// The engine message the last sync sent the new revision in (a swap or an activation).
    sent: Option<u64>,
}

/// When the last edit or seek reached the engine, for gate measurements (step 3a's design, §9.2):
/// when it was asked for and when the session had done its part, on the player's clock
/// ([`Player::now_ns`]), and the message that carried it, whose [`Graph::applied`] stamp says when
/// the engine took it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EditTiming {
    pub requested_ns: u64,
    /// The session's work done (its compile included): what the edit budgets count from.
    pub compiled_ns: u64,
    /// The engine's message queued (data prepared or a renderer built).
    pub sent_ns: u64,
    /// None when nothing was sent, or a fresh engine took it (an open, a rate change).
    pub seq: Option<u64>,
}

/// The renderer the engine plays: what it was built from, and how `SwapLane` reaches its lanes.
struct Plays {
    /// The player it was loaded into ([`Device::epoch`]).
    epoch: u64,
    generation: u64,
    rate: u32,
    flat: Flat,
    snapshot: Snapshot,
    data: DataSet,
    slots: Vec<LaneRef>,
    /// The room each notes lane's scheduler has: data that needs more is activated instead.
    caps: Vec<Option<Caps>>,
}

/// What the transport needs of the open project, kept apart from it so seek, the loop and status
/// never wait for an edit (RENDER-29).
struct Timing {
    /// The accepted revision's: what a loop is checked against, and a seek's target timed in.
    tempo: TempoMap,
    rate: u32,
    /// The loop's beats, kept while it is off, and whether it is on.
    range: Option<(f64, f64)>,
    on: bool,
}

/// The player, and which one it is.
#[derive(Clone)]
struct Device {
    player: Player,
    /// Counts the players started: a project loaded into an earlier one is loaded again.
    epoch: u64,
    /// Where to load the project into it: the beat the player it replaced had got to.
    resume: f64,
}

struct Preview {
    /// The control's id as the gesture named it when it started.
    id: String,
    gesture: String,
    /// Baselines belong to the renderer revision, never a newly adopted source.
    revision: u64,
    origin: Origin,
    baseline: f64,
}

/// Whether the player is playing, and where: in seconds, and in beats (0 with no project open);
/// and the loop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct Status {
    pub playing: bool,
    pub seconds: f64,
    pub beat: f64,
    /// None until a loop is set; its range is kept while it is off.
    #[serde(rename = "loop")]
    pub looping: Option<Loop>,
}

/// A loop between two beats of the accepted revision, and whether playback loops there.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Loop {
    pub start: f64,
    pub end: f64,
    pub on: bool,
}

#[derive(Default)]
pub struct Graph {
    open: Mutex<Option<Open>>,
    player: Mutex<Option<Device>>,
    /// Kept apart from `open`, which edits hold while they compile, so the transport never waits
    /// for them. None with no project open.
    timing: Arc<Mutex<Option<Timing>>>,
    /// Players started so far, and the beat the last one that stopped responding had got to.
    players: AtomicU64,
    resume: Mutex<f64>,
    next_session: AtomicU64,
    /// The code editor's files with edits not yet saved, as it last said, and the project
    /// (its token) it said so for: the app asks before closing or replacing that project.
    unsaved: Mutex<(u64, Vec<String>)>,
    /// What every change is announced with (see [`Graph::news`]). Apart from `open` like
    /// `timing`, so announcing a change never waits for an edit.
    news: Mutex<Value>,
    last_edit: Mutex<Option<EditTiming>>,
}

fn packages() -> Result<PackageSet> {
    let mut p = PackageSet::default();
    p.load(&crate::resources::std_dir()).context("loading the std package")?;
    Ok(p)
}

/// A renderer for the accepted revision at the project's rate (RENDER-31), built without playing
/// anything: the engine puts it where it is (RENDER-8). Assets and clip renders come from `audio`,
/// which then keeps only what this one uses (the renderer it replaces holds its own until it is
/// retired).
fn build(session: &Session, audio: &mut audio::Cache) -> Result<Renderer> {
    let rev = session.accepted();
    let r = Renderer::build_with(&rev.compiled.flat, &rev.compiled.data, &session.packages, rev.project.sample_rate(), audio)?;
    audio.prune();
    Ok(r)
}

/// Let go of a player that stopped responding, off the caller's thread: telling its engine to stop
/// waits for room, and its device may never call back again. Its device thread is left behind.
fn retire(player: Player) {
    let _ = std::thread::Builder::new().name("m2-play-retire".into()).spawn(move || {
        let _ = player.unload();
    });
}

/// Refuse a loop the engine couldn't play: one that isn't from a beat to a later one, at or after
/// 0, or is shorter than two blocks or 5 ms at the project's rate (RENDER-9a).
fn check_loop(t: &Timing, start: f64, end: f64) -> Result<()> {
    if !(start.is_finite() && end.is_finite() && 0.0 <= start && start < end) {
        bail!("a loop goes from a beat to a later one, from beat 0 on (not {start} to {end})");
    }
    let rate = t.rate as f64;
    let frames = frame_of(&t.tempo, end, rate).saturating_sub(frame_of(&t.tempo, start, rate));
    let least = Transport::min_loop(rate);
    if frames < least {
        bail!(
            "a loop must last at least {:.1} ms; beats {start} to {end} last {:.1} ms",
            least as f64 / rate * 1e3,
            frames as f64 / rate * 1e3
        );
    }
    Ok(())
}

impl Open {
    fn check_token(&self, token: u64) -> Result<()> {
        if token != self.token {
            return Err(anyhow!("the open project changed; refresh the graph before editing"));
        }
        Ok(())
    }

    fn restore_previews(&mut self, player: &Player) -> Result<()> {
        for preview in self.previews.drain(..) {
            if preview.revision == self.playing {
                player.commit_const(preview.origin, preview.baseline)?;
            }
        }
        Ok(())
    }

    /// Gestures name controls by the revision they started in, so ownership is
    /// decided by the value in the playing graph, not by the name.
    fn check_owner(&self, origin: &Origin, gesture: &str) -> Result<()> {
        let accepted = self.session.accepted().number;
        if self.previews.iter().any(|p| p.revision == accepted && p.origin.same_value(origin) && p.gesture != gesture) {
            return Err(anyhow!("this control is being edited by another gesture"));
        }
        Ok(())
    }

    fn cancel(&mut self, player: &Player, id: &str, gesture: &str) -> Result<()> {
        if let Some(index) = self.previews.iter().position(|p| p.id == id && p.gesture == gesture) {
            let preview = self.previews.remove(index);
            if preview.revision == self.playing {
                player.commit_const(preview.origin, preview.baseline)?;
            }
        }
        Ok(())
    }

    /// Whether the project plays in `device`'s player.
    fn loaded(&self, device: &Device) -> bool {
        self.plays.as_ref().is_some_and(|p| p.epoch == device.epoch)
    }

    /// What the engine plays once `r`, built from the accepted revision, plays in `epoch`'s player.
    fn describe(&self, r: &Renderer, epoch: u64) -> Plays {
        let rev = self.session.accepted();
        let slots = r.lane_slots().to_vec();
        Plays {
            epoch,
            generation: r.generation(),
            rate: r.sample_rate(),
            flat: rev.compiled.flat.clone(),
            snapshot: rev.snapshot.clone(),
            data: rev.compiled.data.clone(),
            caps: (0..slots.len()).map(|s| r.caps(s)).collect(),
            slots,
        }
    }

    /// The renderer plays the accepted revision now (a value commit set what differed): a lane
    /// edit after this still finds the graphs equal, and swaps.
    fn adopt(&mut self) {
        let rev = self.session.accepted();
        if let Some(p) = &mut self.plays {
            (p.flat, p.snapshot, p.data) = (rev.compiled.flat.clone(), rev.snapshot.clone(), rev.compiled.data.clone());
        }
    }

    /// Load the accepted revision into `device`'s player as a fresh engine, stopped at `beat`
    /// and looping over `looping`. Previews belonged to what played before.
    fn load(&mut self, device: &Device, beat: f64, looping: Option<(f64, f64)>) -> Result<()> {
        let r = build(&self.session, &mut self.audio)?;
        let plays = self.describe(&r, device.epoch);
        device.player.open(r)?;
        if beat > 0.0 {
            device.player.seek(beat)?;
        }
        if looping.is_some() {
            device.player.set_loop(looping)?;
        }
        self.plays = Some(plays);
        self.previews.clear();
        Ok(())
    }

    /// The swaps that turn what `p` plays into the accepted revision: only its lane data changed
    /// (PLANE-1, RENDER-26), and each notes lane fits the scheduler playing it. None: a new
    /// renderer must play it.
    fn swaps(&self, p: &Plays) -> Option<Vec<(u32, LaneSwap)>> {
        let rev = self.session.accepted();
        let rate = rev.project.sample_rate();
        let old = plane::Plays { flat: &p.flat, snapshot: &p.snapshot, data: &p.data, rate: p.rate };
        let new = plane::Plays { flat: &rev.compiled.flat, snapshot: &rev.snapshot, data: &rev.compiled.data, rate };
        let data = &rev.compiled.data;
        let mut out = vec![];
        for addr in plane::lane_swaps(old, new)? {
            let (module, lane) = (addr.module.as_str(), addr.lane.as_str());
            // A lane no node reads plays nothing, before or after.
            let Some(slot) = p.slots.iter().position(|s| s.module == module && s.lane == lane) else { continue };
            let swap = match p.slots[slot].kind {
                Kind::Events(_) => {
                    let notes = notes_data(data, module, lane, rate as f64);
                    // More notes at once than its scheduler holds: a renderer sized for them.
                    if !p.caps[slot]?.fits(notes.needs()) {
                        return None;
                    }
                    LaneSwap::Notes(Arc::new(notes))
                }
                Kind::Control(_) => LaneSwap::Control(Arc::new(data.control(module, lane))),
                // Its clips must be rendered first: a new renderer.
                Kind::Audio(_) => return None,
            };
            out.push((slot as u32, swap));
        }
        Some(out)
    }

    /// Play the accepted revision, if the player doesn't already. Returns whether what plays
    /// changed.
    fn sync(&mut self, device: &Device) -> Result<bool> {
        let number = self.session.accepted().number;
        if self.loaded(device) && number == self.playing {
            return Ok(false);
        }
        let rate = self.session.accepted().project.sample_rate();
        let player = &device.player;
        enum Way {
            Swap(u64, Vec<(u32, LaneSwap)>),
            Activate,
            Replace,
            Load,
        }
        let way = match self.plays.as_ref().filter(|p| p.epoch == device.epoch) {
            Some(p) if p.rate != rate => Way::Replace,
            Some(p) => self.swaps(p).map_or(Way::Activate, |s| Way::Swap(p.generation, s)),
            None => Way::Load,
        };
        match way {
            Way::Swap(generation, swaps) => {
                for (slot, data) in swaps {
                    self.sent = Some(player.swap_lane(generation, slot, data)?);
                }
                self.adopt();
                // The same renderer and origins: previews go on, in the new revision.
                for p in self.previews.iter_mut().filter(|p| p.revision == self.playing) {
                    p.revision = number;
                }
            }
            Way::Activate => {
                let r = build(&self.session, &mut self.audio)?;
                let plays = self.describe(&r, device.epoch);
                // Crossfaded where the engine is, with no jump back (RENDER-25a).
                self.sent = Some(player.activate(r)?.1);
                self.plays = Some(plays);
                self.previews.clear();
            }
            Way::Replace => {
                let r = build(&self.session, &mut self.audio)?;
                let plays = self.describe(&r, device.epoch);
                // A manifest edit changed the rate: a fresh engine at the same beat (RENDER-31).
                player.replace(r)?;
                self.plays = Some(plays);
                self.previews.clear();
            }
            Way::Load => {
                // A new player (the one before stopped responding): where it was, looping as it did.
                let looping = self.timing.lock().as_ref().and_then(|t| t.range.filter(|_| t.on));
                self.load(device, device.resume, looping)?;
            }
        }
        self.playing = number;
        if let Some(t) = self.timing.lock().as_mut() {
            (t.tempo, t.rate) = (self.session.accepted().tempo(), rate);
        }
        Ok(true)
    }

    fn restore(&mut self, device: &Device) -> Result<()> {
        self.restore_previews(&device.player)?;
        self.sync(device)?;
        Ok(())
    }

    fn reject(&mut self, device: &Device, id: &str, gesture: &str) -> Result<()> {
        if self.playing != self.session.accepted().number || !self.session.draft_errors.is_empty() {
            self.restore(device)
        } else {
            self.cancel(&device.player, id, gesture)
        }
    }
}

impl Graph {
    /// One whose player plays into nothing, for tests.
    #[cfg(test)]
    pub(crate) fn silent() -> Graph {
        let device = Device { player: Player::silent().expect("a silent player"), epoch: 0, resume: 0.0 };
        Graph { player: Mutex::new(Some(device)), ..Graph::default() }
    }

    /// The player, started if there is none. One that stopped responding (a send timed out, the
    /// device failed or went quiet) or whose engine panicked is let go: this call fails, the next
    /// starts a new one, and the next sync loads the project into it where it was.
    fn device(&self) -> Result<Device> {
        let mut d = self.player.lock();
        if let Some(old) = d.take_if(|d| d.player.crashed() || d.player.stalled()) {
            let st = old.player.status();
            *self.resume.lock() = st.pending.unwrap_or(st.beats);
            retire(old.player);
            bail!("the audio device stopped responding; it starts again with the next action");
        }
        if d.is_none() {
            // Unit tests never open an audio device.
            let player = if cfg!(test) { Player::silent()? } else { Player::start()? };
            let epoch = self.players.fetch_add(1, Ordering::Relaxed) + 1;
            *d = Some(Device { player, epoch, resume: std::mem::take(&mut *self.resume.lock()) });
        }
        Ok(d.clone().expect("started"))
    }

    fn player(&self) -> Result<Player> {
        Ok(self.device()?.player)
    }

    pub fn open(&self, dir: &Path) -> Result<Value> {
        let session = Session::open(dir, packages()?).map_err(|d| {
            // Files in the project by their names in it (`song.ts:22:5`), so the reason fits a toast.
            let root = format!("{}/", dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf()).display());
            anyhow!(d.iter().map(|d| d.to_string().replace(&root, "")).collect::<Vec<_>>().join("\n"))
        })?;
        let mut open = self.open.lock();
        let device = self.device()?;
        let token = self.next_session.fetch_add(1, Ordering::Relaxed) + 1;
        let mut o = Open {
            token,
            session,
            playing: 0,
            plays: None,
            previews: vec![],
            audio: audio::Cache::default(),
            timing: self.timing.clone(),
            sent: None,
        };
        // A fresh engine, stopped at the start, not looping; what played before fades out.
        o.load(&device, 0.0, None)?;
        let rev = o.session.accepted();
        o.playing = rev.number;
        *self.timing.lock() = Some(Timing { tempo: rev.tempo(), rate: rev.project.sample_rate(), range: None, on: false });
        // Read while still holding the project: another open meanwhile can't answer for this one.
        let state = self.snapshot(&o);
        self.publish(Some(&o));
        *open = Some(o);
        Ok(state)
    }

    pub fn close(&self) {
        let mut open = self.open.lock();
        if let Some(d) = self.player.lock().clone() {
            let _ = d.player.unload();
        }
        *open = None;
        *self.timing.lock() = None;
        self.publish(None);
    }

    /// What every change to the open project is announced with: its session, the accepted
    /// revision, the problems that revision has with their fixes (DIAG-4), and why the draft
    /// doesn't compile. Empty with no project open.
    pub fn news(&self) -> Value {
        self.news.lock().clone()
    }

    /// Keep what [`Graph::news`] says up to date with `o`. Called wherever the open project
    /// may have changed, while it is still held.
    fn publish(&self, o: Option<&Open>) {
        *self.news.lock() = match o {
            Some(o) => json!({
                "session": o.token,
                "revision": o.session.accepted().number,
                // What needs attention though the song plays, with the fixes (DIAG-1, DIAG-4).
                "problems": o.session.problems(),
                "diagnostics": files::diagnostics(&o.session),
            }),
            None => json!({}),
        };
    }

    /// The view, the timeline, the project's name and folder, and the transport.
    pub fn state(&self) -> Option<Value> {
        let open = self.open.lock();
        Some(self.snapshot(open.as_ref()?))
    }

    fn snapshot(&self, o: &Open) -> Value {
        let rev = o.session.accepted();
        let status = self.status();
        json!({
            "session": o.token,
            "name": rev.project.manifest.project.name,
            "dir": rev.project.dir,
            "view": o.session.view(),
            "timeline": o.session.timeline(),
            "can_undo": o.session.can_undo(),
            "can_redo": o.session.can_redo(),
            "effects": o.session.effects(),
            "diagnostics": files::diagnostics(&o.session),
            "problems": rev.compiled.problems,
            "sample_rate": rev.project.sample_rate(),
            "playing": status.playing,
            "seconds": status.seconds,
            "beat": status.beat,
            "loop": status.looping,
        })
    }

    /// The open project's name, from its manifest (None with no project open).
    pub fn name(&self) -> Option<String> {
        Some(self.open.lock().as_ref()?.session.accepted().project.manifest.project.name.clone())
    }

    /// The open project's folder.
    pub fn dir(&self) -> Option<PathBuf> {
        Some(self.open.lock().as_ref()?.session.accepted().project.dir.clone())
    }

    /// An offline render of the accepted revision, which is what plays, with the song's name
    /// and folder, and what the export must carry: a plugin that isn't installed plays silence or
    /// passes its input through (FMT-18a). Only building it holds the project: the caller renders
    /// it on a thread of its own.
    pub fn render(&self, length: Length) -> Result<(String, PathBuf, Render, Vec<m2_song::Diagnostic>)> {
        let open = self.open.lock();
        let o = open.as_ref().context("no project is open")?;
        let rev = o.session.accepted();
        // Offline renders run at the project's rate (RENDER-31a).
        let r = Render::new(&rev.compiled.flat, &rev.compiled.data, &o.session.packages, rev.project.sample_rate(), 0.0, length)?;
        let carried = rev.compiled.problems.iter().filter(|d| d.code == "plugin.missing").cloned().collect();
        Ok((rev.project.manifest.project.name.clone(), rev.project.dir.clone(), r, carried))
    }

    /// Where playback is, as the audio thread last said: a seek it hasn't taken yet is where it
    /// goes. Never waits for the project (RENDER-29).
    pub fn status(&self) -> Status {
        let Some(d) = self.player.lock().clone() else { return Status::default() };
        let timing = self.timing.lock();
        let Some(t) = timing.as_ref() else { return Status::default() };
        let s = d.player.status();
        let (beat, seconds) = match s.pending {
            Some(beat) => (beat, t.tempo.seconds_at(beat)),
            None => (s.beats, s.seconds),
        };
        // A tempo change that makes the loop too short turns it off (RENDER-9a).
        let looping = t.range.map(|(start, end)| Loop { start, end, on: t.on && !d.player.loop_refused() });
        Status { playing: s.playing, seconds, beat, looping }
    }

    /// Move playback to `beat` (at least 0), playing or not: a jump of the transport at the next
    /// block, never a rebuild (RENDER-9). Previews go on, and it never waits for an edit
    /// (RENDER-29). Notes already sounding at `beat` aren't chased (RENDER-11).
    pub fn seek(&self, beat: f64) -> Result<()> {
        if !beat.is_finite() {
            bail!("can't seek to beat {beat}");
        }
        if self.timing.lock().is_none() {
            bail!("no project is open");
        }
        let player = self.player()?;
        let requested_ns = player.now_ns();
        let seq = player.seek(beat.max(0.0))?;
        *self.last_edit.lock() = Some(EditTiming { requested_ns, compiled_ns: requested_ns, sent_ns: player.now_ns(), seq: Some(seq) });
        Ok(())
    }

    /// When the last edit made here or the last seek reached the engine (see [`EditTiming`]).
    pub fn last_edit(&self) -> Option<EditTiming> {
        *self.last_edit.lock()
    }

    /// When the engine took message `seq`, while it is among the last few it took: the stamp of
    /// the audio callback that rendered it, on [`Graph::now_ns`]'s clock.
    pub fn applied(&self, seq: u64) -> Option<m2_play::Applied> {
        self.player.lock().clone()?.player.applied(seq)
    }

    /// The clock [`EditTiming`] and the engine's stamps count on, in nanoseconds.
    pub fn now_ns(&self) -> Option<u64> {
        Some(self.player.lock().clone()?.player.now_ns())
    }

    /// Set the loop: its range in beats of the accepted revision, which is kept while it is off,
    /// and whether playback loops there. Setting a range turns it on unless `on` says otherwise.
    /// Applied at the next block, and never waits for an edit (RENDER-29).
    pub fn set_loop(&self, range: Option<(f64, f64)>, on: Option<bool>) -> Result<Status> {
        let player = self.player()?;
        let (range, on) = {
            let timing = self.timing.lock();
            let t = timing.as_ref().context("no project is open")?;
            let on = on.unwrap_or(range.is_some() || t.on);
            if let Some((start, end)) = range {
                check_loop(t, start, end)?;
            }
            let range = range.or(t.range);
            match range {
                Some((start, end)) if on => check_loop(t, start, end)?,
                None if on => bail!("set the loop's range first"),
                _ => {}
            }
            (range, on)
        };
        // Not while holding the timing, which status reads: a send may wait for room.
        player.transport(Cmd::Loop(range.filter(|_| on)))?;
        if let Some(t) = self.timing.lock().as_mut() {
            (t.range, t.on) = (range, on);
        }
        Ok(self.status())
    }

    /// Change a value in the playing audio only (while a knob is dragged).
    pub fn preview(&self, token: u64, revision: u64, id: &str, gesture: &str, value: f64) -> Result<()> {
        let mut open = self.open.lock();
        let o = open.as_mut().context("no project is open")?;
        o.check_token(token)?;
        let device = self.device()?;
        // The session carries the control forward across saves that didn't touch it.
        match o.session.preview(revision, id, value) {
            Ok(audition) => {
                o.check_owner(&audition.origin, gesture)?;
                o.sync(&device)?;
                if !o.previews.iter().any(|p| p.origin.same_value(&audition.origin)) {
                    o.previews.push(Preview {
                        id: id.into(),
                        gesture: gesture.into(),
                        revision: o.playing,
                        origin: audition.origin.clone(),
                        baseline: audition.current,
                    });
                }
                device.player.set_const(audition.origin, audition.value);
                Ok(())
            }
            Err(error) => {
                o.reject(&device, id, gesture)?;
                Err(anyhow!("{error}"))
            }
        }
    }

    /// Close only the preview owned by this gesture. Old callbacks are harmless:
    /// a preview from an older renderer was already discarded with it.
    pub fn cancel_preview(&self, token: u64, id: &str, gesture: &str) -> Result<()> {
        let mut open = self.open.lock();
        let Some(o) = open.as_mut() else { return Ok(()) };
        if token != o.token {
            return Ok(());
        }
        o.cancel(&self.player()?, id, gesture)
    }

    /// Commit a value to code. Errors carry what went wrong (stale, conflict, …).
    pub fn set(&self, token: u64, revision: u64, id: &str, gesture: &str, value: f64, expect: Option<f64>) -> Result<Value> {
        let mut open = self.open.lock();
        let o = open.as_mut().context("no project is open")?;
        o.check_token(token)?;
        let device = self.device()?;
        o.session.refresh();
        if let Ok(target) = o.session.preview(revision, id, value) {
            o.check_owner(&target.origin, gesture)?;
        }
        let before = o.session.accepted().number;
        // The session carries the edit forward from `revision` if the code changed
        // without touching the control, and refuses it as stale otherwise.
        let set = o.session.set(revision, id, value, expect);
        self.publish(Some(o));
        match set {
            Ok(c) => {
                // Only a verified single value edit can update the existing renderer.
                // set() may refresh again and adopt unrelated source changes first.
                let single_edit = c.before != c.after && c.revision == before + 1;
                if o.playing == before && o.loaded(&device) && (single_edit || c.revision == before) {
                    o.previews.retain(|p| p.gesture != gesture);
                    o.restore_previews(&device.player)?;
                    device.player.commit_const(c.origin.clone(), c.value)?;
                    o.playing = c.revision;
                    o.adopt();
                } else {
                    o.restore(&device)?;
                }
                Ok(json!({"file": c.file, "before": c.before, "after": c.after, "revision": c.revision}))
            }
            Err(e) => {
                o.reject(&device, id, gesture)?;
                Err(anyhow!("{e}"))
            }
        }
    }

    /// Connect a named signal to a control's input, in code. The graph changes shape, so a new
    /// renderer takes over where playback is.
    pub fn connect(&self, token: u64, revision: u64, from: &str, to: &str) -> Result<Value> {
        self.restructure(token, |session| session.connect(revision, from, to))
    }

    /// Disconnect a cable, writing the input's declared default in code.
    pub fn disconnect(&self, token: u64, revision: u64, id: &str) -> Result<Value> {
        self.restructure(token, |session| session.disconnect(revision, id))
    }

    /// Put one of the packages' effects after a call's audio, in code.
    pub fn add_effect(&self, token: u64, revision: u64, call: &str, module: &str, name: &str) -> Result<Value> {
        self.restructure(token, |session| session.add_effect(revision, call, module, name))
    }

    /// Take an effect out, leaving what it was applied to.
    pub fn remove_effect(&self, token: u64, revision: u64, call: &str) -> Result<Value> {
        self.restructure(token, |session| session.remove_effect(revision, call))
    }

    /// Rewrite a notes lane's clips (the piano roll, or clips made, moved or deleted in the
    /// timeline). Only the lane's data changes, so it is swapped into the playing renderer
    /// (RENDER-26): heard at once, with nothing cut.
    pub fn write_notes(&self, token: u64, revision: u64, module: &str, lane: &str, clips: &[NoteClip]) -> Result<Value> {
        self.restructure(token, |session| session.write_notes(revision, module, lane, clips))
    }

    /// Move data no lane or plugin reads from `from` to `to`, project paths: a lane file, a
    /// track's folder or a plugin state (see [`Session::relink`]).
    pub fn relink(&self, token: u64, revision: u64, from: &str, to: &str) -> Result<Value> {
        self.restructure(token, |session| session.relink(revision, Path::new(from), Path::new(to)))
    }

    /// Delete data no lane or plugin reads (see [`Session::delete_data`]); undo puts it back.
    pub fn delete_data(&self, token: u64, revision: u64, file: &str) -> Result<Value> {
        self.restructure(token, |session| session.delete_data(revision, Path::new(file)))
    }

    /// Apply a fix a problem offers, named by its code and edit as the problem carried them
    /// (see [`Session::fix`]). Undo takes it back.
    pub fn fix(&self, token: u64, revision: u64, code: &str, edit: &Value) -> Result<Value> {
        self.restructure(token, |session| session.fix(revision, code, edit))
    }

    /// Undo the last edit made through this session (knob or cable).
    pub fn undo(&self, token: u64) -> Result<Value> {
        self.restructure(token, Session::undo)
    }

    pub fn redo(&self, token: u64) -> Result<Value> {
        self.restructure(token, Session::redo)
    }

    fn restructure(&self, token: u64, edit: impl FnOnce(&mut Session) -> Result<Edited, EditError>) -> Result<Value> {
        let mut open = self.open.lock();
        let o = open.as_mut().context("no project is open")?;
        o.check_token(token)?;
        let device = self.device()?;
        let requested_ns = device.player.now_ns();
        let result = edit(&mut o.session);
        let compiled_ns = device.player.now_ns();
        self.publish(Some(o));
        // Whatever happened, play the accepted revision; previews belong to the old one.
        o.sent = None;
        o.restore(&device)?;
        *self.last_edit.lock() = Some(EditTiming { requested_ns, compiled_ns, sent_ns: device.player.now_ns(), seq: o.sent });
        let e = result.map_err(|e| anyhow!("{e}"))?;
        Ok(json!({"file": e.file, "before": e.before, "after": e.after, "revision": e.revision}))
    }

    /// Play, stop, toggle between them, or go back to the start. Stop ends the notes, and the
    /// engine plays on so tails decay (RENDER-6a). Never waits for an edit (RENDER-29).
    pub fn transport(&self, action: &str) -> Result<()> {
        let player = self.player()?;
        let cmd = match action {
            "play" => Cmd::Play,
            "stop" => Cmd::Stop,
            "toggle" if player.status().playing => Cmd::Stop,
            "toggle" => Cmd::Play,
            "restart" => return self.seek(0.0),
            other => bail!("unknown transport action {other}"),
        };
        player.transport(cmd)?;
        Ok(())
    }

    /// Pick up edits made outside the app. Returns whether clients should refresh.
    pub fn poll(&self) -> bool {
        let mut open = self.open.lock();
        let Some(o) = open.as_mut() else { return false };
        let changed = o.session.refresh();
        if changed {
            self.publish(Some(o));
        }
        let device = match self.device() {
            Ok(d) => d,
            Err(e) => {
                eprintln!("music2: {e:#}");
                return changed;
            }
        };
        if !o.session.draft_errors.is_empty()
            && let Err(e) = o.restore_previews(&device.player)
        {
            eprintln!("music2: couldn't restore the preview: {e:#}");
        }
        let synced = match o.sync(&device) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("music2: couldn't play the accepted revision: {e:#}");
                false
            }
        };
        changed || synced
    }

    /// The project's TypeScript files, for the code editor (see [`files::list`]).
    pub fn files(&self) -> Result<Value> {
        let open = self.open.lock();
        files::list(&open.as_ref().context("no project is open")?.session)
    }

    /// The SDK and package sources the code editor checks imports against.
    pub fn types(&self) -> Result<Value> {
        let open = self.open.lock();
        Ok(files::types(&open.as_ref().context("no project is open")?.session.packages))
    }

    /// Save a file from the code editor if it still says `base` (else a [`files::Conflict`]),
    /// and adopt it at once, so the state returned shows it. Code that doesn't compile is
    /// saved too: the last revision that did plays on, and the diagnostics say why. A save
    /// that compiles is an edit made here, which undo takes back like any other.
    pub fn write_file(&self, token: Option<u64>, path: &str, base: &str, text: &str) -> Result<Value> {
        {
            let mut open = self.open.lock();
            let o = open.as_mut().context("no project is open")?;
            if let Some(token) = token {
                o.check_token(token)?;
            }
            let before = o.session.accepted().snapshot.files.get(Path::new(path)).cloned();
            files::write(&o.session.accepted().project.dir, path, base, text)?;
            o.session.saved(Path::new(path), before);
            self.publish(Some(o));
        }
        self.poll();
        self.state().context("no project is open")
    }

    /// The code editor of project `token` says which of its files have unsaved edits.
    pub fn set_unsaved(&self, token: u64, files: Vec<String>) -> Result<()> {
        self.open.lock().as_ref().context("no project is open")?.check_token(token)?;
        *self.unsaved.lock() = (token, files);
        Ok(())
    }

    /// The open project's files with edits in the code editor not yet saved, as it last said.
    pub fn unsaved(&self) -> Vec<String> {
        let Some(token) = self.open.lock().as_ref().map(|o| o.token) else { return vec![] };
        let unsaved = self.unsaved.lock();
        if unsaved.0 == token { unsaved.1.clone() } else { vec![] }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str =
        "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.1\"\n";

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let dir = std::env::temp_dir().join(format!("m2-graph-server-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("music2.toml"), MANIFEST).unwrap();
            let fixture = Self(dir);
            fixture.write("export default song(() => osc.sine(hz(800)));\n");
            fixture
        }

        fn write(&self, body: &str) {
            std::fs::write(
                self.0.join("song.ts"),
                format!("import {{ hz, song }} from '@m2/core';\nimport {{ osc }} from 'std/dsp';\n{body}"),
            )
            .unwrap();
        }

        fn open(&self) -> Graph {
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

    #[test]
    fn a_project_that_does_not_compile_says_where_without_its_folder() {
        let fixture = Fixture::new();
        fixture.write("export default song(() => osc.sine(hz(800)) oops);\n");
        let graph = Graph::silent();
        let e = format!("{:#}", graph.open(&fixture.0).unwrap_err());
        assert!(e.contains("song.ts:3:"), "{e}");
        assert!(!e.contains(&*fixture.0.canonicalize().unwrap().to_string_lossy()), "{e}");
    }

    fn control(graph: &Graph) -> (u64, u64, String) {
        let open = graph.open.lock();
        let o = open.as_ref().unwrap();
        let control = o.session.controls().into_iter().find(|c| c.value == 800.0).unwrap();
        (o.token, o.session.accepted().number, control.id)
    }

    #[test]
    fn rejected_edits_and_previews_close_their_preview() {
        let fixture = Fixture::new();
        let graph = fixture.open();
        let (token, revision, id) = control(&graph);
        graph.preview(token, revision, &id, "drag", 900.0).unwrap();
        assert_eq!(graph.open.lock().as_ref().unwrap().previews[0].baseline, 800.0);
        assert!(graph.set(token, revision, &id, "drag", 950.0, Some(100.0)).is_err());
        assert!(graph.open.lock().as_ref().unwrap().previews.is_empty());
        assert_eq!(control(&graph), (token, revision, id.clone()));

        graph.preview(token, revision, &id, "drag-2", 900.0).unwrap();
        assert!(graph.preview(token, revision, &id, "drag-2", f64::NAN).is_err());
        assert!(graph.open.lock().as_ref().unwrap().previews.is_empty());
    }

    #[test]
    fn cancellation_and_conflicts_respect_gesture_ownership() {
        let fixture = Fixture::new();
        let graph = fixture.open();
        let (token, revision, id) = control(&graph);
        graph.preview(token, revision, &id, "owner", 900.0).unwrap();
        graph.cancel_preview(token, &id, "old-gesture").unwrap();
        assert!(graph.preview(token, revision, &id, "other", 1000.0).is_err());
        assert!(graph.set(token, revision, &id, "other", 1000.0, Some(800.0)).is_err());
        assert_eq!(graph.open.lock().as_ref().unwrap().previews.len(), 1);
        assert_eq!(graph.open.lock().as_ref().unwrap().previews[0].gesture, "owner");
        graph.cancel_preview(token, &id, "owner").unwrap();
        assert!(graph.open.lock().as_ref().unwrap().previews.is_empty());
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
        assert_eq!(graph.open.lock().as_ref().unwrap().previews.len(), 1);
        assert_eq!(control(&graph), (current, revision, id));
    }

    #[test]
    fn a_commit_carried_past_inserted_code_rebuilds_instead_of_patching_an_old_origin() {
        let fixture = Fixture::new();
        let graph = fixture.open();
        let (token, revision, id) = control(&graph);
        graph.preview(token, revision, &id, "drag", 900.0).unwrap();
        let old_origin = graph.open.lock().as_ref().unwrap().previews[0].origin.clone();
        fixture.write("const first = hz(123);\nconst second = hz(456);\nexport default song(() => osc.sine(hz(800)));\n");
        // The inserted calls shift the control's position in the graph. The commit is
        // carried to it, and the player is rebuilt rather than patched at the old one.
        graph.set(token, revision, &id, "drag", 950.0, Some(800.0)).unwrap();
        let open = graph.open.lock();
        let o = open.as_ref().unwrap();
        assert!(o.session.accepted().number > revision + 1);
        assert_eq!(o.playing, o.session.accepted().number);
        assert!(o.previews.is_empty());
        let current = o.session.controls().into_iter().find(|c| c.value == 950.0).unwrap();
        assert_ne!(current.origin, old_origin);
        let source = std::fs::read_to_string(fixture.0.join("song.ts")).unwrap();
        assert!(source.contains("hz(123)") && source.contains("hz(456)") && source.contains("hz(950)"));
    }

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
        {
            let open = graph.open.lock();
            let o = open.as_ref().unwrap();
            assert_eq!(o.playing, o.session.accepted().number);
            assert_eq!((o.previews.len(), o.previews[0].baseline), (1, 800.0));
        }
        // Another gesture can't take the carried control.
        assert!(graph.preview(token, revision + 1, &id, "other", 1000.0).is_err());
        let result = graph.set(token, revision, &id, "drag", 950.0, Some(800.0)).unwrap();
        assert_eq!(result["after"], "950");
        let source = std::fs::read_to_string(fixture.0.join("song.ts")).unwrap();
        assert!(source.contains("// notes, edited elsewhere") && source.contains("hz(950)"));
        let open = graph.open.lock();
        let o = open.as_ref().unwrap();
        assert!(o.previews.is_empty());
        assert_eq!(o.playing, o.session.accepted().number);
    }

    #[test]
    fn a_save_that_touches_the_dragged_value_ends_the_drag() {
        let fixture = Fixture::new();
        let graph = fixture.open();
        let (token, revision, id) = control(&graph);
        graph.preview(token, revision, &id, "drag", 900.0).unwrap();
        fixture.write("export default song(() => osc.sine(hz(801)));\n");
        assert!(graph.poll());
        assert!(graph.preview(token, revision, &id, "drag", 920.0).is_err());
        assert!(graph.set(token, revision, &id, "drag", 950.0, Some(800.0)).is_err());
        assert!(std::fs::read_to_string(fixture.0.join("song.ts")).unwrap().contains("hz(801)"));
        assert!(graph.open.lock().as_ref().unwrap().previews.is_empty());
    }

    #[test]
    fn a_cable_edit_closes_previews_and_plays_the_new_graph() {
        let fixture = Fixture::new();
        fixture.write("export default song(() => {\n  const tone = osc.saw(hz(110));\n  return osc.sine(hz(800));\n});\n");
        let graph = fixture.open();
        let (token, revision, id) = control(&graph);
        graph.preview(token, revision, &id, "drag", 900.0).unwrap();
        // An audio signal can't drive a frequency: refused, and the preview is closed.
        let from = "song.ts#0";
        assert!(graph.connect(token, revision, from, &id).is_err());
        let open = graph.open.lock();
        let o = open.as_ref().unwrap();
        assert!(o.previews.is_empty());
        assert_eq!((o.playing, o.session.accepted().number), (revision, revision));
        drop(open);
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
        let outlet = graph.open.lock().as_ref().unwrap().session.outlets()[0].id.clone();
        let connected = graph.connect(token, revision, &outlet, &id).unwrap();
        assert_eq!((connected["before"].as_str(), connected["after"].as_str()), (Some("hz(800)"), Some("sweep")));
        let inlet = {
            let open = graph.open.lock();
            let o = open.as_ref().unwrap();
            assert_eq!(o.playing, o.session.accepted().number);
            o.session.inlets()[0].id.clone()
        };
        let revision = connected["revision"].as_u64().unwrap();
        let disconnected = graph.disconnect(token, revision, &inlet).unwrap();
        // std:sine declares `freq = 440`.
        assert_eq!(disconnected["after"], "hz(440)");
        let open = graph.open.lock();
        let o = open.as_ref().unwrap();
        assert_eq!(o.playing, o.session.accepted().number);
        assert!(std::fs::read_to_string(fixture.0.join("song.ts")).unwrap().contains("osc.sine(hz(440))"));
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
        {
            let open = graph.open.lock();
            let o = open.as_ref().unwrap();
            assert_eq!(o.playing, o.session.accepted().number);
            assert_eq!(o.session.controls()[0].value, 800.0);
        }
        graph.redo(token).unwrap();
        assert!(std::fs::read_to_string(fixture.0.join("song.ts")).unwrap().contains("hz(950)"));
        assert!(graph.undo(token + 1).is_err(), "another session's request is refused");
    }

    #[test]
    fn effects_are_listed_added_and_removed_and_the_player_follows() {
        let fixture = Fixture::new();
        let graph = fixture.open();
        let state = graph.state().unwrap();
        let (token, revision) = (state["session"].as_u64().unwrap(), state["view"]["revision"].as_u64().unwrap());
        assert!(state["effects"].as_array().unwrap().iter().any(|e| e["name"] == "delay" && e["module"] == "std/fx"));
        let node = state["view"]["modules"][0]["nodes"].as_array().unwrap().iter().find(|n| n["audio"] == true).unwrap().clone();
        let call = node["id"].as_str().unwrap();
        let added = graph.add_effect(token, revision, call, "std/fx", "delay").unwrap();
        assert_eq!(added["after"], "delay(osc.sine(hz(800)), { time: 0.25, feedback: 0.3, mix: 0.3 })");
        let state = graph.state().unwrap();
        let delay = state["view"]["modules"][0]["nodes"].as_array().unwrap().iter().find(|n| n["effect"] == "delay").unwrap().clone();
        {
            let open = graph.open.lock();
            let o = open.as_ref().unwrap();
            assert_eq!(o.playing, o.session.accepted().number);
        }
        let revision = state["view"]["revision"].as_u64().unwrap();
        graph.remove_effect(token, revision, delay["id"].as_str().unwrap()).unwrap();
        assert!(std::fs::read_to_string(fixture.0.join("song.ts")).unwrap().contains("song(() => osc.sine(hz(800)))"));
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
        let synced = |graph: &Graph| graph.open.lock().as_ref().map(|o| o.playing == o.session.accepted().number).unwrap();
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
        assert_eq!(relink["edit"]["ops"], json!([{"op": "rename", "from": "data/kick", "to": "data/kicks"}]));
        let news = graph.news();
        assert_eq!((&news["session"], &news["revision"], &news["problems"]), (&json!(token), &json!(revision), &state["problems"]));

        let synced = |graph: &Graph| graph.open.lock().as_ref().map(|o| o.playing == o.session.accepted().number).unwrap();
        let clips = |graph: &Graph| graph.state().unwrap()["timeline"]["tracks"][0]["lanes"][0]["clips"].as_array().unwrap().len();
        assert!(graph.fix(token + 1, revision, "relink", &relink["edit"]).is_err(), "another session's request is refused");
        let moved = graph.fix(token, revision, "relink", &relink["edit"]).unwrap();
        assert_eq!((&moved["before"], &moved["after"]), (&json!("unused data"), &json!("kicks")));
        assert!(fixture.0.join("data/kicks/hits.json").exists() && !fixture.0.join("data/kick").exists());
        assert_eq!((graph.state().unwrap()["problems"].clone(), clips(&graph)), (json!([]), 1), "the track plays its notes");
        assert_eq!(graph.news()["problems"], json!([]));
        assert!(synced(&graph));
        assert!(graph.fix(token, revision, "relink", &relink["edit"]).is_err(), "offered no more");

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
        let base = std::fs::read_to_string(fixture.0.join("song.ts")).unwrap();
        let saved = base.replace("// ", "").replacen('\n', "\n// a comment\n", 1);
        let state = graph.write_file(Some(token), "song.ts", &base, &saved).unwrap();
        assert_eq!(state["can_undo"], true);
        let undone = graph.undo(token).unwrap();
        assert_eq!((undone["before"].as_str(), undone["after"].as_str()), (Some("// a comment"), Some("nothing")));
        assert_eq!(std::fs::read_to_string(fixture.0.join("song.ts")).unwrap(), base);
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

    #[test]
    fn poll_restores_invalid_drafts_and_syncs_already_refreshed_sources() {
        let fixture = Fixture::new();
        let graph = fixture.open();
        let (token, revision, id) = control(&graph);
        graph.preview(token, revision, &id, "drag", 900.0).unwrap();
        fixture.write("(");
        assert!(graph.poll());
        {
            let open = graph.open.lock();
            let o = open.as_ref().unwrap();
            assert!(!o.session.draft_errors.is_empty());
            assert!(o.previews.is_empty());
            assert_eq!(o.playing, revision);
        }
        assert!(graph.preview(token, revision, &id, "drag", 900.0).is_err());
        fixture.write("export default song(() => osc.sine(hz(900)));\n");
        {
            let mut open = graph.open.lock();
            let o = open.as_mut().unwrap();
            assert!(o.session.refresh());
            assert!(!o.session.refresh());
            assert_ne!(o.playing, o.session.accepted().number);
        }
        assert!(graph.poll(), "a renderer behind the accepted revision still needs synchronization");
        let open = graph.open.lock();
        let o = open.as_ref().unwrap();
        assert_eq!(o.playing, o.session.accepted().number);
    }

    // RENDER-9, RENDER-29: going back to the start is a seek, not a rebuild: a drag goes on.
    #[test]
    fn a_commit_closes_previews_and_a_restart_keeps_them() {
        let fixture = Fixture::new();
        let graph = fixture.open();
        let (token, revision, id) = control(&graph);
        graph.preview(token, revision, &id, "drag", 900.0).unwrap();
        let result = graph.set(token, revision, &id, "drag", 950.0, Some(800.0)).unwrap();
        {
            let open = graph.open.lock();
            let o = open.as_ref().unwrap();
            assert!(o.previews.is_empty());
            assert_eq!(o.playing, result["revision"].as_u64().unwrap());
            assert_eq!(o.session.controls()[0].value, 950.0);
        }
        let revision = result["revision"].as_u64().unwrap();
        graph.preview(token, revision, &id, "next", 1000.0).unwrap();
        graph.transport("restart").unwrap();
        let open = graph.open.lock();
        let o = open.as_ref().unwrap();
        assert_eq!((o.previews.len(), o.previews[0].gesture.as_str()), (1, "next"));
        assert_eq!(o.playing, revision);
        assert_eq!(o.session.controls()[0].value, 950.0);
    }

    /// Wait for the player's thread to catch up.
    fn eventually(what: &str, done: impl Fn() -> bool) {
        let start = std::time::Instant::now();
        while !done() {
            assert!(start.elapsed() < std::time::Duration::from_secs(5), "timed out waiting until {what}");
            std::thread::sleep(std::time::Duration::from_millis(5));
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

        // Stopped, the position moves at once and stays there.
        let stopped = |seconds, beat| Status { playing: false, seconds, beat, looping: None };
        graph.seek(2.0).unwrap();
        assert_eq!(graph.status(), stopped(1.0, 2.0), "at 120 bpm, 1 second is 2 beats");
        graph.seek(8.0).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        assert_eq!(graph.status(), stopped(6.0, 8.0), "then 4 beats at 60 bpm");
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
        std::thread::sleep(std::time::Duration::from_millis(30));
        assert_eq!(graph.status(), stopped(0.5, 1.0));

        // RENDER-8: a seek replays nothing, so one far past the song lands there at once.
        let t = std::time::Instant::now();
        graph.seek(4.0 + 2000.0).unwrap();
        eventually("it lands far past the song", || graph.status() == stopped(2002.0, 2004.0));
        assert!(t.elapsed() < std::time::Duration::from_secs(1), "{:?}", t.elapsed());

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
        let player = graph.player().unwrap();
        eventually("the seek lands", || player.status().pending.is_none());
        assert_eq!((player.device_rate, player.rate(), player.status().pos), (48_000, 44_100, 44_100));
        let at = Status { playing: false, seconds: 1.0, beat: 2.0, looping: None };
        assert_eq!(graph.status(), at, "at 120 bpm, 2 beats are 1 second");
        // A manifest edit to the rate plays on from the same beat, in a fresh engine at the new rate.
        std::fs::write(fixture.0.join("music2.toml"), MANIFEST.replace("48000", "96000")).unwrap();
        assert!(graph.poll());
        assert_eq!(graph.status(), at);
        let player = graph.player().unwrap();
        eventually("the new engine plays", || player.status().pending.is_none());
        assert_eq!((player.rate(), player.status().pos), (96_000, 96_000));
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
        fixture.write("import { vca } from 'std/dsp';\nexport default song(({ lane }) => vca(osc.sine(hz(440)), lane.control(\"g\", \"gain\", 1)));\n");
        let graph = fixture.open();
        graph.seek(12.0).unwrap();
        assert_eq!(graph.status().beat, 12.0);
        graph.transport("play").unwrap();
        eventually("it plays on from there", || graph.status().beat > 12.0);
    }

    #[test]
    fn status_does_not_wait_for_the_project() {
        let fixture = Fixture::new();
        let graph = &fixture.open();
        graph.seek(2.0).unwrap();
        // As a seek or an edit does while it replays the song.
        let held = graph.open.lock();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::scope(|s| {
            s.spawn(move || tx.send(graph.status()).unwrap());
            let status = rx.recv_timeout(std::time::Duration::from_secs(2)).expect("status answers while the project is held");
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
        {
            let mut open = graph.open.lock();
            let o = open.as_mut().unwrap();
            assert!(o.session.refresh());
            assert_ne!(o.playing, o.session.accepted().number);
        }
        graph.seek(4.0).unwrap();
        {
            let open = graph.open.lock();
            let o = open.as_ref().unwrap();
            assert_eq!((o.previews.len(), o.playing), (1, revision));
        }
        assert!(graph.poll());
        let open = graph.open.lock();
        let o = open.as_ref().unwrap();
        assert!(o.previews.is_empty(), "the new graph took over");
        assert_eq!(o.playing, o.session.accepted().number);
        assert_eq!(graph.status().beat, 4.0, "where the seek put it");
    }

    #[test]
    fn returning_a_drag_to_its_start_closes_the_preview_without_rewriting_source() {
        let fixture = Fixture::new();
        fixture.write("export default song(() => osc.sine(hz(8_00)));\n");
        let graph = fixture.open();
        let (token, revision, id) = control(&graph);
        let source = std::fs::read_to_string(fixture.0.join("song.ts")).unwrap();
        graph.preview(token, revision, &id, "drag", 950.0).unwrap();
        let result = graph.set(token, revision, &id, "drag", 800.0, Some(800.0)).unwrap();
        assert_eq!(result["revision"].as_u64().unwrap(), revision);
        assert_eq!(std::fs::read_to_string(fixture.0.join("song.ts")).unwrap(), source);
        let open = graph.open.lock();
        let o = open.as_ref().unwrap();
        assert!(o.previews.is_empty());
        assert_eq!(o.playing, revision);
        assert_eq!(o.session.controls()[0].value, 800.0);
    }

    #[test]
    fn a_saved_file_plays_at_once_and_a_save_over_a_change_is_refused() {
        let fixture = Fixture::new();
        let graph = fixture.open();
        let (token, revision, _) = control(&graph);
        let playing = |graph: &Graph| graph.open.lock().as_ref().map(|o| (o.playing, o.session.accepted().number)).unwrap();
        let base = std::fs::read_to_string(fixture.0.join("song.ts")).unwrap();
        let saved = base.replace("800", "900");
        let state = graph.write_file(Some(token), "song.ts", &base, &saved).unwrap();
        assert_eq!((&state["view"]["revision"], &state["diagnostics"]), (&json!(revision + 1), &json!([])));
        assert_eq!(playing(&graph), (revision + 1, revision + 1), "adopted and playing without waiting for the watcher");
        assert!(!graph.poll());

        // Saved from the old text: refused with what the file says now, and nothing written.
        let e = graph.write_file(Some(token), "song.ts", &base, "export default 1;\n").unwrap_err();
        assert_eq!(e.downcast_ref::<files::Conflict>().unwrap().current, saved);
        assert_eq!(std::fs::read_to_string(fixture.0.join("song.ts")).unwrap(), saved);

        // Code that doesn't compile is saved; the last revision that did plays on.
        let state = graph.write_file(Some(token), "song.ts", &saved, "export default song(() => (\n").unwrap();
        assert_eq!(state["view"]["revision"], revision + 1);
        assert_eq!((&state["diagnostics"][0]["file"], &state["diagnostics"][0]["line"]), (&json!("song.ts"), &json!(2)));
        assert_eq!(playing(&graph), (revision + 1, revision + 1));
        assert!(std::fs::read_to_string(fixture.0.join("song.ts")).unwrap().ends_with("song(() => (\n"));

        assert!(graph.write_file(Some(token + 1), "song.ts", "", "").is_err(), "another session's editor is refused");
        assert!(graph.write_file(Some(token), "../song.ts", "", "").is_err());
        graph.close();
        assert!(graph.files().is_err() && graph.write_file(None, "song.ts", "", "").is_err());
    }

    /// A kick track whose decay is a knob at 0.3 s.
    const KICK: &str = "import { secs, track } from '@m2/core';\nimport { kick } from 'std/instruments';\n\
                        const k = track('kick', ({ lane }) => kick(lane.notes('hits'), { decay: secs(0.3) }));\n\
                        export default song(() => k.out);\n";

    fn kick_clips(pitches: &[&str]) -> Vec<NoteClip> {
        let notes: Vec<Value> = pitches.iter().enumerate().map(|(i, p)| json!({"pitch": p, "start": i, "length": 0.25})).collect();
        serde_json::from_value(json!([{"id": "a", "start": 0, "length": 4, "notes": notes}])).unwrap()
    }

    /// The renderer the player plays, once it has taken everything sent.
    fn generation(graph: &Graph) -> u64 {
        let player = graph.player().unwrap();
        eventually("the player takes what was sent", || player.status().pending.is_none());
        std::thread::sleep(std::time::Duration::from_millis(30));
        player.status().generation
    }

    // RENDER-26, PLANE-1: a lane write is swapped into the playing renderer, whoever wrote it: the
    // same renderer plays on, and a drag goes on. Undo, and a lane write after a knob commit, take
    // the same way.
    #[test]
    fn a_lane_write_while_playing_is_swapped_into_the_playing_renderer() {
        let fixture = Fixture::new();
        fixture.write(KICK);
        let graph = fixture.open();
        graph.transport("play").unwrap();
        eventually("it plays", || graph.status().playing);
        let first = generation(&graph);
        let knob = |graph: &Graph| {
            let open = graph.open.lock();
            let o = open.as_ref().unwrap();
            let control = o.session.controls().into_iter().find(|c| c.literal == "0.3").unwrap();
            (o.token, o.session.accepted().number, control.id)
        };
        let (token, revision, id) = knob(&graph);
        graph.preview(token, revision, &id, "drag", 0.5).unwrap();
        // Written by an agent, mid-drag.
        let hits = r#"{"clips": [{"id": "a", "start": 0, "length": 4, "notes": [{"pitch": "C1", "start": 0, "length": 1}]}]}"#;
        std::fs::create_dir_all(fixture.0.join("data/kick")).unwrap();
        std::fs::write(fixture.0.join("data/kick/hits.json"), hits).unwrap();
        assert!(graph.poll());
        let after = {
            let open = graph.open.lock();
            let o = open.as_ref().unwrap();
            let after = o.session.accepted().number;
            assert!(after > revision && o.playing == after);
            assert_eq!((o.previews.len(), o.previews[0].revision), (1, after), "the drag goes on, in the new revision");
            after
        };
        assert_eq!(generation(&graph), first, "no new renderer");
        assert!(graph.status().playing);
        graph.write_notes(token, after, "kick", "hits", &kick_clips(&["C1", "D1"])).unwrap();
        assert_eq!(generation(&graph), first, "the piano roll's write swaps too");

        // A knob commit leaves the renderer playing the committed graph: a lane edit still swaps.
        let (_, after, _) = knob(&graph);
        graph.preview(token, after, &id, "drag", 0.5).unwrap();
        graph.set(token, after, &id, "drag", 0.5, Some(0.3)).unwrap();
        let revision = graph.state().unwrap()["view"]["revision"].as_u64().unwrap();
        graph.write_notes(token, revision, "kick", "hits", &kick_clips(&["C1"])).unwrap();
        graph.undo(token).unwrap();
        assert_eq!(generation(&graph), first, "still no new renderer");
        let open = graph.open.lock();
        let o = open.as_ref().unwrap();
        assert_eq!(o.playing, o.session.accepted().number);
        assert!(std::fs::read_to_string(fixture.0.join("song.ts")).unwrap().contains("secs(0.5)"));
    }

    // Step 3's gates time an edit or a seek from when it was asked for (or the session finished)
    // to the callback that took it: what the server records says which message carried it, and
    // the engine's stamp says when it applied.
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
            assert!(t.requested_ns <= t.compiled_ns && t.compiled_ns <= t.sent_ns, "{t:?}");
            // The callback that took it started after it was queued, or was already running.
            assert!(t.compiled_ns <= a.callback_ns + 20_000_000, "{t:?} then {a:?}");
            assert!(a.callback_ns <= graph.now_ns().unwrap());
        };
        graph.write_notes(token, revision, "kick", "hits", &kick_clips(&["C1", "D1"])).unwrap();
        taken(graph.last_edit().unwrap());
        graph.seek(2.0).unwrap();
        let seek = graph.last_edit().unwrap();
        assert_eq!(seek.requested_ns, seek.compiled_ns);
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
        std::fs::create_dir_all(fixture.0.join("data/song")).unwrap();
        std::fs::write(fixture.0.join("data/song/g.json"), gain(0.5)).unwrap();
        fixture.write("import { vca } from 'std/dsp';\nexport default song(({ lane }) => vca(osc.sine(hz(440)), lane.control(\"g\", \"gain\", 1)));\n");
        let graph = fixture.open();
        graph.transport("play").unwrap();
        let first = generation(&graph);
        std::fs::write(fixture.0.join("data/song/g.json"), gain(0.25)).unwrap();
        assert!(graph.poll());
        assert_eq!(generation(&graph), first, "no new renderer");
        let open = graph.open.lock();
        let o = open.as_ref().unwrap();
        assert_eq!(o.playing, o.session.accepted().number);
        assert_eq!(o.plays.as_ref().unwrap().data.control("song", "g"), o.session.accepted().compiled.data.control("song", "g"));
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
        graph.add_effect(token, revision, call.unwrap()["id"].as_str().unwrap(), "std/fx", "delay").unwrap();
        let after = graph.status();
        assert!(after.playing && after.beat >= before, "no jump back: beat {before} then {}", after.beat);
        assert_ne!(generation(&graph), first, "a new renderer plays");
        let mut last = graph.status().beat;
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(5));
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
        eventually("it stops", || !graph.status().playing);
        let (beat, player) = (graph.status().beat, graph.player().unwrap());
        let clock = player.clock();
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert_eq!(graph.status().beat, beat, "the transport holds");
        assert!(player.clock() > clock, "the engine clock goes on");
        assert!(graph.transport("rewind").is_err());
    }

    // RENDER-9a: a loop is beats of the accepted revision, refused when the engine couldn't play it;
    // its range is kept while it is off, and playback wraps inside it. A project opened anew has none.
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
        let t = std::time::Instant::now();
        let mut wrapped = false;
        while t.elapsed() < std::time::Duration::from_millis(1200) {
            let beat = graph.status().beat;
            assert!((4.0..5.0).contains(&beat), "inside the loop: {beat}");
            wrapped |= beat < 4.4;
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(wrapped, "it went round");
        graph.open(&fixture.0).unwrap();
        assert_eq!(graph.status().looping, None);
    }

    // RENDER-29: seek, the loop and play never wait for the project, which an edit holds while it
    // compiles.
    #[test]
    fn the_transport_does_not_wait_for_the_project() {
        let fixture = Fixture::new();
        let graph = &fixture.open();
        let held = graph.open.lock();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::scope(|s| {
            s.spawn(move || {
                graph.seek(3.0).unwrap();
                graph.set_loop(Some((2.0, 6.0)), None).unwrap();
                graph.transport("play").unwrap();
                tx.send(graph.status()).unwrap();
            });
            let status = rx.recv_timeout(std::time::Duration::from_secs(2)).expect("the transport answers while the project is held");
            assert_eq!(status.looping, Some(Loop { start: 2.0, end: 6.0, on: true }));
            drop(held);
        });
        eventually("it plays from beat 3", || graph.status().playing && graph.status().beat >= 3.0);
    }

    // A player that stops responding is let go, and the project is loaded into a new one where it
    // was (step 3a design, §9.5).
    #[test]
    fn a_stalled_player_is_replaced_and_the_project_plays_on_where_it_was() {
        let fixture = Fixture::new();
        // A device that never calls back.
        let (player, driver) = Player::driven(48_000);
        let graph = Graph { player: Mutex::new(Some(Device { player, epoch: 0, resume: 0.0 })), ..Graph::default() };
        graph.open(&fixture.0).unwrap();
        let t = std::time::Instant::now();
        let e = loop {
            if let Err(e) = graph.seek(3.0) {
                break e;
            }
            assert!(t.elapsed() < std::time::Duration::from_secs(5), "the queue never filled");
        };
        assert!(e.to_string().contains("stopped responding"), "{e:#}");
        assert!(t.elapsed() < std::time::Duration::from_secs(5), "a send gives up: {:?}", t.elapsed());
        assert!(graph.transport("play").is_err(), "the stalled player is let go");
        assert!(graph.poll(), "a new one plays the project");
        eventually("where it was", || graph.status() == Status { playing: false, seconds: 1.5, beat: 3.0, looping: None });
        assert_eq!(graph.player().unwrap().device, "none");
        drop(driver);
    }
}
