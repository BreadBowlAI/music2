//! `m2 pack` (PACK-6, PACK-8, PACK-12): check, build, install, list, remove, lock and new, each
//! in a scratch HOME (PLAT-7), so the store is the test's own.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let (f, t) = (e.path(), to.join(e.file_name()));
        if f.is_dir() { copy(&f, &t) } else { std::fs::copy(&f, &t).map(drop).unwrap() }
    }
}

/// A scratch folder: the test's HOME and its files. Removed when dropped, read-only files too.
struct Temp(PathBuf);

impl Temp {
    fn new(what: &str) -> Temp {
        let d = std::env::temp_dir().join(format!("m2-pack-cli-{what}-{}", std::process::id()));
        let t = Temp(d);
        t.clean();
        std::fs::create_dir_all(&t.0).unwrap();
        t
    }

    fn clean(&self) {
        let mut stack = vec![self.0.clone()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                if e.file_type().is_ok_and(|t| t.is_dir()) {
                    stack.push(e.path());
                } else if let Ok(m) = e.metadata() {
                    let mut p = m.permissions();
                    #[allow(clippy::permissions_set_readonly_false)]
                    p.set_readonly(false);
                    let _ = std::fs::set_permissions(e.path(), p);
                }
            }
        }
        let _ = std::fs::remove_dir_all(&self.0);
    }

    fn m2(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_m2"))
            .env("HOME", self.0.join("home"))
            .env("M2_AUDIO", "none")
            .env("M2_MIDI", "none")
            .args(args)
            .output()
            .unwrap()
    }

    fn store(&self) -> PathBuf {
        self.0.join("home/Music/music2/Packs")
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        self.clean();
    }
}

fn said(out: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../core/session/tests/fixtures/packs")
}

// PACK-6, PACK-12: a good pack checks, builds its listing and installs read-only; installing the
// same again does nothing, and the same version with other files is refused (P5). list shows it,
// and remove takes it out, saying what a song that locks it will say.
#[test]
fn installing_twice_is_a_no_op_and_a_new_hash_is_refused() {
    let t = Temp::new("install");
    let tiny = t.0.join("tiny");
    copy(&fixtures().join("tiny"), &tiny);
    let dir = tiny.to_str().unwrap();
    let out = t.m2(&["pack", "check", dir]);
    assert!(out.status.success(), "{}", said(&out));
    let out = t.m2(&["pack", "build", dir]);
    assert!(out.status.success() && said(&out).contains("sha256:"), "{}", said(&out));
    assert!(tiny.join("m2pack.json").is_file());

    let out = t.m2(&["pack", "install", dir]);
    assert!(out.status.success(), "{}", said(&out));
    let installed = t.store().join("@test/tiny/0.1.0");
    assert!(installed.join("m2pack.json").is_file() && installed.join("samples/pluck.wav").is_file());
    assert!(std::fs::metadata(installed.join("src/index.ts")).unwrap().permissions().readonly(), "installed files are read-only");
    let out = t.m2(&["pack", "install", dir]);
    assert!(out.status.success() && said(&out).contains("already installed"), "{}", said(&out));

    std::fs::write(tiny.join("README.md"), "# changed\n").unwrap();
    let out = t.m2(&["pack", "install", dir]);
    assert_eq!(out.status.code(), Some(1), "{}", said(&out));
    assert!(said(&out).contains("never changed"), "{}", said(&out));

    let out = t.m2(&["pack", "list"]);
    assert!(said(&out).contains("@test/tiny") && said(&out).contains("0.1.0"), "{}", said(&out));
    let out = t.m2(&["pack", "remove", "@test/tiny", "0.1.0"]);
    assert!(out.status.success() && said(&out).contains("pkg.missing"), "{}", said(&out));
    assert!(!installed.exists() && !t.store().join("@test").exists(), "its empty folders go too");
}

// PACK-12: check names each problem in words and exits 1.
#[test]
fn check_says_what_is_wrong() {
    let t = Temp::new("check");
    let tiny = t.0.join("tiny");
    copy(&fixtures().join("tiny"), &tiny);
    std::fs::write(tiny.join("tool.sh"), "echo hi\n").unwrap();
    let index = std::fs::read_to_string(tiny.join("src/index.ts")).unwrap();
    std::fs::write(
        tiny.join("src/index.ts"),
        index.replace("samples/pluck.wav", "samples/gone.wav").replace("category: \"Distortion\", ", ""),
    )
    .unwrap();
    std::fs::write(
        tiny.join("presets/tiny-keys/Odd.json"),
        std::fs::read_to_string(tiny.join("presets/tiny-keys/Dark.json")).unwrap().replace("Dark", "Odd").replace("brightness", "sparkle"),
    )
    .unwrap();
    let toml = std::fs::read_to_string(tiny.join("m2pkg.toml")).unwrap();
    std::fs::write(tiny.join("m2pkg.toml"), toml.replace("license = \"CC0-1.0\"\n", "")).unwrap();
    let out = t.m2(&["pack", "check", tiny.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1), "{}", said(&out));
    let text = said(&out);
    for want in ["tool.sh", "licence", "sparkle", "samples/gone.wav"] {
        assert!(text.contains(want), "says {want}: {text}");
    }
    // An effect without a category isn't one the lists can offer, nor an instrument.
    assert!(text.contains("tiny-crunch"), "{text}");
}

// PACK-12: a new pack checks as made, and installs.
#[test]
fn a_new_pack_checks() {
    let t = Temp::new("new");
    let folder = t.0.join("felt");
    let out = t.m2(&["pack", "new", folder.to_str().unwrap(), "--name", "@alice/felt-piano"]);
    assert!(out.status.success(), "{}", said(&out));
    let out = t.m2(&["pack", "check", folder.to_str().unwrap()]);
    assert!(out.status.success(), "{}", said(&out));
    let out = t.m2(&["pack", "new", t.0.join("x").to_str().unwrap(), "--name", "felt"]);
    assert_eq!(out.status.code(), Some(1), "a name that isn't @owner/name: {}", said(&out));
}

// PACK-8: `m2 pack lock` writes the lock from what resolves now, and the song then builds with it.
#[test]
fn lock_writes_what_resolves_now() {
    let t = Temp::new("lock");
    copy(&fixtures(), &t.0.join("packs"));
    let out = t.m2(&["pack", "install", t.0.join("packs/tiny").to_str().unwrap()]);
    assert!(out.status.success(), "{}", said(&out));
    let song = t.0.join("packs/song-store");
    std::fs::remove_file(song.join("music2.lock")).unwrap();
    let out = t.m2(&["pack", "lock", song.to_str().unwrap()]);
    assert!(out.status.success(), "{}", said(&out));
    let lock = std::fs::read_to_string(song.join("music2.lock")).unwrap();
    assert!(lock.contains("name = \"@test/tiny\"") && lock.contains("version = \"0.1.0\""), "{lock}");
    let out = t.m2(&["check", song.to_str().unwrap()]);
    assert!(out.status.success(), "{}", said(&out));
}
