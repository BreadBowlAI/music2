//! A declared control that a call leaves out (inherited, PKG-3) is a knob at its default; its
//! first edit writes it into the call (D5), verified against the graph, and undo takes it out
//! again wherever later edits carried it (WRITE-21).

use std::path::{Path, PathBuf};

use super::*;
use crate::label::fold;
use m2_runtime::PackageSet;
use m2_session::{Home, Session, StepError};

struct Fixture(PathBuf);

impl Fixture {
    fn new(source: &str) -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("m2-insert-{}-{n}", std::process::id()));
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

    /// An outside save: the file as `f` makes it, written behind the session's back.
    fn edit(&self, f: impl FnOnce(&str) -> String) {
        let text = f(&self.source());
        std::fs::write(self.0.join("song.ts"), text).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const HEAD: &str = "import { song } from \"@m2/core\";\nimport { osc } from \"std/dsp\";\nimport { lowpass } from \"std/filter\";\n\
                    import { delay } from \"std/fx\";\n";

/// The control of slot `slot` that call `callee` leaves out.
fn inherited(s: &Session, callee: &str, slot: &str) -> Control {
    let sites = &s.accepted().compiled.sites;
    s.controls()
        .into_iter()
        .find(|c| !c.written && c.origin.slot() == Some(slot) && sites.iter().any(|x| x.id == c.origin.site && x.callee == callee))
        .unwrap_or_else(|| panic!("no inherited {callee} {slot} in {:?}", s.controls().iter().map(|c| &c.id).collect::<Vec<_>>()))
}

// D5, PKG-3: an inherited control's first edit writes it into its call: positionally when it is
// the next argument, else into the call's options object, else in a new one; a plain number in
// base units. Afterwards it is that call's written knob, with the value, and the graph is the
// same apart from it.
#[test]
fn an_inherited_control_is_written_on_its_first_edit() {
    let cases = [
        ("lowpass(osc.saw(110))", "lowpass", "cutoff", 1200.0, "lowpass(osc.saw(110), 1200)"),
        ("lowpass(osc.saw(110), 800)", "lowpass", "q", 0.9, "lowpass(osc.saw(110), 800, 0.9)"),
        ("lowpass(osc.saw(110))", "lowpass", "q", 0.9, "lowpass(osc.saw(110), { q: 0.9 })"),
        ("delay(osc.saw(110))", "delay", "mix", 0.5, "delay(osc.saw(110), { mix: 0.5 })"),
        ("delay(osc.saw(110), { time: 0.3 })", "delay", "mix", 0.5, "delay(osc.saw(110), { time: 0.3, mix: 0.5 })"),
        ("delay(osc.saw(110), {})", "delay", "mix", 0.5, "delay(osc.saw(110), { mix: 0.5 })"),
        ("delay(osc.tri())", "osc.tri", "freq", 220.0, "delay(osc.tri(220))"),
        ("delay(osc.square(110))", "osc.square", "width", 0.25, "delay(osc.square(110, 0.25))"),
    ];
    for (call, callee, slot, value, after) in cases {
        let fixture = Fixture::new(&format!("{HEAD}export default song(() => {call});\n"));
        let mut s = fixture.open();
        let c = inherited(&s, callee, slot);
        assert_eq!((c.written, c.literal.as_str(), c.home), (false, "", Home::Code));
        assert!(c.id.ends_with(&format!("/@{slot}")), "{}", c.id);
        let before = s.controls().len();
        let done = s.set(s.accepted().number, &c.id, value, Some(c.value)).unwrap_or_else(|e| panic!("{call} {slot}: {e}"));
        assert_eq!(fixture.source(), format!("{HEAD}export default song(() => {after});\n"), "{call} {slot}");
        // Shown as the call before and after.
        assert!(after.contains(&done.after) && call.contains(&done.before) && done.before.starts_with(callee), "{done:?}");
        assert_eq!((done.value, done.origin.inherited(), done.origin.slot()), (value, false, Some(slot)));
        let now = s.controls().into_iter().find(|k| k.origin.same_value(&done.origin)).expect("the written control");
        assert_eq!((now.written, now.value), (true, value));
        assert_eq!(s.controls().len(), before, "{call} {slot}: the same controls, one of them written now");
    }
}

// D5: releasing an inherited knob at its default writes nothing, as a literal at its own value.
#[test]
fn releasing_an_inherited_knob_at_its_default_writes_nothing() {
    let original = format!("{HEAD}export default song(() => lowpass(osc.saw(110)));\n");
    let fixture = Fixture::new(&original);
    let mut s = fixture.open();
    let q = inherited(&s, "lowpass", "q");
    let done = s.set(s.accepted().number, &q.id, q.value, None).unwrap();
    assert_eq!((done.revision, done.before.as_str(), done.after.as_str()), (1, "", ""));
    assert_eq!(fixture.source(), original);
    assert!(!s.can_undo());
}

// D5, HOME-1: a control the call leaves out but that can't be written into it (an argument at its
// place reads `undefined`, or its options aren't an object written in the call) is shown as a
// read-only value with its home, never as a knob that would write something else.
#[test]
fn a_control_the_call_cannot_take_is_read_only() {
    for call in ["lowpass(osc.saw(110), undefined, 0.8)", "delay(osc.saw(110), opts)"] {
        let fixture = Fixture::new(&format!("{HEAD}const opts = {{ time: 0.3 }};\nexport default song(() => {call});\n"));
        let s = fixture.open();
        let slot = if call.starts_with("lowpass") { "cutoff" } else { "mix" };
        assert!(s.controls().iter().all(|c| c.origin.slot() != Some(slot)), "{call}: no control for {slot}");
        let view = s.view();
        let node = view.modules[0].nodes.iter().find(|n| call.starts_with(&n.label)).unwrap();
        let ro = node.readonly.iter().find(|r| r.label == slot).unwrap_or_else(|| panic!("{call}: {:?}", node.readonly));
        assert_eq!(ro.home, Home::Code);
        assert!(ro.reason.starts_with("Plays its default"), "{}", ro.reason);
    }
}

// PKG-3, design 6.4: only the call's run tells `lowpass(x, opts)`'s options from a cutoff, so the
// planner reads where the call held its options (`Frame.options`), not the source. Options it
// can't write into make the controls behind them read-only, whatever argument holds them and
// whatever they give; options left out (`undefined`, forwarded) are no object to write into
// either. A signal at that place is a value, so the next control is still written after it.
#[test]
fn options_the_call_reads_are_where_controls_go() {
    let head = format!(
        "{HEAD}import {{ lfo }} from \"std/control\";\nimport {{ xrange }} from \"std/math\";\n\
         const opts = {{ cutoff: 500 }};\nconst base = {{ cutoff: 700 }};\nconst none = {{}};\n\
         const maybe: {{ q?: number; mix?: number }} | undefined = undefined;\n"
    );
    let song = |call: &str| {
        format!(
            "{head}export default song(({{ time }}) => {{\n  const wobble = xrange(lfo(time.beats), 200, 2000);\n  \
             const wired = {{ cutoff: wobble }};\n  return {call};\n}});\n"
        )
    };
    for (call, slot) in [
        ("lowpass(osc.saw(110), opts)", "q"),
        ("lowpass(osc.saw(110), { ...base })", "q"),
        ("lowpass(osc.saw(110), none)", "q"),
        ("lowpass(osc.saw(110), none)", "cutoff"),
        ("lowpass(osc.saw(110), wired)", "q"),
        ("delay(osc.saw(110), maybe)", "mix"),
    ] {
        let fixture = Fixture::new(&song(call));
        let s = fixture.open();
        let callee = &call[..call.find('(').unwrap()];
        let sites = &s.accepted().compiled.sites;
        let ours = |c: &Control| sites.iter().any(|x| x.id == c.origin.site && x.callee == callee);
        assert!(!s.controls().iter().any(|c| ours(c) && c.origin.slot() == Some(slot) && !c.written), "{call}: {slot} is no knob");
        let view = s.view();
        let node = view.modules[0].nodes.iter().find(|n| n.label == callee).unwrap_or_else(|| panic!("{call}"));
        let ro = node.readonly.iter().find(|r| r.label == slot).unwrap_or_else(|| panic!("{call}: {:?}", node.readonly));
        assert!(ro.reason.starts_with("Plays its default"), "{call}: {}", ro.reason);
    }
    // A signal where the cutoff goes: `q` is the next argument.
    let fixture = Fixture::new(&song("lowpass(osc.saw(110), wobble)"));
    let mut s = fixture.open();
    let q = inherited(&s, "lowpass", "q");
    s.set(s.accepted().number, &q.id, 0.9, None).unwrap();
    assert!(fixture.source().contains("return lowpass(osc.saw(110), wobble, 0.9);"), "{}", fixture.source());
}

// Design 6.4: a control written into an options object with no properties goes inside its
// braces, and whatever else is in them (a comment, a line break) stays, after it; undo gives
// back exactly what was there.
#[test]
fn an_empty_options_object_keeps_what_is_in_it() {
    for (options, written) in [
        ("{ }", "{ mix: 0.5 }"),
        ("{ /* nothing yet */ }", "{ mix: 0.5, /* nothing yet */ }"),
        ("{\n    // TODO: tune the echo\n  }", "{ mix: 0.5,\n    // TODO: tune the echo\n  }"),
        ("{\n  }", "{ mix: 0.5,\n  }"),
    ] {
        let original = format!("{HEAD}export default song(() => delay(osc.saw(110), {options}));\n");
        let fixture = Fixture::new(&original);
        let mut s = fixture.open();
        let mix = inherited(&s, "delay", "mix");
        s.set(s.accepted().number, &mix.id, 0.5, None).unwrap_or_else(|e| panic!("{options}: {e}"));
        assert_eq!(fixture.source(), format!("{HEAD}export default song(() => delay(osc.saw(110), {written}));\n"), "{options}");
        s.undo().unwrap();
        assert_eq!(fixture.source(), original, "{options}");
    }
}

// D5: a value or a cable written into a call is shown with its call, before and after. A call
// that doesn't fit on a line shows `…` for each long argument the edit doesn't touch, and for
// each long property of the options object it edits, so the message shows what was written.
#[test]
fn a_long_call_is_shown_around_what_was_written() {
    let original = format!(
        "{HEAD}import {{ lfo }} from \"std/control\";\nimport {{ xrange }} from \"std/math\";\n\
         import {{ master, mixer }} from \"std/mix\";\nexport default song(({{ time }}) =>\n  master(\n    mixer({{\n      \
         // the lead\n      a: delay(osc.saw(110), {{ mix: xrange(lfo(time.beats), 0.1, 0.5) }}),\n      \
         b: osc.sine(220),\n    }}),\n  ),\n);\n"
    );
    let fixture = Fixture::new(&original);
    let mut s = fixture.open();
    let ceiling = inherited(&s, "master", "ceiling");
    let done = s.set(s.accepted().number, &ceiling.id, -3.0, None).unwrap();
    assert_eq!((done.before.as_str(), done.after.as_str()), ("master(…)", "master(…, { ceiling: -3 })"));
    let undone = s.undo().unwrap();
    assert_eq!((undone.before.as_str(), undone.after.as_str()), ("master(…, { ceiling: -3 })", "master(…)"));
    assert_eq!(fixture.source(), original);
    let feedback = inherited(&s, "delay", "feedback");
    let done = s.set(s.accepted().number, &feedback.id, 0.5, None).unwrap();
    let shown = (done.before.as_str(), done.after.as_str());
    assert_eq!(shown, ("delay(osc.saw(110), { mix: … })", "delay(osc.saw(110), { mix: …, feedback: 0.5 })"));
}

// Next D4e (Q31): a first options object in a call written over lines goes on a line of its own,
// at the arguments' indent, with a comma after it when the last argument had one. A call written
// on one line, or whose first argument starts on the `(`'s line, keeps it after the last argument.
#[test]
fn a_first_options_object_goes_on_its_own_line() {
    let head = format!("{HEAD}import {{ master, mixer }} from \"std/mix\";\nexport default song(() =>\n");
    let cases = [
        ("  master(\n    mixer({ a: osc.sine(220) }),\n  ),\n", "  master(\n    mixer({ a: osc.sine(220) }),\n    { ceiling: -3 },\n  ),\n"),
        ("  master(\n    mixer({ a: osc.sine(220) })\n  ),\n", "  master(\n    mixer({ a: osc.sine(220) }),\n    { ceiling: -3 }\n  ),\n"),
        ("  master(\n\tmixer({ a: osc.sine(220) }), // the mix\n  ),\n", "  master(\n\tmixer({ a: osc.sine(220) }),\n\t{ ceiling: -3 }, // the mix\n  ),\n"),
        ("  master(mixer({ a: osc.sine(220) })),\n", "  master(mixer({ a: osc.sine(220) }), { ceiling: -3 }),\n"),
        ("  master(mixer({\n    a: osc.sine(220),\n  })),\n", "  master(mixer({\n    a: osc.sine(220),\n  }), { ceiling: -3 }),\n"),
    ];
    for (call, after) in cases {
        let original = format!("{head}{call});\n");
        let fixture = Fixture::new(&original);
        let mut s = fixture.open();
        let ceiling = inherited(&s, "master", "ceiling");
        let done = s.set(s.accepted().number, &ceiling.id, -3.0, None).unwrap_or_else(|e| panic!("{call}: {e}"));
        assert_eq!(fixture.source(), format!("{head}{after});\n"), "{call}");
        assert!(done.after.ends_with(", { ceiling: -3 })"), "{call}: shown on one line: {}", done.after);
        s.undo().unwrap();
        assert_eq!(fixture.source(), original, "{call}: undone");
    }
}

// D5: a call is folded onto one line for a message: a line comment goes, since it would run on
// over what follows it, and block comments and strings stay as written.
#[test]
fn a_folded_call_keeps_all_but_line_comments() {
    let call = "reverb(b, {\n  mix: 0.2, // wet\n  size: 0.5, /* a // b */\n  label: \"x // y\",\n})";
    assert_eq!(fold(call), "reverb(b, { mix: 0.2, size: 0.5, /* a // b */ label: \"x // y\", })");
}

// WRITE-21: undoing an insertion finds the inserted text where later edits carried it, takes out
// exactly that and nothing else, and checks the control plays its default again; redo writes it
// back.
#[test]
fn undo_of_an_inserted_argument_survives_other_edits() {
    let body = "export default song(() => {\n  const dark = lowpass(osc.saw(110), 800);\n  return delay(dark, { time: 0.3 });\n});\n";
    let fixture = Fixture::new(&format!("{HEAD}{body}"));
    let mut s = fixture.open();
    let q = inherited(&s, "lowpass", "q");
    s.set(s.accepted().number, &q.id, 0.9, None).unwrap();
    assert!(fixture.source().contains("lowpass(osc.saw(110), 800, 0.9)"));
    // An outside save above it and on another line: a comment, and another number. (A change
    // inside the insertion touches it, as for any carried range, WRITE-12.)
    fixture.edit(|t| format!("// a note\n{}", t.replace("{ time: 0.3 }", "{ time: 0.35 }")));
    assert!(s.refresh());
    assert!(s.can_undo());
    let undone = s.undo().unwrap();
    assert_eq!((undone.before.as_str(), undone.after.as_str()), ("lowpass(osc.saw(110), 800, 0.9)", "lowpass(osc.saw(110), 800)"));
    let expect = format!("// a note\n{HEAD}{}", body.replace("0.3", "0.35"));
    assert_eq!(fixture.source(), expect, "only the insertion is taken out");
    assert!(!inherited(&s, "lowpass", "q").written);
    s.redo().unwrap();
    assert!(fixture.source().contains("lowpass(osc.saw(110), 800, 0.9)") && fixture.source().contains("{ time: 0.35 }"));
    s.undo().unwrap();
    assert_eq!(fixture.source(), expect);
}

// WRITE-21: an outside edit inside the inserted text is a conflict: undo would overwrite it.
#[test]
fn an_edited_insertion_conflicts() {
    let fixture = Fixture::new(&format!("{HEAD}export default song(() => delay(osc.saw(110)));\n"));
    let mut s = fixture.open();
    let mix = inherited(&s, "delay", "mix");
    s.set(s.accepted().number, &mix.id, 0.5, None).unwrap();
    fixture.edit(|t| t.replace("{ mix: 0.5 }", "{ mix: 0.55 }"));
    assert!(s.refresh());
    assert!(!s.can_undo(), "can_undo agrees with undo");
    let before = fixture.source();
    assert!(matches!(s.undo(), Err(StepError::Refused(_))));
    assert_eq!(fixture.source(), before);
}

// WRITE-21: a knob edit of the inserted number is its own step on top, undone first.
#[test]
fn a_knob_edit_of_an_inserted_argument_is_undone_first() {
    let original = format!("{HEAD}export default song(() => delay(osc.saw(110)));\n");
    let fixture = Fixture::new(&original);
    let mut s = fixture.open();
    let mix = inherited(&s, "delay", "mix");
    s.set(s.accepted().number, &mix.id, 0.5, None).unwrap();
    let written = s.controls().into_iter().find(|c| c.written && c.origin.slot() == Some("mix")).unwrap();
    s.set(s.accepted().number, &written.id, 0.7, Some(0.5)).unwrap();
    assert!(fixture.source().contains("delay(osc.saw(110), { mix: 0.7 })"));
    s.undo().unwrap();
    assert!(fixture.source().contains("delay(osc.saw(110), { mix: 0.5 })"));
    s.undo().unwrap();
    assert_eq!(fixture.source(), original);
}

// D5, WRITE-22: connecting a signal to a control the call leaves out writes its name into the call,
// verified as a rewire of every constant of that control.
#[test]
fn connecting_a_control_the_call_leaves_out_writes_the_name_into_it() {
    let original = "import { song } from \"@m2/core\";\nimport { lfo } from \"std/control\";\nimport { osc } from \"std/dsp\";\n\
                    import { delay } from \"std/fx\";\nexport default song(({ time }) => {\n  const wobble = lfo(time.beats);\n  \
                    return delay(osc.saw(110));\n});\n";
    let fixture = Fixture::new(original);
    let mut s = fixture.open();
    let mix = inherited(&s, "delay", "mix");
    let outlet = s.outlets().into_iter().find(|o| o.name == "wobble").unwrap();
    let done = s.connect(s.accepted().number, &outlet.id, &mix.id).unwrap();
    assert!(fixture.source().contains("return delay(osc.saw(110), { mix: wobble });"), "{}", fixture.source());
    assert!(s.inlets().iter().any(|i| i.name == "wobble"));
    // Shown with its call, as a value written there is, never as the bare `, { mix: wobble }`.
    assert_eq!((done.before.as_str(), done.after.as_str()), ("delay(osc.saw(110))", "delay(osc.saw(110), { mix: wobble })"));
    let undone = s.undo().unwrap();
    assert_eq!((undone.before.as_str(), undone.after.as_str()), ("delay(osc.saw(110), { mix: wobble })", "delay(osc.saw(110))"));
    assert_eq!(fixture.source(), original);
}

// D14, 6.5: disconnecting writes the receiving control's default as its descriptor spells it:
// `ms(45)`, the constructor imported when the file doesn't have it, never `0.045` with float noise
// or a base value in another unit; a spelling of scale 1 is written as its plain number.
#[test]
fn disconnect_writes_the_spelled_default() {
    for imported in ["song", "ms, song"] {
        let source = format!(
            "import {{ {imported} }} from \"@m2/core\";\nimport {{ lfo }} from \"std/control\";\nimport {{ hat }} from \"std/instruments\";\n\
             import {{ xrange }} from \"std/math\";\nexport default song(({{ lane, time }}) => {{\n  \
             const d = xrange(lfo(time.beats), 0.02, 0.08);\n  return hat(lane.notes(\"hits\"), {{ decay: d }});\n}});\n"
        );
        let fixture = Fixture::new(&source);
        let mut s = fixture.open();
        let inlet = s.inlets().into_iter().find(|i| i.name == "d").unwrap();
        s.disconnect(s.accepted().number, &inlet.id).unwrap_or_else(|e| panic!("{imported}: {e}"));
        let now = fixture.source();
        assert!(now.contains("hat(lane.notes(\"hits\"), { decay: ms(45) })"), "{now}");
        assert!(
            now.starts_with("import { ms, song } from \"@m2/core\";") || now.starts_with("import { song, ms } from \"@m2/core\";"),
            "{now}"
        );
        let decay = s.controls().into_iter().find(|c| c.origin.slot() == Some("decay")).unwrap();
        assert_eq!(decay.value, 45.0 * 0.001, "exactly `ms(45)`");
    }
    // A spelling of scale 1 is its number.
    let fixture = Fixture::new(&format!(
        "{HEAD}import {{ lfo }} from \"std/control\";\nimport {{ xrange }} from \"std/math\";\nexport default song(({{ time }}) => {{\n  \
         const c = xrange(lfo(time.beats), 200, 2000);\n  return lowpass(osc.saw(110), c);\n}});\n"
    ));
    let mut s = fixture.open();
    let inlet = s.inlets().into_iter().find(|i| i.name == "c").unwrap();
    s.disconnect(s.accepted().number, &inlet.id).unwrap();
    assert!(fixture.source().contains("lowpass(osc.saw(110), 1000)"), "{}", fixture.source());
}

// WRITE-12, WRITE-22: a control a call leaves out is carried to a later revision like a written
// one, by its call: a save that adds a call above it renumbers its id, and the carried id names the
// same call's slot, so a lease on it lives through the save.
#[test]
fn an_inherited_control_is_carried_across_a_save_above_it() {
    let fixture = Fixture::new(&format!("{HEAD}export default song(() => delay(osc.saw(110)));\n"));
    let mut s = fixture.open();
    let mix = inherited(&s, "delay", "mix");
    let before = s.accepted().number;
    fixture.edit(|t| t.replace("export default", "const unused = () => osc.sine(220);\nexport default"));
    assert!(s.refresh() && s.draft_errors.is_empty() && s.accepted().number > before);
    let carried = s.carry(before, &mix.id, &s.controls_by_id()).unwrap();
    assert_ne!(carried.id, mix.id, "the calls above renumber it");
    assert_eq!((carried.origin.slot(), carried.written, carried.value), (Some("mix"), false, mix.value));
    assert!(carried.id.ends_with("/@mix"));
}
