//! PACK-27: `@m2/wavetable`'s own kernel in wasm, as music2's own: born and sealed in its voices'
//! sandbox, it plays notes on every path (a file's table and a built-in one, eight unison voices,
//! the LFO) with no heap operation and no growth, and renders the same bits every time.

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
