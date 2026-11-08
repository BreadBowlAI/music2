//! PACK-26, ABI-13, TAP-15: `@m2/mixing`'s meters in wasm, as music2's own: the gate's, the
//! de-esser's and the multiband's, each read by a look where its card's picture reads it (CARD-112,
//! CARD-114, CARD-115). What each says, measured against its sound, is the kernels' native tests
//! (`packs/@m2/mixing/kernels-src/src/tests.rs`).

use super::*;

/// A loud 1 kHz tone through a gate, silence through another, noise through the de-esser, and a
/// 40 Hz tone through the multiband with its low band's threshold at −40 dB, summed.
fn tools() -> Ir {
    song(
        json!([
            {"id": "f", "op": "const", "value": 1000, "unit": "hz"},
            {"id": "x", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "f"}},
            {"id": "none", "op": "const", "value": 0, "unit": "gain"},
            {"id": "q", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "x", "by": "none"}},
            {"id": "open", "op": "kernel", "kernel": "@m2/mixing:gate", "inputs": {"in": "x", "key": "x"}},
            {"id": "shut", "op": "kernel", "kernel": "@m2/mixing:gate", "inputs": {"in": "q", "key": "q"}},
            {"id": "n", "op": "kernel", "kernel": "std:noise"},
            {"id": "de", "op": "kernel", "kernel": "@m2/mixing:deesser", "inputs": {"in": "n"}},
            {"id": "lf", "op": "const", "value": 40, "unit": "hz"},
            {"id": "lo", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "lf"}},
            {"id": "lt", "op": "const", "value": -40, "unit": "db"},
            {"id": "mb", "op": "kernel", "kernel": "@m2/mixing:multiband", "inputs": {"in": "lo", "low_threshold": "lt"}},
            {"id": "a", "op": "kernel", "kernel": "std:add", "inputs": {"a": "open", "b": "shut"}},
            {"id": "b", "op": "kernel", "kernel": "std:add", "inputs": {"a": "a", "b": "de"}},
            {"id": "out", "op": "kernel", "kernel": "std:add", "inputs": {"a": "b", "b": "mb"}},
        ]),
        "out",
        json!([]),
    )
}

// PACK-26, ABI-13, TAP-15: each of the mixing tools' meters, which their kernels write after their
// sound, reaches a look, read once a block: the gate on a loud tone open with nothing turned down,
// on silence closed at its range (80 dB); the de-esser on noise, by up to its amount (6 dB); the
// multiband's low band over its threshold (a full-scale tone 40 dB over, at a ratio of 2: about
// 20 dB), its mid and high bands nothing.
#[test]
fn the_mixing_meters_reach_a_look() {
    let ir = tools();
    let (tl, dir) = timeline_for(&ir, &[]);
    let pk = own_packages();
    let flat = check(&ir, &pk).unwrap_or_else(|e| panic!("{e:?}"));
    let mut r = Renderer::prepare(&flat, &tl, &pk, SR, &mut audio::Cache::default(), &m2_plugins::Owner::discard(), Basis::Fresh).unwrap();
    r.set_transport(Transport::new(0, true));
    let taps = r.info().taps.clone().expect("a live plan has taps");
    let node = |id: &str| flat.nodes.iter().position(|n| n.name == format!("song/{id}")).unwrap_or_else(|| panic!("no {id}"));
    // Each tool's meters follow its one output.
    let looks = [("open", 1), ("open", 2), ("shut", 1), ("shut", 2), ("de", 1), ("mb", 1), ("mb", 2), ("mb", 3)];
    for (slot, (id, port)) in looks.iter().enumerate() {
        taps.point_meter(slot, Some(TapAt::Port(node(id), *port))).unwrap();
    }
    for _ in 0..200 {
        r.render_block(r.clock()).unwrap();
    }
    let said: Vec<f32> = (0..looks.len()).map(|slot| taps.meter(slot).expect("a reading").now).collect();
    assert_eq!((said[0], said[1]), (0.0, 1.0), "the gate on a loud tone: nothing turned down, open");
    assert!((said[2] - 80.0).abs() < 1e-3 && said[3] == 0.0, "the gate on silence: its range, closed: {:?}", &said[2..4]);
    assert!(said[4] > 0.0 && said[4] <= 6.0 + 1e-4, "the de-esser on noise: by up to its amount: {}", said[4]);
    assert!((said[5] - 20.0).abs() < 1.5, "the multiband's low band: about 20 dB: {}", said[5]);
    assert_eq!((said[6], said[7]), (0.0, 0.0), "its mid and high bands: nothing");
    std::fs::remove_dir_all(dir).ok();
}
