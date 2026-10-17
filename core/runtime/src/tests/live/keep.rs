//! The arena (step 7, RENDER-25, 25a, 26a, 27, HOST-5): what an activation keeps, what crossfades,
//! what plays out as a ghost, what it refuses and resets, and the plugin instances that move with
//! it, with the probe plugin (`m2_plugins::probe`) and hand-written kernels.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use m2_plugins::probe::{self, Note, Stats};
use m2_plugins::{Handle, Owner};

use super::super::cell::{framed, origin, smooth_const, step_const};
use super::*;

/// 120 bpm: a beat is 24 000 frames.
const BEAT: u64 = 24_000;

mod plugins;

fn probe_packages() -> PackageSet {
    m2_plugins::test_main_thread();
    probe::install();
    std_packages()
}

/// A probe on the notes lane, its gain a constant, keyed `key` when given; `more` adds nodes after
/// it (the output is the last plugin, or `out`).
fn probe_song(pk: &mut PackageSet, gain: f64, key: Option<&str>, more: Value, out: &str) -> Flat {
    let mut p = json!({"id": "p", "op": "plugin", "plugin": "clap:m2.probe", "inputs": {"notes": "n", "gain_1": "g"}});
    if let Some(k) = key {
        p["key"] = json!(k);
    }
    let mut nodes = vec![json!({"id": "n", "op": "lane", "lane": "notes"}), json!({"id": "g", "op": "const", "value": gain}), p];
    nodes.extend(more.as_array().unwrap().iter().cloned());
    plugged(pk, song(Value::Array(nodes), out, json!([{"key": "notes", "kind": "events<note>"}, {"key": "other", "kind": "events<note>"}])))
}

/// `ir` with its plugins bound, checked.
fn plugged(pk: &mut PackageSet, mut ir: Ir) -> Flat {
    let diags = pk.bind_plugins(&mut ir);
    assert!(diags.is_empty(), "{diags:?}");
    check(&ir, &*pk).unwrap_or_else(|e| panic!("{e:?}"))
}

/// A chord of `pitches` over beats 0–3.5, then one note at beat 4.
fn chord(pitches: &[u32]) -> DataSet {
    let mut ns: Vec<Value> = pitches.iter().map(|p| json!({"pitch": p, "start": 0, "length": 3.5})).collect();
    ns.push(json!({"pitch": 72, "start": 4, "length": 1}));
    with_notes(json!({"clips": [{"id": "c", "start": 0, "length": 8, "notes": ns}]}), at_bpm(120.0))
}

/// A plan as the live engine gets one when nothing plays: every instance new, not strict, stopped
/// at 0.
fn build(flat: &Flat, data: &DataSet, pk: &PackageSet, owner: &Owner) -> Renderer {
    let mut r = Renderer::prepare(flat, data, pk, SR, &mut audio::Cache::default(), owner, Basis::Fresh).unwrap();
    r.set_strict(false);
    r.set_transport(Transport::new(0, false));
    r
}

/// An engine playing `flat` from 0, `frames` in.
fn playing(flat: &Flat, data: &DataSet, pk: &PackageSet, frames: usize) -> Rig {
    let mut rig = Rig::of(flat, data, build(flat, data, pk, &Owner::discard()));
    rig.cmd(Cmd::Play);
    rig.run(frames);
    rig
}

fn n(a: &std::sync::atomic::AtomicU64) -> u64 {
    a.load(Ordering::Relaxed)
}

/// The probe's note events from steady time `from` on.
fn notes_from(s: &Stats, from: i64) -> Vec<(i64, Note, i32)> {
    s.notes().into_iter().filter(|e| e.0 >= from).collect()
}

fn eventually(mut f: impl FnMut() -> bool) -> bool {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(2) {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    false
}

/// The top-level node of `flat` named `song/<id>`.
fn node(flat: &Flat, id: &str) -> usize {
    flat.nodes.iter().position(|n| n.name == format!("song/{id}")).unwrap_or_else(|| panic!("no node {id}"))
}

/// One note into sine voices releasing over 50 ms, through a 250 ms delay with feedback, plus a
/// ring (a feedback loop at the top), out of `x`; `more` adds nodes after it.
fn echoes(more: Value, out: &str) -> Ir {
    shaped(0.05, json!({"op": "kernel", "kernel": "std:delay", "inputs": {"in": "v", "time": "t", "feedback": "f", "mix": "m"}}), more, out)
}

/// [`echoes`], its voices releasing over `release`, its delay `d` made `d`.
fn shaped(release: f64, d: Value, more: Value, out: &str) -> Ir {
    let mut d = d;
    d["id"] = json!("d");
    let mut after = vec![
        json!({"id": "t", "op": "const", "value": 0.25, "unit": "seconds"}),
        json!({"id": "f", "op": "const", "value": 0.7}),
        json!({"id": "m", "op": "const", "value": 0.5}),
        d,
        json!({"id": "fb", "op": "feedback", "kind": "audio<1>", "input": "x"}),
        json!({"id": "k", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "fb", "by": "h"}}),
        json!({"id": "h", "op": "const", "value": 0.3}),
        json!({"id": "x", "op": "kernel", "kernel": "std:add", "inputs": {"a": "d", "b": "k"}}),
    ];
    after.extend(more.as_array().unwrap().iter().cloned());
    synth(release, Value::Array(after), out)
}

fn one_note() -> DataSet {
    with_notes(json!({"clips": [{"start": 0, "length": 8, "notes": [{"pitch": 69, "start": 0, "length": 0.5}]}]}), at_bpm(120.0))
}

/// What `flat` plays from 0 for `frames`, with `edit` (the frame and the plan) activated on the way.
fn with_edit(flat: &Flat, data: &DataSet, pk: &PackageSet, frames: usize, edit: Option<(usize, &Flat, &DataSet)>) -> (Vec<[f32; 2]>, Rig) {
    let mut rig = Rig::of(flat, data, build(flat, data, pk, &Owner::discard()));
    rig.cmd(Cmd::Play);
    let Some((at, next, next_data)) = edit else { return (rig.run(frames), rig) };
    let mut out = rig.run(at);
    let r = rig.prepare(next, next_data, pk);
    rig.send(Msg::Activate { renderer: Box::new(r) });
    out.extend(rig.run(frames - at));
    (out, rig)
}

// ---------------------------------------------------------------- RENDER-25: what is kept

// RENDER-25: an activation of the same graph moves every instance into the new plan (voices,
// kernels and a ring), mid-note and mid-echo, and changes no bit: nothing fades, nothing is new.
#[test]
fn an_activation_moves_every_instance_that_continues() {
    let pk = std_packages();
    let f = flat(&echoes(json!([]), "x"));
    let data = one_note();
    let (plain, _) = with_edit(&f, &data, &pk, SR as usize * 3 / 2, None);
    let (edited, rig) = with_edit(&f, &data, &pk, SR as usize * 3 / 2, Some((SR as usize * 6 / 10, &f, &data)));
    assert!(peak(&plain[SR as usize * 6 / 10..SR as usize * 7 / 10]) > 0.05, "an echo sounds at the activation");
    assert!(edited == plain, "an activation that changes nothing changes no bit");
    let s = rig.e.current().unwrap().stats().clone();
    assert_eq!(s.kept, Count { kernels: 3, polys: 1, plugins: 0, rings: 1 });
    assert_eq!((s.fresh.total(), s.ghosts, s.fades, s.resets), (0, 0, 0, 0));
    assert_eq!(rig.e.refused_activations(), 0);
}

// RENDER-25: a node whose shape changed starts from init: another kernel at the same name, a voice
// graph with another plain number in it, a kernel whose input became stereo. What it replaced
// plays out as a ghost, and the output fades to what replaced it.
#[test]
fn a_changed_shape_starts_from_init() {
    let pk = std_packages();
    let data = one_note();
    let base = flat(&echoes(json!([]), "x"));
    let delay = json!({"op": "kernel", "kernel": "std:delay", "inputs": {"in": "v", "time": "t", "feedback": "f", "mix": "m"}});
    let reverb = flat(&shaped(0.05, json!({"op": "kernel", "kernel": "std:reverb", "inputs": {"in": "v"}}), json!([]), "x"));
    let longer = flat(&shaped(0.3, delay, json!([]), "x"));
    for (what, next, fresh) in [
        ("another kernel", reverb, Count { kernels: 1, ..Count::default() }),
        ("a plain number in the voice", longer, Count { polys: 1, ..Count::default() }),
    ] {
        let (_, rig) = with_edit(&base, &data, &pk, SR as usize, Some((SR as usize / 2, &next, &data)));
        let s = rig.e.current().unwrap().stats().clone();
        assert_eq!(s.fresh, fresh, "{what}: {s:?}");
        assert!(s.ghosts >= 1 && s.fades >= 1, "{what}: the replaced one plays out, faded: {s:?}");
    }
    // A kernel whose input became stereo: another channel count, so another shape.
    let mono = flat(&sine_then(json!([{"id": "d", "op": "kernel", "kernel": "std:delay", "inputs": {"in": "o"}}]), "d"));
    let pan = json!([{"id": "pn", "op": "kernel", "kernel": "std:pan", "inputs": {"in": "o"}}, {"id": "d", "op": "kernel", "kernel": "std:delay", "inputs": {"in": "pn"}}]);
    let stereo = flat(&sine_then(pan, "d"));
    let (_, rig) = with_edit(&mono, &DataSet::empty(), &pk, SR as usize / 4, Some((SR as usize / 8, &stereo, &DataSet::empty())));
    let s = rig.e.current().unwrap().stats().clone();
    assert_eq!((s.kept.kernels, s.fresh.kernels), (1, 2), "the sine kept, the pan and the stereo delay new: {s:?}");
}

/// A copy of std whose wasm module holds the same code and one custom section more: another
/// SHA-256, the same kernels.
fn rebuilt_std() -> PathBuf {
    let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std");
    let to = project(&[]).join("std");
    std::fs::create_dir_all(to.join("kernels")).unwrap();
    std::fs::copy(from.join("m2pkg.toml"), to.join("m2pkg.toml")).unwrap();
    let mut wasm = std::fs::read(from.join("kernels/std.wasm")).unwrap();
    // A custom section (id 0) named `m2-test`, one byte of payload.
    wasm.extend_from_slice(&[0, 9, 7, b'm', b'2', b'-', b't', b'e', b's', b't', 42]);
    std::fs::write(to.join("kernels/std.wasm"), wasm).unwrap();
    to
}

// RENDER-25 (G7d): a kernel module rebuilt with the same kernel ids but a new hash starts every
// instance from init: mid-echo, the same graph prepared with the rebuilt std keeps no std
// instance, the echo is gone once the window ends, and the output differs from never activating.
// Its control: std loaded again from the same bytes (a new module, the same hash) keeps every
// instance, and changes no bit.
#[test]
fn a_rebuilt_kernel_module_starts_from_init() {
    let pk = std_packages();
    let f = flat(&echoes(json!([]), "x"));
    let data = one_note();
    let frames = SR as usize * 3 / 2;
    let at = SR as usize * 6 / 10;
    let (plain, _) = with_edit(&f, &data, &pk, frames, None);
    let mut rebuilt = PackageSet::default();
    rebuilt.load(&rebuilt_std()).unwrap();
    assert_ne!(rebuilt.module_sha("std:delay"), pk.module_sha("std:delay"));
    let (same, other) = (std_packages(), rebuilt);
    for (label, packages, kept) in [("same bytes", &same, true), ("rebuilt", &other, false)] {
        let mut rig = Rig::of(&f, &data, build(&f, &data, &pk, &Owner::discard()));
        rig.cmd(Cmd::Play);
        let mut out = rig.run(at);
        let r = rig.prepare(&f, &data, packages);
        rig.send(Msg::Activate { renderer: Box::new(r) });
        out.extend(rig.run(frames - at));
        let s = rig.e.current().unwrap().stats().clone();
        if kept {
            assert_eq!((s.fresh.total(), s.ghosts, s.fades), (0, 0, 0), "{label}: {s:?}");
            assert!(out == plain, "{label}: no bit changes");
        } else {
            assert_eq!((s.kept.kernels, s.kept.polys), (0, 0), "{label}: no std instance kept: {s:?}");
            assert_eq!(s.kept.rings, 1, "{label}: a ring holds no kernel");
            let diff: Vec<[f32; 2]> = out.iter().zip(&plain).map(|(a, b)| [a[0] - b[0], a[1] - b[1]]).collect();
            assert!(peak(&diff[at..]) > 1e-3, "{label}: the output differs from never activating");
            // The new delay started empty: the echo is gone after the window, and the ring's own
            // trickle is all that is left of it.
            assert!(peak(&out[at + 1000..at + 6000]) < 0.5 * peak(&plain[at + 1000..at + 6000]), "{label}: the echo is gone");
        }
    }
}

// RENDER-25: the audio thread checks each move's shape before it moves anything: an entry with one
// wrong shape hash is refused whole, and what plays plays on, bit-identical. So is a plan prepared
// against another plan than the one that plays.
#[test]
fn a_wrong_shape_move_is_refused() {
    let pk = std_packages();
    let f = flat(&echoes(json!([]), "x"));
    let data = one_note();
    let frames = SR as usize;
    let (plain, _) = with_edit(&f, &data, &pk, frames, None);
    let mut rig = Rig::of(&f, &data, build(&f, &data, &pk, &Owner::discard()));
    rig.cmd(Cmd::Play);
    let mut out = rig.run(frames / 2);
    let mut r = rig.prepare(&f, &data, &pk);
    r.plan.entry.as_mut().unwrap().moves[1].shape ^= 1;
    let seq = rig.send(Msg::Activate { renderer: Box::new(r) });
    out.extend(rig.run(frames / 2));
    assert_eq!((rig.e.refused_activations(), rig.e.activations()), (1, 0));
    let (stamps, _) = rig.e.applied();
    assert!(stamps.iter().any(|a| a.seq == seq && a.refused), "its stamp says refused");
    assert!(out == plain, "what played plays on, bit-identical");
    assert_eq!(rig.retired_renderers().len(), 1, "the refused plan is retired unplayed");
}

// RENDER-25, RENDER-28: an activation names the plan it was prepared against; sent to an engine
// playing another, it is refused whole.
#[test]
fn an_activation_against_another_plan_is_refused() {
    let pk = std_packages();
    let f = flat(&echoes(json!([]), "x"));
    let data = one_note();
    let mut a = playing(&f, &data, &pk, BLOCK);
    let r = a.prepare(&f, &data, &pk);
    let mut b = playing(&f, &data, &pk, BLOCK);
    b.send(Msg::Activate { renderer: Box::new(r) });
    b.run(BLOCK);
    assert_eq!((b.e.refused_activations(), b.e.activations()), (1, 0));
}

/// A package `t` of hand-written kernels: `t:latch` (its output turns NaN for good once its input
/// goes over 0.5, until a reset), `t:spike` (infinite while its input is over 0.5, else 1; no
/// state), `t:denan` (NaN and infinities to 0), `t:trap` (traps while its input is over 0.5,
/// else outputs 1) and `t:tiny` (its init and reset keep 1e-30 × 1e-10, a subnormal, which it
/// outputs times 1e40: 1 where subnormals are kept, 0 where they were flushed; active while that
/// product isn't 0).
fn t_kernels() -> PathBuf {
    let dir = project(&[]).join("t");
    std::fs::create_dir_all(&dir).unwrap();
    let port = |k: &str| {
        format!(
            r#"(func (export "{k}_port") (param i32) (result i32) (i32.add (i32.const 1024) (i32.mul (local.get 0) (i32.const 1024))))"#
        )
    };
    let common = |k: &str| {
        format!(
            r#"(func (export "{k}_init") (param f32 i32 i32) (f32.store (i32.const 4096) (f32.const 0)))
               (func (export "{k}_ports") (result i32) (i32.const 257)) {}
               (func (export "{k}_reset") (f32.store (i32.const 4096) (f32.const 0)))
               (func (export "{k}_active") (result i32) (i32.const 0))"#,
            port(k)
        )
    };
    let each = |k: &str, body: &str| {
        format!(
            r#"(func (export "{k}_process") (param $n i32) (local $i i32) (local $x f32)
                 (block $done (loop $l
                   (br_if $done (i32.ge_u (local.get $i) (local.get $n)))
                   (local.set $x (f32.load (i32.add (i32.const 1024) (i32.shl (local.get $i) (i32.const 2)))))
                   {body}
                   (local.set $i (i32.add (local.get $i) (i32.const 1)))
                   (br $l))))"#
        )
    };
    let out = |v: &str| format!("(f32.store (i32.add (i32.const 2048) (i32.shl (local.get $i) (i32.const 2))) {v})");
    let latch = format!(
        "(if (f32.gt (local.get $x) (f32.const 0.5)) (then (f32.store (i32.const 4096) (f32.const nan)))) {}",
        out("(f32.add (f32.load (i32.const 4096)) (f32.const 1))")
    );
    let spike = out("(select (f32.const inf) (f32.const 1) (f32.gt (local.get $x) (f32.const 0.5)))");
    let denan = out("(select (local.get $x) (f32.const 0) (f32.lt (f32.abs (local.get $x)) (f32.const inf)))");
    let trap = format!("(if (f32.gt (local.get $x) (f32.const 0.5)) (then (unreachable))) {}", out("(f32.const 1)"));
    // The factors are loaded from memory, so no compiler multiplies them ahead of time.
    let product = "(f32.mul (f32.load (i32.const 4100)) (f32.load (i32.const 4104)))";
    let tiny = format!(
        r#"(func (export "tiny_init") (param f32 i32 i32)
             (f32.store (i32.const 4100) (f32.const 1e-30)) (f32.store (i32.const 4104) (f32.const 1e-10)) (call $tiny_reset))
           (func (export "tiny_ports") (result i32) (i32.const 257)) {}
           (func $tiny_reset (export "tiny_reset") (f32.store (i32.const 4096) {product}))
           (func (export "tiny_active") (result i32) (f32.ne {product} (f32.const 0)))"#,
        port("tiny")
    ) + &each("tiny", &out("(f32.mul (f32.mul (f32.load (i32.const 4096)) (f32.const 1e30)) (f32.const 1e10))"));
    let wat = format!(
        r#"(module (memory (export "memory") 1) (func (export "m2_abi") (result i32) (i32.const 0)) {} {} {} {} {})"#,
        common("latch") + &each("latch", &latch),
        common("spike") + &each("spike", &spike),
        common("denan") + &each("denan", &denan),
        common("trap") + &each("trap", &trap),
        tiny,
    );
    std::fs::write(dir.join("k.wasm"), wat).unwrap();
    let kernel = |k: &str, input: &str| format!("[[kernel]]\nname = \"{k}\"\ninputs = [\"in: {input}\"]\noutputs = [\"out: audio<1>\"]\n");
    let manifest = format!(
        "[package]\nname = \"t\"\nversion = \"0.1.0\"\nwasm = \"k.wasm\"\n\n{}{}{}{}{}",
        kernel("latch", "control<*>"),
        kernel("spike", "control<*>"),
        kernel("denan", "audio<1>"),
        kernel("trap", "control<*>"),
        kernel("tiny", "control<*>")
    );
    std::fs::write(dir.join("m2pkg.toml"), manifest).unwrap();
    dir
}

fn t_packages() -> PackageSet {
    let mut pk = std_packages();
    pk.load(&t_kernels()).unwrap();
    pk
}

/// Two tracks: `kind` (latch or trap) on a cue cell (site 5), its NaNs mapped to 0, and a sine
/// through a delay; `edit` adds a gain after the delay (an edit on the other track).
fn cued(kind: &str, edit: bool) -> Ir {
    let mut nodes = vec![
        step_const("cue", 0.0, 5),
        json!({"id": "lat", "op": "kernel", "kernel": format!("t:{kind}"), "inputs": {"in": "cue"}}),
        json!({"id": "den", "op": "kernel", "kernel": "t:denan", "inputs": {"in": "lat"}}),
        json!({"id": "fr", "op": "const", "value": 330, "unit": "hz"}),
        json!({"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "fr"}}),
        json!({"id": "d", "op": "kernel", "kernel": "std:delay", "inputs": {"in": "o"}}),
    ];
    let other = if edit {
        nodes.push(json!({"id": "g", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "d", "by": "half"}}));
        nodes.push(json!({"id": "half", "op": "const", "value": 0.5}));
        "g"
    } else {
        "d"
    };
    nodes.push(json!({"id": "x", "op": "kernel", "kernel": "std:add", "inputs": {"a": "den", "b": other}}));
    song(Value::Array(nodes), "x", json!([]))
}

/// Write `v` into the cue cell of what `rig` plays.
fn cue(rig: &Rig, v: f32, seq: u64) {
    let r = rig.e.current().unwrap();
    r.cell_slots().set(r.cell_map().cells(&origin(5)), v, seq);
}

// RENDER-25, RENDER-10: a box whose state went NaN behind a kernel that turns NaN into 0 is reset as
// an edit on another track moves it, and only it: the track sounds again, and every other
// instance goes on bit for bit.
#[test]
fn a_box_that_went_non_finite_is_reset_when_it_moves() {
    let pk = t_packages();
    let (a, b) = (check(&cued("latch", false), &pk).unwrap(), check(&cued("latch", true), &pk).unwrap());
    let run = |poison: bool| {
        let mut rig = playing(&a, &DataSet::empty(), &pk, 1024);
        if poison {
            cue(&rig, 1.0, 1);
            rig.run(BLOCK);
            cue(&rig, 0.0, 2);
        } else {
            rig.run(BLOCK);
        }
        rig.run(4 * BLOCK);
        let lat = rig.e.current().unwrap().output_of(node(&a, "lat")).to_vec();
        let r = rig.prepare(&b, &DataSet::empty(), &pk);
        rig.send(Msg::Activate { renderer: Box::new(r) });
        rig.run(BLOCK);
        let cur = rig.e.current().unwrap();
        (lat, cur.output_of(node(&b, "lat")).to_vec(), cur.output_of(node(&b, "d")).to_vec(), cur.stats().clone())
    };
    let (before, after, delay, s) = run(true);
    assert!(before.iter().all(|x| x.is_nan()), "latched: its state went NaN");
    assert!(after.iter().all(|&x| x == 1.0), "reset as it moved: the track sounds again");
    assert_eq!(s.resets, 1, "{s:?}");
    assert_eq!(s.fresh.total(), 1, "only the gain after the delay is new: {s:?}");
    let (_, clean, clean_delay, s) = run(false);
    assert!(clean.iter().all(|&x| x == 1.0) && s.resets == 0);
    assert_eq!(delay, clean_delay, "the other track's instances go on bit for bit");
}

// RENDER-25: one non-finite block that a box recovers from clears with its next finite block and
// costs nothing: an infinity from a stateless kernel, then an edit elsewhere, resets nothing.
#[test]
fn a_transient_inf_keeps_every_instance() {
    let pk = t_packages();
    let (a, b) = (check(&cued("spike", false), &pk).unwrap(), check(&cued("spike", true), &pk).unwrap());
    let mut rig = playing(&a, &DataSet::empty(), &pk, 1024);
    cue(&rig, 1.0, 1);
    rig.run(BLOCK);
    assert!(rig.e.current().unwrap().output_of(node(&a, "lat")).iter().all(|x| x.is_infinite()));
    cue(&rig, 0.0, 2);
    rig.run(2 * BLOCK);
    let r = rig.prepare(&b, &DataSet::empty(), &pk);
    rig.send(Msg::Activate { renderer: Box::new(r) });
    rig.run(BLOCK);
    assert_eq!(rig.e.current().unwrap().stats().resets, 0, "a stray block costs nothing");
}

/// Two tracks summed in the song: `a`, `t:<kind>` on a cue cell (site 5), and `b`, a sine through
/// a delay; `edit` adds a gain after `b`'s delay (an edit on the other track).
fn two_tracks(kind: &str, edit: bool) -> Ir {
    let a = json!([step_const("cue", 0.0, 5), {"id": "lat", "op": "kernel", "kernel": format!("t:{kind}"), "inputs": {"in": "cue"}}]);
    let mut b = vec![
        json!({"id": "fr", "op": "const", "value": 330, "unit": "hz"}),
        json!({"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "fr"}}),
        json!({"id": "d", "op": "kernel", "kernel": "std:delay", "inputs": {"in": "o"}}),
    ];
    if edit {
        b.push(json!({"id": "half", "op": "const", "value": 0.5}));
        b.push(json!({"id": "g", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "d", "by": "half"}}));
    }
    let track = |key: &str, nodes: Value, out: &str| json!({"key": key, "track": true, "nodes": nodes, "exports": {"out": out}});
    let song = json!([
        {"id": "a", "op": "import", "module": "a", "export": "out"},
        {"id": "b", "op": "import", "module": "b", "export": "out"},
        {"id": "x", "op": "kernel", "kernel": "std:add", "inputs": {"a": "a", "b": "b"}},
    ]);
    let modules = json!([track("a", a, "lat"), track("b", Value::Array(b), if edit { "g" } else { "d" }),
        {"key": "song", "nodes": song, "exports": {"out": "x"}}]);
    serde_json::from_value(json!({"ir": m2_ir::IR_VERSION, "root": {"module": "song", "export": "out"}, "modules": modules})).unwrap()
}

/// The top-level node of `flat` named `name` (`<module>/<id>`).
fn named(flat: &Flat, name: &str) -> usize {
    flat.nodes.iter().position(|n| n.name == name).unwrap_or_else(|| panic!("no node {name}"))
}

/// What [`two_tracks`] plays for `blocks` blocks, its kernel on `a` made to trap at the second
/// block when `trap`, on this thread or with `threads` (spread): the output, then `a/lat`'s and
/// `b/d`'s last block, and the engine.
fn trapped(pk: &PackageSet, trap: bool, blocks: usize, threads: Option<Spread>) -> (Vec<[f32; 2]>, Vec<f32>, Vec<f32>, Rig) {
    let f = check(&two_tracks("trap", false), pk).unwrap();
    let mut rig = Rig::of(&f, &DataSet::empty(), build(&f, &DataSet::empty(), pk, &Owner::discard()));
    rig.threads = threads;
    rig.cmd(Cmd::Play);
    let mut out = rig.run(BLOCK);
    cue(&rig, if trap { 1.0 } else { 0.0 }, 1);
    out.extend(rig.run(BLOCK));
    cue(&rig, 0.0, 2);
    out.extend(rig.run((blocks - 2) * BLOCK));
    let cur = rig.e.current().unwrap();
    let (a, b) = (cur.output_of(named(&f, "a/lat")).to_vec(), cur.output_of(named(&f, "b/d")).to_vec());
    (out, a, b, rig)
}

// RENDER-25: live, a kernel that traps goes silent alone: the other track plays on bit for bit, the
// transport plays, nothing counts as a crash, and the plan marks the node for the session to tell.
#[test]
fn a_trapped_kernel_goes_silent_alone() {
    let pk = t_packages();
    let lat = named(&check(&two_tracks("trap", false), &pk).unwrap(), "a/lat");
    let (out, a, b, rig) = trapped(&pk, true, 8, None);
    let (clean, _, clean_b, clean_rig) = trapped(&pk, false, 8, None);
    let cur = rig.e.current().unwrap();
    assert!(a.iter().all(|&x| x == 0.0), "its track is silent");
    assert_eq!(b, clean_b, "the other track plays on bit for bit");
    assert!(cur.transport().playing() && rig.e.crashes() == 0, "the song plays on");
    assert_eq!(cur.info().stopped.why(lat), Some(Why::Crashed));
    assert_eq!(clean_rig.e.current().unwrap().info().stopped.why(lat), None);
    // The song is the other track alone now, and before the trap both.
    let last: Vec<f32> = out[out.len() - BLOCK..].iter().map(|f| f[0]).collect();
    assert_eq!(last, b, "what plays is the other track, exactly");
    assert_eq!(out[..BLOCK], clean[..BLOCK]);
}

// RENDER-25, RENDER-10: a stopped kernel is reset as the next activation moves it, and plays again;
// the new plan has no mark.
#[test]
fn a_stopped_kernel_starts_again_at_the_next_activation() {
    let pk = t_packages();
    let b = check(&two_tracks("trap", true), &pk).unwrap();
    let (_, _, _, mut rig) = trapped(&pk, true, 6, None);
    let r = rig.prepare(&b, &DataSet::empty(), &pk);
    rig.send(Msg::Activate { renderer: Box::new(r) });
    let out = rig.run(4 * BLOCK);
    let cur = rig.e.current().unwrap();
    assert_eq!(cur.stats().resets, 1, "it was reset as it moved");
    assert!(cur.output_of(named(&b, "a/lat")).iter().all(|&x| x == 1.0), "it plays again");
    assert_eq!(cur.info().stopped.why(named(&b, "a/lat")), None, "the new plan has no mark");
    assert!(peak(&out) > 0.5);
}

// RENDER-25, RENDER-10: Stop All Sound resets every instance, so a stopped kernel plays again, and
// its mark goes; one that fails again is marked again, and told again.
#[test]
fn stop_all_sound_starts_a_stopped_kernel_again() {
    let pk = t_packages();
    let f = check(&two_tracks("trap", false), &pk).unwrap();
    let lat = named(&f, "a/lat");
    let (_, _, _, mut rig) = trapped(&pk, true, 4, None);
    let stopped = rig.e.current().unwrap().info().stopped.clone();
    assert_eq!(stopped.news(), [(lat, Why::Crashed)]);
    rig.send(Msg::Panic);
    rig.run(2 * BLOCK);
    assert!(rig.e.current().unwrap().output_of(lat).iter().all(|&x| x == 1.0), "it plays again");
    assert_eq!((stopped.why(lat), stopped.news()), (None, vec![]));
    cue(&rig, 1.0, 3);
    rig.run(BLOCK);
    assert_eq!(stopped.news(), [(lat, Why::Crashed)], "stopped again, and told again");
    assert!(stopped.news().is_empty(), "once");
}

// RENDER-2, RENDER-25: a trap stops the same node at the same sample on any number of threads, so
// the same bits come out.
#[test]
fn a_trap_on_any_thread_count_gives_the_same_bits() {
    let pk = t_packages();
    let lat = named(&check(&two_tracks("trap", false), &pk).unwrap(), "a/lat");
    let (one, ..) = trapped(&pk, true, 8, None);
    let (four, _, _, rig) = trapped(&pk, true, 8, Some(spread(4)));
    assert!(one == four, "the same bits on 1 and 4 threads");
    assert_eq!(rig.e.current().unwrap().info().stopped.why(lat), Some(Why::Crashed));
}

// RENDER-25: offline a trap still fails the render, naming the node: an export never leaves a sound
// out.
#[test]
fn a_trap_still_fails_an_export_and_names_the_node() {
    let pk = t_packages();
    let mut ir = two_tracks("trap", false);
    ir.modules[0].nodes[0] = serde_json::from_value(json!({"id": "cue", "op": "const", "value": 1})).unwrap();
    let f = check(&ir, &pk).unwrap();
    let e = Renderer::new(&f, &DataSet::empty(), &pk, SR).unwrap().render(0.01).map(drop).unwrap_err();
    assert!(format!("{e:#}").contains("a/lat crashed"), "{e:#}");
}

// RENDER-10: a panic resets every box and clears every mark: the activation after it resets
// nothing.
#[test]
fn panic_clears_every_flag() {
    let pk = t_packages();
    let (a, b) = (check(&cued("latch", false), &pk).unwrap(), check(&cued("latch", true), &pk).unwrap());
    let mut rig = playing(&a, &DataSet::empty(), &pk, 1024);
    cue(&rig, 1.0, 1);
    rig.run(BLOCK);
    cue(&rig, 0.0, 2);
    rig.send(Msg::Panic);
    rig.run(BLOCK);
    let r = rig.prepare(&b, &DataSet::empty(), &pk);
    rig.send(Msg::Activate { renderer: Box::new(r) });
    rig.run(BLOCK);
    let cur = rig.e.current().unwrap();
    assert_eq!(cur.stats().resets, 0);
    assert!(cur.output_of(node(&b, "lat")).iter().all(|&x| x == 1.0), "the panic reset it");
}

// RENDER-2a: what runs a kernel or a plugin outside a block computes in the one mode too, and
// leaves the thread as it found it: a build, an activation that resets a poisoned kernel and
// plugin, the idle check and a seek, all on a thread that flushes subnormals to zero (an audio
// device's may). `t:tiny` outputs 0 where its state was computed flushing.
#[test]
fn between_blocks_kernels_and_plugins_compute_in_the_one_mode() {
    std::thread::spawn(|| {
        let mut pk = probe_packages();
        pk.load(&t_kernels()).unwrap();
        let tiny = [
            json!({"id": "c", "op": "const", "value": 0.0}),
            json!({"id": "tiny", "op": "kernel", "kernel": "t:tiny", "inputs": {"in": "c"}}),
        ];
        let with_probe = |pk: &mut PackageSet, gain: f64| {
            let mut more = tiny.to_vec();
            more.push(json!({"id": "tp", "op": "kernel", "kernel": "std:pan", "inputs": {"in": "tiny"}}));
            more.push(json!({"id": "x", "op": "kernel", "kernel": "std:add", "inputs": {"a": "p", "b": "tp"}}));
            probe_song(pk, gain, None, Value::Array(more), "x")
        };
        let (a, b) = (with_probe(&mut pk, 1.0), with_probe(&mut pk, 0.5));
        let alone = check(&song(Value::Array(tiny.to_vec()), "tiny", json!([])), &pk).unwrap();
        let ones = |r: &Renderer, flat: &Flat| r.output_of(node(flat, "tiny")).iter().all(|&x| x > 0.5);
        crate::fpenv::flush_to_zero();
        let mut rig = playing(&a, &DataSet::empty(), &pk, BLOCK);
        assert!(ones(rig.e.current().unwrap(), &a), "built in the one mode");
        assert!(crate::fpenv::flushes(), "a build leaves the thread flushing");
        for id in ["p", "tiny"] {
            rig.e.current_mut().unwrap().plan.graph.poisoned[node(&a, id)] = true;
        }
        let r = rig.prepare(&b, &DataSet::empty(), &pk);
        rig.send(Msg::Activate { renderer: Box::new(r) });
        rig.run(BLOCK);
        let cur = rig.e.current().unwrap();
        assert_eq!(cur.stats().resets, 2, "the probe and the kernel were reset as they moved");
        assert!(ones(cur, &b), "the kernel was reset in the one mode");
        assert!(crate::fpenv::flushes(), "the plugin's reset left the thread as it found it");
        // A plan with no plugin: a probe that plays never lets the engine idle.
        let mut r = Renderer::new(&alone, &DataSet::empty(), &pk, SR).unwrap();
        assert!(!r.can_idle(), "the kernel says it is active, in the one mode");
        r.seek(BLOCK as u64).unwrap();
        assert!(ones(&r, &alone), "a seek resets in the one mode");
        assert!(crate::fpenv::flushes(), "and leaves the thread flushing");
    })
    .join()
    .unwrap();
}

// RENDER-25: a prepare that fails (here a kernel that isn't loaded, after a plugin instance was
// created) changes nothing that plays: what it made is dropped, and the plan that plays plays on.
#[test]
fn a_failed_prepare_leaves_what_plays_untouched() {
    let mut pk = probe_packages();
    let owner = Owner::discard();
    let data = chord(&[60]);
    let a = probe_song(&mut pk, 1.0, None, json!([]), "p");
    let mut tk = t_packages();
    let mut ir = song(
        json!([{"id": "n", "op": "lane", "lane": "notes"}, {"id": "q", "op": "plugin", "plugin": "clap:m2.probe", "inputs": {"notes": "n"}},
            {"id": "c", "op": "const", "value": 0}, {"id": "l", "op": "kernel", "kernel": "t:spike", "inputs": {"in": "c"}},
            {"id": "pl", "op": "kernel", "kernel": "std:pan", "inputs": {"in": "l"}},
            {"id": "s", "op": "kernel", "kernel": "std:add", "inputs": {"a": "q", "b": "pl"}}]),
        "s",
        json!([{"key": "notes", "kind": "events<note>"}]),
    );
    assert!(tk.bind_plugins(&mut ir).is_empty());
    let b = check(&ir, &tk).unwrap();
    let mut rig = Rig::of(&a, &data, build(&a, &data, &pk, &owner));
    rig.cmd(Cmd::Play);
    rig.run(BEAT as usize / 2);
    let before = (owner.live(), rig.e.current().unwrap().plugin_handles());
    let failed = rig.prepare_with(&b, &data, &pk, &owner, &Continues::named(&a, &b));
    assert!(failed.is_err_and(|e| format!("{e:#}").contains("not loaded")));
    assert!(eventually(|| owner.live() == before.0), "what it created was dropped");
    assert_eq!(rig.e.current().unwrap().plugin_handles(), before.1);
    // The next plan is still prepared against what plays.
    let r = rig.prepare(&a, &data, &pk);
    rig.send(Msg::Activate { renderer: Box::new(r) });
    rig.run(BLOCK);
    assert_eq!((rig.e.activations(), rig.e.refused_activations()), (1, 0));
    assert_eq!(rig.e.current().unwrap().plugin_handles(), before.1, "the instance moved on");
}

// ---------------------------------------------------------------- RENDER-25a: what fades

/// A 440 Hz sine `o`, then `more`, out of `out`.
fn sine_then(more: Value, out: &str) -> Ir {
    let mut nodes = vec![
        json!({"id": "f", "op": "const", "value": 440, "unit": "hz"}),
        json!({"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "f"}}),
    ];
    nodes.extend(more.as_array().unwrap().iter().cloned());
    song(Value::Array(nodes), out, json!([]))
}

/// The output and the taps of `nodes`, block by block, for `blocks` blocks.
fn taps(rig: &mut Rig, nodes: &[usize], blocks: usize) -> (Vec<[f32; 2]>, Vec<Vec<f32>>) {
    let (mut out, mut taps) = (vec![], vec![vec![]; nodes.len()]);
    for _ in 0..blocks {
        out.extend(rig.run(BLOCK));
        let cur = rig.e.current().unwrap();
        for (t, &n) in taps.iter_mut().zip(nodes) {
            t.extend_from_slice(&cur.output_of(n)[..BLOCK]);
        }
    }
    (out, taps)
}

// RENDER-25a: an effect inserted after a kept sine first hears the sine fade in from silence over
// 5 ms, while the output plays the dry side, the very signal that played; then the output fades
// from dry to wet over the next 5 ms, exactly as `xfade` says, and plays the wet side exactly.
#[test]
fn an_inserted_effect_fades_dry_to_wet() {
    let pk = std_packages();
    let (a, b) = (
        flat(&sine_then(json!([]), "o")),
        flat(&sine_then(json!([{"id": "d", "op": "kernel", "kernel": "std:delay", "inputs": {"in": "o"}}]), "d")),
    );
    let mut rig = playing(&a, &DataSet::empty(), &pk, 8 * BLOCK);
    let r = rig.prepare(&b, &DataSet::empty(), &pk);
    rig.send(Msg::Activate { renderer: Box::new(r) });
    // The output's crossfade, then the delay input's fade-in.
    let n = b.nodes.len();
    let (out, t) = taps(&mut rig, &[node(&b, "o"), node(&b, "d"), n + 1], 8);
    let s = rig.e.current().unwrap().stats().clone();
    assert_eq!((s.kept.kernels, s.fresh.kernels, s.ghosts, s.fades), (1, 1, 0, 2), "{s:?}");
    let len = 240usize;
    for k in 0..len {
        assert_eq!(t[2][k].to_bits(), xfade(0.0, t[0][k], k as u32, len as u32).to_bits(), "frame {k}: the delay hears the sine fade in");
        assert_eq!(out[k][0].to_bits(), t[0][k].to_bits(), "frame {k}: dry while it does");
    }
    for (k, f) in out.iter().enumerate().skip(len).take(len) {
        assert_eq!(f[0].to_bits(), xfade(t[0][k], t[1][k], (k - len) as u32, len as u32).to_bits(), "frame {k}");
    }
    assert!(out[2 * len..].iter().zip(&t[1][2 * len..]).all(|(f, w)| f[0] == *w), "the wet side exactly after the window");
}

// RENDER-25a, RENDER-6a: an edit that lands while the engine idles (stopped and silent: the common
// "stop, then edit and save") fades in as one made while it plays. A new drone added at the output
// is silent while its input fades in, then ramps in over 5 ms. Before, the window started at the
// clock the renderer last rendered at, long gone by, so it was over at once and the drone came in
// at full level: a step of 1.0.
#[test]
fn an_edit_on_an_idle_engine_fades_in() {
    let pk = std_packages();
    let x = json!([{"id": "h", "op": "const", "value": 0.5}, {"id": "x", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "v", "by": "h"}}]);
    let mut drone = x.as_array().unwrap().clone();
    drone.extend([
        json!({"id": "hz", "op": "const", "value": 110, "unit": "hz"}),
        json!({"id": "tri", "op": "kernel", "kernel": "std:tri", "inputs": {"freq": "hz"}}),
        json!({"id": "y", "op": "kernel", "kernel": "std:add", "inputs": {"a": "x", "b": "tri"}}),
    ]);
    let (a, b) = (flat(&synth(0.05, x, "x")), flat(&synth(0.05, Value::Array(drone), "y")));
    let data = one_note();
    let mut rig = Rig::of(&a, &data, build(&a, &data, &pk, &Owner::discard()));
    rig.run(SR as usize);
    assert!(rig.e.is_idle(), "stopped and silent, the engine idles");
    let r = rig.prepare(&b, &data, &pk);
    rig.send(Msg::Activate { renderer: Box::new(r) });
    let out = rig.run(8 * BLOCK);
    assert_eq!(rig.e.current().unwrap().stats().fresh.kernels, 2, "the drone and the add are new");
    let fade = 240;
    assert!(out[..fade].iter().all(|f| *f == [0.0; 2]), "silent while the add's input fades in");
    assert!(peak(&out[2 * fade..]) > 0.9, "then the drone plays");
    let step = max_step(&out);
    assert!(step < 0.02, "it ramps in over 5 ms: a step of {step}");
}

// RENDER-25a (G6): a delay inserted on a sounding sine never takes a step into its memory, so its
// first echo starts as smoothly as the sine moves: no sample-to-sample step over twice the sine's.
#[test]
fn an_inserted_delay_plays_back_no_step() {
    let pk = std_packages();
    let fx = json!([{"id": "t", "op": "const", "value": 0.01, "unit": "seconds"}, {"id": "d", "op": "kernel", "kernel": "std:delay", "inputs": {"in": "o", "time": "t"}}]);
    let (a, b) = (flat(&sine_then(json!([]), "o")), flat(&sine_then(fx, "d")));
    let frames = 16 * BLOCK;
    let (out, rig) = with_edit(&a, &DataSet::empty(), &pk, frames, Some((4 * BLOCK, &b, &DataSet::empty())));
    assert_eq!(rig.e.current().unwrap().stats().fresh.kernels, 1);
    let own = (std::f32::consts::TAU * 440.0 / SR as f32).sin();
    let step = out.windows(2).map(|w| (w[1][0] - w[0][0]).abs()).fold(0.0, f32::max);
    assert!(step <= 2.0 * own, "a step of {step} against the sine's own {own}");
}

// RENDER-25a: a removed effect plays out its window as a ghost, reading the kept sine, while the
// output fades back to dry; at the window's end it leaves the audio thread in the leftovers.
#[test]
fn a_removed_effect_rings_out_as_a_ghost_then_goes() {
    let pk = std_packages();
    let fx = json!([{"id": "t", "op": "const", "value": 0.01, "unit": "seconds"}, {"id": "d", "op": "kernel", "kernel": "std:delay", "inputs": {"in": "o", "time": "t"}}]);
    let (a, b) = (flat(&sine_then(fx, "d")), flat(&sine_then(json!([]), "o")));
    let mut rig = playing(&a, &DataSet::empty(), &pk, 8 * BLOCK);
    let r = rig.prepare(&b, &DataSet::empty(), &pk);
    rig.send(Msg::Activate { renderer: Box::new(r) });
    let ghost = b.nodes.len();
    let (out, t) = taps(&mut rig, &[node(&b, "o"), ghost], 2);
    let s = rig.e.current().unwrap().stats().clone();
    assert_eq!((s.kept.kernels, s.ghosts, s.fades), (1, 1, 1), "{s:?}");
    for (k, f) in out.iter().enumerate().take(240) {
        assert_eq!(f[0].to_bits(), xfade(t[1][k], t[0][k], k as u32, 240).to_bits(), "frame {k}: the ghost fades to dry");
    }
    assert!(t[1].iter().any(|x| *x != 0.0), "the ghost plays");
    rig.run(BLOCK);
    let left: Vec<&Leftovers> = rig.retired.iter().filter_map(|r| if let Retired::Nodes(l) = r { Some(&**l) } else { None }).collect();
    assert_eq!(left.len(), 1, "the window's end retired its leftovers");
    assert!(matches!(left[0].exec[..], [Exec::Kernel(KernelNode { inst: Some(_), .. })]), "with the ghost and its instance");
    assert!(!rig.e.in_window());
}

// RENDER-25a: an edit that changes no sample (an exact multiply by 1 inserted) changes no bit, its
// window included: equal sides stay equal through the fade.
#[test]
fn a_silent_edit_changes_no_bit() {
    let pk = std_packages();
    let unit = json!([{"id": "one", "op": "const", "value": 1}, {"id": "u", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "o", "by": "one"}}]);
    let (a, b) = (flat(&sine_then(json!([]), "o")), flat(&sine_then(unit, "u")));
    let frames = 8 * BLOCK;
    let (plain, _) = with_edit(&a, &DataSet::empty(), &pk, frames, None);
    let (edited, rig) = with_edit(&a, &DataSet::empty(), &pk, frames, Some((3 * BLOCK, &b, &DataSet::empty())));
    // The scale's input fades in, then the output crossfades to it: both exact on equal sides.
    assert_eq!(rig.e.current().unwrap().stats().fades, 2);
    assert!(edited == plain);
}

// RENDER-25a, the cycle rule (Q4): reordering two kept effects would make the window's graph
// cyclic, so both restart, and nothing else does: the other track's delay keeps its echoes, bit
// for bit.
#[test]
fn an_effect_reorder_replaces_the_cycle_and_keeps_the_rest() {
    let pk = std_packages();
    let order = |first: &str, second: &str| {
        let (a_in, b_in) = if first == "a" { ("o", "a") } else { ("b", "o") };
        let last = second;
        flat(&sine_then(
            json!([
                {"id": "a", "op": "kernel", "kernel": "std:delay", "inputs": {"in": a_in}},
                {"id": "b", "op": "kernel", "kernel": "std:reverb", "inputs": {"in": b_in}},
                {"id": "f2", "op": "const", "value": 220, "unit": "hz"},
                {"id": "o2", "op": "kernel", "kernel": "std:saw", "inputs": {"freq": "f2"}},
                {"id": "y", "op": "kernel", "kernel": "std:delay", "inputs": {"in": "o2"}},
                {"id": "s", "op": "kernel", "kernel": "std:add", "inputs": {"a": last, "b": "y"}}]),
            "s",
        ))
    };
    let (ab, ba) = (order("a", "b"), order("b", "a"));
    let y = node(&ab, "y");
    let run = |edit: bool| {
        let mut rig = playing(&ab, &DataSet::empty(), &pk, 40 * BLOCK);
        if edit {
            let r = rig.prepare(&ba, &DataSet::empty(), &pk);
            rig.send(Msg::Activate { renderer: Box::new(r) });
        }
        let (_, t) = taps(&mut rig, &[y], 8);
        (t, rig)
    };
    let (edited, rig) = run(true);
    let s = rig.e.current().unwrap().stats().clone();
    assert_eq!(s.fresh.kernels, 2, "the reordered pair restarts: {s:?}");
    assert!(s.fresh_nodes.iter().all(|&i| i == node(&ba, "a") || i == node(&ba, "b")), "{s:?}");
    assert_eq!(s.kept.kernels, 4, "both sines, the other delay and the sum are kept: {s:?}");
    assert_eq!(run(false).0, edited, "the other track's delay goes on bit for bit");
}

// RENDER-25a: a ghost that reads a control lane the new plan no longer reads reads that lane for
// its window, moved in as a ghost lane, not its default.
#[test]
fn a_ghost_reading_a_removed_control_lane_reads_its_ghost_lane() {
    let pk = std_packages();
    let lanes = json!([{"key": "lvl", "kind": "control<gain>", "default": 0}]);
    let mut a = sine_then(
        json!([{"id": "c", "op": "lane", "lane": "lvl"}, {"id": "g", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "o", "by": "c"}}]),
        "g",
    );
    a.modules[0].lanes = serde_json::from_value(lanes.clone()).unwrap();
    let mut b = sine_then(json!([]), "o");
    b.modules[0].lanes = serde_json::from_value(lanes).unwrap();
    let (data, dir) = timeline_for(
        &a,
        &[("data/song/lvl.json", json!({"unit": "gain", "clips": [{"start": 0, "length": 8, "points": [[0, 0.5], [8, 0.5]]}]}))],
    );
    let (fa, fb) = (flat(&a), flat(&b));
    let mut rig = playing(&fa, &data, &pk, 8 * BLOCK);
    let r = rig.prepare(&fb, &data, &pk);
    assert_eq!(r.lanes.slots.len() - r.lanes.live, 1, "one ghost lane");
    rig.send(Msg::Activate { renderer: Box::new(r) });
    let ghost = fb.nodes.len();
    let (_, t) = taps(&mut rig, &[node(&fb, "o"), ghost], 1);
    assert!(t[0].iter().zip(&t[1]).all(|(o, g)| *g == o * 0.5), "the ghost reads the lane's 0.5");
    assert!(t[0].iter().any(|x| x.abs() > 0.1));
    std::fs::remove_dir_all(dir).ok();
}

// RENDER-25: a feedback ring moves with its plan: no 128-sample hole, no bit changed.
#[test]
fn feedback_rings_survive_an_activation() {
    let pk = std_packages();
    let ring = json!([
        {"id": "fb", "op": "feedback", "kind": "audio<1>", "input": "x"},
        {"id": "k", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "fb", "by": "h"}},
        {"id": "h", "op": "const", "value": 0.9},
        {"id": "x", "op": "kernel", "kernel": "std:add", "inputs": {"a": "o", "b": "k"}}]);
    let f = flat(&sine_then(ring, "x"));
    let (plain, _) = with_edit(&f, &DataSet::empty(), &pk, 20 * BLOCK, None);
    let (edited, rig) = with_edit(&f, &DataSet::empty(), &pk, 20 * BLOCK, Some((7 * BLOCK, &f, &DataSet::empty())));
    assert_eq!(rig.e.current().unwrap().stats().kept.rings, 1);
    assert!(edited == plain);
}

// ---------------------------------------------------------------- RENDER-26a: notes

/// A chord playing into a probe, then a plan with the probe's gain edited, activated at beat 1:
/// the rig, the probe's handle and record, and the engine clock of the activation's block.
fn kept_probe(new_data: &DataSet) -> (Rig, Handle, Arc<Stats>, u64, Owner) {
    let mut pk = probe_packages();
    let owner = Owner::discard();
    let data = chord(&[57, 60, 64]);
    let flat = probe_song(&mut pk, 1.0, None, json!([]), "p");
    let r1 = build(&flat, &data, &pk, &owner);
    let h = r1.plugin_handles()[0];
    let mut rig = Rig::of(&flat, &data, r1);
    rig.cmd(Cmd::Play);
    rig.run(BEAT as usize);
    let edited = probe_song(&mut pk, 0.5, None, json!([]), "p");
    let r2 = rig.prepare_with(&edited, new_data, &pk, &owner, &Continues::named(&flat, &edited)).unwrap();
    assert_eq!(owner.live(), 1, "the plan made no instance");
    let at = rig.e.clock();
    rig.send(Msg::Activate { renderer: Box::new(r2) });
    (rig, h, probe::stats(h).unwrap(), at, owner)
}

// HOST-5, RENDER-25, RENDER-26a: a plan whose code still names the probe keeps its instance, with
// its state and the notes sounding into it: the chord goes on with the same ids, no Off reaches
// the probe at the activation, and its Offs come at the chord's own end (beat 3.5). The new
// code's value is sent at once. The instance is processed once per engine block.
#[test]
fn a_kept_consumer_keeps_its_notes() {
    let (mut rig, h, stats, at, _owner) = kept_probe(&chord(&[57, 60, 64]));
    let before = n(&stats.process);
    let ons: Vec<i32> = stats.notes().iter().filter(|e| e.1 == Note::On).map(|e| e.2).collect();
    assert_eq!(ons, [1, 2, 3]);
    rig.run(BLOCK);
    let views = rig.e.current().unwrap().plugins();
    assert_eq!((views[0].handle, views[0].notes.clone(), views[0].offs), (h, vec![(1, 57), (2, 60), (3, 64)], 0), "the chord moved");
    let gain = m2_plugins::host().main_call(h, m2_plugins::MainCall::Value(probe::GAIN));
    assert!(matches!(gain, Ok(m2_plugins::Reply::Value(Some(v))) if v == 0.5), "the new code's gain is sent at once: {gain:?}");
    rig.run(4 * BLOCK);
    assert_eq!(n(&stats.process) - before, 5, "once per engine block");
    assert_eq!(n(&stats.doubled), 0);
    // Nothing ends at the activation; the chord ends where it was written.
    rig.run(3 * BEAT as usize);
    let after = notes_from(&stats, at as i64);
    let offs: Vec<(i64, i32)> = after.iter().filter(|e| e.1 != Note::On).map(|e| (e.0, e.2)).collect();
    let end = (3 * BEAT + BEAT / 2) as i64;
    assert_eq!(offs.iter().map(|o| o.1).collect::<Vec<_>>(), [1, 2, 3], "{after:?}");
    assert!(offs.iter().all(|o| o.0 == end), "the chord's Offs at beat 3.5: {offs:?}");
    assert_eq!(n(&stats.unknown_offs) + n(&stats.repeated_ids), 0);
    let ons: Vec<i32> = after.iter().filter(|e| e.1 == Note::On).map(|e| e.2).collect();
    assert_eq!(ons, [4], "the note at beat 4 takes the next id");
    rig.run(BEAT as usize);
    assert_eq!(rig.e.current().unwrap().held_notes(), 0, "nothing stuck");
}

// RENDER-26a: a note the edit removed is let go at the activation's first frame (its scheduler
// moved with its notes and reconciles them with the new data); the others go on. Ids never
// repeat into the kept instance.
#[test]
fn notes_the_edit_removed_are_let_go() {
    let (mut rig, _h, stats, at, _owner) = kept_probe(&chord(&[57, 60]));
    rig.run(BEAT as usize * 4);
    let after = notes_from(&stats, at as i64);
    let (first, rest) = after.split_first().expect("events after the activation");
    assert_eq!(*first, (at as i64, Note::Off, 3), "the dropped top note, at the activation");
    let end = (3 * BEAT + BEAT / 2) as i64;
    assert_eq!(rest[..2].iter().map(|e| (e.0, e.1, e.2)).collect::<Vec<_>>(), [(end, Note::Off, 1), (end, Note::Off, 2)]);
    assert_eq!(rest[2].1, Note::On);
    assert!(rest[2].2 > 3, "a new note never takes an id the instance had: {after:?}");
    assert_eq!(n(&stats.repeated_ids) + n(&stats.unknown_offs), 0);
}

// RENDER-26a, HOST-5: ids never repeat into a kept instance, not even across a plan whose node read
// no notes (the code took `notes:` off the plugin, then put it back): note ids are the engine's.
#[test]
fn note_ids_never_repeat_into_a_plugin() {
    let mut pk = probe_packages();
    let owner = Owner::discard();
    let without = plugged(
        &mut pk,
        song(
            json!([{"id": "p", "op": "plugin", "plugin": "clap:m2.probe", "inputs": {}}]),
            "p",
            json!([{"key": "notes", "kind": "events<note>"}]),
        ),
    );
    let with = probe_song(&mut pk, 1.0, None, json!([]), "p");
    let r0 = build(&with, &chord(&[57, 60, 64]), &pk, &owner);
    let h = r0.plugin_handles()[0];
    let stats = probe::stats(h).unwrap();
    let mut rig = Rig::of(&with, &chord(&[57, 60, 64]), r0);
    rig.cmd(Cmd::Play);
    // The chord (ids 1 to 3), and the note at beat 4 (id 4).
    rig.run((4 * BEAT + BEAT / 4) as usize);
    let r1 = rig.prepare_with(&without, &chord(&[57, 60, 64]), &pk, &owner, &Continues::named(&with, &without)).unwrap();
    rig.send(Msg::Activate { renderer: Box::new(r1) });
    rig.run(BEAT as usize);
    assert_eq!(rig.e.current().unwrap().plugin_handles(), [h], "kept without its notes");
    let note = json!({"pitch": 62, "start": 7, "length": 1});
    let later = with_notes(json!({"clips": [{"id": "c", "start": 0, "length": 16, "notes": [note]}]}), at_bpm(120.0));
    let r2 = rig.prepare_with(&with, &later, &pk, &owner, &Continues::named(&without, &with)).unwrap();
    rig.send(Msg::Activate { renderer: Box::new(r2) });
    rig.run(3 * BEAT as usize);
    assert_eq!(rig.e.current().unwrap().plugin_handles(), [h], "and with them again");
    let ons: Vec<i32> = stats.notes().iter().filter(|e| e.1 == Note::On).map(|e| e.2).collect();
    assert_eq!(ons, [1, 2, 3, 4, 5], "the note at beat 7 takes an id the instance never had");
    assert_eq!(n(&stats.repeated_ids) + n(&stats.unknown_offs), 0);
}

/// What an export does with the live instances `lent` lends, on this thread while the engine holds
/// (RENDER-1, `Lent` in `crates/server/src/export.rs`): each is taken offline (deactivated and
/// activated again), the export's renderer plays `flat` with them from 0 for `frames` (its first
/// call resets each), and each is given back, active again, its code's parameters sent again.
fn export_with(flat: &Flat, data: &DataSet, pk: &PackageSet, lent: &HashMap<usize, Arc<PluginInstance>>, frames: u64) {
    let cycle = |inst: &PluginInstance| {
        let (host, h) = (inst.host(), inst.handle());
        host.deactivate(h);
        host.activate(h, SR, BLOCK as u32).unwrap();
    };
    lent.values().for_each(|i| cycle(i));
    let mut r = Renderer::lending(flat, data, pk, SR, lent).unwrap();
    for k in 0..frames.div_ceil(BLOCK as u64) {
        r.render_block(k * BLOCK as u64).unwrap();
    }
    drop(r);
    for i in lent.values() {
        cycle(i);
        i.mark_reloaded();
    }
}

// RENDER-1 (the owner's decision of 2026-10-15): a note held across an export plays again after
// the resume. The export ended every note playback left in the plugin it borrowed, so at the resume
// block that plugin gets a NoteOn for each note the song still holds, with the note's own id, which
// its Off later ends. None for a note that ended before the hold, none for one an edit made while
// held removed (and no Off for it either: the plugin no longer holds it), and nothing for a plugin
// the export didn't borrow, whose voices were only frozen.
#[test]
fn a_note_held_across_an_export_plays_again_at_the_resume() {
    let mut pk = probe_packages();
    let q = json!([{"id": "q", "op": "plugin", "plugin": "clap:m2.probe", "inputs": {"in": "p", "notes": "n"}}]);
    let flat = probe_song(&mut pk, 1.0, None, q, "q");
    let notes = |pitches: &[(u32, f64)]| {
        let ns: Vec<Value> = pitches.iter().map(|&(p, len)| json!({"pitch": p, "start": 0, "length": len})).collect();
        with_notes(json!({"clips": [{"id": "c", "start": 0, "length": 8, "notes": ns}]}), at_bpm(120.0))
    };
    // A long note, one that ends before the hold, and one the edit made while held removes.
    let data = notes(&[(60, 4.0), (64, 0.5), (67, 4.0)]);
    let r = build(&flat, &data, &pk, &Owner::discard());
    let [hp, hq] = r.plugin_handles()[..] else { panic!("two probes") };
    let (sp, sq) = (probe::stats(hp).unwrap(), probe::stats(hq).unwrap());
    let mut rig = Rig::of(&flat, &data, r);
    rig.cmd(Cmd::Play);
    rig.run(BEAT as usize);
    let held: Vec<(u32, i16)> = rig.e.current().unwrap().plugins()[0].notes.clone();
    let id = |key: i16| held.iter().find(|k| k.1 == key).map(|k| k.0 as i32).unwrap();
    assert_eq!(held.len(), 2, "the long notes sound: {held:?}");
    rig.send(Msg::Hold(true));
    rig.run(4 * BLOCK);
    assert!(rig.e.is_held());
    let generation = rig.e.current().unwrap().generation();
    let edited = notes(&[(60, 4.0), (64, 0.5)]);
    rig.send(Msg::SwapLane { generation, slot: 0, data: LaneSwap::Notes(Arc::new(notes_data(&edited, "song", "notes", SR as f64))) });
    rig.run(BLOCK);
    let mut lent = rig.e.current().unwrap().info().lend(&flat, &edited, &pk, &Continues::identity(&flat));
    lent.retain(|&i, _| i == node(&flat, "p"));
    export_with(&flat, &edited, &pk, &lent, 5 * BEAT);
    drop(lent);
    assert_eq!(sp.sounding.load(Ordering::Relaxed), 0, "the export ended every note");
    // What each probe hears from the resume on, after the export's own notes.
    let (resume, unknown, from) = (rig.e.clock() as i64, n(&sp.unknown_offs), (sp.notes().len(), sq.notes().len()));
    let since = |s: &Stats, from: usize| s.notes()[from..].to_vec();
    rig.send(Msg::Hold(false));
    rig.run(BLOCK);
    assert_eq!(since(&sp, from.0), [(resume, Note::On, id(60))], "the long note again, at the resume");
    assert_eq!(since(&sq, from.1), [(resume, Note::Off, id(67))], "the other probe only lets go of the removed note");
    assert_eq!(rig.e.current().unwrap().plugins()[0].notes, [(id(60) as u32, 60)]);
    assert_eq!(sp.sounding.load(Ordering::Relaxed), 1);
    rig.run(3 * BEAT as usize);
    let end = (4 * BEAT) as i64;
    assert_eq!(since(&sp, from.0)[1..], [(end, Note::Off, id(60))], "its Off ends it, where it was written");
    assert_eq!((sp.sounding.load(Ordering::Relaxed), n(&sp.unknown_offs) - unknown), (0, 0), "nothing left on, no Off for a note it lacks");
    assert_eq!(rig.e.current().unwrap().held_notes(), 0);
}

// RENDER-25, RENDER-26a: a plan that keeps nothing (an instance made fresh) doesn't chase the
// chord: the new probe hears nothing until the next note, and the old one plays out the window as
// a ghost, its notes let go. The reference the with-plugins gate's level check must fail on.
#[test]
fn a_fresh_instance_chases_nothing() {
    let mut pk = probe_packages();
    let owner = Owner::discard();
    let data = chord(&[57, 60, 64]);
    let flat = probe_song(&mut pk, 1.0, None, json!([]), "p");
    let r1 = build(&flat, &data, &pk, &owner);
    let old = probe::stats(r1.plugin_handles()[0]).unwrap();
    let mut rig = Rig::of(&flat, &data, r1);
    rig.cmd(Cmd::Play);
    rig.run(BEAT as usize);
    let next = probe_song(&mut pk, 0.5, None, json!([]), "p");
    let r2 = rig.prepare_with(&next, &data, &pk, &owner, &Continues::nothing(&next)).unwrap();
    let h2 = r2.plugin_handles()[0];
    let at = rig.e.clock();
    rig.send(Msg::Activate { renderer: Box::new(r2) });
    let out = rig.run(BEAT as usize / 2);
    assert!(peak(&out[out.len() / 2..]) == 0.0, "the new instance plays nothing of the chord");
    assert_eq!(probe::stats(h2).unwrap().notes(), [], "nothing chased");
    let offs: Vec<i64> = notes_from(&old, at as i64).iter().filter(|e| e.1 == Note::Off).map(|e| e.0).collect();
    assert_eq!(offs, [at as i64; 3], "the ghost let go of the chord at the activation");
}

// RENDER-26a, RENDER-28: when the lane shrank, the scheduler that moved keeps its room and lets go
// of what the new data dropped, with no event dropped (debug builds assert it) and none sent for a
// note the probe doesn't hold.
#[test]
fn a_lane_that_shrank_lets_go_of_what_doesnt_fit() {
    let mut pk = probe_packages();
    let owner = Owner::discard();
    let lane = |n: usize| {
        let ns: Vec<Value> = (0..n).map(|k| json!({"pitch": 20 + k % 100, "start": (k / 100) as f64 * 0.001, "length": 4})).collect();
        with_notes(json!({"clips": [{"id": "c", "start": 0, "length": 8, "notes": ns}]}), at_bpm(120.0))
    };
    let flat = probe_song(&mut pk, 1.0, None, json!([]), "p");
    let r1 = build(&flat, &lane(300), &pk, &owner);
    let h = r1.plugin_handles()[0];
    let mut rig = Rig::of(&flat, &lane(300), r1);
    rig.cmd(Cmd::Play);
    rig.run(BEAT as usize);
    let stats = probe::stats(h).unwrap();
    assert_eq!(stats.sounding.load(Ordering::Relaxed), 300);
    let edited = probe_song(&mut pk, 0.5, None, json!([]), "p");
    let r2 = rig.prepare_with(&edited, &lane(250), &pk, &owner, &Continues::named(&flat, &edited)).unwrap();
    rig.send(Msg::Activate { renderer: Box::new(r2) });
    rig.run(2 * BLOCK);
    let held = rig.e.current().unwrap().plugins()[0].notes.len();
    assert!(held <= 250 && held > 0, "{held}");
    assert_eq!(stats.sounding.load(Ordering::Relaxed) as usize, held, "the probe holds what the node holds");
    assert_eq!((n(&stats.unknown_offs), n(&stats.full)), (0, 0));
    rig.run(4 * BEAT as usize);
    assert_eq!((stats.sounding.load(Ordering::Relaxed), rig.e.current().unwrap().held_notes()), (0, 0), "nothing stuck");
}

/// The probe's main-thread calls named `what` so far.
fn calls(stats: &Stats, what: &str) -> usize {
    stats.main_calls().iter().filter(|c| c.what == what).count()
}

/// One of the probe's parameters, as the plugin reads it now.
fn param_of(h: Handle, id: u32) -> f64 {
    match m2_plugins::host().main_call(h, m2_plugins::MainCall::Value(id)) {
        Ok(m2_plugins::Reply::Value(Some(v))) => v,
        other => panic!("the probe's parameter {id}: {other:?}"),
    }
}

/// A project whose keyed probe `lead` has `blob` saved (its bytes), the IR its data is read
/// against, and the folder.
fn with_blob(blob: Option<Vec<u8>>) -> (DataSet, PathBuf) {
    let dir = project(&[(
        "data/song/notes.json",
        json!({"clips": [{"start": 0, "length": 8, "notes": [{"pitch": 60, "start": 0, "length": 4}]}]}),
    )]);
    if let Some(b) = blob {
        std::fs::write(dir.join("data/song/lead.state"), b).unwrap();
        let side = json!({"plugin": "clap:m2.probe", "plugin_version": "1.0.0"});
        std::fs::write(dir.join("data/song/lead.state.json"), side.to_string()).unwrap();
    }
    // What the data is read against: the lane, and the state the keyed probe files (ID-8).
    let ir = song(
        json!([{"id": "n", "op": "lane", "lane": "notes"}, {"id": "p", "op": "plugin", "plugin": "clap:m2.probe", "key": "lead", "inputs": {"notes": "n"}}]),
        "p",
        json!([{"key": "notes", "kind": "events<note>"}]),
    );
    (data(&ir, &dir), dir)
}

// RENDER-26b, HOME-6, HOST-5: a changed state blob loads into the instance that plays, on the plugin
// main thread, while the plan is prepared and the old one plays: none is made. The same blob again
// loads nothing. A blob taken away makes the instance new (CLAP has no "load defaults"): the old one
// plays out the window as a ghost, and is destroyed on the plugin main thread only once the plan
// that held it is retired and dropped.
#[test]
fn a_changed_state_loads_into_the_same_instance() {
    let mut pk = probe_packages();
    let owner = Owner::discard();
    let flat = probe_song(&mut pk, 1.0, Some("lead"), json!([]), "p");
    let (data1, d1) = with_blob(Some(probe::state("one", 1.0)));
    let (data2, d2) = with_blob(Some(probe::state("two", 1.0)));
    let (data0, d0) = with_blob(None);
    assert!(data1.state_hash("song", &["lead"]).is_some() && data0.state_hash("song", &["lead"]).is_none());
    let r1 = build(&flat, &data1, &pk, &owner);
    let h = r1.plugin_handles()[0];
    let stats = probe::stats(h).unwrap();
    assert_eq!(*stats.tag.lock().unwrap(), "one");
    let mut rig = Rig::of(&flat, &data1, r1);
    rig.cmd(Cmd::Play);
    rig.run(BLOCK);

    // The same blob: kept, nothing loaded.
    let same = rig.prepare_with(&flat, &data1, &pk, &owner, &Continues::identity(&flat)).unwrap();
    assert_eq!((same.stats().kept.plugins, same.stats().loads), (1, 0));
    rig.send(Msg::Activate { renderer: Box::new(same) });
    rig.run(BLOCK);

    // Another blob: loaded into it as the plan is prepared, while the old plan plays.
    let loads = calls(&stats, "load");
    let r2 = rig.prepare_with(&flat, &data2, &pk, &owner, &Continues::identity(&flat)).unwrap();
    let s = r2.stats().clone();
    assert_eq!((s.kept.plugins, s.fresh.plugins, s.loads), (1, 0, 1), "{s:?}");
    assert_eq!((r2.info().plugins[0].handle, owner.live()), (h, 1), "the same instance, none made");
    assert_eq!((calls(&stats, "load"), stats.tag.lock().unwrap().clone()), (loads + 1, "two".to_string()));
    let load = stats.main_calls().into_iter().rfind(|c| c.what == "load").unwrap();
    assert!(load.says_main && !load.says_audio, "loaded on the plugin main thread: {load:?}");
    rig.send(Msg::Activate { renderer: Box::new(r2) });
    rig.run(BLOCK);
    assert_eq!(rig.e.current().unwrap().plugin_handles(), [h]);
    // Its blob again: nothing to load.
    let again = rig.prepare_with(&flat, &data2, &pk, &owner, &Continues::identity(&flat)).unwrap();
    assert_eq!((again.stats().kept.plugins, again.stats().loads, calls(&stats, "load")), (1, 0, loads + 1));
    rig.send(Msg::Activate { renderer: Box::new(again) });
    rig.run(BLOCK);

    // The blob taken away: a new instance, on its defaults.
    let r0 = rig.prepare_with(&flat, &data0, &pk, &owner, &Continues::identity(&flat)).unwrap();
    assert_eq!((r0.stats().kept.plugins, r0.stats().fresh.plugins), (0, 1));
    let h0 = r0.plugin_handles()[0];
    assert_ne!(h0, h);
    rig.send(Msg::Activate { renderer: Box::new(r0) });
    rig.run(10 * BLOCK);
    assert_eq!(rig.retired_renderers().len(), 4, "every plan before was retired");
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(owner.live(), 2, "retired, but not dropped: its instance lives");
    rig.retired.clear();
    assert!(eventually(|| owner.live() == 1), "dropped: its instance is destroyed");
    assert!(probe::stats(h).is_none());
    let calls = stats.main_calls();
    assert!(calls.iter().any(|c| c.what == "destroy" && c.says_main && !c.says_audio));
    for d in [d1, d2, d0] {
        std::fs::remove_dir_all(d).ok();
    }
}

// RENDER-26b (build notes, slice 4), HOME-6: a state load that fails never fails the prepare, as the
// loads before it have landed in instances that play. Two keyed plugins get new blobs; the first
// loads, the second refuses its. The plan is prepared whole and both instances are kept; the refusal
// is named once: the plan says the refused blob was given, so the next plan neither tries it again
// nor says so again. The blob it held before, written back (an undo), loads: the plugin may hold
// part of the refused one.
#[test]
fn a_state_that_fails_to_load_never_fails_the_prepare() {
    let mut pk = probe_packages();
    let owner = Owner::discard();
    let nodes = json!([
        {"id": "n", "op": "lane", "lane": "notes"},
        {"id": "p", "op": "plugin", "plugin": "clap:m2.probe", "key": "lead", "inputs": {"notes": "n"}},
        {"id": "q", "op": "plugin", "plugin": "clap:m2.probe.fails-state", "key": "bad", "inputs": {"in": "p"}},
    ]);
    let ir = song(nodes, "q", json!([{"key": "notes", "kind": "events<note>"}]));
    let flat = plugged(&mut pk, ir.clone());
    let files = |lead: &str, bad: &str| {
        let dir = project(&[]);
        std::fs::create_dir_all(dir.join("data/song")).unwrap();
        for (key, plugin, bytes) in [("lead", "clap:m2.probe", lead), ("bad", "clap:m2.probe.fails-state", bad)] {
            std::fs::write(dir.join(format!("data/song/{key}.state")), bytes).unwrap();
            let side = json!({"plugin": plugin, "plugin_version": "1.0.0"});
            std::fs::write(dir.join(format!("data/song/{key}.state.json")), side.to_string()).unwrap();
        }
        let d = data(&ir, &dir);
        (d, dir)
    };
    let (data1, d1) = files("one", "fine");
    let (data2, d2) = files("two", "fail: this blob doesn't load");
    let r1 = build(&flat, &data1, &pk, &owner);
    let hs = r1.plugin_handles();
    let mut rig = Rig::of(&flat, &data1, r1);
    rig.cmd(Cmd::Play);
    rig.run(BLOCK);
    let r2 = rig.prepare_with(&flat, &data2, &pk, &owner, &Continues::identity(&flat)).expect("a failed load fails no prepare");
    let s = r2.stats().clone();
    assert_eq!((s.kept.plugins, s.fresh.plugins, s.loads), (2, 0, 1), "{s:?}");
    assert_eq!(s.unloaded.len(), 1, "{s:?}");
    assert!(flat.nodes[s.unloaded[0].0].name.ends_with("/q") && s.unloaded[0].1.contains("fails-state"), "{s:?}");
    assert_eq!(*probe::stats(hs[0]).unwrap().tag.lock().unwrap(), "two", "the first landed");
    let info = r2.info();
    let held = |node: &str| info.plugins.iter().find(|p| flat.nodes[p.node].name.ends_with(node)).unwrap().blob;
    assert_eq!(held("/p"), data2.state_hash("song", &["lead"]));
    assert_eq!(held("/q"), data2.state_hash("song", &["bad"]), "the refused blob counts as given");
    rig.send(Msg::Activate { renderer: Box::new(r2) });
    rig.run(BLOCK);
    assert_eq!(rig.e.current().unwrap().plugin_handles(), hs, "both kept");
    // The next plan neither tries it again nor says so again.
    let q = probe::stats(hs[1]).unwrap();
    let loads = || q.main_calls().iter().filter(|c| c.what == "load").count();
    let tried = loads();
    let again = rig.prepare_with(&flat, &data2, &pk, &owner, &Continues::identity(&flat)).unwrap();
    assert_eq!((again.stats().loads, again.stats().unloaded.len(), loads()), (0, 0, tried));
    // Its old blob written back loads, though it was what the plugin held before the refusal.
    let back = rig.prepare_with(&flat, &data1, &pk, &owner, &Continues::identity(&flat)).unwrap();
    assert_eq!((back.stats().loads, back.stats().unloaded.len(), loads()), (2, 0, tried + 1), "{:?}", back.stats());
    for d in [d1, d2] {
        std::fs::remove_dir_all(d).ok();
    }
}

// HOME-4, RENDER-26b: code beats the blob. A state load sets every parameter the blob holds, and
// the instance's next calls (here the old plan's, before the activation) send every parameter code
// owns again, whatever was last sent: the blob's value for one of those plays for less than a
// block, or two for a plugin that applies a loaded state at the end of its next call, over what
// that call sent (the probe's `defer_load`, as Surge XT does). A parameter code doesn't own keeps
// the blob's value.
#[test]
fn a_state_load_resends_code_owned_parameters() {
    let mut pk = probe_packages();
    let owner = Owner::discard();
    // Code owns the gain (1); the level is the blob's.
    let flat = probe_song(&mut pk, 1.0, Some("lead"), json!([]), "p");
    let (data1, d1) = with_blob(Some(probe::state_with("one", 0.25, 0.2)));
    let (data2, d2) = with_blob(Some(probe::state_with("two", 0.75, 0.6)));
    for deferred in [false, true] {
        let r1 = build(&flat, &data1, &pk, &owner);
        let h = r1.plugin_handles()[0];
        probe::stats(h).unwrap().defer_load.store(deferred, Ordering::Relaxed);
        let read = || (param_of(h, probe::GAIN), param_of(h, probe::LEVEL));
        let mut rig = Rig::of(&flat, &data1, r1);
        rig.cmd(Cmd::Play);
        rig.run(BLOCK);
        assert_eq!(read(), (1.0, 0.2), "code's gain, the blob's level");
        let r2 = rig.prepare_with(&flat, &data2, &pk, &owner, &Continues::identity(&flat)).unwrap();
        assert_eq!(r2.stats().loads, 1);
        rig.run(BLOCK);
        if deferred {
            assert_eq!(read(), (0.75, 0.6), "applied at the end of the next call, over what it sent");
            rig.run(BLOCK);
        } else {
            assert_eq!(read(), (1.0, 0.6), "the next call: code's gain, the blob's level");
        }
        assert_eq!(read(), (1.0, 0.6), "code's gain again, the blob's level (deferred: {deferred})");
        rig.send(Msg::Activate { renderer: Box::new(r2) });
        rig.run(4 * BLOCK);
        assert_eq!(read(), (1.0, 0.6));
        // Nothing loaded: nothing sent again, and the level the blob set stays.
        let same = rig.prepare_with(&flat, &data2, &pk, &owner, &Continues::identity(&flat)).unwrap();
        assert_eq!(same.stats().loads, 0);
    }
    for d in [d1, d2] {
        std::fs::remove_dir_all(d).ok();
    }
}

// RENDER-6a, §5.6: with every plugin asleep, a stopped engine idles; a plugin that asks to be
// processed from a thread of its own wakes it within one block.
#[test]
fn sleeping_plugins_let_the_engine_idle_and_wake_it() {
    let mut pk = probe_packages();
    let owner = Owner::discard();
    let flat = probe_song(&mut pk, 1.0, None, json!([]), "p");
    let r = build(&flat, &chord(&[60]), &pk, &owner);
    let h = r.plugin_handles()[0];
    let stats = probe::stats(h).unwrap();
    let mut rig = Rig::new(r);
    rig.cmd(Cmd::Play);
    rig.run(BEAT as usize);
    rig.cmd(Cmd::Stop);
    rig.run(20 * BLOCK);
    assert!(!rig.e.is_idle(), "a plugin that doesn't sleep keeps the engine running");
    stats.sleep.store(true, Ordering::Relaxed);
    rig.run(20 * BLOCK);
    assert!(rig.e.is_idle(), "every plugin asleep and the output silent: idle");
    let calls = n(&stats.process);
    rig.run(4 * BLOCK);
    assert_eq!(n(&stats.process), calls, "idle: not processed");
    stats.sleep.store(false, Ordering::Relaxed);
    std::thread::spawn(move || assert!(probe::request_process(h))).join().unwrap();
    rig.run(BLOCK);
    assert!(!rig.e.is_idle() && n(&stats.process) == calls + 1, "woken at the next block");
}

// ---------------------------------------------------------------- RENDER-26a: consumers

// RENDER-26a: a replaced instrument plays out the window as a ghost with its notes let go at the
// activation (its voices go into their release); the new one chases nothing, and nothing is held.
#[test]
fn a_replaced_instrument_releases_its_notes() {
    let pk = std_packages();
    let long = with_notes(json!({"clips": [{"start": 0, "length": 16, "notes": [{"pitch": 69, "start": 0, "length": 8}]}]}), at_bpm(120.0));
    let (a, b) = (flat(&synth(0.2, json!([]), "v")), flat(&synth(0.3, json!([]), "v")));
    let mut rig = playing(&a, &long, &pk, BEAT as usize);
    let r = rig.prepare(&b, &long, &pk);
    let at = rig.e.clock();
    rig.send(Msg::Activate { renderer: Box::new(r) });
    let out = rig.run(BEAT as usize / 2);
    let cur = rig.e.current().unwrap();
    assert_eq!((cur.stats().fresh.polys, cur.stats().ghosts), (1, 1));
    assert_eq!((cur.held_notes(), cur.active_voices()), (0, 0), "the new instrument chases nothing");
    assert!(cur.gates_are_owned());
    // 24,000 frames in, the activation's block line is 64 frames on; its window ends 240 later.
    assert!(peak(&out[64 + 240..]) == 0.0, "silent once the window ends");
    let ghost = rig.retired.iter().find_map(|r| if let Retired::Nodes(l) = r { l.exec.first() } else { None });
    let Some(Exec::Poly(p)) = ghost else { panic!("the ghost poly left with the window") };
    let offs: Vec<u64> = p.pool.iter().flat_map(|pool| &pool.voices).filter_map(|v| v.note).map(|n| n.off).collect();
    assert_eq!(offs, [at], "its note was let go at the activation");
}

// RENDER-26a (§3.5): a kept poly whose notes now come from another lane lets go of what it holds:
// its voices release and ring out, nothing sticks, and the other lane's notes play on it.
#[test]
fn a_rewired_poly_is_kept_and_releases_its_notes() {
    let pk = std_packages();
    let lanes = |lane: &str| {
        let mut ir = synth(0.1, json!([]), "v");
        ir.modules[0].nodes[0].op = serde_json::from_value(json!({"op": "lane", "lane": lane})).unwrap();
        ir.modules[0].lanes =
            serde_json::from_value(json!([{"key": "a", "kind": "events<note>"}, {"key": "b", "kind": "events<note>"}])).unwrap();
        flat(&ir)
    };
    let notes = |n: Value| super::notes(json!({"clips": [{"start": 0, "length": 16, "notes": [n]}]}));
    let data = DataSet::with_lanes(
        at_bpm(120.0),
        [
            (addr("a"), notes(json!({"pitch": 69, "start": 0, "length": 8}))),
            (addr("b"), notes(json!({"pitch": 72, "start": 2, "length": 1}))),
        ],
    );
    let (a, b) = (lanes("a"), lanes("b"));
    let mut rig = playing(&a, &data, &pk, BEAT as usize);
    let r = rig.prepare(&b, &data, &pk);
    rig.send(Msg::Activate { renderer: Box::new(r) });
    let tail = rig.run(BEAT as usize / 2);
    let cur = rig.e.current().unwrap();
    assert_eq!((cur.stats().kept.polys, cur.stats().ghosts), (1, 0));
    assert_eq!(cur.held_notes(), 0, "let go at the activation");
    assert!(peak(&tail[..200]) > 0.5 && peak(&tail[tail.len() - 1000..]) < 1e-3, "it rings out its release");
    let next = rig.run(BEAT as usize);
    assert!(peak(&next[BEAT as usize / 2..]) > 0.5, "the other lane's note plays on it");
    assert!(rig.e.current().unwrap().gates_are_owned());
}

// RENDER-26a (§3.5), HOST-5: a kept plugin whose notes now come from another lane gets a NoteOff
// for each note it holds at the activation, and none for a note it doesn't hold.
#[test]
fn a_rewired_plugin_lets_go_of_its_keys() {
    let mut pk = probe_packages();
    let owner = Owner::discard();
    let data = chord(&[57, 60, 64]);
    let a = probe_song(&mut pk, 1.0, None, json!([]), "p");
    let other = song(
        json!([{"id": "n", "op": "lane", "lane": "other"}, {"id": "g", "op": "const", "value": 1.0},
            {"id": "p", "op": "plugin", "plugin": "clap:m2.probe", "inputs": {"notes": "n", "gain_1": "g"}}]),
        "p",
        json!([{"key": "notes", "kind": "events<note>"}, {"key": "other", "kind": "events<note>"}]),
    );
    let b = plugged(&mut pk, other);
    let r = build(&a, &data, &pk, &owner);
    let h = r.plugin_handles()[0];
    let stats = probe::stats(h).unwrap();
    let mut rig = Rig::of(&a, &data, r);
    rig.cmd(Cmd::Play);
    rig.run(BEAT as usize);
    let r = rig.prepare_with(&b, &data, &pk, &owner, &Continues::named(&a, &b)).unwrap();
    let at = rig.e.clock();
    rig.send(Msg::Activate { renderer: Box::new(r) });
    rig.run(BLOCK);
    assert_eq!(rig.e.current().unwrap().plugin_handles(), [h], "kept");
    let offs: Vec<(i64, i32)> = notes_from(&stats, at as i64).iter().filter(|e| e.1 == Note::Off).map(|e| (e.0, e.2)).collect();
    assert_eq!(offs, [(at as i64, 1), (at as i64, 2), (at as i64, 3)]);
    assert_eq!((stats.sounding.load(Ordering::Relaxed), n(&stats.unknown_offs)), (0, 0));
}

// RENDER-26a: notes that outgrow their lane's scheduler grow it at an activation of the same code:
// a bigger scheduler takes over the sounding notes with their ids, the instance is kept, nothing
// ends at the activation, and the new notes all play.
#[test]
fn a_lane_that_outgrows_its_room_grows_without_a_restart() {
    let mut pk = probe_packages();
    let owner = Owner::discard();
    let lane = |more: usize| {
        let mut ns: Vec<Value> = (0..10).map(|k| json!({"pitch": 40 + k, "start": 0, "length": 6})).collect();
        ns.extend((0..more).map(|k| json!({"pitch": k % 120, "start": 3, "length": 2})));
        with_notes(json!({"clips": [{"id": "c", "start": 0, "length": 8, "notes": ns}]}), at_bpm(120.0))
    };
    let f = probe_song(&mut pk, 1.0, None, json!([]), "p");
    let r = build(&f, &lane(0), &pk, &owner);
    let h = r.plugin_handles()[0];
    let stats = probe::stats(h).unwrap();
    let mut rig = Rig::of(&f, &lane(0), r);
    rig.cmd(Cmd::Play);
    rig.run(BEAT as usize);
    let r = rig.prepare_with(&f, &lane(300), &pk, &owner, &Continues::identity(&f)).unwrap();
    assert!(r.info().lanes[0].caps.unwrap().sounding >= 310, "a scheduler with room for them");
    let at = rig.e.clock();
    rig.send(Msg::Activate { renderer: Box::new(r) });
    rig.run(BEAT as usize * 5 / 2);
    assert_eq!((rig.e.current().unwrap().plugin_handles(), owner.live()), (vec![h], 1), "the same instance");
    assert!(notes_from(&stats, at as i64).iter().filter(|e| e.0 < at as i64 + BEAT as i64).all(|e| e.1 == Note::On), "nothing ends at it");
    assert_eq!(stats.sounding.load(Ordering::Relaxed), 310, "the ten held and the 300 new");
    assert_eq!(n(&stats.unknown_offs) + n(&stats.repeated_ids) + n(&stats.full), 0);
    assert_eq!(rig.e.current().unwrap().dropped_ons(), 0);
}

/// Sine voices releasing over 50 ms on `notes`, beside an audio lane of `clips` (of the assets
/// `dc.wav`, 20 s of 0.25, and `s.wav`, a 220 Hz sine), mixed, at `bpm`: the graph, its data and
/// the project's folder.
fn voiced(bpm: f64, notes: Value, clips: Value) -> (Flat, DataSet, PathBuf) {
    let mut ir = synth(
        0.05,
        json!([{"id": "a", "op": "lane", "lane": "clips"}, {"id": "s", "op": "kernel", "kernel": "std:add", "inputs": {"a": "v", "b": "a"}}]),
        "s",
    );
    ir.modules[0].lanes =
        serde_json::from_value(json!([{"key": "notes", "kind": "events<note>"}, {"key": "clips", "kind": "audio<1>"}])).unwrap();
    let dir = project(&[
        ("data/arrangement.json", json!({"tempo": [{"beat": 0, "bpm": bpm}]})),
        ("data/song/notes.json", json!({"clips": [{"start": 0, "length": 32, "notes": notes}]})),
        ("data/song/clips.json", json!({"clips": clips})),
    ]);
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    audio::write_wav(&dir.join("assets/dc.wav"), &[vec![0.25; 20 * SR as usize]], SR).unwrap();
    audio::write_wav(&dir.join("assets/s.wav"), &[sine(220.0, 20.0, SR)], SR).unwrap();
    let data = data(&ir, &dir);
    (flat(&ir), data, dir)
}

// RENDER-26a, RENDER-9 (A1): a stop, a seek or a block-line wrap on an activation's block line is
// that jump: the moved scheduler ends its notes there and the moved audio player declicks, as on
// any block, where `Enter` would have held every note.
#[test]
fn a_jump_on_the_activation_line_ends_its_notes() {
    let pk = std_packages();
    let (f, data, dir) =
        voiced(120.0, json!([{"pitch": 69, "start": 0, "length": 20}]), json!([{"start": 0, "length": 20, "asset": "s.wav"}]));
    let own = max_step(&Renderer::new(&f, &data, &pk, SR).unwrap().render(0.5).unwrap());
    for jump in [Cmd::Stop, Cmd::Seek(4.0), Cmd::Loop(Some((0.0, 16.0)))] {
        let wrap = matches!(jump, Cmd::Loop(_));
        let mut r = build(&f, &data, &pk, &Owner::discard());
        r.trace(1 << 12);
        let mut rig = Rig::of(&f, &data, r);
        rig.cmd(Cmd::Play);
        if wrap {
            // 16 beats at 120 bpm is 3,000 blocks: the wrap falls on a block line.
            rig.cmd(jump);
        }
        let mut out = rig.run(if wrap { 3000 * BLOCK } else { BEAT as usize });
        let mut next = rig.prepare(&f, &data, &pk);
        next.trace(1 << 12);
        let at = rig.e.clock();
        rig.send(Msg::Activate { renderer: Box::new(next) });
        if !wrap {
            rig.cmd(jump);
        }
        out.extend(rig.run(4 * BLOCK));
        let cur = rig.e.current().unwrap();
        let entries = cur.trace_entries();
        let first = entries.iter().find(|e| e.engine == at && matches!(e.kind, TraceKind::Jump(_))).map(|e| e.kind);
        let off = entries.iter().find(|e| e.kind == TraceKind::Off).map(|e| e.engine);
        assert_eq!(off, Some(at), "{jump:?}: the note ends at the activation's line ({first:?})");
        assert!(!matches!(first, Some(TraceKind::Jump(Jump::Enter))), "{jump:?}: the jump wins over Enter");
        if matches!(jump, Cmd::Stop) {
            assert_eq!((cur.held_notes(), cur.sounding_notes()), (0, 0), "stopped: nothing held");
        }
        let around = &out[out.len() - 6 * BLOCK..];
        assert!(max_step(around) <= 2.0 * own, "{jump:?}: the moved audio player declicks: {} against {own}", max_step(around));
    }
    std::fs::remove_dir_all(dir).ok();
}

// ---------------------------------------------------------------- RENDER-27: arrangement switches

/// The engine frame transport frame `t` plays at, playing on from the activation at engine frame
/// `at`, transport `t0`, with no jump.
fn engine_at(at: u64, t0: u64, t: u64) -> u64 {
    at + t - t0
}

// RENDER-27: a tempo change is an activation of the same structure: every kernel, voice and ring is
// kept, nothing fades, the beat goes on, and a clip after the change (unwarped) starts at its beat
// in the new map.
#[test]
fn a_tempo_change_keeps_every_kernel_and_the_beat() {
    let pk = std_packages();
    let notes = json!([{"pitch": 69, "start": 0, "length": 1}]);
    let clips = json!([{"start": 8, "length": 2, "asset": "dc.wav"}]);
    let (f, d120, dir1) = voiced(120.0, notes.clone(), clips.clone());
    let (_, d90, dir2) = voiced(90.0, notes, clips);
    let mut rig = playing(&f, &d120, &pk, BEAT as usize * 2 + 77);
    let r = rig.prepare(&f, &d90, &pk);
    let s = r.stats().clone();
    assert_eq!((s.fresh.total(), s.ghosts, s.fades), (0, 0, 0), "{s:?}");
    assert_eq!(s.kept.polys + s.kept.kernels, 2);
    let old = rig.e.current().unwrap();
    let beat = old.transport().beat(old.tempo(), SR as f64);
    let at = rig.e.clock();
    rig.send(Msg::Activate { renderer: Box::new(r) });
    rig.run(BLOCK);
    let cur = rig.e.current().unwrap();
    let t0 = cur.transport().pos() - BLOCK as u64;
    assert!((d90.tempo().beat_at(t0 as f64 / SR as f64) - beat).abs() < 1e-4, "the beat is kept");
    // Where the clip starts: beat 8 at 90 bpm.
    let onset = engine_at(at, t0, crate::transport::frame_of(d90.tempo(), 8.0, SR as f64));
    let a = node(&f, "a");
    let mut first = None;
    while first.is_none() && rig.e.clock() < onset + 4 * BLOCK as u64 {
        let clock = rig.e.clock();
        rig.run(BLOCK);
        let tap = rig.e.current().unwrap().output_of(a);
        first = tap[..BLOCK].iter().position(|x| *x != 0.0).map(|k| clock + k as u64);
    }
    assert_eq!(first, Some(onset), "the clip starts at beat 8 in the new map");
    for d in [dir1, dir2] {
        std::fs::remove_dir_all(d).ok();
    }
}

// RENDER-27, RENDER-26a: a note sounding across a tempo change ends where its beat falls in the new
// map: its scheduler reconciles with the data rebuilt in it.
#[test]
fn a_tempo_change_recomputes_pending_offs() {
    let pk = std_packages();
    let notes = json!([{"pitch": 69, "start": 0, "length": 6}]);
    let (f, d120, dir1) = voiced(120.0, notes.clone(), json!([]));
    let (_, d90, dir2) = voiced(90.0, notes, json!([]));
    let mut rig = playing(&f, &d120, &pk, BEAT as usize * 2);
    let mut r = rig.prepare(&f, &d90, &pk);
    r.trace(64);
    let at = rig.e.clock();
    rig.send(Msg::Activate { renderer: Box::new(r) });
    rig.run(BLOCK);
    let t0 = rig.e.current().unwrap().transport().pos() - BLOCK as u64;
    rig.run(4 * SR as usize);
    let off = rig.e.current().unwrap().trace_entries().into_iter().find(|e| e.kind == TraceKind::Off).map(|e| e.engine);
    assert_eq!(off, Some(engine_at(at, t0, crate::transport::frame_of(d90.tempo(), 6.0, SR as f64))));
    for d in [dir1, dir2] {
        std::fs::remove_dir_all(d).ok();
    }
}

/// The lane `clips` of `voiced`, with a clip at beat 0 and one at beat 6 at `gain` dB.
fn two_clips(gain: f64) -> Value {
    json!([{"start": 0, "length": 8, "asset": "s.wav"}, {"start": 6, "length": 2, "asset": "s.wav", "gain_db": gain}])
}

// RENDER-27 (A4): an audio swap where one clip changed and another plays crossfades sample by
// sample, and the playing clip, the same on both sides, changes no bit through the window.
#[test]
fn an_audio_swap_of_unchanged_clips_changes_no_bit() {
    let pk = std_packages();
    let (f, d0, dir1) = voiced(120.0, json!([]), two_clips(0.0));
    let (_, d6, dir2) = voiced(120.0, json!([]), two_clips(-6.0));
    let frames = 4 * BEAT as usize;
    let (plain, _) = with_edit(&f, &d0, &pk, frames, None);
    let (edited, rig) = with_edit(&f, &d0, &pk, frames, Some((2 * BEAT as usize, &f, &d6)));
    assert_eq!(rig.e.current().unwrap().stats().clone().fresh.total(), 0);
    assert!(peak(&plain[2 * BEAT as usize..]) > 0.1);
    assert!(edited == plain, "the playing clip is the same bits on both sides");
    for d in [dir1, dir2] {
        std::fs::remove_dir_all(d).ok();
    }
}

// RENDER-27 (A4, P3): a +1 dB edit to the clip that plays moves linearly between its two levels
// over the window: every sample between the two sides, no swell.
#[test]
fn a_clip_gain_edit_stays_between_its_two_levels() {
    let pk = std_packages();
    let clip = |db: f64| json!([{"start": 0, "length": 8, "asset": "s.wav", "gain_db": db}]);
    let (f, d0, dir1) = voiced(120.0, json!([]), clip(0.0));
    let (_, d1, dir2) = voiced(120.0, json!([]), clip(1.0));
    let at = 2 * BEAT as usize;
    let frames = at + 2 * BLOCK;
    let (lo, _) = with_edit(&f, &d0, &pk, frames, None);
    let (hi, _) = with_edit(&f, &d1, &pk, frames, None);
    let (edited, _) = with_edit(&f, &d0, &pk, frames, Some((at, &f, &d1)));
    let rms = |x: &[[f32; 2]]| (x.iter().map(|f| f[0] * f[0]).sum::<f32>() / x.len() as f32).sqrt();
    for k in at..at + 240 {
        let (a, b, x) = (lo[k][0], hi[k][0], edited[k][0]);
        assert!(a.min(b) <= x && x <= a.max(b), "frame {k}: {x} outside {a}..{b}");
    }
    let w = at..at + 240;
    let (r, a, b) = (rms(&edited[w.clone()]), rms(&lo[w.clone()]), rms(&hi[w]));
    assert!(a.min(b) <= r && r <= a.max(b), "the window's level {r} between {a} and {b}");
    assert!(edited[at + 240..] == hi[at + 240..], "the new level exactly after the window");
    for d in [dir1, dir2] {
        std::fs::remove_dir_all(d).ok();
    }
}

// ---------------------------------------------------------------- RENDER-22a, RENDER-23: cells

/// A sine times a smooth level (site 3) at `lvl`, plus, with `more`, a delay on another sine.
fn leveled(lvl: f64, more: bool) -> Flat {
    let mut nodes = vec![
        json!({"id": "f", "op": "const", "value": 440, "unit": "hz"}),
        json!({"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "f"}}),
        smooth_const("lvl", lvl, 3),
        json!({"id": "g", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "o", "by": "lvl"}}),
    ];
    let out = if more {
        nodes.push(json!({"id": "d", "op": "kernel", "kernel": "std:delay", "inputs": {"in": "o"}}));
        nodes.push(json!({"id": "s", "op": "kernel", "kernel": "std:add", "inputs": {"a": "g", "b": "d"}}));
        "s"
    } else {
        "g"
    };
    flat(&framed(song(Value::Array(nodes), out, json!([]))))
}

// RENDER-22a, RENDER-23: a committed smooth value that rides with a structural edit ramps from the
// old value over the activation's first block, as a `SetCell` would, then holds.
#[test]
fn a_committed_smooth_value_ramps_through_an_activation() {
    let pk = std_packages();
    let (a, b) = (leveled(0.8, false), leveled(0.4, true));
    let mut rig = playing(&a, &DataSet::empty(), &pk, 4 * BLOCK);
    let r = rig.prepare(&b, &DataSet::empty(), &pk);
    rig.send(Msg::Activate { renderer: Box::new(r) });
    let (_, t) = taps(&mut rig, &[node(&b, "lvl")], 2);
    let ramp: Vec<f32> = (0..BLOCK).map(|i| 0.8 + (0.4 - 0.8) * ((i + 1) as f32 / BLOCK as f32)).collect();
    assert_eq!(&t[0][..BLOCK - 1], &ramp[..BLOCK - 1], "a ramp over the first block");
    assert!(t[0][BLOCK - 1..].iter().all(|&x| x == 0.4), "then the new value");
}

// RENDER-22a (A6), RENDER-23: a kept poly's voice cells take the new code's values, the ones that
// changed only (the other's constant keeps its version), from what its voices held: a ramp written
// the block before the activation ends there, and every block after the new value's own ramp is
// flat, never a ramp copied block after block.
#[test]
fn a_kept_polys_voice_cells_take_the_new_values() {
    let pk = std_packages();
    let voiced = |wet: f64| {
        let mut ir = synth(0.0, json!([]), "v");
        let mut body = serde_json::to_value(&ir.modules[0].nodes[1]).unwrap();
        let nodes = body["body"]["nodes"].as_array_mut().unwrap();
        nodes.push(smooth_const("wet", wet, 4));
        nodes.push(step_const("dry", 0.5, 6));
        nodes.push(json!({"id": "w", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "amp", "by": "wet"}}));
        nodes.push(json!({"id": "y", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "w", "by": "dry"}}));
        body["body"]["out"] = json!("y");
        ir.modules[0].nodes[1] = serde_json::from_value(body).unwrap();
        flat(&framed(ir))
    };
    let data = with_notes(json!({"clips": [{"start": 0, "length": 8, "notes": [{"pitch": 69, "start": 0, "length": 4}]}]}), at_bpm(120.0));
    let (a, b) = (voiced(0.25), voiced(0.6));
    let mut rig = playing(&a, &data, &pk, 8 * BLOCK);
    // A ramp in flight in the voices, written the block before the activation.
    let r = rig.e.current().unwrap();
    r.cell_slots().set(r.cell_map().cells(&origin(4)), 0.3, 1);
    rig.run(BLOCK);
    let next = rig.prepare(&b, &data, &pk);
    rig.send(Msg::Activate { renderer: Box::new(next) });
    let v = node(&b, "v");
    let FOp::Poly { voice: body, .. } = &b.nodes[v].op else { panic!("a poly") };
    let at = |id: &str| body.nodes.iter().position(|n| n.name == format!("song/{id}")).unwrap();
    let (wet, dry) = (at("wet"), at("dry"));
    // Voice 0's wet constant this block, and the version of its dry one.
    let voice = |rig: &Rig| {
        let Exec::Poly(p) = &rig.e.current().unwrap().plan.graph.exec[v] else { panic!("a poly") };
        let g = &p.pool.as_ref().unwrap().voices[0].graph;
        (g.bufs[wet][0].clone(), g.versions[dry])
    };
    rig.run(BLOCK);
    let (first, dry1) = voice(&rig);
    assert_eq!(*first.last().unwrap(), 0.6, "from what the voices held, to the new value over one block");
    assert!(first[0] > 0.3 && first[0] < 0.31, "the ramp starts from 0.3: {}", first[0]);
    for _ in 0..4 {
        rig.run(BLOCK);
        let (wet, dry) = voice(&rig);
        assert!(wet.iter().all(|&x| x == 0.6), "flat after it");
        assert_eq!(dry, dry1, "the unchanged cell was never written");
    }
}

// RENDER-23, WRITE-22: a lease's value set on a plan before it is sent (as the old plan heard it)
// plays from the new plan's first block, with no ramp from the code's value.
#[test]
fn a_lease_value_set_before_the_send_plays_from_the_first_block() {
    let pk = std_packages();
    let (a, b) = (leveled(0.8, false), leveled(0.8, true));
    let mut rig = playing(&a, &DataSet::empty(), &pk, 4 * BLOCK);
    let old = rig.e.current().unwrap();
    old.cell_slots().set(old.cell_map().cells(&origin(3)), 0.7, 1);
    rig.run(2 * BLOCK);
    let mut r = rig.prepare(&b, &DataSet::empty(), &pk);
    let cells = r.cell_map().cells(&origin(3)).to_vec();
    r.set_cells(&cells, 0.7);
    rig.send(Msg::Activate { renderer: Box::new(r) });
    let (_, t) = taps(&mut rig, &[node(&b, "lvl")], 1);
    assert!(t[0].iter().all(|&x| x == 0.7), "the lease's value from the first block");
}

// ---------------------------------------------------------------- HOST-5, RENDER-28, panic

// HOST-5 (A2): a plain number passed to a kept plugin, edited, is sent to the instance at once;
// unchanged, it isn't sent again.
#[test]
fn a_changed_constant_parameter_reaches_a_kept_plugin() {
    let mut pk = probe_packages();
    let owner = Owner::discard();
    let data = chord(&[60]);
    let (a, b) = (probe_song(&mut pk, 0.7, None, json!([]), "p"), probe_song(&mut pk, 0.5, None, json!([]), "p"));
    let r = build(&a, &data, &pk, &owner);
    let h = r.plugin_handles()[0];
    let mut rig = Rig::of(&a, &data, r);
    rig.cmd(Cmd::Play);
    rig.run(BLOCK);
    let gain = || match m2_plugins::host().main_call(h, m2_plugins::MainCall::Value(probe::GAIN)) {
        Ok(m2_plugins::Reply::Value(v)) => v,
        _ => None,
    };
    assert_eq!(gain(), Some(0.7));
    for (next, want) in [(&b, 0.5), (&b, 0.5)] {
        let r = rig.prepare_with(next, &data, &pk, &owner, &Continues::identity(next)).unwrap();
        rig.send(Msg::Activate { renderer: Box::new(r) });
        rig.run(BLOCK);
        assert_eq!((rig.e.current().unwrap().plugin_handles(), gain()), (vec![h], Some(want)));
    }
}

// RENDER-28: activations are never coalesced: each was prepared against the one before, so each
// applies, in order, once the window before it ends.
#[test]
fn activations_are_never_coalesced() {
    let pk = std_packages();
    let (a, b) = (
        flat(&sine_then(json!([]), "o")),
        flat(&sine_then(json!([{"id": "d", "op": "kernel", "kernel": "std:delay", "inputs": {"in": "o"}}]), "d")),
    );
    let mut rig = playing(&a, &DataSet::empty(), &pk, 4 * BLOCK);
    let seqs: Vec<u64> = [&b, &a, &b]
        .iter()
        .map(|f| {
            let r = rig.prepare(f, &DataSet::empty(), &pk);
            rig.send(Msg::Activate { renderer: Box::new(r) })
        })
        .collect();
    rig.run(20 * BLOCK);
    let (stamps, _) = rig.e.applied();
    let clocks: Vec<u64> = seqs.iter().map(|s| stamps.iter().find(|a| a.seq == *s && !a.refused).expect("applied").clock).collect();
    assert!(clocks.windows(2).all(|w| w[1] >= w[0] + 240), "each after the window before: {clocks:?}");
    assert_eq!((rig.e.activations(), rig.e.refused_activations()), (3, 0));
}

// RENDER-10, §7.3: a panic during a window resets the plan, its ghosts included, and the window
// still ends on time.
#[test]
fn panic_during_a_window_resets_ghosts_and_ends_on_time() {
    let pk = std_packages();
    let (a, b) = (
        flat(&sine_then(json!([{"id": "d", "op": "kernel", "kernel": "std:delay", "inputs": {"in": "o"}}]), "d")),
        flat(&sine_then(json!([]), "o")),
    );
    let mut rig = playing(&a, &DataSet::empty(), &pk, 40 * BLOCK);
    let r = rig.prepare(&b, &DataSet::empty(), &pk);
    rig.send(Msg::Activate { renderer: Box::new(r) });
    rig.send(Msg::Panic);
    rig.run(BLOCK);
    assert_eq!(rig.e.resets(), 1);
    assert!(rig.e.in_window());
    rig.run(BLOCK);
    assert!(rig.e.in_window(), "240 frames: two blocks");
    rig.run(BLOCK);
    assert!(!rig.e.in_window(), "over at the next block line");
    assert_eq!(rig.retired.iter().filter(|r| matches!(r, Retired::Nodes(_))).count(), 1);
}

// ---------------------------------------------------------------- 8a: the window on many threads

/// A song before and after an edit whose window has every kind of node step 7 and 8b made: the
/// kick, muted, deleted (its consumer a ghost that reads it through an overlay held at the old
/// mute); track `a`'s delay removed (a ghost) and a sidechain from `b` added, while `b`'s sidechain
/// from `a` goes (ghosts): so `a` and `b` read each other only in the window; `b`'s poly kept; `c`'s
/// control lane read only by a ghost (a ghost lane); and a new node in the song reading `a` (a
/// fade-in, which the changed edges wait for).
fn window_edit() -> (Ir, Ir) {
    let c = |id: &str, v: f64| json!({"id": id, "op": "const", "value": v});
    let hz = |id: &str, v: f64| json!({"id": id, "op": "const", "value": v, "unit": "hz"});
    let k = |id: &str, kernel: &str, inputs: Value| json!({"id": id, "op": "kernel", "kernel": kernel, "inputs": inputs});
    let import = |id: &str| json!({"id": id, "op": "import", "module": id, "export": "out"});
    let module = |key: &str, lanes: Value, nodes: Value, out: &str| json!({"key": key, "track": true, "lanes": lanes, "nodes": nodes, "exports": {"out": out}});
    let poly = json!({"id": "v", "op": "poly", "events": "n", "voices": 4, "body": {"out": "o", "nodes": [
        {"id": "p", "op": "lane", "lane": "pitch"},
        {"id": "hz", "op": "kernel", "kernel": "std:mtof", "inputs": {"pitch": "p"}},
        {"id": "o", "op": "kernel", "kernel": "std:saw", "inputs": {"freq": "hz"}}]}});
    let notes = json!([{"key": "n", "kind": "events<note>"}]);
    let lvl = json!([{"key": "lvl", "kind": "control<gain>", "default": 0}]);
    let kick = module("kick", json!([]), json!([hz("f", 55.0), k("o", "std:sine", json!({"freq": "f"}))]), "o");
    let ir = |modules: Vec<Value>| -> Ir {
        serde_json::from_value(json!({"ir": m2_ir::IR_VERSION, "root": {"module": "song", "export": "out"}, "modules": modules})).unwrap()
    };
    let before = ir(vec![
        kick,
        module(
            "a",
            json!([]),
            json!([hz("f", 220.0), k("o", "std:sine", json!({"freq": "f"})),
            {"id": "t", "op": "const", "value": 0.01, "unit": "seconds"}, k("d", "std:delay", json!({"in": "o", "time": "t"}))]),
            "d",
        ),
        module(
            "b",
            notes.clone(),
            json!([{"id": "n", "op": "lane", "lane": "n"}, poly.clone(), import("a"),
            k("af", "std:follow", json!({"in": "a"})), k("bd", "std:duck", json!({"in": "v", "side": "af"}))]),
            "bd",
        ),
        module(
            "c",
            lvl.clone(),
            json!([hz("f", 330.0), k("o", "std:sine", json!({"freq": "f"})), {"id": "l", "op": "lane", "lane": "lvl"},
            k("g", "std:scale", json!({"in": "o", "by": "l"}))]),
            "g",
        ),
        json!({"key": "song", "nodes": [import("kick"), import("a"), import("b"), import("c"), c("half", 0.5),
            k("g", "std:scale", json!({"in": "kick", "by": "half"})), k("m", "std:add", json!({"a": "g", "b": "a"})),
            k("n2", "std:add", json!({"a": "b", "b": "c"})), k("mix", "std:add", json!({"a": "m", "b": "n2"}))], "exports": {"out": "mix"}}),
    ]);
    let after = ir(vec![
        module(
            "a",
            json!([]),
            json!([
                hz("f", 220.0),
                k("o", "std:sine", json!({"freq": "f"})),
                import("b"),
                k("bf", "std:follow", json!({"in": "b"})),
                k("du", "std:duck", json!({"in": "o", "side": "bf"}))
            ]),
            "du",
        ),
        module("b", notes, json!([{"id": "n", "op": "lane", "lane": "n"}, poly]), "v"),
        module("c", lvl, json!([hz("f", 330.0), k("o", "std:sine", json!({"freq": "f"}))]), "o"),
        json!({"key": "song", "nodes": [import("a"), import("b"), import("c"), c("q", 0.25),
            k("a2", "std:scale", json!({"in": "a", "by": "q"})), k("m", "std:add", json!({"a": "a2", "b": "a"})),
            k("n2", "std:add", json!({"a": "b", "b": "c"})), k("mix", "std:add", json!({"a": "m", "b": "n2"}))], "exports": {"out": "mix"}}),
    ]);
    (before, after)
}

/// [`window_edit`] played live on `threads` threads (spread) or this one alone: 8 blocks, the edit,
/// then 24 blocks, its window included. What it played, the rig, and the edit's plan's facts.
fn window_on(threads: Option<usize>) -> (Vec<[f32; 2]>, Rig, Vec<(String, u16)>) {
    let pk = std_packages();
    let (before, after) = window_edit();
    let notes: Vec<Value> = (0..12).map(|i| json!({"pitch": 48 + (i * 7) % 24, "start": i as f64 * 0.25, "length": 0.4})).collect();
    let (data, dir) = timeline_for(
        &before,
        &[
            ("data/b/n.json", json!({"clips": [{"start": 0, "length": 8, "notes": notes}]})),
            ("data/c/lvl.json", json!({"unit": "gain", "clips": [{"start": 0, "length": 8, "points": [[0, 0.5], [8, 0.9]]}]})),
        ],
    );
    let (old, new) = (flat(&before), flat(&after));
    let mut r = build(&old, &data, &pk, &Owner::discard());
    r.set_overlays(&old.overlays.iter().map(|s| if s.module == "kick" { 0.0 } else { 1.0 }).collect::<Vec<_>>());
    let mut rig = Rig::of(&old, &data, r);
    rig.threads = threads.map(spread);
    rig.cmd(Cmd::Play);
    let mut out = rig.run(8 * BLOCK);
    let mut next = rig.prepare(&new, &data, &pk);
    next.set_overlays(&vec![1.0; new.overlays.len()]);
    let parts = &next.plan.schedule.parts;
    let labels = (0..parts.len() as u32).map(|q| (parts.label(q).0.to_string(), parts.label(q).1)).collect();
    let s = next.stats().clone();
    assert!(s.ghosts >= 4 && next.plan.wait > 0 && s.kept.polys == 1, "ghosts, a waiting fade and a kept poly: {s:?}");
    assert!(next.lanes.slots.len() > next.lanes.live, "a ghost lane");
    assert!(!next.plan.window_held.is_empty() && !next.plan.windowed.is_empty(), "the window's own overlays, one held at the kick's mute");
    rig.send(Msg::Activate { renderer: Box::new(next) });
    out.extend(rig.run(24 * BLOCK));
    assert!(!rig.e.in_window(), "the window is over");
    std::fs::remove_dir_all(dir).ok();
    (out, rig, labels)
}

// RENDER-2 (8a): an edit's window renders the same bits on 4 threads as on one, through the
// activation and after it: ghosts, fades waiting for fade-ins, a kept poly, a ghost lane, a muted
// track deleted while muted, and two tracks that read each other only in the window, split by
// depth.
#[test]
fn an_edit_window_renders_the_same_bits_on_1_and_n_threads() {
    let (one, _, labels) = window_on(None);
    assert!(
        labels.iter().any(|(l, d)| l == "a" && *d > 0) && labels.iter().any(|(l, d)| l == "b" && *d > 0),
        "a and b split by depth: {labels:?}"
    );
    assert!(peak(&one) > 0.1);
    let (four, rig, _) = window_on(Some(4));
    let t = rig.threads.as_ref().unwrap().runner.spread_report();
    assert!(t.tasks_on_workers > 0 && t.edges_across > 0, "{t:?}");
    assert!(four == one, "the same bits on 4 threads");
}

// RENDER-25a (8a): once the window is over, its ghosts and its own overlays are constant zeros
// that nothing reads, the one schedule plays on, and only the ghosts left the audio thread.
#[test]
fn ghosts_and_window_overlays_go_quiet_after_the_window() {
    let (_, rig, _) = window_on(None);
    let r = rig.e.current().unwrap();
    for i in r.plan.ghosts.clone().chain(r.plan.windowed.clone()) {
        assert!(matches!(r.plan.graph.exec[i], Exec::Const(0.0)), "node {i} is a constant zero");
    }
    let left: Vec<&Leftovers> = rig.retired.iter().filter_map(|r| if let Retired::Nodes(l) = r { Some(&**l) } else { None }).collect();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].exec.len(), r.plan.ghosts.len(), "only the ghosts left");
    assert!(left[0].exec.iter().all(|e| !matches!(e, Exec::Overlay { .. })));
}
