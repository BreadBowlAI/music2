//! What the tests of `m2` with the app open share (AGENT-1): the app, as this process stands for
//! it (its server, with the song open, holding the song's lock as `music2` with this pid), the
//! `session.json` that names it in a home of the test's own, and a fake app for what the real one
//! can't be made to do on cue (quit half way, answer stale).

#![allow(dead_code)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

pub struct Temp(pub PathBuf);

impl Temp {
    pub fn new(name: &str) -> Temp {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let d = std::env::temp_dir().join(format!("m2-{name}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Temp(d)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let (f, t) = (e.path(), to.join(e.file_name()));
        if f.is_dir() { copy(&f, &t) } else { std::fs::copy(&f, &t).map(drop).unwrap() }
    }
}

/// An example of the repository, `name`.
pub fn example(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples").join(name)
}

/// No sound output and no MIDI for this process's server, as for every program the tests run.
pub fn quiet() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    // SAFETY: set once, before this process starts a server (the only reader), and to the same
    // values every test would give.
    ONCE.call_once(|| unsafe {
        std::env::set_var("M2_AUDIO", "none");
        std::env::set_var("M2_MIDI", "none");
    });
}

/// `~/.music2/session.json` in `home`, as the app writes it: its url, the tools key and its pid.
pub fn publish(home: &Path, url: &str, key: &str, pid: u32) {
    std::fs::create_dir_all(home.join(".music2")).unwrap();
    std::fs::write(home.join(".music2/session.json"), json!({"url": url, "token": key, "pid": pid}).to_string()).unwrap();
}

/// `m2 args…` with `home` as HOME, and no name from the environment.
pub fn m2_in(home: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_m2"));
    c.args(args).env("HOME", home).env("M2_AUDIO", "none").env("M2_MIDI", "none").env_remove("M2_AS");
    for (k, v) in env {
        c.env(k, v);
    }
    c.output().unwrap()
}

/// [`m2_in`], started and not waited for: the process, and each line of its standard error as it
/// comes (the process's output keeps only its standard output).
pub fn m2_started(home: &Path, args: &[&str], env: &[(&str, &str)]) -> (Child, Receiver<String>) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_m2"));
    c.args(args).env("HOME", home).env("M2_AUDIO", "none").env("M2_MIDI", "none").env_remove("M2_AS");
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

/// The lines `rx` gives until one says `what`: however long the program takes to get there on a
/// busy machine, but no longer than it runs. Fails, with what it said, when it ends first.
pub fn until_said(rx: &Receiver<String>, what: &str) -> Vec<String> {
    let mut got = vec![];
    // Five minutes stands for "it hangs": the program ends long before when it can't get there.
    while let Ok(l) = rx.recv_timeout(std::time::Duration::from_secs(300)) {
        let done = l.contains(what);
        got.push(l);
        if done {
            return got;
        }
    }
    panic!("nothing said `{what}`: {got:?}");
}

pub fn said(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

/// The app with a copy of example `name` open (prepared by `prepare` first), and `m2`'s home.
pub struct App {
    pub temp: Temp,
    pub song: PathBuf,
    pub home: PathBuf,
    pub running: m2_server::Running,
    pub session: u64,
}

impl App {
    pub fn open(name: &str, prepare: impl FnOnce(&Path)) -> App {
        quiet();
        let temp = Temp::new(&format!("app-{name}"));
        let song = temp.0.join("song");
        copy(&example(name), &song);
        prepare(&song);
        let home = temp.0.join("home");
        let options = m2_server::ServeOptions { port: 0, web_dir: temp.0.join("no-web"), token: "tools".into(), page_token: "page".into() };
        let running = m2_server::start(options).unwrap();
        let session = running.state.graph.open(&song).unwrap()["session"].as_u64().unwrap();
        publish(&home, &running.url, "tools", std::process::id());
        App { temp, song, home, running, session }
    }

    pub fn dir(&self) -> &str {
        self.song.to_str().unwrap()
    }

    pub fn m2(&self, args: &[&str]) -> Output {
        m2_in(&self.home, args, &[])
    }

    pub fn m2_with(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        m2_in(&self.home, args, env)
    }

    /// The step on top of the app's undo, as its state names it.
    pub fn top(&self) -> Value {
        self.running.state.graph.state().unwrap()["undo_name"].clone()
    }

    pub fn read(&self, file: &str) -> String {
        std::fs::read_to_string(self.song.join(file)).unwrap()
    }
}

/// One request a fake app got: its method, its path and its body.
pub type Asked = (String, String, Value);

/// A fake app: an HTTP server on 127.0.0.1 that answers `GET /api/graph` with `state`, and every
/// other request with what `answer` says: a status and a body, or None to close the connection
/// without a word, as an app that quit does; `{"__raw": text}` answers `text` as it is; `{"__ws":
/// [text, …]}` opens a WebSocket, sends those messages and drops it. What it was asked is kept.
pub fn fake(state: Value, answer: impl FnMut(&Asked) -> Option<(u16, Value)> + Send + 'static) -> (String, Arc<Mutex<Vec<Asked>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let asked = Arc::new(Mutex::new(vec![]));
    let kept = asked.clone();
    let answer = Arc::new(Mutex::new(answer));
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() {
                continue;
            }
            let mut parts = line.split_whitespace();
            let (method, path) = (parts.next().unwrap_or("").to_string(), parts.next().unwrap_or("").to_string());
            let (mut length, mut key) = (0, String::new());
            loop {
                let mut h = String::new();
                if reader.read_line(&mut h).is_err() || h == "\r\n" || h.is_empty() {
                    break;
                }
                if let Some((k, v)) = h.split_once(':') {
                    if k.eq_ignore_ascii_case("content-length") {
                        length = v.trim().parse().unwrap_or(0);
                    } else if k.eq_ignore_ascii_case("sec-websocket-key") {
                        key = v.trim().to_string();
                    }
                }
            }
            let mut body = vec![0; length];
            let _ = reader.read_exact(&mut body);
            let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            let asked = (method, path.split('?').next().unwrap_or("").to_string(), body);
            kept.lock().unwrap().push(asked.clone());
            let reply =
                if asked.0 == "GET" && asked.1 == "/api/graph" { Some((200, state.clone())) } else { (answer.lock().unwrap())(&asked) };
            // `{"__ws": [text, …]}` opens the socket, sends each text as a message, then drops the
            // connection with no close frame, as an app that quits does.
            if let Some(Some(sent)) = reply.as_ref().map(|(_, v)| v.get("__ws").and_then(Value::as_array)) {
                let accept = tungstenite::handshake::derive_accept_key(key.as_bytes());
                let _ = write!(
                    stream,
                    "HTTP/1.1 101 Switching Protocols\r\nupgrade: websocket\r\nconnection: upgrade\r\nsec-websocket-accept: {accept}\r\n\r\n"
                );
                for text in sent.iter().filter_map(Value::as_str) {
                    let mut frame = vec![0x81u8];
                    match text.len() {
                        n if n < 126 => frame.push(n as u8),
                        n if n <= u16::MAX as usize => {
                            frame.push(126);
                            frame.extend_from_slice(&(n as u16).to_be_bytes());
                        }
                        n => {
                            frame.push(127);
                            frame.extend_from_slice(&(n as u64).to_be_bytes());
                        }
                    }
                    frame.extend_from_slice(text.as_bytes());
                    let _ = stream.write_all(&frame);
                }
                continue;
            }
            if let Some((status, v)) = reply {
                // `{"__raw": text}` answers `text` as it is, not as JSON, as a server's own refusal is.
                let (kind, text) = match v.get("__raw").and_then(Value::as_str) {
                    Some(raw) => ("text/plain; charset=utf-8", raw.to_string()),
                    None => ("application/json", v.to_string()),
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} X\r\ncontent-type: {kind}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{text}",
                    text.len()
                );
            }
        }
    });
    (url, asked)
}

/// std, as `m2` and the app load it.
pub fn packages() -> m2_runtime::PackageSet {
    let mut p = m2_runtime::PackageSet::default();
    p.load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")).unwrap();
    p
}
