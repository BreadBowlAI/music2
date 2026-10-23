//! `m2 render` and the app's Export Audio write the same file for the same project.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

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

#[test]
fn the_cli_and_the_apps_export_write_the_same_file() {
    // The app's player plays into nothing, and it listens to no MIDI keyboard: tests have no
    // audio device and open no MIDI one.
    // SAFETY: the only test in this binary, before it starts any thread of its own.
    unsafe {
        std::env::set_var("M2_AUDIO", "none");
        std::env::set_var("M2_MIDI", "none");
    }
    let temp = Temp(std::env::temp_dir().join(format!("m2-render-cli-{}", std::process::id())));
    let song = temp.0.join("first-song");
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song"), &song);

    let cli = temp.0.join("cli.wav");
    let out = Command::new(env!("CARGO_BIN_EXE_m2"))
        .env("HOME", std::env::temp_dir().join("m2-tests-no-app"))
        .arg("render")
        .arg(&song)
        .arg(&cli)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    let running = m2_server::start(m2_server::ServeOptions {
        port: 0,
        web_dir: temp.0.join("no-web"),
        token: "test-tools".into(),
        page_token: "test".into(),
    })
    .unwrap();
    let state = running.state;
    state.graph.open(&song).unwrap();
    let mut events = state.events.subscribe();
    let app = temp.0.join("app.wav");
    assert_eq!(m2_server::export::start(&state, Some(app.clone())).unwrap(), app);
    let start = Instant::now();
    let done = loop {
        assert!(start.elapsed() < Duration::from_secs(60), "the export didn't finish");
        match events.try_recv() {
            Ok(text) if text.contains(r#""done":true"#) || text.contains(r#""error""#) => break text,
            Ok(_) => {}
            Err(_) => std::thread::sleep(Duration::from_millis(10)),
        }
    };
    assert!(done.contains(r#""done":true"#), "{done}");

    let (cli_bytes, app_bytes) = (std::fs::read(&cli).unwrap(), std::fs::read(&app).unwrap());
    assert!(cli_bytes == app_bytes, "the files differ ({} and {} bytes)", cli_bytes.len(), app_bytes.len());

    // The song's 32 beats at 124 bpm, and the kick's and keys' release after them.
    let wav = hound::WavReader::open(&app).unwrap();
    assert_eq!((wav.spec().sample_rate, wav.spec().channels, wav.spec().bits_per_sample), (48_000, 2, 24));
    let end = (32.0 * 60.0 / 124.0 * 48_000.0f64).round() as u32;
    assert!(wav.duration() >= end + 24_000 && wav.duration() <= end + 480_000, "{} frames for a song that ends at {end}", wav.duration());
    let peak = wav.into_samples::<i32>().map(|s| s.unwrap().unsigned_abs()).max().unwrap();
    assert!(peak > 1 << 20, "it isn't silent: peak {peak}");
}
