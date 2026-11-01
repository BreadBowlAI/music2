//! What every change answers at its top (AGENT-2): the revision it made (none when it made none: a
//! change of `layout.json` alone), its undo step, and the files whose ids it moved (`moved`). A window keeps
//! its own step from that answer, whatever the route, and moves a gesture drawn before the change to
//! the newer revision only when it moved none. Every write route of the table is run here on a copy
//! of first-song, or named below with why it answers no change of its own.

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::path::Path;

use serde_json::{Value, json};

use crate::api;
use crate::graph::Graph;

const PAGE: &str = "t0k";
const TOOLS: &str = "t00ls";

/// The write routes that make no change of their own, and why: each is checked elsewhere.
const NO_CHANGE: [(&str, &str); 10] = [
    ("/api/graph/preview", "begins a knob's drag: its commit answers the change"),
    ("/api/graph/lease/begin", "begins a knob's drag: its commit answers the change"),
    ("/api/graph/lane/lease/begin", "begins a clip's drag: its commit answers the change"),
    ("/api/graph/tempo/lease/begin", "begins a tempo drag: its commit answers the change"),
    ("/api/graph/record/start", "a take: the news that it was written names its step (a_takes_news_names_its_step)"),
    ("/api/graph/record/stop", "a take: the news that it was written names its step (a_takes_news_names_its_step)"),
    ("/api/graph/record/cancel", "drops a take: nothing is written"),
    ("/api/library/get", "installs a pack: the song changes only by the watcher's batch, an outside step"),
    ("/api/graph/undo/skip", "takes a step out of the history: nothing is written, so no revision"),
    ("/api/graph/upgrade", "run below on a song that needs none: it writes nothing"),
];

/// `method path` with `key` and `body`: the status and the JSON answer.
fn ask(url: &str, method: &str, path: &str, key: &str, body: &Value) -> (u16, Value) {
    ask_as(url, method, path, key, None, body)
}

/// [`ask`] as the agent `name` says it is (`x-m2-as`).
fn ask_as(url: &str, method: &str, path: &str, key: &str, name: Option<&str>, body: &Value) -> (u16, Value) {
    ask_with(url, method, path, key, name.map(|n| format!("x-m2-as: {n}\r\n")).unwrap_or_default(), body)
}

/// [`ask`] from the page's window `window` (`x-m2-window`, AGENT-2).
fn ask_in(url: &str, method: &str, path: &str, window: &str, body: &Value) -> (u16, Value) {
    ask_with(url, method, path, PAGE, format!("x-m2-window: {window}\r\n"), body)
}

fn ask_with(url: &str, method: &str, path: &str, key: &str, named: String, body: &Value) -> (u16, Value) {
    let addr = url.trim_start_matches("http://");
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    let body = if body.is_null() { String::new() } else { body.to_string() };
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nx-m2-token: {key}\r\n{named}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).unwrap();
    let mut answer = String::new();
    s.read_to_string(&mut answer).unwrap();
    let status = answer[9..12].parse().unwrap();
    (status, answer.split_once("\r\n\r\n").map_or(Value::Null, |(_, b)| serde_json::from_str(b).unwrap_or(Value::Null)))
}

/// A copy of `from` at `to`.
fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let (f, t) = (e.path(), to.join(e.file_name()));
        if f.is_dir() {
            copy(&f, &t);
        } else {
            std::fs::copy(&f, &t).unwrap();
        }
    }
}

/// A mono 16-bit WAV at 48 kHz, `seconds` long, of a quiet sine.
fn wav(seconds: f64) -> Vec<u8> {
    let n = (seconds * 48_000.0) as u32;
    let mut b = Vec::new();
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + n * 2).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&48_000u32.to_le_bytes());
    b.extend_from_slice(&96_000u32.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&(n * 2).to_le_bytes());
    for i in 0..n {
        let v = (0.2 * (std::f64::consts::TAU * 220.0 * i as f64 / 48_000.0).sin() * 32767.0) as i16;
        b.extend_from_slice(&v.to_le_bytes());
    }
    b
}

/// A one-track MIDI file at 96 ticks a beat: two notes.
fn midi() -> Vec<u8> {
    let body: Vec<u8> = vec![0, 0x90, 40, 100, 96, 0x80, 40, 0, 0, 0x90, 43, 100, 96, 0x80, 43, 0, 0, 0xff, 0x2f, 0];
    let mut b = vec![0x4d, 0x54, 0x68, 0x64, 0, 0, 0, 6, 0, 0, 0, 1, 0, 96];
    b.extend_from_slice(b"MTrk");
    b.extend_from_slice(&(body.len() as u32).to_be_bytes());
    b.extend_from_slice(&body);
    b
}

/// What a change is to answer.
#[derive(Clone, Copy, Debug)]
enum Made {
    /// A new revision, its step, and whether it moved ids in some file (`None`: either, by what it
    /// wrote).
    Revision(Option<bool>),
    /// A step, but no revision: `layout.json` alone (PLANE-3).
    NoRevision,
    /// An undo or a redo: a new revision, the step it took back, and whether that moved ids.
    TookBack(Option<bool>),
    /// Nothing written.
    Nothing,
}

struct Run {
    url: String,
    session: u64,
    seen: BTreeSet<String>,
}

impl Run {
    fn state(&self) -> Value {
        ask(&self.url, "GET", "/api/graph", PAGE, &Value::Null).1
    }

    fn revision(&self) -> u64 {
        self.state()["view"]["revision"].as_u64().unwrap()
    }

    /// `method path` with `body` (its session filled in), checked as `made` says: the answer.
    fn change(&mut self, method: &str, path: &str, key: &str, mut body: Value, made: Made) -> Value {
        if let Some(o) = body.as_object_mut().filter(|o| !o.contains_key("lease")) {
            o.insert("session".into(), json!(self.session));
        }
        let before = self.revision();
        let (status, answer) = ask(&self.url, method, path, key, &body);
        let at = format!("{method} {path} {body}: {status} {answer}");
        assert_eq!(status, 200, "{at}");
        let after = self.revision();
        let top = |k: &str| answer.get(k).cloned().unwrap_or(Value::Null);
        match made {
            Made::Revision(moved) => {
                assert!(after > before, "a new revision: {at}");
                assert_eq!(top("revision"), json!(after), "the answer names the revision it made: {at}");
                assert!(top("step").is_u64(), "and its step: {at}");
                assert!(top("moved").is_array(), "and the files whose ids it moved: {at}");
                if let Some(m) = moved {
                    assert_eq!(!top("moved").as_array().unwrap().is_empty(), m, "moved ids: {at}");
                }
            }
            Made::NoRevision => {
                assert_eq!(after, before, "no revision: {at}");
                assert!(top("revision").is_null(), "so the answer names none: {at}");
                assert!(top("step").is_u64(), "but its step: {at}");
            }
            Made::TookBack(moved) => {
                assert!(after > before, "a new revision: {at}");
                assert_eq!(top("revision"), json!(after), "the answer names the revision it made: {at}");
                assert!(top("step").is_u64() && top("moved").is_array(), "the step it took back, and the files whose ids it moved: {at}");
                if let Some(m) = moved {
                    assert_eq!(!top("moved").as_array().unwrap().is_empty(), m, "moved ids as its step did: {at}");
                }
            }
            Made::Nothing => {
                assert_eq!(after, before, "nothing written: {at}");
                assert!(top("revision").is_null() && top("step").is_null(), "so it names no revision and no step: {at}");
            }
        }
        self.seen.insert(path.to_string());
        answer
    }

    /// The node of `module` that `pick` finds.
    fn node(&self, module: &str, pick: impl Fn(&Value) -> bool) -> Value {
        let st = self.state();
        let m =
            st["view"]["modules"].as_array().unwrap().iter().find(|m| m["key"] == module).cloned().unwrap_or_else(|| panic!("no {module}"));
        m["nodes"].as_array().unwrap().iter().find(|n| pick(n)).cloned().unwrap_or_else(|| panic!("no such node in {module}"))
    }

    fn component(&self, module: &str, component: &str) -> String {
        self.node(module, |n| n["component"] == component)["id"].as_str().unwrap().to_string()
    }

    fn knob(&self, module: &str, label: &str) -> String {
        let n = self.node(module, |n| n["knobs"].as_array().unwrap().iter().any(|k| k["label"] == label));
        n["knobs"].as_array().unwrap().iter().find(|k| k["label"] == label).unwrap()["id"].as_str().unwrap().to_string()
    }

    /// The modules the timeline has now.
    fn modules(&self) -> BTreeSet<String> {
        self.state()["view"]["modules"].as_array().unwrap().iter().map(|m| m["key"].as_str().unwrap().to_string()).collect()
    }
}

// AGENT-2, AGENT-8: every write route's answer says, at its top, the revision it made (none for
// `layout.json` alone), its step and whether it moved ids: the code save's too. A route the table
// adds must be run here, or named in NO_CHANGE with why.
#[test]
fn every_change_answers_its_revision_step_and_moved() {
    let base = std::env::temp_dir().join(format!("m2-answers-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let dir = base.join("song");
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song"), &dir);
    // Data no lane reads, to delete, to fix and to relink: hats' lane moved where nothing reads it.
    std::fs::create_dir_all(dir.join("data/ghost")).unwrap();
    std::fs::copy(dir.join("data/kick/hits.json"), dir.join("data/ghost/one.json")).unwrap();
    std::fs::copy(dir.join("data/kick/hits.json"), dir.join("data/ghost/two.json")).unwrap();
    std::fs::create_dir_all(dir.join("data/moved")).unwrap();
    std::fs::rename(dir.join("data/hats/hits.json"), dir.join("data/moved/hits.json")).unwrap();
    let files = base.join("files");
    std::fs::create_dir_all(&files).unwrap();
    std::fs::write(files.join("tone.wav"), wav(2.0)).unwrap();
    std::fs::write(files.join("riff.mid"), midi()).unwrap();
    let options = crate::ServeOptions { port: 0, web_dir: base.join("no-web"), token: TOOLS.into(), page_token: PAGE.into() };
    let running = crate::start_with(options, Graph::silent().stored()).unwrap();
    let session = running.state.graph.open(&dir).unwrap()["session"].as_u64().unwrap();
    let mut r = Run { url: running.url.clone(), session, seen: BTreeSet::new() };
    let rev = |r: &Run| r.revision();

    // Values: no ids move.
    let pan = r.knob("song", "hats.pan");
    r.change(
        "POST",
        "/api/graph/set",
        PAGE,
        json!({"revision": rev(&r), "id": pan, "gesture": "set", "value": 0.4}),
        Made::Revision(Some(false)),
    );
    r.change(
        "POST",
        "/api/graph/clips",
        PAGE,
        json!({"gesture": {"op": "create", "module": "kick", "lane": "hits", "start": 200, "length": 4}}),
        Made::Revision(Some(false)),
    );
    let bass: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("data/bass/notes.json")).unwrap()).unwrap();
    let mut clips = bass["clips"].clone();
    clips[0]["notes"].as_array_mut().unwrap().push(json!({"pitch": "C2", "start": 0.5, "length": 0.25}));
    r.change(
        "POST",
        "/api/graph/lane",
        PAGE,
        json!({"revision": rev(&r), "module": "bass", "lane": "notes", "clips": clips}),
        Made::Revision(Some(false)),
    );
    r.change("POST", "/api/graph/tempo", PAGE, json!({"at": 0, "bpm": 121}), Made::Revision(Some(false)));
    r.change("POST", "/api/graph/meter", PAGE, json!({"at": 0, "beats": 3, "unit": 4}), Made::Revision(Some(false)));
    // Code written otherwise than as values: by its kind, it moves its file's ids.
    r.change("POST", "/api/graph/title", PAGE, json!({"name": "Answers"}), Made::Revision(None));
    r.change("POST", "/api/graph/label", PAGE, json!({"module": "bass", "label": "Low end"}), Made::Revision(Some(true)));
    r.change("POST", "/api/graph/track/mute", PAGE, json!({"modules": ["keys"], "muted": true}), Made::Revision(None));
    r.change(
        "POST",
        "/api/graph/sections",
        PAGE,
        json!({"gesture": {"op": "insert-bars", "at": 0, "bars": 1}}),
        Made::Revision(Some(false)),
    );
    r.change("POST", "/api/graph/mix/insert", PAGE, json!({"channel": "kick", "control": "pan", "value": 0.1}), Made::Revision(Some(true)));
    let song = std::fs::read_to_string(dir.join("song.ts")).unwrap();
    let saved = song.replacen("pan: 0.4", "pan: 0.35", 1);
    assert_ne!(saved, song, "the save changes a number");
    // A code save moves ids, whatever it changed (the second review's P1).
    r.change("PUT", "/api/graph/file", PAGE, json!({"path": "song.ts", "base": song, "text": saved}), Made::Revision(Some(true)));
    let knob = r.knob("song", "hats.pan");
    let begun = ask(
        &r.url,
        "POST",
        "/api/graph/lease/begin",
        PAGE,
        &json!({"session": session, "revision": rev(&r), "id": knob, "client": "c", "gesture": "drag"}),
    )
    .1;
    r.change("POST", "/api/graph/lease/commit", PAGE, json!({"lease": begun["lease"], "value": 0.2}), Made::Revision(Some(false)));
    let hits: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("data/kick/hits.json")).unwrap()).unwrap();
    let kick = hits["clips"][0]["id"].clone();
    let moving = |by: f64| json!({"op": "move", "clips": [{"module": "kick", "lane": "hits", "id": kick}], "by": by});
    let begun =
        ask(&r.url, "POST", "/api/graph/lane/lease/begin", PAGE, &json!({"session": session, "gesture": moving(1.0), "client": "c"})).1;
    assert!(begun["lease"].is_u64(), "{begun}");
    r.change(
        "POST",
        "/api/graph/lane/lease/commit",
        PAGE,
        json!({"lease": begun["lease"], "gesture": moving(2.0)}),
        Made::Revision(Some(false)),
    );
    let begun = ask(&r.url, "POST", "/api/graph/tempo/lease/begin", PAGE, &json!({"session": session, "at": 0, "client": "c"})).1;
    r.change("POST", "/api/graph/tempo/lease/commit", PAGE, json!({"lease": begun["lease"], "bpm": 118}), Made::Revision(Some(false)));
    // An agent's Tx: a note in Bass's lane.
    let text = std::fs::read_to_string(dir.join("data/bass/notes.json")).unwrap();
    let sha = format!("sha256:{}", m2_song::Sha::of(text.as_bytes()).hex());
    let tx = json!({"id": "tx-answers", "base": {"data/bass/notes.json": sha},
        "ops": [{"op": "clips", "lane": ["bass", "notes"], "put": [{"id": "a1an5wer", "start": 64, "length": 4, "notes": [{"pitch": "E1", "start": 0, "length": 0.25}]}], "delete": []}],
        "expect": {"confined": {"ranges": {}, "lanes": [["bass", "notes"]], "modules": []}}});
    r.change("POST", "/api/tx/commit", TOOLS, json!({"tx": tx}), Made::Revision(Some(false)));
    // Data no lane reads.
    r.change(
        "POST",
        "/api/graph/data/delete",
        PAGE,
        json!({"revision": rev(&r), "file": "data/ghost/one.json"}),
        Made::Revision(Some(true)),
    );
    r.change(
        "POST",
        "/api/graph/relink",
        PAGE,
        json!({"revision": rev(&r), "from": "data/moved/hits.json", "to": "data/hats/hits.json"}),
        Made::Revision(Some(true)),
    );
    let st = r.state();
    let fix = st["problems"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|p| p["fixes"].as_array().cloned().unwrap_or_default())
        .next()
        .expect("a fix offered");
    r.change("POST", "/api/graph/fix", PAGE, json!({"revision": rev(&r), "fix": fix}), Made::Revision(None));

    // Calls added, removed or moved: ids move.
    // A code save that changes a call, and a drum machine made a kit.
    let hats = std::fs::read_to_string(dir.join("tracks/hats.ts")).unwrap();
    let machine = hats.replace("drums.drum(lane.notes(\"hits\"), { drum: \"hats\", tune: 5 })", "drums.kit(lane.notes(\"hits\"))");
    assert_ne!(machine, hats, "hats plays a drum machine");
    r.change("PUT", "/api/graph/file", PAGE, json!({"path": "tracks/hats.ts", "base": hats, "text": machine}), Made::Revision(Some(true)));
    r.change("POST", "/api/graph/track/make-kit", PAGE, json!({"track": "hats"}), Made::Revision(Some(true)));

    let drive = r.knob("bass", "drive");
    let automated = r.change("POST", "/api/graph/automate", PAGE, json!({"revision": rev(&r), "id": drive}), Made::Revision(Some(true)));
    let lane = automated["lane"].clone();
    r.change(
        "POST",
        "/api/graph/automate/remove",
        PAGE,
        json!({"module": lane["module"], "lane": lane["lane"], "beat": 0}),
        Made::Revision(Some(true)),
    );
    let pluck = r.component("keys", "@m2/synths:pluck");
    r.change(
        "POST",
        "/api/graph/effect/add",
        PAGE,
        json!({"revision": rev(&r), "call": pluck, "component": "@m2/effects:reverb"}),
        Made::Revision(Some(true)),
    );
    let reverb = r.component("keys", "@m2/effects:reverb");
    r.change("POST", "/api/graph/effect/bypass", PAGE, json!({"revision": rev(&r), "node": reverb, "on": true}), Made::Revision(None));
    let reverb = r.component("keys", "@m2/effects:reverb");
    r.change(
        "POST",
        "/api/graph/effect/add",
        PAGE,
        json!({"revision": rev(&r), "call": reverb, "component": "@m2/effects:delay"}),
        Made::Revision(Some(true)),
    );
    let delay = r.component("keys", "@m2/effects:delay");
    r.change(
        "POST",
        "/api/graph/effect/move",
        PAGE,
        json!({"revision": rev(&r), "node": delay, "earlier": true}),
        Made::Revision(Some(true)),
    );
    let delay = r.component("keys", "@m2/effects:delay");
    r.change("POST", "/api/graph/effect/remove", PAGE, json!({"revision": rev(&r), "call": delay}), Made::Revision(Some(true)));
    let duck = r.component("song", "std:duck");
    r.change(
        "POST",
        "/api/graph/effect/sidechain",
        PAGE,
        json!({"revision": rev(&r), "node": duck, "track": "hats"}),
        Made::Revision(None),
    );
    let drive = r.knob("bass", "phase");
    let outlet = r.node("bass", |n| n["outlet"] == "drop")["id"].clone();
    r.change("POST", "/api/graph/connect", PAGE, json!({"revision": rev(&r), "from": outlet, "to": drive}), Made::Revision(None));
    let wired = |i: &Value| i["id"].as_str().is_some_and(|x| x.ends_with("phase"));
    let inlet = r.node("bass", |n| n["inlets"].as_array().unwrap().iter().any(wired))["inlets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| wired(i))
        .unwrap()["id"]
        .clone();
    r.change("POST", "/api/graph/disconnect", PAGE, json!({"revision": rev(&r), "id": inlet}), Made::Revision(None));
    let had = r.modules();
    r.change("POST", "/api/graph/mix/bus", PAGE, json!({"kind": "reverb"}), Made::Revision(Some(true)));
    let bus = r.modules().difference(&had).next().unwrap().clone();
    r.change("POST", "/api/graph/mix/send", PAGE, json!({"channel": "hats", "bus": bus, "action": "add"}), Made::Revision(None));
    let had = r.modules();
    r.change("POST", "/api/graph/mix/bus", PAGE, json!({"kind": "group"}), Made::Revision(Some(true)));
    let group = r.modules().difference(&had).next().unwrap().clone();
    r.change("POST", "/api/graph/mix/route", PAGE, json!({"channel": "hats", "to": group}), Made::Revision(None));
    let had = r.modules();
    r.change("POST", "/api/graph/track/add", PAGE, json!({"component": "@m2/drums:drum", "label": "Clap"}), Made::Revision(Some(true)));
    let clap = r.modules().difference(&had).next().unwrap().clone();
    let had = r.modules();
    r.change("POST", "/api/graph/track/duplicate", PAGE, json!({"module": clap}), Made::Revision(Some(true)));
    let twin = r.modules().difference(&had).next().unwrap().clone();
    r.change("POST", "/api/graph/track/rekey", PAGE, json!({"module": twin, "key": "clap3"}), Made::Revision(Some(true)));
    r.change(
        "POST",
        "/api/graph/track/instrument",
        PAGE,
        json!({"module": "clap3", "component": "@m2/synths:synth-pluck"}),
        Made::Revision(Some(true)),
    );
    let pluck = r.component("clap3", "@m2/synths:synth-pluck");
    r.change(
        "POST",
        "/api/graph/preset/load",
        PAGE,
        json!({"at": rev(&r), "node": pluck, "from": "std", "name": "Glass"}),
        Made::Revision(None),
    );
    r.change("POST", "/api/graph/track/delete", PAGE, json!({"module": "clap3"}), Made::Revision(Some(true)));
    let had = r.modules();
    r.change("POST", "/api/graph/track/kit", PAGE, json!({"component": "@m2/drums:drum", "kit": "Basic kit"}), Made::Revision(Some(true)));
    let kit = r.state()["view"]["drum_kits"][0]["bus"]
        .as_str()
        .map(String::from)
        .unwrap_or_else(|| r.modules().difference(&had).next().unwrap().clone());
    r.change("POST", "/api/graph/kit/drum", PAGE, json!({"bus": kit, "sound": "clap"}), Made::Revision(Some(true)));
    let imported = r.change(
        "POST",
        "/api/graph/audio/import",
        PAGE,
        json!({"paths": [files.join("tone.wav")], "to": {"kind": "below", "beat": 0}}),
        Made::Revision(Some(true)),
    );
    r.change("POST", "/api/graph/audio/track", PAGE, json!({"label": "Vox"}), Made::Revision(Some(true)));
    r.change(
        "POST",
        "/api/graph/midifile/import",
        PAGE,
        json!({"path": files.join("riff.mid"), "to": {"kind": "below", "beat": 0}, "tempo": "keep"}),
        Made::Revision(Some(true)),
    );
    r.change(
        "POST",
        "/api/library/sample",
        PAGE,
        json!({"id": "sample:std:samples/kit/kick.wav", "to": {"kind": "below", "beat": 0}}),
        Made::Revision(Some(true)),
    );
    let had = r.modules();
    r.change("POST", "/api/graph/track/add", PAGE, json!({"component": "std:sampler", "label": "Sampler"}), Made::Revision(Some(true)));
    let sampler = r.modules().difference(&had).next().unwrap().clone();
    let call = r.component(&sampler, "std:sampler");
    r.change("POST", "/api/graph/sample", PAGE, json!({"at": rev(&r), "call": call, "file": files.join("tone.wav")}), Made::Revision(None));
    // A take over a loop: the audio clip just brought in, with two passes noted.
    let made = &imported["made"][0];
    let asset = imported["assets"][0].as_str().unwrap();
    m2_editor::take::note_takes(&dir, asset, &[0.0, 1.0]).unwrap();
    // A file written outside the app: the watcher may take it in as a revision of its own. Once it
    // has, nothing else changes the song while the routes below are judged.
    let quiet = |r: &Run| {
        let mut last = (r.revision(), std::time::Instant::now());
        let start = std::time::Instant::now();
        while last.1.elapsed() < std::time::Duration::from_millis(700) && start.elapsed() < std::time::Duration::from_secs(5) {
            std::thread::sleep(std::time::Duration::from_millis(50));
            let now = r.revision();
            if now != last.0 {
                last = (now, std::time::Instant::now());
            }
        }
    };
    quiet(&r);
    r.change(
        "POST",
        "/api/graph/take",
        PAGE,
        json!({"module": made["module"], "lane": made["lane"], "id": made["id"], "take": 2}),
        Made::Revision(None),
    );
    r.change("POST", "/api/project/rename", PAGE, json!({"name": "Answers again", "folder": false}), Made::Revision(None));

    // `layout.json` alone: a step, no revision.
    r.change("POST", "/api/graph/layout", PAGE, json!({"colour": {"module": "keys", "colour": "teal"}}), Made::NoRevision);
    let marked = r.change("POST", "/api/graph/markers", PAGE, json!({"markers": [{"beat": 32, "name": "Chorus"}]}), Made::NoRevision);
    // The window names such a step by its id, having no revision to name it by.
    let (status, named) =
        ask(&r.url, "POST", "/api/graph/step", PAGE, &json!({"session": session, "step": marked["step"], "name": "adding Chorus"}));
    assert_eq!((status, &named["named"], &named["undo_name"]["name"]), (200, &json!(true), &json!("adding Chorus")), "{named}");
    r.change("POST", "/api/graph/song-key", PAGE, json!({"key": {"root": "A", "scale": "minor"}}), Made::NoRevision);
    r.change("POST", "/api/graph/upgrade", PAGE, json!({"check": false}), Made::Nothing);

    // Undo and redo: the revision they make, the step they took back. An undo of a track's add (it
    // writes layout.json first) still names its revision.
    r.change("POST", "/api/graph/undo", PAGE, json!({}), Made::NoRevision);
    let had = r.modules();
    r.change(
        "POST",
        "/api/graph/track/add",
        PAGE,
        json!({"component": "@m2/synths:synth-pad", "label": "Pad"}),
        Made::Revision(Some(true)),
    );
    assert_ne!(r.modules(), had);
    r.change("POST", "/api/graph/undo", PAGE, json!({}), Made::TookBack(Some(true)));
    r.change("POST", "/api/graph/redo", PAGE, json!({}), Made::TookBack(Some(true)));
    // A value's undo and redo move none, as the value didn't.
    let pan = r.knob("song", "hats.pan");
    r.change(
        "POST",
        "/api/graph/set",
        PAGE,
        json!({"revision": rev(&r), "id": pan, "gesture": "set", "value": 0.25}),
        Made::Revision(Some(false)),
    );
    r.change("POST", "/api/graph/undo", PAGE, json!({}), Made::TookBack(Some(false)));
    r.change("POST", "/api/graph/redo", PAGE, json!({}), Made::TookBack(Some(false)));

    // Every write route of the table: run above, or named with why it answers no change.
    let exempt: BTreeSet<&str> = NO_CHANGE.iter().map(|(p, _)| *p).collect();
    let missing: Vec<String> = api::all()
        .into_iter()
        .filter(|x| x.writes && !exempt.contains(x.path) && !r.seen.contains(x.path))
        .map(|x| format!("{} {}", x.method, x.path))
        .collect();
    assert!(missing.is_empty(), "write routes neither run here nor named in NO_CHANGE: {missing:?}");
    drop(running);
    let _ = std::fs::remove_dir_all(&base);
}

/// A server on a copy of first-song, with its folder: the url, the session.
fn serving(name: &str) -> (std::path::PathBuf, crate::Running, u64) {
    let dir = std::env::temp_dir().join(format!("m2-answers-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song"), &dir);
    let options = crate::ServeOptions { port: 0, web_dir: dir.join("no-web"), token: TOOLS.into(), page_token: PAGE.into() };
    let running = crate::start_with(options, Graph::silent().stored()).unwrap();
    let session = running.state.graph.open(&dir).unwrap()["session"].as_u64().unwrap();
    (dir, running, session)
}

// AGENT-2 (review O8): a window's gesture drawn before an agent changed its knob, with the window's
// own change in between, goes from what it was drawn on: the server refuses it, the agent's value
// is kept, and the refusal names the agent. A drag's lease is refused so too.
#[test]
fn a_knob_an_agent_changed_since_it_was_drawn_is_refused_naming_the_agent() {
    let (dir, running, session) = serving("knob");
    let r = Run { url: running.url.clone(), session, seen: BTreeSet::new() };
    let drawn = r.revision();
    let pan = r.knob("song", "hats.pan");
    // The window's own change, in another file.
    let keys = r.knob("keys", "brightness");
    let (status, mine) = ask(
        &r.url,
        "POST",
        "/api/graph/set",
        PAGE,
        &json!({"session": session, "revision": drawn, "id": keys, "gesture": "set", "value": 0.7}),
    );
    assert_eq!(status, 200, "{mine}");
    // An agent's, of the knob.
    let at = r.revision();
    let (status, theirs) = ask_as(
        &r.url,
        "POST",
        "/api/graph/set",
        TOOLS,
        Some("claude"),
        &json!({"session": session, "revision": at, "id": pan, "gesture": "set", "value": 0.1}),
    );
    assert_eq!(status, 200, "{theirs}");
    let kept = std::fs::read_to_string(dir.join("song.ts")).unwrap();
    assert!(kept.contains("pan: 0.1"), "{kept}");
    // The window's gesture, as drawn.
    let (status, refused) = ask(
        &r.url,
        "POST",
        "/api/graph/set",
        PAGE,
        &json!({"session": session, "revision": drawn, "id": pan, "gesture": "set", "value": 0.6, "expect": 0.3}),
    );
    assert_ne!(status, 200, "{refused}");
    let said = refused["error"].as_str().unwrap_or_default();
    assert!(said.contains("the agent claude changed it"), "the refusal names the agent: {refused}");
    let (status, refused) = ask(
        &r.url,
        "POST",
        "/api/graph/lease/begin",
        PAGE,
        &json!({"session": session, "revision": drawn, "id": pan, "client": "w", "gesture": "drag"}),
    );
    let said = refused.to_string();
    assert!(status != 200 || refused["lease"].is_null(), "a drag drawn before is refused too: {refused}");
    assert!(said.contains("the agent claude changed it"), "naming the agent: {refused}");
    assert_eq!(std::fs::read_to_string(dir.join("song.ts")).unwrap(), kept, "the agent's value is kept");
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
}

// AGENT-2 (review O8): a clip an agent changed after the window drew it is refused when the window
// moves it, naming the agent; the agent's clip is kept.
#[test]
fn a_clip_an_agent_changed_since_it_was_drawn_is_refused_naming_the_agent() {
    let (dir, running, session) = serving("clip");
    let r = Run { url: running.url.clone(), session, seen: BTreeSet::new() };
    let st = r.state();
    let lane = st["timeline"]["tracks"].as_array().unwrap().iter().find(|t| t["module"] == "kick").unwrap()["lanes"][0].clone();
    let clip = lane["clips"][0].clone();
    let drawn = json!({"module": "kick", "lane": "hits", "id": clip["id"], "sha": clip["sha"]});
    assert!(clip["sha"].is_string(), "the window draws a clip with its hash: {clip}");
    // An agent's change of that clip: one more hit.
    let path = dir.join("data/kick/hits.json");
    let text = std::fs::read_to_string(&path).unwrap();
    let mut hits: Value = serde_json::from_str(&text).unwrap();
    let mut changed = hits["clips"][0].clone();
    changed["notes"].as_array_mut().unwrap().push(json!({"pitch": "C1", "start": 0.5, "length": 0.25, "velocity": 0.8}));
    hits["clips"][0] = changed.clone();
    let sha = format!("sha256:{}", m2_song::Sha::of(text.as_bytes()).hex());
    let tx = json!({"id": "tx-o8", "base": {"data/kick/hits.json": sha},
        "ops": [{"op": "clips", "lane": ["kick", "hits"], "put": [changed], "delete": []}],
        "expect": {"confined": {"ranges": {}, "lanes": [["kick", "hits"]], "modules": []}}});
    let (status, theirs) = ask_as(&r.url, "POST", "/api/tx/commit", TOOLS, Some("claude"), &json!({"session": session, "tx": tx}));
    assert_eq!(status, 200, "{theirs}");
    let kept = std::fs::read_to_string(&path).unwrap();
    assert_ne!(kept, text);
    let (status, refused) =
        ask(&r.url, "POST", "/api/graph/clips", PAGE, &json!({"session": session, "gesture": {"op": "move", "clips": [drawn], "by": 4}}));
    assert_ne!(status, 200, "{refused}");
    assert!(refused.to_string().contains("the agent claude changed"), "the refusal names the agent: {refused}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), kept, "the agent's clip is kept");
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
}

/// `method path` as the page sends it, with the session: the status and answer.
fn page(r: &Run, method: &str, path: &str, mut body: Value) -> (u16, Value) {
    body["session"] = json!(r.session);
    ask(&r.url, method, path, PAGE, &body)
}

/// `method path` as the agent Claude sends it, with the session.
fn claude(r: &Run, method: &str, path: &str, mut body: Value) -> (u16, Value) {
    body["session"] = json!(r.session);
    ask_as(&r.url, method, path, TOOLS, Some("Claude"), &body)
}

/// The ids of the Reverbs in Keys, inner first.
fn reverbs(r: &Run) -> Vec<String> {
    let st = r.state();
    let m = st["view"]["modules"].as_array().unwrap().iter().find(|m| m["key"] == "keys").unwrap().clone();
    let mut ids: Vec<String> = m["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|n| n["component"] == "@m2/effects:reverb")
        .map(|n| n["id"].as_str().unwrap().to_string())
        .collect();
    ids.sort_by_key(|id| std::cmp::Reverse(id.rsplit_once('#').unwrap().1.parse::<u32>().unwrap()));
    ids
}

// AGENT-2 (the second review of own-edits, P1, P5): `moved` is the files a change moved, by its
// kind. Two Reverbs trading places (each call calling what it did) move tracks/keys.ts, and so do
// the move's undo and redo; an effect added on Keys moves tracks/keys.ts and not song.ts; a code
// save moves its file, even one that swaps two mixer entries; a channel's volume written at its
// default, as a new `db()` call, moves song.ts; a value written in place moves none.
#[test]
fn moved_is_the_files_a_change_moved_by_its_kind() {
    let (dir, running, session) = serving("moved");
    let r = Run { url: running.url.clone(), session, seen: BTreeSet::new() };
    let moved = |v: &Value| {
        v["moved"].as_array().unwrap_or_else(|| panic!("{v}")).iter().map(|m| m.as_str().unwrap().to_string()).collect::<Vec<_>>()
    };
    let keys = vec!["tracks/keys.ts".to_string()];
    let pluck = r.component("keys", "@m2/synths:pluck");
    let (_, a) =
        page(&r, "POST", "/api/graph/effect/add", json!({"revision": r.revision(), "call": pluck, "component": "@m2/effects:reverb"}));
    assert_eq!(moved(&a), keys, "an effect added on Keys moves its file only: {a}");
    let (_, a) = page(
        &r,
        "POST",
        "/api/graph/effect/add",
        json!({"revision": r.revision(), "call": reverbs(&r)[0], "component": "@m2/effects:reverb"}),
    );
    assert_eq!(moved(&a), keys, "{a}");
    let inner = reverbs(&r)[0].clone();
    // The inner one made different, so the move changes the text.
    let size = r.state()["view"]["modules"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|m| m["nodes"].as_array().unwrap().clone())
        .find(|n| n["id"] == json!(inner))
        .unwrap()["knobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|k| k["label"] == "size")
        .unwrap()["id"]
        .clone();
    let (status, _) = page(&r, "POST", "/api/graph/set", json!({"revision": r.revision(), "id": size, "gesture": "set", "value": 0.3}));
    assert_eq!(status, 200);
    let before = std::fs::read_to_string(dir.join("tracks/keys.ts")).unwrap();
    let (status, a) = page(&r, "POST", "/api/graph/effect/move", json!({"revision": r.revision(), "node": inner, "earlier": false}));
    assert_ne!(std::fs::read_to_string(dir.join("tracks/keys.ts")).unwrap(), before, "the move changed the text: {a}");
    assert_eq!((status, moved(&a)), (200, keys.clone()), "two Reverbs trading places move ids: {a}");
    let (_, a) = page(&r, "POST", "/api/graph/undo", json!({}));
    assert_eq!(moved(&a), keys, "and so does its undo: {a}");
    let (_, a) = page(&r, "POST", "/api/graph/redo", json!({}));
    assert_eq!(moved(&a), keys, "and its redo: {a}");

    let pan = r.knob("song", "hats.pan");
    let (_, a) = page(&r, "POST", "/api/graph/set", json!({"revision": r.revision(), "id": pan, "gesture": "set", "value": 0.4}));
    assert_eq!(moved(&a), Vec::<String>::new(), "a value written in place moves none: {a}");
    let kick = r.knob("song", "kick.db");
    let (_, a) = page(&r, "POST", "/api/graph/set", json!({"revision": r.revision(), "id": kick, "gesture": "set", "value": -3.0}));
    assert!(std::fs::read_to_string(dir.join("song.ts")).unwrap().contains("db(-3)"), "written as a db() call");
    assert_eq!(moved(&a), vec!["song.ts".to_string()], "a value written as a new call moves its file: {a}");

    // A code save that swaps two mixer entries: each call keeps what it calls.
    let song = std::fs::read_to_string(dir.join("song.ts")).unwrap();
    let hats = song.lines().find(|l| l.trim_start().starts_with("hats:")).unwrap().to_string();
    let keys_line = song.lines().find(|l| l.trim_start().starts_with("keys:")).unwrap().to_string();
    let swapped = song.replacen(&hats, "\u{0}", 1).replacen(&keys_line, &hats, 1).replacen('\u{0}', &keys_line, 1);
    assert_ne!(swapped, song);
    let (status, a) = page(&r, "PUT", "/api/graph/file", json!({"path": "song.ts", "base": song, "text": swapped}));
    assert_eq!((status, moved(&a)), (200, vec!["song.ts".to_string()]), "a code save moves its file's ids: {a}");
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
}

// AGENT-2 (the second review, P3): a file saved outside the app that rides along with the window's
// own commit makes that revision `mixed`, and a refusal for a change in it names outside the app.
#[test]
fn a_commit_that_took_in_an_outside_save_is_mixed() {
    let (dir, running, session) = serving("mixed");
    let r = Run { url: running.url.clone(), session, seen: BTreeSet::new() };
    let drawn = r.revision();
    let keys = r.knob("song", "keys.pan");
    let song = std::fs::read_to_string(dir.join("song.ts")).unwrap();
    let outside = song.replacen("pan: -0.25", "pan: 0.6", 1);
    assert_ne!(outside, song, "keys pan at -0.25");
    std::fs::write(dir.join("song.ts"), &outside).unwrap();
    let pan = r.knob("song", "hats.pan");
    let (status, a) = page(&r, "POST", "/api/graph/set", json!({"revision": drawn, "id": pan, "gesture": "set", "value": 0.4}));
    assert_eq!((status, &a["mixed"]), (200, &json!(true)), "the outside save rode along: {a}");
    let (status, refused) =
        page(&r, "POST", "/api/graph/set", json!({"revision": drawn, "id": keys, "gesture": "set", "value": -0.05, "expect": -0.25}));
    assert_ne!(status, 200, "{refused}");
    assert!(refused.to_string().contains("changed outside the app"), "{refused}");
    assert!(std::fs::read_to_string(dir.join("song.ts")).unwrap().contains("pan: 0.6"), "the outside value is kept");
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
}

// AGENT-2 (the second review, P6): the window's own value, then an agent's change in another file;
// the window's next gesture on its value, from the revision its own change made, goes.
#[test]
fn a_gesture_from_the_windows_own_revision_goes_past_an_agents_other_file() {
    let (dir, running, session) = serving("base");
    let r = Run { url: running.url.clone(), session, seen: BTreeSet::new() };
    let pan = r.knob("song", "hats.pan");
    let (_, mine) = page(&r, "POST", "/api/graph/set", json!({"revision": r.revision(), "id": pan, "gesture": "set", "value": 0.4}));
    let base = mine["revision"].as_u64().unwrap();
    let bright = r.knob("keys", "brightness");
    let (status, _) =
        claude(&r, "POST", "/api/graph/set", json!({"revision": r.revision(), "id": bright, "gesture": "agent", "value": 0.7}));
    assert_eq!(status, 200);
    let (status, a) =
        page(&r, "POST", "/api/graph/set", json!({"revision": base, "id": pan, "gesture": "set", "value": 0.5, "expect": 0.4}));
    assert_eq!(status, 200, "never refused for the window's own change: {a}");
    assert!(std::fs::read_to_string(dir.join("song.ts")).unwrap().contains("pan: 0.5"));
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
}

// AGENT-2 (the second review, P2): bars, and the Beat grid's steps, say the revision they go from;
// after an agent inserted bars, a delete of the bars drawn is refused, naming the agent, and a
// step on a lane the agent changed is too. From the revision after it, they go.
#[test]
fn bars_and_steps_after_an_agents_change_are_refused_naming_it() {
    let (dir, running, session) = serving("bars");
    let r = Run { url: running.url.clone(), session, seen: BTreeSet::new() };
    let drawn = r.revision();
    let kick = std::fs::read_to_string(dir.join("data/kick/hits.json")).unwrap();
    let (status, _) = claude(&r, "POST", "/api/graph/sections", json!({"gesture": {"op": "insert-bars", "at": 0, "bars": 4}}));
    assert_eq!(status, 200);
    let inserted = std::fs::read_to_string(dir.join("data/kick/hits.json")).unwrap();
    assert_ne!(inserted, kick);
    let (status, refused) =
        page(&r, "POST", "/api/graph/sections", json!({"revision": drawn, "gesture": {"op": "delete-bars", "start": 16, "end": 32}}));
    assert_ne!(status, 200, "{refused}");
    assert!(refused.to_string().contains("the agent Claude changed"), "names the agent: {refused}");
    assert_eq!(std::fs::read_to_string(dir.join("data/kick/hits.json")).unwrap(), inserted, "nothing deleted");
    let (status, refused) = page(
        &r,
        "POST",
        "/api/graph/clips",
        json!({"revision": drawn, "gesture": {"op": "hits", "module": "kick", "lane": "hits", "at": [0.5], "step": 0.25, "to": "on"}}),
    );
    assert_ne!(status, 200, "{refused}");
    assert!(refused.to_string().contains("the agent Claude changed"), "{refused}");
    let now = r.revision();
    let (status, a) =
        page(&r, "POST", "/api/graph/sections", json!({"revision": now, "gesture": {"op": "delete-bars", "start": 16, "end": 32}}));
    assert_eq!(status, 200, "from the revision after it, it goes: {a}");
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
}

// AGENT-2 (the second review, P4): a list of layout.json says the list it was made from; when the
// markers or the order changed since, it is refused and nothing is lost.
#[test]
fn a_list_changed_since_it_was_drawn_is_refused() {
    let (dir, running, session) = serving("lists");
    let r = Run { url: running.url.clone(), session, seen: BTreeSet::new() };
    let drawn = json!([{"beat": 0, "name": "Intro"}]);
    let (status, _) = page(&r, "POST", "/api/graph/markers", json!({"markers": drawn}));
    assert_eq!(status, 200);
    let (status, _) =
        claude(&r, "POST", "/api/graph/markers", json!({"markers": [{"beat": 0, "name": "Intro"}, {"beat": 32, "name": "Chorus"}]}));
    assert_eq!(status, 200);
    let (status, refused) = page(&r, "POST", "/api/graph/markers", json!({"markers": [{"beat": 8, "name": "Intro"}], "base": drawn}));
    assert_ne!(status, 200, "{refused}");
    assert!(refused.to_string().contains("the agent Claude changed"), "{refused}");
    assert!(std::fs::read_to_string(dir.join("layout.json")).unwrap().contains("Chorus"), "the agent's marker is kept");
    let order: Vec<String> = r.state()["timeline"]["tracks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["track"] == true && t["module"] != "song")
        .map(|t| t["module"].as_str().unwrap().to_string())
        .collect();
    let mut theirs = order.clone();
    theirs.reverse();
    let (status, _) = claude(&r, "POST", "/api/graph/layout", json!({"order": theirs}));
    assert_eq!(status, 200);
    let mut mine = order.clone();
    mine.rotate_left(1);
    let (status, refused) = page(&r, "POST", "/api/graph/layout", json!({"order": mine, "base": order}));
    assert_ne!(status, 200, "{refused}");
    let (status, _) = page(&r, "POST", "/api/graph/layout", json!({"order": order, "base": theirs}));
    assert_eq!(status, 200, "from the order as it is, it goes");
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
}

// AGENT-2 (the second review, P7): an agent's undo is the agent's: a clip gesture drawn before it,
// refused, names the agent, not the window's own step below it.
#[test]
fn a_refusal_for_an_agents_undo_names_the_agent() {
    let (dir, running, session) = serving("undoer");
    let r = Run { url: running.url.clone(), session, seen: BTreeSet::new() };
    let path = dir.join("data/hats/hits.json");
    let text = std::fs::read_to_string(&path).unwrap();
    let hits: Value = serde_json::from_str(&text).unwrap();
    let mut clip = hits["clips"][0].clone();
    let pitch = clip["notes"][0]["pitch"].clone();
    clip["notes"].as_array_mut().unwrap().push(json!({"pitch": pitch, "start": 0.5, "length": 0.25}));
    let sha = format!("sha256:{}", m2_song::Sha::of(text.as_bytes()).hex());
    let tx = json!({"id": "tx-p7", "base": {"data/hats/hits.json": sha},
        "ops": [{"op": "clips", "lane": ["hats", "hits"], "put": [clip], "delete": []}],
        "expect": {"confined": {"ranges": {}, "lanes": [["hats", "hits"]], "modules": []}}});
    let (status, _) = claude(&r, "POST", "/api/tx/commit", json!({"tx": tx}));
    assert_eq!(status, 200);
    // The window draws the clip with the agent's hit.
    let drawn = r.revision();
    let lane = r.state()["timeline"]["tracks"].as_array().unwrap().iter().find(|t| t["module"] == "hats").unwrap()["lanes"][0].clone();
    let seen = lane["clips"][0].clone();
    let (status, _) = claude(&r, "POST", "/api/graph/undo", json!({}));
    assert_eq!(status, 200);
    let gesture = json!({"op": "move", "clips": [{"module": "hats", "lane": "hits", "id": seen["id"], "sha": seen["sha"]}], "by": 4});
    let (status, refused) = page(&r, "POST", "/api/graph/clips", json!({"revision": drawn, "gesture": gesture}));
    assert_ne!(status, 200, "{refused}");
    assert!(refused.to_string().contains("the agent Claude changed"), "the agent's undo, by the agent: {refused}");
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
}

/// `method path` from the page's window `w1`, with the session.
fn win(r: &Run, method: &str, path: &str, mut body: Value) -> (u16, Value) {
    body["session"] = json!(r.session);
    ask_in(&r.url, method, path, "w1", &body)
}

// AGENT-2 (the third check of own-edits, Q1): an undo whose commit took in a file saved outside says
// `mixed`, and a gesture drawn before it is refused over the outside change, naming outside.
#[test]
fn an_undo_that_took_in_an_outside_save_is_mixed() {
    let (dir, running, session) = serving("undomixed");
    let r = Run { url: running.url.clone(), session, seen: BTreeSet::new() };
    let pan = r.knob("song", "hats.pan");
    let (_, _) = win(&r, "POST", "/api/graph/set", json!({"revision": r.revision(), "id": pan, "gesture": "set", "value": 0.4}));
    let drawn = r.revision();
    let keys = r.knob("song", "keys.pan");
    let song = std::fs::read_to_string(dir.join("song.ts")).unwrap();
    std::fs::write(dir.join("song.ts"), song.replacen("pan: -0.25", "pan: 0.6", 1)).unwrap();
    let (status, u) = win(&r, "POST", "/api/graph/undo", json!({}));
    assert_eq!((status, &u["mixed"]), (200, &json!(true)), "the undo took the outside save in with it: {u}");
    let (status, refused) =
        win(&r, "POST", "/api/graph/set", json!({"revision": drawn, "id": keys, "gesture": "set", "value": -0.05, "expect": -0.25}));
    assert_ne!(status, 200, "{refused}");
    assert!(refused.to_string().contains("changed outside the app"), "{refused}");
    assert!(std::fs::read_to_string(dir.join("song.ts")).unwrap().contains("pan: 0.6"));
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
}

// AGENT-2 (the third check, Q2): bars inserted, their undo, and a tempo change say they moved
// beats; a value doesn't. After an agent's bars, a Beat grid step on an empty lane, a clip made
// and a paste, each drawn before, are refused naming the agent.
#[test]
fn a_change_that_moves_beats_says_so_and_refuses_gestures_named_by_beats() {
    let (dir, running, session) = serving("beats");
    let r = Run { url: running.url.clone(), session, seen: BTreeSet::new() };
    let (_, a) = page(&r, "POST", "/api/graph/sections", json!({"gesture": {"op": "insert-bars", "at": 0, "bars": 1}}));
    assert_eq!(a["beats"], json!(true), "bars inserted move beats: {a}");
    let (_, a) = page(&r, "POST", "/api/graph/undo", json!({}));
    assert_eq!(a["beats"], json!(true), "and so does their undo: {a}");
    let (_, a) = page(&r, "POST", "/api/graph/tempo", json!({"at": 0, "bpm": 121}));
    assert_eq!(a["beats"], json!(true), "a tempo change moves beats: {a}");
    let pan = r.knob("song", "hats.pan");
    let (_, a) = page(&r, "POST", "/api/graph/set", json!({"revision": r.revision(), "id": pan, "gesture": "set", "value": 0.4}));
    assert!(a["beats"].is_null(), "a value doesn't: {a}");
    let (_, made) = page(&r, "POST", "/api/graph/track/add", json!({"component": "@m2/drums:drum", "label": "Clap"}));
    assert!(made["revision"].is_u64(), "{made}");
    let st = r.state();
    let clap = st["timeline"]["tracks"].as_array().unwrap().iter().find(|t| t["label"] == "Clap").unwrap();
    let (module, lane) = (clap["module"].clone(), clap["lanes"][0]["key"].clone());
    assert_eq!(clap["lanes"][0]["clips"], json!([]), "an empty lane");
    let drawn = r.revision();
    let (status, _) = claude(&r, "POST", "/api/graph/sections", json!({"gesture": {"op": "insert-bars", "at": 0, "bars": 4}}));
    assert_eq!(status, 200);
    let steps = json!({"op": "hits", "module": module, "lane": lane, "at": [1.0], "step": 0.25, "to": "on"});
    let made = json!({"op": "create", "module": "kick", "lane": "hits", "start": 200, "length": 4});
    let copied = page(&r, "POST", "/api/graph/sections/copy", json!({"start": 0, "end": 16})).1;
    for (what, gesture) in [("a Beat grid step on an empty lane", steps), ("a clip made at a beat", made)] {
        let (status, refused) = page(&r, "POST", "/api/graph/clips", json!({"revision": drawn, "gesture": gesture}));
        assert_ne!(status, 200, "{what}: {refused}");
        assert!(refused.to_string().contains("the agent Claude changed"), "{what}, naming the agent: {refused}");
    }
    let (status, refused) = page(
        &r,
        "POST",
        "/api/graph/sections",
        json!({"revision": drawn, "gesture": {"op": "paste", "at": 64, "section": copied["section"]}}),
    );
    assert_ne!(status, 200, "bars pasted: {refused}");
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
}

// AGENT-2 (the third check, Q3): the server knows each revision's window. An agent's change in
// another file, then the window's own value, then the window's gesture drawn before both: it goes,
// as a value and as a drag's preview, never refused for the window's own change. Another window's
// change of the same value refuses it.
#[test]
fn the_windows_own_change_never_refuses_its_gesture_whatever_came_between() {
    let (dir, running, session) = serving("window");
    let r = Run { url: running.url.clone(), session, seen: BTreeSet::new() };
    let drawn = r.revision();
    let pan = r.knob("song", "hats.pan");
    let bright = r.knob("keys", "brightness");
    let (status, _) = claude(&r, "POST", "/api/graph/set", json!({"revision": drawn, "id": bright, "gesture": "agent", "value": 0.7}));
    assert_eq!(status, 200);
    let (status, _) = win(&r, "POST", "/api/graph/set", json!({"revision": drawn, "id": pan, "gesture": "a", "value": 0.4, "expect": 0.3}));
    assert_eq!(status, 200);
    let (status, a) =
        win(&r, "POST", "/api/graph/preview", json!({"revision": drawn, "id": pan, "gesture": "b", "value": 0.45, "client": "w1"}));
    assert_eq!(status, 200, "a drag drawn before both begins: {a}");
    let (status, a) = win(&r, "POST", "/api/graph/set", json!({"revision": drawn, "id": pan, "gesture": "b", "value": 0.5, "expect": 0.3}));
    assert_eq!(status, 200, "and lands: {a}");
    assert!(std::fs::read_to_string(dir.join("song.ts")).unwrap().contains("pan: 0.5"));
    // Another window's change of the same value.
    let now = r.revision();
    let (_, _) = ask_in(
        &r.url,
        "POST",
        "/api/graph/set",
        "w2",
        &json!({"session": session, "revision": now, "id": pan, "gesture": "c", "value": 0.2}),
    );
    let (status, refused) =
        win(&r, "POST", "/api/graph/set", json!({"revision": now, "id": pan, "gesture": "d", "value": 0.6, "expect": 0.5}));
    assert_ne!(status, 200, "{refused}");
    assert!(std::fs::read_to_string(dir.join("song.ts")).unwrap().contains("pan: 0.2"), "the other window's value is kept");
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
}
