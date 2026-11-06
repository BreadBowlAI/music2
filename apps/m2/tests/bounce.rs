//! `m2 bounce --track` after its root moved into `m2_editor::render::rooted` (studio C3): it still
//! writes the track's own output, and that file is a stem's, byte for byte. And Bounce to audio
//! (RENDER-40): the app's, and `m2 bounce --in-place`, write those same bytes.

mod app;

use std::path::{Path, PathBuf};
use std::process::Command;

use m2_editor::render::{Length, Render, Threads, bounce_seconds, rooted};

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

// RENDER-3, C3: a bounce of Bass is 32-bit float, 2 s past the last clip, and the same bytes as
// the app's stem of Bass (`rooted`, `Render`, `write_float`).
#[test]
fn bounce_of_a_track_still_writes_its_file() {
    let temp = Temp(std::env::temp_dir().join(format!("m2-bounce-cli-{}", std::process::id())));
    let song = temp.0.join("first-song");
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song"), &song);
    let out = Command::new(env!("CARGO_BIN_EXE_m2"))
        .env("HOME", m2_song::pack::test_home())
        .args(["bounce", song.to_str().unwrap(), "bass-stem", "--track", "bass"])
        .env("M2_AUDIO", "none")
        .env("M2_MIDI", "none")
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let bounced = song.join("assets/bass-stem.wav");
    let wav = hound::WavReader::open(&bounced).unwrap();
    let spec = wav.spec();
    assert_eq!((spec.channels, spec.bits_per_sample, spec.sample_format), (2, 32, hound::SampleFormat::Float));
    // 32 beats at 124 bpm, and 2 s.
    let seconds = 32.0 * 60.0 / 124.0 + 2.0;
    assert_eq!(wav.duration() as u64, (seconds * spec.sample_rate as f64).round() as u64);

    let mut packages = m2_runtime::PackageSet::default().with_store(m2_song::pack::test_store());
    packages.load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")).unwrap();
    let c = m2_session::compile(&m2_song::Project::open(&song).unwrap(), &mut packages).unwrap_or_else(|e| panic!("{e:?}"));
    let ir = rooted(&c.ir, "bass", "out").unwrap();
    let stem = temp.0.join("1 Bass.wav");
    let seconds = bounce_seconds(&c.data);
    let song = std::sync::Arc::new(m2_editor::render::Parts::of(&c, ir, &packages).unwrap());
    Render::new(song, &packages, spec.sample_rate, 0.0, Length::Seconds(seconds), Threads::Exactly(1))
        .unwrap()
        .write_float(&stem, |_| {})
        .unwrap();
    assert!(std::fs::read(&stem).unwrap() == std::fs::read(&bounced).unwrap(), "the stem and the bounce differ");
}

/// A WAV's samples.
fn samples(path: &Path) -> Vec<f32> {
    hound::WavReader::open(path).unwrap().into_samples::<f32>().map(Result::unwrap).collect()
}

// RENDER-40: Bounce to audio renders what `m2 bounce --track` renders, to the bit, where both have
// sound (it goes on until the track falls silent; `m2 bounce`, to 2 s past the song's last clip).
// `m2 bounce --in-place` with the app open is the app's own (one undo step of the agent's, Keys
// muted in the song, Keys audio added); with the app closed `m2` does the same itself, the same bytes.
#[test]
fn the_apps_bounce_is_m2_bounces_bits() {
    let a = app::App::open("first-song", |_| {});
    let temp = app::Temp::new("bounce-bits");
    let closed = temp.0.join("first-song");
    app::copy(&app::example("first-song"), &closed);
    let shut = closed.to_str().unwrap();
    let ok = |o: std::process::Output| {
        assert!(o.status.success(), "{}", app::said(&o));
        app::said(&o)
    };
    ok(a.m2(&["bounce", shut, "keys-bounce", "--track", "keys"]));
    let plain = samples(&closed.join("assets/keys-bounce.wav"));

    let out = ok(a.m2(&["bounce", a.dir(), "keys-bounce", "--track", "keys", "--in-place", "--as", "Claude"]));
    assert!(out.contains("in the open app: one undo step"), "{out}");
    let apps = std::fs::read(a.song.join("assets/keys-bounce.wav")).unwrap();
    let heard = samples(&a.song.join("assets/keys-bounce.wav"));
    let n = plain.len().min(heard.len());
    assert!(n > 48_000 * 2 * 15, "both hold the song: {n} samples");
    let differ = plain[..n].iter().zip(&heard[..n]).filter(|(x, y)| x.to_bits() != y.to_bits()).count();
    assert_eq!(differ, 0, "the app's bounce is m2 bounce's render where both have it");
    assert!(heard.len() < plain.len(), "and ends sooner: Keys is silent before 2 s past the song's end");
    assert!(a.read("tracks/keys.ts").contains("muted: true") && a.read("song.ts").contains("keys-audio"), "{}", a.read("song.ts"));
    assert_eq!((&a.top()["by"], &a.top()["name"]), (&serde_json::json!("Claude"), &serde_json::json!("bouncing Keys")), "{}", a.top());

    let out = ok(a.m2(&["bounce", shut, "keys-again", "--track", "keys", "--in-place"]));
    assert!(out.contains("Keys audio"), "{out}");
    assert!(std::fs::read(closed.join("assets/keys-again.wav")).unwrap() == apps, "and m2's own, the app closed, the same bytes");
    let text = std::fs::read_to_string(closed.join("song.ts")).unwrap();
    assert!(text.contains("keys-audio") && std::fs::read_to_string(closed.join("tracks/keys.ts")).unwrap().contains("muted: true"));

    // Refused with the app open: its words alone, as with it closed.
    let o = a.m2(&["bounce", a.dir(), "keys-3", "--track", "keys", "--in-place"]);
    let said = app::said(&o);
    assert!(!o.status.success() && said.contains("error: Keys is muted in the song"), "{said}");
}
