//! The canvas may edit only an explicit source literal, and must display the
//! boundary when a value is calculated or its ownership is ambiguous.

use std::path::{Path, PathBuf};

use super::*;
use crate::controls::walk;
use m2_ir::Op;
use m2_runtime::PackageSet;
use m2_session::{Session, StepError};

struct ProjectFixture(PathBuf);

impl ProjectFixture {
    fn new(source: &str) -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "m2-restricted-edit-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
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

impl Drop for ProjectFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn oscillator(expression: &str) -> String {
    format!(
        r#"import {{ hz, khz, song }} from "@m2/core";
import {{ osc }} from "std/dsp";
export default song(() => osc.sine({expression}));
"#
    )
}

#[test]
fn calculated_arguments_compile_but_are_visible_as_readonly() {
    for expression in ["hz(400 * 2)", "400 * 2"] {
        let fixture = ProjectFixture::new(&oscillator(expression));
        let session = fixture.open();
        assert!(session.controls().is_empty(), "{expression} has no independently owned literal");
        let view = session.view();
        assert!(view.modules.iter().flat_map(|m| &m.nodes).all(|n| n.knobs.is_empty()));
        let rows: Vec<_> = view.modules.iter().flat_map(|m| &m.nodes).flat_map(|n| &n.readonly).collect();
        assert!(
            rows.iter().any(|r| r.expression.contains("400 * 2") && !r.reason.is_empty()),
            "the canvas explains {expression}: {rows:?}"
        );
    }
}

#[test]
fn object_literals_are_editable_only_when_the_property_has_one_explicit_owner() {
    let source = |argument: &str| {
        format!(
            r#"import {{ component, kernel, song }} from "@m2/core";
const tone = component({{ controls: ["0.cutoff"] }}, (o) => kernel("std:sine", {{ freq: o.cutoff }}));
export default song(() => tone({argument}));
"#
        )
    };

    let direct = ProjectFixture::new(&source("{ cutoff: 800 }"));
    assert_eq!(direct.open().controls().len(), 1, "an ordinary property owns a writable literal");

    for argument in ["{ cutoff: 800, ...{} }", "{ cutoff: 800, cutoff: 800 }", "{ ['cutoff']: 800 }", "{ get cutoff() { return 800; } }"] {
        let fixture = ProjectFixture::new(&source(argument));
        let session = fixture.open();
        assert!(session.controls().is_empty(), "the canvas must not guess ownership in {argument}");
        let view = session.view();
        assert!(view.modules.iter().flat_map(|m| &m.nodes).all(|n| n.knobs.is_empty()));
        assert!(view.modules.iter().flat_map(|m| &m.nodes).any(|n| !n.readonly.is_empty()), "{argument} stays visible with an explanation");
    }
}

const SHARED_CUTOFF: &str = r#"import { hz, song } from "@m2/core";
import { osc } from "std/dsp";
import { add } from "std/math";
export default song(() => {
  const cutoff = hz(800);
  const left = osc.sine(cutoff);
  const right = osc.saw(cutoff);
  return add(left, right);
});
"#;

#[test]
fn a_shared_value_has_one_knob_and_visible_connections_to_both_consumers() {
    let fixture = ProjectFixture::new(SHARED_CUTOFF);
    let session = fixture.open();
    let controls = session.controls();
    assert_eq!(controls.len(), 1);
    assert_eq!(controls[0].uses, 2);

    let view = session.view();
    let module = view.modules.iter().find(|m| m.key == "song").unwrap();
    let shared: Vec<_> = module.nodes.iter().filter(|n| n.kind == "value" && n.label == "cutoff").collect();
    assert_eq!(shared.len(), 1, "the shared declaration is shown once: {:?}", module.nodes);
    let shared = shared[0];
    assert_eq!(shared.knobs.len(), 1);
    assert_eq!(shared.knobs[0].id, controls[0].id);
    assert_eq!(shared.knobs[0].uses, 2);

    for label in ["osc.sine", "osc.saw"] {
        let consumer = module.nodes.iter().find(|n| n.label == label).unwrap();
        assert!(consumer.knobs.is_empty(), "{label} must not look independently adjustable");
        assert!(module.edges.contains(&(shared.id.clone(), consumer.id.clone())), "the shared value visibly feeds {label}");
    }
}

#[test]
fn editing_a_shared_control_changes_its_one_declaration_and_all_its_uses() {
    let fixture = ProjectFixture::new(SHARED_CUTOFF);
    let mut session = fixture.open();
    let control = session.controls().into_iter().next().unwrap();
    session.set(control.revision, &control.id, 950.0, Some(800.0)).unwrap();
    assert_eq!(fixture.source(), SHARED_CUTOFF.replace("hz(800)", "hz(950)"));

    let mut values = vec![];
    for module in &session.accepted().compiled.ir.modules {
        walk(&module.nodes, &mut |node| {
            if let (Op::Const { value, .. }, Some(origin)) = (&node.op, &node.origin)
                && origin.same_value(&control.origin)
            {
                values.push(*value);
            }
        });
    }
    assert_eq!(values, vec![950.0, 950.0], "both graph values come from the same accepted edit");
    session.undo().unwrap();
    assert_eq!(fixture.source(), SHARED_CUTOFF);
}

#[test]
fn editing_a_parenthesized_literal_keeps_its_comments_and_surrounding_syntax() {
    let original = oscillator("hz((/* tuning */ 800 /* keep this */))");
    let fixture = ProjectFixture::new(&original);
    let mut session = fixture.open();
    let control = session.controls().into_iter().next().unwrap();
    assert_eq!(control.literal, "800", "the source lens owns the number token");
    session.set(control.revision, &control.id, 950.0, Some(800.0)).unwrap();
    assert_eq!(fixture.source(), original.replace("800", "950"));
    session.undo().unwrap();
    assert_eq!(fixture.source(), original);
}

#[test]
fn setting_the_current_value_preserves_its_spelling_without_recording_an_edit() {
    for (spelling, value) in [("0x320", 800.0), ("8e2", 800.0), ("8_00", 800.0), ("-0", 0.0)] {
        let original = oscillator(&format!("hz({spelling})"));
        let fixture = ProjectFixture::new(&original);
        let mut session = fixture.open();
        let control = session.controls().into_iter().next().unwrap();

        let committed = session.set(control.revision, &control.id, value, Some(value)).unwrap();
        assert_eq!(committed.before, spelling);
        assert_eq!(committed.after, spelling);
        assert_eq!(committed.revision, control.revision);
        assert_eq!(session.accepted().number, control.revision);
        assert_eq!(fixture.source(), original);
        assert!(matches!(session.undo(), Err(StepError::Nothing(_))), "{spelling} did not create an undo entry");
    }
}

#[test]
fn audition_and_commit_use_the_same_unit_conversion_without_preview_writing_code() {
    let original = oscillator("khz(0.8)");
    let fixture = ProjectFixture::new(&original);
    let mut session = fixture.open();
    let control = session.controls().into_iter().next().unwrap();
    let audition = session.preview(control.revision, &control.id, 950.0).unwrap();
    let (origin, preview_value) = (audition.origin, audition.value);
    assert_eq!(origin, control.origin);
    assert_eq!(audition.current, 800.0);
    assert_eq!(preview_value, 950.0);
    assert_eq!(fixture.source(), original);
    assert_eq!(session.accepted().number, control.revision);
    assert_eq!(session.controls()[0].value, 800.0);

    let committed = session.set(control.revision, &control.id, 950.0, Some(800.0)).unwrap();
    assert_eq!(committed.value, preview_value);
    assert_eq!(fixture.source(), original.replace("khz(0.8)", "khz(0.95)"));
}

#[test]
fn audition_refuses_stale_controls_invalid_drafts_and_nonfinite_values() {
    let original = oscillator("hz(800)");
    let fixture = ProjectFixture::new(&original);
    let mut session = fixture.open();
    let control = session.controls().into_iter().next().unwrap();
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e100] {
        assert!(matches!(session.preview(control.revision, &control.id, value), Err(EditError::Rejected(_))));
    }
    assert!(matches!(session.preview(control.revision, "song.ts#999/0", 950.0), Err(EditError::NotFound(_))));
    assert_eq!(fixture.source(), original);

    // An unrelated save carries the drag forward to the new revision.
    std::fs::write(fixture.0.join("song.ts"), format!("// Saved elsewhere.\n{original}")).unwrap();
    assert!(session.refresh());
    let carried = session.preview(control.revision, &control.id, 950.0).unwrap();
    let current = session.controls().into_iter().next().unwrap();
    assert_eq!((carried.id.as_str(), &carried.origin, carried.value), (current.id.as_str(), &current.origin, 950.0));

    // A save that touches the control's own call makes the old drag stale.
    std::fs::write(fixture.0.join("song.ts"), format!("// Saved elsewhere.\n{}", original.replace("hz(800)", "hz(801)"))).unwrap();
    assert!(session.refresh());
    assert_eq!(session.preview(control.revision, &control.id, 950.0), Err(EditError::Stale { current: 3 }));
    let current = session.controls().into_iter().next().unwrap();
    std::fs::write(fixture.0.join("song.ts"), "(").unwrap();
    assert!(session.refresh());
    assert!(matches!(session.preview(current.revision, &current.id, 950.0), Err(EditError::DraftInvalid(_))));
    assert_eq!(session.accepted().number, 3);
    assert_eq!(session.controls()[0].value, 801.0);
}

#[test]
fn an_unrelated_non_ascii_comment_edit_can_carry_a_control_to_the_new_revision() {
    let original = format!("// é\n{}", oscillator("hz(800)"));
    let fixture = ProjectFixture::new(&original);
    let mut session = fixture.open();
    let control = session.controls().into_iter().next().unwrap();

    // These characters share a first UTF-8 byte. A common byte prefix need not
    // end at a string boundary, even though the control's own span still does.
    let changed = original.replacen("é", "ê", 1);
    std::fs::write(fixture.0.join("song.ts"), &changed).unwrap();
    session.set(control.revision, &control.id, 950.0, Some(800.0)).unwrap();
    assert_eq!(fixture.source(), changed.replace("hz(800)", "hz(950)"));
}

fn song(body: &str) -> String {
    format!(
        r#"import {{ hz, semis, song }} from "@m2/core";
import {{ osc }} from "std/dsp";
import {{ add, mtof, scale }} from "std/math";
{body}
"#
    )
}

#[test]
fn graph_values_cannot_escape_into_javascript_arithmetic() {
    // Each would let a number affect the song where the graph, and so a knob, can't see it.
    for escape in ["base.value * 2", "base * 2", "Number(`${base}`)", "osc.sine(base) * 2", "Math.max(base, 2)"] {
        let fixture = ProjectFixture::new(&song(&format!(
            "const base = hz(800);\nexport default song(() => add(osc.sine(base), osc.saw({escape})));"
        )));
        let mut packages = PackageSet::default().with_store(m2_song::pack::test_store());
        packages.load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")).unwrap();
        // WRITE-20: the project opens, with no accepted code and the build's errors.
        let session = Session::open(&fixture.0, packages).unwrap();
        assert_eq!(session.accepted().code_rev, 0, "{escape} compiled");
        let message = session.draft_errors.iter().map(|e| e.to_string()).collect::<String>();
        assert!(message.contains("not a JavaScript number") && message.contains("scale(x, 2)"), "{escape}: {message}");
    }
}

#[test]
fn a_value_used_twice_through_graph_operations_is_one_knob_that_commits() {
    let fixture =
        ProjectFixture::new(&song("const base = hz(800);\nexport default song(() => add(osc.sine(base), osc.saw(scale(base, 2))));"));
    let mut session = fixture.open();
    let base = session.controls().into_iter().find(|c| c.literal == "800").unwrap();
    assert_eq!(base.uses, 2, "both oscillators read it");
    let audition = session.preview(base.revision, &base.id, 900.0).unwrap();
    let committed = session.set(base.revision, &base.id, 900.0, Some(800.0)).unwrap();
    assert_eq!((committed.origin, committed.value), (audition.origin, audition.value));
    assert!(fixture.source().contains("const base = hz(900);"));
}

#[test]
fn a_pitch_converted_to_frequency_stays_editable() {
    let fixture = ProjectFixture::new(&song("export default song(() => osc.sine(mtof(semis(60))));"));
    let mut session = fixture.open();
    let pitch = session.controls().into_iter().find(|c| c.literal == "60").expect("the pitch is a knob");
    session.set(pitch.revision, &pitch.id, 62.0, Some(60.0)).unwrap();
    assert!(fixture.source().contains("mtof(semis(62))"));
}
