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
            writable(&d);
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                match e.file_type() {
                    Ok(t) if t.is_dir() => stack.push(e.path()),
                    Ok(t) if t.is_file() => writable(&e.path()),
                    _ => {}
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

/// Make `path` writable, as an installed (read-only) file or folder isn't.
fn writable(path: &Path) {
    if let Ok(m) = std::fs::symlink_metadata(path)
        && !m.file_type().is_symlink()
    {
        let mut p = m.permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        p.set_readonly(false);
        let _ = std::fs::set_permissions(path, p);
    }
}

/// Whether `out` succeeded, saying what it said when it didn't.
fn ok(out: Output) -> String {
    assert!(out.status.success(), "{}", said(&out));
    said(&out)
}

/// Whether `out` failed with exit 1, and what it said.
fn failed(out: Output) -> String {
    assert_eq!(out.status.code(), Some(1), "{}", said(&out));
    said(&out)
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

// PACK-6: an installed version's files and folders are read-only; one changed all the same is
// caught (pkg.changed, naming the file), installing the same version again puts it back, and
// remove copes with what is read-only.
#[test]
fn a_changed_installed_pack_is_caught_and_reinstalling_repairs_it() {
    let t = Temp::new("repair");
    copy(&fixtures(), &t.0.join("packs"));
    let (tiny, song) = (t.0.join("packs/tiny"), t.0.join("packs/song-store"));
    let (tiny, song) = (tiny.to_str().unwrap(), song.to_str().unwrap());
    ok(t.m2(&["pack", "install", tiny]));
    let installed = t.store().join("@test/tiny/0.1.0");
    for d in [installed.clone(), installed.join("src"), installed.join("samples")] {
        assert!(std::fs::metadata(&d).unwrap().permissions().readonly(), "{} is read-only", d.display());
    }
    ok(t.m2(&["check", song]));
    // Changed all the same, as a sync or an editor saving in place might.
    let index = installed.join("src/index.ts");
    writable(&installed.join("src"));
    writable(&index);
    std::fs::write(&index, std::fs::read_to_string(&index).unwrap() + "// changed\n").unwrap();
    let text = failed(t.m2(&["check", song]));
    for want in ["pkg.changed", "src/index.ts", "m2 pack install"] {
        assert!(text.contains(want), "says {want}: {text}");
    }
    // Installing the same version again puts it back, read-only again.
    let text = ok(t.m2(&["pack", "install", tiny]));
    assert!(text.contains("put back"), "{text}");
    assert!(std::fs::metadata(installed.join("src")).unwrap().permissions().readonly());
    ok(t.m2(&["check", song]));
    ok(t.m2(&["pack", "remove", "@test/tiny", "0.1.0"]));
    assert!(!installed.exists());
}

/// A copy of the pack `tiny` at `version`, in a folder of its own beside it.
fn tiny_at(t: &Temp, version: &str) -> PathBuf {
    let dir = t.0.join(format!("tiny-{version}"));
    copy(&t.0.join("packs/tiny"), &dir);
    let toml = std::fs::read_to_string(dir.join("m2pkg.toml")).unwrap();
    std::fs::write(dir.join("m2pkg.toml"), toml.replace("version = \"0.1.0\"", &format!("version = \"{version}\""))).unwrap();
    let _ = std::fs::remove_file(dir.join("m2pack.json"));
    dir
}

/// The version `music2.lock` keeps of `@test/tiny`.
fn locked(song: &Path) -> String {
    let lock = std::fs::read_to_string(song.join("music2.lock")).unwrap();
    lock.lines().find_map(|l| l.strip_prefix("version = \"")).map(|v| v.trim_end_matches('"').to_string()).unwrap_or_default()
}

// PACK-8, and the guide's recipe end to end: a song moves to another version when its line
// changes and `m2 pack lock` runs; `m2 pack update` moves it to the newest it admits; and every
// command a problem names works from where it is said.
#[test]
fn moving_a_song_to_another_version() {
    let t = Temp::new("move");
    copy(&fixtures(), &t.0.join("packs"));
    let song = t.0.join("packs/song-store");
    let s = song.to_str().unwrap();
    ok(t.m2(&["pack", "install", t.0.join("packs/tiny").to_str().unwrap()]));
    ok(t.m2(&["pack", "install", tiny_at(&t, "0.2.0").to_str().unwrap()]));
    ok(t.m2(&["check", s]));
    assert_eq!(locked(&song), "0.1.0", "a newer version installed moves nothing");

    // The guide's recipe: change the line, then lock.
    let manifest = std::fs::read_to_string(song.join("music2.toml")).unwrap();
    std::fs::write(song.join("music2.toml"), manifest.replace("\"@test/tiny\" = \"0.1\"", "\"@test/tiny\" = \"0.2\"")).unwrap();
    let text = failed(t.m2(&["check", s]));
    assert!(text.contains("pkg.unsatisfied") && text.contains("m2 pack lock"), "{text}");
    ok(t.m2(&["pack", "lock", s]));
    assert_eq!(locked(&song), "0.2.0");
    ok(t.m2(&["check", s]));

    // A newer version it admits: lock keeps what it has, update moves to it.
    ok(t.m2(&["pack", "install", tiny_at(&t, "0.2.1").to_str().unwrap()]));
    assert!(ok(t.m2(&["pack", "lock", s])).contains("says that already"));
    assert_eq!(locked(&song), "0.2.0");
    ok(t.m2(&["pack", "update", s]));
    assert_eq!(locked(&song), "0.2.1");
    ok(t.m2(&["check", s]));

    // The installed copy isn't the one the lock names: what check says to do works.
    let lock = std::fs::read_to_string(song.join("music2.lock")).unwrap();
    let at = lock.find("sha256:").unwrap() + "sha256:".len();
    std::fs::write(song.join("music2.lock"), format!("{}{}{}", &lock[..at], "0".repeat(64), &lock[at + 64..])).unwrap();
    let text = failed(t.m2(&["check", s]));
    assert!(text.contains("pkg.changed") && text.contains("m2 pack update"), "{text}");
    let text = failed(t.m2(&["pack", "lock", s]));
    assert!(text.contains("m2 pack install") && text.contains("m2 pack update"), "lock keeps the version, so it says how: {text}");
    ok(t.m2(&["pack", "update", s, "@test/tiny"]));
    ok(t.m2(&["check", s]));

    // A locked version that isn't installed, while another it admits is: lock moves to that one.
    let lock = std::fs::read_to_string(song.join("music2.lock")).unwrap();
    std::fs::write(song.join("music2.lock"), lock.replace("version = \"0.2.1\"", "version = \"0.2.9\"")).unwrap();
    let text = failed(t.m2(&["check", s]));
    assert!(text.contains("pkg.missing") && text.contains("m2 pack lock"), "{text}");
    ok(t.m2(&["pack", "lock", s]));
    assert_eq!(locked(&song), "0.2.1");
    ok(t.m2(&["check", s]));
}

// RENDER-2, PACK-5: a bounce's provenance names every package file its kernels read (a pack's
// recording), with its hash, and the version hash of each pack.
#[test]
fn a_bounce_names_the_pack_files_it_played() {
    let t = Temp::new("bounce");
    copy(&fixtures(), &t.0.join("packs"));
    let song = t.0.join("packs/song-store");
    ok(t.m2(&["pack", "install", t.0.join("packs/tiny").to_str().unwrap()]));
    ok(t.m2(&["bounce", song.to_str().unwrap(), "pl", "--track", "pluck", "--seconds", "1"]));
    let record: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(song.join("assets/pl.wav.json")).unwrap()).unwrap();
    let tiny = &record["packages"]["@test/tiny"];
    let wav = m2_song::Sha::of(&std::fs::read(t.0.join("packs/tiny/samples/pluck.wav")).unwrap());
    assert_eq!(tiny["files"]["@test/tiny/samples/pluck.wav"], wav.hex(), "{record:#}");
    let lock = std::fs::read_to_string(song.join("music2.lock")).unwrap();
    let hash = tiny["hash"].as_str().unwrap_or("none");
    assert!(lock.contains(hash) && hash.starts_with("sha256:"), "{record:#}");
}

// PACK-6, PACK-12: a pack holds no link: check names one, build and install refuse it, and nothing
// outside the pack is ever copied into the store.
#[cfg(unix)]
#[test]
fn links_are_refused_in_a_pack() {
    let t = Temp::new("links");
    let tiny = t.0.join("tiny");
    copy(&fixtures().join("tiny"), &tiny);
    std::fs::write(t.0.join("outside-secret.txt"), "SECRET-TOKEN-123\n").unwrap();
    std::os::unix::fs::symlink(t.0.join("outside-secret.txt"), tiny.join("samples/notes.wav")).unwrap();
    let dir = tiny.to_str().unwrap();
    for args in [["pack", "check", dir], ["pack", "build", dir], ["pack", "install", dir]] {
        let text = failed(t.m2(&args));
        assert!(text.contains("samples/notes.wav") && text.contains("link"), "{args:?}: {text}");
    }
    assert!(!t.store().join("@test").exists(), "nothing was installed");
    // A folder that is a link, too.
    std::fs::remove_file(tiny.join("samples/notes.wav")).unwrap();
    std::os::unix::fs::symlink(&t.0, tiny.join("more")).unwrap();
    let text = failed(t.m2(&["pack", "check", dir]));
    assert!(text.contains("more") && text.contains("link"), "{text}");
}

// PACK-12: a file's type is its extension in any capitals: `.WAV` is a WAV.
#[test]
fn a_capital_extension_is_its_type() {
    let t = Temp::new("capitals");
    let tiny = t.0.join("tiny");
    copy(&fixtures().join("tiny"), &tiny);
    std::fs::copy(tiny.join("samples/pluck.wav"), tiny.join("samples/Extra.WAV")).unwrap();
    std::fs::write(tiny.join("NOTES.TXT"), "notes\n").unwrap();
    ok(t.m2(&["pack", "check", tiny.to_str().unwrap()]));
}

// PACK-12: a preset's value outside its control's strict range is named; one outside a range that
// is only a hint is not (PKG-8).
#[test]
fn a_preset_outside_a_strict_range_is_named() {
    let t = Temp::new("strict");
    let tiny = t.0.join("tiny");
    copy(&fixtures().join("tiny"), &tiny);
    let dark = std::fs::read_to_string(tiny.join("presets/tiny-keys/Dark.json")).unwrap();
    let low = dark.replace("\"Dark\"", "\"Low\"").replace("600", "50");
    assert_ne!(low, dark.replace("\"Dark\"", "\"Low\""), "the fixture's Dark sets brightness 600");
    std::fs::write(tiny.join("presets/tiny-keys/Low.json"), low).unwrap();
    ok(t.m2(&["pack", "check", tiny.to_str().unwrap()]));
    let index = std::fs::read_to_string(tiny.join("src/index.ts")).unwrap();
    std::fs::write(tiny.join("src/index.ts"), index.replace(".range(200, 20000) }", ".range(200, 20000).strict() }")).unwrap();
    let text = failed(t.m2(&["pack", "check", tiny.to_str().unwrap()]));
    assert!(text.contains("presets/tiny-keys/Low.json") && text.contains("brightness") && text.contains("range"), "{text}");
}

fn sfz_fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../core/session/tests/fixtures/sfz/tiny-keys")
}

/// A song in `dir` that plays `@test/tiny-keys`'s instrument from the folder `pack`: soft and hard
/// notes, low and high, each released.
fn tiny_keys_song(dir: &Path, pack: &Path) {
    std::fs::create_dir_all(dir.join("data/keys")).unwrap();
    let manifest = format!(
        "[project]\nname = \"Tiny keys\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.5\"\n\
         \"@test/tiny-keys\" = {{ path = {:?} }}\n",
        pack.to_str().unwrap()
    );
    std::fs::write(dir.join("music2.toml"), manifest).unwrap();
    std::fs::write(
        dir.join("song.ts"),
        "import { song, track } from \"@m2/core\";\nimport { master, mixer } from \"std/mix\";\nimport { tinyKeys } from \"@test/tiny-keys/instruments\";\n\n\
         const keys = track(\"keys\", ({ lane }) => tinyKeys(lane.notes(\"notes\")));\n\nexport default song(() => master(mixer({ keys: keys.out })));\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("data/keys/notes.json"),
        "{\"clips\": [\n  {\"id\": \"p7n3c2wa\", \"start\": 0, \"length\": 4, \"notes\": [\n\
         {\"pitch\": \"C4\", \"start\": 0, \"length\": 0.5, \"velocity\": 0.3},\n\
         {\"pitch\": \"C4\", \"start\": 1, \"length\": 0.5, \"velocity\": 0.9},\n\
         {\"pitch\": \"G5\", \"start\": 2, \"length\": 0.25, \"velocity\": 0.4},\n\
         {\"pitch\": \"E4\", \"start\": 3, \"length\": 0.5, \"velocity\": 1.0}\n  ]}\n]}\n",
    )
    .unwrap();
}

// PACK-14, PACK-13: an SFZ imports into a pack that checks (saying what it couldn't keep), a song
// plays its instrument from the pack's folder, and renders the same bits on 1 and 4 threads.
#[test]
fn an_sfz_imports_checks_and_renders_alike_on_1_and_4_threads() {
    let t = Temp::new("sfz");
    copy(&sfz_fixture(), &t.0.join("in"));
    let pack = t.0.join("pack");
    let sfz = t.0.join("in/Tiny Keys.sfz");
    let text =
        ok(t.m2(&["pack", "import-sfz", sfz.to_str().unwrap(), pack.to_str().unwrap(), "--name", "@test/tiny-keys", "--category", "Keys"]));
    for want in [
        "tiny-keys plays 5 regions from 5 recordings",
        "left out: 1 region belongs to the key switch c#0, not the default",
        "left out: 1 region plays only with a controller moved (CC 64)",
        "changed: 1 region has its own amp_veltrack (90%): the instrument's velocity is 80% for all",
        "key switches: kept the default one's regions (c0)",
        "good to install",
    ] {
        assert!(text.contains(want), "says {want}: {text}");
    }
    let toml = std::fs::read_to_string(pack.join("instruments/tiny-keys.toml")).unwrap();
    assert!(toml.contains("file = \"samples/g4-soft.wav\"\nroot = 67\nkeys = [72, 127]\nvelocity = [1, 64]\npan = -0.3\n"), "{toml}");
    assert!(toml.contains("trigger = \"release\"\nvolume = -12\n"), "{toml}");
    let m = std::fs::read_to_string(pack.join("m2pkg.toml")).unwrap();
    assert!(m.contains("license = \"CC0-1.0\""), "{m}");
    assert!(std::fs::read_to_string(pack.join("README.md")).unwrap().contains("Tiny Keys: a test instrument for music2"));

    let song = t.0.join("song");
    tiny_keys_song(&song, &pack);
    let (one, four) = (t.0.join("one.wav"), t.0.join("four.wav"));
    ok(t.m2(&["render", song.to_str().unwrap(), one.to_str().unwrap(), "--seconds", "4", "--threads", "1"]));
    ok(t.m2(&["render", song.to_str().unwrap(), four.to_str().unwrap(), "--seconds", "4", "--threads", "4"]));
    let (a, b) = (std::fs::read(&one).unwrap(), std::fs::read(&four).unwrap());
    assert!(a == b, "the same bits on 1 and 4 threads");
    let samples: Vec<i32> = hound::WavReader::open(&one).unwrap().into_samples::<i32>().map(Result::unwrap).collect();
    let peak =
        |from: f64, to: f64| samples[(from * 96_000.0) as usize..(to * 96_000.0) as usize].iter().map(|s| s.abs()).max().unwrap_or(0);
    // 120 bpm: a beat is half a second. The soft C4, then the loud one, louder.
    assert!(peak(0.0, 0.25) > 0 && peak(0.5, 0.75) > peak(0.0, 0.25), "soft then loud: {} {}", peak(0.0, 0.25), peak(0.5, 0.75));
    assert!(peak(0.25, 0.3) > 0, "the release sounds as the soft note ends");
}

// PACK-14: a FLAC sample stops the import before anything is written.
#[test]
fn a_flac_sample_stops_the_import() {
    let t = Temp::new("sfz-flac");
    copy(&sfz_fixture(), &t.0.join("in"));
    let sfz = t.0.join("in/Tiny Keys.sfz");
    std::fs::write(&sfz, std::fs::read_to_string(&sfz).unwrap().replace("c4-loud.wav", "c4-loud.flac")).unwrap();
    let pack = t.0.join("pack");
    let text = failed(t.m2(&["pack", "import-sfz", sfz.to_str().unwrap(), pack.to_str().unwrap(), "--name", "@test/tiny-keys"]));
    assert!(text.contains("c4-loud.flac is a FLAC file: convert it to WAV first; music2 reads only WAV for now"), "{text}");
    assert!(!pack.exists());
}

// PACK-13: code that would hide the instruments written as data is refused by check.
#[test]
fn check_refuses_code_that_hides_data_instruments() {
    let t = Temp::new("sfz-hide");
    copy(&sfz_fixture(), &t.0.join("in"));
    let pack = t.0.join("pack");
    let sfz = t.0.join("in/Tiny Keys.sfz");
    ok(t.m2(&["pack", "import-sfz", sfz.to_str().unwrap(), pack.to_str().unwrap(), "--name", "@test/tiny-keys"]));
    std::fs::create_dir_all(pack.join("src")).unwrap();
    std::fs::write(pack.join("src/instruments.ts"), "export const x = 1;\n").unwrap();
    let text = failed(t.m2(&["pack", "check", pack.to_str().unwrap()]));
    assert!(text.contains("src/instruments.ts and instruments/*.toml are both the module \"@test/tiny-keys/instruments\""), "{text}");
}
