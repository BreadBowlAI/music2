//! The registry in the app over HTTP (REG-11 to REG-17): a bucket the robot builds from two
//! made-up packs (`@alice/drums`, one sample; `@bob/keys`, an instrument and a sound of it), served
//! over http on this computer. Every server here keeps scratch folders, never yours, and reaches
//! no other computer.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use m2_editor::get::Remote;
use serde_json::{Value, json};

use crate::graph::Graph;
use crate::library::Folders;

/// A scratch folder for one test, removed when dropped (read-only files too: an installed pack's).
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("m2-get-{name}-{}", std::process::id()));
        let s = Scratch(dir);
        s.clean();
        std::fs::create_dir_all(&s.0).unwrap();
        s
    }

    fn clean(&self) {
        let mut stack = vec![self.0.clone()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                let mut perm = e.metadata().unwrap().permissions();
                #[allow(clippy::permissions_set_readonly_false)]
                perm.set_readonly(false);
                let _ = std::fs::set_permissions(e.path(), perm);
                if e.file_type().unwrap().is_dir() {
                    stack.push(e.path());
                }
            }
        }
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        self.clean();
    }
}

fn write(path: &Path, text: impl AsRef<[u8]>) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// A 16-bit mono WAV of a tenth of a second of a tone at 48 kHz.
fn tone(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let spec = hound::WavSpec { channels: 1, sample_rate: 48_000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for i in 0..4_800 {
        w.write_sample(((i as f32 / 10.0).sin() * 8_000.0) as i16).unwrap();
    }
    w.finalize().unwrap();
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

/// A bucket in `root/bucket` that the robot built from two made-up packs, as the registry's CI
/// builds one: `@alice/drums` 1.0.0 (one sample) and `@bob/keys` 0.1.0 (an instrument, and its
/// sound Dark). The keys' folder too. With `newer`, `@bob/keys` 0.2.0 too, the latest, which adds
/// Hard keys.
fn bucket(root: &Path, newer: bool) -> (PathBuf, PathBuf) {
    let std = crate::resources::std_dir();
    let registry = root.join("registry");
    write(&registry.join("tags.txt"), "drums\nkeys\n");
    write(&registry.join("packs/.gitkeep"), "");
    let drums = root.join("src/drums");
    write(
        &drums.join("m2pkg.toml"),
        "[package]\nname = \"@alice/drums\"\nversion = \"1.0.0\"\nlicense = \"CC0-1.0\"\ndescription = \"One kick\"\n\
         authors = [\"Alice\"]\nrepository = \"https://github.com/alice/drums\"\ntags = [\"drums\"]\n",
    );
    write(&drums.join("samples.toml"), "[folder.\"kicks\"]\ninstrument = \"kick\"\n\n[folder.\"snaps\"]\ninstrument = \"clap\"\n");
    tone(&drums.join("samples/kicks/Boom.wav"));
    let flac = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../core/runtime/tests/fixtures/flac/16bit-mono.flac");
    write(&drums.join("samples/snaps/Snap.flac"), std::fs::read(flac).unwrap());
    let keys = root.join("src/keys");
    write(
        &keys.join("m2pkg.toml"),
        "[package]\nname = \"@bob/keys\"\nversion = \"0.1.0\"\nlicense = \"MIT\"\ndescription = \"Soft keys\"\nauthors = [\"Bob\"]\n\
         repository = \"https://github.com/bob/keys\"\ntags = [\"keys\"]\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.6\"\n",
    );
    write(&keys.join("src/index.ts"), KEYS);
    write(
        &keys.join("presets/soft-keys/Dark.json"),
        "{\n  \"m2preset\": 1,\n  \"name\": \"Dark\",\n  \"sound\": \"@bob/keys:soft-keys\",\n  \"values\": {\n    \"brightness\": 600\n  }\n}\n",
    );
    let keys2 = root.join("src/keys2");
    let mut packs = vec![
        (drums.clone(), "https://github.com/alice/drums/releases/download/v1.0.0/drums-1.0.0.tar.gz"),
        (keys.clone(), "https://github.com/bob/keys/releases/download/v0.1.0/keys-0.1.0.tar.gz"),
    ];
    if newer {
        copy(&keys, &keys2);
        let m = std::fs::read_to_string(keys.join("m2pkg.toml")).unwrap();
        write(&keys2.join("m2pkg.toml"), m.replace("version = \"0.1.0\"", "version = \"0.2.0\""));
        let hard = KEYS.replace("softKeys", "hardKeys").replace("soft-keys", "hard-keys").replace("Soft keys", "Hard keys");
        write(&keys2.join("src/index.ts"), format!("{KEYS}\n{}", hard.split_once("\n\n").unwrap().1));
        packs.push((keys2.clone(), "https://github.com/bob/keys/releases/download/v0.2.0/keys-0.2.0.tar.gz"));
    }
    let sources = root.join("sources");
    for (dir, source) in &packs {
        let added = m2_editor::registry::add(&m2_editor::registry::Add { registry: &registry, pack: dir, source, std: &std });
        let dir = &dir.to_path_buf();
        let m2_editor::registry::Added::Listed { hash, .. } = added.unwrap_or_else(|e| panic!("{}", e.join("\n"))) else { panic!() };
        copy(dir, &sources.join(hash.hex()).join(dir.file_name().unwrap()));
    }
    let out = root.join("bucket");
    let built = m2_editor::registry::build(&m2_editor::registry::Build {
        registry: &registry,
        sources: &sources,
        out: &out,
        from: None,
        made: "2026-10-30T12:00:00Z",
        std: &std,
        work: &root.join("work"),
        to_delete: None,
        limit: m2_editor::registry::LONGEST,
    })
    .unwrap_or_else(|e| panic!("{}", e.join("\n")));
    assert!(built.failed.is_empty(), "{:?}", built.failed);
    (out, keys)
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let (f, t) = (e.path(), to.join(e.file_name()));
        if f.is_dir() { copy(&f, &t) } else { std::fs::copy(&f, &t).map(drop).unwrap() }
    }
}

/// A folder served over http on 127.0.0.1, each request on a connection of its own; the paths
/// asked for are kept. A path starting with `hold` is answered with half its bytes, then the rest
/// once `hold` no longer names it.
struct Serve {
    url: String,
    asked: Arc<Mutex<Vec<String>>>,
    hold: Arc<Mutex<Option<String>>>,
}

impl Serve {
    fn folder(dir: &Path) -> Serve {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let asked = Arc::new(Mutex::new(vec![]));
        let hold: Arc<Mutex<Option<String>>> = Arc::default();
        let (dir, kept, held) = (dir.to_path_buf(), asked.clone(), hold.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { return };
                let (dir, kept, held) = (dir.clone(), kept.clone(), held.clone());
                std::thread::spawn(move || {
                    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
                    let mut head = vec![];
                    let mut b = [0u8; 1];
                    while !head.ends_with(b"\r\n\r\n") && s.read(&mut b).is_ok_and(|n| n == 1) {
                        head.push(b[0]);
                    }
                    let head = String::from_utf8_lossy(&head).to_string();
                    let path = head.split(' ').nth(1).unwrap_or("/").trim_start_matches('/').to_string();
                    kept.lock().unwrap().push(path.clone());
                    let Ok(body) = std::fs::read(dir.join(&path)) else {
                        let _ = s.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                        return;
                    };
                    let _ =
                        s.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).as_bytes());
                    let holding = || held.lock().unwrap().as_ref().is_some_and(|h| path.starts_with(h.as_str()));
                    if holding() {
                        let _ = s.write_all(&body[..body.len() / 2]);
                        let _ = s.flush();
                        // Until it is let go (or 20 s): then the rest.
                        let t = Instant::now();
                        while holding() && t.elapsed() < Duration::from_secs(20) {
                            std::thread::sleep(Duration::from_millis(20));
                        }
                        let _ = s.write_all(&body[body.len() / 2..]);
                        return;
                    }
                    let _ = s.write_all(&body);
                });
            }
        });
        Serve { url, asked, hold }
    }

    /// The paths asked for since the last call.
    fn asked(&self) -> Vec<String> {
        std::mem::take(&mut *self.asked.lock().unwrap())
    }
}

/// A server with no song open, keeping `folders` (scratch ones), with the tools key `t00ls`.
fn serving(folders: Folders) -> crate::Running {
    let options =
        crate::ServeOptions { port: 0, web_dir: std::env::temp_dir().join("m2-no-web"), token: "t00ls".into(), page_token: "t0k".into() };
    let running = crate::start_with(options, Graph::silent()).unwrap();
    running.state.keep_library(folders);
    running
}

/// A world: the bucket, served; a server reading it, with a scratch store and cache.
struct World {
    scratch: Scratch,
    bucket: PathBuf,
    keys: PathBuf,
    serve: Serve,
    running: crate::Running,
}

impl World {
    fn new(name: &str) -> World {
        World::with(name, false)
    }

    /// With `newer`, the bucket has `@bob/keys` 0.2.0 too.
    fn with(name: &str, newer: bool) -> World {
        let scratch = Scratch::new(name);
        let (bucket, keys) = bucket(&scratch.0, newer);
        let serve = Serve::folder(&bucket);
        let cache = scratch.0.join("cache");
        let folders = Folders {
            packs: Some(scratch.0.join("Packs")),
            previews: Some(cache.join("previews")),
            samples: Some(cache.join("samples")),
            registry: Some(Remote::new(&serve.url, &cache.join("registry")).unwrap()),
            ..Folders::default()
        };
        let running = serving(folders);
        World { scratch, bucket, keys, serve, running }
    }

    fn store(&self) -> m2_song::pack::Store {
        m2_song::pack::Store::at(self.scratch.0.join("Packs"))
    }

    /// `GET /api/library?q=<q>` once the catalog isn't being fetched and no samples are being
    /// indexed: the whole answer.
    fn search(&self, q: &str) -> Value {
        for _ in 0..600 {
            let (status, body) = get(&self.running.url, &format!("/api/library?q={}", enc(q)));
            assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
            let answer: Value = serde_json::from_slice(&body).unwrap();
            if answer["registry"]["fetching"] != json!(true) && answer["folders"]["indexing"] == json!(0) {
                return answer;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("the catalog was never fetched");
    }

    /// The ids From the library `q` finds.
    fn library(&self, q: &str) -> Vec<String> {
        ids(&self.search(q)["library"])
    }

    /// A copy of the first song, open: its session.
    fn song(&self, edit: impl FnOnce(&Path)) -> (u64, PathBuf) {
        self.song_in("song", edit)
    }

    /// A copy of the first song in `name`, open: its session.
    fn song_in(&self, name: &str, edit: impl FnOnce(&Path)) -> (u64, PathBuf) {
        let dir = self.scratch.0.join(name);
        copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song"), &dir);
        edit(&dir);
        let session = self.running.state.graph.open(&dir).unwrap()["session"].as_u64().unwrap();
        (session, dir)
    }
}

fn ids(list: &Value) -> Vec<String> {
    list.as_array().unwrap().iter().map(|i| i["id"].as_str().unwrap().to_string()).collect()
}

fn enc(text: &str) -> String {
    text.bytes().map(|b| if b.is_ascii_alphanumeric() { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// `GET path` with the tools key: the status and the body.
fn get(url: &str, path: &str) -> (u16, Vec<u8>) {
    let addr = url.trim_start_matches("http://");
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    s.write_all(format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nx-m2-token: t00ls\r\nConnection: close\r\n\r\n").as_bytes()).unwrap();
    let mut answer = vec![];
    s.read_to_end(&mut answer).unwrap();
    let at = answer.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let head = String::from_utf8_lossy(&answer[..at]).to_string();
    (head[9..12].parse().unwrap(), answer[at + 4..].to_vec())
}

/// `POST path` with `body` as JSON and the tools key: the status and the answer.
fn post(url: &str, path: &str, body: &Value) -> (u16, Value) {
    let addr = url.trim_start_matches("http://");
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(120))).unwrap();
    let body = body.to_string();
    let head = format!(
        "POST {path} HTTP/1.1\r\nHost: {addr}\r\nx-m2-token: t00ls\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    s.write_all(head.as_bytes()).unwrap();
    s.write_all(body.as_bytes()).unwrap();
    let mut answer = vec![];
    s.read_to_end(&mut answer).unwrap();
    let text = String::from_utf8_lossy(&answer).to_string();
    let status = text[9..12].parse().unwrap();
    (status, text.split_once("\r\n\r\n").map_or(Value::Null, |(_, b)| serde_json::from_str(b).unwrap_or(Value::Null)))
}

const KEYS_ID: &str = "library:@bob/keys:soft-keys";
const KICK_ID: &str = "library:sample:@alice/drums:samples/kicks/Boom.wav";

// REG-13, REG-11: the search answers From the library after what you have: the catalog's items of
// packs this computer hasn't got, by the one search, each with what a Get fetches. A server given
// no registry answers none, and fetches nothing.
#[test]
fn the_library_route_answers_from_the_library_too() {
    let w = World::new("route");
    let answer = w.search("soft");
    assert_eq!(ids(&answer["library"]), [KEYS_ID, "library:sound:std/@bob/keys:soft-keys/Dark"]);
    assert!(!ids(&answer["items"]).iter().any(|i| i.starts_with("library:")), "what you have and the library are apart");
    let keys = &answer["library"][0];
    assert_eq!(
        (&keys["kind"], &keys["label"], &keys["package"], &keys["library"]["version"], &keys["library"]["license"]),
        (&json!("instrument"), &json!("Soft keys"), &json!("@bob/keys"), &json!("0.1.0"), &json!("MIT"))
    );
    assert!(keys["library"]["size"].as_u64().unwrap() > 0);
    assert_eq!(answer["registry"]["made"], json!("2026-10-30T12:00:00Z"));
    assert_eq!(w.library("kick one-shot"), [KICK_ID]);

    let none = serving(Folders::default());
    let (_, body) = get(&none.url, "/api/library?q=soft");
    let answer: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!((&answer["library"], &answer["registry"]), (&json!([]), &Value::Null), "no registry: no group, nothing said");
}

// NET-2, REG-12: what is typed is never sent: once the catalog is here, a search asks the
// registry nothing, whatever the words.
#[test]
fn a_search_sends_nothing() {
    let w = World::new("sends-nothing");
    w.search("");
    let fetched = w.serve.asked();
    assert!(fetched.iter().all(|p| p == "catalog.json" || p.starts_with("catalog/")), "{fetched:?}");
    for q in ["soft keys", "kick", "my secret song idea", "@bob/keys", "90 bpm Am", "dark"] {
        w.search(q);
    }
    assert!(w.serve.asked().is_empty(), "a search sends nothing");
}

// REG-14: a library item's preview is its blob, fetched when played and kept by its hash.
#[test]
fn a_library_items_preview_is_its_blob() {
    let w = World::new("preview");
    let keys = &w.search("soft")["library"][0];
    assert_eq!(keys["preview"], json!(true));
    w.serve.asked();
    let (status, wav) = get(&w.running.url, &format!("/api/library/preview?id={}", enc(KEYS_ID)));
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&wav));
    let catalog: Value = serde_json::from_slice(&std::fs::read(w.bucket.join("catalog.json")).unwrap()).unwrap();
    let blobs = w.serve.asked();
    assert_eq!(blobs.len(), 1, "{blobs:?} ({catalog})");
    assert_eq!(wav, std::fs::read(w.bucket.join(&blobs[0])).unwrap());
    let (status, again) = get(&w.running.url, &format!("/api/library/preview?id={}", enc(KEYS_ID)));
    assert_eq!((status, again == wav), (200, true));
    assert!(w.serve.asked().is_empty(), "kept by its hash");
    let (status, body) = get(&w.running.url, &format!("/api/library/preview?id={}", enc("library:@carol/none:x")));
    assert!(status == 404 && String::from_utf8_lossy(&body).contains("isn't in the library"), "{status}");
}

/// The open song's state.
fn state(w: &World) -> Value {
    w.running.state.graph.state().unwrap()
}

// REG-15, PACK-10, PACK-11: Get installs the pack's whole version into the store, the open song's
// lists offer it at once, and adding its instrument writes the dependency and the lock. Once
// installed, it is no longer From the library.
#[test]
fn get_installs_the_pack_and_the_song_offers_it() {
    let w = World::new("pack");
    let (session, dir) = w.song(|_| {});
    w.search("soft");
    let instruments = |w: &World| {
        state(w)["instruments"].as_array().unwrap().iter().filter_map(|i| i["component"].as_str().map(String::from)).collect::<Vec<_>>()
    };
    assert!(!instruments(&w).contains(&"@bob/keys:soft-keys".to_string()));
    let (status, done) = post(&w.running.url, "/api/library/get", &json!({"id": KEYS_ID}));
    assert_eq!(status, 200, "{done:#}");
    assert_eq!(
        (&done["got"]["pack"], &done["got"]["version"], &done["id"]),
        (&json!("@bob/keys"), &json!("0.1.0"), &json!("@bob/keys:soft-keys"))
    );
    let hash = m2_song::pack::Listing::of(&w.keys).unwrap().hash();
    assert_eq!(w.store().check(&m2_song::PackName::new("@bob/keys").unwrap(), &m2_song::Version(0, 1, 0)).unwrap(), hash);
    assert!(instruments(&w).contains(&"@bob/keys:soft-keys".to_string()), "the song's lists offer it at once");
    let (status, added) = post(&w.running.url, "/api/graph/track/add", &json!({"session": session, "component": "@bob/keys:soft-keys"}));
    assert_eq!(status, 200, "{added:#}");
    assert!(std::fs::read_to_string(dir.join("music2.toml")).unwrap().contains("\"@bob/keys\" = \"0.1\""));
    assert!(std::fs::read_to_string(dir.join("music2.lock")).unwrap().contains(&hash.prefixed()));
    assert!(w.library("soft").is_empty(), "installed: no longer From the library");
    assert!(ids(&w.search("soft")["items"]).contains(&"@bob/keys:soft-keys".to_string()));
}

// REG-16, LIB-7: Get of a sample fetches its one file and adds it to the song with its note
// naming the pack, a clip on a new audio track, one undo step; nothing is left in the cache.
#[test]
fn getting_a_sample_adds_it_with_its_note() {
    let w = World::new("sample");
    let (session, dir) = w.song(|_| {});
    w.search("kick");
    w.serve.asked();
    let (status, done) =
        post(&w.running.url, "/api/library/get", &json!({"id": KICK_ID, "session": session, "to": {"kind": "below", "beat": 4}}));
    assert_eq!(status, 200, "{done:#}");
    assert_eq!((&done["asset"], &done["noted"]), (&json!("Boom.wav"), &json!(true)), "{done:#}");
    assert_eq!(w.serve.asked().len(), 1, "its one file");
    let note: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("assets/Boom.wav.json")).unwrap()).unwrap();
    assert_eq!(
        note,
        json!({"from": {"pack": "@alice/drums", "version": "1.0.0", "file": "samples/kicks/Boom.wav", "author": "Alice", "license": "CC0-1.0"}})
    );
    assert!(done["tracks"][0]["module"].is_string(), "{done:#}");
    assert!(w.store().list().is_empty(), "a sample comes alone: no pack is installed");
    let getting: Vec<_> = walk(&w.scratch.0.join("cache")).into_iter().filter(|p| p.to_string_lossy().contains("getting")).collect();
    assert!(getting.is_empty(), "{getting:?}");
    // With no song open, a sample has nowhere to go: said before anything is fetched.
    w.running.state.graph.close();
    let (status, why) = post(&w.running.url, "/api/library/get", &json!({"id": KICK_ID}));
    assert!(status >= 400 && why["error"].as_str().unwrap().contains("Open a song"), "{status} {why}");
    assert!(w.serve.asked().is_empty());
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            out.push(e.path());
            if e.file_type().unwrap().is_dir() {
                stack.push(e.path());
            }
        }
    }
    out
}

// REG-15: a Get says how far it is in `get` events and can be cancelled; one runs at a time, and
// one cancelled leaves nothing.
#[test]
fn a_get_says_how_far_and_can_be_cancelled() {
    let w = World::new("progress");
    w.search("soft");
    let index = m2_song::pack::Listing::of(&w.keys).unwrap().files.into_iter().find(|f| f.path == "src/index.ts").unwrap().sha256;
    *w.serve.hold.lock().unwrap() = Some(m2_song::registry::blob_path(&index));
    let mut events = w.running.state.events.subscribe();
    let url = w.running.url.clone();
    let getting = std::thread::spawn(move || post(&url, "/api/library/get", &json!({"id": KEYS_ID})));
    let t = Instant::now();
    let mut heard = None;
    while heard.is_none() && t.elapsed() < Duration::from_secs(20) {
        match events.try_recv() {
            Ok(m) if m.contains("\"t\":\"get\"") => heard = Some(serde_json::from_str::<Value>(&m).unwrap()),
            Ok(_) => {}
            Err(_) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    let heard = heard.expect("a get event");
    assert_eq!((&heard["id"], heard["total"].as_u64().is_some_and(|t| t > 0)), (&json!(KEYS_ID), true), "{heard}");
    // Wait until it waits on the held file.
    while !w.serve.asked.lock().unwrap().iter().any(|p| p.contains(&index.hex())) {
        assert!(t.elapsed() < Duration::from_secs(20));
        std::thread::sleep(Duration::from_millis(10));
    }
    let (status, busy) = post(&w.running.url, "/api/library/get", &json!({"id": "library:sound:std/@bob/keys:soft-keys/Dark"}));
    assert!(status >= 400 && busy["error"].as_str().unwrap().contains("already"), "one at a time: {status} {busy}");
    let (status, cancelled) = post(&w.running.url, "/api/library/get/cancel", &json!({}));
    assert_eq!((status, &cancelled["cancelled"]), (200, &json!(true)));
    let (status, why) = getting.join().unwrap();
    assert!(status >= 400 && why["error"].as_str().unwrap().contains("cancelled"), "{status} {why}");
    assert!(w.store().list().is_empty());
    let left: Vec<_> = walk(&w.store().dir).into_iter().filter(|p| p.to_string_lossy().contains("getting")).collect();
    assert!(left.is_empty(), "{left:?}");
    let (_, nothing) = post(&w.running.url, "/api/library/get/cancel", &json!({}));
    assert_eq!(nothing["cancelled"], json!(false), "nothing to cancel");
}

// REG-17: a song whose lock names a version that isn't installed is offered Get when the library
// has it with that very hash; once got, the song builds and plays.
#[test]
fn a_song_missing_a_locked_pack_gets_it_and_plays() {
    let w = World::new("missing");
    let hash = m2_song::pack::Listing::of(&w.keys).unwrap().hash();
    let (_, _dir) = w.song(|dir| {
        let manifest = std::fs::read_to_string(dir.join("music2.toml")).unwrap();
        std::fs::write(dir.join("music2.toml"), manifest + "\"@bob/keys\" = \"0.1\"\n").unwrap();
        std::fs::write(
            dir.join("music2.lock"),
            format!("[[pack]]\nname = \"@bob/keys\"\nversion = \"0.1.0\"\nhash = \"{}\"\n", hash.prefixed()),
        )
        .unwrap();
    });
    assert_eq!(state(&w)["has_code"], json!(false), "it waits for the pack");
    w.search("");
    let (status, body) = get(&w.running.url, "/api/library/missing");
    assert_eq!(status, 200);
    let missing: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        missing["missing"],
        json!([{"pack": "@bob/keys", "version": "0.1.0", "hash": hash.prefixed(), "can": true, "size": missing["missing"][0]["size"]}]),
        "{missing:#}"
    );
    let (status, done) = post(&w.running.url, "/api/library/get", &json!({"pack": "@bob/keys", "version": "0.1.0"}));
    assert_eq!(status, 200, "{done:#}");
    assert_eq!(state(&w)["has_code"], json!(true), "it builds now: {:#}", state(&w)["diagnostics"]);
    let (_, body) = get(&w.running.url, "/api/library/missing");
    assert_eq!(serde_json::from_slice::<Value>(&body).unwrap()["missing"], json!([]));
}

// REG-16, REG-14, LIB-7: a FLAC sample from the library is previewed as a WAV, and comes into the
// song as one, with its note.
#[test]
fn a_flac_sample_comes_in_as_a_wav() {
    let w = World::new("flac");
    let (session, dir) = w.song(|_| {});
    let snap = "library:sample:@alice/drums:samples/snaps/Snap.flac";
    assert_eq!(w.library("snap"), [snap]);
    let (status, wav) = get(&w.running.url, &format!("/api/library/preview?id={}", enc(snap)));
    assert_eq!((status, &wav[..4], &wav[8..12]), (200, &b"RIFF"[..], &b"WAVE"[..]), "its preview is the WAV of it");
    let (status, done) =
        post(&w.running.url, "/api/library/get", &json!({"id": snap, "session": session, "to": {"kind": "below", "beat": 0}}));
    assert_eq!(status, 200, "{done:#}");
    assert_eq!(done["asset"], json!("Snap.wav"), "{done:#}");
    let bytes = std::fs::read(dir.join("assets/Snap.wav")).unwrap();
    assert_eq!((&bytes[..4], &bytes[8..12]), (&b"RIFF"[..], &b"WAVE"[..]));
    let note: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("assets/Snap.wav.json")).unwrap()).unwrap();
    assert_eq!(note["from"]["file"], json!("samples/snaps/Snap.flac"));
}

// REG-15: once a Get is being installed it can't be cancelled: a cancel then is refused in
// words, and the Get ends installed.
#[test]
fn a_cancel_once_installing_is_refused() {
    let w = World::new("installing");
    w.search("soft");
    let mut events = w.running.state.events.subscribe();
    let url = w.running.url.clone();
    let getting = std::thread::spawn(move || post(&url, "/api/library/get", &json!({"id": KEYS_ID})));
    let t = Instant::now();
    let mut installing = false;
    while !installing && t.elapsed() < Duration::from_secs(30) {
        match events.try_recv() {
            Ok(m) => installing = m.contains("\"t\":\"get\"") && m.contains("\"installing\":true"),
            Err(_) => std::thread::sleep(Duration::from_millis(5)),
        }
    }
    assert!(installing, "a get event says it is installing");
    let (status, cancelled) = post(&w.running.url, "/api/library/get/cancel", &json!({}));
    assert_eq!((status, &cancelled["cancelled"]), (200, &json!(false)), "{cancelled}");
    let (status, done) = getting.join().unwrap();
    assert_eq!(status, 200, "{done:#}");
    assert!(!w.store().list().is_empty(), "installed");
}

// REG-13: a pack the open song lists or locks is offered nothing of: moving the song to
// another version is `m2 pack update`, never a Get. With no song that uses it, its latest is
// offered, saying the version you have, and Get gets that version, whose items the lists then
// offer.
#[test]
fn a_pack_the_open_song_uses_is_offered_nothing_of() {
    let w = World::with("song-uses", true);
    let (session, _) = w.song(|_| {});
    w.search("keys");
    // 0.1.0 in the store, and in the song.
    let store = w.store();
    let old = w.scratch.0.join("old-keys");
    copy(&w.keys, &old);
    m2_editor::packs::install(&old, &store, &crate::resources::std_dir()).unwrap();
    w.running.state.graph.packs_installed().unwrap();
    let (status, added) = post(&w.running.url, "/api/graph/track/add", &json!({"session": session, "component": "@bob/keys:soft-keys"}));
    assert_eq!(status, 200, "{added:#}");
    assert!(w.library("keys").is_empty(), "the song uses @bob/keys: nothing of it is offered");
    assert!(w.library("hard").is_empty());

    // Another song, which doesn't use it: 0.2.0 is offered, saying you have 0.1.0.
    let (_, _) = w.song_in("other", |_| {});
    let answer = w.search("hard");
    assert_eq!(ids(&answer["library"]), ["library:@bob/keys:hard-keys"]);
    assert_eq!((&answer["library"][0]["library"]["version"], &answer["library"][0]["library"]["have"]), (&json!("0.2.0"), &json!("0.1.0")));
    let (status, done) = post(&w.running.url, "/api/library/get", &json!({"id": "library:@bob/keys:hard-keys"}));
    assert_eq!((status, &done["got"]["version"]), (200, &json!("0.2.0")), "{done:#}");
    let instruments: Vec<String> =
        state(&w)["instruments"].as_array().unwrap().iter().filter_map(|i| i["component"].as_str().map(String::from)).collect();
    assert!(instruments.contains(&"@bob/keys:hard-keys".to_string()), "the lists offer the version got: {instruments:?}");
}

// REG-13: From the library is searched over an index made once a catalog, never again a
// keystroke, and answers at most `LIBRARY_ROWS` rows, saying how many more there are. The times,
// at 100,000 items, are printed.
#[test]
fn from_the_library_is_capped_and_made_once_a_catalog() {
    use m2_song::registry::{CatalogPack, CatalogVersion, ItemFacts, ItemKind};
    let scratch = Scratch::new("big");
    let bucket = scratch.0.join("bucket");
    let item = |p: usize, i: usize| ItemFacts {
        kind: ItemKind::Instrument,
        id: format!("@a/p{p:04}:soft-{i:02}"),
        label: format!("Soft thing {p} {i}"),
        category: "Keys".into(),
        description: "A soft pluck".into(),
        tags: vec!["keys".into()],
        component: format!("@a/p{p:04}:soft-{i:02}"),
        instrument: true,
        effect: false,
        preset: None,
        sample: None,
        preview: None,
        no_preview: Some("none".into()),
        file: None,
    };
    let packs: Vec<CatalogPack> = (0..2000)
        .map(|p| {
            let mut items: Vec<ItemFacts> = (0..50).map(|i| item(p, i)).collect();
            items.sort_by(|a, b| a.id.cmp(&b.id));
            CatalogPack {
                name: m2_song::PackName::new(&format!("@a/p{p:04}")).unwrap(),
                repository: format!("https://github.com/a/p{p:04}"),
                description: String::new(),
                license: "MIT".into(),
                authors: vec![],
                tags: vec![],
                latest: Some(m2_song::Version(1, 0, 0)),
                versions: vec![CatalogVersion {
                    version: m2_song::Version(1, 0, 0),
                    hash: m2_song::Sha::of(format!("{p}").as_bytes()),
                    source: String::new(),
                    size: Some(1),
                    files: Some(1),
                    yanked: None,
                    removed: None,
                }],
                items,
            }
        })
        .collect();
    let catalog = m2_song::registry::Catalog { format: 1, made: "2026-10-30T12:00:00Z".into(), packs };
    let pointer = catalog.pointer();
    write(&bucket.join(m2_song::registry::catalog_path(&pointer.catalog)), catalog.bytes());
    write(&bucket.join("catalog.json"), pointer.bytes());
    let cache = scratch.0.join("cache");
    let remote = Remote::new(&m2_editor::fetch::file_address(&bucket).unwrap(), &cache.join("registry")).unwrap();
    let running = serving(Folders { packs: Some(scratch.0.join("Packs")), registry: Some(remote), ..Folders::default() });
    let w = |q: &str| {
        let t = Instant::now();
        for _ in 0..600 {
            let (status, body) = get(&running.url, &format!("/api/library?q={}", enc(q)));
            assert_eq!(status, 200);
            let answer: Value = serde_json::from_slice(&body).unwrap();
            if answer["registry"]["fetching"] != json!(true) && !answer["registry"]["made"].is_null() {
                return (answer, body.len(), t.elapsed());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("never fetched");
    };
    w("");
    let made = running.state.registry.offers_made();
    for q in ["s", "soft", "soft thing 1999", "zzzq", "keys", "s"] {
        let (answer, bytes, took) = w(q);
        let rows = answer["library"].as_array().unwrap().len();
        eprintln!("100,000 items: `{q}` answered {rows} rows (+{} more), {bytes} bytes, in {took:?}", answer["library_more"]);
        assert!(rows <= crate::registry::LIBRARY_ROWS, "{q}: {rows} rows");
        if q == "s" {
            assert_eq!(rows, crate::registry::LIBRARY_ROWS);
            assert_eq!(answer["library_more"], json!(100_000 - crate::registry::LIBRARY_ROWS), "it says how many more");
        }
    }
    assert_eq!(running.state.registry.offers_made(), made, "made once for the catalog, never a keystroke");
}

// REG-16: a sample's folder in the cache that a crashed Get left (another process's, or one
// with this process's id that no Get of it uses) is swept by the next sample's Get.
#[test]
fn a_samples_folder_a_crash_left_is_swept() {
    let w = World::new("sample-sweep");
    let (session, _) = w.song(|_| {});
    w.search("kick");
    let cache = w.scratch.0.join("cache/registry");
    for left in ["getting-4000000000-0".to_string(), format!("getting-{}-123456", std::process::id())] {
        write(&cache.join(left).join("Boom.wav"), "half");
    }
    let (status, done) =
        post(&w.running.url, "/api/library/get", &json!({"id": KICK_ID, "session": session, "to": {"kind": "below", "beat": 0}}));
    assert_eq!(status, 200, "{done:#}");
    let left: Vec<_> = std::fs::read_dir(&cache)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("getting-"))
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

// REG-16: a sample's Get remembers the song it was asked for: when another is open by the
// time its file is in, nothing is added to either, said in words.
#[test]
fn a_sample_got_for_a_song_no_longer_open_is_not_added() {
    let w = World::new("sample-song-gone");
    let (session, first) = w.song(|_| {});
    w.search("kick");
    let file = m2_song::Sha::of(&std::fs::read(w.scratch.0.join("src/drums/samples/kicks/Boom.wav")).unwrap());
    *w.serve.hold.lock().unwrap() = Some(m2_song::registry::blob_path(&file));
    let url = w.running.url.clone();
    let getting = std::thread::spawn(move || {
        post(&url, "/api/library/get", &json!({"id": KICK_ID, "session": session, "to": {"kind": "below", "beat": 0}}))
    });
    let t = Instant::now();
    while !w.serve.asked.lock().unwrap().iter().any(|p| p.contains(&file.hex())) {
        assert!(t.elapsed() < Duration::from_secs(20));
        std::thread::sleep(Duration::from_millis(10));
    }
    let (_, other) = w.song_in("other", |_| {});
    *w.serve.hold.lock().unwrap() = None;
    let (status, why) = getting.join().unwrap();
    assert!(status >= 400 && why["error"].as_str().unwrap().contains("isn't open any more"), "{status} {why}");
    assert!(!first.join("assets/Boom.wav").exists() && !other.join("assets/Boom.wav").exists());
}

// REG-17: told the store changed (`m2 pack get` with the app open), the app builds a song that
// waited for a pack, as a Get in the app does.
#[test]
fn told_the_store_changed_a_song_that_waited_builds() {
    let w = World::new("store-changed");
    let hash = m2_song::pack::Listing::of(&w.keys).unwrap().hash();
    w.song(|dir| {
        let manifest = std::fs::read_to_string(dir.join("music2.toml")).unwrap();
        std::fs::write(dir.join("music2.toml"), manifest + "\"@bob/keys\" = \"0.1\"\n").unwrap();
        let lock = format!("[[pack]]\nname = \"@bob/keys\"\nversion = \"0.1.0\"\nhash = \"{}\"\n", hash.prefixed());
        std::fs::write(dir.join("music2.lock"), lock).unwrap();
    });
    assert_eq!(state(&w)["has_code"], json!(false));
    let old = w.scratch.0.join("by-m2");
    copy(&w.keys, &old);
    m2_editor::packs::install(&old, &w.store(), &crate::resources::std_dir()).unwrap();
    assert_eq!(state(&w)["has_code"], json!(false), "the app doesn't watch the store");
    let (status, _) = post(&w.running.url, "/api/library/store-changed", &json!({}));
    assert_eq!(status, 200);
    assert_eq!(state(&w)["has_code"], json!(true), "told, it builds: {:#}", state(&w)["diagnostics"]);
}
