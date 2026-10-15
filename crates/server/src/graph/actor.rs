//! The session's actor (WRITE-22): one thread owns the [`Session`] and serves requests one at a
//! time, first in, first out. Each may compile; nothing else waits for it but the request's own
//! caller.
//!
//! After every change the actor settles: it plays the accepted revision (a lane swap, cell
//! writes, or a plan prepared against what plays and activated where the engine is, keeping every
//! instance that continues), carries every gesture lease to it, and publishes it
//! to the control plane in two phases: first what the engine and leases need, under the lock
//! and with the engine messages, then the view and the news, before the request replies.

use std::collections::{HashMap, HashSet, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};
use m2_editor::render::{Length, Render};
use m2_editor::{Control, EditError, Edited, Gestures};
use m2_ir::{FOp, Flat, Kind, Origin};
use m2_plugins::{Handle, HostEvent, HostEvents};
use m2_runtime::cells::{CellAt, CellId, CellTarget};
use m2_runtime::{Basis, CellMap, Continues, Keep, LaneSwap, PackageSet, PlanInfo, Renderer, audio, notes_data};
use m2_session::{Revision, Session};
use m2_song::Snapshot;
use serde_json::{Value, json};

use super::lease::{Committed, PlaysView, Revs, State};
use super::{Device, EditTiming, EngineView, Inner, Published, Shown, Timing, preview, put_back, watcher};
use crate::files;

/// Free `old` on a thread of its own, so a large value (a view of every note) is never dropped on
/// the actor's way to a reply. When that thread is behind, it is freed here, so nothing piles up.
fn retire<T: Send + 'static>(old: T) {
    static DROPS: std::sync::OnceLock<Option<mpsc::SyncSender<Box<dyn Send>>>> = std::sync::OnceLock::new();
    let drops = DROPS.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel::<Box<dyn Send>>(4);
        let spawned = std::thread::Builder::new().name("m2-retire".into()).spawn(move || rx.into_iter().for_each(drop));
        spawned.ok().map(|_| tx)
    });
    if let Some(tx) = drops
        && let Err(mpsc::TrySendError::Full(old) | mpsc::TrySendError::Disconnected(old)) = tx.try_send(Box::new(old))
    {
        drop(old);
    }
}

/// A request: run on the actor with its state, answering through what it captured.
pub(super) type Job = Box<dyn FnOnce(&mut Actor) + Send>;

/// Tests: what runs between a settle's two phases.
#[cfg(test)]
pub(super) type Hook = Box<dyn FnMut(&Inner) + Send>;

pub(crate) struct Actor {
    inner: Arc<Inner>,
    pub(crate) open: Option<Open>,
    /// What this Graph's plugin instances report unasked (its [`m2_plugins::Owner`]'s): no other
    /// session, test or rig sees them.
    events: HostEvents,
    /// Tests: every renderer build fails, as one whose plugin can't load would.
    #[cfg(test)]
    pub(crate) fail_builds: bool,
    /// Tests: run between a settle's two phases, with what it published so far.
    #[cfg(test)]
    pub(super) between_phases: Option<Hook>,
    /// Tests: every session opened from now on commits through the disk this makes.
    #[cfg(test)]
    pub(super) disk: Option<Box<dyn Fn() -> Box<dyn m2_session::tx::stage::Disk> + Send>>,
}

/// Serve requests until the [`super::Graph`] is dropped.
pub(super) fn run(inner: Arc<Inner>, jobs: mpsc::Receiver<Job>, events: HostEvents) {
    let mut actor = Actor {
        inner,
        open: None,
        events,
        #[cfg(test)]
        fail_builds: false,
        #[cfg(test)]
        between_phases: None,
        #[cfg(test)]
        disk: None,
    };
    while let Ok(job) = jobs.recv() {
        // On the player's clock: 0 until there is a player (an open starts it).
        let start = actor.inner.now_ns();
        actor.inner.busy.lock().since = start.max(1);
        actor.heard_plugins();
        // A request that panics answers with an error (its reply is dropped unsent), and the
        // actor serves the next.
        if catch_unwind(AssertUnwindSafe(|| job(&mut actor))).is_err() {
            eprintln!("music2: the session hit a bug; that request failed");
        }
        actor.told_watcher();
        let mut busy = actor.inner.busy.lock();
        busy.since = 0;
        if start > 0 {
            busy.spans.push_back((start, actor.inner.now_ns()));
        }
        while busy.spans.len() > 16 {
            busy.spans.pop_front();
        }
    }
}

impl Actor {
    /// Tell the watcher what the session wrote itself, so its own writes make no batch (R12).
    fn told_watcher(&mut self) {
        let Some(o) = self.open.as_mut() else { return };
        let wrote = o.session.take_wrote();
        if let Some(w) = self.inner.watch.lock().as_mut().filter(|w| w.token == o.token) {
            w.wrote(&wrote, o.session.accepted());
        }
    }
}

/// The open project, as the actor owns it.
pub(crate) struct Open {
    pub(crate) token: u64,
    pub(crate) session: Session,
    /// The accepted revision the player is playing: None while a commit's candidate plays, as its
    /// preview (PLANE-5), until the settle after it.
    pub(crate) playing: Option<u64>,
    /// What the engine plays, to tell lane data from structure. None until the project is loaded
    /// into the player.
    pub(crate) plays: Option<Plays>,
    /// Decoded assets and clip renders, shared by every renderer built for this project, so an
    /// activation never decodes or warps them again (RENDER-27).
    audio: audio::Cache,
    /// The engine message the last settle sent the new revision in (a swap or an activation).
    pub(crate) sent: Option<u64>,
    /// The session's code revision of the accepted revision the last settle saw (WRITE-13): a
    /// lane write leaves it, so leases and ids are left alone.
    pub(crate) code_rev: u64,
    /// The accepted revision's controls, by id.
    controls: Arc<HashMap<String, Control>>,
    /// The controls of the two code revisions before the accepted one's, oldest first: what a
    /// client a little behind names, which aliases carry to the accepted ids.
    codes: VecDeque<(u64, Arc<HashMap<String, Control>>)>,
    types: Arc<Value>,
    /// Plugin instances that asked to be restarted (`HostEvent::Restart`): the next activation,
    /// made at once, replaces each from its committed state (HOST-5).
    pub(crate) restarts: HashSet<Handle>,
    /// An export borrows the live plugin instances (RENDER-1): settles prepare nothing and send no
    /// activation until it gives them back, so no host call reaches them meanwhile.
    pub(crate) exporting: bool,
    /// What preparing the last plan took, for the edit's timing.
    pub(super) prepared: Prepared,
    /// The timeline last published, whose lanes the next one shares where they are the same.
    timeline: Arc<super::Timeline>,
}

/// The plan the engine plays: what it was built from, and how `SwapLane` reaches its lanes.
pub(crate) struct Plays {
    /// The player it was loaded into ([`Device::epoch`]).
    pub(crate) epoch: u64,
    pub(crate) generation: u64,
    rate: u32,
    /// The activations the player had refused when it was sent: one more means it never played,
    /// and a fresh engine replaces what does (RENDER-28).
    pub(super) refused: u64,
    /// What it plays: an accepted revision (the one it was built from, or adopted since: lanes
    /// swapped in, cells written), or a commit's candidate while that is its preview (PLANE-5).
    /// Every settle's engine path is the delta from it to the accepted one (RENDER-26; the step 6
    /// design's R4), so a change that never reached the engine (a build that failed) is never
    /// skipped by a later one, and a preview whose commit failed is swapped back.
    pub(crate) revision: Arc<Revision>,
    /// Its lanes (and each notes lane's room: data that needs more is activated instead), its
    /// shapes and instances, which the next plan is prepared against, and its cells' slots.
    pub(super) info: Arc<PlanInfo>,
    /// The controls of the code it plays: after a build that failed, what updates aim at.
    controls: Arc<HashMap<String, Control>>,
    /// Which cells hold each value of the revision it plays: what gesture updates and committed
    /// values are written to (RENDER-22, RENDER-23).
    cells: Arc<CellMap>,
}

/// Whether two checked graphs number their cells alike (the same constants at the same places),
/// so the cells of a renderer built from one hold the other's values by the same CellIds: what
/// lets a value edit, or an inherited control's first edit (which only moves its origin), be
/// taken by what plays without a new renderer.
fn same_layout(a: &Flat, b: &Flat) -> bool {
    let (x, y) = (m2_runtime::cells::cells(a), m2_runtime::cells::cells(b));
    x.len() == y.len() && x.iter().zip(&y).all(|(p, q)| p.at == q.at)
}

/// The cells whose constants differ from `old` to `new`, with their new values, when nothing else
/// differs in how the two graphs process (where each node was written aside). None when anything
/// does: a renderer of `old` can't play `new`. The delta says what changed by name; this checks
/// that a renderer's cells can carry it, constants folded into kernels included.
fn value_writes(old: &Flat, new: &Flat) -> Option<Vec<(CellId, f64)>> {
    if old == new {
        return Some(vec![]);
    }
    let (a, b) = (m2_runtime::cells::cells(old), m2_runtime::cells::cells(new));
    if a.len() != b.len() || a.iter().zip(&b).any(|(p, q)| p.at != q.at) {
        return None;
    }
    // Both with the new values in their cells and no positions: equal when the cells are all.
    let bare = |f: &Flat| {
        let mut f = f.clone();
        for c in &b {
            let node = match c.at {
                CellAt::Top(i) => Some(&mut f.nodes[i]),
                CellAt::Voice { poly, node } => match &mut f.nodes[poly].op {
                    FOp::Poly { voice, .. } => voice.nodes.get_mut(node),
                    _ => None,
                },
            };
            if let Some(n) = node {
                n.op = FOp::Const(c.value);
            }
        }
        for n in &mut f.nodes {
            (n.src, n.origin) = (None, None);
            if let FOp::Poly { voice, .. } = &mut n.op {
                for v in &mut voice.nodes {
                    (v.src, v.origin) = (None, None);
                }
            }
        }
        f
    };
    if bare(old) != bare(new) {
        return None;
    }
    let mut out = vec![];
    for (i, (p, q)) in a.iter().zip(&b).enumerate() {
        if p.value.to_bits() != q.value.to_bits() {
            out.push((CellId(i as u32), q.value));
        }
    }
    Some(out)
}

/// What turns what a renderer plays into another revision without a new renderer: cell writes and
/// lane swaps.
pub(super) struct Reach {
    pub(super) cells: Vec<(CellId, f64)>,
    pub(super) swaps: Vec<(u32, LaneSwap)>,
}

/// What turns a plan playing `from` (its lanes and each notes lane's room in `info`) into one
/// playing `to` without a new plan, decided by the delta between the two (RENDER-26, RENDER-22;
/// the step 6 design's 5.5): the cells of the constants whose values alone changed, and the notes
/// and control lanes whose data changed, each fitting the scheduler playing it. Whoever wrote the
/// files (PLANE-1). None: an activation must play it (anything structural, a plugin state, an
/// asset a clip plays, the arrangement, an audio lane), keeping every instance that continues.
pub(super) fn reach_to(from: &Revision, info: &PlanInfo, to: &Revision) -> Option<Reach> {
    let d = m2_session::tx::delta_between(from, to);
    if d.structural() || !d.states.is_empty() || !d.assets.is_empty() || d.arrangement {
        return None;
    }
    // The same build plays the same constants: nothing to compare.
    let cells = match Arc::ptr_eq(from.compiled.code(), to.compiled.code()) {
        true => vec![],
        false => value_writes(&from.compiled.flat, &to.compiled.flat)?,
    };
    let rate = to.project.sample_rate();
    let data = &to.compiled.data;
    let mut swaps = vec![];
    for addr in &d.lanes {
        let (module, lane) = (addr.module.as_str(), addr.lane.as_str());
        // A lane no node reads plays nothing, before or after.
        let Some(slot) = info.lanes.iter().position(|s| s.lane.module == module && s.lane.lane == lane) else { continue };
        let swap = match info.lanes[slot].lane.kind {
            Kind::Events(_) => {
                let notes = notes_data(data, module, lane, rate as f64);
                // More notes at once than its scheduler holds: an activation grows it.
                if !info.lanes[slot].caps?.fits(notes.needs()) {
                    return None;
                }
                LaneSwap::Notes(Arc::new(notes))
            }
            Kind::Control(_) => LaneSwap::Control(Arc::new(data.control(module, lane))),
            // Its clips must be rendered first, then crossfade in an activation (RENDER-27).
            Kind::Audio(_) => return None,
        };
        swaps.push((slot as u32, swap));
    }
    Some(Reach { cells, swaps })
}

/// The state's `read_only` (WRITE-17): null for the lock's holder, else why, WRITE-18's message,
/// and who holds it when its file says.
fn read_only(access: &m2_session::lock::Access) -> Value {
    use m2_session::lock::{Access, ReadOnly};
    let message = access.refusal().map(|d| d.message);
    match access {
        Access::Writer(_) => Value::Null,
        Access::ReadOnly(ReadOnly::Unsupported(_)) => json!({"why": "unsupported", "message": message}),
        Access::ReadOnly(ReadOnly::Held(h)) => json!({
            "why": "held",
            "message": message,
            "holder": h.as_ref().map(|h| json!({"pid": h.pid, "program": h.program, "host": h.host, "since": h.since})),
        }),
        Access::ReadOnly(ReadOnly::Asked) => json!({"why": "held", "message": message, "holder": null}),
    }
}

/// How the accepted revision reaches the engine.
enum Way {
    /// It plays already.
    Same,
    /// Into the renderer that plays: its changed constants' cells written (RENDER-22), its
    /// changed lanes swapped (RENDER-26).
    Reach(u64, Reach),
    /// A plan prepared against what plays, activated where the engine is: the instances that
    /// continue move into it, and what changed crossfades (RENDER-25, RENDER-25a).
    Activate(Box<Renderer>, Plays),
    /// A fresh engine at the same beat: the rate changed (RENDER-31), or the engine refused an
    /// activation (RENDER-28). Every instance new.
    Replace(Box<Renderer>, Plays),
    /// A new player (the one before stopped responding): where it was, looping as it did. Every
    /// instance new: the old device thread may still be inside an instance's `process`.
    Load(Box<Renderer>, Plays),
    /// The renderer didn't build, or there is no player: what plays plays on.
    Failed(anyhow::Error),
    /// An export borrows the live instances: what plays plays on until it gives them back, and
    /// the settle after it plays this (RENDER-1).
    Deferred,
}

impl Way {
    fn renderer(&mut self) -> Option<&mut Renderer> {
        match self {
            Way::Activate(r, ..) | Way::Replace(r, ..) | Way::Load(r, ..) => Some(r),
            _ => None,
        }
    }
}

/// What a settle sent the engine.
enum Sent {
    Nothing,
    /// Nothing yet: it waits for an export (RENDER-1).
    Deferred,
    /// Into what plays: these cells written, its lanes swapped.
    Reached(Vec<CellId>),
    Renderer,
}

/// A committed value the engine must hear in the step that ends its gesture (WRITE-22).
pub(super) struct Install {
    /// The gesture's lease, which ends here; None for a plain value edit.
    lease: Option<u64>,
    /// The control's accepted id and origin, and the value written.
    id: String,
    origin: Origin,
    value: f64,
}

/// When a settle's two phases ended, and whether what plays changed.
struct Settled {
    changed: bool,
    sent_ns: u64,
    published_ns: u64,
}

/// What preparing the last activation took: naming what continues, and the plan (nanoseconds);
/// and how many instances the plan made new.
#[derive(Clone, Copy, Default)]
pub(super) struct Prepared {
    pub(super) continues_ns: u64,
    pub(super) prepare_ns: u64,
    pub(super) fresh: usize,
}

fn packages() -> Result<PackageSet> {
    let mut p = PackageSet::default();
    p.load(&crate::resources::std_dir()).context("loading the std package")?;
    Ok(p)
}

/// A plan for the accepted revision at the project's rate (RENDER-31), built without playing
/// anything: the engine puts it where it is (RENDER-8). Assets and clip renders come from `audio`,
/// which then keeps only what this one uses (the plan it replaces holds its own until it is
/// retired). Against `basis` it keeps every instance that continues; the rest are created,
/// reporting to `owner`, on the plugin main thread while the old plan plays (RENDER-25).
fn build(session: &Session, audio: &mut audio::Cache, owner: &m2_plugins::Owner, basis: Basis) -> Result<Renderer> {
    let rev = session.accepted();
    let (flat, data, rate) = (&rev.compiled.flat, &rev.compiled.data, rev.project.sample_rate());
    let r = Renderer::prepare(flat, data, &session.packages, rate, audio, owner, basis)?;
    audio.prune();
    Ok(r)
}

impl Open {
    fn check_token(&self, token: u64) -> Result<()> {
        if token != self.token {
            return Err(anyhow!("the open project changed; refresh the graph before editing"));
        }
        Ok(())
    }

    /// Whether the project plays in `device`'s player.
    fn loaded(&self, device: &Device) -> bool {
        self.plays.as_ref().is_some_and(|p| p.epoch == device.epoch)
    }

    /// What the engine plays once `r`, built from the accepted revision, plays in `device`'s player.
    fn describe(&self, r: &Renderer, device: &Device) -> Plays {
        let info = r.info();
        Plays {
            epoch: device.epoch,
            generation: info.id,
            rate: info.rate,
            refused: device.player.refused(),
            revision: self.session.accepted_arc(),
            cells: info.cell_map.clone(),
            info,
            controls: self.controls.clone(),
        }
    }

    /// The renderer plays the accepted revision now (a value commit set what differed, or its
    /// lanes were swapped, or its cells written): the next settle's delta starts from it.
    /// Refused (false) when the accepted graph numbers its cells otherwise, so what plays can't
    /// reach its values: a new renderer must play it.
    fn adopt(&mut self) -> bool {
        let rev = self.session.accepted_arc();
        let Some(p) = &mut self.plays else { return true };
        if Arc::ptr_eq(&p.revision, &rev) {
            return true;
        }
        let (old, new) = (&p.revision.compiled.flat, &rev.compiled.flat);
        if old != new {
            if !same_layout(old, new) {
                return false;
            }
            // The same cells, now named by the accepted revision's origins.
            p.cells = Arc::new(CellMap::of(&m2_runtime::cells::cells(new)));
        }
        p.revision = rev;
        // Settled already when a swap adopts; a value commit adopts first, and the settle
        // after it brings these up to the new code.
        p.controls = self.controls.clone();
        true
    }

    /// Load `r` into `device`'s player as a fresh engine, stopped at `beat` and looping over
    /// `looping`.
    fn load(&mut self, device: &Device, r: Renderer, plays: Plays, beat: f64, looping: Option<(f64, f64)>) -> Result<()> {
        device.player.open(r)?;
        if beat > 0.0 {
            device.player.seek(beat)?;
        }
        if looping.is_some() {
            device.player.set_loop(looping)?;
        }
        self.plays = Some(plays);
        Ok(())
    }

    /// What turns what `p` plays into the accepted revision without a new plan ([`reach_to`]).
    fn reach(&self, p: &Plays) -> Option<Reach> {
        reach_to(&p.revision, &p.info, self.session.accepted())
    }

    /// Whether `device`'s engine refused the last activation it was sent: it plays what it played,
    /// and only a fresh engine brings it back in step (RENDER-28).
    fn refused(&self, device: &Device) -> bool {
        self.plays.as_ref().is_some_and(|p| p.epoch == device.epoch && device.player.refused() != p.refused)
    }

    /// How the accepted revision reaches `device`'s engine, its plan prepared if it needs one, and
    /// what preparing it took.
    fn way(&mut self, device: &Device, owner: &m2_plugins::Owner, fail: bool) -> (Way, Prepared) {
        let rev = self.session.accepted();
        let (number, rate) = (rev.number, rev.project.sample_rate());
        let refused = self.refused(device);
        if self.loaded(device) && self.playing == Some(number) && self.restarts.is_empty() && !refused {
            return (Way::Same, Prepared::default());
        }
        // Opened with code that doesn't compile (WRITE-20): silent until it does, then loaded.
        if !rev.has_code() {
            return (Way::Same, Prepared::default());
        }
        enum Kind {
            Reach(u64, Reach),
            Activate,
            Replace,
            Load,
        }
        let kind = match self.plays.as_ref().filter(|p| p.epoch == device.epoch) {
            Some(p) if p.rate != rate || refused => Kind::Replace,
            // A restart replaces instances: an activation, though nothing else changed.
            Some(_) if !self.restarts.is_empty() => Kind::Activate,
            Some(p) => self.reach(p).map_or(Kind::Activate, |r| Kind::Reach(p.generation, r)),
            None => Kind::Load,
        };
        if let Kind::Reach(generation, reach) = kind {
            return (Way::Reach(generation, reach), Prepared::default());
        }
        // An export borrows the live instances: nothing is prepared, so no state load or other host
        // call reaches them, and nothing is sent until it gives them back (RENDER-1).
        if self.exporting {
            return (Way::Deferred, Prepared::default());
        }
        // Only an activation in the same engine keeps instances; a new engine creates them all.
        let Open { session, audio, plays, restarts, .. } = self;
        let mut prepared = Prepared::default();
        let t = std::time::Instant::now();
        let accepted = session.accepted();
        let keep = match (&kind, plays.as_ref()) {
            (Kind::Activate, Some(p)) => {
                // The same build: every node continues itself. Else the names of what changed.
                let cont = match Arc::ptr_eq(p.revision.compiled.code(), accepted.compiled.code()) {
                    true => Continues::identity(&accepted.compiled.flat),
                    false => m2_session::tx::continues::continues(&p.revision, accepted),
                };
                prepared.continues_ns = t.elapsed().as_nanos() as u64;
                Some((p, cont))
            }
            _ => None,
        };
        let basis = match &keep {
            Some((p, cont)) => {
                Basis::Keep(Keep { old: &p.revision.compiled.flat, old_data: &p.revision.compiled.data, info: &p.info, cont, restarts })
            }
            None => Basis::Fresh,
        };
        let built = if fail { Err(anyhow!("the renderer failed to build (a test)")) } else { build(session, audio, owner, basis) };
        prepared.prepare_ns = t.elapsed().as_nanos() as u64 - prepared.continues_ns;
        let r = match built {
            Ok(r) => Box::new(r),
            Err(e) => return (Way::Failed(e), prepared),
        };
        prepared.fresh = r.stats().fresh.total();
        let plays = self.describe(&r, device);
        let way = match kind {
            Kind::Activate => Way::Activate(r, plays),
            Kind::Replace => Way::Replace(r, plays),
            _ => Way::Load(r, plays),
        };
        (way, prepared)
    }

    /// While what plays is built from older code than the accepted revision's (its build
    /// failed): each accepted control's origin in what plays, carried from there.
    fn origins(&self) -> Option<Arc<HashMap<String, Origin>>> {
        let p = self.plays.as_ref()?;
        if p.revision.code_rev == self.code_rev {
            return None;
        }
        let at = p.revision.number;
        let carried = p.controls.values().filter_map(|c| Some((self.session.carry(at, &c.id, &self.controls).ok()?.id, c.origin.clone())));
        Some(Arc::new(carried.collect()))
    }

    /// The ids of the two code revisions before the accepted one's, carried to the accepted ids.
    fn aliases(&self) -> super::Aliases {
        let mut ids = HashMap::new();
        for (code, controls) in &self.codes {
            for id in controls.keys() {
                if let Ok(c) = self.session.carry(*code, id, &self.controls) {
                    ids.insert((*code, id.clone()), c.id);
                }
            }
        }
        super::Aliases { to: self.code_rev, ids }
    }

    /// What reads return for the accepted revision.
    fn shown(&mut self) -> Shown {
        self.timeline = Arc::new(self.timeline());
        let s = &self.session;
        let rev = s.accepted();
        let diagnostics = files::diagnostics(s);
        let read_only = read_only(s.access());
        Shown {
            revision: rev.number,
            timeline: self.timeline.clone(),
            state: json!({
                "session": self.token,
                "name": rev.project.manifest.project.name,
                "dir": rev.project.dir,
                "view": s.view(),
                "can_undo": s.can_undo(),
                "can_redo": s.can_redo(),
                "effects": s.effects(),
                "diagnostics": diagnostics,
                "problems": s.problems(),
                "sample_rate": rev.project.sample_rate(),
                // WRITE-20: opened with code that doesn't compile, nothing plays until it does.
                "has_code": rev.has_code(),
                "read_only": read_only,
            }),
            // What needs attention though the song plays, with the fixes (DIAG-1, DIAG-4).
            news: json!({
                "session": self.token,
                "revision": rev.number,
                "problems": s.problems(),
                "diagnostics": diagnostics,
            }),
            files: files::accepted(s),
            types: self.types.clone(),
            name: rev.project.manifest.project.name.clone(),
            dir: rev.project.dir.clone(),
            read_only: s.access().refusal().map(|d| d.message),
        }
    }

    /// The accepted revision's timeline, as JSON: a lane that is the same data, shown the same way,
    /// as in the timeline last published is that lane's JSON again, shared, so an edit of one lane
    /// makes one lane's JSON.
    fn timeline(&self) -> super::Timeline {
        use m2_editor::{TimelineLane, TimelineTrack};
        let data = &self.session.accepted().compiled.data;
        let view = self.session.timeline();
        let mut head = serde_json::Map::new();
        for (k, v) in [("tempo", value(&view.tempo)), ("meter", value(&view.meter)), ("end", value(&view.end))] {
            head.insert(k.into(), v);
        }
        let tracks = view.tracks.into_iter().map(|TimelineTrack { module, label, track, lanes }| {
            let lanes = lanes.into_iter().map(|mut lane| {
                let key = lane.key().to_string();
                let shown = data.lane(&module, &key).cloned();
                // The lane without its clips: what else it shows.
                let clips = match &mut lane {
                    TimelineLane::Notes { clips, .. } => Clips::Notes(std::mem::take(clips)),
                    TimelineLane::Control { clips, .. } => Clips::Control(std::mem::take(clips)),
                    TimelineLane::Audio { clips, .. } => Clips::Audio(std::mem::take(clips)),
                };
                let rest = serde_json::to_string(&lane).expect("a lane serialises");
                let same = self.timeline.lane(&module, &key).filter(|l| {
                    l.head == rest && match (&l.data, &shown) {
                        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                        (None, None) => true,
                        _ => false,
                    }
                });
                let json = match same {
                    Some(l) => l.json.clone(),
                    None => {
                        match (&mut lane, clips) {
                            (TimelineLane::Notes { clips, .. }, Clips::Notes(c)) => *clips = c,
                            (TimelineLane::Control { clips, .. }, Clips::Control(c)) => *clips = c,
                            (TimelineLane::Audio { clips, .. }, Clips::Audio(c)) => *clips = c,
                            _ => unreachable!("taken from this lane"),
                        }
                        Arc::new(value(&lane))
                    }
                };
                super::TimelineLaneJson { key, data: shown, head: rest, json }
            });
            let lanes = lanes.collect();
            let mut head = match value(&TimelineTrack { module: module.clone(), label, track, lanes: vec![] }) {
                Value::Object(m) => m,
                _ => unreachable!("a track is an object"),
            };
            head.remove("lanes");
            super::TimelineRow { module, head, lanes }
        });
        super::Timeline::new(head, tracks.collect())
    }
}

/// A timeline lane's clips, taken out while the rest of it is compared.
enum Clips {
    Notes(Vec<m2_song::NoteClip>),
    Control(Vec<m2_song::ControlClip>),
    Audio(Vec<m2_song::AudioClip>),
}

/// A part of the timeline as a value, as `json!` makes it.
fn value(v: &impl serde::Serialize) -> Value {
    serde_json::to_value(v).expect("a timeline serialises")
}

impl Actor {
    /// After every change: play the accepted revision, carry every gesture lease to it, and
    /// publish it, in two phases (see the module). `install` is a value commit the engine must
    /// hear as its gesture ends.
    fn settle(&mut self, install: Option<Install>) -> Settled {
        let inner = self.inner.clone();
        #[cfg(test)]
        let fail = self.fail_builds;
        #[cfg(not(test))]
        let fail = false;
        let Some(o) = self.open.as_mut() else { return Settled { changed: false, sent_ns: 0, published_ns: 0 } };
        let token = o.token;
        let (number, code_rev) = (o.session.accepted().number, o.session.accepted().code_rev);
        // WRITE-13: the session's code revision; a lane write leaves it, and with it every id.
        let code_changed = code_rev != o.code_rev;
        if code_changed {
            let old = std::mem::replace(&mut o.controls, Arc::new(o.session.controls_by_id()));
            o.codes.push_back((o.code_rev, old));
            while o.codes.len() > 2 {
                o.codes.pop_front();
            }
            o.code_rev = code_rev;
        }
        if o.playing == Some(number)
            && let Some(p) = &mut o.plays
        {
            p.controls = o.controls.clone();
        }

        // Prepared off every lock: the swaps, or the renderer built.
        let device = inner.device();
        let (mut way, prepared) = match &device {
            Ok(d) => o.way(d, &inner.owner, fail),
            Err(e) => (Way::Failed(anyhow!("{e:#}")), Prepared::default()),
        };
        o.prepared = prepared;
        // A state load that gave a plugin latency plays it uncompensated, and says so; one that
        // failed leaves the plugin as it was, and says why (RENDER-26b).
        if let Some(r) = way.renderer() {
            let mut notices = inner.notices.lock();
            notices.extend(r.stats().latent.iter().map(|(name, frames)| {
                format!(
                    "{name} has {frames} samples of latency since its state changed; latency compensation isn't built yet, so it plays \
                     that late (plugin.latency-uncompensated)"
                )
            }));
            notices.extend(
                r.stats().unloaded.iter().map(|(name, why)| format!("{name} couldn't load its new state, so it plays as it was: {why}")),
            );
        }
        // When the old build plays on, where the accepted revision's controls are in it: a carry
        // per control, done once here rather than under the lock.
        let failed = matches!(way, Way::Failed(_) | Way::Deferred);
        let origins = if failed { o.origins() } else { None };

        // Phase 1, under the control plane's lock: what `begin` and `update` need, with the
        // engine messages, so every lease's sends reach the queue in order.
        let mut cp = inner.control.lock();
        let closing = install.as_ref().and_then(|i| i.lease);
        // Each live lease is carried to the accepted code, or ends (WRITE-22): it stays only while
        // its call is untouched and still reads its value.
        let decisions: Vec<(u64, Result<Control, &'static str>)> = cp
            .leases
            .live
            .iter()
            .filter(|l| l.token == token && Some(l.id) != closing)
            .map(|l| {
                let carried = if l.code_rev == o.code_rev {
                    let mut c = l.control.clone();
                    c.revision = number;
                    Ok(c)
                } else {
                    o.session.carry(l.revision, &l.control.id, &o.controls)
                };
                let kept = match carried {
                    Ok(c) if c.value == l.accepted && c.scale == l.control.scale => Ok(c),
                    Ok(_) => Err("the code changed this control's value"),
                    Err(_) => Err("the code changed under this control"),
                };
                (l.id, kept)
            })
            .collect();
        // A new renderer plays each kept lease's value from its first block: written into its
        // cells before it is sent, so no update is lost to the activation.
        if let Some(r) = way.renderer() {
            let map = r.cell_map();
            for (id, kept) in &decisions {
                if let (Ok(c), Some(l)) = (kept, cp.leases.live.iter().find(|l| l.id == *id)) {
                    r.set_cells(map.cells(&c.origin), l.playing);
                }
            }
        }
        let sent = (|| -> Result<Sent> {
            match way {
                Way::Same => Ok(Sent::Nothing),
                Way::Reach(generation, reach) => {
                    let d = device.as_ref().map_err(|e| anyhow!("{e:#}"))?;
                    let p = o.plays.as_ref().expect("what plays is reached");
                    let mut written = vec![];
                    for (cell, value) in reach.cells {
                        let target = CellTarget { slots: p.info.cell_slots.clone(), cells: Arc::from([cell]) };
                        o.sent = Some(d.player.set_cells(&target, value));
                        written.push(cell);
                    }
                    for (slot, data) in reach.swaps {
                        o.sent = Some(d.player.swap_lane(generation, slot, data)?);
                    }
                    // Its cells hold the accepted values now, and its lanes the accepted data.
                    o.adopt();
                    Ok(Sent::Reached(written))
                }
                Way::Activate(r, plays) => {
                    let player = &device.as_ref().map_err(|e| anyhow!("{e:#}"))?.player;
                    // Where the engine is, with no jump back (RENDER-25a); what restarted is new.
                    o.sent = Some(player.activate(*r)?.1);
                    o.plays = Some(plays);
                    o.restarts.clear();
                    Ok(Sent::Renderer)
                }
                Way::Replace(r, plays) => {
                    let player = &device.as_ref().map_err(|e| anyhow!("{e:#}"))?.player;
                    // A manifest edit changed the rate, or the engine refused an activation: a
                    // fresh engine at the same beat (RENDER-31, RENDER-28).
                    player.replace(*r)?;
                    o.plays = Some(plays);
                    o.restarts.clear();
                    Ok(Sent::Renderer)
                }
                Way::Load(r, plays) => {
                    let d = device.as_ref().map_err(|e| anyhow!("{e:#}"))?;
                    let looping = inner.timing.lock().as_ref().and_then(|t| t.range.filter(|_| t.on));
                    o.load(d, *r, plays, d.resume, looping)?;
                    o.restarts.clear();
                    Ok(Sent::Renderer)
                }
                Way::Failed(e) => Err(e),
                Way::Deferred => Ok(Sent::Deferred),
            }
        })();
        match &sent {
            Ok(Sent::Deferred) => {}
            Ok(_) => {
                o.playing = Some(number);
                let rev = o.session.accepted();
                if let Some(t) = inner.timing.lock().as_mut() {
                    (t.tempo, t.rate) = (rev.tempo(), rev.project.sample_rate());
                }
            }
            Err(e) => eprintln!("music2: couldn't play the accepted revision: {e:#}"),
        }
        let renderer = matches!(sent, Ok(Sent::Renderer));
        // Cells written with the accepted constants: a lease that ends there leaves them (the
        // new code's value plays), never puts its old value back over them.
        let rewritten: Vec<CellId> = match &sent {
            Ok(Sent::Reached(cells)) => cells.clone(),
            _ => vec![],
        };
        let keeps = |l: &super::Lease| l.engine.as_ref().is_some_and(|t| t.cells.cells.iter().any(|c| rewritten.contains(c)));
        let plays = o.plays.as_ref().map(|p| PlaysView {
            epoch: p.epoch,
            generation: p.generation,
            slots: p.info.cell_slots.clone(),
            cells: p.cells.clone(),
            origins: if o.playing == Some(number) {
                None
            } else if failed {
                origins.clone()
            } else {
                // A send that failed (the player stopped responding): rare enough to carry here.
                o.origins()
            },
        });
        let dev = device.as_ref().ok().filter(|d| plays.as_ref().is_some_and(|p| p.epoch == d.epoch));
        for (id, kept) in decisions {
            let Ok(i) = cp.leases.index(id) else { continue };
            match kept {
                Ok(c) => {
                    let l = &mut cp.leases.live[i];
                    let target = plays.as_ref().filter(|_| dev.is_some()).and_then(|p| p.target(&c));
                    // Heard here first (no player before, or older code without it): its value.
                    if !renderer
                        && target != l.engine
                        && let (Some(t), Some(d)) = (&target, dev)
                    {
                        d.player.set_cells(&t.cells, l.playing);
                    }
                    l.moved(number, &c.id);
                    (l.engine, l.control, l.revision, l.code_rev) = (target, c, number, o.code_rev);
                }
                Err(why) => {
                    let l = cp.leases.end(i, why);
                    // A new renderer plays the code; the one that plays on gets its value back.
                    if !renderer && !keeps(&l) {
                        put_back(&l, dev);
                    }
                    // A pending commit's release was promised a save: say it won't be (C5).
                    inner.notices.lock().extend(l.unsaved(why));
                }
            }
        }
        if let Some(inst) = &install {
            let lease = inst.lease.and_then(|id| cp.leases.index(id).ok()).map(|i| cp.leases.close(i));
            // The new renderer was built from the revision that holds the value; any other
            // hears it now, in the step that ends the gesture.
            if !renderer {
                let target = match &lease {
                    Some(l) => l.engine.clone(),
                    None => plays.as_ref().and_then(|p| p.target_of(&inst.id, &inst.origin)),
                };
                if let (Some(t), Some(d)) = (target, dev) {
                    d.player.set_cells(&t.cells, inst.value);
                }
            }
        }
        if let Some(p) = cp.open.as_mut().filter(|p| p.engine.token == token) {
            p.revs.push(number, o.code_rev);
            p.engine = Arc::new(EngineView { token, revision: number, code_rev: o.code_rev, controls: o.controls.clone(), plays });
        }
        drop(cp);
        let sent_ns = inner.now_ns();
        #[cfg(test)]
        if let Some(f) = self.between_phases.as_mut() {
            f(&inner);
        }

        // Phase 2, off the lock: the view, the news, and on a code change the aliases.
        let shown = Arc::new(o.shown());
        let aliases = code_changed.then(|| Arc::new(o.aliases()));
        let mut cp = inner.control.lock();
        let mut old = None;
        if let Some(p) = cp.open.as_mut().filter(|p| p.engine.token == token) {
            old = Some(std::mem::replace(&mut p.shown, shown));
            if let Some(a) = aliases {
                p.aliases = a;
            }
        }
        drop(cp);
        // The view it replaces is a tree of every note: freed elsewhere, neither under the lock
        // nor before the reply (or the next request) on this thread.
        retire(old);
        Settled { changed: matches!(sent, Ok(Sent::Reached(_) | Sent::Renderer)), sent_ns, published_ns: inner.now_ns() }
    }

    pub(super) fn open(&mut self, dir: &Path) -> Result<Value> {
        // The project lock is one session's at a time (WRITE-17): opening the open project again
        // lets the old session's go first, and takes it back if the new one doesn't open.
        let same = |o: &Open| o.session.accepted().project.dir == dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        let reopening = self.open.as_mut().filter(|o| same(o)).map(|o| o.session.unlock()).is_some();
        // The watcher starts from the folder as it was before it is read: a write meanwhile is a
        // change it takes in.
        let before = watcher::Before::take(dir, self.inner.walk_only.load(std::sync::atomic::Ordering::Relaxed), &self.inner.wake);
        let opened = self.open_session(dir).and_then(|s| self.play(s, before));
        if opened.is_err()
            && reopening
            && let Some(o) = self.open.as_mut()
        {
            o.session.retry_lock();
        }
        opened
    }

    /// Make `session` the open project, and play it: a fresh engine, stopped at the start, not
    /// looping; what played before fades out. One opened with code that doesn't compile plays
    /// nothing until its code does (WRITE-20), and is shown with its problems meanwhile.
    fn play(&mut self, session: Session, before: watcher::Before) -> Result<Value> {
        let device = self.inner.device()?;
        let token = self.inner.next_session.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        let (number, code_rev) = (session.accepted().number, session.accepted().code_rev);
        let controls = Arc::new(session.controls_by_id());
        let types = Arc::new(files::types(&session.packages));
        let mut o = Open {
            token,
            session,
            playing: Some(number),
            plays: None,
            audio: audio::Cache::default(),
            sent: None,
            code_rev,
            controls: controls.clone(),
            codes: VecDeque::new(),
            types,
            restarts: HashSet::new(),
            exporting: false,
            prepared: Prepared::default(),
            timeline: Arc::default(),
        };
        let built = match o.session.accepted().has_code() {
            true => {
                let r = build(&o.session, &mut o.audio, &self.inner.owner, Basis::Fresh)?;
                let plays = o.describe(&r, &device);
                Some((r, plays))
            }
            false => None,
        };
        {
            // Before the new engine is sent: every gesture of the project before ends, and none
            // aims anywhere, so no update of it can reach the new engine's constants.
            let mut cp = self.inner.control.lock();
            self.inner.end_all(&mut cp);
            if let Some(p) = cp.open.as_mut() {
                p.engine = Arc::new(EngineView { plays: None, controls: p.engine.controls.clone(), ..*p.engine });
            }
        }
        let view = match built {
            Some((r, plays)) => {
                let view = PlaysView {
                    epoch: device.epoch,
                    generation: plays.generation,
                    slots: plays.info.cell_slots.clone(),
                    cells: plays.cells.clone(),
                    origins: None,
                };
                o.load(&device, r, plays, 0.0, None)?;
                Some(view)
            }
            None => {
                device.player.unload()?;
                None
            }
        };
        let rev = o.session.accepted();
        *self.inner.timing.lock() = Some(Timing { tempo: rev.tempo(), rate: rev.project.sample_rate(), range: None, on: false });
        let shown = Arc::new(o.shown());
        {
            let mut cp = self.inner.control.lock();
            // Every gesture of the project before ends: a fresh engine plays this one's code.
            self.inner.end_all(&mut cp);
            let mut revs = Revs::default();
            revs.push(number, code_rev);
            let engine = Arc::new(EngineView { token, revision: number, code_rev, controls, plays: view });
            let aliases = Arc::new(super::Aliases { to: code_rev, ids: HashMap::new() });
            cp.open = Some(Published { engine, shown, revs, aliases });
        }
        *self.inner.watch.lock() = Some(watcher::Watch::new(token, o.session.accepted(), before));
        self.open = Some(o);
        self.inner.state().context("no project is open")
    }

    /// A session on `dir`, for writing when its lock is free (WRITE-17). One whose code doesn't
    /// compile opens too, with no accepted code and the build's problems (WRITE-20), so the code
    /// editor can fix it; only a folder that isn't a project in this format doesn't.
    fn open_session(&self, dir: &Path) -> Result<Session> {
        // Files in the project by their names in it (`song.ts:22:5`), so the reason fits a toast.
        let root = format!("{}/", dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf()).display());
        let said = |d: &[m2_ir::Diagnostic]| anyhow!(d.iter().map(|d| d.to_string().replace(&root, "")).collect::<Vec<_>>().join("\n"));
        #[cfg(test)]
        if let Some(disk) = &self.disk {
            return Session::open_with_disk(dir, packages()?, "music2", disk()).map_err(|d| said(&d));
        }
        Session::open_as(dir, packages()?, "music2").map_err(|d| said(&d))
    }

    /// Take the open project's lock if it is free now (the read-only banner's Retry), and publish
    /// what that changes.
    pub(super) fn retry_lock(&mut self, token: u64) -> Result<Value> {
        let o = self.open.as_mut().context("no project is open")?;
        o.check_token(token)?;
        o.session.retry_lock();
        self.settle(None);
        self.inner.state().context("no project is open")
    }

    /// Close the project: every lease ends, the engine fades out, and the session's plugin
    /// instances are destroyed on the plugin main thread before this returns, or 2 s pass (a device
    /// thread that hangs still holds a renderer).
    pub(super) fn close(&mut self) -> super::Teardown {
        {
            let mut cp = self.inner.control.lock();
            self.inner.end_all(&mut cp);
            cp.open = None;
        }
        if let Some(d) = self.inner.current_device() {
            let _ = d.player.unload();
        }
        *self.inner.watch.lock() = None;
        self.open = None;
        *self.inner.timing.lock() = None;
        let owner = &self.inner.owner;
        let (before, t) = (owner.live(), Instant::now());
        while owner.live() > 0 && t.elapsed() < super::TEARDOWN {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        super::Teardown { destroyed: before.saturating_sub(owner.live()), left: owner.live() }
    }

    /// Take in what the session's plugin instances reported unasked. An instance that asked for a
    /// restart is replaced by the next activation, which the catch-up makes at once: a new
    /// instance from the committed state, which fades in as the old one rings out (HOST-5).
    fn heard_plugins(&mut self) {
        while let Ok(e) = self.events.try_recv() {
            match e {
                HostEvent::Restart(h) => {
                    if let Some(o) = &mut self.open {
                        o.restarts.insert(h);
                    }
                }
                // Latency compensation and state autosave come later (RENDER-17, HOME-6).
                HostEvent::Latency(h, frames) => eprintln!("music2: plugin {h:?} reports {frames} frames of latency"),
                HostEvent::Dirty(_) => {}
            }
        }
    }

    pub(super) fn render(&mut self, length: Length) -> Result<(String, PathBuf, Render, Vec<m2_song::Diagnostic>)> {
        let o = self.open.as_ref().context("no project is open")?;
        let rev = o.session.accepted();
        // Offline renders run at the project's rate (RENDER-31a).
        let r = Render::new(&rev.compiled.flat, &rev.compiled.data, &o.session.packages, rev.project.sample_rate(), 0.0, length)?;
        let carried = rev.compiled.problems.iter().filter(|d| d.code == "plugin.missing").cloned().collect();
        Ok((rev.project.manifest.project.name.clone(), rev.project.dir.clone(), r, carried))
    }

    /// The accepted revision ready to export (RENDER-1). A song whose plugins play borrows their
    /// live instances: matched as an activation matches them (`PlanInfo::lend`), playback held,
    /// and each instance taken offline holding its committed blob. Settles prepare nothing until
    /// they are given back ([`Self::export_done`]). With none playing, or when playback can't
    /// pause (its device stalled), it renders with instances of its own, beside playback.
    pub(super) fn export(&mut self, length: Length) -> Result<super::Export> {
        let device = self.inner.current_device();
        let o = self.open.as_mut().context("no project is open")?;
        let rev = o.session.accepted_arc();
        let (flat, data, rate, packages) = (&rev.compiled.flat, &rev.compiled.data, rev.project.sample_rate(), &o.session.packages);
        let problems = rev.compiled.problems.iter().filter(|d| d.code == "plugin.missing").cloned().collect();
        let (song, dir) = (rev.project.manifest.project.name.clone(), rev.project.dir.clone());
        let lend = match (&device, &o.plays) {
            (Some(d), Some(p)) if p.epoch == d.epoch && p.rate == rate && !o.exporting => {
                let cont = match Arc::ptr_eq(p.revision.compiled.code(), rev.compiled.code()) {
                    true => Continues::identity(flat),
                    false => m2_session::tx::continues::continues(&p.revision, &rev),
                };
                p.info.lend(flat, data, packages, &cont)
            }
            _ => HashMap::new(),
        };
        // Offline renders run at the project's rate (RENDER-31a).
        let own = || Render::new(flat, data, packages, rate, 0.0, length);
        let Some(d) = device.filter(|_| !lend.is_empty()) else {
            return Ok(super::Export { song, dir, render: own()?, problems, lent: None });
        };
        let render = Render::lending(flat, data, packages, rate, length, &lend)?;
        let mut blobs = vec![];
        for (&i, inst) in &lend {
            blobs.push((inst.clone(), m2_runtime::state_blob(data, &flat.nodes[i])?));
        }
        o.exporting = true;
        match crate::export::Lent::take(d.player, blobs, rate, o.token) {
            Ok(l) => Ok(super::Export { song, dir, render, problems, lent: Some(l) }),
            Err(e) => {
                o.exporting = false;
                drop(render);
                self.inner.notices.lock().push(format!("Playback couldn't pause for the export ({e:#}): it renders beside playback."));
                Ok(super::Export { song, dir, render: own()?, problems, lent: None })
            }
        }
    }

    /// An export gave back the live instances it borrowed (RENDER-1): what waited for it plays
    /// now, and an instance that didn't come back active is replaced from its committed state.
    pub(super) fn export_done(&mut self, token: u64, failed: Vec<Handle>) {
        let Some(o) = self.open.as_mut().filter(|o| o.token == token) else { return };
        o.exporting = false;
        o.restarts.extend(failed);
        self.settle(None);
    }

    /// A gesture named at a revision the control plane no longer knows: the session carries it.
    pub(super) fn begin(&mut self, token: u64, revision: u64, id: &str, owner: super::Owner) -> Result<super::Begun> {
        let o = self.open.as_ref().context("no project is open")?;
        o.check_token(token)?;
        let c = o.session.carry(revision, id, &o.controls).map_err(|e| anyhow!("{e}"))?;
        let device = self.inner.current_device();
        let mut cp = self.inner.control.lock();
        self.inner.register(&mut cp, device.as_ref(), c, owner, (revision, id))
    }

    /// The live lease of `gesture` on the control a compatible route's request names, for one
    /// named at a revision the control plane can't resolve: the session carries its id (see
    /// [`super::Graph`]'s `lease_of`).
    pub(super) fn lease_of(&mut self, token: u64, revision: u64, id: &str, gesture: &str) -> Result<Option<u64>> {
        let o = self.open.as_ref().context("no project is open")?;
        o.check_token(token)?;
        let c = o.session.carry(revision, id, &o.controls).ok();
        let mut cp = self.inner.control.lock();
        let found = cp.leases.of_gesture(token, gesture, (revision, id), c.as_ref());
        Ok(found.map(|i| cp.leases.named(i, (revision, id))))
    }

    /// Commit lease `lease`, which the control plane marked closing (WRITE-22): one verified value
    /// edit, which may adopt unrelated changes first.
    pub(super) fn commit(&mut self, lease: u64) -> Result<Committed> {
        let inner = self.inner.clone();
        let (token, revision, id, value, expect) = {
            let cp = inner.control.lock();
            let l = &cp.leases.live[cp.leases.index(lease).map_err(|e| anyhow!(e))?];
            let State::Closing { value, expect } = l.state else { bail!("this gesture isn't being committed") };
            (l.token, l.revision, l.control.id.clone(), value, expect)
        };
        let Some(o) = self.open.as_mut().filter(|o| o.token == token) else {
            let mut cp = inner.control.lock();
            if let Ok(i) = cp.leases.index(lease) {
                cp.leases.end(i, "the project was closed");
            }
            bail!("the open project changed; refresh the graph before editing");
        };
        // What plays takes the commit as the cells whose constants differ from it (an outside
        // change in the knob's file that rode along included), in the settle (RENDER-22).
        match o.session.set(revision, &id, value, expect) {
            Ok(c) => {
                let written = json!({"file": c.file, "before": c.before, "after": c.after, "revision": c.revision});
                self.settle(Some(Install { lease: Some(lease), id, origin: c.origin, value: c.value }));
                Ok(Committed::Written(written))
            }
            Err(EditError::DraftInvalid(_)) => {
                // C5: refused only because the draft doesn't compile. The value plays on, and is
                // written once the draft compiles.
                let errors = json!(files::diagnostics(&o.session));
                {
                    let device = inner.current_device();
                    let mut cp = inner.control.lock();
                    if let Ok(i) = cp.leases.index(lease) {
                        let l = &mut cp.leases.live[i];
                        l.playing = l.control.audition(value).unwrap_or(l.playing);
                        l.state = State::Pending { value, expect, since: Instant::now(), errors: errors.clone() };
                        if let (Some(t), Some(d)) = (&l.engine, &device)
                            && t.epoch == d.epoch
                        {
                            d.player.set_cells(&t.cells, l.playing);
                        }
                    }
                }
                self.settle(None);
                Ok(Committed::Pending { errors })
            }
            Err(e) => {
                {
                    let device = inner.current_device();
                    let mut cp = inner.control.lock();
                    if let Ok(i) = cp.leases.index(lease) {
                        let l = cp.leases.end(i, e.to_string());
                        put_back(&l, device.as_ref());
                    }
                }
                self.settle(None);
                Err(anyhow!("{e}"))
            }
        }
    }

    /// Commit, oldest first, every pending gesture once the draft compiles (C5).
    pub(super) fn retry_pending(&mut self) {
        loop {
            let Some(o) = self.open.as_ref() else { return };
            if !o.session.draft_errors.is_empty() {
                return;
            }
            let next = {
                let mut cp = self.inner.control.lock();
                let l = cp.leases.live.iter_mut().find(|l| l.token == o.token && matches!(l.state, State::Pending { .. }));
                l.map(|l| {
                    let State::Pending { value, expect, .. } = l.state else { unreachable!() };
                    l.state = State::Closing { value, expect };
                    (l.id, format!("{}:{}", l.control.file, l.control.line))
                })
            };
            let Some((lease, at)) = next else { return };
            let said = match self.commit(lease) {
                Ok(Committed::Written(v)) => format!("{at}: saved once the code compiled ({} → {})", v["before"], v["after"]),
                Ok(Committed::Pending { .. }) => return,
                Err(e) => format!("{at}: the value wasn't saved: {e:#}"),
            };
            self.inner.notices.lock().push(said);
        }
    }

    /// A plain value edit, with no gesture lease (an agent's `POST /set`). A control another
    /// gesture holds is refused.
    pub(super) fn set_value(
        &mut self,
        token: u64,
        revision: u64,
        id: &str,
        gesture: &str,
        value: f64,
        expect: Option<f64>,
    ) -> Result<Value> {
        let inner = self.inner.clone();
        let o = self.open.as_mut().context("no project is open")?;
        o.check_token(token)?;
        // The edit plans on the accepted revision, which the watcher keeps up with the disk; an
        // outside change in the knob's own file rides along through the commit's baseline (R2).
        let mut target = id.to_string();
        if let Ok(c) = o.session.carry(revision, id, &o.controls) {
            let cp = inner.control.lock();
            let owned = |l: &&super::Lease| {
                l.token == token && l.control.origin.same_value(&c.origin) && l.owner.gesture.as_deref() != Some(gesture)
            };
            if cp.leases.live.iter().any(|l| owned(&l)) {
                bail!("this control is being edited by another gesture");
            }
            target = c.id;
        }
        match o.session.set(revision, id, value, expect) {
            Ok(c) => {
                let v = json!({"file": c.file, "before": c.before, "after": c.after, "revision": c.revision});
                self.settle(Some(Install { lease: None, id: target, origin: c.origin, value: c.value }));
                Ok(v)
            }
            Err(e) => {
                self.settle(None);
                Err(anyhow!("{e}"))
            }
        }
    }

    /// An edit through the session; whatever happened, the accepted revision plays after it.
    pub(super) fn edit(&mut self, token: u64, edit: impl FnOnce(&mut Session) -> Result<Edited, EditError>) -> Result<Value> {
        self.edit_and(token, |s| edit(s).map(|e| (e, json!({}))))
    }

    /// [`Actor::edit`] for a gesture with more to say than its label: what it made (the lane an
    /// automate added), as more fields of the answer.
    pub(super) fn edit_and(&mut self, token: u64, edit: impl FnOnce(&mut Session) -> Result<(Edited, Value), EditError>) -> Result<Value> {
        let inner = self.inner.clone();
        let o = self.open.as_mut().context("no project is open")?;
        o.check_token(token)?;
        let requested_ns = inner.now_ns();
        o.sent = None;
        let (result, heard) = self.previewing(edit);
        let compiled_ns = inner.now_ns();
        let s = self.settle(None);
        let last = self.open.as_ref().and_then(|o| o.sent);
        let prepared = self.open.as_ref().map(|o| o.prepared).unwrap_or_default();
        // A previewed edit was sent before it was durable; one whose commit failed was swapped
        // back by the settle (PLANE-5).
        let timing = match heard.previewed {
            Some((seq, ready_ns, sent_ns)) => EditTiming {
                requested_ns,
                ready_ns,
                compiled_ns,
                sent_ns,
                published_ns: s.published_ns,
                seq: Some(seq),
                previewed: true,
                taken_back: last.filter(|l| result.is_err() && *l != seq),
                ..EditTiming::default()
            },
            None => EditTiming {
                requested_ns,
                ready_ns: compiled_ns,
                compiled_ns,
                sent_ns: s.sent_ns,
                published_ns: s.published_ns,
                seq: last,
                previewed: false,
                taken_back: None,
                continues_ns: prepared.continues_ns,
                prepare_ns: prepared.prepare_ns,
                fresh: prepared.fresh,
            },
        };
        *inner.last_edit.lock() = Some(timing);
        let result = result.map_err(|e| heard.said(&e.to_string()));
        // Its user heard it or was told it isn't saved yet: every window hears how it ended,
        // whoever sent it (PLANE-5).
        inner.notices.lock().extend(heard.told(result.as_ref().err().map(String::as_str)));
        let (e, more) = result.map_err(|e| anyhow!("{e}"))?;
        let mut answer = json!({"file": e.file, "before": e.before, "after": e.after, "revision": e.revision});
        if let (Some(a), Value::Object(more)) = (answer.as_object_mut(), more) {
            a.extend(more);
        }
        Ok(answer)
    }

    /// Take in a batch the watcher settled (WRITE-19): its paths as the folder has them now, each
    /// only while it still reads as it settled (one written to since goes on in the next batch),
    /// and nothing else of the folder, which is neither read nor listed. Returns whether clients
    /// should refresh.
    pub(super) fn settled(&mut self, token: u64, ready: watcher::Ready) -> bool {
        let requested_ns = self.inner.now_ns();
        let Some(o) = self.open.as_mut().filter(|o| o.token == token) else { return false };
        let watcher::Ready { paths, unknown, named } = ready;
        // The assets the watcher tracked by their hashes are hashed, so each reads as it settled.
        let hashed = |p: &Path| named.contains(&m2_song::path::slashed(p));
        let Ok(snapshot) = Snapshot::read_paths(&o.session.accepted().project.dir, &paths, &hashed, unknown) else { return false };
        let paths = match self.inner.watch.lock().as_mut().filter(|w| w.token == token) {
            Some(w) => {
                let kept = w.settle.confirm(&paths, &|p| snapshot.sha(p));
                // Written again since it settled: it goes on in the next batch, which the next tick
                // looks at whether an event names it or not.
                w.requeue(paths.difference(&kept).map(|p| m2_song::path::slashed(p)));
                kept
            }
            None => return false,
        };
        if paths.is_empty() {
            return false;
        }
        let taken = o.session.take_in_only(snapshot, &paths);
        // DIAG-3: an outside write that makes a module play data nothing played says so, once.
        if let Some(a) = &taken.accepted {
            self.inner.notices.lock().extend(a.notices.iter().map(|d| d.message.clone()));
        }
        if !taken.draft_changed {
            return false;
        }
        o.sent = None;
        let compiled_ns = self.inner.now_ns();
        let s = self.settle(None);
        let seq = self.open.as_ref().and_then(|o| o.sent);
        let (sent_ns, published_ns) = (s.sent_ns, s.published_ns);
        let timing = EditTiming { requested_ns, ready_ns: compiled_ns, compiled_ns, sent_ns, published_ns, seq, ..EditTiming::default() };
        *self.inner.last_taken_in.lock() = Some(timing);
        true
    }

    /// Settle when the engine is behind the accepted revision (a renderer that didn't build, a
    /// player let go of): what [`Actor::settled`] and every edit do anyway, done here when nothing
    /// else changes. Returns whether clients should refresh.
    pub(super) fn catch_up(&mut self) -> bool {
        let Some(o) = self.open.as_mut() else { return false };
        let has_code = o.session.accepted().has_code();
        let behind = match self.inner.device() {
            // An export's end settles what waited for it.
            Ok(_) if o.exporting => false,
            Ok(d) => {
                has_code && (!o.loaded(&d) || o.playing != Some(o.session.accepted().number) || !o.restarts.is_empty() || o.refused(&d))
            }
            // Let go of: the settle starts a new one, and loads the project into it where it was.
            Err(_) => true,
        };
        behind && self.settle(None).changed
    }

    /// Take in what the folder says now, as one batch, without the settle rule (tests, the gates).
    /// Returns whether clients should refresh.
    pub(super) fn poll(&mut self) -> bool {
        let Some(o) = self.open.as_mut() else { return false };
        let changed = o.session.refresh();
        let behind = match self.inner.device() {
            Ok(d) => !o.loaded(&d) || o.playing != Some(o.session.accepted().number),
            // Let go of: the settle starts a new one, and loads the project into it where it was.
            Err(_) => true,
        };
        if !changed && !behind {
            return false;
        }
        self.settle(None).changed || changed
    }

    /// Save a file from the code editor (WRITE-2: the session's `save()`), and take it in.
    pub(super) fn write_file(&mut self, token: Option<u64>, path: &str, base: &str, text: &str) -> Result<Value> {
        let o = self.open.as_mut().context("no project is open")?;
        if let Some(token) = token {
            o.check_token(token)?;
        }
        files::save(&mut o.session, path, base, text)?;
        self.settle(None);
        self.inner.state().context("no project is open")
    }

    /// Check or commit an agent's Tx (WRITE-4, WRITE-5, WRITE-11), answering in the editor's one
    /// JSON shape, with the status the route answers: 200, or 409 for a conflict, 422 for a
    /// rejection, 500 for a disk that failed. A commit is played and announced as any edit is.
    pub(super) fn tx(&mut self, token: Option<u64>, tx: m2_song::Tx, commit: bool) -> Result<(u16, Value)> {
        let o = self.open.as_mut().context("no project is open")?;
        if let Some(token) = token {
            o.check_token(token)?;
        }
        let refused = |r: &m2_session::Refusal| {
            let status = match r {
                m2_session::Refusal::Conflict { .. } => 409,
                m2_session::Refusal::Rejected { .. } => 422,
                m2_session::Refusal::Io { .. } => 500,
            };
            (status, m2_editor::json::refusal(r))
        };
        if !commit {
            return Ok(match o.session.check(&tx) {
                Ok(c) => (200, m2_editor::json::checked(&c)),
                Err(r) => refused(&r),
            });
        }
        o.sent = None;
        let (done, heard) = self.previewing(|s| s.commit(tx));
        self.settle(None);
        // Heard in the app, or said there to be not saved yet: the app hears how it ended too,
        // not only the agent (PLANE-5).
        let said = done.as_ref().err().map(|r| heard.said(&r.to_string()));
        let told = heard.told(said.as_deref()).map(|t| if said.is_some() { format!("An agent's edit: {t}") } else { t });
        self.inner.notices.lock().extend(told);
        Ok(match done {
            Ok(c) => (200, m2_editor::json::committed(&c)),
            Err(r) => {
                let (status, mut answer) = refused(&r);
                // Heard before it failed (PLANE-5): an agent is told as the app is.
                if heard.previewed.is_some()
                    && let Some(why) = answer.as_object_mut().and_then(|a| a.values_mut().next()).and_then(Value::as_object_mut)
                {
                    if !heard.interrupted {
                        why.insert("taken_back".into(), json!(true));
                    }
                    if let Some(Value::String(m)) = why.get_mut("message") {
                        *m = heard.said(m);
                    }
                }
                (status, answer)
            }
        })
    }

    /// Run `edit` on the open project's session with the preview hook installed (PLANE-5): a data
    /// commit is heard once it passed its checks, while the session makes it durable. What the
    /// hook sent is recorded in what plays before the settle that follows, whatever the edit did:
    /// one that panics is brought back to the accepted revision first, then the panic goes on.
    fn previewing<R>(&mut self, edit: impl FnOnce(&mut Session) -> R) -> (R, preview::Heard) {
        let inner = self.inner.clone();
        let o = self.open.as_mut().expect("an open project");
        let (lent, before) = preview::lend(o);
        o.session.before_write(Some(preview::hook(inner, lent.clone())));
        let ran = catch_unwind(AssertUnwindSafe(|| edit(&mut o.session)));
        o.session.before_write(None);
        let heard = preview::give_back(&self.inner, &lent, o, before);
        match ran {
            Ok(r) => (r, heard),
            Err(panic) => {
                self.settle(None);
                std::panic::resume_unwind(panic)
            }
        }
    }
}
