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
            .env_remove("M2_REGISTRY")
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
    let out = t.m2(&["registry", "build", r, "--sources", s, "--out", fresh.to_str().unwrap(), "--from", "http://packs.example.com"]);
    assert!(!out.status.success() && said(&out).contains("--from:"), "plain http only to this computer (NET-3): {}", said(&out));
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

/// Serve the files under `root` over http on this computer, as a bucket's host would: a GET of a
/// path there is its bytes, anything else 404. The port.
fn serve(root: PathBuf) -> u16 {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let root = root.clone();
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).unwrap() == 0 || h == "\r\n" {
                        break;
                    }
                }
                let path = line.split(' ').nth(1).unwrap_or("/").trim_start_matches('/').to_string();
                let file = root.join(&path);
                let body = (!path.contains("..")).then(|| std::fs::read(&file).ok()).flatten();
                let mut stream = stream;
                let (status, body) = match body {
                    Some(b) => ("200 OK", b),
                    None => ("404 Not Found", b"not here".to_vec()),
                };
                let head = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = stream.write_all(head.as_bytes()).and_then(|()| stream.write_all(&body));
            });
        }
    });
    port
}

// REG-9, REG-5: a bucket is read over http too (on this computer, NET-3; https elsewhere): what it
// has isn't pending, a build from it builds nothing it has and makes the same catalog, and what it
// publishes holds: a published version listed with another hash refuses the build.
#[test]
fn a_bucket_is_read_over_http() {
    let t = Temp::new("http");
    let (reg, src, bucket) = (t.0.join("registry"), t.0.join("sources"), t.0.join("bucket"));
    write(&reg.join("tags.txt"), "drums\n");
    std::fs::create_dir_all(reg.join("packs")).unwrap();
    let drums = t.0.join("drums");
    write(
        &drums.join("m2pkg.toml"),
        "[package]\nname = \"@alice/drums\"\nversion = \"1.0.0\"\nlicense = \"CC0-1.0\"\nrepository = \"https://github.com/alice/drums\"\n",
    );
    write(&drums.join("samples.toml"), "[folder.\"kicks\"]\ninstrument = \"kick\"\n");
    short_wav(&drums.join("samples/kicks/Boom.wav"));
    let (r, s, b) = (reg.to_str().unwrap(), src.to_str().unwrap(), bucket.to_str().unwrap());
    let source = "https://github.com/alice/drums/releases/download/v1.0.0/drums-1.0.0.tar.gz";
    ok(t.m2(&["registry", "add", r, drums.to_str().unwrap(), "--source", source]));
    copy(&drums, &src.join(Listing::of(&drums).unwrap().hash().hex()).join("drums"));
    ok(t.m2(&["registry", "build", r, "--sources", s, "--out", b, "--made", "2026-10-30T12:00:00Z"]));

    let from = format!("http://127.0.0.1:{}", serve(bucket.clone()));
    let fresh = t.0.join("fresh");
    let f = fresh.to_str().unwrap();
    let out = t.m2(&["registry", "pending", r, "--out", f, "--from", &from]);
    assert!(out.status.success() && out.stdout.is_empty(), "the bucket has it: {}", said(&out));
    ok(t.m2(&["registry", "build", r, "--sources", s, "--out", f, "--from", &from, "--made", "2026-10-30T12:00:00Z"]));
    assert_eq!(std::fs::read(fresh.join("catalog.json")).unwrap(), std::fs::read(bucket.join("catalog.json")).unwrap(), "the same catalog");
    assert!(!fresh.join("blobs").exists() && !fresh.join("versions").exists(), "nothing the bucket has is built again");

    let file = reg.join("packs/@alice/drums.toml");
    let text = std::fs::read_to_string(&file).unwrap();
    let hash = text.lines().find(|l| l.starts_with("hash")).unwrap();
    std::fs::write(&file, text.replace(hash, "hash = \"sha256:0000000000000000000000000000000000000000000000000000000000000000\""))
        .unwrap();
    let out = failed(t.m2(&[
        "registry",
        "build",
        r,
        "--sources",
        s,
        "--out",
        t.0.join("other").to_str().unwrap(),
        "--from",
        &from,
        "--made",
        "2026-10-30T12:00:00Z",
    ]));
    assert!(out.contains("@alice/drums 1.0.0 is published with the hash"), "{out}");
}

const KEYS: &str = r#"import { component, control, port, signature, type Audio, type Events, type Params } from "@m2/core";
import { synth } from "std/synth";

/** A soft pluck: std's synth pluck, darker. */
export const softKeys = component(
  {
    id: "soft-keys",
    label: "Soft keys",
    signature: signature<(notes: Events) => Audio>(),
    inputs: { notes: port.notes() },
    controls: { brightness: control.hz(1800).range(200, 20000) },
    output: port.audio(),
    editor: { category: "Keys", description: "A soft pluck, from std's synth" },
  },
  (notes: Events, { brightness }: Params): Audio => synth.pluck(notes, { brightness }),
);
"#;

/// A bucket the robot built in `t`'s `bucket` from two made-up packs, as the registry's CI builds
/// one: `@alice/drums` 1.0.0, one sample, and `@bob/keys` 0.1.0, an instrument. Each pack's hash.
fn library_bucket(t: &Temp) -> (PathBuf, String, String) {
    let (reg, src) = (t.0.join("registry"), t.0.join("sources"));
    write(&reg.join("tags.txt"), "drums\nkeys\n");
    std::fs::create_dir_all(reg.join("packs")).unwrap();
    let drums = t.0.join("drums");
    write(
        &drums.join("m2pkg.toml"),
        "[package]\nname = \"@alice/drums\"\nversion = \"1.0.0\"\nlicense = \"CC0-1.0\"\nauthors = [\"Alice\"]\n\
         repository = \"https://github.com/alice/drums\"\ntags = [\"drums\"]\n",
    );
    write(&drums.join("samples.toml"), "[folder.\"kicks\"]\ninstrument = \"kick\"\n");
    short_wav(&drums.join("samples/kicks/Boom.wav"));
    let keys = t.0.join("keys");
    write(
        &keys.join("m2pkg.toml"),
        "[package]\nname = \"@bob/keys\"\nversion = \"0.1.0\"\nlicense = \"MIT\"\nauthors = [\"Bob\"]\n\
         repository = \"https://github.com/bob/keys\"\ntags = [\"keys\"]\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.6\"\n",
    );
    write(&keys.join("src/index.ts"), KEYS);
    let r = reg.to_str().unwrap();
    let mut hashes = vec![];
    for (dir, owner) in [(&drums, "alice"), (&keys, "bob")] {
        let short = dir.file_name().unwrap().to_str().unwrap();
        let source = format!("https://github.com/{owner}/{short}/releases/download/v1/{short}.tar.gz");
        ok(t.m2(&["registry", "add", r, dir.to_str().unwrap(), "--source", &source]));
        let hash = Listing::of(dir).unwrap().hash();
        copy(dir, &src.join(hash.hex()).join(short));
        hashes.push(hash.prefixed());
    }
    let bucket = t.0.join("bucket");
    ok(t.m2(&[
        "registry",
        "build",
        r,
        "--sources",
        src.to_str().unwrap(),
        "--out",
        bucket.to_str().unwrap(),
        "--made",
        "2026-10-30T12:00:00Z",
    ]));
    (bucket, hashes.remove(0), hashes.remove(0))
}

impl Temp {
    /// `m2` with `args`, reading the registry at `registry` (REG-11), run in `dir`.
    fn m2_with(&self, registry: &str, dir: &Path, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_m2"))
            .current_dir(dir)
            .env("HOME", self.0.join("home"))
            .env("M2_AUDIO", "none")
            .env("M2_MIDI", "none")
            .env("M2_REGISTRY", registry)
            .args(args)
            .output()
            .unwrap()
    }
}

/// The bucket in `dir` as a `file://` address.
fn file_url(dir: &Path) -> String {
    format!("file://{}", dir.display())
}

// REG-11: with no registry set, `m2 pack get` says so plainly, and how to set one; `m2 search`
// finds what you have all the same.
#[test]
fn no_registry_is_said_plainly() {
    let t = Temp::new("no-registry");
    let out = failed(t.m2(&["pack", "get", "@bob/keys"]));
    assert!(out.contains("No registry is set") && out.contains("M2_REGISTRY"), "{out}");
    let out = ok(t.m2(&["search", "reverb"]));
    assert!(out.contains("std:reverb") && !out.contains("library"), "{out}");
}

// REG-17, REG-15: `m2 pack get <name>` gets the latest version, whole and checked, into the store;
// in a song's folder `m2 pack get` gets every locked version missing, and then nothing.
#[test]
fn m2_pack_get_gets_a_pack_and_a_songs_missing_ones() {
    let t = Temp::new("pack-get");
    let (bucket, drums, _) = library_bucket(&t);
    let at = file_url(&bucket);
    let out = ok(t.m2_with(&at, &t.0, &["pack", "get", "@bob/keys"]));
    assert!(out.contains("got @bob/keys 0.1.0"), "{out}");
    let store = t.0.join("home/Music/music2/Packs");
    assert!(store.join("@bob/keys/0.1.0/m2pack.json").is_file());
    assert!(ok(t.m2(&["pack", "list"])).contains("@bob/keys 0.1.0"));
    let out = failed(t.m2_with(&at, &t.0, &["pack", "get", "@bob/keys@9.0.0"]));
    assert!(out.contains("isn't in the library") && out.contains("0.1.0"), "{out}");
    let out = failed(t.m2_with(&at, &t.0, &["pack", "get", "@carol/none"]));
    assert!(out.contains("isn't in the library"), "{out}");

    let song = t.0.join("song");
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song"), &song);
    let manifest = std::fs::read_to_string(song.join("music2.toml")).unwrap();
    std::fs::write(song.join("music2.toml"), manifest + "\"@alice/drums\" = \"1.0\"\n").unwrap();
    std::fs::write(song.join("music2.lock"), format!("[[pack]]\nname = \"@alice/drums\"\nversion = \"1.0.0\"\nhash = \"{drums}\"\n"))
        .unwrap();
    let out = ok(t.m2_with(&at, &song, &["pack", "get"]));
    assert!(out.contains("got @alice/drums 1.0.0"), "{out}");
    assert!(store.join("@alice/drums/1.0.0/m2pack.json").is_file());
    let out = ok(t.m2_with(&at, &t.0, &["pack", "get", song.to_str().unwrap()]));
    assert!(out.contains("nothing is missing"), "{out}");
}

// REG-13: `m2 search` with the app closed lists the library's items too, after what you have,
// each marked library; once the pack is got, it is what you have.
#[test]
fn m2_search_lists_the_librarys_items() {
    let t = Temp::new("search-library");
    let (bucket, _, _) = library_bucket(&t);
    let at = file_url(&bucket);
    let out = ok(t.m2_with(&at, &t.0, &["search", "soft"]));
    let row = out.lines().find(|l| l.contains("Soft keys")).unwrap_or_else(|| panic!("{out}"));
    assert!(row.contains("library") && row.contains("library:@bob/keys:soft-keys"), "{row}");
    let json: serde_json::Value = serde_json::from_slice(&t.m2_with(&at, &t.0, &["search", "kick", "--json"]).stdout).unwrap();
    let kick = json
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == "library:sample:@alice/drums:samples/kicks/Boom.wav")
        .expect("the library's kick");
    assert_eq!(kick["library"]["pack"], "@alice/drums");
    ok(t.m2_with(&at, &t.0, &["pack", "get", "@bob/keys"]));
    let out = ok(t.m2_with(&at, &t.0, &["search", "soft"]));
    assert!(out.contains("@bob/keys:soft-keys") && !out.contains("library:@bob/keys"), "{out}");
}

// REG-17: `m2 pack get` with the app open tells it the store changed
// (`POST /api/library/store-changed`, with the tools key), so a song of it that waited for the
// pack builds, and its lists offer it, with no reopening.
#[test]
fn m2_pack_get_tells_the_open_app() {
    use std::io::{BufRead, BufReader, Write};
    let t = Temp::new("tells-app");
    let (bucket, _, _) = library_bucket(&t);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    write(&t.0.join("home/.music2/session.json"), serde_json::json!({"url": url, "token": "k3y", "pid": std::process::id()}).to_string());
    listener.set_nonblocking(true).unwrap();
    let app = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let mut s = loop {
            match listener.accept() {
                Ok((s, _)) => break s,
                Err(_) if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(20)),
                Err(_) => return vec!["nothing was asked".to_string()],
            }
        };
        s.set_nonblocking(false).unwrap();
        let mut head = vec![];
        let mut r = BufReader::new(s.try_clone().unwrap());
        loop {
            let mut line = String::new();
            r.read_line(&mut line).unwrap();
            if line == "\r\n" || line.is_empty() {
                break;
            }
            head.push(line.trim_end().to_string());
        }
        let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}");
        let _ = s.shutdown(std::net::Shutdown::Both);
        head
    });
    let out = ok(t.m2_with(&file_url(&bucket), &t.0, &["pack", "get", "@bob/keys"]));
    let head = app.join().unwrap();
    assert_eq!(head[0], "POST /api/library/store-changed HTTP/1.1", "{head:?}");
    assert!(head.iter().any(|h| h.eq_ignore_ascii_case("x-m2-token: k3y")), "with the tools key: {head:?}");
    assert!(out.contains("got @bob/keys 0.1.0") && out.contains("the app was told"), "{out}");
}
