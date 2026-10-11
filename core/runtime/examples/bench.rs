//! The M1 decision gate: are WebAssembly kernels fast enough?
//!
//! Renders the same 16-voice synth (mtof → saw → svf → adsr → scale) for 60 seconds
//! twice: once through the runtime (WebAssembly kernels, one instance per node), and
//! once with the identical kernel source compiled natively, driven by a minimal loop
//! that copies buffers between kernels the same way the runtime does.
//!
//! `cargo run --release -p m2-runtime --example bench`

use std::path::Path;
use std::time::Instant;

use m2_kernel::{Host, Kernel};
use m2_runtime::{BLOCK, PackageSet, Renderer};
use m2_std_kernels::{Adsr, Mtof, Osc, Scale, Svf};
use serde_json::json;

const SR: u32 = 48_000;
const SECONDS: f64 = 60.0;
const VOICES: usize = 16;

fn wasm() -> f64 {
    let mut pk = PackageSet::default();
    pk.load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")).unwrap();
    let ir: m2_ir::Ir = serde_json::from_value(json!({"ir": 0, "root": {"module": "song", "export": "out"}, "modules": [{
        "name": "song", "lanes": [{"id": "notes", "kind": "events<note>"}], "exports": {"out": "v"},
        "nodes": [
            {"id": "n", "op": "lane", "lane": "notes"},
            {"id": "v", "op": "poly", "events": "n", "voices": VOICES, "body": {"out": "amp", "nodes": [
                {"id": "p", "op": "lane", "lane": "pitch"},
                {"id": "g", "op": "lane", "lane": "gate"},
                {"id": "hz", "op": "kernel", "kernel": "std:mtof", "inputs": {"pitch": "p"}},
                {"id": "o", "op": "kernel", "kernel": "std:saw", "inputs": {"freq": "hz"}},
                {"id": "c", "op": "const", "value": 1200, "unit": "hz"},
                {"id": "f", "op": "kernel", "kernel": "std:svf", "inputs": {"in": "o", "cutoff": "c"}},
                {"id": "e", "op": "kernel", "kernel": "std:adsr", "inputs": {"gate": "g"}},
                {"id": "amp", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "f.lp", "by": "e"}}]}}]}]}))
    .unwrap();
    let flat = m2_ir::check(&ir, &pk).unwrap();
    let dir = std::env::temp_dir().join(format!("m2-bench-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let notes: Vec<_> = (0..VOICES).map(|i| json!({"pitch": 36 + i * 3, "start": 0, "length": 1000})).collect();
    std::fs::write(dir.join("song.notes.json"), json!({"clips": [{"start": 0, "length": 1000, "notes": notes}]}).to_string()).unwrap();
    let tl = m2_song::Timeline::load(&dir).unwrap();
    let mut r = Renderer::new(&flat, &tl, &pk, SR).unwrap();
    let t = Instant::now();
    let out = r.render(SECONDS).unwrap();
    let elapsed = t.elapsed().as_secs_f64();
    std::hint::black_box(out);
    std::fs::remove_dir_all(dir).ok();
    elapsed
}

struct NativeVoice {
    mtof: Host<Mtof>,
    saw: Host<Osc<1>>,
    svf: Host<Svf>,
    adsr: Host<Adsr>,
    scale: Host<Scale>,
}

fn copy<A: Kernel, B: Kernel>(from: &mut Host<A>, port: usize, to: &mut Host<B>, to_port: usize, frames: usize) {
    let src: Vec<f32> = from.port_mut(port)[..frames].to_vec();
    to.port_mut(to_port)[..frames].copy_from_slice(&src);
}

fn native() -> f64 {
    let mb = BLOCK as u32;
    let mut voices: Vec<NativeVoice> = (0..VOICES)
        .map(|i| {
            let mut v = NativeVoice {
                mtof: Host::new(SR as f32, mb, 1),
                saw: Host::new(SR as f32, mb, 1),
                svf: Host::new(SR as f32, mb, 1),
                adsr: Host::new(SR as f32, mb, 1),
                scale: Host::new(SR as f32, mb, 1),
            };
            v.mtof.port_mut(0).fill(36.0 + i as f32 * 3.0);
            v.svf.port_mut(1).fill(1200.0);
            v.svf.port_mut(2).fill(0.707);
            v.adsr.port_mut(0).fill(1.0);
            for (p, d) in [(1, 0.005), (2, 0.2), (3, 0.7), (4, 0.2)] {
                v.adsr.port_mut(p).fill(d);
            }
            v
        })
        .collect();
    let mut mix = vec![0.0f32; BLOCK];
    let blocks = (SECONDS * SR as f64) as usize / BLOCK;
    let t = Instant::now();
    for _ in 0..blocks {
        mix.fill(0.0);
        for v in &mut voices {
            let f = BLOCK;
            v.mtof.process(f as u32);
            copy(&mut v.mtof, 1, &mut v.saw, 0, f);
            v.saw.process(f as u32);
            copy(&mut v.saw, 1, &mut v.svf, 0, f);
            v.svf.process(f as u32);
            v.adsr.process(f as u32);
            copy(&mut v.svf, 3, &mut v.scale, 0, f);
            copy(&mut v.adsr, 5, &mut v.scale, 1, f);
            v.scale.process(f as u32);
            for (m, x) in mix.iter_mut().zip(&v.scale.port_mut(2)[..f]) {
                *m += x;
            }
        }
        std::hint::black_box(&mix);
    }
    t.elapsed().as_secs_f64()
}

fn main() {
    // Warm up both paths once, then take the best of three.
    let best = |f: fn() -> f64| (0..3).map(|_| f()).fold(f64::INFINITY, f64::min);
    native();
    let n = best(native);
    let w = best(wasm);
    println!("{VOICES} voices × (mtof → saw → svf → adsr → scale), {SECONDS}s of audio at {SR} Hz");
    println!("  native:      {:6.3}s  ({:5.0}× real time)", n, SECONDS / n);
    println!("  webassembly: {:6.3}s  ({:5.0}× real time)", w, SECONDS / w);
    println!("  ratio:       {:.2}× slower than native", w / n);
}
