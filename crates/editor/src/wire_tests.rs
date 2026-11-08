//! + on a wire (CARD-36): each wire of the view says where an effect goes on it, by one of the add
//! route's two forms, or why none can, in words; the second form wraps one argument of one reader;
//! and Remove says why it can't take a node out, in the view and the route alike.

use super::*;
use crate::Gestures;
use crate::testing::{Fixture, notes_pack};
use crate::view::{GraphView, ModuleView, PortEdge};

const HEAD: &str = "import { hz, song } from \"@m2/core\";\nimport { osc } from \"std/dsp\";\n";

/// A project whose `song.ts` is `text` (and `files`, each its path and text), opened.
fn open(text: &str, files: &[(&str, &str)]) -> (Fixture, Session) {
    let f = Fixture::new();
    f.write("music2.toml", crate::testing::MANIFEST);
    f.write("song.ts", text);
    for (path, text) in files {
        f.write(path, text);
    }
    let s = f.open();
    (f, s)
}

fn rev(s: &Session) -> u64 {
    s.accepted().number
}

/// The id of the one call to `callee` in the project.
fn call(s: &Session, callee: &str) -> String {
    let mut found = s.accepted().compiled.sites.iter().filter(|x| x.in_project && x.callee == callee);
    let site = found.next().unwrap_or_else(|| panic!("no call `{callee}`"));
    assert!(found.next().is_none(), "one call `{callee}`");
    format!("{}#{}", site.file, site.ordinal)
}

fn module<'a>(v: &'a GraphView, key: &str) -> &'a ModuleView {
    v.modules.iter().find(|m| m.key == key).unwrap_or_else(|| panic!("no module {key}"))
}

/// The wires of module `key` from node `from` to node `to`, by their ids.
fn wires<'a>(m: &'a ModuleView, from: &str, to: &str) -> Vec<&'a PortEdge> {
    m.port_edges.iter().filter(|e| e.from == from && e.to == to).collect()
}

/// The + of the one wire of the song from `from` to `to`.
fn plus(s: &Session, from: &str, to: &str) -> Plus {
    let v = crate::view::view(s);
    let w = wires(module(&v, "song"), from, to);
    assert_eq!(w.len(), 1, "one wire {from} → {to}: {:?}", module(&v, "song").port_edges);
    w[0].plus.clone()
}

fn refused(p: &Plus) -> &str {
    match p {
        Plus::Refused { refused } => refused,
        other => panic!("refused, not {other:?}"),
    }
}

// CARD-36: a sound read by two calls. The + on one of its wires wraps that reader's argument only,
// one undo step; the other reader still reads it as it did. A wire into an export, from a call read
// once, goes after that call.
#[test]
fn an_effect_on_a_wire_changes_that_wire_only() {
    let text = format!(
        "{HEAD}import {{ limit }} from \"std/fx\";\nimport {{ lowpass }} from \"std/filter\";\nimport {{ add }} from \"std/math\";\n\
         export default song(() => {{\n  const p = osc.saw(hz(110));\n  return add(limit(p, -1), lowpass(p, 1000));\n}});\n"
    );
    let (f, mut s) = open(&text, &[]);
    let (saw, limit, lowpass, add) = (call(&s, "osc.saw"), call(&s, "limit"), call(&s, "lowpass"), call(&s, "add"));
    assert_eq!(plus(&s, &saw, &lowpass), Plus::Wire { reader: lowpass.clone(), input: "0".into() });
    assert_eq!(plus(&s, &saw, &limit), Plus::Wire { reader: limit.clone(), input: "0".into() });
    assert_eq!(plus(&s, &lowpass, &add), Plus::Wire { reader: add.clone(), input: "1".into() });
    assert_eq!(plus(&s, &add, "export:out"), Plus::After { call: add.clone() });
    // Serialized as the route's own fields, so a window posts it as it is.
    assert_eq!(serde_json::to_value(plus(&s, &saw, &lowpass)).unwrap(), serde_json::json!({ "reader": lowpass, "input": "0" }));
    assert_eq!(serde_json::to_value(plus(&s, &add, "export:out")).unwrap(), serde_json::json!({ "call": add }));

    let at = rev(&s);
    let added = s.add_effect_on(at, &lowpass, "0", "@m2/effects:delay").unwrap_or_else(|e| panic!("{e}"));
    assert_eq!((added.before.as_str(), added.after.as_str()), ("p", "delay(p, { time: 0.25, feedback: 0.3, mix: 0.3 })"));
    assert_eq!(
        f.read("song.ts"),
        text.replace("from \"std/math\";\n", "from \"std/math\";\nimport { delay } from \"@m2/effects\";\n")
            .replace("lowpass(p, 1000)", "lowpass(delay(p, { time: 0.25, feedback: 0.3, mix: 0.3 }), 1000)")
    );
    // The view: the saw feeds the Limiter and the Delay, and the Delay the Lowpass.
    let v = crate::view::view(&s);
    let song = module(&v, "song");
    let (saw, delay, lowpass) = (call(&s, "osc.saw"), call(&s, "delay"), call(&s, "lowpass"));
    assert_eq!(wires(song, &saw, &delay).len(), 1);
    assert_eq!(wires(song, &delay, &lowpass).len(), 1);
    assert!(wires(song, &saw, &lowpass).is_empty());
    s.undo().unwrap();
    assert_eq!(f.read("song.ts"), text, "one undo step");
}

// CARD-36: a mixer's channel is an argument of its call too: `"0.a"`, or `"0.b.in"` inside a
// channel's options. A track's sound read in the song keeps CARD-36's first form, `import:`.
#[test]
fn a_mixers_channels_take_an_effect_on_their_wires() {
    let text = format!(
        "{HEAD}import {{ mixer }} from \"std/mix\";\nimport k from \"./tracks/k\";\n\
         export default song(() => mixer({{ a: osc.saw(hz(110)), b: {{ in: osc.sine(hz(220)), db: -6 }}, k: k.out }}));\n"
    );
    let track = "import { hz, track } from \"@m2/core\";\nimport { osc } from \"std/dsp\";\n\nexport default track(\"k\", { name: \"K\" }, () => osc.square(hz(55)));\n";
    let (f, mut s) = open(&text, &[("tracks/k.ts", track)]);
    let mixer = call(&s, "mixer");
    assert_eq!(plus(&s, &call(&s, "osc.saw"), &mixer), Plus::Wire { reader: mixer.clone(), input: "0.a".into() });
    assert_eq!(plus(&s, &call(&s, "osc.sine"), &mixer), Plus::Wire { reader: mixer.clone(), input: "0.b.in".into() });
    assert_eq!(plus(&s, "import:k.out", &mixer), Plus::After { call: "import:k.out".into() });
    let at = rev(&s);
    s.add_effect_on(at, &mixer, "0.b.in", "std:limit").unwrap_or_else(|e| panic!("{e}"));
    assert!(f.read("song.ts").contains("b: { in: limit(osc.sine(hz(220)), -1), db: -6 }"), "{}", f.read("song.ts"));
    let at = rev(&s);
    s.add_effect(at, "import:k.out", "std:limit").unwrap_or_else(|e| panic!("{e}"));
    assert!(f.read("song.ts").contains("k: limit(k.out, -1)"), "{}", f.read("song.ts"));
}

// CARD-36: a notes wire takes a note effect, written around the notes as the reader reads them; an
// effect there is refused in words, and a note effect on a sound's wire too. A control wire takes
// neither.
#[test]
fn a_wire_takes_what_fits_its_kind() {
    let f = Fixture::new();
    let pack = f.0.join("notes");
    notes_pack(&pack, true);
    f.write("music2.toml", format!("{}\"@test/notes\" = {{ path = {:?} }}\n", crate::testing::MANIFEST, pack.to_string_lossy()));
    let text = "import { song } from \"@m2/core\";\nimport { synth } from \"@m2/synths\";\nimport { lfo } from \"std/control\";\n\
                import { vca } from \"std/dsp\";\n\n\
                export default song(({ lane, time }) => vca(synth.pluck(lane.notes(\"chords\")), lfo(time.beats)));\n";
    f.write("song.ts", text);
    let mut s = f.open();
    let (notes, pluck, vca, lfo) = (call(&s, "lane.notes"), call(&s, "synth.pluck"), call(&s, "vca"), call(&s, "lfo"));
    assert_eq!(plus(&s, &notes, &pluck), Plus::Wire { reader: pluck.clone(), input: "0".into() });
    assert_eq!(plus(&s, &pluck, &vca), Plus::Wire { reader: vca.clone(), input: "0".into() });
    assert_eq!(refused(&plus(&s, &lfo, &vca)), "This wire carries a control: an effect goes on a sound, a note effect on notes.");
    let at = rev(&s);
    let e = s.add_effect_on(at, &pluck, "0", "@m2/effects:delay").unwrap_err().to_string();
    assert!(e.contains("Delay goes on a sound, and this wire carries notes."), "{e}");
    let e = s.add_effect_on(at, &vca, "0", "@test/notes:arp").unwrap_err().to_string();
    assert!(e.contains("Arpeggiator goes on notes, and this wire carries a sound."), "{e}");
    let e = s.add_effect_on(at, &vca, "1", "@m2/effects:delay").unwrap_err().to_string();
    assert!(e.contains("This wire carries a control: an effect goes on a sound, a note effect on notes."), "{e}");
    assert_eq!(f.read("song.ts"), text, "nothing is written");
    let added = s.add_effect_on(at, &pluck, "0", "@test/notes:arp").unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(added.after, "arp(lane.notes(\"chords\"))");
    assert!(f.read("song.ts").contains("synth.pluck(arp(lane.notes(\"chords\")))"));
}

// CARD-36: where nothing can go, the + is greyed with why, and the route refuses in the same words:
// a source read in more places than the wire into the export, a call made more than once, an
// argument that spreads, and a helper function's parameter.
#[test]
fn where_nothing_can_go_the_view_and_the_route_say_why_alike() {
    // A track's drum is read by its Follow and its export: its wire into the export would change both.
    let track = "import { hz, track } from \"@m2/core\";\nimport { follow, osc } from \"std/dsp\";\n\n\
                 export default track(\"k\", { name: \"K\" }, () => {\n  const out = osc.square(hz(55));\n  return { out, env: follow(out) };\n});\n";
    let text = format!(
        "{HEAD}import {{ mixer }} from \"std/mix\";\nimport k from \"./tracks/k\";\nexport default song(() => mixer({{ k: k.out }}));\n"
    );
    let (_f, s) = open(&text, &[("tracks/k.ts", track)]);
    let v = crate::view::view(&s);
    let k = module(&v, "k");
    let (square, follow) = (call(&s, "osc.square"), call(&s, "follow"));
    let out = wires(k, &square, "export:out");
    assert_eq!(
        refused(&out[0].plus),
        "Square is read in 2 places, so an effect after it would change them all: put it on one of its wires, or change it in code."
    );
    assert_eq!(wires(k, &square, &follow)[0].plus, Plus::Wire { reader: follow.clone(), input: "0".into() });

    // A helper made twice; a spread; a helper's parameter, made once.
    let text = format!(
        "{HEAD}import {{ lowpass }} from \"std/filter\";\nimport {{ add, sum }} from \"std/math\";\n\
         const soft = (x: any) => lowpass(x, 800);\nconst once = (y: any) => lowpass(y, 2000);\n\
         export default song(() => add(add(soft(osc.saw(hz(110))), soft(osc.sine(hz(220)))), add(sum(...[osc.square(hz(55)), osc.square(hz(56))]), once(osc.tri(hz(330))))));\n"
    );
    let (f, mut s) = open(&text, &[]);
    let v = crate::view::view(&s);
    let song = module(&v, "song");
    let into = |callee: &str| -> Vec<&PortEdge> {
        let id = call(&s, callee);
        song.port_edges.iter().filter(|e| e.from == id).collect()
    };
    let lowpasses: Vec<&str> = song.nodes.iter().filter(|n| n.label == "lowpass").map(|n| n.id.as_str()).collect();
    assert_eq!(lowpasses.len(), 2, "one node for each lowpass written");
    let made_twice = into("osc.saw");
    assert_eq!(refused(&made_twice[0].plus), "Lowpass is made 2 times, in a function or a loop: change it in code.");
    let spread: Vec<&PortEdge> = song.port_edges.iter().filter(|e| e.to == call(&s, "sum")).collect();
    assert_eq!(spread.len(), 2, "a wire from each square: {spread:?}");
    for e in spread {
        assert_eq!(refused(&e.plus), "Sum takes its sounds from a spread: change it in code.");
    }
    let param = into("osc.tri");
    assert_eq!(refused(&param[0].plus), "Lowpass reads Tri as `y`, not where it is made: change it in code.");
    // The route refuses the same, in the same words, and writes nothing.
    let (sum, at) = (call(&s, "sum"), rev(&s));
    let e = s.add_effect_on(at, &sum, "0", "std:limit").unwrap_err().to_string();
    assert!(e.contains("Sum takes its sounds from a spread: change it in code."), "{e}");
    let reader = param[0].to.clone();
    let e = s.add_effect_on(at, &reader, "0", "std:limit").unwrap_err().to_string();
    assert!(e.contains("Lowpass reads Tri as `y`, not where it is made: change it in code."), "{e}");
    assert_eq!(f.read("song.ts"), text);
}

// CARD-36: a loop's wire, read a block late, takes no effect.
#[test]
fn a_loops_wire_takes_none() {
    let text = format!(
        "{HEAD}import {{ feedback }} from \"@m2/core\";\nimport {{ lowpass }} from \"std/filter\";\nimport {{ add }} from \"std/math\";\n\
         export default song(() => {{\n  const back = feedback(\"audio<1>\");\n  const out = lowpass(add(osc.saw(hz(110)), back), 800);\n  back.set(out);\n  return out;\n}});\n"
    );
    let (_f, s) = open(&text, &[]);
    let v = crate::view::view(&s);
    let song = module(&v, "song");
    let late: Vec<&PortEdge> = song.port_edges.iter().filter(|e| e.late).collect();
    assert!(!late.is_empty(), "a loop: {:?}", song.port_edges);
    for e in late {
        assert_eq!(refused(&e.plus), "This wire closes a loop: change it in code.");
    }
}

// CARD-36: Remove says why it can't take a node out, in the view and in the route alike: an
// instrument, a call of several sounds, one that makes a control or a sound of its own, one whose
// sound in and out differ in kind, and the rest. An effect has none.
#[test]
fn remove_says_why_in_the_view_and_the_route() {
    let text = "import { hz, song } from \"@m2/core\";\nimport { synth } from \"@m2/synths\";\nimport { lfo } from \"std/control\";\n\
                import { follow, osc, vca } from \"std/dsp\";\nimport { limit } from \"std/fx\";\nimport { add, sum } from \"std/math\";\n\
                import { mixer } from \"std/mix\";\n\n\
                export default song(({ lane, time }) => mixer({ a: limit(sum(synth.pluck(lane.notes(\"chords\")), osc.saw(hz(110))), -1), \
                b: vca(osc.sine(hz(220)), add(lfo(time.beats), follow(osc.square(hz(55))))) }));\n";
    let (f, mut s) = open(text, &[]);
    let v = crate::view::view(&s);
    let song = module(&v, "song");
    let ids: Vec<(String, Option<String>)> = song.nodes.iter().map(|n| (n.id.clone(), n.remove.clone())).collect();
    let remove = |id: &str| ids.iter().find(|(i, _)| i == id).unwrap().1.clone();
    assert_eq!(remove(&call(&s, "limit")), None, "an effect has no refusal");
    let said = [
        ("synth.pluck", "Synth pluck is an instrument: change it with Change instrument, in its track's menu."),
        ("sum", "Sum mixes several sounds: change it in code."),
        ("mixer", "Mixer mixes several sounds: change it in code."),
        ("lfo", "LFO makes a control: change it in code."),
        ("osc.saw", "Saw makes a sound of its own: change it in code."),
        ("follow", "Follow turns a sound into a control: change it in code."),
        ("add", "Add isn't an effect: change it in code."),
    ];
    for (callee, words) in said {
        assert_eq!(remove(&call(&s, callee)).as_deref(), Some(words), "{callee}");
        let at = rev(&s);
        let e = s.remove_effect(at, &call(&s, callee)).unwrap_err().to_string();
        assert!(e.contains(words), "{callee}: {e}");
    }
    assert_eq!(f.read("song.ts"), text, "nothing is written");
}

// CARD-36 (the graph note §3): a + inside a voice puts the effect in each voice, written once in
// the voice's code.
#[test]
fn a_wire_inside_a_voice_takes_an_effect_for_each_voice() {
    let text = "import { poly, song } from \"@m2/core\";\nimport { osc } from \"std/dsp\";\nimport { lowpass } from \"std/filter\";\n\n\
                export default song(({ lane }) => poly(lane.notes(\"chords\"), (n) => lowpass(osc.saw(n.pitch), 800)));\n";
    let (f, mut s) = open(text, &[]);
    let lowpass = call(&s, "lowpass");
    assert_eq!(plus(&s, &call(&s, "osc.saw"), &lowpass), Plus::Wire { reader: lowpass.clone(), input: "0".into() });
    let at = rev(&s);
    s.add_effect_on(at, &lowpass, "0", "std:limit").unwrap_or_else(|e| panic!("{e}"));
    assert!(f.read("song.ts").contains("(n) => lowpass(limit(osc.saw(n.pitch), -1), 800)"), "{}", f.read("song.ts"));
}
