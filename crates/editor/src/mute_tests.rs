//! Mute in song (MON-5): the option written and taken out on each way a `track()` call can be
//! written, several tracks in one undo step, refusals in words, and what a track still drives.

use m2_session::Session;

use super::*;
use crate::testing::{Fixture, MANIFEST, fixture};

fn muted(s: &Session) -> Vec<String> {
    s.accepted().compiled.flat.muted.clone()
}

fn render(s: &Session) -> Vec<[f32; 2]> {
    let rev = s.accepted();
    m2_runtime::Renderer::new(&rev.compiled.flat, &rev.compiled.data, &s.packages, 48_000).unwrap().render(2.0).unwrap()
}

fn keys(k: &[&str]) -> Vec<String> {
    k.iter().map(|s| s.to_string()).collect()
}

// MON-5: muting appends `muted: true` to the options; unmuting takes it out again, to the byte;
// the song renders without the kick meanwhile; history names it "Kick → Kick, muted in song".
#[test]
fn mute_in_song_inserts_and_removes_the_option() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let (text, sound) = (f.read("tracks/kick.ts"), render(&s));
    let done = mute_in_song(&mut s, &keys(&["kick"]), true).unwrap();
    assert_eq!((done.file.as_str(), done.before.as_str(), done.after.as_str()), ("tracks/kick.ts", "Kick", "Kick, muted in song"));
    assert_eq!(f.read("tracks/kick.ts"), text.replace("{ name: \"Kick\" }", "{ name: \"Kick\", muted: true }"));
    assert_eq!(muted(&s), ["kick"]);
    assert!(render(&s) != sound, "the kick is out of the song");
    // Again: nothing to do.
    assert_eq!(mute_in_song(&mut s, &keys(&["kick"]), true).unwrap().revision, done.revision);
    let back = mute_in_song(&mut s, &keys(&["kick"]), false).unwrap();
    assert_eq!((back.before.as_str(), back.after.as_str()), ("Kick, muted in song", "Kick"));
    assert_eq!(f.read("tracks/kick.ts"), text);
    assert!(muted(&s).is_empty());
    assert!(render(&s) == sound, "the same bits again");
}

// MON-5: a call with no options gets them after its key, and loses them whole; one whose options
// say `muted: false` has the value replaced.
#[test]
fn every_way_a_track_call_is_written() {
    let f = Fixture::new();
    f.write("music2.toml", MANIFEST);
    let song = "import { song, track } from '@m2/core';\nimport { osc } from 'std/dsp';\nimport { mixer } from 'std/mix';\n\
                const a = track('a', () => osc.sine(220));\nconst b = track('b', { muted: false }, () => osc.saw(110));\n\
                export default song(() => mixer({ a: a.out, b: b.out }));\n";
    f.write("song.ts", song);
    let mut s = f.open();
    mute_in_song(&mut s, &keys(&["a", "b"]), true).unwrap();
    let now = f.read("song.ts");
    assert!(now.contains("track('a', { muted: true }, () =>") && now.contains("track('b', { muted: true }, () =>"), "{now}");
    mute_in_song(&mut s, &keys(&["a", "b"]), false).unwrap();
    let now = f.read("song.ts");
    assert!(now.contains("track('a', () =>") && now.contains("track('b', () =>"), "{now}");
}

// MON-5, WRITE-21: several tracks muted in one Tx are one undo step; undo puts every option back.
#[test]
fn several_tracks_mute_in_one_tx_and_undo_puts_the_options_back() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let (kick, bass) = (f.read("tracks/kick.ts"), f.read("tracks/bass.ts"));
    let done = mute_in_song(&mut s, &keys(&["kick", "bass"]), true).unwrap();
    assert_eq!((done.before.as_str(), done.after.as_str()), ("Kick and Bass", "Kick and Bass, muted in song"));
    assert_eq!(muted(&s), ["bass", "kick"]);
    assert!(f.read("tracks/bass.ts").contains("{ name: \"Bass\", muted: true }"));
    let undone = crate::undo(&mut s).unwrap();
    assert_eq!(undone.after, "Kick and Bass");
    assert_eq!((f.read("tracks/kick.ts"), f.read("tracks/bass.ts")), (kick, bass));
    assert!(muted(&s).is_empty());
}

// MON-1: the song, a module that isn't there, and an empty list are refused in words.
#[test]
fn the_song_and_missing_tracks_are_refused() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let said = |e: EditError| match e {
        EditError::Rejected(m) | EditError::NotFound(m) => m,
        e => format!("{e:?}"),
    };
    assert!(said(mute_in_song(&mut s, &keys(&["song"]), true).unwrap_err()).contains("song itself can't be muted"));
    assert!(said(mute_in_song(&mut s, &keys(&["nope"]), true).unwrap_err()).contains("nope"));
    assert!(said(mute_in_song(&mut s, &[], true).unwrap_err()).contains("Name a track"));
}

// MON-2: muting the kick still ducks the bass; what the kick drives besides its sound is named by
// the module and the component that reads it.
#[test]
fn feeds_name_the_duck() {
    let f = fixture("examples/first-song");
    let s = f.open();
    let feeds = feeds(s.accepted());
    assert_eq!(feeds.get("kick").map(Vec::as_slice), Some(&[Feed { module: "song".into(), label: "Duck".into() }][..]), "{feeds:?}");
    assert!(!feeds.contains_key("hats") && !feeds.contains_key("bass"), "{feeds:?}");
}
