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
        let _ = std::fs::remove_dir_all(&self.0);
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
    let yours = home.0.join("Music/music2/Presets/std/synth-pad");
    std::fs::create_dir_all(&yours).unwrap();
    std::fs::write(
        yours.join("Night.json"),
        "{\n  \"m2preset\": 1,\n  \"name\": \"Night\",\n  \"sound\": \"std:synth-pad\",\n  \"values\": {}\n}\n",
    )
    .unwrap();

    let o = m2_in(&home.0, &["search", "reverb"]);
    assert!(o.status.success(), "{o:?}");
    let first = out(&o).lines().next().unwrap_or_default().to_string();
    assert!(first.starts_with("Reverb ") && first.contains("effect") && first.ends_with("std:reverb"), "the best first: {first:?}");

    let o = m2_in(&home.0, &["search", "night", "--json"]);
    let items: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap_or_else(|_| panic!("{o:?}"));
    assert_eq!(items[0]["id"], "sound:yours/std:synth-pad/Night", "your presets are sounds: {items}");
    assert_eq!(items[0]["package"], "yours");

    let o = m2_in(&home.0, &["search", "pluck", "--kind", "sound", "--json"]);
    let items: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    let ids: Vec<&str> = items.as_array().unwrap().iter().map(|i| i["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["sound:std/std:synth-pluck/Glass", "sound:std/std:synth-pluck/Soft"], "--kind sound: only sounds");

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
