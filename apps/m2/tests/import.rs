//! `m2 import` (studio lane A's A4): WAV files into a song from the terminal, as the app's File ▸
//! Import Audio… brings them: copied into `assets/`, their clips on a track's audio lane one after
//! another, or each on a new audio track; one edit that undo takes back. It takes the project's
//! lock (WRITE-18).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let (f, t) = (e.path(), to.join(e.file_name()));
        if f.is_dir() { copy(&f, &t) } else { std::fs::copy(&f, &t).map(drop).unwrap() }
    }
}

struct Temp(PathBuf);

impl Drop for Temp {
    fn drop(&mut self) {
        // Read-only files too: a pack m2 installed in a HOME here (PACK-16).
        let _ = m2_song::pack::remove_tree(&self.0);
    }
}

fn m2(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_m2")).env("HOME", m2_song::pack::test_home()).args(args).output().unwrap()
}

/// A mono 16-bit WAV of `seconds` at 48 kHz.
fn wav(path: &Path, seconds: f64) {
    let spec = hound::WavSpec { channels: 1, sample_rate: 48_000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for i in 0..(seconds * 48_000.0) as usize {
        w.write_sample(((i as f32 * 0.03).sin() * 8_000.0) as i16).unwrap();
    }
    w.finalize().unwrap();
}

#[test]
fn import_places_a_clip_and_takes_the_lock() {
    let temp = Temp(std::env::temp_dir().join(format!("m2-import-cli-{}", std::process::id())));
    let song = temp.0.join("resample");
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/resample"), &song);
    let (a, b) = (temp.0.join("Lead Vox.WAV"), temp.0.join("hit.wav"));
    wav(&a, 1.0);
    wav(&b, 0.25);
    let dir = song.to_str().unwrap();
    let out = m2(&["import", dir, a.to_str().unwrap(), b.to_str().unwrap(), "--track", "chops", "--at", "9"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(said.contains("imported Lead Vox.WAV as assets/Lead Vox.wav") && said.contains("onto chops from beat 32"), "{said}");
    let lane = std::fs::read_to_string(song.join("data/chops/clips.json")).unwrap();
    // Bar 9 is beat 32; 1 s at 120 BPM is 2 beats, then the hit.
    assert!(lane.contains(r#""start": 32, "length": 2, "asset": "Lead Vox.wav""#), "{lane}");
    assert!(lane.contains(r#""start": 34, "length": 0.5, "asset": "hit.wav""#), "{lane}");
    assert_eq!(std::fs::read(song.join("assets/hit.wav")).unwrap(), std::fs::read(&b).unwrap());
    // With no track named: a new audio track, named after the file, at the end.
    let new = m2(&["import", dir, a.to_str().unwrap(), "--at", "3"]);
    assert!(new.status.success(), "{}", String::from_utf8_lossy(&new.stderr));
    assert!(
        String::from_utf8_lossy(&new.stdout).contains("onto a new track Lead Vox (ID lead-vox) from beat 8"),
        "{}",
        String::from_utf8_lossy(&new.stdout)
    );
    assert!(std::fs::read_to_string(song.join("tracks/lead-vox.ts")).unwrap().contains("lane.audio(\"clips\")"));
    assert!(
        std::fs::read_to_string(song.join("data/lead-vox/clips.json"))
            .unwrap()
            .contains(r#""start": 8, "length": 2, "asset": "Lead Vox.wav""#)
    );
    // Refused in words: a track with no audio lane, a file that isn't a WAV.
    let none = m2(&["import", dir, b.to_str().unwrap(), "--track", "keys"]);
    assert!(!none.status.success() && String::from_utf8_lossy(&none.stderr).contains("has no audio lane"));
    let text = temp.0.join("notes.wav");
    std::fs::write(&text, "words").unwrap();
    let bad = m2(&["import", dir, text.to_str().unwrap(), "--track", "chops"]);
    assert!(
        String::from_utf8_lossy(&bad.stderr).contains("notes.wav isn't a WAV or FLAC file. music2 reads WAV and FLAC files."),
        "{}",
        String::from_utf8_lossy(&bad.stderr)
    );
    // While another program holds the song, it copies nothing and names the holder (WRITE-18).
    let held = m2_session::lock::acquire(&song, "music2", m2_session::lock::RETRY).unwrap();
    let c = temp.0.join("c.wav");
    wav(&c, 0.5);
    let out = m2(&["import", dir, c.to_str().unwrap(), "--track", "chops"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success() && err.contains("nothing was changed") && err.contains("music2"), "{err}");
    assert!(!song.join("assets/c.wav").exists(), "nothing copied");
    drop(held);
}
