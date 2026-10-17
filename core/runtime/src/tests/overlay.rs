//! Monitoring overlays in the runtime (MON-1, MON-5, RENDER-22a): one gain node at each site, in
//! every plan; unity changes no bit; a new value glides over 10 ms; mute in song silences offline
//! renders; a new plan starts where the old one was; and an edit while a track is muted never
//! lets it through for the fade window.

use super::live::{Rig, flat, live};
use super::*;
use crate::engine::Msg;

/// A `kick` track (a 110 Hz sine) and `song` with `nodes` after an import of it (`k`), output `out`.
fn kicked(nodes: Value, out: &str, muted: bool) -> Ir {
    let mut song = vec![json!({"id": "k", "op": "import", "module": "kick", "export": "out"})];
    song.extend(nodes.as_array().unwrap().iter().cloned());
    serde_json::from_value(json!({"ir": m2_ir::IR_VERSION, "root": {"module": "song", "export": "out"}, "modules": [
        {"key": "kick", "track": true, "muted": muted, "nodes": [
            {"id": "f", "op": "const", "value": 110, "unit": "hz"},
            {"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "f"}}],
         "exports": {"out": "o"}},
        {"key": "song", "nodes": song, "exports": {"out": out}}]}))
    .unwrap()
}

/// `song`'s mixer: the kick at half, and the kick read again at a quarter.
fn mixer() -> Value {
    json!([
        {"id": "a", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "k", "by": "h"}},
        {"id": "h", "op": "const", "value": 0.5},
        {"id": "b", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "k", "by": "q"}},
        {"id": "q", "op": "const", "value": 0.25},
        {"id": "m", "op": "kernel", "kernel": "std:add", "inputs": {"a": "a", "b": "b"}}])
}

fn bits(x: &[[f32; 2]]) -> Vec<[u32; 2]> {
    x.iter().map(|f| [f[0].to_bits(), f[1].to_bits()]).collect()
}

/// Every sample +0.0 or −0.0 (A9: compared as numbers).
fn silent(x: &[[f32; 2]]) -> bool {
    x.iter().all(|f| f[0] == 0.0 && f[1] == 0.0)
}

// MON-1, RENDER-2: every plan has an overlay at each site, and at 1 it copies: the song renders the
// same bits as the graph with none.
#[test]
fn an_overlay_at_unity_changes_no_bit() {
    let pk = std_packages();
    for (nodes, out) in [(mixer(), "m"), (json!([]), "k")] {
        let f = flat(&kicked(nodes, out, false));
        let mut bare = f.clone();
        bare.overlays.clear();
        let r = Renderer::new(&f, &DataSet::empty(), &pk, SR).unwrap();
        assert_eq!(r.info().overlays.len(), if out == "m" { 2 } else { 1 }, "one cell per site");
        let with = r.info().overlays.iter().map(|o| r.cell_value(o.2)).collect::<Vec<_>>();
        assert!(with.iter().all(|v| *v == Some(1.0)), "{with:?}");
        let a = Renderer::new(&f, &DataSet::empty(), &pk, SR).unwrap().render(0.5).unwrap();
        let b = Renderer::new(&bare, &DataSet::empty(), &pk, SR).unwrap().render(0.5).unwrap();
        assert!(peak(&a) > 0.1);
        assert!(bits(&a) == bits(&b), "{out}: the same bits");
    }
}

// RENDER-22a: an overlay's new value glides there by 1/480 a frame at 48 kHz (10 ms), from the
// block line after the write, each sample its input times the gain; then it writes +0.0.
#[test]
fn an_overlay_glides_over_ten_milliseconds() {
    let pk = std_packages();
    let f = flat(&kicked(json!([]), "k", false));
    let mut bare = f.clone();
    bare.overlays.clear();
    let mut r = Renderer::new(&f, &DataSet::empty(), &pk, SR).unwrap();
    let cell = r.info().overlays[0].2;
    let reference = Renderer::new(&bare, &DataSet::empty(), &pk, SR).unwrap().render(16.0 * 128.0 / SR as f64).unwrap();
    let mut out = r.render(10.0 * 128.0 / SR as f64).unwrap();
    r.cell_slots().set(&[cell], 0.0, 1);
    out.extend(r.render(6.0 * 128.0 / SR as f64).unwrap());
    let (at, step) = (10 * 128, 1.0f32 / 480.0);
    assert!(bits(&out[..at]) == bits(&reference[..at]), "nothing moves before the block line");
    let mut g = 1.0f32;
    for k in 0..480 {
        g = (g - step).max(0.0);
        let want = reference[at + k][0] * g;
        assert_eq!(out[at + k][0].to_bits(), want.to_bits(), "frame {k} of the glide");
    }
    assert_eq!(g, 0.0, "the glide ends at 480 frames");
    assert!(out[at + 480..].iter().all(|x| x[0].to_bits() == 0 && x[1].to_bits() == 0), "+0.0 once there");
    assert_eq!(r.cell_value(cell), Some(0.0));
}

// MON-5: mute in song silences the track in an offline render, wherever it is read; the song still
// renders its other parts.
#[test]
fn muted_in_song_silences_an_offline_render() {
    let pk = std_packages();
    for (nodes, out) in [(mixer(), "m"), (json!([]), "k")] {
        let f = flat(&kicked(nodes, out, true));
        assert_eq!(f.muted, ["kick"]);
        let r = Renderer::new(&f, &DataSet::empty(), &pk, SR).unwrap();
        assert!(r.info().overlays.iter().all(|o| r.cell_value(o.2) == Some(0.0)));
        let x = render(&kicked(json!([]), "k", true), &DataSet::empty(), 0.25);
        assert!(x.iter().all(|f| f[0].to_bits() == 0), "+0.0, never −0.0");
        let y = Renderer::new(&f, &DataSet::empty(), &pk, SR).unwrap().render(0.25).unwrap();
        assert!(silent(&y), "{out}");
    }
    // Only the muted track: a second track plays on.
    let ir: Ir = serde_json::from_value(json!({"ir": m2_ir::IR_VERSION, "root": {"module": "song", "export": "out"}, "modules": [
        {"key": "kick", "track": true, "muted": true, "nodes": [{"id": "o", "op": "kernel", "kernel": "std:sine"}], "exports": {"out": "o"}},
        {"key": "bass", "track": true, "nodes": [{"id": "o", "op": "kernel", "kernel": "std:sine"}], "exports": {"out": "o"}},
        {"key": "song", "nodes": [
            {"id": "k", "op": "import", "module": "kick", "export": "out"},
            {"id": "b", "op": "import", "module": "bass", "export": "out"},
            {"id": "m", "op": "kernel", "kernel": "std:add", "inputs": {"a": "k", "b": "b"}}], "exports": {"out": "m"}}]}))
    .unwrap();
    let mut alone = ir.clone();
    alone.modules.retain(|m| m.key != "kick");
    alone.modules[1].nodes = vec![serde_json::from_value(json!({"id": "b", "op": "import", "module": "bass", "export": "out"})).unwrap()];
    alone.modules[1].exports.insert("out".into(), "b".into());
    assert!(bits(&render(&ir, &DataSet::empty(), 0.25)) == bits(&render(&alone, &DataSet::empty(), 0.25)), "the bass alone, to the bit");
}

/// A live plan of `ir` with every overlay at `v`, played from 0.
fn muted_rig(ir: &Ir, v: f32) -> (Rig, Flat) {
    let f = flat(ir);
    let mut r = live(ir, &DataSet::empty());
    r.set_overlays(&vec![v; f.overlays.len()]);
    let mut rig = Rig::of(&f, &DataSet::empty(), r);
    rig.cmd(Cmd::Play);
    (rig, f)
}

// MON-1, R6: a plan given its overlays' values before it is sent starts there, with no glide at
// entry; one not given them starts at 1. A panic leaves them as they are.
#[test]
fn a_new_plan_starts_at_its_overlay_values() {
    let pk = std_packages();
    let ir = kicked(mixer(), "m", false);
    let (mut rig, f) = muted_rig(&ir, 0.0);
    assert!(silent(&rig.run(4096)), "muted from its first block");
    let mut next = rig.prepare(&f, &DataSet::empty(), &pk);
    next.set_overlays(&[0.0, 0.0]);
    rig.send(Msg::Activate { renderer: Box::new(next) });
    assert!(silent(&rig.run(4096)), "the new plan starts muted");
    rig.send(Msg::Panic);
    assert!(silent(&rig.run(4096)), "a panic leaves overlays alone");
    let unset = rig.prepare(&f, &DataSet::empty(), &pk);
    rig.send(Msg::Activate { renderer: Box::new(unset) });
    assert!(peak(&rig.run(4096)) > 0.3, "the negative control: a plan not given them plays the kick");
}

// MON-1, A3: an edit while the kick is muted keeps it out through the activation's window: a
// replaced consumer plays out as a ghost that reads the kick through an overlay, and a changed
// input's fade reads the kick on its old side through one too.
#[test]
fn an_edit_while_muted_keeps_the_track_silent() {
    let pk = std_packages();
    let scale = |id: &str, input: &str, by: &str| json!({"id": id, "op": "kernel", "kernel": "std:scale", "inputs": {"in": input, "by": by}});
    let c = |id: &str, v: f64| json!({"id": id, "op": "const", "value": v});
    let cases = [
        // The consumer replaced: `g` goes, a new `h` reads the kick instead.
        ("replaced", kicked(json!([scale("g", "k", "x"), c("x", 0.5)]), "g", false), kicked(json!([scale("h", "k", "y"), c("y", 0.25)]), "h", false)),
        // The consumer kept, its input changed: `m` read the kick, and reads a new `fx` of it.
        (
            "faded",
            kicked(json!([scale("m", "k", "x"), c("x", 0.5)]), "m", false),
            kicked(json!([scale("fx", "k", "y"), c("y", 0.5), scale("m", "fx", "x"), c("x", 0.5)]), "m", false),
        ),
    ];
    for (what, before, after) in cases {
        for muted in [true, false] {
            let (mut rig, _) = muted_rig(&before, if muted { 0.0 } else { 1.0 });
            rig.run(4096);
            let f = flat(&after);
            let mut next = rig.prepare(&f, &DataSet::empty(), &pk);
            next.set_overlays(&vec![if muted { 0.0 } else { 1.0 }; f.overlays.len()]);
            assert!(next.stats().ghosts + next.stats().fades > 0, "{what}: an activation with a window");
            rig.send(Msg::Activate { renderer: Box::new(next) });
            let out = rig.run(4096);
            match muted {
                true => assert!(silent(&out), "{what}: the kick stays out through the window, peak {}", peak(&out)),
                false => assert!(peak(&out) > 0.05, "{what}: the control: unmuted, it plays"),
            }
        }
    }
}
