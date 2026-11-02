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
        let mut packages = PackageSet::default().with_store(m2_song::pack::test_store());
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

    let added = s.add_effect(rev(&s), &call(&s, "osc.saw"), "@m2/effects:delay").unwrap();
    let delayed = "delay(osc.saw(hz(110)), { time: 0.25, feedback: 0.3, mix: 0.3 })";
    assert_eq!((added.before.as_str(), added.after.as_str()), ("osc.saw(hz(110))", delayed));
    assert_eq!(
        fixture.source(),
        original
            .replace("from \"std/dsp\";\n", "from \"std/dsp\";\nimport { delay } from \"@m2/effects\";\n")
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
    assert_eq!(readers, ["std:saw", "@m2/effects:delay"]);

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
        "{HEAD}import {{ eq3 }} from \"std/fx\";\nimport {{ add }} from \"std/math\";\nexport default song(() => add(eq3(osc.saw(hz(110))), osc.sine(hz(220))));\n"
    );
    let fixture = Fixture::new(&original);
    let mut s = fixture.open();
    s.add_effect(rev(&s), &call(&s, "osc.sine"), "std:limit").unwrap();
    assert_eq!(
        fixture.source(),
        original.replace("{ eq3 }", "{ eq3, limit }").replace("osc.sine(hz(220))", "limit(osc.sine(hz(220)), -1)")
    );
}

#[test]
fn removing_takes_the_effects_own_options_with_it() {
    let original = format!(
        "{HEAD}import {{ delay }} from \"@m2/effects\";\nexport default song(() => delay(osc.saw(hz(110)), {{ time: 0.3, mix: 0.5 }}));\n"
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
        "{HEAD}import {{ delay }} from \"@m2/effects\";\nexport default song(() => delay(delay(osc.saw(hz(110)), {{ mix: 1 }})));\n"
    ));
    let mut s = fixture.open();
    let outer = s.accepted().compiled.sites.iter().filter(|x| x.callee == "delay").min_by_key(|x| x.start).unwrap().clone();
    s.remove_effect(rev(&s), &format!("{}#{}", outer.file, outer.ordinal)).unwrap();
    assert!(fixture.source().contains("song(() => delay(osc.saw(hz(110)), { mix: 1 }))"));
}

#[test]
fn the_check_accepts_exactly_the_graph_without_the_effect() {
    let with =
        Fixture::new(&format!("{HEAD}import {{ delay }} from \"@m2/effects\";\nexport default song(() => delay(osc.saw(hz(110))));\n"));
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
    assert!(s.add_effect(rev(&s), &hz, "@m2/effects:delay").is_err());
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
    let delay = effects.iter().find(|e| e.component == "@m2/effects:delay").unwrap();
    assert_eq!((delay.name.as_str(), delay.module.as_str(), delay.category.as_str()), ("delay", "@m2/effects", "Delay & Reverb"));
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
    assert_eq!((cutoff.value.clone(), cutoff.unit, cutoff.log, cutoff.written), (serde_json::json!(1000), Some("hz"), true, true));
    // The resonance it leaves out is a knob too, at its default (PKG-3).
    let q = filter.knobs.iter().find(|k| k.label == "q").unwrap_or_else(|| panic!("{:?}", filter.knobs));
    assert_eq!((q.value.clone(), q.written, q.default.clone()), (serde_json::json!(0.707), false, Some(serde_json::json!(0.707))));

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
    let added = s.add_effect(rev(&s), &call(&s, "osc.saw"), "@m2/effects:chorus").unwrap();
    assert_eq!(added.after, "chorus(osc.saw(n.pitch), { rate: 0.8, depth: 0.5, mix: 0.5 })");
    assert!(fixture.source().contains("vca(svf.lowpass(chorus(osc.saw(n.pitch), { rate: 0.8, depth: 0.5, mix: 0.5 }), 800)"));
    let view = s.view();
    let chorus = view.modules[0].nodes.iter().find(|n| n.component.as_deref() == Some("@m2/effects:chorus") && n.effect.is_some());
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
    let original = format!("{HEAD}import {{ delay as echo }} from \"@m2/effects\";\nexport default song(() => echo(osc.saw(hz(110))));\n");
    let fixture = Fixture::new(&original);
    let mut s = fixture.open();
    let frames: Vec<Option<&str>> = s.accepted().compiled.ir.modules[0].frames.iter().map(|f| f.component.as_deref()).collect();
    assert!(frames.contains(&Some("@m2/effects:delay")), "{frames:?}");
    let view = s.view();
    let echo = view.modules[0].nodes.iter().find(|n| n.label == "echo").expect("the echo call");
    let label = s.catalog().components["@m2/effects:delay"].label.clone();
    assert_eq!((echo.effect.as_deref(), echo.component.as_deref()), (Some(label.as_str()), Some("@m2/effects:delay")));
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
        let mut packages = PackageSet::default().with_store(m2_song::pack::test_store());
        packages.load(&self.0).unwrap();
        packages
    }
}

impl Drop for StdCopy {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// PKG-9: moving `compress` from std/fx.ts to its own file changes nothing: its ComponentId names
// the package and the descriptor's id, never the file, so the IR and the view are the same.
#[test]
fn moving_a_component_keeps_its_id() {
    let source =
        format!("{HEAD}import {{ compress }} from \"std/fx\";\nexport default song(() => compress(osc.saw(hz(110)), {{ ratio: 4 }}));\n");
    let fixture = Fixture::new(&source);
    let moved = StdCopy::new(|dir| {
        let fx = std::fs::read_to_string(dir.join("src/fx.ts")).unwrap();
        let start = fx.find("/**\n * Compressor:").expect("compress's doc comment");
        let end = start + fx[start..].find("\n);\n").expect("compress's end") + 4;
        let imports = "import { component, control, kernel, port, signature, type Audio, type Params, type Value } from \"@m2/core\";\n";
        std::fs::write(dir.join("src/compress.ts"), format!("{imports}{}", &fx[start..end])).unwrap();
        std::fs::write(dir.join("src/fx.ts"), format!("{}export {{ compress }} from \"./compress\";\n{}", &fx[..start], &fx[end..]))
            .unwrap();
    });
    let here = fixture.open();
    // A second session of one project in one process can't take its lock (C10): it only reads.
    let there = Session::open_read_only(&fixture.0, moved.packages()).unwrap();
    assert!(there.problems().is_empty(), "{:?}", there.problems());
    let components =
        |s: &Session| -> Vec<Option<String>> { s.accepted().compiled.ir.modules[0].frames.iter().map(|f| f.component.clone()).collect() };
    assert_eq!(components(&there), components(&here));
    assert!(components(&there).contains(&Some("std:compress".into())));
    assert_eq!(there.accepted().compiled.components["std:compress"].module, "std/compress");
    assert_eq!(there.view(), here.view());
    // The catalog still offers it, from the module that defines it now.
    let compress = there.effects().into_iter().find(|e| e.component == "std:compress").unwrap();
    assert_eq!((compress.module.as_str(), compress.name.as_str()), ("std/compress", "compress"));
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

/// A kernel node by what it reads as constants: `@m2/effects:delay[0.388,0.3,0.3]`.
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
        "{HEAD}import {{ delay }} from \"@m2/effects\";\nexport default song(() => delay(osc.saw(hz(110)), {{ time: 0.388 }}));\n"
    ));
    let mut s = f.open();
    let saw = call(&s, "osc.saw");
    let pairs = continued(&mut s, "@m2/effects:delay", |s| drop(s.add_effect(rev(s), &saw, "@m2/effects:delay").unwrap()));
    assert!(f.source().contains("delay(delay(osc.saw(hz(110)), { time: 0.25, feedback: 0.3, mix: 0.3 }), { time: 0.388 })"));
    assert_eq!(
        pairs,
        [made("@m2/effects:delay[0.25,0.3,0.3]"), kept("@m2/effects:delay[0.388,0.3,0.3]", "@m2/effects:delay[0.388,0.3,0.3]")]
    );
}

// ID-13: Add Effect on the delay call itself wraps it in a new delay: the wrapped one keeps its
// instance.
#[test]
fn a_delay_wrapped_around_a_delay_keeps_the_wrapped_one() {
    let f = Fixture::new(&format!(
        "{HEAD}import {{ delay }} from \"@m2/effects\";\nexport default song(() => delay(osc.saw(hz(110)), {{ time: 0.388 }}));\n"
    ));
    let mut s = f.open();
    let delay = call(&s, "delay");
    let pairs = continued(&mut s, "@m2/effects:delay", |s| drop(s.add_effect(rev(s), &delay, "@m2/effects:delay").unwrap()));
    assert!(f.source().contains("delay(delay(osc.saw(hz(110)), { time: 0.388 }), { time: 0.25, feedback: 0.3, mix: 0.3 })"));
    assert_eq!(
        pairs,
        [made("@m2/effects:delay[0.25,0.3,0.3]"), kept("@m2/effects:delay[0.388,0.3,0.3]", "@m2/effects:delay[0.388,0.3,0.3]")]
    );
}

// ID-13: Remove Effect on either of two delays, one inside the other: the delay that stays keeps
// its own instance, never the removed one's.
#[test]
fn removing_either_of_two_delays_keeps_the_other() {
    let source = format!(
        "{HEAD}import {{ delay }} from \"@m2/effects\";\nexport default song(() => delay(delay(osc.saw(hz(110)), {{ time: 0.25 }}), {{ time: 0.388 }}));\n"
    );
    for (which, stays) in [(1, "@m2/effects:delay[0.388,0.3,0.3]"), (0, "@m2/effects:delay[0.25,0.3,0.3]")] {
        let f = Fixture::new(&source);
        let mut s = f.open();
        let target = calls_in(&s, "song.ts", "delay")[which].clone();
        let pairs = continued(&mut s, "@m2/effects:delay", |s| drop(s.remove_effect(rev(s), &target).unwrap()));
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
    let pairs = continued(&mut s, "@m2/effects:reverb", |s| drop(s.add_effect(rev(s), &widen, "@m2/effects:reverb").unwrap()));
    assert!(
        f.read("tracks/hook.ts")
            .contains("reverb(reverb(widen(echoes, { width: 1.3 }), { size: 0.6, damping: 0.5, mix: 0.25 }), { size: 0.6, mix: 0.2 })")
    );
    let hook = [made("@m2/effects:reverb[0.6,0.5,0.25]"), kept("@m2/effects:reverb[0.6,0.5,0.2]", "@m2/effects:reverb[0.6,0.5,0.2]")];
    assert!(pairs.iter().filter(|p| p.1.is_none()).count() == 1 && hook.iter().all(|p| pairs.contains(p)), "{pairs:?}");
    let widen = calls_in(&s, "tracks/chords.ts", "widen")[0].clone();
    let pairs = continued(&mut s, "std:chorus", |s| drop(s.add_effect(rev(s), &widen, "@m2/effects:chorus").unwrap()));
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
        "import { poly, song, track } from \"@m2/core\";\nimport { osc, vca } from \"std/dsp\";\nimport { delay } from \"@m2/effects\";\n\
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
    s.add_effect(rev(&s), &poly, "@m2/effects:delay").unwrap();
    let after = s.accepted_arc();
    let (at, frames) = (33_600, 48_000 * 2);
    let alone = played(&before, &s.packages, frames, None);
    let edited = played(&before, &s.packages, frames, Some((at, &after)));
    let peak = alone[at..at + 4800].iter().flatten().fold(0f32, |m, x| m.max(x.abs()));
    assert!(peak > 0.05, "echoes ring through the edit: {peak}");
    let first = alone.iter().zip(&edited).position(|(x, y)| x.map(f32::to_bits) != y.map(f32::to_bits));
    assert_eq!(first, None, "the first frame that differs from no edit (the edit is at {at})");
}

// ---------------------------------------------------------------- turned off (Next D1)

/// The song with `body` as its function's result, the saw and std's effects imported.
fn fx_song(body: &str) -> String {
    format!(
        "{HEAD}import {{ delay, reverb }} from \"@m2/effects\";\nimport {{ lowpass }} from \"std/filter\";\nimport {{ master, mixer }} from \"std/mix\";\n\
         export default song(() => {body});\n"
    )
}

/// Turn `callee`'s call off (`on`) or on.
fn switch(s: &mut Session, callee: &str, on: bool) -> Result<Edited, EditError> {
    let (at, id) = (rev(s), call(s, callee));
    bypass(s, at, &id, on)
}

/// Turn `callee`'s call off (`on`) or on, and say what the file says now.
fn turned(f: &Fixture, s: &mut Session, callee: &str, on: bool) -> String {
    switch(s, callee, on).unwrap_or_else(|e| panic!("{callee} {on}: {e}"));
    f.source()
}

/// The kernels the engine runs: the checked graph's order.
fn running(s: &Session) -> Vec<String> {
    let flat = &s.accepted().compiled.flat;
    flat.order
        .iter()
        .filter_map(|&i| match &flat.nodes[i].op {
            m2_ir::FOp::Kernel { kernel, .. } => Some(kernel.clone()),
            _ => None,
        })
        .collect()
}

// Next D1 (Q24): turning an effect off writes `bypass: true` after its options' last property; its
// call still makes its nodes and knobs, and gives its input on, so the engine runs none of it.
// Undo puts the code back.
#[test]
fn bypass_writes_bypass_true_into_the_options() {
    let original = fx_song("reverb(osc.saw(hz(110)), { size: 0.8 })");
    let f = Fixture::new(&original);
    let mut s = f.open();
    let knobs = s.controls().len();
    assert!(running(&s).contains(&"@m2/effects:reverb".to_string()));
    let done = switch(&mut s, "reverb", true).unwrap();
    assert_eq!(f.source(), original.replace("{ size: 0.8 }", "{ size: 0.8, bypass: true }"));
    assert_eq!(
        (done.before.as_str(), done.after.as_str()),
        ("reverb(osc.saw(hz(110)), { size: 0.8 })", "reverb(osc.saw(hz(110)), { size: 0.8, bypass: true })")
    );
    let site = s.accepted().compiled.sites.iter().find(|x| x.callee == "reverb").unwrap().id;
    assert!(is_bypassed(&s.accepted().compiled, site));
    assert_eq!(running(&s), ["std:saw"], "the reverb runs nothing");
    assert_eq!(s.controls().len(), knobs, "its knobs stay to set");
    s.undo().unwrap();
    assert_eq!(f.source(), original);
    assert!(running(&s).contains(&"@m2/effects:reverb".to_string()));
}

// Next D1: a call with no options object gets a new one, after its last argument; one written
// over lines gets it on a line of its own (D4e); `bypass: false` becomes true.
#[test]
fn bypass_into_a_call_with_no_options() {
    let cases = [
        ("reverb(osc.saw(hz(110)))", "reverb", "reverb(osc.saw(hz(110)), { bypass: true })"),
        ("lowpass(osc.saw(hz(110)), 800)", "lowpass", "lowpass(osc.saw(hz(110)), 800, { bypass: true })"),
        ("delay(osc.saw(hz(110)), {})", "delay", "delay(osc.saw(hz(110)), { bypass: true })"),
        ("delay(osc.saw(hz(110)), { bypass: false })", "delay", "delay(osc.saw(hz(110)), { bypass: true })"),
        (
            "master(\n    reverb(\n      osc.saw(hz(110)),\n    ),\n  )",
            "reverb",
            "master(\n    reverb(\n      osc.saw(hz(110)),\n      { bypass: true },\n    ),\n  )",
        ),
    ];
    for (call_text, callee, after) in cases {
        let original = fx_song(call_text);
        let f = Fixture::new(&original);
        let mut s = f.open();
        let before = running(&s);
        assert_eq!(turned(&f, &mut s, callee, true), fx_song(after), "{call_text}");
        let site = s.accepted().compiled.sites.iter().find(|x| x.callee == callee).unwrap().id;
        assert!(is_bypassed(&s.accepted().compiled, site), "{call_text}");
        assert!(running(&s).len() < before.len(), "{call_text}: the effect's kernels don't run");
    }
}

// Next D1: turning it on again takes the property out with its comma, and the options object
// when it was alone.
#[test]
fn bypass_off_takes_it_out_and_its_braces() {
    let cases = [
        ("reverb(osc.saw(hz(110)), { size: 0.8, bypass: true })", "reverb(osc.saw(hz(110)), { size: 0.8 })"),
        ("reverb(osc.saw(hz(110)), { bypass: true, size: 0.8 })", "reverb(osc.saw(hz(110)), { size: 0.8 })"),
        ("reverb(osc.saw(hz(110)), { size: 0.8, bypass: true, mix: 0.2 })", "reverb(osc.saw(hz(110)), { size: 0.8, mix: 0.2 })"),
        ("reverb(osc.saw(hz(110)), { bypass: true })", "reverb(osc.saw(hz(110)))"),
        ("reverb(osc.saw(hz(110)), {\n    size: 0.8,\n    bypass: true,\n  })", "reverb(osc.saw(hz(110)), {\n    size: 0.8,\n  })"),
    ];
    for (call_text, after) in cases {
        let original = fx_song(call_text);
        let f = Fixture::new(&original);
        let mut s = f.open();
        assert_eq!(running(&s), ["std:saw"], "{call_text}: off at first");
        assert_eq!(turned(&f, &mut s, "reverb", false), fx_song(after), "{call_text}");
        assert!(running(&s).contains(&"@m2/effects:reverb".to_string()), "{call_text}");
    }
}

// Next D1: off then on is the code it was, in each place an options object can go.
#[test]
fn bypass_then_unbypass_is_the_same_code() {
    for body in [
        "reverb(osc.saw(hz(110)))",
        "reverb(osc.saw(hz(110)), { size: 0.8 })",
        "lowpass(osc.saw(hz(110)), 800)",
        "master(\n    reverb(osc.saw(hz(110))),\n  )",
        "master(\n    reverb(\n      osc.saw(hz(110)),\n    ),\n  )",
        "master(\n    reverb(\n      osc.saw(hz(110))\n    ),\n  )",
    ] {
        let original = fx_song(body);
        let f = Fixture::new(&original);
        let mut s = f.open();
        let callee = if body.contains("lowpass") { "lowpass" } else { "reverb" };
        assert_ne!(turned(&f, &mut s, callee, true), original, "{body}");
        assert_eq!(turned(&f, &mut s, callee, false), original, "{body}");
    }
}

// Next D1: a bypassed effect is in the graph but nothing reads it, so the song sounds exactly as
// it does with the effect removed; and remove still takes it out.
#[test]
fn a_bypassed_effect_runs_nothing() {
    let f = Fixture::new(&fx_song("delay(reverb(osc.saw(hz(110)), { bypass: true }), { time: 0.1 })"));
    let mut s = f.open();
    assert_eq!(running(&s), ["std:saw", "@m2/effects:delay"]);
    let off = s.accepted_arc();
    let removed = s.remove_effect(rev(&s), &call(&s, "reverb")).unwrap();
    assert_eq!(removed.after, "osc.saw(hz(110))");
    let without = s.accepted_arc();
    let (a, b) = (played(&off, &s.packages, 4800, None), played(&without, &s.packages, 4800, None));
    assert!(a.iter().any(|x| x[0] != 0.0));
    assert!(a.iter().zip(&b).all(|(x, y)| x.map(f32::to_bits) == y.map(f32::to_bits)), "the same to the bit");
}

// Next D1: what can't be turned off here says why in words.
#[test]
fn bypass_refuses_a_call_that_spreads_its_arguments() {
    let f = Fixture::new(&fx_song("reverb(...([osc.saw(hz(110)), { size: 0.8 }] as const))"));
    let mut s = f.open();
    let e = switch(&mut s, "reverb", true).unwrap_err().to_string();
    assert!(e.contains("Reverb can't be turned off here: its call spreads its arguments. Turn it off in code."), "{e}");
    // On again when it is on already, and off when it is off.
    let f = Fixture::new(&fx_song("reverb(osc.saw(hz(110)))"));
    let mut s = f.open();
    let e = switch(&mut s, "reverb", false).unwrap_err().to_string();
    assert!(e.contains("Reverb is on already."), "{e}");
    // A component with a bypass of its own keeps it.
    let own = "import { component, control, kernel, port, song, type Audio, type Params } from \"@m2/core\";\nimport { osc } from \"std/dsp\";\n\
               const fuzz = component(\n  { id: \"fuzz\", label: \"Fuzz\", inputs: { in: port.audio() }, controls: { bypass: control.db(0) }, output: port.audio(), editor: { category: \"Drive\" } },\n  \
               (x: Audio, { bypass }: Params) => kernel(\"std:gain\", { in: x, db: bypass }),\n);\nexport default song(() => fuzz(osc.saw(110)));\n";
    let f = Fixture::new(own);
    let mut s = f.open();
    let e = switch(&mut s, "fuzz", true).unwrap_err().to_string();
    assert!(e.contains("It has its own Bypass knob."), "{e}");
    let view = crate::view::view(&s);
    let card = view.modules[0].nodes.iter().find(|n| n.label == "fuzz").unwrap();
    assert_eq!(card.menu.as_ref().and_then(|m| m.bypass.as_deref()), Some("It has its own Bypass knob."));
}

// Next D1 (X10): the Device view keeps a turned-off card where it was: what reads the call is
// drawn from the card, dashed, not from the effect's input.
#[test]
fn a_bypassed_effect_keeps_its_place_in_the_chain() {
    let f = Fixture::new(&fx_song("master(reverb(osc.saw(hz(110)), { size: 0.8 }))"));
    let mut s = f.open();
    let shape = |s: &Session| {
        let v = crate::view::view(s);
        let m = &v.modules[0];
        let name = |id: &str| m.nodes.iter().find(|n| n.id == id).map_or(id.to_string(), |n| n.label.clone());
        let mut e: Vec<(String, String, bool)> = m.port_edges.iter().map(|e| (name(&e.from), name(&e.to), e.bypassed)).collect();
        e.sort();
        let card = m.nodes.iter().find(|n| n.label == "reverb").unwrap();
        (e, card.bypassed, card.menu.clone().unwrap().bypass)
    };
    let on = shape(&s);
    let edge = |a: &str, b: &str, dashed: bool| (a.to_string(), b.to_string(), dashed);
    assert!(on.0.contains(&edge("osc.saw", "reverb", false)) && on.0.contains(&edge("reverb", "master", false)), "{on:?}");
    assert_eq!((on.1, on.2), (false, None));
    switch(&mut s, "reverb", true).unwrap();
    let off = shape(&s);
    let edges: Vec<(String, String, bool)> = on.0.iter().map(|(a, b, _)| (a.clone(), b.clone(), a == "reverb")).collect();
    assert_eq!(off.0, edges, "the same edges; the one out of the card is dashed");
    assert_eq!((off.1, off.2), (true, None), "off, and Turn on can run");
}

// ---------------------------------------------------------------- moved (Next D2)

/// Move `callee`'s effect before the one it is written around (`earlier`) or after the one around it.
fn moved(s: &mut Session, callee: &str, earlier: bool) -> Result<Edited, EditError> {
    let (at, id) = (rev(s), call(s, callee));
    reorder(s, at, &id, earlier)
}

// Next D2 (Q26): two effects written one inside the other swap: each keeps its own options, the
// sound they work on stays where it is, and the chain plays them the other way round. Either
// effect's row does it; undo puts the code back.
#[test]
fn two_nested_effects_swap() {
    let original = fx_song("delay(reverb(osc.saw(hz(110)), { size: 0.8 }), { time: 0.1 })");
    let swapped = fx_song("reverb(delay(osc.saw(hz(110)), { time: 0.1 }), { size: 0.8 })");
    let f = Fixture::new(&original);
    let mut s = f.open();
    assert_eq!(running(&s), ["std:saw", "@m2/effects:reverb", "@m2/effects:delay"]);
    let done = moved(&mut s, "delay", true).unwrap();
    assert_eq!(f.source(), swapped);
    assert_eq!((done.before.as_str(), done.after.as_str()), ("delay(reverb(…))", "reverb(delay(…))"));
    assert_eq!(running(&s), ["std:saw", "@m2/effects:delay", "@m2/effects:reverb"]);
    s.undo().unwrap();
    assert_eq!(f.source(), original);
    moved(&mut s, "reverb", false).unwrap();
    assert_eq!(f.source(), swapped, "Move right on the inner one is the same swap");
}

// Next D2: the options go with their effect, positional or in an object, or none; a call over
// lines keeps its line breaks where they were.
#[test]
fn the_options_go_with_their_effect() {
    for (body, callee, earlier, after) in [
        ("lowpass(delay(osc.saw(hz(110)), { time: 0.1 }), 800)", "lowpass", true, "delay(lowpass(osc.saw(hz(110)), 800), { time: 0.1 })"),
        ("delay(reverb(osc.saw(hz(110))))", "reverb", false, "reverb(delay(osc.saw(hz(110))))"),
        (
            "delay(reverb(osc.saw(hz(110)), { size: 0.8, bypass: true }))",
            "delay",
            true,
            "reverb(delay(osc.saw(hz(110))), { size: 0.8, bypass: true })",
        ),
        (
            "master(\n    delay(\n      reverb(osc.saw(hz(110)), { size: 0.8 }),\n      { time: 0.1 },\n    ),\n  )",
            "delay",
            true,
            "master(\n    reverb(\n      delay(osc.saw(hz(110)),\n      { time: 0.1 },\n    ), { size: 0.8 }),\n  )",
        ),
    ] {
        let original = fx_song(body);
        let f = Fixture::new(&original);
        let mut s = f.open();
        moved(&mut s, callee, earlier).unwrap_or_else(|e| panic!("{body}: {e}"));
        assert_eq!(f.source(), fx_song(after), "{body}");
        s.undo().unwrap();
        assert_eq!(f.source(), original, "{body}: undone");
    }
}

// Next D2 (Q26): effects next to each other in the sound but not written one inside the other
// can't swap here, and say so in words; so does the first effect's Move left and the last one's
// Move right. The menu's rows say the same.
#[test]
fn an_effect_not_next_to_another_is_refused_in_words() {
    let f = Fixture::new(&fx_song("{\n  const wet = reverb(osc.saw(hz(110)));\n  return delay(wet, { time: 0.1 });\n}"));
    let mut s = f.open();
    let words = |r: Result<Edited, EditError>| r.unwrap_err().to_string();
    let apart = "Reverb and Delay aren't next to each other in the code, so they can't swap here. Change their order in code.";
    assert!(words(moved(&mut s, "delay", true)).contains(apart));
    assert!(words(moved(&mut s, "reverb", false)).contains(apart));
    assert!(words(moved(&mut s, "reverb", true)).contains("Reverb is the first effect here."));
    assert!(words(moved(&mut s, "delay", false)).contains("Delay is the last effect here."));
    let view = crate::view::view(&s);
    let menu = |label: &str| view.modules[0].nodes.iter().find(|n| n.label == label).and_then(|n| n.menu.clone()).unwrap();
    assert_eq!(menu("delay").left.refused.as_deref(), Some(apart));
    assert_eq!(menu("reverb").right.refused.as_deref(), Some(apart));
    // Written one inside the other, the rows name the effect they swap with.
    let f = Fixture::new(&fx_song("delay(reverb(osc.saw(hz(110))))"));
    let s = f.open();
    let view = crate::view::view(&s);
    let menu = |label: &str| view.modules[0].nodes.iter().find(|n| n.label == label).and_then(|n| n.menu.clone()).unwrap();
    assert_eq!((menu("delay").left.with.as_deref(), menu("reverb").right.with.as_deref()), (Some("Reverb"), Some("Delay")));
    assert_eq!(menu("reverb").left.refused.as_deref(), Some("Reverb is the first effect here."));
}

// Next D2: moving one way and back is the code it was.
#[test]
fn moving_twice_is_the_same_code() {
    let original = fx_song("delay(reverb(osc.saw(hz(110)), { size: 0.8 }), { time: 0.1 })");
    let f = Fixture::new(&original);
    let mut s = f.open();
    moved(&mut s, "delay", true).unwrap();
    assert_ne!(f.source(), original);
    moved(&mut s, "delay", false).unwrap();
    assert_eq!(f.source(), original);
}

// ---------------------------------------------------------------- what a Duck listens to (Next D3)

/// The Duck call of `s` whose audio is `audio` (`bass.out`), as `file#ordinal`.
fn duck_on(s: &Session, audio: &str) -> String {
    let rev = s.accepted();
    let site = rev.compiled.sites.iter().find(|x| {
        x.callee == "duck"
            && x.args.first().is_some_and(|a| {
                let text = &rev.snapshot.files[Path::new(&x.file)];
                &text[a.span().0 as usize..a.span().1 as usize] == audio
            })
    });
    let site = site.unwrap_or_else(|| panic!("no duck on {audio}"));
    format!("{}#{}", site.file, site.ordinal)
}

fn listen(s: &mut Session, audio: &str, track: &str) -> Result<Edited, EditError> {
    let (at, id) = (rev(s), duck_on(s, audio));
    sidechain(s, at, &id, track)
}

// Next D3 (Q27, X28): a track with no `env` export is heard through `follow(<its>.out)`, and
// `follow` is imported from std/dsp; undo takes both back. The Duck's card says what it listens
// to, and offers the song's other tracks.
#[test]
fn a_track_with_no_env_gets_follow_of_its_out_and_the_import() {
    let f = crate::testing::fixture("templates/starter");
    let mut s = f.open();
    let original = f.read("song.ts");
    let card = |s: &Session| {
        let v = crate::view::view(s);
        let song = v.modules.iter().find(|m| m.key == "song").unwrap();
        let id = duck_on(s, "bass.out");
        song.nodes.iter().find(|n| n.id == id).and_then(|n| n.listens.clone()).expect("the Duck's Listens to row")
    };
    let was = card(&s);
    assert_eq!(was.track.as_deref(), Some("kick"));
    let keys: Vec<&str> = was.choices.iter().map(|(k, _)| k.as_str()).collect();
    assert!(keys.contains(&"clap") && keys.contains(&"kick") && !keys.contains(&"bass"), "{keys:?}");
    assert!(was.port.starts_with("in:"), "{}", was.port);
    let done = listen(&mut s, "bass.out", "clap").unwrap();
    assert_eq!((done.before.as_str(), done.after.as_str()), ("kick.env", "follow(clap.out)"));
    let expected = original
        .replace(
            "import { compress, duck } from \"std/fx\";\n",
            "import { compress, duck } from \"std/fx\";\nimport { follow } from \"std/dsp\";\n",
        )
        .replace("duck(bass.out, kick.env,", "duck(bass.out, follow(clap.out),");
    let now = f.read("song.ts");
    assert!(
        now.contains("duck(bass.out, follow(clap.out), { depth: db(-9) })") && now.contains("import { follow } from \"std/dsp\";"),
        "{now}"
    );
    assert_eq!(now.replace("import { follow } from \"std/dsp\";\n", ""), expected.replace("import { follow } from \"std/dsp\";\n", ""));
    assert_eq!(card(&s).track.as_deref(), Some("clap"));
    s.undo().unwrap();
    assert_eq!(f.read("song.ts"), original);
}

// Next D3 (X28): a track that exports `env` is heard through it, as the starter's kick is.
#[test]
fn the_picker_writes_the_tracks_env() {
    let f = crate::testing::fixture("templates/starter");
    let mut s = f.open();
    let original = f.read("song.ts");
    listen(&mut s, "chords.out", "hats").unwrap();
    assert!(f.read("song.ts").contains("duck(chords.out, follow(hats.out), { depth: db(-6) })"));
    let done = listen(&mut s, "chords.out", "kick").unwrap();
    assert_eq!(done.after, "kick.env");
    assert!(f.read("song.ts").contains("duck(chords.out, kick.env, { depth: db(-6) })"));
    let e = listen(&mut s, "chords.out", "kick").unwrap_err().to_string();
    assert!(e.contains("Duck listens to Kick already."), "{e}");
    s.undo().unwrap();
    s.undo().unwrap();
    assert_eq!(f.read("song.ts"), original);
}

// Next D3 (Q27): a Duck in a track's file can't hear other tracks: refused in words, and its card
// says so.
#[test]
fn a_duck_in_a_track_file_is_refused_in_words() {
    let f = crate::testing::fixture("templates/starter");
    let bass = f.read("tracks/bass.ts");
    f.write(
        "tracks/bass.ts",
        bass.replace("import { bass } from \"@m2/synths\";", "import { bass } from \"@m2/synths\";\nimport { duck } from \"std/fx\";")
            .replace(
                "({ lane }) => bass(lane.notes(\"notes\"), { cutoff: hz(420), drive: 3 })",
                "({ lane }) => duck(bass(lane.notes(\"notes\"), { cutoff: hz(420), drive: 3 }), lane.mask(\"pump\"))",
            ),
    );
    let mut s = f.open();
    let id = s
        .accepted()
        .compiled
        .sites
        .iter()
        .find(|x| x.callee == "duck" && x.file == "tracks/bass.ts")
        .map(|x| format!("{}#{}", x.file, x.ordinal))
        .unwrap();
    let words = "Bass's Duck is in tracks/bass.ts, which can't hear other tracks. Change it in code.";
    let at = rev(&s);
    let e = sidechain(&mut s, at, &id, "kick").unwrap_err().to_string();
    assert!(e.contains(words), "{e}");
    let v = crate::view::view(&s);
    let card = v.modules.iter().find(|m| m.key == "bass").and_then(|m| m.nodes.iter().find(|n| n.id == id)).unwrap();
    assert_eq!(card.listens.as_ref().and_then(|l| l.refused.as_deref()), Some(words));
}
