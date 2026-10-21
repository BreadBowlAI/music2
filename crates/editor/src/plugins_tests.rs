//! A plugin added after a sound: one Tx that wraps the call, refused in words when it can't be.

use m2_ir::Op;
use m2_session::Session;

use crate::Gestures;
use crate::controls::walk;
use crate::testing::{Fixture, MANIFEST};

const KICK: &str = "kick(lane.notes(\"hits\"))";

/// A song whose one track plays a kick, with the probe plugin findable (tests: in this process).
fn kick_song() -> Fixture {
    m2_plugins::test_main_thread();
    m2_plugins::probe::install();
    let f = Fixture::new();
    f.write("music2.toml", MANIFEST);
    f.write(
        "song.ts",
        format!(
            "import {{ song, track }} from \"@m2/core\";\nimport {{ kick }} from \"std/instruments\";\n\n\
             const drums = track(\"kick\", {{ name: \"Kick\" }}, ({{ lane }}) => {KICK});\n\
             export default song(() => drums.out);\n"
        ),
    );
    f.write("data/kick/hits.json", m2_song::canonical(&m2_song::LaneData::Notes(vec![])));
    f
}

/// The id of the last call to `callee` in the accepted revision.
fn call(s: &Session, callee: &str) -> String {
    let site =
        s.accepted().compiled.sites.iter().rfind(|x| x.in_project && x.callee == callee).unwrap_or_else(|| panic!("no call `{callee}`"));
    format!("{}#{}", site.file, site.ordinal)
}

/// Each plugin node of the accepted revision: its id, its key, and what its `in` reads.
fn plugins(s: &Session) -> Vec<(String, Option<String>, Option<String>)> {
    let mut out = vec![];
    for m in &s.accepted().compiled.ir.modules {
        walk(&m.nodes, &mut |n| {
            if let Op::Plugin { plugin, key, inputs, .. } = &n.op {
                out.push((plugin.clone(), key.clone(), inputs.get("in").cloned()));
            }
        });
    }
    out
}

// Gaps #32, WRITE-8, WRITE-10, R5: adding an installed plugin after a sound wraps its call where it
// is written, `plugin("clap:<id>", { in: <call>, key })`, importing `plugin`, as one Tx. Its key is
// the plugin's name as a key, so its settings can be saved (HOME-6). One plugin node reads the
// kick, and the track's out reads it. One Undo gives the old bytes back.
#[test]
fn adding_a_plugin_effect_wraps_the_call_in_one_tx() {
    let f = kick_song();
    let mut s = f.open();
    let before = f.read("song.ts");
    let at = s.accepted().number;
    let added = s.add_effect(at, &call(&s, "kick"), "clap:m2.probe").unwrap();
    let wrapped = format!("plugin(\"clap:m2.probe\", {{ in: {KICK}, key: \"m2-probe\" }})");
    assert_eq!((added.before.as_str(), added.after.as_str()), (KICK, wrapped.as_str()));
    assert_eq!(f.read("song.ts"), before.replace("{ song, track }", "{ song, track, plugin }").replace(KICK, &wrapped));
    assert_eq!(s.accepted().number, at + 1, "one Tx");
    assert!(s.accepted().compiled.problems.iter().all(|d| d.code != "plugin.missing"), "{:?}", s.accepted().compiled.problems);
    let found = plugins(&s);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!((found[0].0.as_str(), found[0].1.as_deref()), ("clap:m2.probe", Some("m2-probe")));
    let m = &s.accepted().compiled.ir.modules.iter().find(|m| m.key == "kick").unwrap();
    let reads_kick = found[0].2.as_deref().unwrap();
    assert!(
        m.nodes.iter().any(|n| n.id == m2_ir::split_ref(reads_kick).0 && matches!(&n.op, Op::Kernel { .. } | Op::Poly { .. })),
        "it reads the kick"
    );
    crate::undo(&mut s).unwrap();
    assert_eq!(f.read("song.ts"), before, "one Undo gives the old bytes back");
}

// ID-2, HOME-6: a second plugin in the same frame gets a key no other has there, `m2-probe-2`, so
// the build doesn't refuse two plugins with one key (`key.duplicate`).
#[test]
fn a_second_plugin_in_one_frame_gets_a_free_key() {
    let f = kick_song();
    let mut s = f.open();
    let at = s.accepted().number;
    s.add_effect(at, &call(&s, "kick"), "clap:m2.probe").unwrap();
    let at = s.accepted().number;
    let added = s.add_effect(at, &call(&s, "plugin"), "clap:m2.probe").unwrap_or_else(|e| panic!("{e}"));
    assert!(added.after.ends_with(", key: \"m2-probe-2\" })"), "{}", added.after);
    let mut keys: Vec<Option<String>> = plugins(&s).into_iter().map(|p| p.1).collect();
    keys.sort();
    assert_eq!(keys, [Some("m2-probe".to_string()), Some("m2-probe-2".to_string())]);
}

// Gaps #32: a plugin that isn't installed (removed since the list was made) is refused in plain
// words, and nothing is written.
#[test]
fn a_plugin_that_isnt_installed_is_refused_in_words() {
    let f = kick_song();
    let mut s = f.open();
    let before = f.read("song.ts");
    let at = s.accepted().number;
    let refused = s.add_effect(at, &call(&s, "kick"), "clap:org.example.not-installed").unwrap_err();
    assert_eq!(
        refused.to_string(),
        "org.example.not-installed isn't installed on this computer, so it can't be added. Install it, then choose Look again."
    );
    assert_eq!((f.read("song.ts"), s.accepted().number), (before, at), "nothing written");
}
