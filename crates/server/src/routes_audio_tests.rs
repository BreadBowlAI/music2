//! The audio routes through a running server: a waveform's peaks (Q9).

use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::Duration;

use crate::graph::Graph;
use crate::peaks::Peaks;
use crate::{ServeOptions, start_with};

const SONG: &str = "import { song, track } from '@m2/core';\n\
                    const vox = track('vox', ({ lane }) => lane.audio('clips'));\nexport default song(() => vox.out);\n";

/// A song with one audio track playing `assets/tone.wav`, in a folder of its own.
fn project(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("m2-routes-audio-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("data/vox")).unwrap();
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    let manifest = "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.8\"\n\"@m2/drums\" = \"1\"\n\"@m2/effects\" = \"1\"\n\"@m2/synths\" = \"1\"\n";
    std::fs::write(dir.join("music2.toml"), manifest).unwrap();
    std::fs::write(dir.join("song.ts"), SONG).unwrap();
    let spec = hound::WavSpec { channels: 1, sample_rate: 48_000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(dir.join("assets/tone.wav"), spec).unwrap();
    for i in 0..4_800 {
        w.write_sample(((i as f32 / 10.0).sin() * 16_000.0) as i16).unwrap();
    }
    w.finalize().unwrap();
    std::fs::write(dir.join("data/vox/clips.json"), r#"{"clips": [{"id": "a", "start": 0, "length": 4, "asset": "tone.wav"}]}"#).unwrap();
    dir
}

/// GET `path` with the token: the status and the body's bytes. Fails rather than wait past a minute.
fn get(url: &str, path: &str) -> (u16, Vec<u8>) {
    let addr = url.trim_start_matches("http://");
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
    let req = format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nx-m2-token: t0k\r\nConnection: close\r\n\r\n");
    s.write_all(req.as_bytes()).unwrap();
    let mut answer = vec![];
    s.read_to_end(&mut answer).expect("an answer, not a wait");
    let head_end = answer.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let head = String::from_utf8_lossy(&answer[..head_end]).to_string();
    let status = head[9..12].parse().unwrap();
    let mut body = answer[head_end + 4..].to_vec();
    if head.to_ascii_lowercase().contains("transfer-encoding: chunked") {
        body = unchunk(&body);
    }
    (status, body)
}

fn unchunk(mut b: &[u8]) -> Vec<u8> {
    let mut out = vec![];
    loop {
        let eol = b.windows(2).position(|w| w == b"\r\n").unwrap();
        let n = usize::from_str_radix(std::str::from_utf8(&b[..eol]).unwrap().trim(), 16).unwrap();
        if n == 0 {
            return out;
        }
        out.extend_from_slice(&b[eol + 2..eol + 2 + n]);
        b = &b[eol + 2 + n + 2..];
    }
}

// Q9: the peaks of an asset, by the open song's session; a name that isn't an asset's, or another
// session, is refused in words.
#[test]
fn peaks_of_an_asset() {
    let dir = project("peaks");
    let running = start_with(
        ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "t00ls".into(), page_token: "t0k".into() },
        Graph::silent().stored(),
    )
    .unwrap();
    let session = running.state.graph.open(&dir).unwrap()["session"].as_u64().unwrap();
    let (status, body) = get(&running.url, &format!("/api/graph/audio/peaks?session={session}&asset=tone.wav"));
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    let p = Peaks::read(&body).unwrap();
    assert_eq!((p.channels, p.rate, p.frames), (1, 48_000, 4_800));
    assert_eq!(p.data.iter().max(), Some(&62), "the tone at half level: 16,000 of 32,768");
    let (status, body) = get(&running.url, &format!("/api/graph/audio/peaks?session={session}&asset=..%2Fsong.ts"));
    assert_eq!(status, 400, "{}", String::from_utf8_lossy(&body));
    let (status, body) = get(&running.url, &format!("/api/graph/audio/peaks?session={}&asset=tone.wav", session + 1));
    assert!(status == 400 && String::from_utf8_lossy(&body).contains("changed"), "{status} {}", String::from_utf8_lossy(&body));
    let (status, body) = get(&running.url, &format!("/api/graph/audio/peaks?session={session}&asset=gone.wav"));
    assert!(status == 400 && String::from_utf8_lossy(&body).contains("gone.wav"), "{status} {}", String::from_utf8_lossy(&body));
    let _ = std::fs::remove_dir_all(&dir);
}

// Guard: the peaks answer while the session is busy (held as a long compile would hold it).
#[test]
fn peaks_never_wait_for_the_actor() {
    let dir = project("busy");
    let running = start_with(
        ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "t00ls".into(), page_token: "t0k".into() },
        Graph::silent().stored(),
    )
    .unwrap();
    let session = running.state.graph.open(&dir).unwrap()["session"].as_u64().unwrap();
    let held = running.state.graph.hold();
    let (status, body) = get(&running.url, &format!("/api/graph/audio/peaks?session={session}&asset=tone.wav"));
    drop(held);
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    assert_eq!(Peaks::read(&body).unwrap().frames, 4_800);
    let _ = std::fs::remove_dir_all(&dir);
}

/// POST `body` as `kind` to `path` with the token: the status and the JSON answer.
fn post(url: &str, path: &str, kind: &str, body: &[u8]) -> (u16, serde_json::Value) {
    let addr = url.trim_start_matches("http://");
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
    let head = format!(
        "POST {path} HTTP/1.1\r\nHost: {addr}\r\nx-m2-token: t0k\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    s.write_all(head.as_bytes()).unwrap();
    s.write_all(body).unwrap();
    let mut answer = vec![];
    s.read_to_end(&mut answer).unwrap();
    let text = String::from_utf8_lossy(&answer).to_string();
    let status = text[9..12].parse().unwrap();
    let json =
        text.split_once("\r\n\r\n").map_or(serde_json::Value::Null, |(_, b)| serde_json::from_str(b).unwrap_or(serde_json::Value::Null));
    (status, json)
}

fn clips_of(dir: &std::path::Path) -> Vec<serde_json::Value> {
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("data/vox/clips.json")).unwrap()).unwrap();
    v["clips"].as_array().unwrap().clone()
}

// A2: a browser uploads files back to back in one body, an app names paths: each is one undo step
// that lands its clips one after another; the answer names the step and the clips made.
#[test]
fn an_upload_and_paths_import_through_the_route() {
    let dir = project("import");
    let running = start_with(
        ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "t00ls".into(), page_token: "t0k".into() },
        Graph::silent().stored(),
    )
    .unwrap();
    let session = running.state.graph.open(&dir).unwrap()["session"].as_u64().unwrap();
    let tone = std::fs::read(dir.join("assets/tone.wav")).unwrap();
    let mut other = tone.clone();
    *other.last_mut().unwrap() ^= 1;
    let body = [tone.clone(), other.clone()].concat();
    let files = format!(r#"[{{"name":"a.wav","size":{}}},{{"name":"b.wav","size":{}}}]"#, tone.len(), other.len());
    let to = r#"{"kind":"lane","module":"vox","lane":"clips","beat":8}"#;
    let enc = |s: &str| s.bytes().map(|b| format!("%{b:02X}")).collect::<String>();
    let path = format!("/api/graph/audio/import?session={session}&files={}&to={}", enc(&files), enc(to));
    let (status, done) = post(&running.url, &path, "application/octet-stream", &body);
    assert_eq!(status, 200, "{done:#}");
    assert_eq!((done["name"].as_str(), done["made"].as_array().map(Vec::len)), (Some("importing 2 files"), Some(2)), "{done:#}");
    // tone.wav's bytes are there already: used again. b.wav is new.
    assert_eq!(done["assets"], serde_json::json!(["tone.wav", "b.wav"]));
    let starts: Vec<f64> =
        clips_of(&dir).iter().filter(|c| c["start"].as_f64() >= Some(8.0)).map(|c| c["start"].as_f64().unwrap()).collect();
    // 0.1 s at 120 BPM is 0.2 beats.
    assert_eq!(starts.len(), 2);
    assert!((starts[0] - 8.0).abs() < 1e-9 && (starts[1] - 8.2).abs() < 1e-9, "{starts:?}");
    // The app's paths.
    let outside = dir.join("outside.wav");
    std::fs::write(&outside, &other).unwrap();
    let json =
        serde_json::json!({"session": session, "paths": [outside], "to": {"kind": "lane", "module": "vox", "lane": "clips", "beat": 12}});
    let (status, done) = post(&running.url, "/api/graph/audio/import", "application/json", json.to_string().as_bytes());
    assert_eq!((status, &done["assets"]), (200, &serde_json::json!(["b.wav"])), "{done:#}");
    // A body cut short is refused.
    let (status, why) = post(&running.url, &path, "application/octet-stream", &body[..10]);
    assert!(status == 400 && why["error"].as_str().unwrap().contains("cut short"), "{why:#}");
    let _ = std::fs::remove_dir_all(&dir);
}

// WRITE-17: a read-only window's import answers 423 in WRITE-18's words.
#[test]
fn a_read_only_window_cant_import() {
    let dir = project("ro");
    let held = m2_session::lock::acquire(&dir, "m2", m2_session::lock::RETRY).unwrap();
    let running = start_with(
        ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "t00ls".into(), page_token: "t0k".into() },
        Graph::silent().stored(),
    )
    .unwrap();
    let session = running.state.graph.open(&dir).unwrap()["session"].as_u64().unwrap();
    let json = serde_json::json!({"session": session, "paths": [dir.join("assets/tone.wav")], "to": {"kind": "below", "beat": 0}});
    let (status, why) = post(&running.url, "/api/graph/audio/import", "application/json", json.to_string().as_bytes());
    assert_eq!(status, 423, "{why:#}");
    assert!(why["error"].as_str().unwrap().contains("nothing was changed"), "{why:#}");
    // Peaks are a read: still answered.
    assert_eq!(get(&running.url, &format!("/api/graph/audio/peaks?session={session}&asset=tone.wav")).0, 200);
    drop(held);
    let _ = std::fs::remove_dir_all(&dir);
}

// Q8: + Track ▸ Audio track adds one, named "Audio", as one step named "adding Audio"; a read-only
// window can't.
#[test]
fn an_audio_track_through_the_route() {
    // The first song: a mixer to give the track a channel.
    let dir = std::env::temp_dir().join(format!("m2-routes-audio-track-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song"), &dir);
    let running = start_with(
        ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "t00ls".into(), page_token: "t0k".into() },
        Graph::silent().stored(),
    )
    .unwrap();
    let session = running.state.graph.open(&dir).unwrap()["session"].as_u64().unwrap();
    let (status, done) =
        post(&running.url, "/api/graph/audio/track", "application/json", serde_json::json!({"session": session}).to_string().as_bytes());
    assert_eq!((status, &done["module"], &done["label"]), (200, &serde_json::json!("audio"), &serde_json::json!("Audio")), "{done:#}");
    assert!(std::fs::read_to_string(dir.join("tracks/audio.ts")).unwrap().contains("lane.audio(\"clips\")"));
    let state = running.state.graph.state().unwrap();
    assert_eq!(state["undo_name"]["name"], "adding Audio", "{:#}", state["undo_name"]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The sampler card's node in the open song's view: its call's id and what its recording, `sample`,
/// plays (CARD-1).
fn sampler_node(graph: &Graph) -> (u64, String, serde_json::Value) {
    let view = graph.state().unwrap()["view"].clone();
    let nodes = view["modules"].as_array().unwrap().iter().flat_map(|m| m["nodes"].as_array().unwrap().clone()).collect::<Vec<_>>();
    let node = nodes.into_iter().find(|n| n["component"] == "std:sampler").expect("a sampler's card");
    let sample = node["settings"].as_array().into_iter().flatten().find(|x| x["slot"] == "sample").expect("its recording");
    assert_eq!((&sample["kind"], &sample.get("readonly")), (&serde_json::json!("recording"), &None), "{sample:#}");
    (view["revision"].as_u64().unwrap(), node["id"].as_str().unwrap().to_string(), sample["value"].clone())
}

// Track ab: a WAV dropped on a sampler's card comes into assets/ and makes one Tx, one undo step
// named "setting Sampler to kick.wav"; a file that isn't a WAV is refused in words, adding nothing.
#[test]
fn a_wav_dropped_on_a_sampler_sets_its_file_through_the_route() {
    let dir = project("sample");
    let song = "import { song, track } from '@m2/core';\nimport { sampler } from 'std/sampler';\n\
                const s = track('s', ({ lane }) => sampler(lane.notes('notes')));\nexport default song(() => s.out);\n";
    std::fs::write(dir.join("song.ts"), song).unwrap();
    let running = start_with(
        ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "t00ls".into(), page_token: "t0k".into() },
        Graph::silent().stored(),
    )
    .unwrap();
    let session = running.state.graph.open(&dir).unwrap()["session"].as_u64().unwrap();
    let (at, call, card) = sampler_node(&running.state.graph);
    assert_eq!(card, serde_json::Value::Null, "no file yet");
    let enc = |s: &str| s.bytes().map(|b| format!("%{b:02X}")).collect::<String>();
    let mut kick = std::fs::read(dir.join("assets/tone.wav")).unwrap();
    *kick.last_mut().unwrap() ^= 1;
    let path = format!("/api/graph/sample?session={session}&at={at}&call={}&name=kick.wav", enc(&call));
    let (status, done) = post(&running.url, &path, "application/octet-stream", &kick);
    assert_eq!(status, 200, "{done:#}");
    assert_eq!((done["asset"].as_str(), done["name"].as_str()), (Some("kick.wav"), Some("setting Sampler to kick.wav")), "{done:#}");
    assert!(std::fs::read_to_string(dir.join("song.ts")).unwrap().contains("sampler(lane.notes('notes'), { sample: \"kick.wav\" })"));
    assert_eq!(std::fs::read(dir.join("assets/kick.wav")).unwrap(), kick);
    let state = running.state.graph.state().unwrap();
    assert_eq!(state["undo_name"]["name"], "setting Sampler to kick.wav", "{:#}", state["undo_name"]);
    let (at, call, card) = sampler_node(&running.state.graph);
    assert_eq!(card, serde_json::json!("kick.wav"));
    // Not a WAV: refused in words; nothing is copied or written.
    let path = format!("/api/graph/sample?session={session}&at={at}&call={}&name=notes.txt", enc(&call));
    let (status, why) = post(&running.url, &path, "application/octet-stream", b"hello, not a sound");
    assert!(status == 400 && why["error"].as_str().unwrap().contains("notes.txt isn't a WAV or FLAC file"), "{status} {why:#}");
    assert!(!dir.join("assets/notes.txt").exists());
    assert!(std::fs::read_to_string(dir.join("song.ts")).unwrap().contains("{ sample: \"kick.wav\" }"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Setting `slot` of the card whose title is `title` in the open song's view, as it is sent.
fn setting_of(graph: &Graph, title: &str, slot: &str) -> (u64, serde_json::Value) {
    let view = graph.state().unwrap()["view"].clone();
    let nodes = view["modules"].as_array().unwrap().iter().flat_map(|m| m["nodes"].as_array().unwrap().clone()).collect::<Vec<_>>();
    let node = nodes.into_iter().find(|n| n["title"] == title).unwrap_or_else(|| panic!("no {title}"));
    let found = node["settings"].as_array().into_iter().flatten().find(|x| x["slot"] == slot).cloned();
    (view["revision"].as_u64().unwrap(), found.unwrap_or_else(|| panic!("no {slot} on {title}: {node:#}")))
}

// CARD-4, CARD-5 over HTTP: `POST /api/graph/set` writes a setting's value (a word, a boolean) as
// one undo step, and refuses a fraction for a whole number in words; `POST /api/graph/sample` with
// a `setting` brings a file in and makes it that recording, named as the drop names it.
#[test]
fn a_setting_and_a_recording_change_through_the_routes() {
    let dir = project("settings");
    let song = "import { component, port, setting, song, track } from '@m2/core';\nimport { lfo } from 'std/control';\n\
                import { vca } from 'std/dsp';\nimport { sampler } from 'std/sampler';\n\
                const box = component({ id: 'box', label: 'Box', inputs: { notes: port.notes() },\n\
                  settings: { held: setting.bool(false), voices: setting.int(4), sample: setting.asset().optional() }, output: port.audio() },\n\
                  (notes, p) => sampler(notes, { sample: p.sample, mode: p.held ? 'gate' : 'oneshot', voices: p.voices }));\n\
                const s = track('s', ({ lane }) => box(lane.notes('notes')));\n\
                export default song(({ time }) => vca(s.out, lfo(time.beats)));\n";
    std::fs::write(dir.join("song.ts"), song).unwrap();
    let running = start_with(
        ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "t00ls".into(), page_token: "t0k".into() },
        Graph::silent().stored(),
    )
    .unwrap();
    let session = running.state.graph.open(&dir).unwrap()["session"].as_u64().unwrap();
    let set = |id: &serde_json::Value, at: u64, value: serde_json::Value| {
        let body = serde_json::json!({"session": session, "revision": at, "id": id, "gesture": "set", "value": value});
        post(&running.url, "/api/graph/set", "application/json", body.to_string().as_bytes())
    };
    let (at, shape) = setting_of(&running.state.graph, "LFO", "shape");
    assert_eq!(
        (&shape["kind"], &shape["value"], &shape["written"]),
        (&serde_json::json!("choice"), &serde_json::json!("sine"), &serde_json::json!(false))
    );
    let (status, done) = set(&shape["id"], at, serde_json::json!("square"));
    assert_eq!(status, 200, "{done:#}");
    assert!(std::fs::read_to_string(dir.join("song.ts")).unwrap().contains("lfo(time.beats, { shape: \"square\" })"));
    assert!(done["step"].is_number(), "one undo step: {done:#}");
    let (at, held) = setting_of(&running.state.graph, "Box", "held");
    let (status, done) = set(&held["id"], at, serde_json::json!(true));
    assert_eq!(status, 200, "{done:#}");
    assert!(std::fs::read_to_string(dir.join("song.ts")).unwrap().contains("box(lane.notes('notes'), { held: true })"));
    let (at, voices) = setting_of(&running.state.graph, "Box", "voices");
    let before = std::fs::read_to_string(dir.join("song.ts")).unwrap();
    let (status, why) = set(&voices["id"], at, serde_json::json!(2.5));
    assert!(status == 400 && why["error"].as_str().unwrap().contains("voices is a whole number, not 2.5"), "{status} {why:#}");
    assert_eq!(std::fs::read_to_string(dir.join("song.ts")).unwrap(), before, "nothing written");
    // A file dropped on Box's recording.
    let (at, sample) = setting_of(&running.state.graph, "Box", "sample");
    let call = sample["id"].as_str().unwrap().rsplit_once("/@").unwrap().0.to_string();
    let file = dir.join("assets/tone.wav").to_string_lossy().to_string();
    let body = serde_json::json!({"session": session, "at": at, "call": call, "setting": "sample", "file": file});
    let (status, done) = post(&running.url, "/api/graph/sample", "application/json", body.to_string().as_bytes());
    assert_eq!(status, 200, "{done:#}");
    assert_eq!((done["asset"].as_str(), done["name"].as_str()), (Some("tone.wav"), Some("setting Box to tone.wav")), "{done:#}");
    assert!(std::fs::read_to_string(dir.join("song.ts")).unwrap().contains("{ held: true, sample: \"tone.wav\" }"));
    assert_eq!(setting_of(&running.state.graph, "Box", "sample").1["value"], "tone.wav");
    // CARD-5: a drop the call can't take is refused before the file is copied in: not a recording.
    let elsewhere = dir.with_extension("elsewhere.wav");
    let mut other = std::fs::read(dir.join("assets/tone.wav")).unwrap();
    *other.last_mut().unwrap() ^= 1;
    std::fs::write(&elsewhere, other).unwrap();
    let files =
        || std::fs::read_dir(dir.join("assets")).unwrap().flatten().map(|e| e.file_name()).collect::<std::collections::BTreeSet<_>>();
    let before = files();
    let (at, _) = setting_of(&running.state.graph, "Box", "voices");
    let body = serde_json::json!({"session": session, "at": at, "call": call, "setting": "voices", "file": elsewhere.to_string_lossy()});
    let (status, why) = post(&running.url, "/api/graph/sample", "application/json", body.to_string().as_bytes());
    assert!(status == 400 && why["error"].as_str().unwrap().contains("Box's voices isn't a recording"), "{status} {why:#}");
    assert_eq!(files(), before, "nothing was copied into assets/");
    let _ = std::fs::remove_file(&elsewhere);
    let _ = std::fs::remove_dir_all(&dir);
}

pub(crate) fn copy(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let (f, t) = (e.path(), to.join(e.file_name()));
        if f.is_dir() { copy(&f, &t) } else { std::fs::copy(&f, &t).map(drop).unwrap() }
    }
}
