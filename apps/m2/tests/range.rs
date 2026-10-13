//! `m2 render --preroll`: the range render agents listen with (RENDER-4, RENDER-5).

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
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn m2(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_m2")).args(args).output().unwrap()
}

fn stdout(o: &Output) -> String {
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn a_range_render_says_it_is_approximate_and_never_goes_into_assets() {
    let temp = Temp(std::env::temp_dir().join(format!("m2-range-cli-{}", std::process::id())));
    let song = temp.0.join("first-song");
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song"), &song);
    let (song, out) = (song.to_str().unwrap(), |name: &str| temp.0.join(name).to_str().unwrap().to_string());
    let read = |name: &str| std::fs::read(out(name)).unwrap();

    // From bar 3 (beat 8 at 124 bpm, 3.9 s in) with a second of preroll: approximate, and the same
    // bits every time.
    let range = ["--from-bar", "3", "--seconds", "2", "--preroll", "1"];
    let said = stdout(&m2(&[&["render", song, &out("a.wav")][..], &range].concat()));
    assert!(said.contains("approximate: it started 2.871 s in"), "{said}");
    assert!(!said.contains("not reproducible"), "{said}");
    stdout(&m2(&[&["render", song, &out("b.wav")][..], &range].concat()));
    assert!(read("a.wav") == read("b.wav"));

    // Without a preroll it replays: exact, and it says nothing more.
    let said = stdout(&m2(&["render", song, &out("exact.wav"), "--from-bar", "3", "--seconds", "2"]));
    assert!(!said.contains("approximate"), "{said}");
    assert_eq!(read("exact.wav").len(), read("a.wav").len());
    // A preroll back to the song's start is the exact render too.
    let said = stdout(&m2(&["render", song, &out("whole.wav"), "--from-bar", "3", "--seconds", "2", "--preroll", "4"]));
    assert!(!said.contains("approximate") && read("whole.wav") == read("exact.wav"), "{said}");

    // RENDER-5: never into the project's assets/.
    let into = format!("{song}/assets/x.wav");
    let refused = m2(&[&["render", song, &into][..], &range].concat());
    let why = String::from_utf8_lossy(&refused.stderr);
    assert!(!refused.status.success() && why.contains("assets/") && why.contains("RENDER-5"), "{why}");
    assert!(!Path::new(&into).exists());

    for bad in [&["--from", "1", "--from-bar", "2"][..], &["--from-bar", "0"], &["--preroll", "-1"]] {
        assert!(!m2(&[&["render", song, &out("bad.wav")][..], bad].concat()).status.success(), "{bad:?}");
    }
}
