//! Renaming a track's label: each way its `track()` call can be written, what is refused, and
//! that nothing but the name changes.

use m2_session::Session;
use m2_song::tx::{Expect, Tx};

use super::*;
use crate::Gestures;
use crate::testing::{Fixture, MANIFEST, fixture};

/// What the timeline and the canvas show for `module`.
fn shown(s: &Session, module: &str) -> (String, String) {
    let t = s.timeline().tracks.into_iter().find(|t| t.module == module).map(|t| t.label).unwrap();
    let v = s.view().modules.into_iter().find(|m| m.key == module).map(|m| m.label).unwrap();
    (t, v)
}

fn render(s: &Session) -> Vec<[f32; 2]> {
    let rev = s.accepted();
    m2_runtime::Renderer::new(&rev.compiled.flat, &rev.compiled.data, &s.packages, 48_000).unwrap().render(1.0).unwrap()
}

// ID-10, WRITE-6: a label is written as the track's `name`, replaced in place, and taken out when
// it is the key again; the timeline and the canvas show it, nothing that plays changes, and undo
// puts the old name back. History names it "Kick → Kick Drum".
#[test]
fn a_label_is_replaced_taken_back_to_the_key_and_written_again() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let before = f.read("tracks/kick.ts");
    let sound = render(&s);

    let done = s.rename_label("kick", "Kick Drum").unwrap();
    assert_eq!((done.file.as_str(), done.before.as_str(), done.after.as_str()), ("tracks/kick.ts", "Kick", "Kick Drum"));
    assert_eq!(f.read("tracks/kick.ts"), before.replace("{ name: \"Kick\" }", "{ name: \"Kick Drum\" }"));
    assert_eq!(shown(&s, "kick"), ("Kick Drum".into(), "Kick Drum".into()));
    assert_eq!(render(&s), sound, "a name is never heard");
    assert!(!s.refresh(), "the session took in its own write");

    // Back to the key: the label defaults to it, so the options go.
    s.rename_label("kick", "kick").unwrap();
    assert_eq!(f.read("tracks/kick.ts"), before.replace(", { name: \"Kick\" }", ""));
    assert_eq!(shown(&s, "kick"), ("kick".into(), "kick".into()));
    // And written again into a call that has no options: after the key.
    s.rename_label("kick", "Big Kick").unwrap();
    assert_eq!(f.read("tracks/kick.ts"), before.replace("\"Kick\"", "\"Big Kick\""));

    // WRITE-21: each is one undo step.
    let undone = s.undo().unwrap();
    assert_eq!((undone.before.as_str(), undone.after.as_str()), ("Big Kick", "kick"));
    s.undo().unwrap();
    s.undo().unwrap();
    assert_eq!(f.read("tracks/kick.ts"), before);
    assert_eq!(shown(&s, "kick").0, "Kick");
}

// ID-10: the same label is no edit; an empty `{}` gets the name; a label is written as a string
// whatever it holds.
#[test]
fn a_label_is_written_as_a_string_whatever_it_holds() {
    let f = Fixture::new();
    f.write("music2.toml", MANIFEST);
    let song = "import { song, track } from \"@m2/core\";\nimport { hat } from \"std/instruments\";\n\
                const h = track(\"hats\", {}, ({ lane }) => hat(lane.notes(\"hits\")));\n\
                export default song(() => h.out);\n";
    f.write("song.ts", song);
    let mut s = f.open();
    let at = s.accepted().number;
    let same = s.rename_label("hats", "  hats ").unwrap();
    assert_eq!((same.revision, same.before.as_str(), same.after.as_str()), (at, "hats", "hats"), "nothing to write");
    assert_eq!(f.read("song.ts"), song);

    let odd = "Hats \"808\" ☆ \\ back";
    s.rename_label("hats", odd).unwrap();
    assert_eq!(f.read("song.ts"), song.replace("{}", "{ name: \"Hats \\\"808\\\" ☆ \\\\ back\" }"));
    assert_eq!(shown(&s, "hats"), (odd.to_string(), odd.to_string()));
}

// ID-10: only a name written as a string in the call's own options is renamed here; the song has
// none; an empty name is refused (the SDK refuses one too).
#[test]
fn a_name_the_call_doesnt_write_plainly_is_changed_in_the_code() {
    let f = Fixture::new();
    f.write("music2.toml", MANIFEST);
    let song = "import { song, track } from \"@m2/core\";\nimport { hat, kick } from \"std/instruments\";\n\
                import { mixer } from \"std/mix\";\nconst name = \"Hats\";\nconst opts = { name: \"Kick\" };\n\
                const h = track(\"hats\", { name }, ({ lane }) => hat(lane.notes(\"hits\")));\n\
                const k = track(\"kick\", opts, ({ lane }) => kick(lane.notes(\"hits\")));\n\
                export default song(() => mixer({ hats: h.out, kick: k.out }));\n";
    f.write("song.ts", song);
    let mut s = f.open();
    assert!(s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    for (module, label) in [("hats", "Hi-hats"), ("kick", "Kick Drum")] {
        let refused = s.rename_label(module, label);
        assert!(matches!(&refused, Err(EditError::Rejected(why)) if why.contains("change its name there")), "{module}: {refused:?}");
    }
    assert!(matches!(s.rename_label("song", "Song"), Err(EditError::Rejected(_))));
    assert!(matches!(s.rename_label("hats", " "), Err(EditError::Rejected(_))));
    assert!(matches!(s.rename_label("hats", "two\nlines"), Err(EditError::Rejected(_))));
    assert!(matches!(s.rename_label("bass", "Bass"), Err(EditError::NotFound(_))));
    assert!(!s.can_undo(), "nothing was written");
}

// WRITE-6, WRITE-10: a rename's Tx is `Identical`, so the same edit with anything else in it is
// refused and writes nothing.
#[test]
fn a_rename_that_would_change_more_is_refused() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let before = f.read("tracks/kick.ts");
    let text = s.accepted().snapshot.files[std::path::Path::new("tracks/kick.ts")].clone();
    let file = plan::path("tracks/kick.ts").unwrap();
    let at = |needle: &str| text.find(needle).unwrap() as u32;
    let edits = [(at("\"Kick\""), at("\"Kick\"") + 6, "\"Kick Drum\"".to_string()), (at("150"), at("150") + 3, "151".to_string())];
    let tx: Tx = plan::planned(&s, ("Kick".into(), "Kick Drum".into()), vec![plan::text_edit(&file, &text, &edits)], Expect::Identical);
    let refused = plan::run(&mut s, tx, |_, _| Ok(()));
    assert!(matches!(&refused, Err(EditError::Rejected(why)) if why.contains("150") || why.contains("change")), "{refused:?}");
    assert_eq!(f.read("tracks/kick.ts"), before);
}
