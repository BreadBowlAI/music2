//! Monitoring overlays (MON-1, RENDER-22a): one built-in gain node at each place a track's audio
//! enters another module (`Flat.overlays`), in every plan, offline renders included. At 1 it copies
//! what it reads, so it changes no bit; a new value glides there over 10 ms; at 0 it writes +0.0,
//! so a muted track's NaN never reaches the song.
//!
//! Each site's node has a cell: a monitoring mute, a solo and mute in song all write it. In an
//! activation's window, every audio edge that crosses tracks reads through an overlay too:
//! - a fade on a site's input (a changed edge, or a new node's fade-in) has the site's node on its
//!   new side;
//! - a fade's old side, and a ghost's input, that the old plan read through an overlay get a node of
//!   their own for the window, with no cell, at the new plan's value for the same track and consumer
//!   (or its mute in song). So an edit while a track is muted never lets it through for 5 ms.
//!
//! Each node runs right after its source, so a feedback consumer (which runs before its source)
//! still reads this block's. Nodes hold no box: nothing moves between plans.

use m2_ir::{Consumer, FOp, Flat, Input, PortRef};

use crate::BLOCK;
use crate::keep::{Edge, Wiring};

/// One overlay node, as placed: what it reads, its channels and its first value.
pub(crate) struct Node {
    pub(crate) src: PortRef,
    pub(crate) channels: u8,
    pub(crate) value: f32,
    /// The site of the new graph whose value it takes: its own, or for a window-only node the one
    /// with the same track and consumer, if any.
    pub(crate) follows: Option<usize>,
}

/// A plan's overlay nodes, at exec nodes `base..base + nodes.len()`: the new graph's sites first, in
/// their order (the celled ones), then the window's.
pub(crate) struct Placed {
    pub(crate) nodes: Vec<Node>,
    pub(crate) sites: usize,
}

/// How far an overlay moves in one frame: 0 to 1 in 10 ms (RENDER-22a).
pub(crate) fn step(rate: f64) -> f32 {
    1.0 / (0.010 * rate).round().max(1.0) as f32
}

/// The edge a consumer is, on its node (None for the song's output).
fn edge(c: Consumer) -> Option<(usize, Edge)> {
    match c {
        Consumer::Input { node, input } => Some((node, Edge::Input(input as usize))),
        Consumer::Capture { poly, index } => Some((poly, Edge::Capture(index as usize))),
        Consumer::Feedback { node } => Some((node, Edge::Feedback)),
        Consumer::Out => None,
    }
}

/// The consumer an edge of node `node` is.
fn consumer(node: usize, e: Edge) -> Consumer {
    match e {
        Edge::Input(p) => Consumer::Input { node, input: p as u16 },
        Edge::Capture(k) => Consumer::Capture { poly: node, index: k as u16 },
        Edge::Feedback => Consumer::Feedback { node },
        Edge::Out => Consumer::Out,
    }
}

/// `x` reading `src` instead, if it reads a port.
fn reading(x: &Input, src: PortRef) -> Input {
    match x {
        Input::Port { src_channels, channels, .. } => Input::Port { src, src_channels: *src_channels, channels: *channels },
        other => other.clone(),
    }
}

/// What a ghost's edge `e` reads.
fn ghost_reads(op: &FOp, e: Edge) -> Option<PortRef> {
    match (op, e) {
        (FOp::Kernel { inputs, .. } | FOp::Plugin { inputs, .. }, Edge::Input(p)) => match inputs.get(p)? {
            Input::Port { src, .. } => Some(*src),
            _ => None,
        },
        (FOp::Poly { voice, .. }, Edge::Capture(k)) => {
            voice.nodes.iter().filter_map(|n| if let FOp::Capture(p) = n.op { Some(p) } else { None }).nth(k)
        }
        (FOp::Feedback { input, .. }, Edge::Feedback) => Some(*input),
        _ => None,
    }
}

/// Place `flat`'s overlays into wiring `w`, its overlay nodes to be pushed at exec nodes from `base`
/// (after its fades, which start at `fades`; its ghosts start at `flat.nodes.len()`). `old` is the
/// graph that plays, for an activation. Every input that reads an overlay is added to `w.rewire`, or
/// set on a fade's side; each node goes right after its source in `w.order`.
pub(crate) fn place(flat: &Flat, old: Option<&Flat>, w: &mut Wiring, fades: usize, base: usize) -> Placed {
    let init = |module: &str| if flat.muted.iter().any(|m| m == module) { 0.0 } else { 1.0 };
    let mut nodes: Vec<Node> = vec![];
    // The new graph's sites: the consumer reads its node, or a fade on that input reads it new.
    for (k, s) in flat.overlays.iter().enumerate() {
        let ov = base + nodes.len();
        nodes.push(Node { src: s.src, channels: s.channels, value: init(&s.module), follows: Some(k) });
        let to = PortRef { node: ov, port: 0 };
        match edge(s.consumer) {
            None if w.out == flat.out => w.out = to,
            None => {
                let f = &mut w.fades[w.out.node - fades];
                f.new = reading(&f.new, to);
            }
            Some((c, e)) => match w.rewire.iter().find(|r| r.0 == c && r.1 == e) {
                Some(&(_, _, f)) => {
                    let f = &mut w.fades[f - fades];
                    f.new = reading(&f.new, to);
                }
                None => w.rewire.push((c, e, ov)),
            },
        }
    }
    let sites = nodes.len();
    // The window: what the old plan read through an overlay, the fades' old sides and the ghosts'
    // inputs, reads through one until it ends.
    if let Some(old) = old.filter(|_| !w.fades.is_empty() || !w.ghosts.is_empty()) {
        let was = |c: Consumer| old.overlays.iter().find(|s| s.consumer == c);
        let window = |nodes: &mut Vec<Node>, module: &str, into: &str, src: PortRef, channels: u8| {
            let follows = flat.overlays.iter().position(|t| t.module == module && t.into == into);
            let value = follows.map_or(init(module), |t| nodes[t].value);
            nodes.push(Node { src, channels, value, follows });
            PortRef { node: base + nodes.len() - 1, port: 0 }
        };
        // Fades of changed edges, on continuing nodes and on the output.
        let mut changed: Vec<(Consumer, usize)> =
            w.rewire.iter().filter(|r| (fades..fades + w.fades.len()).contains(&r.2)).filter_map(|&(i, e, f)| Some((consumer(w.same[i]?, e), f))).collect();
        if w.out != flat.out {
            changed.push((Consumer::Out, w.out.node));
        }
        for (c, f) in changed {
            let Some(s) = was(c) else { continue };
            let Input::Port { src, .. } = w.fades[f - fades].old else { continue };
            let to = window(&mut nodes, &s.module, &s.into, src, s.channels);
            let fade = &mut w.fades[f - fades];
            fade.old = reading(&fade.old, to);
        }
        // Ghosts' inputs.
        let n = flat.nodes.len();
        for g in 0..w.ghosts.len() {
            let j = w.ghosts[g].0;
            let at: Vec<(Edge, &m2_ir::OverlaySite)> =
                old.overlays.iter().filter_map(|s| edge(s.consumer).filter(|(c, _)| *c == j).map(|(_, e)| (e, s))).collect();
            for (e, s) in at {
                let Some(src) = ghost_reads(&w.ghosts[g].1.op, e) else { continue };
                let to = window(&mut nodes, &s.module, &s.into, src, s.channels);
                w.rewire.push((n + g, e, to.node));
            }
        }
    }
    // Each right after its source (A4); one whose source doesn't run first of all.
    let mut order = Vec::with_capacity(w.order.len() + nodes.len());
    let after = |x: usize| nodes.iter().enumerate().filter(move |(_, o)| o.src.node == x).map(|(j, _)| base + j);
    let ran: std::collections::HashSet<usize> = w.order.iter().copied().collect();
    order.extend(nodes.iter().enumerate().filter(|(_, o)| !ran.contains(&o.src.node)).map(|(j, _)| base + j));
    for &x in &w.order {
        order.push(x);
        order.extend(after(x));
    }
    w.order = order;
    Placed { nodes, sites }
}

/// An overlay's block: `out` is `src` times its gain, which moves toward `target` by `step` a frame
/// while it differs. At 1 it copies, so it changes no bit; at 0 it writes +0.0. Never allocates.
pub(crate) fn process(src: &[f32], out: &mut [f32], target: f32, cur: &mut f32, step: f32) {
    let channels = out.len() / BLOCK;
    let have = (src.len() / BLOCK).max(1);
    if *cur == target {
        if target == 1.0 {
            for c in 0..channels {
                let s = c.min(have - 1) * BLOCK;
                out[c * BLOCK..(c + 1) * BLOCK].copy_from_slice(&src[s..s + BLOCK]);
            }
        } else if target == 0.0 {
            out.fill(0.0);
        } else {
            for c in 0..channels {
                let s = c.min(have - 1) * BLOCK;
                for f in 0..BLOCK {
                    out[c * BLOCK + f] = src[s + f] * target;
                }
            }
        }
        return;
    }
    for f in 0..BLOCK {
        *cur = if *cur < target { (*cur + step).min(target) } else { (*cur - step).max(target) };
        for c in 0..channels {
            out[c * BLOCK + f] = src[c.min(have - 1) * BLOCK + f] * *cur;
        }
    }
}
