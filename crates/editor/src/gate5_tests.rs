//! The step 5 gates, as a running session sees them: every declared control is a knob with its
//! declared range, written or inherited; every value names its home; a broken package never stops
//! a project from opening; and the deleted UI vocabulary is gone from core.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use m2_ir::{Node, Op, Origin, Unit};
use m2_script::Descriptor;
use m2_script::sites::{Arg, Site};

use m2_session::{Home, Session};

use crate::testing::{Fixture, files_under, fixture};
use crate::{Gestures, Knob};

/// The five fixtures (FIXTURES in the contract's words).
const FIXTURES: [&str; 5] = ["examples/first-song", "examples/resample", "examples/with-plugins", "templates/starter", "templates/empty"];

/// A copy of `every-component`: a song that calls each std component once, with some controls
/// written, some connected and some left out.
pub(crate) fn every_component() -> Fixture {
    let from = crate::testing::fixtures().join("every-component");
    let f = Fixture::new();
    for file in files_under(&from) {
        f.write(file.strip_prefix(&from).unwrap().to_str().unwrap(), std::fs::read(&file).unwrap());
    }
    f
}

/// Every node of `nodes`, voices included.
fn walk<'a>(nodes: &'a [Node], f: &mut impl FnMut(&'a Node)) {
    for n in nodes {
        f(n);
        if let Op::Poly { body, .. } = &n.op {
            walk(&body.nodes, f);
        }
    }
}

/// Whether `slot` of `d`, a call of which made no constant for it, is given a signal in the call
/// at `site`: written at its positional place, or named in an options object.
fn connected(site: &Site, d: &Descriptor, slot: &str) -> bool {
    let mut params: Vec<&str> = d.inputs.iter().filter(|i| !d.positional.contains(&i.name)).map(|i| i.name.as_str()).collect();
    params.extend(d.positional.iter().map(String::as_str));
    let positional = params.iter().position(|p| *p == slot).is_some_and(|p| p < site.args.len());
    positional || site.args.iter().any(|a| matches!(a, Arg::Object { props, .. } if props.iter().any(|(k, _)| k == slot)))
}

// The step 5 gate, PKG-3, PKG-8: every declared control of every component call in the project,
// written or inherited, is exactly one knob on that call, with its slot, whether it is written, and
// the range and log scale its descriptor declares for its unit (the value inside it); and it is
// exactly one constant per instance and slot. A control with no constant is connected to a signal,
// or optional and left out (PKG-5). On the five fixtures and on a song that calls every std
// component.
#[test]
fn every_declared_control_is_a_knob() {
    let mut called = BTreeSet::new();
    for (name, f) in FIXTURES.iter().map(|n| (*n, fixture(n))).chain([("every-component", every_component())]) {
        let s = f.open();
        let rev = s.accepted();
        let view = s.view();
        let controls = s.controls();
        let sites: BTreeMap<u32, &Site> = rev.compiled.sites.iter().map(|x| (x.id, x)).collect();
        for m in &rev.compiled.ir.modules {
            let mv = view.modules.iter().find(|v| v.key == m.key).unwrap();
            let knobs: Vec<(&str, &Knob)> = mv.nodes.iter().flat_map(|n| n.knobs.iter().map(move |k| (n.id.as_str(), k))).collect();
            let mut consts: BTreeMap<(u32, String), Vec<&Origin>> = BTreeMap::new();
            walk(&m.nodes, &mut |n| {
                if let (Op::Const { .. }, Some(o)) = (&n.op, &n.origin)
                    && let Some(c) = &o.control
                {
                    consts.entry((c.frame, c.slot.clone())).or_default().push(o);
                }
            });
            for ((frame, slot), list) in &consts {
                assert_eq!(list.len(), 1, "{name}: PKG-3, one constant for frame {frame}'s {slot}");
            }
            for (i, frame) in m.frames.iter().enumerate() {
                let (Some(cid), Some(site)) = (&frame.component, sites.get(&frame.site)) else { continue };
                if !site.in_project {
                    continue;
                }
                called.insert(cid.clone());
                let d = &rev.compiled.components[cid];
                for ((_, slot), list) in consts.range((i as u32, String::new())..(i as u32 + 1, String::new())) {
                    let o = list[0];
                    let at = format!("{name}: {}:{} {cid} {slot}", site.file, site.line);
                    let found: Vec<&(&str, &Knob)> = knobs.iter().filter(|(_, k)| k.origin.same_value(o)).collect();
                    let [(node, k)] = found.as_slice() else { panic!("{at}: {} knobs", found.len()) };
                    assert_eq!(k.written, !o.arg.is_empty(), "{at}");
                    assert_eq!(k.home, Home::Code, "{at}");
                    let c = controls.iter().find(|c| c.id == k.id).expect("a knob is a control");
                    if c.declared.len() == 1 {
                        assert_eq!((k.slot.as_deref(), k.component.as_deref()), (Some(slot.as_str()), Some(cid.as_str())), "{at}");
                        let declared = d.range_for(slot, c.unit);
                        assert_eq!(
                            (k.range, k.log),
                            (declared.map(|((lo, hi), _)| [lo, hi]), declared.is_some_and(|(_, log)| log)),
                            "{at}"
                        );
                    }
                    if let Some([lo, hi]) = k.range {
                        let v = k.value.as_f64().unwrap_or_else(|| panic!("{at}: {} isn't a number", k.value));
                        assert!(lo <= v && v <= hi, "{at}: {v} is outside {lo}..{hi}");
                    }
                    if !k.written {
                        assert_eq!(*node, format!("{}#{}", site.file, site.ordinal), "{at}: on its call");
                    }
                }
                for c in d.controls.iter().filter(|c| c.each.is_none()) {
                    if !consts.contains_key(&(i as u32, c.name.clone())) {
                        assert!(c.optional || connected(site, d, &c.name), "{name}: {cid}'s {} has no knob and isn't connected", c.name);
                    }
                }
            }
        }
    }
    // The song does call every component std declares.
    let s = every_component().open();
    let missing: Vec<&String> = s.catalog().components.keys().filter(|c| !called.contains(*c)).collect();
    assert!(missing.is_empty(), "every-component calls none of {missing:?}");
}

// D4: a knob's unit is its constant's. `osc.saw(semis(60))` is a semitones knob, with the range its
// frequency accepts semitones in; a `frac` given to a gain is a unit01 value that no range is
// declared for, so the UI picks one.
#[test]
fn knob_unit_is_the_constants() {
    let f = Fixture::new();
    f.write("music2.toml", crate::testing::MANIFEST);
    f.write(
        "song.ts",
        "import { frac, semis, song } from \"@m2/core\";\nimport { osc, vca } from \"std/dsp\";\n\
         export default song(() => vca(osc.saw(semis(60)), frac(0.5)));\n",
    );
    let s = f.open();
    let view = s.view();
    let knobs: Vec<&Knob> = view.modules[0].nodes.iter().flat_map(|n| &n.knobs).collect();
    let knob = |label: &str| knobs.iter().find(|k| k.label == label).unwrap_or_else(|| panic!("no {label} in {knobs:?}"));
    assert_eq!((knob("freq").unit, knob("freq").range, knob("freq").log), (Some(Unit::Semitones.name()), Some([0.0, 127.0]), false));
    assert_eq!((knob("gain").unit, knob("gain").range, knob("gain").default.clone()), (Some(Unit::Unit01.name()), None, None));
}

// D4, D10: one literal reaching two slots (`const g = frac(0.3)` given to a delay's feedback and
// mix) is one control with both declared, and one knob; the slots declare different ranges, so
// the knob has none of its own.
#[test]
fn a_shared_literal_is_one_control() {
    let f = Fixture::new();
    f.write("music2.toml", crate::testing::MANIFEST);
    f.write(
        "song.ts",
        "import { frac, song } from \"@m2/core\";\nimport { osc } from \"std/dsp\";\nimport { delay } from \"@m2/effects\";\n\
         export default song(() => {\n  const g = frac(0.3);\n  return delay(osc.saw(110), { feedback: g, mix: g });\n});\n",
    );
    let s = f.open();
    let shared: Vec<_> = s.controls().into_iter().filter(|c| c.literal == "0.3").collect();
    assert_eq!(shared.len(), 1, "{shared:?}");
    let slots: Vec<&str> = shared[0].declared.iter().map(|d| d.slot.as_str()).collect();
    assert_eq!((shared[0].uses, slots), (2, vec!["feedback", "mix"]));
    let view = s.view();
    let knobs: Vec<&Knob> = view.modules[0].nodes.iter().flat_map(|n| &n.knobs).filter(|k| k.id == shared[0].id).collect();
    assert_eq!(knobs.len(), 1);
    assert_eq!((knobs[0].slot.as_deref(), knobs[0].range), (None, None), "the two slots declare other ranges");
}

// HOME-1 (a CI gate from step 5): every value `controls()` and the view return names its home, a
// card's settings included (CARD-1). The view is checked as it is sent, so a value without the
// field fails; so is the timeline, whose tempo and time signatures live in `data/arrangement.json`
// (8b.2).
#[test]
fn every_value_names_its_home() {
    for (name, f) in FIXTURES.iter().map(|n| (*n, fixture(n))).chain([("every-component", every_component())]) {
        let s = f.open();
        assert!(s.controls().iter().all(|c| c.home == Home::Code), "{name}");
        let timeline = serde_json::to_value(s.timeline()).unwrap();
        assert_eq!((&timeline["home"], timeline["tempo"].as_array().map(Vec::len)), (&serde_json::json!("arrangement"), Some(1)), "{name}");
        let view = serde_json::to_value(s.view()).unwrap();
        let mut values = 0;
        for n in view["modules"].as_array().unwrap().iter().flat_map(|m| m["nodes"].as_array().unwrap()) {
            // A card's settings too (CARD-1), sent only when it has some.
            let settings = n["settings"].as_array().into_iter().flatten();
            for v in n["knobs"].as_array().unwrap().iter().chain(n["readonly"].as_array().unwrap()).chain(settings) {
                assert_eq!(v["home"], "code", "{name}: {v}");
                values += 1;
            }
        }
        assert!(name == "templates/empty" || values > 0, "{name}: no values");
    }
}

/// A copy of std whose sources `edit` changes.
struct StdCopy(Fixture);

impl StdCopy {
    fn new(edit: impl FnOnce(&Path)) -> StdCopy {
        let std = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std");
        let f = Fixture::new();
        for file in ["m2pkg.toml", "kernels/std.wasm"].iter().map(|p| std.join(p)).chain(files_under(&std.join("src"))) {
            f.write(file.strip_prefix(&std).unwrap().to_str().unwrap(), std::fs::read(&file).unwrap());
        }
        edit(&f.0);
        StdCopy(f)
    }

    fn packages(&self) -> m2_runtime::PackageSet {
        let mut p = m2_runtime::PackageSet::default().with_store(m2_song::pack::test_store());
        p.load(&self.0.0).unwrap();
        p
    }
}

// WRITE-20, PKG-9, DIAG-2: a package module that throws while it loads, and one component id
// declared in two files, never stop a project from opening. Each is `pkg.invalid` among the
// session's problems, naming the files; only those components leave the catalog; and a song that
// doesn't use them plays.
#[test]
fn a_broken_package_module_still_opens() {
    let std = StdCopy::new(|dir| {
        let control = std::fs::read_to_string(dir.join("src/control.ts")).unwrap();
        std::fs::write(dir.join("src/control.ts"), format!("{control}\nthrow new Error(\"broken on purpose\");\n")).unwrap();
        // filter.ts and fx.ts import neither the other, nor does any module import both.
        let filter = std::fs::read_to_string(dir.join("src/filter.ts")).unwrap();
        let another =
            "\nexport const echo = component({ id: \"limit\", inputs: { in: port.audio() }, output: port.audio() }, (x: Audio) => x);\n";
        std::fs::write(dir.join("src/filter.ts"), format!("{filter}{another}")).unwrap();
    });
    let f = Fixture::new();
    f.write("music2.toml", crate::testing::MANIFEST);
    f.write(
        "song.ts",
        "import { song, track } from \"@m2/core\";\nimport { drums } from \"@m2/drums\";\n\
         const k = track(\"kick\", ({ lane }) => drums.drum(lane.notes(\"hits\")));\nexport default song(() => k.out);\n",
    );
    let s = Session::open(&f.0, std.packages()).unwrap_or_else(|d| panic!("the project must open: {d:?}"));
    let invalid: Vec<String> = s.problems().iter().filter(|d| d.code == "pkg.invalid").map(|d| d.to_string()).collect();
    assert_eq!(invalid.len(), 2, "{invalid:?}");
    assert!(invalid.iter().any(|m| m.contains("std/control") && m.contains("broken on purpose")), "{invalid:?}");
    assert!(invalid.iter().any(|m| m.contains("std/src/fx.ts") && m.contains("std/src/filter.ts")), "both places: {invalid:?}");
    let catalog = s.catalog();
    assert!(!catalog.components.contains_key("std:lfo") && !catalog.components.contains_key("std:limit"));
    assert!(catalog.components.contains_key("@m2/synths:pluck") && catalog.components.contains_key("std:compress"));
    assert!(s.effects().iter().all(|e| e.component != "std:limit"));
    let rev = s.accepted();
    let r = m2_runtime::Renderer::new(&rev.compiled.flat, &rev.compiled.data, &s.packages, 48_000).unwrap().render(0.1).unwrap();
    assert_eq!(r.len(), 4800, "the song plays");
}

// The step 5 gate, P10, PKG-2, PKG-7, PKG-11, PKG-8: the four deleted items are gone. std declares
// no effect in TOML, core holds no list of categories, the cable planner writes units from the
// build's table, and the view guesses no range from a unit.
#[test]
fn the_deleted_items_are_gone() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let read = |p: &str| std::fs::read_to_string(root.join(p)).unwrap();
    assert!(!read("std/m2pkg.toml").contains("[[effect]]"), "std/m2pkg.toml still declares effects");
    let core: Vec<_> = files_under(&root.join("core")).into_iter().filter(|p| p.extension().is_some_and(|e| e == "rs")).collect();
    // Spelled in two halves, so this file doesn't name it.
    let categories = ["EFFECT", "_CATEGORIES"].concat();
    for file in &core {
        assert!(!std::fs::read_to_string(file).unwrap().contains(&categories), "{} keeps a list of effect categories", file.display());
    }
    assert!(!read("crates/editor/src/cable.rs").contains(&["Unit::Hz", " =>"].concat()), "cable.rs keeps a unit table");
    assert!(!read("crates/editor/src/view.rs").contains(&["unit", "_range"].concat()), "view.rs guesses ranges from units");
}

/// Call `call` of `source` with `edits` (positions in `source`, in order) made in it.
fn edited(source: &str, call: &Site, edits: &[(u32, u32, String)]) -> String {
    let (mut out, mut at) = (String::new(), call.start as usize);
    for (start, end, text) in edits {
        out += &source[at..*start as usize];
        out += text;
        at = *end as usize;
    }
    out + &source[at..call.end as usize]
}

/// Every-component with its mixer's `fx` channel written as `db(-4)`, which the file imports.
const SPELLED: [(&str, &str); 2] =
    [("import { semis, song } from \"@m2/core\";", "import { db, semis, song } from \"@m2/core\";"), ("db: -4 }", "db: db(-4) }")];

// Critique 1 of the step 5 design, D5, D11, PKG-1: every call the planners write type-checks as
// the code editor checks songs (strict, against the SDK and std sources). On every-component,
// each control a call leaves out is written in (a number as a value edit writes it, a choice's name
// or a switch's true or false, and a number's signal by name as `connect` does), each effect is added, and each instrument + Track lists is added as a
// new track's `track(…)` call (8b.1); each mixer channel with no pan gets its first pan, and each
// that leaves out its fader gets one in the style of a mixer whose other entries write `db(…)`
// (8b.3b), and each kit's tracks are added with their values (KIT-5); every call so written is
// listed in tests/forms/forms.ts, which must be as recorded
// here. `tsc -p crates/editor/tests/forms` type-checks it with web's tsc, in web's
// `npm run typecheck` and so in `npm run build`.
#[test]
fn planned_call_forms_are_current() {
    let f = every_component();
    let s = f.open();
    let source = f.read("song.ts");
    let sites = &s.accepted().compiled.sites;
    let site = |id: u32| sites.iter().find(|x| x.id == id).unwrap();
    // The signal a cable writes: declared before every call it can reach.
    let wobble = s.outlets().into_iter().find(|o| o.name == "wobble").expect("every-component names `wobble`");
    let wobble_end = site(wobble.site).end;
    let mut forms: Vec<String> = vec![];
    let mut inherited: Vec<_> = s.controls().into_iter().filter(|c| !c.written).collect();
    inherited.sort_by_key(|c| (site(c.origin.site).start, c.declared.first().map(|d| d.slot.clone())));
    for c in &inherited {
        let call = site(c.origin.site);
        let slot = c.origin.slot().unwrap();
        // A choice's value is written as its name and a switch's as true or false (CARD-21); no
        // cable of a number reaches either (CARD-12).
        let steps = c.unit.filter(|u| !u.is_number());
        let value = match steps {
            Some(u) => crate::kind::said(Some(u), c.value).expect("one of its places").to_string(),
            None => c.number(&if c.value == 0.0 { "0".to_string() } else { c.value.to_string() }),
        };
        for (what, written) in [("a value", value.as_str()), ("a cable", "wobble")] {
            if steps.is_some() && written == "wobble" {
                continue;
            }
            let Some(edits) = c.planned(written) else { continue };
            if written == "wobble" && edits[0].0 < wobble_end {
                continue;
            }
            let edited = edited(&source, call, &edits);
            forms.push(format!("    // song.ts:{} `{}` leaves out {slot}: {what} written in\n    {edited},\n", call.line, call.callee));
        }
    }
    // A channel's first pan (8b.3b).
    let crate::mix::Mixing::One(m) = crate::mix::mixer(s.accepted()) else { panic!("every-component has one mixer") };
    let call = site(m.site);
    for c in &m.channels {
        let Ok(i) = crate::mix::planned(s.accepted(), &m, &c.key, "pan") else { continue };
        let edited = edited(&source, call, &i.edits(&i.number("0")));
        forms.push(format!("    // song.ts:{} `mixer`: the first pan of channel {}\n    {edited},\n", call.line, c.key));
    }
    // A fader written into a mixer whose other entries write `db(…)`.
    let spelled = SPELLED.iter().fold(source.clone(), |t, (from, to)| t.replacen(from, to, 1));
    f.write("song.ts", &spelled);
    let s2 = f.open();
    let sites2 = &s2.accepted().compiled.sites;
    for c in s2.controls().into_iter().filter(|c| !c.written && c.origin.slot().is_some_and(|x| x.starts_with("channels."))) {
        let call = sites2.iter().find(|x| x.id == c.origin.site).unwrap();
        let edits = c.planned(&c.number("0")).expect("insertable");
        let edited = edited(&spelled, call, &edits);
        assert!(edited.contains("db: db(0)"), "{edited}");
        forms.push(format!("    // song.ts:{} `mixer` writes db(…): {} written in\n    {edited},\n", call.line, c.origin.slot().unwrap()));
    }
    for e in s.effects() {
        forms.push(format!("    // {} added after a sound\n    {}(osc.saw(semis(60)){}),\n", e.component, e.name, e.insert));
    }
    // An installed plugin added after a sound (gaps #32), as the probe.
    let (open, close) = crate::plugins::effect_text("clap:m2.probe", "m2-probe");
    forms.push(format!("    // clap:m2.probe added after a sound\n    {open}osc.saw(semis(60)){close},\n"));
    let (head, rest) = source.split_once("export default song(({ lane, time }) => {\n").expect("every-component's song");
    // A new track's file (8b.1): its `track(…)` call, for every instrument + Track lists, under the
    // key and name it proposes. Every-component imports what each plays (`synth` for `synth.pad`).
    let imported: BTreeSet<&str> = head
        .lines()
        .filter_map(|l| l.strip_prefix("import {")?.split_once('}'))
        .flat_map(|(names, _)| names.split(',').map(str::trim))
        .collect();
    for i in s.instruments() {
        let binding = crate::tracks::instrument_binding(&i);
        assert!(imported.contains(binding), "every-component doesn't import {binding}, which a track playing {} imports", i.component);
        let key = crate::keys::propose(&i.label, |_| false);
        forms.push(format!("    // {} added as a track\n    {},\n", i.component, crate::tracks::track_call_text(&i, &key, &i.label, "")));
    }
    // Each kit's tracks (KIT-5), each call with the kit's values, under the key and name it proposes.
    for k in crate::drums::kits(&s) {
        let i = s.instruments().into_iter().find(|i| i.component == k.component).expect("the kit's instrument");
        let d = &s.catalog().components[&k.component];
        for t in &d.editor.kits.iter().find(|x| x.name == k.name).expect("the kit").tracks {
            let (values, ctors) = crate::tracks::values_text(d, &t.values, &s.accepted().compiled.units).unwrap();
            assert!(ctors.is_empty(), "every-component imports no unit for {}'s {}", k.name, t.name);
            let key = crate::keys::propose(&t.name, |_| false);
            forms.push(format!(
                "    // {}'s {} added as a track\n    {},\n",
                k.name,
                t.name,
                crate::tracks::track_call_text(&i, &key, &t.name, &values)
            ));
        }
    }
    // A kit's Drums bus, its gain at -6 dB (KIT-5).
    let gain = &s.catalog().components["std:gain"];
    let (values, _) = crate::tracks::values_text(gain, &crate::drums::kit_bus().values, &s.accepted().compiled.units).unwrap();
    forms.push(format!(
        "    // a kit's Drums bus added\n    {},\n",
        crate::bus::bus_call_text(crate::bus::BusKind::Group, "drums", "Drums", &values)
    ));
    // An installed instrument plugin added as a track (gaps #32), as the probe.
    forms.push(format!(
        "    // clap:m2.probe added as a track\n    {},\n",
        crate::plugins::track_call_text("clap:m2.probe", "m2-probe", "m2 probe")
    ));
    // An audio track (studio lane A's Q8).
    forms.push(format!("    // an audio track added\n    {},\n", crate::tracks::audio_call_text("audio", "Audio")));
    // A bus of each kind the Add bus menu makes (RENDER-34).
    for kind in crate::bus::BusKind::ALL {
        let name = crate::bus::bus_import(kind);
        assert!(imported.contains(name), "every-component doesn't import {name}, which a {kind:?} bus imports");
        forms.push(format!("    // a {kind:?} bus added\n    {},\n", crate::bus::bus_call_text(kind, "bus", &format!("{kind:?}"), "")));
    }
    let (body, ret) = rest.split_once("  return ").expect("its return");
    let text = format!(
        "// Generated by `planned_call_forms_are_current` (crates/editor/src/gate5_tests.rs) from\n\
         // core/session/tests/fixtures/every-component/song.ts: every call the editor's planners write\n\
         // there, which `tsc -p crates/editor/tests/forms` checks as the code editor checks songs.\n{}\
         import {{ bus, plugin, track }} from \"@m2/core\";\n\
         export default song(({{ lane, time }}) => {{\n{body}  const planned = [\n{}  ];\n  void planned;\n  return {ret}",
        // The spelled song's imports: every-component's, and `db`.
        spelled.lines().filter(|l| l.starts_with("import")).map(|l| format!("{l}\n")).collect::<String>(),
        forms.concat(),
    );
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/forms/forms.ts");
    if std::env::var_os("M2_RECORD_FORMS").is_some() {
        std::fs::write(&path, &text).unwrap();
    }
    let recorded = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        recorded == text,
        "the call forms the planners write changed: record them (M2_RECORD_FORMS=1 cargo test -p m2-editor \
         planned_call_forms), check them with `web/node_modules/.bin/tsc -p crates/editor/tests/forms`, and commit both:\n{text}"
    );
}
