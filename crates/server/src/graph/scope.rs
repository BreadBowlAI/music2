//! What windows and agents ask to look at (TAP-4, TAP-6, TAP-8 to TAP-15): a look at a box, its
//! scope, its spectrum, its level or one of its sound's meters (`POST /api/graph/look`), or the
//! song's loudness (`POST /api/graph/loudness`, the Mix view's). One mechanism: an ask points a ring
//! (a meter slot, for a meter) of the plan that plays, lasts while its client asks, and its place
//! points nowhere once no ask uses it.
//!
//! - **A place for each point.** A plan has [`m2_runtime::RINGS`] rings: the first is the output's,
//!   for the loudness ([`super::meters::LOUD`]); the rest are for looks at sound, one for each point
//!   (a box's output or its input), whatever looks at it and whoever asks. A meter takes one of
//!   [`m2_runtime::METERS`] slots. A point past them is refused in words (TAP-10).
//! - **While it asks.** An ask lasts while its client asks: it ends when the client says stop,
//!   when its window's socket closes ([`Graph::client_gone`]), or [`KEEP`] after its last ask (a
//!   window asks again every few seconds). An agent's client is `agent:` and its own name, so it
//!   never reaches a window's ask.
//! - **Each window its own looks.** A look's last frames (a scope), bands (a spectrum), peak and RMS
//!   (a level) or numbers (a meter) go only to the sockets of the windows that asked for it
//!   ([`Graph::meters`]); an agent gets them in each answer to its ask. The spectrum is worked out
//!   after the control plane's lock is let go ([`Shown::json`]).
//! - **The view's box.** A box is named by the shown (accepted) view's id, always. It is found in
//!   the accepted revision, and followed into each one after as its call, by its `(` as ID-13 names
//!   a call ([`Asks::follow`]): an edit that moves the call keeps the look on it under the id the
//!   new view gives it; one that deletes it leaves the look waiting for that call, sending nothing
//!   (an undo brings it back), and an ask by the old id is for whatever box has it now. While what
//!   plays is built from other code than the accepted revision's (Compare's Before, or the version
//!   before a change that couldn't get ready to play), a look sends nothing, and says why.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use m2_editor::scope::At;
use m2_editor::spectrum::Spectrum;
use m2_ir::Kernels;
use m2_runtime::{METERS, RING, RINGS, Reading, TapAt, TapSlots};
use m2_session::Revision;
use parking_lot::Mutex;
use serde_json::{Value, json};

use super::actor::Actor;
use super::{Graph, meters};

/// The frames each message carries of a scope: 21 ms at 48 kHz.
pub const SCOPE: usize = 1024;
/// How long an ask lasts after its client last asked.
pub const KEEP: Duration = Duration::from_secs(10);

/// Why a scope shows nothing while what plays isn't the song as the view shows it.
const ELSEWHERE: &str = "This shows the song as the view shows it, and another version plays now: Compare's Before, \
                         or the version before a change that isn't ready to play yet. It shows again once that one plays.";

/// What kind of look (TAP-12).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The last [`SCOPE`] frames (TAP-8).
    Scope,
    /// Bands a twelfth of an octave wide (TAP-13).
    Spectrum,
    /// Each channel's peak and RMS since the last read (TAP-14).
    Level,
    /// A sound's meter: now, and the most since the last read (TAP-15).
    Meter,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Scope => "scope",
            Kind::Spectrum => "spectrum",
            Kind::Level => "level",
            Kind::Meter => "meter",
        }
    }
}

impl std::str::FromStr for Kind {
    type Err = String;

    fn from_str(s: &str) -> Result<Kind, String> {
        Ok(match s {
            "scope" => Kind::Scope,
            "spectrum" => Kind::Spectrum,
            "level" => Kind::Level,
            "meter" => Kind::Meter,
            _ => return Err(format!("A look is a scope, a spectrum, a level or a meter, not {s}.")),
        })
    }
}

/// A look at a box (TAP-11, TAP-12): its call, by its module and the shown view's id, its kind, and
/// the point of the call it reads.
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    pub module: String,
    pub node: String,
    pub kind: Kind,
    pub at: At,
}

impl Look {
    /// A scope of the call's own output, as TAP-8's ask is.
    pub fn scope(module: &str, node: &str) -> Look {
        Look { module: module.to_string(), node: node.to_string(), kind: Kind::Scope, at: At::Out }
    }

    /// Refused in words when its kind and its point don't go together: a meter by its name, sound
    /// at `out` or `in`.
    pub fn check(&self) -> Result<()> {
        match (self.kind, &self.at) {
            (Kind::Meter, At::Out | At::In) => bail!("A meter look names the meter it shows, such as at: \"reduction\"."),
            (Kind::Meter, At::Meter(m)) if m.is_empty() => bail!("A meter look names the meter it shows, such as at: \"reduction\"."),
            (Kind::Meter, At::Meter(_)) | (_, At::Out | At::In) => Ok(()),
            (kind, At::Meter(_)) => {
                bail!("A {} shows sound at a call's out or in; a meter is a look of its own, kind: \"meter\".", kind.name())
            }
        }
    }

    /// Whether it reads the same point as `o`, whatever its kind: the two share a place.
    fn same_point(&self, o: &Look) -> bool {
        self.module == o.module && self.node == o.node && self.at == o.at
    }
}

/// What a client asks to look at.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum What {
    /// A look at a box (TAP-8, TAP-12).
    Box(Look),
    /// The song's loudness (TAP-6).
    Loudness,
}

/// Where an ask reads in a plan: a ring, or a meter slot (TAP-10).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    Ring(usize),
    Meter(usize),
}

/// What the windows and agents ask to look at in the open song, on the control plane.
#[derive(Default)]
pub(super) struct Asks {
    asked: Vec<Ask>,
}

struct Ask {
    what: What,
    place: Place,
    /// Each client asking, when it last asked, and where it last read.
    clients: Vec<Client>,
    /// A box's channels while its place points at it.
    channels: Option<u8>,
    /// A meter's unit.
    unit: Option<String>,
    /// The accepted revision a box's call was last found in, under its id.
    found: Option<Arc<Revision>>,
    /// Its call is gone from the accepted revision: it waits for it, and no ask by id finds it.
    gone: bool,
    /// Why it shows nothing, while it waits for what plays to be the song as shown.
    why: Option<&'static str>,
    /// A spectrum's lows and buffers between reads, worked out off the control plane's lock.
    spectrum: Arc<Mutex<Spectrum>>,
}

/// A client of a look: who, when it last asked, and its own reads (TAP-14, TAP-15): a level's next
/// frame (the plan, by its id, never reused, and the frame in its ring), and a meter's last number
/// and the most since its last read.
struct Client {
    name: String,
    asked: Instant,
    from: (u64, u64),
    now: Option<f32>,
    most: Option<f32>,
}

impl Client {
    fn new(name: &str, asked: Instant) -> Client {
        Client { name: name.to_string(), asked, from: (0, 0), now: None, most: None }
    }

    /// What its meter says since its last read, the most never under now; and from here, again.
    fn take(&mut self) -> Option<Reading> {
        let now = self.now?;
        Some(Reading { now, most: self.most.take().map_or(now, |m| m.max(now)) })
    }
}

/// What a look saw at a read, as a window's message carries it.
enum Seen {
    Frames(Vec<[f32; 2]>),
    /// The frames a spectrum is worked out from, at the song's rate, and its look's own.
    Spectrum(Vec<[f32; 2]>, u32, Arc<Mutex<Spectrum>>),
    Level(Vec<[f32; 2]>),
    Meter(Reading),
}

/// One look as a window's message carries it: what it saw, or why it saw nothing.
pub(super) struct Shown {
    look: Look,
    channels: u8,
    seen: Option<Seen>,
    why: Option<&'static str>,
    /// The clients it goes to.
    pub(super) clients: Vec<String>,
}

impl Shown {
    /// `{"module", "node", "kind", "at"}` and what it saw: `frames` (a list per channel, each sample
    /// to a millionth), `bins` (dB), `peak` and `rms` (per channel, linear), or `now` and `most`
    /// (the meter's unit); or `why`. A spectrum is worked out here, off the control plane's lock.
    pub(super) fn json(&self) -> Value {
        let mut v = json!({"module": self.look.module, "node": self.look.node, "kind": self.look.kind.name(), "at": self.look.at.name()});
        if let Some(why) = self.why {
            v["why"] = json!(why);
            return v;
        }
        if let Some(seen) = &self.seen {
            seen.put(&mut v, self.channels);
        }
        v
    }
}

impl Seen {
    /// What it saw, into `v`, of `channels` sides.
    fn put(&self, v: &mut Value, channels: u8) {
        match self {
            Seen::Frames(frames) => v["frames"] = json!(sides(frames, channels)),
            Seen::Spectrum(frames, rate, look) => v["bins"] = json!(look.lock().bands(frames, channels, *rate)),
            Seen::Level(frames) => {
                let n = (channels as usize).clamp(1, 2);
                let peak = |c: usize| frames.iter().fold(0.0f32, |m, f| m.max(f[c].abs())) as f64;
                let rms = |c: usize| (frames.iter().map(|f| f[c] as f64 * f[c] as f64).sum::<f64>() / frames.len().max(1) as f64).sqrt();
                let six = |x: f64| (x * 1e6).round() / 1e6;
                v["peak"] = json!((0..n).map(|c| six(peak(c))).collect::<Vec<_>>());
                v["rms"] = json!((0..n).map(|c| six(rms(c))).collect::<Vec<_>>());
            }
            Seen::Meter(r) => {
                let two = |x: f32| (x as f64 * 100.0).round() / 100.0;
                (v["now"], v["most"]) = (json!(two(r.now)), json!(two(r.most)));
            }
        }
    }
}

/// `frames`' first `channels` sides, each a list, each sample to a millionth.
fn sides(frames: &[[f32; 2]], channels: u8) -> Vec<Vec<f64>> {
    let side = |c: usize| frames.iter().map(|f| (f[c] as f64 * 1e6).round() / 1e6).collect::<Vec<_>>();
    (0..(channels as usize).clamp(1, 2)).map(side).collect()
}

/// What `a`, a look, saw in `taps`, a plan with id `plan` at `rate`, for its client `client`: a
/// level from where that client last read, its meter since its last read (folded in already,
/// [`Asks::fold`]). None when there is nothing yet.
fn see(a: &mut Ask, client: usize, taps: &TapSlots, plan: u64, rate: u32) -> Option<Seen> {
    let What::Box(look) = &a.what else { return None };
    let mut frames = vec![];
    match (look.kind, a.place) {
        (Kind::Scope, Place::Ring(r)) => {
            taps.frames(r, 0, SCOPE, &mut frames)?;
            (!frames.is_empty()).then_some(Seen::Frames(frames))
        }
        (Kind::Spectrum, Place::Ring(r)) => {
            taps.frames(r, 0, RING, &mut frames)?;
            (frames.len() >= m2_editor::spectrum::HIGHS).then(|| Seen::Spectrum(frames, rate, a.spectrum.clone()))
        }
        (Kind::Level, Place::Ring(r)) => {
            let c = a.clients.get_mut(client)?;
            let from = if c.from.0 == plan { c.from.1 } else { 0 };
            let first = taps.frames(r, from, RING, &mut frames)?;
            c.from = (plan, first + frames.len() as u64);
            (!frames.is_empty()).then_some(Seen::Level(frames))
        }
        (Kind::Meter, Place::Meter(_)) => a.clients.get_mut(client)?.take().map(Seen::Meter),
        _ => None,
    }
}

impl Ask {
    fn asked_by(&self, client: &str) -> bool {
        self.clients.iter().any(|c| c.name == client)
    }

    /// Its meter's read since the last, into each client's own (TAP-15).
    fn fold(&mut self, taps: &TapSlots) {
        let (Place::Meter(m), What::Box(_), false) = (self.place, &self.what, self.gone) else { return };
        let Some(r) = taps.meter(m) else { return };
        for c in &mut self.clients {
            c.now = Some(r.now);
            c.most = Some(c.most.map_or(r.most, |x| x.max(r.most)));
        }
    }

    /// Point its place in `taps` at `at`, or nowhere.
    fn point(&self, taps: &TapSlots, at: Option<TapAt>) -> Result<()> {
        match self.place {
            Place::Ring(r) => taps.point(r, at).map(drop),
            Place::Meter(m) => taps.point_meter(m, at),
        }
    }
}

impl Asks {
    /// The ask for `what` that an ask by its name finds: a box gone from the song isn't one.
    fn find(&mut self, what: &What) -> Option<&mut Ask> {
        self.asked.iter_mut().find(|a| a.what == *what && !a.gone)
    }

    /// `client` asks for `what` again, or anew where another asked it already: it lasts [`KEEP`]
    /// from now. None when no one asked for it yet.
    fn renew(&mut self, client: &str, what: &What, now: Instant) -> Option<&mut Ask> {
        let a = self.find(what)?;
        match a.clients.iter_mut().find(|c| c.name == client) {
            Some(c) => c.asked = now,
            None => a.clients.push(Client::new(client, now)),
        }
        Some(a)
    }

    /// A new ask, in its place: the loudness's ring; the place of a look at a point shown already;
    /// or a free one, a ring for sound or a slot for a meter. Refused in words when none is free
    /// (TAP-10).
    fn add(&mut self, client: &str, what: What, now: Instant) -> Result<&mut Ask> {
        let taken = |p: Place| self.asked.iter().any(|a| a.place == p);
        let place = match &what {
            What::Loudness => Place::Ring(meters::LOUD),
            What::Box(look) => {
                let shown = self.asked.iter().find(|a| !a.gone && matches!(&a.what, What::Box(o) if o.same_point(look)));
                match shown {
                    Some(a) => a.place,
                    None if look.kind == Kind::Meter => (0..METERS).map(Place::Meter).find(|&p| !taken(p)).with_context(|| {
                        format!(
                            "No room to show this: {METERS} meters are shown already. Collapse a node or scroll one away, then try again."
                        )
                    })?,
                    None => (meters::LOUD + 1..RINGS).map(Place::Ring).find(|&p| !taken(p)).with_context(|| {
                        format!(
                            "No room to show this: {} sounds are shown already. Collapse a node or scroll one away, then try again.",
                            RINGS - 1
                        )
                    })?,
                }
            }
        };
        let clients = vec![Client::new(client, now)];
        let spectrum = Arc::default();
        self.asked.push(Ask { what, place, clients, channels: None, unit: None, found: None, gone: false, why: None, spectrum });
        Ok(self.asked.last_mut().expect("just pushed"))
    }

    /// Each ask with no client left goes; its place points nowhere in `taps` once no ask uses it.
    fn prune(&mut self, taps: Option<&TapSlots>) {
        let (kept, left): (Vec<Ask>, Vec<Ask>) = std::mem::take(&mut self.asked).into_iter().partition(|a| !a.clients.is_empty());
        if let Some(t) = taps {
            for a in left.iter().filter(|a| !kept.iter().any(|k| k.place == a.place)) {
                let _ = a.point(t, None);
            }
        }
        self.asked = kept;
    }

    /// `client` stops asking for `what`, a box gone from the song by that id too.
    fn stop(&mut self, client: &str, what: &What, taps: Option<&TapSlots>) {
        for a in self.asked.iter_mut().filter(|a| a.what == *what) {
            a.clients.retain(|c| c.name != client);
        }
        self.prune(taps);
    }

    /// `client` is gone (its window closed): it asks for nothing now.
    fn gone(&mut self, client: &str, taps: Option<&TapSlots>) {
        self.asked.iter_mut().for_each(|a| a.clients.retain(|c| c.name != client));
        self.prune(taps);
    }

    /// Each client that hasn't asked again for [`KEEP`] stops asking.
    pub(super) fn expire(&mut self, now: Instant, taps: Option<&TapSlots>) {
        self.asked.iter_mut().for_each(|a| a.clients.retain(|c| now.duration_since(c.asked) < KEEP));
        self.prune(taps);
    }

    /// Whether anyone asks for the song's loudness.
    pub(super) fn loudness(&self) -> bool {
        self.asked.iter().any(|a| a.what == What::Loudness)
    }

    /// How many rings the looks at sound take.
    #[cfg(test)]
    pub(super) fn rings(&self) -> usize {
        let mut rings: Vec<usize> = self
            .asked
            .iter()
            .filter_map(|a| match (&a.what, a.place) {
                (What::Box(_), Place::Ring(r)) => Some(r),
                _ => None,
            })
            .collect();
        rings.sort_unstable();
        rings.dedup();
        rings.len()
    }

    /// Follow each box into `accepted`, the revision the view now shows (TAP-9): its call, carried
    /// there by its `(` as ID-13 names it, under the id the new view gives it; or gone, waiting for
    /// that call as it was last found (an undo brings it back). The same code: nothing to do.
    pub(super) fn follow(&mut self, accepted: &Arc<Revision>) {
        for a in &mut self.asked {
            let What::Box(look) = &mut a.what else { continue };
            let Some(from) = a.found.clone() else { continue };
            if Arc::ptr_eq(from.compiled.code(), accepted.compiled.code()) {
                a.found = Some(accepted.clone());
                continue;
            }
            match m2_editor::scope::carry(&from, accepted, &look.node) {
                Some(id) => (look.node, a.found, a.gone) = (id, Some(accepted.clone()), false),
                None => a.gone = true,
            }
        }
    }

    /// Point each ask's place in `taps`, a plan built from `plays` (TAP-9): the loudness's at the
    /// output; a look's at its point of its call in `accepted`, by the view's id, when `plays` is
    /// built from the same code, else nowhere, saying why; a gone box's nowhere.
    pub(super) fn point(&mut self, taps: &TapSlots, plays: &Revision, accepted: &Arc<Revision>, kernels: &dyn Kernels) {
        let shown = Arc::ptr_eq(plays.compiled.code(), accepted.compiled.code());
        for a in &mut self.asked {
            let What::Box(look) = &a.what else {
                let _ = a.point(taps, Some(TapAt::Out));
                continue;
            };
            (a.channels, a.why) = (None, None);
            if a.gone {
                let _ = a.point(taps, None);
                continue;
            }
            if !shown {
                a.why = Some(ELSEWHERE);
                let _ = a.point(taps, None);
                continue;
            }
            match m2_editor::scope::look(accepted, kernels, &look.module, &look.node, &look.at) {
                Ok(p) if a.point(taps, Some(p.at)).is_ok() => {
                    (a.channels, a.unit) = (Some(p.channels), p.unit);
                    a.found = Some(accepted.clone());
                }
                _ => {
                    let _ = a.point(taps, None);
                }
            }
        }
    }

    /// Each meter's read since the last, into each of its clients' own: at every read of the
    /// meters, whoever listens, so an agent with no window open gets the most since its last ask.
    pub(super) fn fold(&mut self, taps: &TapSlots) {
        self.asked.iter_mut().for_each(|a| a.fold(taps));
    }

    /// What each look saw in `taps`, a plan with id `plan` at `rate`, or why it saw nothing, with
    /// the windows it goes to: a scope or a spectrum once for all its clients, a level or a meter
    /// for each client its own. An agent's is in its answer. A gone box, or one whose place has
    /// nothing yet, sends nothing.
    pub(super) fn looks(&mut self, taps: &TapSlots, plan: u64, rate: u32) -> Vec<Shown> {
        let mut out = vec![];
        for a in &mut self.asked {
            let What::Box(look) = &a.what else { continue };
            let look = look.clone();
            let windows: Vec<usize> = (0..a.clients.len()).filter(|&c| !a.clients[c].name.starts_with("agent:")).collect();
            if windows.is_empty() {
                continue;
            }
            if let Some(why) = a.why.filter(|_| !a.gone) {
                let clients = windows.iter().map(|&c| a.clients[c].name.clone()).collect();
                out.push(Shown { look, channels: 0, seen: None, why: Some(why), clients });
                continue;
            }
            let (Some(channels), false) = (a.channels, a.gone) else { continue };
            let each = matches!(look.kind, Kind::Level | Kind::Meter);
            for (i, &c) in windows.iter().enumerate() {
                if !each && i > 0 {
                    break;
                }
                let clients =
                    if each { vec![a.clients[c].name.clone()] } else { windows.iter().map(|&w| a.clients[w].name.clone()).collect() };
                if let Some(seen) = see(a, c, taps, plan, rate) {
                    out.push(Shown { look: look.clone(), channels, seen: Some(seen), why: None, clients });
                }
            }
        }
        out
    }
}

/// What an ask for a look answers, made under the control plane's lock and said after it: its kind
/// and point, its channels (a meter's unit), why it waits, a spectrum's bands' centres, and what it
/// saw for its client since that client's last ask, when its place has something.
struct Answer {
    head: Value,
    channels: u8,
    seen: Option<Seen>,
}

impl Answer {
    /// `{look: {…}}`: a spectrum is worked out here, off the lock.
    fn json(self) -> Value {
        let mut v = self.head;
        if let Some(seen) = &self.seen {
            seen.put(&mut v, self.channels);
        }
        json!({"look": v})
    }
}

/// The answer to `client`'s ask for `a`, taking what it saw since its last ask.
fn answer(a: &mut Ask, client: &str, taps: Option<(&TapSlots, u64, u32)>) -> Answer {
    let What::Box(look) = &a.what else { return Answer { head: json!({}), channels: 0, seen: None } };
    let mut head = json!({"module": look.module, "node": look.node, "kind": look.kind.name(), "at": look.at.name()});
    match look.kind {
        Kind::Meter => head["unit"] = json!(a.unit),
        _ => head["channels"] = json!(a.channels),
    }
    if let (Kind::Spectrum, Some((_, _, rate))) = (look.kind, taps) {
        head["hz"] = json!(m2_editor::spectrum::centres(rate));
    }
    if let Some(why) = a.why {
        head["why"] = json!(why);
    }
    let mut seen = None;
    if let (Some(_), Some((t, plan, rate)), Some(c)) = (a.channels, taps, a.clients.iter().position(|c| c.name == client)) {
        a.fold(t);
        seen = see(a, c, t, plan, rate);
    }
    Answer { head, channels: a.channels.unwrap_or(0), seen }
}

impl Graph {
    /// Start, keep or stop the scope of box `node` of module `module`: [`Graph::look`] at its
    /// output (TAP-8).
    pub fn scope(&self, token: u64, client: &str, module: &str, node: &str, on: bool) -> Result<Value> {
        self.look(token, client, &Look::scope(module, node), on)
    }

    /// Start, keep or stop `look` for `client` (`POST /api/graph/look`, TAP-8, TAP-12). Not a write:
    /// it works read-only too. A look asked for already is kept on the control plane alone; a new
    /// one is found in the accepted revision, on the session's actor, and pointed in what plays.
    /// Answers `{look: {module, node, kind, at, channels (a meter's unit), …}}`: `channels` null and
    /// `why` while it can't show, what it saw last when it has something (an agent's way to see
    /// it); `{}` for a stop. What doesn't fit is refused in words.
    pub fn look(&self, token: u64, client: &str, look: &Look, on: bool) -> Result<Value> {
        look.check()?;
        let what = What::Box(look.clone());
        {
            let mut cp = self.inner.control.lock();
            let open = cp.open.as_ref().filter(|o| o.engine.token == token).context("the open song changed; refresh it first")?;
            let plays = open.engine.plays.as_ref().and_then(|p| Some((p.info.taps.clone()?, p.info.id, p.info.rate)));
            if !on {
                cp.asks.stop(client, &what, plays.as_ref().map(|p| &*p.0));
                return Ok(json!({}));
            }
            if let Some(a) = cp.asks.renew(client, &what, Instant::now()) {
                let said = answer(a, client, plays.as_ref().map(|p| (&*p.0, p.1, p.2)));
                drop(cp);
                return Ok(said.json());
            }
        }
        let (client, look) = (client.to_string(), look.clone());
        self.call(move |a| a.look(token, &client, &look))
    }

    /// Start, keep or stop the song's loudness for `client` (`POST /api/graph/loudness`, TAP-6):
    /// the Mix view asks while it shows. Not a write. Answers `{loudness: {short, true_peak}}` as
    /// last measured (each null until there is one), or `{}` for a stop.
    pub fn loudness(&self, token: u64, client: &str, on: bool) -> Result<Value> {
        let mut cp = self.inner.control.lock();
        let open = cp.open.as_ref().filter(|o| o.engine.token == token).context("the open song changed; refresh it first")?;
        let taps = open.engine.plays.as_ref().and_then(|p| p.info.taps.clone());
        if !on {
            cp.asks.stop(client, &What::Loudness, taps.as_deref());
            return Ok(json!({}));
        }
        let now = Instant::now();
        if cp.asks.renew(client, &What::Loudness, now).is_none() {
            let a = cp.asks.add(client, What::Loudness, now)?;
            if let Some(t) = &taps {
                a.point(t, Some(TapAt::Out))?;
            }
        }
        Ok(json!({"loudness": cp.meters.loudness()}))
    }

    /// `client`'s window closed: what it asked for ends (TAP-8).
    pub(super) fn asks_gone(&self, client: &str) {
        let mut cp = self.inner.control.lock();
        let taps = cp.open.as_ref().and_then(|o| o.engine.plays.as_ref()).and_then(|p| p.info.taps.clone());
        cp.asks.gone(client, taps.as_deref());
    }
}

impl Actor {
    /// A new look (TAP-8, TAP-12): its point found in the accepted revision, by the view's id, and
    /// its place pointed at it in the plan that plays, when that plan is built from the same code.
    /// Here, on the actor, no plan is sent meanwhile, and every later one points it
    /// ([`Asks::point`]).
    pub(super) fn look(&mut self, token: u64, client: &str, look: &Look) -> Result<Value> {
        let o = self.open.as_ref().filter(|o| o.token == token).context("the open song changed; refresh it first")?;
        let accepted = o.session.accepted_arc();
        m2_editor::scope::look(&accepted, &o.session.packages, &look.module, &look.node, &look.at).map_err(anyhow::Error::msg)?;
        let what = What::Box(look.clone());
        let mut cp = self.inner.control.lock();
        let now = Instant::now();
        if cp.asks.renew(client, &what, now).is_none() {
            cp.asks.add(client, what.clone(), now)?.found = Some(accepted.clone());
        }
        let plays = o.plays.as_ref().and_then(|p| Some((p.info.taps.clone()?, p.revision.clone(), p.info.id, p.info.rate)));
        if let Some((taps, plays, ..)) = &plays {
            cp.asks.point(taps, plays, &accepted, &o.session.packages);
        }
        let a = cp.asks.find(&what).context("the look ended as it was asked for")?;
        if !a.asked_by(client) {
            bail!("the look ended as it was asked for");
        }
        let said = answer(a, client, plays.as_ref().map(|p| (&*p.0, p.2, p.3)));
        drop(cp);
        Ok(said.json())
    }
}

#[cfg(test)]
#[path = "scope_tests.rs"]
mod tests;
