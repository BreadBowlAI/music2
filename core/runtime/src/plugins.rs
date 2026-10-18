//! Plugin instances (HOST-5, RENDER-25): one is created, its verified state loaded and activated
//! on the plugin main thread while the plan before plays, then held by a plugin node. An activation
//! moves it into the next plan like every other box, so its state, its tail and the notes sounding
//! into it go on; a new committed state is loaded into it as that plan is prepared (RENDER-26b).
//! An export in the app borrows it (RENDER-1). The last reference to an instance destroys it on
//! the plugin main thread (queued, never on the audio thread: plans are retired off it).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, AtomicUsize, Ordering};

use anyhow::Result;
use m2_plugins::{Blob, Found, Handle, Owner, PluginHost, RenderMode};

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
    /// An export took it and ended the notes playback left in it: at the resume, its live node
    /// starts again the ones the song still holds (RENDER-1).
    lent: AtomicBool,
    /// The code stopped setting one of its parameters, which keeps the value it was last sent
    /// (HOME-5a): it holds a state no file records until a state load lands, so an export doesn't
    /// borrow it, and plays that node from the committed state, as `m2 render` does (RENDER-1).
    released: AtomicBool,
    /// It failed live (a processing error, or it didn't come back active after an export): its
    /// node plays silence and never calls it again, and the session replaces it from its
    /// committed state (HOST-5).
    dead: AtomicBool,
    /// Parameters its window took over from code (HOME-5, HOME-5a, MON-6).
    overrides: Overrides,
    /// Code-owned parameters its node set back, as the plugin changed them outside a gesture
    /// (HOME-5a): what `plugin.param-reverted` lists.
    reverted: Ring,
    /// It changed a parameter code doesn't set (in its window): its state changed (HOME-6).
    changed: AtomicBool,
    /// It holds a state its file doesn't record: changed in its window, not yet autosaved. An
    /// export doesn't borrow it (RENDER-1).
    unsaved: AtomicBool,
    /// Its latency as last measured ([`Self::latency`]).
    latency: AtomicU32,
}

/// How many parameters of one instance its window may hold or latch at once; a gesture on one
/// more is set back, as one outside a gesture is.
pub const OVERRIDES: usize = 32;
/// A gesture on it is under way in the plugin's window.
pub const GESTURE: u8 = 1;
/// A code literal held by a gesture: its code value isn't sent until the session lets it go,
/// once the gesture's Tx is committed or refused (HOME-5, HOME-5a).
pub const HELD: u8 = 2;
/// A parameter a lane or a cable drives, latched by a gesture: the driven value isn't sent until
/// "Re-enable" (MON-6). For the session only, and never in an export.
pub const LATCHED: u8 = 4;
const EMPTY: u64 = u64::MAX;

/// The parameters a window took over, in a fixed table of words: `id << 8 | flags`, and the
/// value the plugin set. The audio thread takes a slot and sets flags; the session clears them.
/// No lock, no allocation (RENDER-28).
pub struct Overrides {
    slots: Box<[(AtomicU64, AtomicU64)]>,
}

impl Overrides {
    fn new() -> Overrides {
        Overrides { slots: (0..OVERRIDES).map(|_| (AtomicU64::new(EMPTY), AtomicU64::new(0))).collect() }
    }

    fn slot(&self, id: u32) -> Option<&(AtomicU64, AtomicU64)> {
        self.slots.iter().find(|s| {
            let w = s.0.load(Ordering::Acquire);
            w != EMPTY && (w >> 8) as u32 == id
        })
    }

    /// Its flags and the plugin's value, while any flag is set.
    pub fn get(&self, id: u32) -> Option<(u8, f64)> {
        let s = self.slot(id)?;
        Some((s.0.load(Ordering::Acquire) as u8, f64::from_bits(s.1.load(Ordering::Acquire))))
    }

    /// The audio thread: a gesture began on `id`, which it holds or latches (`kind`) from now,
    /// at `value` until the plugin says more. False when the table is full.
    pub(crate) fn begin(&self, id: u32, kind: u8, value: f64) -> bool {
        if let Some(s) = self.slot(id) {
            let w = s.0.load(Ordering::Acquire);
            if s.0.compare_exchange(w, w | (GESTURE | kind) as u64, Ordering::AcqRel, Ordering::Acquire).is_ok() {
                return true;
            }
        }
        // Only this thread takes a free slot, so one seen free stays free until taken.
        let word = (id as u64) << 8 | (GESTURE | kind) as u64;
        let Some(s) = self.slots.iter().find(|s| s.0.load(Ordering::Acquire) == EMPTY) else { return false };
        s.1.store(value.to_bits(), Ordering::Release);
        s.0.store(word, Ordering::Release);
        true
    }

    /// The audio thread: the plugin set `id` to `value`.
    pub(crate) fn value(&self, id: u32, value: f64) {
        if let Some(s) = self.slot(id) {
            s.1.store(value.to_bits(), Ordering::Release);
        }
    }

    /// The audio thread: the gesture on `id` ended; what it holds or latches stays, and a slot
    /// left with no flag (released mid-gesture) is free.
    pub(crate) fn end(&self, id: u32) {
        if let Some(s) = self.slot(id) {
            let left = |w: u64| if w & 0xff & !(GESTURE as u64) == 0 { EMPTY } else { w & !(GESTURE as u64) };
            let _ = s.0.try_update(Ordering::AcqRel, Ordering::Acquire, |w| (w != EMPTY && (w >> 8) as u32 == id).then(|| left(w)));
        }
    }

    /// The session: clear `flags` of `id`; a slot left with none is free.
    pub fn release(&self, id: u32, flags: u8) {
        if let Some(s) = self.slot(id) {
            let _ = s.0.try_update(Ordering::AcqRel, Ordering::Acquire, |w| {
                if w == EMPTY || (w >> 8) as u32 != id {
                    return None;
                }
                let left = w & !(flags as u64);
                Some(if left & 0xff == 0 { EMPTY } else { left })
            });
        }
    }

    /// Every parameter taken over: `(id, flags, value)`.
    pub fn list(&self) -> Vec<(u32, u8, f64)> {
        self.slots
            .iter()
            .filter_map(|s| {
                let w = s.0.load(Ordering::Acquire);
                (w != EMPTY).then(|| ((w >> 8) as u32, w as u8, f64::from_bits(s.1.load(Ordering::Acquire))))
            })
            .collect()
    }

    /// Whether any parameter is taken over.
    pub fn any(&self) -> bool {
        self.slots.iter().any(|s| s.0.load(Ordering::Acquire) != EMPTY)
    }
}

/// Parameter ids from the audio thread to the session, at most [`Ring::ROOM`] waiting; more are
/// dropped (the session hears them on the next set-back).
struct Ring {
    ids: Box<[AtomicU32]>,
    head: AtomicUsize,
    tail: AtomicUsize,
}

impl Ring {
    const ROOM: usize = 64;

    fn new() -> Ring {
        Ring { ids: (0..Ring::ROOM).map(|_| AtomicU32::new(0)).collect(), head: AtomicUsize::new(0), tail: AtomicUsize::new(0) }
    }

    /// The audio thread's: one producer.
    fn push(&self, id: u32) {
        let (h, t) = (self.head.load(Ordering::Relaxed), self.tail.load(Ordering::Acquire));
        if h - t < Ring::ROOM {
            self.ids[h % Ring::ROOM].store(id, Ordering::Relaxed);
            self.head.store(h + 1, Ordering::Release);
        }
    }

    /// The session's: one consumer.
    fn drain(&self) -> Vec<u32> {
        let (t, h) = (self.tail.load(Ordering::Relaxed), self.head.load(Ordering::Acquire));
        let out = (t..h).map(|i| self.ids[i % Ring::ROOM].load(Ordering::Relaxed)).collect();
        self.tail.store(h, Ordering::Release);
        out
    }
}

/// The calls after a state load that send every code-owned parameter again. Two: a plugin may
/// apply a loaded state at its next `process` call, over the values that call brings (Surge XT
/// does), so only the call after it is sure to keep them.
const RELOADED_CALLS: u8 = 2;

impl PluginInstance {
    /// Take ownership of `h`, created through `host` for a node of `shape`.
    pub(crate) fn new(host: Arc<dyn PluginHost>, h: Handle, shape: u64) -> PluginInstance {
        PluginInstance {
            host,
            h,
            shape,
            reloaded: AtomicU8::new(0),
            lent: AtomicBool::new(false),
            released: AtomicBool::new(false),
            dead: AtomicBool::new(false),
            overrides: Overrides::new(),
            reverted: Ring::new(),
            changed: AtomicBool::new(false),
            unsaved: AtomicBool::new(false),
            latency: AtomicU32::new(0),
        }
    }

    /// A new instance of `plugin` for a node of shape `shape` (its hash): created through `host`
    /// with `blob` loaded, reporting to `owner`, set to `mode` and activated at `rate`, all on the
    /// plugin main thread (RENDER-25). A plugin starts in realtime mode (CLAP's default); one for
    /// a render that isn't live is set to offline mode before it is activated (RENDER-1).
    pub(crate) fn create(
        host: Arc<dyn PluginHost>,
        plugin: &Found,
        blob: Option<Arc<Blob>>,
        owner: &Owner,
        rate: u32,
        shape: u64,
        mode: RenderMode,
    ) -> Result<Arc<PluginInstance>> {
        let (h, _) = host.create(plugin, blob, owner)?;
        // Owned from here: dropped on an error, it is destroyed.
        let inst = Arc::new(PluginInstance::new(host.clone(), h, shape));
        if mode == RenderMode::Offline {
            host.set_render_mode(h, mode)?;
        }
        inst.set_latency(host.activate(h, rate, BLOCK as u32)?);
        Ok(inst)
    }

    /// Its latency, in frames, as last measured: when it was activated, after a state load, or when
    /// it said it changed (RENDER-17). What the next plan compensates.
    pub fn latency(&self) -> u32 {
        self.latency.load(Ordering::Acquire)
    }

    /// Its latency was measured again (RENDER-17).
    pub fn set_latency(&self, frames: u32) {
        self.latency.store(frames, Ordering::Release);
    }

    /// A new instance to play in its place in an export: the same plugin and shape, made from
    /// `blob`, set to offline mode and activated at `rate`. An export makes one when it can't
    /// borrow this one (RENDER-1).
    pub fn another(&self, plugin: &Found, blob: Option<Arc<Blob>>, rate: u32) -> Result<Arc<PluginInstance>> {
        PluginInstance::create(self.host.clone(), plugin, blob, &Owner::discard(), rate, self.shape, RenderMode::Offline)
    }

    /// The code stopped setting one of its parameters: until a state load lands it holds a value
    /// no file records (HOME-5a), and an export makes that node anew (RENDER-1).
    pub(crate) fn release(&self) {
        self.released.store(true, Ordering::Release);
    }

    /// A state load landed: it holds that committed blob, with its code-owned parameters sent again.
    pub(crate) fn loaded(&self) {
        self.released.store(false, Ordering::Release);
    }

    /// Whether it holds a parameter value the code released and no file records ([`Self::release`]).
    pub fn released(&self) -> bool {
        self.released.load(Ordering::Acquire)
    }

    /// It failed: it plays silence and is never called again (an atomic store, safe on the audio
    /// thread). The session replaces it (HOST-5).
    pub fn kill(&self) {
        self.dead.store(true, Ordering::Release);
    }

    /// Whether it failed ([`Self::kill`]).
    pub fn dead(&self) -> bool {
        self.dead.load(Ordering::Acquire)
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

    /// An export ended the notes playback left in it: when it took it offline, and when its render
    /// reset it at its first call. Marked when it took it too, so an export that fails before its
    /// first block (a file it can't write) still has those notes start again at the resume.
    pub fn mark_lent(&self) {
        self.lent.store(true, Ordering::Release);
    }

    /// Whether an export ended its notes since this was last asked (the audio thread's, at a
    /// resume; an atomic swap, no lock).
    pub(crate) fn take_lent(&self) -> bool {
        self.lent.swap(false, Ordering::AcqRel)
    }

    pub fn handle(&self) -> Handle {
        self.h
    }

    /// The parameters its window took over (HOME-5a, MON-6).
    pub fn overrides(&self) -> &Overrides {
        &self.overrides
    }

    /// The audio thread: its node set `id` back to the code's value (HOME-5a).
    pub(crate) fn reverted(&self, id: u32) {
        self.reverted.push(id);
    }

    /// The session: the parameters set back since it last asked.
    pub fn take_reverted(&self) -> Vec<u32> {
        self.reverted.drain()
    }

    /// The audio thread: its window changed a parameter code doesn't set.
    pub(crate) fn state_changed(&self) {
        self.changed.store(true, Ordering::Release);
    }

    /// The session: whether its window changed its state since it last asked (HOME-6).
    pub fn take_changed(&self) -> bool {
        self.changed.swap(false, Ordering::AcqRel)
    }

    /// The session: whether it holds a state its file doesn't record yet.
    pub fn unsaved(&self) -> bool {
        self.unsaved.load(Ordering::Acquire)
    }

    pub fn set_unsaved(&self, unsaved: bool) {
        self.unsaved.store(unsaved, Ordering::Release);
    }

    /// Whether an export may borrow it: it holds its committed state and the values its code
    /// sets, and plays (RENDER-1).
    pub fn lendable(&self) -> bool {
        !self.released() && !self.dead() && !self.unsaved() && !self.overrides.any()
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

#[cfg(test)]
mod tests {
    use super::*;

    // MON-6 (review ph-r2): a hold released while its gesture goes on frees its slot when the
    // gesture ends, so the instance can be lent again (`lendable`).
    #[test]
    fn a_hold_released_mid_gesture_frees_its_slot_at_the_end() {
        let o = Overrides::new();
        assert!(o.begin(7, HELD, 0.5));
        o.release(7, HELD);
        o.end(7);
        assert!(!o.any(), "{:?}", o.list());
    }
}
