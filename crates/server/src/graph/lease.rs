//! Gesture leases (WRITE-22): a knob drag resolved once, then played straight into the engine.
//!
//! A lease is `begin`, `update`s, then `commit` or `cancel`. It keeps two things apart:
//! - **the session's side**, `control`: the control in the accepted revision, which a commit
//!   writes and whose literal spells the values; carried to every new code revision (WRITE-12);
//! - **the engine's side**, `engine`: the renderer and the cells in it that updates go to
//!   (RENDER-22, RENDER-23). It moves only once a renderer holding the lease's value was sent, so
//!   after a renderer that failed to build, updates go on to the one that plays.
//!
//! A lease survives revisions that don't touch its call (an outside save to another file, a
//! lane write, an edit elsewhere), refused edits and drafts that don't compile. It ends when the
//! code under it changes, when it is cancelled, when it is idle past its TTL, or when the window
//! that began it closes (C6). Every lease is listed in the news while it lives.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use m2_editor::Control;
use m2_ir::Origin;
use m2_runtime::{CellMap, CellSlots, CellTarget};
use m2_session::Home;
use serde::Serialize;
use serde_json::Value;

/// How long a lease lives without an update or a keepalive. The web sends a keepalive every 2 s
/// while a knob is held, so a knob held still while listening never expires.
pub const TTL: Duration = Duration::from_secs(10);
/// The longest TTL a client may ask for.
pub const MAX_TTL: Duration = Duration::from_secs(60);
/// How long a commit waits for the code to compile again (C5).
pub const PENDING: Duration = Duration::from_secs(30);
/// Revisions whose code revision the control plane remembers: as many as the session can carry
/// an edit from.
const REVS: usize = 64;
/// Ended leases remembered, so a late update says why its lease ended.
const ENDED: usize = 64;
/// The names and ids a lease keeps (see [`Lease::names`] and [`Lease::ids`]).
const NAMES: usize = 8;

/// Where a lease's updates go: the cells of its value in one renderer of one player, written
/// through that renderer's slots (RENDER-23), so they can never reach another's.
#[derive(Clone)]
pub struct Target {
    pub epoch: u64,
    pub generation: u64,
    pub cells: CellTarget,
}

impl PartialEq for Target {
    fn eq(&self, o: &Target) -> bool {
        (self.epoch, self.generation) == (o.epoch, o.generation)
            && Arc::ptr_eq(&self.cells.slots, &o.cells.slots)
            && self.cells.cells == o.cells.cells
    }
}

impl std::fmt::Debug for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("Target")
            .field("epoch", &self.epoch)
            .field("generation", &self.generation)
            .field("cells", &self.cells.cells)
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Live,
    /// A commit of `value` is on the actor: updates and cancels are refused, and it decides.
    Closing {
        value: f64,
        expect: Option<f64>,
    },
    /// Its commit was refused only because the draft doesn't compile: the value plays on, and the
    /// actor commits it once the draft compiles, or cancels it after [`PENDING`] (C5).
    Pending {
        value: f64,
        expect: Option<f64>,
        since: Instant,
        errors: Value,
    },
}

/// Who began a lease, and for how long it lives idle.
#[derive(Clone, Debug, Default)]
pub struct Owner {
    /// The window (its WebSocket's `client`): when that closes, its live leases end.
    pub client: Option<String>,
    /// The UI's gesture id, by which the compatible routes find the lease.
    pub gesture: Option<String>,
    /// Idle time before it ends; [`TTL`] by default, [`MAX_TTL`] at most.
    pub ttl: Option<Duration>,
}

pub struct Lease {
    pub id: u64,
    /// The open project it belongs to.
    pub token: u64,
    pub owner: Owner,
    pub ttl: Duration,
    pub began: Instant,
    pub touched: Instant,
    /// The accepted revision `control` was read in, and that revision's code revision. Moved
    /// forward by every settle.
    pub revision: u64,
    pub code_rev: u64,
    pub control: Control,
    /// The `(revision, id)` pairs requests named it by, its begin's first: how the compatible
    /// routes find a gesture's lease on the control a request names (see [`Leases::of_gesture`]).
    pub names: Vec<(u64, String)>,
    /// Its control's id in each accepted revision since it began, from the revision it took that
    /// id in: what a view of an older revision calls it (the page keeps a dragged knob mounted
    /// through a save that renumbers it, see [`Lease::seen_at`]).
    pub ids: Vec<(u64, String)>,
    /// The code's value: put back when the lease ends without a commit.
    pub accepted: f64,
    /// None while nothing plays the control (no player yet, or a build that plays older code
    /// without it).
    pub engine: Option<Target>,
    /// The last update taken.
    pub seq: u64,
    /// What the engine plays for it: exactly what a commit would write.
    pub playing: f64,
    pub state: State,
}

/// Keep `name` among `names`, the first kept and at most [`NAMES`].
fn remember(names: &mut Vec<(u64, String)>, name: (u64, &str)) {
    if names.iter().any(|n| n.0 == name.0 && n.1 == name.1) {
        return;
    }
    if names.len() >= NAMES {
        names.remove(1.min(names.len() - 1));
    }
    names.push((name.0, name.1.to_string()));
}

impl Lease {
    /// The lease as the view of `revision` names it: its control's id there, if known. None for a
    /// view older than every id kept, or one from before the lease began that its begin didn't
    /// name it in.
    pub fn seen_at(&self, revision: u64, now: Instant) -> Option<LeaseInfo> {
        let id = match self.ids.iter().rev().find(|(r, _)| *r <= revision) {
            Some((_, id)) => id,
            None => &self.names.iter().find(|(r, _)| *r == revision)?.1,
        };
        Some(LeaseInfo { id: id.clone(), revision, ..self.info(now) })
    }

    /// Its control's id is `id` from accepted revision `revision` on.
    pub fn moved(&mut self, revision: u64, id: &str) {
        if self.ids.last().is_none_or(|(_, last)| last != id) {
            if self.ids.len() >= NAMES {
                self.ids.remove(0);
            }
            self.ids.push((revision, id.to_string()));
        }
    }

    /// What to tell the user when this lease ends unsaved, for `why`: only a pending commit (C5),
    /// which the page said would be saved once the code compiles, and which no request of its
    /// client will hear of. A live lease's client hears why at its next request.
    pub fn unsaved(&self, why: &str) -> Option<String> {
        matches!(self.state, State::Pending { .. })
            .then(|| format!("{}:{}: the value wasn't saved: {why}", self.control.file, self.control.line))
    }

    pub fn info(&self, now: Instant) -> LeaseInfo {
        let state = match self.state {
            State::Live => "live",
            State::Closing { .. } => "committing",
            State::Pending { .. } => "pending",
        };
        LeaseInfo {
            lease: self.id,
            id: self.control.id.clone(),
            file: self.control.file.clone(),
            line: self.control.line,
            client: self.owner.client.clone(),
            gesture: self.owner.gesture.clone(),
            state,
            home: self.control.home,
            value: self.playing,
            accepted: self.accepted,
            revision: self.revision,
            generation: self.engine.as_ref().map(|t| t.generation),
            age_ms: now.duration_since(self.began).as_millis() as u64,
            idle_ms: now.duration_since(self.touched).as_millis() as u64,
        }
    }
}

/// A lease as the news and the probes list it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LeaseInfo {
    pub lease: u64,
    /// The control's id in the accepted revision.
    pub id: String,
    pub file: String,
    pub line: u32,
    pub client: Option<String>,
    pub gesture: Option<String>,
    /// `live`, `committing` or `pending`.
    pub state: &'static str,
    /// Where the value lives (HOME-1).
    pub home: Home,
    /// What plays, and the code's value.
    pub value: f64,
    pub accepted: f64,
    pub revision: u64,
    /// The renderer its updates go to; None while nothing plays it.
    pub generation: Option<u64>,
    pub age_ms: u64,
    pub idle_ms: u64,
}

/// What `begin` answers.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Begun {
    pub lease: u64,
    /// The accepted revision the lease reads its control in, and the control's id there.
    pub revision: u64,
    pub id: String,
    /// The code's value.
    pub value: f64,
    /// How many constants the code makes for the value (a helper called twice gives two).
    pub uses: usize,
    /// How many cells of what plays its updates reach (RENDER-22): one per constant, a voice's
    /// counted once for all its voices.
    pub cells: usize,
    /// Where the value lives (HOME-1): the code, for a number written there or a declared control
    /// a call leaves out, until override latches come (MON-6).
    pub home: Home,
    /// Whether anything plays it yet. Updates are recorded either way, and written into the
    /// renderer that next plays.
    pub heard: bool,
}

/// What `commit` answers.
#[derive(Clone, Debug, PartialEq)]
pub enum Committed {
    /// Written: the file, what the literal said before and after, and the revision.
    Written(Value),
    /// The draft doesn't compile: the value plays on and is written once it does (C5).
    Pending { errors: Value },
}

/// Why a lease ended, for a late update or commit.
pub struct Ended {
    pub id: u64,
    pub token: u64,
    pub gesture: Option<String>,
    /// What requests named it by ([`Lease::names`]).
    pub names: Vec<(u64, String)>,
    pub why: String,
}

#[derive(Default)]
pub struct Leases {
    pub live: Vec<Lease>,
    ended: VecDeque<Ended>,
    next: u64,
}

impl Leases {
    pub fn next_id(&mut self) -> u64 {
        self.next += 1;
        self.next
    }

    /// Where lease `id` is, or why it isn't live.
    pub fn index(&self, id: u64) -> Result<usize, String> {
        self.live.iter().position(|l| l.id == id).ok_or_else(|| match self.ended.iter().find(|e| e.id == id) {
            Some(e) => format!("this gesture ended: {}", e.why),
            None => format!("no gesture lease {id}"),
        })
    }

    /// Whether `gesture` holds a live lease in project `token`.
    pub fn holds(&self, token: u64, gesture: &str) -> bool {
        self.live.iter().any(|l| l.token == token && l.owner.gesture.as_deref() == Some(gesture))
    }

    /// The live lease of `gesture` in project `token` that a compatible route's request naming
    /// `name` is for: one requests named so before, else the one on `control`, what `name`
    /// resolves to in the accepted revision (None when that isn't known here). A gesture string
    /// has a lease per control, as it had a preview per control before leases: a script that sends
    /// one gesture for every knob reaches each knob's own, never another's (WRITE-22).
    pub fn of_gesture(&self, token: u64, gesture: &str, name: (u64, &str), control: Option<&Control>) -> Option<usize> {
        let mine = |l: &Lease| l.token == token && l.owner.gesture.as_deref() == Some(gesture);
        self.live.iter().position(|l| mine(l) && l.names.iter().any(|n| n.0 == name.0 && n.1 == name.1)).or_else(|| {
            let c = control?;
            self.live.iter().position(|l| mine(l) && l.control.origin.same_value(&c.origin))
        })
    }

    /// Lease `index` was found by `name`: requests naming it so find it at once from now on.
    pub fn named(&mut self, index: usize, name: (u64, &str)) -> u64 {
        let l = &mut self.live[index];
        remember(&mut l.names, name);
        l.id
    }

    /// Why the lease of `gesture` that requests named `name` ended, if it did: a late preview or
    /// release of an ended gesture is refused with why, while the same gesture string naming
    /// another control begins anew.
    pub fn ended_of(&self, token: u64, gesture: &str, name: (u64, &str)) -> Option<&str> {
        let named = |e: &Ended| e.names.iter().any(|n| n.0 == name.0 && n.1 == name.1);
        self.ended.iter().rev().find(|e| e.token == token && e.gesture.as_deref() == Some(gesture) && named(e)).map(|e| e.why.as_str())
    }

    /// Take lease `index` out, remembering why it ended.
    pub fn end(&mut self, index: usize, why: impl Into<String>) -> Lease {
        let l = self.live.remove(index);
        let gesture = l.owner.gesture.clone();
        self.ended.push_back(Ended { id: l.id, token: l.token, gesture, names: l.names.clone(), why: why.into() });
        while self.ended.len() > ENDED {
            self.ended.pop_front();
        }
        l
    }

    /// Take lease `index` out without remembering why: it committed, or its own gesture cancelled
    /// it through the compatible route.
    pub fn close(&mut self, index: usize) -> Lease {
        self.live.remove(index)
    }
}

/// The code revision of each of the last [`REVS`] accepted revisions the control plane
/// published, so a gesture named in one of them begins without the session (WRITE-13).
#[derive(Default)]
pub struct Revs(VecDeque<(u64, u64)>);

impl Revs {
    pub fn push(&mut self, revision: u64, code_rev: u64) {
        if self.0.back().is_some_and(|r| r.0 == revision) {
            return;
        }
        self.0.push_back((revision, code_rev));
        while self.0.len() > REVS {
            self.0.pop_front();
        }
    }

    pub fn code_rev(&self, revision: u64) -> Option<u64> {
        self.0.iter().rev().find(|r| r.0 == revision).map(|r| r.1)
    }
}

/// Control ids of the two code revisions before `to`, carried to their ids in it: `(code_rev,
/// id)` → the id in `to`. Control ids are `file#ordinal/arg`, so equal code gives equal ids, and a
/// client a revision or two behind needs no carry of its own.
#[derive(Default)]
pub struct Aliases {
    /// The code revision the ids are carried to. They are read only while it is the accepted
    /// revision's: a settle publishes a new code revision's controls with the engine (its first
    /// phase) and carries these to it after (its second), and in between an id carried to the
    /// code revision before would be looked up among the new one's controls, where it can name
    /// another call (WRITE-22).
    pub to: u64,
    pub ids: HashMap<(u64, String), String>,
}

/// What plays, as the control plane needs it to aim updates.
#[derive(Clone)]
pub struct PlaysView {
    pub epoch: u64,
    pub generation: u64,
    /// Its renderer's cell slots, and which of its cells hold each value of the code it plays.
    pub slots: Arc<CellSlots>,
    pub cells: Arc<CellMap>,
    /// Its plan: its overlay sites and their cells (MON-1), among the rest.
    pub info: Arc<m2_runtime::PlanInfo>,
    /// The tracks muted in the song in what it plays (MON-5).
    pub muted: Vec<String>,
    /// The tracks of what it plays: a solo of any other is ignored (MON-4).
    pub tracks: Vec<String>,
    /// Set only while what plays is built from older code than the accepted revision's (its
    /// build failed): the accepted control id → its constant's origin in what plays.
    pub origins: Option<Arc<HashMap<String, Origin>>>,
}

impl PlaysView {
    /// Where updates to `control` go in what plays.
    pub fn target(&self, control: &Control) -> Option<Target> {
        self.target_of(&control.id, &control.origin)
    }

    /// Where a value of the accepted control `id`, at `origin`, goes in what plays: its cells
    /// there. None when nothing there plays it.
    pub fn target_of(&self, id: &str, origin: &Origin) -> Option<Target> {
        let origin = match &self.origins {
            None => origin,
            Some(map) => map.get(id)?,
        };
        let cells = self.cells.cells(origin);
        if cells.is_empty() {
            return None;
        }
        let cells = CellTarget { slots: self.slots.clone(), cells: cells.into() };
        Some(Target { epoch: self.epoch, generation: self.generation, cells })
    }
}
