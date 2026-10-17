//! A graph of numbered tasks, built once off the audio thread, and the state one run of it uses.
//!
//! Building sorts everything, so the same graph gives the same tasks whatever order its edges
//! came in: each task's consumers and the roots, most critical first (the highest bottom level,
//! then the lowest number). The run state is sized here, so a run allocates nothing.

use std::any::Any;
use std::cell::UnsafeCell;
use std::fmt;

use crate::sync::{AtomicBool, AtomicU8, AtomicU32, Ordering};

/// A queue slot that is reserved but not written yet.
pub(crate) const EMPTY: u32 = u32::MAX;

/// What a task did in the last run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// It returned `true`.
    Ran,
    /// It returned `false`, or panicked.
    Failed,
    /// Something it reads, directly or not, failed, so it wasn't called. Also every task's
    /// outcome before the first run.
    Skipped,
}

pub(crate) const RAN: u8 = 1;
pub(crate) const FAILED: u8 = 2;

/// Tasks that read each other in a circle: each task reads the one before it, and the first
/// reads the last. Such a graph has no order to run in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cycle(pub Vec<u32>);

impl fmt::Display for Cycle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<String> = self.0.iter().chain(self.0.first()).map(|t| t.to_string()).collect();
        write!(f, "tasks read each other in a circle: {}", names.join(" → "))
    }
}

impl std::error::Error for Cycle {}

/// Padding to a cache line pair, so two threads writing neighbours don't slow each other.
#[repr(align(128))]
pub(crate) struct Padded<T>(pub(crate) T);

impl<T> std::ops::Deref for Padded<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> std::ops::DerefMut for Padded<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

/// One task's part of the run state.
pub(crate) struct State {
    /// Inputs not yet complete.
    pub(crate) waiting: AtomicU32,
    /// Something it reads failed: complete it without calling it.
    pub(crate) skip: AtomicBool,
    /// 0 before it completes, then `RAN` or `FAILED`; skipped stays 0.
    pub(crate) outcome: AtomicU8,
    /// The thread that completed it: 0 is the caller of `run`, k is worker k.
    pub(crate) ran_on: AtomicU8,
}

/// Where a panicking task's payload waits for the end of the run.
pub(crate) struct Slot(UnsafeCell<Option<Box<dyn Any + Send>>>);

// SAFETY: during a run, slot t is written only by the one thread that runs task t (each task is
// popped exactly once). Between runs it is read and cleared only through `&mut Tasks`, after
// `run` has seen every task complete and every worker leave.
unsafe impl Sync for Slot {}

impl Slot {
    /// Keep a panic's payload.
    ///
    /// # Safety
    /// Only the thread running this slot's task may call it, and only during the run.
    pub(crate) unsafe fn put(&self, payload: Box<dyn Any + Send>) {
        // SAFETY: the caller is the slot's only writer, and nobody reads it during the run.
        unsafe { *self.0.get() = Some(payload) }
    }

    /// The payload kept, if any, leaving none.
    pub(crate) fn take(&mut self) -> Option<Box<dyn Any + Send>> {
        self.0.get_mut().take()
    }
}

/// A graph of numbered tasks: an edge `(a, b)` means `b` reads what `a` wrote, so `b` runs after
/// `a` completes. Its run state is sized here, so [`Runner::run`](crate::Runner::run) allocates
/// nothing.
pub struct Tasks {
    /// Distinct producers of each task.
    pub(crate) inputs: Box<[u32]>,
    /// The consumers of task t are `consumers[starts[t]..starts[t + 1]]`, most critical first.
    starts: Box<[u32]>,
    consumers: Box<[u32]>,
    /// Tasks with no inputs, most critical first.
    pub(crate) roots: Box<[u32]>,
    width: usize,
    pub(crate) state: Box<[State]>,
    /// One slot per task, as each is pushed once a run. `EMPTY` until written.
    pub(crate) queue: Box<[AtomicU32]>,
    pub(crate) head: Padded<AtomicU32>,
    pub(crate) tail: Padded<AtomicU32>,
    /// Tasks not yet complete.
    pub(crate) left: Padded<AtomicU32>,
    /// Tasks being run now.
    pub(crate) running: Padded<AtomicU32>,
    pub(crate) panics: Box<[Slot]>,
    /// For the models: after `run` returns, nothing may touch the run state (`touch`).
    #[cfg(feature = "models")]
    pub(crate) sealed: std::sync::atomic::AtomicBool,
}

impl Tasks {
    /// The graph of `costs.len()` tasks with these edges. A task's cost (any unit) sets its bottom
    /// level: its cost plus the largest bottom level among its consumers. Repeated edges count
    /// once. A cycle is refused in every build: a run could never finish it.
    ///
    /// # Panics
    /// If an edge names a task at or past `costs.len()`, or there are `u32::MAX` tasks or more.
    pub fn new(costs: &[u32], edges: &[(u32, u32)]) -> Result<Tasks, Cycle> {
        let n = costs.len();
        assert!(n < EMPTY as usize, "m2-pool: {n} tasks is too many");
        let mut edges = edges.to_vec();
        for &(a, b) in &edges {
            assert!((a as usize) < n && (b as usize) < n, "m2-pool: the edge ({a}, {b}) names a task past the last ({n} tasks)");
        }
        edges.sort_unstable();
        edges.dedup();
        let mut inputs = vec![0u32; n];
        for &(_, b) in &edges {
            inputs[b as usize] += 1;
        }
        // Consumers grouped by producer (edges are sorted by producer).
        let mut starts = vec![0u32; n + 1];
        for &(a, _) in &edges {
            starts[a as usize + 1] += 1;
        }
        for t in 0..n {
            starts[t + 1] += starts[t];
        }
        let mut consumers: Vec<u32> = edges.iter().map(|&(_, b)| b).collect();

        // A topological order (Kahn's), lowest number first among the ready.
        let mut waiting = inputs.clone();
        let mut ready: std::collections::BinaryHeap<std::cmp::Reverse<u32>> =
            (0..n as u32).filter(|&t| inputs[t as usize] == 0).map(std::cmp::Reverse).collect();
        let mut order = Vec::with_capacity(n);
        while let Some(std::cmp::Reverse(t)) = ready.pop() {
            order.push(t);
            for &c in &consumers[starts[t as usize] as usize..starts[t as usize + 1] as usize] {
                waiting[c as usize] -= 1;
                if waiting[c as usize] == 0 {
                    ready.push(std::cmp::Reverse(c));
                }
            }
        }
        if order.len() < n {
            return Err(cycle(&edges, &waiting));
        }

        // Bottom levels, from the last task in the order back; layers, from the first.
        let mut level = vec![0u64; n];
        for &t in order.iter().rev() {
            let cs = &consumers[starts[t as usize] as usize..starts[t as usize + 1] as usize];
            level[t as usize] = costs[t as usize] as u64 + cs.iter().map(|&c| level[c as usize]).max().unwrap_or(0);
        }
        let mut layer = vec![0usize; n];
        for &t in &order {
            for &c in &consumers[starts[t as usize] as usize..starts[t as usize + 1] as usize] {
                layer[c as usize] = layer[c as usize].max(layer[t as usize] + 1);
            }
        }
        let mut per_layer = vec![0usize; n];
        for &l in &layer {
            per_layer[l] += 1;
        }
        let width = per_layer.into_iter().max().unwrap_or(0);

        let critical = |a: &u32, b: &u32| level[*b as usize].cmp(&level[*a as usize]).then(a.cmp(b));
        for t in 0..n {
            consumers[starts[t] as usize..starts[t + 1] as usize].sort_unstable_by(critical);
        }
        let mut roots: Vec<u32> = (0..n as u32).filter(|&t| inputs[t as usize] == 0).collect();
        roots.sort_unstable_by(critical);

        Ok(Tasks {
            inputs: inputs.into(),
            starts: starts.into(),
            consumers: consumers.into(),
            roots: roots.into(),
            width,
            state: (0..n)
                .map(|_| State { waiting: AtomicU32::new(0), skip: AtomicBool::new(false), outcome: AtomicU8::new(0), ran_on: AtomicU8::new(0) })
                .collect(),
            queue: (0..n).map(|_| AtomicU32::new(EMPTY)).collect(),
            head: Padded(AtomicU32::new(0)),
            tail: Padded(AtomicU32::new(0)),
            left: Padded(AtomicU32::new(0)),
            running: Padded(AtomicU32::new(0)),
            panics: (0..n).map(|_| Slot(UnsafeCell::new(None))).collect(),
            #[cfg(feature = "models")]
            sealed: std::sync::atomic::AtomicBool::new(false),
        })
    }

    /// How many tasks.
    pub fn len(&self) -> usize {
        self.inputs.len()
    }

    /// Whether there are no tasks.
    pub fn is_empty(&self) -> bool {
        self.inputs.is_empty()
    }

    /// The widest layer, when tasks are layered by their longest path from a root: at most
    /// `width − 1` workers are woken for a run. 0 with no tasks.
    pub fn width(&self) -> usize {
        self.width
    }

    /// The tasks that read task `t`, most critical first.
    pub fn consumers(&self, t: u32) -> &[u32] {
        &self.consumers[self.starts[t as usize] as usize..self.starts[t as usize + 1] as usize]
    }

    /// The tasks with no inputs, most critical first.
    pub fn roots(&self) -> &[u32] {
        &self.roots
    }

    /// What task `t` did in the last run.
    pub fn outcome(&self, t: u32) -> Outcome {
        match self.state[t as usize].outcome.load(Ordering::Relaxed) {
            RAN => Outcome::Ran,
            FAILED => Outcome::Failed,
            _ => Outcome::Skipped,
        }
    }

    /// For the models: from now on (or no longer) any touch of this run state by the pool is a
    /// breach, counted in [`models::breaches`](crate::models::breaches).
    #[cfg(feature = "models")]
    #[doc(hidden)]
    pub fn seal(&self, on: bool) {
        self.sealed.store(on, std::sync::atomic::Ordering::SeqCst);
    }
}

/// One cycle among the tasks a topological sort couldn't reach: each of them still waits on
/// another of them, so walking back along those inputs must come round.
fn cycle(edges: &[(u32, u32)], waiting: &[u32]) -> Cycle {
    let stuck = |t: u32| waiting[t as usize] > 0;
    // For each stuck task, one stuck producer.
    let mut from = vec![EMPTY; waiting.len()];
    for &(a, b) in edges {
        if stuck(a) && stuck(b) && from[b as usize] == EMPTY {
            from[b as usize] = a;
        }
    }
    let start = (0..waiting.len() as u32).find(|&t| stuck(t)).expect("a stuck task");
    let mut seen = vec![false; waiting.len()];
    let mut t = start;
    while !seen[t as usize] {
        seen[t as usize] = true;
        t = from[t as usize];
    }
    // t is on the cycle: walk it once more, then put it in reading order.
    let mut path = vec![t];
    let mut u = from[t as usize];
    while u != t {
        path.push(u);
        u = from[u as usize];
    }
    path.reverse();
    let low = path.iter().enumerate().min_by_key(|(_, t)| **t).map(|(i, _)| i).unwrap_or(0);
    path.rotate_left(low);
    Cycle(path)
}
