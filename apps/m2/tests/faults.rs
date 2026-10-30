//! RENDER-1 (the ex plan's slice 2): `m2 render` and `m2 bounce` never finish with a part silent. A
//! fault that changed the sound stops them: they exit with an error, its words and its code, and
//! write no file.
//!
//! Debug builds only: they play the probe plugin, which only a debug `m2` loads (`M2_PROBE`).
#![cfg(debug_assertions)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Temp(PathBuf);

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn put(dir: &Path, name: &str, bytes: &[u8]) {
    let p = dir.join(name);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, bytes).unwrap();
}

/// `m2 <args>`, the probe plugin playable, with no audio device and no MIDI one.
fn m2(args: &[&std::ffi::OsStr]) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_m2"));
    c.env("HOME", std::env::temp_dir().join("m2-tests-no-app"));
    c.args(args).env("M2_PROBE", "1").env("M2_AUDIO", "none").env("M2_MIDI", "none");
    c.output().unwrap()
}

// RENDER-1, HOST-3: the probe's 40th call crashes the plugin host process (once), while `m2 render`
// plays it, then while `m2 bounce` does: each exits non-zero, saying the host crashed while the
// probe on its track played, with the code `plugin.crashed`, and leaves no file. Before, the probe
// played silence from there and each said it was done.
#[test]
fn m2_render_of_a_host_crash_exits_non_zero_naming_it() {
    let temp = Temp(std::env::temp_dir().join(format!("m2-faults-cli-{}", std::process::id())));
    let song = temp.0.join("song");
    put(
        &song,
        "music2.toml",
        b"[project]\nname = \"crash\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.5\"\n",
    );
    put(
        &song,
        "song.ts",
        b"import { plugin, song, track } from '@m2/core';\nimport { osc } from 'std/dsp';\n\
          const p = track('p', () => plugin('clap:m2.probe', { in: osc.sine(440), key: 'lead' }));\n\
          export default song(() => p.out);\n",
    );
    put(&song, "data/p/lead.state.json", br#"{"plugin": "clap:m2.probe", "plugin_version": "1.0.0"}"#);
    for (i, cmd) in ["render", "bounce"].into_iter().enumerate() {
        let once = temp.0.join(format!("crashed-{i}"));
        put(&song, "data/p/lead.state", &m2_plugins::probe::state(&format!("crash-at-40:{}", once.display()), 1.0));
        let out = temp.0.join("out.wav");
        let args: Vec<&std::ffi::OsStr> = match cmd {
            "render" => vec!["render".as_ref(), song.as_os_str(), out.as_os_str(), "--seconds".as_ref(), "2".as_ref()],
            _ => vec!["bounce".as_ref(), song.as_os_str(), "out".as_ref(), "--seconds".as_ref(), "2".as_ref()],
        };
        let o = m2(&args);
        let err = String::from_utf8_lossy(&o.stderr);
        assert!(!o.status.success() && once.exists(), "m2 {cmd}: {err}");
        assert!(
            err.contains("error: The plugin host crashed while m2 probe (p) played, so the file wasn't made. Try again. (plugin.crashed)"),
            "m2 {cmd}: {err}"
        );
        assert!(!out.exists() && !song.join("assets/out.wav").exists(), "m2 {cmd} wrote no file");
    }
}
