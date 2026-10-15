//! Plugin instances (HOST-5, RENDER-25): one is created, its verified state loaded and activated
//! on the plugin main thread while the plan before plays, then held by a plugin node. An activation
//! moves it into the next plan like every other box, so its state, its tail and the notes sounding
//! into it go on; a new committed state is loaded into it as that plan is prepared (RENDER-26b).
//! An export in the app borrows it (RENDER-1). The last reference to an instance destroys it on
//! the plugin main thread (queued, never on the audio thread: plans are retired off it).

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use anyhow::Result;
use m2_plugins::{Blob, Found, Handle, Owner, PluginHost};

use crate::BLOCK;

/// A live plugin instance, held by the plugin node that plays it. Dropping the last reference
/// queues its destruction on the plugin main thread.
pub struct PluginInstance {
    host: Arc<dyn PluginHost>,
    h: Handle,
    /// The hash of the shape it was made for: what an activation checks before moving it.
    pub(crate) shape: u64,
    /// Its state was loaded: its node's next calls send every parameter code owns again, as the
    /// blob's values would win over them otherwise (HOME-4). Counts the calls still to do so.
    reloaded: AtomicU8,
}

/// The calls after a state load that send every code-owned parameter again. Two: a plugin may
/// apply a loaded state at its next `process` call, over the values that call brings (Surge XT
/// does), so only the call after it is sure to keep them.
const RELOADED_CALLS: u8 = 2;

impl PluginInstance {
    /// Take ownership of `h`, created through `host` for a node of `shape`.
    pub(crate) fn new(host: Arc<dyn PluginHost>, h: Handle, shape: u64) -> PluginInstance {
        PluginInstance { host, h, shape, reloaded: AtomicU8::new(0) }
    }

    /// A new instance of `plugin` for a node of shape `shape` (its hash): created through `host`
    /// with `blob` loaded, reporting to `owner`, and activated at `rate`, all on the plugin main
    /// thread (RENDER-25).
    pub(crate) fn create(
        host: Arc<dyn PluginHost>,
        plugin: &Found,
        blob: Option<Arc<Blob>>,
        owner: &Owner,
        rate: u32,
        shape: u64,
    ) -> Result<Arc<PluginInstance>> {
        let (h, _) = host.create(plugin, blob, owner)?;
        // Owned from here: dropped on an error, it is destroyed.
        let inst = Arc::new(PluginInstance::new(host.clone(), h, shape));
        host.activate(h, rate, BLOCK as u32)?;
        Ok(inst)
    }

    /// A new instance to play in its place: the same plugin and shape, made from `blob` and
    /// activated at `rate`. An export makes one when it can't borrow this one (RENDER-1).
    pub fn another(&self, plugin: &Found, blob: Option<Arc<Blob>>, rate: u32) -> Result<Arc<PluginInstance>> {
        PluginInstance::create(self.host.clone(), plugin, blob, &Owner::discard(), rate, self.shape)
    }

    /// Its state was loaded outside its node (a state change, an export): the node's next calls
    /// send every parameter code owns again (HOME-4).
    pub fn mark_reloaded(&self) {
        self.reloaded.store(RELOADED_CALLS, Ordering::Release);
    }

    /// Whether this call must send every code-owned parameter again, after a load (the audio
    /// thread's; an atomic step, no lock).
    pub(crate) fn take_reloaded(&self) -> bool {
        self.reloaded.try_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_sub(1)).is_ok()
    }

    pub fn handle(&self) -> Handle {
        self.h
    }

    /// The host it lives in: what an export that borrows it calls (RENDER-1).
    pub fn host(&self) -> &dyn PluginHost {
        &*self.host
    }
}

impl Drop for PluginInstance {
    fn drop(&mut self) {
        // Queued to the main thread (HOST-2); never on the audio thread, which retires plans.
        self.host.destroy(self.h);
    }
}

impl std::fmt::Debug for PluginInstance {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("PluginInstance").field("h", &self.h).finish()
    }
}
