//! Each track's channel, found by following the graph (ID-18, design-8b §2.3), on first-song
//! written in other ways.

use m2_session::Session;

use super::*;
use crate::Gestures;
use crate::testing::{Fixture, fixture};

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
