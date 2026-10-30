//! `m2 registry` (REG-2, REG-5, REG-8, REG-9): the robot as the registry's CI runs it, and `add`
//! as a pack's author does, each in a scratch HOME (PLAT-7).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use m2_song::pack::Listing;

/// A scratch folder, removed when dropped.
struct Temp(PathBuf);

impl Temp {
    fn new(what: &str) -> Temp {
        let d = std::env::temp_dir().join(format!("m2-registry-cli-{what}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Temp(d)
    }

    /// `m2` with `args`, run in the scratch folder, so a relative path is one inside it.
    fn m2(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_m2"))
            .current_dir(&self.0)
            .env("HOME", self.0.join("home"))
            .env("M2_AUDIO", "none")
            .env("M2_MIDI", "none")
            .args(args)
            .output()
            .unwrap()
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn said(out: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

fn ok(out: Output) -> String {
    assert!(out.status.success(), "{}", said(&out));
    said(&out)
}

fn failed(out: Output) -> String {
    assert_eq!(out.status.code(), Some(1), "{}", said(&out));
    said(&out)
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let (f, t) = (e.path(), to.join(e.file_name()));
        if f.is_dir() { copy(&f, &t) } else { std::fs::copy(&f, &t).map(drop).unwrap() }
    }
}

fn write(path: &Path, text: impl AsRef<[u8]>) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
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

// REG-8, REG-2, REG-9, REG-5: an author lists a pack with add; check passes it, and refuses it to
// another author; pending prints what to fetch, one `name version source` a line; build writes
// the bucket and the list of what to delete; read from that bucket, nothing is pending; a
// bucket over the network waits for the download module, said in words.
#[test]
fn the_robot_runs_from_the_command_line() {
    let t = Temp::new("robot");
    let (reg, src, bucket) = (t.0.join("registry"), t.0.join("sources"), t.0.join("bucket"));
    write(&reg.join("tags.txt"), "drums\n");
    std::fs::create_dir_all(reg.join("packs")).unwrap();
    let drums = t.0.join("drums");
    write(
        &drums.join("m2pkg.toml"),
        "[package]\nname = \"@alice/drums\"\nversion = \"1.0.0\"\nlicense = \"CC0-1.0\"\nrepository = \"https://github.com/alice/drums\"\n\
         tags = [\"drums\"]\n",
    );
    write(&drums.join("samples.toml"), "[folder.\"kicks\"]\ninstrument = \"kick\"\n");
    short_wav(&drums.join("samples/kicks/Boom.wav"));
    let source = "https://github.com/alice/drums/releases/download/v1.0.0/drums-1.0.0.tar.gz";
    let (r, d) = (reg.to_str().unwrap(), drums.to_str().unwrap());

    let out = ok(t.m2(&["registry", "add", r, d, "--source", source]));
    assert!(out.contains("listed @alice/drums 1.0.0 (sha256:") && out.contains("a new file"), "{out}");
    assert!(ok(t.m2(&["registry", "check", r])).contains("is good: 1 pack, 1 version"));
    let out = failed(t.m2(&["registry", "check", r, "--author", "mallory"]));
    assert!(out.contains("problem: packs/@alice/drums.toml is @alice's: mallory may change only packs named @mallory/…"), "{out}");

    let b = bucket.to_str().unwrap();
    let out = t.m2(&["registry", "pending", r, "--out", b]);
    let hex = Listing::of(&drums).unwrap().hash().hex();
    assert_eq!(String::from_utf8_lossy(&out.stdout), format!("{hex} @alice/drums 1.0.0 {source}\n"), "{}", said(&out));
    // As the CI unpacks it: into sources/<its hash's hex>/.
    copy(&drums, &src.join(&hex).join("drums"));
    let deletes = t.0.join("to-delete.txt");
    let s = src.to_str().unwrap();
    let out = ok(t.m2(&[
        "registry",
        "build",
        r,
        "--sources",
        s,
        "--out",
        b,
        "--made",
        "2026-10-30T12:00:00Z",
        "--to-delete",
        deletes.to_str().unwrap(),
    ]));
    assert!(out.contains("built @alice/drums 1.0.0: 1 item (1 with a preview), 4 files"), "{out}");
    assert!(out.contains("made 2026-10-30T12:00:00Z): 1 pack, 1 item"), "{out}");
    assert_eq!(std::fs::read_to_string(&deletes).unwrap(), "", "nothing is removed");
    assert!(bucket.join("catalog.json").is_file());

    let from = format!("file://{b}");
    let fresh = t.0.join("fresh");
    let out = t.m2(&["registry", "pending", r, "--out", fresh.to_str().unwrap(), "--from", &from]);
    assert!(out.status.success() && out.stdout.is_empty(), "the bucket has it: {}", said(&out));
    let out = t.m2(&["registry", "build", r, "--sources", s, "--out", fresh.to_str().unwrap(), "--from", "https://packs.example.com"]);
    assert!(!out.status.success() && said(&out).contains("needs music2's download module"), "{}", said(&out));
    let out = failed(t.m2(&["registry", "build", r, "--sources", s, "--out", fresh.to_str().unwrap(), "--made", "today"]));
    assert!(out.contains("`today` isn't a time"), "{out}");

    // A version whose source isn't there fails alone: the rest are built and in the catalog, and
    // the build says which failed, exit 1 (REG-5).
    let hats = t.0.join("hats");
    copy(&drums, &hats);
    let m = hats.join("m2pkg.toml");
    std::fs::write(&m, std::fs::read_to_string(&m).unwrap().replace("alice/drums", "alice/hats")).unwrap();
    ok(t.m2(&["registry", "add", r, hats.to_str().unwrap(), "--source", &source.replace("drums", "hats")]));
    let out = failed(t.m2(&["registry", "build", r, "--sources", s, "--out", fresh.to_str().unwrap(), "--made", "2026-10-30T12:00:00Z"]));
    assert!(out.contains("built @alice/drums 1.0.0") && out.contains("problem: @alice/hats 1.0.0 isn't built yet"), "{out}");
    assert!(out.contains("1 version not built: it stays pending, and the rest are in the catalog"), "{out}");
    assert!(fresh.join("catalog.json").is_file());
}

// REG-6, REG-5: the robot run as the registry's CI runs it, with paths relative to where it runs:
// an instrument made by `m2 pack new`, listed, packed up with tar and unpacked as the CI unpacks
// it, gets a preview rendered from where it was unpacked.
#[test]
fn a_pack_unpacked_where_the_ci_unpacks_it_gets_its_preview() {
    let t = Temp::new("relative");
    write(&t.0.join("registry/tags.txt"), "keys\n");
    std::fs::create_dir_all(t.0.join("registry/packs")).unwrap();
    ok(t.m2(&["pack", "new", "felt-piano", "--name", "@alice/felt-piano"]));
    let manifest = t.0.join("felt-piano/m2pkg.toml");
    let text = std::fs::read_to_string(&manifest).unwrap();
    std::fs::write(&manifest, text.replace("[package]\n", "[package]\nrepository = \"https://github.com/alice/felt-piano\"\n")).unwrap();
    let source = "https://github.com/alice/felt-piano/releases/download/v0.1.0/felt-piano-0.1.0.tar.gz";
    ok(t.m2(&["registry", "add", "registry", "felt-piano", "--source", source]));
    // As the README says to pack it up, and as the CI unpacks it.
    let tar = |args: &[&str]| assert!(Command::new("tar").current_dir(&t.0).args(args).status().unwrap().success());
    tar(&["-czf", "felt-piano-0.1.0.tar.gz", "--exclude=.*", "felt-piano"]);
    let pending = String::from_utf8(t.m2(&["registry", "pending", "registry", "--out", "bucket"]).stdout).unwrap();
    let hex = pending.split(' ').next().unwrap();
    let to = format!("sources/{hex}");
    std::fs::create_dir_all(t.0.join(&to)).unwrap();
    tar(&["xzf", "felt-piano-0.1.0.tar.gz", "-C", &to, "--no-same-owner", "--no-same-permissions"]);
    let out = ok(t.m2(&["registry", "build", "registry", "--sources", "sources", "--out", "bucket", "--made", "2026-10-30T12:00:00Z"]));
    assert!(out.contains("built @alice/felt-piano 0.1.0: 1 item (1 with a preview)"), "{out}");
}
