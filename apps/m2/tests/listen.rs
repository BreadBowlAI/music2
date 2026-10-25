//! `m2 listen` (AGENT-7; track 5 of the agents design): an offline range render in a process of
//! its own, at background priority, one at a time, that measures the mix and every track two ways
//! and writes nothing into the song.

mod app;

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use app::{App, Temp, copy, example, said};
use serde_json::{Value, json};

/// One `m2 listen` of these tests at a time, as on a machine (AGENT-7): each test has a home of its
/// own, so `~/.music2/listen.lock` doesn't keep their listens apart. Many at once, each at
/// background priority, share what is left of the machine (on a Mac its two efficiency cores), and
/// each waits far longer for a CPU than one alone: on the Mac's first full check ten ran at once,
/// and a listen of one bar didn't end in a minute.
fn one_listen() -> MutexGuard<'static, ()> {
    static ONE: Mutex<()> = Mutex::new(());
    ONE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// `m2 listen` with `home` as HOME, no device and no MIDI.
fn listen(home: &Path, args: &[&str]) -> std::process::Output {
    let _one = one_listen();
    app::m2_in(home, &[&["listen"], args].concat(), &[])
}

/// What `m2 listen … --json` printed.
fn json_of(o: &std::process::Output) -> Value {
    assert!(o.status.success(), "m2 listen: {}", said(o));
    serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("not JSON ({e}): {}", said(o)))
}

/// Every file of the song but its `.music2/`, by path.
fn tree(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.ends_with(".music2") {
                continue;
            }
            if p.is_dir() {
                stack.push(p);
            } else {
                out.insert(p.strip_prefix(dir).unwrap().to_path_buf(), std::fs::read(&p).unwrap());
            }
        }
    }
    out
}

/// A copy of first-song in a temp folder, and a home of its own.
fn first_song(name: &str) -> (Temp, PathBuf, PathBuf) {
    let temp = Temp::new(name);
    let song = temp.0.join("song");
    copy(&example("first-song"), &song);
    let home = temp.0.join("home");
    std::fs::create_dir_all(&home).unwrap();
    (temp, song, home)
}

fn number(v: &Value, what: &str) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("{what} isn't a number: {v}"))
}

// AGENT-7: the mix and every track, each two ways: its own sound and as it reaches the mix, each
// with its loudness, true peak and crest; the mix and each track as heard with its bands and
// stereo. The whole song is exact. The bars named are that range, approximate when the preroll
// doesn't reach the start, and `--tracks` measures those tracks alone. Nothing is written.
#[test]
fn m2_listen_reports_every_track_and_the_mix() {
    let (_temp, song, home) = first_song("listen-every");
    let dir = song.to_str().unwrap();
    let before = tree(&song);
    let v = json_of(&listen(&home, &[dir, "--json"]));
    assert_eq!((&v["bars"], &v["exact"], &v["as_heard"]), (&json!([1, 8]), &json!(true), &json!(false)), "{v:#}");
    let mix = &v["mix"];
    for k in ["lufs", "lufs_short_max", "true_peak", "peak", "rms", "crest"] {
        number(&mix[k], &format!("the mix's {k}"));
    }
    assert_eq!(mix["bands"].as_object().map(|b| b.len()), Some(10), "{mix:#}");
    number(&mix["stereo"]["correlation"], "the mix's correlation");
    let tracks = v["tracks"].as_object().expect("tracks");
    assert_eq!(tracks.keys().collect::<Vec<_>>(), ["bass", "hats", "keys", "kick"], "{v:#}");
    for (key, t) in tracks {
        for k in ["lufs", "true_peak", "crest"] {
            number(&t["own"][k], &format!("{key}'s own {k}"));
            number(&t["heard"][k], &format!("{key} as heard, {k}"));
        }
        assert_eq!(t["heard"]["bands"].as_object().map(|b| b.len()), Some(10), "{key}: {t:#}");
        assert_eq!(t["channel"], json!(key), "{key}'s channel");
    }
    // The hats are turned down 6 dB in the mixer: quieter as heard than on their own.
    let hats = &tracks["hats"];
    let down = number(&hats["own"]["lufs"], "hats") - number(&hats["heard"]["lufs"], "hats as heard");
    assert!((5.0..8.0).contains(&down), "the hats' fader: {down} dB");
    assert!(v["masking"].is_array() && v["silent"] == json!([]), "{v:#}");
    // Bars 5 to 6, of the kick and the bass: approximate, as the preroll starts at bar 3.
    let v = json_of(&listen(&home, &[dir, "--bars", "5-6", "--tracks", "kick,bass", "--json"]));
    assert_eq!((&v["bars"], &v["exact"]), (&json!([5, 6]), &json!(false)), "{v:#}");
    assert!(v["approximate"].as_str().is_some_and(|s| s.contains("bar 3")), "{v:#}");
    assert_eq!(v["tracks"].as_object().unwrap().keys().collect::<Vec<_>>(), ["bass", "kick"]);
    let seconds = number(&v["seconds"][1], "the end") - number(&v["seconds"][0], "the start");
    assert!((seconds - 2.0 * 4.0 * 60.0 / 124.0).abs() < 1e-3, "two bars at 124 bpm: {seconds}");
    // In words, without --json: the mix, each track, and that these are measures, not listening.
    let o = listen(&home, &[dir]);
    let text = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{}", said(&o));
    for word in ["mix", "kick", "hats", "bass", "keys", "LUFS", "dBTP"] {
        assert!(text.contains(word), "{word}: {text}");
    }
    assert!(text.contains("press V"), "it asks the musician to listen: {text}");
    // Unknown tracks and bars past the end are refused, naming what there is.
    let o = listen(&home, &[dir, "--tracks", "snare"]);
    assert!(!o.status.success() && said(&o).contains("kick"), "{}", said(&o));
    let o = listen(&home, &[dir, "--bars", "9-12"]);
    assert!(!o.status.success() && said(&o).contains("8 bars"), "{}", said(&o));
    assert_eq!(tree(&song), before, "listening writes nothing");
}

/// A Tx that sets the hats' fader in song.ts from −6 dB to 0.
fn hats_up(song: &Path, at: &Path) -> PathBuf {
    let text = std::fs::read_to_string(song.join("song.ts")).unwrap();
    let start = text.find("db: db(-6)").unwrap() + "db: db(".len();
    let sha = m2_song::Sha::of(text.as_bytes()).prefixed();
    let tx = json!({"id": "tx-listen", "base": {"song.ts": sha}, "label": ["hats -6 dB", "hats 0 dB"],
        "ops": [{"op": "text_edit", "file": "song.ts", "edits": [{"start": start, "end": start + 2, "expected": "-6", "text": "0"}]}],
        "expect": {"confined": {"ranges": {"song.ts": [[start, start + 2]]}}}});
    let path = at.join("hats-up.json");
    std::fs::write(&path, tx.to_string()).unwrap();
    path
}

// AGENT-7: `--with` checks a Tx on a read-only session and writes nothing: it renders the song as
// it is and the candidate, and says both and the difference. The hats' fader 6 dB up is 6 dB
// louder as heard, and their own sound doesn't change. With the app open it reads the files the
// same, and the app's song and undo stay as they were.
#[test]
fn m2_listen_with_a_tx_writes_nothing() {
    let (temp, song, home) = first_song("listen-with");
    let dir = song.to_str().unwrap();
    let tx = hats_up(&song, &temp.0);
    let before = tree(&song);
    let v = json_of(&listen(&home, &[dir, "--bars", "1-4", "--with", tx.to_str().unwrap(), "--json"]));
    assert_eq!(tree(&song), before, "nothing written");
    let (was, now) = (&v["before"]["tracks"]["hats"], &v["after"]["tracks"]["hats"]);
    let up = number(&now["heard"]["lufs"], "after") - number(&was["heard"]["lufs"], "before");
    assert!((up - 6.0).abs() < 0.3, "the hats as heard: {up} dB louder");
    let said_up = number(&v["difference"]["tracks"]["hats"]["heard"]["lufs"], "the difference");
    assert!((said_up - up).abs() < 0.11, "the difference says it: {said_up} against {up}");
    let difference = &v["difference"];
    assert!(difference["tracks"]["hats"].get("own").is_none(), "their own sound is as it was: {difference:#}");
    assert!(difference["tracks"].get("bass").is_none(), "the bass is as it was: {difference:#}");
    assert!(number(&difference["mix"]["bands"]["16000"], "the mix at 16 kHz") > 3.0, "the hats lift the mix's highs: {difference:#}");
    // A Tx that doesn't apply is refused, as `m2 tx --check` refuses it, and nothing renders.
    let stale = temp.0.join("stale.json");
    std::fs::write(&stale, std::fs::read_to_string(&tx).unwrap().replace("\"-6\"", "\"-7\"")).unwrap();
    let o = listen(&home, &[dir, "--with", stale.to_str().unwrap(), "--json"]);
    assert!(!o.status.success(), "{}", said(&o));
    // With the app open: the same, and the app's undo is as it was.
    let a = App::open("first-song", |_| {});
    let tx = hats_up(&a.song, &a.temp.0);
    let (top, before) = (a.top(), tree(&a.song));
    let v = json_of(&listen(&a.home, &[a.dir(), "--bars", "1-2", "--with", tx.to_str().unwrap(), "--json"]));
    assert!(v["difference"]["tracks"]["hats"]["heard"]["lufs"].as_f64().unwrap() > 5.0, "{v:#}");
    assert_eq!((a.top(), tree(&a.song)), (top, before), "the app's song and undo unchanged");
}

// AGENT-7, MON-4 (D4): `--as-heard` hears the listening mute and solo, from the open app, else
// `.music2/overlays.json`, and says so; without it, an agent's listen hears every track.
#[test]
fn m2_listen_as_heard_applies_the_listening_mutes_and_says_so() {
    let (_temp, song, home) = first_song("listen-as-heard");
    let dir = song.to_str().unwrap();
    std::fs::create_dir_all(song.join(".music2")).unwrap();
    std::fs::write(song.join(".music2/overlays.json"), r#"{"mute": ["kick"], "solo": []}"#).unwrap();
    let plain = json_of(&listen(&home, &[dir, "--bars", "1-2", "--json"]));
    assert_eq!(plain["as_heard"], json!(false));
    assert_eq!(plain["tracks"]["kick"]["heard"]["silent"], json!(false), "without --as-heard the kick sounds: {plain:#}");
    let heard = json_of(&listen(&home, &[dir, "--bars", "1-2", "--as-heard", "--json"]));
    assert_eq!(heard["as_heard"], json!(true));
    assert_eq!(heard["muted"], json!(["kick"]), "{heard:#}");
    for way in ["own", "heard"] {
        assert_eq!(heard["tracks"]["kick"][way]["silent"], json!(true), "the kick muted, {way}: {heard:#}");
        assert_eq!(heard["tracks"]["bass"][way]["silent"], json!(false), "the bass, {way}");
    }
    assert!(number(&heard["mix"]["lufs"], "as heard") < number(&plain["mix"]["lufs"], "plain"), "the mix without its kick");
    let text = String::from_utf8_lossy(&listen(&home, &[dir, "--bars", "1-2", "--as-heard"]).stdout).into_owned();
    assert!(text.contains("as you hear it") && text.contains("kick"), "{text}");
    // With the app open: its mute and solo, the hats soloed.
    let a = App::open("first-song", |_| {});
    let change = serde_json::from_value(json!({"solo": {"hats": true}})).unwrap();
    a.running.state.graph.overlay(a.session, change).unwrap();
    let v = json_of(&listen(&a.home, &[a.dir(), "--bars", "1-2", "--as-heard", "--json"]));
    assert_eq!((&v["as_heard"], &v["soloed"]), (&json!(true), &json!(["hats"])), "{v:#}");
    assert_eq!((&v["tracks"]["hats"]["heard"]["silent"], &v["tracks"]["keys"]["heard"]["silent"]), (&json!(false), &json!(true)));
}

/// A PNG's chunks, checked: each one's type and data, its CRC right.
fn chunks(png: &[u8]) -> Vec<(String, Vec<u8>)> {
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "a PNG's signature");
    let crc = |bytes: &[u8]| {
        let mut c = 0xffff_ffffu32;
        for &b in bytes {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 == 1 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    };
    let (mut at, mut out) = (8, vec![]);
    while at < png.len() {
        let n = u32::from_be_bytes(png[at..at + 4].try_into().unwrap()) as usize;
        let body = &png[at + 4..at + 8 + n];
        let want = u32::from_be_bytes(png[at + 8 + n..at + 12 + n].try_into().unwrap());
        assert_eq!(crc(body), want, "the CRC of {}", String::from_utf8_lossy(&body[..4]));
        out.push((String::from_utf8_lossy(&body[..4]).into_owned(), body[4..].to_vec()));
        at += 12 + n;
    }
    out
}

/// The rows of pixels of an 8-bit RGB PNG, as a decoder reads them.
fn pixels(png: &[u8]) -> (usize, usize, Vec<Vec<[u8; 3]>>) {
    let c = chunks(png);
    let ihdr = &c[0].1;
    let (w, h) = (u32::from_be_bytes(ihdr[0..4].try_into().unwrap()) as usize, u32::from_be_bytes(ihdr[4..8].try_into().unwrap()) as usize);
    assert_eq!(&ihdr[8..], &[8, 2, 0, 0, 0], "8-bit RGB");
    let mut reader = png::Decoder::new(std::io::Cursor::new(png)).read_info().unwrap();
    let mut raw = vec![0; reader.output_buffer_size().unwrap()];
    reader.next_frame(&mut raw).unwrap();
    assert_eq!(raw.len(), h * 3 * w);
    let rows = raw.chunks(3 * w).map(|r| r.chunks(3).map(|p| [p[0], p[1], p[2]]).collect()).collect();
    (w, h, rows)
}

// AGENT-7: `--png` draws the bands over time: third-octave rows, 50 ms columns, one block for the
// mix and one for each track `--tracks` names. The kick's block is bright low down and dark high
// up; the hats' the other way round.
#[test]
fn m2_listen_png_writes_a_band_picture() {
    let (temp, song, home) = first_song("listen-png");
    let out = temp.0.join("bands.png");
    let v = json_of(&listen(
        &home,
        &[song.to_str().unwrap(), "--bars", "1-4", "--tracks", "kick,hats", "--png", out.to_str().unwrap(), "--json"],
    ));
    let png = &v["png"];
    assert_eq!((&png["path"], &png["blocks"]), (&json!(out.to_str().unwrap()), &json!(["mix", "kick", "hats"])), "{v:#}");
    let columns = number(&png["columns"], "columns") as usize;
    assert_eq!(columns, (4.0 * 4.0 * 60.0 / 124.0 / 0.05) as usize, "50 ms columns");
    let (w, h, rows) = pixels(&std::fs::read(&out).unwrap());
    let (row, gap) = (number(&png["row_px"], "row_px") as usize, number(&png["gap_px"], "gap_px") as usize);
    assert_eq!(h, 3 * 31 * row + 2 * gap, "three blocks of 31 rows");
    assert_eq!(w % columns, 0, "whole pixels a column: {w} for {columns}");
    let bright = |block: usize, band: usize| {
        // Band 0 is the lowest, drawn at the block's bottom.
        let top = block * (31 * row + gap) + (30 - band) * row;
        rows[top..top + row].iter().flatten().map(|p| p.iter().map(|&c| c as u32).sum::<u32>()).sum::<u32>()
    };
    let (low, high) = (3, 27); // 31.5 Hz and 10 kHz
    assert!(bright(1, low) > 2 * bright(1, high), "the kick: low {} against high {}", bright(1, low), bright(1, high));
    assert!(bright(2, high) > 2 * bright(2, low), "the hats: high {} against low {}", bright(2, high), bright(2, low));
}

/// `m2 listen` running in `home`, its standard error read line by line, and this test's turn to
/// listen ([`one_listen`]): kept until the listen ends.
fn spawn_listen(home: &Path, args: &[&str], env: &[(&str, &str)]) -> (Child, std::sync::mpsc::Receiver<String>, MutexGuard<'static, ()>) {
    let one = one_listen();
    let mut c = Command::new(env!("CARGO_BIN_EXE_m2"));
    c.arg("listen").args(args).env("HOME", home).env("M2_AUDIO", "none").env("M2_MIDI", "none").env_remove("M2_AS");
    for (k, v) in env {
        c.env(k, v);
    }
    let mut child = c.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let err = child.stderr.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(err).lines().map_while(Result::ok) {
            let _ = tx.send(line);
        }
    });
    (child, rx, one)
}

/// `~/.music2/listen.lock` in `home`, held by this process.
fn hold_the_lock(home: &Path) -> std::fs::File {
    std::fs::create_dir_all(home.join(".music2")).unwrap();
    let f = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(home.join(".music2/listen.lock")).unwrap();
    f.lock().unwrap();
    f
}

/// Lines from `rx` until one says `what`, or 30 s pass.
fn until_said(rx: &std::sync::mpsc::Receiver<String>, what: &str) -> Vec<String> {
    let (t, mut got) = (Instant::now(), vec![]);
    while t.elapsed() < Duration::from_secs(30) {
        if let Ok(l) = rx.recv_timeout(Duration::from_millis(100)) {
            let done = l.contains(what);
            got.push(l);
            if done {
                return got;
            }
        }
    }
    panic!("nothing said `{what}`: {got:?}");
}

// AGENT-7: one at a time on a machine. A second `m2 listen` waits on `~/.music2/listen.lock`,
// says so, and goes on when the first is done.
#[test]
fn a_second_listen_waits_for_the_first() {
    let (_temp, song, home) = first_song("listen-waits");
    let held = hold_the_lock(&home);
    let (mut child, rx, _one) = spawn_listen(&home, &[song.to_str().unwrap(), "--bars", "1-1", "--json"], &[]);
    until_said(&rx, "waiting for another m2 listen");
    std::thread::sleep(Duration::from_millis(800));
    assert!(child.try_wait().unwrap().is_none(), "it waits while the other listens");
    drop(held);
    let o = child.wait_with_output().unwrap();
    assert!(o.status.success(), "it went on: {}", said(&o));
    json_of(&o);
}

// AGENT-7: its threads and its plugin host process run at background priority. While it waits for
// the lock every thread of `m2 listen` has a nice of 10 or more (Linux); and what it says, read from
// the OS once it rendered a song whose plugin plays in the host process, is background for both.
#[cfg(debug_assertions)]
#[test]
fn m2_listen_runs_at_background_priority() {
    // first-song with the probe plugin on a track of its own, in the mixer.
    let (_temp, song, home) = first_song("listen-priority");
    let put = |rel: &str, bytes: &[u8]| {
        let p = song.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    };
    put(
        "tracks/p.ts",
        b"import { plugin, track } from '@m2/core';\nimport { osc } from 'std/dsp';\n\
          export default track('p', () => plugin('clap:m2.probe', { in: osc.sine(440), key: 'lead' }));\n",
    );
    let code = std::fs::read_to_string(song.join("song.ts")).unwrap();
    let code = code.replace("import kick from \"./tracks/kick\";", "import kick from \"./tracks/kick\";\nimport p from \"./tracks/p\";");
    put("song.ts", code.replace("kick: kick.out,", "kick: kick.out,\n      p: p.out,").as_bytes());
    put("data/p/lead.state.json", br#"{"plugin": "clap:m2.probe", "plugin_version": "1.0.0"}"#);
    put("data/p/lead.state", &m2_plugins::probe::state("", 1.0));
    let held = hold_the_lock(&home);
    let (child, rx, _one) = spawn_listen(&home, &[song.to_str().unwrap(), "--bars", "1-1", "--json"], &[("M2_PROBE", "1")]);
    until_said(&rx, "waiting for another m2 listen");
    #[cfg(target_os = "linux")]
    {
        // Each thread's nice (field 19 of its stat) and scheduling policy (field 41): SCHED_IDLE, 5,
        // the class that runs only when nothing else wants the core (the review of ag-listen, F6).
        let tasks = format!("/proc/{}/task", child.id());
        let field = |s: &str, n: usize| -> i64 { s[s.rfind(')').unwrap() + 2..].split(' ').nth(n - 3).unwrap().parse().unwrap() };
        let threads: Vec<(i64, i64)> = std::fs::read_dir(&tasks)
            .unwrap()
            .flatten()
            .filter_map(|t| std::fs::read_to_string(t.path().join("stat")).ok())
            .map(|s| (field(&s, 19), field(&s, 41)))
            .collect();
        assert!(!threads.is_empty() && threads.iter().all(|&(nice, _)| nice >= 10), "every thread's nice: {threads:?}");
        assert!(threads.iter().all(|&(_, policy)| policy == 5), "every thread's policy is SCHED_IDLE: {threads:?}");
    }
    drop(held);
    let o = child.wait_with_output().unwrap();
    // Its standard error went to `rx`: what it said, should it fail.
    assert!(o.status.success(), "m2 listen: {:?}", the_rest(&rx));
    let v = json_of(&o);
    assert_eq!(v["priority"], json!({"threads": "background", "plugin_host": "background"}), "{v:#}");
}

/// A song in a temp folder: first-song's manifest and tempo, its hats and their clips (8 bars of
/// them), and `files` over them; and a home of its own.
fn song_of(name: &str, files: &[(&str, &str)]) -> (Temp, PathBuf, PathBuf) {
    let temp = Temp::new(name);
    let song = temp.0.join("song");
    let first = example("first-song");
    for rel in ["music2.toml", "data/arrangement.json", "data/hats/hits.json", "tracks/hats.ts"] {
        std::fs::create_dir_all(song.join(rel).parent().unwrap()).unwrap();
        std::fs::copy(first.join(rel), song.join(rel)).unwrap();
    }
    for (rel, text) in files {
        std::fs::create_dir_all(song.join(rel).parent().unwrap()).unwrap();
        std::fs::write(song.join(rel), text).unwrap();
    }
    let home = temp.0.join("home");
    std::fs::create_dir_all(&home).unwrap();
    (temp, song, home)
}

/// A track of std's sine at `hz`, its peaks at `db` dBFS.
fn sine_track(key: &str, hz: f64, db: f64) -> String {
    format!(
        "import {{ db, track }} from \"@m2/core\";\nimport {{ gain, osc }} from \"std/dsp\";\n\nexport default track(\"{key}\", {{ name: \
         \"{key}\" }}, () => gain(osc.sine({hz}), db({db})));\n"
    )
}

/// A Tx that replaces `was` in `file` of the song at `song` with `now`, written beside it as `name`.
fn tx_text(song: &Path, at: &Path, name: &str, file: &str, was: &str, now: &str) -> PathBuf {
    let text = std::fs::read_to_string(song.join(file)).unwrap();
    let start = text.find(was).unwrap_or_else(|| panic!("{was} in {file}"));
    let sha = m2_song::Sha::of(text.as_bytes()).prefixed();
    let tx = json!({"id": format!("tx-{name}"), "base": {file: sha}, "label": [was, now],
        "ops": [{"op": "text_edit", "file": file, "edits": [{"start": start, "end": start + was.len(), "expected": was, "text": now}]}],
        "expect": {"confined": {"ranges": {file: "whole"}}}});
    let path = at.join(format!("{name}.json"));
    std::fs::write(&path, tx.to_string()).unwrap();
    path
}

/// The masking pairs in `v`'s `masking`, each as (band, its two tracks sorted, share).
fn pairs(v: &Value) -> Vec<(f64, Vec<String>, f64)> {
    v.as_array()
        .unwrap_or_else(|| panic!("masking: {v}"))
        .iter()
        .map(|m| {
            let mut t: Vec<String> = m["tracks"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect();
            t.sort();
            (number(&m["hz"], "hz"), t, number(&m["share"], "share"))
        })
        .collect()
}

// The review of ag-listen, F1: a range that starts while a sound is loud reads it as it is. A
// sustained tone with its peaks at -1 dBFS reads the same true peak from bar 2 on, where it starts
// on a peak, as over the whole song: no inter-sample over that the song hasn't got.
#[test]
fn a_range_that_starts_on_a_loud_sound_reads_its_true_peak() {
    let song_ts = "import { song, db } from \"@m2/core\";\nimport { master, mixer } from \"std/mix\";\nimport hats from \"./tracks/hats\";\n\
                   import kick from \"./tracks/kick\";\n\nexport default song(() => master(mixer({ kick: kick.out, hats: { in: hats.out, db: \
                   db(-90) } })));\n";
    let kick = sine_track("kick", 54.896, -1.0);
    let (_temp, song, home) = song_of("listen-cold", &[("song.ts", song_ts), ("tracks/kick.ts", &kick)]);
    let dir = song.to_str().unwrap();
    let whole = json_of(&listen(&home, &[dir, "--json"]));
    let want = number(&whole["mix"]["true_peak"], "the whole song's true peak");
    assert!(want <= -0.95, "the whole song: {want}");
    for bars in ["2-8", "2-2"] {
        let v = json_of(&listen(&home, &[dir, "--bars", bars, "--json"]));
        for (what, tp) in [
            ("the mix", &v["mix"]["true_peak"]),
            ("the kick", &v["tracks"]["kick"]["own"]["true_peak"]),
            ("as heard", &v["tracks"]["kick"]["heard"]["true_peak"]),
        ] {
            let tp = number(tp, what);
            assert!((tp - want).abs() <= 0.05, "--bars {bars}, {what}: {tp} dBTP against the whole song's {want}");
        }
    }
}

/// first-song with `.music2/overlays.json` saying `overlays`.
fn first_song_with(name: &str, overlays: &str) -> (Temp, PathBuf, PathBuf) {
    let (temp, song, home) = first_song(name);
    std::fs::create_dir_all(song.join(".music2")).unwrap();
    std::fs::write(song.join(".music2/overlays.json"), overlays).unwrap();
    (temp, song, home)
}

// The review of ag-listen, F2 and F9: --as-heard mutes tracks by key, in the song as it is and in
// the change: a Tx that takes the kick out of the mixer shifts which tracks come where, and the
// muted keys stay muted after it. The difference says the kick no longer reaches the mix.
#[test]
fn as_heard_mutes_by_key_in_the_change_too() {
    let (temp, song, home) = first_song_with("listen-shift", r#"{"mute": ["keys"], "solo": []}"#);
    let dir = song.to_str().unwrap();
    let tx = tx_text(&song, &temp.0, "drop-kick", "song.ts", "      kick: kick.out,\n", "");
    let v = json_of(&listen(&home, &[dir, "--bars", "1-4", "--as-heard", "--with", tx.to_str().unwrap(), "--json"]));
    for side in ["before", "after"] {
        for way in ["own", "heard"] {
            assert_eq!(v[side]["tracks"]["keys"][way]["silent"], json!(true), "{side}, the keys muted, {way}: {v:#}");
            assert_eq!(v[side]["tracks"]["hats"][way]["silent"], json!(false), "{side}, the hats, {way}");
        }
    }
    let kick = &v["difference"]["tracks"]["kick"]["heard"];
    assert!(kick["was"].is_object() && kick["now"].is_null(), "the kick no longer reaches the mix: {:#}", v["difference"]);
}

// The review of ag-listen, F9: a track that goes silent shows in the difference, in JSON and words.
#[test]
fn the_difference_says_a_track_went_silent() {
    let (temp, song, home) = first_song("listen-silent");
    let dir = song.to_str().unwrap();
    let tx = tx_text(&song, &temp.0, "hats-off", "song.ts", "db: db(-6), pan: 0.3", "db: db(-96), pan: 0.3");
    let v = json_of(&listen(&home, &[dir, "--bars", "1-4", "--with", tx.to_str().unwrap(), "--json"]));
    let hats = &v["difference"]["tracks"]["hats"]["heard"];
    assert_eq!(hats["silent"], json!({"was": false, "now": true}), "{:#}", v["difference"]);
    assert!(hats["lufs"]["was"].is_number() && hats["lufs"]["now"].is_null(), "{:#}", v["difference"]);
    let o = listen(&home, &[dir, "--bars", "1-4", "--with", tx.to_str().unwrap()]);
    assert!(said(&o).contains("hats as heard now silent"), "{}", said(&o));
}

// The review of ag-listen, F8: --with a change of tempo listens to the same bars before and after,
// each at its own tempo, and says so.
#[test]
fn a_change_of_tempo_compares_the_same_bars() {
    let (temp, song, home) = first_song("listen-tempo");
    let dir = song.to_str().unwrap();
    let tx = tx_text(&song, &temp.0, "half-time", "data/arrangement.json", "\"bpm\": 124", "\"bpm\": 62");
    let v = json_of(&listen(&home, &[dir, "--bars", "7-8", "--with", tx.to_str().unwrap(), "--json"]));
    let bar = 4.0 * 60.0 / 124.0;
    let seconds = |side: &str| (number(&v[side]["seconds"][0], side), number(&v[side]["seconds"][1], side));
    let (b, a) = (seconds("before"), seconds("after"));
    assert!((b.0 - 6.0 * bar).abs() < 1e-3 && (b.1 - 8.0 * bar).abs() < 1e-3, "before: {b:?}");
    assert!((a.0 - 12.0 * bar).abs() < 1e-3 && (a.1 - 16.0 * bar).abs() < 1e-3, "after, bars 7-8 at 62 bpm: {a:?}");
    let o = listen(&home, &[dir, "--bars", "7-8", "--with", tx.to_str().unwrap()]);
    assert!(said(&o).contains("tempo or meter"), "it says the bars moved: {}", said(&o));
}

/// A song with a group bus: the kick and the hats go to `drums`, whose fader is at `drums_db`, and
/// the bass to the master; the kick a 60 Hz sine and the bass a 64 Hz one, each at -6 dBFS.
fn grouped(name: &str, drums_db: &str) -> (Temp, PathBuf, PathBuf) {
    let song_ts = format!(
        "import {{ song, db }} from \"@m2/core\";\nimport {{ master, mixer }} from \"std/mix\";\nimport bass from \"./tracks/bass\";\n\
         import drums from \"./tracks/drums\";\nimport hats from \"./tracks/hats\";\nimport kick from \"./tracks/kick\";\n\n\
         export default song(() =>\n  master(\n    mixer({{\n      kick: {{ in: kick.out, to: \"drums\" }},\n      hats: {{ in: hats.out, \
         db: db(-6), to: \"drums\" }},\n      bass: bass.out,\n      drums: {{ in: drums.out, db: db({drums_db}) }},\n    }}),\n  ),\n);\n"
    );
    let drums = "import { bus } from \"@m2/core\";\n\nexport default bus(\"drums\", { name: \"Drums\" }, ({ input }) => input);\n";
    song_of(
        name,
        &[
            ("song.ts", &song_ts),
            ("tracks/drums.ts", drums),
            ("tracks/kick.ts", &sine_track("kick", 60.0, -6.0)),
            ("tracks/bass.ts", &sine_track("bass", 64.0, -6.0)),
        ],
    )
}

// The review of ag-listen, F5: a track that goes to a group bus reaches the mix inside it: the bus
// is measured as a strip of its own, and masking is among what the master sums. With the drums
// group 30 dB down, its kick masks the bass nowhere; with it at 0 dB, the drums and the bass mask
// each other at 63 Hz all the time.
#[test]
fn a_group_bus_is_measured_as_it_reaches_the_mix() {
    let (temp, song, home) = grouped("listen-bus", "-30");
    let dir = song.to_str().unwrap();
    let v = json_of(&listen(&home, &[dir, "--json"]));
    let drums = &v["tracks"]["drums"];
    assert_eq!(drums["channel"], json!("drums"), "{v:#}");
    number(&drums["heard"]["lufs"], "the drums as they reach the mix");
    number(&drums["own"]["lufs"], "the drums' own sound");
    let kick = &v["tracks"]["kick"];
    assert_eq!(kick["to"], json!("drums"), "{v:#}");
    assert!(kick["why"].as_str().is_some_and(|w| w.contains("drums")), "{kick:#}");
    let low: Vec<_> = pairs(&v["masking"]).into_iter().filter(|p| p.0 == 63.0).collect();
    assert_eq!(low, vec![], "the drums 30 dB down mask nothing at 63 Hz: {:#}", v["masking"]);
    let tx = tx_text(&song, &temp.0, "drums-up", "song.ts", "db: db(-30)", "db: db(0)");
    let v = json_of(&listen(&home, &[dir, "--with", tx.to_str().unwrap(), "--json"]));
    let low: Vec<_> = pairs(&v["after"]["masking"]).into_iter().filter(|p| p.0 == 63.0).collect();
    assert!(
        low.len() == 1 && low[0].1 == ["bass", "drums"] && low[0].2 > 0.9,
        "the drums at 0 dB and the bass, at 63 Hz: {:#}",
        v["after"]["masking"]
    );
}

// The review of ag-listen, F4: two tracks that take turns, a bar each, never sound at once, so
// they mask each other hardly at all: only where one's release meets the other's start.
#[test]
fn tracks_that_take_turns_hardly_mask_each_other() {
    let track = |key: &str| {
        format!(
            "import {{ track }} from \"@m2/core\";\nimport {{ bass }} from \"std/instruments\";\n\nexport default track(\"{key}\", {{ name: \
             \"{key}\" }}, ({{ lane }}) => bass(lane.notes(\"notes\")));\n"
        )
    };
    let notes = |first: f64| {
        let n: Vec<Value> = (0..4).map(|i| json!({"pitch": "A1", "start": first + 8.0 * i as f64, "length": 3, "velocity": 0.8})).collect();
        json!({"clips": [{"id": "c", "start": 0, "length": 32, "notes": n}]}).to_string()
    };
    let song_ts = "import { song } from \"@m2/core\";\nimport { master, mixer } from \"std/mix\";\nimport a from \"./tracks/a\";\n\
                   import b from \"./tracks/b\";\n\nexport default song(() => master(mixer({ a: a.out, b: b.out })));\n";
    let (_temp, song, home) = song_of(
        "listen-turns",
        &[
            ("song.ts", song_ts),
            ("tracks/a.ts", &track("a")),
            ("tracks/b.ts", &track("b")),
            ("data/a/notes.json", &notes(0.0)),
            ("data/b/notes.json", &notes(4.0)),
        ],
    );
    let v = json_of(&listen(&home, &[song.to_str().unwrap(), "--json"]));
    for (hz, tracks, share) in pairs(&v["masking"]) {
        assert!(share < 0.05, "{hz} Hz {tracks:?}: {share} of the range: {:#}", v["masking"]);
    }
}

// The review of ag-listen, F11: a track named twice is measured once, and never masks itself.
#[test]
fn a_track_named_twice_is_measured_once() {
    let (_temp, song, home) = first_song("listen-twice");
    let v = json_of(&listen(&home, &[song.to_str().unwrap(), "--bars", "1-2", "--tracks", "kick,kick", "--json"]));
    assert_eq!(v["tracks"].as_object().unwrap().keys().collect::<Vec<_>>(), ["kick"]);
    assert_eq!(v["masking"], json!([]), "{v:#}");
}

// The review of ag-listen, F12: a flag `m2 listen` doesn't know is refused, and so is a flag where
// a value should be; a picture is never written into the song, and one that can't be written is
// refused before anything is rendered.
#[test]
fn m2_listen_refuses_what_it_doesnt_know() {
    let (temp, song, home) = first_song("listen-flags");
    let dir = song.to_str().unwrap();
    for (args, word) in
        [(vec!["--bras", "1-2"], "--bras"), (vec!["--track", "kick"], "--track"), (vec!["--bars", "2", "--png", "--json"], "--png")]
    {
        let o = Command::new(env!("CARGO_BIN_EXE_m2"))
            .arg("listen")
            .arg(dir)
            .args(&args)
            .current_dir(&temp.0)
            .env("HOME", &home)
            .env("M2_AUDIO", "none")
            .env("M2_MIDI", "none")
            .output()
            .unwrap();
        assert!(!o.status.success() && said(&o).contains(word), "{args:?}: {}", said(&o));
    }
    assert!(!temp.0.join("--json").exists(), "no picture named --json");
    let before = tree(&song);
    let inside = song.join("bands.png");
    let o = listen(&home, &[dir, "--bars", "1", "--png", inside.to_str().unwrap()]);
    assert!(!o.status.success() && said(&o).contains("song"), "{}", said(&o));
    assert_eq!(tree(&song), before, "nothing written into the song");
    let o = listen(&home, &[dir, "--png", "/no-such-folder/bands.png"]);
    assert!(!o.status.success() && said(&o).contains("nothing was rendered"), "{}", said(&o));
}

// The review of ag-listen, F13: a song with no clips has no bars to listen to, and says so once,
// the same with --bars as without.
#[test]
fn a_song_with_no_clips_says_so_once() {
    let song_ts = "import { song } from \"@m2/core\";\nimport { master, mixer } from \"std/mix\";\nimport kick from \"./tracks/kick\";\n\n\
                   export default song(() => master(mixer({ kick: kick.out })));\n";
    let (_temp, song, home) = song_of("listen-drone", &[("song.ts", song_ts), ("tracks/kick.ts", &sine_track("kick", 55.0, -6.0))]);
    std::fs::remove_dir_all(song.join("data/hats")).unwrap();
    std::fs::remove_file(song.join("tracks/hats.ts")).unwrap();
    let dir = song.to_str().unwrap();
    let error = |o: &std::process::Output| {
        assert!(!o.status.success(), "{}", said(o));
        String::from_utf8_lossy(&o.stderr).lines().find(|l| l.starts_with("error")).unwrap_or_default().to_string()
    };
    let (plain, bars) = (error(&listen(&home, &[dir])), error(&listen(&home, &[dir, "--bars", "1-4"])));
    assert_eq!(plain, bars, "one message");
    assert!(plain.contains("no clips") && !plain.contains("--bars"), "{plain}");
}

/// A song whose lock a session of this process holds, as the app holds it, with a `session.json`
/// naming a server that takes each request and never answers: an app that is busy for good.
fn held_by_a_silent_app(name: &str) -> (Temp, PathBuf, PathBuf, m2_session::Session) {
    let (temp, song, home) = first_song(name);
    let writer = m2_session::Session::open(&song, app::packages()).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        let mut kept = vec![];
        for s in listener.incoming() {
            kept.push(s);
        }
    });
    app::publish(&home, &url, "tools", std::process::id());
    (temp, song, home, writer)
}

/// `m2 listen args…` in `home`, ended if it takes longer than `limit`: its output, or what it said
/// before it was ended.
fn listen_within(home: &Path, args: &[&str], limit: Duration) -> Result<std::process::Output, Vec<String>> {
    let (mut child, err, _one) = spawn_listen(home, args, &[]);
    let t = Instant::now();
    while t.elapsed() < limit {
        if child.try_wait().unwrap().is_some() {
            return Ok(child.wait_with_output().unwrap());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = child.kill();
    let _ = child.wait();
    Err(the_rest(&err))
}

/// What `rx` still has to say: its lines until it ends, or until 5 s pass with nothing new (a
/// plugin host it started may hold its standard error a moment longer).
fn the_rest(rx: &std::sync::mpsc::Receiver<String>) -> Vec<String> {
    let mut got = vec![];
    while let Ok(l) = rx.recv_timeout(Duration::from_secs(5)) {
        got.push(l);
    }
    got
}

// The review of ag-listen, F15: with the app open, `m2 listen` asks it only for what the range or
// --as-heard need, and gives up on an app that doesn't answer: with bars named it doesn't ask; with
// --as-heard it reads the files' mute and solo, and says the app didn't answer.
#[test]
fn a_busy_app_never_holds_up_a_listen() {
    let (_temp, song, home, _writer) = held_by_a_silent_app("listen-silent-app");
    let dir = song.to_str().unwrap();
    let limit = Duration::from_secs(60);
    let o = listen_within(&home, &[dir, "--bars", "1-1", "--json"], limit)
        .unwrap_or_else(|said| panic!("it didn't wait for the app: {said:?}"));
    let v = json_of(&o);
    assert!(v["app"].is_null(), "it didn't ask: {v:#}");
    let o = listen_within(&home, &[dir, "--bars", "1-1", "--as-heard", "--json"], limit)
        .unwrap_or_else(|said| panic!("it gave up on the app: {said:?}"));
    let v = json_of(&o);
    assert!(v["app"].as_str().is_some_and(|s| s.contains("didn't answer")), "{v:#}");
    assert_eq!(v["as_heard"], json!(true));
}
