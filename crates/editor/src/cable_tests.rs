//! Cables are names in code. Connecting and disconnecting each change one input, are
//! verified against the graph, and leave the rest of the file as it was.

use std::path::{Path, PathBuf};

use m2_session::{Refusal, StepError};

use super::*;
use crate::Gestures;
use crate::controls::Control;
use m2_runtime::PackageSet;

struct Fixture(PathBuf);

impl Fixture {
    fn new(body: &str) -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("m2-cable-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("music2.toml"), crate::testing::MANIFEST).unwrap();
        let fixture = Self(dir);
        fixture.write(body);
        fixture
    }

    fn write(&self, body: &str) {
        let source = format!(
            r#"import {{ hz, khz, song }} from "@m2/core";
import {{ lfo }} from "std/control";
import {{ osc, svf }} from "std/dsp";
import {{ add, xrange }} from "std/math";
{body}
"#
        );
        std::fs::write(self.0.join("song.ts"), source).unwrap();
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

const WOBBLE: &str = "const wobble = xrange(lfo(time.beats, { period: 0.5 }), hz(200), khz(2.5));";

fn outlet(s: &Session, name: &str) -> Outlet {
    s.outlets().into_iter().find(|o| o.name == name).unwrap_or_else(|| panic!("no outlet `{name}`"))
}

fn control(s: &Session, literal: &str) -> Control {
    s.controls().into_iter().find(|c| c.literal == literal).unwrap_or_else(|| panic!("no control `{literal}`"))
}

/// The kernel input that reads `port` in the song module, as `kernel.input`.
fn readers(s: &Session, port: &str) -> Vec<String> {
    let mut found = vec![];
    for m in &s.accepted().compiled.ir.modules {
        walk(&m.nodes, &mut |n| {
            if let Op::Kernel { kernel, inputs, .. } = &n.op {
                found.extend(inputs.iter().filter(|(_, r)| *r == port).map(|(p, _)| format!("{kernel}.{p}")));
            }
        });
    }
    found
}

#[test]
fn connecting_writes_the_name_in_place_of_the_value_and_rewires_only_that_input() {
    let fixture = Fixture::new(&format!(
        "export default song(({{ time }}) => {{\n  {WOBBLE}\n  return add(osc.sine(hz(800)), osc.saw(hz(55)));\n}});"
    ));
    let mut s = fixture.open();
    let before = fixture.source();
    let (wobble, freq) = (outlet(&s, "wobble"), control(&s, "800"));
    let edited = s.connect(wobble.revision, &wobble.id, &freq.id).unwrap();
    assert_eq!((edited.before.as_str(), edited.after.as_str(), edited.revision), ("hz(800)", "wobble", 2));
    assert_eq!(fixture.source(), before.replace("osc.sine(hz(800))", "osc.sine(wobble)"));
    assert_eq!(readers(&s, &outlet(&s, "wobble").port), ["std:sine.freq"]);
    assert!(s.controls().iter().any(|c| c.literal == "55"), "the other oscillator keeps its value");
}

#[test]
fn a_plain_number_control_can_be_connected() {
    let fixture = Fixture::new(&format!(
        "export default song(({{ time }}) => {{\n  {WOBBLE}\n  const level = lfo(time.beats);\n  return svf.lowpass(osc.saw(hz(55)), wobble, 0.7);\n}});"
    ));
    let mut s = fixture.open();
    let (level, q) = (outlet(&s, "level"), control(&s, "0.7"));
    let edited = s.connect(level.revision, &level.id, &q.id).unwrap();
    assert_eq!((edited.before.as_str(), edited.after.as_str()), ("0.7", "level"));
    assert!(fixture.source().contains("svf.lowpass(osc.saw(hz(55)), wobble, level)"));
}

#[test]
fn disconnecting_writes_the_declared_default_and_undo_restores_the_cable() {
    let fixture =
        Fixture::new(&format!("export default song(({{ time }}) => {{\n  {WOBBLE}\n  return svf.lowpass(osc.saw(hz(55)), wobble);\n}});"));
    let mut s = fixture.open();
    let before = fixture.source();
    let inlet = s.inlets().into_iter().find(|i| i.name == "wobble").expect("the cutoff is connected");
    assert_eq!((inlet.path.as_slice(), inlet.from.as_str()), (&["1".to_string()][..], outlet(&s, "wobble").id.as_str()));
    let edited = s.disconnect(inlet.revision, &inlet.id).unwrap();
    // std:svf declares `cutoff = 1000`, in hz, and the file imports `hz`.
    assert_eq!((edited.before.as_str(), edited.after.as_str()), ("wobble", "hz(1000)"));
    assert_eq!(fixture.source(), before.replace("svf.lowpass(osc.saw(hz(55)), wobble)", "svf.lowpass(osc.saw(hz(55)), hz(1000))"));
    assert!(s.inlets().is_empty());
    assert_eq!(control(&s, "1000").value, 1000.0);

    s.undo().unwrap();
    assert_eq!(fixture.source(), before);
    assert_eq!(s.inlets().len(), 1);
}

#[test]
fn connect_then_disconnect_is_an_exact_round_trip_through_the_graph() {
    let fixture = Fixture::new(&format!(
        "export default song(({{ time }}) => {{\n  {WOBBLE}\n  return svf.lowpass(osc.saw(hz(55)), hz(1000));\n}});"
    ));
    let mut s = fixture.open();
    let before = fixture.source();
    let original = canonical(&s.accepted().compiled.ir);
    let (wobble, cutoff) = (outlet(&s, "wobble"), control(&s, "1000"));
    s.connect(wobble.revision, &wobble.id, &cutoff.id).unwrap();
    let inlet = s.inlets().pop().unwrap();
    s.disconnect(inlet.revision, &inlet.id).unwrap();
    assert_eq!(fixture.source(), before);
    assert_eq!(canonical(&s.accepted().compiled.ir), original);
}

#[test]
fn undo_of_a_connection_restores_the_value() {
    let fixture = Fixture::new(&format!("export default song(({{ time }}) => {{\n  {WOBBLE}\n  return osc.sine(khz(0.8));\n}});"));
    let mut s = fixture.open();
    let before = fixture.source();
    let (wobble, freq) = (outlet(&s, "wobble"), control(&s, "0.8"));
    s.connect(wobble.revision, &wobble.id, &freq.id).unwrap();
    assert!(fixture.source().contains("osc.sine(wobble)"));
    let undone = s.undo().unwrap();
    assert_eq!((undone.before.as_str(), undone.after.as_str()), ("wobble", "khz(0.8)"));
    assert_eq!(fixture.source(), before);
    assert_eq!(control(&s, "0.8").value, 800.0);
}

#[test]
fn a_cable_carries_past_an_unrelated_save() {
    let fixture = Fixture::new(&format!("export default song(({{ time }}) => {{\n  {WOBBLE}\n  return osc.sine(hz(800));\n}});"));
    let mut s = fixture.open();
    let (wobble, freq) = (outlet(&s, "wobble"), control(&s, "800"));
    let saved = fixture.source().replacen("import", "// A comment saved elsewhere.\nimport", 1);
    std::fs::write(fixture.0.join("song.ts"), &saved).unwrap();
    // Taken in (a gesture never reads the folder itself, R2); the ids are carried past it.
    assert!(s.refresh());
    s.connect(wobble.revision, &wobble.id, &freq.id).unwrap();
    assert_eq!(fixture.source(), saved.replace("osc.sine(hz(800))", "osc.sine(wobble)"));
}

#[test]
fn connecting_refuses_what_would_not_be_one_honest_input_change() {
    // Declared after the value: the name would not exist there yet.
    let fixture = Fixture::new(&format!(
        "export default song(({{ time }}) => {{\n  const tone = osc.sine(hz(800));\n  {WOBBLE}\n  return add(tone, osc.saw(wobble));\n}});"
    ));
    let mut s = fixture.open();
    let (wobble, freq) = (outlet(&s, "wobble"), control(&s, "800"));
    assert!(matches!(s.connect(wobble.revision, &wobble.id, &freq.id), Err(EditError::Rejected(m)) if m.contains("declared after")));

    // Into its own declaration.
    let lo = control(&s, "200");
    assert!(matches!(s.connect(wobble.revision, &wobble.id, &lo.id), Err(EditError::Rejected(_))));

    // An audio signal into a frequency: the build refuses the kinds.
    let fixture = Fixture::new("export default song(() => {\n  const tone = osc.saw(hz(55));\n  return osc.sine(hz(800));\n});");
    let mut s = fixture.open();
    let (tone, freq) = (outlet(&s, "tone"), control(&s, "800"));
    let before = fixture.source();
    assert!(matches!(s.connect(tone.revision, &tone.id, &freq.id), Err(EditError::Rejected(_))));
    assert_eq!(fixture.source(), before);
    assert_eq!(s.accepted().number, 1);
}

#[test]
fn only_const_bindings_of_one_top_level_signal_are_outlets() {
    let fixture = Fixture::new(
        "export default song(({ time }) => {\n  let changing = lfo(time.beats);\n  const fixed = lfo(time.beats);\n  const tones = [1, 2].map(() => osc.sine(hz(800)));\n  return add(osc.sine(xrange(fixed, hz(100), hz(200))), osc.saw(xrange(changing, hz(100), hz(200))));\n});",
    );
    let s = fixture.open();
    let names: Vec<String> = s.outlets().into_iter().map(|o| o.name).collect();
    assert_eq!(names, ["fixed"]);
}

#[test]
fn an_input_without_a_declared_default_stays_connected() {
    // xrange's `lo` reaches std:xrange, whose ports declare no default.
    let fixture = Fixture::new(
        "export default song(({ time }) => {\n  const low = xrange(lfo(time.beats), hz(50), hz(100));\n  const sweep = xrange(lfo(time.beats), low, hz(900));\n  return osc.sine(sweep);\n});",
    );
    let mut s = fixture.open();
    let before = fixture.source();
    let inlet = s.inlets().into_iter().find(|i| i.name == "low").unwrap();
    assert!(matches!(s.disconnect(inlet.revision, &inlet.id), Err(EditError::Rejected(m)) if m.contains("no default")));
    assert_eq!(fixture.source(), before);
}

#[test]
fn a_component_that_builds_differently_for_a_signal_is_refused_by_the_graph_check() {
    // osc.sine converts a pitch with an extra node: the candidate compiles, but it is
    // not the old graph with one input rewired, so only the comparison can catch it.
    let fixture = Fixture::new(
        "import { semis } from \"@m2/core\";\nimport { range } from \"std/math\";\nexport default song(({ time }) => {\n  const pitch = range(lfo(time.beats), semis(48), semis(60));\n  return osc.sine(hz(800));\n});",
    );
    let mut s = fixture.open();
    let before = fixture.source();
    let (pitch, freq) = (outlet(&s, "pitch"), control(&s, "800"));
    let refused = s.connect(pitch.revision, &pitch.id, &freq.id);
    assert!(matches!(&refused, Err(EditError::Rejected(m)) if m.contains("would change more")), "{refused:?}");
    assert_eq!(fixture.source(), before);
}

#[test]
fn undo_and_redo_walk_values_and_cables_back_and_forth() {
    let fixture = Fixture::new(&format!("export default song(({{ time }}) => {{\n  {WOBBLE}\n  return osc.sine(hz(800));\n}});"));
    let mut s = fixture.open();
    let start = fixture.source();
    let freq = control(&s, "800");
    s.set(freq.revision, &freq.id, 900.0, Some(800.0)).unwrap();
    let tuned = fixture.source();
    let (wobble, freq) = (outlet(&s, "wobble"), control(&s, "900"));
    s.connect(wobble.revision, &wobble.id, &freq.id).unwrap();
    let wired = fixture.source();
    assert!(!s.can_redo());

    s.undo().unwrap();
    assert_eq!(fixture.source(), tuned);
    s.undo().unwrap();
    assert_eq!(fixture.source(), start);
    assert!(!s.can_undo() && s.can_redo());
    assert!(matches!(s.undo(), Err(StepError::Nothing(_))));

    let redone = s.redo().unwrap();
    assert_eq!((redone.before.as_str(), redone.after.as_str()), ("800", "900"));
    assert_eq!(fixture.source(), tuned);
    s.redo().unwrap();
    assert_eq!(fixture.source(), wired);
    assert!(!s.can_redo());

    // Undo again, then a new edit: the undone change can no longer be redone.
    s.undo().unwrap();
    let freq = control(&s, "900");
    s.set(freq.revision, &freq.id, 950.0, Some(900.0)).unwrap();
    assert!(!s.can_redo());
    assert!(matches!(s.redo(), Err(StepError::Nothing(_))));
    s.undo().unwrap();
    s.undo().unwrap();
    assert_eq!(fixture.source(), start);
}

// WRITE-21 (the step 6 design's 20.1): undo is located by the cable's carried bytes, so only a
// change to those bytes blocks it: refused, not offered, and offered again once taken back.
#[test]
fn a_cable_undo_refused_after_another_change_waits_until_that_change_is_taken_back() {
    let fixture = Fixture::new(&format!("export default song(({{ time }}) => {{\n  {WOBBLE}\n  return osc.sine(hz(800));\n}});"));
    let mut s = fixture.open();
    let (wobble, freq) = (outlet(&s, "wobble"), control(&s, "800"));
    s.connect(wobble.revision, &wobble.id, &freq.id).unwrap();
    let wired = fixture.source();
    // Someone saves another value over the cable's own bytes: undoing would overwrite it, so
    // undo refuses, says it can't, and keeps the change.
    let saved = wired.replacen("osc.sine(wobble)", "osc.sine(hz(700))", 1);
    std::fs::write(fixture.0.join("song.ts"), &saved).unwrap();
    assert!(s.refresh());
    assert!(matches!(s.undo(), Err(StepError::Refused(Refusal::Conflict { .. }))));
    assert!(!s.can_undo(), "undo isn't offered while it would be refused");
    assert_eq!(fixture.source(), saved);
    // Taken back, the file is as the cable left it: undo is offered and works again.
    std::fs::write(fixture.0.join("song.ts"), &wired).unwrap();
    s.refresh();
    assert!(s.can_undo());
    s.undo().unwrap();
    assert!(fixture.source().contains("osc.sine(hz(800))"));
}

// WRITE-21: an unrelated change elsewhere in the same file never blocks a cable's undo, which
// keeps it.
#[test]
fn an_unrelated_change_in_the_file_never_blocks_a_cable_undo() {
    let fixture = Fixture::new(&format!("export default song(({{ time }}) => {{\n  {WOBBLE}\n  return osc.sine(hz(800));\n}});"));
    let mut s = fixture.open();
    let (wobble, freq) = (outlet(&s, "wobble"), control(&s, "800"));
    s.connect(wobble.revision, &wobble.id, &freq.id).unwrap();
    let saved = fixture.source().replacen("import", "// Saved elsewhere.\nimport", 1);
    std::fs::write(fixture.0.join("song.ts"), &saved).unwrap();
    assert!(s.refresh());
    assert!(s.can_undo());
    s.undo().unwrap();
    let now = fixture.source();
    assert!(now.contains("osc.sine(hz(800))") && now.starts_with("// Saved elsewhere.\n"), "{now}");
}

#[test]
fn a_save_in_another_file_never_blocks_undo() {
    let fixture = Fixture::new(&format!("export default song(({{ time }}) => {{\n  {WOBBLE}\n  return osc.sine(hz(800));\n}});"));
    std::fs::create_dir_all(fixture.0.join("data")).unwrap();
    let mut s = fixture.open();
    let (wobble, freq) = (outlet(&s, "wobble"), control(&s, "800"));
    s.connect(wobble.revision, &wobble.id, &freq.id).unwrap();
    // A file the cable didn't touch changes (as a lane write or a save elsewhere would).
    std::fs::write(fixture.0.join("data/arrangement.json"), r#"{"tempo": [{"beat": 0, "bpm": 90}]}"#).unwrap();
    s.refresh();
    assert!(s.can_undo());
    s.undo().unwrap();
    assert!(fixture.source().contains("osc.sine(hz(800))"));
    assert!(std::fs::read_to_string(fixture.0.join("data/arrangement.json")).unwrap().contains("90"), "the other file keeps its change");
    s.redo().unwrap();
    assert!(fixture.source().contains("osc.sine(wobble)"));
}

#[test]
fn cables_and_knobs_in_a_track_file_are_found_by_their_ids() {
    // Ids name the file, `tracks/lead.ts#3/1`: the `/` in its path is not the argument's.
    let fixture = Fixture::new("import lead from \"./tracks/lead\";\nexport default song(() => lead.out);");
    std::fs::create_dir_all(fixture.0.join("tracks")).unwrap();
    let track = format!(
        "import {{ hz, khz, track }} from \"@m2/core\";\nimport {{ lfo }} from \"std/control\";\nimport {{ osc, svf }} from \"std/dsp\";\n\
         import {{ xrange }} from \"std/math\";\n\nexport default track(\"lead\", ({{ time }}) => {{\n  {WOBBLE}\n  \
         return svf.lowpass(osc.saw(hz(55)), wobble);\n}});\n"
    );
    std::fs::write(fixture.0.join("tracks/lead.ts"), &track).unwrap();
    let mut s = fixture.open();
    let inlet = s.inlets().into_iter().find(|i| i.name == "wobble").expect("the cutoff is connected");
    assert!(inlet.id.starts_with("tracks/lead.ts#"), "{}", inlet.id);
    s.disconnect(inlet.revision, &inlet.id).unwrap();
    let read = || std::fs::read_to_string(fixture.0.join("tracks/lead.ts")).unwrap();
    assert_eq!(read(), track.replace("osc.saw(hz(55)), wobble)", "osc.saw(hz(55)), hz(1000))"));

    // A knob read before an unrelated save still finds its value after it.
    let freq = control(&s, "55");
    let saved = read().replacen("import", "// Saved elsewhere.\nimport", 1);
    std::fs::write(fixture.0.join("tracks/lead.ts"), &saved).unwrap();
    s.set(freq.revision, &freq.id, 110.0, Some(55.0)).unwrap();
    assert_eq!(read(), saved.replace("hz(55)", "hz(110)"));
}
