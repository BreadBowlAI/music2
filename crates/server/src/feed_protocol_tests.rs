//! `feed/1` (FEED-8): the feed routes' bodies, answers and refusals as a protocol any player can
//! speak, written up in the guide (`docs/guide/feed-protocol.md`). Its fixture,
//! `tests/fixtures/feed-protocol.json`, lists the acts in order on one song and what each answer
//! must be: every field it names with its type and no field it doesn't name (but the app's envelope,
//! `session`), the values it gives, and each refusal's words. Here it runs against the app's own
//! routes, over HTTP with the tools key, on a copy of first-song in a driven player, so the
//! musician's Play is the test's. A page's relay runs the same fixture (track `web-feeds`).

use std::io::{Read, Write};
use std::path::Path;

use m2_play::{Driver, Player};
use serde_json::{Value, json};

const FIXTURE: &str = include_str!("../tests/fixtures/feed-protocol.json");

/// The route each act is, in the app.
fn route(act: &str) -> (&'static str, &'static str) {
    match act {
        "open" => ("POST", "/api/feed/open"),
        "put" => ("POST", "/api/feed/put"),
        "status" => ("GET", "/api/feed"),
        "close" => ("POST", "/api/feed/close"),
        other => panic!("feed/1 has no act `{other}`"),
    }
}

/// `method path` with `body` and the tools key, no name in a header (the body's `as` names the
/// agent): the status and the answer's text.
fn ask(url: &str, method: &str, path: &str, body: Option<&Value>) -> (u16, String) {
    let addr = url.trim_start_matches("http://");
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    let body = body.map(Value::to_string).unwrap_or_default();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nx-m2-token: t00ls\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).unwrap();
    let mut answer = String::new();
    s.read_to_string(&mut answer).unwrap();
    let status = answer[9..12].parse().unwrap();
    (status, answer.split_once("\r\n\r\n").map_or(String::new(), |(_, b)| b.to_string()))
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let (f, t) = (e.path(), to.join(e.file_name()));
        if f.is_dir() { copy(&f, &t) } else { std::fs::copy(&f, &t).map(drop).unwrap() }
    }
}

/// `n` blocks of 128 frames, as the sound card would ask for them.
fn fill(driver: &mut Driver, n: usize) {
    for _ in 0..n {
        let mut buf = [0.0f32; 256];
        driver.fill(&mut buf, 2);
        drop(driver.drain_retired());
    }
}

/// The value at `path` (fields and list indexes, by dots), if there is one.
fn at<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(v, |v, key| match key.parse::<usize>() {
        Ok(i) => v.get(i),
        Err(_) => v.get(key),
    })
}

/// Whether `v` is the value `want`: numbers as numbers (`0` is `0.0`).
fn same(v: &Value, want: &Value) -> bool {
    match (v.as_f64(), want.as_f64()) {
        (Some(a), Some(b)) => (a - b).abs() < 1e-9,
        _ => v == want,
    }
}

/// What is wrong with `v` as the shape `spec` (`answers`' words), at `place`: a field missing, one
/// of another type, or one `feed/1` doesn't name (the envelope's are the app's own).
fn misfits(v: &Value, spec: &Value, answers: &Value, envelope: &Value, place: &str, out: &mut Vec<String>) {
    match spec {
        Value::String(t) => {
            let t = t.trim_end_matches('?');
            let fits = match t {
                "string" => v.is_string(),
                "number" => v.is_number(),
                "bool" => v.is_boolean(),
                "string|null" => v.is_string() || v.is_null(),
                named => return misfits(v, &answers[named], answers, envelope, place, out),
            };
            if !fits {
                out.push(format!("{place} is {v}, not a {t}"));
            }
        }
        Value::Array(inner) => match v.as_array() {
            Some(items) => {
                for (i, item) in items.iter().enumerate() {
                    misfits(item, &inner[0], answers, envelope, &format!("{place}.{i}"), out);
                }
            }
            None => out.push(format!("{place} is {v}, not a list")),
        },
        Value::Object(fields) => {
            let Some(got) = v.as_object() else { return out.push(format!("{place} is {v}, not an object")) };
            for (k, t) in fields {
                match got.get(k) {
                    Some(x) => misfits(x, t, answers, envelope, &format!("{place}.{k}"), out),
                    None if t.as_str().is_some_and(|t| t.ends_with('?')) => {}
                    None => out.push(format!("{place} has no `{k}`")),
                }
            }
            for k in got.keys().filter(|k| !fields.contains_key(*k) && envelope.get(k.as_str()).is_none()) {
                out.push(format!("{place} has `{k}`, which feed/1 doesn't name"));
            }
        }
        other => panic!("the fixture's shape {other} isn't one"),
    }
}

/// A put's notes, as the fixture's `many` makes them.
fn many(m: &Value) -> Value {
    let n = |k: &str| m[k].as_f64().unwrap_or(0.0);
    let notes: Vec<Value> = (0..m["count"].as_u64().unwrap())
        .map(|i| i as f64)
        .map(|i| json!({"pitch": n("pitch") + i * n("pitch_step"), "start": n("start") + i * n("step"), "length": n("length")}))
        .collect();
    json!(notes)
}

// FEED-8, FEED-4, FEED-5: the fixture's acts against the app's routes: each answer has every field
// feed/1 names, of its type, and none it doesn't; each value it names; each refusal its words.
#[test]
fn the_feed_protocol_fixture_runs_against_the_app() {
    let fixture: Value = serde_json::from_str(FIXTURE).expect("the fixture reads");
    assert_eq!(fixture["protocol"], "feed/1");
    let (answers, envelope) = (&fixture["answers"], &fixture["envelope"]);
    let bpm = fixture["song"]["bpm"].as_f64().unwrap();

    let base = std::env::temp_dir().join(format!("m2-feed-protocol-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let dir = base.join("song");
    let example = fixture["song"]["example"].as_str().unwrap();
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples").join(example), &dir);
    let (player, mut driver) = Player::driven(48_000);
    let options = crate::ServeOptions { port: 0, web_dir: base.join("no-web"), token: "t00ls".into(), page_token: "p4ge".into() };
    let running = crate::start_with(options, crate::graph::Graph::with_player(player).stored()).unwrap();
    let graph = &running.state.graph;
    let session = graph.open(&dir).unwrap()["session"].as_u64().unwrap();
    fill(&mut driver, 2);
    let url = running.url.clone();

    let mut wrong: Vec<String> = vec![];
    let steps = fixture["steps"].as_array().unwrap();
    for (n, step) in steps.iter().enumerate() {
        let say = format!("step {n} ({})", step["say"].as_str().unwrap_or_default());
        if let Some(m) = step.get("musician") {
            let to = m["play_to"].as_f64().expect("the musician plays to a beat");
            graph.transport("play").unwrap();
            let mut blocks = 0;
            while graph.status().beat < to {
                fill(&mut driver, 1);
                blocks += 1;
                assert!(blocks < 100_000, "{say}: the transport never reached beat {to}");
            }
            continue;
        }
        let act = step["act"].as_str().unwrap();
        let (method, path) = route(act);
        let mut body = step["body"].clone();
        if let Some(m) = step.get("many") {
            body["notes"] = many(m);
        }
        // The app's envelope: which open song.
        let sent = (method == "POST").then(|| {
            let mut b = body.clone();
            b["session"] = json!(session);
            b
        });
        let (_, mut events, _) = running.state.subscribe(false);
        let (status, text) = ask(&url, method, path, sent.as_ref());
        let Ok(v) = serde_json::from_str::<Value>(&text) else {
            wrong.push(format!("{say}: answered {status} with text that isn't JSON: {text}"));
            continue;
        };
        let refused = step.get("refused").or(step.get("refused_starting"));
        if let Some(words) = refused {
            let words = words.as_str().unwrap();
            let mut out = vec![];
            misfits(&v, &json!("refusal"), answers, &json!({}), "the refusal", &mut out);
            wrong.extend(out.into_iter().map(|w| format!("{say}: {w}")));
            if status != 400 {
                wrong.push(format!("{say}: refused with {status}, not 400"));
            }
            let said = v["error"].as_str().unwrap_or_default();
            let fits = match step.get("refused") {
                Some(_) => said == words,
                None => said.starts_with(words),
            };
            if !fits {
                wrong.push(format!("{say}: refused in the words {said:?}, not {words:?}"));
            }
            continue;
        }
        if status != 200 {
            wrong.push(format!("{say}: answered {status}: {v}"));
            continue;
        }
        let shape = step["answer"].as_str().expect("a step that isn't refused names its answer");
        let mut out = vec![];
        misfits(&v, &json!(shape), answers, envelope, shape, &mut out);
        wrong.extend(out.into_iter().map(|w| format!("{say}: {w}")));
        for (path, want) in step["is"].as_object().into_iter().flatten() {
            match at(&v, path) {
                Some(got) if same(got, want) => {}
                got => wrong.push(format!("{say}: `{path}` is {}, not {want}", got.map_or("absent".into(), Value::to_string))),
            }
        }
        for path in step["absent"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            if let Some(got) = at(&v, path) {
                wrong.push(format!("{say}: `{path}` is {got}; it is absent here"));
            }
        }
        if let Some(least) = step["lock_ahead_seconds_at_least"].as_f64() {
            let ahead = (v["lock"].as_f64().unwrap_or(0.0) - v["playhead"].as_f64().unwrap_or(0.0)) * 60.0 / bpm;
            if ahead < least - 1e-9 {
                wrong.push(format!("{say}: the lock is {ahead:.4} s past the playhead, not at least {least} s"));
            }
        }
        if step["told"] == true {
            let mut told = vec![];
            while let Ok(m) = events.try_recv() {
                if let Ok(e) = serde_json::from_str::<Value>(&m)
                    && e["t"] == "feed"
                    && e.get("why") == v.get("why")
                {
                    told.push(e);
                }
            }
            if told != [v.clone()] {
                wrong.push(format!("{say}: the event with its `why` isn't the answer, once: told {told:?}"));
            }
        }
    }

    // The app's envelope refuses a body for a song that isn't open, in its own words.
    let (status, text) = ask(&url, "POST", "/api/feed/put", Some(&json!({"session": session + 1, "feed": "bass", "from": 0, "to": 4})));
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let words = format!("the open song changed: read the state again (session {} isn't open)", session + 1);
    if status != 400 || v != json!({"error": words}) {
        wrong.push(format!("a session that isn't open: answered {status} {text}"));
    }

    let _ = std::fs::remove_dir_all(&base);
    assert!(wrong.is_empty(), "feed/1 against the app, {} wrong:\n{}", wrong.len(), wrong.join("\n"));
}
