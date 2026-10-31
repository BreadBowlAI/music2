//! Choices and switches (CARD-10 to CARD-15): a kernel that reads a choice gets the option's
//! place, one that reads a switch gets 0 or 1, and a lane of names steps between places, playing
//! its fallback where a point names an option the choice doesn't have.

use m2_song::Severity;

use super::cell::framed;
use super::*;

/// std, and a package `@test/t` whose kernel `k` copies its one input, declared `input`, to sound.
fn copy_kernel(input: &str) -> PackageSet {
    let k = kernel_v1("k", &out_v1("(local.get $x)"));
    let dir = package_with(&module_v1(&k), &format!("inputs = [\"{input}\"]\noutputs = [\"out: audio<1>\"]"));
    let mut pk = std_packages();
    pk.load(&dir).unwrap();
    pk
}

/// The first channel of `seconds` of `ir`, with `data`.
fn played(ir: &Ir, data: &DataSet, pk: &PackageSet, seconds: f64) -> Vec<f32> {
    let flat = check(ir, pk).unwrap_or_else(|e| panic!("{e:?}"));
    Renderer::new(&flat, data, pk, SR).unwrap().render(seconds).unwrap().iter().map(|f| f[0]).collect()
}

const SHAPE: &str = "x: control<choice<saw|square|sine>> = 0";

/// A song whose kernel `k` reads `shape`, a lane of names with fallback `square`.
fn shape_lane() -> Ir {
    song(
        json!([{"id": "l", "op": "lane", "lane": "shape"}, {"id": "k", "op": "kernel", "kernel": "@test/t:k", "inputs": {"x": "l"}}]),
        "k",
        json!([{"key": "shape", "kind": "control<choice<saw|square|sine>>", "default": 1}]),
    )
}

/// Each value `x` holds over each quarter of a second, if it holds one there.
fn quarters(x: &[f32]) -> Vec<Option<f32>> {
    x.chunks(SR as usize / 4).map(|q| q.iter().all(|v| *v == q[0]).then_some(q[0])).collect()
}

// CARD-10, CARD-11, CARD-12: a choice written in code reaches its kernel as its place, a switch as
// 0 or 1. Each constant is the one `@m2/core` writes for `shape: "sine"` and `sub: true`
// (`a_choice_reaches_its_kernel_as_its_place` and `a_switch_reaches_its_kernel_as_0_or_1` in
// `core/script/src/kinds_tests.rs`): a cell of the call's control.
#[test]
fn a_choice_plays_its_place() {
    for (input, unit, value) in [(SHAPE, "choice<saw|square|sine>", 2.0), ("x: control<switch> = 0", "switch", 1.0)] {
        let pk = copy_kernel(input);
        let c = json!({"id": "c", "op": "const", "value": value, "unit": unit,
                       "origin": {"site": 1, "arg": ["1", "shape"], "control": {"frame": 0, "slot": "shape"}}});
        let ir = framed(song(json!([c, {"id": "k", "op": "kernel", "kernel": "@test/t:k", "inputs": {"x": "c"}}]), "k", json!([])));
        let out = played(&ir, &DataSet::empty(), &pk, 0.05);
        assert!(out.iter().all(|&x| x == value as f32), "{unit}: {:?}", &out[..4]);
    }
}

// CARD-15: a lane of names steps, each point holding to the next, and plays its fallback outside
// its clips. At 120 bpm a beat is half a second.
#[test]
fn a_lane_of_names_steps_between_places() {
    let pk = copy_kernel(SHAPE);
    let ir = shape_lane();
    let clips = json!({"unit": "choice", "clips": [{"start": 0, "length": 2, "points": [[0, "sine"], [1, "saw"]]}]});
    let (tl, dir) = timeline_for(&ir, &[("data/song/shape.json", clips)]);
    let out = played(&ir, &tl, &pk, 1.5);
    assert_eq!(quarters(&out), [Some(2.0), Some(2.0), Some(0.0), Some(0.0), Some(1.0), Some(1.0)]);
    std::fs::remove_dir_all(dir).ok();

    // A switch's lane: true is 1, false 0.
    let pk = copy_kernel("x: control<switch> = 0");
    let ir = song(
        json!([{"id": "l", "op": "lane", "lane": "sub"}, {"id": "k", "op": "kernel", "kernel": "@test/t:k", "inputs": {"x": "l"}}]),
        "k",
        json!([{"key": "sub", "kind": "control<switch>", "default": 0}]),
    );
    let clips = json!({"unit": "switch", "clips": [{"start": 0, "length": 1, "points": [[0, true]]}]});
    let (tl, dir) = timeline_for(&ir, &[("data/song/sub.json", clips)]);
    assert_eq!(quarters(&played(&ir, &tl, &pk, 1.0)), [Some(1.0), Some(1.0), Some(0.0), Some(0.0)]);
    std::fs::remove_dir_all(dir).ok();
}

// CARD-15: a point naming an option the choice doesn't have is a warning that names the file,
// the clip, the name and the options; the lane plays its fallback from there to the next point.
#[test]
fn an_unknown_name_is_a_warning_and_plays_the_fallback() {
    let pk = copy_kernel(SHAPE);
    let ir = shape_lane();
    let clips = json!({"unit": "choice", "clips": [{"start": 0, "length": 3, "points": [[0, "sine"], [1, "tri"], [2, "saw"]]}]});
    let dir = project(&[("data/song/shape.json", clips)]);
    let (tl, diags) = Project::open(&dir).unwrap().data(&decls(&ir), None);
    let warned: Vec<_> = diags.iter().filter(|d| d.code.starts_with("lane.")).collect();
    assert_eq!(warned.len(), 1, "{diags:?}");
    let w = warned[0];
    assert_eq!((w.code, w.severity), ("lane.unknown-option", Severity::Warning));
    for says in ["data/song/shape.json", "clip 1 (at beat 0)", "\"tri\"", "\"saw\", \"square\", \"sine\"", "fallback"] {
        assert!(w.message.contains(says), "{says}: {}", w.message);
    }
    let out = played(&ir, &tl, &pk, 2.0);
    assert_eq!(quarters(&out), [Some(2.0), Some(2.0), Some(1.0), Some(1.0), Some(0.0), Some(0.0), Some(1.0), Some(1.0)]);
    std::fs::remove_dir_all(dir).ok();
}

/// The module of the pack fixture `core/session/tests/fixtures/packs/kinds`, as text: `k` copies its
/// one input (a choice) to its sound, and `s` (a switch) the same.
fn kinds_module() -> String {
    module_v1(&format!("{}{}", kernel_v1("k", &out_v1("(local.get $x)")), kernel_v1("s", &out_v1("(local.get $x)"))))
}

// CARD-10, CARD-12: `core/session`'s `a_choice_in_a_pack_plays_its_place` hears a song's choice end to
// end, through this kernel; the fixture's bytes are this text's.
#[test]
fn the_kinds_pack_fixture_holds_this_module() {
    let at = Path::new(env!("CARGO_MANIFEST_DIR")).join("../session/tests/fixtures/packs/kinds/k.wasm");
    assert_eq!(std::fs::read(&at).ok(), Some(wasm(&kinds_module())), "{} is wasm(kinds_module())", at.display());
}
