//! The session's actor (WRITE-22): one thread owns the [`Session`] and serves requests one at a
//! time, first in, first out. Each may compile; nothing else waits for it but the request's own
//! caller.
//!
//! After every change the actor settles: it plays the accepted revision (a lane swap, or a new
//! renderer activated where the engine is), carries every gesture lease to it, and publishes it
//! to the control plane in two phases: first what the engine and leases need, under the lock
//! and with the engine messages, then the view and the news, before the request replies.

use std::collections::{HashMap, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};
use m2_ir::{Flat, Kind, Origin};
use m2_plugins::{HostEvent, HostEvents};
use m2_runtime::{Caps, Carry, CellMap, CellSlots, Fresh, InstanceSource, LaneRef, LaneSwap, PackageSet, Renderer, audio, notes_data};
use m2_session::render::{Length, Render};
use m2_session::{Control, EditError, Edited, Session, plane};
use m2_song::{DataSet, Snapshot};
use serde_json::{Value, json};

use super::lease::{Committed, PlaysView, Revs, State};
use super::{Device, EditTiming, EngineView, Inner, Published, Shown, Timing, put_back};
use crate::files;

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

/// The open project, as the actor owns it.
pub(crate) struct Open {
    pub(crate) token: u64,
    pub(crate) session: Session,
    /// The accepted revision the player is playing.
    pub(crate) playing: u64,
    /// What the engine plays, to tell lane data from structure. None until the project is loaded
    /// into the player.
    pub(crate) plays: Option<Plays>,
    /// Decoded assets and clip renders, shared by every renderer built for this project, so an
    /// activation never decodes or warps them again (RENDER-27).
    audio: audio::Cache,
    /// The engine message the last settle sent the new revision in (a swap or an activation).
    sent: Option<u64>,
    /// The accepted revision the last settle saw, and its snapshot: a new code revision begins
    /// where the code differs from it (WRITE-13).
    seen: u64,
    snapshot: Snapshot,
    pub(crate) code_rev: u64,
    /// The accepted revision's controls, by id.
    controls: Arc<HashMap<String, Control>>,
    /// The controls of the two code revisions before the accepted one's, oldest first: what a
    /// client a little behind names, which aliases carry to the accepted ids.
    codes: VecDeque<(u64, Arc<HashMap<String, Control>>)>,
    types: Arc<Value>,
    /// The plugin instances of the renderer last sent to the engine, for the next one built for it
    /// (HOST-5): an activation takes them over, with their state, tails and notes.
    pub(crate) carry: Carry,
}

/// The renderer the engine plays: what it was built from, and how `SwapLane` reaches its lanes.
pub(crate) struct Plays {
    /// The player it was loaded into ([`Device::epoch`]).
    pub(crate) epoch: u64,
    pub(crate) generation: u64,
    rate: u32,
    flat: Flat,
    snapshot: Snapshot,
    pub(crate) data: DataSet,
    slots: Vec<LaneRef>,
    /// The room each notes lane's scheduler has: data that needs more is activated instead.
    caps: Vec<Option<Caps>>,
    /// The accepted revision it plays, its code revision, and its controls: after a build that
    /// failed, what updates aim at.
    revision: u64,
    code_rev: u64,
    controls: Arc<HashMap<String, Control>>,
    /// Its cells' slots, and which cells hold each value of the revision it plays: what gesture
    /// updates and committed values are written to (RENDER-22, RENDER-23).
    cell_slots: Arc<CellSlots>,
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

/// How the accepted revision reaches the engine.
enum Way {
    /// It plays already.
    Same,
    Swap(u64, Vec<(u32, LaneSwap)>),
    /// A new renderer, crossfaded where the engine is (RENDER-25a), with the instances it carried
    /// and created.
    Activate(Box<Renderer>, Plays, Carry),
    /// A fresh engine at the same beat: the rate changed (RENDER-31). Never carries.
    Replace(Box<Renderer>, Plays, Carry),
    /// A new player (the one before stopped responding): where it was, looping as it did. Never
    /// carries: the old device thread may still be inside an instance's `process`.
    Load(Box<Renderer>, Plays, Carry),
    /// The renderer didn't build, or there is no player: what plays plays on.
    Failed(anyhow::Error),
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
    Swapped,
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

fn packages() -> Result<PackageSet> {
    let mut p = PackageSet::default();
    p.load(&crate::resources::std_dir()).context("loading the std package")?;
    Ok(p)
}

/// A renderer for the accepted revision at the project's rate (RENDER-31), built without playing
/// anything: the engine puts it where it is (RENDER-8). Assets and clip renders come from `audio`,
/// which then keeps only what this one uses (the renderer it replaces holds its own until it is
/// retired). Plugin instances come from `source` when one matches (HOST-5), and the rest are
/// created, reporting to `owner`, on the plugin main thread while the old renderer plays
/// (RENDER-25).
fn build(session: &Session, audio: &mut audio::Cache, owner: &m2_plugins::Owner, source: &mut dyn InstanceSource) -> Result<Renderer> {
    let rev = session.accepted();
    let (flat, data, rate) = (&rev.compiled.flat, &rev.compiled.data, rev.project.sample_rate());
    let r = Renderer::build_live(flat, data, &session.packages, rate, audio, owner, source)?;
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
            revision: rev.number,
            code_rev: self.code_rev,
            controls: self.controls.clone(),
            cell_slots: r.cell_slots(),
            cells: r.cell_map(),
        }
    }

    /// The renderer plays the accepted revision now (a value commit set what differed, or its
    /// lanes were swapped): a lane edit after this still finds the graphs equal, and swaps.
    /// Refused (false) when the accepted graph numbers its cells otherwise, so what plays can't
    /// reach its values: a new renderer must play it.
    fn adopt(&mut self) -> bool {
        let rev = self.session.accepted();
        let Some(p) = &mut self.plays else { return true };
        if p.flat != rev.compiled.flat {
            if !same_layout(&p.flat, &rev.compiled.flat) {
                return false;
            }
            // The same cells, now named by the accepted revision's origins.
            p.cells = Arc::new(CellMap::of(&m2_runtime::cells::cells(&rev.compiled.flat)));
        }
        (p.flat, p.snapshot, p.data, p.revision) = (rev.compiled.flat.clone(), rev.snapshot.clone(), rev.compiled.data.clone(), rev.number);
        // Settled already when a swap adopts; a value commit adopts first, and the settle
        // after it brings these up to the new code.
        (p.code_rev, p.controls) = (self.code_rev, self.controls.clone());
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

    /// How the accepted revision reaches `device`'s engine, its renderer built if it needs one.
    fn way(&mut self, device: &Device, owner: &m2_plugins::Owner, fail: bool) -> Way {
        let rev = self.session.accepted();
        let (number, rate) = (rev.number, rev.project.sample_rate());
        if self.loaded(device) && number == self.playing {
            return Way::Same;
        }
        enum Kind {
            Swap(u64, Vec<(u32, LaneSwap)>),
            Activate,
            Replace,
            Load,
        }
        let kind = match self.plays.as_ref().filter(|p| p.epoch == device.epoch) {
            Some(p) if p.rate != rate => Kind::Replace,
            Some(p) => self.swaps(p).map_or(Kind::Activate, |s| Kind::Swap(p.generation, s)),
            None => Kind::Load,
        };
        if let Kind::Swap(generation, swaps) = kind {
            return Way::Swap(generation, swaps);
        }
        // Only an activation in the same engine carries instances; a new engine creates them all.
        let Open { session, audio, carry, .. } = self;
        let source: &mut dyn InstanceSource = match kind {
            Kind::Activate => carry.source(device.epoch),
            _ => &mut Fresh,
        };
        let built = if fail { Err(anyhow!("the renderer failed to build (a test)")) } else { build(session, audio, owner, source) };
        let r = match built {
            Ok(r) => Box::new(r),
            Err(e) => return Way::Failed(e),
        };
        let (plays, next) = (self.describe(&r, device.epoch), Carry::of(&r, device.epoch));
        match kind {
            Kind::Activate => Way::Activate(r, plays, next),
            Kind::Replace => Way::Replace(r, plays, next),
            _ => Way::Load(r, plays, next),
        }
    }

    /// While what plays is built from older code than the accepted revision's (its build
    /// failed): each accepted control's origin in what plays, carried from there.
    fn origins(&self) -> Option<Arc<HashMap<String, Origin>>> {
        let p = self.plays.as_ref()?;
        if p.code_rev == self.code_rev {
            return None;
        }
        let carried =
            p.controls.values().filter_map(|c| Some((self.session.carry(p.revision, &c.id, &self.controls).ok()?.id, c.origin.clone())));
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
    fn shown(&self) -> Shown {
        let s = &self.session;
        let rev = s.accepted();
        let diagnostics = files::diagnostics(s);
        Shown {
            revision: rev.number,
            state: json!({
                "session": self.token,
                "name": rev.project.manifest.project.name,
                "dir": rev.project.dir,
                "view": s.view(),
                "timeline": s.timeline(),
                "can_undo": s.can_undo(),
                "can_redo": s.can_redo(),
                "effects": s.effects(),
                "diagnostics": diagnostics,
                "problems": s.problems(),
                "sample_rate": rev.project.sample_rate(),
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
        }
    }

    /// A value edit `c`, made when revision `before` was accepted, that the renderer can take as
    /// a value: only a verified single value edit (or none) on the revision that plays. `set()`
    /// may adopt unrelated changes first, which need a new renderer. Step 6's delta replaces
    /// this guess.
    fn single(&mut self, c: &m2_session::Committed, before: u64, device: Option<&Device>) {
        let single = c.before != c.after && c.revision == before + 1;
        if (single || c.revision == before) && self.playing == before && device.is_some_and(|d| self.loaded(d)) && self.adopt() {
            self.playing = c.revision;
        }
    }
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
        let number = o.session.accepted().number;
        // WRITE-13: a code revision begins where the code differs from the revision seen last.
        let code_changed = number != o.seen && plane::code_changed(&o.snapshot, &o.session.accepted().snapshot);
        if number != o.seen {
            (o.seen, o.snapshot) = (number, o.session.accepted().snapshot.clone());
        }
        if code_changed {
            let old = std::mem::replace(&mut o.controls, Arc::new(o.session.controls_by_id()));
            o.codes.push_back((o.code_rev, old));
            while o.codes.len() > 2 {
                o.codes.pop_front();
            }
            o.code_rev = number;
        }
        if o.playing == number
            && let Some(p) = &mut o.plays
        {
            (p.revision, p.code_rev, p.controls) = (number, o.code_rev, o.controls.clone());
        }

        // Prepared off every lock: the swaps, or the renderer built.
        let device = inner.device();
        let mut way = match &device {
            Ok(d) => o.way(d, &inner.owner, fail),
            Err(e) => Way::Failed(anyhow!("{e:#}")),
        };
        // When the old build plays on, where the accepted revision's controls are in it: a carry
        // per control, done once here rather than under the lock.
        let failed = matches!(way, Way::Failed(_));
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
                Way::Swap(generation, swaps) => {
                    let player = &device.as_ref().map_err(|e| anyhow!("{e:#}"))?.player;
                    for (slot, data) in swaps {
                        o.sent = Some(player.swap_lane(generation, slot, data)?);
                    }
                    // The graphs are equal, so the cells are too.
                    o.adopt();
                    Ok(Sent::Swapped)
                }
                Way::Activate(r, plays, carry) => {
                    let player = &device.as_ref().map_err(|e| anyhow!("{e:#}"))?.player;
                    // Crossfaded where the engine is, with no jump back (RENDER-25a).
                    o.sent = Some(player.activate(*r)?.1);
                    (o.plays, o.carry) = (Some(plays), carry);
                    Ok(Sent::Renderer)
                }
                Way::Replace(r, plays, carry) => {
                    let player = &device.as_ref().map_err(|e| anyhow!("{e:#}"))?.player;
                    // A manifest edit changed the rate: a fresh engine at the same beat (RENDER-31).
                    player.replace(*r)?;
                    (o.plays, o.carry) = (Some(plays), carry);
                    Ok(Sent::Renderer)
                }
                Way::Load(r, plays, carry) => {
                    let d = device.as_ref().map_err(|e| anyhow!("{e:#}"))?;
                    let looping = inner.timing.lock().as_ref().and_then(|t| t.range.filter(|_| t.on));
                    o.load(d, *r, plays, d.resume, looping)?;
                    o.carry = carry;
                    Ok(Sent::Renderer)
                }
                Way::Failed(e) => Err(e),
            }
        })();
        match &sent {
            Ok(_) => {
                o.playing = number;
                let rev = o.session.accepted();
                if let Some(t) = inner.timing.lock().as_mut() {
                    (t.tempo, t.rate) = (rev.tempo(), rev.project.sample_rate());
                }
            }
            Err(e) => eprintln!("music2: couldn't play the accepted revision: {e:#}"),
        }
        let renderer = matches!(sent, Ok(Sent::Renderer));
        let plays = o.plays.as_ref().map(|p| PlaysView {
            epoch: p.epoch,
            generation: p.generation,
            slots: p.cell_slots.clone(),
            cells: p.cells.clone(),
            origins: if o.playing == number {
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
                    if !renderer {
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
        if let Some(p) = cp.open.as_mut().filter(|p| p.engine.token == token) {
            p.shown = shown;
            if let Some(a) = aliases {
                p.aliases = a;
            }
        }
        drop(cp);
        Settled { changed: matches!(sent, Ok(Sent::Swapped | Sent::Renderer)), sent_ns, published_ns: inner.now_ns() }
    }

    pub(super) fn open(&mut self, dir: &Path) -> Result<Value> {
        let session = Session::open(dir, packages()?).map_err(|d| {
            // Files in the project by their names in it (`song.ts:22:5`), so the reason fits a toast.
            let root = format!("{}/", dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf()).display());
            anyhow!(d.iter().map(|d| d.to_string().replace(&root, "")).collect::<Vec<_>>().join("\n"))
        })?;
        let device = self.inner.device()?;
        let token = self.inner.next_session.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        let (number, snapshot) = (session.accepted().number, session.accepted().snapshot.clone());
        let controls = Arc::new(session.controls_by_id());
        let types = Arc::new(files::types(&session.packages));
        let mut o = Open {
            token,
            session,
            playing: number,
            plays: None,
            audio: audio::Cache::default(),
            sent: None,
            seen: number,
            snapshot,
            code_rev: number,
            controls: controls.clone(),
            codes: VecDeque::new(),
            types,
            carry: Carry::none(),
        };
        // A fresh engine, stopped at the start, not looping; what played before fades out.
        let r = build(&o.session, &mut o.audio, &self.inner.owner, &mut Fresh)?;
        let plays = o.describe(&r, device.epoch);
        o.carry = Carry::of(&r, device.epoch);
        {
            // Before the new engine is sent: every gesture of the project before ends, and none
            // aims anywhere, so no update of it can reach the new engine's constants.
            let mut cp = self.inner.control.lock();
            self.inner.end_all(&mut cp);
            if let Some(p) = cp.open.as_mut() {
                p.engine = Arc::new(EngineView { plays: None, controls: p.engine.controls.clone(), ..*p.engine });
            }
        }
        let view =
            PlaysView { epoch: device.epoch, generation: plays.generation, slots: plays.cell_slots.clone(), cells: plays.cells.clone(), origins: None };
        o.load(&device, r, plays, 0.0, None)?;
        let rev = o.session.accepted();
        *self.inner.timing.lock() = Some(Timing { tempo: rev.tempo(), rate: rev.project.sample_rate(), range: None, on: false });
        let shown = Arc::new(o.shown());
        {
            let mut cp = self.inner.control.lock();
            // Every gesture of the project before ends: a fresh engine plays this one's code.
            self.inner.end_all(&mut cp);
            let mut revs = Revs::default();
            revs.push(number, number);
            let engine = Arc::new(EngineView { token, revision: number, code_rev: number, controls, plays: Some(view) });
            let aliases = Arc::new(super::Aliases { to: number, ids: HashMap::new() });
            cp.open = Some(Published { engine, shown, revs, aliases });
        }
        self.open = Some(o);
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
    /// restart isn't carried into the next renderer: that one creates it anew, activated again.
    fn heard_plugins(&mut self) {
        while let Ok(e) = self.events.try_recv() {
            match e {
                HostEvent::Restart(h) => {
                    if let Some(o) = &self.open {
                        o.carry.restart(h);
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
        let before = o.session.accepted().number;
        let device = inner.device().ok();
        match o.session.set(revision, &id, value, expect) {
            Ok(c) => {
                o.single(&c, before, device.as_ref());
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
        // Leases read the accepted revision: settle what is on disk first, so the two agree.
        if o.session.refresh() {
            self.settle(None);
        }
        let o = self.open.as_mut().context("no project is open")?;
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
        let before = o.session.accepted().number;
        let device = inner.device().ok();
        match o.session.set(revision, id, value, expect) {
            Ok(c) => {
                o.single(&c, before, device.as_ref());
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
        let inner = self.inner.clone();
        let o = self.open.as_mut().context("no project is open")?;
        o.check_token(token)?;
        let requested_ns = inner.now_ns();
        let result = edit(&mut o.session);
        let compiled_ns = inner.now_ns();
        o.sent = None;
        let s = self.settle(None);
        let seq = self.open.as_ref().and_then(|o| o.sent);
        *inner.last_edit.lock() = Some(EditTiming { requested_ns, compiled_ns, sent_ns: s.sent_ns, published_ns: s.published_ns, seq });
        let e = result.map_err(|e| anyhow!("{e}"))?;
        Ok(json!({"file": e.file, "before": e.before, "after": e.after, "revision": e.revision}))
    }

    /// Pick up edits made outside the app. Returns whether clients should refresh.
    pub(super) fn poll(&mut self) -> bool {
        let Some(o) = self.open.as_mut() else { return false };
        let changed = o.session.refresh();
        let behind = match self.inner.device() {
            Ok(d) => !o.loaded(&d) || o.playing != o.session.accepted().number,
            // Let go of: the settle starts a new one, and loads the project into it where it was.
            Err(_) => true,
        };
        if !changed && !behind {
            return false;
        }
        self.settle(None).changed || changed
    }

    /// Save a file from the code editor, and take it in.
    pub(super) fn write_file(&mut self, token: Option<u64>, path: &str, base: &str, text: &str) -> Result<Value> {
        let o = self.open.as_mut().context("no project is open")?;
        if let Some(token) = token {
            o.check_token(token)?;
        }
        let before = o.session.accepted().snapshot.files.get(Path::new(path)).cloned();
        files::write(&o.session.accepted().project.dir, path, base, text)?;
        o.session.saved(Path::new(path), before);
        self.settle(None);
        self.inner.state().context("no project is open")
    }
}
