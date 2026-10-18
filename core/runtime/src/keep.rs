//! How a plan is wired against the plan that plays (RENDER-25, RENDER-25a), off the audio thread:
//! which nodes keep their instances, which inputs fade, which replaced nodes play out the window
//! as ghosts, and in what order the window's graph runs.
//!
//! - A stateful node is kept when the session says it continues an old node (`Continues`) and the
//!   two have the same [`Shape`], unless it is a plugin whose committed state blob is gone (CLAP
//!   has no "load defaults") or that asked to be restarted; a pure node (a constant, a lane, time,
//!   a missing plugin) continues when it is the same op. Nothing else keeps anything.
//! - An input of a kept node (or the song's output) whose source isn't the old input's source, or
//!   whose constant changed, is a changed edge: it reads a fade node, which crossfades from what it
//!   read to what it reads now over 5 ms, then copies the new side exactly. A kept node whose
//!   inputs are all unchanged reads what it read before, so an untouched track is bit-identical.
//! - A new node's audio input from a node that plays on fades in from silence over the window's
//!   first 5 ms, and the changed edges crossfade over the next 5: nothing new takes a step into
//!   its memory (a delay's first echo), and an exact wrap stays exact.
//! - The old sources of the changed edges that don't continue run on as ghosts for the window,
//!   with their instances and whatever they read that doesn't continue; then they are retired.
//! - An edit that reverses an edge between kept nodes (a reorder of two effects) would make the
//!   window's graph cyclic: the nodes on such a cycle are replaced, and it is wired again.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

use m2_ir::{FNode, FOp, Flat, Input, Kind, PortRef};
use m2_song::Sha;

use crate::plan::{Keep, LanePlan, Move, Shape};
use crate::sched::Caps;
use crate::{LaneRef, PackageSet};

/// Which input of a node an edge is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Edge {
    /// A kernel's or plugin's input, by its place (a missing plugin's audio input is its first).
    Input(usize),
    /// A `poly`'s capture, by its place in node order.
    Capture(usize),
    Feedback,
    /// The song's output.
    Out,
}

/// A fade node: what it fades from and to, its channels, and whether it waits one fade for the
/// fade-ins of new nodes' inputs before it crossfades.
pub(crate) struct FadeNode {
    pub(crate) old: Input,
    pub(crate) new: Input,
    pub(crate) channels: u8,
    pub(crate) waits: bool,
}

/// A plan's wiring against the plan that plays.
pub(crate) struct Wiring {
    /// For each top-level node, whether its box is moved in from the old plan.
    pub(crate) kept: Vec<bool>,
    /// Each continuing top-level node's old node.
    pub(crate) same: Vec<Option<usize>>,
    /// Ghosts, in exec order after the new nodes: the old node, wired to the new plan's nodes.
    pub(crate) ghosts: Vec<(usize, FNode)>,
    pub(crate) fades: Vec<FadeNode>,
    /// (node, which input, the fade node it reads now).
    pub(crate) rewire: Vec<(usize, Edge, usize)>,
    pub(crate) out: PortRef,
    pub(crate) order: Vec<usize>,
    pub(crate) moves: Vec<Move>,
    pub(crate) release: Vec<u32>,
    /// By exec node: the room the old plugin node had, which its notes move into.
    pub(crate) room: HashMap<usize, Caps>,
}

impl Wiring {
    /// Nothing kept, nothing fades: `flat` as it is.
    pub(crate) fn fresh(flat: &Flat) -> Wiring {
        let n = flat.nodes.len();
        Wiring {
            kept: vec![false; n],
            same: vec![None; n],
            ghosts: vec![],
            fades: vec![],
            rewire: vec![],
            out: flat.out,
            order: flat.order.clone(),
            moves: vec![],
            release: vec![],
            room: HashMap::new(),
        }
    }
}

/// The nodes `flat`'s processing order reaches.
pub(crate) fn used(flat: &Flat) -> Vec<bool> {
    let mut u = vec![false; flat.nodes.len()];
    for &i in &flat.order {
        u[i] = true;
    }
    u
}

/// A voice's captures, in node order.
fn captures(op: &FOp) -> Vec<PortRef> {
    match op {
        FOp::Poly { voice, .. } => voice.nodes.iter().filter_map(|n| if let FOp::Capture(p) = n.op { Some(p) } else { None }).collect(),
        _ => vec![],
    }
}

/// An output as an input reads it.
fn port(nodes: &[FNode], p: PortRef) -> Input {
    let ch = nodes.get(p.node).and_then(|n| n.outputs.get(p.port)).map_or(1, |k| k.channels());
    Input::Port { src: p, src_channels: ch, channels: ch }
}

/// The edges of node `n` that may fade, with what each reads: a kernel's inputs, a plugin's audio
/// input (parameters are events the plugin smooths itself, §6.3), a missing plugin's pass-through,
/// a feedback's input and a poly's captures.
fn edges(nodes: &[FNode], n: &FNode, packages: &PackageSet) -> Vec<(Edge, Input)> {
    match &n.op {
        FOp::Kernel { inputs, .. } => inputs.iter().cloned().enumerate().map(|(p, i)| (Edge::Input(p), i)).collect(),
        FOp::Plugin { plugin, inputs, .. } if packages.plugin_missing(plugin) => inputs
            .first()
            .map(|i| (Edge::Input(0), if matches!(i, Input::Port { .. }) { i.clone() } else { Input::Unset }))
            .into_iter()
            .collect(),
        FOp::Plugin { plugin, inputs, .. } => {
            let sig = m2_ir::Kernels::plugin(packages, plugin);
            let at = sig.and_then(|s| s.inputs.iter().position(|p| p.name == "in"));
            at.and_then(|p| Some((Edge::Input(p), inputs.get(p)?.clone()))).into_iter().collect()
        }
        FOp::Feedback { input, .. } => vec![(Edge::Feedback, port(nodes, *input))],
        FOp::Delay { input, .. } => vec![(Edge::Input(0), port(nodes, *input))],
        op => captures(op).into_iter().enumerate().map(|(k, p)| (Edge::Capture(k), port(nodes, p))).collect(),
    }
}

/// The notes lane a consumer reads, and how late (RENDER-18): through a note delay, if it is.
fn notes_lane<'a>(nodes: &'a [FNode], n: &FNode, packages: &PackageSet) -> Option<(&'a str, &'a str, u32)> {
    let src = match &n.op {
        FOp::Poly { lane, .. } => *lane,
        FOp::Plugin { plugin, inputs, .. } => {
            let at = m2_ir::Kernels::plugin(packages, plugin)?.inputs.iter().position(|p| p.name == "notes")?;
            match inputs.get(at)? {
                Input::Port { src, .. } => src.node,
                _ => return None,
            }
        }
        _ => return None,
    };
    let (src, frames) = match nodes.get(src)?.op {
        FOp::NoteDelay { lane, frames } => (lane, frames),
        _ => (src, 0),
    };
    match &nodes.get(src)?.op {
        FOp::Lane { module, lane, .. } => Some((module, lane, frames)),
        _ => None,
    }
}

/// The note delay a consumer reads its notes through, if it does.
fn note_delay(nodes: &[FNode], n: &FNode, packages: &PackageSet) -> Option<usize> {
    let src = match &n.op {
        FOp::Poly { lane, .. } => *lane,
        FOp::Plugin { plugin, inputs, .. } => {
            let at = m2_ir::Kernels::plugin(packages, plugin)?.inputs.iter().position(|p| p.name == "notes")?;
            match inputs.get(at)? {
                Input::Port { src, .. } => src.node,
                _ => return None,
            }
        }
        _ => return None,
    };
    matches!(nodes.get(src)?.op, FOp::NoteDelay { .. }).then_some(src)
}

/// Whether a pure node continues an old one: the same op, a cell whatever its value (cell carry
/// applies it, RENDER-22a), a plain constant only with the same value.
fn continues_pure(a: &FNode, b: &FNode, a_box: bool, b_box: bool) -> bool {
    match (&a.op, &b.op) {
        (FOp::Const(x), FOp::Const(y)) => (a.origin.is_some() && b.origin.is_some()) || x.to_bits() == y.to_bits(),
        (FOp::Time(x), FOp::Time(y)) => x == y,
        (FOp::Lane { module: m, lane: l, kind: k, .. }, FOp::Lane { module: n, lane: o, kind: j, .. }) => (m, l, k) == (n, o, j),
        // A missing plugin passes its input through.
        (FOp::Plugin { plugin: x, .. }, FOp::Plugin { plugin: y, .. }) => !a_box && !b_box && x == y,
        _ => false,
    }
}

/// Matching (§6.2): for each node of `new`, whether its box is kept, and the old node it continues:
/// kept boxes and continuing pure nodes, each old node once. Before any cycle a reorder makes.
pub(crate) fn matching(k: &Keep, new: &Flat, shapes: &[Option<Shape>], blobs: &[Option<Sha>]) -> (Vec<bool>, Vec<Option<usize>>) {
    let (old, info) = (k.old, k.info);
    let (n, used_new, used_old) = (new.nodes.len(), used(new), used(old));
    let restarted = |j: usize| info.handle(j).is_some_and(|h| k.restarts.contains(&h));
    let lost_blob = |i: usize, j: usize| blobs[i].is_none() && info.plugin(j).is_some_and(|p| p.blob.is_some());
    let (mut kept, mut same) = (vec![false; n], vec![None; n]);
    let mut claimed = vec![false; old.nodes.len()];
    for i in (0..n).filter(|&i| used_new[i]) {
        let Some(j) = k.cont.top.get(i).copied().flatten().map(|j| j as usize) else { continue };
        if j >= old.nodes.len() || !used_old[j] || claimed[j] {
            continue;
        }
        let was = info.shapes.get(j).and_then(Option::as_ref);
        let keep = match &shapes[i] {
            Some(s) => was == Some(s) && !restarted(j) && !lost_blob(i, j),
            None => continues_pure(&new.nodes[i], &old.nodes[j], false, was.is_some()),
        };
        if keep {
            (kept[i], same[i], claimed[j]) = (shapes[i].is_some(), Some(j), true);
        }
    }
    (kept, same)
}

/// Wire `new` against what `k` says plays. `shapes` are `new`'s nodes' shapes, and `blobs` the
/// committed state each of its plugins loads. Lanes only ghosts read are added to `lanes`.
pub(crate) fn wire(k: &Keep, new: &Flat, shapes: &[Option<Shape>], blobs: &[Option<Sha>], packages: &PackageSet, lanes: &mut LanePlan) -> Wiring {
    let (old, info) = (k.old, k.info);
    let n = new.nodes.len();
    let mut w = Wiring::fresh(new);
    (w.kept, w.same) = matching(k, new, shapes, blobs);
    // Edges, ghosts, fades and order, again for each cycle a reorder made (§6.3 to §6.6).
    loop {
        if let Some(cycle) = window(k, new, packages, &mut w) {
            // Each node on the cycle with inputs is replaced: a reorder restarts what it reordered.
            for i in cycle.into_iter().filter(|&i| i < n) {
                if w.same[i].is_some() && !matches!(new.nodes[i].op, FOp::Const(_) | FOp::Time(_) | FOp::Lane { .. }) {
                    (w.same[i], w.kept[i]) = (None, false);
                }
            }
            continue;
        }
        break;
    }
    // Ghost lanes: control and audio lanes only ghosts read (§6.1).
    for (_, g) in &mut w.ghosts {
        if let FOp::Lane { module, lane, kind, .. } = &g.op
            && !matches!(kind, Kind::Events(_))
        {
            lanes.ghost(&LaneRef { module: module.clone(), lane: lane.clone(), kind: *kind }, k);
        }
    }
    // Moves: kept boxes, then ghosts' (§6.2, §6.4).
    let hash = |j: usize| info.shapes[j].as_ref().map(Shape::hash64);
    for i in 0..n {
        if let (true, Some(j)) = (w.kept[i], w.same[i]) {
            w.moves.push(Move { from: j as u32, to: i as u32, shape: hash(j).expect("kept: a shape") });
        }
    }
    for (g, (j, node)) in w.ghosts.iter().enumerate() {
        if let Some(shape) = hash(*j) {
            w.moves.push(Move { from: *j as u32, to: (n + g) as u32, shape });
        }
        if matches!(node.op, FOp::Poly { .. } | FOp::Plugin { .. }) && hash(*j).is_some() {
            w.release.push((n + g) as u32);
        }
    }
    // A kept consumer whose notes now come from another lane lets go of what it holds (§3.5); so
    // does one whose note delay doesn't move into the new plan with the notes in flight in it.
    for i in (0..n).filter(|&i| w.kept[i]) {
        let j = w.same[i].expect("kept");
        let lost = note_delay(&old.nodes, &old.nodes[j], packages)
            .is_some_and(|d| note_delay(&new.nodes, &new.nodes[i], packages).is_none_or(|e| !w.kept[e] || w.same[e] != Some(d)));
        if lost || notes_lane(&new.nodes, &new.nodes[i], packages) != notes_lane(&old.nodes, &old.nodes[j], packages) {
            w.release.push(i as u32);
        }
    }
    // The room the old plugin nodes had, which their notes move into.
    let old_room = |j: usize| {
        let (m, l, _) = notes_lane(&old.nodes, &old.nodes[j], packages)?;
        info.lanes.iter().find(|x| x.lane.module == m && x.lane.lane == l).and_then(|x| x.caps)
    };
    for i in (0..n).filter(|&i| w.kept[i] && matches!(new.nodes[i].op, FOp::Plugin { .. })) {
        if let Some(c) = old_room(w.same[i].expect("kept")) {
            w.room.insert(i, c);
        }
    }
    for (g, (j, node)) in w.ghosts.iter().enumerate() {
        if let (FOp::Plugin { .. }, Some(c)) = (&node.op, old_room(*j)) {
            w.room.insert(n + g, c);
        }
    }
    w
}

/// Changed edges, ghosts, fades and the window's order for the matching in `w`. Returns the nodes
/// of a cycle when the window's graph has one, and leaves `w` to be wired again.
fn window(k: &Keep, new: &Flat, packages: &PackageSet, w: &mut Wiring) -> Option<Vec<usize>> {
    let old = k.old;
    let n = new.nodes.len();
    let mut new_of = vec![None; old.nodes.len()];
    for (i, j) in w.same.iter().enumerate() {
        if let Some(j) = j {
            new_of[*j] = Some(i);
        }
    }
    let unchanged = |a: &Input, b: &Input| match (a, b) {
        (Input::Port { src: x, src_channels: xc, channels: xh }, Input::Port { src: y, src_channels: yc, channels: yh }) => {
            w.same[x.node] == Some(y.node) && x.port == y.port && (xc, xh) == (yc, yh)
        }
        (Input::Const(x), Input::Const(y)) => x.to_bits() == y.to_bits(),
        (Input::Unset, Input::Unset) => true,
        _ => false,
    };
    // Changed edges into continuing nodes, then the output (§6.3): (node or n for the output,
    // edge, old input, new input).
    let mut changed: Vec<(usize, Edge, Input, Input)> = vec![];
    for i in 0..n {
        let Some(j) = w.same[i] else { continue };
        let (a, b) = (edges(&new.nodes, &new.nodes[i], packages), edges(&old.nodes, &old.nodes[j], packages));
        for ((edge, x), (_, y)) in a.into_iter().zip(b) {
            if !unchanged(&x, &y) {
                changed.push((i, edge, y, x));
            }
        }
    }
    let (out_new, out_old) = (port(&new.nodes, new.out), port(&old.nodes, old.out));
    if !unchanged(&out_new, &out_old) {
        changed.push((n, Edge::Out, out_old, out_new));
    }
    // A new node's audio inputs from nodes that play on fade in from silence (§6.5): a delay or a
    // reverb inserted on a sounding track would otherwise take a step into its memory, and play it
    // back as a click.
    let mut ins: Vec<(usize, Edge, Input)> = vec![];
    for &i in new.order.iter().filter(|&&i| w.same[i].is_none()) {
        for (edge, x) in edges(&new.nodes, &new.nodes[i], packages) {
            if let Input::Port { src, .. } = &x
                && w.same[src.node].is_some()
                && matches!(new.nodes[src.node].outputs.get(src.port), Some(Kind::Audio(_)))
            {
                ins.push((i, edge, x));
            }
        }
    }
    // Ghosts: the old sources of changed edges that don't continue, and what they read that
    // doesn't (§6.4). Events lanes aren't ghosts: a ghost reads no notes.
    let is_events = |j: usize| matches!(old.nodes[j].op, FOp::Lane { kind: Kind::Events(_), .. } | FOp::NoteDelay { .. });
    let mut ghost_of: HashMap<usize, usize> = HashMap::new();
    let mut order_of_ghosts = vec![];
    let mut work: Vec<usize> =
        changed.iter().filter_map(|c| if let Input::Port { src, .. } = &c.2 { Some(src.node) } else { None }).collect();
    while let Some(j) = work.pop() {
        if new_of[j].is_some() || ghost_of.contains_key(&j) || is_events(j) {
            continue;
        }
        ghost_of.insert(j, order_of_ghosts.len());
        order_of_ghosts.push(j);
        let o = &old.nodes[j].op;
        work.extend(o.deps());
        if let FOp::Feedback { input, .. } = o {
            work.push(input.node);
        }
    }
    let remap = |p: PortRef| -> Option<PortRef> {
        let node = new_of[p.node].or_else(|| ghost_of.get(&p.node).map(|g| n + g))?;
        Some(PortRef { node, port: p.port })
    };
    let remap_input = |i: &Input| match i {
        Input::Port { src, src_channels, channels } => match remap(*src) {
            Some(src) => Input::Port { src, src_channels: *src_channels, channels: *channels },
            None => Input::Unset,
        },
        other => other.clone(),
    };
    w.ghosts = order_of_ghosts
        .iter()
        .map(|&j| {
            let mut g = old.nodes[j].clone();
            match &mut g.op {
                FOp::Kernel { inputs, .. } | FOp::Plugin { inputs, .. } => {
                    for i in inputs.iter_mut() {
                        *i = remap_input(i);
                    }
                }
                FOp::Feedback { input, .. } => *input = remap(*input).unwrap_or(*input),
                FOp::Delay { input, .. } => *input = remap(*input).unwrap_or(*input),
                FOp::Poly { lane, voice, glide, .. } => {
                    // A ghost reads no notes: its lane is never looked at.
                    *lane = usize::MAX;
                    *glide = glide.and_then(remap);
                    for v in &mut voice.nodes {
                        if let FOp::Capture(p) = &mut v.op {
                            *p = remap(*p).unwrap_or(*p);
                        }
                    }
                }
                _ => {}
            }
            (j, g)
        })
        .collect();
    // Fades (§6.5): the changed edges crossfade once the fade-ins are done, so what they fade to
    // has heard its inputs whole.
    let first = n + w.ghosts.len();
    w.fades.clear();
    w.rewire.clear();
    w.out = new.out;
    let zero = |x: &Input| if matches!(x, Input::Unset) { Input::Const(0.0) } else { x.clone() };
    let channels = |x: &Input| if let Input::Port { src_channels, .. } = x { *src_channels } else { 1 };
    for (i, edge, was, now) in &changed {
        let f = first + w.fades.len();
        w.fades.push(FadeNode { old: zero(&remap_input(was)), new: zero(now), channels: channels(now), waits: !ins.is_empty() });
        if *edge == Edge::Out {
            w.out = PortRef { node: f, port: 0 };
        } else {
            w.rewire.push((*i, *edge, f));
        }
    }
    for (i, edge, now) in ins {
        w.rewire.push((i, edge, first + w.fades.len()));
        w.fades.push(FadeNode { old: Input::Const(0.0), channels: channels(&now), new: now, waits: false });
    }
    // Order (§6.6): the new graph's own when nothing fades; else Kahn's, lowest index first, over
    // the new nodes, ghosts and fades.
    if w.ghosts.is_empty() && w.fades.is_empty() {
        w.order = new.order.clone();
        return None;
    }
    let total = first + w.fades.len();
    let mut deps: Vec<Vec<usize>> = vec![vec![]; total];
    let mut live = vec![false; total];
    for &i in &new.order {
        live[i] = true;
        deps[i] = new.nodes[i].op.deps();
        // Captures and inputs that read a fade now; a feedback input never counts (§6.6).
        let caps = captures(&new.nodes[i].op);
        for &(c, edge, f) in w.rewire.iter().filter(|r| r.0 == i && r.1 != Edge::Feedback) {
            let src = match edge {
                Edge::Input(p) => match &new.nodes[c].op {
                    FOp::Kernel { inputs, .. } | FOp::Plugin { inputs, .. } => match inputs.get(p) {
                        Some(Input::Port { src, .. }) => Some(src.node),
                        _ => None,
                    },
                    FOp::Delay { input, .. } => Some(input.node),
                    _ => None,
                },
                Edge::Capture(k) => caps.get(k).map(|p| p.node),
                Edge::Feedback | Edge::Out => unreachable!("not a node's dependency"),
            };
            if let Some(s) = src
                && let Some(d) = deps[i].iter().position(|&x| x == s)
            {
                deps[i].remove(d);
            }
            deps[i].push(f);
        }
    }
    for (g, (_, node)) in w.ghosts.iter().enumerate() {
        live[n + g] = true;
        deps[n + g] = node.op.deps().into_iter().filter(|&d| d < total).collect();
    }
    for (k, f) in w.fades.iter().enumerate() {
        live[first + k] = true;
        for x in [&f.old, &f.new] {
            if let Input::Port { src, .. } = x {
                deps[first + k].push(src.node);
            }
        }
    }
    let mut users: Vec<Vec<usize>> = vec![vec![]; total];
    let mut waiting = vec![0usize; total];
    for (i, d) in deps.iter().enumerate().filter(|(i, _)| live[*i]) {
        for &x in d.iter().filter(|&&x| live[x]) {
            users[x].push(i);
            waiting[i] += 1;
        }
    }
    let mut ready: BinaryHeap<Reverse<usize>> = (0..total).filter(|&i| live[i] && waiting[i] == 0).map(Reverse).collect();
    let mut order = vec![];
    while let Some(Reverse(i)) = ready.pop() {
        order.push(i);
        for &u in &users[i] {
            waiting[u] -= 1;
            if waiting[u] == 0 {
                ready.push(Reverse(u));
            }
        }
    }
    if order.len() == live.iter().filter(|x| **x).count() {
        w.order = order;
        return None;
    }
    // A cycle: walk from a node left over through its left-over inputs until one repeats.
    let left = |i: usize| live[i] && waiting[i] > 0;
    let mut at = (0..total).find(|&i| left(i)).expect("a node left over");
    let mut seen: Vec<usize> = vec![];
    while !seen.contains(&at) {
        seen.push(at);
        at = *deps[at].iter().find(|&&x| left(x)).expect("a left-over node waits on another");
    }
    let from = seen.iter().position(|&x| x == at).expect("seen");
    Some(seen.split_off(from))
}
