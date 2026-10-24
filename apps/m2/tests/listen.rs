//! `m2 listen` (AGENT-7; track 5 of the agents design): an offline range render in a process of
//! its own, at background priority, one at a time, that measures the mix and every track two ways
//! and writes nothing into the song.

mod app;

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use app::{App, Temp, copy, example, said};
use serde_json::{Value, json};

/// `m2 listen` with `home` as HOME, no device and no MIDI.
fn listen(home: &Path, args: &[&str]) -> std::process::Output {
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

/// The rows of pixels of an RGB PNG whose deflate stream is stored blocks, as `m2 listen` writes.
fn pixels(png: &[u8]) -> (usize, usize, Vec<Vec<[u8; 3]>>) {
    let c = chunks(png);
    let ihdr = &c[0].1;
    let (w, h) = (u32::from_be_bytes(ihdr[0..4].try_into().unwrap()) as usize, u32::from_be_bytes(ihdr[4..8].try_into().unwrap()) as usize);
    assert_eq!(&ihdr[8..], &[8, 2, 0, 0, 0], "8-bit RGB");
    let z: Vec<u8> = c.iter().filter(|c| c.0 == "IDAT").flat_map(|c| c.1.clone()).collect();
    let (mut at, mut raw) = (2, vec![]);
    loop {
        let last = z[at] & 1 == 1;
        assert_eq!(z[at] >> 1, 0, "stored blocks");
        let n = u16::from_le_bytes([z[at + 1], z[at + 2]]) as usize;
        raw.extend_from_slice(&z[at + 5..at + 5 + n]);
        at += 5 + n;
        if last {
            break;
        }
    }
    assert_eq!(raw.len(), h * (1 + 3 * w));
    let rows = raw.chunks(1 + 3 * w).map(|r| r[1..].chunks(3).map(|p| [p[0], p[1], p[2]]).collect()).collect();
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

/// `m2 listen` running in `home`, its standard error read line by line.
fn spawn_listen(home: &Path, args: &[&str], env: &[(&str, &str)]) -> (Child, std::sync::mpsc::Receiver<String>) {
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
    (child, rx)
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
    let (mut child, rx) = spawn_listen(&home, &[song.to_str().unwrap(), "--bars", "1-1", "--json"], &[]);
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
    let (child, rx) = spawn_listen(&home, &[song.to_str().unwrap(), "--bars", "1-1", "--json"], &[("M2_PROBE", "1")]);
    until_said(&rx, "waiting for another m2 listen");
    #[cfg(target_os = "linux")]
    {
        let tasks = format!("/proc/{}/task", child.id());
        let nice: Vec<i64> = std::fs::read_dir(&tasks)
            .unwrap()
            .flatten()
            .filter_map(|t| std::fs::read_to_string(t.path().join("stat")).ok())
            .map(|s| s[s.rfind(')').unwrap() + 2..].split(' ').nth(16).unwrap().parse().unwrap())
            .collect();
        assert!(!nice.is_empty() && nice.iter().all(|&n| n >= 10), "every thread's nice: {nice:?}");
    }
    drop(held);
    let v = json_of(&child.wait_with_output().unwrap());
    assert_eq!(v["priority"], json!({"threads": "background", "plugin_host": "background"}), "{v:#}");
}
