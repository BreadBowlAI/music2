//! `m2 bounce --track` after its root moved into `m2_editor::render::rooted` (studio C3): it still
//! writes the track's own output, and that file is a stem's, byte for byte.

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
        let _ = std::fs::remove_dir_all(&self.0);
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

    let mut packages = m2_runtime::PackageSet::default();
    packages.load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")).unwrap();
    let c = m2_session::compile(&m2_song::Project::open(&song).unwrap(), &mut packages).unwrap_or_else(|e| panic!("{e:?}"));
    let ir = rooted(&c.ir, "bass", "out").unwrap();
    let flat = m2_ir::check(&ir, &packages).unwrap();
    let stem = temp.0.join("1 Bass.wav");
    let seconds = bounce_seconds(&c.data);
    let song = std::sync::Arc::new(m2_editor::render::Parts { ir, flat, data: c.data.clone() });
    Render::new(song, &packages, spec.sample_rate, 0.0, Length::Seconds(seconds), Threads::Exactly(1))
        .unwrap()
        .write_float(&stem, |_| {})
        .unwrap();
    assert!(std::fs::read(&stem).unwrap() == std::fs::read(&bounced).unwrap(), "the stem and the bounce differ");
}
