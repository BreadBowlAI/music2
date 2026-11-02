//! `continues` (ID-8, ID-13, ID-15): which nodes of a new revision continue nodes of the one that
//! plays, so the engine keeps their instances.

use std::sync::Arc;

use m2_ir::{FNode, Flat};
use m2_runtime::Continues;

use super::continues::continues;
use super::tests::{Dir, std_packages};
use crate::{Revision, Session};

/// The revision before `edit` rewrote `path` and the one after.
fn edited(d: &Dir, s: &mut Session, path: &str, edit: impl Fn(&str) -> String) -> (Arc<Revision>, Arc<Revision>) {
    let before = s.accepted_arc();
    let text = d.read(path);
    let after = edit(&text);
    assert_ne!(after, text, "the edit changes {path}");
    d.write(path, &after);
    assert!(s.refresh());
    (before, s.accepted_arc())
}

/// The pairs `c` makes, each node of `new` with the node of `old` it continues: always of one
/// module and one op.
fn pairs<'a>(old: &'a Flat, new: &'a Flat, c: &Continues) -> Vec<(&'a FNode, &'a FNode)> {
    let out: Vec<(&FNode, &FNode)> =
        c.top.iter().enumerate().filter_map(|(i, j)| Some((&new.nodes[i], &old.nodes[(*j)? as usize]))).collect();
    for (n, o) in &out {
        assert_eq!(n.module, o.module, "{} continues {}", n.name, o.name);
        assert_eq!(std::mem::discriminant(&n.op), std::mem::discriminant(&o.op), "{} continues {}", n.name, o.name);
    }
    out
}

/// Whether every node of `old` that `module` holds is continued.
fn kept(old: &Flat, c: &Continues, module: &str) -> bool {
    let claimed: std::collections::HashSet<u32> = c.top.iter().flatten().copied().collect();
    old.nodes.iter().enumerate().filter(|(_, n)| n.module == module).all(|(j, _)| claimed.contains(&(j as u32)))
}

// ID-13, ID-15, the step 6 gate's edit: a track inserted at the top of `song.ts` shifts every
// offset after it, and renames nothing: every node continues.
#[test]
fn an_insertion_above_continues_everything() {
    let d = Dir::first_song();
    let mut s = d.open();
    let top =
        "import { osc } from \"std/dsp\";\nimport { track } from \"@m2/core\";\nconst probe = track(\"probe\", () => osc.sine(440));\n";
    let (a, b) = edited(&d, &mut s, "song.ts", |t| format!("{top}{t}"));
    let c = continues(&a, &b);
    let (old, new) = (&a.compiled.flat, &b.compiled.flat);
    let p = pairs(old, new, &c);
    assert_eq!(p.len(), old.nodes.len(), "every node continues");
    assert!(new.nodes.iter().zip(&c.top).all(|(n, j)| j.is_some() || n.module == "probe"), "only the new track is new");
}

// ID-10, ID-13: a track's label renamed changes no call's place: everything continues.
#[test]
fn a_label_rename_continues_everything() {
    let d = Dir::first_song();
    let mut s = d.open();
    let (a, b) = edited(&d, &mut s, "tracks/bass.ts", |t| t.replace("{ name: \"Bass\" }", "{ name: \"Bass line\" }"));
    let c = continues(&a, &b);
    assert_eq!(pairs(&a.compiled.flat, &b.compiled.flat, &c).len(), a.compiled.flat.nodes.len());
}

// ID-13, ID-15: a structural edit to one track (an effect wrapped around its instrument, saved as a
// file) continues every node of every other file, the song's mixer included, and every node of the
// edited track: the wrapped call's `(` is kept, wherever the diff puts its `)`.
#[test]
fn untouched_files_continue_whatever_changed() {
    let d = Dir::first_song();
    let mut s = d.open();
    let (a, b) = edited(&d, &mut s, "tracks/keys.ts", |t| {
        t.replace("pluck(lane.notes(\"chords\"))", "delay(pluck(lane.notes(\"chords\")))")
            .replace("import { pluck }", "import { delay } from \"@m2/effects\";\nimport { pluck }")
    });
    let c = continues(&a, &b);
    let old = &a.compiled.flat;
    for m in ["bass", "hats", "kick", "song", "keys"] {
        assert!(kept(old, &c, m), "every node of {m} continues");
    }
    pairs(old, &b.compiled.flat, &c);
}

/// A song of probe plugins, keyed as `body` keys them.
fn probes(body: &str) -> Dir {
    m2_plugins::test_main_thread();
    m2_plugins::probe::install();
    let song = format!(
        "import {{ plugin, song }} from \"@m2/core\";\nimport {{ delay }} from \"@m2/effects\";\nimport {{ sum }} from \"std/math\";\n\
         export default song(() => {{\n{body}\n}});\n"
    );
    Dir::with(&[("song.ts", &song)])
}

/// For each keyed plugin of `new`, the key of the plugin of `old` it continues.
fn followed(old: &Flat, new: &Flat, c: &Continues) -> Vec<(Vec<String>, Option<Vec<String>>)> {
    let key = |n: &FNode| match &n.op {
        m2_ir::FOp::Plugin { state, .. } => state.clone(),
        _ => None,
    };
    let mut out: Vec<_> =
        new.nodes.iter().zip(&c.top).filter_map(|(n, j)| Some((key(n)?, j.and_then(|j| key(&old.nodes[j as usize]))))).collect();
    out.sort();
    out
}

// ID-8, ID-15, HOST-5: a plugin with a state path continues the one with that path, wherever it
// is: two keys swapped in place, a list of keys reordered, a keyed plugin wrapped in an effect.
#[test]
fn keyed_plugins_follow_their_paths() {
    let k = |s: &str| vec![s.to_string()];
    let cases = [
        (
            "  const a = plugin(\"clap:m2.probe\", { key: \"lead\" });\n  const b = plugin(\"clap:m2.probe\", { key: \"pad\" });\n  return sum(a, b);",
            ("key: \"lead\" });\n  const b", "key: \"pad\" });\n  const b"),
            ("key: \"pad\" });\n  return", "key: \"lead\" });\n  return"),
        ),
        (
            "  const [a, b] = [\"lead\", \"pad\"].map((k) => plugin(\"clap:m2.probe\", { key: k }));\n  return sum(a, b);",
            ("[\"lead\", \"pad\"]", "[\"pad\", \"lead\"]"),
            ("", ""),
        ),
        (
            "  const a = plugin(\"clap:m2.probe\", { key: \"lead\" });\n  const b = plugin(\"clap:m2.probe\", { key: \"pad\" });\n  return sum(a, b);",
            ("const a = plugin(\"clap:m2.probe\", { key: \"lead\" });", "const a = delay(plugin(\"clap:m2.probe\", { key: \"lead\" }));"),
            ("", ""),
        ),
    ];
    for (body, first, second) in cases {
        let d = probes(body);
        let mut s = Session::open(&d.0, std_packages()).unwrap_or_else(|e| panic!("{e:?}"));
        let (a, b) = edited(&d, &mut s, "song.ts", |t| {
            let t = t.replacen(first.0, first.1, 1);
            if second.0.is_empty() { t } else { t.replacen(second.0, second.1, 1) }
        });
        let c = continues(&a, &b);
        let f = followed(&a.compiled.flat, &b.compiled.flat, &c);
        assert_eq!(f, [(k("lead"), Some(k("lead"))), (k("pad"), Some(k("pad")))], "{body}");
    }
}

/// A one-track song whose track sums what `body` makes of its notes lane `n`.
fn summed(body: &str) -> Dir {
    let song = format!(
        "import {{ key, song, track }} from \"@m2/core\";\nimport {{ pluck }} from \"@m2/synths\";\nimport {{ sum }} from \"std/math\";\n\
         const t = track(\"t\", ({{ lane }}) => {{\n  const n = lane.notes(\"n\");\n  return sum(...{body});\n}});\nexport default song(() => t.out);\n"
    );
    Dir::with(&[("song.ts", &song)])
}

/// For each `poly` of `new`, in flat order, the index among `old`'s polys of the one it continues.
fn polys(old: &Flat, new: &Flat, c: &Continues) -> Vec<Option<usize>> {
    let of =
        |f: &Flat| f.nodes.iter().enumerate().filter(|(_, n)| matches!(n.op, m2_ir::FOp::Poly { .. })).map(|(i, _)| i).collect::<Vec<_>>();
    let (was, is) = (of(old), of(new));
    is.iter().map(|&i| c.top[i].and_then(|j| was.iter().position(|&w| w == j as usize))).collect()
}

// ID-14: an unkeyed loop keeps its instances by index while its count is the same (a reorder, an
// edit elsewhere); when the count changes every occurrence resets, so no state goes to a neighbour.
#[test]
fn a_loop_that_grows_resets_its_occurrences() {
    let d = summed("[60, 64].map(() => pluck(n))");
    let mut s = d.open();
    let (a, b) = edited(&d, &mut s, "song.ts", |t| t.replace("[60, 64]", "[64, 60]"));
    assert_eq!(polys(&a.compiled.flat, &b.compiled.flat, &continues(&a, &b)), [Some(0), Some(1)], "by index");
    let (a, b) = edited(&d, &mut s, "song.ts", |t| t.replace("[64, 60]", "[64, 60, 67]"));
    assert_eq!(polys(&a.compiled.flat, &b.compiled.flat, &continues(&a, &b)), [None, None, None], "every occurrence resets");
}

// ID-14, ID-7: a keyed loop keeps every instance through a reorder and growth, each by its key.
#[test]
fn a_keyed_loop_keeps_through_reorder_and_growth() {
    let d = summed("[\"a\", \"b\"].map((k) => key(k, () => pluck(n)))");
    let mut s = d.open();
    let (a, b) = edited(&d, &mut s, "song.ts", |t| t.replace("[\"a\", \"b\"]", "[\"b\", \"a\", \"c\"]"));
    assert_eq!(polys(&a.compiled.flat, &b.compiled.flat, &continues(&a, &b)), [Some(1), Some(0), None]);
}

// ID-8, ID-15: a plugin given a key, which its old revision has no state path for, continues by its
// name after the path pass: its first state save doesn't restart it.
#[test]
fn adding_a_plugin_key_continues_its_instance() {
    let d = probes("  return plugin(\"clap:m2.probe\", { gain_1: 0.5 });");
    let mut s = Session::open(&d.0, std_packages()).unwrap_or_else(|e| panic!("{e:?}"));
    let (a, b) = edited(&d, &mut s, "song.ts", |t| t.replace("{ gain_1: 0.5 }", "{ gain_1: 0.5, key: \"lead\" }"));
    let c = continues(&a, &b);
    let f = followed(&a.compiled.flat, &b.compiled.flat, &c);
    assert_eq!(f.len(), 1);
    assert!(f[0].1.is_none(), "the old one had no key");
    let new = b.compiled.flat.nodes.iter().position(|n| matches!(n.op, m2_ir::FOp::Plugin { .. })).unwrap();
    assert!(c.top[new].is_some(), "the keyed plugin continues the unkeyed one");
}

// ID-13 (review id-3): a whole file re-indented, or its line ends made CRLF, changes no token,
// however long it is: past 500 changed lines the token diff still runs, so every node continues.
#[test]
fn a_long_file_reformatted_continues_everything() {
    let body: String = (0..300).map(|k| format!("  y = lowpass(y, {});\n", 200 + k)).collect();
    let song = format!(
        "import {{ song, track }} from \"@m2/core\";\nimport {{ osc }} from \"std/dsp\";\nimport {{ lowpass }} from \"std/filter\";\n\
         const t = track(\"t\", () => {{\n  let y = osc.saw(110);\n{body}  return y;\n}});\nexport default song(() => t.out);\n"
    );
    let d = Dir::with(&[("song.ts", &song)]);
    let mut s = d.open();
    for edit in [|t: &str| t.lines().map(|l| format!("  {l}\n")).collect::<String>(), |t: &str| t.replace('\n', "\r\n")] {
        let (a, b) = edited(&d, &mut s, "song.ts", edit);
        let c = continues(&a, &b);
        assert!(a.compiled.flat.nodes.len() > 300);
        assert_eq!(pairs(&a.compiled.flat, &b.compiled.flat, &c).len(), a.compiled.flat.nodes.len(), "every node continues");
    }
}
