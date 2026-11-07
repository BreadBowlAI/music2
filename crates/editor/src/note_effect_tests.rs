//! Note effects in the editor (PKG-21): the lists know them as a third kind of sound, + Note effect
//! wraps the notes of the call it goes before, and Bypass, Move and Remove work on one as on an
//! effect, each one undo step.
//!
//! The tests that write a note effect use one whose notes a kernel gives (ABI-15): one that only
//! gives its notes on as they come makes no node for the planner's checks (WRITE-9), which then
//! refuse the edit. That kernel builds but can't play (`testing::notes_pack`), and no test here
//! plays it. The rest use a note effect that gives its notes on as they come.

use std::path::{Path, PathBuf};

use super::*;
use crate::Gestures;
use crate::library::{Kind, Sources};
use crate::testing::{Fixture, notes_pack};

/// A song that plays `keys` (given `{ lane }`) beside a saw, with `@test/notes` at `pack`, and
/// `names` imported from it.
fn song(f: &Fixture, pack: &Path, names: &str, keys: &str) -> String {
    let manifest = format!("{}\"@test/notes\" = {{ path = {:?} }}\n", crate::testing::MANIFEST, pack.to_string_lossy());
    f.write("music2.toml", manifest);
    let text = format!(
        "import {{ hz, song }} from \"@m2/core\";\nimport {{ synth }} from \"@m2/synths\";\nimport {{ osc }} from \"std/dsp\";\n\
         {IMPORT}\nexport default song(({{ lane }}) => mixer({{ keys: {keys}, hum: osc.saw(hz(110)) }}));\n"
    );
    let text = if names.is_empty() { text } else { with(&text, names, "", "") };
    f.write("song.ts", &text);
    text
}

/// A song playing `keys`, with `names` imported, and its session, with `@test/notes` (`kernels`, or
/// giving notes on).
fn open(names: &str, keys: &str, kernels: bool) -> (Fixture, PathBuf, Session, String) {
    let f = Fixture::new();
    let pack = f.0.join("notes");
    notes_pack(&pack, kernels);
    let text = song(&f, &pack, names, keys);
    let s = f.open();
    (f, pack, s, text)
}

fn call(s: &Session, callee: &str) -> String {
    let site = s.accepted().compiled.sites.iter().find(|x| x.in_project && x.callee == callee).unwrap_or_else(|| panic!("no `{callee}`"));
    format!("{}#{}", site.file, site.ordinal)
}

fn rev(s: &Session) -> u64 {
    s.accepted().number
}

/// Call `callee` turned off (`on`) or on again.
fn turned(s: &mut Session, callee: &str, on: bool) -> Result<Edited, EditError> {
    let (at, id) = (rev(s), call(s, callee));
    bypass(s, at, &id, on)
}

/// Call `callee` moved before the one written in it (`earlier`), or after the one around it.
fn moved(s: &mut Session, callee: &str, earlier: bool) -> Result<Edited, EditError> {
    let (at, id) = (rev(s), call(s, callee));
    reorder(s, at, &id, earlier)
}

const IMPORT: &str = "import { mixer } from \"std/mix\";\n";

/// `text` with the import of `names` from `@test/notes` after std/mix's, and `from` written `to`.
fn with(text: &str, names: &str, from: &str, to: &str) -> String {
    text.replace(IMPORT, &format!("{IMPORT}import {{ {names} }} from \"@test/notes\";\n")).replace(from, to)
}

// PKG-21: the lists know a note effect: the effects list with `notes`, and the library as a kind of
// its own. No instrument and no effect is one.
#[test]
fn a_note_effect_is_listed_as_one() {
    let (_f, _pack, s, _) = open("", "synth.pluck(lane.notes(\"chords\"))", false);
    let listed = s.effects();
    let arp = listed.iter().find(|e| e.component == "@test/notes:arp").expect("the effects list has it");
    assert!(arp.notes && arp.category == "Arpeggiator" && arp.module == "@test/notes", "{arp:?}");
    let delay = listed.iter().find(|e| e.component == "@m2/effects:delay").expect("and the effects");
    assert!(!delay.notes);
    assert!(serde_json::to_value(delay).unwrap().get("notes").is_none(), "an effect's entry keeps its bytes");
    let items = crate::library::items(&Sources { catalog: s.catalog(), packages: &[], yours: None, plugins: &[], samples: &[] });
    let item = items.iter().find(|i| i.id == "@test/notes:arp").expect("the library lists it");
    assert_eq!((item.kind, item.note_effect, item.instrument, item.effect), (Kind::NoteEffect, true, false, false));
    assert_eq!(serde_json::to_value(item).unwrap()["kind"], "note-effect");
    let pluck = items.iter().find(|i| i.id == "@m2/synths:synth-pluck").unwrap();
    assert!(!pluck.note_effect && serde_json::to_value(pluck).unwrap().get("note_effect").is_none());
}

// PKG-21: a note effect goes before a call that takes notes first; before a saw, or a track's sound,
// it is refused in words, and nothing is written.
#[test]
fn a_note_effect_goes_only_before_what_plays_notes() {
    let (f, _pack, mut s, text) = open("", "synth.pluck(lane.notes(\"chords\"))", false);
    let e = s.add_effect(rev(&s), &call(&s, "osc.saw"), "@test/notes:arp").unwrap_err().to_string();
    assert!(e.contains("Arpeggiator can't go here: it goes before a sound that plays notes, and `osc.saw` plays none."), "{e}");
    assert_eq!(f.read("song.ts"), text);
}

// PKG-21 (review N1, design D2): a note effect goes before any call that plays notes: a pack's
// instrument (its first argument), a `poly` (its first) or a plugin (its `notes`), found from what
// the call reads, not from a list. The text it would wrap is those notes, as written.
#[test]
fn the_notes_a_note_effect_goes_around_are_found_in_any_player() {
    m2_plugins::probe::install();
    let (f, _pack, s, _) = open("", "synth.pluck(lane.notes(\"a\"))", false);
    let text = f.read("song.ts").replace(
        "mixer({ keys: synth.pluck(lane.notes(\"a\")), hum: osc.saw(hz(110)) })",
        "mixer({ keys: synth.pluck(lane.notes(\"a\")), voice: poly(lane.notes(\"b\"), (n) => osc.saw(n.pitch)), \
         probe: plugin(\"clap:m2.probe\", { notes: lane.notes(\"c\"), key: \"p\" }), hum: osc.saw(hz(110)) })",
    );
    f.write("song.ts", text.replace("import { hz, song }", "import { hz, plugin, poly, song }"));
    drop(s);
    let s = f.open();
    let around = |callee: &str| {
        let site = s.accepted().compiled.sites.iter().find(|x| x.in_project && x.callee == callee).unwrap().clone();
        notes_of(&s, &site, "Arpeggiator").map(|span| f.read("song.ts")[span.start as usize..span.end as usize].to_string())
    };
    assert_eq!(around("synth.pluck").unwrap(), "lane.notes(\"a\")");
    assert_eq!(around("poly").unwrap(), "lane.notes(\"b\")");
    assert_eq!(around("plugin").unwrap(), "lane.notes(\"c\")");
    let e = around("osc.saw").unwrap_err().to_string();
    assert!(e.contains("Arpeggiator can't go here: it goes before a sound that plays notes, and `osc.saw` plays none."), "{e}");
}

// PKG-21: + Note effect wraps the notes of the call it goes before, where they are written, and
// imports it: one undo step. Before a note effect, it goes between that one and its notes.
#[test]
fn adding_a_note_effect_wraps_the_notes_of_what_plays_them() {
    let (f, _pack, mut s, text) = open("", "synth.pluck(lane.notes(\"chords\"))", true);
    let added = s.add_effect(rev(&s), &call(&s, "synth.pluck"), "@test/notes:arp").unwrap_or_else(|e| panic!("{e}"));
    assert_eq!((added.before.as_str(), added.after.as_str()), ("lane.notes(\"chords\")", "arp(lane.notes(\"chords\"))"));
    let one = with(&text, "arp", "synth.pluck(lane.notes(\"chords\"))", "synth.pluck(arp(lane.notes(\"chords\")))");
    assert_eq!(f.read("song.ts"), one);
    s.add_effect(rev(&s), &call(&s, "arp"), "@test/notes:chord").unwrap_or_else(|e| panic!("{e}"));
    let two = with(&text, "arp, chord", "synth.pluck(lane.notes(\"chords\"))", "synth.pluck(arp(chord(lane.notes(\"chords\"))))");
    assert_eq!(f.read("song.ts"), two, "the chord goes before the arpeggiator");
    s.undo().unwrap();
    assert_eq!(f.read("song.ts"), one);
    s.undo().unwrap();
    assert_eq!(f.read("song.ts"), text);
}

// PKG-21 (review N1): before a `poly` and a plugin, as before a pack's instrument: their notes,
// wrapped where they are written.
#[test]
fn a_note_effect_goes_before_a_poly_and_a_plugin() {
    m2_plugins::probe::install();
    let f = Fixture::new();
    let pack = f.0.join("notes");
    notes_pack(&pack, true);
    let text = song(
        &f,
        &pack,
        "",
        "poly(lane.notes(\"b\"), (n) => osc.saw(n.pitch)), probe: plugin(\"clap:m2.probe\", { notes: lane.notes(\"c\"), key: \"p\" })",
    );
    let text = text.replace("import { hz, song }", "import { hz, plugin, poly, song }");
    f.write("song.ts", &text);
    let mut s = f.open();
    s.add_effect(rev(&s), &call(&s, "poly"), "@test/notes:arp").unwrap_or_else(|e| panic!("{e}"));
    s.add_effect(rev(&s), &call(&s, "plugin"), "@test/notes:chord").unwrap_or_else(|e| panic!("{e}"));
    let now = f.read("song.ts");
    assert!(now.contains("poly(arp(lane.notes(\"b\")), (n) => osc.saw(n.pitch))"), "{now}");
    assert!(now.contains("plugin(\"clap:m2.probe\", { notes: chord(lane.notes(\"c\")), key: \"p\" })"), "{now}");
    s.undo().unwrap();
    s.undo().unwrap();
    assert_eq!(f.read("song.ts"), text);
}

// PKG-21, PKG-3: Bypass, Move and Remove work on a note effect as on an effect, each one undo step.
#[test]
fn a_note_effect_is_bypassed_moved_and_removed_as_an_effect_is() {
    let (f, _pack, mut s, text) = open("arp, chord", "synth.pluck(arp(chord(lane.notes(\"chords\")), { gate: 0.5 }))", true);

    let off = turned(&mut s, "arp", true).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(off.after, "arp(…, { gate: 0.5, bypass: true })");
    let site = s.accepted().compiled.sites.iter().find(|x| x.callee == "arp").unwrap().id;
    assert!(is_bypassed(&s.accepted().compiled, site));
    s.undo().unwrap();
    assert_eq!(f.read("song.ts"), text);

    let swapped = moved(&mut s, "chord", false).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!((swapped.before.as_str(), swapped.after.as_str()), ("arp(chord(…))", "chord(arp(…))"));
    assert!(f.read("song.ts").contains("synth.pluck(chord(arp(lane.notes(\"chords\"), { gate: 0.5 })))"), "{}", f.read("song.ts"));
    let first = moved(&mut s, "arp", true).unwrap_err().to_string();
    assert!(first.contains("Arpeggiator is the first note effect here."), "{first}");
    s.undo().unwrap();
    assert_eq!(f.read("song.ts"), text);

    let removed = s.remove_effect(rev(&s), &call(&s, "chord")).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!((removed.before.as_str(), removed.after.as_str()), ("chord(lane.notes(\"chords\"))", "lane.notes(\"chords\")"));
    s.undo().unwrap();
    assert_eq!(f.read("song.ts"), text);
}

// PKG-21, PACK-29, PKG-3: each of `@m2/notes`' four goes before an instrument as its notes' wrap,
// one undo step, and every control it declares is a knob on it from the start, at its default.
#[test]
fn music2s_own_note_effects_go_before_an_instrument_each_value_a_knob() {
    let text = "import { song } from \"@m2/core\";\nimport { synth } from \"@m2/synths\";\n\
                export default song(({ lane }) => synth.pluck(lane.notes(\"chords\")));\n";
    let ours = |s: &Session| -> Vec<crate::effect::EffectInfo> {
        s.effects().into_iter().filter(|e| e.component.starts_with("@m2/notes:")).collect()
    };
    let f = Fixture::new();
    f.write("music2.toml", format!("{}\"@m2/notes\" = \"0.1\"\n", crate::testing::MANIFEST));
    f.write("song.ts", text);
    let listed = ours(&f.open());
    let ids: Vec<&str> = listed.iter().map(|e| e.component.as_str()).collect();
    assert_eq!(ids, ["@m2/notes:arp", "@m2/notes:chord", "@m2/notes:echo", "@m2/notes:lock-to-key"]);
    for e in &listed {
        assert!(e.notes, "{}: a note effect", e.component);
        let mut s = f.open();
        let added = s.add_effect(rev(&s), &call(&s, "synth.pluck"), &e.component).unwrap_or_else(|x| panic!("{}: {x}", e.component));
        assert_eq!(added.after, format!("{}(lane.notes(\"chords\"))", e.name), "{}", e.component);
        assert!(f.read("song.ts").contains(&format!("import {{ {} }} from \"@m2/notes\";", e.name)), "{}", f.read("song.ts"));
        let d = s.catalog().components[&e.component].clone();
        let node = s.view().modules[0].nodes.iter().find(|n| n.component.as_deref() == Some(&e.component)).cloned().expect("its card");
        let mut knobs: Vec<&str> = node.knobs.iter().map(|k| k.slot.as_deref().unwrap()).collect();
        knobs.sort();
        let mut declared: Vec<&str> = d.controls.iter().map(|c| c.name.as_str()).collect();
        declared.sort();
        assert_eq!(knobs, declared, "{}: every value a knob", e.component);
        assert!(node.knobs.iter().all(|k| !k.written), "{}: each at its default", e.component);
        s.undo().unwrap();
        assert_eq!(f.read("song.ts"), text, "{}: one undo step", e.component);
    }
}

// PACK-10, PACK-18: adding the Arpeggiator to a song that doesn't list `@m2/notes` lists it, at the
// line for the version music2 comes with, and locks it, in the same undo step.
#[test]
fn adding_music2s_arpeggiator_lists_and_locks_its_pack() {
    let f = Fixture::new();
    f.write("music2.toml", crate::testing::MANIFEST);
    f.write("song.ts", "import { song } from \"@m2/core\";\nimport { synth } from \"@m2/synths\";\nexport default song(({ lane }) => synth.pluck(lane.notes(\"chords\")));\n");
    let mut s = f.open();
    s.add_effect(rev(&s), &call(&s, "synth.pluck"), "@m2/notes:arp").unwrap_or_else(|e| panic!("{e}"));
    assert!(f.read("music2.toml").contains("\"@m2/notes\" = \"0.1\""), "{}", f.read("music2.toml"));
    assert!(f.read("music2.lock").contains("name = \"@m2/notes\"\nversion = \"0.1.0\""), "{}", f.read("music2.lock"));
    assert!(f.read("song.ts").contains("synth.pluck(arp(lane.notes(\"chords\")))"));
    s.undo().unwrap();
    assert!(!f.read("music2.toml").contains("@m2/notes"));
}
