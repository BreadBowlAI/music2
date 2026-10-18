//! Sounds that stopped while the song played (RENDER-25). Live, a node that fails (a kernel's trap,
//! a voice's, a kernel that asks for more than its memory) goes silent alone: the rest of the plan
//! plays on. Its mark here is what the session reads to tell the user, once.
//!
//! The audio thread only stores into atomics here: nothing allocates, locks or waits (RENDER-28).
//! A plan's marks are its own: a new plan starts with none, and its node plays again.

use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};

/// Why a node stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// It failed: a kernel trapped.
    Crashed,
    /// A kernel asked for more memory than a sound may use.
    OutOfMemory,
}

impl Why {
    /// Why `e`, a node's error, stopped it.
    pub fn of(e: &anyhow::Error) -> Why {
        match e.downcast_ref::<OutOfMemory>() {
            Some(_) => Why::OutOfMemory,
            None => Why::Crashed,
        }
    }

    fn code(self) -> u8 {
        match self {
            Why::Crashed => 1,
            Why::OutOfMemory => 2,
        }
    }
}

/// A node's mark once the session has read it.
const TOLD: u8 = 0x80;

/// One plan's stopped nodes, by top-level node: what [`crate::PlanInfo::stopped`] shares with the
/// session. Ghosts, fades and overlays aren't marked: they leave with the window.
#[derive(Debug, Default)]
pub struct Stopped {
    /// By node: 0 playing, else a [`Why`]'s code, with [`TOLD`] once read.
    nodes: Box<[AtomicU8]>,
    /// Stops so far: what the session checks before it looks at every node.
    stops: AtomicU32,
    /// `stops` when the session last looked.
    seen: AtomicU32,
}

impl Stopped {
    /// No node stopped, of `nodes`.
    pub(crate) fn new(nodes: usize) -> Stopped {
        Stopped { nodes: (0..nodes).map(|_| AtomicU8::new(0)).collect(), ..Stopped::default() }
    }

    /// Node `node` stopped, for `why`: on the audio thread, as a node fails, or by a test as the
    /// audio thread would. A node past the plan's own (a ghost, a fade) is left unmarked.
    pub fn stop(&self, node: usize, why: Why) {
        if let Some(n) = self.nodes.get(node) {
            n.store(why.code(), Ordering::Relaxed);
            self.stops.fetch_add(1, Ordering::Release);
        }
    }

    /// Stops so far, told or not: a node that plays again and stops again counts twice.
    pub fn count(&self) -> u32 {
        self.stops.load(Ordering::Relaxed)
    }

    /// Node `node` plays again (a panic reset it): a later stop is told again.
    pub(crate) fn clear(&self, node: usize) {
        if let Some(n) = self.nodes.get(node) {
            n.store(0, Ordering::Relaxed);
        }
    }

    /// Why node `node` is stopped, if it is.
    pub fn why(&self, node: usize) -> Option<Why> {
        match self.nodes.get(node)?.load(Ordering::Relaxed) & !TOLD {
            1 => Some(Why::Crashed),
            2 => Some(Why::OutOfMemory),
            _ => None,
        }
    }

    /// The nodes stopped since the last call, each once, in node order: what the session tells.
    pub fn news(&self) -> Vec<(usize, Why)> {
        let stops = self.stops.load(Ordering::Acquire);
        if self.seen.swap(stops, Ordering::Relaxed) == stops {
            return vec![];
        }
        let mut news = vec![];
        for (i, n) in self.nodes.iter().enumerate() {
            let was = n.load(Ordering::Relaxed);
            // A stop that lands meanwhile moves `stops` again: the next call reads it.
            if was != 0 && was & TOLD == 0 && n.compare_exchange(was, was | TOLD, Ordering::Relaxed, Ordering::Relaxed).is_ok() {
                news.extend(self.why(i).map(|why| (i, why)));
            }
        }
        news
    }
}

/// A kernel asked for more memory than a sound may use: [`crate::KERNEL_MEMORY`], 256 MiB, while
/// its sandbox was born, or any at all after it was sealed (RENDER-32).
#[derive(Debug)]
pub struct OutOfMemory {
    /// The node, `<module>/<id>`.
    pub node: String,
}

impl std::fmt::Display for OutOfMemory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ran out of memory: a sound may use up to {} MB", self.node, crate::KERNEL_MEMORY >> 20)
    }
}

impl std::error::Error for OutOfMemory {}

/// A song needs more sandboxes than a renderer may hold, [`crate::SANDBOX_LIMIT`] (RENDER-32): told
/// at its build, before any is made, in words that say what to do.
#[derive(Debug)]
pub struct TooManySounds {
    pub needed: usize,
    pub limit: usize,
}

impl std::fmt::Display for TooManySounds {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "This song needs {} sound parts that each run on their own (each effect or sound outside an instrument, and each \
             instrument), and music2 can play {} at once. Remove some effects or tracks",
            thousands(self.needed),
            thousands(self.limit)
        )
    }
}

impl std::error::Error for TooManySounds {}

/// `n` with a comma between thousands: 4,000.
fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}
