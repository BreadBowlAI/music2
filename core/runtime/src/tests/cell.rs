//! Cells (step 5): values written into a playing renderer through its slots, applied at the next
//! block line with no rebuild and no allocation, stepped or ramped (RENDER-22, RENDER-22a,
//! RENDER-28); and kernel input memory that is the host's alone (ABI-1, RENDER-24).

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use m2_ir::{Frame, KernelSig, Origin, PortKind, Unit};

use super::live::{Rig, live};
use super::*;

/// `ir` with a component frame (site 1) in its first module, which declared controls name.
pub(super) fn framed(mut ir: Ir) -> Ir {
    ir.modules[0].frames.push(Frame { site: 1, component: Some("test:cells".into()), ..Frame::default() });
    ir
}

/// The origin of the value written at `site`'s first argument.
pub(super) fn origin(site: u32) -> Origin {
    Origin { site, arg: vec!["0".into()], ..Origin::default() }
}

/// A cell: a constant written at `site`.
pub(super) fn step_const(id: &str, value: f64, site: u32) -> Value {
    json!({"id": id, "op": "const", "value": value, "origin": {"site": site, "arg": ["0"]}})
}

/// A cell of a control declared smooth ([`framed`] gives it its frame).
pub(super) fn smooth_const(id: &str, value: f64, site: u32) -> Value {
    json!({"id": id, "op": "const", "value": value,
           "origin": {"site": site, "arg": ["0"], "control": {"frame": 0, "slot": id, "smooth": true}}})
}

/// A 440 Hz sine times a gain cell written at site 2, smooth or not.
fn sine_times(gain: f64, smooth: bool) -> Ir {
    let g = if smooth { smooth_const("g", gain, 2) } else { step_const("g", gain, 2) };
    framed(song(
        json!([
            {"id": "f", "op": "const", "value": 440, "unit": "hz"},
            {"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "f"}},
            g,
            {"id": "out", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "o", "by": "g"}}]),
        "out",
        json!([]),
    ))
}

fn renderer(ir: &Ir, data: &DataSet) -> Renderer {
    let pk = std_packages();
    Renderer::new(&check(ir, &pk).unwrap_or_else(|e| panic!("{e:?}")), data, &pk, SR).unwrap()
}

/// The next `blocks` blocks, rendered one at a time.
fn blocks(r: &mut Renderer, blocks: usize) -> Vec<[f32; 2]> {
    let mut out = vec![];
    for _ in 0..blocks {
        let clock = r.clock();
        out.extend_from_slice(r.render_block(clock).unwrap());
    }
    out
}

/// `x` times `g` frame by frame, `g` as one block's buffer.
fn times(x: &[[f32; 2]], g: &[f32]) -> Vec<[f32; 2]> {
    x.iter().zip(g).map(|(f, g)| [f[0] * g, f[1] * g]).collect()
}

/// A ramp from `a` to `b` over one block, as RENDER-22a writes it.
fn ramp(a: f32, b: f32) -> Vec<f32> {
    let mut r: Vec<f32> = (0..BLOCK).map(|i| a + (b - a) * ((i + 1) as f32 / BLOCK as f32)).collect();
    r[BLOCK - 1] = b;
    r
}

// RENDER-22, RENDER-28: a value written into a cell's slot changes it at the next block line,
// with no rebuild; the latest write before the line wins, and the renderer reports every write up
// to it as heard.
#[test]
fn a_cell_changes_at_the_next_block_line() {
    let (mut r, mut twin) = (renderer(&sine_times(1.0, false), &DataSet::empty()), renderer(&sine_times(1.0, false), &DataSet::empty()));
    let generation = r.generation();
    let g = r.cell_map().cells(&origin(2)).to_vec();
    assert_eq!(g, [CellId(0)], "one cell");
    assert_eq!(blocks(&mut r, 3), blocks(&mut twin, 3));
    assert_eq!(r.take_heard(), None);
    r.cell_slots().set(&g, 0.5, 1);
    let base = blocks(&mut twin, 1);
    assert_eq!(blocks(&mut r, 1), times(&base, &[0.5; BLOCK]), "the next block plays the new value, all of it");
    assert_eq!(r.take_heard(), Some((0, 1)));
    // Two writes before one line: the latest plays, and both are heard there.
    r.cell_slots().set(&g, 0.25, 2);
    r.cell_slots().set(&g, 0.75, 3);
    let base = blocks(&mut twin, 1);
    assert_eq!(blocks(&mut r, 1), times(&base, &[0.75; BLOCK]));
    assert_eq!((r.take_heard(), r.cell_value(g[0])), (Some((1, 3)), Some(0.75)));
    // No write, no report, and the value stays.
    let base = blocks(&mut twin, 2);
    assert_eq!(blocks(&mut r, 2), times(&base, &[0.75; 2 * BLOCK]));
    assert_eq!(r.take_heard(), None);
    assert_eq!((r.generation(), r.resets()), (generation, 0), "nothing was rebuilt or reset");
}

// RENDER-22a, PKG-14: a cell of a control declared smooth ramps linearly from its old value to
// the new one over one block, ending exactly on it; a step cell doesn't.
#[test]
fn a_smooth_cell_ramps_over_one_block() {
    let (mut r, mut twin) = (renderer(&sine_times(1.0, true), &DataSet::empty()), renderer(&sine_times(1.0, true), &DataSet::empty()));
    let g = r.cell_map().cells(&origin(2)).to_vec();
    blocks(&mut r, 2);
    blocks(&mut twin, 2);
    r.cell_slots().set(&g, 0.5, 1);
    let base = blocks(&mut twin, 3);
    let out = blocks(&mut r, 3);
    assert_eq!(out[..BLOCK], times(&base[..BLOCK], &ramp(1.0, 0.5)), "the ramp");
    assert_eq!(out[BLOCK..], times(&base[BLOCK..], &[0.5; 2 * BLOCK]), "then the value");
    // A new value while steady ramps again, from where it is.
    r.cell_slots().set(&g, 0.8, 2);
    let base = blocks(&mut twin, 1);
    assert_eq!(blocks(&mut r, 1), times(&base, &ramp(0.5, 0.8)));
    // A step cell steps.
    let mut s = renderer(&sine_times(1.0, false), &DataSet::empty());
    let mut twin = renderer(&sine_times(1.0, false), &DataSet::empty());
    s.cell_slots().set(&g, 0.5, 1);
    assert_eq!(blocks(&mut s, 1), times(&blocks(&mut twin, 1), &[0.5; BLOCK]));
}

// RENDER-22a, RENDER-33: a ramp is deterministic, and the same live as offline: the live engine,
// asked for frames in uneven pieces, plays the bits an offline renderer does with the same write
// at the same block line, and stamps when it heard it.
#[test]
fn ramps_are_the_same_offline_and_live() {
    let ir = sine_times(0.9, true);
    let g = [CellId(0)];
    let offline = |seq: u64| {
        let mut r = renderer(&ir, &DataSet::empty());
        let mut out = vec![[0.0; 2]; 8 * BLOCK];
        r.process(&mut out[..3 * BLOCK]).unwrap();
        r.cell_slots().set(&g, 0.2, seq);
        r.process(&mut out[3 * BLOCK..]).unwrap();
        out
    };
    let reference = offline(1);
    assert!(offline(7) == reference, "deterministic");
    let mut rig = Rig::new(live(&ir, &DataSet::empty()));
    rig.cmd(Cmd::Play);
    let slots = rig.e.current().unwrap().cell_slots();
    let mut out = vec![];
    let mut uneven = |rig: &mut Rig, upto: usize| {
        for n in [1usize, 7, 100, 3, 64].iter().cycle() {
            let n = (*n).min(upto - out.len());
            if n == 0 {
                break;
            }
            out.extend(rig.run(n));
        }
    };
    uneven(&mut rig, 3 * BLOCK);
    slots.set(&g, 0.2, 1);
    uneven(&mut rig, 8 * BLOCK);
    assert!(out == reference, "live differs from offline from frame {:?}", out.iter().zip(&reference).position(|(a, b)| a != b));
    let (stamps, n) = rig.e.heard();
    assert_eq!(n, 1);
    assert_eq!((stamps[0].from, stamps[0].to, stamps[0].clock), (0, 1, 3 * BLOCK as u64), "heard at the block that applied it");
    assert_eq!((rig.e.activations(), rig.e.applied().1), (0, 1), "no message but the play");
}

// RENDER-22: a cell inside a `poly` voice is one cell that reaches its constant in every voice,
// sounding or not, and voices split mid-block read it whole.
#[test]
fn a_voice_cell_reaches_every_voice() {
    let ir = |gain: f64| {
        song(
            json!([
                {"id": "n", "op": "lane", "lane": "notes"},
                {"id": "v", "op": "poly", "events": "n", "voices": 3, "body": {"out": "w", "nodes": [
                    {"id": "p", "op": "lane", "lane": "pitch"}, {"id": "g", "op": "lane", "lane": "gate"},
                    {"id": "hz", "op": "kernel", "kernel": "std:mtof", "inputs": {"pitch": "p"}},
                    {"id": "o", "op": "kernel", "kernel": "std:saw", "inputs": {"freq": "hz"}},
                    {"id": "e", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "o", "by": "g"}},
                    step_const("vc", gain, 4),
                    {"id": "w", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "e", "by": "vc"}}]}}]),
            "v",
            json!([{"key": "notes", "kind": "events<note>"}]),
        )
    };
    // 120 bpm: a block is 0.00533 beats. Three notes from the start, and one that starts inside
    // block 6, after the write.
    let data = live::with_notes(
        json!({"clips": [{"start": 0, "length": 4, "notes": [
            {"pitch": 57, "start": 0.0013, "length": 1}, {"pitch": 64, "start": 0.0021, "length": 1},
            {"pitch": 69, "start": 0.017, "length": 1}, {"pitch": 72, "start": 0.0333, "length": 1}]}]}),
        live::at_bpm(120.0),
    );
    let (mut r, mut before, mut after) = (renderer(&ir(1.0), &data), renderer(&ir(1.0), &data), renderer(&ir(0.5), &data));
    let vc = r.cell_map().cells(&origin(4)).to_vec();
    assert_eq!(vc.len(), 1, "one cell for every voice");
    assert_eq!(blocks(&mut r, 4), blocks(&mut before, 4));
    blocks(&mut after, 4);
    assert_eq!(r.active_voices(), 3);
    r.cell_slots().set(&vc, 0.5, 1);
    assert!(blocks(&mut r, 8) == blocks(&mut after, 8), "every voice plays the new value from the block line on");
    assert_eq!(r.active_voices(), 3, "the fourth note stole one");
}

// RENDER-22: a steady cell is written into the kernel input it feeds once, and again only when it
// changes; while it ramps, each call copies its part of the ramp.
#[test]
fn a_steady_cell_is_written_into_its_kernel_once() {
    let mut r = renderer(&sine_times(1.0, true), &DataSet::empty());
    let g = [CellId(0)];
    // What the scale kernel's `by` input last took from its cell, and the cell's version.
    let by = |r: &Renderer| {
        let at = r.plan.graph.exec.iter().rposition(|e| matches!(e, Exec::Kernel(_))).unwrap();
        let Exec::Kernel(k) = &r.plan.graph.exec[at] else { unreachable!() };
        let Input::Port { src, .. } = k.inputs[1] else { panic!("`by` comes from the cell") };
        (k.seen[1], r.plan.graph.versions[src.node])
    };
    assert_eq!(by(&r), (COPY, 0), "not yet written");
    blocks(&mut r, 3);
    assert_eq!(by(&r), (0, 0), "written once, at its first block");
    r.cell_slots().set(&g, 0.5, 1);
    blocks(&mut r, 1);
    assert_eq!(by(&r), (COPY, COPY), "ramping: copied at each call");
    blocks(&mut r, 2);
    let (seen, version) = by(&r);
    assert!(seen == version && version != COPY && version > 0, "steady again, written once more");
}

// RENDER-23, PKG-3: a lease on a value reaches every cell that holds it: one literal written once
// and given to two controls is one value with two cells, and the controls a call left out are
// one value each, told apart by their slots.
#[test]
fn a_value_maps_to_every_cell_that_holds_it() {
    let slot = |frame: u32, slot: &str, inherited: bool| Some(m2_ir::Slot { frame, slot: slot.into(), inherited, smooth: false });
    let cell = |origin: Origin| CellInfo { at: CellAt::Top(0), origin, value: 0.3, smooth: false };
    let written = |s: &str| Origin { control: slot(0, s, false), ..origin(5) };
    let inherited = |s: &str| Origin { site: 5, control: slot(0, s, true), ..Origin::default() };
    let map =
        CellMap::of(&[cell(written("feedback")), cell(inherited("time")), cell(written("mix")), cell(inherited("q")), cell(origin(6))]);
    assert_eq!(map.cells(&written("mix")), [CellId(0), CellId(2)], "`const g = frac(0.3)` given to two slots");
    assert_eq!(map.cells(&origin(5)), [CellId(0), CellId(2)], "the literal, whatever slot it is asked for by");
    assert_eq!((map.cells(&inherited("time")), map.cells(&inherited("q"))), (&[CellId(1)][..], &[CellId(3)][..]));
    assert_eq!(map.cells(&inherited("mix")), [] as [CellId; 0], "a slot written at the call isn't inherited");
    assert_eq!(map.cells(&origin(6)), [CellId(4)]);
    assert_eq!(map.cells(&origin(7)), [] as [CellId; 0]);
}

// RENDER-6a: a cell write wakes an idle engine within a block, as a message does, though it
// sends none.
#[test]
fn a_cell_write_wakes_an_idle_engine() {
    let mut rig = Rig::new(live(&sine_times(0.0, false), &DataSet::empty()));
    rig.run(BLOCK * 20);
    assert!(rig.e.is_idle(), "stopped and silent");
    let slots = rig.e.current().unwrap().cell_slots();
    slots.set(&[CellId(0)], 1.0, 1);
    let out = rig.run(BLOCK * 2);
    assert!(!rig.e.is_idle() && peak(&out) > 0.5, "woken by the write");
    assert_eq!(rig.e.applied().1, 0, "no message was sent");
}

// RENDER-28: the slot protocol never reports a write heard before it is applied. One thread
// writes cells with rising seqs while another drains; after each drain that reports `p`, every
// cell holds a write at least as new as the last write to it up to `p`.
//
// The scheduler doesn't promise that the reader runs while the writer writes: on a loaded machine
// the writes (about a millisecond in all) can end before the reader's first turn, and then only the
// end state is checked (76 of 300 runs beside 32 busy threads). So at every `GATE`th write the
// writer waits for one more drain, which makes the reader drain mid-stream at least once per gate.
// Between gates the two still race freely.
#[test]
fn cell_heard_never_runs_ahead() {
    const CELLS: usize = 128;
    const GATE: u64 = 4096;
    let writes: u64 = if cfg!(debug_assertions) { 200_000 } else { 1_000_000 };
    let mut seed = 0x9e3779b97f4a7c15u64;
    // Write `s` (from 1) sets cell `plan[s - 1]` to `s`, which an f32 holds exactly.
    let plan: Arc<Vec<u32>> = Arc::new(
        (0..writes)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                (seed % CELLS as u64) as u32
            })
            .collect(),
    );
    let slots = Arc::new(CellSlots::new(CELLS));
    let done = Arc::new(AtomicBool::new(false));
    // How many drains have reported something: the writer's gates wait for it to move.
    let drains = Arc::new(AtomicU64::new(0));
    let writer = {
        let (plan, slots, done, drains) = (plan.clone(), slots.clone(), done.clone(), drains.clone());
        std::thread::spawn(move || {
            for (k, &c) in plan.iter().enumerate() {
                let s = k as u64 + 1;
                // The count is read before the write, so the drain that hears this write moves it
                // even when the writer is held up between the two and the drain comes first.
                let gate = s.is_multiple_of(GATE) && s < writes;
                let seen = if gate { drains.load(Ordering::Acquire) } else { 0 };
                slots.set(&[CellId(c)], s as f32, s);
                if gate {
                    while drains.load(Ordering::Acquire) == seen {
                        std::thread::yield_now();
                    }
                }
            }
            done.store(true, Ordering::Release);
        })
    };
    let (mut held, mut least, mut heard, mut mid) = (vec![0u64; CELLS], vec![0u64; CELLS], 0u64, 0);
    loop {
        let finished = done.load(Ordering::Acquire);
        if let Some(p) = slots.drain(|id, v| held[id.0 as usize] = v as u64) {
            assert!(p >= heard);
            for s in heard + 1..=p {
                least[plan[s as usize - 1] as usize] = s;
            }
            heard = p;
            mid += (p < writes) as u64;
            if let Some(c) = (0..CELLS).find(|&c| held[c] < least[c]) {
                panic!("write {p} was heard, but cell {c} holds write {} and write {} set it", held[c], least[c]);
            }
            drains.fetch_add(1, Ordering::Release);
        }
        if finished && !slots.pending() {
            break;
        }
    }
    writer.join().unwrap();
    assert_eq!(heard, writes);
    assert_eq!(held, least, "every cell ends on its last write");
    // Each gate's wait ends on a drain that heard no more than the gate's write.
    assert!(mid >= (writes - 1) / GATE, "drained while writing: {mid} times mid-stream");
}

// ---------------------------------------------------------------- ABI-1

/// std, and a package `t` holding one hand-written kernel `k` with one audio input and output,
/// whose `k_reset` and `k_process` (with `$io`, its port table) are given. Its sandbox's first block
/// is its port buffers, so its input is at 8192 ([`crate::tests::module_v1`]).
fn with_kernel(reset: &str, process: &str) -> (PackageSet, PathBuf) {
    let dir = project(&[]);
    let wat = crate::tests::module_v1(&format!(
        r#"(func (export "k_new") (param i32) (result i32) (i32.const 4096))
  (func (export "k_ports") (result i32) (i32.const 257))
  (func (export "k_reset") (param i32) {reset})
  (func (export "k_process") (param $s i32) (param $io i32) (param $from i32) (param $to i32) {process})
  (func (export "k_active") (param i32) (result i32) (i32.const 0))"#
    ));
    std::fs::write(dir.join("k.wasm"), crate::tests::wasm(&wat)).unwrap();
    std::fs::write(
        dir.join("m2pkg.toml"),
        "[package]\nname = \"@test/t\"\nversion = \"0.1.0\"\nwasm = \"k.wasm\"\n\n[[kernel]]\nname = \"k\"\ninputs = [\"in: audio<1>\"]\noutputs = [\"out: audio<1>\"]\n",
    )
    .unwrap();
    let mut pk = std_packages();
    pk.load(&dir).unwrap();
    (pk, dir)
}

/// A sine through `@test/t:k`.
fn through_k(pk: &PackageSet) -> Result<Renderer> {
    let ir = song(
        json!([{"id": "x", "op": "kernel", "kernel": "std:sine"}, {"id": "k", "op": "kernel", "kernel": "@test/t:k", "inputs": {"in": "x"}}]),
        "k",
        json!([]),
    );
    Renderer::new(&check(&ir, pk).unwrap_or_else(|e| panic!("{e:?}")), &DataSet::empty(), pk, SR)
}

// ABI-1, RENDER-24: debug builds check that a call into a kernel leaves its input memory as the
// host wrote it: a kernel that writes an input fails the render, naming it.
#[cfg(debug_assertions)]
#[test]
fn a_kernel_that_writes_its_input_fails() {
    let (pk, dir) = with_kernel("", "(f32.store (i32.load (local.get $io)) (f32.const 12345))");
    let e = through_k(&pk).unwrap().render(0.01).unwrap_err();
    assert!(format!("{e:#}").contains("song/k: `@test/t:k` wrote its input `in` (ABI-1: kernel input memory is read-only)"), "{e:#}");
    // An export (not strict) fails the same way (RENDER-25).
    let mut r = through_k(&pk).unwrap();
    r.set_strict(false);
    assert!(r.render(0.01).is_err(), "an export fails");
    // Live, it stops alone, as a crashed kernel does (RENDER-25): `song/k`, node 1, the output.
    let mut r = through_k(&pk).unwrap();
    r.set_strict(false);
    let mut rig = Rig::new(r);
    let out = rig.run(4 * BLOCK);
    let told = rig.e.current().unwrap().info().faults.drain();
    let stopped = crate::fault::Fault {
        code: crate::fault::Code::Stopped,
        plan: rig.e.current().unwrap().info().id,
        subject: crate::fault::Subject::Node(1),
    };
    assert!(peak(&out) == 0.0 && told == [stopped], "{told:?}");
    std::fs::remove_dir_all(dir).ok();
}

// ABI-1: `k_reset` must not change input memory either; a renderer resets every kernel when it is
// built (and on panic and retrigger), so such a kernel fails there.
#[cfg(debug_assertions)]
#[test]
fn a_reset_that_changes_its_input_fails() {
    let (pk, dir) = with_kernel("(f32.store (i32.const 8196) (f32.const 7))", "");
    let e = through_k(&pk).map(drop).unwrap_err();
    assert!(format!("{e:#}").contains("song/k: `@test/t:k`'s reset changed its input `in` (ABI-1"), "{e:#}");
    std::fs::remove_dir_all(dir).ok();
}

/// What feeds the generated kernels' inputs in one graph.
struct Sources {
    audio: &'static str,
    stereo: &'static str,
    unit01: &'static str,
    gate: &'static str,
    beats: &'static str,
    /// None: a cell.
    pitch: Option<&'static str>,
}

/// A node for every kernel in `sigs`, ids prefixed `p`: an input with a default is a cell (every
/// other one smooth), one without is a signal from `src`, and the first output of each is summed
/// into the returned node (a control output first scales the audio, or sets a sine's frequency).
fn every_kernel(sigs: &[&KernelSig], p: &str, src: &Sources, site: &mut u32, nodes: &mut Vec<Value>) -> String {
    let mut sum: Option<String> = None;
    for (i, sig) in sigs.iter().enumerate() {
        let id = format!("{p}{i}");
        let mut inputs = serde_json::Map::new();
        for port in &sig.inputs {
            let from = match (port.kind, port.name.as_str()) {
                (PortKind::Audio(Some(2)), _) => src.stereo.to_string(),
                (PortKind::Audio(_) | PortKind::Any, _) => src.audio.to_string(),
                (PortKind::Control(_), "gate") => src.gate.to_string(),
                (PortKind::Control(_), "beats") => src.beats.to_string(),
                (PortKind::Control(_), "pitch") if src.pitch.is_some() => src.pitch.unwrap().to_string(),
                (PortKind::Control(Some(Unit::Unit01)), _) if port.default.is_none() => src.unit01.to_string(),
                (PortKind::Control(unit), name) => {
                    let cell = format!("{id}c{name}");
                    let value = port.default.unwrap_or(match name {
                        "lo" => 100.0,
                        "hi" => 1000.0,
                        "pitch" => 60.0,
                        _ => 0.5,
                    });
                    *site += 1;
                    let mut node = if site.is_multiple_of(2) { smooth_const(&cell, value, *site) } else { step_const(&cell, value, *site) };
                    if let Some(u) = unit {
                        node["unit"] = json!(u.name());
                    }
                    nodes.push(node);
                    cell
                }
                (PortKind::Events(_), _) => unreachable!("kernels take no events"),
            };
            inputs.insert(port.name.clone(), json!(from));
        }
        nodes.push(json!({"id": id, "op": "kernel", "kernel": sig.id, "inputs": inputs}));
        let first = format!("{id}.{}", sig.outputs[0].name);
        let audio = match sig.outputs[0].kind {
            PortKind::Control(Some(Unit::Hz)) => {
                nodes.push(json!({"id": format!("{id}s"), "op": "kernel", "kernel": "std:sine", "inputs": {"freq": first}}));
                format!("{id}s")
            }
            PortKind::Control(_) => {
                nodes
                    .push(json!({"id": format!("{id}s"), "op": "kernel", "kernel": "std:scale", "inputs": {"in": src.audio, "by": first}}));
                format!("{id}s")
            }
            _ => first,
        };
        sum = Some(match sum {
            None => audio,
            Some(acc) => {
                nodes.push(json!({"id": format!("{id}a"), "op": "kernel", "kernel": "std:add", "inputs": {"a": acc, "b": audio}}));
                format!("{id}a")
            }
        });
    }
    sum.expect("std has kernels")
}

// ABI-1, step 5 gate: the debug input-memory check passes for every std kernel. Each is played at
// the top level and in a voice, fed by signals and cells, through notes that start mid-block and
// steal voices, cell steps and ramps, and a panic.
#[test]
fn every_std_kernel_keeps_its_inputs() {
    let pk = std_packages();
    let mut sigs: Vec<&KernelSig> = pk.kernel_sigs().collect();
    sigs.sort_by(|a, b| a.id.cmp(&b.id));
    let mut site = 1;
    let mut nodes = vec![
        json!({"id": "n", "op": "lane", "lane": "notes"}),
        json!({"id": "x", "op": "kernel", "kernel": "std:saw"}),
        json!({"id": "xs", "op": "kernel", "kernel": "std:pan", "inputs": {"in": "x"}}),
        json!({"id": "t", "op": "time", "of": "beats"}),
        json!({"id": "lf", "op": "kernel", "kernel": "std:lfo", "inputs": {"beats": "t"}}),
    ];
    let top = Sources { audio: "x", stereo: "xs", unit01: "lf", gate: "lf", beats: "t", pitch: None };
    let top_sum = every_kernel(&sigs, "k", &top, &mut site, &mut nodes);
    // In a voice, time and the lfo are captured from the top.
    let mut body = vec![
        json!({"id": "vp", "op": "lane", "lane": "pitch"}),
        json!({"id": "vg", "op": "lane", "lane": "gate"}),
        json!({"id": "vhz", "op": "kernel", "kernel": "std:mtof", "inputs": {"pitch": "vp"}}),
        json!({"id": "vo", "op": "kernel", "kernel": "std:saw", "inputs": {"freq": "vhz"}}),
        json!({"id": "vst", "op": "kernel", "kernel": "std:pan", "inputs": {"in": "vo"}}),
    ];
    let voice = Sources { audio: "vo", stereo: "vst", unit01: "lf", gate: "vg", beats: "t", pitch: Some("vp") };
    let voice_sum = every_kernel(&sigs, "vk", &voice, &mut site, &mut body);
    nodes.push(json!({"id": "v", "op": "poly", "events": "n", "voices": 2, "body": {"out": voice_sum, "nodes": body}}));
    nodes.push(json!({"id": "out", "op": "kernel", "kernel": "std:add", "inputs": {"a": top_sum, "b": "v"}}));
    let ir = framed(song(Value::Array(nodes), "out", json!([{"key": "notes", "kind": "events<note>"}])));
    let flat = check(&ir, &pk).unwrap_or_else(|e| panic!("{e:?}"));
    // Complete: every std kernel at the top and in the voice.
    let kernels = |nodes: &[FNode]| -> BTreeSet<String> {
        nodes.iter().filter_map(|n| if let FOp::Kernel { kernel, .. } = &n.op { Some(kernel.clone()) } else { None }).collect()
    };
    let all: BTreeSet<String> = sigs.iter().map(|s| s.id.clone()).collect();
    let FOp::Poly { voice, .. } = &flat.nodes.iter().find(|n| n.name == "song/v").unwrap().op else { panic!("the poly") };
    assert_eq!((kernels(&flat.nodes), kernels(&voice.nodes)), (all.clone(), all), "every std kernel, both places");
    // Three overlapping notes on two voices, starting inside blocks.
    let data = live::with_notes(
        json!({"clips": [{"start": 0, "length": 8, "notes": [
            {"pitch": 45, "start": 0.0013, "length": 0.6}, {"pitch": 52, "start": 0.2071, "length": 0.4},
            {"pitch": 57, "start": 0.3109, "length": 0.9}, {"pitch": 60, "start": 1.5, "length": 0.2}]}]}),
        live::at_bpm(120.0),
    );
    let mut r = Renderer::new(&flat, &data, &pk, SR).unwrap();
    let cells = cells::cells(&flat);
    assert!(cells.len() > 100 && cells.iter().any(|c| c.smooth), "{} cells", cells.len());
    r.render(0.3).unwrap();
    // Every cell moves a little: steps and ramps, at the top and in voices.
    for (i, c) in cells.iter().enumerate() {
        r.cell_slots().set(&[CellId(i as u32)], (c.value * 0.9) as f32, i as u64 + 1);
    }
    r.render(0.3).unwrap();
    assert!(cells.iter().enumerate().all(|(i, c)| r.cell_value(CellId(i as u32)) == Some((c.value * 0.9) as f32)));
    r.panic();
    r.render(0.6).unwrap();
}
