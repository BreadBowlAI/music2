//! VOX-20, VOX-24, RENDER-28: Tune's three kernels (`@m2/vocals`) in wasm, as music2's own: born and
//! sealed in their sandbox, they play a voice moving up through a key, at 44.1, 48 and 96 kHz, with
//! no heap operation and no growth, as late as `latency_s` says at each rate, and the same bits every
//! time.

use super::*;

/// A buzzy voice (a saw through a low-pass) sliding up from 196 to 262 Hz, and Tune's chain on it,
/// as the pack joins it: pitch, snap (to the song's key, `time.key`) and shift.
fn tuned(glide: f64) -> Ir {
    song(
        json!([
            {"id": "f", "op": "lane", "lane": "f"},
            {"id": "x", "op": "kernel", "kernel": "std:saw", "inputs": {"freq": "f"}},
            {"id": "cut", "op": "const", "value": 1800.0},
            {"id": "v", "op": "kernel", "kernel": "std:svf", "inputs": {"in": "x", "cutoff": "cut"}},
            {"id": "p", "op": "kernel", "kernel": "@m2/vocals:pitch", "inputs": {"in": "v.lp"}},
            {"id": "key", "op": "time", "of": "key"},
            {"id": "g", "op": "const", "value": glide},
            {"id": "one", "op": "const", "value": 1.0},
            {"id": "s", "op": "kernel", "kernel": "@m2/vocals:snap", "inputs": {"hz": "p.hz", "clarity": "p.clarity", "key": "key", "glide": "g", "amount": "one"}},
            {"id": "out", "op": "kernel", "kernel": "@m2/vocals:shift", "inputs": {"in": "v.lp", "semitones": "s", "hz": "p.hz", "clarity": "p.clarity"}},
        ]),
        "out",
        json!([{"key": "f", "kind": "control<hz>", "default": 196.0}]),
    )
}

/// The song's data: its pitch lane sliding over 4 s, and the key of C major.
fn sliding(ir: &Ir) -> (DataSet, PathBuf) {
    timeline_for(
        ir,
        &[
            ("data/song/f.json", json!({"unit": "hz", "clips": [{"start": 0, "length": 8, "points": [[0, 196.0], [8, 262.0]]}]})),
            (
                "data/arrangement.json",
                json!({"tempo": [{"beat": 0, "bpm": 120}], "meter": [{"beat": 0, "beats": 4, "unit": 4}], "key": {"root": "C", "scale": "major"}}),
            ),
        ],
    )
}

// RENDER-28, ABI-5, VOX-24: born and sealed, Tune allocates nothing while it plays, at every rate,
// and the song is as late as its `latency_s` counts there.
#[test]
fn tune_plays_in_a_sealed_sandbox_and_allocates_nothing_at_every_rate() {
    let ir = tuned(0.0);
    let (tl, dir) = sliding(&ir);
    let pk = own_packages();
    let flat = check(&ir, &pk).unwrap_or_else(|e| panic!("{e:?}"));
    for rate in [44_100, 48_000, 96_000] {
        let mut r = Renderer::new(&flat, &tl, &pk, rate).unwrap();
        assert_eq!(r.latency(), m2_kernel::latency_frames(0.015, rate as f64), "{rate} Hz: as late as latency_s says");
        r.render_block(0).unwrap();
        let blocks = 4 * rate as u64 / BLOCK as u64;
        let (peak, allocs) =
            alloc_count::during(|| (1..blocks).map(|b| r.render_block(b * BLOCK as u64).unwrap()[0][0].abs()).fold(0.0f32, f32::max));
        assert_eq!(allocs, 0, "{rate} Hz: no heap operation in {blocks} blocks");
        assert!(peak > 0.05, "{rate} Hz: it played: peak {peak}");
    }
    std::fs::remove_dir_all(dir).ok();
}

// RENDER-2: the same song renders the same bits every time.
#[test]
fn tune_renders_the_same_bits_every_time() {
    let ir = tuned(0.05);
    let (tl, dir) = sliding(&ir);
    let pk = own_packages();
    let flat = check(&ir, &pk).unwrap();
    let once = Renderer::new(&flat, &tl, &pk, SR).unwrap().render(4.0).unwrap();
    assert!(peak(&once) > 0.05, "it played");
    let again = Renderer::new(&flat, &tl, &pk, SR).unwrap().render(4.0).unwrap();
    assert!(once == again, "two renders differ");
    std::fs::remove_dir_all(dir).ok();
}

// VOX-22, VOX-27, ABI-13, TAP-15: snap's meters, Tune's pitch line, reach a look, read once a
// block: before pitch has read the voice, no pitch, 0 and 0; two seconds in, the voice sliding up
// past 229 Hz (57.69 as a MIDI number), the pitch it hears within a tenth of a semitone of it, and
// the note of C major it moves it to, A (57).
#[test]
fn tunes_pitch_line_reaches_a_look() {
    let ir = tuned(0.05);
    let (tl, dir) = sliding(&ir);
    let pk = own_packages();
    let flat = check(&ir, &pk).unwrap_or_else(|e| panic!("{e:?}"));
    let mut r = Renderer::prepare(&flat, &tl, &pk, SR, &mut audio::Cache::default(), &m2_plugins::Owner::discard(), Basis::Fresh).unwrap();
    r.set_transport(Transport::new(0, true));
    let taps = r.info().taps.clone().expect("a live plan has taps");
    let snap = flat.nodes.iter().position(|n| n.name == "song/s").expect("snap's node");
    // Its meters follow its one output: heard, then note.
    taps.point_meter(0, Some(TapAt::Port(snap, 1))).unwrap();
    taps.point_meter(1, Some(TapAt::Port(snap, 2))).unwrap();
    let mut read = |until: f64| {
        while (r.clock() as f64) < until * SR as f64 {
            r.render_block(r.clock()).unwrap();
        }
        (taps.meter(0).expect("a reading").now, taps.meter(1).expect("a reading").now)
    };
    assert_eq!(read(0.001), (0.0, 0.0), "no pitch read yet");
    let (heard, note) = read(2.0);
    // The lane's pitch where the block ends: 196 Hz to 262 Hz over 4 s.
    let lane = 69.0 + 12.0 * ((196.0 + 66.0 * r.clock() as f64 / SR as f64 / 4.0) / 440.0).log2();
    assert!((heard as f64 - lane).abs() < 0.1, "heard {heard}, the voice is at {lane:.2}");
    assert_eq!(note, 57.0, "moved to A, the nearest note of C major");
    std::fs::remove_dir_all(dir).ok();
}
