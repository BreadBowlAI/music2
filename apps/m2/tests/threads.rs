//! `m2 render --threads` and `M2_THREADS` (8a): the same file on any number of threads, and the
//! output says how many it used.

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

/// `m2 render <song> <out> --seconds 4 <more>`, with `M2_THREADS` set to `env` when given.
fn render(song: &Path, out: &Path, more: &[&str], env: Option<&str>) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_m2"));
    c.arg("render").arg(song).arg(out).args(["--from", "1", "--seconds", "4"]).args(more).env("M2_AUDIO", "none").env_remove("M2_THREADS");
    if let Some(v) = env {
        c.env("M2_THREADS", v);
    }
    c.output().unwrap()
}

fn said(o: &Output) -> String {
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

// RENDER-2 (8a): `--threads 1` and `--threads 4` write the same file and say how many threads they
// used; `M2_THREADS` sets it, and `--threads` wins over it; nonsense is refused in plain words.
#[test]
fn render_takes_threads_and_says_how_many() {
    let temp = Temp(std::env::temp_dir().join(format!("m2-render-threads-{}", std::process::id())));
    let song = temp.0.join("first-song");
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song"), &song);
    let (a, b) = (temp.0.join("a.wav"), temp.0.join("b.wav"));
    assert!(said(&render(&song, &a, &["--threads", "1"], None)).contains(" on 1 thread, "));
    assert!(said(&render(&song, &b, &["--threads", "4"], None)).contains(" on 4 threads, "));
    assert!(std::fs::read(&a).unwrap() == std::fs::read(&b).unwrap(), "the same file on 1 and 4 threads");
    assert!(said(&render(&song, &temp.0.join("c.wav"), &[], Some("2"))).contains(" on 2 threads, "));
    assert!(said(&render(&song, &temp.0.join("d.wav"), &["--threads", "3"], Some("2"))).contains(" on 3 threads, "), "--threads wins");
    for (more, env) in [(&["--threads", "0"][..], None), (&[][..], Some("x")), (&["--threads", "65"][..], None)] {
        let o = render(&song, &temp.0.join("no.wav"), more, env);
        let e = String::from_utf8_lossy(&o.stderr);
        assert!(!o.status.success() && e.contains("a number from 1 to 64"), "{more:?} {env:?}: {e}");
    }
}
