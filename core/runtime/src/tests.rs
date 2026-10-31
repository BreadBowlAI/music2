use std::path::{Path, PathBuf};

use m2_ir::{Ir, check};
use m2_song::{DataSet, Project};
use serde_json::{Value, json};

use super::*;

mod audition;
mod cell;
mod decode;
mod events;
mod fpenv;
mod latency;
mod live;
mod overlay;
mod parts;
mod poly;
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
        static MOST: Cell<usize> = const { Cell::new(0) };
    }

    /// Count one heap operation, of `size` bytes asked for (0 to free).
    fn count(size: usize) {
        let _ = ON.try_with(|on| {
            if on.get() {
                let _ = N.try_with(|n| n.set(n.get() + 1));
                let _ = MOST.try_with(|m| m.set(m.get().max(size)));
            }
        });
    }

    unsafe impl GlobalAlloc for Counter {
        unsafe fn alloc(&self, l: Layout) -> *mut u8 {
            count(l.size());
            unsafe { System.alloc(l) }
        }

        unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
            count(0);
            unsafe { System.dealloc(p, l) }
        }

        unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
            count(n);
            unsafe { System.realloc(p, l, n) }
        }
    }

    /// What `f` returns, and the largest block it took from the heap on this thread, in bytes.
    pub fn largest<R>(f: impl FnOnce() -> R) -> (R, usize) {
        MOST.with(|m| m.set(0));
        let (r, _) = during(f);
        (r, MOST.with(|m| m.get()))
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
    KernelInst::new(pk, n, &Kernel { id, channels, inputs, values: &[], assets: &[], sample_rate, notes: &[] }, 0)
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
    let born = |sb: &mut sandbox::Sandbox| {
        sb.state(
            prefix,
            &sandbox::Spec {
                inputs: 4,
                outputs: 1,
                channels: &[1; 5],
                audio: 1,
                sample_rate: SR as f64,
                values: &[],
                assets: &[],
                offline: None,
                notes: &[],
            },
        )
        .unwrap()
    };
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

/// A package folder holding one hand-written kernel module, `@test/t:k`, its ports as `ports` declares.
fn package_with(wat: &str, ports: &str) -> PathBuf {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir =
        std::env::temp_dir().join(format!("m2-pkg-test-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("k.wasm"), wasm(wat)).unwrap();
    let manifest = format!("[package]\nname = \"@test/t\"\nversion = \"0.1.0\"\nwasm = \"k.wasm\"\n\n[[kernel]]\nname = \"k\"\n{ports}\n");
    std::fs::write(dir.join("m2pkg.toml"), manifest).unwrap();
    dir
}

/// A module's text as the bytes a package ships.
pub(crate) fn wasm(wat: &str) -> Vec<u8> {
    wat::parse_str(wat).unwrap()
}

/// A kernel module speaking ABI v`abi` around `kernels`, the exports of its kernel types: its
/// version in its `m2_abi` section, its one page of memory and its allocator (`m2_alloc`), which
/// hands out room forward from 8 KiB, growing the memory as it must, as its sandbox is born. Below
/// 8 KiB a hand-written kernel may keep one state of its own at a fixed place, as these tests do:
/// they make one state in a sandbox.
pub(crate) fn module_v(abi: u32, kernels: &str) -> String {
    let section: String = abi.to_le_bytes().iter().map(|b| format!("\\{b:02x}")).collect();
    format!(
        r#"(module
  (@custom "m2_abi" "{section}")
  (memory (export "memory") 1)
  (global $heap (mut i32) (i32.const 8192))
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

// ABI-3 (D1 of track sx): the version is read from the module's bytes, its `m2_abi` section,
// before the module is compiled. So a module built for a later ABI is refused by its version,
// whatever else it holds: here an import v1 doesn't know, and a 64-bit memory.
#[test]
fn a_later_abi_module_is_refused_by_its_version_first() {
    let v2 = module_v(2, &kernel_v1("k", &out_v1("(local.get $x)")))
        .replacen("(module", r#"(module (import "m2" "other" (func))"#, 1)
        .replace(r#"(memory (export "memory") 1)"#, r#"(memory (export "memory") i64 1)"#);
    let dir = package(&v2);
    let e = PackageSet::default().load(&dir).map(|_| ()).unwrap_err();
    assert!(format!("{e:#}").contains("built for kernel ABI v2, but this music2 speaks v1 only"), "{e:#}");
    std::fs::remove_dir_all(dir).ok();
}

// The version has one place (D1, D2): std.wasm says v1 in its `m2_abi` section, and has no
// `m2_abi()` export.
#[test]
fn std_wasm_says_v1_in_its_section() {
    let bytes = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std/kernels/std.wasm")).unwrap();
    assert_eq!(packages::abi_version(&bytes).unwrap(), Some(packages::KERNEL_ABI));
    let module = wasmtime::Module::new(&wasmtime::Engine::default(), &bytes).unwrap();
    assert!(module.exports().all(|e| e.name() != "m2_abi"), "no `m2_abi()` export");
}

// ABI-3 (D2): a module with no `m2_abi` section was built for kernel ABI v0, or isn't a music2
// kernel. It is refused, saying so and what to do, whatever it exports: an `m2_abi()` that
// returns 1 doesn't count.
#[test]
fn a_module_with_no_abi_section_is_refused_as_v0() {
    let v0 = r#"(module
      (memory (export "memory") 1)
      (func (export "m2_abi") (result i32) (i32.const 1))
      (func (export "k_init") (param f32 i32 i32))
      (func (export "k_ports") (result i32) (i32.const 1))
      (func (export "k_port") (param i32) (result i32) (i32.const 1024))
      (func (export "k_reset"))
      (func (export "k_process") (param i32))
      (func (export "k_active") (result i32) (i32.const 0)))"#;
    for module in [v0, "(module)"] {
        let dir = package(module);
        let e = PackageSet::default().load(&dir).map(|_| ()).unwrap_err();
        let said = "built for kernel ABI v0, or isn't a music2 kernel: it has no `m2_abi` section. Rebuild it with the v1 kit";
        assert!(format!("{e:#}").contains(said), "{e:#}");
        std::fs::remove_dir_all(dir).ok();
    }
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
            {"id": "k", "op": "kernel", "kernel": "@test/t:k", "inputs": {"in": "o"}},
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
    nodes.extend(
        (1..n).map(|i| json!({"id": format!("g{i}"), "op": "kernel", "kernel": "std:gain", "inputs": {"in": format!("g{}", i - 1)}})),
    );
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
            {"id": "k", "op": "kernel", "kernel": "@test/t:k", "inputs": {"in": "o"}},
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

/// The std package without its kernel `name` (a manifest entry cut out; the same module): its
/// packages, and the folder to remove after.
fn std_without(name: &str) -> (PackageSet, PathBuf) {
    let std = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std");
    let dir = project(&[]);
    std::fs::create_dir_all(dir.join("kernels")).unwrap();
    std::fs::copy(std.join("kernels/std.wasm"), dir.join("kernels/std.wasm")).unwrap();
    let text = std::fs::read_to_string(std.join("m2pkg.toml")).unwrap();
    let at = text.find(&format!("[[kernel]]\nname = \"{name}\"")).unwrap_or_else(|| panic!("std has {name}"));
    let end = text[at + 1..].find("[[kernel]]").map_or(text.len(), |e| at + 1 + e);
    std::fs::write(dir.join("m2pkg.toml"), format!("{}{}", &text[..at], &text[end..])).unwrap();
    let mut pk = PackageSet::default();
    pk.load(&dir).unwrap();
    (pk, dir)
}

// The sx plan, D7 and D9: a warped clip is stretched by the song's `std:stretch`, run off the graph
// to make its render: the lane plays the kernel's whole run at the clip's start, bit for bit. A std
// without it plays the clip silent, as the runtime has no stretch of its own, and the clip beside it
// still plays.
#[test]
fn a_warped_clip_is_stretched_by_std_stretch() {
    let src = sine(440.0, 3.0, SR);
    let (ir, tl, dir) = audio_song(
        &[("a440.wav", vec![src.clone()], SR)],
        json!([
            {"start": 1, "length": 3, "asset": "a440.wav", "warp": [[0, 0], [1, 1], [2.5, 3]], "pitch": 3, "fade_out": 0.05},
            {"start": 6, "length": 1, "asset": "a440.wav"}
        ]),
    );
    let out = render_audio(&ir, &tl, 3.5);
    let clip = tl.audio("song", "clips")[0].clone();
    let asset = Arc::new(audio::Asset { channels: vec![src], sample_rate: SR as f64 });
    let want = audio::render_clip(&clip, &asset, tl.tempo(), SR as f64, Some(&stream::stretch())).unwrap();
    // 120 bpm: beat 1 is frame 24 000; the clip is 3 beats, 72 000 frames.
    let (at, len) = (24_000, want[0].len());
    assert_eq!(len, 72_000);
    assert!(peak(&out[at..at + len]) > 0.3, "the warped clip plays");
    let first = (0..len).find(|&i| out[at + i] != [want[0][i]; 2]);
    assert_eq!(first, None, "the lane plays the kernel's run, bit for bit");
    let (pk, std) = std_without("stretch");
    let flat = check(&ir, &pk).unwrap();
    let without = Renderer::new(&flat, &tl, &pk, SR).unwrap().render(3.5).unwrap();
    assert!(without[at..at + len].iter().all(|f| *f == [0.0, 0.0]), "without std:stretch, the warped clip is silent");
    assert!(peak(&without[3 * SR as usize..]) > 0.3, "the plain clip beside it plays");
    for d in [dir, std] {
        std::fs::remove_dir_all(d).ok();
    }
}

// The sx plan, slice 2 (RENDER-2, RENDER-30): an export with an empty cache folder equals one with
// the renders already there, bit for bit: a render read back is the render made. The first cache
// made both of examples/resample's warped clips, the second only read them.
#[test]
fn an_export_with_an_empty_cache_folder_equals_one_with_a_full_one() {
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/resample");
    let clips: Value = serde_json::from_str(&std::fs::read_to_string(example.join("data/chops/clips.json")).unwrap()).unwrap();
    let (ir, _, dir) = audio_song(&[], clips["clips"].clone());
    std::fs::copy(example.join("assets/keys-bounce.wav"), dir.join("assets/keys-bounce.wav")).unwrap();
    let tl = data(&ir, &dir);
    let pk = std_packages();
    let flat = check(&ir, &pk).unwrap();
    let folder = dir.join("renders");
    let export = || {
        let mut cache = audio::Cache::with_renders_in(Some(folder.clone()));
        let owner = m2_plugins::Owner::discard();
        let mut r = Renderer::prepare(&flat, &tl, &pk, SR, &mut cache, &owner, Basis::Fresh).unwrap();
        (r.render(8.5).unwrap(), cache.work().1)
    };
    let (cold, made) = export();
    let (warm, remade) = export();
    assert_eq!((made, remade), (2, 0), "the first export made the two renders, the second read them");
    assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 2, "two renders in the folder, no part file left");
    // 120 bpm: the warped clips play from 3 s to 5 s and from 6 s.
    assert!(peak(&cold[3 * SR as usize..5 * SR as usize]) > 0.05 && peak(&cold[6 * SR as usize..]) > 0.05, "the warped clips play");
    assert!(cold == warm, "a full cache folder gives the same bits as an empty one");
    std::fs::remove_dir_all(dir).ok();
}

/// The finished renders in `folder`, and its part files.
fn renders_in(folder: &Path) -> (usize, usize) {
    let names: Vec<String> =
        std::fs::read_dir(folder).map_or(vec![], |d| d.map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect());
    let parts = names.iter().filter(|n| n.ends_with(".part")).count();
    (names.len() - parts, parts)
}

// The sx plan, slice 3 (RENDER-1, RENDER-30, D10): an export never plays a render half made. A live
// session's tempo change asks for a new stretch, and its stretch thread has made a second of it,
// into a part file in the cache folder, when an export starts there. The export's cache is offline:
// it makes its own render whole before it plays, so it equals an export with no live session and
// no cache folder.
#[test]
fn an_export_never_plays_a_render_half_made() {
    let src = sine(440.0, 3.0, SR);
    let (ir, tl, dir) = audio_song(
        &[("a.wav", vec![src.clone(), src], SR)],
        json!([{"start": 1, "length": 6, "asset": "a.wav", "warp": [[0, 0], [3, 6]]}]),
    );
    let pk = std_packages();
    let flat = check(&ir, &pk).unwrap();
    let (folder, owner) = (dir.join("renders"), m2_plugins::Owner::discard());
    let mut live = audio::Cache::live_in(folder.clone());
    live.hold_renders(Some(SR as usize));
    drop(Renderer::prepare(&flat, &tl, &pk, SR, &mut live, &owner, Basis::Fresh).unwrap());
    live.prune();
    // The tempo goes from 120 to 90 BPM: the clip plays from 2/3 s for 4 s.
    std::fs::write(dir.join("data/arrangement.json"), json!({"tempo": [{"beat": 0, "bpm": 90}]}).to_string()).unwrap();
    let slower = data(&ir, &dir);
    drop(Renderer::prepare(&flat, &slower, &pk, SR, &mut live, &owner, Basis::Fresh).unwrap());
    live.prune();
    assert!(live.settle_renders(std::time::Duration::from_secs(20)));
    assert_eq!(renders_in(&folder), (0, 1), "the new stretch is half made, in its part file");
    let export =
        |cache: &mut audio::Cache| Renderer::prepare(&flat, &slower, &pk, SR, cache, &owner, Basis::Fresh).unwrap().render(5.0).unwrap();
    let after = export(&mut audio::Cache::with_renders_in(Some(folder.clone())));
    let cold = export(&mut audio::Cache::with_renders_in(None));
    assert!(peak(&cold[SR as usize..4 * SR as usize]) > 0.2, "the warped clip plays");
    let first = (0..cold.len()).find(|&i| after[i] != cold[i]);
    assert_eq!(first, None, "the export beside a half-made render is the cold export, bit for bit");
    drop(live);
    assert_eq!(renders_in(&folder), (1, 0), "the export's render is whole in the folder; the live one's part went with its cache");
    std::fs::remove_dir_all(dir).ok();
}

// Review sx-r1 (RENDER-30, D10): a cache folder that can't be written never silences a warped clip.
// Its stretch is made in memory instead. An export plays it as one with no cache folder does, bit
// for bit; so does a live session, once its stretch is made. Nothing is left in the folder.
#[test]
fn a_cache_folder_that_cannot_be_written_still_plays_warped_clips() {
    let src = sine(440.0, 3.0, SR);
    let (ir, tl, dir) = audio_song(
        &[("a.wav", vec![src.clone(), src], SR)],
        json!([{"start": 1, "length": 6, "asset": "a.wav", "warp": [[0, 0], [3, 6]]}]),
    );
    let pk = std_packages();
    let flat = check(&ir, &pk).unwrap();
    let (folder, owner) = (dir.join("renders"), m2_plugins::Owner::discard());
    std::fs::create_dir_all(&folder).unwrap();
    let read_only = |on: bool| {
        let mut p = std::fs::metadata(&folder).unwrap().permissions();
        p.set_readonly(on);
        std::fs::set_permissions(&folder, p).unwrap();
    };
    read_only(true);
    let play = |cache: &mut audio::Cache| {
        let mut r = Renderer::prepare(&flat, &tl, &pk, SR, cache, &owner, Basis::Fresh).unwrap();
        assert!(cache.settle_renders(std::time::Duration::from_secs(60)));
        r.render(5.0).unwrap()
    };
    let cold = play(&mut audio::Cache::with_renders_in(None));
    assert!(peak(&cold[SR as usize..4 * SR as usize]) > 0.2, "the warped clip plays");
    let export = play(&mut audio::Cache::with_renders_in(Some(folder.clone())));
    assert!(export == cold, "an export whose cache folder is read-only plays the clip, as one with no folder");
    let live = play(&mut audio::Cache::live_in(folder.clone()));
    assert!(live == cold, "so does a live session");
    assert_eq!(renders_in(&folder), (0, 0), "nothing in the folder");
    read_only(false);
    std::fs::remove_dir_all(dir).ok();
}

// Review sx-r1, RENDER-1: offline, a stretch that fails fails the build, naming its clip, so an
// export never finishes without it: its error is the fault `stretch.failed` about that clip, which
// the session names. Here the stretch kernel traps. Its part file goes. Live, the clip is silent
// alone, the lane is built, and it reports `stretch.failed` as it plays the clip.
#[test]
fn an_export_whose_stretch_fails_names_the_clip() {
    let trap = r#"(func (export "k_new") (param i32) (result i32) (i32.const 4096))
        (func (export "k_ports") (result i32) (i32.const 1))
        (func (export "k_reset") (param i32))
        (func (export "k_active") (param i32) (result i32) (i32.const 0))
        (func (export "k_process") (param i32 i32 i32 i32) unreachable)"#;
    let pkg = package_with(&module_v1(trap), "outputs = [\"out: audio<2>\"]\noffline = true");
    let mut pk = PackageSet::default();
    pk.load(&pkg).unwrap();
    let k = offline::Kernel::of(&pk, "@test/t:k").expect("an offline kernel");
    let dir = project(&[]);
    audio::write_wav(&dir.join("a.wav"), &[sine(440.0, 1.0, SR)], SR).unwrap();
    let clip: m2_song::AudioClip =
        serde_json::from_value(json!({"start": 2, "length": 4, "asset": "a.wav", "warp": [[0, 0], [1, 4]]})).unwrap();
    let tempo = m2_song::TempoMap::constant(120.0);
    let folder = dir.join("renders");
    let build = |cache: &mut audio::Cache| {
        audio::AudioLane::build_with(std::slice::from_ref(&clip), &|a| Some(dir.join(a)), cache, &tempo, SR as f64, 2, Some(&k))
    };
    let e = build(&mut audio::Cache::with_renders_in(Some(folder.clone()))).map(|_| ()).expect_err("the export fails");
    let e = format!("{e:#}");
    assert!(e.starts_with("the warped clip of a.wav at beat 2 can't be stretched: ") && e.contains("crashed"), "{e}");
    let mut cache = audio::Cache::with_renders_in(Some(folder.clone()));
    let e = build(&mut cache).map(drop).unwrap_err();
    let fault = e.downcast_ref::<crate::fault::Failed>().map(|f| (f.fault.code, f.fault.subject));
    let clip = crate::fault::Subject::Clip { lane: 0, clip: 0 };
    assert_eq!(fault, Some((Code::StretchFailed, clip)), "the fault, with its code");
    assert_eq!(renders_in(&folder), (0, 0), "its part file went");
    let mut live = audio::Cache::live_in(folder.clone());
    let lane = build(&mut live).expect("live, the lane is built");
    assert!(live.settle_renders(std::time::Duration::from_secs(20)));
    assert_eq!(renders_in(&folder), (0, 0));
    // The chunk that plays it: what a player's disk thread fills.
    let k = lane.frame_of(2.0) / lane.chunk();
    assert!(lane.can_fill(k) && lane.fill_chunk(k, &|_| true));
    let told: Vec<_> = live.faults().drain().into_iter().map(|f| (f.code, f.subject)).collect();
    assert_eq!(told, [(Code::StretchFailed, clip)], "live, told once");
    assert!(lane.fill_chunk(k + 1, &|_| true) && live.faults().take().is_none(), "once");
    drop((lane, live));
    for d in [dir, pkg] {
        std::fs::remove_dir_all(d).ok();
    }
}

// The sx plan, slice 3 (D10): a render the cache's next prune drops stops, and leaves no part file;
// one a kept lane plays goes on. Two warped clips are asked for, the first held at a second made,
// the second waiting behind it, each in its part file. A rebuild of the same lane keeps both. A
// build without them drops both: their part files go, neither is ever named in the folder, and
// the next render is made.
#[test]
fn a_dropped_render_stops_and_leaves_no_part_file() {
    let dir = project(&[]);
    let src = sine(440.0, 3.0, SR);
    audio::write_wav(&dir.join("a.wav"), &[src.clone(), src], SR).unwrap();
    let k = offline::Kernel::of(&std_packages(), offline::STRETCH).unwrap();
    let tempo = m2_song::TempoMap::constant(120.0);
    let clip = |beats: f64| -> m2_song::AudioClip {
        serde_json::from_value(json!({"start": 0, "length": beats, "asset": "a.wav", "warp": [[0, 0], [3, beats]]})).unwrap()
    };
    let folder = dir.join("renders");
    let mut cache = audio::Cache::live_in(folder.clone());
    cache.hold_renders(Some(SR as usize));
    let mut build = |clips: &[m2_song::AudioClip]| {
        let lane = audio::AudioLane::build_with(clips, &|a| Some(dir.join(a)), &mut cache, &tempo, SR as f64, 2, Some(&k)).unwrap();
        cache.prune();
        assert!(cache.settle_renders(std::time::Duration::from_secs(20)));
        lane
    };
    let first = build(&[clip(6.0), clip(4.0)]);
    assert_eq!(renders_in(&folder), (0, 2), "two renders asked for, each in its part file");
    let again = build(&[clip(6.0), clip(4.0)]);
    assert!(Arc::ptr_eq(&first, &again), "the same lane");
    assert_eq!(renders_in(&folder), (0, 2), "a lane kept keeps its renders going");
    let other = build(&[clip(5.0)]);
    assert_eq!(renders_in(&folder), (0, 1), "the two dropped renders stopped, and their part files went");
    cache.hold_renders(None);
    assert!(cache.settle_renders(std::time::Duration::from_secs(60)));
    assert_eq!(renders_in(&folder), (1, 0), "only the render a lane plays is made");
    assert_eq!(cache.work().1, 3);
    drop((first, again, other, cache));
    std::fs::remove_dir_all(dir).ok();
}

// The sx plan, D9 (kernel ABI v1, flags bit 0): an offline kernel is run by music2 to make a clip's
// render, never by song code. A song that calls one gets `kernel.offline` at its node; no graph
// types it, and no list of the kernels songs call has it.
#[test]
fn song_code_cannot_call_an_offline_kernel() {
    let ir = song(json!([{"id": "s", "op": "kernel", "kernel": "std:stretch", "src": "song.ts:3:5"}]), "s", json!([]));
    let pk = std_packages();
    let e = check(&ir, &pk).expect_err("refused");
    assert_eq!(e.iter().map(|d| (d.code, d.src.as_deref())).collect::<Vec<_>>(), [("kernel.offline", Some("song.ts:3:5"))], "{e:?}");
    assert!(e[0].message.starts_with("`std:stretch` is an offline kernel"), "{}", e[0].message);
    assert!(m2_ir::Kernels::kernel(&pk, "std:stretch").is_none() && pk.kernel_sigs().all(|s| s.id != "std:stretch"));
}

// Information only (PLAT-1): how fast `std:stretch` makes a warped clip's render here. A minute of
// stereo 44.1 kHz audio laid out over 75 s and up 3 semitones, in a 48 kHz song, into its file.
#[test]
#[ignore = "prints a measurement; run with --ignored --nocapture"]
fn stretch_speed_report() {
    let dir = project(&[]);
    let wave = |k: f32| (0..60 * 44_100).map(|i| (i as f32 * k).sin() * 0.3).collect::<Vec<f32>>();
    audio::write_wav(&dir.join("a.wav"), &[wave(0.031), wave(0.047)], 44_100).unwrap();
    let clip: m2_song::AudioClip =
        serde_json::from_value(json!({"start": 0, "length": 150, "asset": "a.wav", "warp": [[0, 0], [60, 150]], "pitch": 3})).unwrap();
    let k = offline::Kernel::of(&std_packages(), offline::STRETCH).unwrap();
    let mut cache = audio::Cache::with_renders_in(Some(dir.join("renders")));
    let t = std::time::Instant::now();
    let tempo = m2_song::TempoMap::constant(120.0);
    let lane = audio::AudioLane::build_with(&[clip], &|a| Some(dir.join(a)), &mut cache, &tempo, SR as f64, 2, Some(&k)).unwrap();
    let secs = t.elapsed().as_secs_f64();
    assert_eq!(cache.work().1, 1);
    println!("std:stretch: 75 s of stereo in {secs:.2} s, {:.0} times faster than real time", 75.0 / secs);
    drop(lane);
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

/// A package whose kernel `@test/t:k`, with no input and one mono output, plays channel 0 of its first
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

/// A song whose output is `@test/t:k` of [`asset_player`] reading `names`, in a project whose `assets/`
/// holds `files` (mono, at the song's rate): the song, its packages, its data, the project and
/// the package.
fn asset_song(names: &[&str], files: &[(&str, Vec<f32>)]) -> (Ir, PackageSet, DataSet, PathBuf, PathBuf) {
    let ir = song(json!([{"id": "k", "op": "kernel", "kernel": "@test/t:k", "assets": names}]), "k", json!([]));
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

// RENDER-28, the ex plan's D1: a fault report never allocates, on any thread. 8 threads report
// 10,000 faults at once while one reads; the ring is full at times. Each fault is read or counted
// lost.
#[test]
fn a_fault_report_never_allocates_locks_or_waits() {
    use crate::fault::{Code, Fault, Faults, Subject};
    let faults = std::sync::Arc::new(Faults::default());
    let writers: Vec<_> = (0..8u64)
        .map(|t| {
            let faults = faults.clone();
            std::thread::spawn(move || {
                let f = |n| Fault { code: Code::Stopped, plan: t, subject: Subject::Node(n) };
                alloc_count::during(|| (0..1250).for_each(|n| faults.report(f(n)))).1
            })
        })
        .collect();
    let mut read = 0u64;
    while writers.iter().any(|w| !w.is_finished()) {
        read += std::iter::from_fn(|| faults.take()).count() as u64;
    }
    let heap: Vec<usize> = writers.into_iter().map(|w| w.join().unwrap()).collect();
    read += std::iter::from_fn(|| faults.take()).count() as u64;
    assert_eq!(heap, [0; 8], "no writer touched the heap");
    assert_eq!((read + faults.lost(), faults.count()), (10_000, 10_000), "each read or counted lost");
}

// D3 of track sx (ABI-2): a sandbox off the audio thread may hold an asset as its file on disk,
// not decoded whole. A kernel reads the same bits either way: 16-bit and float files, mono, stereo
// and 3 channels, each channel and one past them, in odd pieces (one longer than the host reads at
// once) and past the end, with the same counts. Its settings entry says the same.
#[test]
fn an_asset_read_from_disk_is_the_decoded_asset() {
    // Its state at 4096: its settings' address, the frame it is at, where it reads to (room it
    // takes at birth), and what the last read returned. Its asset's channel is its first value.
    let k = r#"
  (func (export "k_new") (param $cfg i32) (result i32)
    (i32.store (i32.const 4096) (local.get $cfg))
    (i32.store (i32.const 4100) (i32.const 0))
    (i32.store (i32.const 4104) (i32.shl (memory.grow (i32.const 4)) (i32.const 16)))
    (i32.const 4096))
  (func (export "k_ports") (result i32) (i32.const 1))
  (func (export "k_reset") (param $s i32) (i32.store offset=4 (local.get $s) (i32.const 0)))
  (func (export "k_active") (param i32) (result i32) (i32.const 0))
  (func (export "k_process") (param $s i32) (param $io i32) (param $from i32) (param $to i32) (local $cfg i32)
    (local.set $cfg (i32.load (local.get $s)))
    (i32.store offset=12 (local.get $s) (call $read
      (i32.load (i32.load offset=36 (local.get $cfg)))
      (i32.trunc_f64_u (f64.load (i32.load offset=28 (local.get $cfg))))
      (i32.load offset=4 (local.get $s))
      (i32.load offset=8 (local.get $s))
      (i32.sub (local.get $to) (local.get $from))))
    (i32.store offset=4 (local.get $s) (i32.add (i32.load offset=4 (local.get $s)) (i32.sub (local.get $to) (local.get $from)))))"#;
    let import = r#"(module (import "m2" "asset_read" (func $read (param i32 i32 i32 i32 i32) (result i32)))"#;
    let pkg = package(&module_v1(k).replacen("(module", import, 1));
    let mut pk = PackageSet::default();
    pk.load(&pkg).unwrap();
    let (module, prefix, _) = pk.module_for("@test/t:k").unwrap();
    let frames = 50_000;
    let sample = |c: usize, i: usize| (((i * (31 + 7 * c)) % 997) as f32 / 498.5 - 1.0) * 0.9;
    let mut files = vec![];
    for (channels, float) in [(1, false), (2, false), (3, false), (1, true), (2, true)] {
        let path = pkg.join(format!("{channels}-{float}.wav"));
        let (bits, format) = if float { (32, hound::SampleFormat::Float) } else { (16, hound::SampleFormat::Int) };
        let spec = hound::WavSpec { channels, sample_rate: 44_100, bits_per_sample: bits, sample_format: format };
        let mut w = hound::WavWriter::create(&path, spec).unwrap();
        for i in 0..frames {
            for c in 0..channels as usize {
                match float {
                    true => w.write_sample(sample(c, i)).unwrap(),
                    false => w.write_sample((sample(c, i) * 32767.0) as i16).unwrap(),
                }
            }
        }
        w.finalize().unwrap();
        files.push(path);
    }
    // Each read, in turn: the count `asset_read` returned, and the floats' bits.
    let reads = |held: sandbox::Held, channel: u32| -> Vec<(u32, Vec<u32>)> {
        let mut sb = sandbox::Sandbox::new(&pk.engine, module).unwrap();
        let values = [channel as f64];
        let assets = [Some(held)];
        let spec = sandbox::Spec {
            inputs: 0,
            outputs: 1,
            channels: &[1],
            audio: 1,
            sample_rate: SR as f64,
            values: &values,
            assets: &assets,
            offline: None,
            notes: &[],
        };
        let s = sb.state(prefix, &spec).unwrap();
        sb.seal().unwrap();
        [1, 37, 128, 1_000, 20_000, 5, 30_000, 300]
            .iter()
            .map(|&n| {
                s.process(&mut sb, n).unwrap();
                let data = sb.data();
                let word = |at: usize| u32::from_le_bytes(data[at..at + 4].try_into().unwrap());
                let dst = word(4104) as usize;
                (word(4108), (0..n).map(|i| word(dst + 4 * i)).collect())
            })
            .collect()
    };
    for path in &files {
        let whole = sandbox::Held::Whole(Arc::new(crate::decode::load(path).unwrap()));
        let disk = sandbox::Held::Disk(Arc::new(crate::stream::Wav::open(path).unwrap()));
        assert_eq!(whole.shape(), disk.shape(), "{}: the settings entry", path.display());
        for channel in 0..3 {
            let (w, d) = (reads(whole.clone(), channel), reads(disk.clone(), channel));
            if channel < 2 {
                assert!(w.iter().any(|(_, f)| f.iter().any(|&b| b != 0)), "it reads sound");
            }
            let counts = |r: &[(u32, Vec<u32>)]| r.iter().map(|(n, _)| *n).collect::<Vec<_>>();
            assert_eq!(counts(&d), counts(&w), "{} channel {channel}: the counts", path.display());
            assert!(w == d, "{} channel {channel}: the same bits", path.display());
        }
    }
    std::fs::remove_dir_all(pkg).ok();
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

// RENDER-27 for kernels: a live session decodes a kernel's file once. Each edit builds a new
// renderer over the same cache and prunes it, as the session does at each settle; the file is
// decoded again only when its bytes change.
#[test]
fn a_kernels_file_is_decoded_once_across_edits() {
    let (ir, pk, tl, dir, pkg) = asset_song(&["hit.wav"], &[("hit.wav", sine(440.0, 0.5, SR))]);
    let flat = check(&ir, &pk).unwrap();
    let (mut cache, owner) = (audio::Cache::default(), m2_plugins::Owner::discard());
    for _ in 0..3 {
        drop(Renderer::prepare(&flat, &tl, &pk, SR, &mut cache, &owner, Basis::Fresh).unwrap());
        cache.prune();
    }
    assert_eq!(cache.work().0, 1, "three builds, one decode");
    audio::write_wav(&dir.join("assets/hit.wav"), &[sine(220.0, 0.7, SR)], SR).unwrap();
    drop(Renderer::prepare(&flat, &data(&ir, &dir), &pk, SR, &mut cache, &owner, Basis::Fresh).unwrap());
    assert_eq!(cache.work().0, 2, "new bytes, a new decode");
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
    assert_eq!(crate::decode::load(&new).unwrap().channels, vec![vec![0.5; 4]]);
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
