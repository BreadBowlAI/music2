//! The open app (AGENT-1): while music2 holds a song's lock, `m2` hands its writes to it instead of
//! failing. Each write is built and checked here, on a read-only session that forwards
//! (`Session::forward_to`), and sent to the app's `POST /api/tx/commit` with the hash of that
//! check, the new assets it names and who sent it, using the url and the tools key in
//! `~/.music2/session.json` (AGENT-10). The app commits it as one undo step, the agent's, or
//! answers busy while the musician holds what it touches (AGENT-3). Plain HTTP/1.1 to 127.0.0.1,
//! as the server's own tests speak it: no client crate.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow, bail};
use m2_session::forward::{Forward, Landed, Sent};
use m2_session::lock::Holder;
use m2_session::{ConflictFile, Refusal};
use m2_song::tx::TxPath;
use serde_json::{Value, json};

/// `m2`'s exit status for busy: nothing changed; try again later (AGENT-3).
pub const BUSY: u8 = 3;

/// What every busy answer ends with.
const DIRECTLY: &str = "Don't write the file directly instead: the app can't refuse that, and it would end what the musician holds.";

/// The app has another song open, or none, or opened this one again since `m2` reached it.
const OPENING: &str = "the app is opening or closing this song; nothing was changed; run this again";

/// Who this `m2` says it is, and why it makes its change (AGENT-10): `--as`, else `M2_AS`; `--why`.
/// The app shows them as plain text, at most 80 characters.
#[derive(Clone, Debug, Default)]
pub struct Who {
    pub by: Option<String>,
    pub why: Option<String>,
}

static WHO: Mutex<Who> = Mutex::new(Who { by: None, why: None });
/// The exit status a refusal asks for, when it isn't 1 (0: none asked).
static EXIT: AtomicU8 = AtomicU8::new(0);
/// What to say once the command said what it did.
static AFTER: Mutex<Vec<String>> = Mutex::new(vec![]);

/// Take `--as <name>` and `--why <words>` off the command line before the command reads it (`m2
/// clips` refuses flags it doesn't know); `--as` wins over `M2_AS`.
pub fn take_who(args: &mut Vec<String>) -> Result<()> {
    let mut who = Who { by: std::env::var("M2_AS").ok().filter(|s| !s.trim().is_empty()), why: None };
    let mut i = 0;
    while i < args.len() {
        let flag = args[i].clone();
        if flag != "--as" && flag != "--why" {
            i += 1;
            continue;
        }
        let Some(value) = args.get(i + 1).cloned() else { bail!("{flag} needs words after it: {flag} \"…\"") };
        match flag.as_str() {
            "--as" => who.by = Some(value),
            _ => who.why = Some(value),
        }
        args.drain(i..i + 2);
    }
    *WHO.lock().unwrap() = who;
    Ok(())
}

fn who() -> Who {
    WHO.lock().unwrap().clone()
}

/// The exit status the last refusal asked for: busy is 3.
pub fn exit_code() -> Option<u8> {
    match EXIT.load(Ordering::Relaxed) {
        0 => None,
        c => Some(c),
    }
}

/// What to say once the command said what it did: that its change is one undo step in the app.
pub fn after() -> Vec<String> {
    std::mem::take(&mut *AFTER.lock().unwrap())
}

fn say_after(line: String) {
    AFTER.lock().unwrap().push(line);
}

/// How long a busy write waits for the musician, retrying every 250 ms: 30 s, or `M2_BUSY_WAIT`
/// seconds (the tests).
fn patience() -> Duration {
    let secs = std::env::var("M2_BUSY_WAIT").ok().and_then(|s| s.parse().ok()).unwrap_or(30.0f64);
    Duration::from_secs_f64(secs.max(0.0))
}

/// Who holds the lock of the song at `dir` now, when it is the app (`music2`): `m2` hands its
/// writes to it. A lock held by another program, or a free one, is None.
pub fn holder(dir: &Path) -> Option<Holder> {
    if !m2_session::lock::taken(dir) {
        return None;
    }
    m2_session::lock::read_holder(&m2_session::lock::lock_path(dir)).filter(|h| h.program == "music2")
}

/// A step as the app names it, after "one undo step, ": an agent's by its name first.
fn step_said(step: &Value) -> String {
    let name = step["name"].as_str().unwrap_or("this change");
    match (step["source"].as_str(), step["by"].as_str()) {
        (Some("agent"), Some(by)) => format!("{by}: {name}"),
        _ => name.to_string(),
    }
}

/// How the connection to the app ended without an answer.
#[derive(Debug)]
enum Gone {
    /// Nothing was sent: the app wasn't there.
    Before,
    /// After the request went: the app may have done it.
    During,
}

/// The app that holds a song's lock, reached: its url, the tools key, its pid, and the song it
/// has open, by the token the app checks each commit against.
pub struct App {
    url: String,
    key: String,
    pid: u32,
    dir: PathBuf,
    session: u64,
    /// The app's state when it was reached: the song's problems, with the fixes they offer.
    state: Value,
    /// It was said that no name was given.
    told: bool,
}

/// Reach the app `holder` names, which holds the lock of the song at `dir`: through
/// `~/.music2/session.json`, which must name the same process, and only while the app has this
/// song open. `also` follows the refusal when it can't be reached.
pub fn reach(dir: &Path, holder: &Holder, also: &str) -> Result<App> {
    let cant = || {
        anyhow!(
            "nothing was changed: this song is open in music2 (pid {}), which m2 can't reach. Make the edit there, or close it there and \
             run this again.{also}",
            holder.pid
        )
    };
    let file = m2_os::dirs::home().ok_or_else(cant)?.join(".music2/session.json");
    let published: Value = std::fs::read_to_string(file).ok().and_then(|t| serde_json::from_str(&t).ok()).ok_or_else(cant)?;
    let (Some(url), Some(key), Some(pid)) = (published["url"].as_str(), published["token"].as_str(), published["pid"].as_u64()) else {
        return Err(cant());
    };
    if pid != u64::from(holder.pid) {
        return Err(cant());
    }
    let dir = dir.canonicalize()?;
    let mut app = App { url: url.into(), key: key.into(), pid: holder.pid, dir: dir.clone(), session: 0, state: Value::Null, told: false };
    let (status, state) = app.ask("GET", "/api/graph", None).map_err(|_| cant())?;
    let open = state["dir"].as_str().and_then(|d| Path::new(d).canonicalize().ok());
    if status != 200 || open.as_deref() != Some(dir.as_path()) {
        bail!("{OPENING}");
    }
    app.session = state["session"].as_u64().ok_or_else(|| anyhow!("{OPENING}"))?;
    app.state = state;
    Ok(app)
}

impl App {
    /// `method path` with `body`, the tools key and `Connection: close`: the status and the JSON
    /// answer. While the app takes its time (a long compile), it waits, saying so every 5 s, as
    /// long as the app's process lives.
    fn ask(&self, method: &str, path: &str, body: Option<&Value>) -> Result<(u16, Value), Gone> {
        let addr = self.url.trim_start_matches("http://");
        let mut s = TcpStream::connect(addr).map_err(|_| Gone::Before)?;
        let body = body.map(Value::to_string).unwrap_or_default();
        let req = format!(
            "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nx-m2-token: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: \
             close\r\n\r\n{body}",
            self.key,
            body.len()
        );
        s.write_all(req.as_bytes()).map_err(|_| Gone::During)?;
        let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
        let mut answer = vec![];
        let mut buf = [0u8; 1 << 16];
        loop {
            match s.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => answer.extend_from_slice(&buf[..n]),
                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                    if !m2_os::process::alive(self.pid) {
                        return Err(Gone::During);
                    }
                    eprintln!("waiting for the app…");
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => return Err(Gone::During),
            }
        }
        parse(&answer).ok_or(Gone::During)
    }

    /// Apply the fix `code` that the problem at `src` offers in the app (the review's m1): the app
    /// knows which journals are still being written; a read-only session never reads them.
    pub fn fix(&self, src: &str, code: &str) -> Result<Value> {
        let problems = self.state["problems"].as_array().cloned().unwrap_or_default();
        let fix = problems
            .iter()
            .filter(|p| p["src"].as_str() == Some(src))
            .flat_map(|p| p["fixes"].as_array().cloned().unwrap_or_default())
            .find(|f| f["code"].as_str() == Some(code))
            .ok_or_else(|| anyhow!("the app offers no `{code}` fix for {src}"))?;
        let w = who();
        self.write("/api/graph/fix", &json!({"session": self.session, "fix": fix, "as": w.by, "why": w.why}), true)
    }

    /// `m2 undo` (AGENT-4): take back the newest step in the app, only while it is this agent's.
    pub fn undo(&self) -> Result<Value> {
        let w = who();
        self.write("/api/graph/undo", &json!({"session": self.session, "as": w.by}), false)
    }

    /// Post a write: its answer, or why it was refused, waiting while the musician holds what it
    /// touches (busy) for up to [`patience`]. `step`: it makes a step, which is said after.
    fn write(&self, path: &str, body: &Value, step: bool) -> Result<Value> {
        let start = Instant::now();
        let mut said = None;
        loop {
            let (status, v) = self.ask("POST", path, Some(body)).map_err(|_| anyhow!("the app went away: nothing was changed"))?;
            match status {
                200 => {
                    if let Some(made) = v.get("undo_name").filter(|s| step && s.is_object()) {
                        say_after(format!("in the open app: one undo step, {}", step_said(made)));
                    }
                    return Ok(v);
                }
                423 if v["busy"].is_object() => match busy(&v["busy"], start, &mut said) {
                    Some(why) => bail!("{why}"),
                    None => continue,
                },
                _ => bail!("{}", words(&v)),
            }
        }
    }

    /// The connection ended with no answer. Whether the commit landed is read from the disk: each
    /// target at the hash the check made means it did; a journal of this Tx, that the app was
    /// killed half way (WRITE-16); each target at its base, that it never started.
    fn gone(&self, gone: Gone, sent: &Sent) -> Result<Landed, Refusal> {
        let io = |message: &str| Err(Refusal::Io { message: message.into() });
        if matches!(gone, Gone::Before) {
            return io("nothing was changed: the app closed first. Run this again; it will write the files itself.");
        }
        let now = |p: &TxPath| current(&self.dir, p);
        if !sent.after.is_empty() && sent.after.iter().all(|(p, a)| now(p) == *a) {
            say_after("it is saved, but its undo step went with the app".into());
            return Ok(Landed::default());
        }
        if journaled(&self.dir, &sent.tx.id) {
            // Its fixes, as every command prints them, once the app let go of the lock.
            crate::attention(&self.dir, &m2_session::journal_problems(&self.dir));
            return io(
                "nothing may have changed: the app stopped while it wrote this. Finish it or put it back with one of the commands above.",
            );
        }
        if sent.tx.base.iter().all(|(p, b)| now(p) == b.sha()) {
            return io("nothing was changed: the app closed first. Run this again; it will write the files itself.");
        }
        io("m2 can't tell whether this landed: the files changed meanwhile. Read the song again before you retry.")
    }
}

impl Forward for App {
    fn commit(&mut self, sent: &Sent) -> Result<Landed, Refusal> {
        let w = who();
        if w.by.is_none() && !self.told {
            self.told = true;
            eprintln!("m2: no name given, so the app shows this change as `agent`'s. Give yours with --as <name>, or M2_AS.");
        }
        let assets: Vec<&str> = sent.assets.iter().map(TxPath::as_str).collect();
        let body = json!({
            "session": self.session, "tx": sent.tx.to_json(), "seen": sent.seen.prefixed(), "assets": assets, "as": w.by, "why": w.why,
        });
        let start = Instant::now();
        let mut said = None;
        loop {
            let (status, v) = match self.ask("POST", "/api/tx/commit", Some(&body)) {
                Ok(answer) => answer,
                Err(gone) => return self.gone(gone, sent),
            };
            return match status {
                200 => {
                    let c = &v["committed"];
                    let n = |k: &str| c[k].as_u64().unwrap_or(0);
                    if let Some(step) = v.get("undo_name").filter(|s| s.is_object()) {
                        say_after(format!("in the open app: one undo step, {}", step_said(step)));
                    }
                    Ok(Landed {
                        step: v["step"].as_u64().unwrap_or(0),
                        number: n("revision"),
                        code_rev: n("code_rev"),
                        data_rev: n("data_rev"),
                    })
                }
                423 if v["busy"].is_object() => match busy(&v["busy"], start, &mut said) {
                    Some(why) => Err(Refusal::Io { message: why }),
                    None => continue,
                },
                409 if v["conflict"].is_object() => Err(conflict(&v["conflict"])),
                422 if v["rejected"].is_object() => Err(rejected(&v["rejected"])),
                _ => Err(Refusal::Io { message: words(&v) }),
            };
        }
    }
}

/// A busy answer (AGENT-3): None to try again after 250 ms, saying once what the musician holds;
/// else why it gave up, and the exit status is busy's. Unsaved code isn't waited for: the musician
/// may be away from the editor for long.
fn busy(b: &Value, start: Instant, said: &mut Option<String>) -> Option<String> {
    let words = b["words"].as_str().unwrap_or("the musician holds what this changes").to_string();
    let give_up = |why: String| {
        EXIT.store(BUSY, Ordering::Relaxed);
        Some(format!("{why} {DIRECTLY}"))
    };
    if b["taken_back"] == json!(true) {
        return give_up(format!("nothing was changed: this was taken back as it landed, because {words}."));
    }
    if b["what"] == json!("code") || start.elapsed() >= patience() {
        return give_up(format!("nothing was changed: {words}."));
    }
    if said.as_deref() != Some(&words) {
        eprintln!("waiting: {words}");
        *said = Some(words);
    }
    std::thread::sleep(Duration::from_millis(250));
    None
}

/// What an answer that isn't 200 says, in words.
fn words(v: &Value) -> String {
    let said = v["error"].as_str().or(v["io"]["message"].as_str()).map(str::to_string).unwrap_or_else(|| v.to_string());
    if said.contains("the open project changed") || said.contains("no project is open") { OPENING.into() } else { said }
}

/// A conflict as the app said it (WRITE-12): each file that changed since this was read.
fn conflict(c: &Value) -> Refusal {
    let files = c["files"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|f| ConflictFile {
            file: f["file"].as_str().unwrap_or_default().into(),
            current: f["current"].as_str().and_then(m2_song::Sha::parse_prefixed),
            clip: f["clip"].as_str().map(str::to_string),
            why: f["why"].as_str().map(str::to_string),
        })
        .collect();
    Refusal::Conflict { files }
}

/// A rejection as the app said it: its diagnostics, by their codes. `m2` is a run of its own, so
/// the few codes it reads are kept for it.
fn rejected(r: &Value) -> Refusal {
    let diagnostics = r["diagnostics"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|d| {
            let code: &'static str = Box::leak(d["code"].as_str().unwrap_or("tx.rejected").to_string().into_boxed_str());
            m2_song::Diagnostic::error(code, d["message"].as_str().unwrap_or("refused by the app"))
        })
        .collect();
    Refusal::Rejected { diagnostics, unexpected: None }
}

/// An HTTP/1.1 answer: its status and its JSON body (`Content-Length` or chunked).
fn parse(answer: &[u8]) -> Option<(u16, Value)> {
    let text = std::str::from_utf8(answer).ok()?;
    let (head, body) = text.split_once("\r\n\r\n")?;
    let status = head.get(9..12)?.parse().ok()?;
    let chunked = head.lines().any(|l| l.to_ascii_lowercase().starts_with("transfer-encoding:") && l.contains("chunked"));
    let body = if chunked {
        let (mut out, mut rest) = (String::new(), body);
        loop {
            let (size, after) = rest.split_once("\r\n")?;
            let n = usize::from_str_radix(size.trim(), 16).ok()?;
            if n == 0 {
                break;
            }
            out.push_str(after.get(..n)?);
            rest = after.get(n + 2..)?;
        }
        out
    } else {
        body.to_string()
    };
    Some((status, serde_json::from_str(&body).unwrap_or(Value::Null)))
}

/// What `p` holds on disk now, by its hash (a folder's directory hash): None when nothing is there,
/// or it can't be read.
fn current(dir: &Path, p: &TxPath) -> Option<m2_song::Sha> {
    let path = dir.join(p.path());
    match std::fs::symlink_metadata(&path) {
        Ok(m) if m.is_dir() => m2_session::tx::stage::folder_sha(&mut m2_session::tx::stage::Real, dir, p.as_str()).ok().flatten(),
        Ok(_) => std::fs::read(&path).ok().map(|b| m2_song::Sha::of(&b)),
        Err(_) => None,
    }
}

/// Whether `.music2/tx/` holds a journal of Tx `id`: a commit of it the app began and didn't end.
fn journaled(dir: &Path, id: &str) -> bool {
    let Ok(entries) = std::fs::read_dir(dir.join(".music2/tx")) else { return false };
    entries.flatten().any(|e| {
        e.path().extension().is_some_and(|x| x == "json")
            && std::fs::read_to_string(e.path())
                .ok()
                .and_then(|t| m2_session::tx::journal::Journal::parse(&t).ok())
                .is_some_and(|j| j.tx == id)
    })
}
