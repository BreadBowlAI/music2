//! The library over HTTP (LIB-1, LIB-2) and its previews, kept in the cache (LIB-3).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use m2_editor::presets::{self, Preset};
use serde_json::Value;

use crate::graph::Graph;
use crate::library::{Previews, preview};

/// A server with no song open, and the tools key `t00ls`.
fn serving() -> crate::Running {
    let options =
        crate::ServeOptions { port: 0, web_dir: std::env::temp_dir().join("m2-no-web"), token: "t00ls".into(), page_token: "t0k".into() };
    crate::start_with(options, Graph::silent()).unwrap()
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
    let p = Preset {
        name: name.into(),
        sound: "std:synth-pluck".into(),
        values: [("brightness".to_string(), brightness)].into(),
        plugin_version: None,
        state: None,
    };
    presets::store(&yours.join(presets::folder(&p.sound)), &p, true).unwrap();
}

// LIB-1, LIB-2: the route answers the one search, ranked, to agents, with no song open (std's) and
// with one (the song's build's).
#[test]
fn the_library_route_answers_the_one_search() {
    let running = serving();
    assert_eq!(found(&running.url, "reverb")[0], "std:reverb");
    assert_eq!(found(&running.url, "glass pluck")[0], "sound:std/std:synth-pluck/Glass");
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
    let first = preview(&previews, id, Some(&yours), &cache).unwrap();
    assert!(first.rendered && first.file.starts_with(&cache), "{first:?}");
    let bytes = std::fs::read(&first.file).unwrap();
    assert_eq!(&bytes[..4], b"RIFF");
    let again = preview(&previews, id, Some(&yours), &cache).unwrap();
    assert!(!again.rendered && again.file == first.file, "kept: {again:?}");
    assert_eq!(std::fs::read(&again.file).unwrap(), bytes);

    mine(&yours, "Mine", 12000.0);
    let changed = preview(&previews, id, Some(&yours), &cache).unwrap();
    assert!(changed.rendered && changed.file != first.file, "a changed preset renders anew: {changed:?}");
    assert_ne!(std::fs::read(&changed.file).unwrap(), bytes, "brighter");
    assert_eq!(wavs(&cache), std::slice::from_ref(&changed.file), "only the newest of an item is kept");

    let pad = preview(&previews, "std:synth-pad", Some(&yours), &cache).unwrap();
    assert!(pad.rendered && !preview(&previews, "std:synth-pad", Some(&yours), &cache).unwrap().rendered);
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
            std::thread::spawn(move || preview(&previews, "std:synth-strings", None, &cache).unwrap())
        })
        .collect();
    let answers: Vec<_> = asks.into_iter().map(|a| a.join().unwrap()).collect();
    assert_eq!(answers.iter().filter(|a| a.rendered).count(), 1, "{answers:?}");
    assert_eq!(answers[0].file, answers[1].file);
}

// LIB-3: the route answers a WAV, or says in words why there is none.
#[test]
fn the_preview_route_answers_a_wav_or_says_why_not() {
    let running = serving();
    let (status, kind, body) = get(&running.url, "/api/library/preview?id=std:fm-bell");
    assert_eq!((status, kind.as_str(), &body[..4]), (200, "audio/wav", &b"RIFF"[..]));
    let (status, _, body) = get(&running.url, "/api/library/preview?id=clap:org.example.none");
    assert_eq!(status, 404);
    assert_eq!(json(&body)["error"], "There is no clap:org.example.none in the library.");
}
