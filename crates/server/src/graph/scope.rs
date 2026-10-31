//! Scopes (TAP-8, TAP-9): a window or an agent asks for the sound of one box of the graph view, by
//! its id (`POST /api/graph/scope`), and every meters message carries that box's last
//! [`SCOPE`] frames while it asks.
//!
//! - **One ring each.** A plan has [`m2_runtime::RINGS`] rings; the first is the output's, for its
//!   loudness ([`super::meters::LOUD`]), the rest are for scopes. Two clients asking for the same box
//!   share its ring. A fifth box is refused in words.
//! - **While it asks.** A scope lasts while its client asks: it ends when the client says stop,
//!   when its window's socket closes ([`Graph::client_gone`]), or [`KEEP`] after its last ask (a
//!   window asks again every few seconds).
//! - **Found again in each plan.** The box is found in the revision that plays, on the session's
//!   actor, where every plan is sent: so a new scope is pointed in the plan that plays, and every
//!   new plan has each scope pointed before it is sent ([`Scopes::point`]). A box is its call: an
//!   edit that moves the call to another place keeps the scope on it, under the id the new view
//!   gives it. A box gone from a plan sends nothing until it is back or its scope ends.

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
/// How long a scope lasts after its client last asked.
pub const KEEP: Duration = Duration::from_secs(10);

/// The scopes asked for in the open song, on the control plane.
#[derive(Default)]
pub(super) struct Scopes {
    asked: Vec<Scope>,
}

struct Scope {
    module: String,
    /// The box's id in the view of the revision it was last found in.
    node: String,
    ring: usize,
    /// Each client asking, and when it last asked.
    clients: Vec<(String, Instant)>,
    /// Its channels in the plan that plays, once found there.
    channels: Option<u8>,
    /// Where it was last found: the revision, and the call's node in its graph.
    found: Option<(Arc<Revision>, usize)>,
}

/// One scope's frames, as read for a message.
pub(super) struct Shown {
    module: String,
    node: String,
    channels: u8,
    frames: Vec<[f32; 2]>,
}

impl Shown {
    /// `{"module", "node", "frames": [[…], …]}`: a list per channel, each sample to a millionth.
    pub(super) fn json(&self) -> Value {
        let side = |c: usize| self.frames.iter().map(|f| (f[c] as f64 * 1e6).round() / 1e6).collect::<Vec<_>>();
        let frames: Vec<Vec<f64>> = (0..(self.channels as usize).clamp(1, 2)).map(side).collect();
        json!({"module": self.module, "node": self.node, "frames": frames})
    }
}

impl Scopes {
    fn find(&mut self, module: &str, node: &str) -> Option<&mut Scope> {
        self.asked.iter_mut().find(|s| s.module == module && s.node == node)
    }

    /// `client` asks for a scope asked already: it lasts [`KEEP`] from now. Its channels, when it is
    /// found in what plays; None when no one asked for it yet.
    fn renew(&mut self, client: &str, module: &str, node: &str, now: Instant) -> Option<Option<u8>> {
        let s = self.find(module, node)?;
        match s.clients.iter_mut().find(|c| c.0 == client) {
            Some(c) => c.1 = now,
            None => s.clients.push((client.to_string(), now)),
        }
        Some(s.channels)
    }

    /// A new scope, on a ring no other scope has: refused in words when every one is taken.
    fn add(&mut self, client: &str, module: &str, node: &str, now: Instant) -> Result<&mut Scope> {
        let Some(ring) = (meters::LOUD + 1..RINGS).find(|r| !self.asked.iter().any(|s| s.ring == *r)) else {
            bail!("{} scopes are showing already: close one, then try again.", RINGS - 1);
        };
        let clients = vec![(client.to_string(), now)];
        self.asked.push(Scope { module: module.to_string(), node: node.to_string(), ring, clients, channels: None, found: None });
        Ok(self.asked.last_mut().expect("just pushed"))
    }

    /// Each scope with no client left goes, its ring pointed nowhere in `taps`.
    fn prune(&mut self, taps: Option<&TapSlots>) {
        self.asked.retain(|s| {
            let keep = !s.clients.is_empty();
            if !keep && let Some(t) = taps {
                let _ = t.point(s.ring, None);
            }
            keep
        });
    }

    /// `client` stops asking for one scope.
    fn stop(&mut self, client: &str, module: &str, node: &str, taps: Option<&TapSlots>) {
        if let Some(s) = self.find(module, node) {
            s.clients.retain(|c| c.0 != client);
        }
        self.prune(taps);
    }

    /// `client` is gone (its window closed): it asks for nothing now.
    fn gone(&mut self, client: &str, taps: Option<&TapSlots>) {
        self.asked.iter_mut().for_each(|s| s.clients.retain(|c| c.0 != client));
        self.prune(taps);
    }

    /// Each client that hasn't asked again for [`KEEP`] stops asking.
    pub(super) fn expire(&mut self, now: Instant, taps: Option<&TapSlots>) {
        self.asked.iter_mut().for_each(|s| s.clients.retain(|c| now.duration_since(c.1) < KEEP));
        self.prune(taps);
    }

    /// Point each scope's ring of `taps` at its box in `rev`, the revision its plan was built from
    /// (TAP-9). A box is its call: one an edit moved, which the new view gives another id, is found
    /// as the call that continues it (ID-13), under that id. A box gone points nowhere.
    pub(super) fn point(&mut self, taps: &TapSlots, rev: &Arc<Revision>, kernels: &dyn Kernels) {
        for s in &mut self.asked {
            if let Some((from, node)) = &s.found
                && !Arc::ptr_eq(from.compiled.code(), rev.compiled.code())
            {
                let cont = super::actor::continuing(from, rev);
                let now = cont.top.iter().position(|j| *j == Some(*node as u32));
                if let Some(id) = now.and_then(|n| m2_editor::scope::id_of(rev, kernels, &s.module, n)) {
                    s.node = id;
                }
            }
            s.channels = None;
            s.found = None;
            if let Ok((at, c)) = m2_editor::scope::point(rev, kernels, &s.module, &s.node)
                && taps.point(s.ring, Some(at)).is_ok()
            {
                s.channels = Some(c);
                s.found = node_of(at).map(|n| (rev.clone(), n));
            } else {
                let _ = taps.point(s.ring, None);
            }
        }
    }

    /// Each scope's last [`SCOPE`] frames in `taps`, when it is found there and its ring has them.
    pub(super) fn frames(&self, taps: &TapSlots) -> Vec<Shown> {
        let mut out = vec![];
        for s in &self.asked {
            let Some(channels) = s.channels else { continue };
            let mut frames = Vec::with_capacity(SCOPE);
            if taps.frames(s.ring, 0, SCOPE, &mut frames).is_some() && !frames.is_empty() {
                out.push(Shown { module: s.module.clone(), node: s.node.clone(), channels, frames });
            }
        }
        out
    }
}

/// The flat node a point reads.
fn node_of(at: TapAt) -> Option<usize> {
    match at {
        TapAt::Node(n) => Some(n),
        _ => None,
    }
}

impl Graph {
    /// Start, keep or stop the scope of box `node` of module `module` for `client`
    /// (`POST /api/graph/scope`, TAP-8). Not a write: it works read-only too. A scope asked for
    /// already is kept on the control plane alone; a new one is found in the revision that plays,
    /// on the session's actor, and pointed there. Answers `{scope: {module, node, channels}}`
    /// (`channels` null while nothing plays it), or `{}` for a stop; a box that isn't sound is
    /// refused in words.
    pub fn scope(&self, token: u64, client: &str, module: &str, node: &str, on: bool) -> Result<Value> {
        let answer = |channels: Option<u8>| json!({"scope": {"module": module, "node": node, "channels": channels}});
        {
            let mut cp = self.inner.control.lock();
            let open = cp.open.as_ref().filter(|o| o.engine.token == token).context("the open song changed; refresh it first")?;
            let taps = open.engine.plays.as_ref().and_then(|p| p.info.taps.clone());
            if !on {
                cp.scopes.stop(client, module, node, taps.as_deref());
                return Ok(json!({}));
            }
            if let Some(channels) = cp.scopes.renew(client, module, node, Instant::now()) {
                return Ok(answer(channels));
            }
        }
        let (client, module_, node_) = (client.to_string(), module.to_string(), node.to_string());
        let channels = self.call(move |a| a.scope(token, &client, &module_, &node_))?;
        Ok(answer(channels))
    }

    /// `client`'s window closed: its scopes end (TAP-8).
    pub(super) fn scopes_gone(&self, client: &str) {
        let mut cp = self.inner.control.lock();
        let taps = cp.open.as_ref().and_then(|o| o.engine.plays.as_ref()).and_then(|p| p.info.taps.clone());
        cp.scopes.gone(client, taps.as_deref());
    }
}

impl Actor {
    /// A new scope (TAP-8): its box found in the revision that plays, else the accepted one, and
    /// its ring pointed there, in the plan that plays. Here, on the actor, no plan is sent meanwhile,
    /// and every later one points it ([`Scopes::point`]). Its channels, None while nothing plays.
    pub(super) fn scope(&mut self, token: u64, client: &str, module: &str, node: &str) -> Result<Option<u8>> {
        let o = self.open.as_ref().filter(|o| o.token == token).context("the open song changed; refresh it first")?;
        let plays = o.plays.as_ref();
        let rev = plays.map_or_else(|| o.session.accepted_arc(), |p| p.revision.clone());
        let (at, channels) = m2_editor::scope::point(&rev, &o.session.packages, module, node).map_err(anyhow::Error::msg)?;
        let mut cp = self.inner.control.lock();
        let scope = match cp.scopes.renew(client, module, node, Instant::now()) {
            Some(_) => cp.scopes.find(module, node).expect("renewed"),
            None => cp.scopes.add(client, module, node, Instant::now())?,
        };
        scope.found = node_of(at).map(|n| (rev.clone(), n));
        let Some(taps) = plays.and_then(|p| p.info.taps.as_ref()) else { return Ok(None) };
        taps.point(scope.ring, Some(at))?;
        scope.channels = Some(channels);
        Ok(Some(channels))
    }
}

#[cfg(test)]
#[path = "scope_tests.rs"]
mod tests;
