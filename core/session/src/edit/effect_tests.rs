//! Effects wrap a call where it is written, and unwrap it again, verified against the graph.

use std::path::{Path, PathBuf};

use super::*;

struct Fixture(PathBuf);

impl Fixture {
    fn new(source: &str) -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("m2-effect-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
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
    let site = s.accepted().compiled.sites.iter().find(|x| x.in_project && x.callee == callee).unwrap_or_else(|| panic!("no call `{callee}`"));
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

    let added = s.add_effect(rev(&s), &call(&s, "osc.saw"), "std/fx", "delay").unwrap();
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
    let readers: Vec<String> = s.accepted().compiled.ir.modules[0].nodes.iter().filter_map(|n| match &n.op {
        Op::Kernel { kernel, .. } => Some(kernel.clone()),
        _ => None,
    }).collect();
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
    s.add_effect(rev(&s), &call(&s, "osc.sine"), "std/fx", "limit").unwrap();
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
    let original = format!(
        "{HEAD}const delay = (x: any) => osc.sine(hz(440));\nexport default song(() => delay(osc.saw(hz(110))));\n"
    );
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
    assert!(s.add_effect(rev(&s), &hz, "std/fx", "delay").is_err());
    assert!(matches!(s.add_effect(rev(&s), &call(&s, "osc.sine"), "std/fx", "wobble"), Err(EditError::NotFound(_))));
    assert_eq!(fixture.source(), original);
}

#[test]
fn effects_are_listed_with_their_category_and_args() {
    let fixture = Fixture::new(&format!("{HEAD}export default song(() => osc.saw(hz(110)));\n"));
    let effects = fixture.open().effects();
    let lowpass = effects.iter().find(|e| e.name == "lowpass").unwrap();
    assert_eq!((lowpass.module.as_str(), lowpass.category.as_str(), lowpass.args.as_deref()), ("std/filter", "Filter", Some("1000")));
    let delay = effects.iter().find(|e| e.name == "delay").unwrap();
    assert_eq!(delay.category, "Delay & Reverb");
    assert!(effects.iter().all(|e| m2_runtime::packages::EFFECT_CATEGORIES.contains(&e.category.as_str())));
}

#[test]
fn an_effects_args_are_written_after_the_audio_as_knobs_and_removing_restores_the_graph() {
    let original = format!("{HEAD}export default song(() => osc.saw(hz(110)));\n");
    let fixture = Fixture::new(&original);
    let mut s = fixture.open();
    let graph = canonical(&s.accepted().compiled.ir);

    let added = s.add_effect(rev(&s), &call(&s, "osc.saw"), "std/filter", "lowpass").unwrap();
    assert_eq!(added.after, "lowpass(osc.saw(hz(110)), 1000)");
    assert!(fixture.source().contains("import { lowpass } from \"std/filter\";\n"));
    // The cutoff is a knob on the filter, typed by the port it feeds.
    let view = s.view();
    let filter = view.modules[0].nodes.iter().find(|n| n.label == "lowpass").unwrap();
    assert_eq!(filter.effect.as_deref(), Some("lowpass"));
    let cutoff = filter.knobs.iter().find(|k| k.label == "cutoff").unwrap_or_else(|| panic!("{:?}", filter.knobs));
    assert_eq!((cutoff.value, cutoff.unit, cutoff.log), (1000.0, Some("hz"), true));

    s.remove_effect(rev(&s), &call(&s, "lowpass")).unwrap();
    assert!(fixture.source().contains("song(() => osc.saw(hz(110)))"));
    assert_eq!(canonical(&s.accepted().compiled.ir), graph);
    s.undo().unwrap();
    s.undo().unwrap();
    assert_eq!(fixture.source(), original);
}

#[test]
fn every_effect_goes_after_a_sound_and_every_number_in_its_args_is_a_knob() {
    for effect in Fixture::new(&format!("{HEAD}export default song(() => osc.saw(hz(110)));\n")).open().effects() {
        let fixture = Fixture::new(&format!("{HEAD}export default song(() => osc.saw(hz(110)));\n"));
        let mut s = fixture.open();
        let added = s
            .add_effect(rev(&s), &call(&s, "osc.saw"), &effect.module, &effect.name)
            .unwrap_or_else(|e| panic!("{}.{}: {e}", effect.module, effect.name));
        let args = effect.args.as_deref().unwrap_or("");
        let numbers = args.split(|c: char| !(c.is_ascii_digit() || c == '.')).filter(|w| !w.is_empty()).count();
        let nodes = &s.view().modules[0].nodes;
        let knobs: usize = nodes.iter().filter(|n| n.effect.as_deref() == Some(&effect.name)).map(|n| n.knobs.len()).sum();
        assert_eq!(knobs, numbers, "{}: every number in `{args}` is a knob ({})", effect.name, added.after);
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
        s.add_effect(rev(&s), &call(&s, "osc.saw"), &effect.module, &effect.name)
            .unwrap_or_else(|e| panic!("{}.{} before a pan: {e}", effect.module, effect.name));
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
    let added = s.add_effect(rev(&s), &call(&s, "osc.saw"), "std/fx", "chorus").unwrap();
    assert_eq!(added.after, "chorus(osc.saw(n.pitch), { rate: 0.8, depth: 0.5, mix: 0.5 })");
    assert!(fixture.source().contains("vca(svf.lowpass(chorus(osc.saw(n.pitch), { rate: 0.8, depth: 0.5, mix: 0.5 }), 800)"));
    let view = s.view();
    let chorus = view.modules[0].nodes.iter().find(|n| n.effect.as_deref() == Some("chorus")).expect("the effect can be removed");
    s.remove_effect(rev(&s), &chorus.id.clone()).unwrap();
    assert!(fixture.source().contains("vca(svf.lowpass(osc.saw(n.pitch), 800)"));
    assert_eq!(canonical(&s.accepted().compiled.ir), graph);
}
