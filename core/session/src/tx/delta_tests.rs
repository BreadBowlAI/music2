//! The delta's names (WRITE-8): an insertion renames nothing, names are unique, and two processes
//! hash one Tx's delta alike.

use std::collections::BTreeMap;

use m2_ir::{Node, Op};
use m2_song::tx::{Anchor, Edit, Expect, ModuleChange, Op as TxOp, Pos, Range, Spot};
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
const KICK_SONG: &str = "import { secs, song, track } from '@m2/core';\nimport { kick } from 'std/instruments';\n\
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
