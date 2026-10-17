//! The open song project in the app, shown as a graph.
//!
//! One editing session ([`m2_session::Session`]) and one player ([`m2_play::Player`]), whose live
//! engine runs while the project is open, playing or not: stop ends the notes and lets tails ring
//! out (RENDER-6a).
//!
//! The session runs as an actor on its own thread (`m2-session`, see `actor`): opening, every
//! edit, saves, the watcher's polls and commits are requests it serves one at a time, and each
//! may compile. Nothing that waits for it blocks the engine, the transport or a knob drag
//! (WRITE-22, RENDER-29). What everything else reads comes from the control plane, which the actor
//! publishes in two phases after each change: first what the engine and gesture leases need
//! (the accepted revision's controls, and which renderer plays), then the view, the timeline and
//! the news. Reads and the transport never wait for a compile.
//!
//! Knob drags are gesture leases (WRITE-22, see `lease`): `begin` resolves the control against
//! what the session last published, updates go straight to the engine, and the commit goes to
//! the session. A lease survives every revision that doesn't touch its call, so an agent saving
//! another file mid-drag doesn't interrupt it.
//!
//! Every accepted revision reaches the engine the way its files say (PLANE-1,
//! [`m2_session::plane`]): lane data that changed alone is swapped into the running renderer
//! (RENDER-26), a value commit is written into its cells when the delta from what plays is values
//! alone (WRITE-8), and anything else is built into a new renderer that takes over where the
//! engine is, crossfaded, with no jump back (RENDER-25a). Nothing replays the song (RENDER-8).
//! Edits made elsewhere (a text editor, an agent, git) are taken in by the watcher once they
//! settle ([`Graph::watch`], WRITE-19), and take the same way.
//!
//! Every write is a Tx or a `save()` (P4): the gestures are `crates/editor`'s builders, an agent's
//! Tx JSON is checked and committed as it comes ([`Graph::tx_check`], [`Graph::tx_commit`]), and
//! the code editor's saves are the session's `save()`.
//!
//! Every change is announced with what needs attention in the revision it leaves (see
//! [`Graph::news`]): its problems with their fixes (DIAG-1, DIAG-4), why the draft doesn't
//! compile, and the gestures in progress.
//!
//! The player runs the engine at the project's `sample_rate` (RENDER-31), so every frame here
//! counts at that rate.

mod actor;
pub mod audition;
pub mod clips;
mod instrument;
mod rekey;
mod events;
pub mod lease;
mod markers;
pub mod meters;
pub mod overlay;
mod preview;
mod projects;
mod songkey;
mod stopped;
mod view_file;
mod watcher;

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};
use m2_editor::render::{Length, Render};
use m2_editor::{Control, Gestures};
use m2_play::Player;
use m2_runtime::transport::frame_of;
use m2_runtime::{Cmd, Transport};
use m2_song::{NoteClip, TempoMap};
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::{Value, json};

use crate::files;
use actor::{Actor, Job};
use lease::{Aliases, Lease, Leases, PlaysView, Revs, State, TTL};
pub use lease::{Begun, Committed, LeaseInfo, Owner};
pub use watcher::WatchStats;

/// When the last edit or seek reached the engine, for gate measurements (step 3a's design, §9.2):
/// when it was asked for and when the session had done its part, on the player's clock
/// ([`Player::now_ns`]), and the message that carried it, whose [`Graph::applied`] stamp says when
/// the engine took it. In order: `requested ≤ ready ≤ sent ≤ published`, `ready ≤ compiled ≤
/// published`, and for a previewed edit `sent ≤ compiled` (PLANE-5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EditTiming {
    pub requested_ns: u64,
    /// The session's work before the engine could be told: for a previewed data Tx, when its
    /// checks passed and its files read as their bases (PLANE-5); else `compiled_ns`. What the
    /// edit budgets count from.
    pub ready_ns: u64,
    /// The session's work done, its compile included: for a Tx, durable and accepted.
    pub compiled_ns: u64,
    /// The first engine message that carried it queued (data prepared or a renderer built): the
    /// preview's, or the end of the actor's first phase.
    pub sent_ns: u64,
    /// The view, the timeline and the news published: the end of the second phase, after which
    /// the request replies. It never delays the engine.
    pub published_ns: u64,
    /// None when nothing was sent, or a fresh engine took it (an open, a rate change).
    pub seq: Option<u64>,
    /// Heard as its commit's preview, before it was durable (PLANE-5).
    pub previewed: bool,
    /// Its commit failed after the preview: the message that swapped the accepted data back.
    pub taken_back: Option<u64>,
    /// Of the server's part before it sent: naming what continues (a code change's activation),
    /// and preparing the plan, in nanoseconds; 0 when it prepared none.
    pub continues_ns: u64,
    pub prepare_ns: u64,
    /// The instances the plan it prepared made new, by kind (every one for a fresh engine; G7a).
    pub fresh: m2_runtime::Count,
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

/// Whether the player is playing, and where: in seconds, and in beats (0 with no project open);
/// the tempo it plays there; and the loop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct Status {
    pub playing: bool,
    pub seconds: f64,
    pub beat: f64,
    /// The tempo at `beat` of the revision the engine plays, which the page's playhead moves at.
    /// A tempo edit reaches it at the switch (RENDER-27), not when it is accepted: the timeline
    /// says the new tempo before then, and during an export's hold until it ends. None with no
    /// player.
    pub bpm: Option<f64>,
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

/// An export of the open song (see [`Graph::export`]).
pub struct Export {
    pub song: String,
    pub dir: PathBuf,
    pub render: Render,
    /// What the export carries: a plugin that isn't installed is silent in it (FMT-18a).
    pub problems: Vec<m2_song::Diagnostic>,
    /// The live plugin instances it borrows, taken on the export's thread with playback paused,
    /// given back when this drops.
    pub lent: Option<crate::export::Lent>,
}

/// What the engine is busy with besides playing, and what its device reported. Its fields go into
/// the engine view and `/api/graph/status` by these names ([`Activity::put`]).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Activity {
    /// `"exporting"` while an export holds playback (RENDER-1).
    pub paused: Option<&'static str>,
    /// The plugins being created for the next plan, by name (RENDER-25).
    pub loading: Vec<String>,
    /// The dropouts (xruns) the device reported since the player started: counted, and play
    /// goes on (RENDER-28).
    pub xruns: u64,
    /// The device's other errors since the player started. A change of output device, or a
    /// device that is gone, lets the player go; the rest are only counted.
    pub device_errors: u64,
    /// Live notes sounding: audition's (MON-7).
    pub live: u32,
    /// The DSP load: the longest audio callback of the last second, as a share of the device's
    /// period, while the meters read it (8b.8).
    pub load: f32,
    /// Audio callbacks that took longer than their period since the player started: dropouts too.
    pub late: u64,
}

impl Activity {
    /// Its fields, set in the JSON object `v`.
    pub fn put(&self, v: &mut Value) {
        if let (Some(v), Value::Object(fields)) = (v.as_object_mut(), json!(self)) {
            v.extend(fields);
        }
    }
}

/// What `begin` and `update` read: the accepted revision's controls and what plays. Published by
/// the actor's first phase, before the view.
struct EngineView {
    token: u64,
    revision: u64,
    /// The revision whose code the accepted one has (WRITE-13).
    code_rev: u64,
    controls: Arc<HashMap<String, Control>>,
    /// None only while no player holds the project.
    plays: Option<PlaysView>,
}

/// What reads return: published by the actor's second phase.
struct Shown {
    /// The accepted revision the view shows.
    revision: u64,
    /// The state without the transport and the gestures, which [`Graph::state`] adds as they are
    /// now, nor the timeline.
    state: Value,
    /// The timeline, each lane's JSON shared with the views before it that had the same lane.
    timeline: Arc<Timeline>,
    news: Value,
    files: files::Accepted,
    types: Arc<Value>,
    name: String,
    dir: PathBuf,
    /// Why the session can't write, in WRITE-18's words (WRITE-17): None for the lock's holder.
    read_only: Option<String>,
}

/// The timeline as published (`m2_editor::TimelineView` as JSON): each lane made once and shared
/// by every view whose lane is the same data, so a one-lane edit makes one lane's JSON, not every
/// note of the song's (the G11 fix's A.8).
#[derive(Default)]
pub(crate) struct Timeline {
    /// `tempo`, `meter`, `home` and `end`.
    head: serde_json::Map<String, Value>,
    tracks: Vec<TimelineRow>,
}

/// A track of the timeline: its own fields, and its lanes.
pub(crate) struct TimelineRow {
    pub(crate) module: String,
    pub(crate) head: serde_json::Map<String, Value>,
    pub(crate) lanes: Vec<TimelineLaneJson>,
}

/// A lane's JSON, with what it was made of: the data it shows (held, so it is never another's),
/// and the JSON of the rest of it.
pub(crate) struct TimelineLaneJson {
    pub(crate) key: String,
    pub(crate) data: Option<Arc<m2_song::LaneData>>,
    pub(crate) head: String,
    pub(crate) json: Arc<Value>,
}

impl Timeline {
    pub(crate) fn new(head: serde_json::Map<String, Value>, tracks: Vec<TimelineRow>) -> Timeline {
        Timeline { head, tracks }
    }

    /// The lane `key` of `module`, as published.
    pub(crate) fn lane(&self, module: &str, key: &str) -> Option<&TimelineLaneJson> {
        self.tracks.iter().find(|t| t.module == module)?.lanes.iter().find(|l| l.key == key)
    }

    /// As one value: what serialising the timeline view gives.
    fn to_value(&self) -> Value {
        let tracks = self.tracks.iter().map(|t| {
            let mut row = t.head.clone();
            row.insert("lanes".into(), Value::Array(t.lanes.iter().map(|l| (*l.json).clone()).collect()));
            Value::Object(row)
        });
        let mut out = self.head.clone();
        out.insert("tracks".into(), Value::Array(tracks.collect()));
        Value::Object(out)
    }
}

/// The open project as the control plane holds it.
struct Published {
    engine: Arc<EngineView>,
    shown: Arc<Shown>,
    revs: Revs,
    aliases: Arc<Aliases>,
    /// Monitoring mute and solo (MON-4): made new at each open.
    overlays: overlay::Monitor,
    /// The page's view settings (`view_file::KEYS`), as read at the open and set since: the
    /// state's `local`.
    local: serde_json::Map<String, Value>,
}

/// What `begin`, `update`, `cancel` and every read use, behind one short lock. The actor holds it
/// only to publish a change: never over a compile, a build or a wait for the main thread. Lock
/// order: `control`, then `player`, then the leaf locks (`timing`, `last_edit`, `unsaved`,
/// `busy`, `notices`, `saving`).
#[derive(Default)]
struct ControlPlane {
    open: Option<Published>,
    leases: Leases,
    /// The windows that play notes live (MON-7).
    audition: audition::Sources,
    /// The DSP load of the last second, read by the meters (8b.8).
    meters: meters::Meters,
}

/// How a control named at a revision is found without the session.
enum Resolved {
    // Boxed: a control (with its origin) is far larger than the other answers.
    Control(Box<Control>),
    Missing,
    /// Too old for the control plane: the session carries it.
    Slow,
}

impl Published {
    fn find(&self, revision: u64, id: &str) -> Resolved {
        let Some(code) = self.revs.code_rev(revision) else { return Resolved::Slow };
        let engine = &self.engine;
        let id = if code == engine.code_rev {
            id
        } else if self.aliases.to != engine.code_rev {
            // Between a settle's phases: the aliases still carry to the code revision before.
            return Resolved::Slow;
        } else {
            match self.aliases.ids.get(&(code, id.to_string())) {
                Some(id) => id.as_str(),
                None => return Resolved::Slow,
            }
        };
        match engine.controls.get(id) {
            Some(c) => Resolved::Control(Box::new(c.clone())),
            None => Resolved::Missing,
        }
    }
}

/// A commit heard as its preview and not yet durable (PLANE-5): since when, whether the user was
/// told it is taking a while, and whether the transport played meanwhile (what is said of it then
/// says it played).
#[derive(Clone, Copy)]
struct Saving {
    since: Instant,
    said: bool,
    played: bool,
}

/// How long a commit heard as its preview may take to be durable before the user is told it isn't
/// saved yet: a disk that stopped answering (a network volume gone, a failing drive).
const SLOW_SAVE: std::time::Duration = std::time::Duration::from_secs(2);

/// When the actor was busy: since when, and its last requests' spans, on the player's clock.
#[derive(Default)]
struct Busy {
    since: u64,
    spans: VecDeque<(u64, u64)>,
}

struct Inner {
    control: Mutex<ControlPlane>,
    player: Mutex<Option<Device>>,
    /// Kept apart from the session, which edits hold while they compile, so the transport never
    /// waits for them. None with no project open.
    timing: Mutex<Option<Timing>>,
    /// Players started so far, and the beat the last one that stopped responding had got to.
    players: AtomicU64,
    resume: Mutex<f64>,
    next_session: AtomicU64,
    /// The code editor's files with edits not yet saved, as it last said, and the project
    /// (its token) it said so for: the app asks before closing or replacing that project.
    unsaved: Mutex<(u64, Vec<String>)>,
    last_edit: Mutex<Option<EditTiming>>,
    /// When the last batch the watcher settled reached the engine, for the gates.
    last_taken_in: Mutex<Option<EditTiming>>,
    busy: Mutex<Busy>,
    /// What the actor ended or saved on its own (a pending commit), for the watcher to toast.
    notices: Mutex<Vec<String>>,
    /// The commit being made durable after its preview, if one is (PLANE-5).
    saving: Mutex<Option<Saving>>,
    /// Who creates this Graph's plugin instances: their events come to its actor alone, and their
    /// count is what closing waits on.
    owner: m2_plugins::Owner,
    /// The open project's settle rule (WRITE-19): polled by the watcher, told by the actor what the
    /// session wrote itself. A leaf lock, never held over a call to the actor, nor over a stat.
    watch: Mutex<Option<watcher::Watch>>,
    /// Held through each tick of the watcher, so two never overlap. Never taken by the actor.
    ticking: Mutex<()>,
    /// What wakes the watcher's thread early: file system events.
    wake: Arc<watcher::Wake>,
    /// Projects opened from now on are watched by walks alone, never events (the gates).
    walk_only: AtomicBool,
    /// Tests: what an export of this project does inside its window, and whether it holds.
    #[cfg(test)]
    export_hooks: Arc<crate::export::Hooks>,
    /// `layout.json`'s stamp as the watcher last saw it, by project token, and whether a change
    /// waits to be published: no snapshot holds it (PLANE-3), so it is watched on its own.
    layout: Mutex<LayoutSeen>,
}

/// A delete that waits: whether it is blocked, and the code that reads the track.
type Waits = (bool, Vec<m2_editor::Dependent>);

/// What the watcher last saw of `layout.json`.
#[derive(Default)]
struct LayoutSeen {
    token: u64,
    stamp: Option<(u64, std::time::SystemTime)>,
    changed: bool,
    /// How many times it was stat'd, for tests.
    looks: u64,
}

/// How long closing waits for the session's plugin instances to be destroyed.
const TEARDOWN: std::time::Duration = std::time::Duration::from_secs(2);

/// What closing the project left behind: the plugin instances destroyed, and those still alive
/// when the wait ended (a device thread that hangs holds its renderer). With some left, the app
/// leaves without running exit handlers, which would unload plugin code under live instances.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Teardown {
    pub destroyed: u64,
    pub left: u64,
}

/// Whether `e` says the computer ran out of memory, or of address space: wasmtime's "mmap failed to
/// reserve …" when a kernel instance's memory can't be reserved. Not a sound over its own 32 MB
/// ([`m2_runtime::OutOfMemory`]): that one says so itself, and closing other apps won't help.
pub(crate) fn out_of_memory(e: &anyhow::Error) -> bool {
    e.downcast_ref::<m2_runtime::OutOfMemory>().is_none() && e.chain().any(|c| {
        let t = c.to_string();
        ["mmap failed", "Cannot allocate memory", "out of memory"].iter().any(|s| t.contains(s))
    })
}

/// `e`, from making a song ready to play (opening it, exporting it), in plain words: running out
/// of memory says so and what to do, with wasmtime's text after as the detail. Anything else is
/// already in the app's words.
pub(crate) fn plainly(e: anyhow::Error) -> anyhow::Error {
    match out_of_memory(&e) {
        true => anyhow!("the computer ran out of memory for it. Close other apps or songs, then try again. (In detail: {e:#})"),
        false => e,
    }
}

/// Why `player` must be let go, if it must: it stopped responding or its engine panicked; or its
/// output moved to another device, whose rate and period a new player opens with.
fn let_go(player: &Player) -> Option<&'static str> {
    if player.crashed() || player.stalled() {
        Some("the audio device stopped responding")
    } else if player.changed() {
        Some("the sound output moved to another device")
    } else {
        None
    }
}

/// Let go of a player that stopped responding or whose output moved, off the caller's thread:
/// telling its engine to stop waits for room, and its device may never call back again. Its device
/// thread ends once its last clone is dropped: it stops the stream, and the engine is freed off
/// the audio thread.
fn retire(player: Player) {
    let _ = std::thread::Builder::new().name("m2-play-retire".into()).spawn(move || {
        let _ = player.unload();
    });
}

/// Refuse a loop the engine couldn't play: one that isn't from a beat to a later one, at or after
/// 0, or is shorter than two blocks or 5 ms at the project's rate (RENDER-9a).
fn check_loop(t: &Timing, start: f64, end: f64) -> Result<()> {
    loop_fits(&t.tempo, t.rate, start, end)
}

/// [`check_loop`] in the tempo map `tempo`, at `rate`.
fn loop_fits(tempo: &TempoMap, rate: u32, start: f64, end: f64) -> Result<()> {
    if !(start.is_finite() && end.is_finite() && 0.0 <= start && start < end) {
        bail!("a loop goes from a beat to a later one, from beat 0 on (not {start} to {end})");
    }
    let rate = rate as f64;
    let frames = frame_of(tempo, end, rate).saturating_sub(frame_of(tempo, start, rate));
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

/// A Tx from its JSON (WRITE-3), or the answer to one that doesn't read: rejected, with its
/// `tx.invalid` or `tx.path` diagnostics, as every other refusal is answered (the step 6 design's
/// 1.2).
fn parse_tx(v: &Value) -> Result<m2_song::Tx, (u16, Value)> {
    m2_song::Tx::from_json(v).map_err(|diagnostics| {
        (422, m2_editor::json::refusal(&m2_session::Refusal::Rejected { diagnostics, unexpected: None }))
    })
}

/// Put a lease's code value back where its updates went: it ended without a commit.
fn put_back(l: &Lease, device: Option<&Device>) {
    if let (Some(t), Some(d)) = (&l.engine, device)
        && t.epoch == d.epoch
    {
        d.player.set_cells(&t.cells, l.accepted);
    }
}

impl Inner {
    fn new(device: Option<Device>, owner: m2_plugins::Owner) -> Inner {
        Inner {
            owner,
            control: Mutex::default(),
            player: Mutex::new(device),
            timing: Mutex::default(),
            players: AtomicU64::default(),
            resume: Mutex::default(),
            next_session: AtomicU64::default(),
            unsaved: Mutex::default(),
            last_edit: Mutex::default(),
            last_taken_in: Mutex::default(),
            busy: Mutex::default(),
            notices: Mutex::default(),
            saving: Mutex::default(),
            watch: Mutex::default(),
            ticking: Mutex::default(),
            wake: Arc::default(),
            walk_only: AtomicBool::default(),
            #[cfg(test)]
            export_hooks: Arc::default(),
            layout: Mutex::default(),
        }
    }

    /// One tick of the watcher, up to the batch that settled in it, if one did, for the actor to
    /// take in; and whether to look at `layout.json` (FMT-19).
    fn tick(&self) -> (Option<(u64, watcher::Ready)>, bool) {
        let _one = self.ticking.lock();
        let (plan, layout) = match self.watch.lock().as_mut() {
            Some(w) => (w.plan(Instant::now()), w.look_at_layout()),
            None => (None, false),
        };
        let Some(plan) = plan else { return (None, layout) };
        (self.looked(plan), layout)
    }

    /// Look at what a tick planned, with no lock held, and apply it.
    fn looked(&self, plan: watcher::Plan) -> Option<(u64, watcher::Ready)> {
        // With no lock held: the actor tells the watcher what the session wrote after every job,
        // and must never wait for a walk.
        let looked = plan.look();
        let mut w = self.watch.lock();
        let w = w.as_mut().filter(|w| w.token == plan.token)?;
        let ready = w.apply(&plan, looked, Instant::now()).map(|r| (w.token, r));
        if let Some(n) = w.notice.take() {
            self.notices.lock().push(n);
        }
        ready
    }

    /// The player, started if there is none. One that stopped responding (a send timed out, the
    /// device is gone or went quiet), whose engine panicked, or whose output moved to another
    /// device is let go: this call fails, the next starts a new one, on the device that is the
    /// default then, and the next settle loads the project into it where it was. An xrun lets
    /// nothing go: the player counts it and plays on (RENDER-28).
    fn device(&self) -> Result<Device> {
        let mut d = self.player.lock();
        if let Some(why) = d.as_ref().and_then(|d| let_go(&d.player)) {
            let old = d.take().expect("a player to let go");
            let st = old.player.status();
            *self.resume.lock() = st.pending.unwrap_or(st.beats);
            retire(old.player);
            bail!("{why}; it starts again with the next action");
        }
        if d.is_none() {
            // Unit tests never open an audio device.
            let player = if cfg!(test) { Player::silent()? } else { Player::start()? };
            let epoch = self.players.fetch_add(1, Ordering::Relaxed) + 1;
            *d = Some(Device { player, epoch, resume: std::mem::take(&mut *self.resume.lock()) });
        }
        Ok(d.clone().expect("started"))
    }

    /// Now on the clock the engine's stamps count on ([`m2_play::Applied`]); 0 with no player.
    fn now_ns(&self) -> u64 {
        self.current_device().map_or(0, |d| d.player.now_ns())
    }

    /// The player there is, as it is: what gesture leases send to, which never start one.
    fn current_device(&self) -> Option<Device> {
        self.player.lock().clone()
    }

    fn status(&self) -> Status {
        let Some(d) = self.current_device() else { return Status::default() };
        let timing = self.timing.lock();
        let Some(t) = timing.as_ref() else { return Status::default() };
        let s = d.player.status();
        let (beat, seconds) = match s.pending {
            Some(beat) => (beat, t.tempo.seconds_at(beat)),
            None => (s.beats, s.seconds),
        };
        // A tempo change that makes the loop too short turns it off (RENDER-9a).
        let looping = t.range.map(|(start, end)| Loop { start, end, on: t.on && !d.player.loop_refused() });
        Status { playing: s.playing, seconds, beat, bpm: Some(t.tempo.bpm_at(beat)), looping }
    }

    fn activity(&self) -> Activity {
        let device = self.current_device();
        let held = device.as_ref().is_some_and(|d| d.player.held());
        let e = device.as_ref().map(|d| d.player.device_errors()).unwrap_or_default();
        let (load, late) = {
            let cp = self.control.lock();
            (cp.meters.load(), cp.meters.late())
        };
        Activity {
            paused: held.then_some("exporting"),
            loading: self.owner.loading(),
            xruns: e.xruns,
            device_errors: e.changes + e.gone + e.other,
            live: device.as_ref().map_or(0, |d| d.player.live_notes()),
            load,
            late,
        }
    }

    /// The view, the timeline, the project's name and folder, the transport as it is now, and the
    /// gestures in progress as the view names their controls: a save that renumbers the calls
    /// above a dragged knob gives it a new id, and the page finds its drag's knob by its lease
    /// (WRITE-22).
    fn state(&self) -> Option<Value> {
        let (shown, leases, overlays, local) = {
            let cp = self.control.lock();
            let p = cp.open.as_ref()?;
            let now = Instant::now();
            let token = p.engine.token;
            let leases: Vec<LeaseInfo> =
                cp.leases.live.iter().filter(|l| l.token == token).filter_map(|l| l.seen_at(p.shown.revision, now)).collect();
            (p.shown.clone(), leases, p.overlays.state(), Value::Object(p.local.clone()))
        };
        let status = self.status();
        let mut state = shown.state.clone();
        state["timeline"] = shown.timeline.to_value();
        state["playing"] = json!(status.playing);
        state["seconds"] = json!(status.seconds);
        state["beat"] = json!(status.beat);
        state["bpm"] = json!(status.bpm);
        state["loop"] = json!(status.looping);
        self.activity().put(&mut state);
        state["leases"] = json!(leases);
        state["overlays"] = overlays;
        state["local"] = local;
        Some(state)
    }

    /// End every lease, as the project they belong to closes. A pending commit's release was
    /// promised a save: the user is told it wasn't (C5).
    fn end_all(&self, cp: &mut ControlPlane) {
        let why = "the project was closed";
        while !cp.leases.live.is_empty() {
            let l = cp.leases.end(0, why);
            self.notices.lock().extend(l.unsaved(why));
        }
    }

    /// Register a lease on `control`, read in the published accepted revision, which the request
    /// named `name` (WRITE-22). One gesture owns a control at a time; a pending lease of the same
    /// window is taken over.
    fn register(&self, cp: &mut ControlPlane, device: Option<&Device>, control: Control, owner: Owner, name: (u64, &str)) -> Result<Begun> {
        let engine = cp.open.as_ref().context("no project is open")?.engine.clone();
        let now = Instant::now();
        let mut playing = control.value;
        if let Some(i) = cp.leases.live.iter().position(|l| l.token == engine.token && l.control.origin.same_value(&control.origin)) {
            let other = &cp.leases.live[i];
            let takeover =
                matches!(other.state, State::Pending { .. }) && other.owner.client.is_some() && other.owner.client == owner.client;
            if !takeover {
                bail!(
                    "this control is being edited by another gesture (for {:.1} s, last moved {:.1} s ago)",
                    now.duration_since(other.began).as_secs_f64(),
                    now.duration_since(other.touched).as_secs_f64()
                );
            }
            // The same window took hold of it again: its pending value isn't put back, and plays
            // on until this gesture moves it.
            playing = cp.leases.end(i, "superseded by a new gesture on the same control").playing;
        }
        // A lease begins only where a device already plays the project (it never starts one).
        let target = engine.plays.as_ref().filter(|p| device.is_some_and(|d| d.epoch == p.epoch)).and_then(|p| p.target(&control));
        let id = cp.leases.next_id();
        let begun = Begun {
            lease: id,
            revision: engine.revision,
            id: control.id.clone(),
            value: control.value,
            uses: control.uses,
            cells: target.as_ref().map_or(0, |t| t.cells.cells.len()),
            home: control.home,
            heard: target.is_some(),
        };
        let ttl = owner.ttl.unwrap_or(TTL).min(lease::MAX_TTL);
        cp.leases.live.push(Lease {
            id,
            token: engine.token,
            owner,
            ttl,
            began: now,
            touched: now,
            revision: engine.revision,
            code_rev: engine.code_rev,
            accepted: control.value,
            names: vec![(name.0, name.1.to_string())],
            ids: vec![(engine.revision, control.id.clone())],
            control,
            engine: target,
            seq: 0,
            playing,
            state: State::Live,
        });
        Ok(begun)
    }
}

/// The open project: a session served by its actor, a player, and the control plane between them.
pub struct Graph {
    inner: Arc<Inner>,
    jobs: mpsc::Sender<Job>,
}

impl Default for Graph {
    fn default() -> Graph {
        Graph::with(None)
    }
}

impl Graph {
    fn with(device: Option<Device>) -> Graph {
        // Tests: plugins' main-thread calls run on a dedicated thread (HOST-2, C1), as libtest
        // owns the process's main thread.
        #[cfg(test)]
        m2_plugins::test_main_thread();
        let (owner, events) = m2_plugins::Owner::new();
        let inner = Arc::new(Inner::new(device, owner));
        let (jobs, rx) = mpsc::channel::<Job>();
        let actor_inner = inner.clone();
        // QuickJS gets as much stack as it has on a main thread.
        std::thread::Builder::new()
            .name("m2-session".into())
            .stack_size(8 << 20)
            .spawn(move || actor::run(actor_inner, rx, events))
            .expect("a thread for the session");
        Graph { inner, jobs }
    }

    /// One whose player plays into nothing, for tests.
    #[cfg(test)]
    pub(crate) fn silent() -> Graph {
        Graph::with(Some(Device { player: Player::silent().expect("a silent player"), epoch: 0, resume: 0.0 }))
    }

    /// One whose player is `player`, such as a driven one ([`Player::driven`]) a test or a gate
    /// fills itself.
    #[doc(hidden)]
    pub fn with_player(player: Player) -> Graph {
        Graph::with(Some(Device { player, epoch: 0, resume: 0.0 }))
    }

    /// Run `f` on the session's actor and wait for its answer. A request that panics answers
    /// with an error, and the actor serves the next.
    fn call<T: Send + 'static>(&self, f: impl FnOnce(&mut Actor) -> Result<T> + Send + 'static) -> Result<T> {
        // The actor may wait for the plugin main thread (creating plugins): that thread must never
        // wait for the actor, or the two deadlock (design §2.7).
        debug_assert!(!m2_plugins::on_main_thread(), "the plugin main thread waited for the session");
        let (tx, rx) = mpsc::sync_channel(1);
        self.jobs
            .send(Box::new(move |a: &mut Actor| {
                let r = f(a);
                // A change may let a pending commit be written (C5): before the reply, so the
                // caller sees it.
                a.retry_pending();
                let _ = tx.send(r);
            }))
            .map_err(|_| anyhow!("the session has stopped"))?;
        rx.recv().map_err(|_| anyhow!("music2 hit a bug while doing that; the session carries on"))?
    }

    pub fn open(&self, dir: &Path) -> Result<Value> {
        let dir = dir.to_path_buf();
        let opened = self.call(move |a| a.open(&dir))?;
        // The loop it had when it was last open here (design-8b §2.2), set before anything plays.
        // One the song's tempo makes too short now is left off.
        match self.dir().and_then(|d| view_file::looped(&d)) {
            Some((range, on)) if self.set_loop(Some(range), Some(on)).is_ok() => self.inner.state().context("no project is open"),
            _ => Ok(opened),
        }
    }

    /// Close the project. Returns once its plugin instances are destroyed, or after 2 s.
    pub fn close(&self) -> Teardown {
        self.call(|a| Ok(a.close())).unwrap_or_default()
    }

    /// What every change to the open project is announced with: its session, the accepted
    /// revision, the problems that revision has with their fixes (DIAG-4), why the draft doesn't
    /// compile, and the gestures in progress (C6). Empty with no project open.
    pub fn news(&self) -> Value {
        let cp = self.inner.control.lock();
        let Some(p) = cp.open.as_ref() else { return json!({}) };
        let mut news = p.shown.news.clone();
        let now = Instant::now();
        news["leases"] = json!(cp.leases.live.iter().filter(|l| l.token == p.engine.token).map(|l| l.info(now)).collect::<Vec<_>>());
        news
    }

    /// The view, the timeline, the project's name and folder, and the transport.
    pub fn state(&self) -> Option<Value> {
        self.inner.state()
    }

    /// The open project's name, from its manifest (None with no project open).
    pub fn name(&self) -> Option<String> {
        Some(self.inner.control.lock().open.as_ref()?.shown.name.clone())
    }

    /// The open project's folder.
    pub fn dir(&self) -> Option<PathBuf> {
        Some(self.inner.control.lock().open.as_ref()?.shown.dir.clone())
    }

    /// An offline render of the accepted revision, which is what plays, with the song's name
    /// and folder, and what the export must carry: a plugin that isn't installed plays silence or
    /// passes its input through (FMT-18a). Only building it waits for the session: the caller
    /// renders it on a thread of its own.
    pub fn render(&self, length: Length) -> Result<(String, PathBuf, Render, Vec<m2_song::Diagnostic>)> {
        self.call(move |a| a.render(length))
    }

    /// The accepted revision ready to export (RENDER-1, RENDER-3). A song whose plugins play
    /// borrows their live instances ([`Export::lent`]): its thread pauses playback and takes each
    /// instance offline, holding its committed state; when it drops, they are given back, playback
    /// resumes, and the session plays what waited for it. Otherwise it renders with instances of
    /// its own, beside playback.
    pub fn export(&self, length: Length) -> Result<Export> {
        // What tells the session the export is over: given to the export on the actor, so a job
        // that fails after it still tells (the export's guard drops then).
        let jobs = self.jobs.clone();
        let mut export = self.call(move |a| a.export(length, jobs))?;
        export.problems.extend(self.overlays_warning());
        Ok(export)
    }

    /// What the engine is busy with besides playing: paused for an export, and the plugins being
    /// loaded for the next plan, by name (RENDER-1, RENDER-25); and what its device reported
    /// (RENDER-28).
    pub fn activity(&self) -> Activity {
        self.inner.activity()
    }

    /// The engine clock after the player's last callback, in frames (gates).
    #[doc(hidden)]
    pub fn engine_clock(&self) -> Option<u64> {
        self.inner.current_device().map(|d| d.player.clock())
    }

    /// The plugin instances of the plan last sent to the engine (gates).
    #[doc(hidden)]
    pub fn plugin_handles(&self) -> Vec<m2_plugins::Handle> {
        let handles =
            |a: &mut Actor| a.open.as_ref().and_then(|o| o.plays.as_ref()).map(|p| p.info.plugins.iter().map(|p| p.handle).collect());
        self.call(move |a| Ok(handles(a).unwrap_or_default())).unwrap_or_default()
    }

    /// Where playback is, as the audio thread last said: a seek it hasn't taken yet is where it
    /// goes. Never waits for the session (RENDER-29).
    pub fn status(&self) -> Status {
        self.inner.status()
    }

    /// Move playback to `beat` (at least 0), playing or not: a jump of the transport at the next
    /// block, never a rebuild (RENDER-9). Drags go on, and it never waits for an edit
    /// (RENDER-29). Notes already sounding at `beat` aren't chased (RENDER-11).
    pub fn seek(&self, beat: f64) -> Result<()> {
        // Asked for now: a wait for the locks below is part of what it takes (RENDER-29).
        let requested_ns = m2_play::now_ns();
        if !beat.is_finite() {
            bail!("can't seek to beat {beat}");
        }
        if self.inner.timing.lock().is_none() {
            bail!("no project is open");
        }
        let player = self.inner.device()?.player;
        let seq = player.seek(beat.max(0.0))?;
        let sent_ns = player.now_ns();
        *self.inner.last_edit.lock() = Some(EditTiming {
            requested_ns,
            ready_ns: requested_ns,
            compiled_ns: requested_ns,
            sent_ns,
            published_ns: sent_ns,
            seq: Some(seq),
            ..EditTiming::default()
        });
        Ok(())
    }

    /// The renderer the player plays, once it took everything sent: what tells a lane swap from a
    /// new renderer, for the gates. None with no player.
    #[doc(hidden)]
    pub fn generation(&self) -> Option<u64> {
        let p = self.inner.current_device()?.player;
        let t = Instant::now();
        while p.status().pending.is_some() && t.elapsed() < std::time::Duration::from_secs(2) {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        Some(p.status().generation)
    }

    /// When the last edit made here or the last seek reached the engine (see [`EditTiming`]).
    pub fn last_edit(&self) -> Option<EditTiming> {
        *self.inner.last_edit.lock()
    }

    /// When the last batch of outside writes the watcher settled reached the engine (WRITE-19), as
    /// [`EditTiming`] says of an edit: requested when the session's actor began taking it in.
    #[doc(hidden)]
    pub fn last_taken_in(&self) -> Option<EditTiming> {
        *self.inner.last_taken_in.lock()
    }

    /// When the engine took message `seq`, while it is among the last few it took: the stamp of
    /// the audio callback that rendered it, on [`Graph::now_ns`]'s clock.
    pub fn applied(&self, seq: u64) -> Option<m2_play::Applied> {
        self.inner.current_device()?.player.applied(seq)
    }

    /// The clock [`EditTiming`] and the engine's stamps count on, in nanoseconds. It takes no
    /// lock, so a call can be timed from its start, whatever it then waits for.
    pub fn now_ns(&self) -> u64 {
        m2_play::now_ns()
    }

    /// Since when the session's actor has been busy with a request (its compile included), on
    /// [`Graph::now_ns`]'s clock; None while it is idle. For the gates.
    pub fn busy(&self) -> Option<u64> {
        Some(self.inner.busy.lock().since).filter(|s| *s > 0)
    }

    /// When the actor's last few requests ran, oldest first, on [`Graph::now_ns`]'s clock.
    pub fn busy_spans(&self) -> Vec<(u64, u64)> {
        self.inner.busy.lock().spans.iter().copied().collect()
    }

    /// Set the loop: its range in beats of the accepted revision, which is kept while it is off,
    /// and whether playback loops there. Setting a range turns it on unless `on` says otherwise.
    /// Applied at the next block, and never waits for an edit (RENDER-29).
    pub fn set_loop(&self, range: Option<(f64, f64)>, on: Option<bool>) -> Result<Status> {
        let player = self.inner.device()?.player;
        let (range, on) = {
            let timing = self.inner.timing.lock();
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
        if let Some(t) = self.inner.timing.lock().as_mut() {
            (t.range, t.on) = (range, on);
        }
        // Kept for the next time the song opens, in this machine's `.music2/view.json`: never a Tx
        // (design-8b §2.2). A read-only window keeps it in memory only.
        if let (Some((start, end)), Some(dir), None) = (range, self.dir(), self.read_only())
            && let Err(e) = view_file::write(&dir, "loop", json!({"start": start, "end": end, "on": on}))
        {
            eprintln!("music2: couldn't keep the loop in .music2/view.json: {e:#}");
        }
        Ok(self.status())
    }

    // ------------------------------------------------------------ gesture leases (WRITE-22)

    /// Begin a gesture on the control `id`, as revision `revision` named it. Resolved against
    /// what the session last published, without waiting for it: a revision the control plane
    /// remembers, or one of the two code revisions before the accepted one's. Only a client
    /// further behind waits for the session to carry its id.
    pub fn begin(&self, token: u64, revision: u64, id: &str, owner: Owner) -> Result<Begun> {
        let device = self.inner.current_device();
        {
            let mut cp = self.inner.control.lock();
            let p = cp.open.as_ref().context("no project is open")?;
            if p.engine.token != token {
                bail!("the open project changed; refresh the graph before editing");
            }
            match p.find(revision, id) {
                Resolved::Control(c) => return self.inner.register(&mut cp, device.as_ref(), *c, owner, (revision, id)),
                Resolved::Missing => bail!("no control `{id}` in that revision"),
                Resolved::Slow => {}
            }
        }
        self.begin_via_actor(token, revision, id, owner)
    }

    /// [`Graph::begin`] through the session, which carries `id` from `revision` itself. Waits
    /// behind whatever the session is doing; the gates and tests use it as the negative control.
    #[doc(hidden)]
    pub fn begin_via_actor(&self, token: u64, revision: u64, id: &str, owner: Owner) -> Result<Begun> {
        let id = id.to_string();
        self.call(move |a| a.begin(token, revision, &id, owner))
    }

    /// Play `value` for lease `lease`, straight into the engine: a write to its cells' slots
    /// (`SetCell`, RENDER-22), never through the session or the queue, so never behind a compile
    /// or a full queue (WRITE-22, RENDER-28, RENDER-29). With `seq`, an update older than one
    /// taken is dropped (Ok(None)); without, it follows the last. Returns the cell write's
    /// sequence number ([`Graph::applied`] says when a block heard it), or None when nothing plays
    /// the control yet: the value is then written into the renderer that next plays.
    pub fn update(&self, lease: u64, seq: Option<u64>, value: f64) -> Result<Option<u64>> {
        let device = self.inner.current_device();
        let mut cp = self.inner.control.lock();
        let i = cp.leases.index(lease).map_err(|e| anyhow!(e))?;
        let l = &mut cp.leases.live[i];
        match l.state {
            State::Live => {}
            State::Closing { .. } => bail!("this gesture is being committed"),
            State::Pending { .. } => bail!("this gesture's value is waiting for the code to compile, to be saved"),
        }
        let seq = match seq {
            Some(s) if s <= l.seq => return Ok(None),
            Some(s) => s,
            None => l.seq + 1,
        };
        let v = match l.control.audition(value) {
            Ok(v) => v,
            Err(e) => {
                let l = cp.leases.end(i, e.to_string());
                put_back(&l, device.as_ref());
                bail!("{e}");
            }
        };
        (l.seq, l.playing, l.touched) = (seq, v, Instant::now());
        Ok(match (&l.engine, &device) {
            (Some(t), Some(d)) if t.epoch == d.epoch => Some(d.player.set_cells(&t.cells, v)),
            _ => None,
        })
    }

    /// Keep lease `lease` alive while its knob is held still.
    pub fn keepalive(&self, lease: u64) -> Result<()> {
        let mut cp = self.inner.control.lock();
        let i = cp.leases.index(lease).map_err(|e| anyhow!(e))?;
        cp.leases.live[i].touched = Instant::now();
        Ok(())
    }

    /// Commit lease `lease` at `value` (with `expect`, only while the code still says that): one
    /// verified value edit through the session, and the engine installs the value in the same
    /// step that ends the lease (WRITE-22). While the draft doesn't compile, the commit waits,
    /// pending, and the value plays on (C5).
    pub fn commit(&self, lease: u64, value: f64, expect: Option<f64>) -> Result<Committed> {
        {
            let device = self.inner.current_device();
            let mut cp = self.inner.control.lock();
            let i = cp.leases.index(lease).map_err(|e| anyhow!(e))?;
            let l = &mut cp.leases.live[i];
            match &mut l.state {
                State::Live => l.state = State::Closing { value, expect },
                State::Closing { .. } => bail!("this gesture is being committed"),
                State::Pending { value: v, expect: x, errors, .. } => {
                    // Still waiting: the newest value is the one to save, and it plays now.
                    let errors = errors.clone();
                    let played = l.control.audition(value).map_err(|e| anyhow!("{e}"))?;
                    (*v, *x) = (value, expect);
                    l.playing = played;
                    if let (Some(t), Some(d)) = (&l.engine, &device)
                        && t.epoch == d.epoch
                    {
                        d.player.set_cells(&t.cells, played);
                    }
                    return Ok(Committed::Pending { errors });
                }
            }
        }
        let committed = self.call(move |a| a.commit(lease));
        if committed.is_err() {
            // The actor failed without deciding (a bug): the lease mustn't stay closing forever.
            let device = self.inner.current_device();
            let mut cp = self.inner.control.lock();
            if let Ok(i) = cp.leases.index(lease)
                && matches!(cp.leases.live[i].state, State::Closing { .. })
            {
                let l = cp.leases.end(i, "its commit failed");
                put_back(&l, device.as_ref());
            }
        }
        committed
    }

    /// End lease `lease` without a commit: the code's value plays again. Acts on the control
    /// plane at once, never waiting for the session (C2). During a commit, the commit decides.
    pub fn cancel(&self, lease: u64) -> Result<()> {
        let device = self.inner.current_device();
        let mut cp = self.inner.control.lock();
        let Ok(i) = cp.leases.index(lease) else { return Ok(()) };
        if matches!(cp.leases.live[i].state, State::Closing { .. }) {
            bail!("this gesture is being committed; the commit decides");
        }
        let l = cp.leases.end(i, "cancelled");
        put_back(&l, device.as_ref());
        // An agent may cancel a released drag waiting for the code to compile (C5, C6).
        self.inner.notices.lock().extend(l.unsaved("cancelled"));
        Ok(())
    }

    /// End every lease idle past its TTL, and every commit that waited too long for the code to
    /// compile, putting their code values back (C6). Returns what to tell the user, with what
    /// the session ended or saved on its own since, and a commit heard as its preview that the
    /// disk is slow to make durable (PLANE-5). It never waits for the session's actor: the server
    /// calls it every 300 ms from a thread of its own, so this is said while the actor is busy.
    pub fn expire(&self) -> Vec<String> {
        let device = self.inner.current_device();
        let now = Instant::now();
        let mut said = vec![];
        let mut cp = self.inner.control.lock();
        let mut i = 0;
        while i < cp.leases.live.len() {
            let l = &cp.leases.live[i];
            let why = match &l.state {
                State::Live if now.duration_since(l.touched) > l.ttl => {
                    Some(format!("abandoned: no update for {} s", l.ttl.as_secs_f64().round()))
                }
                State::Pending { since, .. } if now.duration_since(*since) > lease::PENDING => Some(format!(
                    "the code didn't compile for {} s, so {}:{}'s value wasn't saved",
                    lease::PENDING.as_secs(),
                    l.control.file,
                    l.control.line
                )),
                _ => None,
            };
            match why {
                Some(why) => {
                    let l = cp.leases.end(i, why.clone());
                    put_back(&l, device.as_ref());
                    said.push(format!("{}:{}: {why}", l.control.file, l.control.line));
                }
                None => i += 1,
            }
        }
        drop(cp);
        if let Some(s) = self.inner.saving.lock().as_mut() {
            let playing = device.as_ref().is_some_and(|d| d.player.status().playing);
            s.played |= playing;
            if !s.said && s.since.elapsed() >= SLOW_SAVE {
                s.said = true;
                // Stopped, nothing plays it: the transport says what the user hears.
                let plays = if playing { " plays, but" } else { "" };
                said.push(format!(
                    "Saving is taking a while: the disk hasn't answered for {} s. The last edit{plays} isn't saved yet.",
                    SLOW_SAVE.as_secs()
                ));
            }
        }
        said.extend(std::mem::take(&mut *self.inner.notices.lock()));
        said
    }

    /// The window `client` closed (its WebSocket went away): its live leases end, and their code
    /// values play again. Its pending commits stay: it let go, so it meant to save.
    pub fn client_gone(&self, client: &str) {
        self.audition_gone(client);
        let device = self.inner.current_device();
        let mut cp = self.inner.control.lock();
        let mut i = 0;
        while i < cp.leases.live.len() {
            let l = &cp.leases.live[i];
            if l.owner.client.as_deref() == Some(client) && matches!(l.state, State::Live) {
                let l = cp.leases.end(i, "the window that owned it closed");
                put_back(&l, device.as_ref());
            } else {
                i += 1;
            }
        }
    }

    /// The gestures in progress.
    pub fn leases(&self) -> Vec<LeaseInfo> {
        let now = Instant::now();
        self.inner.control.lock().leases.live.iter().map(|l| l.info(now)).collect()
    }

    // ------------------------------------------------------------ the knob routes

    /// Change a value in the playing audio only, while a knob is dragged: an update of the
    /// gesture's lease, begun at its first preview. A gesture whose lease ended is refused with
    /// why.
    pub fn preview(&self, token: u64, revision: u64, id: &str, gesture: &str, value: f64) -> Result<()> {
        self.preview_for(None, token, revision, id, gesture, value)
    }

    /// [`Graph::preview`] from the window `client`, whose leases end when it closes.
    pub fn preview_for(&self, client: Option<&str>, token: u64, revision: u64, id: &str, gesture: &str, value: f64) -> Result<()> {
        let lease = match self.lease_of(token, revision, id, gesture)? {
            Some(l) => l,
            None => {
                let owner = Owner { client: client.map(str::to_string), gesture: Some(gesture.to_string()), ttl: None };
                self.begin(token, revision, id, owner)?.lease
            }
        };
        self.update(lease, None, value).map(|_| ())
    }

    /// The live lease of `gesture` on the control a compatible route's request names as
    /// `(revision, id)` (see [`Leases::of_gesture`]): None when there is none, and the request
    /// begins one or is a plain edit; refused with why when that lease ended. Resolved against
    /// what the session last published; only a request too old for that, from a gesture that
    /// holds a lease, waits for the session to carry its id.
    fn lease_of(&self, token: u64, revision: u64, id: &str, gesture: &str) -> Result<Option<u64>> {
        {
            let mut cp = self.inner.control.lock();
            let cp = &mut *cp;
            let p = cp.open.as_ref().context("no project is open")?;
            if p.engine.token != token {
                bail!("the open project changed; refresh the graph before editing");
            }
            let found = p.find(revision, id);
            let control = match &found {
                Resolved::Control(c) => Some(&**c),
                _ => None,
            };
            if let Some(i) = cp.leases.of_gesture(token, gesture, (revision, id), control) {
                return Ok(Some(cp.leases.named(i, (revision, id))));
            }
            if let Some(why) = cp.leases.ended_of(token, gesture, (revision, id)) {
                bail!("this gesture ended: {why}");
            }
            if !matches!(found, Resolved::Slow) || !cp.leases.holds(token, gesture) {
                return Ok(None);
            }
        }
        let (id, gesture) = (id.to_string(), gesture.to_string());
        self.call(move |a| a.lease_of(token, revision, &id, &gesture))
    }

    /// Close only the preview owned by this gesture on the control `id` names (as the gesture
    /// named it, or as the accepted revision does). Old callbacks are harmless: a gesture whose
    /// lease already ended, or another project's, changes nothing. Its own client ended it, so
    /// nothing remembers why: the same gesture string may begin on that control again (a script
    /// sends one gesture for every knob). During a commit, the commit decides.
    pub fn cancel_preview(&self, token: u64, id: &str, gesture: &str) -> Result<()> {
        let device = self.inner.current_device();
        let mut cp = self.inner.control.lock();
        let mine = |l: &Lease| l.token == token && l.owner.gesture.as_deref() == Some(gesture);
        let named = |l: &Lease| l.names.iter().any(|n| n.1 == id) || l.control.id == id;
        let (mut i, mut closing) = (0, false);
        while i < cp.leases.live.len() {
            let l = &cp.leases.live[i];
            if !mine(l) || !named(l) || matches!(l.state, State::Closing { .. }) {
                closing |= mine(l) && named(l);
                i += 1;
                continue;
            }
            let l = cp.leases.close(i);
            put_back(&l, device.as_ref());
            self.inner.notices.lock().extend(l.unsaved("cancelled"));
        }
        if closing {
            bail!("this gesture is being committed; the commit decides");
        }
        Ok(())
    }

    /// Commit a value to code: the gesture's lease on that control when it has one, else a plain
    /// verified value edit (an agent's). Errors carry what went wrong (stale, conflict, …). While
    /// the draft doesn't compile, a gesture's commit is pending: `{"pending": true, "errors": …}`
    /// (C5).
    pub fn set(&self, token: u64, revision: u64, id: &str, gesture: &str, value: f64, expect: Option<f64>) -> Result<Value> {
        match self.lease_of(token, revision, id, gesture)? {
            Some(lease) => match self.commit(lease, value, expect)? {
                Committed::Written(v) => Ok(v),
                Committed::Pending { errors } => Ok(json!({"pending": true, "errors": errors})),
            },
            None => {
                let (id, gesture) = (id.to_string(), gesture.to_string());
                self.call(move |a| a.set_value(token, revision, &id, &gesture, value, expect))
            }
        }
    }

    // ------------------------------------------------------------ edits through the session

    /// Connect a named signal to a control's input, in code. The graph changes shape, so a new
    /// renderer takes over where playback is.
    pub fn connect(&self, token: u64, revision: u64, from: &str, to: &str) -> Result<Value> {
        let (from, to) = (from.to_string(), to.to_string());
        self.edit(token, move |s| s.connect(revision, &from, &to))
    }

    /// Disconnect a cable, writing the input's declared default in code.
    pub fn disconnect(&self, token: u64, revision: u64, id: &str) -> Result<Value> {
        let id = id.to_string();
        self.edit(token, move |s| s.disconnect(revision, &id))
    }

    /// Put one of the packages' effects, named by its ComponentId (`std:delay`), after a call's
    /// audio, in code.
    pub fn add_effect(&self, token: u64, revision: u64, call: &str, component: &str) -> Result<Value> {
        let (call, component) = (call.to_string(), component.to_string());
        self.edit(token, move |s| s.add_effect(revision, &call, &component))
    }

    /// Take an effect out, leaving what it was applied to.
    pub fn remove_effect(&self, token: u64, revision: u64, call: &str) -> Result<Value> {
        let call = call.to_string();
        self.edit(token, move |s| s.remove_effect(revision, &call))
    }

    /// Rewrite a notes lane's clips (the piano roll, or clips made, moved or deleted in the
    /// timeline). Only the lane's data changes, so it is swapped into the playing renderer
    /// (RENDER-26): heard at once, with nothing cut, and a drag goes on.
    pub fn write_notes(&self, token: u64, revision: u64, module: &str, lane: &str, clips: &[NoteClip]) -> Result<Value> {
        let (module, lane, clips) = (module.to_string(), lane.to_string(), clips.to_vec());
        self.edit(token, move |s| s.write_notes(revision, &module, &lane, &clips))
    }

    /// Move data no lane or plugin reads from `from` to `to`, project paths: a lane file, a
    /// track's folder or a plugin state (see [`m2_editor::data::relink`]).
    pub fn relink(&self, token: u64, revision: u64, from: &str, to: &str) -> Result<Value> {
        let (from, to) = (PathBuf::from(from), PathBuf::from(to));
        self.edit(token, move |s| s.relink(revision, &from, &to))
    }

    /// Delete data no lane or plugin reads (see [`m2_editor::data::delete_data`]); undo puts it
    /// back.
    pub fn delete_data(&self, token: u64, revision: u64, file: &str) -> Result<Value> {
        let file = PathBuf::from(file);
        self.edit(token, move |s| s.delete_data(revision, &file))
    }

    /// Why the open project can't be written from here (WRITE-17, WRITE-18): another program holds
    /// its lock, or its volume can't lock. None when it can, or none is open.
    pub fn read_only(&self) -> Option<String> {
        self.inner.control.lock().open.as_ref()?.shown.read_only.clone()
    }

    /// Try to take the open project's lock again (the read-only banner's Retry): when it is free,
    /// the session can write. Answers with the state.
    pub fn retry_lock(&self, token: u64) -> Result<Value> {
        self.call(move |a| a.retry_lock(token))
    }

    /// Show a track, named by its key, as `label` (ID-10): its `track()` call's name, verified to
    /// change nothing that plays. Undo takes it back.
    pub fn label(&self, token: u64, module: &str, label: &str) -> Result<Value> {
        let (module, label) = (module.to_string(), label.to_string());
        self.edit(token, move |s| s.rename_label(&module, &label))
    }

    /// Automate a control, as named at `revision` (HOME-2): its value becomes the fallback of a
    /// new, empty control lane read where it was. The song sounds the same; the answer names the
    /// lane (`lane: {module, lane}`) and the knob (`name`).
    pub fn automate(&self, token: u64, revision: u64, id: &str) -> Result<Value> {
        let id = id.to_string();
        self.call(move |a| {
            a.edit_and(token, |s| {
                let done = s.automate(revision, &id)?;
                let lane = json!({"module": done.lane.module.as_str(), "lane": done.lane.lane.as_str()});
                Ok((done.edited, json!({"lane": lane, "name": done.name})))
            })
        })
    }

    /// Add a track playing instrument `component` (`std:synth-pluck`), named `label` (else the
    /// instrument's), keyed `key` (else proposed from the label), below track `after` (WRITE-10).
    /// The answer names the track: `module` and `label`.
    pub fn add_track(&self, token: u64, component: &str, label: Option<String>, key: Option<String>, after: Option<String>) -> Result<Value> {
        let component = component.to_string();
        let added = self.call(move |a| {
            a.edit_and(token, |s| {
                let made = s.add_track(&component, label.as_deref(), key.as_deref(), after.as_deref())?;
                Ok((made.edited, json!({"module": made.module, "label": made.label})))
            })
        })?;
        self.forget_made(&added);
        Ok(added)
    }

    /// A track just made under a key a gone track's mute or solo was kept for starts heard (Q7).
    fn forget_made(&self, made: &Value) {
        if let Some(k) = made["module"].as_str() {
            self.forget_overlays(&[k.to_string()]);
        }
    }

    /// A mixer channel's first pan (design-8b §5.3): write optional control `control` (`pan`) of
    /// channel `channel` as `value` into its entry. The answer names the new knob: `control`, its id
    /// in the revision the answer gives, which the drag's lease then names.
    pub fn mix_insert(&self, token: u64, channel: &str, control: &str, value: f64) -> Result<Value> {
        let (channel, control) = (channel.to_string(), control.to_string());
        self.call(move |a| {
            a.edit_and(token, |s| {
                let done = s.mix_insert(&channel, &control, value)?;
                Ok((done.edited, json!({"control": done.control})))
            })
        })
    }

    /// Duplicate track `module`: its code, data and channel, below it in its colour.
    pub fn duplicate_track(&self, token: u64, module: &str) -> Result<Value> {
        let module = module.to_string();
        let copy = self.call(move |a| {
            a.edit_and(token, |s| {
                let made = s.duplicate_track(&module)?;
                Ok((made.edited, json!({"module": made.module, "label": made.label, "new_channel": made.new_channel})))
            })
        })?;
        self.forget_made(&copy);
        Ok(copy)
    }

    /// Delete track `module`. The status the route answers: 200 when done (with the calls taken
    /// out with it), or 409 with the dependents when other code reads it: `blocked` when that can't
    /// be taken out from here, else they go with `cascade`.
    pub fn delete_track(&self, token: u64, module: &str, cascade: bool) -> Result<(u16, Value)> {
        let gone = module.to_string();
        let module = module.to_string();
        // What a delete that waits found, kept beside the edit, which wrote nothing.
        let waits: Arc<Mutex<Option<Waits>>> = Arc::default();
        let found = waits.clone();
        let done = self.call(move |a| {
            a.edit_and(token, |s| match s.delete_track(&module, cascade)? {
                m2_editor::Deleted::Done { edited, excised } => Ok((edited, json!({"excised": excised}))),
                m2_editor::Deleted::Asks(d) => {
                    *found.lock() = Some((false, d));
                    Err(m2_editor::EditError::Rejected("other tracks read it".into()))
                }
                m2_editor::Deleted::Blocked(d) => {
                    *found.lock() = Some((true, d));
                    Err(m2_editor::EditError::Rejected("other code reads it".into()))
                }
            })
        });
        if let Some((blocked, deps)) = waits.lock().take() {
            let error = match blocked {
                true => "other code reads this track in a way that can't be taken out from here: change it in code",
                false => "other tracks read this track: delete again with cascade to take those reads out too",
            };
            return Ok((409, json!({"error": error, "blocked": blocked, "dependents": deps})));
        }
        let done = done?;
        // Its mute and solo go with it (Q7): undone, it comes back heard.
        self.forget_overlays(&[gone]);
        Ok((200, done))
    }

    /// A layout write (FMT-19): the tracks' new `order`, one track's `colour`, or with `reset` the
    /// defaults over a file that doesn't read. No revision (PLANE-3); one undo step.
    pub fn layout(&self, token: u64, order: Option<Vec<String>>, colour: Option<(String, String)>, reset: bool) -> Result<Value> {
        self.edit(token, move |s| match (order, colour, reset) {
            (_, _, true) => m2_editor::layout::reset(s),
            (Some(order), None, false) => s.reorder(&order),
            (None, Some((module, colour)), false) => s.recolour(&module, &colour),
            _ => Err(m2_editor::EditError::Rejected("a layout write is an order, a colour or a reset".into())),
        })
    }

    /// The song bar's tempo (design-8b §4.1): the tempo in effect at beat `at` (the playhead's) set
    /// to `bpm`; with `add`, a change at `at`; with `remove`, the change at `at` taken out, and with
    /// `meter` too the time signature change there, in the same step. One undo step. When the loop was on and the new tempo makes it too short to play, the engine turns it
    /// off (RENDER-9a) and the answer says `loop_off`.
    pub fn tempo(&self, token: u64, at: f64, bpm: Option<f64>, add: bool, remove: bool, meter: bool) -> Result<Value> {
        let looped = self.inner.timing.lock().as_ref().and_then(|t| t.range.filter(|_| t.on));
        // The tempo map the edit made, to judge the loop in: the engine plays it from the switch,
        // which an export's hold puts off, so the timing's map may still be the one before.
        let made: Arc<Mutex<Option<(TempoMap, u32)>>> = Arc::default();
        let kept = made.clone();
        let mut v = self.edit(token, move |s| {
            let edited = match (bpm, add, remove) {
                (_, _, true) if meter => m2_editor::song::remove_changes(s, at),
                (_, _, true) => m2_editor::song::remove_tempo(s, at),
                (Some(bpm), true, false) => m2_editor::song::tempo_change(s, at, bpm),
                (Some(bpm), false, false) => m2_editor::song::tempo(s, at, bpm),
                (None, _, false) => Err(m2_editor::EditError::Rejected("a tempo needs its BPM".into())),
            }?;
            let rev = s.accepted();
            *kept.lock() = Some((rev.tempo(), rev.project.sample_rate()));
            Ok(edited)
        })?;
        if let (Some((start, end)), Some((tempo, rate))) = (looped, made.lock().take()) {
            let short = loop_fits(&tempo, rate, start, end).is_err();
            let mut timing = self.inner.timing.lock();
            if let Some(t) = timing.as_mut().filter(|_| short) {
                // Off here too, as the engine has it: C turns it on again, if it can play.
                t.on = false;
            }
            v["loop_off"] = json!(short);
        }
        Ok(v)
    }

    /// The song bar's time signature: the one in effect at beat `at` set to `beats`/`unit`; with
    /// `add`, a change on the bar line at or before `at`; with `remove`, the change at `at` taken out.
    pub fn meter(&self, token: u64, at: f64, beats: Option<u32>, unit: Option<u32>, add: bool, remove: bool) -> Result<Value> {
        self.edit(token, move |s| match (beats.zip(unit), add, remove) {
            (_, _, true) => m2_editor::song::remove_meter(s, at),
            (Some((beats, unit)), true, false) => m2_editor::song::meter_change(s, at, beats, unit),
            (Some((beats, unit)), false, false) => m2_editor::song::meter(s, at, beats, unit),
            (None, _, false) => Err(m2_editor::EditError::Rejected("a time signature needs its beats and unit".into())),
        })
    }

    /// Call the song `name`: its manifest's `[project] name`, verified to change nothing that plays.
    pub fn title(&self, token: u64, name: &str) -> Result<Value> {
        let name = name.to_string();
        self.edit(token, move |s| m2_editor::song::title(s, &name))
    }

    /// Stop all sound (⌘., RENDER-10): every note off and every instance reset, at the next block.
    /// It never waits for the session, and changes nothing in it. With no player there is no
    /// sound, so it does nothing: it never opens the audio device.
    pub fn panic(&self) -> Result<()> {
        if let Some(d) = self.inner.current_device() {
            d.player.panic()?;
        }
        Ok(())
    }

    /// Apply a fix a problem offers, as the problem carried it (`{code, title, likely, tx}`):
    /// committing its Tx (DIAG-1), which its bases guard. Undo takes it back.
    pub fn fix(&self, token: u64, fix: &Value) -> Result<Value> {
        let tx = m2_song::Tx::from_json(fix.get("tx").context("a fix carries its Tx")?)
            .map_err(|d| anyhow!("{}", d.iter().map(|d| d.message.clone()).collect::<Vec<_>>().join("; ")))?;
        self.edit(token, move |s| m2_editor::data::apply_tx(s, tx))
    }

    /// Check an agent's Tx (WRITE-4, WRITE-11): its delta, the delta's hash and the
    /// diagnostics, as `m2 tx --check` prints them; nothing is written. `token` names the open
    /// project when given. The status says how it went: 200, or 409 for a conflict, 422 for a
    /// rejection, 500 for a disk that failed.
    pub fn tx_check(&self, token: Option<u64>, tx: &Value) -> Result<(u16, Value)> {
        match parse_tx(tx) {
            Ok(tx) => self.call(move |a| a.tx(token, tx, false)),
            Err(answer) => Ok(answer),
        }
    }

    /// Commit an agent's Tx (WRITE-5), and play what it made: one JSON shape with `m2 tx`.
    pub fn tx_commit(&self, token: Option<u64>, tx: &Value) -> Result<(u16, Value)> {
        match parse_tx(tx) {
            Ok(tx) => self.call(move |a| a.tx(token, tx, true)),
            Err(answer) => Ok(answer),
        }
    }

    /// Undo the last edit made through this session: its stored inverse (WRITE-21).
    pub fn undo(&self, token: u64) -> Result<Value> {
        self.edit(token, m2_editor::undo)
    }

    pub fn redo(&self, token: u64) -> Result<Value> {
        self.edit(token, m2_editor::redo)
    }

    fn edit(
        &self,
        token: u64,
        edit: impl FnOnce(&mut m2_session::Session) -> Result<m2_editor::Edited, m2_editor::EditError> + Send + 'static,
    ) -> Result<Value> {
        self.call(move |a| a.edit(token, edit))
    }

    /// Play, stop, toggle between them, or go back to the start. Stop ends the notes, and the
    /// engine plays on so tails decay (RENDER-6a). Never waits for an edit (RENDER-29).
    pub fn transport(&self, action: &str) -> Result<()> {
        let player = self.inner.device()?.player;
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

    /// Stop, and go back to `to`, where play started (taste.md §5.1): the stop, then a seek, sent
    /// together, so the status says it stopped there at once ([`Player::status`] reports both as
    /// done until the audio thread takes them). Answers where it stopped, for ⇧Space to continue
    /// from: where playback was as the stop was sent, a block before it took it at most. Never
    /// waits for an edit (RENDER-29).
    pub fn stop_to(&self, to: f64) -> Result<f64> {
        if !to.is_finite() {
            bail!("can't go back to beat {to}");
        }
        let at = self.status().beat;
        self.transport("stop")?;
        self.seek(to)?;
        Ok(at)
    }

    /// One tick of the watcher (WRITE-19): what file system events named, or a stat walk of the
    /// open project (see [`watcher`]), fed to the settle rule. A batch that settled (no tracked file
    /// changed for 150 ms, each the same on two polls, git not writing) is read once and taken in
    /// whole. Returns whether clients should refresh. The server's watcher calls it every 50 ms.
    pub fn watch(&self) -> bool {
        let (ticked, layout) = self.inner.tick();
        let relayout = self.layout_moved(layout);
        let Some((token, ready)) = ticked else { return relayout && self.republish() };
        let settled = self.call(move |a| Ok(a.settled(token, ready))).unwrap_or_else(|e| {
            eprintln!("music2: {e:#}");
            false
        });
        settled || (relayout && self.republish())
    }

    /// Whether `layout.json` changed and has held still since the last look (FMT-19): its size and
    /// time, compared when the tick says to `look` (every tick of a walk; with events, when one named
    /// it or the tick walked everything) and on every tick while a change waits to hold still, so an
    /// outside edit (an agent, git) shows within two ticks of its event, and an idle project is
    /// watched with no stat (WRITE-19).
    fn layout_moved(&self, look: bool) -> bool {
        let Some((token, dir)) = self.inner.control.lock().open.as_ref().map(|p| (p.engine.token, p.shown.dir.clone())) else { return false };
        {
            let seen = self.inner.layout.lock();
            if seen.token == token && !look && !seen.changed {
                return false;
            }
        }
        let stamp = std::fs::metadata(dir.join(m2_song::path::LAYOUT)).ok().map(|m| (m.len(), m.modified().unwrap_or(std::time::UNIX_EPOCH)));
        let mut seen = self.inner.layout.lock();
        seen.looks += 1;
        if seen.token != token {
            *seen = LayoutSeen { token, stamp, changed: false, looks: seen.looks };
            return false;
        }
        if seen.stamp != stamp {
            (seen.stamp, seen.changed) = (stamp, true);
            return false;
        }
        std::mem::take(&mut seen.changed)
    }

    /// Publish the view again with no revision: the layout changed (PLANE-3).
    fn republish(&self) -> bool {
        self.call(|a| Ok(a.republish())).unwrap_or(false)
    }

    /// Wait for the watcher's next tick: `most` at most, less when an event names a path while no
    /// batch is open (looked at at once), or when the open batch becomes due (its 150 ms of quiet
    /// end then). The server's watcher thread waits so between ticks.
    pub fn watch_wait(&self, most: std::time::Duration) {
        let until = Instant::now() + most;
        let (due, heard) = self.inner.watch.lock().as_ref().map_or((None, false), |w| w.wake_at());
        self.inner.wake.wait(due.map_or(until, |d| d.min(until)), heard);
    }

    /// What the open project's watcher has done (see [`WatchStats`]), for the gates.
    #[doc(hidden)]
    pub fn watch_stats(&self) -> Option<WatchStats> {
        self.inner.watch.lock().as_ref().map(|w| w.stats)
    }

    /// Watch projects opened from now on by walks alone, never file system events, as on a
    /// network volume: the gates' walk mode (`M2_WATCH=walk` does the same for a whole run).
    #[doc(hidden)]
    pub fn watch_by_walking(&self, on: bool) {
        self.inner.walk_only.store(on, Ordering::Relaxed);
    }

    /// Bring the engine up to the accepted revision when it is behind it (a renderer that didn't
    /// build, a player that stopped responding), without reading the project. Returns whether
    /// clients should refresh. The server's watcher calls it every 300 ms. While the actor is
    /// busy it does nothing: what it serves settles when done.
    pub fn catch_up(&self) -> bool {
        if self.busy().is_some() {
            return false;
        }
        self.call(|a| Ok(a.catch_up())).unwrap_or_else(|e| {
            eprintln!("music2: {e:#}");
            false
        })
    }

    /// Take in what the project's folder says now, at once, without the settle rule: tests, and
    /// the gates that time a save from its write. Returns whether clients should refresh.
    pub fn poll(&self) -> bool {
        self.call(|a| Ok(a.poll())).unwrap_or_else(|e| {
            eprintln!("music2: {e:#}");
            false
        })
    }

    /// The project's TypeScript files, for the code editor (see [`files::list`]): the accepted
    /// revision's as published, with what the disk says now.
    pub fn files(&self) -> Result<Value> {
        let shown = self.inner.control.lock().open.as_ref().context("no project is open")?.shown.clone();
        files::list(&shown.files)
    }

    /// The SDK and package sources the code editor checks imports against.
    pub fn types(&self) -> Result<Value> {
        let shown = self.inner.control.lock().open.as_ref().context("no project is open")?.shown.clone();
        Ok((*shown.types).clone())
    }

    /// Save a file from the code editor if it still says `base` (else a [`files::Conflict`]),
    /// and adopt it at once, so the state returned shows it. Code that doesn't compile is
    /// saved too: the last revision that did plays on, and the diagnostics say why. A save
    /// that compiles is an edit made here, which undo takes back like any other.
    pub fn write_file(&self, token: Option<u64>, path: &str, base: &str, text: &str) -> Result<Value> {
        let (path, base, text) = (path.to_string(), base.to_string(), text.to_string());
        self.call(move |a| a.write_file(token, &path, &base, &text))
    }

    /// The code editor of project `token` says which of its files have unsaved edits.
    pub fn set_unsaved(&self, token: u64, files: Vec<String>) -> Result<()> {
        let open = self.inner.control.lock().open.as_ref().map(|p| p.engine.token);
        if open.context("no project is open")? != token {
            bail!("the open project changed; refresh the graph before editing");
        }
        *self.inner.unsaved.lock() = (token, files);
        Ok(())
    }

    /// The open project's files with edits in the code editor not yet saved, as it last said.
    pub fn unsaved(&self) -> Vec<String> {
        let Some(token) = self.inner.control.lock().open.as_ref().map(|p| p.engine.token) else { return vec![] };
        let unsaved = self.inner.unsaved.lock();
        if unsaved.0 == token { unsaved.1.clone() } else { vec![] }
    }

    /// Hold the session's actor until the returned guard is dropped, as a long compile would.
    #[cfg(test)]
    pub(crate) fn hold(&self) -> mpsc::Sender<()> {
        let (tx, rx) = mpsc::channel::<()>();
        let (held, started) = mpsc::channel();
        self.jobs
            .send(Box::new(move |_: &mut Actor| {
                let _ = held.send(());
                let _ = rx.recv();
            }))
            .unwrap();
        started.recv().unwrap();
        tx
    }

    /// Run `f` on the actor, with the open project, for tests.
    #[cfg(test)]
    pub(crate) fn inspect<R: Send + 'static>(&self, f: impl FnOnce(&mut Actor) -> R + Send + 'static) -> R {
        self.call(move |a| Ok(f(a))).unwrap()
    }
}

#[cfg(test)]
mod forget_tests;
#[cfg(test)]
mod preview_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod watch_tests;
