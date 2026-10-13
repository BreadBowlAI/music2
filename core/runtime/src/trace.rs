//! A renderer's trace: what its schedulers turned on and off, its jumps and swaps, at their
//! engine and transport frames. Tests and gate runs read it; it is off unless asked for, and
//! recording never allocates: it is a ring of fixed size.

use crate::transport::Jump;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TraceKind {
    On {
        pitch: f32,
    },
    Off,
    Jump(Jump),
    /// A lane's data was swapped at this block line.
    Swap,
    /// Ons a full event buffer dropped, so far (RENDER-28's overflow policy).
    Dropped(u32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TraceEntry {
    pub engine: u64,
    pub transport: u64,
    /// The lane slot, or `u32::MAX` for the transport's own entries.
    pub slot: u32,
    /// The note id, for Ons and Offs.
    pub id: u32,
    pub kind: TraceKind,
}

pub struct Trace {
    buf: Vec<TraceEntry>,
    next: usize,
}

impl Trace {
    pub fn new(cap: usize) -> Trace {
        Trace { buf: Vec::with_capacity(cap.max(1)), next: 0 }
    }

    pub fn push(&mut self, e: TraceEntry) {
        if self.buf.len() < self.buf.capacity() {
            self.buf.push(e);
        } else {
            self.buf[self.next] = e;
            self.next = (self.next + 1) % self.buf.len();
        }
    }

    /// The entries kept, oldest first.
    pub fn entries(&self) -> Vec<TraceEntry> {
        let (a, b) = self.buf.split_at(self.next);
        b.iter().chain(a).copied().collect()
    }
}
