//! Latency compensation (RENDER-17 to RENDER-20), off the audio thread.
//!
//! Each node has a latency: a plugin's as its instance last measured it, a kernel's as its package
//! declares it, every other node's 0. A node's offset is when its inputs arrive: the latest arrival
//! over them, where an input's arrival is its source's offset plus its source's latency. Every other
//! input of the node is delayed by the difference, so all of them arrive together (RENDER-18).
//! Lanes, notes and time are inputs like any other: a node after a latent one reads them late by
//! its offset, and a plugin there is told the transport its offset earlier (HOST-6). The song's
//! latency `L` is when its output arrives (RENDER-19).
//!
//! A feedback's input is read a block later, so that edge is cut: the feedback's output arrives at
//! 0. A loop through it can't be compensated: a node on the loop that adds latency, or a delay
//! inserted on it, makes the loop longer, and is listed for `latency.in-feedback` (RENDER-20).
//!
//! When every latency is 0 nothing is delayed and the graph plays as it was checked: the same plan,
//! the same bits. Otherwise [`insert`] gives the graph that plays: each delay a node of its own
//! ([`FOp::Delay`] for audio and control, [`FOp::NoteDelay`] for notes) after the checked graph's
//! nodes, so every node keeps its index, and its consumer reads it.

use m2_ir::{Consumer, FNode, FOp, Flat, Input, Kind, PortRef};

/// Which input of its consumer a delay is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum At {
    /// A kernel's or a plugin's input, by its place (a plugin's notes too).
    Input(usize),
    /// A `poly`'s capture, by its place in node order.
    Capture(usize),
    /// A `poly`'s notes.
    Notes,
}

/// One delay: on input `at` of node `consumer`, `frames` long. What a delay is kept by across
/// activations, as its consumer continues (RENDER-25).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Delay {
    pub consumer: usize,
    pub at: At,
    pub frames: u32,
}

/// A graph's latencies and the delays that align it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Latency {
    /// By node: when its inputs arrive, in frames after the song time they carry.
    pub offset: Vec<u32>,
    /// The delays, by consumer in node order, then by input.
    pub delays: Vec<Delay>,
    /// The song's latency `L`: when its output arrives.
    pub total: u32,
    /// Nodes on a feedback loop that make it longer: latent ones, and consumers of a delay on it.
    pub in_feedback: Vec<usize>,
}

/// The inputs of node `n` that arrive with a time, with where each is.
fn inputs(n: &FNode) -> Vec<(At, PortRef)> {
    let port = |i: &Input| if let Input::Port { src, .. } = i { Some(*src) } else { None };
    match &n.op {
        FOp::Kernel { inputs, .. } | FOp::Plugin { inputs, .. } => {
            inputs.iter().enumerate().filter_map(|(p, i)| Some((At::Input(p), port(i)?))).collect()
        }
        FOp::Poly { lane, voice, .. } => {
            let caps = voice.nodes.iter().filter_map(|v| if let FOp::Capture(p) = v.op { Some(p) } else { None });
            std::iter::once((At::Notes, PortRef { node: *lane, port: 0 }))
                .chain(caps.enumerate().map(|(k, p)| (At::Capture(k), p)))
                .collect()
        }
        FOp::Delay { input, .. } => vec![(At::Input(0), *input)],
        FOp::NoteDelay { lane, .. } => vec![(At::Notes, PortRef { node: *lane, port: 0 })],
        // A feedback's input is read a block later: cut (RENDER-20).
        FOp::Feedback { .. } | FOp::Lane { .. } | FOp::Time(_) | FOp::Const(_) | FOp::NoteField(_) | FOp::Capture(_) => vec![],
    }
}

/// Offsets and delays for `flat`, each node's own latency in `latency` (by node).
pub fn plan(flat: &Flat, latency: &[u32]) -> Latency {
    let n = flat.nodes.len();
    let lat = |i: usize| latency.get(i).copied().unwrap_or(0);
    let mut offset = vec![0u32; n];
    let mut delays = vec![];
    if flat.order.iter().all(|&i| lat(i) == 0) {
        return Latency { offset, ..Latency::default() };
    }
    let arrival = |offset: &[u32], src: usize| offset[src] + lat(src);
    for &i in &flat.order {
        let ins = inputs(&flat.nodes[i]);
        let at = ins.iter().map(|(_, p)| arrival(&offset, p.node)).max().unwrap_or(0);
        offset[i] = at;
    }
    // In node order, so the plan is the same for the same graph.
    let used: std::collections::HashSet<usize> = flat.order.iter().copied().collect();
    for i in (0..n).filter(|i| used.contains(i)) {
        for (where_, src) in inputs(&flat.nodes[i]) {
            // A constant reads the same whenever it is read.
            if matches!(flat.nodes[src.node].op, FOp::Const(_)) {
                continue;
            }
            let frames = offset[i] - arrival(&offset, src.node);
            if frames > 0 {
                delays.push(Delay { consumer: i, at: where_, frames });
            }
        }
    }
    let total = arrival(&offset, flat.out.node);
    let in_feedback = loops(flat, &lat, &delays);
    Latency { offset, delays, total, in_feedback }
}

/// The nodes on a feedback loop that make it longer (RENDER-20): for each feedback, the nodes both
/// after its output and before its input.
fn loops(flat: &Flat, lat: &impl Fn(usize) -> u32, delays: &[Delay]) -> Vec<usize> {
    let n = flat.nodes.len();
    let mut out = vec![];
    for &f in &flat.order {
        let FOp::Feedback { input, .. } = flat.nodes[f].op else { continue };
        let mut after = vec![false; n];
        after[f] = true;
        for &i in &flat.order {
            if inputs(&flat.nodes[i]).iter().any(|(_, p)| after[p.node]) {
                after[i] = true;
            }
        }
        let mut before = vec![false; n];
        let mut work = vec![input.node];
        while let Some(j) = work.pop() {
            if std::mem::replace(&mut before[j], true) {
                continue;
            }
            work.extend(inputs(&flat.nodes[j]).iter().map(|(_, p)| p.node));
        }
        let on = |i: usize| i != f && after[i] && before[i];
        out.extend((0..n).filter(|&i| on(i) && lat(i) > 0));
        // A delay lengthens the loop when what it delays is on it too.
        let src = |d: &Delay| inputs(&flat.nodes[d.consumer]).into_iter().find(|(a, _)| *a == d.at).map(|(_, p)| p.node);
        out.extend(delays.iter().filter(|d| on(d.consumer) && src(d).is_some_and(|s| s == f || on(s))).map(|d| d.consumer));
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// The graph that plays `flat` with `l`'s delays: each a node after `flat`'s, named by its consumer,
/// input and length, in its consumer's module, run right before it; its consumer reads it. An
/// overlay on a delayed input reads the delay (MON-1). `flat` itself when there are none.
pub fn insert(flat: &Flat, l: &Latency) -> Flat {
    let mut g = flat.clone();
    let mut before: Vec<Vec<usize>> = vec![vec![]; flat.nodes.len()];
    for d in &l.delays {
        let at = g.nodes.len();
        let c = &flat.nodes[d.consumer];
        let src = match (&c.op, d.at) {
            (FOp::Kernel { inputs, .. } | FOp::Plugin { inputs, .. }, At::Input(p)) => match inputs[p] {
                Input::Port { src, .. } => src,
                _ => unreachable!("a delay is on a port"),
            },
            (FOp::Poly { lane, .. }, At::Notes) => PortRef { node: *lane, port: 0 },
            (FOp::Poly { voice, .. }, At::Capture(k)) => {
                voice.nodes.iter().filter_map(|v| if let FOp::Capture(p) = v.op { Some(p) } else { None }).nth(k).expect("a capture")
            }
            _ => unreachable!("no delay on {:?}", d.at),
        };
        let kind = flat.nodes[src.node].outputs.get(src.port).copied().unwrap_or(Kind::Control(None));
        let op = match kind {
            Kind::Events(_) => FOp::NoteDelay { lane: src.node, frames: d.frames },
            _ => FOp::Delay { input: src, frames: d.frames },
        };
        let tag = match d.at {
            At::Input(p) => format!("in{p}"),
            At::Capture(k) => format!("capture{k}"),
            At::Notes => "notes".into(),
        };
        g.nodes.push(FNode {
            name: format!("{}~{tag}~{}", c.name, d.frames),
            module: c.module.clone(),
            src: None,
            origin: None,
            op,
            outputs: vec![kind],
        });
        let to = PortRef { node: at, port: 0 };
        match (&mut g.nodes[d.consumer].op, d.at) {
            (FOp::Kernel { inputs, .. } | FOp::Plugin { inputs, .. }, At::Input(p)) => {
                if let Input::Port { src, .. } = &mut inputs[p] {
                    *src = to;
                }
            }
            (FOp::Poly { lane, .. }, At::Notes) => *lane = at,
            (FOp::Poly { voice, .. }, At::Capture(k)) => {
                if let Some(FOp::Capture(p)) = voice.nodes.iter_mut().map(|v| &mut v.op).filter(|o| matches!(o, FOp::Capture(_))).nth(k) {
                    *p = to;
                }
            }
            _ => {}
        }
        for s in &mut g.overlays {
            let on = match (s.consumer, d.at) {
                (Consumer::Input { node, input }, At::Input(p)) => node == d.consumer && input as usize == p,
                (Consumer::Capture { poly, index }, At::Capture(k)) => poly == d.consumer && index as usize == k,
                _ => false,
            };
            if on {
                s.src = to;
            }
        }
        before[d.consumer].push(at);
    }
    g.order = flat.order.iter().flat_map(|&i| before[i].iter().copied().chain(std::iter::once(i))).collect();
    g
}
