//! Bounce to audio (RENDER-40): the track rendered as `m2 bounce --track` renders it, a new audio
//! track after it in a copy of its channel, and the track muted in the song: one undo step, and
//! the song sounds the same.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use m2_song::{AssetName, LaneData};

use super::*;
use crate::render::{Length, Render, Threads, bounce_seconds};
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

/// Bounce `track` of the open song to `assets/<name>.wav`, then put it in place.
fn bounce(f: &Fixture, s: &mut Session, track: &str, name: &str) -> Result<InPlace, EditError> {
    let b = Bounce::in_place_of(s.accepted(), &s.packages, track)?;
    let render = b.render(&s.packages, Threads::Exactly(1)).unwrap();
    let done = b.write(render, &f.0, &AssetName::new(&format!("{name}.wav")).unwrap(), |_| {}).unwrap();
    in_place(s, track, &done)
}

/// The song as an export renders it, from 0 for `seconds`.
fn rendered(s: &Session, seconds: f64) -> Vec<f32> {
    let rev = s.accepted_arc();
    let path = std::env::temp_dir().join(format!("m2-bounce-heard-{}-{}.wav", std::process::id(), rev.number));
    let song: Arc<dyn crate::render::Song> = rev.clone();
    Render::new(song, &s.packages, 48_000, 0.0, Length::Seconds(seconds), Threads::Exactly(1)).unwrap().write_float(&path, |_| {}).unwrap();
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
    let seconds = bounce_seconds(&s.accepted().compiled.data);
    let placed = bounce(&song, &mut s, "keys", "keys-bounce").unwrap();
    assert_eq!((placed.made.module.as_str(), placed.made.label.as_str()), ("keys-audio", "Keys audio"), "{placed:?}");
    let rev = s.accepted();
    let module = |k: &str| rev.compiled.ir.modules.iter().find(|m| m.key == k).unwrap();
    assert!(module("keys").muted, "Keys is muted in the song");
    assert!(song.read("tracks/keys.ts").contains("muted: true"), "in its code");
    let Some(LaneData::Audio(clips)) = rev.compiled.data.lane("keys-audio", "clips").map(|d| (**d).clone()) else {
        panic!("an audio lane")
    };
    let beats = seconds * 124.0 / 60.0;
    assert_eq!(clips.len(), 1);
    assert_eq!((clips[0].start, clips[0].asset.as_str()), (0.0, "keys-bounce.wav"));
    assert!((clips[0].length - beats).abs() < 1e-6, "{} beats, not {beats}", clips[0].length);
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

// RENDER-40: the song sounds the same after it, to the bit: the Kick, whose envelope still ducks
// the bass; Hats and Keys with their faders and pans (Keys is mono, so its bounce plays on a mono
// lane, which the pan places as it placed Keys); and Bass, whose channel ducks it under the kick.
#[test]
fn bounce_in_place_sounds_the_same() {
    for track in ["kick", "hats", "bass", "keys"] {
        let song = fixture("examples/first-song");
        let mut s = song.open();
        let seconds = bounce_seconds(&s.accepted().compiled.data);
        let heard = rendered(&s, seconds);
        bounce(&song, &mut s, track, &format!("{track}-bounce")).unwrap();
        let now = rendered(&s, seconds);
        assert_eq!(heard.len(), now.len());
        let differ = heard.iter().zip(&now).filter(|(a, b)| a.to_bits() != b.to_bits()).count();
        assert_eq!(differ, 0, "{track}: {differ} of {} samples differ", heard.len());
    }
}

// RENDER-40: what can't be bounced in place is refused in words, before anything is rendered.
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
    drop(s);
    // Muted in the song already: its bounce would be silence.
    let hats = song.read("tracks/hats.ts");
    song.write("tracks/hats.ts", hats.replace("{ name: \"Hats\" }", "{ name: \"Hats\", muted: true }"));
    // Read by its channel through a name of the song's own: there is no `kick.out` to swap.
    let text = song.read("song.ts");
    let text = text.replace("export default song(() =>\n  master(", "export default song(() => {\n  const k = kick.out;\n  return master(");
    song.write("song.ts", text.replace("  ),\n);\n", "  );\n});\n").replace("kick: kick.out,", "kick: k,"));
    let s = song.open();
    let e = refused(&s, "hats");
    assert!(e.contains("Hats is muted in the song"), "{e}");
    let e = refused(&s, "kick");
    assert!(e.contains("doesn't read it by name"), "{e}");
    assert!(!song.0.join("assets").exists(), "nothing was written");
}
