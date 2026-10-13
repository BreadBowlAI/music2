//! The control side of cell writes (RENDER-22, RENDER-28): a value goes straight into the slots of
//! the renderer it is for, never through the queue, and its seq says when the engine heard it.
//! Writers take one lock here, which only control threads ever take: the audio thread reads cells
//! from atomics.

use std::sync::{Mutex, MutexGuard};

use m2_runtime::{CellTarget, Renderer};

/// Cell writes count from here, so a cell write's seq never equals a message's and one
/// [`crate::Player::applied`] answers both. Below 2^53, so it is exact in JSON.
pub(crate) const CELL_SEQ: u64 = 1 << 48;

pub(crate) struct Writer {
    /// The last seq handed out.
    state: Mutex<u64>,
}

impl Default for Writer {
    fn default() -> Writer {
        Writer { state: Mutex::new(CELL_SEQ) }
    }
}

impl Writer {
    /// One writer at a time, so slots publish their seqs in order (see `CellSlots`). Held only
    /// for a few stores, never across a wait.
    fn state(&self) -> MutexGuard<'_, u64> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// `r` is about to be sent: every write so far counts as heard by it, since a gesture's latest
    /// value is set into every renderer before it is sent.
    pub(crate) fn sending(&self, r: &mut Renderer) {
        let seq = self.state();
        if *seq > CELL_SEQ {
            r.hear_from(*seq);
        }
    }

    /// Write `value` into `t`'s cells; returns the write's seq.
    pub(crate) fn set(&self, t: &CellTarget, value: f32) -> u64 {
        let mut seq = self.state();
        *seq += 1;
        t.slots.set(&t.cells, value, *seq);
        *seq
    }
}
