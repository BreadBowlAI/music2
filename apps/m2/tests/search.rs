//! LIB-2: `m2 search` finds what can be added with the library's one search: through the open app,
//! or with the app closed by gathering the same list itself, from std and your presets.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Output};

/// A scratch HOME, removed after.
struct Home(std::path::PathBuf);

impl Home {
    fn new(tag: &str) -> Home {
        let dir = std::env::temp_dir().join(format!("m2-search-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Home(dir)
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        // An installed pack's files and folders are read-only (PACK-6).
        writable(&self.0);
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Make everything under `path` writable again, links not followed.
fn writable(path: &Path) {
    let Ok(m) = std::fs::symlink_metadata(path) else { return };
    if m.file_type().is_symlink() {
        return;
    }
    let mut p = m.permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    p.set_readonly(false);
    let _ = std::fs::set_permissions(path, p);
    if m.is_dir() {
        for e in std::fs::read_dir(path).into_iter().flatten().flatten() {
            writable(&e.path());
        }
    }
}

fn m2_in(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_m2")).env("HOME", home).env("M2_AUDIO", "none").env("M2_MIDI", "none").args(args).output().unwrap()
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

// LIB-2: with the app closed, m2 gathers the library itself (std, your presets) and ranks it with
// the same search: one line an item, the best first; --json prints the items; --kind filters.
#[test]
fn with_the_app_closed_m2_searches_std_and_your_presets() {
    let home = Home::new("closed");
    let yours = home.0.join("Music/music2/Presets/_m2_synths/synth-pad");
    std::fs::create_dir_all(&yours).unwrap();
    std::fs::write(
        yours.join("Night.json"),
        "{\n  \"m2preset\": 1,\n  \"name\": \"Night\",\n  \"sound\": \"@m2/synths:synth-pad\",\n  \"values\": {}\n}\n",
    )
    .unwrap();

    let o = m2_in(&home.0, &["search", "reverb"]);
    assert!(o.status.success(), "{o:?}");
    let first = out(&o).lines().next().unwrap_or_default().to_string();
    assert!(first.starts_with("Reverb ") && first.contains("effect") && first.ends_with("@m2/effects:reverb"), "the best first: {first:?}");

    let o = m2_in(&home.0, &["search", "night", "--json"]);
    let items: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap_or_else(|_| panic!("{o:?}"));
    assert_eq!(items[0]["id"], "sound:yours/@m2/synths:synth-pad/Night", "your presets are sounds: {items}");
    assert_eq!(items[0]["package"], "yours");

    let o = m2_in(&home.0, &["search", "pluck", "--kind", "sound", "--json"]);
    let items: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    let ids: Vec<&str> = items.as_array().unwrap().iter().map(|i| i["id"].as_str().unwrap()).collect();
    assert_eq!(
        ids,
        [
            "sound:std/@m2/wavetable:wavetable/Short pluck",
            "sound:std/@m2/synths:synth-pluck/Glass",
            "sound:std/@m2/synths:synth-pluck/Soft"
        ],
        "--kind sound: only sounds"
    );

    let o = m2_in(&home.0, &["search", "zzzq"]);
    assert!(o.status.success() && out(&o).is_empty(), "{o:?}");
    assert!(String::from_utf8_lossy(&o.stderr).contains("Nothing in the library matches zzzq"), "{o:?}");

    let o = m2_in(&home.0, &["search", "pad", "--kind", "synth"]);
    assert!(
        !o.status.success() && String::from_utf8_lossy(&o.stderr).contains("--kind is instrument, sound, effect, sample or plugin"),
        "{o:?}"
    );
}

// LIB-2: with the app open, m2 asks it (`GET /api/library`, with the tools key), whose list holds
// the open song's components and the plugins it has listed.
#[test]
fn with_the_app_open_m2_asks_it() {
    let home = Home::new("open");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::fs::create_dir_all(home.0.join(".music2")).unwrap();
    let published = serde_json::json!({"url": url, "token": "k3y", "pid": std::process::id()});
    std::fs::write(home.0.join(".music2/session.json"), published.to_string()).unwrap();
    // The app, as far as m2 search sees it: one answer, and what was asked.
    // It waits a minute at most for m2 to ask, so a run that never asks fails rather than hangs.
    listener.set_nonblocking(true).unwrap();
    let app = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let mut s = loop {
            match listener.accept() {
                Ok((s, _)) => break s,
                Err(_) if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(20)),
                Err(_) => return vec!["nothing was asked".to_string()],
            }
        };
        s.set_nonblocking(false).unwrap();
        let mut head = vec![];
        let mut r = BufReader::new(s.try_clone().unwrap());
        loop {
            let mut line = String::new();
            r.read_line(&mut line).unwrap();
            if line == "\r\n" || line.is_empty() {
                break;
            }
            head.push(line.trim_end().to_string());
        }
        let body = r#"{"items": [{"kind": "plugin", "id": "clap:com.example.synth", "label": "Example Synth", "category": "Plugins", "description": "", "tags": ["Example"], "package": "plugin", "component": "clap:com.example.synth", "instrument": true, "preview": false}]}"#;
        write!(s, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
            .unwrap();
        // Closed, as an answer to `Connection: close` is.
        let _ = s.shutdown(std::net::Shutdown::Both);
        head
    });
    let o = m2_in(&home.0, &["search", "warm", "synth", "--kind", "plugin"]);
    let head = app.join().unwrap();
    assert_eq!(head[0], "GET /api/library?q=warm%20synth%20plugin HTTP/1.1", "{head:?}");
    assert!(head.iter().any(|h| h.eq_ignore_ascii_case("x-m2-token: k3y")), "with the tools key: {head:?}");
    assert!(o.status.success(), "{o:?}");
    assert!(out(&o).starts_with("Example Synth ") && out(&o).contains("clap:com.example.synth"), "{}", out(&o));
}

/// A 16-bit mono WAV of `seconds` of a tone at 48 kHz, made byte by byte (no WAV crate here).
fn tone(path: &Path, seconds: f64) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let frames = (seconds * 48_000.0) as u32;
    let mut b = vec![];
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + frames * 2).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&48_000u32.to_le_bytes());
    b.extend_from_slice(&96_000u32.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&(frames * 2).to_le_bytes());
    for i in 0..frames {
        b.extend_from_slice(&(((i as f32 / 20.0).sin() * 12_000.0) as i16).to_le_bytes());
    }
    std::fs::write(path, b).unwrap();
}

/// A copy of `from` at `to`.
fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let (f, t) = (e.path(), to.join(e.file_name()));
        if f.is_dir() { copy(&f, &t) } else { std::fs::copy(&f, &t).map(drop).unwrap() }
    }
}

// LIB-2, LIB-4, LIB-5: with the app closed, m2 search finds std's kit and the samples of your
// folders (kept in the settings), and says what it skipped.
#[test]
fn with_the_app_closed_m2_searches_samples() {
    let home = Home::new("samples");
    let mine = home.0.join("My Samples");
    tone(&mine.join("Kicks/Deep Kick.wav"), 0.2);
    let flac = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../core/runtime/tests/fixtures/flac/16bit-mono.flac");
    std::fs::copy(flac, mine.join("Kicks/Old Kick.flac")).unwrap();
    std::fs::write(mine.join("Kicks/old.mp3"), b"ID3 not read").unwrap();
    std::fs::create_dir_all(home.0.join(".music2")).unwrap();
    std::fs::write(home.0.join(".music2/settings.json"), serde_json::json!({"samples": [mine]}).to_string()).unwrap();
    let o = m2_in(&home.0, &["search", "kick", "--kind", "sample", "--json"]);
    let items: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap_or_else(|_| panic!("{o:?}"));
    let ids: Vec<&str> = items.as_array().unwrap().iter().map(|i| i["id"].as_str().unwrap()).collect();
    let deep = format!("sample:folder:{}", mine.join("Kicks/Deep Kick.wav").display());
    let old = format!("sample:folder:{}", mine.join("Kicks/Old Kick.flac").display());
    assert_eq!(ids, ["sample:std:samples/kit/kick.wav", deep.as_str(), old.as_str()], "std's first, then yours, a FLAC too");
    let said = String::from_utf8_lossy(&o.stderr);
    assert!(said.contains("1 audio files that aren't WAV or FLAC were skipped: WAV and FLAC only for now"), "{o:?}");
    assert!(home.0.join(".cache/music2/samples").is_dir() || home.0.join("Library/Caches/music2/samples").is_dir(), "measures kept");
}

// LIB-7: m2 add-sample, with the app closed, copies the sample with its note and places its clip
// on a new audio track, or on a track's audio lane with --track.
#[test]
fn m2_adds_a_sample_with_the_app_closed() {
    let home = Home::new("add-sample");
    let song = home.0.join("song");
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song"), &song);
    let s = song.to_str().unwrap();
    let o = m2_in(&home.0, &["add-sample", s, "sample:std:samples/kit/kick.wav", "--at", "2"]);
    assert!(o.status.success(), "{o:?}");
    let said = out(&o);
    assert!(said.starts_with("added assets/kick.wav, its note in assets/kick.wav.json\nonto a new track Kick"), "{said}");
    assert_eq!(
        std::fs::read(song.join("assets/kick.wav")).unwrap(),
        std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std/samples/kit/kick.wav")).unwrap()
    );
    let note: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(song.join("assets/kick.wav.json")).unwrap()).unwrap();
    assert_eq!(note["from"]["pack"], "std");
    assert_eq!(note["from"]["file"], "samples/kit/kick.wav");
    let o = m2_in(&home.0, &["add-sample", s, "sample:folder:/etc/hosts"]);
    assert!(!o.status.success() && String::from_utf8_lossy(&o.stderr).contains("There is no sample sample:folder:/etc/hosts"), "{o:?}");
}

/// A pack of samples alone, `@alice/loops` at `version`, in `dir`: a 90 bpm loop of keys.
fn loops_pack(dir: &Path, version: &str) {
    std::fs::create_dir_all(dir).unwrap();
    let toml = format!(
        "[package]\nname = \"@alice/loops\"\nversion = \"{version}\"\nlicense = \"CC-BY-4.0\"\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.7\"\n"
    );
    std::fs::write(dir.join("m2pkg.toml"), toml).unwrap();
    std::fs::write(dir.join("samples.toml"), "[folder.\"loops\"]\n").unwrap();
    tone(&dir.join("samples/loops/groove 90.wav"), 2.7);
}

/// The first song in `home`, taking `@alice/loops` by `line` (its manifest's right side).
fn song_taking(home: &Path, line: &str) -> std::path::PathBuf {
    let song = home.join("song");
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song"), &song);
    let manifest = std::fs::read_to_string(song.join("music2.toml")).unwrap();
    std::fs::write(song.join("music2.toml"), format!("{manifest}\"@alice/loops\" = {line}\n")).unwrap();
    song
}

// LIB-7 (review D7): with the app closed, m2 add-sample finds a sample in the library the song's own
// build lists: a pack it takes from a folder, and a store's pack at the version its lock holds, not
// the newest installed; the note says that version.
#[test]
fn m2_adds_a_songs_own_packs_sample_at_its_locked_version() {
    let home = Home::new("own-packs");
    let pack = home.0.join("mypack");
    loops_pack(&pack, "0.3.0");
    let song = song_taking(&home.0, &format!("{{ path = {} }}", serde_json::to_string(pack.to_str().unwrap()).unwrap()));
    let id = "sample:@alice/loops:samples/loops/groove 90.wav";
    let o = m2_in(&home.0, &["add-sample", song.to_str().unwrap(), id]);
    assert!(o.status.success(), "{o:?}");
    let note: serde_json::Value = serde_json::from_slice(&std::fs::read(song.join("assets/groove 90.wav.json")).unwrap()).unwrap();
    assert_eq!(note["from"]["version"], "0.3.0", "{note}");

    // From the store: 1.2.0 locked, 1.3.0 installed since.
    let home = Home::new("locked");
    for v in ["1.2.0", "1.3.0"] {
        let dir = home.0.join(format!("pack-{v}"));
        loops_pack(&dir, v);
        if v == "1.3.0" {
            let s = song_taking(&home.0, "\"1\"");
            let o = m2_in(&home.0, &["pack", "lock", s.to_str().unwrap()]);
            assert!(o.status.success(), "{o:?}");
        }
        let o = m2_in(&home.0, &["pack", "install", dir.to_str().unwrap()]);
        assert!(o.status.success(), "{o:?}");
    }
    let song = home.0.join("song");
    let o = m2_in(&home.0, &["add-sample", song.to_str().unwrap(), id]);
    assert!(o.status.success(), "{o:?}");
    let note: serde_json::Value = serde_json::from_slice(&std::fs::read(song.join("assets/groove 90.wav.json")).unwrap()).unwrap();
    assert_eq!(note["from"]["version"], "1.2.0", "the song's own version: {note}");
}
