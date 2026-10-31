//! `GET /api/theory` through a running server: the one table of scales (NOTE-1), the chord
//! stamps' kinds (NOTE-4) and the grooves (NOTE-7), with no song open.

use std::io::{Read, Write};

use serde_json::{Value, json};

use crate::graph::Graph;
use crate::{ServeOptions, start_with};

// NOTE-1: an agent (and the key field) reads what music2 knows from one route, as the table has it.
#[test]
fn the_theory_route_lists_the_tables() {
    let dir = std::env::temp_dir().join(format!("m2-routes-notes-{}", std::process::id()));
    let running = start_with(
        ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "t00ls".into(), page_token: "t0k".into() },
        Graph::silent().stored(),
    )
    .unwrap();
    let addr = running.url.trim_start_matches("http://").to_string();
    let get = |token: Option<&str>| -> (u16, Value) {
        let mut s = std::net::TcpStream::connect(&addr).unwrap();
        let auth = token.map_or(String::new(), |t| format!("x-m2-token: {t}\r\n"));
        s.write_all(format!("GET /api/theory HTTP/1.1\r\nHost: {addr}\r\n{auth}Connection: close\r\n\r\n").as_bytes()).unwrap();
        let mut answer = String::new();
        s.read_to_string(&mut answer).unwrap();
        let status = answer[9..12].parse().unwrap();
        (status, answer.split_once("\r\n\r\n").map_or(Value::Null, |(_, b)| serde_json::from_str(b).unwrap_or(Value::Null)))
    };
    assert_eq!(get(None).0, 401, "behind the token");
    let (status, t) = get(Some("t00ls"));
    assert_eq!(status, 200, "an agent reads it, with no song open: {t}");
    let names: Vec<&str> = t["scales"].as_array().unwrap().iter().map(|s| s["name"].as_str().unwrap()).collect();
    assert_eq!(names, m2_editor::scales::TABLE.iter().map(|r| r.name).collect::<Vec<_>>());
    assert_eq!(names.len(), 13);
    assert_eq!(
        t["scales"][7],
        json!({"name": "harmonic-minor", "label": "Harmonic minor", "words": "Minor with a raised seventh: classical, Middle Eastern", "steps": [0, 2, 3, 5, 7, 8, 11]})
    );
    let chords: Vec<(&str, u64)> =
        t["chords"].as_array().unwrap().iter().map(|c| (c["name"].as_str().unwrap(), c["inversions"].as_u64().unwrap())).collect();
    assert_eq!(chords, [("triad", 3), ("seventh", 4), ("sus2", 3), ("sus4", 3), ("add9", 3)]);
    let grooves: Vec<&str> = t["grooves"].as_array().unwrap().iter().map(|g| g["name"].as_str().unwrap()).collect();
    assert_eq!(grooves, ["swing", "laid-back", "pushed", "accents"]);
    assert_eq!(t["swings"], json!([0.5, 0.55, 0.6, 0.66, 0.71]));
    // Each groove says what a request writes for it (review N15), and that is one a gesture reads.
    assert_eq!(t["grooves"][0]["example"], json!({"name": "swing", "amount": 0.6, "step": "sixteenths"}));
    for g in t["grooves"].as_array().unwrap() {
        assert!(serde_json::from_value::<m2_editor::grooves::Groove>(g["example"].clone()).is_ok(), "{g}");
        assert_eq!(g["example"]["name"], g["name"]);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// NOTE-1, review N13: a key that doesn't read is refused as an edit, in the app's JSON refusal with
// the words that say which scales there are; nothing is written. A key in any spelling is written.
#[test]
fn a_key_that_doesnt_read_is_refused_in_words() {
    let dir = std::env::temp_dir().join(format!("m2-routes-notes-key-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let manifest = "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.9\"\n";
    std::fs::write(dir.join("music2.toml"), manifest).unwrap();
    std::fs::write(
        dir.join("song.ts"),
        "import { song, track } from '@m2/core';\nimport { osc } from 'std/dsp';\n\
         const kick = track('kick', () => osc.sine(55));\nexport default song(() => kick.out);\n",
    )
    .unwrap();
    let running = start_with(
        ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "t00ls".into(), page_token: "t0k".into() },
        Graph::silent().stored(),
    )
    .unwrap();
    let session = running.state.graph.open(&dir).unwrap()["session"].as_u64().unwrap();
    let post = |key: Value| {
        crate::routes_8b::tests::post(&running.url, "/api/graph/song-key", Some("t0k"), &json!({"session": session, "key": key}))
    };
    let (status, answer) = post(json!({"root": "A", "scale": "bebop"}));
    assert_eq!(status, 400, "{answer}");
    assert_eq!(
        answer["error"],
        "edit rejected: a key's scale is one of major, minor, dorian, phrygian, lydian, mixolydian, locrian, harmonic-minor, \
         melodic-minor, major-pentatonic, minor-pentatonic, blues, chromatic, not bebop"
    );
    let (status, answer) = post(json!({"root": "H", "scale": "minor"}));
    assert!(status == 400 && answer["error"].as_str().unwrap().contains("H isn't a key's root"), "{answer}");
    assert!(!dir.join("layout.json").exists(), "nothing written");
    let (status, answer) = post(json!({"root": "d♭", "scale": "Major Pentatonic"}));
    assert_eq!((status, &answer["after"]), (200, &json!("C♯ major pentatonic")), "{answer}");
    assert!(
        std::fs::read_to_string(dir.join("layout.json"))
            .unwrap()
            .contains("\"key\": { \"root\": \"C#\", \"scale\": \"major-pentatonic\" }")
    );
    let _ = std::fs::remove_dir_all(&dir);
}
