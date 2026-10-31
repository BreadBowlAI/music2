//! 8b's routes through a running server: the token check covers them, and mute and solo answer
//! with the overlay state, in words when refused; the server's own thread saves them. Audition
//! over HTTP and over a window's WebSocket.

use std::io::{Read, Write};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::graph::Graph;
use crate::{ServeOptions, start_with};

/// `POST path` with `body`, with the token or not: the status and the JSON answer.
pub(crate) fn post(url: &str, path: &str, token: Option<&str>, body: &Value) -> (u16, Value) {
    let addr = url.trim_start_matches("http://");
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    let body = body.to_string();
    let auth = token.map_or(String::new(), |t| format!("x-m2-token: {t}\r\n"));
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: {addr}\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).unwrap();
    let mut answer = String::new();
    s.read_to_string(&mut answer).unwrap();
    let status = answer[9..12].parse().unwrap();
    let json = answer.split_once("\r\n\r\n").map_or(Value::Null, |(_, b)| serde_json::from_str(b).unwrap_or(Value::Null));
    (status, json)
}

// MON-1, MON-4, G5: `POST /api/graph/overlay` is behind the token, mutes and solos, refuses the
// song in words, and its change is saved by the server's thread soon after.
#[test]
fn mute_and_solo_answer_with_the_overlay_state() {
    let dir = std::env::temp_dir().join(format!("m2-routes-8b-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let manifest = "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.5\"\n";
    std::fs::write(dir.join("music2.toml"), manifest).unwrap();
    std::fs::write(
        dir.join("song.ts"),
        "import { song, track } from '@m2/core';\nimport { osc } from 'std/dsp';\n\
         const kick = track('kick', () => osc.sine(55));\nexport default song(() => kick.out);\n",
    )
    .unwrap();
    let running =
        start_with(ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "t00ls".into(), page_token: "t0k".into() }, Graph::silent())
            .unwrap();
    let state = running.state.graph.open(&dir).unwrap();
    let session = state["session"].as_u64().unwrap();
    let url = &running.url;
    assert_eq!(post(url, "/api/graph/overlay", None, &json!({"session": session})).0, 401, "behind the token");
    let (status, answer) = post(url, "/api/graph/overlay", Some("t0k"), &json!({"session": session, "mute": {"kick": true}}));
    assert_eq!((status, &answer["mute"], &answer["heard"]["kick"]), (200, &json!(["kick"]), &json!("muted")), "{answer:#}");
    let (status, answer) = post(url, "/api/graph/overlay", Some("t0k"), &json!({"session": session, "solo": {"song": true}}));
    assert_eq!(status, 400);
    assert!(answer["error"].as_str().unwrap().contains("song can't be muted or soloed"), "{answer:#}");
    let file = dir.join(".music2/overlays.json");
    let start = Instant::now();
    while !file.exists() && start.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(20));
    }
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(saved, json!({"mute": ["kick"], "solo": []}));

    // MON-5: mute in song, a write: the option in the code, and the state says so.
    let (status, done) = post(url, "/api/graph/track/mute", Some("t0k"), &json!({"session": session, "modules": ["kick"], "muted": true}));
    assert_eq!((status, &done["before"], &done["after"]), (200, &json!("kick"), &json!("kick, muted in song")), "{done:#}");
    assert!(std::fs::read_to_string(dir.join("song.ts")).unwrap().contains("track('kick', { muted: true },"));
    let state = running.state.graph.state().unwrap();
    assert_eq!((&state["overlays"]["muted_in_song"], &state["overlays"]["heard"]["kick"]), (&json!(["kick"]), &json!("muted-in-song")));
    let (status, _) = post(url, "/api/graph/track/mute", Some("t0k"), &json!({"session": session, "modules": ["song"], "muted": true}));
    assert_eq!(status, 400);
    let _ = std::fs::remove_dir_all(&dir);
}

/// `GET path` with the token: the JSON answer.
fn get(url: &str, path: &str, token: &str) -> Value {
    let addr = url.trim_start_matches("http://");
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    let req = format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nx-m2-token: {token}\r\nConnection: close\r\n\r\n");
    s.write_all(req.as_bytes()).unwrap();
    let mut answer = String::new();
    s.read_to_string(&mut answer).unwrap();
    answer.split_once("\r\n\r\n").map_or(Value::Null, |(_, b)| serde_json::from_str(b).unwrap_or(Value::Null))
}

/// A window's WebSocket, by hand (RFC 6455): enough to send text and read what comes back.
struct Ws(std::net::TcpStream);

impl Ws {
    fn connect(url: &str, query: &str) -> Ws {
        let addr = url.trim_start_matches("http://");
        let mut s = std::net::TcpStream::connect(addr).unwrap();
        let req = format!(
            "GET /api/ws?{query} HTTP/1.1\r\nHost: {addr}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
             Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n"
        );
        s.write_all(req.as_bytes()).unwrap();
        let mut head = vec![];
        let mut b = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            s.read_exact(&mut b).unwrap();
            head.push(b[0]);
        }
        assert!(head.starts_with(b"HTTP/1.1 101"), "{}", String::from_utf8_lossy(&head));
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        Ws(s)
    }

    /// Send `v` as one masked text frame.
    fn send(&mut self, v: &Value) {
        let text = v.to_string();
        let mask = [1u8, 2, 3, 4];
        let mut frame = vec![0x81];
        match text.len() {
            n if n < 126 => frame.push(0x80 | n as u8),
            n => {
                frame.push(0x80 | 126);
                frame.extend((n as u16).to_be_bytes());
            }
        }
        frame.extend(mask);
        frame.extend(text.bytes().enumerate().map(|(i, b)| b ^ mask[i % 4]));
        self.0.write_all(&frame).unwrap();
    }

    /// The next text frame whose `t` is `t`, skipping the events every window gets.
    fn next(&mut self, t: &str) -> Value {
        loop {
            let mut h = [0u8; 2];
            self.0.read_exact(&mut h).unwrap();
            let len = match h[1] & 0x7f {
                126 => {
                    let mut n = [0u8; 2];
                    self.0.read_exact(&mut n).unwrap();
                    u16::from_be_bytes(n) as usize
                }
                127 => {
                    let mut n = [0u8; 8];
                    self.0.read_exact(&mut n).unwrap();
                    u64::from_be_bytes(n) as usize
                }
                n => n as usize,
            };
            let mut body = vec![0u8; len];
            self.0.read_exact(&mut body).unwrap();
            if h[0] & 0x0f == 1
                && let Ok(v) = serde_json::from_slice::<Value>(&body)
                && v["t"] == t
            {
                return v;
            }
        }
    }
}

/// Wait until `/api/graph/status` says `live` notes sound.
fn live_becomes(url: &str, live: u64) {
    let start = Instant::now();
    while get(url, "/api/graph/status", "t0k")["live"] != json!(live) {
        assert!(start.elapsed() < Duration::from_secs(5), "live never became {live}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

// MON-7, Q12: audition through `POST /api/graph/audition` and a window's WebSocket, behind the
// token: notes sound and end, `/api/graph/status` counts them, a lane nothing plays answers that
// window only, and a window that closes lets go of its notes.
#[test]
fn audition_over_http_and_the_websocket() {
    let dir = std::env::temp_dir().join(format!("m2-routes-8b-audition-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let manifest = "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.5\"\n";
    std::fs::write(dir.join("music2.toml"), manifest).unwrap();
    std::fs::write(
        dir.join("song.ts"),
        "import { song, track } from '@m2/core';\nimport { kick } from 'std/instruments';\n\
         const k = track('kick', ({ lane }) => kick(lane.notes('hits')));\nexport default song(() => k.out);\n",
    )
    .unwrap();
    let running =
        start_with(ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "t00ls".into(), page_token: "t0k".into() }, Graph::silent())
            .unwrap();
    running.state.graph.open(&dir).unwrap();
    let url = &running.url;
    let strike = |h: u32| json!({"client": "agent", "module": "kick", "lane": "hits", "on": [{"h": h, "pitch": 36}]});
    assert_eq!(post(url, "/api/graph/audition", None, &strike(1)).0, 401, "behind the token");
    assert_eq!(post(url, "/api/graph/audition", Some("t0k"), &strike(1)), (200, json!({})));
    live_becomes(url, 1);
    let (status, answer) = post(url, "/api/graph/audition", Some("t0k"), &json!({"client": "agent", "off": [1]}));
    assert_eq!((status, answer), (200, json!({})));
    live_becomes(url, 0);
    let (status, answer) = post(
        url,
        "/api/graph/audition",
        Some("t0k"),
        &json!({"client": "agent", "module": "kick", "lane": "hits", "on": [{"h": 1, "pitch": 300}]}),
    );
    assert_eq!(status, 400);
    assert!(answer["error"].as_str().unwrap().contains("pitch"), "{answer:#}");

    // A window: its notes, its answers, and its close.
    let mut ws = Ws::connect(url, "client=w1&token=t0k");
    ws.send(&json!({"t": "audition", "module": "kick", "lane": "hits", "on": [{"h": 7, "pitch": 36, "velocity": 0.8}]}));
    live_becomes(url, 1);
    ws.send(&json!({"t": "audition", "keep": [7]}));
    ws.send(&json!({"t": "audition", "module": "kick", "lane": "nope", "on": [{"h": 8, "pitch": 40}]}));
    assert_eq!(ws.next("audition"), json!({"t": "audition", "module": "kick", "lane": "nope", "silent": "Nothing plays this lane"}));
    ws.send(&json!({"t": "audition", "module": "kick", "lane": "hits", "on": [{"h": 9, "pitch": 40, "velocity": 3}]}));
    assert!(ws.next("audition")["error"].as_str().unwrap().contains("velocity"));
    // Another window gets none of this window's answers.
    let mut other = Ws::connect(url, "client=w2&token=t0k");
    other.send(&json!({"t": "audition", "module": "kick", "lane": "nope", "on": [{"h": 1, "pitch": 40}]}));
    assert_eq!(other.next("audition")["lane"], json!("nope"));
    drop(other);
    assert_eq!(get(url, "/api/graph/status", "t0k")["live"], json!(1));
    drop(ws);
    live_becomes(url, 0);
    let _ = std::fs::remove_dir_all(&dir);
}

// AGENT-11: a WebSocket opened with the tools key hears what the page hears, but what it sends is
// ignored: an agent can't play notes through it, nor aim the MIDI keyboard. The page's socket can.
#[test]
fn a_tools_socket_cant_audition_or_aim_the_midi_keyboard() {
    let dir = std::env::temp_dir().join(format!("m2-routes-8b-tools-socket-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let manifest = "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.5\"\n";
    std::fs::write(dir.join("music2.toml"), manifest).unwrap();
    std::fs::write(
        dir.join("song.ts"),
        "import { song, track } from '@m2/core';\nimport { kick } from 'std/instruments';\n\
         const k = track('kick', ({ lane }) => kick(lane.notes('hits')));\nexport default song(() => k.out);\n",
    )
    .unwrap();
    let running =
        start_with(ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "t00ls".into(), page_token: "t0k".into() }, Graph::silent())
            .unwrap();
    running.state.graph.open(&dir).unwrap();
    let url = &running.url;
    let mut agent = Ws::connect(url, "client=agent&token=t00ls");
    agent.send(&json!({"t": "midi", "module": "kick", "lane": "hits"}));
    agent.send(&json!({"t": "audition", "module": "kick", "lane": "hits", "on": [{"h": 7, "pitch": 36}]}));
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(500) {
        assert_eq!(get(url, "/api/graph/status", "t0k")["live"], json!(0), "no note sounds");
        assert_eq!(running.state.graph.midi_state()["target"], Value::Null, "the MIDI keyboard aims nowhere");
        std::thread::sleep(Duration::from_millis(20));
    }
    // It hears what the page hears.
    running.state.toast("hello");
    assert_eq!(agent.next("toast")["text"], json!("hello"));
    // The page's socket aims the MIDI keyboard, and plays.
    let mut page = Ws::connect(url, "client=w1&token=t0k");
    page.send(&json!({"t": "midi", "module": "kick", "lane": "hits"}));
    page.send(&json!({"t": "audition", "module": "kick", "lane": "hits", "on": [{"h": 7, "pitch": 36}]}));
    live_becomes(url, 1);
    assert_eq!(running.state.graph.midi_state()["target"], json!({"module": "kick", "lane": "hits"}));
    drop(page);
    live_becomes(url, 0);
    let _ = std::fs::remove_dir_all(&dir);
}

// WRITE-17, G5: a window open read-only answers mute in song 423 with WRITE-18's message, and
// writes nothing; mute while listening still works there.
#[test]
fn mute_in_song_is_refused_read_only() {
    let dir = std::env::temp_dir().join(format!("m2-routes-8b-ro-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let manifest = "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.5\"\n";
    std::fs::write(dir.join("music2.toml"), manifest).unwrap();
    let song = "import { song, track } from '@m2/core';\nimport { osc } from 'std/dsp';\n\
                const kick = track('kick', () => osc.sine(55));\nexport default song(() => kick.out);\n";
    std::fs::write(dir.join("song.ts"), song).unwrap();
    let held = m2_session::lock::acquire(&dir, "m2", m2_session::lock::RETRY).unwrap();
    let running =
        start_with(ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "t00ls".into(), page_token: "t0k".into() }, Graph::silent())
            .unwrap();
    let session = running.state.graph.open(&dir).unwrap()["session"].as_u64().unwrap();
    let (status, answer) =
        post(&running.url, "/api/graph/track/mute", Some("t0k"), &json!({"session": session, "modules": ["kick"], "muted": true}));
    assert_eq!(status, 423, "{answer:#}");
    assert!(answer["error"].as_str().unwrap().contains("nothing was changed"), "{answer:#}");
    assert_eq!(std::fs::read_to_string(dir.join("song.ts")).unwrap(), song);
    assert_eq!(post(&running.url, "/api/graph/overlay", Some("t0k"), &json!({"session": session, "mute": {"kick": true}})).0, 200);
    drop(held);
    let _ = std::fs::remove_dir_all(&dir);
}

// 8b.8, M1: every window gets the meters over its WebSocket while the song plays, on their own
// channel, behind the token; the status carries the DSP load the meters read.
#[test]
fn meters_reach_every_window_and_the_status_has_the_load() {
    let dir = std::env::temp_dir().join(format!("m2-routes-8b-meters-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let manifest = "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.5\"\n";
    std::fs::write(dir.join("music2.toml"), manifest).unwrap();
    std::fs::write(
        dir.join("song.ts"),
        "import { song, track } from '@m2/core';\nimport { osc } from 'std/dsp';\n\
         const lead = track('lead', () => osc.sine(440));\nexport default song(() => lead.out);\n",
    )
    .unwrap();
    let running =
        start_with(ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "t00ls".into(), page_token: "t0k".into() }, Graph::silent())
            .unwrap();
    running.state.graph.open(&dir).unwrap();
    running.state.graph.transport("play").unwrap();
    let url = &running.url;
    let (mut a, mut b) = (Ws::connect(url, "client=w1&token=t0k"), Ws::connect(url, "client=w2&token=t0k"));
    for ws in [&mut a, &mut b] {
        let m = ws.next("meters");
        assert!(m["tracks"]["lead"][0].as_f64().unwrap() > 0.5 && m["out"][0].as_f64().unwrap() > 0.5, "{m}");
        assert!(m["dsp"]["load"].as_f64().is_some() && m["dsp"]["dropouts"].as_u64().is_some(), "{m}");
    }
    let start = Instant::now();
    while get(url, "/api/graph/status", "t0k")["load"].as_f64().unwrap_or(0.0) <= 0.0 {
        assert!(start.elapsed() < Duration::from_secs(5), "the status never had a load");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(get(url, "/api/graph/status", "t0k")["late"].as_u64().is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

// 8b.5, G5: `POST /api/graph/clips` takes a gesture as JSON, answers with the clips it made, says a
// refusal in words, and is a write: 423 in a read-only window.
#[test]
fn clip_gestures_over_http() {
    let dir = std::env::temp_dir().join(format!("m2-routes-8b-clips-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let manifest = "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.5\"\n";
    std::fs::write(dir.join("music2.toml"), manifest).unwrap();
    std::fs::write(
        dir.join("song.ts"),
        "import { song, track } from '@m2/core';\nimport { kick } from 'std/instruments';\n\
         const k = track('kick', ({ lane }) => kick(lane.notes('hits')));\nexport default song(() => k.out);\n",
    )
    .unwrap();
    let running =
        start_with(ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "t00ls".into(), page_token: "t0k".into() }, Graph::silent())
            .unwrap();
    let session = running.state.graph.open(&dir).unwrap()["session"].as_u64().unwrap();
    let url = &running.url;
    let create = json!({"session": session, "gesture": {"op": "create", "module": "kick", "lane": "hits", "start": 4}});
    assert_eq!(post(url, "/api/graph/clips", None, &create).0, 401, "behind the token");
    let (status, done) = post(url, "/api/graph/clips", Some("t0k"), &create);
    assert_eq!((status, &done["after"], &done["made"][0]["lane"]), (200, &json!("clip at bar 2 (1 bar)"), &json!("hits")), "{done:#}");
    let id = done["made"][0]["id"].as_str().unwrap();
    let file = std::fs::read_to_string(dir.join("data/kick/hits.json")).unwrap();
    assert!(file.contains(&format!("{{\"id\": \"{id}\", \"start\": 4, \"length\": 4, \"notes\": []}}")), "{file}");
    let back = json!({"session": session, "gesture": {"op": "move", "clips": [{"module": "kick", "lane": "hits", "id": id}], "by": -8}});
    let (status, refused) = post(url, "/api/graph/clips", Some("t0k"), &back);
    assert_eq!(status, 400);
    assert!(refused["error"].as_str().unwrap().contains("before the song starts"), "{refused:#}");
    let gone = json!({"session": session, "gesture": {"op": "delete", "clips": [{"module": "kick", "lane": "hits", "id": "zzzzzzzz"}]}});
    let (status, refused) = post(url, "/api/graph/clips", Some("t0k"), &gone);
    assert_eq!((status, &refused["error"]), (400, &json!("kick's hits lane has no clip `zzzzzzzz` now.")), "said whole");
    let unknown = json!({"session": session, "gesture": {"op": "fold"}});
    assert_eq!(post(url, "/api/graph/clips", Some("t0k"), &unknown).0, 422, "a gesture that doesn't read");

    // Read-only: refused before anything is planned, and nothing changes.
    running.state.graph.close();
    let held = m2_session::lock::acquire(&dir, "m2", m2_session::lock::RETRY).unwrap();
    let session = running.state.graph.open(&dir).unwrap()["session"].as_u64().unwrap();
    let create = json!({"session": session, "gesture": {"op": "create", "module": "kick", "lane": "hits", "start": 8}});
    let (status, answer) = post(url, "/api/graph/clips", Some("t0k"), &create);
    assert_eq!(status, 423, "{answer:#}");
    assert_eq!(std::fs::read_to_string(dir.join("data/kick/hits.json")).unwrap(), file);
    drop(held);
    let _ = std::fs::remove_dir_all(&dir);
}

// ARR-9: `POST /api/graph/sections` takes a section gesture as JSON, answers as the clip route does,
// says a refusal in words, and is a write: 423 in a read-only window.
#[test]
fn section_gestures_over_http() {
    let dir = std::env::temp_dir().join(format!("m2-routes-8b-sections-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("data/kick")).unwrap();
    let manifest = "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.5\"\n";
    std::fs::write(dir.join("music2.toml"), manifest).unwrap();
    std::fs::write(
        dir.join("song.ts"),
        "import { song, track } from '@m2/core';\nimport { kick } from 'std/instruments';\n\
         const k = track('kick', ({ lane }) => kick(lane.notes('hits')));\nexport default song(() => k.out);\n",
    )
    .unwrap();
    let hits = "{\"clips\": [\n  {\"id\": \"k0000001\", \"start\": 0, \"length\": 4, \"notes\": []}\n]}\n";
    std::fs::write(dir.join("data/kick/hits.json"), hits).unwrap();
    let running =
        start_with(ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "t00ls".into(), page_token: "t0k".into() }, Graph::silent())
            .unwrap();
    let session = running.state.graph.open(&dir).unwrap()["session"].as_u64().unwrap();
    let url = &running.url;
    let duplicate = json!({"session": session, "gesture": {"op": "duplicate", "start": 0, "end": 4}});
    assert_eq!(post(url, "/api/graph/sections", None, &duplicate).0, 401, "behind the token");
    let (status, done) = post(url, "/api/graph/sections", Some("t0k"), &duplicate);
    assert_eq!((status, &done["before"], &done["after"]), (200, &json!("bar 2"), &json!("a copy of bar 1")), "{done:#}");
    assert_eq!((&done["file"], &done["made"][0]["lane"]), (&json!("data/kick/hits.json"), &json!("hits")), "{done:#}");
    let file = std::fs::read_to_string(dir.join("data/kick/hits.json")).unwrap();
    assert!(file.contains("\"start\": 4, \"length\": 4"), "{file}");
    let off = json!({"session": session, "gesture": {"op": "insert-bars", "at": 2, "bars": 1}});
    let (status, refused) = post(url, "/api/graph/sections", Some("t0k"), &off);
    assert_eq!(
        (status, &refused["error"]),
        (400, &json!("edit rejected: New bars must be on a bar line: beat 2 is inside bar 1.")),
        "said whole"
    );
    let unknown = json!({"session": session, "gesture": {"op": "fold", "start": 0, "end": 4}});
    assert_eq!(post(url, "/api/graph/sections", Some("t0k"), &unknown).0, 422, "a gesture that doesn't read");

    // Read-only: refused before anything is planned, and nothing changes.
    running.state.graph.close();
    let held = m2_session::lock::acquire(&dir, "m2", m2_session::lock::RETRY).unwrap();
    let session = running.state.graph.open(&dir).unwrap()["session"].as_u64().unwrap();
    let delete = json!({"session": session, "gesture": {"op": "delete", "start": 0, "end": 4}});
    let (status, answer) = post(url, "/api/graph/sections", Some("t0k"), &delete);
    assert_eq!(status, 423, "{answer:#}");
    assert_eq!(std::fs::read_to_string(dir.join("data/kick/hits.json")).unwrap(), file);
    drop(held);
    let _ = std::fs::remove_dir_all(&dir);
}

// Q9: until the piano roll sends `notes` gestures, it writes a whole lane, with the clips as the
// timeline shows them, each with its `sha` (8b.5): the lane route reads them as clips.
#[test]
fn the_rolls_lane_write_takes_clips_as_the_timeline_shows_them() {
    let dir = std::env::temp_dir().join(format!("m2-routes-8b-lane-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let manifest = "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.5\"\n";
    std::fs::write(dir.join("music2.toml"), manifest).unwrap();
    std::fs::write(
        dir.join("song.ts"),
        "import { song, track } from '@m2/core';\nimport { kick } from 'std/instruments';\n\
         const k = track('kick', ({ lane }) => kick(lane.notes('hits')));\nexport default song(() => k.out);\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("data/kick")).unwrap();
    std::fs::write(dir.join("data/kick/hits.json"), "{\"clips\": [{\"id\": \"a\", \"start\": 0, \"length\": 4, \"notes\": []}]}\n")
        .unwrap();
    let running =
        start_with(ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "t00ls".into(), page_token: "t0k".into() }, Graph::silent())
            .unwrap();
    let state = running.state.graph.open(&dir).unwrap();
    let (session, revision) = (state["session"].as_u64().unwrap(), state["view"]["revision"].as_u64().unwrap());
    let mut clips = state["timeline"]["tracks"][0]["lanes"][0]["clips"].clone();
    assert!(clips[0]["sha"].is_string(), "{clips}");
    clips[0]["length"] = json!(8);
    let body = json!({"session": session, "revision": revision, "module": "kick", "lane": "hits", "clips": clips});
    let (status, done) = post(&running.url, "/api/graph/lane", Some("t0k"), &body);
    assert_eq!(status, 200, "{done:#}");
    let file = std::fs::read_to_string(dir.join("data/kick/hits.json")).unwrap();
    assert_eq!(file, "{\"clips\": [\n  {\"id\": \"a\", \"start\": 0, \"length\": 8, \"notes\": []}\n]}\n");
    let _ = std::fs::remove_dir_all(&dir);
}

// HOME-2, Q10: `POST /api/graph/automate/remove` puts the lane's value at the playhead back where
// the lane was read, and takes the lane file away; a write, so 423 read-only.
#[test]
fn remove_automation_over_http() {
    let dir = std::env::temp_dir().join(format!("m2-routes-8b-unautomate-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let manifest = "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.5\"\n";
    std::fs::write(dir.join("music2.toml"), manifest).unwrap();
    let song = "import { hz, song } from '@m2/core';\nimport { osc } from 'std/dsp';\n\
                export default song(({ lane }) => osc.sine(lane.control(\"freq\", \"hz\", hz(440))));\n";
    std::fs::write(dir.join("song.ts"), song).unwrap();
    std::fs::create_dir_all(dir.join("data/song")).unwrap();
    std::fs::write(
        dir.join("data/song/freq.json"),
        "{\"unit\": \"hz\", \"clips\": [{\"id\": \"a\", \"start\": 0, \"length\": 4, \"points\": [[0, 200], [4, 600]]}]}\n",
    )
    .unwrap();
    let running =
        start_with(ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "t00ls".into(), page_token: "t0k".into() }, Graph::silent())
            .unwrap();
    let session = running.state.graph.open(&dir).unwrap()["session"].as_u64().unwrap();
    let body = json!({"session": session, "module": "song", "lane": "freq", "beat": 2});
    let (status, done) = post(&running.url, "/api/graph/automate/remove", Some("t0k"), &body);
    assert_eq!((status, &done["before"], &done["after"]), (200, &json!("freq automated"), &json!("freq hz(400)")), "{done:#}");
    assert!(std::fs::read_to_string(dir.join("song.ts")).unwrap().contains("osc.sine(hz(400))"));
    assert!(!dir.join("data/song/freq.json").exists());
    let (status, refused) = post(&running.url, "/api/graph/automate/remove", Some("t0k"), &body);
    assert_eq!(status, 400);
    assert_eq!(refused["error"], json!("song has no lane `freq`."), "said whole");

    running.state.graph.close();
    let held = m2_session::lock::acquire(&dir, "m2", m2_session::lock::RETRY).unwrap();
    let session = running.state.graph.open(&dir).unwrap()["session"].as_u64().unwrap();
    let body = json!({"session": session, "module": "song", "lane": "freq"});
    assert_eq!(post(&running.url, "/api/graph/automate/remove", Some("t0k"), &body).0, 423);
    drop(held);
    let _ = std::fs::remove_dir_all(&dir);
}

// The sx plan, D11: Follow tempo through `POST /api/graph/clips`, as the clip menu sends it. On, the
// resample example's first clip (2 beats at 120 BPM: a second of its file) gets warp markers at its
// start and end; off, none. Each is one step; a notes clip has no tempo to follow.
#[test]
fn follow_tempo_through_the_clips_route() {
    let dir = std::env::temp_dir().join(format!("m2-routes-8b-follow-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    crate::routes_audio::tests::copy(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/resample"), &dir);
    let running =
        start_with(ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "t00ls".into(), page_token: "t0k".into() }, Graph::silent())
            .unwrap();
    let session = running.state.graph.open(&dir).unwrap()["session"].as_u64().unwrap();
    let follow = |on: bool, lane: &str, id: &str| {
        let gesture =
            json!({"op": "audio", "clip": {"module": lane.split('/').next(), "lane": lane.split('/').nth(1), "id": id}, "follow": on});
        post(&running.url, "/api/graph/clips", Some("t0k"), &json!({"session": session, "gesture": gesture}))
    };
    let line = || {
        let file = std::fs::read_to_string(dir.join("data/chops/clips.json")).unwrap();
        file.lines().find(|l| l.contains("a3s9vpxb")).unwrap().to_string()
    };
    let rev = |done: &Value| done["revision"].as_u64().unwrap();
    let (status, on) = follow(true, "chops/clips", "a3s9vpxb");
    assert_eq!(status, 200, "{on:#}");
    assert!(line().contains(r#""warp": [[0, 0], [1, 2]]"#), "{}", line());
    let (status, off) = follow(false, "chops/clips", "a3s9vpxb");
    assert_eq!((status, rev(&off)), (200, rev(&on) + 1), "{off:#}");
    assert!(line().contains(r#""warp": []"#), "{}", line());
    let (status, why) = follow(true, "kick/hits", "my53vvkc");
    assert_eq!(status, 400, "{why:#}");
    let _ = std::fs::remove_dir_all(&dir);
}
