//! Mute in song in the delta (WRITE-8, MON-5): a `muted` change is `ModuleChange::Muted`, which a
//! `Confined` Tx on the track's options declares; undo turns it back.

use m2_song::Key;
use m2_song::tx::{Confined, Declared, Expect, ModuleChange, Range};

use super::tests::{Dir, based, p, replace};

/// Kick's options in first-song's `tracks/kick.ts`, set to `to`: a `Confined` Tx declaring `modules`.
fn options(d: &Dir, from: &str, to: &str, modules: Vec<ModuleChange>) -> m2_song::Tx {
    let text = d.read("tracks/kick.ts");
    let (op, r) = replace("tracks/kick.ts", &text, "track(\"kick\"", from, to);
    let ranges = [(p("tracks/kick.ts"), Declared::Ranges(vec![Range::bytes(r.start as u32, r.end as u32)]))].into();
    based(d, vec![op], Expect::Confined(Confined { ranges, modules, ..Confined::default() }))
}

fn muted(on: bool) -> ModuleChange {
    ModuleChange::Muted { module: Key::new("kick").unwrap(), muted: on }
}

// WRITE-8, MON-5: adding `muted: true` changes no node; the delta says the kick is muted in the
// song, and only that; taking it out says so back. A Tx that doesn't declare it is refused.
#[test]
fn a_mute_in_song_toggle_is_a_muted_change() {
    let d = Dir::first_song();
    let mut s = d.open();
    let undeclared = options(&d, "{ name: \"Kick\" }", "{ name: \"Kick\", muted: true }", vec![]);
    assert!(s.commit(undeclared).is_err(), "Confined declares the module change");
    let on = options(&d, "{ name: \"Kick\" }", "{ name: \"Kick\", muted: true }", vec![muted(true)]);
    let done = s.commit(on).unwrap_or_else(|e| panic!("{e}"));
    let delta = &done.delta;
    assert_eq!(delta.modules, [muted(true)]);
    assert!(delta.added.is_empty() && delta.removed.is_empty() && delta.rewired.is_empty() && delta.values.is_empty(), "{delta:?}");
    assert_eq!(s.accepted().compiled.flat.muted, ["kick"]);
    let off = options(&d, "{ name: \"Kick\", muted: true }", "{ name: \"Kick\" }", vec![muted(false)]);
    assert_eq!(s.commit(off).unwrap_or_else(|e| panic!("{e}")).delta.modules, [muted(false)]);
    assert!(s.accepted().compiled.flat.muted.is_empty());
}
