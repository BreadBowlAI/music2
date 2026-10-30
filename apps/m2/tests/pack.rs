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

/// A copy of the pack `tiny` at `version` with a kernel of its own, in a folder beside it:
/// `@test/tiny:sine`, std's module under its own name (`tag` as a custom section, so each copy's
/// module has its own hash), played by the instrument `tinyTone`.
fn tiny_with_kernel(t: &Temp, version: &str, tag: &str) -> PathBuf {
    let dir = tiny_at(t, version);
    let toml = std::fs::read_to_string(dir.join("m2pkg.toml")).unwrap();
    let toml = toml.replace(&format!("version = \"{version}\""), &format!("version = \"{version}\"\nwasm = \"k.wasm\""))
        + "\n[[kernel]]\nname = \"sine\"\ndescription = \"A sine\"\ninputs = [\"freq: control<hz> = 440\"]\noutputs = [\"out: audio<1>\"]\n";
    std::fs::write(dir.join("m2pkg.toml"), toml).unwrap();
    let mut wasm = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std/kernels/std.wasm")).unwrap();
    let name = b"m2-test";
    wasm.extend([0u8, (1 + name.len() + tag.len()) as u8, name.len() as u8]);
    wasm.extend(name);
    wasm.extend(tag.as_bytes());
    std::fs::write(dir.join("k.wasm"), &wasm).unwrap();
    let index = std::fs::read_to_string(dir.join("src/index.ts")).unwrap();
    let tone = "\nimport { hz, kernel } from \"@m2/core\";\n\n/** A sine of the pack's own kernel. */\nexport const tinyTone = component(\n  {\n    \
                id: \"tiny-tone\",\n    label: \"Tiny tone\",\n    signature: signature<(notes: Events) => Audio>(),\n    \
                inputs: { notes: port.notes() },\n    output: port.audio(),\n    editor: { category: \"Keys\", description: \"A sine\" },\n  },\n  \
                (_notes: Events): Audio => kernel(\"@test/tiny:sine\", { freq: hz(440) }),\n);\n";
    std::fs::write(dir.join("src/index.ts"), index + tone).unwrap();
    dir
}

/// A Tx that writes `files` (path, text) in the song at `song`, each based on what it holds now.
fn tx_writing(song: &Path, files: &[(&str, String)]) -> serde_json::Value {
    let mut base = serde_json::Map::new();
    let mut ops = vec![];
    let mut ranges = serde_json::Map::new();
    for (path, text) in files {
        let now = std::fs::read(song.join(path)).unwrap();
        base.insert(path.to_string(), serde_json::json!(m2_song::Sha::of(&now).prefixed()));
        ops.push(serde_json::json!({"op": "write", "file": path, "text": text}));
        ranges.insert(path.to_string(), serde_json::json!("whole"));
    }
    serde_json::json!({"id": "tx-move", "base": base, "label": ["before", "after"], "ops": ops, "expect": {"confined": {"ranges": ranges}}})
}

// PACK-9, AGENT-7: `m2 listen --with` renders each side with its own packs: the song as it is with
// 0.1.0 (no kernel of its own), the change with 0.2.0 and the kernel it plays.
#[test]
fn listen_with_renders_each_side_with_its_own_packs() {
    let t = Temp::new("listen-with");
    copy(&fixtures(), &t.0.join("packs"));
    let song = t.0.join("packs/song-store");
    ok(t.m2(&["pack", "install", t.0.join("packs/tiny").to_str().unwrap()]));
    ok(t.m2(&["pack", "install", tiny_with_kernel(&t, "0.2.0", "two").to_str().unwrap()]));
    ok(t.m2(&["check", song.to_str().unwrap()]));
    assert_eq!(locked(&song), "0.1.0");
    // The move: the line, the lock (as `m2 pack update` would write it), and the keys playing the
    // new version's kernel.
    let manifest = std::fs::read_to_string(song.join("music2.toml")).unwrap().replace("\"@test/tiny\" = \"0.1\"", "\"@test/tiny\" = \"0.2\"");
    let hash = m2_song::Sha::of(&std::fs::read(t.store().join("@test/tiny/0.2.0/m2pack.json")).unwrap());
    let lock = std::fs::read_to_string(song.join("music2.lock")).unwrap();
    let at = lock.find("sha256:").unwrap();
    let lock = format!("{}{}{}", &lock[..at], hash.prefixed(), &lock[at + 71..]).replace("version = \"0.1.0\"", "version = \"0.2.0\"");
    let keys = "import { track } from \"@m2/core\";\nimport { tinyTone } from \"@test/tiny\";\n\nexport default track(\"keys\", { name: \"Keys\" }, ({ lane }) => tinyTone(lane.notes(\"notes\")));\n";
    let tx = tx_writing(&song, &[("music2.toml", manifest), ("music2.lock", lock), ("tracks/keys.ts", keys.to_string())]);
    let file = t.0.join("move.json");
    std::fs::write(&file, tx.to_string()).unwrap();
    let out = ok(t.m2(&["listen", song.to_str().unwrap(), "--with", file.to_str().unwrap(), "--json"]));
    let heard: serde_json::Value = serde_json::from_str(&out).unwrap_or_else(|e| panic!("{e}: {out}"));
    let own = |side: &str| heard[side]["tracks"]["keys"]["own"].clone();
    // Only 0.2.0 has the kernel, so "after" played it: a steady sine, 3 dB from peak to RMS.
    let after = own("after");
    assert!(after["silent"] == false && after["crest"].as_f64().is_some_and(|c| (c - 3.0).abs() < 0.2), "{heard:#}");
    assert!(own("before")["silent"] == false, "before plays the pack's keys: {heard:#}");
    assert!(heard["difference"].is_object(), "{heard:#}");
    // Nothing was written: the song still uses 0.1.0.
    assert_eq!(locked(&song), "0.1.0");
}

// PACK-8, PACK-9, PACK-2: a version in the store this app can't run (a newer app installed it,
// needing a newer std) is never chosen: not by `m2 pack update`, nor by a song with no lock entry;
// with only it installed, the song says why.
#[test]
fn a_version_this_app_cant_run_is_never_chosen() {
    let t = Temp::new("runs");
    copy(&fixtures(), &t.0.join("packs"));
    let song = t.0.join("packs/song-store");
    let s = song.to_str().unwrap();
    ok(t.m2(&["pack", "install", t.0.join("packs/tiny").to_str().unwrap()]));
    // 0.1.1 needs std 0.9: install refuses it here, so it is put in the store as a newer app would.
    let later = tiny_at(&t, "0.1.1");
    let toml = std::fs::read_to_string(later.join("m2pkg.toml")).unwrap();
    assert!(toml.contains("std = \"0.4\""), "{toml}");
    std::fs::write(later.join("m2pkg.toml"), toml.replace("std = \"0.4\"", "std = \"0.9\"")).unwrap();
    ok(t.m2(&["pack", "build", later.to_str().unwrap()]));
    copy(&later, &t.store().join("@test/tiny/0.1.1"));

    ok(t.m2(&["pack", "update", s]));
    assert_eq!(locked(&song), "0.1.0", "update stays on the newest version this app runs");
    std::fs::remove_file(song.join("music2.lock")).unwrap();
    ok(t.m2(&["check", s]));
    ok(t.m2(&["pack", "lock", s]));
    assert_eq!(locked(&song), "0.1.0");

    ok(t.m2(&["pack", "remove", "@test/tiny", "0.1.0"]));
    std::fs::remove_file(song.join("music2.lock")).unwrap();
    let text = failed(t.m2(&["check", s]));
    for want in ["pkg.unsatisfied", "0.1.1", "std \"0.9\""] {
        assert!(text.contains(want), "says {want}: {text}");
    }
}
