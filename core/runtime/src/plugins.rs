//! Plugin instances (HOST-5, RENDER-25): one is created, its verified state loaded and activated
//! on the plugin main thread while the plan before plays, then held by a plugin node. An activation
//! moves it into the next plan like every other box, so its state, its tail and the notes sounding
//! into it go on; a new committed state is loaded into it as that plan is prepared (RENDER-26b).
//! An export in the app borrows it (RENDER-1). The last reference to an instance destroys it on
//! the plugin main thread (queued, never on the audio thread: plans are retired off it).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use m2_plugins::{Handle, PluginHost};

/// A live plugin instance, held by the plugin node that plays it. Dropping the last reference
/// queues its destruction on the plugin main thread.
pub struct PluginInstance {
    host: Arc<dyn PluginHost>,
    h: Handle,
    /// The hash of the shape it was made for: what an activation checks before moving it.
    pub(crate) shape: u64,
    /// Its state was loaded since its node's last call: that call sends every parameter code owns
    /// again, as the blob's values would win over them otherwise (HOME-4).
    reloaded: AtomicBool,
}

impl PluginInstance {
    /// Take ownership of `h`, created through `host` for a node of `shape`.
    pub(crate) fn new(host: Arc<dyn PluginHost>, h: Handle, shape: u64) -> PluginInstance {
        PluginInstance { host, h, shape, reloaded: AtomicBool::new(false) }
    }

    /// Its state was loaded outside its node (a state change, an export): the node's next call
    /// sends every parameter code owns again (HOME-4).
    pub fn mark_reloaded(&self) {
        self.reloaded.store(true, Ordering::Release);
    }

    /// Whether it was loaded since the last call that asked (the audio thread's).
    pub(crate) fn take_reloaded(&self) -> bool {
        self.reloaded.swap(false, Ordering::Acquire)
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
