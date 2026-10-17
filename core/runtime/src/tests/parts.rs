//! A plan's partitions and their schedule (8a, RENDER-2): what each partition holds, which waits for
//! which, and that every node reads only what the schedule puts before it.

use super::live::{Rig, flat, live};
use super::*;
use crate::engine::Msg;
use crate::parts::{Parts, Schedule};

/// A module `key` that is a track: `nodes`, exporting `out`.
fn module(key: &str, nodes: Value, out: &str) -> Value {
    json!({"key": key, "track": true, "nodes": nodes, "exports": {"out": out}})
}

/// A song of `tracks` (key, nodes, out) and `song`'s own `nodes` after an import of each track (as
/// its key), output `out`.
fn tracks(tracks: &[(&str, Value, &str)], nodes: Value, out: &str) -> Ir {
    let mut song: Vec<Value> = tracks.iter().map(|(k, _, _)| json!({"id": k, "op": "import", "module": k, "export": "out"})).collect();
    song.extend(nodes.as_array().unwrap().iter().cloned());
    let mut modules: Vec<Value> = tracks.iter().map(|(k, n, o)| module(k, n.clone(), o)).collect();
    modules.push(json!({"key": "song", "nodes": song, "exports": {"out": out}}));
    serde_json::from_value(json!({"ir": m2_ir::IR_VERSION, "root": {"module": "song", "export": "out"}, "modules": modules})).unwrap()
}

/// A sine at `hz` through `kernels` scales by a half: a track of `kernels` + 2 kernels.
fn sine_track(hz: f64, kernels: usize) -> (Value, &'static str) {
    let mut nodes = vec![
        json!({"id": "f", "op": "const", "value": hz, "unit": "hz"}),
        json!({"id": "o0", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "f"}}),
        json!({"id": "h", "op": "const", "value": 0.5}),
    ];
    for k in 0..kernels {
        nodes.push(
            json!({"id": format!("o{}", k + 1), "op": "kernel", "kernel": "std:scale", "inputs": {"in": format!("o{k}"), "by": "h"}}),
        );
    }
    nodes.push(json!({"id": "out", "op": "kernel", "kernel": "std:scale", "inputs": {"in": format!("o{kernels}"), "by": "h"}}));
    (Value::Array(nodes), "out")
}

/// Three tracks of different weights, summed in the song.
fn three() -> Ir {
    let (a, b, c) = (sine_track(110.0, 1), sine_track(220.0, 6), sine_track(330.0, 3));
    tracks(
        &[("a", a.0, a.1), ("b", b.0, b.1), ("c", c.0, c.1)],
        json!([
            {"id": "ab", "op": "kernel", "kernel": "std:add", "inputs": {"a": "a", "b": "b"}},
            {"id": "m", "op": "kernel", "kernel": "std:add", "inputs": {"a": "ab", "b": "c"}}]),
        "m",
    )
}

fn schedule(r: &Renderer) -> &Schedule {
    &r.plan.schedule
}

/// Each partition's label, its nodes' modules, and the partitions it reads.
fn shape(r: &Renderer) -> Vec<(String, u16, Vec<u32>, Vec<u32>)> {
    let p = &schedule(r).parts;
    (0..p.len() as u32)
        .map(|q| {
            let (l, d) = p.label(q);
            let reads = (0..p.len() as u32).filter(|&o| p.reads(q, o)).collect();
            (l.to_string(), d, p.nodes(q).to_vec(), reads)
        })
        .collect()
}

/// Every node of the order is in one partition, in plan order there, and may read all it reads.
fn whole(order: &[usize], exec: &[Exec], p: &Parts) {
    let mut seen = vec![0; exec.len()];
    for q in 0..p.len() as u32 {
        let nodes = p.nodes(q);
        let at: Vec<usize> = nodes.iter().map(|&i| order.iter().position(|&o| o == i as usize).unwrap()).collect();
        assert!(at.windows(2).all(|w| w[0] < w[1]), "partition {q} is in plan order: {at:?}");
        for &i in nodes {
            seen[i as usize] += 1;
            assert_eq!(p.part_of(i as usize), q);
            exec[i as usize].reads(|j| assert!(p.may_read(q, i as usize, j), "node {i} may read {j}"));
        }
    }
    for (i, n) in seen.iter().enumerate() {
        assert_eq!(*n, usize::from(order.contains(&i)), "node {i} is in one partition, or none outside the order");
    }
}

/// Whether the partitions' reads have no cycle: a topological order exists.
fn acyclic(p: &Parts) -> bool {
    let n = p.len() as u32;
    let mut done = vec![false; n as usize];
    for _ in 0..n {
        let Some(q) = (0..n).find(|&q| !done[q as usize] && (0..n).all(|o| !p.reads(q, o) || done[o as usize])) else { return false };
        done[q as usize] = true;
    }
    true
}

// RENDER-2 (8a): each track is one partition and the song another; the song reads every track,
// and the tracks read nothing.
#[test]
fn a_track_is_one_partition() {
    let pk = std_packages();
    let f = flat(&three());
    let r = Renderer::new(&f, &DataSet::empty(), &pk, SR).unwrap();
    let g = &r.plan.graph;
    whole(&g.order, &g.exec, &schedule(&r).parts);
    let s = shape(&r);
    let labels: Vec<(&str, u16)> = s.iter().map(|(l, d, _, _)| (l.as_str(), *d)).collect();
    assert_eq!(labels, [("a", 0), ("b", 0), ("c", 0), ("song", 0)]);
    for (q, (label, _, nodes, reads)) in s.iter().enumerate() {
        for &i in nodes {
            let module = match i as usize {
                i if i < r.plan.nodes => f.nodes[i].module.as_str(),
                // Overlays, after the plan's nodes: each goes with the track it carries.
                i => match &g.exec[i] {
                    Exec::Overlay { src, .. } => f.nodes[src.node].module.as_str(),
                    _ => panic!("node {i} isn't a plan node or an overlay"),
                },
            };
            assert_eq!(module, label, "node {i} of partition {q}");
        }
        let want: Vec<u32> = if label == "song" { vec![0, 1, 2] } else { vec![] };
        assert_eq!(*reads, want, "{label} reads");
    }
    assert_eq!(schedule(&r).tasks.width(), 3);
}

/// A kernel node reading `inputs` (nodes), with no instance: enough to schedule.
fn reader(inputs: &[usize]) -> Exec {
    let inputs: Vec<Input> =
        inputs.iter().map(|&n| Input::Port { src: PortRef { node: n, port: 0 }, src_channels: 1, channels: 1 }).collect();
    Exec::Kernel(KernelNode { seen: vec![COPY; inputs.len()], inputs, inst: None })
}

fn boxed(labels: &[&str]) -> Vec<Box<str>> {
    labels.iter().map(|l| Box::from(*l)).collect()
}

// RENDER-2 (8a): where two modules read each other both ways, their nodes split by how many
// crossings lead to them, so no path leaves a partition and comes back. Modules on no such cycle
// stay whole.
#[test]
fn modules_that_read_each_other_split_by_depth() {
    // a0 → b1 → a2 → b3, and a4 reads only a0; c5 reads b3, c6 reads c5.
    let exec = vec![reader(&[]), reader(&[0]), reader(&[1]), reader(&[2]), reader(&[0]), reader(&[3]), reader(&[5])];
    let labels = boxed(&["a", "b", "a", "b", "a", "c", "c"]);
    let order: Vec<usize> = (0..7).collect();
    let s = Schedule::build(&order, &exec, &labels).unwrap();
    let p = &s.parts;
    let parts: Vec<((&str, u16), Vec<u32>)> = (0..p.len() as u32).map(|q| (p.label(q), p.nodes(q).to_vec())).collect();
    assert_eq!(parts, [(("a", 0), vec![0, 4]), (("a", 2), vec![2]), (("b", 1), vec![1]), (("b", 3), vec![3]), (("c", 0), vec![5, 6])]);
    whole(&order, &exec, p);
    assert!(acyclic(p));
    assert_eq!(s.tasks.width(), 1, "one chain: a0 a4, b1, a2, b3, then c5 c6");
}

/// A small deterministic generator.
fn next(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *seed >> 33
}

// RENDER-2 (8a): over random graphs with random labels, cycles between labels included, every
// node is in one partition, every read is earlier in its partition or in one it waits for, and the
// partitions have no cycle.
#[test]
fn every_edge_goes_forward() {
    for seed in 0..500u64 {
        let mut s = seed + 1;
        let n = 2 + (next(&mut s) % 40) as usize;
        let labels_n = 1 + (next(&mut s) % 5) as usize;
        let mut exec = vec![];
        let mut labels = vec![];
        for i in 0..n {
            let reads: Vec<usize> = (0..i).filter(|_| next(&mut s).is_multiple_of(4)).collect();
            exec.push(reader(&reads));
            labels.push(format!("m{}", next(&mut s) as usize % labels_n));
        }
        // Some nodes left out of the order: what reads them reads a frozen buffer.
        let order: Vec<usize> = (0..n).filter(|_| !next(&mut s).is_multiple_of(10)).collect();
        let labels: Vec<Box<str>> = labels.into_iter().map(Box::from).collect();
        let sch = Schedule::build(&order, &exec, &labels).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        whole(&order, &exec, &sch.parts);
        assert!(acyclic(&sch.parts), "seed {seed}");
    }
}

// RENDER-2 (8a): the same plan gives the same schedule: partitions, their nodes, labels and reads.
#[test]
fn the_schedule_is_the_same_for_the_same_plan() {
    let pk = std_packages();
    let f = flat(&three());
    let a = Renderer::new(&f, &DataSet::empty(), &pk, SR).unwrap();
    let b = Renderer::new(&f, &DataSet::empty(), &pk, SR).unwrap();
    assert_eq!(shape(&a), shape(&b));
    assert_eq!(schedule(&a).tasks.roots(), schedule(&b).tasks.roots());
}

// 8a (design §5): the heaviest track is taken first; the song, which reads all, is no root.
#[test]
fn the_most_critical_partitions_go_first() {
    let pk = std_packages();
    let r = Renderer::new(&flat(&three()), &DataSet::empty(), &pk, SR).unwrap();
    let p = &schedule(&r).parts;
    let roots: Vec<&str> = schedule(&r).tasks.roots().iter().map(|&q| p.label(q).0).collect();
    assert_eq!(roots, ["b", "c", "a"], "by weight: b has 8 kernels, c 5, a 3");
}

// 8a: a song of one module is one partition, and a chain of modules wakes no worker: width 1.
#[test]
fn a_chain_has_width_one() {
    let pk = std_packages();
    let (a, _) = sine_track(110.0, 2);
    let ir = song(a, "out", json!([]));
    let r = Renderer::new(&flat(&ir), &DataSet::empty(), &pk, SR).unwrap();
    assert_eq!((schedule(&r).parts.len(), schedule(&r).tasks.width()), (1, 1));
    let exec = vec![reader(&[]), reader(&[0]), reader(&[1])];
    let s = Schedule::build(&[0, 1, 2], &exec, &boxed(&["a", "b", "c"])).unwrap();
    assert_eq!((s.parts.len(), s.tasks.width()), (3, 1));
}

/// A kick, a pad whose `m` adds the kick to its sine (`hz`, node `sine`), and the song's `nodes`
/// after imports of both, its output `out`.
fn sidechained(sine: &str, hz: f64, nodes: Value, out: &str) -> Ir {
    let (k, _) = sine_track(55.0, 0);
    let pad = json!([
        {"id": "x", "op": "import", "module": "kick", "export": "out"},
        {"id": "f", "op": "const", "value": hz, "unit": "hz"},
        {"id": sine, "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "f"}},
        {"id": "m", "op": "kernel", "kernel": "std:add", "inputs": {"a": sine, "b": "x"}}]);
    tracks(&[("kick", k, "out"), ("pad", pad, "m")], nodes, out)
}

/// What each exec node's label must be (§5.1): its module; a ghost its old module; a fade the
/// module of what reads it, else of its new side; an overlay its source's.
fn rules_hold(r: &Renderer, old: &Flat, new: &Flat) {
    let g = &r.plan.graph;
    let p = &schedule(r).parts;
    let label = |i: usize| p.label(p.part_of(i)).0.to_string();
    let moves = &r.plan.entry.as_ref().expect("an activation").moves;
    let mut checked = [0; 4];
    for &i in &g.order {
        let (want, kind) = match &g.exec[i] {
            _ if i < r.plan.nodes => (new.nodes[i].module.clone(), 0),
            _ if r.plan.ghosts.contains(&i) => {
                let from = moves.iter().find(|m| m.to as usize == i).expect("a ghost's box moves in").from;
                (old.nodes[from as usize].module.clone(), 1)
            }
            Exec::Overlay { src, .. } => (label(src.node), 2),
            Exec::Fade { new: side, .. } => {
                let mut readers = vec![];
                for (j, e) in g.exec.iter().enumerate() {
                    if !matches!(e, Exec::Feedback { .. }) {
                        e.reads(|k| readers.extend((k == i).then_some(j)));
                    }
                }
                match (readers.first(), side) {
                    (Some(&j), _) => (label(j), 3),
                    (None, Input::Port { src, .. }) => (label(src.node), 3),
                    _ => continue,
                }
            }
            _ => panic!("node {i} is no plan node, ghost, fade or overlay"),
        };
        assert_eq!(label(i), want, "node {i}");
        checked[kind] += 1;
    }
    assert!(checked.iter().all(|&c| c > 0), "plan nodes, ghosts, overlays and fades all checked: {checked:?}");
}

// RENDER-2 (8a, §5.1): in an activation's window, a ghost runs with its old module, a fade with
// what reads it (the output's with its new side), and every overlay, the window's own included,
// with the track it carries.
#[test]
fn ghosts_fades_and_overlays_take_their_labels() {
    let pk = std_packages();
    let before =
        sidechained("s", 220.0, json!([{"id": "mix", "op": "kernel", "kernel": "std:add", "inputs": {"a": "kick", "b": "pad"}}]), "mix");
    // The pad's sine is replaced: a ghost, and `m`'s input fades. The song's mix reads a new delay
    // on the pad (a fade-in, and the old side read through a window overlay), and the output is a
    // new node (the output fades).
    let after = sidechained(
        "s2",
        330.0,
        json!([
            {"id": "dt", "op": "const", "value": 0.01, "unit": "seconds"},
            {"id": "d", "op": "kernel", "kernel": "std:delay", "inputs": {"in": "pad", "time": "dt"}},
            {"id": "mix", "op": "kernel", "kernel": "std:add", "inputs": {"a": "kick", "b": "d"}},
            {"id": "h", "op": "const", "value": 0.5},
            {"id": "o2", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "mix", "by": "h"}}]),
        "o2",
    );
    let (old, new) = (flat(&before), flat(&after));
    let mut rig = Rig::of(&old, &DataSet::empty(), live(&before, &DataSet::empty()));
    rig.cmd(Cmd::Play);
    rig.run(4 * BLOCK);
    let next = rig.prepare(&new, &DataSet::empty(), &pk);
    assert!(!next.plan.ghosts.is_empty() && !next.plan.windowed.is_empty(), "a ghost, and overlays of the window's own");
    rules_hold(&next, &old, &new);
    let g = &next.plan.graph;
    whole(&g.order, &g.exec, &schedule(&next).parts);
    // The window plays, every read checked against the schedule (debug builds).
    rig.send(Msg::Activate { renderer: Box::new(next) });
    rig.run(16 * BLOCK);
    assert!(!rig.e.in_window(), "the window is over");
}

/// Every kind of read across tracks: a voice capturing the kick (a poly's capture), a missing
/// plugin passing the kick through, and kernels reading tracks (the song's mix).
fn every_read() -> (Ir, DataSet) {
    let (k, _) = sine_track(55.0, 0);
    let pad = json!([
        {"id": "k", "op": "import", "module": "kick", "export": "out"},
        {"id": "n", "op": "lane", "lane": "notes"},
        {"id": "v", "op": "poly", "events": "n", "voices": 2, "body": {"out": "x", "nodes": [
            {"id": "p", "op": "lane", "lane": "pitch"},
            {"id": "hz", "op": "kernel", "kernel": "std:mtof", "inputs": {"pitch": "p"}},
            {"id": "o", "op": "kernel", "kernel": "std:saw", "inputs": {"freq": "hz"}},
            {"id": "x", "op": "kernel", "kernel": "std:add", "inputs": {"a": "o", "b": "k"}}]}}]);
    let gone = json!([
        {"id": "k", "op": "import", "module": "kick", "export": "out"},
        {"id": "g", "op": "plugin", "plugin": "clap:org.example.gone-fx", "inputs": {"in": "k"}}]);
    let mut ir = tracks(
        &[("kick", k, "out"), ("pad", pad, "v"), ("gone", gone, "g")],
        json!([
            {"id": "a", "op": "kernel", "kernel": "std:add", "inputs": {"a": "kick", "b": "pad"}},
            {"id": "m", "op": "kernel", "kernel": "std:add", "inputs": {"a": "a", "b": "gone"}}]),
        "m",
    );
    ir.modules[1].lanes = serde_json::from_value(json!([{"key": "notes", "kind": "events<note>"}])).unwrap();
    let notes = json!({"clips": [{"start": 0, "length": 4, "notes": [{"pitch": 60, "start": 0, "length": 1}]}]});
    let data = DataSet::with_lanes(
        super::live::at_bpm(120.0),
        [(m2_song::LaneAddr::new("pad", "notes").unwrap(), {
            m2_song::LaneData::read(&notes.to_string(), &m2_song::LaneKind::Notes).unwrap()
        })],
    );
    (ir, data)
}

// RENDER-2 (8a): a node reads only what `Exec::reads` names, so the schedule puts all it reads
// before it. Debug builds check every read as it happens (`view.rs`); here a voice's capture, a
// missing plugin and kernels all read across tracks, and render without the check firing. The
// window's reads are checked by `ghosts_fades_and_overlays_take_their_labels`.
#[test]
#[cfg_attr(not(debug_assertions), ignore = "the check runs in debug builds")]
fn reads_lists_every_read() {
    let mut pk = std_packages();
    let (mut ir, data) = every_read();
    let diags = pk.bind_plugins(&mut ir);
    assert!(diags.iter().all(|d| d.code == "plugin.missing"), "{diags:?}");
    let f = check(&ir, &pk).unwrap_or_else(|e| panic!("{e:?}"));
    let mut r = Renderer::new(&f, &data, &pk, SR).unwrap();
    let p = &r.plan.schedule.parts;
    assert!(p.len() >= 4, "the kick, the pad, the plugin's track and the song");
    let out = r.render(0.5).unwrap();
    assert!(out.iter().any(|f| f[0] != 0.0));
}

/// Tracks `a` and `b` (each a sine at `hz` through `kernels` scales, its pitch `pitch` semitones
/// through `std:mtof`), summed in the song. A pitch of 100 000 makes `mtof` give an infinity.
fn two(a: (f64, usize), b: (f64, usize)) -> Ir {
    let track = |(pitch, kernels): (f64, usize)| {
        let mut nodes = vec![
            json!({"id": "p", "op": "const", "value": pitch, "unit": "semitones"}),
            json!({"id": "hz", "op": "kernel", "kernel": "std:mtof", "inputs": {"pitch": "p"}}),
            json!({"id": "o0", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "hz"}}),
            json!({"id": "h", "op": "const", "value": 0.5}),
        ];
        for k in 0..kernels {
            nodes.push(
                json!({"id": format!("o{}", k + 1), "op": "kernel", "kernel": "std:scale", "inputs": {"in": format!("o{k}"), "by": "h"}}),
            );
        }
        (Value::Array(nodes), format!("o{kernels}"))
    };
    let ((na, oa), (nb, ob)) = (track(a), track(b));
    tracks(
        &[("a", na, &oa), ("b", nb, &ob)],
        json!([{"id": "m", "op": "kernel", "kernel": "std:add", "inputs": {"a": "a", "b": "b"}}]),
        "m",
    )
}

/// Where node `name` is in `r`'s order.
fn at(f: &Flat, r: &Renderer, name: &str) -> usize {
    let i = f.nodes.iter().position(|n| n.name == name).unwrap_or_else(|| panic!("no node {name}"));
    r.plan.graph.order.iter().position(|&o| o == i).unwrap()
}

/// The first block of `ir` on `runner`, strict: what it says, and `r` after it.
fn first_block(ir: &Ir, runner: &mut Runner) -> (Result<(), String>, Renderer, Flat) {
    let pk = std_packages();
    let f = flat(ir);
    let mut r = Renderer::new(&f, &DataSet::empty(), &pk, SR).unwrap();
    let said = r.render_block_on(0, runner).map(drop).map_err(|e| e.to_string());
    (said, r, f)
}

// RENDER-2 (8a): when two nodes fail in one block, the error names the one first in plan order,
// whichever ran first and on whichever thread. Here `b`'s track is heavier, so it is taken first.
#[test]
fn strict_names_the_first_failing_node_on_any_thread_count() {
    let ir = two((100_000.0, 1), (100_000.0, 6));
    for threads in [1, 4, 7] {
        let mut t = (threads > 1).then(|| spread(threads));
        let mut one = Runner::one();
        let runner = t.as_mut().map_or(&mut one, |t| &mut t.runner);
        let (said, r, f) = first_block(&ir, runner);
        assert!(at(&f, &r, "a/hz") < at(&f, &r, "b/hz"), "a is first in plan order");
        let roots: Vec<&str> = r.plan.schedule.tasks.roots().iter().map(|&q| r.plan.schedule.parts.label(q).0).collect();
        assert_eq!(roots[0], "b", "b is taken first");
        let e = said.unwrap_err();
        assert!(e.contains("a/hz produced inf"), "{threads} threads: {e}");
        if let Some(t) = &t {
            assert!(t.runner.spread_report().tasks_on_workers > 0, "{threads} threads: tasks ran on workers");
        }
    }
}

// RENDER-2 (8a): a failing node stops only what reads it: the other track still runs that block,
// and its output is a clean render's. (Step 7's loop stopped at the failure, before `b`.)
#[test]
fn a_failing_node_stops_only_what_reads_it() {
    let (said, r, f) = first_block(&two((100_000.0, 1), (60.0, 2)), &mut Runner::one());
    assert!(said.unwrap_err().contains("a/hz produced inf"));
    let (clean, c, _) = first_block(&two((60.0, 1), (60.0, 2)), &mut Runner::one());
    clean.unwrap();
    let b = f.nodes.iter().position(|n| n.name == "b/o2").unwrap();
    assert!(at(&f, &r, "a/hz") < at(&f, &r, "b/o2"), "a is first in plan order");
    assert!(c.output_of(b).iter().any(|x| *x != 0.0));
    assert_eq!(r.output_of(b), c.output_of(b), "b ran, as in a clean render");
}

/// A random song of std kernels with a feedback: sines, scales, adds and a delay, each reading
/// earlier nodes; a feedback reads the last node and feeds an add in the middle.
fn random_song(seed: u64) -> Ir {
    let mut s = seed + 7;
    let n = 4 + (next(&mut s) % 14) as usize;
    let mut nodes =
        vec![json!({"id": "c", "op": "const", "value": 0.5}), json!({"id": "t", "op": "const", "value": 0.003, "unit": "seconds"})];
    let fb_at = 1 + (next(&mut s) as usize % (n - 1));
    for k in 0..n {
        let pick = |s: &mut u64| format!("n{}", next(s) as usize % k.max(1));
        let node = match (k, next(&mut s) % 4) {
            (0, _) | (_, 0) => {
                nodes.push(json!({"id": format!("f{k}"), "op": "const", "value": 110.0 * (1 + k) as f64, "unit": "hz"}));
                json!({"op": "kernel", "kernel": "std:sine", "inputs": {"freq": format!("f{k}")}})
            }
            _ if k == fb_at => json!({"op": "kernel", "kernel": "std:add", "inputs": {"a": pick(&mut s), "b": "fb"}}),
            (_, 1) => json!({"op": "kernel", "kernel": "std:scale", "inputs": {"in": pick(&mut s), "by": "c"}}),
            (_, 2) => json!({"op": "kernel", "kernel": "std:add", "inputs": {"a": pick(&mut s), "b": pick(&mut s)}}),
            _ => json!({"op": "kernel", "kernel": "std:delay", "inputs": {"in": pick(&mut s), "time": "t"}}),
        };
        let mut node = node;
        node["id"] = json!(format!("n{k}"));
        nodes.push(node);
    }
    nodes.push(json!({"id": "fb", "op": "feedback", "kind": "audio<1>", "input": "q"}));
    nodes.push(json!({"id": "q", "op": "kernel", "kernel": "std:scale", "inputs": {"in": format!("n{}", n - 1), "by": "c"}}));
    song(Value::Array(nodes), "q", json!([]))
}

/// `r`'s graph in a random order that keeps every read after what it reads, cut into random runs.
fn shuffle_and_cut(r: &mut Renderer, seed: u64) -> usize {
    let mut s = seed + 11;
    let g = &mut r.plan.graph;
    let n = g.exec.len();
    let mut left: Vec<usize> = g.order.clone();
    let mut order = vec![];
    while !left.is_empty() {
        let ready: Vec<usize> = (0..left.len())
            .filter(|&k| {
                let mut ok = true;
                g.exec[left[k]].reads(|j| ok &= !left.contains(&j) || j >= n);
                ok
            })
            .collect();
        let k = ready[next(&mut s) as usize % ready.len()];
        order.push(left.remove(k));
    }
    let cuts: Vec<usize> = (1..order.len()).filter(|_| next(&mut s).is_multiple_of(3)).collect();
    g.order = order;
    r.plan.schedule = Schedule::from_cuts(&r.plan.graph.order, &r.plan.graph.exec, &cuts).unwrap();
    cuts.len() + 1
}

// RENDER-2 (8a): any order that keeps reads after what they read, cut into any runs, renders the
// same bits as the plan's own schedule on one thread, on 1, 4 and 7 threads: 200 random songs with
// a feedback, 64 blocks each. With more than one thread, tasks ran on workers in every render, and
// over the whole test some read a producer that ran on another thread.
#[test]
fn random_convex_partitions_give_the_same_bits() {
    let pk = std_packages();
    let seconds = 64.0 * BLOCK as f64 / SR as f64;
    let mut pools = spreads([4, 7]);
    for seed in 0..200u64 {
        let f = check(&random_song(seed), &pk).unwrap_or_else(|e| panic!("{e:?}"));
        let reference = Renderer::new(&f, &DataSet::empty(), &pk, SR).unwrap().render(seconds).unwrap();
        let mut r = Renderer::new(&f, &DataSet::empty(), &pk, SR).unwrap();
        let parts = shuffle_and_cut(&mut r, seed);
        assert!(r.render(seconds).unwrap() == reference, "seed {seed}: the same bits on one thread");
        for t in &mut pools {
            let mut r = Renderer::new(&f, &DataSet::empty(), &pk, SR).unwrap();
            shuffle_and_cut(&mut r, seed);
            let before = t.runner.spread_report();
            assert!(
                r.render_on(seconds, &mut t.runner).unwrap() == reference,
                "seed {seed}: the same bits on {} threads",
                t.runner.threads()
            );
            let on = t.runner.spread_report().tasks_on_workers - before.tasks_on_workers;
            assert!(on > 0, "seed {seed}, {parts} partitions: tasks ran on workers");
        }
    }
    for t in &pools {
        assert!(t.runner.spread_report().edges_across > 0, "{} threads: some read crossed threads", t.runner.threads());
    }
}

/// A kick; a pad whose voices capture the kick and duck under it (`follow`); the song mixing both,
/// plus a feedback from the pad's output (read after the graph, in another partition).
fn crossings() -> (Ir, DataSet) {
    let (k, _) = sine_track(55.0, 0);
    let pad = json!([
        {"id": "k", "op": "import", "module": "kick", "export": "out"},
        {"id": "n", "op": "lane", "lane": "notes"},
        {"id": "v", "op": "poly", "events": "n", "voices": 4, "body": {"out": "x", "nodes": [
            {"id": "p", "op": "lane", "lane": "pitch"},
            {"id": "hz", "op": "kernel", "kernel": "std:mtof", "inputs": {"pitch": "p"}},
            {"id": "o", "op": "kernel", "kernel": "std:saw", "inputs": {"freq": "hz"}},
            {"id": "x", "op": "kernel", "kernel": "std:add", "inputs": {"a": "o", "b": "k"}}]}},
        {"id": "f", "op": "kernel", "kernel": "std:follow", "inputs": {"in": "k"}},
        {"id": "d", "op": "kernel", "kernel": "std:duck", "inputs": {"in": "v", "side": "f"}}]);
    let mut ir = tracks(
        &[("kick", k, "out"), ("pad", pad, "d")],
        json!([
            {"id": "fb", "op": "feedback", "kind": "audio<1>", "input": "pad"},
            {"id": "h", "op": "const", "value": 0.3},
            {"id": "e", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "fb", "by": "h"}},
            {"id": "a", "op": "kernel", "kernel": "std:add", "inputs": {"a": "kick", "b": "pad"}},
            {"id": "m", "op": "kernel", "kernel": "std:add", "inputs": {"a": "a", "b": "e"}}]),
        "m",
    );
    ir.modules[1].lanes = serde_json::from_value(json!([{"key": "notes", "kind": "events<note>"}])).unwrap();
    let notes: Vec<Value> = (0..16).map(|k| json!({"pitch": 48 + (k * 5) % 24, "start": k as f64 * 0.25, "length": 0.5})).collect();
    let notes = json!({"clips": [{"start": 0, "length": 8, "notes": notes}]});
    let data = DataSet::with_lanes(
        super::live::at_bpm(120.0),
        [(m2_song::LaneAddr::new("pad", "notes").unwrap(), {
            m2_song::LaneData::read(&notes.to_string(), &m2_song::LaneKind::Notes).unwrap()
        })],
    );
    (ir, data)
}

// RENDER-2 (8a): reads across tracks give the same bits on 4 and 7 threads as on one: a voice's
// capture of another track, a sidechain (`follow` of the kick), and a feedback whose input runs in
// another partition.
#[test]
fn cross_module_feedback_captures_and_sidechains_on_n_threads() {
    let pk = std_packages();
    let (ir, data) = crossings();
    let f = check(&ir, &pk).unwrap_or_else(|e| panic!("{e:?}"));
    let one = Renderer::new(&f, &data, &pk, SR).unwrap().render(4.0).unwrap();
    assert!(peak(&one) > 0.1);
    for threads in [4, 7] {
        let mut t = spread(threads);
        let out = Renderer::new(&f, &data, &pk, SR).unwrap().render_on(4.0, &mut t.runner).unwrap();
        assert!(out == one, "{threads} threads: the same bits");
        let s = t.runner.spread_report();
        assert!(s.tasks_on_workers > 0 && s.edges_across > 0, "{threads} threads: {s:?}");
    }
}

// 8a (Decision 22): a song that hosts a plugin renders every task on the calling thread, whatever
// runner it is given: plugins on worker threads come with live playback on all cores.
#[test]
fn a_song_with_a_plugin_runs_no_task_on_a_worker() {
    m2_plugins::test_main_thread();
    m2_plugins::probe::install();
    let mut pk = std_packages();
    let ((k, _), (h, _)) = (sine_track(55.0, 1), sine_track(440.0, 1));
    let mut ir = tracks(
        &[
            ("kick", k, "out"),
            ("hat", h, "out"),
            (
                "fx",
                json!([
            {"id": "k", "op": "import", "module": "kick", "export": "out"},
            {"id": "p", "op": "plugin", "plugin": "clap:m2.probe", "inputs": {"in": "k"}}]),
                "p",
            ),
        ],
        json!([
            {"id": "a", "op": "kernel", "kernel": "std:add", "inputs": {"a": "hat", "b": "fx"}},
            {"id": "m", "op": "kernel", "kernel": "std:add", "inputs": {"a": "kick", "b": "a"}}]),
        "m",
    );
    assert!(pk.bind_plugins(&mut ir).is_empty());
    let f = check(&ir, &pk).unwrap_or_else(|e| panic!("{e:?}"));
    let mut t = spread(4);
    let mut r = Renderer::new(&f, &DataSet::empty(), &pk, SR).unwrap();
    assert!(r.hosts_plugins() && r.plan.schedule.tasks.width() > 1);
    r.render_on(0.25, &mut t.runner).unwrap();
    let workers: u64 = t.runner.counters()[1..].iter().map(|c| c.tasks).sum();
    assert_eq!((t.runner.spread_report().tasks_on_workers, workers), (0, 0), "every task ran on the caller");
}
