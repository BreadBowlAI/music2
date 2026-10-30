//! The library over HTTP (LIB-1, LIB-2) and its previews, kept in the cache (LIB-3). Every server
//! here keeps scratch folders for your presets and the cache, never yours.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use m2_editor::presets::{self, Preset};
use serde_json::Value;

use crate::graph::Graph;
use crate::library::{Folders, Previews, preview};

/// A server with no song open, and the tools key `t00ls`, keeping `folders` (scratch ones).
fn serving(folders: Folders) -> crate::Running {
    let options =
        crate::ServeOptions { port: 0, web_dir: std::env::temp_dir().join("m2-no-web"), token: "t00ls".into(), page_token: "t0k".into() };
    let running = crate::start_with(options, Graph::silent()).unwrap();
    running.state.keep_library(folders);
    running
}

/// std alone: what a preview may load with no song open.
fn std_only() -> Vec<(String, PathBuf)> {
    vec![("std".to_string(), crate::resources::std_dir())]
}

/// `GET path` with the tools key: the status, the content type and the body.
fn get(url: &str, path: &str) -> (u16, String, Vec<u8>) {
    let addr = url.trim_start_matches("http://");
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    s.write_all(format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nx-m2-token: t00ls\r\nConnection: close\r\n\r\n").as_bytes()).unwrap();
    let mut answer = vec![];
    s.read_to_end(&mut answer).unwrap();
    let at = answer.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let head = String::from_utf8_lossy(&answer[..at]).to_string();
    let kind = head.lines().find_map(|l| l.to_lowercase().strip_prefix("content-type: ").map(str::to_string)).unwrap_or_default();
    (head[9..12].parse().unwrap(), kind, answer[at + 4..].to_vec())
}

fn json(body: &[u8]) -> Value {
    serde_json::from_slice(body).unwrap()
}

/// The ids of `GET /api/library?q=<q>`, in order.
fn found(url: &str, q: &str) -> Vec<String> {
    let (status, _, body) = get(url, &format!("/api/library?q={}", q.replace(' ', "%20")));
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    json(&body)["items"].as_array().unwrap().iter().map(|i| i["id"].as_str().unwrap().to_string()).collect()
}

/// A scratch folder for one test, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("m2-library-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The WAV files in `dir`.
fn wavs(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "wav"))
        .collect();
    v.sort();
    v
}

/// Store your preset `name` for std's Synth pluck in `yours`, with this brightness.
fn mine(yours: &Path, name: &str, brightness: f64) {
    yours_for(yours, "std:synth-pluck", name, brightness);
}

/// Store your preset `name` for `sound` in `yours`, with this brightness.
fn yours_for(yours: &Path, sound: &str, name: &str, brightness: f64) {
    let p = Preset {
        name: name.into(),
        sound: sound.into(),
        values: [("brightness".to_string(), brightness)].into(),
        plugin_version: None,
        state: None,
    };
    presets::store(&yours.join(presets::folder(&p.sound)), &p, true).unwrap();
}

// LIB-1, LIB-2: the route answers the one search, ranked, to agents, with no song open (std's) and
// with one (the song's build's); your presets are the ones in the folder the server keeps.
#[test]
fn the_library_route_answers_the_one_search() {
    let scratch = Scratch::new("route");
    let running = serving(Folders { presets: Some(scratch.0.join("Presets")), previews: None });
    assert_eq!(found(&running.url, "reverb")[0], "std:reverb");
    assert_eq!(found(&running.url, "glass pluck")[0], "sound:std/std:synth-pluck/Glass");
    mine(&scratch.0.join("Presets"), "Glass pluck", 3000.0);
    assert_eq!(found(&running.url, "glass pluck")[..2], ["sound:yours/std:synth-pluck/Glass pluck", "sound:std/std:synth-pluck/Glass"]);
    assert_eq!(
        found(&running.url, "delay effects"),
        ["std:delay", "std:reverb"],
        "a kind typed is a filter: no sounds; Reverb by its category"
    );
    let (_, _, body) = get(&running.url, "/api/library?q=synth%20pluck");
    let first = &json(&body)["items"][0];
    assert_eq!(
        (
            first["kind"].as_str(),
            first["label"].as_str(),
            first["package"].as_str(),
            first["instrument"].as_bool(),
            first["preview"].as_bool()
        ),
        (Some("instrument"), Some("Synth pluck"), Some("std"), Some(true), Some(true))
    );
    let all = found(&running.url, "");
    assert!(all.len() > 40 && all.contains(&"std:drum-kit".to_string()), "every item with no words: {}", all.len());

    // With a song open: what its build offers.
    let dir = Scratch::new("route-song");
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song"), &dir.0);
    running.state.graph.open(&dir.0).unwrap();
    assert_eq!(found(&running.url, "reverb")[0], "std:reverb");
}

// LIB-1: a search never waits for a plugin scan, which can take minutes: while one runs it
// answers with the list from before it.
#[test]
fn a_search_never_waits_for_a_plugin_scan() {
    let running = serving(Folders::default());
    // A scan in progress holds the scan's lock, as `AppState::plugins` does.
    let scan = running.state.scanning.lock();
    let url = running.url.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || tx.send(found(&url, "reverb")));
    let answer = rx.recv_timeout(std::time::Duration::from_secs(30));
    drop(scan);
    assert_eq!(answer.expect("the search answers while the plugins are scanned").first().map(String::as_str), Some("std:reverb"));
}

/// A copy of `from` at `to`, but its audio.
fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let (f, t) = (e.path(), to.join(e.file_name()));
        if f.is_dir() {
            copy(&f, &t);
        } else if f.extension().is_none_or(|x| x != "wav") {
            std::fs::copy(&f, &t).unwrap();
        }
    }
}

// LIB-3: a preview is kept in the cache: a second ask renders nothing and gives the same file; a
// changed preset renders anew, and its old preview goes.
#[test]
fn a_preview_is_kept_until_what_it_renders_changes() {
    let scratch = Scratch::new("kept");
    let (yours, cache) = (scratch.0.join("Presets"), scratch.0.join("cache"));
    let previews = Previews::default();
    mine(&yours, "Mine", 2000.0);
    let id = "sound:yours/std:synth-pluck/Mine";
    let first = preview(&previews, id, &std_only(), Some(&yours), &cache).unwrap();
    assert!(first.rendered && first.file.starts_with(&cache), "{:?}", first.file);
    let bytes = std::fs::read(&first.file).unwrap();
    assert_eq!(&bytes[..4], b"RIFF");
    assert_eq!(first.wav, bytes, "the answer is the file");
    let again = preview(&previews, id, &std_only(), Some(&yours), &cache).unwrap();
    assert!(!again.rendered && again.file == first.file, "kept: {:?}", again.file);
    assert_eq!(std::fs::read(&again.file).unwrap(), bytes);

    mine(&yours, "Mine", 12000.0);
    let changed = preview(&previews, id, &std_only(), Some(&yours), &cache).unwrap();
    assert!(changed.rendered && changed.file != first.file, "a changed preset renders anew: {:?}", changed.file);
    assert_ne!(std::fs::read(&changed.file).unwrap(), bytes, "brighter");
    assert_eq!(wavs(&cache), std::slice::from_ref(&changed.file), "only the newest of an item is kept");

    // An answer is read while the item's lock is held: a newer render of the item, which removes
    // the file it found, can't take it from under its read.
    let older = preview(&previews, id, &std_only(), Some(&yours), &cache).unwrap();
    mine(&yours, "Mine", 6000.0);
    let newer = preview(&previews, id, &std_only(), Some(&yours), &cache).unwrap();
    assert!(newer.rendered && !older.file.exists());
    assert_eq!(older.wav.len(), newer.wav.len());
    assert_ne!(older.wav, newer.wav);
    // A part file of a render that stopped half way goes at the item's next render; one another
    // app may be writing now stays.
    let crashed = cache.join(format!("{}.12345.part", newer.file.file_stem().unwrap().to_string_lossy()));
    let busy = cache.join(format!("{}.23456.part", newer.file.file_stem().unwrap().to_string_lossy()));
    for f in [&crashed, &busy] {
        std::fs::write(f, b"RIFF").unwrap();
    }
    let long_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(600);
    std::fs::File::options().write(true).open(&crashed).unwrap().set_modified(long_ago).unwrap();
    mine(&yours, "Mine", 7000.0);
    assert!(preview(&previews, id, &std_only(), Some(&yours), &cache).unwrap().rendered);
    assert!(
        !crashed.exists() && busy.exists(),
        "{:?}",
        std::fs::read_dir(&cache).unwrap().flatten().map(|e| e.file_name()).collect::<Vec<_>>()
    );
    std::fs::remove_file(&busy).unwrap();

    let pad = preview(&previews, "std:synth-pad", &std_only(), Some(&yours), &cache).unwrap();
    assert!(pad.rendered && !preview(&previews, "std:synth-pad", &std_only(), Some(&yours), &cache).unwrap().rendered);
    assert_eq!(wavs(&cache).len(), 2);
    assert!(std::fs::read_dir(&cache).unwrap().flatten().all(|e| e.path().is_file()), "no song is left in the cache");
}

// LIB-3: one render of an item at a time: two asks at once render it once, and both get it.
#[test]
fn a_second_ask_waits_for_the_first_render() {
    let scratch = Scratch::new("waits");
    let cache = scratch.0.join("cache");
    let previews = std::sync::Arc::new(Previews::default());
    let asks: Vec<_> = (0..2)
        .map(|_| {
            let (previews, cache) = (previews.clone(), cache.clone());
            std::thread::spawn(move || preview(&previews, "std:synth-strings", &std_only(), None, &cache).unwrap())
        })
        .collect();
    let answers: Vec<_> = asks.into_iter().map(|a| a.join().unwrap()).collect();
    assert_eq!(answers.iter().filter(|a| a.rendered).count(), 1, "{answers:?}");
    assert_eq!(answers[0].file, answers[1].file);
}

// LIB-3: the route answers a WAV, kept in the server's cache folder, or says in words why there is
// none.
#[test]
fn the_preview_route_answers_a_wav_or_says_why_not() {
    let scratch = Scratch::new("route-preview");
    let cache = scratch.0.join("previews");
    let running = serving(Folders { presets: Some(scratch.0.join("Presets")), previews: Some(cache.clone()) });
    let (status, kind, body) = get(&running.url, "/api/library/preview?id=std:fm-bell");
    assert_eq!((status, kind.as_str(), &body[..4]), (200, "audio/wav", &b"RIFF"[..]));
    assert_eq!(wavs(&cache).iter().map(std::fs::read).collect::<Result<Vec<_>, _>>().unwrap(), [body], "kept in its cache folder");
    let (status, _, body) = get(&running.url, "/api/library/preview?id=std:sampler");
    assert_eq!((status, json(&body)["error"].as_str()), (404, Some("Sampler plays a file you give it, so it has no preview.")));
    let (status, _, body) = get(&running.url, "/api/library/preview?id=clap:org.example.none");
    assert_eq!(status, 404);
    assert_eq!(json(&body)["error"], "There is no clap:org.example.none in the library.");
}

// LIB-3: an instrument from another package (a pack) plays in its preview from its package, loaded
// beside std, which it is built on; the preview's hash covers that package's files.
#[test]
fn a_packs_instrument_plays_from_its_pack() {
    let scratch = Scratch::new("pack");
    let acme = scratch.0.join("acme");
    std::fs::create_dir_all(acme.join("src")).unwrap();
    std::fs::write(acme.join("m2pkg.toml"), "[package]\nname = \"acme\"\nversion = \"0.1.0\"\n").unwrap();
    std::fs::write(
        acme.join("src/keys.ts"),
        r#"import { component, port, signature, type Audio, type Events } from "@m2/core";
import { synth } from "std/synth";

/** A test pack's instrument, built on std's pluck. */
export const bell = component(
  {
    id: "bell",
    label: "Acme bell",
    signature: signature<(notes: Events) => Audio>(),
    inputs: { notes: port.notes() },
    controls: {},
    output: port.audio(),
    editor: { category: "Instruments", description: "A bell from a pack" },
  },
  (notes: Events): Audio => synth.pluck(notes),
);
"#,
    )
    .unwrap();
    let packages = [std_only(), vec![("acme".to_string(), acme.clone())]].concat();
    let cache = scratch.0.join("cache");
    let made = preview(&Previews::default(), "acme:bell", &packages, None, &cache).unwrap_or_else(|e| panic!("{e:#}"));
    assert!(made.rendered && made.wav.len() > 800_000, "{} bytes", made.wav.len());
    // Its package changed: rendered anew.
    std::fs::write(acme.join("src/keys.ts"), std::fs::read_to_string(acme.join("src/keys.ts")).unwrap() + "// changed\n").unwrap();
    assert!(preview(&Previews::default(), "acme:bell", &packages, None, &cache).unwrap().rendered);
}
