//! Buses (RENDER-34, `@m2/core` 1.5.0): `bus(key, opts, fn)` is a track whose `fn` gets `input`,
//! what the song feeds it with `feed(bus, x)`: song code only, once per bus. A bus has no notes or
//! audio lanes.

use std::time::Duration;

use m2_ir::Op;

use super::*;

fn build_of(song: &str) -> Result<Ir, Vec<Diagnostic>> {
    let files: BTreeMap<String, String> = [("song.ts".to_string(), song.to_string())].into();
    let mut o = Options::new(files, "song.ts");
    o.packages.push(("std".into(), Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")));
    o.dependencies.push("std".into());
    o.time_limit = Duration::from_secs(2);
    build(&o)
}

/// A song with a `vox` track and a `verb` bus whose function is `verb`, and `body` as the song's.
fn with_verb(verb: &str, body: &str) -> Result<Ir, Vec<Diagnostic>> {
    build_of(&format!(
        "import {{ bus, feed, kernel, song, track }} from \"@m2/core\";\n\
         const vox = track(\"vox\", () => kernel(\"std:saw\", {{ freq: 220 }}));\n\
         const verb = bus(\"verb\", {{ name: \"Reverb\" }}, {verb});\n\
         export default song(() => {{\n{body}\n}});\n"
    ))
}

/// The bus halves what it is fed; the song feeds it a quarter of the vox and plays both.
const HALF: &str = "({ input }) => kernel(\"std:scale\", { in: input, by: 0.5 })";
const FEEDS: &str =
    "feed(verb, kernel(\"std:scale\", { in: vox.out, by: 0.25 }));\nreturn kernel(\"std:add\", { a: vox.out, b: verb.out });";

/// The one error a build fails with.
fn error(r: Result<Ir, Vec<Diagnostic>>) -> Diagnostic {
    let Err(e) = r else { panic!("expected an error") };
    assert_eq!(e.len(), 1, "{e:?}");
    e[0].clone()
}

// RENDER-34: a bus is a track (`bus: true`) whose `input` is an import of the song's export
// `bus:<key>`, which `feed` writes. A track keeps no `bus` (ID-11).
#[test]
fn a_bus_reads_what_the_song_feeds_it() {
    let ir = with_verb(HALF, FEEDS).unwrap_or_else(|e| panic!("{e:?}"));
    let verb = ir.modules.iter().find(|m| m.key == "verb").expect("the bus");
    assert!(verb.track && verb.bus && !verb.muted);
    assert_eq!(verb.label.as_deref(), Some("Reverb"));
    assert!(matches!(&verb.nodes[0].op, Op::Import { module, export } if module == "song" && export == "bus:verb"), "{:?}", verb.nodes[0]);
    let song = &ir.modules[0];
    let fed = song.exports.get(&m2_ir::feed_export("verb")).expect("the song exports the feed");
    let node = song.nodes.iter().find(|n| &n.id == fed).expect("its node");
    assert!(matches!(&node.op, Op::Kernel { kernel, .. } if kernel == "std:scale"), "{node:?}");
    let vox = ir.modules.iter().find(|m| m.key == "vox").unwrap();
    assert!(!vox.bus);
    assert!(!serde_json::to_string(vox).unwrap().contains("\"bus\""), "only a bus says so");
    // A bus is muted in the song as a track is (MON-5).
    let muted = build_of(
        "import { bus, feed, kernel, song } from \"@m2/core\";\n\
         const verb = bus(\"verb\", { muted: true }, ({ input }) => input);\n\
         export default song(() => { feed(verb, kernel(\"std:sine\")); return verb.out; });\n",
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    assert!(muted.modules[1].bus && muted.modules[1].muted);
    assert_eq!(
        SDK_VERSION, "2.0.0",
        "additions to @m2/core (FMT-2): buses in 1.5.0, regions in 1.6.0, file() in 1.7.0, kinds in 1.8.0, keyCount in 1.9.0, editor.unlisted in 1.10.0; a filter view's resonance, and the pack contract frozen (PACK-22), in 2.0.0"
    );
}

// RENDER-34: a bus plays what it is fed, so it declares no notes or audio lane. A control lane,
// to automate it, is fine.
#[test]
fn a_bus_with_notes_is_refused() {
    for (lane, word) in [("lane.notes(\"n\")", "notes"), ("lane.audio(\"a\")", "audio")] {
        let verb = format!("({{ input, lane }}) => kernel(\"std:add\", {{ a: input, b: kernel(\"std:scale\", {{ in: {lane}, by: 0 }}) }})");
        let e = error(with_verb(&verb, FEEDS));
        assert!(e.message.contains(&format!("the bus \"verb\" can't have a {word} lane")), "{e}");
        assert!(e.message.contains("A control lane, to automate it, is fine"), "{e}");
    }
    let automated = "({ input, lane }) => kernel(\"std:scale\", { in: input, by: lane.control(\"level\", \"gain\", 1) })";
    with_verb(automated, FEEDS).unwrap_or_else(|e| panic!("{e:?}"));
}

// RENDER-34: only a bus is fed, and only from song code.
#[test]
fn feeding_a_track_that_is_not_a_bus_is_refused() {
    let e = error(with_verb(HALF, "feed(vox, kernel(\"std:sine\"));\nfeed(verb, vox.out);\nreturn verb.out;"));
    assert!(e.message.contains("feed(): \"vox\" is a track, not a bus. Only a bus is fed: make it with bus(\"vox\", …)"), "{e}");
    let e = error(with_verb(HALF, "feed(kernel(\"std:sine\"), vox.out);\nreturn verb.out;"));
    assert!(e.message.contains("feed(): the first argument must be a bus, such as verb from bus(\"verb\", …), not a signal"), "{e}");
    // From a track's own code.
    let e = error(build_of(
        "import { bus, feed, kernel, song, track } from \"@m2/core\";\n\
         const verb = bus(\"verb\", ({ input }) => input);\n\
         const vox = track(\"vox\", () => { const x = kernel(\"std:sine\"); feed(verb, x); return x; });\n\
         export default song(() => kernel(\"std:add\", { a: vox.out, b: verb.out }));\n",
    ));
    assert!(e.message.contains("feed() is for song code, and \"vox\" is a track: feed buses in song.ts"), "{e}");
}

// RENDER-34: a bus is fed once: what goes in it is summed first.
#[test]
fn a_bus_fed_twice_is_refused() {
    let e = error(with_verb(HALF, &format!("feed(verb, vox.out);\n{FEEDS}")));
    assert!(e.message.contains("the bus \"verb\" is fed twice: at song.ts:5:1 and at song.ts:6:1."), "{e}");
    assert!(e.message.contains("Feed a bus once: sum what goes into it first"), "{e}");
}

// RENDER-34: a bus nobody feeds is a build error (std's mixer always feeds its bus channels).
#[test]
fn an_unfed_bus_is_refused() {
    let e = error(with_verb(HALF, "return kernel(\"std:add\", { a: vox.out, b: verb.out });"));
    assert!(e.message.contains("the bus \"verb\" is never fed"), "{e}");
    assert!(e.message.contains("give it a channel in std's mixer (verb: verb.out), or feed it in song code: feed(verb, x)"), "{e}");
}
