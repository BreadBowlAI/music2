//! Ports: every node's inputs and outputs, named where they are written, and edges
//! that join an output to an input.

use std::path::{Path, PathBuf};

use m2_runtime::PackageSet;

use super::*;
use crate::Gestures;

struct Fixture(PathBuf);

impl Fixture {
    fn dir() -> PathBuf {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        std::env::temp_dir().join(format!("m2-view-test-{}-{n}", std::process::id()))
    }

    /// A copy of the session tests' first-song project.
    fn first_song() -> Self {
        fn copy(from: &Path, to: &Path) {
            std::fs::create_dir_all(to).unwrap();
            for e in std::fs::read_dir(from).unwrap().flatten() {
                let (f, t) = (e.path(), to.join(e.file_name()));
                if f.is_dir() {
                    copy(&f, &t)
                } else {
                    std::fs::copy(&f, &t).unwrap();
                }
            }
        }
        let dir = Self::dir();
        copy(&crate::testing::fixtures().join("first-song"), &dir);
        Self(dir)
    }

    fn song(source: &str) -> Self {
        let dir = Self::dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("music2.toml"), crate::testing::MANIFEST).unwrap();
        std::fs::write(dir.join("song.ts"), source).unwrap();
        Self(dir)
    }

    fn open(&self) -> Session {
        let mut packages = PackageSet::default();
        packages.load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")).unwrap();
        Session::open(&self.0, packages).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn module<'a>(v: &'a GraphView, name: &str) -> &'a ModuleView {
    v.modules.iter().find(|m| m.key == name).unwrap()
}

fn node<'a>(m: &'a ModuleView, label: &str) -> &'a ViewNode {
    m.nodes.iter().find(|n| n.label == label).unwrap_or_else(|| panic!("no {label} in {}", m.key))
}

/// `label:kind` of each input, in order.
fn inputs(n: &ViewNode) -> Vec<String> {
    n.inputs.iter().map(|p| format!("{}:{}", p.label, p.kind)).collect()
}

fn outputs(n: &ViewNode) -> Vec<String> {
    n.outputs.iter().map(|p| format!("{}:{}", p.label, p.kind)).collect()
}

/// The port edge from node `a` to node `b`: (output label, input label, kind).
fn wire(m: &ModuleView, a: &str, b: &str) -> (String, String, &'static str) {
    let (a, b) = (node(m, a), node(m, b));
    let e = m.port_edges.iter().find(|e| e.from == a.id && e.to == b.id).unwrap_or_else(|| panic!("no edge {} → {}", a.label, b.label));
    let out = a.outputs.iter().find(|p| p.id == e.from_port).expect("the edge starts at an output of its node");
    let into = b.inputs.iter().find(|p| p.id == e.to_port).expect("the edge ends at an input of its node");
    (out.label.clone(), into.label.clone(), e.kind)
}

/// Every port edge joins ports that exist, and they are the node edges, port by port.
fn consistent(m: &ModuleView) {
    for e in &m.port_edges {
        let from = m.nodes.iter().find(|n| n.id == e.from).unwrap();
        let to = m.nodes.iter().find(|n| n.id == e.to).unwrap();
        let out = from.outputs.iter().find(|p| p.id == e.from_port).unwrap_or_else(|| panic!("{e:?}: no such output"));
        let into = to.inputs.iter().find(|p| p.id == e.to_port).unwrap_or_else(|| panic!("{e:?}: no such input"));
        assert_eq!((out.kind, into.kind), (e.kind, e.kind), "{e:?}");
    }
    let mut pairs: Vec<(String, String)> = m.port_edges.iter().map(|e| (e.from.clone(), e.to.clone())).collect();
    pairs.sort();
    pairs.dedup();
    let mut edges = m.edges.clone();
    edges.sort();
    assert_eq!(pairs, edges, "{}", m.key);
}

// ID-10, ID-17: a label renamed by hand reaches what shows it (the step 4 gate's last check,
// moved here with the timeline and the canvas; the gate itself checks nothing else changed).
#[test]
fn a_renamed_label_is_what_the_timeline_and_the_canvas_show() {
    let fixture = Fixture::first_song();
    let mut s = fixture.open();
    let file = fixture.0.join("tracks/kick.ts");
    let kick = std::fs::read_to_string(&file).unwrap();
    std::fs::write(&file, kick.replacen("{ name: \"Kick\" }", "{ name: \"Big Kick\" }", 1)).unwrap();
    assert!(s.refresh() && s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    let track = s.timeline().tracks.into_iter().find(|t| t.module == "kick").unwrap();
    assert_eq!(track.label, "Big Kick");
    assert_eq!(module(&s.view(), "kick").label, "Big Kick");
}

#[test]
fn the_bass_tracks_ports_are_named_where_they_are_written_and_typed() {
    let fixture = Fixture::first_song();
    let v = fixture.open().view();
    let bass = module(&v, "bass");
    consistent(bass);
    assert_eq!(bass.port_edges.len(), 8);

    let xrange = node(bass, "xrange");
    assert_eq!(inputs(xrange), ["x:control", "lo:control", "hi:control"]);
    let knobs: Vec<Option<&str>> = xrange.inputs.iter().map(|p| p.knob.as_deref()).collect();
    assert_eq!(knobs, [None, Some(xrange.knobs[0].id.as_str()), Some(xrange.knobs[1].id.as_str())]);
    assert_eq!(outputs(xrange), ["wobble:control"]);
    assert!(xrange.outputs[0].outlet, "a cable starts at the `const` it is bound to");
    assert!(!node(bass, "lfo").outputs[0].outlet, "lfo's result has no name to connect");

    // Cables are the inputs they are written at.
    let blend = node(bass, "blend");
    assert_eq!(inputs(blend), ["a:control", "b:control", "m:control"]);
    let inlets: Vec<Option<&str>> = blend.inputs.iter().map(|p| p.inlet.as_deref()).collect();
    assert_eq!(inlets, blend.inlets.iter().map(|i| Some(i.id.as_str())).collect::<Vec<_>>());
    assert_eq!(wire(bass, "xrange", "blend"), ("wobble".into(), "b".into(), "control"));
    assert_eq!(wire(bass, "drop", "blend"), ("drop".into(), "m".into(), "control"));

    // The cutoff is read by two kernels inside the instrument, but it is one input. The drive it
    // leaves out is an input too, a knob at its default, after what the call writes (PKG-3).
    let instrument = node(bass, "bass");
    assert_eq!(inputs(instrument), ["notes:notes", "cutoff:control", "drive:control"]);
    assert_eq!(outputs(instrument), ["audio:audio"]);
    assert_eq!(wire(bass, "notes", "bass"), ("notes".into(), "notes".into(), "notes"));
    assert_eq!(wire(bass, "blend", "bass"), ("control".into(), "cutoff".into(), "control"));
    assert_eq!(wire(bass, "time.beats", "lfo"), ("control".into(), "beats".into(), "control"));
    assert_eq!(wire(bass, "bass", "out"), ("audio".into(), "audio".into(), "audio"));
    assert!(node(bass, "out").outputs.is_empty() && node(bass, "notes").inputs.is_empty());
}

#[test]
fn a_call_is_titled_by_its_components_label_and_keeps_its_code_name() {
    let fixture = Fixture::first_song();
    let v = fixture.open().view();
    let bass = module(&v, "bass");
    // The canvas says "LFO" and "Bass" (taste.md DV-2); the code's names stay what nodes are found by.
    for (code, title) in [("lfo", "LFO"), ("xrange", "Xrange"), ("blend", "Blend"), ("bass", "Bass")] {
        assert_eq!(node(bass, code).title.as_deref(), Some(title), "{code}");
    }
    // Lanes, the time and exports aren't calls of a component: they keep their own names.
    for name in ["notes", "cutoff", "time.beats", "out"] {
        assert_eq!(node(bass, name).title, None, "{name}");
    }
    let song = module(&v, "song");
    assert_eq!(node(song, "mixer").title.as_deref(), Some("Mixer"));
    assert_eq!(node(song, "duck").title.as_deref(), Some("Duck"));
}

#[test]
fn the_mixers_inputs_follow_the_code_and_imports_land_where_they_are_written() {
    let fixture = Fixture::first_song();
    let v = fixture.open().view();
    let song = module(&v, "song");
    consistent(song);
    let mixer = node(song, "mixer");
    let labels: Vec<&str> = mixer.inputs.iter().map(|p| p.label.as_str()).collect();
    assert_eq!(labels, ["kick", "hats.in", "hats.db", "hats.pan", "bass", "keys.in", "keys.db", "keys.pan"]);
    assert!(mixer.inputs.iter().filter(|p| p.label.ends_with("db") || p.label.ends_with("pan")).all(|p| p.knob.is_some()));
    // Imports are shown by their track's label (ID-10).
    assert_eq!(wire(song, "Hats.out", "mixer"), ("audio".into(), "hats.in".into(), "audio"));
    assert_eq!(wire(song, "duck", "mixer"), ("audio".into(), "bass".into(), "audio"));
    assert_eq!(wire(song, "Kick.env", "duck"), ("control".into(), "side".into(), "control"));
    assert_eq!(inputs(node(song, "duck")), ["in:audio", "side:control", "depth:control"]);

    // Another track's export is typed as that track's graph types it.
    let kick = module(&v, "kick");
    consistent(kick);
    let follow = node(kick, "follow");
    // The attack it leaves out comes after what it writes (PKG-3).
    assert_eq!(inputs(follow), ["in:audio", "release:control", "attack:control"]);
    assert!(follow.inputs[0].inlet.is_some(), "`out` is a cable from the kick");
    assert_eq!(wire(kick, "kick", "follow"), ("out".into(), "in".into(), "audio"));
}

#[test]
fn an_unread_outlet_still_has_its_output_and_a_shared_value_feeds_each_consumer() {
    let fixture = Fixture::song(
        r#"import { hz, khz, song } from "@m2/core";
import { lfo } from "std/control";
import { osc, svf } from "std/dsp";
import { xrange } from "std/math";

export default song(({ time }) => {
  const wobble = xrange(lfo(time.beats, { period: 0.5 }), hz(200), khz(2.5));
  return svf.lowpass(osc.saw(hz(55)), hz(1000));
});
"#,
    );
    let v = fixture.open().view();
    let m = module(&v, "song");
    consistent(m);
    let xrange = node(m, "xrange");
    assert_eq!(outputs(xrange), ["wobble:control"]);
    assert!(xrange.outputs[0].outlet);
    assert!(m.port_edges.iter().all(|e| e.from != xrange.id), "nothing reads it yet");
    let filter = node(m, "svf.lowpass");
    assert_eq!(inputs(filter), ["in:audio", "cutoff:control", "q:control"], "the resonance it leaves out is a knob too (PKG-3)");
    assert_eq!(outputs(filter), ["audio:audio"], "its own result is named by what it is, not by the port `lp`");

    let fixture = Fixture::song(
        r#"import { hz, song } from "@m2/core";
import { osc } from "std/dsp";
import { add } from "std/math";
export default song(() => {
  const cutoff = hz(800);
  const left = osc.sine(cutoff);
  const right = osc.saw(cutoff);
  return add(left, right);
});
"#,
    );
    let v = fixture.open().view();
    let m = module(&v, "song");
    consistent(m);
    let shared = m.nodes.iter().find(|n| n.kind == "value").unwrap();
    assert_eq!(outputs(shared), ["control:control"]);
    assert!(shared.inputs[0].knob.is_some());
    assert_eq!(wire(m, "cutoff", "osc.sine"), ("control".into(), "freq".into(), "control"));
    assert_eq!(wire(m, "cutoff", "osc.saw"), ("control".into(), "freq".into(), "control"));
}

// PKG-8: a knob spans the range its control declares, in the declaration's spelling turned to
// base units; the value written never moves it (a cutoff turned down to 20 Hz still turns back
// up), and nothing is guessed from a unit (P10).
#[test]
fn the_new_effects_knobs_span_what_their_controls_declare() {
    let fixture = Fixture::song(
        "import { song } from \"@m2/core\";\nimport { osc } from \"std/dsp\";\nimport { lowpass } from \"std/filter\";\n\
         import { bitcrush, chorus, compress, widen } from \"std/fx\";\n\
         export default song(() => widen(bitcrush(compress(chorus(lowpass(osc.saw(110), 20), { rate: 0.8 }), \
         { ratio: 3, attack: 0.01, release: 0.15 }), { bits: 8 }), { width: 1.5 }));\n",
    );
    let s = fixture.open();
    let v = s.view();
    let knobs: Vec<&Knob> = v.modules.iter().flat_map(|m| m.nodes.iter().flat_map(|n| &n.knobs)).collect();
    let knob = |label: &str| {
        knobs.iter().find(|k| k.label == label && k.written).map(|k| (k.range, k.log)).unwrap_or_else(|| panic!("no {label} knob"))
    };
    assert_eq!(knob("cutoff"), (Some([20.0, 20_000.0]), true), "a cutoff at 20 Hz turns back up");
    // A chorus's 0.8 Hz sweep is a rate, as its control declares: a slow drift to 20 a second.
    assert_eq!(knob("rate"), (Some([0.05, 20.0]), true));
    assert_eq!(knob("ratio"), (Some([1.0, 20.0]), true));
    assert_eq!(knob("attack"), (Some([0.0001, 1.0]), true));
    assert_eq!(knob("release"), (Some([0.001, 5.0]), true));
    assert_eq!(knob("bits"), (Some([1.0, 16.0]), false));
    assert_eq!(knob("width"), (Some([0.0, 2.0]), false));
}

// The sounds plan's O3: std's chorus, phaser, tremolo and bitcrush read song time inside their own
// calls, so a chain of them shows what it showed before: one node per call, the audio running
// from each to the next, and only the knobs their controls declare. No `time` node, no cable from
// one, and no `playing` knob; two choruses in a row too.
#[test]
fn effects_that_follow_the_song_show_no_time_node() {
    let fixture = Fixture::song(
        "import { song } from \"@m2/core\";\nimport { osc } from \"std/dsp\";\n\
         import { bitcrush, chorus, phaser, tremolo } from \"std/fx\";\n\
         export default song(() => bitcrush(tremolo(phaser(chorus(chorus(osc.saw(110)), { rate: 0.8 })))));\n",
    );
    let s = fixture.open();
    let v = s.view();
    let m = module(&v, "song");
    consistent(m);
    let shown: Vec<(&str, &str, Vec<&str>, Vec<String>)> = m
        .nodes
        .iter()
        .filter(|n| n.kind != "export")
        .map(|n| (n.kind, n.label.as_str(), n.knobs.iter().map(|k| k.label.as_str()).collect(), inputs(n)))
        .collect();
    assert!(shown.iter().all(|(kind, ..)| *kind == "call"), "{shown:#?}");
    let calls: Vec<&str> = shown.iter().map(|(_, label, ..)| *label).collect();
    let mut sorted = calls.clone();
    sorted.sort();
    assert_eq!(sorted, ["bitcrush", "chorus", "chorus", "osc.saw", "phaser", "tremolo"], "{shown:#?}");
    for (_, label, knobs, _) in &shown {
        let want: &[&str] = match *label {
            "chorus" => &["rate", "depth", "mix"],
            "phaser" => &["rate", "depth", "feedback", "mix"],
            "tremolo" => &["rate", "depth"],
            "bitcrush" => &["bits", "rate", "mix"],
            _ => continue,
        };
        let mut have = knobs.clone();
        have.sort();
        let mut want = want.to_vec();
        want.sort();
        assert_eq!(have, want, "{label}'s knobs");
    }
    // The audio runs down the chain to the song's output, and nothing else is cabled.
    assert_eq!(m.edges.len(), 6, "{:?}", m.edges);
    assert_eq!(wire(m, "tremolo", "bitcrush").2, "audio");
}

#[test]
fn a_name_written_at_two_inputs_of_one_call_is_an_edge_into_each() {
    for (call, labels) in [
        ("mixer({ left: { in: dry, pan: -1 }, right: { in: dry, pan: 1 } })", ["left.in", "right.in"]),
        ("add(dry, dry)", ["input 1", "input 2"]),
    ] {
        let fixture = Fixture::song(&format!(
            "import {{ hz, song }} from \"@m2/core\";\nimport {{ osc }} from \"std/dsp\";\nimport {{ add }} from \"std/math\";\n\
             import {{ mixer }} from \"std/mix\";\nexport default song(() => {{\n  const dry = osc.saw(hz(110));\n  return {call};\n}});\n"
        ));
        let v = fixture.open().view();
        let m = module(&v, "song");
        consistent(m);
        let to = node(m, call.split('(').next().unwrap());
        let from = node(m, "osc.saw");
        let into: Vec<&str> = m
            .port_edges
            .iter()
            .filter(|e| e.from == from.id && e.to == to.id)
            .map(|e| to.inputs.iter().find(|p| p.id == e.to_port).unwrap().label.as_str())
            .collect();
        assert_eq!(into, labels, "{call}: one edge into each input `dry` is written at");
    }
}

#[test]
fn signals_made_by_std_helpers_and_a_voices_note_are_nodes_with_their_edges() {
    let fixture = Fixture::song(
        r#"import { hz, song } from "@m2/core";
import { when } from "std/control";
import { adsr, noise, osc, vca } from "std/dsp";
import { highpass, lowpass } from "std/filter";
import { sum } from "std/math";
import { poly } from "@m2/core";
export default song(({ lane }) => {
  const dry = osc.saw(hz(110));
  const dark = lowpass(dry, 500);
  const voice = poly(lane.notes("keys"), (n) => vca(osc.saw(n.pitch), adsr(n.gate)));
  return sum(dry, dark, when(lane.mask("open"), highpass(noise(), 8000)), voice);
});
"#,
    );
    let v = fixture.open().view();
    let m = module(&v, "song");
    consistent(m);
    let sum = node(m, "sum");
    assert_eq!(inputs(sum), ["input 1:audio", "input 2:audio", "input 3:audio", "input 4:audio"]);
    assert_eq!(wire(m, "sum", "out").2, "audio", "the export reads the sum");
    assert_eq!(inputs(node(m, "out")), ["audio:audio"]);
    assert_eq!(wire(m, "noise", "highpass"), ("audio".into(), "in".into(), "audio"));
    assert_eq!(wire(m, "highpass", "when").2, "audio");
    assert_eq!(wire(m, "open", "when").2, "control");
    // The note a voice plays comes from its own node, beside the `poly` call.
    let note = node(m, "note");
    assert_eq!((note.kind, outputs(note)), ("voice", vec!["pitch:control".to_string(), "gate:control".into()]));
    let saw = m.nodes.iter().find(|n| n.label == "osc.saw" && n.inputs.iter().any(|p| p.label == "pitch")).expect("the voice's saw");
    let pitch = m.port_edges.iter().find(|e| e.from == note.id && e.to == saw.id).expect("note → saw");
    assert_eq!(note.outputs.iter().find(|p| p.id == pitch.from_port).unwrap().label, "pitch");
    assert_eq!(wire(m, "note", "adsr").0, "gate");
    assert_eq!(wire(m, "vca", "poly"), ("audio".into(), "voice".into(), "audio"));
    assert_eq!(wire(m, "poly", "sum").2, "audio");
}

#[test]
fn the_edge_a_feedback_reads_one_block_late_says_so() {
    let fixture = Fixture::song(
        r#"import { feedback, song, type AudioKind } from "@m2/core";
import { osc } from "std/dsp";
import { lowpass } from "std/filter";
import { add, scale } from "std/math";
export default song(() => {
  const dry = osc.saw(110);
  const echo = feedback<AudioKind>("audio<1>");
  const out = add(dry, scale(echo, 0.5));
  echo.set(lowpass(out, 2000));
  return out;
});
"#,
    );
    let v = fixture.open().view();
    let m = module(&v, "song");
    consistent(m);
    let label = |id: &str| m.nodes.iter().find(|n| n.id == id).unwrap().label.as_str();
    let late: Vec<(&str, &str)> = m.port_edges.iter().filter(|e| e.late).map(|e| (label(&e.from), label(&e.to))).collect();
    assert_eq!(late, [("lowpass", "feedback")]);
}
