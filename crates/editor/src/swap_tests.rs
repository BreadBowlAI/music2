//! Change instrument on the fixtures and templates: what it writes, byte for byte, that it is one
//! undo step and stays in the track, and what is refused, in words.

use std::collections::BTreeMap;

use super::*;
use crate::testing::{Fixture, MANIFEST, files_under, fixture};

/// Every file of the project but `.music2/`, by path, with its bytes.
fn tree(f: &Fixture) -> BTreeMap<String, Vec<u8>> {
    files_under(&f.0)
        .into_iter()
        .filter(|p| !p.strip_prefix(&f.0).unwrap().starts_with(".music2"))
        .map(|p| (p.strip_prefix(&f.0).unwrap().to_str().unwrap().to_string(), std::fs::read(&p).unwrap()))
        .collect()
}

fn refusal(r: Result<Edited, EditError>) -> String {
    match r {
        Err(EditError::Rejected(why)) => why,
        other => panic!("refused, not {other:?}"),
    }
}

// First-song's hats play the Drum, its menu at hats: to FM metal, the call reads the same notes,
// its plain settings go with the old call, and so does `drums`, which nothing else reads, for
// `fm`. One revision, labelled as the toast says it.
#[test]
fn swap_replaces_the_call_and_its_import() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let rev = s.accepted().number;
    let done = swap(&mut s, "hats", "@m2/synths:fm-metal").unwrap();
    assert_eq!(
        f.read("tracks/hats.ts"),
        "import { track } from \"@m2/core\";\nimport { fm } from \"@m2/synths\";\n\n\
         export default track(\"hats\", { name: \"Hats\" }, ({ lane }) => fm.metal(lane.notes(\"hits\")));\n"
    );
    assert_eq!((done.file.as_str(), done.before.as_str(), done.after.as_str()), ("tracks/hats.ts", "Hats", "Hats plays FM metal"));
    assert_eq!(done.revision, rev + 1, "one revision");
    assert!(!s.refresh(), "the session took in its own write");
}

// An import the rest of the file still reads stays (`ms`, read by the kick's `follow`), and so
// does the binding the new instrument shares with the old (`synth`, from the pad to the strings).
#[test]
fn swap_keeps_a_used_import() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    swap(&mut s, "kick", "@m2/synths:fm-metal").unwrap();
    assert_eq!(
        f.read("tracks/kick.ts"),
        "import { ms, track } from \"@m2/core\";\nimport { follow } from \"std/dsp\";\nimport { fm } from \"@m2/synths\";\n\n\
         export default track(\"kick\", { name: \"Kick\" }, ({ lane }) => {\n  const out = fm.metal(lane.notes(\"hits\"));\n  \
         // Exported so other tracks can duck to the kick.\n  return { out, env: follow(out, { release: ms(150) }) };\n});\n"
    );
    swap(&mut s, "keys", "@m2/synths:synth-pad").unwrap();
    let pad = f.read("tracks/keys.ts");
    swap(&mut s, "keys", "@m2/synths:synth-strings").unwrap();
    assert_eq!(f.read("tracks/keys.ts"), pad.replace("synth.pad(", "synth.strings("), "the synths import stays as it was");
}

// First-song's bass sets its cutoff from a lane and an LFO: changing its instrument would lose
// that, so it is refused, in words, and nothing is written.
#[test]
fn swap_refuses_a_setting_that_follows_code() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let before = tree(&f);
    let why = refusal(swap(&mut s, "bass", "@m2/synths:synth-bass"));
    assert_eq!(why, "Bass's cutoff is set by code here. Change its instrument in the Code view.");
    assert_eq!(tree(&f), before);
}

// Two instruments, none (a sound built from parts), and the same one again: each refused in words.
#[test]
fn swap_refuses_two_instruments() {
    let f = Fixture::new();
    f.write("music2.toml", MANIFEST);
    f.write(
        "song.ts",
        "import { song } from \"@m2/core\";\nimport { mixer } from \"std/mix\";\nimport duo from \"./tracks/duo\";\nimport clap from \"./tracks/clap\";\n\
         export default song(() => mixer({ duo: duo.out, clap: clap.out }));\n",
    );
    f.write(
        "tracks/duo.ts",
        "import { track } from \"@m2/core\";\nimport { pad, pluck } from \"@m2/synths\";\nimport { sum } from \"std/math\";\n\n\
         export default track(\"duo\", { name: \"Duo\" }, ({ lane }) => sum(pluck(lane.notes(\"notes\")), pad(lane.notes(\"notes\"))));\n",
    );
    f.write(
        "tracks/clap.ts",
        "import { track } from \"@m2/core\";\nimport { noise } from \"std/dsp\";\n\nexport default track(\"clap\", { name: \"Clap\" }, () => noise(1));\n",
    );
    let mut s = f.open();
    assert_eq!(refusal(swap(&mut s, "duo", "@m2/synths:synth-pad")), "Duo plays 2 instruments. Change them in code.");
    assert_eq!(refusal(swap(&mut s, "clap", "@m2/synths:synth-pad")), "Clap is built from parts, not one instrument. Change it in code.");
    swap(&mut s, "duo", "@m2/synths:synth-pad").unwrap_err();
    let f2 = fixture("templates/starter");
    let mut s2 = f2.open();
    assert_eq!(refusal(swap(&mut s2, "clap", "@m2/synths:synth-pad")), "Clap is built from parts, not one instrument. Change it in code.");
    swap(&mut s2, "hats", "@m2/synths:fm-metal").unwrap();
    assert_eq!(refusal(swap(&mut s2, "hats", "@m2/synths:fm-metal")), "Hats plays FM metal already.");
    assert!(matches!(swap(&mut s2, "hats", "std:no-such"), Err(EditError::NotFound(_))));
    // The drum machine is no instrument to change to (KIT-2): code plays it, no list offers it.
    assert!(matches!(swap(&mut s2, "hats", "@m2/drums:drum-kit"), Err(EditError::NotFound(_))));
}

// The starter's bass writes plain settings: they go with the old call, and so does `hz`, which
// only they read; the new instrument starts at its own settings, as a new track's does.
#[test]
fn swap_drops_plain_settings() {
    let f = fixture("templates/starter");
    let mut s = f.open();
    swap(&mut s, "bass", "@m2/synths:synth-pad").unwrap();
    assert_eq!(
        f.read("tracks/bass.ts"),
        "import { track } from \"@m2/core\";\nimport { synth } from \"@m2/synths\";\n\n\
         export default track(\"bass\", { name: \"Bass\" }, ({ lane }) => synth.pad(lane.notes(\"notes\")));\n"
    );
    // A pad in the middle of other code: only the call changes, and its import goes after the last.
    let chords = f.read("tracks/chords.ts");
    swap(&mut s, "chords", "@m2/synths:synth-strings").unwrap();
    assert_eq!(
        f.read("tracks/chords.ts"),
        chords
            .replace(
                "import { hz, track } from \"@m2/core\";\nimport { pad } from \"@m2/synths\";\n",
                "import { track } from \"@m2/core\";\n"
            )
            .replace("from \"@m2/effects\";\n", "from \"@m2/effects\";\nimport { synth } from \"@m2/synths\";\n")
            .replace("pad(lane.notes(\"notes\"), { cutoff: hz(1600) })", "synth.strings(lane.notes(\"notes\"))")
    );
}

// One undo puts every byte of the project back, and a redo writes it again.
#[test]
fn undo_puts_every_byte_back() {
    for (fixture_dir, module, to) in [
        ("examples/first-song", "hats", "@m2/synths:fm-metal"),
        ("templates/starter", "bass", "@m2/synths:synth-pad"),
        ("templates/starter", "hook", "@m2/synths:fm-bell"),
    ] {
        let f = fixture(fixture_dir);
        let mut s = f.open();
        let before = tree(&f);
        swap(&mut s, module, to).unwrap();
        let after = tree(&f);
        assert_ne!(after, before);
        crate::undo(&mut s).unwrap();
        assert_eq!(tree(&f), before, "{fixture_dir} {module}");
        crate::redo(&mut s).unwrap();
        assert_eq!(tree(&f), after, "{fixture_dir} {module}");
    }
}

// Only the track's file changes, and in it only the call and its imports: its name, its key, its
// mute in the song, its lane and notes, the song's mixer and effects (the bass's duck reads it the
// same), the layout, and every other track stay. Its lanes are the same lanes.
#[test]
fn the_change_stays_in_the_track() {
    let f = fixture("examples/first-song");
    f.write("tracks/keys.ts", f.read("tracks/keys.ts").replace("{ name: \"Keys\" }", "{ name: \"Keys\", muted: true }"));
    let mut s = f.open();
    let before = tree(&f);
    let lanes = |s: &Session, k: &str| s.accepted().compiled.ir.modules.iter().find(|m| m.key == k).map(|m| m.lanes.clone());
    let (keys_lanes, kick_nodes) =
        (lanes(&s, "keys"), s.accepted().compiled.ir.modules.iter().find(|m| m.key == "kick").unwrap().nodes.len());
    swap(&mut s, "keys", "@m2/synths:fm-keys").unwrap();
    let after = tree(&f);
    let changed: Vec<&String> = after.keys().filter(|k| before.get(*k) != after.get(*k)).collect();
    assert_eq!(changed, ["tracks/keys.ts"]);
    let keys = f.read("tracks/keys.ts");
    assert_eq!(
        keys,
        "import { track } from \"@m2/core\";\nimport { fm } from \"@m2/synths\";\n\n\
         export default track(\"keys\", { name: \"Keys\", muted: true }, ({ lane }) => fm.keys(lane.notes(\"chords\")));\n"
    );
    let m = s.accepted().compiled.ir.modules.iter().find(|m| m.key == "keys").unwrap();
    assert!(m.muted && m.label() == "Keys", "the mute and the name stay");
    assert_eq!(lanes(&s, "keys"), keys_lanes, "the same lanes");
    assert_eq!(s.accepted().compiled.ir.modules.iter().find(|m| m.key == "kick").unwrap().nodes.len(), kick_nodes);
}
