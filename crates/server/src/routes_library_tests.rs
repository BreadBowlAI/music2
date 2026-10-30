//! The library over HTTP (LIB-1, LIB-2) and its previews, kept in the cache (LIB-3). Every server
//! here keeps scratch folders for your presets and the cache, never yours.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use m2_editor::presets::{self, Preset};
use serde_json::{Value, json};

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
    let running = serving(Folders { presets: Some(scratch.0.join("Presets")), ..Folders::default() });
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
    let running = serving(Folders { presets: Some(scratch.0.join("Presets")), previews: Some(cache.clone()), ..Folders::default() });
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
    std::fs::write(acme.join("m2pkg.toml"), "[package]\nname = \"@acme/tones\"\nversion = \"0.1.0\"\n").unwrap();
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
    let packages = [std_only(), vec![("@acme/tones".to_string(), acme.clone())]].concat();
    let cache = scratch.0.join("cache");
    let made = preview(&Previews::default(), "@acme/tones:bell", &packages, None, &cache).unwrap_or_else(|e| panic!("{e:#}"));
    assert!(made.rendered && made.wav.len() > 800_000, "{} bytes", made.wav.len());
    // Its package changed: rendered anew.
    std::fs::write(acme.join("src/keys.ts"), std::fs::read_to_string(acme.join("src/keys.ts")).unwrap() + "// changed\n").unwrap();
    assert!(preview(&Previews::default(), "@acme/tones:bell", &packages, None, &cache).unwrap().rendered);
}

// ---------------------------------------------------------------- samples (LIB-4 to LIB-7)

/// A 16-bit WAV of `seconds` of a tone in `channels` channels at 48 kHz.
fn tone(path: &Path, channels: u16, seconds: f64) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let spec = hound::WavSpec { channels, sample_rate: 48_000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for i in 0..(seconds * 48_000.0) as usize {
        for _ in 0..channels {
            w.write_sample(((i as f32 / 20.0).sin() * 12_000.0) as i16).unwrap();
        }
    }
    w.finalize().unwrap();
}

/// A pack of samples alone, `@alice/loops` 1.2.0, in `dir`: a 90 bpm loop in A minor (four bars,
/// 10⅔ s), a 120 bpm drum loop (two bars, 4 s) and a kick.
fn loops_pack(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join("m2pkg.toml"),
        "[package]\nname = \"@alice/loops\"\nversion = \"1.2.0\"\nlicense = \"CC-BY-4.0\"\nauthors = [\"Alice Example\"]\n\n\
         [dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.5\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("samples.toml"), "[folder.\"loops\"]\ntags = [\"dusty\"]\n\n[folder.\"one-shots\"]\n").unwrap();
    tone(&dir.join("samples/loops/dusty keys 90 Am.wav"), 2, 32.0 / 3.0);
    tone(&dir.join("samples/loops/drums 120.wav"), 2, 4.0);
    tone(&dir.join("samples/one-shots/kick.wav"), 1, 0.25);
}

/// `@alice/loops` installed in the store `store`, with its listing (PACK-6).
fn installed(store: &Path) {
    let dir = store.join("@alice/loops/1.2.0");
    loops_pack(&dir);
    m2_editor::packs::build(&dir).unwrap();
}

/// A copy of the first song in `dir` (124 bpm), in A minor, taking `@alice/loops` from `pack`.
fn song_with_pack(dir: &Path, pack: &Path) {
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song"), dir);
    let manifest = std::fs::read_to_string(dir.join("music2.toml")).unwrap();
    let line = format!("\"@alice/loops\" = {{ path = {} }}\n", serde_json::to_string(&pack.to_string_lossy()).unwrap());
    std::fs::write(dir.join("music2.toml"), manifest + &line).unwrap();
    std::fs::write(dir.join("layout.json"), "{\"key\": {\"root\": \"A\", \"scale\": \"minor\"}}\n").unwrap();
}

/// `POST path` with `body` as JSON and `key`: the status and the answer.
fn post(url: &str, path: &str, key: &str, body: &Value) -> (u16, Value) {
    let addr = url.trim_start_matches("http://");
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(std::time::Duration::from_secs(120))).unwrap();
    let body = body.to_string();
    let head = format!(
        "POST {path} HTTP/1.1\r\nHost: {addr}\r\nx-m2-token: {key}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
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

/// The items of `GET /api/library?<query>`, as JSON, once the indexes of samples its answer says
/// are running are done (LIB-5: a search never waits for them; it is asked again, as the page and
/// `m2 search` do).
fn items(url: &str, query: &str) -> Vec<Value> {
    for _ in 0..600 {
        let (status, _, body) = get(url, &format!("/api/library?{query}"));
        assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
        let answer = json(&body);
        if answer["folders"]["indexing"] == json!(0) {
            return answer["items"].as_array().unwrap().clone();
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    panic!("the indexes of samples never ended");
}

fn ids(items: &[Value]) -> Vec<String> {
    items.iter().map(|i| i["id"].as_str().unwrap().to_string()).collect()
}

const DUSTY: &str = "sample:@alice/loops:samples/loops/dusty keys 90 Am.wav";
const DRUMS: &str = "sample:@alice/loops:samples/loops/drums 120.wav";
const KICK: &str = "sample:@alice/loops:samples/one-shots/kick.wav";

/// `text` for a query string.
fn enc(text: &str) -> String {
    text.bytes().map(|b| if b.is_ascii_alphanumeric() { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

// LIB-1, LIB-4: with no song open the library has std's kit and the store's packs' samples; with a
// song open, its packs' too; the filters find them, and Fits my song keeps the loops near the
// song's tempo in its key.
#[test]
fn the_samples_of_std_and_a_pack_are_found_and_fit_the_song() {
    let scratch = Scratch::new("samples");
    let store = scratch.0.join("Packs");
    installed(&store);
    let folders = Folders { packs: Some(store.clone()), samples: Some(scratch.0.join("cache")), ..Folders::default() };
    let running = serving(folders);
    let kick = items(&running.url, "q=kick%20one-shot");
    let kicks = ids(&kick);
    assert!(kicks.contains(&"sample:std:samples/kit/kick.wav".to_string()) && kicks.contains(&KICK.to_string()), "{kicks:?}");
    let std_kick = kick.iter().find(|i| i["id"] == "sample:std:samples/kit/kick.wav").unwrap();
    assert_eq!(
        (&std_kick["kind"], &std_kick["package"], &std_kick["sample"]["type"], &std_kick["sample"]["instrument"]),
        (&json!("sample"), &json!("std"), &json!("one-shot"), &json!("kick")),
        "{std_kick:#}"
    );
    assert!(std_kick["sample"].get("file").is_none(), "its path is never sent");
    assert_eq!(std_kick["sample"]["from"]["license"], json!("MIT OR Apache-2.0"));
    assert_eq!(ids(&items(&running.url, &format!("q={}", enc("loop 89 bpm")))), [DUSTY]);
    assert_eq!(ids(&items(&running.url, &format!("q={}", enc("loop C major")))), [DUSTY], "Am is C major's relative");
    let dusty = &items(&running.url, "q=dusty")[0];
    assert_eq!((&dusty["sample"]["bpm"], &dusty["sample"]["key"], &dusty["package"]), (&json!(90.0), &json!("Am"), &json!("@alice/loops")));
    // Fits my song needs a song.
    let (status, _, body) = get(&running.url, "/api/library?q=loop&fits=0");
    assert!(status == 400 && String::from_utf8_lossy(&body).contains("needs a song open"), "{status}");

    // A song at 124 bpm in A minor that takes the pack from a folder: 120 is within 8%, 90 isn't.
    let song = Scratch::new("samples-song");
    let pack = scratch.0.join("loops");
    loops_pack(&pack);
    song_with_pack(&song.0, &pack);
    running.state.graph.open(&song.0).unwrap();
    // Only this test's pack: a song's build also offers the packs installed for its user.
    let ours = |items: Vec<Value>| ids(&items).into_iter().filter(|i| i.starts_with("sample:@alice/loops:")).collect::<Vec<_>>();
    assert_eq!(ours(items(&running.url, "q=loop&fits=0")), [DRUMS], "drums have no key: they fit any");
    assert_eq!(ours(items(&running.url, &format!("q={}", enc("dusty Am")))), [DUSTY]);
    // What the song's pack folder says wins: its samples.toml's tags.
    assert!(items(&running.url, "q=dusty")[0]["tags"].as_array().unwrap().contains(&json!("dusty")));
}

// LIB-6: a sample's preview is its own file, and only for a sample the library lists: an id that
// names a path, or a file outside a package's samples, is refused.
#[test]
fn a_samples_preview_is_its_file_and_only_a_listed_one() {
    let scratch = Scratch::new("sample-preview");
    let store = scratch.0.join("Packs");
    installed(&store);
    let running = serving(Folders { packs: Some(store.clone()), ..Folders::default() });
    let (status, kind, body) = get(&running.url, &format!("/api/library/preview?id={}", enc(KICK)));
    assert_eq!((status, kind.as_str()), (200, "audio/wav"));
    assert_eq!(body, std::fs::read(store.join("@alice/loops/1.2.0/samples/one-shots/kick.wav")).unwrap());
    let elsewhere = scratch.0.join("secret.wav");
    tone(&elsewhere, 1, 0.1);
    for id in [
        format!("sample:folder:{}", elsewhere.display()),
        "sample:@alice/loops:m2pkg.toml".to_string(),
        "sample:@alice/loops:samples/../m2pkg.toml".to_string(),
        "sample:std:m2pkg.toml".to_string(),
    ] {
        let (status, _, body) = get(&running.url, &format!("/api/library/preview?id={}", enc(&id)));
        assert_eq!(status, 404, "{id}: {}", String::from_utf8_lossy(&body));
        assert!(json(&body)["error"].as_str().unwrap().contains("There is no sample"), "{id}");
    }
}

/// A FLAC fixture's bytes, as libsndfile made it (`core/runtime/tests/fixtures/flac/make.py`).
fn flac(name: &str) -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../core/runtime/tests/fixtures/flac").join(name)).unwrap()
}

/// Each channel's samples as their bits.
fn bits(a: &m2_runtime::audio::Asset) -> Vec<Vec<u32>> {
    a.channels.iter().map(|c| c.iter().map(|s| s.to_bits()).collect()).collect()
}

const SNARE: &str = "sample:@alice/loops:samples/one-shots/snare.flac";

// LIB-6, FMT-24: a FLAC sample's preview is the WAV of its samples, made when it is asked for, so
// the page needs no FLAC decoder: the FLAC's very bits.
#[test]
fn a_flac_samples_preview_is_the_wav_of_it() {
    let scratch = Scratch::new("flac-preview");
    let store = scratch.0.join("Packs");
    let dir = store.join("@alice/loops/1.2.0");
    loops_pack(&dir);
    std::fs::write(dir.join("samples/one-shots/snare.flac"), flac("24bit-stereo.flac")).unwrap();
    m2_editor::packs::build(&dir).unwrap();
    let running = serving(Folders { packs: Some(store.clone()), ..Folders::default() });
    let (status, kind, body) = get(&running.url, &format!("/api/library/preview?id={}", enc(SNARE)));
    assert_eq!((status, kind.as_str()), (200, "audio/wav"), "{}", String::from_utf8_lossy(&body));
    let wav = scratch.0.join("preview.wav");
    std::fs::write(&wav, &body).unwrap();
    assert_eq!(hound::WavReader::open(&wav).unwrap().spec().bits_per_sample, 24);
    let want = bits(&m2_runtime::decode::load(&dir.join("samples/one-shots/snare.flac")).unwrap());
    assert!(bits(&m2_runtime::decode::load(&wav).unwrap()) == want, "the FLAC's bits");
}

// LIB-6, FMT-24 (review F5): a FLAC sample's preview is held to 64 MB as the WAV it makes, checked
// from its header before it is decoded: 64 MiB frames of silence (14 KB as FLAC) has none, in words.
#[test]
fn a_flac_whose_wav_is_over_64_mb_has_no_preview() {
    let scratch = Scratch::new("flac-long");
    let mine = scratch.0.join("Samples");
    std::fs::create_dir_all(&mine).unwrap();
    std::fs::write(mine.join("Long Hum.flac"), flac("silence-64mib.flac")).unwrap();
    let running = with_settings(&scratch);
    added(&running, &mine);
    let real = std::fs::canonicalize(&mine).unwrap();
    let id = format!("sample:folder:{}", real.join("Long Hum.flac").display());
    let (status, _, body) = get(&running.url, &format!("/api/library/preview?id={}", enc(&id)));
    assert_eq!(status, 404, "{} bytes served", body.len());
    let words = json(&body)["error"].as_str().unwrap().to_string();
    assert!(words.contains("as a WAV it is 65 MB, too long to preview here (up to 64 MB)"), "{words}");
}

// LIB-7, FMT-24: adding a FLAC sample brings in the WAV of its samples, with its bits, and a note
// with the pack's credit and what the WAV was made from.
#[test]
fn adding_a_flac_sample_brings_in_its_wav_with_its_note() {
    let scratch = Scratch::new("add-flac");
    let pack = scratch.0.join("loops");
    loops_pack(&pack);
    let source = flac("16bit-stereo.flac");
    std::fs::write(pack.join("samples/one-shots/snare.flac"), &source).unwrap();
    let song = Scratch::new("add-flac-song");
    song_with_pack(&song.0, &pack);
    let running = serving(Folders::default());
    let session = running.state.graph.open(&song.0).unwrap()["session"].as_u64().unwrap();
    let (status, done) =
        post(&running.url, "/api/library/sample", "t00ls", &json!({"session": session, "id": SNARE, "to": {"kind": "below", "beat": 0}}));
    assert_eq!(status, 200, "{done:#}");
    assert_eq!((&done["asset"], &done["reused"], &done["noted"]), (&json!("snare.wav"), &json!(false), &json!(true)));
    let want = bits(&m2_runtime::decode::load(&pack.join("samples/one-shots/snare.flac")).unwrap());
    assert!(bits(&m2_runtime::decode::load(&song.0.join("assets/snare.wav")).unwrap()) == want, "the FLAC's bits");
    let note: Value = serde_json::from_str(&std::fs::read_to_string(song.0.join("assets/snare.wav.json")).unwrap()).unwrap();
    let from = json!({"pack": "@alice/loops", "version": "1.2.0", "file": "samples/one-shots/snare.flac", "author": "Alice Example", "license": "CC-BY-4.0"});
    let made = json!({"file": "snare.flac", "format": "FLAC", "sha256": m2_song::Sha::of(&source).hex()});
    assert_eq!(note, json!({"from": from, "made_from": made}));
    assert!(!song.0.join("assets/snare.flac").exists(), "no FLAC in assets/");
    let track = done["tracks"][0]["module"].as_str().unwrap().to_string();
    assert_eq!(audio_clips(&song.0, &track)[0]["asset"], json!("snare.wav"));
}

/// The open song's session and the clips of its audio lane `module`, as the files say.
fn audio_clips(dir: &Path, module: &str) -> Vec<Value> {
    let text = std::fs::read_to_string(dir.join(format!("data/{module}/clips.json"))).unwrap_or_else(|_| "{\"clips\": []}".into());
    serde_json::from_str::<Value>(&text).unwrap()["clips"].as_array().unwrap().clone()
}

// LIB-7: adding a sample copies its file into assets/ with its note, and places one clip on a new
// audio track: one undo step that takes the clip and the track back, leaving the file and its
// note. Added again, the same bytes are that file, and its note is kept.
#[test]
fn adding_a_sample_copies_it_notes_it_and_places_one_clip() {
    let scratch = Scratch::new("add-sample");
    let pack = scratch.0.join("loops");
    loops_pack(&pack);
    let song = Scratch::new("add-sample-song");
    song_with_pack(&song.0, &pack);
    let running = serving(Folders::default());
    let session = running.state.graph.open(&song.0).unwrap()["session"].as_u64().unwrap();
    let (status, done) =
        post(&running.url, "/api/library/sample", "t00ls", &json!({"session": session, "id": KICK, "to": {"kind": "below", "beat": 4}}));
    assert_eq!(status, 200, "{done:#}");
    assert_eq!(
        (&done["asset"], &done["reused"], &done["noted"], &done["follows"]),
        (&json!("kick.wav"), &json!(false), &json!(true), &Value::Null)
    );
    assert_eq!(std::fs::read(song.0.join("assets/kick.wav")).unwrap(), std::fs::read(pack.join("samples/one-shots/kick.wav")).unwrap());
    let note: Value = serde_json::from_str(&std::fs::read_to_string(song.0.join("assets/kick.wav.json")).unwrap()).unwrap();
    assert_eq!(
        note,
        json!({"from": {"pack": "@alice/loops", "version": "1.2.0", "file": "samples/one-shots/kick.wav", "author": "Alice Example", "license": "CC-BY-4.0"}})
    );
    let track = done["tracks"][0]["module"].as_str().unwrap().to_string();
    let clips = audio_clips(&song.0, &track);
    assert_eq!(clips.len(), 1, "{clips:#?}");
    assert_eq!((clips[0]["start"].as_f64(), &clips[0]["asset"], &clips[0]["warp"]), (Some(4.0), &json!("kick.wav"), &json!([])));
    assert_eq!(running.state.graph.state().unwrap()["undo_name"]["name"], "adding kick.wav");
    running.state.graph.undo(session).unwrap();
    assert!(!song.0.join(format!("tracks/{track}.ts")).exists() && audio_clips(&song.0, &track).is_empty(), "one undo takes it back");
    assert!(song.0.join("assets/kick.wav").exists() && song.0.join("assets/kick.wav.json").exists(), "the file and its note stay");
    let (status, again) =
        post(&running.url, "/api/library/sample", "t00ls", &json!({"session": session, "id": KICK, "to": {"kind": "below", "beat": 0}}));
    assert_eq!(
        (status, &again["asset"], &again["reused"], &again["noted"]),
        (200, &json!("kick.wav"), &json!(true), &json!(false)),
        "{again:#}"
    );
    // A file there with the name but other bytes: the next free name whose note's is free too.
    std::fs::write(song.0.join("assets/drums 120.wav.json"), "{}\n").unwrap();
    let (status, drums) =
        post(&running.url, "/api/library/sample", "t00ls", &json!({"session": session, "id": DRUMS, "to": {"kind": "below", "beat": 0}}));
    assert_eq!((status, &drums["asset"]), (200, &json!("drums 120 2.wav")), "{drums:#}");
    assert!(song.0.join("assets/drums 120 2.wav.json").exists());
    // Not a sample the library lists: refused, nothing written.
    let (status, why) = post(
        &running.url,
        "/api/library/sample",
        "t00ls",
        &json!({"session": session, "id": "sample:folder:/etc/hosts", "to": {"kind": "below", "beat": 0}}),
    );
    assert!(status >= 400 && why["error"].as_str().unwrap().contains("There is no sample"), "{status} {why:#}");
}

// LIB-7: a loop whose tempo differs from the song's there is added with Follow tempo on: warp
// markers from its own tempo, its length a whole number of beats, so it plays in time at once.
#[test]
fn a_loop_at_another_tempo_follows_the_songs() {
    let scratch = Scratch::new("follow");
    let pack = scratch.0.join("loops");
    loops_pack(&pack);
    let song = Scratch::new("follow-song");
    song_with_pack(&song.0, &pack);
    let running = serving(Folders::default());
    let session = running.state.graph.open(&song.0).unwrap()["session"].as_u64().unwrap();
    let (status, done) =
        post(&running.url, "/api/library/sample", "t00ls", &json!({"session": session, "id": DUSTY, "to": {"kind": "below", "beat": 8}}));
    assert_eq!((status, &done["follows"], &done["song_bpm"]), (200, &json!(90.0), &json!(124.0)), "{done:#}");
    let clip = &audio_clips(&song.0, done["tracks"][0]["module"].as_str().unwrap())[0];
    let seconds = 32.0 / 3.0;
    assert_eq!((clip["start"].as_f64(), clip["length"].as_f64()), (Some(8.0), Some(16.0)), "four bars of 4/4 at its own 90 bpm");
    let warp = clip["warp"].as_array().unwrap();
    assert_eq!(warp.len(), 2);
    assert_eq!((warp[0][0].as_f64(), warp[0][1].as_f64(), warp[1][1].as_f64()), (Some(0.0), Some(0.0), Some(16.0)));
    assert!((warp[1][0].as_f64().unwrap() - seconds).abs() < 1e-6, "{warp:?}");
}

// LIB-7: a sample dropped on a Sampler's card is copied and noted, and becomes its file: one undo
// step.
#[test]
fn a_sample_dropped_on_a_sampler_is_its_file() {
    let scratch = Scratch::new("to-sampler");
    let pack = scratch.0.join("loops");
    loops_pack(&pack);
    let song = Scratch::new("to-sampler-song");
    song_with_pack(&song.0, &pack);
    let code = "import { song, track } from '@m2/core';\nimport { sampler } from 'std/sampler';\n\
                const s = track('s', ({ lane }) => sampler(lane.notes('notes')));\nexport default song(() => s.out);\n";
    std::fs::write(song.0.join("song.ts"), code).unwrap();
    let _ = std::fs::remove_dir_all(song.0.join("tracks"));
    let _ = std::fs::remove_dir_all(song.0.join("data"));
    let running = serving(Folders::default());
    let session = running.state.graph.open(&song.0).unwrap()["session"].as_u64().unwrap();
    let view = running.state.graph.state().unwrap()["view"].clone();
    let nodes: Vec<Value> = view["modules"].as_array().unwrap().iter().flat_map(|m| m["nodes"].as_array().unwrap().clone()).collect();
    let call = nodes.iter().find(|n| n["component"] == "std:sampler").expect("a sampler's card")["id"].as_str().unwrap().to_string();
    let at = view["revision"].as_u64().unwrap();
    let (status, done) =
        post(&running.url, "/api/library/sample", "t00ls", &json!({"session": session, "id": KICK, "at": at, "call": call}));
    assert_eq!((status, &done["asset"], &done["name"]), (200, &json!("kick.wav"), &json!("setting Sampler to kick.wav")), "{done:#}");
    assert!(std::fs::read_to_string(song.0.join("song.ts")).unwrap().contains("{ sample: \"kick.wav\" }"));
    assert!(song.0.join("assets/kick.wav.json").exists());
}

// LIB-5: a folder of yours is kept in the settings and indexed in the background: a search never
// waits for it, and finds its samples once it is; what isn't WAV or FLAC is counted; Remove forgets
// it.
#[test]
fn a_folder_is_indexed_in_the_background() {
    let scratch = Scratch::new("folder");
    let mine = scratch.0.join("My Samples");
    tone(&mine.join("Kicks/Big Kick.wav"), 1, 0.2);
    tone(&mine.join("Loops/Funk Guitar 92 Gmin.wav"), 2, 1.0);
    std::fs::write(mine.join("Loops/Vox Chop.flac"), flac("16bit-mono.flac")).unwrap();
    std::fs::write(mine.join("Loops/vox.mp3"), b"ID3 not read").unwrap();
    let running = serving(Folders { samples: Some(scratch.0.join("cache")), ..Folders::default() });
    running.state.keep_settings(scratch.0.join("settings.json"));
    let (status, shown) = post(&running.url, "/api/library/folders", "t0k", &json!({"add": mine}));
    assert_eq!(status, 200, "{shown:#}");
    assert_eq!(shown["folders"][0]["path"], json!(mine));
    let kept: Value = serde_json::from_str(&std::fs::read_to_string(scratch.0.join("settings.json")).unwrap()).unwrap();
    assert_eq!(kept["samples"], json!([mine]));
    assert!(running.state.sample_index.settled(std::time::Duration::from_secs(60)), "indexed");
    items(&running.url, "q=sample");
    let (_, _, body) = get(&running.url, "/api/library?q=sample");
    let found = json(&body);
    let labels: Vec<&str> =
        found["items"].as_array().unwrap().iter().filter(|i| i["package"] == "yours").map(|i| i["label"].as_str().unwrap()).collect();
    assert_eq!(labels, ["Big Kick", "Funk Guitar 92 Gmin", "Vox Chop"], "a FLAC is a sample too (FMT-24)");
    assert_eq!(found["folders"]["not_wav"], json!(1), "the MP3: WAV and FLAC only for now");
    let funk = found["items"].as_array().unwrap().iter().find(|i| i["label"] == "Funk Guitar 92 Gmin").unwrap();
    assert_eq!(
        (&funk["sample"]["bpm"], &funk["sample"]["key"], &funk["sample"]["from"]),
        (&json!(92.0), &json!("Gm"), &json!({"folder": true}))
    );
    // Not a folder: refused.
    let (status, _) = post(&running.url, "/api/library/folders", "t0k", &json!({"add": scratch.0.join("nowhere")}));
    assert_eq!(status, 400);
    let (status, shown) = post(&running.url, "/api/library/folders", "t0k", &json!({"remove": mine}));
    assert_eq!((status, shown["folders"].as_array().map(Vec::len)), (200, Some(0)));
    assert!(found_yours(&running.url).is_empty());
}

fn found_yours(url: &str) -> Vec<String> {
    items(url, "q=sample").iter().filter(|i| i["package"] == "yours").map(|i| i["id"].as_str().unwrap().to_string()).collect()
}

/// A server keeping settings and caches in `scratch`, with no song open.
fn with_settings(scratch: &Scratch) -> crate::Running {
    let running = serving(Folders { samples: Some(scratch.0.join("cache")), ..Folders::default() });
    running.state.keep_settings(scratch.0.join("settings.json"));
    running
}

/// Your folders, as `GET /api/library/folders` lists them.
fn your_folders(url: &str) -> Vec<Value> {
    let (status, _, body) = get(url, "/api/library/folders");
    assert_eq!(status, 200);
    json(&body)["folders"].as_array().unwrap().clone()
}

// LIB-5 (review D16, D6): a folder is kept by its real path, once: the same folder written another
// way, a folder inside one of yours, or around one, is refused; so are `/` and your home folder.
#[test]
fn a_folder_is_kept_by_its_real_path_and_not_twice() {
    let scratch = Scratch::new("real-path");
    let mine = scratch.0.join("Samples");
    tone(&mine.join("Kicks/kick.wav"), 1, 0.1);
    let running = with_settings(&scratch);
    let add = |path: &Path| post(&running.url, "/api/library/folders", "t0k", &json!({ "add": path }));
    assert_eq!(add(&mine).0, 200);
    assert_eq!(add(&mine.join("Kicks/..")).0, 200, "the same folder: nothing new");
    assert_eq!(your_folders(&running.url).len(), 1, "{:?}", your_folders(&running.url));
    let (status, why) = add(&mine.join("Kicks"));
    assert!(status == 400 && why["error"].as_str().unwrap().contains("is inside"), "{status} {why}");
    let (status, why) = add(&scratch.0);
    assert!(status == 400 && why["error"].as_str().unwrap().contains("holds"), "{status} {why}");
    let (status, why) = add(Path::new("/"));
    assert!(status == 400 && why["error"].as_str().unwrap().contains("whole disk"), "{status} {why}");
    assert_eq!(your_folders(&running.url).len(), 1);
    // Your home folder itself, whatever it is called.
    let home = scratch.0.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let why = crate::routes_library::folder_to_add(&home.join("."), &[], Some(&home)).unwrap_err().to_string();
    assert!(why.contains("your home folder"), "{why}");
    assert!(crate::routes_library::folder_to_add(&home.join("x"), &[], Some(&home)).is_err(), "not a folder");
}

// LIB-5 (review D16): removing a folder deletes its cache file.
#[test]
fn removing_a_folder_deletes_its_cache() {
    let scratch = Scratch::new("remove");
    let mine = scratch.0.join("Samples");
    tone(&mine.join("kick.wav"), 1, 0.1);
    let running = with_settings(&scratch);
    assert_eq!(post(&running.url, "/api/library/folders", "t0k", &json!({ "add": mine })).0, 200);
    assert!(running.state.sample_index.settled(std::time::Duration::from_secs(60)));
    let file = m2_editor::library::samples::cache_file(&scratch.0.join("cache"), &std::fs::canonicalize(&mine).unwrap());
    assert!(file.exists());
    assert_eq!(post(&running.url, "/api/library/folders", "t0k", &json!({ "remove": mine })).0, 200);
    assert!(!file.exists(), "its cache file is gone");
    assert!(found_yours(&running.url).is_empty());
}

/// Your folder `mine` added to `running` and indexed.
fn added(running: &crate::Running, mine: &Path) {
    assert_eq!(post(&running.url, "/api/library/folders", "t0k", &json!({ "add": mine })).0, 200);
    assert!(running.state.sample_index.settled(std::time::Duration::from_secs(60)));
}

// LIB-6 (review D12): a sample's file is read only as it was found: a link put in its place since
// it was indexed is refused, by the preview and by adding it.
#[cfg(unix)]
#[test]
fn a_preview_never_follows_a_link() {
    let scratch = Scratch::new("link");
    let mine = scratch.0.join("Samples");
    tone(&mine.join("kick.wav"), 1, 0.1);
    let secret = scratch.0.join("private/diary.wav");
    tone(&secret, 1, 0.3);
    let running = with_settings(&scratch);
    added(&running, &mine);
    let id = format!("sample:folder:{}", std::fs::canonicalize(&mine).unwrap().join("kick.wav").display());
    assert_eq!(found_yours(&running.url), std::slice::from_ref(&id));
    std::fs::remove_file(mine.join("kick.wav")).unwrap();
    std::os::unix::fs::symlink(&secret, mine.join("kick.wav")).unwrap();
    let (status, _, body) = get(&running.url, &format!("/api/library/preview?id={}", enc(&id)));
    assert_eq!(status, 404, "the link's target was served: {} bytes", body.len());
    assert!(json(&body)["error"].as_str().unwrap().contains("isn't a plain file"), "{}", String::from_utf8_lossy(&body));
}

// LIB-6 (review D9): a sample's preview is streamed, with its length; a file over 64 MB has none,
// said in words, as the page would hold it whole.
#[test]
fn a_long_file_has_no_preview() {
    let scratch = Scratch::new("long");
    let mine = scratch.0.join("Samples");
    tone(&mine.join("short.wav"), 1, 0.1);
    // A stereo 16-bit file of 68 MB, its data zeros (a sparse file).
    let data: u32 = 68_000_000;
    let mut head = b"RIFF".to_vec();
    head.extend((36 + data).to_le_bytes());
    head.extend(b"WAVEfmt ");
    head.extend(16u32.to_le_bytes());
    for (v, n) in [(1u32, 2), (2, 2), (48_000, 4), (192_000, 4), (4, 2), (16, 2)] {
        head.extend(&v.to_le_bytes()[..n]);
    }
    head.extend(b"data");
    head.extend(data.to_le_bytes());
    let long = mine.join("Long Take.wav");
    std::fs::write(&long, &head).unwrap();
    std::fs::OpenOptions::new().write(true).open(&long).unwrap().set_len(head.len() as u64 + data as u64).unwrap();
    let running = with_settings(&scratch);
    added(&running, &mine);
    let real = std::fs::canonicalize(&mine).unwrap();
    let (status, _, body) =
        get(&running.url, &format!("/api/library/preview?id={}", enc(&format!("sample:folder:{}", real.join("Long Take.wav").display()))));
    assert_eq!(status, 404, "{} bytes served", body.len());
    assert!(json(&body)["error"].as_str().unwrap().contains("too long to preview"), "{}", String::from_utf8_lossy(&body));
    let (status, kind, body) =
        get(&running.url, &format!("/api/library/preview?id={}", enc(&format!("sample:folder:{}", real.join("short.wav").display()))));
    assert_eq!((status, kind.as_str()), (200, "audio/wav"));
    assert_eq!(body, std::fs::read(mine.join("short.wav")).unwrap());
}

// LIB-7 (review D5, D15): every loop with a known tempo follows the song's, at the song's own tempo
// too; its warp is its exact beats (a tail is no whole beat); and the same bytes added from your
// folder, then from a pack, keep the pack's credit.
#[test]
fn every_loop_follows_with_its_exact_beats_and_a_pack_keeps_its_credit() {
    let scratch = Scratch::new("exact");
    let pack = scratch.0.join("loops");
    loops_pack(&pack);
    tone(&pack.join("samples/loops/groove 124.wav"), 1, 60.0 / 124.0 * 8.0);
    tone(&pack.join("samples/loops/tail 120.wav"), 1, 8.3);
    let mine = scratch.0.join("Samples");
    std::fs::create_dir_all(&mine).unwrap();
    std::fs::copy(pack.join("samples/one-shots/kick.wav"), mine.join("my kick.wav")).unwrap();
    let song = Scratch::new("exact-song");
    song_with_pack(&song.0, &pack);
    let running = with_settings(&scratch);
    added(&running, &mine);
    let session = running.state.graph.open(&song.0).unwrap()["session"].as_u64().unwrap();
    items(&running.url, "q=loop");
    let add = |id: &str| {
        post(&running.url, "/api/library/sample", "t00ls", &json!({"session": session, "id": id, "to": {"kind": "below", "beat": 0}}))
    };
    let (status, done) = add("sample:@alice/loops:samples/loops/groove 124.wav");
    assert_eq!((status, &done["follows"], &done["song_bpm"]), (200, &json!(124.0), &json!(124.0)), "{done:#}");
    let clip = &audio_clips(&song.0, done["tracks"][0]["module"].as_str().unwrap())[0];
    assert_eq!(clip["warp"].as_array().map(|w| w[1][1].as_f64()), Some(Some(8.0)), "it follows a later tempo change: {clip:#}");
    let (status, done) = add("sample:@alice/loops:samples/loops/tail 120.wav");
    assert_eq!(status, 200, "{done:#}");
    let clip = &audio_clips(&song.0, done["tracks"][0]["module"].as_str().unwrap())[0];
    let beats = clip["warp"][1][1].as_f64().unwrap();
    assert!(
        (beats - 16.6).abs() < 1e-3 && (clip["length"].as_f64().unwrap() - 16.6).abs() < 1e-3,
        "16 beats and its tail, not 17: {clip:#}"
    );
    // The kick from your folder first, then the same bytes from the pack: the pack's credit.
    let real = std::fs::canonicalize(&mine).unwrap();
    let (status, done) = add(&format!("sample:folder:{}", real.join("my kick.wav").display()));
    assert_eq!((status, &done["asset"]), (200, &json!("my kick.wav")), "{done:#}");
    let (status, done) = add(KICK);
    assert_eq!(
        (status, &done["asset"], &done["reused"], &done["noted"]),
        (200, &json!("my kick.wav"), &json!(true), &json!(true)),
        "{done:#}"
    );
    let note: Value = serde_json::from_slice(&std::fs::read(song.0.join("assets/my kick.wav.json")).unwrap()).unwrap();
    assert_eq!(note["from"]["license"], json!("CC-BY-4.0"), "{note:#}");
}

// LIB-5 (review D1): a package's samples are indexed in the background, once: searches meanwhile
// answer at once with what was there (nothing, the first time), and start no second index.
#[test]
fn a_packages_samples_are_indexed_once_in_the_background() {
    use crate::library::SampleIndex;
    let scratch = Scratch::new("once");
    let pack = scratch.0.join("loops");
    loops_pack(&pack);
    let index = std::sync::Arc::new(SampleIndex::default());
    let cache = scratch.0.join("cache");
    let (first, busy) = index.package("@alice/loops", &pack, Some(&cache));
    assert!(first.is_none() && busy, "nothing yet, and an index running");
    for _ in 0..5 {
        let (none, busy) = index.package("@alice/loops", &pack, Some(&cache));
        assert!(none.is_none() || !busy);
    }
    assert!(index.indexing() <= 1, "one index of the folder at a time");
    assert!(index.settled(std::time::Duration::from_secs(60)));
    let (made, busy) = index.package("@alice/loops", &pack, Some(&cache));
    let made = made.expect("indexed");
    assert!(!busy);
    assert_eq!((made.items.len(), made.measured), (3, 3), "each file measured once");
}

// LIB-5 (review D11): an index that panics still ends: its folder isn't "looking…" for good, and
// the next search starts another.
#[test]
fn a_panic_while_indexing_ends_the_index() {
    use crate::library::{Of, SampleIndex};
    let index = std::sync::Arc::new(SampleIndex::default());
    let of = Of::Folder(PathBuf::from("/nowhere/panics"));
    let (_, busy) = index.get(of.clone(), std::time::Duration::ZERO, |_| panic!("a crafted file"));
    assert!(busy || !index.indexing_of(&of));
    assert!(index.settled(std::time::Duration::from_secs(10)), "the count of indexes running came down");
    assert!(!index.indexing_of(&of));
    let (made, _) = index.get(of.clone(), std::time::Duration::ZERO, |_| m2_editor::library::samples::Indexed::default());
    assert!(made.is_none());
    assert!(index.settled(std::time::Duration::from_secs(10)));
    assert!(index.get(of, std::time::Duration::from_secs(60), |_| unreachable!()).0.is_some(), "another one ran");
}

// LIB-5 (review D16): removing a folder stops its index between two files, and what it made is
// never kept.
#[test]
fn removing_a_folder_stops_its_index() {
    use crate::library::{Of, SampleIndex};
    let index = std::sync::Arc::new(SampleIndex::default());
    let dir = PathBuf::from("/nowhere/huge");
    let (until, waiting) = (std::time::Instant::now() + std::time::Duration::from_secs(30), std::sync::Arc::new(()));
    let held = waiting.clone();
    index.get(Of::Folder(dir.clone()), std::time::Duration::ZERO, move |stop| {
        let _held = held;
        while !stop.load(std::sync::atomic::Ordering::Relaxed) && std::time::Instant::now() < until {
            std::thread::yield_now();
        }
        m2_editor::library::samples::Indexed { stopped: stop.load(std::sync::atomic::Ordering::Relaxed), ..Default::default() }
    });
    index.forget(&dir);
    assert!(index.settled(std::time::Duration::from_secs(20)), "it stopped");
    assert!(!index.indexing_of(&Of::Folder(dir)));
}

// LIB-7 (review D18): a place no clip can go (a lane that plays notes) is refused before the file
// is copied in: no file and no note are left.
#[test]
fn a_place_no_clip_can_go_is_refused_before_any_copy() {
    let scratch = Scratch::new("no-lane");
    let pack = scratch.0.join("loops");
    loops_pack(&pack);
    let song = Scratch::new("no-lane-song");
    song_with_pack(&song.0, &pack);
    let running = serving(Folders::default());
    let session = running.state.graph.open(&song.0).unwrap()["session"].as_u64().unwrap();
    let to = json!({"kind": "lane", "module": "kick", "lane": "hits", "beat": 0});
    let (status, why) = post(&running.url, "/api/library/sample", "t00ls", &json!({"session": session, "id": KICK, "to": to}));
    assert!(status >= 400 && why["error"].as_str().unwrap().contains("plays notes"), "{status} {why:#}");
    let left: Vec<_> = std::fs::read_dir(song.0.join("assets")).into_iter().flatten().flatten().map(|e| e.file_name()).collect();
    assert!(left.is_empty(), "nothing copied: {left:?}");
}

/// A pack in `dir`, `@alice/piano` 0.1.0, of one instrument written as data (PACK-13) that plays
/// one recording: what `m2 pack import-sfz` makes.
fn data_pack(dir: &Path) {
    std::fs::create_dir_all(dir.join("instruments")).unwrap();
    std::fs::write(
        dir.join("m2pkg.toml"),
        "[package]\nname = \"@alice/piano\"\nversion = \"0.1.0\"\nlicense = \"CC0-1.0\"\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.5\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("instruments/piano.toml"),
        "label = \"Felt piano\"\ncategory = \"Keys\"\nengine = \"std:sampler\"\nmode = \"gate\"\n\n[[region]]\nfile = \"samples/C4.wav\"\nroot = 60\nkeys = [0, 127]\n",
    )
    .unwrap();
    tone(&dir.join("samples/C4.wav"), 1, 0.5);
}

// LIB-1, LIB-3, REG-6 (gaps #47): an instrument written as data is in the library with no song
// open, and its preview renders, with no song open and with one that takes its pack.
#[test]
fn a_data_instrument_is_listed_and_previewed_with_or_without_a_song() {
    let scratch = Scratch::new("data-instrument");
    let store = scratch.0.join("Packs");
    let installed = store.join("@alice/piano/0.1.0");
    data_pack(&installed);
    m2_editor::packs::build(&installed).unwrap();
    let running = serving(Folders { packs: Some(store), previews: Some(scratch.0.join("previews")), ..Folders::default() });
    assert_eq!(found(&running.url, "felt piano")[0], "@alice/piano:piano");
    let (status, kind, body) = get(&running.url, "/api/library/preview?id=%40alice%2Fpiano%3Apiano");
    assert_eq!((status, kind.as_str()), (200, "audio/wav"), "{}", String::from_utf8_lossy(&body));

    let song = Scratch::new("data-instrument-song");
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song"), &song.0);
    let pack = scratch.0.join("piano");
    data_pack(&pack);
    let manifest = std::fs::read_to_string(song.0.join("music2.toml")).unwrap();
    let line = format!("\"@alice/piano\" = {{ path = {} }}\n", serde_json::to_string(&pack.to_string_lossy()).unwrap());
    std::fs::write(song.0.join("music2.toml"), manifest + &line).unwrap();
    running.state.graph.open(&song.0).unwrap();
    assert_eq!(found(&running.url, "felt piano")[0], "@alice/piano:piano");
    let (status, kind, body) = get(&running.url, "/api/library/preview?id=%40alice%2Fpiano%3Apiano");
    assert_eq!((status, kind.as_str()), (200, "audio/wav"), "with a song open: {}", String::from_utf8_lossy(&body));
}
