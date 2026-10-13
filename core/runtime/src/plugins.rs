//! Plugin instances carried across rebuilds (HOST-5, RENDER-25): a new renderer takes the
//! instances of the one it replaces that match by module, plugin id and version, state path,
//! ordinal and state hash, with their state, their tails and the notes sounding into them intact.
//!
//! - A renderer gets its instances from an [`InstanceSource`]: a [`Carry`] of the renderer last sent
//!   to the same engine, or [`Fresh`], which takes nothing (a new engine, an offline render).
//! - At activation (`Engine`, at the block line), [`carry_from`] pairs each top-level plugin node of
//!   the new renderer with the old renderer's node that holds the same instance: the new lane
//!   scheduler adopts the notes sounding into it, keeping their ids, and the old node is lent.
//! - During the 5 ms crossfade the new node alone processes the instance, once per engine block,
//!   and the lent node copies its output ([`lend`]), so both sides of the fade hold the same sound.
//! - The last reference to an instance destroys it on the plugin main thread (queued, never on the
//!   audio thread: renderers are retired off it).
//!
//! Step 7's arena replaces `Carry`, `carry_from` and lending; `InstanceSource` and
//! [`PluginInstance`]'s retire-on-drop stay.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use m2_plugins::{Handle, PluginHost};
use m2_song::Sha;

use crate::{Exec, Graph, LaneSlot, Renderer};

/// What an instance must match to be carried into a new renderer (HOST-5).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct CarryKey {
    /// The node's module key.
    pub module: String,
    /// The CLAP id and version: RENDER-25's shape for a plugin.
    pub plugin: String,
    pub version: String,
    /// Its state path's keys (ID-8), when it has one: two keyed instances of one plugin match by key,
    /// whatever order the code makes them in.
    pub state_path: Option<Vec<String>>,
    /// Its place among the top-level nodes with the same module, plugin and state path, in flat
    /// order: what tells unkeyed instances apart until step 7's RuntimeIds.
    pub ordinal: u32,
    /// The verified state blob's hash, from the snapshot: a changed blob is a new instance (no
    /// `LoadState` until step 7).
    pub state: Option<Sha>,
}

/// A live plugin instance, shared by the renderers that use it. Dropping the last reference queues
/// its destruction on the plugin main thread.
pub struct PluginInstance {
    host: Arc<dyn PluginHost>,
    h: Handle,
    key: CarryKey,
    /// The rate it was activated at.
    rate: u32,
    /// It asked to be restarted (`HostEvent::Restart`): never carried again.
    restart: AtomicBool,
    /// The most notes any node that used it could hold: the release room a carry needs (§5.3).
    held_cap: AtomicU32,
    /// Above every note id it was sent, as of its last carry: kept here, since a renderer between
    /// whose node reads no notes has no scheduler to pass the ids on (RENDER-26a, C7).
    next_id: AtomicU32,
}

impl PluginInstance {
    /// Take ownership of `h`, created through `host` and to be activated at `rate`.
    pub(crate) fn new(host: Arc<dyn PluginHost>, h: Handle, key: CarryKey, rate: u32) -> PluginInstance {
        let (restart, held_cap, next_id) = (AtomicBool::new(false), AtomicU32::new(0), AtomicU32::new(0));
        PluginInstance { host, h, key, rate, restart, held_cap, next_id }
    }

    pub fn handle(&self) -> Handle {
        self.h
    }

    pub fn key(&self) -> &CarryKey {
        &self.key
    }

    pub(crate) fn host(&self) -> &dyn PluginHost {
        &*self.host
    }

    /// The plugin asked for a restart: a rebuild gives it a new instance.
    pub fn mark_restart(&self) {
        self.restart.store(true, Ordering::Relaxed);
    }

    /// Make room for `held` notes to be released when it is carried.
    pub(crate) fn hold(&self, held: usize) -> usize {
        self.held_cap.fetch_max(held as u32, Ordering::Relaxed).max(held as u32) as usize
    }
}

impl Drop for PluginInstance {
    fn drop(&mut self) {
        // Queued to the main thread (HOST-2); never on the audio thread, which retires renderers.
        self.host.destroy(self.h);
    }
}

impl std::fmt::Debug for PluginInstance {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("PluginInstance").field("h", &self.h).field("key", &self.key).finish()
    }
}

/// Where a renderer's builder gets plugin instances before it creates new ones.
pub trait InstanceSource {
    /// An instance that matches `key`, activated at `rate`, if there is one to take.
    fn take(&mut self, key: &CarryKey, rate: u32) -> Option<Arc<PluginInstance>>;
}

/// Takes nothing: every instance is created.
pub struct Fresh;

impl InstanceSource for Fresh {
    fn take(&mut self, _: &CarryKey, _: u32) -> Option<Arc<PluginInstance>> {
        None
    }
}

/// The top-level instances of the renderer last sent to one engine, for the next renderer built
/// for it. Tagged with that engine's epoch: a renderer for another engine (a new player after a
/// stall, a rate change) gets [`Fresh`], so two audio threads can never share an instance.
pub struct Carry {
    epoch: u64,
    rate: u32,
    pool: Vec<(Arc<PluginInstance>, bool)>,
    fresh: Fresh,
}

impl Carry {
    pub fn none() -> Carry {
        Carry { epoch: u64::MAX, rate: 0, pool: vec![], fresh: Fresh }
    }

    /// The top-level instances of `r`, which plays in the engine `epoch`.
    pub fn of(r: &Renderer, epoch: u64) -> Carry {
        let pool = r.graph.exec.iter().filter_map(|e| if let Exec::Plugin(p) = e { Some((p.inst.clone(), false)) } else { None }).collect();
        Carry { epoch, rate: r.sample_rate(), pool, fresh: Fresh }
    }

    /// A source for a renderer that will play in the engine `epoch`: this pool when it is the same
    /// engine, else [`Fresh`]. Each build gets the whole pool: what a build that failed took, it
    /// gave back as it was dropped, and the next build from the same carry takes it again.
    pub fn source(&mut self, epoch: u64) -> &mut dyn InstanceSource {
        if epoch != self.epoch {
            return &mut self.fresh;
        }
        for (_, taken) in &mut self.pool {
            *taken = false;
        }
        self
    }

    /// The instances it holds.
    pub fn handles(&self) -> Vec<Handle> {
        self.pool.iter().map(|(i, _)| i.h).collect()
    }

    /// Mark the instance `h` not to be carried again (the plugin asked for a restart).
    pub fn restart(&self, h: Handle) {
        for (i, _) in &self.pool {
            if i.h == h {
                i.mark_restart();
            }
        }
    }
}

impl InstanceSource for Carry {
    fn take(&mut self, key: &CarryKey, rate: u32) -> Option<Arc<PluginInstance>> {
        if rate != self.rate {
            return None;
        }
        let (inst, taken) = self.pool.iter_mut().find(|(i, taken)| !taken && i.key == *key && !i.restart.load(Ordering::Relaxed))?;
        debug_assert_eq!(inst.rate, rate);
        *taken = true;
        Some(inst.clone())
    }
}

/// At activation, on the audio thread: `new` takes over the instances it shares with `old` (§5.3).
/// For each top-level plugin node of `new` whose instance a node of `old` holds:
/// - when both read the same notes lane, `new`'s scheduler adopts the notes `old`'s turned on, with
///   their ids (once per lane), so the notes sounding into the instance go on (C7);
/// - `new`'s scheduler never gives an id the instance had (CLAP matches voices by id), even after a
///   renderer whose node read no notes: the instance keeps the next id across it;
/// - each note the old node held moves to the new node when its scheduler adopted it, and is
///   otherwise released at the new node's first call: a note the new data dropped, one past the new
///   scheduler's room, or one from another lane;
/// - the old node is lent: it no longer processes the instance, and copies the new node's output.
///
/// Never allocates: everything fits the room the nodes were built with.
pub(crate) fn carry_from(new: &mut Renderer, old: &mut Renderer) {
    let Renderer { graph: ng, lanes: nl, slots: nslots, .. } = new;
    let Renderer { graph: og, lanes: ol, slots: oslots, .. } = old;
    for ni in 0..ng.exec.len() {
        let Exec::Plugin(n) = &mut ng.exec[ni] else { continue };
        let found = og.exec.iter_mut().position(|e| matches!(e, Exec::Plugin(o) if o.lent.is_none() && o.inst.h == n.inst.h));
        let Some(oi) = found else { continue };
        let Exec::Plugin(o) = &mut og.exec[oi] else { unreachable!("found a plugin node") };
        let same_lane = match (n.notes, o.notes) {
            (Some(a), Some(b)) => nslots[a] == oslots[b],
            _ => false,
        };
        // The ids it had: the old node's scheduler's, and those the instance kept from before.
        if let Some(b) = o.notes
            && let LaneSlot::Notes(os) = &ol[b]
        {
            o.inst.next_id.fetch_max(os.next_id(), Ordering::Relaxed);
        }
        if let Some(a) = n.notes
            && let LaneSlot::Notes(ns) = &mut nl[a]
        {
            ns.raise_next_id(n.inst.next_id.load(Ordering::Relaxed));
            if same_lane
                && let Some(b) = o.notes
                && let LaneSlot::Notes(os) = &ol[b]
            {
                ns.adopt(os);
            }
        }
        let sched = match (n.notes, same_lane) {
            (Some(a), true) => match &nl[a] {
                LaneSlot::Notes(s) => Some(s),
                _ => None,
            },
            _ => None,
        };
        for &(id, key) in &o.keys {
            let to = if sched.is_some_and(|s| s.is_sounding(id)) { &mut n.keys } else { &mut n.release };
            if to.len() < to.capacity() {
                to.push((id, key));
            } else {
                debug_assert!(false, "no room for a carried note");
            }
        }
        o.keys.clear();
        // Choked and let go after a panic the old node hadn't sent yet: still owed.
        for &k in &o.ended {
            if n.ended.len() < n.ended.capacity() {
                n.ended.push(k);
            }
        }
        o.ended.clear();
        o.lent = Some(ni);
    }
}

/// Before the old renderer's block, during a crossfade: each lent node takes the output its
/// instance just made for the current renderer's node. The current renderer renders first.
pub(crate) fn lend(old: &mut Graph, current: &Graph) {
    for i in 0..old.exec.len() {
        if let Exec::Plugin(p) = &old.exec[i]
            && let Some(ni) = p.lent
            && let (Some(dst), Some(src)) = (old.bufs[i].first_mut(), current.bufs.get(ni).and_then(|b| b.first()))
        {
            let n = dst.len().min(src.len());
            dst[..n].copy_from_slice(&src[..n]);
        }
    }
}
