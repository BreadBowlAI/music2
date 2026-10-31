//! The delta's names (WRITE-8): an insertion renames nothing, names are unique, and two processes
//! hash one Tx's delta alike.

use std::collections::BTreeMap;

use m2_ir::{Node, Op};
use m2_song::tx::{Anchor, Confined, Declared, Edit, Expect, ModuleChange, Op as TxOp, Pos, Range, Spot};
use m2_song::{Key, Sha};

use super::delta::{GraphDelta, Positions, Side};
use super::delta_between;
use super::tests::{Dir, MANIFEST, SAW, based, p, wrap};
use crate::Session;

/// Every node of `nodes`, voices included.
fn count(nodes: &[Node]) -> usize {
    nodes.iter().map(|n| 1 + if let Op::Poly { body, .. } = &n.op { count(&body.nodes) } else { 0 }).sum()
}

/// An insertion of `text` at byte `at`.
fn insert(at: usize, text: &str) -> Edit {
    Edit { start: Pos::Byte(at as u32), end: Pos::Byte(at as u32), expected: String::new(), text: text.into() }
}

/// The step 6 gate's Tx: one unconnected track inserted at the top of `song.ts`.
fn probe(d: &Dir) -> m2_song::Tx {
    let text =
        "import { osc } from \"std/dsp\";\nimport { track } from \"@m2/core\";\nconst probe = track(\"probe\", () => osc.sine(440));\n";
    let op = TxOp::TextEdit { file: p("song.ts"), edits: vec![insert(0, text)] };
    based(d, vec![op], Expect::Seen(Sha::of(b"to be learned")))
}

// WRITE-8, the step 6 gate: inserting one unconnected call at the top of `song.ts` gives a delta
// of exactly that call's nodes, with the same hash in two processes.
#[test]
fn delta_of_an_inserted_call() {
    if let Ok(dir) = std::env::var("M2_DELTA_OF") {
        // The second process: check the same Tx on its own copy and say the hash.
        let d = Dir(dir.into());
        let mut s = Session::open_read_only(&d.0, super::tests::std_packages()).unwrap();
        let checked = s.check(&probe(&d)).unwrap();
        println!("\nhash={}", checked.hash.prefixed());
        std::mem::forget(d);
        return;
    }
    let d = Dir::first_song();
    let mut s = d.open();
    let checked = s.check(&probe(&d)).unwrap();
    let delta = &checked.delta;
    assert_eq!(delta.modules, [ModuleChange::Added(Key::new("probe").unwrap())]);
    assert!(!delta.added.is_empty() && delta.added.iter().all(|n| n.name[0] == "probe"), "{:?}", delta.added);
    let module = checked.candidate().compiled.ir.modules.iter().find(|m| m.key == "probe").unwrap();
    assert_eq!(delta.added.len(), count(&module.nodes), "every node of the new track");
    assert!(delta.removed.is_empty() && delta.rewired.is_empty() && delta.values.is_empty() && delta.lanes.is_empty(), "{delta:?}");

    // The same Tx in another process, on another copy: the same hash (hash maps are seeded per
    // process, so an order that depended on one would show).
    let other = Dir::first_song();
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "tx::delta_tests::delta_of_an_inserted_call", "--nocapture", "--test-threads", "1"])
        .env("M2_DELTA_OF", &other.0)
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&out.stdout);
    let theirs = said.split("hash=").nth(1).and_then(|h| h.split_whitespace().next()).unwrap_or_else(|| panic!("no hash: {said}"));
    assert_eq!(theirs, checked.hash.prefixed());
}

// WRITE-8: every node of every fixture has a name of its own.
#[test]
fn delta_names_are_unique_on_every_fixture() {
    let fixtures = ["examples/first-song", "examples/resample", "examples/with-plugins", "templates/starter", "templates/empty"];
    let copies = fixtures.iter().map(|f| crate::gate_tests::fixture(f)).chain([crate::gate_tests::every_component()]);
    for f in copies {
        let s = f.open();
        let rev = s.accepted();
        let coords = BTreeMap::new();
        let g = Side { compiled: &rev.compiled, snapshot: &rev.snapshot, at: Positions { coords: &coords, new: false } }.graph();
        let nodes: usize = rev.compiled.ir.modules.iter().map(|m| count(&m.nodes)).sum();
        assert_eq!(g.nodes.len(), nodes, "{}: a name built twice", f.0.display());
    }
}

// WRITE-6: writing in a declared control a call leaves out (inherited → written) is a value
// change, by its slot: no node is renamed.
#[test]
fn an_inserted_argument_is_a_value_change() {
    let d = Dir::first_song();
    let mut s = d.open();
    // master's ceiling, left out: written after its one argument, in a new options object.
    let sites = &s.accepted().compiled.sites;
    let site = sites.iter().find(|x| x.file == "song.ts" && x.callee == "master").unwrap().clone();
    let at = sites.iter().find(|x| x.file == "song.ts" && x.callee == "mixer").unwrap().end;
    let slot = "ceiling".to_string();
    let text = ", { ceiling: -2 }".to_string();
    let op = TxOp::TextEdit {
        file: p("song.ts"),
        edits: vec![Edit { start: Pos::Byte(at), end: Pos::Byte(at), expected: String::new(), text }],
    };
    let anchor = Anchor::Slot { site: Spot { file: p("song.ts"), range: Range::bytes(site.start, site.end) }, slot };
    let done = s.commit(based(&d, vec![op], Expect::Values(vec![(anchor, -2.0)]))).unwrap();
    assert_eq!(done.delta.values.len(), 1, "{:?}", done.delta);
    assert!(!done.delta.structural());
    assert!(d.read("song.ts").contains("ceiling: -2"), "{}", d.read("song.ts"));
    // Undo takes it out again: the control plays its default.
    s.undo().unwrap();
    assert!(!d.read("song.ts").contains("ceiling"));
}

// R17: a second read of a lane, added anywhere, renames no node.
#[test]
fn a_second_read_of_a_lane_renames_nothing() {
    let d = Dir::first_song();
    let mut s = d.open();
    let kick = d.read("tracks/kick.ts");
    let at = kick.find("  const out").unwrap();
    let op = TxOp::TextEdit { file: p("tracks/kick.ts"), edits: vec![insert(at, "  const again = lane.notes(\"hits\");\n")] };
    let checked = s.check(&based(&d, vec![op], Expect::Seen(Sha::of(b"x")))).unwrap();
    assert!(checked.delta.removed.is_empty(), "{:?}", checked.delta.removed);
    assert!(checked.delta.rewired.is_empty(), "{:?}", checked.delta.rewired);
}

// WRITE-8: one value edit is one value change, and nothing in a file it didn't touch moves.
#[test]
fn a_value_edit_is_one_value_change() {
    let d = Dir::first_song();
    let mut s = d.open();
    let bass = d.read("tracks/bass.ts");
    let at = bass.find("period: 0.5").unwrap() + "period: ".len();
    let op = TxOp::TextEdit {
        file: p("tracks/bass.ts"),
        edits: vec![Edit { start: Pos::Byte(at as u32), end: Pos::Byte(at as u32 + 3), expected: "0.5".into(), text: "0.25".into() }],
    };
    let anchor = Anchor::Written { file: p("tracks/bass.ts"), range: Range::bytes(at as u32, at as u32 + 3) };
    let checked = s.check(&based(&d, vec![op], Expect::Values(vec![(anchor, 0.25)]))).unwrap();
    assert_eq!(checked.delta.values.len(), 1);
    assert_eq!(checked.delta.values[0].node[0], "bass");
}

// WRITE-8, R5 (the step 6 design's 5.2, 14.4): an effect added by two insertions around a call
// keeps every name the wrapped call's nodes had: the delta is the effect's nodes added (all made
// inside the insertions) and the wrapped output's readers rewired to it; nothing is removed and no
// value changes.
#[test]
fn a_wrap_keeps_the_wrapped_calls_names() {
    let d = Dir::with(&[("song.ts", SAW)]);
    let mut s = d.open();
    let (tx, _) = wrap(&d);
    let checked = s.check(&tx).unwrap();
    let delta = &checked.delta;
    assert!(delta.removed.is_empty() && delta.values.is_empty() && delta.modules.is_empty(), "{delta:?}");
    assert!(delta.added.iter().any(|n| n.op["kernel"] == serde_json::json!("std:delay")), "{:?}", delta.added);
    // Every added node was made by the new call, which starts in the first insertion.
    assert!(delta.added.iter().all(|n| n.name.iter().any(|seg| seg.contains("song.ts:e0."))), "{:?}", delta.added);
    // Each rewired input now reads an added node, and read the wrapped call's output before.
    assert!(!delta.rewired.is_empty());
    let added: Vec<&Vec<String>> = delta.added.iter().map(|n| &n.name).collect();
    for r in &delta.rewired {
        assert!(r.to.as_ref().is_some_and(|t| added.contains(&&t.node)), "{r:?}");
        assert!(r.from.as_ref().is_some_and(|f| !added.contains(&&f.node)), "{r:?}");
    }
    // The wrapped call's own nodes keep their names: its saw is neither removed nor added again.
    assert!(delta.added.iter().all(|n| n.op["kernel"] != serde_json::json!("std:saw")), "{:?}", delta.added);
}

/// A one-track song: a kick playing lane `hits`.
const KICK_SONG: &str = "import { secs, song, track } from '@m2/core';\nimport { kick } from '@m2/synths';\n\
                         const k = track('kick', ({ lane }) => kick(lane.notes('hits')));\nexport default song(() => k.out);\n";
const HITS: &str = r#"{"clips": [{"id": "a", "start": 0, "length": 4, "notes": [{"pitch": "C1", "start": 0, "length": 1}]}]}"#;

/// The delta from the revision accepted now to the one `change` makes: what the engine reads
/// between what plays and what is accepted (the step 6 design's 5.5).
fn after(d: &Dir, s: &mut Session, change: impl FnOnce(&Dir)) -> GraphDelta {
    let old = s.accepted_arc();
    change(d);
    assert!(s.refresh(), "a new revision");
    delta_between(&old, s.accepted())
}

// PLANE-1, RENDER-26 (the step 6 design's 5.5, which replaces `plane::lane_swaps`): a lane file,
// whoever wrote it, changes that lane's data alone, which the engine swaps; code that leaves the
// graph as it was changes nothing; a lane file that doesn't read keeps its data (FMT-9); a new
// node, a value and the arrangement each say what they are, and a rate change is the manifest's.
#[test]
fn between_revisions_lane_data_alone_is_a_lane_change() {
    let d = Dir::with(&[("song.ts", KICK_SONG), ("data/kick/hits.json", HITS)]);
    let mut s = d.open();
    let lanes = |delta: &GraphDelta| delta.lanes.iter().map(|a| format!("{}/{}", a.module, a.lane)).collect::<Vec<_>>();
    let more = HITS.replace(r#""length": 1}]"#, r#""length": 1}, {"pitch": "D1", "start": 2, "length": 1}]"#);
    let delta = after(&d, &mut s, |d| d.write("data/kick/hits.json", &more));
    assert_eq!((lanes(&delta), delta.structural(), delta.values.len()), (vec!["kick/hits".to_string()], false, 0));
    // Code that leaves the graph as it was changes nothing.
    assert!(after(&d, &mut s, |d| d.write("song.ts", &format!("{KICK_SONG}// a comment\n"))).is_empty());
    // A lane file that doesn't read keeps playing its data (FMT-9): nothing changed.
    assert!(after(&d, &mut s, |d| d.write("data/kick/hits.json", "{")).is_empty());
    assert_eq!(lanes(&after(&d, &mut s, |d| d.write("data/kick/hits.json", HITS))), ["kick/hits"]);
    // A value written for a control the call left out: a value change, by its slot.
    let longer = KICK_SONG.replace("kick(lane.notes('hits'))", "kick(lane.notes('hits'), { decay: secs(0.9) })");
    let delta = after(&d, &mut s, |d| d.write("song.ts", &longer));
    assert!(!delta.structural() && delta.values.len() == 1 && delta.lanes.is_empty(), "{delta:?}");
    // A new node is structural; the arrangement says so.
    let spare = longer.replace("export default", "const spare = kick(lane.notes('hits'));\nexport default");
    let spare = spare.replace("const spare = kick(lane.notes('hits'));", "const k2 = track('k2', ({ lane }) => kick(lane.notes('hits')));");
    let delta = after(&d, &mut s, |d| d.write("song.ts", &spare));
    assert!(delta.structural(), "{delta:?}");
    assert!(after(&d, &mut s, |d| d.write("data/arrangement.json", r#"{"tempo": [{"beat": 0, "bpm": 90}]}"#)).arrangement);
    // The rate is the manifest's: a code revision, at another rate (the engine replaces itself).
    let code_rev = s.accepted().code_rev;
    after(&d, &mut s, |d| d.write("music2.toml", &MANIFEST.replace("48000", "44100")));
    assert!(s.accepted().code_rev > code_rev && s.accepted().project.sample_rate() == 44100);
}

// WRITE-13, WRITE-22 (once `plane::code_changed`'s): only code and the manifest move `code_rev`,
// which is what a control id names, so only they carry a gesture lease; lanes, state blobs, the
// arrangement and layout.json leave it.
#[test]
fn only_code_and_the_manifest_move_the_code_revision() {
    let d = Dir::with(&[("song.ts", KICK_SONG), ("data/kick/hits.json", HITS)]);
    let mut s = d.open();
    let mut moved = |write: &dyn Fn(&Dir)| {
        let before = s.accepted().code_rev;
        write(&d);
        s.refresh();
        s.accepted().code_rev != before
    };
    assert!(!moved(&|d| d.write("data/kick/hits.json", &HITS.replace("\"length\": 1}", "\"length\": 2}"))));
    assert!(!moved(&|d| d.write("data/kick/kick.state", "a blob")));
    assert!(!moved(&|d| d.write("layout.json", "{}")));
    assert!(!moved(&|d| d.write("data/arrangement.json", r#"{"tempo": [{"beat": 0, "bpm": 90}]}"#)));
    assert!(moved(&|d| d.write("song.ts", &format!("{KICK_SONG}// a comment\n"))));
    assert!(moved(&|d| d.write("tracks/new.ts", "export const x = 1;\n")));
    assert!(moved(&|d| d.write("music2.toml", &MANIFEST.replace("48000", "44100"))));
    assert!(!moved(&|_| {}));
}

// RENDER-26 (once `plane::lane_swaps`'s binaries rule): an asset a clip plays whose bytes changed
// is in the delta, so the engine builds a renderer for it; one no clip plays changes nothing.
#[test]
fn an_asset_a_clip_plays_is_in_the_delta() {
    let d = Dir::new();
    let from = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/resample");
    let mut stack = vec![from.clone()];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).unwrap().flatten() {
            let f = e.path();
            if f.is_dir() {
                stack.push(f);
            } else {
                d.write_bytes(&m2_song::path::slashed(f.strip_prefix(&from).unwrap()), &std::fs::read(&f).unwrap());
            }
        }
    }
    let mut s = d.open();
    let delta = after(&d, &mut s, |d| d.write_bytes("assets/unplayed.wav", b"RIFF and nothing"));
    assert!(delta.is_empty(), "{delta:?}");
    let wav = std::fs::read(d.0.join("assets/keys-bounce.wav")).unwrap();
    let mut other = wav.clone();
    *other.last_mut().unwrap() ^= 1;
    let delta = after(&d, &mut s, |d| d.write_bytes("assets/keys-bounce.wav", &other));
    assert_eq!(delta.assets, ["keys-bounce.wav"], "{delta:?}");
}

/// Whether some segment of `name` is a frame of component `id` (`@m2/effects:delay@…`).
fn in_frame(name: &[String], id: &str) -> bool {
    name.iter().any(|seg| seg.starts_with(&format!("{id}@")))
}

// ID-13 (identity note F2), RENDER-25: a wrap saved as a file, as an agent or the code editor saves
// it, keeps every name the wrapped call's nodes had, on the engine's path (`delta_between`): on the
// reference project's `pluck(…)` and nested `reverb(pad(…))`, the delta adds the delay's nodes and
// removes none, and the unwrap removes exactly them again.
#[test]
fn a_wrap_by_a_file_write_keeps_the_wrapped_names() {
    let d = Dir::new();
    crate::reference::write(&d.0, &crate::reference::Reference { tracks: 3, repeats: 1, ..crate::reference::Reference::P16 }).unwrap();
    let mut s = d.open();
    for (file, call) in [("tracks/t02.ts", "pluck(lane.notes(\"notes\"))"), ("tracks/t03.ts", "pad(lane.notes(\"notes\"))")] {
        let text = d.read(file);
        assert!(text.contains(call), "{text}");
        let wrapped = text.replace(call, &format!("delay({call})"));
        let delta = after(&d, &mut s, |d| d.write(file, &wrapped));
        assert!(delta.removed.is_empty() && delta.values.is_empty(), "{file}: {delta:?}");
        assert!(!delta.added.is_empty() && delta.added.iter().all(|n| in_frame(&n.name, "@m2/effects:delay")), "{file}: {:?}", delta.added);
        let delta = after(&d, &mut s, |d| d.write(file, &text));
        assert!(delta.added.is_empty() && delta.values.is_empty(), "{file}: {delta:?}");
        assert!(
            !delta.removed.is_empty() && delta.removed.iter().all(|n| in_frame(&n.name, "@m2/effects:delay")),
            "{file}: {:?}",
            delta.removed
        );
    }
}

/// A saw through a lowpass, then a delay, each with a literal on its own line.
const ADJACENT: &str = "import { hz, song } from \"@m2/core\";\nimport { osc, svf } from \"std/dsp\";\nimport { delay } from \"@m2/effects\";\n\
                        export default song(() => {\n  const a = svf.lowpass(osc.saw(hz(110)), hz(800));\n  const b = delay(a, { mix: 0.3 });\n  return b;\n});\n";

// ID-13 (identity note F1), WRITE-8: two literals on adjacent lines edited in one save are two
// value changes, not a structural edit that would restart the delay between them.
#[test]
fn adjacent_value_edits_are_values() {
    let d = Dir::with(&[("song.ts", ADJACENT)]);
    let mut s = d.open();
    let delta = after(&d, &mut s, |d| d.write("song.ts", &ADJACENT.replace("hz(800)", "hz(950)").replace("mix: 0.3", "mix: 0.25")));
    assert!(!delta.structural() && delta.values.len() == 2, "{delta:?}");
}

// ID-15, PKG-9 (identity note F3): a component swapped at one call is a new instance, though its
// `(` is kept, so the old delay's `mix` is never the new chorus's; an alias of the same component
// written at the call (`delay` → `echo`) is the same instance, and changes nothing.
#[test]
fn a_component_swap_at_one_call_is_new_and_an_alias_is_not() {
    let base = "import { hz, song } from \"@m2/core\";\nimport { osc } from \"std/dsp\";\nimport { delay } from \"@m2/effects\";\n\
                export default song(() => delay(osc.saw(hz(110))));\n";
    let d = Dir::with(&[("song.ts", base)]);
    let mut s = d.open();
    let swapped = base.replace("{ delay }", "{ chorus }").replace("delay(osc", "chorus(osc");
    let delta = after(&d, &mut s, |d| d.write("song.ts", &swapped));
    assert!(delta.values.is_empty(), "{:?}", delta.values);
    assert!(
        delta.removed.iter().any(|n| in_frame(&n.name, "@m2/effects:delay") && n.name.iter().any(|x| x == "@mix")),
        "{:?}",
        delta.removed
    );
    assert!(delta.added.iter().any(|n| in_frame(&n.name, "@m2/effects:chorus") && n.name.iter().any(|x| x == "@mix")), "{:?}", delta.added);
    after(&d, &mut s, |d| d.write("song.ts", base));
    let alias = base.replace("{ delay }", "{ delay as echo }").replace("delay(osc", "echo(osc");
    assert!(after(&d, &mut s, |d| d.write("song.ts", &alias)).is_empty());
}

// ID-7, ID-13 (identity note F6): keys are unique per parent key frame, so nothing positional above
// a `key()` frame names what it holds: moving the component call around it renames none of it.
#[test]
fn names_ignore_everything_above_a_key_frame() {
    let lib = "import { component, key, port, signature, type Audio } from \"@m2/core\";\nimport { delay } from \"@m2/effects\";\n\
               export const chain = component(\n  { id: \"chain\", label: \"Chain\", signature: signature<(x: Audio) => Audio>(), inputs: { in: port.audio() }, output: port.audio() },\n  \
               (x: Audio) => key(\"inner\", () => delay(x)),\n);\n";
    let song = "import { hz, song } from \"@m2/core\";\nimport { osc } from \"std/dsp\";\nimport { chain } from \"./lib/chain\";\n\
                export default song(() => chain(osc.saw(hz(110))));\n";
    let d = Dir::with(&[("song.ts", song), ("lib/chain.ts", lib)]);
    let mut s = d.open();
    let old = s.accepted_arc();
    let moved = song.replace("song(() => chain(osc.saw(hz(110))))", "song(() => {\n  const x = osc.saw(hz(110));\n  return chain(x);\n})");
    d.write("song.ts", &moved);
    assert!(s.refresh());
    let new = s.accepted();
    let coords = super::coords_between(&old.snapshot, &new.snapshot);
    let open = new.compiled.sites.iter().find(|x| x.callee == "chain").expect("the chain call").open;
    let at = Positions { coords: &coords, new: true };
    assert!(matches!(at.point("song.ts", open as usize), crate::text::Point::Own { .. }), "the chain call is new text");
    let delta = delta_between(&old, new);
    let touched: Vec<&Vec<String>> = delta.added.iter().chain(&delta.removed).map(|n| &n.name).collect();
    assert!(touched.iter().all(|n| !in_frame(n, "@m2/effects:delay")), "{touched:?}");
    let g = Side { compiled: &new.compiled, snapshot: &new.snapshot, at }.graph();
    let held: Vec<&Vec<String>> = g.nodes.keys().filter(|n| n.iter().any(|x| x == "key:inner")).collect();
    assert!(!held.is_empty() && held.iter().all(|n| !in_frame(n, "chain")), "{held:?}");
}

// WRITE-8, ID-13: the old side of a comparison names every point by its own offset, so its names
// are its revision's alone, whatever it is compared with.
#[test]
fn old_side_names_are_own_offsets() {
    let d = Dir::with(&[("song.ts", ADJACENT)]);
    let mut s = d.open();
    let old = s.accepted_arc();
    d.write("song.ts", &ADJACENT.replace("  const b", "  // b\n  const b").replace("hz(800)", "hz(900)"));
    assert!(s.refresh());
    let coords = super::coords_between(&old.snapshot, &s.accepted().snapshot);
    assert!(!coords.is_empty());
    let none = BTreeMap::new();
    let names = |c| -> Vec<Vec<String>> {
        Side { compiled: &old.compiled, snapshot: &old.snapshot, at: Positions { coords: c, new: false } }
            .graph()
            .nodes
            .into_keys()
            .collect()
    };
    assert_eq!(names(&coords), names(&none));
}

/// A project component whose body reads song time twice, as std's effects do.
const WOB: &str = "import { component, kernel, port, signature, time, type Audio } from \"@m2/core\";\n\
                   export const wob = component(\n  { id: \"wob\", signature: signature<(x: Audio) => Audio>(), inputs: { in: port.audio() }, output: port.audio() },\n  \
                   (x: Audio) => kernel(\"std:scale\", { in: kernel(\"std:scale\", { in: x, by: time.playing }), by: kernel(\"std:lfo\", { beats: time.beats }) }),\n);\n";

/// One `wob` around a saw.
const WOBBLED: &str = "import { hz, song } from \"@m2/core\";\nimport { osc } from \"std/dsp\";\nimport { wob } from \"./lib/wob\";\n\
                       export default song(() => wob(osc.saw(hz(110))));\n";

// WRITE-8 (the sounds plan's attack, A): a `time` node a component call makes is named under its
// frame chain. Adding a call before another of its kind (built first, so a count in the module
// would give it the other's number) adds only its own nodes, and removing it removes only them:
// both are confined to the call, as the editor's Add Effect and Remove Effect declare.
#[test]
fn a_call_reading_time_added_or_removed_beside_another_renames_nothing() {
    let d = Dir::with(&[("song.ts", WOBBLED), ("lib/wob.ts", WOB)]);
    let mut s = d.open();
    let old: Vec<Vec<String>> = {
        let rev = s.accepted();
        let coords = BTreeMap::new();
        Side { compiled: &rev.compiled, snapshot: &rev.snapshot, at: Positions { coords: &coords, new: false } }
            .graph()
            .nodes
            .into_keys()
            .collect()
    };
    assert_eq!(old.iter().filter(|n| n.iter().any(|x| x.starts_with("time:"))).count(), 2, "{old:?}");
    // Add: `wob(` and `)` around the saw, confined to it.
    let text = d.read("song.ts");
    let (start, end) = (text.find("osc.saw").unwrap(), text.find("hz(110))").unwrap() + "hz(110))".len());
    let ins = |at: usize, t: &str| Edit { start: Pos::Byte(at as u32), end: Pos::Byte(at as u32), expected: String::new(), text: t.into() };
    let confined =
        |r: Range| Expect::Confined(Confined { ranges: [(p("song.ts"), Declared::Ranges(vec![r]))].into(), ..Confined::default() });
    let op = TxOp::TextEdit { file: p("song.ts"), edits: vec![ins(start, "wob("), ins(end, ")")] };
    let done = s.commit(based(&d, vec![op], confined(Range::bytes(start as u32, end as u32)))).unwrap_or_else(|e| panic!("{e}"));
    assert!(d.read("song.ts").contains("wob(wob(osc.saw(hz(110))))"));
    let delta = &done.delta;
    assert!(delta.removed.is_empty() && delta.values.is_empty(), "{delta:?}");
    // Every added node is in the new call's frame, which no old name has.
    let frame = |n: &[String]| n.iter().find(|x| x.starts_with("wob@")).cloned();
    let new_frame = frame(&delta.added[0].name).expect("in a wob frame");
    assert!(old.iter().all(|n| frame(n).as_ref() != Some(&new_frame)), "{new_frame} is new");
    assert!(delta.added.iter().all(|n| frame(&n.name).as_ref() == Some(&new_frame)), "{:?}", delta.added);
    assert_eq!(delta.added.iter().filter(|n| n.op["op"] == "time").count(), 2, "{:?}", delta.added);
    let delta_added = delta.added.len();
    // Remove the inner call again: `wob(` and its `)`, confined to the call.
    let text = d.read("song.ts");
    let at = text.find("wob(osc.saw").unwrap();
    let close = text.find("hz(110))").unwrap() + "hz(110))".len();
    let cut = |at: usize, t: &str| Edit {
        start: Pos::Byte(at as u32),
        end: Pos::Byte((at + t.len()) as u32),
        expected: t.into(),
        text: String::new(),
    };
    let op = TxOp::TextEdit { file: p("song.ts"), edits: vec![cut(at, "wob("), cut(close, ")")] };
    let done = s.commit(based(&d, vec![op], confined(Range::bytes(at as u32, close as u32 + 1)))).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(d.read("song.ts"), WOBBLED);
    let delta = &done.delta;
    assert!(delta.added.is_empty() && delta.values.is_empty(), "{delta:?}");
    // Named on the base's side, by its own offsets: the inner call's `(`.
    let inner = format!("wob@song.ts:{}#0", at + "wob".len());
    assert!(delta.removed.iter().all(|n| frame(&n.name).as_ref() == Some(&inner)), "{inner}: {:?}", delta.removed);
    assert_eq!(delta.removed.len(), delta_added, "{:?}", delta.removed);
}

// WRITE-6, WRITE-8: a new track whose instrument plays voices is Confined to the lines that add
// it. A voice's note fields (`lane:gate`) are made by the poly they are in, so they are placed
// where the poly is: without, nothing in the project placed them and no range admitted them.
#[test]
fn a_new_tracks_voices_are_placed_where_its_poly_is() {
    let d = Dir::first_song();
    let mut s = d.open();
    let text = "import { track } from \"@m2/core\";\nimport { pluck } from \"@m2/synths\";\n\
                const probe = track(\"probe\", ({ lane }) => pluck(lane.notes(\"notes\")));\n";
    let op = TxOp::TextEdit { file: p("song.ts"), edits: vec![insert(0, text)] };
    let ranges = m2_song::tx::Declared::Ranges(vec![Range::bytes(0, 0)]);
    let confined = m2_song::tx::Confined {
        ranges: [(p("song.ts"), ranges)].into(),
        lanes: vec![],
        modules: vec![ModuleChange::Added(Key::new("probe").unwrap())],
    };
    let checked = s.check(&based(&d, vec![op], Expect::Confined(confined))).unwrap_or_else(|r| panic!("{r:?}"));
    assert!(checked.delta.added.iter().any(|n| n.name.iter().any(|seg| seg == "voice")), "the voices are in the delta");
}
