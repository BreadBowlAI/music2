//! Cut by (KIT-27): the Drum card's menu writes a cut as TypeScript, an export and an import, in
//! one undo step, and takes it out again; what it can't be sure of it refuses in words.

use std::collections::BTreeMap;

use super::*;
use crate::Gestures;
use crate::testing::{Fixture, files_under, fixture};

fn tree(f: &Fixture) -> BTreeMap<String, Vec<u8>> {
    files_under(&f.0)
        .into_iter()
        .filter(|p| !p.strip_prefix(&f.0).unwrap().starts_with(".music2"))
        .map(|p| (p.strip_prefix(&f.0).unwrap().to_str().unwrap().to_string(), std::fs::read(&p).unwrap()))
        .collect()
}

/// The empty template with the Basic kit (Kick, Snare, Hats) and an Open hat added to it by + Drum.
fn kit() -> (Fixture, m2_session::Session) {
    let f = fixture("templates/empty");
    let mut s = f.open();
    crate::drums::add_kit(&mut s, DRUM, "Basic kit", None).unwrap();
    crate::kits::add_drum(&mut s, "drums", "open hat").unwrap();
    (f, s)
}

/// The Cut by row of track `track`'s Drum card, as the view shows it.
fn row_of(s: &m2_session::Session, track: &str) -> CutBy {
    let view = s.view();
    let m = view.modules.iter().find(|m| m.key == track).unwrap_or_else(|| panic!("{track}"));
    m.nodes.iter().find_map(|n| n.cut_by.clone()).unwrap_or_else(|| panic!("{track}'s Drum has a Cut by row"))
}

// KIT-27: Cut by on the Open hat picks the Hats: one Tx, one undo step, writing `cutBy: hats.hits`
// into the Drum's call, an import of the Hats, and the Hats' lane returned beside its sound; its card
// then says so. None takes `cutBy` out and leaves the import and the export. Undo and redo put back
// each, byte for byte.
#[test]
fn picking_cut_by_writes_the_code_as_one_undo_step() {
    let (f, mut s) = kit();
    let row = row_of(&s, "open-hat");
    assert_eq!(row.by, None);
    let names: Vec<&str> = row.choices.iter().map(|(_, n)| n.as_str()).collect();
    assert_eq!(names, ["Hats", "Kick", "Snare"], "the other Drum tracks, by name");
    let before = tree(&f);
    let done = cut_by(&mut s, "open-hat", Some("hats")).unwrap();
    assert_eq!((done.before.as_str(), done.after.as_str()), ("no cut", "hats.hits"));
    let open = f.read("tracks/open-hat.ts");
    assert!(open.contains("import hats from \"./hats\";"), "{open}");
    assert!(open.contains("drums.drum(lane.notes(\"hits\"), { drum: \"open hat\", cutBy: hats.hits })"), "{open}");
    let hats = f.read("tracks/hats.ts");
    assert!(
        hats.contains("({ lane }) => ({ out: drums.drum(lane.notes(\"hits\"), { drum: \"hats\" }), hits: lane.notes(\"hits\") })"),
        "{hats}"
    );
    assert_eq!(row_of(&s, "open-hat").by.as_deref(), Some("hats"));
    // Still a kit's row (KIT-1), both.
    let rows: Vec<String> = crate::kits::drum_kits(&s).iter().flat_map(|k| k.rows.iter().map(|r| r.track.clone())).collect();
    assert!(rows.contains(&"open-hat".to_string()) && rows.contains(&"hats".to_string()), "{rows:?}");
    let after = tree(&f);
    crate::undo(&mut s).unwrap();
    assert_eq!(tree(&f), before, "one undo takes it all back");
    crate::redo(&mut s).unwrap();
    assert_eq!(tree(&f), after);
    // Picked again, it says so; cut by the Kick, the value changes, the Kick exports its lane too.
    assert!(cut_by(&mut s, "open-hat", Some("hats")).unwrap_err().to_string().contains("Open hat is cut by Hats already."));
    cut_by(&mut s, "open-hat", Some("kick")).unwrap();
    assert!(f.read("tracks/open-hat.ts").contains("cutBy: kick.hits"), "{}", f.read("tracks/open-hat.ts"));
    // None: `cutBy` goes, with the options' comma, and the imports and exports stay.
    let done = cut_by(&mut s, "open-hat", None).unwrap();
    assert_eq!((done.before.as_str(), done.after.as_str()), ("kick.hits", "no cut"));
    let open = f.read("tracks/open-hat.ts");
    assert!(
        open.contains("drums.drum(lane.notes(\"hits\"), { drum: \"open hat\" })") && open.contains("import kick from \"./kick\";"),
        "{open}"
    );
    assert_eq!(row_of(&s, "open-hat").by, None);
    assert!(cut_by(&mut s, "open-hat", None).unwrap_err().to_string().contains("Open hat isn't cut by any track."));
}

// KIT-27: a Drum whose call has no options gets them; one that already returns an object adds its
// lane to it; a function with a block returns its lane beside its sound too.
#[test]
fn cut_by_writes_into_each_form_it_can_follow() {
    let (f, s) = kit();
    drop(s);
    f.write("tracks/open-hat.ts", "import { track } from \"@m2/core\";\nimport { drums } from \"@m2/drums\";\n\nexport default track(\"open-hat\", { name: \"Open hat\" }, ({ lane }) => drums.drum(lane.notes(\"hits\")));\n");
    f.write("tracks/kick.ts", "import { track } from \"@m2/core\";\nimport { drums } from \"@m2/drums\";\n\nexport default track(\"kick\", { name: \"Kick\" }, ({ lane }) => {\n  const kick = drums.drum(lane.notes(\"hits\"));\n  return { out: kick };\n});\n");
    f.write("tracks/snare.ts", "import { track } from \"@m2/core\";\nimport { drums } from \"@m2/drums\";\n\nexport default track(\"snare\", { name: \"Snare\" }, (ctx) => {\n  return drums.drum(ctx.lane.notes(\"hits\"), { drum: \"snare\" });\n});\n");
    let mut s = f.open();
    cut_by(&mut s, "open-hat", Some("kick")).unwrap();
    assert!(
        f.read("tracks/open-hat.ts").contains("drums.drum(lane.notes(\"hits\"), { cutBy: kick.hits })"),
        "{}",
        f.read("tracks/open-hat.ts")
    );
    assert!(f.read("tracks/kick.ts").contains("return { out: kick, hits: lane.notes(\"hits\") };"), "{}", f.read("tracks/kick.ts"));
    cut_by(&mut s, "open-hat", Some("snare")).unwrap();
    assert!(
        f.read("tracks/snare.ts")
            .contains("return { out: drums.drum(ctx.lane.notes(\"hits\"), { drum: \"snare\" }), hits: ctx.lane.notes(\"hits\") };"),
        "{}",
        f.read("tracks/snare.ts")
    );
    assert_eq!(row_of(&s, "open-hat").by.as_deref(), Some("snare"));
}

// KIT-27: refused in words, writing nothing: its own hits, a track that isn't a Drum track, a Drum
// written where the menu can't be sure of it, and an export of the lane's name that is something else.
#[test]
fn cut_by_refuses_what_it_cant_be_sure_of() {
    let (f, mut s) = kit();
    let before = tree(&f);
    let says = |r: Result<Edited, EditError>, words: &str| {
        let e = r.unwrap_err().to_string();
        assert!(e.contains(words), "{e}");
    };
    says(cut_by(&mut s, "open-hat", Some("open-hat")), "Open hat can't be cut by its own hits");
    says(cut_by(&mut s, "open-hat", Some("drums")), "Drums isn't a Drum track");
    says(cut_by(&mut s, "drums", Some("hats")), "Drums plays no Drum from a lane of its own");
    assert_eq!(tree(&f), before);
    f.write("tracks/hats.ts", "import { track } from \"@m2/core\";\nimport { drums } from \"@m2/drums\";\n\nexport default track(\"hats\", { name: \"Hats\" }, ({ lane }) => ({ out: drums.drum(lane.notes(\"hits\"), { drum: \"hats\" }), hits: lane.notes(\"other\") }));\n");
    let mut s = f.open();
    says(cut_by(&mut s, "open-hat", Some("hats")), "Hats can't give its hits from here: it returns something else as hits");
}
