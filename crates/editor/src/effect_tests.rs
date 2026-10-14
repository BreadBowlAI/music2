//! Effects wrap a call where it is written, and unwrap it again, verified against the graph.

use std::path::{Path, PathBuf};

use super::*;
use crate::Gestures;
use m2_runtime::PackageSet;

struct Fixture(PathBuf);

impl Fixture {
    fn new(source: &str) -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("m2-effect-{}-{n}", std::process::id()));
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

    fn source(&self) -> String {
        std::fs::read_to_string(self.0.join("song.ts")).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const HEAD: &str = "import { hz, song } from \"@m2/core\";\nimport { osc } from \"std/dsp\";\n";

/// The id of the call to `callee` in the accepted revision.
fn call(s: &Session, callee: &str) -> String {
    let site =
        s.accepted().compiled.sites.iter().find(|x| x.in_project && x.callee == callee).unwrap_or_else(|| panic!("no call `{callee}`"));
    format!("{}#{}", site.file, site.ordinal)
}

fn rev(s: &Session) -> u64 {
    s.accepted().number
}

#[test]
fn adding_wraps_the_call_and_imports_the_effect_and_removing_restores_it_exactly() {
    let original = format!("{HEAD}export default song(() => osc.saw(hz(110)));\n");
    let fixture = Fixture::new(&original);
    let mut s = fixture.open();
    let graph = canonical(&s.accepted().compiled.ir);

    let added = s.add_effect(rev(&s), &call(&s, "osc.saw"), "std:delay").unwrap();
    let delayed = "delay(osc.saw(hz(110)), { time: 0.25, feedback: 0.3, mix: 0.3 })";
    assert_eq!((added.before.as_str(), added.after.as_str()), ("osc.saw(hz(110))", delayed));
    assert_eq!(
        fixture.source(),
        original
            .replace("from \"std/dsp\";\n", "from \"std/dsp\";\nimport { delay } from \"std/fx\";\n")
            .replace("song(() => osc.saw(hz(110)))", &format!("song(() => {delayed})"))
    );
    // Its settings are knobs.
    let mut knobs: Vec<String> =
        s.controls().iter().filter(|c| c.literal != "110").map(|c| c.id.rsplit('/').next().unwrap().to_string()).collect();
    knobs.sort();
    assert_eq!(knobs, ["1.feedback", "1.mix", "1.time"]);
    // The delay is a new node whose input is the saw.
    let readers: Vec<String> = s.accepted().compiled.ir.modules[0]
        .nodes
        .iter()
        .filter_map(|n| match &n.op {
            Op::Kernel { kernel, .. } => Some(kernel.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(readers, ["std:saw", "std:delay"]);

    let removed = s.remove_effect(rev(&s), &call(&s, "delay")).unwrap();
    assert_eq!((removed.before.as_str(), removed.after.as_str()), (delayed, "osc.saw(hz(110))"));
    // The import stays (removing unused imports is a separate, explicit rule).
    assert!(fixture.source().contains("song(() => osc.saw(hz(110)))"));
    assert_eq!(canonical(&s.accepted().compiled.ir), graph);

    s.undo().unwrap();
    s.undo().unwrap();
    assert_eq!(fixture.source(), original);
}

#[test]
fn an_existing_import_is_joined_and_effects_go_inside_arguments() {
    let original = format!(
        "{HEAD}import {{ drive }} from \"std/fx\";\nimport {{ add }} from \"std/math\";\nexport default song(() => add(drive(osc.saw(hz(110))), osc.sine(hz(220))));\n"
    );
    let fixture = Fixture::new(&original);
    let mut s = fixture.open();
    s.add_effect(rev(&s), &call(&s, "osc.sine"), "std:limit").unwrap();
    assert_eq!(
        fixture.source(),
        original.replace("{ drive }", "{ drive, limit }").replace("osc.sine(hz(220))", "limit(osc.sine(hz(220)), -1)")
    );
}

#[test]
fn removing_takes_the_effects_own_options_with_it() {
    let original = format!(
        "{HEAD}import {{ delay }} from \"std/fx\";\nexport default song(() => delay(osc.saw(hz(110)), {{ time: 0.3, mix: 0.5 }}));\n"
    );
    let fixture = Fixture::new(&original);
    let mut s = fixture.open();
    assert!(s.controls().iter().any(|c| c.literal == "0.3"));
    s.remove_effect(rev(&s), &call(&s, "delay")).unwrap();
    assert!(fixture.source().contains("song(() => osc.saw(hz(110)))"));
    assert!(s.controls().iter().all(|c| c.literal != "0.3"));
}

#[test]
fn only_calls_of_imported_effects_can_be_removed() {
    // `osc.saw` is not an effect, and a local function named like one is not the effect.
    let original = format!("{HEAD}const delay = (x: any) => osc.sine(hz(440));\nexport default song(() => delay(osc.saw(hz(110))));\n");
    let fixture = Fixture::new(&original);
    let mut s = fixture.open();
    assert!(matches!(s.remove_effect(rev(&s), &call(&s, "osc.saw")), Err(EditError::Rejected(_))));
    assert!(matches!(s.remove_effect(rev(&s), &call(&s, "delay")), Err(EditError::Rejected(_))));
    assert_eq!(fixture.source(), original);
}

#[test]
fn removing_the_outer_of_two_effects_keeps_the_inner_one_and_its_options() {
    let fixture = Fixture::new(&format!(
        "{HEAD}import {{ delay }} from \"std/fx\";\nexport default song(() => delay(delay(osc.saw(hz(110)), {{ mix: 1 }})));\n"
    ));
    let mut s = fixture.open();
    let outer = s.accepted().compiled.sites.iter().filter(|x| x.callee == "delay").min_by_key(|x| x.start).unwrap().clone();
    s.remove_effect(rev(&s), &format!("{}#{}", outer.file, outer.ordinal)).unwrap();
    assert!(fixture.source().contains("song(() => delay(osc.saw(hz(110)), { mix: 1 }))"));
}

#[test]
fn the_check_accepts_exactly_the_graph_without_the_effect() {
    let with = Fixture::new(&format!("{HEAD}import {{ delay }} from \"std/fx\";\nexport default song(() => delay(osc.saw(hz(110))));\n"));
    let same = Fixture::new(&format!("{HEAD}export default song(() => osc.saw(hz(110)));\n"));
    let other = Fixture::new(&format!("{HEAD}export default song(() => osc.saw(hz(120)));\n"));
    let s = with.open();
    let site = s.accepted().compiled.sites.iter().find(|x| x.callee == "delay").unwrap().id;
    let saw = s.accepted().compiled.sites.iter().find(|x| x.callee == "osc.saw").unwrap().id;
    assert!(wraps(&s.accepted().compiled, &same.open().accepted().compiled.ir, site));
    assert!(!wraps(&s.accepted().compiled, &other.open().accepted().compiled.ir, site), "a different saw is not the same sound");
    assert!(!wraps(&s.accepted().compiled, &same.open().accepted().compiled.ir, saw), "the saw is not what was added");
}

#[test]
fn adding_is_refused_where_it_would_not_compile() {
    // A delay needs audio: after a control it doesn't type-check, and nothing is written.
    let original = format!("{HEAD}export default song(() => osc.sine(hz(220)));\n");
    let fixture = Fixture::new(&original);
    let mut s = fixture.open();
    let hz = call(&s, "hz");
    assert!(s.add_effect(rev(&s), &hz, "std:delay").is_err());
    assert!(matches!(s.add_effect(rev(&s), &call(&s, "osc.sine"), "std:wobble"), Err(EditError::NotFound(_))));
    assert_eq!(fixture.source(), original);
}

// PKG-2, PKG-7, PKG-11: effects are the catalogued components with an editor category, a free
// string; each names its ComponentId and where it is imported from.
#[test]
fn effects_are_listed_with_their_component_and_category() {
    let fixture = Fixture::new(&format!("{HEAD}export default song(() => osc.saw(hz(110)));\n"));
    let s = fixture.open();
    let effects = s.effects();
    let lowpass = effects.iter().find(|e| e.name == "lowpass").unwrap();
    let found = (lowpass.component.as_str(), lowpass.module.as_str(), lowpass.category.as_str());
    assert_eq!(found, ("std:lowpass", "std/filter", "Filter"));
    assert_eq!(lowpass.label, s.catalog().components["std:lowpass"].label, "named by its descriptor's label");
    let delay = effects.iter().find(|e| e.component == "std:delay").unwrap();
    assert_eq!((delay.name.as_str(), delay.module.as_str(), delay.category.as_str()), ("delay", "std/fx", "Delay & Reverb"));
    // Every effect inserts only its own controls, and is nothing but its descriptor says.
    for e in &effects {
        let d = &s.catalog().components[&e.component];
        assert!(d.is_effect() && !e.category.is_empty() && e.label == d.label, "{}", e.component);
        assert!(d.editor.insert.iter().all(|i| d.control(&i.slot).is_some()), "{}", e.component);
    }
}

#[test]
fn an_effects_args_are_written_after_the_audio_as_knobs_and_removing_restores_the_graph() {
    let original = format!("{HEAD}export default song(() => osc.saw(hz(110)));\n");
    let fixture = Fixture::new(&original);
    let mut s = fixture.open();
    let graph = canonical(&s.accepted().compiled.ir);

    let added = s.add_effect(rev(&s), &call(&s, "osc.saw"), "std:lowpass").unwrap();
    assert_eq!(added.after, "lowpass(osc.saw(hz(110)), 1000)");
    assert!(fixture.source().contains("import { lowpass } from \"std/filter\";\n"));
    // The cutoff is a knob on the filter, typed by the port it feeds.
    let view = s.view();
    let filter = view.modules[0].nodes.iter().find(|n| n.label == "lowpass").unwrap();
    let label = s.catalog().components["std:lowpass"].label.clone();
    assert_eq!((filter.effect.as_deref(), filter.component.as_deref()), (Some(label.as_str()), Some("std:lowpass")));
    let cutoff = filter.knobs.iter().find(|k| k.label == "cutoff").unwrap_or_else(|| panic!("{:?}", filter.knobs));
    assert_eq!((cutoff.value, cutoff.unit, cutoff.log, cutoff.written), (1000.0, Some("hz"), true, true));
    // The resonance it leaves out is a knob too, at its default (PKG-3).
    let q = filter.knobs.iter().find(|k| k.label == "q").unwrap_or_else(|| panic!("{:?}", filter.knobs));
    assert_eq!((q.value, q.written, q.default), (0.707, false, Some(0.707)));

    s.remove_effect(rev(&s), &call(&s, "lowpass")).unwrap();
    assert!(fixture.source().contains("song(() => osc.saw(hz(110)))"));
    assert_eq!(canonical(&s.accepted().compiled.ir), graph);
    s.undo().unwrap();
    s.undo().unwrap();
    assert_eq!(fixture.source(), original);
}

// PKG-3: every effect goes after a sound, and every control it declares is a knob on it from the
// start: the ones its insert writes, written, and the others at their defaults.
#[test]
fn every_effect_goes_after_a_sound_and_every_control_it_declares_is_a_knob() {
    for effect in Fixture::new(&format!("{HEAD}export default song(() => osc.saw(hz(110)));\n")).open().effects() {
        let fixture = Fixture::new(&format!("{HEAD}export default song(() => osc.saw(hz(110)));\n"));
        let mut s = fixture.open();
        let added = s.add_effect(rev(&s), &call(&s, "osc.saw"), &effect.component).unwrap_or_else(|e| panic!("{}: {e}", effect.component));
        let d = s.catalog().components[&effect.component].clone();
        let nodes = &s.view().modules[0].nodes;
        let node =
            nodes.iter().find(|n| n.component.as_deref() == Some(&effect.component)).unwrap_or_else(|| panic!("{}", effect.component));
        let mut knobs: Vec<(&str, bool)> = node.knobs.iter().map(|k| (k.slot.as_deref().unwrap(), k.written)).collect();
        knobs.sort();
        let mut declared: Vec<(&str, bool)> = d
            .controls
            .iter()
            .filter(|c| !c.optional)
            .map(|c| (c.name.as_str(), d.editor.insert.iter().any(|i| i.slot == c.name)))
            .collect();
        declared.sort();
        assert_eq!(knobs, declared, "{}: {}", effect.component, added.after);
    }
}

#[test]
fn every_effect_goes_on_a_channel_the_mixer_pans() {
    // A panned channel takes a stereo sound too (std's pan balances it): a widener, a
    // pan of its own, anything the catalog offers can go before the mixer's pan.
    let source = format!(
        "{HEAD}import {{ mixer }} from \"std/mix\";\nexport default song(() => mixer({{ a: {{ in: osc.saw(hz(110)), pan: 0.3 }} }}));\n"
    );
    for effect in Fixture::new(&source).open().effects() {
        let fixture = Fixture::new(&source);
        let mut s = fixture.open();
        s.add_effect(rev(&s), &call(&s, "osc.saw"), &effect.component).unwrap_or_else(|e| panic!("{} before a pan: {e}", effect.component));
    }
}

#[test]
fn an_effect_goes_into_a_voice_and_comes_out_again() {
    let original = "import { poly, song } from \"@m2/core\";\nimport { adsr, osc, svf, vca } from \"std/dsp\";\n\
                    export default song(({ lane }) => poly(lane.notes(\"keys\"), (n) => vca(svf.lowpass(osc.saw(n.pitch), 800), adsr(n.gate))));\n";
    let fixture = Fixture::new(original);
    let mut s = fixture.open();
    let graph = canonical(&s.accepted().compiled.ir);
    // The + on a node inside the voice: the effect goes on every voice.
    let added = s.add_effect(rev(&s), &call(&s, "osc.saw"), "std:chorus").unwrap();
    assert_eq!(added.after, "chorus(osc.saw(n.pitch), { rate: 0.8, depth: 0.5, mix: 0.5 })");
    assert!(fixture.source().contains("vca(svf.lowpass(chorus(osc.saw(n.pitch), { rate: 0.8, depth: 0.5, mix: 0.5 }), 800)"));
    let view = s.view();
    let chorus = view.modules[0].nodes.iter().find(|n| n.component.as_deref() == Some("std:chorus") && n.effect.is_some());
    let chorus = chorus.expect("the effect can be removed");
    s.remove_effect(rev(&s), &chorus.id.clone()).unwrap();
    assert!(fixture.source().contains("vca(svf.lowpass(osc.saw(n.pitch), 800)"));
    assert_eq!(canonical(&s.accepted().compiled.ir), graph);
}

// PKG-9 (step 5 gate): `import { delay as echo }` is `delay`. The call is recognised by the
// ComponentId its frame records, never by the name it is called by: it is the Delay effect, its
// knobs are delay's three controls with their declared ranges, and it can be taken out.
#[test]
fn an_import_alias_is_the_component() {
    let original = format!("{HEAD}import {{ delay as echo }} from \"std/fx\";\nexport default song(() => echo(osc.saw(hz(110))));\n");
    let fixture = Fixture::new(&original);
    let mut s = fixture.open();
    let frames: Vec<Option<&str>> = s.accepted().compiled.ir.modules[0].frames.iter().map(|f| f.component.as_deref()).collect();
    assert!(frames.contains(&Some("std:delay")), "{frames:?}");
    let view = s.view();
    let echo = view.modules[0].nodes.iter().find(|n| n.label == "echo").expect("the echo call");
    let label = s.catalog().components["std:delay"].label.clone();
    assert_eq!((echo.effect.as_deref(), echo.component.as_deref()), (Some(label.as_str()), Some("std:delay")));
    let knobs: Vec<(&str, bool, Option<[f64; 2]>, bool)> =
        echo.knobs.iter().map(|k| (k.slot.as_deref().unwrap(), k.written, k.range, k.log)).collect();
    assert_eq!(
        knobs,
        [("time", false, Some([0.001, 2.0]), true), ("feedback", false, Some([0.0, 0.95]), false), ("mix", false, Some([0.0, 1.0]), false)]
    );
    s.remove_effect(rev(&s), &echo.id).unwrap();
    assert!(fixture.source().contains("song(() => osc.saw(hz(110)))"));
}

/// A copy of std in a temporary folder, with `edit` applied to its sources.
struct StdCopy(PathBuf);

impl StdCopy {
    fn new(edit: impl FnOnce(&Path)) -> Self {
        fn copy(from: &Path, to: &Path) {
            std::fs::create_dir_all(to).unwrap();
            for e in std::fs::read_dir(from).unwrap().flatten() {
                let (f, t) = (e.path(), to.join(e.file_name()));
                if f.is_dir() {
                    // The kernels-src crate is large and isn't read.
                    if e.file_name() != "kernels-src" && e.file_name() != "target" {
                        copy(&f, &t);
                    }
                } else {
                    std::fs::copy(&f, &t).unwrap();
                }
            }
        }
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("m2-effect-std-{}-{n}", std::process::id()));
        copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std"), &dir);
        edit(&dir);
        Self(dir)
    }

    fn packages(&self) -> PackageSet {
        let mut packages = PackageSet::default();
        packages.load(&self.0).unwrap();
        packages
    }
}

impl Drop for StdCopy {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// PKG-9: moving `delay` from std/fx.ts to its own file changes nothing: its ComponentId names the
// package and the descriptor's id, never the file, so the IR and the view are the same.
#[test]
fn moving_a_component_keeps_its_id() {
    let source =
        format!("{HEAD}import {{ delay }} from \"std/fx\";\nexport default song(() => delay(osc.saw(hz(110)), {{ time: 0.5 }}));\n");
    let fixture = Fixture::new(&source);
    let moved = StdCopy::new(|dir| {
        let fx = std::fs::read_to_string(dir.join("src/fx.ts")).unwrap();
        let start = fx.find("/** Feedback delay.").expect("delay's doc comment");
        let end = start + fx[start..].find("\n);\n").expect("delay's end") + 4;
        let imports = "import { component, control, kernel, port, signature, type Audio, type Params, type Value } from \"@m2/core\";\n";
        std::fs::write(dir.join("src/delay.ts"), format!("{imports}{}", &fx[start..end])).unwrap();
        std::fs::write(dir.join("src/fx.ts"), format!("{}export {{ delay }} from \"./delay\";\n{}", &fx[..start], &fx[end..])).unwrap();
    });
    let here = fixture.open();
    // A second session of one project in one process can't take its lock (C10): it only reads.
    let there = Session::open_read_only(&fixture.0, moved.packages()).unwrap();
    assert!(there.problems().is_empty(), "{:?}", there.problems());
    let components =
        |s: &Session| -> Vec<Option<String>> { s.accepted().compiled.ir.modules[0].frames.iter().map(|f| f.component.clone()).collect() };
    assert_eq!(components(&there), components(&here));
    assert!(components(&there).contains(&Some("std:delay".into())));
    assert_eq!(there.accepted().compiled.components["std:delay"].module, "std/delay");
    assert_eq!(there.view(), here.view());
    // The catalog still offers it, from the module that defines it now.
    let delay = there.effects().into_iter().find(|e| e.component == "std:delay").unwrap();
    assert_eq!((delay.module.as_str(), delay.name.as_str()), ("std/delay", "delay"));
}

// ID-15, D17: a component that native code calls (`Array.from` passing it as the mapping) takes no
// site, so no frame: no `.map(...)` or `Array.from(...)` call is an effect or has knobs. One
// called from an arrow is that arrow's call.
#[test]
fn a_native_caller_takes_no_site() {
    let fixture = Fixture::new(&format!(
        "{HEAD}import {{ lowpass }} from \"std/filter\";\nimport {{ sum }} from \"std/math\";\n\
         export default song(() => sum(...[osc.saw(hz(110))].map((y) => lowpass(y)), ...Array.from({{ length: 1 }}, osc.square)));\n"
    ));
    let s = fixture.open();
    let view = s.view();
    let nodes = &view.modules[0].nodes;
    for callee in ["map", "Array.from"] {
        assert!(
            nodes.iter().filter(|n| n.label.ends_with(callee)).all(|n| n.knobs.is_empty() && n.effect.is_none()),
            "{callee}: {nodes:#?}"
        );
    }
    let lowpass = nodes.iter().find(|n| n.label == "lowpass").expect("the arrow's call");
    assert!(lowpass.effect.is_some() && lowpass.component.as_deref() == Some("std:lowpass"));
    let sites = &s.accepted().compiled.sites;
    let frames: Vec<&str> =
        s.accepted().compiled.ir.modules[0].frames.iter().map(|f| sites.iter().find(|x| x.id == f.site).unwrap().callee.as_str()).collect();
    assert!(!frames.iter().any(|c| c.ends_with("map") || *c == "Array.from"), "{frames:?}");
}
