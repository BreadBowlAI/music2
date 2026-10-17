//! Each track's channel, found by following the graph (ID-18, design-8b §2.3), on first-song
//! written in other ways; the Mix view, a channel's fader written into its entry, and its first pan
//! (design-8b §5.2 to §5.4).

use std::path::Path;

use m2_session::Session;
use m2_song::tx::{Confined, Declared, Expect};

use super::*;
use crate::testing::{Fixture, fixture};
use crate::{EditError, Gestures};

const HEAD: &str = "import { db, song } from \"@m2/core\";\nimport { lfo } from \"std/control\";\nimport { duck } from \"std/fx\";\n\
                    import { range, sum } from \"std/math\";\nimport { master, mixer } from \"std/mix\";\nimport bass from \"./tracks/bass\";\n\
                    import hats from \"./tracks/hats\";\nimport keys from \"./tracks/keys\";\nimport kick from \"./tracks/kick\";\n";

/// first-song with its song written as `body` after [`HEAD`], open.
fn song(body: &str) -> (Fixture, Session) {
    let f = fixture("examples/first-song");
    f.write("song.ts", format!("{HEAD}{body}"));
    let s = f.open();
    assert!(s.draft_errors.is_empty() && s.accepted().has_code(), "{:?}", s.draft_errors);
    (f, s)
}

fn the_mixer(s: &Session) -> Mixer {
    match mixer(s.accepted()) {
        Mixing::One(m) => m,
        other => panic!("{other:?}"),
    }
}

/// Each channel's key and its tracks.
fn channels(s: &Session) -> Vec<(String, Vec<String>)> {
    the_mixer(s).channels.iter().map(|c| (c.key.clone(), c.tracks.clone())).collect()
}

fn owned(list: &[(&str, &[&str])]) -> Vec<(String, Vec<String>)> {
    list.iter().map(|(k, t)| (k.to_string(), t.iter().map(|x| x.to_string()).collect())).collect()
}

// First-song as it is: each track in its own channel. The bass is found through its duck; the
// kick's `env`, read inside the bass's channel, is a sidechain and a control, so it doesn't put
// the kick there.
#[test]
fn each_track_reaches_its_channel() {
    let s = fixture("examples/first-song").open();
    assert_eq!(channels(&s), owned(&[("kick", &["kick"]), ("hats", &["hats"]), ("bass", &["bass"]), ("keys", &["keys"])]));
    let m = the_mixer(&s);
    let bass = m.channel_of("bass").unwrap();
    assert!(bass.reads.iter().any(|r| r.module == "kick" && r.export == "env"), "{:?}", bass.reads);
    assert_eq!(m.channel_of("kick").unwrap().key, "kick");
}

// What the graph finds that a read's place couldn't: a channel given a name bound above the
// mixer. Its property reads no track by name.
#[test]
fn a_channel_given_a_binding_is_found() {
    let (_f, s) = song(
        "export default song(() => {\n  const k = kick.out;\n  \
         return master(mixer({ kick: k, hats: hats.out, bass: duck(bass.out, kick.env, { depth: db(-9) }), keys: keys.out }));\n});\n",
    );
    assert_eq!(channels(&s), owned(&[("kick", &["kick"]), ("hats", &["hats"]), ("bass", &["bass"]), ("keys", &["keys"])]));
    assert!(the_mixer(&s).channel_of("kick").unwrap().reads.is_empty(), "the property names no track");
    // The builders edit a channel's text, so they leave this one to the code, saying why.
    let (f, mut s) = (_f, s);
    let before = std::fs::read_to_string(f.0.join("song.ts")).unwrap();
    let e = s.duplicate_track("kick").unwrap_err().to_string();
    assert!(e.contains("Kick's mixer channel doesn't name it"), "{e}");
    let crate::Deleted::Blocked(deps) = s.delete_track("kick", true).unwrap() else { panic!("the binding blocks the delete") };
    assert_eq!((deps.len(), deps[0].file.as_str(), deps[0].line), (1, "song.ts", 11), "{deps:?}");
    assert_eq!(std::fs::read_to_string(f.0.join("song.ts")).unwrap(), before);
}

// Two tracks summed into one channel share it, in the order the property reads them; a track in
// two channels has none; a track in no channel has none.
#[test]
fn tracks_share_a_channel_and_one_in_two_has_none() {
    let (_f, s) = song("export default song(() => master(mixer({ drums: sum(hats.out, kick.out), bass: bass.out })));\n");
    assert_eq!(channels(&s), owned(&[("drums", &["hats", "kick"]), ("bass", &["bass"])]));
    let m = the_mixer(&s);
    assert_eq!(m.channel_of("kick").unwrap().key, "drums");
    assert!(m.channel_of("keys").is_none(), "keys reaches no channel");
    let (_f, s) = song("export default song(() => master(mixer({ a: kick.out, b: { in: kick.out, db: -3 }, keys: keys.out })));\n");
    assert_eq!(channels(&s), owned(&[("a", &["kick"]), ("b", &["kick"]), ("keys", &["keys"])]));
    assert!(the_mixer(&s).channel_of("kick").is_none(), "kick is in two channels");
}

// PKG-13: an audio edge into a sidechain input only listens. A project's own component that adds
// its key input to its audio passes the kick into the bass's channel only when that input isn't a
// sidechain.
#[test]
fn a_sidechain_input_only_listens() {
    let keyed = |sidechain: bool| {
        format!(
            "import {{ component, kernel, port, signature, type Audio }} from \"@m2/core\";\n\
             export const keyed = component(\n  {{ id: \"keyed\", label: \"Keyed\", signature: signature<(x: Audio, key: Audio) => Audio>(),\n    \
             inputs: {{ in: port.audio(), key: port.audio({{ sidechain: {sidechain} }}) }}, output: port.audio() }},\n  \
             (x: Audio, key: Audio) => kernel(\"std:add\", {{ a: x, b: key }}),\n);\n"
        )
    };
    let body = "import { keyed } from \"./keyed\";\n\
                export default song(() => master(mixer({ kick: kick.out, bass: keyed(bass.out, kick.out), keys: keys.out })));\n";
    let f = fixture("examples/first-song");
    f.write("keyed.ts", keyed(true));
    f.write("song.ts", format!("{HEAD}{body}"));
    let mut s = f.open();
    assert!(s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    assert_eq!(channels(&s), owned(&[("kick", &["kick"]), ("bass", &["bass"]), ("keys", &["keys"])]));
    f.write("keyed.ts", keyed(false));
    assert!(s.refresh());
    assert_eq!(channels(&s), owned(&[("kick", &["kick"]), ("bass", &["bass", "kick"]), ("keys", &["keys"])]));
    assert!(the_mixer(&s).channel_of("kick").is_none(), "heard in two channels");
}

// A strip whose controls are signals: a panner driven by an LFO passes the walk on to its gain,
// which reads the channel's `db`; a gain driven by one, with no panner, reads no control of its
// channel, so the read's place says which.
#[test]
fn a_strip_driven_by_signals_is_found() {
    let (_f, s) = song(
        "export default song(({ time }) => master(mixer({\n  kick: { in: kick.out, db: range(lfo(time.beats), -6, 0) },\n  \
         hats: { in: hats.out, pan: range(lfo(time.beats), -1, 1) },\n  keys: keys.out,\n})));\n",
    );
    assert_eq!(channels(&s), owned(&[("kick", &["kick"]), ("hats", &["hats"]), ("keys", &["keys"])]));
}

// A track another track reads (a bus) reaches the mixer as that one: only the song's own nodes are
// walked.
#[test]
fn a_bus_track_is_its_own_channel() {
    let f = fixture("examples/first-song");
    f.write(
        "tracks/drums.ts",
        "import { track } from \"@m2/core\";\nimport { sum } from \"std/math\";\nimport hats from \"./hats\";\nimport kick from \"./kick\";\n\n\
         export default track(\"drums\", { name: \"Drums\" }, () => sum(kick.out, hats.out));\n",
    );
    let body = "import drums from \"./tracks/drums\";\nexport default song(() => master(mixer({ drums: drums.out, keys: keys.out })));\n";
    f.write("song.ts", format!("{HEAD}{body}"));
    let s = f.open();
    assert!(s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    assert_eq!(channels(&s), owned(&[("drums", &["drums"]), ("keys", &["keys"])]));
}

// ---------------------------------------------------------------- 8b.3b: the builders and the view

/// The control the view's knob id `id` names now.
fn control(s: &Session, id: &str) -> crate::Control {
    s.controls().into_iter().find(|c| c.id == id).unwrap_or_else(|| panic!("no control {id}"))
}

/// The strip of channel `key`.
fn strip(v: &MixView, key: &str) -> Strip {
    v.strips.iter().find(|x| x.channel.as_deref() == Some(key)).unwrap_or_else(|| panic!("no strip for {key}: {v:?}")).clone()
}

/// The song rendered offline for `seconds`, from its start.
fn render(s: &Session, seconds: f64) -> Vec<[f32; 2]> {
    let rev = s.accepted();
    m2_runtime::Renderer::new(&rev.compiled.flat, &rev.compiled.data, &s.packages, 48_000).unwrap().render(seconds).unwrap()
}

/// The largest difference between two renders, sample for sample.
fn apart(a: &[[f32; 2]], b: &[[f32; 2]]) -> f32 {
    assert_eq!(a.len(), b.len());
    a.iter().zip(b).flat_map(|(x, y)| [(x[0] - y[0]).abs(), (x[1] - y[1]).abs()]).fold(0.0, f32::max)
}

// design-8b §5.4, §5.6 `every_channel_has_a_fader`: first-song's Mix view. A strip for each track in
// the track order, with its name and colour; every channel has a fader, the kick's and the bass's
// left out (0 dB, inherited) and knobs all the same; hats and keys write a pan, the others can get
// their first; the master has its fader and its ceiling. Every id is a knob of the graph view.
#[test]
fn every_channel_has_a_fader() {
    let s = fixture("examples/first-song").open();
    let v = s.mix();
    assert_eq!((v.mixer, v.message.as_deref()), ("ok", None));
    let rows: Vec<(&str, &str, &str)> =
        v.strips.iter().map(|x| (x.channel.as_deref().unwrap(), x.label.as_str(), x.colour.as_str())).collect();
    let timeline = s.timeline();
    let colour = |k: &str| timeline.tracks.iter().find(|t| t.module == k).unwrap().colour.clone();
    assert_eq!(
        rows,
        [
            ("kick", "Kick", &*colour("kick")),
            ("hats", "Hats", &*colour("hats")),
            ("bass", "Bass", &*colour("bass")),
            ("keys", "Keys", &*colour("keys"))
        ]
    );
    for key in ["kick", "bass"] {
        let x = strip(&v, key);
        let db = control(&s, x.db.as_deref().unwrap());
        assert_eq!((db.written, db.value, x.pan.is_none(), x.pan_insert), (false, 0.0, true, true), "{key}");
        assert!(db.id.ends_with(&format!("/@channels.{key}.db")), "{}", db.id);
    }
    for (key, level, pan) in [("hats", -6.0, 0.3), ("keys", -5.0, -0.25)] {
        let x = strip(&v, key);
        let (db, p) = (control(&s, x.db.as_deref().unwrap()), control(&s, x.pan.as_deref().unwrap()));
        assert_eq!((db.written, db.value, p.value, x.pan_insert), (true, level, pan, false), "{key}");
    }
    let knobs: Vec<String> =
        s.view().modules.iter().flat_map(|m| m.nodes.iter().flat_map(|n| n.knobs.iter().map(|k| k.id.clone()))).collect();
    let master = v.master.clone().unwrap();
    let ids = v.strips.iter().flat_map(|x| [x.db.clone(), x.pan.clone()]).chain([master.db.clone(), master.ceiling.clone()]).flatten();
    for id in ids {
        assert!(knobs.contains(&id), "{id} is a knob of the graph view");
    }
    assert!(master.db.unwrap().ends_with("/@db") && master.ceiling.unwrap().ends_with("/@ceiling"));
}

// taste.md §2.10, KN-2: a level or a pan an automation lane drives keeps its strip's control, read
// only, and the strip names the lane ("Follows the Hats volume lane", Show lane, Remove automation);
// the master's too. A level that is some other signal names none.
#[test]
fn a_level_a_lane_drives_names_the_lane() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let v = s.mix();
    let (hats, master) = (strip(&v, "hats"), v.master.clone().unwrap());
    assert!(hats.db_lane.is_none() && hats.pan_lane.is_none() && master.db_lane.is_none());
    s.automate(s.accepted().number, hats.db.as_deref().unwrap()).unwrap();
    s.automate(s.accepted().number, hats.pan.as_deref().unwrap()).unwrap();
    s.automate(s.accepted().number, master.db.as_deref().unwrap()).unwrap();
    let v = s.mix();
    let (hats, master) = (strip(&v, "hats"), v.master.clone().unwrap());
    let lane = |l: &str| Some(crate::view::LaneRef { module: "song".into(), lane: l.into() });
    assert_eq!((hats.db.as_deref(), hats.db_lane, hats.pan.as_deref(), hats.pan_lane), (None, lane("hats-volume"), None, lane("hats-pan")));
    assert_eq!((master.db.as_deref(), master.db_lane), (None, lane("master-volume")));
    let json = serde_json::to_value(&v).unwrap();
    assert_eq!(json["strips"][1]["db_lane"], serde_json::json!({"module": "song", "lane": "hats-volume"}));
    assert_eq!(json["strips"][0]["db_lane"], serde_json::Value::Null);
    // A signal that isn't a lane names none.
    let (_, s) = song(
        "export default song(({ time }) => master(mixer({\n  kick: { in: kick.out, db: range(lfo(time.beats), -6, 0) },\n  keys: keys.out,\n})));\n",
    );
    let kick = strip(&s.mix(), "kick");
    assert_eq!((kick.db.as_deref(), kick.db_lane), (None, None));
}

// design-8b §2.3, §5.4: a channel two tracks reach is one strip, named for both; a track in no
// channel, or in two, has a strip with no fader that says why. A channel no track reaches comes
// after the tracks', named by its key.
#[test]
fn shared_and_missing_channels_have_their_strips() {
    // The keys only drive the bass's duck.
    let (_f, s) = song(
        "import { follow, osc } from \"std/dsp\";\n\
         export default song(() => master(mixer({ drums: sum(hats.out, kick.out), bass: duck(bass.out, follow(keys.out)), hum: osc.sine(55) })));\n",
    );
    let v = s.mix();
    type Row<'a> = (Vec<String>, Option<&'a str>, &'a str, Option<&'a str>, bool);
    let rows: Vec<Row> =
        v.strips.iter().map(|x| (x.modules.clone(), x.channel.as_deref(), x.label.as_str(), x.why.as_deref(), x.db.is_some())).collect();
    let m = |k: &[&str]| k.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    assert_eq!(
        rows,
        [
            (m(&["hats", "kick"]), Some("drums"), "Hats + Kick", None, true),
            (m(&["bass"]), Some("bass"), "Bass", None, true),
            (m(&["keys"]), None, "Keys", Some("Not in the mixer"), false),
            (m(&[]), Some("hum"), "hum", None, true),
        ]
    );
    assert_eq!(strip(&v, "hum").colour, "graphite");
    let (_f, s) = song(
        "export default song(() => master(mixer({ a: kick.out, b: { in: kick.out, db: -3 }, rest: sum(hats.out, bass.out, keys.out) })));\n",
    );
    let kick = s.mix().strips.into_iter().find(|x| x.modules == ["kick"]).unwrap();
    assert_eq!((kick.channel, kick.why.as_deref(), kick.db), (None, Some("In 2 channels: set its level in code"), None));
}

// design-8b §2.3: a song with no std mixer, or two, shows its tracks with no faders and says why.
#[test]
fn a_song_without_one_mixer_says_why() {
    let (_f, s) = song("export default song(() => master(sum(kick.out, hats.out, bass.out, keys.out)));\n");
    let v = s.mix();
    assert_eq!((v.mixer, v.message.as_deref()), ("none", Some("This song doesn't use std's mixer: its levels are in code.")));
    let order: Vec<String> = s.timeline().tracks.iter().map(|t| t.label.clone()).collect();
    assert_eq!(v.strips.iter().map(|x| x.label.clone()).collect::<Vec<_>>(), order, "in the track order");
    assert!(v.strips.iter().all(|x| x.db.is_none() && x.channel.is_none() && x.why.is_none()), "{v:?}");
    assert!(v.master.is_some_and(|m| m.db.is_some()));
    let (_f, s) = song(
        "export default song(() => master(sum(mixer({ kick: kick.out }), mixer({ hats: hats.out, bass: bass.out, keys: keys.out }))));\n",
    );
    assert_eq!(s.mix().message.as_deref(), Some("This song has 2 mixers: set its levels in code."));
    let s = fixture("templates/empty").open();
    let v = s.mix();
    assert_eq!((v.mixer, v.strips.len(), v.master.is_some()), ("ok", 0, true));
}

// design-8b §5.2, §5.6 `a_bare_entry_is_wrapped_on_its_first_move` and `undo_puts_the_bare_entry_back`:
// the first move of a fader the channel leaves out writes it into its entry, wrapping a bare one
// (a duck's call included) and keeping its text: one `Values` step, shown as the entry. Undo puts
// the entry's text back, byte for byte.
#[test]
fn a_bare_entry_is_wrapped_on_its_first_move() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let original = f.read("song.ts");
    // Shown as the entry, its old value cut to `…` when the line would be long.
    for (key, before, after, shown) in [
        ("kick", "kick: kick.out", "kick: { in: kick.out, db: db(-3) }", ("kick: kick.out", "kick: { in: kick.out, db: db(-3) }")),
        (
            "bass",
            "bass: duck(bass.out, kick.env, { depth: db(-9) })",
            "bass: { in: duck(bass.out, kick.env, { depth: db(-9) }), db: db(-3) }",
            ("bass: …", "bass: { in: …, db: db(-3) }"),
        ),
    ] {
        let id = strip(&s.mix(), key).db.unwrap();
        let rev = s.accepted().number;
        let done = s.set(rev, &id, -3.0, None).unwrap();
        assert_eq!(f.read("song.ts"), original.replacen(before, after, 1), "{key}");
        assert_eq!((done.before.as_str(), done.after.as_str(), done.value), (shown.0, shown.1, -3.0));
        let now = control(&s, strip(&s.mix(), key).db.as_deref().unwrap());
        assert_eq!((now.written, now.value, now.literal.as_str()), (true, -3.0, "-3"), "{key}");
        assert!(done.origin.same_value(&now.origin), "the edit says where the value is written: {:?}", done.origin);
        let undone = s.undo().unwrap();
        assert_eq!((undone.before.as_str(), undone.after.as_str()), (shown.1, shown.0));
        assert_eq!(f.read("song.ts"), original, "{key}: undo puts the entry back");
        let back = control(&s, strip(&s.mix(), key).db.as_deref().unwrap());
        assert_eq!((back.written, back.value), (false, 0.0));
    }
}

// design-8b §5.2, §5.6 `an_insertion_follows_the_objects_style`: `db(-3)` where another entry writes
// `db(…)` and the file imports it; a plain number where none does; into an object entry as one more
// property; a shorthand (`k`) named in full. Releasing at 0 dB writes nothing.
#[test]
fn an_insertion_follows_the_objects_style() {
    let body = "export default song(() => {\n  const k = kick.out;\n  \
                return master(mixer({ k, hats: { in: hats.out, pan: 0.3 }, bass: bass.out, keys: { in: keys.out, db: -5 } }));\n});\n";
    let (f, mut s) = song(body);
    let original = f.read("song.ts");
    for (key, before, after) in [
        ("k", "{ k,", "{ k: { in: k, db: -3 },"),
        ("hats", "{ in: hats.out, pan: 0.3 }", "{ in: hats.out, pan: 0.3, db: -3 }"),
        ("bass", "bass: bass.out", "bass: { in: bass.out, db: -3 }"),
    ] {
        let id = strip(&s.mix(), key).db.unwrap();
        let rev = s.accepted().number;
        let at_default = s.set(rev, &id, 0.0, None).unwrap();
        assert_eq!((at_default.revision, f.read("song.ts")), (rev, original.clone()), "{key}: at its default, nothing is written");
        s.set(rev, &id, -3.0, None).unwrap();
        assert_eq!(f.read("song.ts"), original.replacen(before, after, 1), "{key}");
        s.undo().unwrap();
        assert_eq!(f.read("song.ts"), original);
    }
    // The same entries, in a mixer whose `keys` writes `db(-5)`.
    f.write("song.ts", original.replace("db: -5", "db: db(-5)"));
    assert!(s.refresh());
    let id = strip(&s.mix(), "hats").db.unwrap();
    s.set(s.accepted().number, &id, -3.0, None).unwrap();
    assert!(f.read("song.ts").contains("{ in: hats.out, pan: 0.3, db: db(-3) }"), "{}", f.read("song.ts"));
}

// design-8b §5.6 `the_master_fader_writes_master`: the master's fader, left out, is written into
// its call as its options, on a line of their own (next D4e); undo takes it out.
#[test]
fn the_master_fader_writes_master() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let original = f.read("song.ts");
    let id = s.mix().master.unwrap().db.unwrap();
    let done = s.set(s.accepted().number, &id, -2.5, None).unwrap();
    assert_eq!((done.before.as_str(), done.after.as_str()), ("master(…)", "master(…, { db: -2.5 })"));
    assert_eq!(f.read("song.ts"), original.replacen("    }),\n  ),\n);", "    }),\n    { db: -2.5 },\n  ),\n);", 1));
    s.undo().unwrap();
    assert_eq!(f.read("song.ts"), original);
}

// design-8b §5.1, §5.6 `the_fader_foot_is_silence`: a fader at its foot writes −96 dB, and that
// channel plays exact silence: the song sounds as it does with the channel taken out of the mixer.
#[test]
fn the_fader_foot_is_silence() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let id = strip(&s.mix(), "kick").db.unwrap();
    s.set(s.accepted().number, &id, -96.0, None).unwrap();
    assert!(f.read("song.ts").contains("kick: { in: kick.out, db: db(-96) }"));
    let foot = render(&s, 4.0);
    f.write("song.ts", f.read("song.ts").replace("      kick: { in: kick.out, db: db(-96) },\n", ""));
    assert!(s.refresh());
    assert_eq!(channels(&s).len(), 3, "the kick's channel is out");
    let without = render(&s, 4.0);
    assert!(without.iter().any(|x| x[0] != 0.0), "the rest plays");
    assert!(foot.iter().zip(&without).all(|(a, b)| a == b), "the kick's channel is silent at its foot");
}

// design-8b §5.3, §5.6 `the_first_pan_inserts_a_centred_panner`, lead decision T4: a channel's first
// pan writes `pan: 0` into its entry (wrapping a bare one) as one `Confined` step, though the new
// panner renames the panners after it (ID-14): the assertion is by channel. It is inaudible: the
// song renders within 1e-5 (−100 dBFS) of before. Its value is then a knob; a turn and two undos put the code
// back.
#[test]
fn the_first_pan_inserts_a_centred_panner() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let original = f.read("song.ts");
    let before = render(&s, 4.0);
    for (key, entry, panned, shown) in [
        ("kick", "kick: kick.out", "kick: { in: kick.out, pan: 0 }", ("kick: kick.out", "kick: { in: kick.out, pan: 0 }")),
        (
            "bass",
            "bass: duck(bass.out, kick.env, { depth: db(-9) })",
            "bass: { in: duck(bass.out, kick.env, { depth: db(-9) }), pan: 0 }",
            ("bass: …", "bass: { in: …, pan: 0 }"),
        ),
    ] {
        let done = s.mix_insert(key, "pan", 0.0).unwrap();
        assert_eq!(f.read("song.ts"), original.replacen(entry, panned, 1), "{key}");
        assert_eq!((done.edited.before.as_str(), done.edited.after.as_str()), shown);
        let x = strip(&s.mix(), key);
        assert_eq!((x.pan.as_deref(), x.pan_insert), (Some(done.control.as_str()), false));
        let pan = control(&s, &done.control);
        assert_eq!((pan.written, pan.value), (true, 0.0));
        // A centred panner is 1e-7 from unity on a mono signal (cos(π/4)·√2 in f32), and the limiter,
        // which works on first-song's kicks, carries that on: measured 1.1e-6, −119 dBFS.
        let heard = apart(&before, &render(&s, 4.0));
        assert!(heard <= 1e-5, "{key}: the first pan is inaudible ({heard})");
        // The drag goes on as the new knob's; its release is a second step.
        s.set(s.accepted().number, &done.control, -0.4, None).unwrap();
        assert!(f.read("song.ts").contains(&panned.replace("pan: 0", "pan: -0.4")));
        s.undo().unwrap();
        assert_eq!(f.read("song.ts"), original.replacen(entry, panned, 1));
        s.undo().unwrap();
        assert_eq!(f.read("song.ts"), original, "{key}: two undos put the code back");
    }
    // An object entry gets one more property.
    let f = fixture("templates/starter");
    let mut s = f.open();
    s.mix_insert("clap", "pan", 0.0).unwrap();
    assert!(f.read("song.ts").contains("clap: { in: clap.out, db: db(2), pan: 0 },"), "{}", f.read("song.ts"));
}

// design-8b §5.3: what the first pan refuses, writing nothing: a channel that pans already, a
// control every channel has, one it doesn't declare, and a channel there isn't.
#[test]
fn the_first_pan_refuses_what_it_cant_write() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let original = f.read("song.ts");
    let said = |e: EditError| e.to_string();
    assert!(said(s.mix_insert("hats", "pan", 0.0).unwrap_err()).contains("Can't add a pan to Hats: its entry names it already."));
    assert!(said(s.mix_insert("kick", "db", 0.0).unwrap_err()).contains("a channel always has its db, so turning it writes it"));
    assert!(said(s.mix_insert("kick", "width", 0.0).unwrap_err()).contains("a channel has no width"));
    assert_eq!(s.mix_insert("lead", "pan", 0.0).unwrap_err(), EditError::NotFound("channel lead".into()));
    assert_eq!(f.read("song.ts"), original);
    assert!(!s.can_undo());
    let (_f, mut s) = song(
        "export default song(() => master(sum(mixer({ kick: kick.out }), mixer({ hats: hats.out, bass: bass.out, keys: keys.out }))));\n",
    );
    assert!(said(s.mix_insert("kick", "pan", 0.0).unwrap_err()).contains("This song has 2 mixers: set its levels in code."));
}

// Lead decision T4: the first pan's assertion holds the edit to the strip, by channel. The same
// Tx writing a filter beside the pan, or the pan on another channel, is refused.
#[test]
fn the_first_pans_assertion_refuses_more_than_the_pan() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let original = f.read("song.ts");
    let Mixing::One(m) = mixer(s.accepted()) else { panic!("one mixer") };
    let before = (super::channels(&m), strips(s.accepted(), m.frame));
    for (written, why) in [
        ("kick: { in: lowpass(kick.out), pan: 0 }", "add or take out more than the pan"),
        ("kick: { in: kick.out, pan: 0.5 }", "give the channel a pan of 0.5"),
        ("kick: { in: kick.out, pan: 0, db: -1 }", "change a value"),
        (
            "kick: kick.out, hats: { in: hats.out, db: db(-6), pan: 0.3 }, bass: { in: duck(bass.out, kick.env, { depth: db(-9) }), pan: 0 }",
            "not give the channel a pan",
        ),
    ] {
        let text = match written.contains("bass") {
            // The whole mixer object, written again.
            true => original.replacen(
                "kick: kick.out,\n      hats: { in: hats.out, db: db(-6), pan: 0.3 },\n      // Sidechain: the bass ducks whenever the kick hits.\n      bass: duck(bass.out, kick.env, { depth: db(-9) }),",
                &format!("{written},"),
                1,
            ),
            false => original.replacen("kick: kick.out", written, 1),
        };
        let text = text.replacen("import { duck }", "import { lowpass } from \"std/filter\";\nimport { duck }", 1);
        assert_ne!(text.replacen("import { lowpass } from \"std/filter\";\n", "", 1), original, "{written}");
        let file = plan::path("song.ts").unwrap();
        let op = m2_song::tx::Op::Write { file: file.clone(), bytes: m2_song::tx::Payload::Text(text.into()) };
        let expect = Expect::Confined(Confined { ranges: [(file, Declared::Whole)].into(), ..Confined::default() });
        let tx = plan::planned(&s, ("a".into(), "b".into()), vec![op], expect);
        let e =
            plan::run(&mut s, tx, |_, checked| one_more(checked, &before, "kick", "pan", 0.0).map_err(EditError::Rejected)).unwrap_err();
        assert!(e.to_string().contains(why), "{written}: {e}");
        assert_eq!(f.read("song.ts"), original);
    }
}

// HOME-1 for the Mix view: every fader and pan it names is a value whose home is the code.
#[test]
fn every_mix_value_names_its_home() {
    for name in ["examples/first-song", "examples/sounds", "templates/starter", "templates/empty"] {
        let s = fixture(name).open();
        let v = s.mix();
        let master = v.master.clone().unwrap();
        for id in v.strips.iter().flat_map(|x| [x.db.clone(), x.pan.clone()]).chain([master.db, master.ceiling]).flatten() {
            assert_eq!(control(&s, &id).home, m2_session::Home::Code, "{name}: {id}");
        }
        assert!(v.strips.iter().all(|x| x.db.is_some()), "{name}: every strip has a fader");
    }
}

// D11, HOME-2, design-8b §5.2: a cable into a fader its channel leaves out is written into the entry
// as a value would be, wrapping a bare one, and so is the lane that automates it; undo puts the
// entry back.
#[test]
fn a_cable_into_a_left_out_fader_wraps_its_entry() {
    let body = "export default song(() => {\n  const wob = range(lfo(1), -12, 0);\n  \
                return master(mixer({ kick: kick.out, hats: hats.out, bass: bass.out, keys: keys.out }));\n});\n";
    let (f, mut s) = song(body);
    let original = f.read("song.ts");
    let wob = s.outlets().into_iter().find(|o| o.name == "wob").unwrap();
    let db = strip(&s.mix(), "kick").db.unwrap();
    let done = s.connect(wob.revision, &wob.id, &db).unwrap();
    assert_eq!(f.read("song.ts"), original.replacen("kick: kick.out", "kick: { in: kick.out, db: wob }", 1));
    assert_eq!((done.before.as_str(), done.after.as_str()), ("kick: kick.out", "kick: { in: kick.out, db: wob }"));
    s.undo().unwrap();
    assert_eq!(f.read("song.ts"), original);
    let db = strip(&s.mix(), "bass").db.unwrap();
    let done = s.automate(s.accepted().number, &db).unwrap();
    assert!(
        f.read("song.ts").contains("bass: { in: bass.out, db: lane.control(\"bass-volume\", \"db\", db(0)) }"),
        "{}\n{done:?}",
        f.read("song.ts")
    );
    s.undo().unwrap();
    assert_eq!(f.read("song.ts"), original);
}

// WRITE-21: undoing a wrap finds both of its inserted texts where later edits carried them and
// takes out exactly those, after an outside save above it and beside it.
#[test]
fn undo_of_a_wrap_survives_other_edits() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let id = strip(&s.mix(), "kick").db.unwrap();
    s.set(s.accepted().number, &id, -3.0, None).unwrap();
    let outside = |t: &str| format!("// a note\n{}", t.replace("db: db(-5)", "db: db(-4)"));
    f.write("song.ts", outside(&f.read("song.ts")));
    assert!(s.refresh());
    s.undo().unwrap();
    let original = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song/song.ts")).unwrap();
    assert_eq!(f.read("song.ts"), outside(&original), "only the wrap is taken out");
}
