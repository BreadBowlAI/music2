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
    let running =
        start_with(ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "t00ls".into(), page_token: "t0k".into() }, Graph::silent())
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
    assert_eq!(grooves, ["swing", "shuffle", "laid-back", "pushed", "accents"]);
    assert_eq!(t["swings"], json!([0.5, 0.55, 0.6, 0.66, 0.71]));
    // Each groove the table names is one a gesture reads.
    for g in &grooves {
        let groove = if *g == "swing" { json!({"name": g, "amount": 0.6, "step": 0.25}) } else { json!({"name": g}) };
        assert!(serde_json::from_value::<m2_editor::grooves::Groove>(groove).is_ok(), "{g}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
