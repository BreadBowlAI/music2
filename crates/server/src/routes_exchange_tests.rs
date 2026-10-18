//! MIDI files through a running server (Q37, Q38): the song and one clip written to a picked file,
//! a new name never over another file, and a clip that isn't there said in words.

use std::io::Read;

use serde_json::{Value, json};

use super::*;
use crate::graph::Graph;
use crate::{ServeOptions, start_with};

const SONG: &str = "import { song, track } from '@m2/core';\nimport { bass } from 'std/instruments';\n\
                    const b = track('bass', { name: 'Bass' }, ({ lane }) => bass(lane.notes('notes')));\nexport default song(() => b.out);\n";
const NOTES: &str = r#"{"clips": [{"id": "a", "start": 4, "length": 4, "notes": [{"pitch": "E2", "start": 1, "length": 1}]}]}"#;

/// A project of [`SONG`] in a folder of its own, named `name`.
fn project(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("m2-routes-exchange-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("data/bass")).unwrap();
    let manifest = "[project]\nname = \"groove\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.4\"\n";
    std::fs::write(dir.join("music2.toml"), manifest).unwrap();
    std::fs::write(dir.join("song.ts"), SONG).unwrap();
    std::fs::write(dir.join("data/bass/notes.json"), NOTES).unwrap();
    dir
}

/// `POST path` with `body` and the token: the status and the JSON answer.
fn post(url: &str, path: &str, body: &Value) -> (u16, Value) {
    let addr = url.trim_start_matches("http://");
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    let body = body.to_string();
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: {addr}\r\nx-m2-token: t0k\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).unwrap();
    let mut answer = String::new();
    s.read_to_string(&mut answer).unwrap();
    let status = answer[9..12].parse().unwrap();
    let json = answer.split_once("\r\n\r\n").map_or(Value::Null, |(_, b)| serde_json::from_str(b).unwrap_or(Value::Null));
    (status, json)
}

/// The notes of a written file's tracks after the first: (key, start, end) in ticks.
fn notes(path: &Path) -> Vec<(u8, u64, u64)> {
    let smf = m2_editor::smf::read(&std::fs::read(path).unwrap()).unwrap();
    smf.tracks[1..].iter().flat_map(|t| t.notes()).map(|n| (n.key, n.start, n.end)).collect()
}

// Q37, Q38: the song to a picked file (".mid" added, replaced when there), and a clip from its own
// start; a clip that isn't there is refused in words.
#[test]
fn the_song_and_a_clip_are_written_where_picked() {
    let dir = project("picked");
    let running = start_with(ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "t0k".into() }, Graph::silent()).unwrap();
    let session = running.state.graph.open(&dir).unwrap()["session"].as_u64().unwrap();
    let url = &running.url;
    let out = dir.join("out");
    std::fs::write(dir.join("out.mid"), "old").unwrap();
    let (status, done) = post(url, "/api/graph/midifile/export", &json!({"session": session, "path": out}));
    assert_eq!(status, 200, "{done:#}");
    let path = dir.join("out.mid");
    assert_eq!(done["path"], json!(path));
    // E2 a beat into a clip at beat 4: beat 5 of the song, 960 ticks a beat.
    assert_eq!(notes(&path), [(40, 4800, 5760)]);
    let clip = json!({"module": "bass", "lane": "notes", "id": "a"});
    let (status, done) = post(url, "/api/graph/midifile/export", &json!({"session": session, "path": dir.join("clip.midi"), "clip": clip}));
    assert_eq!(status, 200, "{done:#}");
    assert_eq!(notes(&dir.join("clip.midi")), [(40, 960, 1920)]);
    let gone = json!({"module": "bass", "lane": "notes", "id": "zzzz"});
    let (status, why) = post(url, "/api/graph/midifile/export", &json!({"session": session, "path": dir.join("x.mid"), "clip": gone}));
    assert_eq!((status, why["error"].as_str()), (400, Some("That clip isn't there any more.")));
    let (status, why) = post(url, "/api/graph/midifile/export", &json!({"session": session + 1, "path": dir.join("x.mid")}));
    assert!(status == 400 && why["error"].as_str().unwrap().contains("changed"), "{why:#}");
    assert!(!dir.join("x.mid").exists());
    // Nothing left beside the files.
    let parts: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".part")).collect();
    assert!(parts.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

// Q38: a new file in Exports takes the first free name, and never replaces a file, not even one
// made a moment before it is named.
#[test]
fn a_new_midi_file_never_replaces_one() {
    let dir = std::env::temp_dir().join(format!("m2-routes-exchange-fresh-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("groove Bass.mid"), "mine").unwrap();
    let first = numbered(&dir, "groove Bass").find(|p| p.symlink_metadata().is_err()).unwrap();
    assert_eq!(first, dir.join("groove Bass 2.mid"));
    // Another file takes that name meanwhile: it goes to the next.
    std::fs::write(&first, "theirs").unwrap();
    let written = write_file(&first, b"MThd", Some("groove Bass")).unwrap();
    assert_eq!(written, dir.join("groove Bass 3.mid"));
    assert_eq!(std::fs::read_to_string(dir.join("groove Bass.mid")).unwrap(), "mine");
    assert_eq!(std::fs::read_to_string(&first).unwrap(), "theirs");
    assert_eq!(std::fs::read(&written).unwrap(), b"MThd");
    // A name with a slash is made safe.
    assert_eq!(numbered(&dir, "a/b").next().unwrap(), dir.join("a-b.mid"));
    assert!(picked(std::path::PathBuf::from("rel.mid")).is_err());
    assert!(picked(dir.join("no-such/x.mid")).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A one-track MIDI file, 96 ticks a beat: E2 for a beat, then A2 for two, from beat 4.
fn riff() -> Vec<u8> {
    use m2_editor::smf::{Event, Smf, SmfTrack};
    let on = |key| Event::NoteOn { channel: 0, key, velocity: 100 };
    let off = |key| Event::NoteOff { channel: 0, key };
    let events = vec![(0, on(40)), (96, off(40)), (384, on(45)), (576, off(45))];
    m2_editor::smf::write(&Smf { format: 1, ppq: 96, tracks: vec![SmfTrack { name: Some("Riff".into()), events, end: 576 }] })
}

/// `POST path?query` with raw `bytes` and the token: the status and the JSON answer.
fn upload(url: &str, path: &str, bytes: &[u8]) -> (u16, Value) {
    let addr = url.trim_start_matches("http://");
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    let head = format!(
        "POST {path} HTTP/1.1\r\nHost: {addr}\r\nx-m2-token: t0k\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        bytes.len()
    );
    s.write_all(head.as_bytes()).unwrap();
    s.write_all(bytes).unwrap();
    let mut answer = String::new();
    s.read_to_string(&mut answer).unwrap();
    let status = answer[9..12].parse().unwrap();
    let json = answer.split_once("\r\n\r\n").map_or(Value::Null, |(_, b)| serde_json::from_str(b).unwrap_or(Value::Null));
    (status, json)
}

// C2, Q36, D11: a one-part MIDI file into Bass's notes lane, as a path (the app) and as bytes (a
// browser): one clip, one undo step named on the server; what isn't a MIDI file is refused in words.
#[test]
fn a_midi_file_comes_into_a_notes_lane_as_one_undo_step() {
    let dir = project("import");
    let running = start_with(ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "t0k".into() }, Graph::silent()).unwrap();
    let session = running.state.graph.open(&dir).unwrap()["session"].as_u64().unwrap();
    let url = &running.url;
    std::fs::write(dir.join("riff.mid"), riff()).unwrap();
    let to = json!({"kind": "lane", "module": "bass", "lane": "notes", "beat": 8});
    let (status, done) = post(url, "/api/graph/midifile/import", &json!({"session": session, "path": dir.join("riff.mid"), "to": to}));
    assert_eq!(status, 200, "{done:#}");
    assert_eq!(
        (done["name"].as_str(), done["undo_name"]["name"].as_str()),
        (Some("importing riff.mid"), Some("importing riff.mid")),
        "{done:#}"
    );
    let id = done["made"][0]["id"].as_str().unwrap().to_string();
    let lane: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("data/bass/notes.json")).unwrap()).unwrap();
    let clip = lane["clips"].as_array().unwrap().iter().find(|c| c["id"] == id.as_str()).unwrap().clone();
    assert_eq!(
        (clip["start"].as_f64(), clip["length"].as_f64(), clip["notes"].as_array().map(Vec::len)),
        (Some(8.0), Some(8.0), Some(2)),
        "{clip}"
    );
    // Bytes, as a browser sends them.
    let q = format!("/api/graph/midifile/import?session={session}&name=riff%202.mid&to={}", urlencode(&to.to_string()));
    let (status, done) = upload(url, &q, &riff());
    assert_eq!((status, done["name"].as_str()), (200, Some("importing riff 2.mid")), "{done:#}");
    // Not a MIDI file: words, nothing written.
    let before = std::fs::read_to_string(dir.join("data/bass/notes.json")).unwrap();
    let q = format!("/api/graph/midifile/import?session={session}&name=notes.mid&to={}", urlencode(&to.to_string()));
    let (status, why) = upload(url, &q, b"hello, this is text");
    assert_eq!((status, why["error"].as_str()), (400, Some("notes.mid isn't a MIDI file: it doesn't start with MThd.")));
    assert_eq!(std::fs::read_to_string(dir.join("data/bass/notes.json")).unwrap(), before);
    let _ = std::fs::remove_dir_all(&dir);
}

/// groove.mid, 96 ticks a beat at 128 BPM: Drums (channel 10) and a bass line (channel 1).
fn groove() -> Vec<u8> {
    use m2_editor::smf::{Event, Smf, SmfTrack};
    let note = |channel, key, at: u64| [(at, Event::NoteOn { channel, key, velocity: 100 }), (at + 48, Event::NoteOff { channel, key })];
    let drums = [note(9, 36, 0), note(9, 38, 96)].concat();
    let bass = [note(0, 40, 0), note(0, 43, 192)].concat();
    let track = |name: &str, events: Vec<(u64, Event)>| SmfTrack { name: Some(name.into()), end: events.last().map_or(0, |e| e.0), events };
    let tracks = vec![track("Tempo", vec![(0, Event::Tempo(468_750))]), track("Drums", drums), track("Low", bass)];
    m2_editor::smf::write(&Smf { format: 1, ppq: 96, tracks })
}

// C2b, Q35, Q36: a file of two parts below the tracks asks the tempo question first, writing
// nothing; answered Use, it is two new tracks and the file's tempo from the bar, one undo step
// named on the server. As bytes (a browser) with Keep onto a track's row, and onto a notes lane
// (two parts: new tracks after it, not into it).
#[test]
fn an_import_below_asks_then_writes() {
    let dir = project("below");
    // New tracks need a mixer to go into.
    let mixed = SONG.replace("export default song(() => b.out);", "import { master, mixer } from 'std/mix';\nexport default song(() => master(mixer({ bass: b.out })));");
    std::fs::write(dir.join("song.ts"), mixed).unwrap();
    let running = start_with(ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "t0k".into() }, Graph::silent()).unwrap();
    let session = running.state.graph.open(&dir).unwrap()["session"].as_u64().unwrap();
    let url = &running.url;
    std::fs::write(dir.join("groove.mid"), groove()).unwrap();
    let song = std::fs::read_to_string(dir.join("song.ts")).unwrap();
    let below = json!({"kind": "below", "beat": 4});
    let (status, asks) = post(url, "/api/graph/midifile/import", &json!({"session": session, "path": dir.join("groove.mid"), "to": below}));
    assert_eq!((status, &asks), (200, &json!({"asks": {"file_bpm": 128.0, "song_bpm": 120.0}})));
    assert_eq!(std::fs::read_to_string(dir.join("song.ts")).unwrap(), song, "nothing written");
    let req = json!({"session": session, "path": dir.join("groove.mid"), "to": below, "tempo": "use"});
    let (status, done) = post(url, "/api/graph/midifile/import", &req);
    assert_eq!(status, 200, "{done:#}");
    assert_eq!(
        (done["name"].as_str(), done["undo_name"]["name"].as_str()),
        (Some("importing groove.mid"), Some("importing groove.mid")),
        "{done:#}"
    );
    let labels: Vec<&str> = done["tracks"].as_array().unwrap().iter().filter_map(|t| t["label"].as_str()).collect();
    assert_eq!(labels, ["Drums", "Low"]);
    assert_eq!(done["made"].as_array().map(Vec::len), Some(2));
    let arrangement = m2_song::Arrangement::read(&std::fs::read_to_string(dir.join("data/arrangement.json")).unwrap()).unwrap();
    let tempo: Vec<(f64, f64)> = arrangement.tempo.iter().map(|t| (t.beat, t.bpm)).collect();
    assert_eq!(tempo, [(0.0, 120.0), (4.0, 128.0)], "the file's tempo from bar 2");
    // As bytes, Keep, onto Bass's row: the tempo stays.
    let q =
        |to: &Value| format!("/api/graph/midifile/import?session={session}&name=groove.mid&tempo=keep&to={}", urlencode(&to.to_string()));
    let (status, done) = upload(url, &q(&json!({"kind": "track", "module": "bass", "beat": 0})), &groove());
    assert_eq!((status, done["tracks"].as_array().map(Vec::len)), (200, Some(2)), "{done:#}");
    // Onto Bass's notes lane: two parts are new tracks.
    let (status, done) = upload(url, &q(&json!({"kind": "lane", "module": "bass", "lane": "notes", "beat": 0})), &groove());
    assert_eq!((status, done["tracks"].as_array().map(Vec::len)), (200, Some(2)), "{done:#}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `text` for a query string.
fn urlencode(text: &str) -> String {
    text.bytes().map(|b| if b.is_ascii_alphanumeric() { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}
