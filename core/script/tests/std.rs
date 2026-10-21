//! std on component descriptors (step 5): every export is a descriptor component, the effects
//! `m2pkg.toml` used to list are its descriptors' editor metadata, every default is the one
//! songs heard before, and the calls songs write keep working. The instruments added since
//! (std/synth, std/fm, std/drums) build bare, and every knob of theirs reaches the sound.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use m2_ir::{Module, Node, Op, Unit, split_ref};
use m2_script::component::Spelled;
use m2_script::{Build, Descriptor, Options, build_full, describe};

fn std_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")
}

fn build(song: &str) -> Build {
    let mut o = Options::new([("song.ts".to_string(), song.to_string())].into(), "song.ts");
    o.packages.push(("std".into(), std_dir()));
    o.dependencies.push("std".into());
    build_full(&o).unwrap_or_else(|e| panic!("{e:?}\n{song}"))
}

/// Every node of a module, voices included.
fn nodes(m: &Module) -> Vec<&Node> {
    fn walk<'a>(ns: &'a [Node], out: &mut Vec<&'a Node>) {
        for n in ns {
            out.push(n);
            if let Op::Poly { body, .. } = &n.op {
                walk(&body.nodes, out);
            }
        }
    }
    let mut out = vec![];
    walk(&m.nodes, &mut out);
    out
}

/// The ComponentId of the frame a constant of a declared control names.
fn component_of<'a>(m: &'a Module, n: &Node) -> &'a str {
    let slot = n.origin.as_ref().and_then(|o| o.control.as_ref()).expect("a declared control");
    m.frames[slot.frame as usize].component.as_deref().expect("a descriptor component")
}

/// Each std export songs had at step 5, as `(ComponentId, module, name)`: 46 components.
const EXPORTS: [(&str, &str, &str); 46] = [
    ("std:add", "std/math", "add"),
    ("std:scale", "std/math", "scale"),
    ("std:blend", "std/math", "blend"),
    ("std:sum", "std/math", "sum"),
    ("std:mtof", "std/math", "mtof"),
    ("std:range", "std/math", "range"),
    ("std:xrange", "std/math", "xrange"),
    ("std:lfo", "std/control", "lfo"),
    ("std:when", "std/control", "when"),
    ("std:choose", "std/control", "choose"),
    ("std:sine", "std/dsp", "osc.sine"),
    ("std:saw", "std/dsp", "osc.saw"),
    ("std:square", "std/dsp", "osc.square"),
    ("std:tri", "std/dsp", "osc.tri"),
    ("std:noise", "std/dsp", "noise"),
    ("std:adsr", "std/dsp", "adsr"),
    ("std:follow", "std/dsp", "follow"),
    ("std:svf-lowpass", "std/dsp", "svf.lowpass"),
    ("std:svf-bandpass", "std/dsp", "svf.bandpass"),
    ("std:svf-highpass", "std/dsp", "svf.highpass"),
    ("std:vca", "std/dsp", "vca"),
    ("std:gain", "std/dsp", "gain"),
    ("std:pan", "std/dsp", "pan"),
    ("std:lowpass", "std/filter", "lowpass"),
    ("std:highpass", "std/filter", "highpass"),
    ("std:bandpass", "std/filter", "bandpass"),
    ("std:notch", "std/filter", "notch"),
    ("std:duck", "std/fx", "duck"),
    ("std:delay", "std/fx", "delay"),
    ("std:drive", "std/fx", "drive"),
    ("std:limit", "std/fx", "limit"),
    ("std:reverb", "std/fx", "reverb"),
    ("std:chorus", "std/fx", "chorus"),
    ("std:phaser", "std/fx", "phaser"),
    ("std:tremolo", "std/fx", "tremolo"),
    ("std:compress", "std/fx", "compress"),
    ("std:eq3", "std/fx", "eq3"),
    ("std:bitcrush", "std/fx", "bitcrush"),
    ("std:widen", "std/fx", "widen"),
    ("std:bass", "std/instruments", "bass"),
    ("std:kick", "std/instruments", "kick"),
    ("std:hat", "std/instruments", "hat"),
    ("std:pluck", "std/instruments", "pluck"),
    ("std:pad", "std/instruments", "pad"),
    ("std:mixer", "std/mix", "mixer"),
    ("std:master", "std/mix", "master"),
];

/// The instruments added since step 5, as `(ComponentId, module, name)`: one engine each for
/// the synth, FM and drums, and the instruments voiced on them.
const INSTRUMENTS: [(&str, &str, &str); 23] = [
    ("std:synth-custom", "std/synth", "synth.custom"),
    ("std:synth-supersaw", "std/synth", "synth.supersaw"),
    ("std:synth-lead", "std/synth", "synth.lead"),
    ("std:synth-pad", "std/synth", "synth.pad"),
    ("std:synth-pluck", "std/synth", "synth.pluck"),
    ("std:synth-sub", "std/synth", "synth.sub"),
    ("std:synth-bass", "std/synth", "synth.bass"),
    ("std:synth-strings", "std/synth", "synth.strings"),
    ("std:fm-custom", "std/fm", "fm.custom"),
    ("std:fm-keys", "std/fm", "fm.keys"),
    ("std:fm-bell", "std/fm", "fm.bell"),
    ("std:fm-metal", "std/fm", "fm.metal"),
    ("std:drum-kit", "std/drums", "drums.kit"),
    ("std:drum-kick", "std/drums", "drums.kick"),
    ("std:drum-snare", "std/drums", "drums.snare"),
    ("std:drum-clap", "std/drums", "drums.clap"),
    ("std:drum-hats", "std/drums", "drums.hats"),
    ("std:drum-tom", "std/drums", "drums.tom"),
    ("std:drum-rim", "std/drums", "drums.rim"),
    ("std:drum-cowbell", "std/drums", "drums.cowbell"),
    ("std:drum-cymbal", "std/drums", "drums.cymbal"),
    ("std:drum-shaker", "std/drums", "drums.shaker"),
    ("std:sampler", "std/sampler", "sampler"),
];

// PKG-1, PKG-2, PKG-9: std declares every component with a descriptor, each export is in the
// catalog under its ComponentId, and the manifest describes no TypeScript.
#[test]
fn std_is_declared_with_descriptors() {
    let catalog = describe(&[("std".into(), std_dir())]);
    assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);
    let ids: BTreeSet<&str> = EXPORTS.iter().chain(&INSTRUMENTS).map(|e| e.0).collect();
    assert_eq!(catalog.components.keys().map(String::as_str).collect::<BTreeSet<_>>(), ids);
    for (id, module, name) in EXPORTS.into_iter().chain(INSTRUMENTS) {
        let exports = &catalog.exports[id];
        assert!(exports.iter().any(|e| e.module == module && e.name == name), "{id} is exported as {module}.{name}: {exports:?}");
        let d = &catalog.components[id];
        assert_eq!(
            (d.module.as_str(), d.file.starts_with("std/src/")),
            (exports[0].module.as_str(), true),
            "{id} is defined where it is first exported"
        );
        assert!(!d.label.is_empty() && d.output.is_some(), "{id} has a label and declares its output");
    }
    // `std/control` re-exports the math it builds on: the same components, not new ones.
    for name in ["blend", "range", "xrange"] {
        assert!(catalog.exports[&format!("std:{name}")].iter().any(|e| e.module == "std/control" && e.name == name));
    }
    for entry in std::fs::read_dir(std_dir().join("src")).unwrap() {
        let path = entry.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("component({ controls"), "{} declares a component the old way (PKG-1)", path.display());
    }
    let manifest = std::fs::read_to_string(std_dir().join("m2pkg.toml")).unwrap();
    assert!(!manifest.contains("[[effect]]") && !manifest.contains("args ="), "effects are descriptors, not manifest entries (PKG-2)");
}

// Polish A4: the drum kit names the notes it plays, for the piano roll's rows: the General MIDI
// notes the `std:drum` kernel maps (std/kernels-src/src/drums/tests.rs, KIT); the hats name their
// pedal and open notes. No other std instrument names its notes.
#[test]
fn the_drum_kit_names_its_notes() {
    let catalog = describe(&[("std".into(), std_dir())]);
    let names = |id: &str| -> Vec<(u8, &str)> { catalog.components[id].editor.notes.iter().map(|(k, v)| (*k, v.as_str())).collect() };
    assert_eq!(
        names("std:drum-kit"),
        [
            (36, "Kick"),
            (37, "Rim"),
            (38, "Snare"),
            (39, "Clap"),
            (41, "Tom 1"),
            (42, "Closed hat"),
            (43, "Tom 2"),
            (44, "Pedal hat"),
            (45, "Tom 3"),
            (46, "Open hat"),
            (47, "Tom 4"),
            (48, "Tom 5"),
            (49, "Crash"),
            (50, "Tom 6"),
            (51, "Ride"),
            (54, "Tambourine"),
            (56, "Cowbell"),
            (69, "Shaker"),
            (70, "Shaker 2"),
            (75, "Claves"),
        ]
    );
    assert_eq!(names("std:drum-hats"), [(42, "Closed hat"), (44, "Pedal hat"), (46, "Open hat")]);
    for (id, _, _) in INSTRUMENTS {
        if id != "std:drum-kit" && id != "std:drum-hats" {
            assert!(catalog.components[id].editor.notes.is_empty(), "{id} names no notes");
        }
    }
}

// PKG-8, PKG-6: every declared control is found by its slot, whatever its place among the controls
// (a lookup once found only each component's first), a dictionary entry's by
// `<slot>.<key>.<control>`, and a value in the declared unit gets the declared range and log.
#[test]
fn every_std_control_is_found_by_its_slot() {
    let catalog = describe(&[("std".into(), std_dir())]);
    let mut found = 0;
    for d in catalog.components.values() {
        for c in &d.controls {
            assert_eq!(d.control(&c.name), Some(c), "{}: {}", d.id, c.name);
            assert_eq!(d.range_for(&c.name, c.unit), c.range.map(|r| (r, c.log)), "{}: {}", d.id, c.name);
            found += 1;
            for e in c.each.iter().flat_map(|e| &e.controls) {
                let slot = format!("{}.some-key.{}", c.name, e.name);
                assert_eq!(d.control(&slot), Some(e), "{}: {slot}", d.id);
                assert_eq!(d.range_for(&slot, e.unit), e.range.map(|r| (r, e.log)), "{}: {slot}", d.id);
            }
        }
        assert_eq!(d.control("nothing"), None);
    }
    assert!(found > 46, "std declares {found} controls");
    let kick = &catalog.components["std:kick"];
    assert_eq!(kick.controls.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["tune", "decay"]);
    assert_eq!(kick.range_for("decay", Some(Unit::Seconds)), Some(((0.01, 2.0), true)), "the second control's range");
}

/// The effects `m2pkg.toml` listed before step 5, as `[[effect]]` entries: module, name,
/// category, description, and the `args` written after the audio when one is added.
const OLD_EFFECTS: [(&str, &str, &str, &str, &str); 17] = [
    ("std/filter", "lowpass", "Filter", "Darker: removes the highs above the cutoff", "1000"),
    ("std/filter", "highpass", "Filter", "Thinner: removes the lows below the cutoff", "200"),
    ("std/filter", "bandpass", "Filter", "Keeps only a band around the cutoff, like a telephone", "1000"),
    ("std/filter", "notch", "Filter", "Cuts a narrow band out around the cutoff", "1000"),
    ("std/fx", "limit", "Dynamics", "Stops peaks from clipping", "-1"),
    ("std/fx", "drive", "Distortion", "Warm saturation, from gentle grit to crunch", "6"),
    ("std/fx", "delay", "Delay & Reverb", "Echoes that repeat and fade away", "{ time: 0.25, feedback: 0.3, mix: 0.3 }"),
    ("std/dsp", "gain", "Utility", "Volume: louder or quieter, in dB", "0"),
    ("std/dsp", "pan", "Utility", "Places a mono sound left or right", "0"),
    ("std/fx", "reverb", "Delay & Reverb", "Space: the sound of a room or a hall around it", "{ size: 0.6, damping: 0.5, mix: 0.25 }"),
    ("std/fx", "chorus", "Modulation", "Thicker and wider, with a gentle shimmer", "{ rate: 0.8, depth: 0.5, mix: 0.5 }"),
    ("std/fx", "phaser", "Modulation", "A swirling, sweeping whoosh", "{ rate: 0.4, depth: 0.7, feedback: 0.5, mix: 0.5 }"),
    ("std/fx", "tremolo", "Modulation", "Pulses the volume up and down", "{ rate: 5, depth: 0.5 }"),
    (
        "std/fx",
        "compress",
        "Dynamics",
        "Evens out loud and quiet moments so the sound sits steady",
        "{ threshold: -18, ratio: 3, attack: 0.01, release: 0.15, makeup: 4 }",
    ),
    ("std/fx", "eq3", "EQ", "Bass, middle and treble, each up or down in dB", "{ low: 0, mid: 0, high: 0 }"),
    ("std/fx", "bitcrush", "Distortion", "Lo-fi digital crunch, like an old sampler", "{ bits: 8, rate: 12000, mix: 1 }"),
    ("std/fx", "widen", "Utility", "Makes the stereo image wider, or narrower toward mono", "{ width: 1.5 }"),
];

/// What an effect's `insert` writes after the audio, as the effect planner writes it (D11):
/// positionally when its slots lead `positional` in order, else as an options object.
fn insert_text(d: &Descriptor) -> String {
    let written = |v: &Spelled| {
        // std writes every insert in a spelling of scale 1, so the number is the base value.
        assert_eq!(v.value, v.written, "{}: an insert in a scaled spelling", d.id);
        v.written.to_string()
    };
    let slots: Vec<&str> = d.editor.insert.iter().map(|i| i.slot.as_str()).collect();
    if d.positional.iter().map(String::as_str).take(slots.len()).eq(slots.iter().copied()) {
        d.editor.insert.iter().map(|i| written(&i.value)).collect::<Vec<_>>().join(", ")
    } else {
        let props: Vec<String> = d.editor.insert.iter().map(|i| format!("{}: {}", i.slot, written(&i.value))).collect();
        format!("{{ {} }}", props.join(", "))
    }
}

// PKG-2, PKG-7, D11: the descriptors offer exactly the effects the manifest listed, in the same
// categories, with the same words, writing the same text when one is added; and each builds
// from that text, and from no options at all.
#[test]
fn std_effects_are_the_manifests_old_effects() {
    let catalog = describe(&[("std".into(), std_dir())]);
    let now: BTreeSet<(String, String, String, String, String)> = catalog
        .effects()
        .map(|d| {
            let e = &catalog.exports[&d.id][0];
            (e.module.clone(), e.name.clone(), d.editor.category.clone().unwrap(), d.editor.description.clone(), insert_text(d))
        })
        .collect();
    let old: BTreeSet<(String, String, String, String, String)> =
        OLD_EFFECTS.iter().map(|(m, n, c, d, a)| (m.to_string(), n.to_string(), c.to_string(), d.to_string(), a.to_string())).collect();
    assert!(now == old, "offered now but not before: {:?}\nbefore but not now: {:?}", now.difference(&old), old.difference(&now));
    for (module, name, _, _, args) in OLD_EFFECTS {
        for call in [format!("{name}(x, {args})"), format!("{name}(x)")] {
            build(&format!(
                "import {{ kernel, song }} from \"@m2/core\";\nimport {{ {name} }} from \"{module}\";\n\
                 export default song(() => {{\n  const x = kernel(\"std:saw\", {{ freq: 110 }});\n  return {call};\n}});\n"
            ));
        }
    }
}

/// Port defaults declared in `m2pkg.toml`, by `(kernel id, port)`: `"freq: control<hz> = 440"`.
fn port_defaults() -> BTreeMap<(String, String), f64> {
    let manifest = std::fs::read_to_string(std_dir().join("m2pkg.toml")).unwrap();
    let (mut kernel, mut inputs, mut out) = (String::new(), false, BTreeMap::new());
    for line in manifest.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix("name = \"").and_then(|s| s.strip_suffix('"')) {
            kernel = format!("std:{name}");
        }
        if line.starts_with("inputs = [") {
            inputs = true;
        }
        if inputs {
            for spec in line.split('"').skip(1).step_by(2) {
                let Some((port, kind)) = spec.split_once(':') else { continue };
                if let Some((_, default)) = kind.split_once(" = ") {
                    let value = default.split_whitespace().next().unwrap().parse::<f64>().unwrap();
                    out.insert((kernel.clone(), port.trim().to_string()), value);
                }
            }
        }
        if inputs && line.ends_with(']') {
            inputs = false;
        }
    }
    out
}

// PKG-4: a control left out is the default songs heard before step 5, to the bit. A default that
// was a kernel port's is that port's value in `m2pkg.toml`; one that was a `??` fallback in an
// instrument's body is that fallback's expression. Settings keep today's values too.
#[test]
fn std_defaults_are_todays() {
    let song = r#"import { kernel, song } from "@m2/core";
import { add, blend, mtof, range, scale, sum, xrange } from "std/math";
import { choose, lfo, when } from "std/control";
import { adsr, follow, gain, noise, osc, pan, svf, vca } from "std/dsp";
import { bandpass, highpass, lowpass, notch } from "std/filter";
import { bitcrush, chorus, compress, delay, drive, duck, eq3, limit, phaser, reverb, tremolo, widen } from "std/fx";
import { bass, hat, kick, pad, pluck } from "std/instruments";
import { master, mixer } from "std/mix";
export default song(({ lane, time }) => {
  const x = kernel("std:saw", { freq: 110 });
  const notes = lane.notes("notes");
  const l = lfo(time.beats);
  const all = [
    add(x, x), scale(x), blend(x, x, l), sum(x, x), mtof(60), range(l, 1, 2), xrange(l, 1, 2), when(l, x), choose(l, x, x),
    osc.sine(), osc.saw(), osc.square(), osc.tri(), noise(), adsr(l), follow(x),
    svf.lowpass(x), svf.bandpass(x), svf.highpass(x), vca(x), gain(x), pan(x),
    lowpass(x), highpass(x), bandpass(x), notch(x),
    duck(x, l), delay(x), drive(x), limit(x), reverb(x), chorus(x), phaser(x), tremolo(x), compress(x), eq3(x), bitcrush(x), widen(x),
    bass(notes), kick(notes), hat(notes), pluck(notes), pad(notes),
  ];
  return master(mixer({ a: x, b: all[0] }));
});
"#;
    let b = build(song);
    let m = &b.ir.modules[0];
    let ns = nodes(m);
    // Defaults that were `??` fallbacks in instrument bodies, with their old expressions.
    let fallbacks: BTreeMap<(&str, &str), f64> = [
        (("std:bass", "cutoff"), 600.0),
        (("std:bass", "drive"), 4.0),
        (("std:kick", "tune"), 48.0),
        (("std:kick", "decay"), 0.32),
        (("std:hat", "decay"), 45.0 * 0.001),
        (("std:pluck", "brightness"), 5000.0),
        (("std:pad", "cutoff"), 1800.0),
        (("std:master", "ceiling"), -1.0),
    ]
    .into();
    let defaults = port_defaults();
    // What reads each node: `(kernel id, port)`.
    let mut readers: BTreeMap<&str, Vec<(String, String)>> = BTreeMap::new();
    for n in &ns {
        if let Op::Kernel { kernel, inputs, .. } = &n.op {
            for (port, r) in inputs {
                readers.entry(split_ref(r).0).or_default().push((kernel.clone(), port.clone()));
            }
        }
    }
    let mut checked = BTreeSet::new();
    for n in &ns {
        let (Op::Const { value, .. }, Some(o)) = (&n.op, &n.origin) else { continue };
        if !o.inherited() {
            continue;
        }
        let (component, slot) = (component_of(m, n), o.slot().unwrap());
        let want = match fallbacks.get(&(component, slot)) {
            Some(v) => *v,
            None => {
                let ports = readers.get(n.id.as_str()).cloned().unwrap_or_default();
                let found: Vec<f64> = ports.iter().filter_map(|p| defaults.get(p).copied()).collect();
                assert!(!found.is_empty(), "{component}.{slot} = {value} feeds no kernel port with a default: {ports:?}");
                assert!(found.iter().all(|v| *v == found[0]), "{component}.{slot} feeds ports with other defaults: {ports:?}");
                found[0]
            }
        };
        assert_eq!(*value, want, "{component}.{slot}");
        assert_eq!(*value as f32, want as f32, "{component}.{slot}");
        checked.insert(format!("{component}.{slot}"));
    }
    // Every control that has a default was left out above, so each was compared.
    let catalog = describe(&[("std".into(), std_dir())]);
    let today: BTreeSet<&str> = EXPORTS.iter().map(|e| e.0).collect();
    for d in catalog.components.values().filter(|d| d.id != "std:mixer" && today.contains(d.id.as_str())) {
        for c in d.controls.iter().filter(|c| c.default.is_some() && !c.optional) {
            assert!(checked.contains(&format!("{}.{}", d.id, c.name)), "{}.{} was not compared", d.id, c.name);
        }
    }
    // The faders 8b.3 gave every mixer channel and the master: 0 dB, a gain's own default.
    for c in ["std:mixer.channels.a.db", "std:mixer.channels.b.db", "std:master.db"] {
        assert!(checked.contains(c), "{c} was not compared");
    }
    // Settings: voices as before, and the seed and shape the kernels defaulted to, as plain numbers.
    let voices: BTreeMap<&str, u32> = ns
        .iter()
        .filter_map(|n| match &n.op {
            Op::Poly { voices, .. } => m.chain(n.frame).iter().rev().find_map(|f| f.component.as_deref()).map(|c| (c, *voices)),
            _ => None,
        })
        .collect();
    assert_eq!(voices, [("std:bass", 4), ("std:hat", 4), ("std:kick", 2), ("std:pad", 12), ("std:pluck", 12)].into());
    let input = |kernel: &str, port: &str| -> Vec<(f64, bool)> {
        ns.iter()
            .filter_map(|n| match &n.op {
                Op::Kernel { kernel: k, inputs, .. } if k == kernel => inputs.get(port),
                _ => None,
            })
            .filter_map(|r| ns.iter().find(|n| n.id == split_ref(r).0))
            .filter_map(|n| match n.op {
                Op::Const { value, .. } => Some((value, n.origin.is_some())),
                _ => None,
            })
            .collect()
    };
    // Settings are no cells: constants with no origin, which the checker folds as before (D3).
    assert_eq!(input("std:noise", "seed"), [(1.0, false), (7.0, false)], "noise() and hat's noise(7)");
    assert_eq!(input("std:lfo", "shape"), [(0.0, false)]);
}

/// The controls std declares smooth (D8): those whose kernels read them every sample without
/// smoothing them, where a step jumps the output, so it clicks wherever that is heard (a gain, a
/// cutoff). The mixer's fader isn't: `gain` smooths it. Nor is a control whose step only changes
/// a rate, as the phase runs on (an oscillator's `freq`, `mtof`'s pitch, an envelope's times;
/// `adsr` approaches a new sustain at its decay rate), or `lfo`'s `period`, which its kernel
/// divides the song's beats by: a ramp of it would sweep the phase through many cycles.
const SMOOTH: [&str; 39] = [
    "std:scale.by",
    "std:add.a",
    "std:add.b",
    "std:blend.a",
    "std:blend.b",
    "std:blend.m",
    "std:range.lo",
    "std:range.hi",
    "std:xrange.lo",
    "std:xrange.hi",
    "std:lfo.phase",
    "std:square.width",
    "std:when.mask",
    "std:choose.mask",
    "std:svf-lowpass.cutoff",
    "std:svf-lowpass.q",
    "std:svf-bandpass.cutoff",
    "std:svf-bandpass.q",
    "std:svf-highpass.cutoff",
    "std:svf-highpass.q",
    "std:vca.gain",
    "std:pan.position",
    "std:lowpass.cutoff",
    "std:lowpass.q",
    "std:highpass.cutoff",
    "std:highpass.q",
    "std:bandpass.cutoff",
    "std:bandpass.q",
    "std:notch.cutoff",
    "std:notch.q",
    "std:delay.time",
    "std:delay.feedback",
    "std:delay.mix",
    "std:drive.amount",
    "std:bass.cutoff",
    "std:bass.drive",
    "std:pluck.brightness",
    "std:pad.cutoff",
    "std:mixer.channels.pan",
];

// PKG-14, D8: exactly the controls whose kernels read them every sample, unsmoothed, and whose
// step jumps the output are smooth, and the constants made for them say so.
#[test]
fn std_smooth_controls_are_the_per_sample_ones() {
    let catalog = describe(&[("std".into(), std_dir())]);
    let mut smooth = BTreeSet::new();
    for d in catalog.components.values() {
        for c in &d.controls {
            if c.smooth {
                smooth.insert(format!("{}.{}", d.id, c.name));
            }
            for e in c.each.iter().flat_map(|e| &e.controls).filter(|e| e.smooth) {
                smooth.insert(format!("{}.{}.{}", d.id, c.name, e.name));
            }
        }
    }
    let want: BTreeSet<String> = SMOOTH.iter().map(|s| s.to_string()).collect();
    assert!(smooth == want, "smooth but not listed: {:?}\nlisted but not smooth: {:?}", smooth.difference(&want), want.difference(&smooth));
    let b = build(
        r#"import { kernel, song } from "@m2/core";
import { delay, limit } from "std/fx";
export default song(() => limit(delay(kernel("std:saw", { freq: 110 }), { mix: 0.5 })));
"#,
    );
    let m = &b.ir.modules[0];
    let flags: BTreeSet<(String, bool, bool)> =
        nodes(m).iter().filter_map(|n| n.origin.as_ref()?.control.as_ref().map(|c| (c.slot.clone(), c.inherited, c.smooth))).collect();
    let want = [("ceiling", true, false), ("feedback", true, true), ("mix", false, true), ("time", true, true)];
    assert_eq!(flags, want.iter().map(|(s, i, m)| (s.to_string(), *i, *m)).collect());
}

/// A constant of a declared control: `(slot, value, unit, inherited, argument path)`.
type SlotConst = (String, f64, Option<Unit>, bool, Vec<String>);

/// The constants of declared controls, by frame.
fn slots(m: &Module) -> BTreeMap<u32, Vec<SlotConst>> {
    let mut out: BTreeMap<u32, Vec<_>> = BTreeMap::new();
    for n in nodes(m) {
        let (Op::Const { value, unit }, Some(o)) = (&n.op, &n.origin) else { continue };
        if let Some(c) = &o.control {
            out.entry(c.frame).or_default().push((c.slot.clone(), *value, *unit, c.inherited, o.arg.clone()));
        }
    }
    out
}

// PKG-3, PKG-6, PKG-9, PKG-13: the calls songs write keep working on descriptors. Positional
// sugar and options objects give one constant per control per call, written or inherited;
// connected controls make none; an alias is the same component; a dictionary slot takes the
// caller's keys; sidechain inputs are recorded.
#[test]
fn std_calls_keep_their_forms() {
    let song = r#"import { kernel, semis, song } from "@m2/core";
import { follow, osc } from "std/dsp";
import { lowpass } from "std/filter";
import { delay as echo, duck } from "std/fx";
import { mixer } from "std/mix";
export default song(() => {
  const x = kernel("std:saw", { freq: 110 });
  const a = lowpass(x, 1000);
  const b = lowpass(a, 1000, 0.8);
  const c = lowpass(b, { q: 0.8 });
  const d = lowpass(c, undefined, 0.8);
  const e = lowpass(d, 500, { q: x });
  const env = follow(e);
  const f = echo(duck(e, env), { time: 0.5 });
  return mixer({ dry: f, wet: { in: osc.saw(semis(60)), db: -6 }, mid: { in: x, pan: 0.3 } });
});
"#;
    let b = build(song);
    let m = &b.ir.modules[0];
    let s = slots(m);
    let path = |p: &[&str]| p.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let frame = |id: &str, n: usize| -> u32 {
        let found: Vec<usize> = m.frames.iter().enumerate().filter(|(_, f)| f.component.as_deref() == Some(id)).map(|(i, _)| i).collect();
        found[n] as u32
    };
    let hz = Some(Unit::Hz);
    let scalar = Some(Unit::Scalar);
    let lp = |n: usize| s.get(&frame("std:lowpass", n)).cloned().unwrap_or_default();
    assert_eq!(lp(0), [("cutoff".into(), 1000.0, hz, false, path(&["1"])), ("q".into(), 0.707, scalar, true, vec![])]);
    assert_eq!(lp(1), [("cutoff".into(), 1000.0, hz, false, path(&["1"])), ("q".into(), 0.8, scalar, false, path(&["2"]))]);
    assert_eq!(lp(2), [("cutoff".into(), 1000.0, hz, true, vec![]), ("q".into(), 0.8, scalar, false, path(&["1", "q"]))]);
    assert_eq!(lp(3), [("cutoff".into(), 1000.0, hz, true, vec![]), ("q".into(), 0.8, scalar, false, path(&["2"]))]);
    // Connected: no constant for `q`.
    assert_eq!(lp(4), [("cutoff".into(), 500.0, hz, false, path(&["1"]))]);
    // `echo` is `delay` (PKG-9): its frame names std:delay, and its other controls are inherited.
    let delay = &s[&frame("std:delay", 0)];
    assert_eq!(delay.iter().map(|c| (c.0.as_str(), c.3)).collect::<Vec<_>>(), [("time", false), ("feedback", true), ("mix", true)]);
    // A pitch stays a pitch: the oscillator converts it, and its knob is in semitones (D4).
    assert_eq!(s[&frame("std:saw", 0)], [("freq".into(), 60.0, Some(Unit::Semitones), false, path(&["0"]))]);
    assert!(m.frames.iter().any(|f| f.component.as_deref() == Some("std:mtof") && f.parent == Some(frame("std:saw", 0))));
    // The mixer: each entry's controls by the caller's key. Every entry has its `db`, inherited
    // when left out (8b.3); a left-out `pan` makes nothing (PKG-5, PKG-6).
    let mix = &s[&frame("std:mixer", 0)];
    assert_eq!(
        mix.iter().map(|c| (c.0.as_str(), c.1, c.3)).collect::<Vec<_>>(),
        [
            ("channels.dry.db", 0.0, true),
            ("channels.wet.db", -6.0, false),
            ("channels.mid.db", 0.0, true),
            ("channels.mid.pan", 0.3, false)
        ]
    );
    // Sidechain inputs record what they received (PKG-13).
    let sidechain = |id: &str| m.frames[frame(id, 0) as usize].sidechain.iter().map(|x| x.input.clone()).collect::<Vec<_>>();
    assert_eq!((sidechain("std:follow"), sidechain("std:duck")), (vec!["in".to_string()], vec!["side".to_string()]));
    // PKG-3: never two constants for one control of one call.
    for (f, cs) in &s {
        let names: BTreeSet<&str> = cs.iter().map(|c| c.0.as_str()).collect();
        assert_eq!(names.len(), cs.len(), "frame {f}: {cs:?}");
    }
}

/// The engines' own knobs and each drum's: left out, each is its kernel port's default, so an
/// engine and its kernel agree on what a bare note sounds like. The other instruments are
/// voiced: their defaults are their patches'.
const ENGINES: [&str; 13] = [
    "std:synth-custom",
    "std:fm-custom",
    "std:drum-kit",
    "std:drum-kick",
    "std:drum-snare",
    "std:drum-clap",
    "std:drum-hats",
    "std:drum-tom",
    "std:drum-rim",
    "std:drum-cowbell",
    "std:drum-cymbal",
    "std:drum-shaker",
    "std:sampler",
];

// PKG-3, PKG-4: every instrument builds bare, and every knob a call leaves out reaches a kernel,
// or a glide reaches the poly that plays one note at a time: none is left over (except the
// polysynth's `glide`, which only a voice at a time plays). The engines' knobs default to their
// kernels' ports, to the f32. mono() hands its voice a pitch in semitones, so an oscillator
// converts it.
#[test]
fn std_instruments_reach_their_kernels() {
    let calls: Vec<String> = INSTRUMENTS.iter().map(|(_, _, name)| format!("{name}(notes)")).collect();
    let song = format!(
        "import {{ song }} from \"@m2/core\";\nimport {{ drums }} from \"std/drums\";\nimport {{ osc }} from \"std/dsp\";\n\
         import {{ fm }} from \"std/fm\";\nimport {{ mixer }} from \"std/mix\";\nimport {{ mono, synth }} from \"std/synth\";\n\
         import {{ sampler }} from \"std/sampler\";\n\
         export default song(({{ lane }}) => {{\n  const notes = lane.notes(\"notes\");\n\
         const all = [{}, mono(notes, (n) => osc.saw(n.pitch))];\n  return mixer({{ a: all[0] }});\n}});\n",
        calls.join(", ")
    );
    let b = build(&song);
    let m = &b.ir.modules[0];
    let ns = nodes(m);
    let mut readers: BTreeMap<&str, Vec<(String, String)>> = BTreeMap::new();
    for n in &ns {
        if let Op::Kernel { kernel, inputs, .. } = &n.op {
            for (port, r) in inputs {
                readers.entry(split_ref(r).0).or_default().push((kernel.clone(), port.clone()));
            }
        }
    }
    let glides: BTreeSet<&str> =
        ns.iter().filter_map(|n| if let Op::Poly { glide: Some(g), .. } = &n.op { Some(split_ref(g).0) } else { None }).collect();
    let defaults = port_defaults();
    let added: BTreeSet<&str> = INSTRUMENTS.iter().map(|e| e.0).collect();
    let mut checked = BTreeSet::new();
    for n in &ns {
        let (Op::Const { value, .. }, Some(o)) = (&n.op, &n.origin) else { continue };
        let (component, slot) = (component_of(m, n), o.slot().unwrap());
        if !o.inherited() || !added.contains(component) {
            continue;
        }
        checked.insert(format!("{component}.{slot}"));
        let ports = readers.get(n.id.as_str()).cloned().unwrap_or_default();
        if (component, slot) == ("std:synth-custom", "glide") {
            assert!(ports.is_empty() && !glides.contains(n.id.as_str()), "with a voice per note there is nothing to glide: {ports:?}");
            continue;
        }
        if slot == "glide" {
            assert!(ports.is_empty() && glides.contains(n.id.as_str()), "{component}.glide reaches its poly");
            continue;
        }
        assert!(!ports.is_empty(), "{component}.{slot} reaches no kernel");
        if ENGINES.contains(&component) {
            for (kernel, port) in &ports {
                let want = defaults.get(&(kernel.clone(), port.clone())).unwrap_or_else(|| panic!("{kernel}.{port} has no default"));
                assert_eq!(*value as f32, *want as f32, "{component}.{slot} is {value}, and {kernel}.{port} defaults to {want}");
            }
        }
    }
    let catalog = describe(&[("std".into(), std_dir())]);
    for (id, _, _) in INSTRUMENTS {
        for c in &catalog.components[id].controls {
            assert!(checked.contains(&format!("{id}.{}", c.name)), "{id}.{} was not checked", c.name);
        }
    }
    let pitch_field = |r: &str| ns.iter().any(|n| n.id == split_ref(r).0 && matches!(&n.op, Op::Lane { lane } if lane == "pitch"));
    let converted = ns.iter().any(|n| match &n.op {
        Op::Kernel { kernel, inputs, .. } if kernel == "std:mtof" => inputs.get("pitch").is_some_and(|r| pitch_field(r)),
        _ => false,
    });
    assert!(converted, "mono()'s pitch reaches the saw through mtof");
}

// RENDER-16, track pm: std's one-note sounds are one poly in "legato" mode, which slides over the
// glide and doesn't retrigger, not two polys of `std:notecv` into `std:mono`. Its glide is the
// instrument's knob, read by the poly. With more voices, a poly as before.
#[test]
fn std_one_note_sounds_are_one_poly() {
    let b = build(
        "import { song } from \"@m2/core\";\nimport { fm } from \"std/fm\";\nimport { mixer } from \"std/mix\";\n\
         import { synth } from \"std/synth\";\n\
         export default song(({ lane }) => mixer({ a: synth.lead(lane.notes(\"a\")), b: fm.keys(lane.notes(\"b\"), { voices: 1 }), \
         c: synth.pad(lane.notes(\"c\")) }));\n",
    );
    let m = &b.ir.modules[0];
    let ns = nodes(m);
    let kernels: BTreeSet<&str> =
        ns.iter().filter_map(|n| if let Op::Kernel { kernel, .. } = &n.op { Some(kernel.as_str()) } else { None }).collect();
    assert!(!kernels.contains("std:mono") && !kernels.contains("std:notecv"), "{kernels:?}");
    let polys: Vec<(&str, u32, m2_ir::Alloc, Option<&Node>)> = ns
        .iter()
        .filter_map(|n| match &n.op {
            Op::Poly { voices, alloc, glide, body, .. } => {
                let engine =
                    body.nodes.iter().find_map(|b| if let Op::Kernel { kernel, .. } = &b.op { Some(kernel.as_str()) } else { None });
                let glide = glide.as_ref().map(|g| *ns.iter().find(|x| x.id == split_ref(g).0).expect("the glide's node"));
                Some((engine.unwrap_or(""), *voices, *alloc, glide))
            }
            _ => None,
        })
        .collect();
    let legato = m2_ir::Alloc { mode: m2_ir::Mode::Legato, retrigger: false, steal: m2_ir::Steal::Oldest };
    assert_eq!(polys.len(), 3, "one poly each");
    let (_, voices, alloc, glide) = polys[0];
    assert_eq!((polys[0].0, voices, alloc), ("std:synth", 1, legato), "the lead");
    let glide = glide.expect("the lead's glide");
    assert!(matches!(glide.op, Op::Const { value, .. } if (value - 0.06).abs() < 1e-12), "{glide:?}");
    assert_eq!(glide.origin.as_ref().and_then(|o| o.slot()), Some("glide"), "the lead's glide knob");
    assert_eq!((polys[1].0, polys[1].1, polys[1].2, polys[1].3.is_none()), ("std:fm", 1, legato, true), "FM keys, one voice: no glide");
    assert_eq!((polys[2].0, polys[2].1, polys[2].2, polys[2].3.is_none()), ("std:synth", 16, m2_ir::Alloc::default(), true), "the pad");
}
