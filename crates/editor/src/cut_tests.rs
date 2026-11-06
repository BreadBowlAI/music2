//! Cut by (KIT-27): a Drum's optional notes input that listens, picked on its card's row of another
//! track (CARD-35), which its label calls "Cut by". The same planner as a Duck's side writes it: the
//! other track's hits as an argument after the notes, an import of that track, and its lane handed
//! out beside its sound; None takes the argument out. One undo step, named by the tracks.

use std::collections::BTreeMap;

use super::*;
use crate::drums::DRUM;
use crate::testing::{Fixture, files_under, fixture};

fn tree(f: &Fixture) -> BTreeMap<String, Vec<u8>> {
    files_under(&f.0)
        .into_iter()
        .filter(|p| !p.strip_prefix(&f.0).unwrap().starts_with(".music2"))
        .map(|p| (p.strip_prefix(&f.0).unwrap().to_str().unwrap().to_string(), std::fs::read(&p).unwrap()))
        .collect()
}

/// The empty template with the Basic kit (Kick, Snare, Hats) and an Open hat added to it by + Drum.
fn kit() -> (Fixture, Session) {
    let f = fixture("templates/empty");
    let mut s = f.open();
    crate::drums::add_kit(&mut s, DRUM, "Basic kit", None).unwrap();
    crate::kits::add_drum(&mut s, "drums", "open hat").unwrap();
    (f, s)
}

/// The Drum call of `file`, as `file#ordinal`.
fn drum_in(s: &Session, file: &str) -> String {
    let x = s.accepted().compiled.sites.iter().find(|x| x.file == file && x.callee == "drums.drum").unwrap_or_else(|| panic!("{file}"));
    format!("{}#{}", x.file, x.ordinal)
}

/// Cut the Open hat's Drum by `by`.
fn cut(s: &mut Session, by: Option<&str>) -> Result<Edited, EditError> {
    let (at, id) = (s.accepted().number, drum_in(s, "tracks/open-hat.ts"));
    sidechain(s, at, &id, by)
}

/// The Open hat's Drum card's row of another track, as the view shows it.
fn row(s: &Session) -> Listens {
    let id = drum_in(s, "tracks/open-hat.ts");
    let v = crate::view::view(s);
    let card = v.modules.iter().find(|m| m.key == "open-hat").and_then(|m| m.nodes.iter().find(|n| n.id == id)).expect("the Drum's card");
    card.listens.clone().expect("its Cut by row")
}

// KIT-27, CARD-35: the Open hat's card has a Cut by row (its input's label), None and the song's other
// tracks that play from one notes lane, by name. Picking Hats is one undo step, named "Open hat cut
// by Hats": `hats.hits` after the notes, an import of the Hats, and the Hats' lane handed out beside
// its sound. None takes the argument out and leaves the import and the export. Undo and redo put
// back each, byte for byte.
#[test]
fn cut_by_writes_the_code_as_one_undo_step() {
    let (f, mut s) = kit();
    let r = row(&s);
    assert_eq!((r.label.as_deref(), r.track.as_deref(), r.optional, r.nothing.as_str()), (Some("Cut by"), None, true, "None"));
    let names: Vec<&str> = r.choices.iter().map(|(_, n)| n.as_str()).collect();
    assert_eq!(names, ["Hats", "Kick", "Snare"], "the other tracks that play notes, by name");
    let before = tree(&f);
    let done = cut(&mut s, Some("hats")).unwrap();
    assert_eq!((done.before.as_str(), done.after.as_str()), ("", "hats.hits"));
    let open = f.read("tracks/open-hat.ts");
    assert!(open.contains("import hats from \"./hats\";"), "{open}");
    assert!(open.contains("drums.drum(lane.notes(\"hits\"), hats.hits, { drum: \"open hat\" })"), "{open}");
    let hats = f.read("tracks/hats.ts");
    assert!(
        hats.contains("({ lane }) => ({ out: drums.drum(lane.notes(\"hits\"), { drum: \"hats\" }), hits: lane.notes(\"hits\") })"),
        "{hats}"
    );
    assert_eq!(row(&s).track.as_deref(), Some("hats"));
    assert_eq!(s.step_names().0.and_then(|n| n.name), Some("Open hat cut by Hats".to_string()), "named by the tracks");
    let rows: Vec<String> = crate::kits::drum_kits(&s).iter().flat_map(|k| k.rows.iter().map(|r| r.track.clone())).collect();
    assert!(rows.contains(&"open-hat".to_string()) && rows.contains(&"hats".to_string()), "still a kit's rows: {rows:?}");
    let after = tree(&f);
    crate::undo(&mut s).unwrap();
    assert_eq!(tree(&f), before, "one undo takes it all back");
    crate::redo(&mut s).unwrap();
    assert_eq!(tree(&f), after);
    assert!(cut(&mut s, Some("hats")).unwrap_err().to_string().contains("Drum's Cut by is Hats already."));
    cut(&mut s, Some("kick")).unwrap();
    assert!(!f.read("tracks/open-hat.ts").contains("hats.hits") && f.read("tracks/open-hat.ts").contains("kick.hits"));
    let done = cut(&mut s, None).unwrap();
    assert_eq!((done.before.as_str(), done.after.as_str()), ("kick.hits", ""));
    let open = f.read("tracks/open-hat.ts");
    assert!(
        open.contains("drums.drum(lane.notes(\"hits\"), { drum: \"open hat\" })") && open.contains("import kick from \"./kick\";"),
        "{open}"
    );
    assert_eq!(row(&s).track, None);
    assert_eq!(s.step_names().0.and_then(|n| n.name), Some("Open hat cut by nothing".to_string()));
}

// KIT-27: a Drum's call with no options gets the argument alone; a track that returns an object
// adds its lane to it; a function with a block hands it out beside its sound too.
#[test]
fn cut_by_writes_into_each_form_it_can_follow() {
    let (f, s) = kit();
    drop(s);
    f.write("tracks/open-hat.ts", "import { track } from \"@m2/core\";\nimport { drums } from \"@m2/drums\";\n\nexport default track(\"open-hat\", { name: \"Open hat\" }, ({ lane }) => drums.drum(lane.notes(\"hits\")));\n");
    f.write("tracks/kick.ts", "import { track } from \"@m2/core\";\nimport { drums } from \"@m2/drums\";\n\nexport default track(\"kick\", { name: \"Kick\" }, ({ lane }) => {\n  const kick = drums.drum(lane.notes(\"hits\"));\n  return { out: kick };\n});\n");
    f.write("tracks/snare.ts", "import { track } from \"@m2/core\";\nimport { drums } from \"@m2/drums\";\n\nexport default track(\"snare\", { name: \"Snare\" }, (ctx) => {\n  return drums.drum(ctx.lane.notes(\"hits\"), { drum: \"snare\" });\n});\n");
    let mut s = f.open();
    cut(&mut s, Some("kick")).unwrap();
    assert!(f.read("tracks/open-hat.ts").contains("drums.drum(lane.notes(\"hits\"), kick.hits)"), "{}", f.read("tracks/open-hat.ts"));
    assert!(f.read("tracks/kick.ts").contains("return { out: kick, hits: lane.notes(\"hits\") };"), "{}", f.read("tracks/kick.ts"));
    cut(&mut s, Some("snare")).unwrap();
    let snare = f.read("tracks/snare.ts");
    assert!(
        snare.contains("return { out: drums.drum(ctx.lane.notes(\"hits\"), { drum: \"snare\" }), hits: ctx.lane.notes(\"hits\") };"),
        "{snare}"
    );
    assert_eq!(row(&s).track.as_deref(), Some("snare"));
}

// KIT-27, CARD-35 (review C5): only tracks the planner can write are offered; another is refused
// in words a musician reads, writing nothing: its own, a bus with no notes, a track written in the
// song (no file of its own to import), and an export of the lane's name that is something else.
#[test]
fn cut_by_offers_and_writes_only_what_it_can() {
    let (f, s) = kit();
    drop(s);
    let song = f.read("song.ts").replace(
        "// const beat = track(\"beat\", { name: \"Beat\" }, ({ lane }) => drums.drum(lane.notes(\"hits\"))); // drums",
        "const beat = track(\"beat\", { name: \"Beat\" }, ({ lane }) => drums.drum(lane.notes(\"hits\")));",
    );
    f.write("song.ts", song.replace("mixer({", "mixer({\n      beat: { in: beat.out, to: \"drums\" },"));
    let mut s = f.open();
    assert!(s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    let before = tree(&f);
    assert!(!row(&s).choices.iter().any(|(k, _)| k == "beat"), "a track written in the song isn't offered");
    let says = |r: Result<Edited, EditError>, words: &str| {
        let e = r.unwrap_err().to_string();
        assert!(e.contains(words), "{e}");
    };
    says(cut(&mut s, Some("open-hat")), "Open hat is the sound it works on");
    says(cut(&mut s, Some("drums")), "Drums doesn't play from one notes lane");
    says(cut(&mut s, Some("beat")), "Beat is written in song.ts, which tracks/open-hat.ts can't import it from: give it a file of its own");
    assert_eq!(tree(&f), before);
    drop(s);
    f.write("tracks/hats.ts", "import { track } from \"@m2/core\";\nimport { drums } from \"@m2/drums\";\n\nexport default track(\"hats\", { name: \"Hats\" }, ({ lane }) => ({ out: drums.drum(lane.notes(\"hits\"), { drum: \"hats\" }), hits: lane.mask(\"other\") }));\n");
    let mut s = f.open();
    says(cut(&mut s, Some("hats")), "Hats can't hand out its notes from here: it returns something else as hits");
}
