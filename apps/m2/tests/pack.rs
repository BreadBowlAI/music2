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

    /// `m2` with `args`, run in the folder `dir`.
    fn m2_in(&self, dir: &Path, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_m2"))
            .current_dir(dir)
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

/// A 16-bit mono WAV of a tenth of a second of a tone at 48 kHz.
fn short_wav(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let spec = hound::WavSpec { channels: 1, sample_rate: 48_000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for i in 0..4_800 {
        w.write_sample(((i as f32 / 10.0).sin() * 8_000.0) as i16).unwrap();
    }
    w.finalize().unwrap();
}

// PACK-12, LIB-4: a pack of samples alone checks and installs, and its samples are found; check
// names a WAV that doesn't decode, a word not on the list and a path samples.toml names that isn't
// there.
#[test]
fn a_pack_of_samples_alone_checks_and_its_samples_are_checked() {
    let t = Temp::new("samples");
    let drums = t.0.join("drums");
    std::fs::create_dir_all(&drums).unwrap();
    std::fs::write(drums.join("m2pkg.toml"), "[package]\nname = \"@alice/drums\"\nversion = \"1.0.0\"\nlicense = \"CC0-1.0\"\n").unwrap();
    std::fs::write(drums.join("samples.toml"), "[folder.\"kicks\"]\ninstrument = \"kick\"\n").unwrap();
    short_wav(&drums.join("samples/kicks/Boom.wav"));
    // FMT-24: a FLAC sample checks too, and is found.
    let flac = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../core/runtime/tests/fixtures/flac/16bit-mono.flac");
    std::fs::copy(flac, drums.join("samples/kicks/Thud.flac")).unwrap();
    let out = t.m2(&["pack", "check", drums.to_str().unwrap()]);
    assert!(out.status.success(), "{}", said(&out));
    let out = t.m2(&["pack", "install", drums.to_str().unwrap()]);
    assert!(out.status.success(), "{}", said(&out));
    let out = t.m2(&["search", "boom", "--kind", "sample", "--json"]);
    let items: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("{}", said(&out)));
    assert_eq!(items[0]["id"], "sample:@alice/drums:samples/kicks/Boom.wav", "{items}");
    assert_eq!((&items[0]["package"], &items[0]["sample"]["instrument"]), (&serde_json::json!("@alice/drums"), &serde_json::json!("kick")));
    let out = t.m2(&["search", "thud", "--kind", "sample", "--json"]);
    let items: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("{}", said(&out)));
    assert_eq!(items[0]["id"], "sample:@alice/drums:samples/kicks/Thud.flac", "{items}");

    std::fs::write(drums.join("samples/kicks/broken.wav"), b"RIFF0000WAVEnope").unwrap();
    std::fs::write(drums.join("samples.toml"), "[folder.\"kicks\"]\ninstrument = \"cowbell\"\n\n[file.\"snares/gone.wav\"]\n").unwrap();
    let out = t.m2(&["pack", "check", drums.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1), "{}", said(&out));
    let text = said(&out);
    for want in [
        "samples/kicks/broken.wav doesn't read as a WAV file",
        "instrument cowbell isn't one of",
        "no WAV or FLAC file samples/snares/gone.wav",
    ] {
        assert!(text.contains(want), "says {want}: {text}");
    }
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
    let manifest =
        std::fs::read_to_string(song.join("music2.toml")).unwrap().replace("\"@test/tiny\" = \"0.1\"", "\"@test/tiny\" = \"0.2\"");
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

// LIB-1, LIB-2, PACK-11: with the app closed, m2 search finds an installed pack's instruments,
// effects and sounds, each with its pack; its recordings (no samples.toml) are no samples.
#[test]
fn an_installed_packs_items_are_found_with_the_app_closed() {
    let t = Temp::new("search");
    let tiny = t.0.join("tiny");
    copy(&fixtures().join("tiny"), &tiny);
    let out = t.m2(&["pack", "install", tiny.to_str().unwrap()]);
    assert!(out.status.success(), "{}", said(&out));
    let out = t.m2(&["search", "tiny", "--json"]);
    let items: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("{}", said(&out)));
    let found: Vec<(&str, &str, &str)> = items
        .as_array()
        .unwrap()
        .iter()
        .map(|i| (i["id"].as_str().unwrap(), i["kind"].as_str().unwrap(), i["package"].as_str().unwrap()))
        .collect();
    for want in [
        ("@test/tiny:tiny-keys", "instrument", "@test/tiny"),
        ("@test/tiny:tiny-crunch", "effect", "@test/tiny"),
        ("sound:std/@test/tiny:tiny-keys/Dark", "sound", "@test/tiny"),
    ] {
        assert!(found.contains(&want), "{want:?} in {found:?}");
    }
    assert!(!found.iter().any(|(_, kind, _)| *kind == "sample"), "its recording is no sample: {found:?}");
}

// FMT-16, PACK-6: m2 keeps the hash cache on disk as the app does, so the next command finds an
// installed pack's unchanged files there (a stat each), not by reading them all again.
#[test]
fn m2_keeps_the_hashes_of_a_packs_files() {
    let t = Temp::new("hashes");
    copy(&fixtures(), &t.0.join("packs"));
    ok(t.m2(&["pack", "install", t.0.join("packs/tiny").to_str().unwrap()]));
    ok(t.m2(&["check", t.0.join("packs/song-store").to_str().unwrap()]));
    let mut cached = vec![];
    let mut stack = vec![t.0.join("home")];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            match e.file_type().unwrap().is_dir() {
                true => stack.push(e.path()),
                false if e.file_name() == "hashes.json" => cached.push(std::fs::read_to_string(e.path()).unwrap()),
                false => {}
            }
        }
    }
    let index = t.store().join("@test/tiny/0.1.0/src/index.ts");
    let index = serde_json::to_string(index.to_str().unwrap()).unwrap();
    assert!(cached.iter().any(|c| c.contains(&index)), "{index} in {cached:?}");
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

// PACK-14, FMT-24, RENDER-2: an SFZ whose recordings are FLAC imports into a pack that keeps them
// as they are, checks, and plays the very bits of the same pack made of the WAVs of them.
#[test]
fn a_flac_sample_is_copied_and_renders_as_its_wav() {
    let t = Temp::new("sfz-flac");
    let flacs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../core/runtime/tests/fixtures/flac/tiny-keys");
    let mut renders = vec![];
    for kind in ["wav", "flac"] {
        let input = t.0.join(format!("in-{kind}"));
        copy(&sfz_fixture(), &input);
        if kind == "flac" {
            for e in std::fs::read_dir(input.join("samples")).unwrap().flatten() {
                std::fs::remove_file(e.path()).unwrap();
            }
            for e in std::fs::read_dir(&flacs).unwrap().flatten() {
                std::fs::copy(e.path(), input.join("samples").join(e.file_name())).unwrap();
            }
            for sfz in ["Tiny Keys.sfz", "soft.sfz"] {
                let text = std::fs::read_to_string(input.join(sfz)).unwrap();
                std::fs::write(input.join(sfz), text.replace(".wav", ".flac")).unwrap();
            }
        }
        let pack = t.0.join(format!("pack-{kind}"));
        let sfz = input.join("Tiny Keys.sfz");
        let text = ok(t.m2(&["pack", "import-sfz", sfz.to_str().unwrap(), pack.to_str().unwrap(), "--name", "@test/tiny-keys"]));
        assert!(text.contains("tiny-keys plays 5 regions from 5 recordings") && text.contains("good to install"), "{kind}: {text}");
        if kind == "flac" {
            assert_eq!(std::fs::read(pack.join("samples/c4-loud.flac")).unwrap(), std::fs::read(flacs.join("c4-loud.flac")).unwrap());
            let toml = std::fs::read_to_string(pack.join("instruments/tiny-keys.toml")).unwrap();
            assert!(toml.contains("file = \"samples/g4-soft.flac\"") && !toml.contains(".wav"), "{toml}");
        }
        let song = t.0.join(format!("song-{kind}"));
        tiny_keys_song(&song, &pack);
        let out = t.0.join(format!("{kind}.wav"));
        ok(t.m2(&["render", song.to_str().unwrap(), out.to_str().unwrap(), "--seconds", "4"]));
        renders.push(std::fs::read(&out).unwrap());
    }
    assert!(renders[0] == renders[1], "the pack of FLACs plays the bits of the pack of WAVs");
}

// PACK-14, FMT-24: a recording of another kind than WAV or FLAC (told by its first bytes) stops the
// import before anything is written, naming it.
#[test]
fn an_ogg_sample_stops_the_import() {
    let t = Temp::new("sfz-ogg");
    copy(&sfz_fixture(), &t.0.join("in"));
    let sfz = t.0.join("in/Tiny Keys.sfz");
    std::fs::write(&sfz, std::fs::read_to_string(&sfz).unwrap().replace("c4-loud.wav", "c4-loud.ogg")).unwrap();
    std::fs::write(t.0.join("in/samples/c4-loud.ogg"), b"OggS\0\x02 a sound in another kind of file").unwrap();
    let pack = t.0.join("pack");
    let text = failed(t.m2(&["pack", "import-sfz", sfz.to_str().unwrap(), pack.to_str().unwrap(), "--name", "@test/tiny-keys"]));
    assert!(text.contains("c4-loud.ogg isn't a WAV or FLAC file: music2 reads those two; save it as a WAV first"), "{text}");
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

/// `@test/tiny-keys`, imported from the SFZ fixture into `<t>/pack`, which checks.
fn imported(t: &Temp) -> PathBuf {
    copy(&sfz_fixture(), &t.0.join("in"));
    let pack = t.0.join("pack");
    let sfz = t.0.join("in/Tiny Keys.sfz");
    ok(t.m2(&["pack", "import-sfz", sfz.to_str().unwrap(), pack.to_str().unwrap(), "--name", "@test/tiny-keys"]));
    pack
}

// PACK-13, PACK-12: check names the file and the line of a mistake in a data instrument: one the
// reader finds (a value out of its range) and one @m2/core finds in a region.
#[test]
fn check_names_the_file_and_line_of_a_data_instruments_mistake() {
    let t = Temp::new("sfz-lines");
    let pack = imported(&t);
    let file = pack.join("instruments/tiny-keys.toml");
    let toml = std::fs::read_to_string(&file).unwrap();
    let line_of = |text: &str, what: &str| text[..text.find(what).unwrap()].matches('\n').count() + 1;

    let bad = toml.replacen("velocity = [1, 64]", "velocity = [0, 64]", 1);
    let region = line_of(&bad, "velocity = [0, 64]") - 4;
    assert!(bad.lines().nth(region - 1).unwrap().starts_with("[[region]]"), "{bad}");
    std::fs::write(&file, &bad).unwrap();
    let text = failed(t.m2(&["pack", "check", pack.to_str().unwrap()]));
    assert!(text.contains(&format!("@test/tiny-keys/instruments/tiny-keys.toml:{region}:1: ")), "{text}");

    let bad = toml.replace("voices = 32", "voices = 200");
    std::fs::write(&file, &bad).unwrap();
    let text = failed(t.m2(&["pack", "check", pack.to_str().unwrap()]));
    let at = format!("@test/tiny-keys/instruments/tiny-keys.toml:{}:10: ", line_of(&bad, "voices = 200"));
    assert!(text.contains(&at) && text.contains("`voices` must be a whole number from 1 to 128, not 200"), "{text}");
}

// PACK-13, PACK-12: two data instruments whose names make one name in code fail check, naming both.
#[test]
fn check_refuses_two_instruments_with_one_name_in_code() {
    let t = Temp::new("sfz-twins");
    let pack = imported(&t);
    let toml = std::fs::read_to_string(pack.join("instruments/tiny-keys.toml")).unwrap();
    std::fs::write(pack.join("instruments/a-1.toml"), &toml).unwrap();
    std::fs::write(pack.join("instruments/a1.toml"), &toml).unwrap();
    let text = failed(t.m2(&["pack", "check", pack.to_str().unwrap()]));
    assert!(text.contains("instruments/a-1.toml` and `@test/tiny-keys/instruments/a1.toml` would both be exported as `a1`"), "{text}");
}

// PACK-14: what check would refuse stops the import, and nothing is left: not the folder, not a
// folder beside it.
#[test]
fn an_import_check_would_refuse_writes_nothing() {
    let t = Temp::new("sfz-nothing");
    copy(&sfz_fixture(), &t.0.join("in"));
    let out = t.0.join("out");
    std::fs::create_dir_all(&out).unwrap();
    let pack = out.join("pack");
    let sfz = t.0.join("in/Tiny Keys.sfz");
    let text = failed(t.m2(&["pack", "import-sfz", sfz.to_str().unwrap(), pack.to_str().unwrap(), "--name", "@test/new"]));
    assert!(text.contains("`new` can't be an instrument's id: its name in code, `new`, is a word JavaScript keeps for itself"), "{text}");
    std::fs::write(
        &sfz,
        std::fs::read_to_string(&sfz).unwrap().replace("<region> sample=c4-loud.wav", "<region> seq_length=2 sample=c4-loud.wav"),
    )
    .unwrap();
    let text = failed(t.m2(&["pack", "import-sfz", sfz.to_str().unwrap(), pack.to_str().unwrap(), "--name", "@test/tiny-keys"]));
    assert!(text.contains("Tiny Keys.sfz:18: a round robin of 2 takes"), "{text}");
    assert_eq!(std::fs::read_dir(&out).unwrap().count(), 0, "nothing is left");
}

// PACK-14: an SFZ named from the working folder (`i.sfz`) reads only its folder: `..` is above it,
// never the working folder again.
#[test]
fn an_sfz_beside_the_working_folder_reads_only_its_folder() {
    let t = Temp::new("sfz-here");
    copy(&sfz_fixture(), &t.0.join("in"));
    std::fs::copy(t.0.join("in/samples/rel.wav"), t.0.join("secret.wav")).unwrap();
    std::fs::copy(t.0.join("in/samples/rel.wav"), t.0.join("in/secret.wav")).unwrap();
    std::fs::write(t.0.join("in/i.sfz"), "<region> sample=../secret.wav\n").unwrap();
    let text = failed(t.m2_in(&t.0.join("in"), &["pack", "import-sfz", "i.sfz", "../pack", "--name", "@test/x"]));
    assert!(text.contains("i.sfz:1: `sample=../secret.wav` leaves the SFZ's folder"), "{text}");
    assert!(!t.0.join("pack").exists());
}
