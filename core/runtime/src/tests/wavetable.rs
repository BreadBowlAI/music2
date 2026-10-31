//! PACK-27: `@m2/wavetable`'s own kernel in wasm, as music2's own: born and sealed in its voices'
//! sandbox, it plays notes on every path (a file's table and a built-in one, eight unison voices,
//! the LFO) with no heap operation and no growth, and renders the same bits every time. Its shelf,
//! the tables every voice of a sandbox shares (ABI-5's sandbox-wide data): found by their settings
//! wherever a voice's bytes are, made once for 16 voices, never one oscillator's for the other's.

use super::*;

/// A poly of three voices of the wavetable on the song's notes: A plays the project's
/// `assets/table.wav`, B the vowels, each with eight unison voices, the LFO on both positions and
/// the cutoff.
fn wavetable_song() -> (Ir, DataSet, PathBuf) {
    let body = json!([
        {"id": "p", "op": "lane", "lane": "pitch"},
        {"id": "g", "op": "lane", "lane": "gate"},
        {"id": "v", "op": "lane", "lane": "velocity"},
        {"id": "half", "op": "const", "value": 0.5},
        {"id": "spread", "op": "const", "value": 1},
        {"id": "two", "op": "const", "value": 2},
        {"id": "w", "op": "kernel", "kernel": "@m2/wavetable:voice", "assets": ["table.wav"], "values": [0, 2, 0, -1, 8],
         "inputs": {"pitch": "p", "gate": "g", "velocity": "v", "b_level": "half", "lfo_position": "half", "lfo_cutoff": "two",
                    "spread": "spread", "env": "two"}}
    ]);
    let ir = song(
        json!([{"id": "n", "op": "lane", "lane": "notes"}, {"id": "poly", "op": "poly", "events": "n", "voices": 3, "body": {"out": "w", "nodes": body}}]),
        "poly",
        json!([{"key": "notes", "kind": "events<note>"}]),
    );
    let dir = project(&[(
        "data/song/notes.json",
        json!({"clips": [{"start": 0, "length": 16, "notes": [
            {"pitch": 45, "start": 0, "length": 1.5}, {"pitch": 64, "start": 0.5, "length": 2}, {"pitch": 71, "start": 0.75, "length": 1},
            {"pitch": 96, "start": 2, "length": 0.5}, {"pitch": 33, "start": 2.25, "length": 3}, {"pitch": 60, "start": 3, "length": 0.25}]}]}),
    )]);
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    // 16 frames, each a cycle with one more harmonic than the one before.
    let table: Vec<f32> = (0..16 * 2048)
        .map(|i| {
            let (frame, n) = (i / 2048, (i % 2048) as f64 / 2048.0);
            (1..=frame + 1).map(|h| (std::f64::consts::TAU * h as f64 * n).sin() / h as f64).sum::<f64>() as f32 * 0.5
        })
        .collect();
    audio::write_wav(&dir.join("assets/table.wav"), &[table], SR).unwrap();
    let tl = data(&ir, &dir);
    (ir, tl, dir)
}

// RENDER-28, ABI-5: the voice allocates nothing while it plays, and its sandbox, sealed at its birth,
// never grows: 6 notes on 3 voices (stolen and reused), every path the kernel has.
#[test]
fn the_wavetable_plays_in_a_sealed_sandbox_and_allocates_nothing() {
    let (ir, tl, dir) = wavetable_song();
    let pk = own_packages();
    let mut r = Renderer::new(&check(&ir, &pk).unwrap(), &tl, &pk, SR).unwrap();
    r.render_block(0).unwrap();
    let (peak, allocs) =
        alloc_count::during(|| (1..1000u64).map(|b| r.render_block(b * BLOCK as u64).unwrap()[0][0].abs()).fold(0.0f32, f32::max));
    assert_eq!(allocs, 0, "no heap operation in 999 blocks of notes");
    assert!(peak > 0.05, "it played: peak {peak}");
    std::fs::remove_dir_all(dir).ok();
}

// RENDER-2: the same song renders the same bits every time: its tables are made the same way at
// every birth, and nothing in a voice hangs on where its blocks fall.
#[test]
fn the_wavetable_renders_the_same_bits_every_time() {
    let (ir, tl, dir) = wavetable_song();
    let pk = own_packages();
    let flat = check(&ir, &pk).unwrap();
    let once = Renderer::new(&flat, &tl, &pk, SR).unwrap().render(4.0).unwrap();
    assert!(peak(&once) > 0.05, "it played");
    let again = Renderer::new(&flat, &tl, &pk, SR).unwrap().render(4.0).unwrap();
    assert!(once == again, "two renders differ");
    std::fs::remove_dir_all(dir).ok();
}

/// The voice's inputs at the manifest's defaults, with `changes` by name.
fn inputs(pk: &PackageSet, changes: &[(&str, f32)]) -> Vec<f32> {
    let sig = m2_ir::Kernels::kernel(pk, "@m2/wavetable:voice").unwrap();
    sig.inputs.iter().map(|p| changes.iter().find(|c| c.0 == p.name).map_or(p.default.unwrap_or(0.0) as f32, |c| c.1)).collect()
}

/// A voice of `@m2/wavetable:voice` born in `sb` with `values` (its tables and unison).
fn voice(sb: &mut sandbox::Sandbox, prefix: &str, values: &[f64]) -> sandbox::State {
    let mut channels = [1u32; 30];
    channels[29] = 2;
    let spec = sandbox::Spec {
        inputs: 29,
        outputs: 1,
        channels: &channels,
        audio: 2,
        sample_rate: SR as f64,
        values,
        assets: &[],
        offline: None,
        notes: &[],
    };
    sb.state(prefix, &spec).unwrap()
}

/// `blocks` blocks of state `s` with constant `ins`: its left channel.
fn play(sb: &mut sandbox::Sandbox, s: &sandbox::State, blocks: usize, ins: &[f32]) -> Vec<f32> {
    let mut out = vec![];
    for _ in 0..blocks {
        let data = sb.data();
        for (p, v) in ins.iter().enumerate() {
            floats(data, s.ins[p], BLOCK).fill(*v);
        }
        s.process(sb, BLOCK).unwrap();
        out.extend_from_slice(floats(sb.data(), s.outs[0], BLOCK));
    }
    out
}

// ABI-5, PACK-27 (review B1): a voice finds its tables on the shelf by their settings, never by an
// address it keeps. Its run of bytes, born in sandbox A (oscillator A on vowels, B on digital) and
// copied at the same offsets into sandbox B, whose voice was born first with the same two tables
// the other way round (so made in the other order), plays the same bits there as in A.
#[test]
fn a_voice_moved_into_a_sandbox_born_in_another_order_plays_the_same_bits() {
    let pk = own_packages();
    let (module, prefix, _) = pk.module_for("@m2/wavetable:voice").unwrap();
    let ins = inputs(&pk, &[("pitch", 57.0), ("gate", 1.0), ("sustain", 1.0), ("a_position", 0.5), ("b_level", 0.5), ("b_position", 0.5)]);
    let mut a = sandbox::Sandbox::new(module, pk.limits.of(false)).unwrap();
    let x = voice(&mut a, prefix, &[2.0, 4.0, -1.0, -1.0, 1.0]);
    a.seal().unwrap();
    x.reset(&mut a).unwrap();
    let bytes = a.data()[x.ins[0]..].to_vec();
    let heard = play(&mut a, &x, 200, &ins);
    assert!(heard.iter().fold(0.0f32, |m, v| m.max(v.abs())) > 0.05, "it plays in A");

    let mut b = sandbox::Sandbox::new(module, pk.limits.of(false)).unwrap();
    let z = voice(&mut b, prefix, &[4.0, 2.0, -1.0, -1.0, 1.0]);
    b.seal().unwrap();
    z.reset(&mut b).unwrap();
    assert_eq!((z.ins[0], z.outs[0]), (x.ins[0], x.outs[0]), "born first, at the same offsets");
    let data = b.data();
    let n = bytes.len().min(data.len() - x.ins[0]);
    data[x.ins[0]..x.ins[0] + n].copy_from_slice(&bytes[..n]);
    let moved = play(&mut b, &z, 200, &ins);
    assert!(moved == heard, "a voice's bytes moved into a sandbox born in another order play other bits");
}

// PACK-27 (review B5): 16 voices of one sound share its tables in wasm: between the sandbox's
// creation and its seal its memory grows by less than one table of 32 frames (over 2 MiB), the
// first voice building the tables onto the module's shelf and every other finding them there.
#[test]
fn sixteen_voices_grow_their_sandbox_by_less_than_one_table() {
    let pk = own_packages();
    let (module, prefix, _) = pk.module_for("@m2/wavetable:voice").unwrap();
    let mut sb = sandbox::Sandbox::new(module, pk.limits.of(false)).unwrap();
    let before = sb.size();
    let voices: Vec<sandbox::State> = (0..16).map(|_| voice(&mut sb, prefix, &[2.0, 4.0, -1.0, -1.0, 8.0])).collect();
    let grew = sb.size() - before;
    assert!(grew < 2 << 20, "16 voices grew their sandbox by {grew} bytes");
    sb.seal().unwrap();
    let ins = inputs(&pk, &[("pitch", 60.0), ("gate", 1.0), ("b_level", 0.5)]);
    for v in &voices {
        v.reset(&mut sb).unwrap();
    }
    assert!(play(&mut sb, &voices[15], 50, &ins).iter().any(|v| *v != 0.0), "the last voice plays");
}

// PACK-27 (review B5): each oscillator plays its own table off the shelf, in wasm. A alone on vowels
// plays the bits B alone on vowels does (pitch, position and level swapped with it); and vowels on
// A with digital on B plays the bits of digital on A with vowels on B, every oscillator's input
// swapped with its table.
#[test]
fn the_oscillators_tables_swapped_play_the_same_bits() {
    let pk = own_packages();
    let (module, prefix, _) = pk.module_for("@m2/wavetable:voice").unwrap();
    let render = |values: &[f64], ins: &[(&str, f32)]| -> Vec<f32> {
        let mut sb = sandbox::Sandbox::new(module, pk.limits.of(false)).unwrap();
        let s = voice(&mut sb, prefix, values);
        sb.seal().unwrap();
        s.reset(&mut sb).unwrap();
        let mut all = vec![("pitch", 52.0), ("gate", 1.0), ("sustain", 1.0)];
        all.extend_from_slice(ins);
        play(&mut sb, &s, 150, &inputs(&pk, &all))
    };
    let a = [("a_level", 1.0), ("a_position", 0.4), ("a_pitch", 7.0), ("b_level", 0.0)];
    let b = [("b_level", 1.0), ("b_position", 0.4), ("b_pitch", 7.0), ("a_level", 0.0)];
    let alone_a = render(&[2.0, 4.0, -1.0, -1.0, 1.0], &a);
    assert!(alone_a.iter().any(|v| *v != 0.0));
    assert!(render(&[4.0, 2.0, -1.0, -1.0, 1.0], &b) == alone_a, "B alone on vowels plays other bits than A alone on vowels");
    let both = [("a_level", 0.7), ("a_position", 0.3), ("b_level", 0.4), ("b_position", 0.8), ("b_pitch", 7.0)];
    let swapped = [("b_level", 0.7), ("b_position", 0.3), ("a_level", 0.4), ("a_position", 0.8), ("a_pitch", 7.0)];
    let one = render(&[2.0, 4.0, -1.0, -1.0, 1.0], &both);
    assert!(render(&[4.0, 2.0, -1.0, -1.0, 1.0], &swapped) == one, "the tables swapped with the oscillators play other bits");
}
