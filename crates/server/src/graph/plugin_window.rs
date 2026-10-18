//! Plugin windows on the server (HOST-4, HOST-7, HOME-5, HOME-5a, HOME-6, MON-6).
//!
//! In short:
//! - A window opens and closes through the host (`MainCall::Window`), off the actor, from
//!   `POST /api/graph/plugin/window`. The graph state's `plugins` says, for each plugin node by
//!   its name in the checked graph, whether its window is `open`, `closed` or `none` (it has
//!   none), and which parameters are latched.
//! - What a window does reaches the actor at its tick (every job, and the watcher's catch-up
//!   every 300 ms), from each instance's atomics (`m2_runtime::plugins`):
//!
//!   | The parameter is | A gesture in the window |
//!   |---|---|
//!   | a value in code | held by the node from its first event; a lease on the literal begins, follows the value, and its end commits one Tx; then the node sends the code's value again (the new one, or the old one when the Tx was refused) |
//!   | driven by a lane or a cable | latched by the node: the driven value isn't sent until "Re-enable" (`POST /api/graph/plugin/latch`). For this session only; an export never borrows a latched instance |
//!   | one code doesn't set | the state changed: autosave |
//!
//!   A change outside a gesture to a parameter code sets was set back by the node at the next
//!   block; the actor says so once per tick, `plugin.param-reverted`.
//! - Autosave: `HostEvent::Dirty`, or a change to a state parameter, then [`QUIET`] with no other;
//!   then `save_state` (off the actor) and the `plugin_state` Tx (blob and sidecar, one Undo step).
//!   The load the next plan makes is skipped by hash. A save that matches the file writes nothing.
//!   An export saves what waits first (RENDER-1).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use m2_editor::Control;
use m2_ir::{FOp, Input};
use m2_plugins::{Blob, Handle, MainCall, Reply, WindowCall};
use m2_runtime::PluginInstance;
use m2_runtime::plugins::{GESTURE, HELD, LATCHED};
use m2_song::{Key, Sidecar};
use serde_json::{Value, json};

use super::actor::{Actor, Open, Plays, plugin_named};
use super::lease::{self, State};
use super::{Graph, Owner};

/// How long a plugin's state must stay unchanged before it is saved (HOME-6).
pub const QUIET: Duration = Duration::from_secs(1);

/// A save that arrived: the instance, and its state or why it couldn't save it.
type Saved = (Handle, Result<Blob, String>);

/// What the session keeps of plugin windows.
pub(crate) struct Windows {
    /// Instances whose state changed in their window, and when last: saved after [`QUIET`].
    dirty: HashMap<Handle, Instant>,
    /// Saves on their way, and where they arrive.
    saving: HashSet<Handle>,
    saved: (Sender<Saved>, Receiver<Saved>),
    /// The lease of each held literal, by instance and parameter.
    leases: HashMap<(Handle, u32), u64>,
    /// Plugin nodes told they can't save, by name: said once.
    unsaveable: HashSet<String>,
}

impl Default for Windows {
    fn default() -> Windows {
        Windows { dirty: HashMap::new(), saving: HashSet::new(), saved: channel(), leases: HashMap::new(), unsaveable: HashSet::new() }
    }
}

/// What a tick decided for a held literal.
enum Held {
    Begin(Handle, u32, Box<Control>),
    Update(u64, f64),
    Commit(Handle, u32, u64, f64),
    /// Not a literal that can be written, or its lease failed: the code's value again, once the
    /// gesture ended.
    Refuse(Arc<PluginInstance>, u32),
}

/// The plugin node `node` of what plays, by its name in the checked graph.
fn node_named(p: &Plays, name: &str) -> Option<usize> {
    p.info.plugins.iter().map(|at| at.node).find(|&n| p.revision.compiled.flat.nodes.get(n).is_some_and(|x| x.name == name))
}

/// The control of the literal plugin node `node`'s parameter `id` is set by, in the accepted
/// revision: none when it isn't a value written in code.
fn literal(o: &Open, p: &Plays, node: usize, id: u32) -> Option<Control> {
    let flat = &p.revision.compiled.flat;
    let FOp::Plugin { plugin, inputs, .. } = &flat.nodes.get(node)?.op else { return None };
    let sig = m2_ir::Kernels::plugin(&o.session.packages, plugin)?;
    let at = sig.inputs.iter().position(|port| m2_runtime::packages::address(&port.name).is_some_and(|(_, pid)| pid == id))?;
    let Some(Input::Port { src, .. }) = inputs.get(at) else { return None };
    let n = &flat.nodes[src.node];
    let (FOp::Const(_), Some(origin)) = (&n.op, &n.origin) else { return None };
    o.controls().values().find(|c| c.origin.same_value(origin)).cloned()
}

/// What people call parameter `id` of plugin node `node`.
fn param_name(o: &Open, p: &Plays, node: usize, id: u32) -> String {
    let plugin = match p.revision.compiled.flat.nodes.get(node).map(|n| &n.op) {
        Some(FOp::Plugin { plugin, .. }) => plugin.as_str(),
        _ => "",
    };
    o.session
        .packages
        .plugin_info(plugin)
        .and_then(|i| i.params.iter().find(|q| q.id == id))
        .map_or(format!("parameter {id}"), |q| q.name.clone())
}

fn list(v: &[String]) -> String {
    match v {
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
        [] => String::new(),
    }
}

impl Actor {
    /// The tick: what each instance's window did since the last one (see the module's table).
    /// `dirty` are the instances that said `mark_dirty`, `closed` those whose window the person
    /// closed.
    pub(super) fn heard_windows(&mut self, dirty: Vec<Handle>, closed: Vec<Handle>) {
        {
            let mut open = self.inner.open_windows.lock();
            for h in &closed {
                open.remove(&h.0);
            }
        }
        while let Some((h, r)) = self.open.as_ref().and_then(|o| o.windows.saved.1.try_recv().ok()) {
            self.autosaved(h, r);
        }
        let Some(o) = self.open.as_mut() else { return };
        let Some(p) = o.plays.clone() else { return };
        let now = Instant::now();
        for h in dirty {
            o.windows.dirty.insert(h, now);
        }
        let (mut said, mut held) = (vec![], vec![]);
        for at in &p.info.plugins {
            let Some(inst) = at.inst.upgrade() else { continue };
            if inst.take_changed() {
                o.windows.dirty.insert(at.handle, now);
            }
            if o.windows.dirty.contains_key(&at.handle) {
                inst.set_unsaved(true);
            }
            let mut back: Vec<String> = vec![];
            for id in inst.take_reverted() {
                let name = param_name(o, &p, at.node, id);
                if !back.contains(&name) {
                    back.push(name);
                }
            }
            if !back.is_empty() {
                let who = plugin_named(&p.revision, &o.session.packages, at.node);
                let it = if back.len() == 1 { "it" } else { "them" };
                said.push(format!(
                    "{who} changed {} by itself (a preset?), but the song's code sets {it}, so music2 set {it} back. To change {it}, \
                     drag {it} in the plugin's window, or change the code. (plugin.param-reverted)",
                    list(&back)
                ));
            }
            for (id, flags, value) in inst.overrides().list() {
                if flags & HELD == 0 {
                    continue;
                }
                let gesture = flags & GESTURE != 0;
                held.push(match o.windows.leases.get(&(at.handle, id)) {
                    Some(&l) if gesture => Held::Update(l, value),
                    Some(&l) => Held::Commit(at.handle, id, l, value),
                    None => match literal(o, &p, at.node, id) {
                        Some(c) => Held::Begin(at.handle, id, Box::new(c)),
                        None if gesture => continue,
                        None => Held::Refuse(inst.clone(), id),
                    },
                });
            }
        }
        self.inner.notices.lock().extend(said);
        for h in held {
            self.held(h, &p);
        }
        self.autosave_due();
        self.publish_plugins();
    }

    /// One held literal's step: its lease begun, moved, or committed (HOME-5, HOME-5a).
    fn held(&mut self, h: Held, p: &Plays) {
        let Some(o) = self.open.as_mut() else { return };
        let token = o.token;
        let inst = |h: Handle| p.info.plugins.iter().find(|at| at.handle == h).and_then(|at| at.inst.upgrade());
        match h {
            Held::Begin(h, id, c) => {
                let revision = o.session.accepted().number;
                let owner = Owner { client: None, gesture: Some(format!("plugin window {} {id}", h.0)), ttl: Some(lease::MAX_TTL) };
                match self.begin(token, revision, &c.id, owner) {
                    Ok(b) => {
                        if let Some(o) = self.open.as_mut() {
                            o.windows.leases.insert((h, id), b.lease);
                        }
                        // Its gesture may have ended already: the next tick commits it.
                    }
                    Err(e) => {
                        self.inner.notices.lock().push(format!("A change in a plugin's window wasn't saved: {e:#}"));
                        if let Some(i) = inst(h) {
                            i.overrides().release(id, HELD);
                        }
                    }
                }
            }
            Held::Update(lease, value) => {
                let mut cp = self.inner.control.lock();
                if let Ok(i) = cp.leases.index(lease) {
                    let l = &mut cp.leases.live[i];
                    (l.playing, l.touched) = (value, Instant::now());
                }
            }
            Held::Commit(h, id, lease, value) => {
                o.windows.leases.remove(&(h, id));
                {
                    let mut cp = self.inner.control.lock();
                    if let Ok(i) = cp.leases.index(lease)
                        && matches!(cp.leases.live[i].state, State::Live)
                    {
                        cp.leases.live[i].state = State::Closing { value, expect: None };
                    }
                }
                if let Err(e) = self.commit(lease) {
                    self.inner
                        .notices
                        .lock()
                        .push(format!("A change in a plugin's window wasn't saved, so the code's value plays again: {e:#}"));
                }
                // Committed or refused: the node sends the code's value again, which is now the
                // window's when it was written.
                if let Some(i) = inst(h) {
                    i.overrides().release(id, HELD);
                }
            }
            Held::Refuse(i, id) => i.overrides().release(id, HELD),
        }
    }

    /// Start the save of each instance whose state has been quiet for [`QUIET`].
    fn autosave_due(&mut self) {
        let Some(o) = self.open.as_mut() else { return };
        let Some(p) = o.plays.clone() else { return };
        let due: Vec<Handle> =
            o.windows.dirty.iter().filter(|(h, t)| t.elapsed() >= QUIET && !o.windows.saving.contains(h)).map(|(h, _)| *h).collect();
        for h in due {
            o.windows.dirty.remove(&h);
            let Some(at) = p.info.plugins.iter().find(|at| at.handle == h) else { continue };
            let Some(inst) = at.inst.upgrade() else { continue };
            let n = &p.revision.compiled.flat.nodes[at.node];
            if !matches!(&n.op, FOp::Plugin { state: Some(_), .. }) {
                if o.windows.unsaveable.insert(n.name.clone()) {
                    let who = plugin_named(&p.revision, &o.session.packages, at.node);
                    self.inner.notices.lock().push(format!(
                        "{who}'s settings changed in its window, but music2 can't save them: give its plugin() call a key, such as key: \"lead\"."
                    ));
                }
                continue;
            }
            o.windows.saving.insert(h);
            let (rx, tx) = (inst.host().save_state(h), o.windows.saved.0.clone());
            std::thread::spawn(move || {
                let r = rx.recv().unwrap_or_else(|_| Err("the plugin didn't answer".into()));
                let _ = tx.send((h, r));
            });
        }
    }

    /// A save arrived: the blob and its sidecar as one Tx (FMT-17, HOME-6), unless the file holds
    /// it already.
    fn autosaved(&mut self, h: Handle, r: Result<Blob, String>) {
        let Some(o) = self.open.as_mut() else { return };
        o.windows.saving.remove(&h);
        let Some(p) = o.plays.clone() else { return };
        let Some(at) = p.info.plugins.iter().find(|at| at.handle == h) else { return };
        let n = &p.revision.compiled.flat.nodes[at.node];
        let who = plugin_named(&p.revision, &o.session.packages, at.node);
        let blob = match r {
            Ok(b) => b,
            Err(e) => {
                self.inner.notices.lock().push(format!("{who}'s settings changed in its window, but it couldn't save them: {e}"));
                return;
            }
        };
        let FOp::Plugin { plugin, state: Some(keys), .. } = &n.op else { return };
        let inst = at.inst.upgrade();
        let accepted = o.session.accepted();
        let held = accepted.compiled.data.state_hash(&n.module, &keys.iter().map(String::as_str).collect::<Vec<_>>());
        let hex: String = blob.sha.iter().map(|b| format!("{b:02x}")).collect();
        if held.is_some_and(|s| s.hex() == hex) {
            if let Some(i) = inst.filter(|_| !o.windows.dirty.contains_key(&h)) {
                i.set_unsaved(false);
            }
            return;
        }
        let version = o.session.packages.plugin_info(plugin).map(|i| i.found.version.clone()).unwrap_or_default();
        let side = Sidecar { plugin: plugin.clone(), plugin_version: version };
        let (module, path) = match (Key::new(&n.module), keys.iter().map(|k| Key::new(k)).collect::<Result<Vec<_>, _>>()) {
            (Ok(m), Ok(p)) => (m, p),
            _ => return,
        };
        let token = o.token;
        let done = self.edit(token, move |s| m2_editor::data::plugin_state(s, &module, &path, blob.bytes, &side));
        match done {
            Ok(_) => {
                if let (Some(i), Some(o)) = (inst, self.open.as_ref())
                    && !o.windows.dirty.contains_key(&h)
                {
                    i.set_unsaved(false);
                }
            }
            Err(e) => self.inner.notices.lock().push(format!("{who}'s settings changed in its window, but they weren't saved: {e:#}")),
        }
    }

    /// Before an export (RENDER-1): every save that waits or is on its way, made now.
    pub(super) fn flush_autosaves(&mut self) {
        let Some(o) = self.open.as_mut() else { return };
        let Some(p) = o.plays.clone() else { return };
        let waiting: Vec<Handle> = o.windows.dirty.keys().chain(o.windows.saving.iter()).copied().collect();
        o.windows.dirty.clear();
        for h in waiting {
            let Some(inst) = p.info.plugins.iter().find(|at| at.handle == h).and_then(|at| at.inst.upgrade()) else { continue };
            if !matches!(
                &p.revision.compiled.flat.nodes[p.info.plugins.iter().find(|at| at.handle == h).map_or(0, |at| at.node)].op,
                FOp::Plugin { state: Some(_), .. }
            ) {
                continue;
            }
            let r =
                inst.host().save_state(h).recv_timeout(Duration::from_secs(30)).unwrap_or_else(|_| Err("the plugin didn't answer".into()));
            self.autosaved(h, r);
        }
    }

    /// The instance plugin node `node` plays, and its window's title.
    pub(super) fn plugin_at(&mut self, token: u64, node: &str) -> Result<(Arc<PluginInstance>, String)> {
        let o = self.open.as_ref().context("no project is open")?;
        if o.token != token {
            bail!("the open project changed; refresh the graph before editing");
        }
        let p = o.plays.as_ref().context("the song isn't playing yet")?;
        let at = node_named(p, node).ok_or_else(|| anyhow!("no plugin plays as {node}"))?;
        let inst = p.info.plugin(at).and_then(|x| x.inst.upgrade()).ok_or_else(|| anyhow!("{node} isn't running now"))?;
        Ok((inst, plugin_named(&p.revision, &o.session.packages, at)))
    }

    /// "Re-enable": the lane or the cable drives parameter `id` of plugin node `node` again (all of
    /// its latched ones without an id) (MON-6).
    pub(super) fn reenable(&mut self, token: u64, node: &str, id: Option<u32>) -> Result<()> {
        let (inst, _) = self.plugin_at(token, node)?;
        for (pid, flags, _) in inst.overrides().list() {
            if flags & LATCHED != 0 && id.is_none_or(|x| x == pid) {
                inst.overrides().release(pid, LATCHED);
            }
        }
        self.publish_plugins();
        Ok(())
    }

    /// The graph state's `plugins`: each plugin node's window, and its latched parameters.
    pub(super) fn publish_plugins(&mut self) {
        let Some(o) = self.open.as_ref() else {
            *self.inner.plugins.lock() = json!({});
            return;
        };
        let Some(p) = o.plays.as_ref() else { return };
        let open = self.inner.open_windows.lock().clone();
        let mut out = serde_json::Map::new();
        for at in &p.info.plugins {
            let n = &p.revision.compiled.flat.nodes[at.node];
            let FOp::Plugin { plugin, .. } = &n.op else { continue };
            let gui = o.session.packages.plugin_info(plugin).is_some_and(|i| i.gui);
            let window = match (gui, open.contains(&at.handle.0)) {
                (false, _) => "none",
                (true, true) => "open",
                (true, false) => "closed",
            };
            let latched: Vec<Value> = at
                .inst
                .upgrade()
                .map(|i| i.overrides().list())
                .unwrap_or_default()
                .into_iter()
                .filter(|(_, f, _)| f & LATCHED != 0)
                .map(|(id, _, value)| json!({"id": id, "name": param_name(o, p, at.node, id), "value": value}))
                .collect();
            out.insert(n.name.clone(), json!({"window": window, "latched": latched}));
        }
        *self.inner.plugins.lock() = Value::Object(out);
    }
}

impl Graph {
    /// Open or close the window of plugin node `node` (`POST /api/graph/plugin/window`, HOST-7).
    /// The host's call is made here, off the session, as a plugin may take a while to draw.
    pub fn plugin_window(&self, session: u64, node: &str, open: bool) -> Result<Value> {
        let name = node.to_string();
        let (inst, title) = self.call(move |a| a.plugin_at(session, &name))?;
        let call = if open { WindowCall::Open { title } } else { WindowCall::Close };
        let now = match inst.host().main_call(inst.handle(), MainCall::Window(call))? {
            Reply::Window(now) => now,
            other => bail!("the plugin host answered {other:?}"),
        };
        {
            let mut w = self.inner.open_windows.lock();
            if now {
                w.insert(inst.handle().0);
            } else {
                w.remove(&inst.handle().0);
            }
        }
        self.call(|a| {
            a.publish_plugins();
            Ok(())
        })?;
        Ok(json!({"window": if now { "open" } else { "closed" }}))
    }

    /// "Re-enable" (`POST /api/graph/plugin/latch`, MON-6): the latched parameter `id` of plugin
    /// node `node`, or all of them, follows its lane or cable again.
    pub fn plugin_reenable(&self, session: u64, node: &str, id: Option<u32>) -> Result<Value> {
        let name = node.to_string();
        self.call(move |a| a.reenable(session, &name, id))?;
        Ok(json!({}))
    }
}

#[cfg(test)]
#[path = "plugin_window_tests.rs"]
mod tests;
