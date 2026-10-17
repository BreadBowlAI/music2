//! A plan's partitions and their schedule (8a, RENDER-2): what runs as one task per block, and
//! which tasks wait for which.
//!
//! - Each node of the order goes into one partition, by its label: its module (a track, or the
//!   song). Where modules read each other both ways (a strongly connected group of modules), their
//!   nodes are split further by depth: the most module crossings on a path that leads to the node
//!   inside the group. So no path leaves a partition and comes back, and the partitions make a graph
//!   with no cycle.
//! - A partition runs its nodes in plan order, on one thread. Partition q waits for p when a node of
//!   q reads a node of p ([`Exec::reads`]): an edge (p, q).
//! - Built off the audio thread, when the plan is: numbering is by label and depth, so the same plan
//!   gives the same schedule.

use std::cell::UnsafeCell;
use std::collections::BTreeMap;

use anyhow::{Result, anyhow};
use m2_pool::Tasks;

use crate::Exec;

/// Not a partition: a node outside the order.
pub(crate) const NONE: u32 = u32::MAX;

/// A plan's partitions and the task graph that runs them.
pub(crate) struct Schedule {
    pub(crate) tasks: Tasks,
    pub(crate) parts: Parts,
}

/// The partitions: which nodes each runs, and where each node is.
pub(crate) struct Parts {
    /// Exec indices grouped by partition, each group in plan order: partition p is
    /// `nodes[starts[p]..starts[p + 1]]`.
    nodes: Box<[u32]>,
    starts: Box<[u32]>,
    /// By exec index: its partition, or [`NONE`] outside the order.
    part_of: Box<[u32]>,
    /// By exec index: its place in plan order, or [`NONE`]: which error is reported.
    pos: Box<[u32]>,
    /// By partition: the partitions it reads, sorted: `producers[pstarts[p]..pstarts[p + 1]]`.
    producers: Box<[u32]>,
    pstarts: Box<[u32]>,
    /// By partition: its label and depth (tests, gates).
    #[cfg_attr(not(test), expect(dead_code, reason = "read by tests now, by the gates later in 8a"))]
    labels: Box<[(Box<str>, u16)]>,
    /// By partition: the first error its task met this block, with the node's place in the order.
    errors: Box<[ErrorSlot]>,
}

/// One partition's error for the block.
struct ErrorSlot(UnsafeCell<Option<(u32, anyhow::Error)>>);

// SAFETY: during a run, slot p is written only by partition p's task, which runs on one thread once
// a block. It is read and cleared only through `&mut Parts`, after the run.
unsafe impl Sync for ErrorSlot {}

/// A node's cost, for the order tasks are taken in (the most critical first), in about a
/// kernel's call: a plugin 40, a `poly` its voices' kernels, a fade, an overlay and a copy 1.
fn cost(e: &Exec) -> u32 {
    match e {
        Exec::Kernel(_) => 2,
        Exec::Plugin(_) => 40,
        Exec::Poly(p) => p.pool.as_ref().map_or(1, |pool| {
            let kernels = |v: &crate::VoiceSlot| v.graph.exec.iter().filter(|e| matches!(e, Exec::Kernel(_))).count() as u32;
            pool.voices.iter().map(|v| 2 * kernels(v)).sum::<u32>().max(1)
        }),
        Exec::Fade { .. } | Exec::Overlay { .. } | Exec::Audio { .. } | Exec::Missing(_) | Exec::Control { .. } | Exec::Time(_) => 1,
        _ => 0,
    }
}

impl Schedule {
    /// The schedule of `exec`'s nodes in `order`, each labelled by `label` (its module), split by
    /// depth where labels read each other both ways. Fails only on a cycle between partitions,
    /// which the depth split rules out: a bug.
    pub(crate) fn build(order: &[usize], exec: &[Exec], label: &[Box<str>]) -> Result<Schedule> {
        let n = exec.len();
        let pos = positions(order, n);
        // Labels by name, so numbering is the same each time.
        let mut names: BTreeMap<&str, u32> = order.iter().map(|&i| (&*label[i], 0)).collect();
        for (k, v) in names.values_mut().enumerate() {
            *v = k as u32;
        }
        let module: Vec<u32> = (0..n).map(|i| if pos[i] == NONE { NONE } else { names[&*label[i]] }).collect();
        let m = names.len();
        // Which labels read each other: the groups of labels on a cycle.
        let mut reads_of: Vec<Vec<u32>> = vec![vec![]; m];
        for &i in order {
            exec[i].reads(|j| {
                if j < n && pos[j] != NONE && module[j] != module[i] {
                    reads_of[module[j] as usize].push(module[i]);
                }
            });
        }
        let group = strongly_connected(&reads_of);
        // Depth: module crossings inside a group, in plan order.
        let mut depth = vec![0u16; n];
        for &i in order {
            let mut d = 0;
            exec[i].reads(|j| {
                if j < n && pos[j] != NONE && pos[j] < pos[i] && group[module[j] as usize] == group[module[i] as usize] {
                    d = d.max(depth[j] + u16::from(module[j] != module[i]));
                }
            });
            depth[i] = d;
        }
        let names: Vec<&str> = names.keys().copied().collect();
        let mut keys: BTreeMap<(u32, u16), u32> = order.iter().map(|&i| ((module[i], depth[i]), 0)).collect();
        for (k, v) in keys.values_mut().enumerate() {
            *v = k as u32;
        }
        let part_of: Vec<u32> = (0..n).map(|i| if pos[i] == NONE { NONE } else { keys[&(module[i], depth[i])] }).collect();
        let labels = keys.keys().map(|&(m, d)| (Box::from(names[m as usize]), d)).collect();
        Schedule::of(order, exec, part_of, pos, labels)
    }

    /// For tests: the order cut into runs at `cuts` (places in the order), each a partition.
    #[cfg(test)]
    pub(crate) fn from_cuts(order: &[usize], exec: &[Exec], cuts: &[usize]) -> Result<Schedule> {
        let pos = positions(order, exec.len());
        let mut part_of = vec![NONE; exec.len()];
        let mut p = 0;
        for (k, &i) in order.iter().enumerate() {
            if k > 0 && cuts.contains(&k) {
                p += 1;
            }
            part_of[i] = p;
        }
        let parts = if order.is_empty() { 0 } else { p as usize + 1 };
        let labels = (0..parts).map(|q| (Box::from(format!("run{q}")), 0)).collect();
        Schedule::of(order, exec, part_of, pos, labels)
    }

    /// The schedule of these partitions.
    fn of(order: &[usize], exec: &[Exec], part_of: Vec<u32>, pos: Vec<u32>, labels: Vec<(Box<str>, u16)>) -> Result<Schedule> {
        let n = exec.len();
        let parts = labels.len();
        let mut groups: Vec<Vec<u32>> = vec![vec![]; parts];
        let mut costs = vec![0u32; parts];
        let mut edges = vec![];
        for &i in order {
            let p = part_of[i];
            groups[p as usize].push(i as u32);
            costs[p as usize] += cost(&exec[i]);
            exec[i].reads(|j| {
                debug_assert!(j < n, "node {i} reads node {j}, past the last ({n})");
                // A node outside the order is frozen this block: no edge.
                if j < n && part_of[j] != NONE && part_of[j] != p {
                    edges.push((part_of[j], p));
                }
            });
        }
        let tasks = Tasks::new(&costs, &edges).map_err(|c| {
            let names: Vec<String> = c.0.iter().map(|&p| format!("{} (depth {})", labels[p as usize].0, labels[p as usize].1)).collect();
            anyhow!("the plan's partitions read each other in a circle: {}", names.join(" → "))
        })?;
        edges.sort_unstable_by_key(|&(a, b)| (b, a));
        edges.dedup();
        let (mut producers, mut pstarts) = (Vec::with_capacity(edges.len()), vec![0u32]);
        for p in 0..parts as u32 {
            producers.extend(edges.iter().filter(|e| e.1 == p).map(|e| e.0));
            pstarts.push(producers.len() as u32);
        }
        let mut starts = vec![0u32];
        let mut nodes = Vec::with_capacity(order.len());
        for g in groups {
            nodes.extend(g);
            starts.push(nodes.len() as u32);
        }
        let parts = Parts {
            nodes: nodes.into(),
            starts: starts.into(),
            part_of: part_of.into(),
            pos: pos.into(),
            producers: producers.into(),
            pstarts: pstarts.into(),
            labels: labels.into(),
            errors: (0..parts).map(|_| ErrorSlot(UnsafeCell::new(None))).collect(),
        };
        #[cfg(debug_assertions)]
        parts.check(order, exec);
        Ok(Schedule { tasks, parts })
    }
}

/// Each node's place in `order`, or [`NONE`].
fn positions(order: &[usize], n: usize) -> Vec<u32> {
    let mut pos = vec![NONE; n];
    for (k, &i) in order.iter().enumerate() {
        pos[i] = k as u32;
    }
    pos
}

/// The strongly connected groups of a graph given by each vertex's successors (Tarjan's, without
/// recursion): by vertex, its group's number.
fn strongly_connected(succ: &[Vec<u32>]) -> Vec<u32> {
    let n = succ.len();
    let (mut index, mut low, mut on) = (vec![NONE; n], vec![0u32; n], vec![false; n]);
    let mut group = vec![NONE; n];
    let (mut next, mut groups) = (0u32, 0u32);
    let mut stack: Vec<u32> = vec![];
    for root in 0..n {
        if index[root] != NONE {
            continue;
        }
        // (vertex, next successor to look at)
        let mut walk: Vec<(u32, usize)> = vec![(root as u32, 0)];
        index[root] = next;
        low[root] = next;
        next += 1;
        stack.push(root as u32);
        on[root] = true;
        while let Some(&mut (v, ref mut k)) = walk.last_mut() {
            let v = v as usize;
            if let Some(&w) = succ[v].get(*k) {
                *k += 1;
                let w = w as usize;
                if index[w] == NONE {
                    index[w] = next;
                    low[w] = next;
                    next += 1;
                    stack.push(w as u32);
                    on[w] = true;
                    walk.push((w as u32, 0));
                } else if on[w] {
                    low[v] = low[v].min(index[w]);
                }
                continue;
            }
            walk.pop();
            if let Some(&(u, _)) = walk.last() {
                low[u as usize] = low[u as usize].min(low[v]);
            }
            if low[v] == index[v] {
                while let Some(w) = stack.pop() {
                    on[w as usize] = false;
                    group[w as usize] = groups;
                    if w as usize == v {
                        break;
                    }
                }
                groups += 1;
            }
        }
    }
    group
}

#[cfg_attr(not(test), expect(dead_code, reason = "read by tests now, by the gates later in 8a"))]
impl Parts {
    /// How many partitions.
    pub(crate) fn len(&self) -> usize {
        self.labels.len()
    }

    /// Partition p's nodes, in plan order.
    pub(crate) fn nodes(&self, p: u32) -> &[u32] {
        &self.nodes[self.starts[p as usize] as usize..self.starts[p as usize + 1] as usize]
    }

    /// Partition p's label and depth.
    pub(crate) fn label(&self, p: u32) -> (&str, u16) {
        let (l, d) = &self.labels[p as usize];
        (l, *d)
    }

    /// The partition of exec node `i`, or [`NONE`].
    pub(crate) fn part_of(&self, i: usize) -> u32 {
        self.part_of.get(i).copied().unwrap_or(NONE)
    }

    /// Whether partition `q` reads partition `p` directly.
    pub(crate) fn reads(&self, q: u32, p: u32) -> bool {
        self.producers[self.pstarts[q as usize] as usize..self.pstarts[q as usize + 1] as usize].binary_search(&p).is_ok()
    }

    /// Whether node `i`, running in partition `p`, may read node `j` now: `j` is outside the order
    /// (frozen this block), earlier in `p`, or in a partition `p` reads directly.
    pub(crate) fn may_read(&self, p: u32, i: usize, j: usize) -> bool {
        match self.part_of(j) {
            NONE => true,
            q if q == p => self.pos[j] < self.pos[i],
            q => self.reads(p, q),
        }
    }

    /// Keep the error node `i` met in partition `p`'s task.
    ///
    /// # Safety
    /// Only partition p's task may call it, during the run.
    pub(crate) unsafe fn fail(&self, p: u32, i: usize, e: anyhow::Error) {
        // SAFETY: the caller is partition p's task, the slot's only writer during the run.
        unsafe { *self.errors[p as usize].0.get() = Some((self.pos[i], e)) }
    }

    /// After a run: the error first in plan order, if any, and none kept (RENDER-2: the one the old
    /// loop met). Allocates nothing when there was none.
    pub(crate) fn take_error(&mut self) -> Option<anyhow::Error> {
        let mut first: Option<(u32, anyhow::Error)> = None;
        for slot in self.errors.iter_mut() {
            if let Some((at, e)) = slot.0.get_mut().take()
                && first.as_ref().is_none_or(|f| at < f.0)
            {
                first = Some((at, e));
            }
        }
        first.map(|f| f.1)
    }

    /// Debug builds: the order reads only earlier nodes, and every read is earlier in the node's own
    /// partition or in a partition with an edge to it.
    #[cfg(debug_assertions)]
    fn check(&self, order: &[usize], exec: &[Exec]) {
        for &i in order {
            exec[i].reads(|j| {
                assert!(self.pos[j] == NONE || self.pos[j] < self.pos[i], "node {i} reads node {j}, which runs after it");
                assert!(self.may_read(self.part_of[i], i, j), "node {i} reads node {j} across partitions with no edge");
            });
        }
    }
}
