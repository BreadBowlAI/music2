//! What the gates measure of a renderer (8a: `m2-gates realtime --offline`), never what plays: a
//! block's passes timed, its graph run as one partition or as step 7's loop, and each partition
//! timed alone. Nothing here is on the block path.

use std::time::Instant;

use anyhow::Result;

use crate::parts::Schedule;
use crate::view::GraphView;
use crate::{Renderer, Runner, Seg, fpenv, run_graph, run_part};

/// How a gauged block runs its graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum How {
    /// The plan's partitions on the runner's threads: what plays.
    Parts,
    /// The whole order as one partition, on this thread: the executor's fixed costs alone.
    One,
    /// Step 7's loop over the order (`Graph::process`): no executor at all.
    Old,
}

/// A block's passes, in ns: before the graph (the transport, cells, lanes, the block's times), the
/// graph, and after it (feedback rings, the output, the meters).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Phases {
    pub pre: u64,
    pub graph: u64,
    pub post: u64,
}

/// One partition, as the gates see it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part {
    /// Its module, and the module crossings before it where modules read each other.
    pub label: String,
    pub depth: u16,
    /// Its nodes, in plan order: exec indices (a plan node's is its index in the flat graph).
    pub nodes: Vec<usize>,
    /// The partitions it waits for.
    pub reads: Vec<usize>,
}

impl Renderer {
    /// The next block at engine clock `clock`, as [`Renderer::render_block_on`] renders it, its
    /// graph run `how`, each pass timed.
    #[doc(hidden)]
    pub fn gauge_block(&mut self, clock: u64, runner: &mut Runner, how: How) -> Result<Phases> {
        let _mode = fpenv::Canon::enter();
        let alone = &mut Runner::one();
        let runner = if self.plan.plugins || how != How::Parts { alone } else { runner };
        let t0 = Instant::now();
        let Some(b) = self.pre(clock, runner) else { return Ok(Phases::default()) };
        let t1 = Instant::now();
        let failed = match how {
            How::Parts => run_graph(&mut self.plan.graph, &mut self.plan.schedule, &self.lanes.slots, &b, runner),
            How::One => {
                if self.whole.is_none() {
                    self.whole = Some(Box::new(Schedule::cut(&self.plan.graph.order, &self.plan.graph.exec, &[])?));
                }
                let whole = self.whole.as_deref_mut().expect("made above");
                run_graph(&mut self.plan.graph, whole, &self.lanes.slots, &b, runner)
            }
            How::Old => self.plan.graph.process(&b, Seg::WHOLE, None, None, &self.lanes.slots).err(),
        };
        let t2 = Instant::now();
        self.post(&b, failed)?;
        let ns = |a: Instant, b: Instant| (b - a).as_nanos() as u64;
        Ok(Phases { pre: ns(t0, t1), graph: ns(t1, t2), post: ns(t2, Instant::now()) })
    }

    /// The next block with each partition run alone, one after another on this thread, each
    /// timed: ns by partition. The same bits as any other block.
    #[doc(hidden)]
    pub fn profile_block(&mut self, clock: u64) -> Result<Vec<u64>> {
        let _mode = fpenv::Canon::enter();
        let order = topological(&self.parts());
        let Some(b) = self.pre(clock, &mut Runner::one()) else { return Ok(vec![0; order.len()]) };
        let mut ns = vec![0; order.len()];
        {
            let view = GraphView::new(&mut self.plan.graph);
            let parts = &self.plan.schedule.parts;
            for p in order {
                let t = Instant::now();
                let ok = run_part(&view, parts, p as u32, &b, &self.lanes.slots);
                ns[p] = t.elapsed().as_nanos() as u64;
                if !ok {
                    break;
                }
            }
        }
        let failed = self.plan.schedule.parts.take_error();
        self.post(&b, failed)?;
        Ok(ns)
    }

    /// The last block rendered.
    #[doc(hidden)]
    pub fn last_block(&self) -> &[[f32; 2]; crate::BLOCK] {
        &self.frames
    }

    /// The plan's partitions, by number.
    #[doc(hidden)]
    pub fn parts(&self) -> Vec<Part> {
        let p = &self.plan.schedule.parts;
        (0..p.len() as u32)
            .map(|q| {
                let (label, depth) = p.label(q);
                let reads = (0..p.len() as u32).filter(|&o| p.reads(q, o)).map(|o| o as usize).collect();
                Part { label: label.to_string(), depth, nodes: p.nodes(q).iter().map(|&i| i as usize).collect(), reads }
            })
            .collect()
    }

    /// The widest layer of its partitions: how many could run at once.
    #[doc(hidden)]
    pub fn width(&self) -> usize {
        self.plan.schedule.tasks.width()
    }
}

/// The partitions in an order where each comes after those it reads.
fn topological(parts: &[Part]) -> Vec<usize> {
    let mut done = vec![false; parts.len()];
    let mut order = Vec::with_capacity(parts.len());
    while order.len() < parts.len() {
        let before = order.len();
        for (q, p) in parts.iter().enumerate() {
            if !done[q] && p.reads.iter().all(|&o| done[o]) {
                done[q] = true;
                order.push(q);
            }
        }
        assert!(order.len() > before, "the partitions read each other in a circle");
    }
    order
}
