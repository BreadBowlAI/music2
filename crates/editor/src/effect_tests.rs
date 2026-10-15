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

// ---------------------------------------------------------------- what the engine keeps (ID-13)

/// A kernel node by what it reads as constants: `std:delay[0.388,0.3,0.3]`.
fn by_consts(flat: &m2_ir::Flat, n: &m2_ir::FNode) -> String {
    let m2_ir::FOp::Kernel { kernel, inputs, .. } = &n.op else { return n.name.clone() };
    let consts: Vec<String> = inputs
        .iter()
        .filter_map(|i| match i {
            m2_ir::Input::Const(v) => Some(*v),
            m2_ir::Input::Port { src, .. } => match flat.nodes[src.node].op {
                m2_ir::FOp::Const(v) => Some(v),
                _ => None,
            },
            m2_ir::Input::Unset => None,
        })
        .map(|v| v.to_string())
        .collect();
    format!("{kernel}[{}]", consts.join(","))
}

/// Each `kernel` node after `edit`, and the node before it that it continues, as the engine is
/// told when it activates the edit (`m2_session::tx::continues`), sorted.
fn continued(s: &mut Session, kernel: &str, edit: impl FnOnce(&mut Session)) -> Vec<(String, Option<String>)> {
    let before = s.accepted_arc();
    edit(s);
    let after = s.accepted_arc();
    let c = m2_session::tx::continues::continues(&before, &after);
    let (old, new) = (&before.compiled.flat, &after.compiled.flat);
    let mut out: Vec<(String, Option<String>)> = new
        .nodes
        .iter()
        .zip(&c.top)
        .filter(|(n, _)| matches!(&n.op, m2_ir::FOp::Kernel { kernel: k, .. } if k == kernel))
        .map(|(n, j)| (by_consts(new, n), j.map(|j| by_consts(old, &old.nodes[j as usize]))))
        .collect();
    out.sort();
    out
}

/// The calls to `callee` in `file`, in the order they are written: `file#ordinal`.
fn calls_in(s: &Session, file: &str, callee: &str) -> Vec<String> {
    let mut sites: Vec<_> = s.accepted().compiled.sites.iter().filter(|x| x.in_project && x.file == file && x.callee == callee).collect();
    sites.sort_by_key(|x| x.start);
    sites.iter().map(|x| format!("{}#{}", x.file, x.ordinal)).collect()
}

fn kept(new: &str, old: &str) -> (String, Option<String>) {
    (new.to_string(), Some(old.to_string()))
}

fn made(new: &str) -> (String, Option<String>) {
    (new.to_string(), None)
}

// ID-13, R5: Add Effect puts a delay inside the user's own delay (after its sound). The user's
// delay is the outer call now, and it keeps its instance, echoes and time; the new one starts
// fresh. Before the fix the old delay went to the new inner call and its time jumped.
#[test]
fn a_delay_added_inside_a_delay_keeps_the_users_delay() {
    let f = Fixture::new(&format!(
        "{HEAD}import {{ delay }} from \"std/fx\";\nexport default song(() => delay(osc.saw(hz(110)), {{ time: 0.388 }}));\n"
    ));
    let mut s = f.open();
    let saw = call(&s, "osc.saw");
    let pairs = continued(&mut s, "std:delay", |s| drop(s.add_effect(rev(s), &saw, "std:delay").unwrap()));
    assert!(f.source().contains("delay(delay(osc.saw(hz(110)), { time: 0.25, feedback: 0.3, mix: 0.3 }), { time: 0.388 })"));
    assert_eq!(pairs, [made("std:delay[0.25,0.3,0.3]"), kept("std:delay[0.388,0.3,0.3]", "std:delay[0.388,0.3,0.3]")]);
}

// ID-13: Add Effect on the delay call itself wraps it in a new delay: the wrapped one keeps its
// instance.
#[test]
fn a_delay_wrapped_around_a_delay_keeps_the_wrapped_one() {
    let f = Fixture::new(&format!(
        "{HEAD}import {{ delay }} from \"std/fx\";\nexport default song(() => delay(osc.saw(hz(110)), {{ time: 0.388 }}));\n"
    ));
    let mut s = f.open();
    let delay = call(&s, "delay");
    let pairs = continued(&mut s, "std:delay", |s| drop(s.add_effect(rev(s), &delay, "std:delay").unwrap()));
    assert!(f.source().contains("delay(delay(osc.saw(hz(110)), { time: 0.388 }), { time: 0.25, feedback: 0.3, mix: 0.3 })"));
    assert_eq!(pairs, [made("std:delay[0.25,0.3,0.3]"), kept("std:delay[0.388,0.3,0.3]", "std:delay[0.388,0.3,0.3]")]);
}

// ID-13: Remove Effect on either of two delays, one inside the other: the delay that stays keeps
// its own instance, never the removed one's.
#[test]
fn removing_either_of_two_delays_keeps_the_other() {
    let source = format!(
        "{HEAD}import {{ delay }} from \"std/fx\";\nexport default song(() => delay(delay(osc.saw(hz(110)), {{ time: 0.25 }}), {{ time: 0.388 }}));\n"
    );
    for (which, stays) in [(1, "std:delay[0.388,0.3,0.3]"), (0, "std:delay[0.25,0.3,0.3]")] {
        let f = Fixture::new(&source);
        let mut s = f.open();
        let target = calls_in(&s, "song.ts", "delay")[which].clone();
        let pairs = continued(&mut s, "std:delay", |s| drop(s.remove_effect(rev(s), &target).unwrap()));
        assert_eq!(pairs, [kept(stays, stays)], "removing the {} delay", ["outer", "inner"][which]);
    }
}

// ID-13: the same for a filter, whose setting is written positionally: a lowpass added inside a
// lowpass leaves the user's on the outer call, and one wrapped around it leaves it on the inner.
#[test]
fn a_lowpass_added_inside_or_around_a_lowpass_keeps_the_users_lowpass() {
    let source = format!("{HEAD}import {{ lowpass }} from \"std/filter\";\nexport default song(() => lowpass(osc.saw(hz(110)), 6000));\n");
    for (on, written) in
        [("osc.saw", "lowpass(lowpass(osc.saw(hz(110)), 1000), 6000)"), ("lowpass", "lowpass(lowpass(osc.saw(hz(110)), 6000), 1000)")]
    {
        let f = Fixture::new(&source);
        let mut s = f.open();
        let target = call(&s, on);
        let pairs = continued(&mut s, "std:svf", |s| drop(s.add_effect(rev(s), &target, "std:lowpass").unwrap()));
        assert!(f.source().contains(written), "{}", f.source());
        assert_eq!(pairs, [made("std:svf[1000,0.707]"), kept("std:svf[6000,0.707]", "std:svf[6000,0.707]")], "after {on}");
    }
}

// ID-13: the starter's hook, Add Reverb after its `widen(...)`: the track's own reverb is the outer
// call and keeps its tail; so does every other node of the track. The chords' Add Chorus after
// `widen(...)` is the same.
#[test]
fn the_starters_add_reverb_keeps_the_hooks_reverb() {
    let f = crate::testing::fixture("templates/starter");
    let mut s = f.open();
    let widen = calls_in(&s, "tracks/hook.ts", "widen")[0].clone();
    let pairs = continued(&mut s, "std:reverb", |s| drop(s.add_effect(rev(s), &widen, "std:reverb").unwrap()));
    assert!(
        f.read("tracks/hook.ts")
            .contains("reverb(reverb(widen(echoes, { width: 1.3 }), { size: 0.6, damping: 0.5, mix: 0.25 }), { size: 0.6, mix: 0.2 })")
    );
    let hook = [made("std:reverb[0.6,0.5,0.25]"), kept("std:reverb[0.6,0.5,0.2]", "std:reverb[0.6,0.5,0.2]")];
    assert!(pairs.iter().filter(|p| p.1.is_none()).count() == 1 && hook.iter().all(|p| pairs.contains(p)), "{pairs:?}");
    let widen = calls_in(&s, "tracks/chords.ts", "widen")[0].clone();
    let pairs = continued(&mut s, "std:chorus", |s| drop(s.add_effect(rev(s), &widen, "std:chorus").unwrap()));
    assert_eq!(pairs, [kept("std:chorus[0.6,0.4,0.4]", "std:chorus[0.6,0.4,0.4]"), made("std:chorus[0.8,0.5,0.5]")]);
}

/// `rev` played live from 0 for `frames`; when `next` is given, it is activated at frame `at` as the
/// server activates an edit: prepared against what plays, keeping what the session says continues.
fn played(rev: &m2_session::Revision, packages: &PackageSet, frames: usize, next: Option<(usize, &m2_session::Revision)>) -> Vec<[f32; 2]> {
    use m2_runtime::{Basis, CallbackStamp, Engine, Envelope, Keep, Msg, Renderer, Transport, audio::Cache};
    let (flat, data) = (&rev.compiled.flat, &rev.compiled.data);
    let mut r = Renderer::new(flat, data, packages, 48_000).unwrap();
    r.set_strict(false);
    r.set_transport(Transport::new(0, true));
    let info = r.info();
    let mut e = Engine::with(Box::new(r));
    let mut retired = vec![];
    let mut run = |e: &mut Engine, inbox: &mut Option<Envelope>, n: usize| {
        let mut out = vec![[0.0; 2]; n];
        let mut retire = |r| {
            retired.push(r);
            Ok(())
        };
        e.process(&mut out, &mut || inbox.take(), &mut retire, CallbackStamp::default());
        out
    };
    let Some((at, new)) = next else { return run(&mut e, &mut None, frames) };
    let mut out = run(&mut e, &mut None, at);
    let cont = m2_session::tx::continues::continues(rev, new);
    let restarts = Default::default();
    let keep = Keep { old: flat, old_data: data, info: &info, cont: &cont, restarts: &restarts };
    let (flat, data) = (&new.compiled.flat, &new.compiled.data);
    let owner = m2_plugins::Owner::discard();
    let mut r = Renderer::prepare(flat, data, packages, 48_000, &mut Cache::default(), &owner, Basis::Keep(keep)).unwrap();
    r.set_strict(false);
    let mut inbox = Some(Envelope { seq: 1, msg: Msg::Activate { renderer: Box::new(r) } });
    out.extend(run(&mut e, &mut inbox, frames - at));
    assert!(inbox.is_none() && e.refused_activations() == 0, "the edit was activated");
    out
}

// ID-13, R5, RENDER-25: what it sounds like. A delay added inside the user's delay while its echoes
// ring and nothing goes in: the kept delay's echoes go on exactly as with no edit, to the bit. With
// the old pairing the user's 0.388 s echoes went to the new 0.25 s delay, whose time cell then
// stepped on a buffer of echoes: a jump in the silence, and other echoes for a second.
#[test]
fn a_delay_added_inside_a_ringing_delay_changes_no_bit_of_its_echoes() {
    let f = Fixture::new(
        "import { poly, song, track } from \"@m2/core\";\nimport { osc, vca } from \"std/dsp\";\nimport { delay } from \"std/fx\";\n\
         const t = track(\"t\", ({ lane }) => delay(poly(lane.notes(\"n\"), (n) => vca(osc.sine(n.pitch), n.gate)), { time: 0.388, feedback: 0.5 }));\n\
         export default song(() => t.out);\n",
    );
    std::fs::create_dir_all(f.0.join("data/t")).unwrap();
    std::fs::write(f.0.join("data/arrangement.json"), "{\"tempo\": [{\"beat\": 0, \"bpm\": 120}], \"meter\": []}").unwrap();
    // One note, an eighth long (0.25 s): from then on the delay hears exact silence.
    let note = "{\"clips\": [{\"id\": \"c\", \"start\": 0, \"length\": 4, \"notes\": [{\"pitch\": \"A4\", \"start\": 0, \"length\": 0.5, \"velocity\": 1}]}]}";
    std::fs::write(f.0.join("data/t/n.json"), note).unwrap();
    let mut s = f.open();
    let before = s.accepted_arc();
    let poly = call(&s, "poly");
    s.add_effect(rev(&s), &poly, "std:delay").unwrap();
    let after = s.accepted_arc();
    let (at, frames) = (33_600, 48_000 * 2);
    let alone = played(&before, &s.packages, frames, None);
    let edited = played(&before, &s.packages, frames, Some((at, &after)));
    let peak = alone[at..at + 4800].iter().flatten().fold(0f32, |m, x| m.max(x.abs()));
    assert!(peak > 0.05, "echoes ring through the edit: {peak}");
    let first = alone.iter().zip(&edited).position(|(x, y)| x.map(f32::to_bits) != y.map(f32::to_bits));
    assert_eq!(first, None, "the first frame that differs from no edit (the edit is at {at})");
}
