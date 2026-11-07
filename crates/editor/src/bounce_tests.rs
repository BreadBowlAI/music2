//! Bounce to audio (RENDER-40): the track rendered as `m2 bounce --track` renders it, until it falls
//! silent as an export does, a new audio track after it in a copy of its channel, and the track muted
//! in the song: one undo step, and the song sounds the same, as long as it was.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use m2_song::{AssetName, LaneData};

use super::*;
use crate::render::{Length, Render, Threads};
use crate::testing::{Fixture, files_under, fixture};

/// Every file of the song, by its path in the folder, but the session's own (`.music2/`) and
/// `assets/`, which a bounce only adds to (FMT-14).
fn song_files(f: &Fixture) -> BTreeMap<PathBuf, Vec<u8>> {
    files_under(&f.0)
        .into_iter()
        .map(|p| p.strip_prefix(&f.0).unwrap().to_path_buf())
        .filter(|p| !p.starts_with(".music2") && !p.starts_with("assets"))
        .map(|p| (p.clone(), std::fs::read(f.0.join(&p)).unwrap()))
        .collect()
}

/// Bounce `track` of the open song to `assets/<name>.wav`, then put it in place: what it put, and
/// the file.
fn bounce(f: &Fixture, s: &mut Session, track: &str, name: &str) -> (Result<InPlace, EditError>, Bounced) {
    let b = Bounce::in_place_of(s.accepted(), &s.packages, track).unwrap();
    let render = b.render(&s.packages, Threads::Exactly(1)).unwrap();
    let done = b.write(render, &f.0, &AssetName::new(&format!("{name}.wav")).unwrap(), |_| {}).unwrap();
    (in_place(s, track, &done), done)
}

/// The song as Export Audio renders it: to the end of its last clip, then on until it falls silent.
fn exported(s: &Session) -> Vec<f32> {
    let rev = s.accepted_arc();
    // Each call its own file: tests in one process share its id, and open songs at one revision.
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let path = std::env::temp_dir().join(format!("m2-bounce-heard-{}-{}-{n}.wav", std::process::id(), rev.number));
    let song: Arc<dyn crate::render::Song> = rev.clone();
    Render::new(song, &s.packages, 48_000, 0.0, Length::Song, Threads::Exactly(1)).unwrap().write_float(&path, |_| {}).unwrap();
    let samples = hound::WavReader::open(&path).unwrap().into_samples::<f32>().map(Result::unwrap).collect();
    let _ = std::fs::remove_file(&path);
    samples
}

// RENDER-40: Keys becomes Keys audio, right after it, playing the bounce from beat 0 in a copy of
// Keys's channel (its fader and pan), and Keys is muted in the song: one Tx, one undo step, which
// puts every file of the song back byte for byte. The bounced file stays.
#[test]
fn bounce_in_place_adds_the_track_and_mutes_in_one_undo_step() {
    let song = fixture("examples/first-song");
    let mut s = song.open();
    let before = song_files(&song);
    assert!(!s.can_undo());
    let (placed, done) = bounce(&song, &mut s, "keys", "keys-bounce");
    let placed = placed.unwrap();
    assert_eq!((placed.made.module.as_str(), placed.made.label.as_str()), ("keys-audio", "Keys audio"), "{placed:?}");
    let rev = s.accepted();
    let module = |k: &str| rev.compiled.ir.modules.iter().find(|m| m.key == k).unwrap();
    assert!(module("keys").muted, "Keys is muted in the song");
    assert!(song.read("tracks/keys.ts").contains("muted: true"), "in its code");
    let Some(LaneData::Audio(clips)) = rev.compiled.data.lane("keys-audio", "clips").map(|d| (**d).clone()) else {
        panic!("an audio lane")
    };
    // To its last sound: past the song's 32 beats by Keys's release, not 2 s more (36.13 beats).
    let beats = done.sounding * 124.0 / 60.0;
    assert_eq!(clips.len(), 1);
    assert_eq!((clips[0].start, clips[0].asset.as_str()), (0.0, "keys-bounce.wav"));
    assert!((clips[0].length - beats).abs() < 1e-6, "{} beats, not {beats}", clips[0].length);
    assert!(beats > 30.0 && beats < 34.0, "{beats} beats");
    let text = song.read("song.ts");
    assert!(text.contains("\"keys-audio\": { in: keysAudio.out, db: db(-5), pan: -0.25 }"), "a copy of Keys's channel:\n{text}");
    assert!(song.read("tracks/keys-audio.ts").contains("lane.audio(\"clips\", 1)"), "Keys is mono, so is its bounce's lane");
    let order: Vec<String> = crate::timeline::timeline(&s).tracks.into_iter().filter(|t| t.track).map(|t| t.module).collect();
    let keys = order.iter().position(|m| m == "keys").unwrap();
    assert_eq!(order[keys + 1], "keys-audio", "right after it");

    crate::undo(&mut s).unwrap();
    assert!(song_files(&song) == before, "one undo puts every file back byte for byte");
    assert!(!s.can_undo(), "it was one undo step");
    assert!(song.0.join("assets/keys-bounce.wav").exists() && song.0.join("assets/keys-bounce.wav.json").exists(), "the files stay");
}

/// What the song's export holds after a bounce, against before: as long, unless the bounce's last
/// sound, `end` seconds in, comes after the song fell silent (its fader turns it under −90 dBFS
/// sooner than the bounce, which is before the fader, does): then to there and the export's half
/// second of silence. The same bits up to `end`; past it, where the bounce left out what its
/// track played under −90 dBFS, nothing louder than that.
fn same_sound(what: &str, heard: &[f32], now: &[f32], end: f64) {
    let end = (end * 48_000.0).round() as usize * 2;
    let quiet = (crate::render::TAIL_SILENCE * 48_000.0).round() as usize * 2;
    assert_eq!(now.len(), heard.len().max(end + quiet), "{what}: the export is as long as it was, or as the bounce's tail");
    // Past where it ended before, the export held nothing: silence.
    let mut heard = heard.to_vec();
    heard.resize(now.len(), 0.0);
    let end = end.min(now.len());
    let differ = heard[..end].iter().zip(&now[..end]).filter(|(a, b)| a.to_bits() != b.to_bits()).count();
    assert_eq!(differ, 0, "{what}: {differ} of {end} samples differ up to the bounce's end");
    let most = heard[end..].iter().zip(&now[end..]).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
    assert!(most < crate::render::SILENT, "{what}: {most} past the bounce's end");
}

// RENDER-40: the song sounds the same after it, as Export Audio renders it, and is as long: the Kick,
// whose envelope still ducks the bass; Hats and Keys with their faders and pans (Keys is mono, so its
// bounce plays on a mono lane, which the pan places as it placed Keys); Bass, whose channel ducks it
// under the kick; and Keys through a long reverb, which rings more than 2 s past the song's last clip.
#[test]
fn bounce_in_place_sounds_the_same() {
    let ringing = "import { track } from \"@m2/core\";\nimport { reverb } from \"@m2/effects\";\nimport { pluck } from \"@m2/synths\";\n\n\
                   export default track(\"keys\", { name: \"Keys\" }, ({ lane }) => reverb(pluck(lane.notes(\"chords\")), { size: 0.85, damping: 0.2, mix: 0.7 }));\n";
    for (track, ring) in [("kick", false), ("hats", false), ("bass", false), ("keys", false), ("keys", true)] {
        let song = fixture("examples/first-song");
        if ring {
            let manifest = song.read("music2.toml");
            song.write("music2.toml", format!("{manifest}\"@m2/effects\" = \"1\"\n"));
            song.write("tracks/keys.ts", ringing);
        }
        let mut s = song.open();
        let heard = exported(&s);
        let (placed, done) = bounce(&song, &mut s, track, &format!("{track}-bounce"));
        placed.unwrap();
        if ring {
            let end = 32.0 * 60.0 / 124.0;
            assert!(done.sounding > end + 2.0 && done.sounding < end + 9.0, "it rings past 2 s after the song's end: {} s", done.sounding);
        }
        same_sound(&format!("{track}{}", if ring { " ringing" } else { "" }), &heard, &exported(&s), done.sounding);
    }
}

// RENDER-40: one bounce after another doesn't make the song longer: each clip ends at its last sound.
#[test]
fn bounces_one_after_another_keep_the_songs_length() {
    let song = fixture("examples/first-song");
    let mut s = song.open();
    let heard = exported(&s);
    let (placed, keys) = bounce(&song, &mut s, "keys", "keys-bounce");
    placed.unwrap();
    let (placed, kick) = bounce(&song, &mut s, "kick", "kick-bounce");
    placed.unwrap();
    same_sound("keys, then kick", &heard, &exported(&s), keys.sounding.min(kick.sounding));
}

// RENDER-40: what can't be bounced in place is refused in words, before anything is rendered; what
// can is let through.
#[test]
fn bounce_in_place_refuses_in_words() {
    let song = fixture("examples/first-song");
    let s = song.open();
    let refused = |s: &Session, track: &str| match Bounce::in_place_of(s.accepted(), &s.packages, track) {
        Err(e) => e.to_string(),
        Ok(_) => panic!("{track} was bounced"),
    };
    assert!(refused(&s, "vox").contains("There's no track `vox`"), "{}", refused(&s, "vox"));
    assert!(refused(&s, "song").contains("isn't a track"), "{}", refused(&s, "song"));
    // What `m2 bounce --export` names must be one of the track's sounds.
    let plan = |export: &str| match Bounce::plan(&s.accepted().compiled, &s.packages, Some(("kick", export)), 0.0, None, 48_000) {
        Err(e) => format!("{e:#}"),
        Ok(_) => panic!("kick.{export} was bounced"),
    };
    assert_eq!(plan("nope"), "Kick has no output `nope`: its outputs are env, out");
    assert_eq!(plan("env"), "Kick's env isn't a sound, so it can't be bounced");
    drop(s);
    // Muted in the song already: its bounce would be silence.
    let hats = song.read("tracks/hats.ts");
    song.write("tracks/hats.ts", hats.replace("{ name: \"Hats\" }", "{ name: \"Hats\", muted: true }"));
    // Read by its channel through a name of the song's own, and in brackets: no `kick.out`, or
    // `keys.out`, to swap.
    let text = song.read("song.ts");
    let text = text.replace("export default song(() =>\n  master(", "export default song(() => {\n  const k = kick.out;\n  return master(");
    let text = text.replace("  ),\n);\n", "  );\n});\n").replace("kick: kick.out,", "kick: k,");
    song.write("song.ts", text.replace("{ in: keys.out,", "{ in: (keys).out,"));
    let s = song.open();
    let e = refused(&s, "hats");
    assert!(e.contains("Hats is muted in the song"), "{e}");
    let e = refused(&s, "kick");
    assert!(e.contains("Kick's mixer channel doesn't read it by name"), "{e}");
    let e = refused(&s, "keys");
    assert_eq!(
        e,
        "edit rejected: Keys's mixer channel doesn't read it by name, so music2 can't copy it for the bounce: write it `keys.out` there."
    );
    assert!(!song.0.join("assets").exists(), "nothing was written");
}

// RENDER-40: another track that plays the track's sound, by any of its outputs, is refused (muting it
// would silence that too); one that only listens to it, as a sidechain key, isn't (a mute never
// silences a key, MON-2).
#[test]
fn bounce_in_place_follows_where_a_mute_reaches() {
    let song = fixture("examples/first-song");
    let kick = song.read("tracks/kick.ts");
    song.write("tracks/kick.ts", kick.replace("return { out, env:", "return { out, click: out, env:"));
    let hats = "import { track } from \"@m2/core\";\nimport { drums } from \"@m2/drums\";\nimport { add, scale } from \"std/math\";\nimport kick from \"./kick\";\n\n\
                export default track(\"hats\", { name: \"Hats\" }, ({ lane }) => add(drums.drum(lane.notes(\"hits\"), { drum: \"hats\" }), scale(kick.click, 0.5)));\n";
    song.write("tracks/hats.ts", hats);
    let s = song.open();
    let e = Bounce::in_place_of(s.accepted(), &s.packages, "kick").err().map(|e| e.to_string()).unwrap_or_default();
    assert_eq!(e, "edit rejected: Hats plays Kick's sound, which muting Kick would silence.");
    drop(s);
    let manifest = song.read("music2.toml");
    song.write("music2.toml", format!("{manifest}\"@m2/mixing\" = \"1\"\n"));
    let keyed = "import { track } from \"@m2/core\";\nimport { drums } from \"@m2/drums\";\nimport { gate } from \"@m2/mixing\";\nimport kick from \"./kick\";\n\n\
                 export default track(\"hats\", { name: \"Hats\" }, ({ lane }) => gate(drums.drum(lane.notes(\"hits\"), { drum: \"hats\" }), kick.out));\n";
    song.write("tracks/hats.ts", keyed);
    let s = song.open();
    let kept = Bounce::in_place_of(s.accepted(), &s.packages, "kick").err().map(|e| e.to_string());
    assert!(kept.is_none(), "the hats only listen to the kick: {kept:?}");
}
