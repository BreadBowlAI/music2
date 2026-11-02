//! `layout.json` (FMT-19): read, written canonically, keeping what it doesn't know, and never
//! overwritten while it doesn't read.

use super::*;
use crate::Gestures;
use crate::testing::fixture;

fn order(s: &Session) -> Vec<String> {
    s.timeline().tracks.into_iter().filter(|t| t.track).map(|t| t.module).collect()
}

fn colour(s: &Session, k: &str) -> String {
    s.timeline().tracks.into_iter().find(|t| t.module == k).unwrap().colour
}

// PLANE-3, WRITE-21: reorder and recolour write only layout.json: no revision, one undo step each.
#[test]
fn reorder_and_recolour_write_only_layout() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let (rev, song) = (s.accepted().number, f.read("song.ts"));
    let done = s.reorder(&["keys".into(), "kick".into(), "hats".into(), "bass".into()]).unwrap();
    assert_eq!((done.revision, done.file.as_str()), (rev, "layout.json"));
    assert_eq!(order(&s), ["keys", "kick", "hats", "bass"]);
    let done = s.recolour("bass", "plum").unwrap();
    assert_eq!((done.revision, done.before.as_str(), done.after.as_str()), (rev, "amber", "plum"));
    assert_eq!(colour(&s, "bass"), "plum");
    assert_eq!(f.read("song.ts"), song);
    // Canonical: two-space JSON, the tracks in the order's order.
    assert_eq!(
        f.read("layout.json"),
        "{\n  \"order\": [\"keys\", \"kick\", \"hats\", \"bass\"],\n  \"tracks\": {\n    \"keys\": { \"colour\": \"indigo\" },\n    \
         \"kick\": { \"colour\": \"rose\" },\n    \"hats\": { \"colour\": \"teal\" },\n    \"bass\": { \"colour\": \"plum\" }\n  }\n}\n"
    );
    s.undo().unwrap();
    assert_eq!(colour(&s, "bass"), "amber");
    s.undo().unwrap();
    assert!(!f.0.join("layout.json").exists());
    assert_eq!(s.accepted().number, rev);
    // A reorder lists every track once.
    assert!(s.reorder(&["kick".into(), "hats".into()]).is_err());
    assert!(s.recolour("bass", "#ff0000").is_err());
}

// taste.md §2.7 rule 4: the first write freezes every track's colour, so later adds and reorders
// never recolour one.
#[test]
fn the_first_layout_write_freezes_every_colour() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    s.reorder(&["bass".into(), "kick".into(), "hats".into(), "keys".into()]).unwrap();
    // By place they would shift: bass first would be rose. Frozen, each keeps its own.
    assert_eq!((colour(&s, "bass"), colour(&s, "kick")), ("amber".into(), "rose".into()));
    let l = Layout::read(&f.read("layout.json")).unwrap();
    for k in ["kick", "hats", "bass", "keys"] {
        assert!(l.colour(k).is_some(), "{k} is written");
    }
}

// FMT-19: a `#rrggbb` written by hand reads as the nearest palette colour by hue.
#[test]
fn a_hex_colour_reads_as_the_nearest_name() {
    assert_eq!(named("#e4572e"), Some("coral"));
    assert_eq!(named("#d48e98"), Some("rose"));
    assert_eq!(named("#61b6ab"), Some("teal"));
    assert_eq!(named("#79aad8"), Some("blue"));
    assert_eq!(named("violet"), Some("violet"));
    assert_eq!(named("#zzzzzz"), None);
    assert_eq!(named("red"), None);
    let f = fixture("examples/first-song");
    f.write("layout.json", "{\"tracks\": {\"kick\": {\"colour\": \"#61b6ab\"}}}");
    let mut s = f.open();
    assert_eq!(colour(&s, "kick"), "teal");
    // The others take the colours no track uses, in the default order.
    assert_eq!((colour(&s, "hats"), colour(&s, "bass")), ("rose".into(), "amber".into()));
    s.recolour("hats", "lime").unwrap();
    let l = Layout::read(&f.read("layout.json")).unwrap();
    assert_eq!((l.colour("kick"), l.colour("hats")), (Some("teal"), Some("lime")));
}

// FMT-19: keys this version doesn't know are written back as read, in their order.
#[test]
fn unknown_layout_keys_survive_a_write() {
    let f = fixture("examples/first-song");
    f.write(
        "layout.json",
        "{\"zoom\": {\"b\": 1, \"a\": [1, 2]}, \"tracks\": {\"kick\": {\"colour\": \"rose\", \"height\": 72}}, \"canvas\": 3}",
    );
    let mut s = f.open();
    s.recolour("kick", "cyan").unwrap();
    assert_eq!(
        f.read("layout.json"),
        "{\n  \"order\": [\"kick\", \"hats\", \"bass\", \"keys\"],\n  \"tracks\": {\n    \"kick\": { \"colour\": \"cyan\", \"height\": 72 },\n    \
         \"hats\": { \"colour\": \"teal\" },\n    \"bass\": { \"colour\": \"amber\" },\n    \"keys\": { \"colour\": \"indigo\" }\n  },\n  \
         \"zoom\": {\n    \"b\": 1,\n    \"a\": [1, 2]\n  },\n  \"canvas\": 3\n}\n"
    );
}

// FMT-19: a track commented out for a while keeps its place and its colour in the file; only
// deleting it removes its key.
#[test]
fn a_missing_tracks_order_and_colour_survive_a_write() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    s.reorder(&["kick".into(), "hats".into(), "keys".into(), "bass".into()]).unwrap();
    s.recolour("keys", "olive").unwrap();
    // Keys commented out of the song.
    let song = f.read("song.ts");
    f.write(
        "song.ts",
        song.replace("import keys from \"./tracks/keys\";\n", "").replace("      keys: { in: keys.out, db: db(-5), pan: -0.25 },\n", ""),
    );
    assert!(s.refresh());
    assert_eq!(order(&s), ["kick", "hats", "bass"]);
    s.recolour("bass", "blue").unwrap();
    s.reorder(&["hats".into(), "kick".into(), "bass".into()]).unwrap();
    let l = Layout::read(&f.read("layout.json")).unwrap();
    // Keys was after hats: it stays after it.
    assert_eq!(l.order, ["hats", "keys", "kick", "bass"], "keys stays after the key before it");
    assert_eq!(l.colour("keys"), Some("olive"));
    // Back in the song: in its place, in its colour.
    f.write("song.ts", song);
    assert!(s.refresh());
    assert_eq!(order(&s), ["hats", "keys", "kick", "bass"]);
    assert_eq!(colour(&s, "keys"), "olive");
}

// R15, DIAG-2: a file that doesn't parse is never overwritten by reorder or recolour (they refuse,
// saying so); the view shows the defaults, and `layout.invalid` offers Reset layout.
#[test]
fn a_layout_that_doesnt_parse_is_never_overwritten() {
    let f = fixture("examples/first-song");
    let broken = "{\n<<<<<<< HEAD\n  \"order\": [\"kick\"]\n=======\n  \"order\": [\"bass\"]\n>>>>>>> theirs\n}\n";
    f.write("layout.json", broken);
    let mut s = f.open();
    assert_eq!(order(&s), ["kick", "hats", "bass", "keys"]);
    let e = s.reorder(&["bass".into(), "kick".into(), "hats".into(), "keys".into()]).unwrap_err().to_string();
    assert!(e.contains("layout.json doesn't read: fix it, or reset the layout"), "{e}");
    assert!(s.recolour("kick", "teal").is_err());
    assert_eq!(f.read("layout.json"), broken);
    let p = problems(&s);
    assert_eq!(
        (p.len(), p[0].code, p[0].fixes[0].code, p[0].fixes[0].title.as_str()),
        (1, "layout.invalid", "reset-layout", "Reset layout")
    );
}

// R15: while layout.json doesn't read, the track builders still work, and leave it as it is.
#[test]
fn the_track_builders_work_while_layout_doesnt_read() {
    let f = fixture("examples/first-song");
    f.write("layout.json", "{ not json");
    let mut s = f.open();
    s.add_track("@m2/synths:synth-pluck", None, None, None).unwrap();
    s.duplicate_track("hats").unwrap();
    s.delete_track("keys", false).unwrap();
    assert_eq!(f.read("layout.json"), "{ not json");
}

// DIAG-1, WRITE-21: Reset layout is one Tx, based on the unreadable text, so undo brings it back.
#[test]
fn reset_layout_is_one_undo_step() {
    let f = fixture("examples/first-song");
    f.write("layout.json", "{ not json");
    let mut s = f.open();
    let fix = problems(&s).remove(0).fixes.remove(0);
    s.apply_fix(&fix).unwrap();
    assert!(problems(&s).is_empty());
    assert!(Layout::read(&f.read("layout.json")).is_ok());
    s.undo().unwrap();
    assert_eq!(f.read("layout.json"), "{ not json");
}

// taste.md §2.7 rule 5: with all twelve used, a new track gets the least used colour.
#[test]
fn a_new_colour_is_the_first_unused_then_the_least_used() {
    assert_eq!(unused(["rose", "teal"]), "amber");
    let all: Vec<&str> = DEFAULT_ORDER.iter().copied().chain(["rose", "teal", "indigo"]).collect();
    assert_eq!(unused(all), "amber");
}
