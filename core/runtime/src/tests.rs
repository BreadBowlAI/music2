use std::path::{Path, PathBuf};

use m2_ir::{Ir, check};
use m2_song::{DataSet, Project};
use serde_json::{Value, json};

use super::*;

mod audition;
mod cell;
mod fpenv;
mod live;
mod overlay;
mod parts;
mod stream;
mod taps;

const SR: u32 = 48_000;

/// Counts heap operations on a thread while it asks to (RENDER-28).
mod alloc_count {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    pub struct Counter;

    thread_local! {
        static ON: Cell<bool> = const { Cell::new(false) };
        static N: Cell<usize> = const { Cell::new(0) };
    }

    fn count() {
        let _ = ON.try_with(|on| {
            if on.get() {
                let _ = N.try_with(|n| n.set(n.get() + 1));
            }
        });
    }

    unsafe impl GlobalAlloc for Counter {
        unsafe fn alloc(&self, l: Layout) -> *mut u8 {
            count();
            unsafe { System.alloc(l) }
        }

        unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
            count();
            unsafe { System.dealloc(p, l) }
        }

        unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
            count();
            unsafe { System.realloc(p, l, n) }
        }
    }

    /// What `f` returns, and the heap operations it made on this thread.
    pub fn during<R>(f: impl FnOnce() -> R) -> (R, usize) {
        N.with(|n| n.set(0));
        ON.with(|o| o.set(true));
        let r = f();
        ON.with(|o| o.set(false));
        (r, N.with(|n| n.get()))
    }
}

#[global_allocator]
static COUNTER: alloc_count::Counter = alloc_count::Counter;

/// One pool at a time in this test binary (8a): each spins its workers, and tests beside them
/// wait on clocks.
static POOLS: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A pool of `threads` threads in spread mode: every worker woken at each run, the caller kept off
/// the first tasks, a random spin before each, so tasks surely run on workers.
pub(crate) struct Spread {
    pub(crate) runner: Runner,
    _pool: Pool,
    _one_at_a_time: Option<std::sync::MutexGuard<'static, ()>>,
}

fn spread(threads: usize) -> Spread {
    let [one] = spreads([threads]);
    one
}

/// Pools of each of `threads` threads, made together. The last holds the lock, and drops last.
fn spreads<const N: usize>(threads: [usize; N]) -> [Spread; N] {
    let mut one = Some(POOLS.lock().unwrap_or_else(|e| e.into_inner()));
    let mut made = 0;
    threads.map(|n| {
        made += 1;
        let (pool, mut runner) = Pool::new(n, crate::threads::setup(false)).unwrap();
        runner.spread(true);
        Spread { runner, _pool: pool, _one_at_a_time: if made == N { one.take() } else { None } }
    })
}

fn std_packages() -> PackageSet {
    let mut p = PackageSet::default();
    p.load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")).expect("std loads");
    p
}

fn song(nodes: Value, out: &str, lanes: Value) -> Ir {
    serde_json::from_value(json!({"ir": m2_ir::IR_VERSION, "root": {"module": "song", "export": "out"},
        "modules": [{"key": "song", "lanes": lanes, "nodes": nodes, "exports": {"out": out}}]}))
    .unwrap()
}

const MANIFEST: &str = "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\n";

/// A project folder with the given data files, in a fresh temp dir, and the data `ir` reads
/// from it.
fn timeline_for(ir: &Ir, files: &[(&str, Value)]) -> (DataSet, PathBuf) {
    let dir = project(files);
    (data(ir, &dir), dir)
}

/// A project folder with the given data files, in a fresh temp dir.
fn project(files: &[(&str, Value)]) -> PathBuf {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "m2-runtime-test-{}-{}",
        std::process::id(),
        N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("music2.toml"), MANIFEST).unwrap();
    std::fs::write(dir.join("song.ts"), "").unwrap();
    for (name, v) in files {
        let p = dir.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, v.to_string()).unwrap();
    }
    dir
}

/// The data `ir` reads from the project at `dir`.
fn data(ir: &Ir, dir: &Path) -> DataSet {
    let (data, diags) = Project::open(dir).unwrap_or_else(|e| panic!("{e:?}")).data(&decls(ir), None);
    assert!(diags.iter().all(|d| !d.code.starts_with("lane.")), "{diags:?}");
    data
}

fn render(ir: &Ir, tl: &DataSet, seconds: f64) -> Vec<[f32; 2]> {
    let pk = std_packages();
    let flat = check(ir, &pk).unwrap_or_else(|e| panic!("{e:?}"));
    Renderer::new(&flat, tl, &pk, SR).unwrap().render(seconds).unwrap()
}

fn peak(x: &[[f32; 2]]) -> f32 {
    x.iter().fold(0.0, |m, f| m.max(f[0].abs()).max(f[1].abs()))
}

#[test]
fn sine_has_the_right_pitch() {
    let ir = song(
        json!([
            {"id": "f", "op": "const", "value": 440, "unit": "hz"},
            {"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "f"}},
        ]),
        "o",
        json!([]),
    );
    let out = render(&ir, &DataSet::empty(), 1.0);
    assert_eq!(out.len(), SR as usize);
    let crossings = out.windows(2).filter(|w| w[0][0] < 0.0 && w[1][0] >= 0.0).count();
    assert!((439..=441).contains(&crossings), "{crossings} upward zero crossings");
    assert!((peak(&out) - 1.0).abs() < 0.01);
    // Mono output is copied to both channels.
    assert!(out.iter().all(|f| f[0] == f[1]));
}

#[test]
fn poly_plays_notes_from_the_timeline_and_is_deterministic() {
    let lanes = json!([{"key": "notes", "kind": "events<note>"}, {"key": "cutoff", "kind": "control<hz>", "default": 2000}]);
    let ir = song(
        json!([
            {"id": "n", "op": "lane", "lane": "notes"},
            {"id": "c", "op": "lane", "lane": "cutoff"},
            {"id": "v", "op": "poly", "events": "n", "voices": 4, "body": {"out": "amp", "nodes": [
                {"id": "p", "op": "lane", "lane": "pitch"},
                {"id": "g", "op": "lane", "lane": "gate"},
                {"id": "hz", "op": "kernel", "kernel": "std:mtof", "inputs": {"pitch": "p"}},
                {"id": "o", "op": "kernel", "kernel": "std:saw", "inputs": {"freq": "hz"}},
                {"id": "f", "op": "kernel", "kernel": "std:svf", "inputs": {"in": "o", "cutoff": "c"}},
                {"id": "e", "op": "kernel", "kernel": "std:adsr", "inputs": {"gate": "g", "release": "r"}},
                {"id": "r", "op": "const", "value": 0.05, "unit": "seconds"},
                {"id": "amp", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "f.lp", "by": "e"}}]}},
        ]),
        "v",
        lanes,
    );
    // 120 bpm: one beat is half a second. A C major chord on beat 0 for one beat, then silence.
    let (tl, dir) = timeline_for(
        &ir,
        &[
            ("data/arrangement.json", json!({"tempo": [{"beat": 0, "bpm": 120}]})),
            (
                "data/song/notes.json",
                json!({"clips": [{"start": 0, "length": 4, "notes": [
            {"pitch": "C4", "start": 0, "length": 1}, {"pitch": "E4", "start": 0, "length": 1}, {"pitch": "G4", "start": 0, "length": 1}]}]}),
            ),
        ],
    );
    let a = render(&ir, &tl, 2.0);
    let b = render(&ir, &tl, 2.0);
    assert!(a == b, "two renders differ");
    let half = SR as usize / 2;
    assert!(peak(&a[..half]) > 0.3, "chord is audible");
    assert!(peak(&a[half + SR as usize / 4..]) < 1e-3, "released and silent after the note");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn voice_stealing_keeps_playing() {
    let ir = song(
        json!([
            {"id": "n", "op": "lane", "lane": "notes"},
            {"id": "v", "op": "poly", "events": "n", "voices": 1, "body": {"out": "amp", "nodes": [
                {"id": "p", "op": "lane", "lane": "pitch"},
                {"id": "g", "op": "lane", "lane": "gate"},
                {"id": "hz", "op": "kernel", "kernel": "std:mtof", "inputs": {"pitch": "p"}},
                {"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "hz"}},
                {"id": "amp", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "o", "by": "g"}}]}},
        ]),
        "v",
        json!([{"key": "notes", "kind": "events<note>"}]),
    );
    let (tl, dir) = timeline_for(
        &ir,
        &[(
            "data/song/notes.json",
            json!({"clips": [{"start": 0, "length": 8, "notes": [
        {"pitch": 60, "start": 0, "length": 4}, {"pitch": 72, "start": 1, "length": 1}]}]}),
        )],
    );
    // Default tempo 120: the second note (beat 1..2) steals the only voice.
    let out = render(&ir, &tl, 1.0);
    let window = &out[SR as usize / 2 + 1000..SR as usize - 1000];
    let crossings = window.windows(2).filter(|w| w[0][0] < 0.0 && w[1][0] >= 0.0).count() as f32;
    let hz = crossings / (window.len() as f32 / SR as f32);
    assert!((hz - 523.25).abs() < 10.0, "the newer note (C5) plays, got {hz} Hz");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn control_lanes_follow_automation() {
    let ir = song(
        json!([
            {"id": "l", "op": "lane", "lane": "level"},
            {"id": "o", "op": "kernel", "kernel": "std:sine"},
            {"id": "a", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "o", "by": "l"}},
        ]),
        "a",
        json!([{"key": "level", "kind": "control<gain>", "default": 0}]),
    );
    let (tl, dir) = timeline_for(
        &ir,
        &[("data/song/level.json", json!({"unit": "gain", "clips": [{"start": 0, "length": 2, "points": [[0, 0], [2, 1]]}]}))],
    );
    let out = render(&ir, &tl, 1.5);
    let q = SR as usize / 4;
    assert!(peak(&out[..q]) < 0.3);
    assert!(peak(&out[3 * q..4 * q]) > 0.7);
    assert!(peak(&out[5 * q..]) == 0.0, "after the clip the lane is back at its default (0)");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn feedback_delays_one_block() {
    // out = noise + 0.5 × (out one block ago): stays bounded and differs from plain noise.
    let ir = song(
        json!([
            {"id": "x", "op": "kernel", "kernel": "std:noise"},
            {"id": "fb", "op": "feedback", "kind": "audio<1>", "input": "sum"},
            {"id": "half", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "fb", "by": "h"}},
            {"id": "h", "op": "const", "value": 0.5},
            {"id": "sum", "op": "kernel", "kernel": "std:add", "inputs": {"a": "x", "b": "half"}},
        ]),
        "sum",
        json!([]),
    );
    let out = render(&ir, &DataSet::empty(), 0.5);
    assert!(peak(&out) < 2.5 && peak(&out) > 1.0);
}

#[test]
fn std_manifest_matches_its_kernels() {
    // Instantiating every kernel checks its port counts against the manifest.
    let pk = std_packages();
    let mut ids: Vec<String> = pk.kernel_sigs().map(|s| s.id.clone()).collect();
    ids.sort();
    assert!(ids.len() >= 20, "{ids:?}");
    for id in ids {
        let sig = m2_ir::Kernels::kernel(&pk, &id).unwrap().clone();
        let inputs: Vec<Input> = sig.inputs.iter().map(|_| Input::Const(0.0)).collect();
        let node = FNode {
            name: id.clone(),
            module: "t".into(),
            src: None,
            origin: None,
            op: FOp::Const(0.0),
            outputs: sig.outputs.iter().map(|_| Kind::Audio(1)).collect(),
        };
        plain_kernel(&pk, &node, &id, 1, &inputs, 48_000.0).unwrap_or_else(|e| panic!("{e:#}"));
    }
}

// RENDER-32: every std kernel fits under the 256 MiB a sandbox may take at its birth, at the highest
// rate a project may have (384 kHz) and in stereo. Std's kernels take their memory as they are born
// (delay lines and reverbs sized by the rate), and never after, so born, one block played and reset
// is the most an instance ever holds. A kernel added later is checked too. Prints the one that holds
// the most.
#[test]
fn every_std_kernel_fits_its_memory_at_the_highest_rate() {
    const RATE: f64 = 384_000.0;
    let pk = std_packages();
    let mut held: Vec<(usize, String)> = vec![];
    for sig in pk.kernel_sigs() {
        let inputs: Vec<Input> = sig.inputs.iter().map(|p| Input::Const(p.default.unwrap_or(0.0))).collect();
        let node = FNode {
            name: sig.id.clone(),
            module: "t".into(),
            src: None,
            origin: None,
            op: FOp::Const(0.0),
            outputs: sig.outputs.iter().map(|_| Kind::Audio(2)).collect(),
        };
        let id = &sig.id;
        let mut k = plain_kernel(&pk, &node, id, 2, &inputs, RATE).unwrap_or_else(|e| panic!("{id}: {e:#}"));
        k.prewarm(&mut []).unwrap_or_else(|e| panic!("{id}: {e:#}"));
        k.process(&mut [], BLOCK).unwrap_or_else(|e| panic!("{id}: {e:#}"));
        k.reset(&mut []).unwrap_or_else(|e| panic!("{id}: {e:#}"));
        held.push((k.own.as_ref().unwrap().size(), id.clone()));
    }
    assert!(held.len() >= 20, "{held:?}");
    held.sort();
    let mib = |b: usize| b as f64 / (1 << 20) as f64;
    let (most, id) = held.last().unwrap();
    println!("the most a std kernel holds at 384 kHz in stereo: {id}, {:.2} MiB of {} MiB", mib(*most), mib(KERNEL_MEMORY));
    // At the cap, the kernel's next allocation fails: it crashes as it plays.
    assert!(*most < KERNEL_MEMORY, "{id} holds {:.2} MiB at 384 kHz: all the {} MiB it may have", mib(*most), mib(KERNEL_MEMORY));
}

/// Kernel `id` for node `n`, with no settings, in a sandbox of its own.
fn plain_kernel(pk: &PackageSet, n: &FNode, id: &str, channels: u8, inputs: &[Input], sample_rate: f64) -> Result<KernelInst> {
    KernelInst::new(pk, n, &Kernel { id, channels, inputs, values: &[], assets: &[], sample_rate }, 0)
}

/// A std kernel `id` as a node of its own, its inputs at their defaults, stereo.
fn std_kernel(pk: &PackageSet, id: &str, rate: f64) -> KernelInst {
    let sig = m2_ir::Kernels::kernel(pk, id).unwrap();
    let inputs: Vec<Input> = sig.inputs.iter().map(|p| Input::Const(p.default.unwrap_or(0.0))).collect();
    let outputs = sig.outputs.iter().map(|_| Kind::Audio(2)).collect();
    let node = FNode { name: id.to_string(), module: "t".into(), src: None, origin: None, op: FOp::Const(0.0), outputs };
    plain_kernel(pk, &node, id, 2, &inputs, rate).unwrap_or_else(|e| panic!("{id}: {e:#}"))
}

// ABI v1 (rules 1 and 2), RENDER-28: every std kernel resets in its sealed sandbox, taking no
// memory, back to where it was born: born and sealed, it plays a block, resets, and plays the same
// block again, bit for bit, its memory as big as at its seal. A kernel whose state holds a `Vec` and
// built it again at a reset would ask for memory there, and trap.
#[test]
fn every_std_kernel_resets_in_a_sealed_sandbox() {
    let pk = std_packages();
    let mut ids: Vec<String> = pk.kernel_sigs().map(|s| s.id.clone()).collect();
    ids.sort();
    assert!(ids.len() >= 20, "{ids:?}");
    for id in ids {
        let mut k = std_kernel(&pk, &id, SR as f64);
        let sealed = k.own.as_ref().unwrap().size();
        let play = |k: &mut KernelInst| -> Vec<u8> {
            k.process(&mut [], BLOCK).unwrap_or_else(|e| panic!("{id}: {e:#}"));
            let p = k.ports(&mut []);
            p.outs.iter().flat_map(|&at| p.data[at..at + PORT_BYTES].to_vec()).collect()
        };
        let first = play(&mut k);
        k.reset(&mut []).unwrap_or_else(|e| panic!("{id}: its reset in a sealed sandbox: {e:#}"));
        assert!(play(&mut k) == first, "{id}: a reset doesn't start over");
        assert_eq!(k.own.as_ref().unwrap().size(), sealed, "{id}: its memory grew");
    }
}

/// Plays state `s` of sandbox `sb` for `blocks` blocks: input 0 is `x(t)`, inputs 1, 2 and 3 are
/// `controls`, all mono. Returns output 0's channel 0.
fn play_state(sb: &mut sandbox::Sandbox, s: &sandbox::State, blocks: usize, x: impl Fn(usize) -> f32, controls: [f32; 3]) -> Vec<f32> {
    let mut out = vec![];
    for b in 0..blocks {
        let data = sb.data();
        for (k, v) in floats(data, s.ins[0], BLOCK).iter_mut().enumerate() {
            *v = x(b * BLOCK + k);
        }
        for (p, c) in controls.iter().enumerate() {
            floats(data, s.ins[p + 1], BLOCK).fill(*c);
        }
        s.process(sb, BLOCK).unwrap();
        out.extend_from_slice(floats(sb.data(), s.outs[0], BLOCK));
    }
    out
}

// ABI v1 (report §2.3), in wasm: every call names its state, so one sandbox holds many. Two delays
// born in one sandbox of std.wasm, fed different sounds in turns, each play exactly what one plays in
// a sandbox of its own: neither hears the other's echoes. (std/kernels-src's test of the same name
// runs the kit natively.)
#[test]
fn two_states_in_one_instance_play_independently() {
    let pk = std_packages();
    let (module, prefix, _) = pk.module_for("std:delay").unwrap();
    let born = |sb: &mut sandbox::Sandbox| sb.state(prefix, &sandbox::Spec { inputs: 4, outputs: 1, channels: &[1; 5], audio: 1, sample_rate: SR as f64, values: &[], assets: &[] }).unwrap();
    let sound = |seed: usize| move |t: usize| ((t * (7 + seed)) as f32 * 0.01).sin() * if seed == 0 { 0.8 } else { 0.3 };
    let controls = [0.01, 0.5, 0.5];
    let alone = |seed: usize| {
        let mut sb = sandbox::Sandbox::new(&pk.engine, module).unwrap();
        let s = born(&mut sb);
        sb.seal().unwrap();
        play_state(&mut sb, &s, 20, sound(seed), controls)
    };
    let mut sb = sandbox::Sandbox::new(&pk.engine, module).unwrap();
    let (a, b) = (born(&mut sb), born(&mut sb));
    sb.seal().unwrap();
    let (mut heard_a, mut heard_b) = (vec![], vec![]);
    for block in 0..20 {
        let at = |seed: usize| move |t: usize| sound(seed)(block * BLOCK + t);
        heard_a.extend(play_state(&mut sb, &a, 1, at(0), controls));
        heard_b.extend(play_state(&mut sb, &b, 1, at(1), controls));
    }
    assert!(heard_a.iter().any(|&v| v != 0.0) && heard_a != heard_b, "they play different sounds");
    assert!(heard_a == alone(0), "the first state plays as it does alone");
    assert!(heard_b == alone(1), "the second state plays as it does alone");
}

// Regression tests for bugs reported in review (2026-10-11).

#[test]
fn output_does_not_depend_on_the_callers_buffer_size() {
    let ir = song(
        json!([
            {"id": "x", "op": "kernel", "kernel": "std:noise"},
            {"id": "fb", "op": "feedback", "kind": "audio<1>", "input": "sum"},
            {"id": "half", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "fb", "by": "h"}},
            {"id": "h", "op": "const", "value": 0.5},
            {"id": "sum", "op": "kernel", "kernel": "std:add", "inputs": {"a": "x", "b": "half"}},
        ]),
        "sum",
        json!([]),
    );
    let pk = std_packages();
    let flat = check(&ir, &pk).unwrap();
    let mut one = vec![[0.0; 2]; 512];
    Renderer::new(&flat, &DataSet::empty(), &pk, SR).unwrap().process(&mut one).unwrap();
    let mut r = Renderer::new(&flat, &DataSet::empty(), &pk, SR).unwrap();
    let mut pieces = vec![[0.0; 2]; 512];
    for chunk in pieces.chunks_mut(64) {
        r.process(chunk).unwrap();
    }
    let first_diff = one.iter().zip(&pieces).position(|(a, b)| a != b);
    assert_eq!(first_diff, None, "renders differ from frame {first_diff:?}");
}

#[test]
fn a_stolen_voice_plays_until_the_new_note_starts() {
    let ir = song(
        json!([
            {"id": "n", "op": "lane", "lane": "notes"},
            {"id": "v", "op": "poly", "events": "n", "voices": 1, "body": {"out": "amp", "nodes": [
                {"id": "p", "op": "lane", "lane": "pitch"},
                {"id": "g", "op": "lane", "lane": "gate"},
                {"id": "hz", "op": "kernel", "kernel": "std:mtof", "inputs": {"pitch": "p"}},
                {"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "hz"}},
                {"id": "amp", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "o", "by": "g"}}]}},
        ]),
        "v",
        json!([{"key": "notes", "kind": "events<note>"}]),
    );
    // 120 bpm at 48 kHz: 96 samples is 0.004 beats.
    let (tl, dir) = timeline_for(
        &ir,
        &[(
            "data/song/notes.json",
            json!({"clips": [{"start": 0, "length": 1, "notes": [
        {"pitch": 69, "start": 0, "length": 1}, {"pitch": 81, "start": 0.004, "length": 0.5}]}]}),
        )],
    );
    let out = render(&ir, &tl, 0.01);
    assert!(peak(&out[1..96]) > 0.0, "the first note is silent before the second one starts");
    std::fs::remove_dir_all(dir).ok();
}

/// A voice with a delay and a feedback loop inside, notes at odd sample positions,
/// and a stolen voice: everything that depends on how a block is split.
fn tricky() -> (Ir, DataSet, PathBuf) {
    let ir = song(
        json!([
            {"id": "n", "op": "lane", "lane": "notes"},
            {"id": "v", "op": "poly", "events": "n", "voices": 2, "body": {"out": "mix", "nodes": [
                {"id": "p", "op": "lane", "lane": "pitch"},
                {"id": "g", "op": "lane", "lane": "gate"},
                {"id": "hz", "op": "kernel", "kernel": "std:mtof", "inputs": {"pitch": "p"}},
                {"id": "o", "op": "kernel", "kernel": "std:saw", "inputs": {"freq": "hz"}},
                {"id": "e", "op": "kernel", "kernel": "std:adsr", "inputs": {"gate": "g"}},
                {"id": "a", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "o", "by": "e"}},
                {"id": "fb", "op": "feedback", "kind": "audio<1>", "input": "mix"},
                {"id": "q", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "fb", "by": "h"}},
                {"id": "h", "op": "const", "value": 0.3},
                {"id": "mix", "op": "kernel", "kernel": "std:add", "inputs": {"a": "a", "b": "q"}}]}},
            {"id": "d", "op": "kernel", "kernel": "std:delay", "inputs": {"in": "v"}},
        ]),
        "d",
        json!([{"key": "notes", "kind": "events<note>"}]),
    );
    let (tl, dir) = timeline_for(
        &ir,
        &[(
            "data/song/notes.json",
            json!({"clips": [{"start": 0, "length": 4, "notes": [
        {"pitch": 57, "start": 0.0013, "length": 0.3}, {"pitch": 64, "start": 0.0071, "length": 0.2},
        {"pitch": 69, "start": 0.0109, "length": 0.5}, {"pitch": 60, "start": 0.61, "length": 0.05}]}]}),
        )],
    );
    (ir, tl, dir)
}

#[test]
fn every_buffer_size_gives_the_same_samples() {
    let (ir, tl, dir) = tricky();
    let pk = std_packages();
    let flat = check(&ir, &pk).unwrap();
    let total = 24_000;
    let mut reference = vec![[0.0; 2]; total];
    Renderer::new(&flat, &tl, &pk, SR).unwrap().process(&mut reference).unwrap();
    assert!(peak(&reference) > 0.1);
    for size in [1, 7, 64, 100, 129, 1000] {
        let mut r = Renderer::new(&flat, &tl, &pk, SR).unwrap();
        let mut out = vec![[0.0; 2]; total];
        for chunk in out.chunks_mut(size) {
            r.process(chunk).unwrap();
        }
        let diff = reference.iter().zip(&out).position(|(a, b)| a != b);
        assert_eq!(diff, None, "buffer size {size} differs from frame {diff:?}");
    }
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn seeking_gives_the_same_samples_as_a_full_render() {
    let (ir, tl, dir) = tricky();
    let pk = std_packages();
    let flat = check(&ir, &pk).unwrap();
    let mut r = Renderer::new(&flat, &tl, &pk, SR).unwrap();
    let full = r.render(1.0).unwrap();
    for from in [0u64, 1, 5_000, 12_345] {
        r.seek(from).unwrap();
        assert_eq!(r.position(), from);
        let mut part = vec![[0.0; 2]; 6_000];
        r.process(&mut part).unwrap();
        assert!(part[..] == full[from as usize..from as usize + 6_000], "seek to {from} differs");
    }
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn echoes_inside_a_voice_outlive_its_note() {
    let ir = song(
        json!([
            {"id": "n", "op": "lane", "lane": "notes"},
            {"id": "v", "op": "poly", "events": "n", "voices": 1, "body": {"out": "d", "nodes": [
                {"id": "p", "op": "lane", "lane": "pitch"},
                {"id": "g", "op": "lane", "lane": "gate"},
                {"id": "hz", "op": "kernel", "kernel": "std:mtof", "inputs": {"pitch": "p"}},
                {"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "hz"}},
                {"id": "a", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "o", "by": "g"}},
                {"id": "t", "op": "const", "value": 0.25, "unit": "seconds"},
                {"id": "d", "op": "kernel", "kernel": "std:delay", "inputs": {"in": "a", "time": "t", "mix": "m"}},
                {"id": "m", "op": "const", "value": 0.5}]}},
        ]),
        "v",
        json!([{"key": "notes", "kind": "events<note>"}]),
    );
    // A 50 ms note at 120 bpm (0.1 beats); the echo comes 250 ms after it starts.
    let (tl, dir) = timeline_for(
        &ir,
        &[("data/song/notes.json", json!({"clips": [{"start": 0, "length": 4, "notes": [{"pitch": 69, "start": 0, "length": 0.1}]}]}))],
    );
    let out = render(&ir, &tl, 1.0);
    let at = |s: f64| (s * SR as f64) as usize;
    assert!(peak(&out[at(0.1)..at(0.24)]) < 1e-3, "silent between the note and its echo");
    assert!(peak(&out[at(0.25)..at(0.30)]) > 0.1, "the first echo is heard");
    assert!(peak(&out[at(0.50)..at(0.55)]) > 0.02, "the second echo is heard");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn strict_mode_names_the_kernel_that_broke() {
    let ir = song(
        json!([
            {"id": "p", "op": "const", "value": 100000, "unit": "semitones"},
            {"id": "hz", "op": "kernel", "kernel": "std:mtof", "inputs": {"pitch": "p"}},
            {"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "hz"}},
        ]),
        "o",
        json!([]),
    );
    let pk = std_packages();
    let flat = check(&ir, &pk).unwrap();
    let e = Renderer::new(&flat, &DataSet::empty(), &pk, SR).unwrap().render(0.01).unwrap_err();
    assert!(e.to_string().contains("song/hz produced inf"), "{e}");
}

/// A package folder holding one hand-written kernel module.
fn package(wat: &str) -> PathBuf {
    package_with(wat, "outputs = [\"out: audio<1>\"]")
}

/// A package folder holding one hand-written kernel module, `t:k`, its ports as `ports` declares.
fn package_with(wat: &str, ports: &str) -> PathBuf {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir =
        std::env::temp_dir().join(format!("m2-pkg-test-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("k.wasm"), wat).unwrap();
    let manifest = format!("[package]\nname = \"t\"\nversion = \"0.1.0\"\nwasm = \"k.wasm\"\n\n[[kernel]]\nname = \"k\"\n{ports}\n");
    std::fs::write(dir.join("m2pkg.toml"), manifest).unwrap();
    dir
}

/// A kernel module speaking ABI v`abi` around `kernels`, the exports of its kernel types: its one
/// page of memory and its allocator (`m2_alloc`), which hands out room forward from 8 KiB, growing the
/// memory as it must, as its sandbox is born. Below 8 KiB a hand-written kernel may keep one state
/// of its own at a fixed place, as these tests do: they make one state in a sandbox.
pub(crate) fn module_v(abi: u32, kernels: &str) -> String {
    format!(
        r#"(module
  (memory (export "memory") 1)
  (global $heap (mut i32) (i32.const 8192))
  (func (export "m2_abi") (result i32) (i32.const {abi}))
  (func (export "m2_alloc") (param $size i32) (param $align i32) (result i32) (local $at i32) (local $end i32)
    (local.set $at (i32.and (i32.add (global.get $heap) (i32.sub (local.get $align) (i32.const 1))) (i32.sub (i32.const 0) (local.get $align))))
    (local.set $end (i32.add (local.get $at) (local.get $size)))
    (if (i32.gt_u (local.get $end) (i32.shl (memory.size) (i32.const 16)))
      (then (if (i32.eq (memory.grow (i32.sub (i32.shr_u (i32.add (local.get $end) (i32.const 65535)) (i32.const 16)) (memory.size))) (i32.const -1))
        (then (return (i32.const 0))))))
    (global.set $heap (local.get $end))
    (local.get $at))
  {kernels})"#
    )
}

/// [`module_v`] for ABI v1.
pub(crate) fn module_v1(kernels: &str) -> String {
    module_v(1, kernels)
}

/// The exports of a v1 kernel type `k` with one mono input and one mono output, its state at 4096,
/// whose `k_process` runs `body` for each frame `$i`, with `$x` the input's sample: [`OUT`] writes the
/// output's.
pub(crate) fn kernel_v1(k: &str, body: &str) -> String {
    format!(
        r#"(func (export "{k}_new") (param i32) (result i32) (f32.store (i32.const 4096) (f32.const 0)) (i32.const 4096))
           (func (export "{k}_ports") (result i32) (i32.const 257))
           (func (export "{k}_reset") (param i32) (f32.store (i32.const 4096) (f32.const 0)))
           (func (export "{k}_active") (param i32) (result i32) (i32.const 0))
           {}"#,
        process_v1(k, body)
    )
}

/// The `k_process` of [`kernel_v1`]: `body` for each frame `$i` of `from..to`, with `$x` the input's
/// sample, read through the port table `$io`.
pub(crate) fn process_v1(k: &str, body: &str) -> String {
    format!(
        r#"(func (export "{k}_process") (param $s i32) (param $io i32) (param $i i32) (param $n i32) (local $x f32)
             (block $done (loop $l
               (br_if $done (i32.ge_u (local.get $i) (local.get $n)))
               (local.set $x (f32.load (i32.add (i32.load (local.get $io)) (i32.shl (local.get $i) (i32.const 2)))))
               {body}
               (local.set $i (i32.add (local.get $i) (i32.const 1)))
               (br $l))))"#
    )
}

/// The output's sample `v` at frame `$i`, in [`kernel_v1`]'s body.
pub(crate) fn out_v1(v: &str) -> String {
    format!("(f32.store (i32.add (i32.load offset=4 (local.get $io)) (i32.shl (local.get $i) (i32.const 2))) {v})")
}

#[test]
fn packages_must_be_sandboxed_and_speak_our_abi() {
    let copy = kernel_v1("k", &out_v1("(local.get $x)"));
    let ok = package(&module_v1(&copy));
    PackageSet::default().load(&ok).unwrap();

    let wrong_abi = package(&module_v(7, &copy));
    let e = PackageSet::default().load(&wrong_abi).map(|_| ()).unwrap_err();
    assert!(format!("{e:#}").contains("ABI v7"), "{e:#}");

    let missing = package(&module_v1(""));
    let e = PackageSet::default().load(&missing).map(|_| ()).unwrap_err();
    assert!(format!("{e:#}").contains("missing export `k_ports`"), "{e:#}");

    for d in [ok, wrong_abi, missing] {
        std::fs::remove_dir_all(d).ok();
    }
}

// ABI-3: a module built for kernel ABI v0 is refused, and the refusal names its version and says
// what to do, before any v1 export is asked for.
#[test]
fn a_v0_module_is_refused_naming_its_version() {
    let v0 = r#"(module
      (memory (export "memory") 1)
      (func (export "m2_abi") (result i32) (i32.const 0))
      (func (export "k_init") (param f32 i32 i32))
      (func (export "k_ports") (result i32) (i32.const 1))
      (func (export "k_port") (param i32) (result i32) (i32.const 1024))
      (func (export "k_reset"))
      (func (export "k_process") (param i32))
      (func (export "k_active") (result i32) (i32.const 0)))"#;
    let dir = package(v0);
    let e = PackageSet::default().load(&dir).map(|_| ()).unwrap_err();
    assert!(format!("{e:#}").contains("built for kernel ABI v0"), "{e:#}");
    assert!(format!("{e:#}").contains("speaks v1"), "{e:#}");
    std::fs::remove_dir_all(dir).ok();
}

// ABI-2, ABI-3: the one import a kernel may have is `m2.asset_read`, with its signature. Any other
// import is refused, naming it, and so is a memory a sandbox can't hold.
#[test]
fn only_the_asset_import_is_allowed() {
    let copy = kernel_v1("k", &out_v1("(local.get $x)"));
    let with = |import: &str| package(&module_v1(&copy).replacen("(module", &format!("(module {import}"), 1));
    let reads = with(r#"(import "m2" "asset_read" (func (param i32 i32 i32 i32 i32) (result i32)))"#);
    PackageSet::default().load(&reads).unwrap();

    let clock = with(r#"(import "env" "clock" (func (result f64)))"#);
    let e = PackageSet::default().load(&clock).map(|_| ()).unwrap_err();
    assert!(format!("{e:#}").contains("may import only `m2.asset_read`, but this module imports `env.clock`"), "{e:#}");

    let wrong = with(r#"(import "m2" "asset_read" (func (param i32) (result i32)))"#);
    let e = PackageSet::default().load(&wrong).map(|_| ()).unwrap_err();
    assert!(format!("{e:#}").contains("`m2.asset_read` has the wrong signature"), "{e:#}");

    let shared = package(&module_v1(&copy).replace(r#"(memory (export "memory") 1)"#, r#"(memory (export "memory") 1 2 shared)"#));
    let e = PackageSet::default().load(&shared).map(|_| ()).unwrap_err();
    assert!(format!("{e:#}").contains("shared"), "{e:#}");
    for d in [reads, clock, wrong, shared] {
        std::fs::remove_dir_all(d).ok();
    }
}

// RENDER-32: a sandbox may take up to 256 MiB while it is born (its birth cap): a kernel that takes
// 40 MiB in `k_new` builds and plays; one that asks for more than 256 MiB fails the build, naming
// the node and the cap in plain words.
#[test]
fn a_kernel_past_its_birth_cap_says_it_ran_out() {
    let taking = |pages: u32| {
        let k = kernel_v1("k", &out_v1("(local.get $x)")).replacen(
            "(f32.store (i32.const 4096) (f32.const 0)) (i32.const 4096))",
            &format!("(f32.store (i32.const 4096) (f32.const 0)) (drop (memory.grow (i32.const {pages}))) (i32.const 4096))"),
            1,
        );
        package_with(&module_v1(&k), "inputs = [\"in: audio<1>\"]\noutputs = [\"out: audio<1>\"]")
    };
    let ir = song(
        json!([
            {"id": "f", "op": "const", "value": 440, "unit": "hz"},
            {"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "f"}},
            {"id": "k", "op": "kernel", "kernel": "t:k", "inputs": {"in": "o"}},
        ]),
        "k",
        json!([]),
    );
    let (big, huge) = (taking(640), taking(4097));
    let mut pk = std_packages();
    pk.load(&big).unwrap();
    let out = Renderer::new(&check(&ir, &pk).unwrap(), &DataSet::empty(), &pk, SR).unwrap().render(0.01).unwrap();
    assert!(peak(&out) > 0.1, "40 MiB at birth is allowed");
    let mut pk = std_packages();
    pk.load(&huge).unwrap();
    let e = Renderer::new(&check(&ir, &pk).unwrap(), &DataSet::empty(), &pk, SR).map(drop).unwrap_err();
    assert_eq!(format!("{e:#}"), "song/k ran out of memory: a sound may use up to 256 MB");
    assert!(e.downcast_ref::<OutOfMemory>().is_some());
    for d in [big, huge] {
        std::fs::remove_dir_all(d).ok();
    }
}

// RENDER-32: a renderer holds at most SANDBOX_LIMIT sandboxes, so two stay within 25% of the
// address space. A song that needs more is refused at its build, before any sandbox is made, in
// plain words that say what to do.
#[test]
fn a_song_past_the_sandbox_limit_is_refused_in_plain_words() {
    let n = SANDBOX_LIMIT + 1;
    let mut nodes = vec![json!({"id": "g0", "op": "kernel", "kernel": "std:sine", "inputs": {}})];
    nodes.extend((1..n).map(|i| json!({"id": format!("g{i}"), "op": "kernel", "kernel": "std:gain", "inputs": {"in": format!("g{}", i - 1)}})));
    let ir = song(Value::Array(nodes), &format!("g{}", n - 1), json!([]));
    let pk = std_packages();
    let flat = check(&ir, &pk).unwrap();
    let e = Renderer::new(&flat, &DataSet::empty(), &pk, SR).map(drop).unwrap_err();
    assert_eq!(
        format!("{e:#}"),
        "This song needs 4,000 sound parts that each run on their own (each effect or sound outside an instrument, and \
         each instrument), and music2 can play 3,999 at once. Remove some effects or tracks"
    );
    assert!(e.downcast_ref::<TooManySounds>().is_some());
}

// RENDER-32, RENDER-28: a sealed sandbox never grows, so its memory never moves and the address it
// keeps from its seal stays its memory's. This kernel copies its input to its output, and asks for
// one more page at every call: once sealed, the ask fails at once and says it ran out of memory, and
// the memory is where it was sealed, as big. A kernel that asks for nothing plays on.
#[test]
fn a_sealed_sandbox_never_grows() {
    let grows = kernel_v1("k", &format!("(drop (memory.grow (i32.const 1))) {}", out_v1("(local.get $x)")));
    let dir = package_with(&module_v1(&grows), "inputs = [\"in: audio<1>\"]\noutputs = [\"out: audio<1>\"]");
    let ir = song(
        json!([
            {"id": "f", "op": "const", "value": 440, "unit": "hz"},
            {"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "f"}},
            {"id": "k", "op": "kernel", "kernel": "t:k", "inputs": {"in": "o"}},
        ]),
        "k",
        json!([]),
    );
    let mut pk = PackageSet::default();
    pk.load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")).unwrap();
    pk.load(&dir).unwrap();
    let mut r = Renderer::new(&check(&ir, &pk).unwrap(), &DataSet::empty(), &pk, SR).unwrap();
    // Where the kernel's memory is and how big: what its store says, and what it kept.
    let at = |r: &Renderer| {
        let k = r.plan.graph.exec.iter().find_map(|e| match e {
            Exec::Kernel(KernelNode { inst: Some(k), .. }) if k.name == "song/k" => Some(k),
            _ => None,
        });
        let k = k.expect("the kernel");
        (k.own.as_ref().unwrap().memory_at(), k.own.as_ref().unwrap().size())
    };
    let sealed = at(&r);
    assert_eq!(sealed.0.0, sealed.0.1, "it keeps its memory's address");
    let e = r.render(0.01).map(drop).unwrap_err();
    assert_eq!(format!("{e:#}"), "song/k ran out of memory: a sound may use up to 256 MB");
    assert_eq!(at(&r), sealed, "its memory didn't grow or move");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn voices_end_when_their_envelope_closes_but_not_while_echoes_remain() {
    // saw → filter → amp: the oscillator never stops, but the voice is over once the
    // envelope has released.
    let synth = song(
        json!([
            {"id": "n", "op": "lane", "lane": "notes"},
            {"id": "v", "op": "poly", "events": "n", "voices": 4, "body": {"out": "amp", "nodes": [
                {"id": "p", "op": "lane", "lane": "pitch"},
                {"id": "g", "op": "lane", "lane": "gate"},
                {"id": "hz", "op": "kernel", "kernel": "std:mtof", "inputs": {"pitch": "p"}},
                {"id": "o", "op": "kernel", "kernel": "std:saw", "inputs": {"freq": "hz"}},
                {"id": "f", "op": "kernel", "kernel": "std:svf", "inputs": {"in": "o"}},
                {"id": "e", "op": "kernel", "kernel": "std:adsr", "inputs": {"gate": "g"}},
                {"id": "amp", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "f.lp", "by": "e"}}]}},
        ]),
        "v",
        json!([{"key": "notes", "kind": "events<note>"}]),
    );
    let (tl, dir) = timeline_for(
        &synth,
        &[("data/song/notes.json", json!({"clips": [{"start": 0, "length": 4, "notes": [{"pitch": 60, "start": 0, "length": 0.2}]}]}))],
    );
    let pk = std_packages();
    let mut r = Renderer::new(&check(&synth, &pk).unwrap(), &tl, &pk, SR).unwrap();
    r.render(0.05).unwrap();
    assert_eq!(r.active_voices(), 1);
    r.render(1.0).unwrap();
    assert_eq!(r.active_voices(), 0, "the voice ended after its release");
    std::fs::remove_dir_all(dir).ok();

    // The same short note through a delay inside the voice stays alive between echoes.
    let echo = song(
        json!([
            {"id": "n", "op": "lane", "lane": "notes"},
            {"id": "v", "op": "poly", "events": "n", "voices": 1, "body": {"out": "d", "nodes": [
                {"id": "p", "op": "lane", "lane": "pitch"},
                {"id": "g", "op": "lane", "lane": "gate"},
                {"id": "hz", "op": "kernel", "kernel": "std:mtof", "inputs": {"pitch": "p"}},
                {"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "hz"}},
                {"id": "a", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "o", "by": "g"}},
                {"id": "d", "op": "kernel", "kernel": "std:delay", "inputs": {"in": "a", "time": "t"}},
                {"id": "t", "op": "const", "value": 0.5, "unit": "seconds"}]}},
        ]),
        "v",
        json!([{"key": "notes", "kind": "events<note>"}]),
    );
    let (tl, dir) = timeline_for(
        &echo,
        &[("data/song/notes.json", json!({"clips": [{"start": 0, "length": 4, "notes": [{"pitch": 60, "start": 0, "length": 0.1}]}]}))],
    );
    let mut r = Renderer::new(&check(&echo, &pk).unwrap(), &tl, &pk, SR).unwrap();
    r.render(0.3).unwrap();
    assert_eq!(r.active_voices(), 1, "silent between echoes, but echoes are coming");
    r.render(20.0).unwrap();
    assert_eq!(r.active_voices(), 0, "the echoes died away");
    std::fs::remove_dir_all(dir).ok();
}

// ---------------------------------------------------------------- audio lanes

/// A song that plays one stereo audio lane, with the given assets and clips.
fn audio_song(assets: &[(&str, Vec<Vec<f32>>, u32)], clips: Value) -> (Ir, DataSet, PathBuf) {
    let ir = song(json!([{"id": "a", "op": "lane", "lane": "clips"}]), "a", json!([{"key": "clips", "kind": "audio<2>"}]));
    let dir = project(&[
        ("data/arrangement.json", json!({"tempo": [{"beat": 0, "bpm": 120}]})),
        ("data/song/clips.json", json!({"clips": clips})),
    ]);
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    for (name, samples, sr) in assets {
        audio::write_wav(&dir.join("assets").join(name), samples, *sr).unwrap();
    }
    // Read the data again, now that the assets are there.
    let tl = data(&ir, &dir);
    (ir, tl, dir)
}

fn render_audio(ir: &Ir, tl: &DataSet, seconds: f64) -> Vec<[f32; 2]> {
    let pk = std_packages();
    let flat = check(ir, &pk).unwrap_or_else(|e| panic!("{e:?}"));
    Renderer::new(&flat, tl, &pk, SR).unwrap().render(seconds).unwrap()
}

fn sine(hz: f64, seconds: f64, sr: u32) -> Vec<f32> {
    (0..(seconds * sr as f64) as usize).map(|i| (i as f64 / sr as f64 * hz * std::f64::consts::TAU).sin() as f32 * 0.5).collect()
}

/// Frequency from upward zero crossings.
fn freq(x: &[[f32; 2]]) -> f64 {
    let n = x.windows(2).filter(|w| w[0][0] < 0.0 && w[1][0] >= 0.0).count();
    n as f64 / (x.len() as f64 / SR as f64)
}

#[test]
fn clips_land_on_their_sample_with_gain_and_fades() {
    let dc = vec![vec![0.5f32; 48_000]];
    let (ir, tl, dir) =
        audio_song(&[("dc.wav", dc, SR)], json!([{"start": 1, "length": 1, "asset": "dc.wav", "gain_db": -6, "fade_out": 0.1}]));
    let out = render_audio(&ir, &tl, 1.5);
    // 120 bpm: beat 1 is sample 24 000; the clip lasts one beat (24 000 samples).
    assert_eq!(out[23_999], [0.0, 0.0]);
    let g = 0.5 * 10f32.powf(-6.0 / 20.0);
    assert!((out[24_000][0] - g).abs() < 1e-6 && out[24_000][0] == out[24_000][1], "mono assets play on both channels");
    assert!((out[30_000][0] - g).abs() < 1e-6);
    assert!(out[47_990][0] < 0.01 * g, "faded out by the end");
    assert_eq!(out[48_000], [0.0, 0.0]);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn pitch_without_warp_is_varispeed_and_with_warp_keeps_timing() {
    let (ir, tl, dir) = audio_song(
        &[("a440.wav", vec![sine(440.0, 3.0, SR)], SR)],
        json!([
            {"start": 0, "length": 2, "asset": "a440.wav", "pitch": 12},
            {"start": 4, "length": 2, "asset": "a440.wav", "warp": [[0, 0], [2, 2]]},
            {"start": 8, "length": 2, "asset": "a440.wav", "warp": [[0, 0], [2, 2]], "pitch": 12}
        ]),
    );
    let out = render_audio(&ir, &tl, 5.0);
    let s = |a: f64, b: f64| &out[(a * SR as f64) as usize..(b * SR as f64) as usize];
    // Varispeed: an octave up is twice the frequency.
    assert!((freq(s(0.1, 0.9)) - 880.0).abs() < 5.0, "{}", freq(s(0.1, 0.9)));
    // Warped: two seconds of source squeezed into one (two beats at 120 bpm), same pitch.
    assert!((freq(s(2.1, 2.9)) - 440.0).abs() < 8.0, "{}", freq(s(2.1, 2.9)));
    assert!(s(2.1, 2.9).iter().all(|f| f[0].abs() <= 0.6));
    // Warped and transposed: same timing, an octave up.
    assert!((freq(s(4.1, 4.9)) - 880.0).abs() < 12.0, "{}", freq(s(4.1, 4.9)));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn reverse_and_other_sample_rates() {
    let ramp: Vec<f32> = (0..24_000).map(|i| i as f32 / 24_000.0).collect();
    let (ir, tl, dir) = audio_song(
        &[("ramp.wav", vec![ramp], SR), ("a441.wav", vec![sine(441.0, 2.0, 44_100)], 44_100)],
        json!([
            {"start": 0, "length": 1, "asset": "ramp.wav", "reverse": true},
            {"start": 2, "length": 2, "asset": "a441.wav"}
        ]),
    );
    let out = render_audio(&ir, &tl, 2.0);
    assert!(out[100][0] > 0.99 && out[23_900][0] < 0.01, "plays backwards");
    assert!((freq(&out[48_000 + 4_800..96_000 - 4_800]) - 441.0).abs() < 3.0, "a 44.1 kHz asset keeps its pitch at 48 kHz");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn a_bounce_placed_as_a_clip_reproduces_the_original() {
    // Render a synth, write it as an asset, play it back from an audio lane: identical.
    let synth = song(
        json!([
            {"id": "o", "op": "kernel", "kernel": "std:saw", "inputs": {"freq": "f"}},
            {"id": "f", "op": "const", "value": 110, "unit": "hz"},
            {"id": "x", "op": "kernel", "kernel": "std:svf", "inputs": {"in": "o"}},
        ]),
        "x.lp",
        json!([]),
    );
    let original = render(&synth, &DataSet::empty(), 1.0);
    let channels = vec![original.iter().map(|f| f[0]).collect::<Vec<f32>>(), original.iter().map(|f| f[1]).collect()];
    let (ir, tl, dir) = audio_song(&[("bounce.wav", channels, SR)], json!([{"start": 0, "length": 2, "asset": "bounce.wav"}]));
    let played = render_audio(&ir, &tl, 1.0);
    assert!(played == original, "the bounce plays back bit for bit");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn audio_lanes_seek_exactly() {
    let (ir, tl, dir) = audio_song(
        &[("a440.wav", vec![sine(440.0, 3.0, SR)], SR)],
        json!([{"start": 0.5, "length": 3, "asset": "a440.wav", "warp": [[0, 0], [3, 3]], "fade_in": 0.05}]),
    );
    let pk = std_packages();
    let flat = check(&ir, &pk).unwrap();
    let mut r = Renderer::new(&flat, &tl, &pk, SR).unwrap();
    let full = r.render(2.0).unwrap();
    r.seek(30_011).unwrap();
    let part = r.render(0.5).unwrap();
    assert!(part[..] == full[30_011..30_011 + part.len()]);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
#[ignore = "prints measurements; run with --ignored --nocapture"]
fn stretch_quality_report() {
    let (ir, tl, dir) = audio_song(
        &[("a440.wav", vec![sine(440.0, 3.0, SR)], SR)],
        json!([
            {"start": 0, "length": 2, "asset": "a440.wav", "warp": [[0, 0], [2, 2]]},
            {"start": 4, "length": 2, "asset": "a440.wav", "warp": [[0, 0], [1, 2]]},
            {"start": 8, "length": 2, "asset": "a440.wav", "warp": [[0, 0], [2, 2]], "pitch": 12}
        ]),
    );
    let out = render_audio(&ir, &tl, 5.0);
    for (name, a) in [("2x faster", 0.1), ("2x slower", 2.1), ("faster + octave", 4.1)] {
        let s = &out[(a * SR as f64) as usize..((a + 0.8) * SR as f64) as usize];
        // Level per 10 ms window: a clean stretch of a sine keeps a constant level.
        let w: Vec<f32> = s.chunks(480).map(|c| c.iter().fold(0.0f32, |m, f| m.max(f[0].abs()))).collect();
        let (lo, hi) = (w.iter().cloned().fold(f32::MAX, f32::min), w.iter().cloned().fold(0.0, f32::max));
        println!("{name}: {:.1} Hz, level {lo:.3}..{hi:.3} ({:.1} dB ripple)", freq(s), 20.0 * (hi / lo).log10());
    }
    std::fs::remove_dir_all(dir).ok();
}

// FMT-15: a clip whose asset is missing is silent; the lane's other clips still play.
#[test]
fn a_missing_asset_silences_only_its_clip() {
    let dc = vec![vec![0.5f32; 48_000]];
    let (ir, tl, dir) = audio_song(
        &[("dc.wav", dc, SR)],
        json!([{"id": "a", "start": 0, "length": 1, "asset": "gone.wav"}, {"id": "b", "start": 1, "length": 1, "asset": "dc.wav"}]),
    );
    let out = render_audio(&ir, &tl, 1.0);
    assert_eq!(out[1_000], [0.0, 0.0], "the missing asset's clip is silent");
    assert_eq!(out[30_000], [0.5, 0.5], "the other clip plays");
    std::fs::remove_dir_all(dir).ok();
}

// ---------------------------------------------------------------- assets kernels read (ABI v1)

/// A package whose kernel `t:k`, with no input and one mono output, plays channel 0 of its first
/// asset through `m2.asset_read`, one call a process call, written straight into its output. Its
/// state at 4096 holds its settings block's address and the frame it is at.
fn asset_player() -> PathBuf {
    let k = r#"
  (func (export "k_new") (param $cfg i32) (result i32)
    (i32.store (i32.const 4096) (local.get $cfg)) (i32.store (i32.const 4100) (i32.const 0)) (i32.const 4096))
  (func (export "k_ports") (result i32) (i32.const 1))
  (func (export "k_reset") (param $s i32) (i32.store offset=4 (local.get $s) (i32.const 0)))
  (func (export "k_active") (param i32) (result i32) (i32.const 0))
  (func (export "k_process") (param $s i32) (param $io i32) (param $from i32) (param $to i32)
    (drop (call $read
      (i32.load (i32.load offset=36 (i32.load (local.get $s))))
      (i32.const 0)
      (i32.load offset=4 (local.get $s))
      (i32.add (i32.load (local.get $io)) (i32.shl (local.get $from) (i32.const 2)))
      (i32.sub (local.get $to) (local.get $from))))
    (i32.store offset=4 (local.get $s) (i32.add (i32.load offset=4 (local.get $s)) (i32.sub (local.get $to) (local.get $from)))))"#;
    let import = r#"(module (import "m2" "asset_read" (func $read (param i32 i32 i32 i32 i32) (result i32)))"#;
    package(&module_v1(k).replacen("(module", import, 1))
}

/// A song whose output is `t:k` of [`asset_player`] reading `names`, in a project whose `assets/`
/// holds `files` (mono, at the song's rate): the song, its packages, its data, the project and
/// the package.
fn asset_song(names: &[&str], files: &[(&str, Vec<f32>)]) -> (Ir, PackageSet, DataSet, PathBuf, PathBuf) {
    let ir = song(json!([{"id": "k", "op": "kernel", "kernel": "t:k", "assets": names}]), "k", json!([]));
    let pkg = asset_player();
    let mut pk = PackageSet::default();
    pk.load(&pkg).unwrap();
    let dir = project(&[]);
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    for (name, samples) in files {
        audio::write_wav(&dir.join("assets").join(name), std::slice::from_ref(samples), SR).unwrap();
    }
    let tl = data(&ir, &dir);
    (ir, pk, tl, dir, pkg)
}

// Kernel ABI v1 (ABI-2): a kernel reads the project's audio files through its one import. This one
// plays its asset's own samples, bit for bit, then silence past its end; and the revision records
// the hash of the file it reads (RENDER-2).
#[test]
fn a_kernel_reads_its_asset_through_the_import() {
    let tone = sine(440.0, 0.5, SR);
    let (ir, pk, tl, dir, pkg) = asset_song(&["tone.wav"], &[("tone.wav", tone.clone())]);
    let out = Renderer::new(&check(&ir, &pk).unwrap(), &tl, &pk, SR).unwrap().render(0.6).unwrap();
    let n = tone.len();
    assert!(out[..n].iter().zip(&tone).all(|(o, t)| o[0] == *t), "it plays its asset's own samples");
    assert!(out[n..].iter().all(|o| o[0] == 0.0), "and silence past its end");
    let sha = m2_song::Sha::of(&std::fs::read(dir.join("assets/tone.wav")).unwrap());
    assert_eq!(tl.kernel_assets().get("tone.wav"), Some(&sha), "the revision records its hash");
    for d in [dir, pkg] {
        std::fs::remove_dir_all(d).ok();
    }
}

// RENDER-28: a read through `m2.asset_read` allocates nothing: the asset and the memory are found
// by index in the sandbox's own data, never by name.
#[test]
fn an_asset_read_allocates_nothing() {
    let (ir, pk, tl, dir, pkg) = asset_song(&["tone.wav"], &[("tone.wav", sine(440.0, 0.5, SR))]);
    let mut r = Renderer::new(&check(&ir, &pk).unwrap(), &tl, &pk, SR).unwrap();
    r.render_block(0).unwrap();
    let (peak, allocs) =
        alloc_count::during(|| (1..100u64).map(|b| r.render_block(b * BLOCK as u64).unwrap()[0][0].abs()).fold(0.0f32, f32::max));
    assert_eq!(allocs, 0, "no heap operation in 99 blocks of reads");
    assert!(peak > 0.1, "it read its asset");
    for d in [dir, pkg] {
        std::fs::remove_dir_all(d).ok();
    }
}

// RENDER-25: a kernel's assets are part of its shape, by their hashes, so a new sample under the
// same name makes its box new; the same file keeps it.
#[test]
fn a_new_sample_makes_the_box_new() {
    let (ir, pk, tl, dir, pkg) = asset_song(&["hit.wav"], &[("hit.wav", sine(440.0, 0.5, SR))]);
    let flat = check(&ir, &pk).unwrap();
    let info = Renderer::new(&flat, &tl, &pk, SR).unwrap().info();
    let kept = |now: &DataSet| {
        let (cont, restarts) = (Continues::identity(&flat), HashSet::new());
        let keep = Keep { old: &flat, old_data: &tl, info: &info, cont: &cont, restarts: &restarts };
        let owner = m2_plugins::Owner::discard();
        let r = Renderer::prepare(&flat, now, &pk, SR, &mut audio::Cache::default(), &owner, Basis::Keep(keep)).unwrap();
        (r.stats().kept.kernels, r.stats().fresh.kernels)
    };
    assert_eq!(kept(&tl), (1, 0), "the same sample keeps its box");
    audio::write_wav(&dir.join("assets/hit.wav"), &[sine(220.0, 0.7, SR)], SR).unwrap();
    assert_eq!(kept(&data(&ir, &dir)), (0, 1), "a new sample under the same name makes it new");
    for d in [dir, pkg] {
        std::fs::remove_dir_all(d).ok();
    }
}

// FMT-15 for kernels: an asset a kernel reads that isn't in assets/ is `asset.missing`, naming it
// and the track; the revision records no hash for it, and its kernel reads silence while the
// render goes on.
#[test]
fn a_missing_asset_is_named_and_silent() {
    let (ir, pk, _, dir, pkg) = asset_song(&["gone.wav"], &[]);
    let (tl, diags) = Project::open(&dir).unwrap().data(&decls(&ir), None);
    let d = diags.iter().find(|d| d.code == "asset.missing").expect("it is named");
    assert!(d.message.starts_with("`gone.wav` isn't in assets/, so the sound on track `song`"), "{}", d.message);
    assert_eq!(d.module.as_deref(), Some("song"));
    assert!(tl.kernel_assets().is_empty());
    let out = Renderer::new(&check(&ir, &pk).unwrap(), &tl, &pk, SR).unwrap().render(0.1).unwrap();
    assert!(out.iter().all(|o| *o == [0.0, 0.0]), "it reads silence");
    for d in [dir, pkg] {
        std::fs::remove_dir_all(d).ok();
    }
}

// FMT-8a: overlapping clips sum in canonical order, so permuting the file's clips renders the
// same bits.
#[test]
fn permuting_audio_clips_renders_the_same_bits() {
    let a = vec![sine(440.0, 1.0, SR)];
    let b = vec![sine(523.0, 1.0, SR)];
    let clips = [
        json!({"id": "x", "start": 0, "length": 1.5, "asset": "a.wav", "gain_db": -1.3}),
        json!({"id": "y", "start": 0, "length": 1.5, "asset": "b.wav", "gain_db": -2.1}),
        json!({"id": "z", "start": 0.25, "length": 1, "asset": "a.wav", "pitch": 7}),
    ];
    let render = |order: [usize; 3]| {
        let (ir, tl, dir) = audio_song(&[("a.wav", a.clone(), SR), ("b.wav", b.clone(), SR)], json!(order.map(|i| clips[i].clone())));
        let out = render_audio(&ir, &tl, 1.0);
        std::fs::remove_dir_all(dir).ok();
        out
    };
    let first = render([0, 1, 2]);
    for order in [[2, 1, 0], [1, 0, 2], [2, 0, 1]] {
        assert!(render(order) == first, "{order:?}");
    }
}

// ID-8, FMT-17: the declarations data is read against, from the IR.
#[test]
fn declarations_from_the_ir() {
    let ir: Ir = serde_json::from_value(json!({"ir": m2_ir::IR_VERSION, "root": {"module": "song", "export": "out"}, "modules": [
        {"key": "song", "lanes": [{"key": "pan", "kind": "control<bipolar>", "default": 0}], "nodes": [], "exports": {}},
        {"key": "vox", "track": true, "lanes": [{"key": "takes", "kind": "audio<2>"}, {"key": "notes", "kind": "events<note>"}], "exports": {},
         "nodes": [{"id": "p", "op": "plugin", "plugin": "clap:x", "key": "tune", "frames": ["chain-l"]},
                   {"id": "q", "op": "plugin", "plugin": "clap:y"}]}]}))
    .unwrap();
    let d = decls(&ir);
    let lanes: Vec<(String, m2_song::LaneKind)> = d.lanes.iter().map(|(a, k)| (a.to_string(), k.clone())).collect();
    assert_eq!(
        lanes,
        [
            ("song/pan".to_string(), m2_song::LaneKind::Control { unit: Some("bipolar".into()) }),
            ("vox/takes".to_string(), m2_song::LaneKind::Audio),
            ("vox/notes".to_string(), m2_song::LaneKind::Notes),
        ]
    );
    let states: Vec<(String, Vec<String>, String)> =
        d.states.iter().map(|s| (s.module.to_string(), s.path.iter().map(|k| k.to_string()).collect(), s.plugin.clone())).collect();
    assert_eq!(states, [("vox".to_string(), vec!["chain-l".to_string(), "tune".to_string()], "clap:x".to_string())]);
}

// FMT-14: a new asset is created, never written over one that exists.
#[test]
fn a_new_asset_never_replaces_one() {
    let dir = project(&[]);
    let path = dir.join("take.wav");
    std::fs::write(&path, b"mine").unwrap();
    assert!(audio::write_new_wav(&path, &[vec![0.0; 4]], SR).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"mine");
    let new = dir.join("take 2.wav");
    audio::write_new_wav(&new, &[vec![0.5; 4]], SR).unwrap();
    assert_eq!(audio::load_wav(&new).unwrap().channels, vec![vec![0.5; 4]]);
    std::fs::remove_dir_all(dir).ok();
}

// FMT-18a: a plugin that isn't installed silences only its own node: an effect passes its
// input through, an instrument plays silence, and everything else renders the same bits.
#[test]
fn a_missing_plugin_silences_only_its_node() {
    let sine = json!({"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {}});
    let alone = song(json!([sine.clone()]), "o", json!([]));
    let with = |plugin: Value| {
        let add = json!({"id": "s", "op": "kernel", "kernel": "std:add", "inputs": {"a": "o", "b": "p"}});
        song(json!([sine.clone(), plugin, add]), "s", json!([]))
    };
    let effect = song(
        json!([sine.clone(), {"id": "m", "op": "const", "value": 0.5},
            {"id": "p", "op": "plugin", "plugin": "clap:org.example.gone-fx", "inputs": {"in": "o", "mix_3": "m"}},
            {"id": "s", "op": "kernel", "kernel": "std:add", "inputs": {"a": "o", "b": "p"}}]),
        "s",
        json!([]),
    );
    let instrument = with(json!({"id": "p", "op": "plugin", "plugin": "clap:org.example.gone-synth", "inputs": {}}));
    let mut pk = std_packages();
    let render = |ir: &Ir, pk: &mut PackageSet| {
        let mut ir = ir.clone();
        let diags = pk.bind_plugins(&mut ir);
        assert!(diags.iter().all(|d| d.code == "plugin.missing"), "{diags:?}");
        let flat = check(&ir, &*pk).unwrap_or_else(|e| panic!("{e:?}"));
        Renderer::new(&flat, &DataSet::empty(), pk, SR).unwrap().render(0.25).unwrap()
    };
    let base = render(&alone, &mut pk);
    let doubled: Vec<[f32; 2]> = base.iter().map(|f| [f[0] + f[0], f[1] + f[1]]).collect();
    assert_eq!(render(&effect, &mut pk), doubled, "the effect passes the sine through");
    assert_eq!(render(&instrument, &mut pk), base, "the instrument adds silence");
}

// ---------------------------------------------------------------- goldens

// RENDER-1, RENDER-2: the offline bits of songs that stress what the live engine rewrote (step
// 3a): voices freed exactly on a block line, stealing, notes inside a block, feedback inside
// voices, control lanes and time across a tempo change, two consumers of one notes lane, and
// uneven device buffers. The hashes were recorded at 0ff97b4, before the rewrite; the offline
// path must never change them (SHA-256 of the little-endian f32 bits of the stereo frames).
mod goldens {
    use m2_song::{Arrangement, LaneAddr, LaneData, LaneKind};

    use super::*;

    fn lane(text: Value, kind: LaneKind) -> LaneData {
        LaneData::read(&text.to_string(), &kind).unwrap()
    }

    pub(super) fn addr(l: &str) -> LaneAddr {
        LaneAddr::new("song", l).unwrap()
    }

    pub(super) fn bits(frames: &[[f32; 2]]) -> String {
        let bytes: Vec<u8> = frames.iter().flatten().flat_map(|x| x.to_le_bytes()).collect();
        m2_song::Sha::of(&bytes).hex()
    }

    /// `frames` of `ir` playing `data`, asked for in buffers of `sizes` in turn.
    pub(super) fn hash(ir: &Ir, data: &DataSet, frames: usize, sizes: &[usize]) -> String {
        let pk = std_packages();
        let flat = check(ir, &pk).unwrap_or_else(|e| panic!("{e:?}"));
        let mut r = Renderer::new(&flat, data, &pk, SR).unwrap();
        let mut out = vec![[0.0f32; 2]; frames];
        let mut done = 0;
        for &n in sizes.iter().cycle() {
            if done == frames {
                break;
            }
            let n = n.min(frames - done);
            r.process(&mut out[done..done + n]).unwrap();
            done += n;
        }
        bits(&out)
    }

    const NOTES: &str = r#"[{"key": "notes", "kind": "events<note>"}]"#;

    /// G1: two voices whose notes end exactly on a block line (beat 0.32 is sample 7680, 60
    /// blocks at 120 bpm), quiet but still gated (sustain 0), then a note on that line.
    pub(super) fn g1() -> (Ir, DataSet) {
        let ir = song(
            json!([
                {"id": "n", "op": "lane", "lane": "notes"},
                {"id": "v", "op": "poly", "events": "n", "voices": 2, "body": {"out": "amp", "nodes": [
                    {"id": "p", "op": "lane", "lane": "pitch"}, {"id": "g", "op": "lane", "lane": "gate"},
                    {"id": "hz", "op": "kernel", "kernel": "std:mtof", "inputs": {"pitch": "p"}},
                    {"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "hz"}},
                    {"id": "e", "op": "kernel", "kernel": "std:adsr", "inputs": {"gate": "g", "decay": "d", "sustain": "s", "release": "r"}},
                    {"id": "d", "op": "const", "value": 0.05, "unit": "seconds"},
                    {"id": "s", "op": "const", "value": 0},
                    {"id": "r", "op": "const", "value": 0.03, "unit": "seconds"},
                    {"id": "amp", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "o", "by": "e"}}]}}]),
            "v",
            serde_json::from_str(NOTES).unwrap(),
        );
        let notes = lane(
            json!({"clips": [{"id": "a", "start": 0, "length": 4, "notes": [
                {"pitch": 45, "start": 0, "length": 0.32}, {"pitch": 57, "start": 0.1, "length": 0.22},
                {"pitch": 52, "start": 0.32, "length": 0.32}, {"pitch": 60, "start": 1, "length": 0.5}]}]}),
            LaneKind::Notes,
        );
        (ir, DataSet::with_lanes(Arrangement::default(), [(addr("notes"), notes)]))
    }

    /// G2: a voice with a feedback loop, a delay after the poly, notes at odd samples, a
    /// stolen voice and a note that starts and ends inside one block.
    pub(super) fn g2() -> (Ir, DataSet) {
        let ir = song(
            json!([
                {"id": "n", "op": "lane", "lane": "notes"},
                {"id": "v", "op": "poly", "events": "n", "voices": 2, "body": {"out": "mix", "nodes": [
                    {"id": "p", "op": "lane", "lane": "pitch"}, {"id": "g", "op": "lane", "lane": "gate"},
                    {"id": "hz", "op": "kernel", "kernel": "std:mtof", "inputs": {"pitch": "p"}},
                    {"id": "o", "op": "kernel", "kernel": "std:saw", "inputs": {"freq": "hz"}},
                    {"id": "e", "op": "kernel", "kernel": "std:adsr", "inputs": {"gate": "g"}},
                    {"id": "a", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "o", "by": "e"}},
                    {"id": "fb", "op": "feedback", "kind": "audio<1>", "input": "mix"},
                    {"id": "q", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "fb", "by": "h"}},
                    {"id": "h", "op": "const", "value": 0.3},
                    {"id": "mix", "op": "kernel", "kernel": "std:add", "inputs": {"a": "a", "b": "q"}}]}},
                {"id": "dl", "op": "kernel", "kernel": "std:delay", "inputs": {"in": "v"}}]),
            "dl",
            serde_json::from_str(NOTES).unwrap(),
        );
        let notes = lane(
            json!({"clips": [{"id": "a", "start": 0, "length": 4, "notes": [
                {"pitch": 57, "start": 0.0013, "length": 0.3}, {"pitch": 64, "start": 0.0071, "length": 0.2},
                {"pitch": 69, "start": 0.0109, "length": 0.5}, {"pitch": 60, "start": 0.61, "length": 0.05},
                {"pitch": 62, "start": 0.7, "length": 0.002}]}]}),
            LaneKind::Notes,
        );
        (ir, DataSet::with_lanes(Arrangement::default(), [(addr("notes"), notes)]))
    }

    /// G3: control lane automation, `time.beats` into an lfo, a top-level feedback loop, and a
    /// tempo change mid-song.
    pub(super) fn g3() -> (Ir, DataSet) {
        let ir = song(
            json!([
                {"id": "c", "op": "lane", "lane": "cutoff"},
                {"id": "t", "op": "time", "of": "beats"},
                {"id": "l", "op": "kernel", "kernel": "std:lfo", "inputs": {"beats": "t"}},
                {"id": "x", "op": "kernel", "kernel": "std:saw"},
                {"id": "f", "op": "kernel", "kernel": "std:svf", "inputs": {"in": "x", "cutoff": "c"}},
                {"id": "m", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "f.lp", "by": "l"}},
                {"id": "fb", "op": "feedback", "kind": "audio<1>", "input": "sum"},
                {"id": "k", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "fb", "by": "h"}},
                {"id": "h", "op": "const", "value": 0.5},
                {"id": "sum", "op": "kernel", "kernel": "std:add", "inputs": {"a": "m", "b": "k"}}]),
            "sum",
            json!([{"key": "cutoff", "kind": "control<hz>", "default": 2000}]),
        );
        let cutoff = lane(
            json!({"unit": "hz", "clips": [{"id": "a", "start": 0.5, "length": 3, "points": [[0, 300], [1.5, 4000], [3, 800]]}]}),
            LaneKind::Control { unit: Some("hz".into()) },
        );
        let arr: Arrangement =
            serde_json::from_value(json!({"tempo": [{"beat": 0, "bpm": 127}, {"beat": 2, "bpm": 90}], "meter": []})).unwrap();
        (ir, DataSet::with_lanes(arr, [(addr("cutoff"), cutoff)]))
    }

    /// G4: two polys reading one notes lane (one with a single voice, so it steals), velocity,
    /// and a note cut by its clip.
    pub(super) fn g4() -> (Ir, DataSet) {
        let ir = song(
            json!([
                {"id": "n", "op": "lane", "lane": "notes"},
                {"id": "a", "op": "poly", "events": "n", "voices": 1, "body": {"out": "amp", "nodes": [
                    {"id": "p", "op": "lane", "lane": "pitch"}, {"id": "g", "op": "lane", "lane": "gate"},
                    {"id": "hz", "op": "kernel", "kernel": "std:mtof", "inputs": {"pitch": "p"}},
                    {"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "hz"}},
                    {"id": "amp", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "o", "by": "g"}}]}},
                {"id": "b", "op": "poly", "events": "n", "voices": 3, "body": {"out": "amp", "nodes": [
                    {"id": "p", "op": "lane", "lane": "pitch"}, {"id": "g", "op": "lane", "lane": "gate"},
                    {"id": "v", "op": "lane", "lane": "velocity"},
                    {"id": "hz", "op": "kernel", "kernel": "std:mtof", "inputs": {"pitch": "p"}},
                    {"id": "o", "op": "kernel", "kernel": "std:saw", "inputs": {"freq": "hz"}},
                    {"id": "e", "op": "kernel", "kernel": "std:adsr", "inputs": {"gate": "g"}},
                    {"id": "ev", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "e", "by": "v"}},
                    {"id": "amp", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "o", "by": "ev"}}]}},
                {"id": "s", "op": "kernel", "kernel": "std:add", "inputs": {"a": "a", "b": "b"}}]),
            "s",
            serde_json::from_str(NOTES).unwrap(),
        );
        let notes = lane(
            json!({"clips": [{"id": "a", "start": 0, "length": 8, "notes": [
                {"pitch": 60, "start": 0, "length": 1, "velocity": 0.5}, {"pitch": 64, "start": 0.25, "length": 1, "velocity": 0.9},
                {"pitch": 67, "start": 0.5, "length": 0.75}, {"pitch": 72, "start": 0.5, "length": 0.25},
                {"pitch": 48, "start": 1.5, "length": 2}]},
                {"id": "b", "start": 2, "length": 2, "notes": [{"pitch": 55, "start": -0.5, "length": 1}, {"pitch": 59, "start": 1.5, "length": 1}]}]}),
            LaneKind::Notes,
        );
        (ir, DataSet::with_lanes(Arrangement::default(), [(addr("notes"), notes)]))
    }

    #[test]
    fn g1_voices_freed_on_a_block_line() {
        let (ir, data) = g1();
        assert_eq!(hash(&ir, &data, 48_000, &[48_000]), "484d8c0d4f8896633c23db4b3a0e58be665726cc774389aa357161ee84d15500");
    }

    #[test]
    fn g2_feedback_steals_and_short_notes() {
        let (ir, data) = g2();
        assert_eq!(
            hash(&ir, &data, 48_000, &[1, 7, 128, 300, 64, 1000]),
            "485c1a80c5ee76dcd0ccb8328ed17457f83b5d30b0ebd6625a2688aa2e6859da"
        );
    }

    #[test]
    fn g3_control_lanes_time_and_tempo() {
        let (ir, data) = g3();
        assert_eq!(hash(&ir, &data, 96_000, &[300, 1000, 7]), "1bc59f421c75899de576638ba94f0c7bb0bb275916abc9c89dbdc0a5d867a9fb");
    }

    #[test]
    fn g4_two_consumers_of_one_lane() {
        let (ir, data) = g4();
        assert_eq!(hash(&ir, &data, 120_000, &[512]), "4ff082cdac4ff3cc83151b1ddec22cabf3610ddb2fe877d0ae4303474142ff77");
    }
}
