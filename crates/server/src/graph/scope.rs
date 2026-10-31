//! What windows and agents ask to look at (TAP-4, TAP-6, TAP-8, TAP-9): a box's sound, its scope
//! (`POST /api/graph/scope`), or the song's loudness (`POST /api/graph/loudness`, the Mix view's).
//! One mechanism: an ask points a ring of the plan that plays, lasts while its client asks, and its
//! ring points nowhere once no client asks.
//!
//! - **One ring each.** A plan has [`m2_runtime::RINGS`] rings: the first is the output's, for the
//!   loudness ([`super::meters::LOUD`]), the rest are for scopes. Two clients asking for the same
//!   box share its ring. A fifth box is refused in words.
//! - **While it asks.** An ask lasts while its client asks: it ends when the client says stop,
//!   when its window's socket closes ([`Graph::client_gone`]), or [`KEEP`] after its last ask (a
//!   window asks again every few seconds). An agent's client is `agent:` and its own name, so it
//!   never reaches a window's ask.
//! - **Each window its own frames.** A scope's last [`SCOPE`] frames go only to the sockets of the
//!   windows that asked for it ([`Graph::meters`]); an agent gets them in each answer to its ask.
//! - **The view's box.** A box is named by the shown (accepted) view's id, always. It is found in
//!   the accepted revision, and followed into each one after as its call, by its `(` as ID-13 names
//!   a call ([`Asks::follow`]): an edit that moves the call keeps the scope on it under the id the
//!   new view gives it; one that deletes it leaves the scope waiting for that call, sending nothing
//!   (an undo brings it back), and an ask by the old id is for whatever box has it now. While what plays is built from other code than
//!   the accepted revision's (Compare's Before, or the version before a change that couldn't get
//!   ready to play), a scope sends no frames, and says why.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use m2_ir::Kernels;
use m2_runtime::{RINGS, TapAt, TapSlots};
use m2_session::Revision;
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

/// What a client asks to look at.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum What {
    /// A box's sound (TAP-8), by its module and the shown view's id.
    Box { module: String, node: String },
    /// The song's loudness (TAP-6).
    Loudness,
}

/// What the windows and agents ask to look at in the open song, on the control plane.
#[derive(Default)]
pub(super) struct Asks {
    asked: Vec<Ask>,
}

struct Ask {
    what: What,
    ring: usize,
    /// Each client asking, and when it last asked.
    clients: Vec<(String, Instant)>,
    /// A box's channels while its ring points at it.
    channels: Option<u8>,
    /// The accepted revision a box's call was last found in, under its id.
    found: Option<Arc<Revision>>,
    /// Its call is gone from the accepted revision: it waits for it, and no ask by id finds it.
    gone: bool,
    /// Why it shows nothing, while it waits for what plays to be the song as shown.
    why: Option<&'static str>,
}

/// One scope as a window's message carries it: its frames, or why it has none.
pub(super) struct Shown {
    module: String,
    node: String,
    channels: u8,
    frames: Vec<[f32; 2]>,
    why: Option<&'static str>,
    /// The clients it goes to.
    pub(super) clients: Vec<String>,
}

impl Shown {
    /// `{"module", "node", "frames": [[…], …]}` (a list per channel, each sample to a millionth), or
    /// `{"module", "node", "why"}`.
    pub(super) fn json(&self) -> Value {
        if let Some(why) = self.why {
            return json!({"module": self.module, "node": self.node, "why": why});
        }
        json!({"module": self.module, "node": self.node, "frames": sides(&self.frames, self.channels)})
    }
}

/// `frames`' first `channels` sides, each a list, each sample to a millionth.
fn sides(frames: &[[f32; 2]], channels: u8) -> Vec<Vec<f64>> {
    let side = |c: usize| frames.iter().map(|f| (f[c] as f64 * 1e6).round() / 1e6).collect::<Vec<_>>();
    (0..(channels as usize).clamp(1, 2)).map(side).collect()
}

impl Ask {
    fn asked_by(&self, client: &str) -> bool {
        self.clients.iter().any(|c| c.0 == client)
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
        match a.clients.iter_mut().find(|c| c.0 == client) {
            Some(c) => c.1 = now,
            None => a.clients.push((client.to_string(), now)),
        }
        Some(a)
    }

    /// A new ask, on its ring: the loudness's, or a box's that no other box has. Refused in words
    /// when every box's is taken.
    fn add(&mut self, client: &str, what: What, now: Instant) -> Result<&mut Ask> {
        let ring = match what {
            What::Loudness => meters::LOUD,
            What::Box { .. } => {
                let free = (meters::LOUD + 1..RINGS).find(|r| !self.asked.iter().any(|a| a.ring == *r));
                free.with_context(|| format!("{} scopes are showing already: close one, then try again.", RINGS - 1))?
            }
        };
        let clients = vec![(client.to_string(), now)];
        self.asked.push(Ask { what, ring, clients, channels: None, found: None, gone: false, why: None });
        Ok(self.asked.last_mut().expect("just pushed"))
    }

    /// Each ask with no client left goes, its ring pointed nowhere in `taps`.
    fn prune(&mut self, taps: Option<&TapSlots>) {
        self.asked.retain(|a| {
            let keep = !a.clients.is_empty();
            if !keep && let Some(t) = taps {
                let _ = t.point(a.ring, None);
            }
            keep
        });
    }

    /// `client` stops asking for `what`, a box gone from the song by that id too.
    fn stop(&mut self, client: &str, what: &What, taps: Option<&TapSlots>) {
        for a in self.asked.iter_mut().filter(|a| a.what == *what) {
            a.clients.retain(|c| c.0 != client);
        }
        self.prune(taps);
    }

    /// `client` is gone (its window closed): it asks for nothing now.
    fn gone(&mut self, client: &str, taps: Option<&TapSlots>) {
        self.asked.iter_mut().for_each(|a| a.clients.retain(|c| c.0 != client));
        self.prune(taps);
    }

    /// Each client that hasn't asked again for [`KEEP`] stops asking.
    pub(super) fn expire(&mut self, now: Instant, taps: Option<&TapSlots>) {
        self.asked.iter_mut().for_each(|a| a.clients.retain(|c| now.duration_since(c.1) < KEEP));
        self.prune(taps);
    }

    /// Whether anyone asks for the song's loudness.
    pub(super) fn loudness(&self) -> bool {
        self.asked.iter().any(|a| a.what == What::Loudness)
    }

    /// Follow each box into `accepted`, the revision the view now shows (TAP-9): its call, carried
    /// there by its `(` as ID-13 names it, under the id the new view gives it; or gone, waiting for
    /// that call as it was last found (an undo brings it back). The same code: nothing to do.
    pub(super) fn follow(&mut self, accepted: &Arc<Revision>) {
        for a in &mut self.asked {
            let What::Box { node, .. } = &mut a.what else { continue };
            let Some(from) = a.found.clone() else { continue };
            if Arc::ptr_eq(from.compiled.code(), accepted.compiled.code()) {
                a.found = Some(accepted.clone());
                continue;
            }
            match m2_editor::scope::carry(&from, accepted, node) {
                Some(id) => (*node, a.found, a.gone) = (id, Some(accepted.clone()), false),
                None => a.gone = true,
            }
        }
    }

    /// Point each ask's ring of `taps`, a plan built from `plays` (TAP-9): the loudness's at the
    /// output; a box's at its call in `accepted`, by the view's id, when `plays` is built from the
    /// same code, else nowhere, saying why; a gone box's nowhere.
    pub(super) fn point(&mut self, taps: &TapSlots, plays: &Revision, accepted: &Arc<Revision>, kernels: &dyn Kernels) {
        let shown = Arc::ptr_eq(plays.compiled.code(), accepted.compiled.code());
        for a in &mut self.asked {
            let What::Box { module, node } = &a.what else {
                let _ = taps.point(a.ring, Some(TapAt::Out));
                continue;
            };
            (a.channels, a.why) = (None, None);
            if a.gone {
                let _ = taps.point(a.ring, None);
                continue;
            }
            if !shown {
                a.why = Some(ELSEWHERE);
                let _ = taps.point(a.ring, None);
                continue;
            }
            match m2_editor::scope::point(accepted, kernels, module, node) {
                Ok((at, c)) if taps.point(a.ring, Some(at)).is_ok() => {
                    a.channels = Some(c);
                    a.found = Some(accepted.clone());
                }
                _ => {
                    let _ = taps.point(a.ring, None);
                }
            }
        }
    }

    /// Each box's last [`SCOPE`] frames in `taps`, or why it has none, with the clients it goes
    /// to; a gone box, or one whose ring has nothing yet, sends nothing.
    pub(super) fn frames(&self, taps: &TapSlots) -> Vec<Shown> {
        let mut out = vec![];
        for a in &self.asked {
            let What::Box { module, node } = &a.what else { continue };
            let clients = a.clients.iter().map(|c| c.0.clone()).collect();
            let (module, node) = (module.clone(), node.clone());
            if let Some(why) = a.why.filter(|_| !a.gone) {
                out.push(Shown { module, node, channels: 0, frames: vec![], why: Some(why), clients });
                continue;
            }
            let (Some(channels), false) = (a.channels, a.gone) else { continue };
            let mut frames = Vec::with_capacity(SCOPE);
            if taps.frames(a.ring, 0, SCOPE, &mut frames).is_some() && !frames.is_empty() {
                out.push(Shown { module, node, channels, frames, why: None, clients });
            }
        }
        out
    }
}

/// What an ask for a box answers: its channels, why it waits, and its last frames when it has them.
fn answer(module: &str, node: &str, a: &Ask, taps: Option<&TapSlots>) -> Value {
    let mut scope = json!({"module": module, "node": node, "channels": a.channels});
    if let Some(why) = a.why {
        scope["why"] = json!(why);
    }
    let mut frames = vec![];
    if let (Some(c), Some(t)) = (a.channels, taps)
        && t.frames(a.ring, 0, SCOPE, &mut frames).is_some()
        && !frames.is_empty()
    {
        scope["frames"] = json!(sides(&frames, c));
    }
    json!({"scope": scope})
}

impl Graph {
    /// Start, keep or stop the scope of box `node` of module `module`, by the shown view's id, for
    /// `client` (`POST /api/graph/scope`, TAP-8). Not a write: it works read-only too. A scope asked
    /// for already is kept on the control plane alone; a new one is found in the accepted revision,
    /// on the session's actor, and pointed in what plays. Answers `{scope: {module, node, channels,
    /// frames}}`: `channels` null and `why` while it can't show, its last frames when its ring has
    /// them (an agent's way to see them); `{}` for a stop. A box that isn't sound is refused in
    /// words.
    pub fn scope(&self, token: u64, client: &str, module: &str, node: &str, on: bool) -> Result<Value> {
        let what = What::Box { module: module.to_string(), node: node.to_string() };
        {
            let mut cp = self.inner.control.lock();
            let open = cp.open.as_ref().filter(|o| o.engine.token == token).context("the open song changed; refresh it first")?;
            let taps = open.engine.plays.as_ref().and_then(|p| p.info.taps.clone());
            if !on {
                cp.asks.stop(client, &what, taps.as_deref());
                return Ok(json!({}));
            }
            if let Some(a) = cp.asks.renew(client, &what, Instant::now()) {
                return Ok(answer(module, node, a, taps.as_deref()));
            }
        }
        let (client, module_, node_) = (client.to_string(), module.to_string(), node.to_string());
        self.call(move |a| a.scope(token, &client, &module_, &node_))
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
            let ring = cp.asks.add(client, What::Loudness, now)?.ring;
            if let Some(t) = &taps {
                t.point(ring, Some(TapAt::Out))?;
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
    /// A new scope (TAP-8): its box found in the accepted revision, by the view's id, and its ring
    /// pointed at it in the plan that plays, when that plan is built from the same code. Here, on
    /// the actor, no plan is sent meanwhile, and every later one points it ([`Asks::point`]).
    pub(super) fn scope(&mut self, token: u64, client: &str, module: &str, node: &str) -> Result<Value> {
        let o = self.open.as_ref().filter(|o| o.token == token).context("the open song changed; refresh it first")?;
        let accepted = o.session.accepted_arc();
        m2_editor::scope::point(&accepted, &o.session.packages, module, node).map_err(anyhow::Error::msg)?;
        let what = What::Box { module: module.to_string(), node: node.to_string() };
        let mut cp = self.inner.control.lock();
        let now = Instant::now();
        if cp.asks.renew(client, &what, now).is_none() {
            cp.asks.add(client, what.clone(), now)?.found = Some(accepted.clone());
        }
        let taps = o.plays.as_ref().and_then(|p| Some((p.info.taps.clone()?, p.revision.clone())));
        if let Some((taps, plays)) = &taps {
            cp.asks.point(taps, plays, &accepted, &o.session.packages);
        }
        let a = cp.asks.find(&what).context("the scope ended as it was asked for")?;
        if !a.asked_by(client) {
            bail!("the scope ended as it was asked for");
        }
        Ok(answer(module, node, a, taps.as_ref().map(|t| &*t.0)))
    }
}

#[cfg(test)]
#[path = "scope_tests.rs"]
mod tests;
