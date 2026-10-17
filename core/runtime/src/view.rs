//! How a partition's task reaches the graph while other tasks run beside it (8a, RENDER-2).
//!
//! A task writes only its own nodes (their exec, buffers, version and `poisoned` mark) and reads
//! the buffers and versions of nodes that finished before it: earlier in its partition, or in a
//! partition it waits for. [`GraphView`] holds raw pointers to the graph's per-node vectors, taken
//! once from `&mut Graph`, and only ever makes a reference to one element: so one thread's `&mut`
//! to node i beside another's `&` to node j never alias. The block path: nothing here allocates,
//! locks or waits.

use std::marker::PhantomData;

use crate::parts::Parts;
use crate::{Buf, Exec, Graph};

/// What a node reads during a block: other nodes' outputs and versions.
pub(crate) trait Reads {
    /// Port `port` of node `node`'s output, channel-major.
    fn buf(&self, node: usize, port: usize) -> &[f32];
    /// Node `node`'s version ([`crate::COPY`] unless it is a constant).
    fn version(&self, node: usize) -> u64;
}

/// A voice graph's reads, inside its own graph: one thread runs it all.
pub(crate) struct Local<'a> {
    pub(crate) bufs: &'a [Vec<Buf>],
    pub(crate) versions: &'a [u64],
}

impl Reads for Local<'_> {
    #[inline]
    fn buf(&self, node: usize, port: usize) -> &[f32] {
        &self.bufs[node][port]
    }

    #[inline]
    fn version(&self, node: usize) -> u64 {
        self.versions[node]
    }
}

/// What one step of node `i` writes: its exec, its outputs and its version.
pub(crate) struct Own<'a> {
    pub(crate) exec: &'a mut Exec,
    pub(crate) out: &'a mut Vec<Buf>,
    pub(crate) version: &'a mut u64,
    pub(crate) poisoned: &'a mut bool,
}

/// A top-level graph seen by every task of a block at once.
pub(crate) struct GraphView<'g> {
    exec: *mut Exec,
    bufs: *mut Vec<Buf>,
    versions: *mut u64,
    poisoned: *mut bool,
    len: usize,
    _graph: PhantomData<&'g mut Graph>,
}

// SAFETY: tasks on other threads use it only as the schedule allows: each node is written by its
// partition's task alone, and read only after that task completed (the pool's edges give the
// happens-before) or by later nodes of the same task. Its nodes may be touched from another thread
// because `Exec: Send` (checked below).
unsafe impl Sync for GraphView<'_> {}

/// The view gives each node's exec to whichever thread runs its partition.
const _: fn() = || {
    fn sent<T: Send>() {}
    sent::<Exec>();
    sent::<Buf>();
};

impl<'g> GraphView<'g> {
    /// The view of `g` for one block's run. `g` stays borrowed until it is dropped.
    pub(crate) fn new(g: &'g mut Graph) -> GraphView<'g> {
        let len = g.exec.len();
        debug_assert!(g.bufs.len() == len && g.versions.len() == len && g.poisoned.len() == len);
        GraphView {
            exec: g.exec.as_mut_ptr(),
            bufs: g.bufs.as_mut_ptr(),
            versions: g.versions.as_mut_ptr(),
            poisoned: g.poisoned.as_mut_ptr(),
            len,
            _graph: PhantomData,
        }
    }

    /// Node `i`'s parts, to step it.
    ///
    /// # Safety
    /// Node `i` is the calling task's: no other thread touches it until the task ends, and the
    /// task makes no other `Own` of it meanwhile.
    #[inline]
    pub(crate) unsafe fn own(&self, i: usize) -> Own<'_> {
        assert!(i < self.len, "node {i} is past the last ({})", self.len);
        // SAFETY: in bounds; the caller holds node i alone.
        unsafe {
            Own {
                exec: &mut *self.exec.add(i),
                out: &mut *self.bufs.add(i),
                version: &mut *self.versions.add(i),
                poisoned: &mut *self.poisoned.add(i),
            }
        }
    }

    /// What node `i` of partition `p` reads through: debug builds check every read against the
    /// schedule ([`Parts::may_read`]).
    #[inline]
    pub(crate) fn reader<'a>(&'a self, parts: &'a Parts, p: u32, i: usize) -> Reader<'a> {
        #[cfg(not(debug_assertions))]
        let _ = (parts, p, i);
        Reader {
            view: self,
            #[cfg(debug_assertions)]
            at: (parts, p, i),
        }
    }
}

/// One node's reads during a block's run.
pub(crate) struct Reader<'a> {
    view: &'a GraphView<'a>,
    /// The schedule, the node's partition and the node: what the debug check reads by.
    #[cfg(debug_assertions)]
    at: (&'a Parts, u32, usize),
}

impl Reader<'_> {
    #[inline]
    fn check(&self, j: usize) {
        assert!(j < self.view.len, "a read of node {j}, past the last ({})", self.view.len);
        #[cfg(debug_assertions)]
        {
            let (parts, p, i) = self.at;
            assert!(j != i, "node {i} reads itself");
            assert!(parts.may_read(p, i, j), "node {i} (partition {p}) reads node {j}, which the schedule doesn't put before it");
        }
    }
}

impl Reads for Reader<'_> {
    #[inline]
    fn buf(&self, node: usize, port: usize) -> &[f32] {
        self.check(node);
        // SAFETY: in bounds; node `node` finished before this read (the schedule), and only its own
        // task ever writes it: this `&` to one element aliases no `&mut`.
        let outputs: &Vec<Buf> = unsafe { &*self.view.bufs.add(node) };
        &outputs[port]
    }

    #[inline]
    fn version(&self, node: usize) -> u64 {
        self.check(node);
        // SAFETY: as for `buf`.
        unsafe { *self.view.versions.add(node) }
    }
}
