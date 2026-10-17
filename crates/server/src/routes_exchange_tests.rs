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
    let manifest = "[project]\nname = \"groove\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.2\"\n";
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
