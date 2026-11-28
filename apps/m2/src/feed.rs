//! `m2 feed` (FEED-6): an agent plays a track live from the terminal. A feed is a track's notes
//! written ahead of the playhead; the open app plays them while the musician plays the song, and
//! says when its buffer runs low. Each form reaches the app that has the song open, with the tools
//! key (AGENT-1): feeds are playback, so they need the app open.

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};

use crate::app::{self, App};
use crate::words::{on, places, value};

/// `m2 feed <project> …`, with the words `words::read` handed on: its usage took them (AGENT-13).
pub fn run(args: &[String]) -> Result<()> {
    let words = places(args);
    let (dir, act, rest) = (Path::new(&words[0]), words[1].as_str(), &words[2..]);
    let app = app::looking(dir).map_err(|e| anyhow!("{e} (a feed is playback: it needs the app open, with this song)"))?;
    let by = app::who().by;
    match act {
        "open" => {
            let mut body = json!({"session": app.session(), "track": rest[0], "as": by});
            if let Some(low) = value(args, "--low") {
                body["low_bars"] = json!(number(&low, "--low")?);
            }
            let v = asked(&app, "POST", "/api/feed/open", &body)?;
            println!(
                "a feed is open on {} (its lane {}): write its notes ahead of the playhead with `m2 feed {} put {} <from> <to> -`; it says low under {} bars",
                v["feed"].as_str().unwrap_or_default(),
                v["lane"].as_str().unwrap_or_default(),
                words[0],
                rest[0],
                v["low_bars"]
            );
        }
        "put" => {
            let (from, to) = (number(&rest[1], "<from>")?, number(&rest[2], "<to>")?);
            let text = match rest[3].as_str() {
                "-" => std::io::read_to_string(std::io::stdin())?,
                path => std::fs::read_to_string(path).map_err(|e| anyhow!("{path} can't be read: {e}"))?,
            };
            let notes: Value = serde_json::from_str(&text).map_err(|e| {
                anyhow!("the notes don't read ({e}): give a list, such as [{{\"pitch\": \"A1\", \"start\": 64, \"length\": 0.5}}], starts in song beats")
            })?;
            if !notes.is_array() {
                bail!("the notes are a JSON list, such as [{{\"pitch\": \"A1\", \"start\": 64, \"length\": 0.5}}]");
            }
            let body = json!({"session": app.session(), "feed": rest[0], "from": from, "to": to, "notes": notes, "as": by});
            let v = asked(&app, "POST", "/api/feed/put", &body)?;
            println!("{v}");
        }
        "status" => {
            let v = asked(&app, "GET", "/api/feed", &Value::Null)?;
            let feeds = v["feeds"].as_array().cloned().unwrap_or_default();
            if on(args, "--json") {
                println!("{}", serde_json::to_string_pretty(&v)?);
            } else if feeds.is_empty() {
                println!("no feed is open");
            } else {
                for f in &feeds {
                    println!("{}", said(f));
                }
            }
        }
        "wait" => wait(&app, rest.first().map(String::as_str), args)?,
        "keep" => {
            let into = value(args, "--into");
            if let Some(i) = into.as_deref().filter(|i| !matches!(*i, "clip" | "version")) {
                bail!("`--into` is `clip` or `version`, not `{i}`");
            }
            let body = json!({"session": app.session(), "feed": rest[0], "into": into, "as": by});
            let v = asked(&app, "POST", "/api/feed/keep", &body)?;
            match v["as"].as_str() {
                Some("version") => println!(
                    "kept {} notes that played on {} as the version `{}` of clip {}/{}: it waits beside what plays; one undo step in the app",
                    v["kept"],
                    rest[0],
                    v["version"].as_str().unwrap_or_default(),
                    rest[0],
                    v["clip"].as_str().unwrap_or_default()
                ),
                _ => println!(
                    "kept {} notes that played on {} as a new clip from beat {} ({} beats): one undo step in the app",
                    v["kept"], rest[0], v["start"], v["length"]
                ),
            }
        }
        "close" => {
            asked(&app, "POST", "/api/feed/close", &json!({"session": app.session(), "feed": rest[0], "as": by}))?;
            println!(
                "the feed on {} is closed: its notes ended; what played can still be kept (`m2 feed {} keep {}`)",
                rest[0], words[0], rest[0]
            );
        }
        _ => unreachable!("its usage takes only these"),
    }
    Ok(())
}

/// The longest `wait`, in seconds: a day.
const LONGEST: f64 = 86_400.0;

/// `text` as a number, for `what`.
fn number(text: &str, what: &str) -> Result<f64> {
    text.parse::<f64>().ok().filter(|x| x.is_finite()).ok_or_else(|| anyhow!("{what} is a number, not `{text}`"))
}

/// The app's answer to `method path` with `body`, or its words when it refuses.
fn asked(app: &App, method: &str, path: &str, body: &Value) -> Result<Value> {
    let body = (!body.is_null()).then_some(body);
    let (status, v) = app.looked(method, path, body)?;
    if status != 200 {
        bail!("{}", app::words(&v));
    }
    Ok(v)
}

/// A feed's state in words.
fn said(f: &Value) -> String {
    let n = |k: &str| f[k].as_f64().unwrap_or(0.0);
    let b = &f["buffer"];
    format!(
        "{}: {}, the playhead at beat {:.2}, written to beat {:.2}: {:.2} beats ahead ({:.2} bars, {:.1} s); low under {} bars; {} empties, {} notes late{}",
        f["feed"].as_str().unwrap_or_default(),
        if f["playing"] == json!(true) { "playing" } else { "stopped" },
        n("playhead"),
        n("written_to"),
        b["beats"].as_f64().unwrap_or(0.0),
        b["bars"].as_f64().unwrap_or(0.0),
        b["seconds"].as_f64().unwrap_or(0.0),
        f["low_bars"],
        f["empties"],
        f["late"],
        f["writer"].as_str().map(|w| format!("; written by {w}")).unwrap_or_default()
    )
}

/// Whether event `v` is what `wait` waits for: `what` (low takes empty too, lower still), or any of
/// low, empty and jump.
fn matches(v: &Value, what: Option<&str>) -> bool {
    let why = v["why"].as_str().unwrap_or_default();
    match what {
        Some("low") => matches!(why, "low" | "empty"),
        Some(w) => why == w,
        None => matches!(why, "low" | "empty" | "jump"),
    }
}

/// `m2 feed <project> wait [<track>] [--for low|empty|jump] --timeout <seconds>` (FEED-6): the
/// first feed event that matches, printed as one JSON line; at once when the feed is already low
/// (or empty). Exits 1 when the time limit passes first.
fn wait(app: &App, track: Option<&str>, args: &[String]) -> Result<()> {
    let what = value(args, "--for");
    if let Some(w) = what.as_deref().filter(|w| !matches!(*w, "low" | "empty" | "jump")) {
        bail!("`--for` is `low`, `empty` or `jump`, not `{w}`");
    }
    let secs = value(args, "--timeout").map(|t| number(&t, "--timeout")).transpose()?.unwrap_or(0.0);
    if !(secs > 0.0 && secs <= LONGEST) {
        bail!("`--timeout` is a number of seconds above 0 and at most {LONGEST} (a day): every wait has a time limit, not {secs}");
    }
    let deadline = Instant::now() + Duration::from_secs_f64(secs);
    // Listening first, so nothing told between the look and the socket is missed.
    let mut socket = m2_server::socket::Socket::open(app.url(), &format!("/api/ws?token={}", app.key()))
        .map_err(|e| anyhow!("couldn't follow the app: {e}"))?;
    let ours = |f: &Value| track.is_none_or(|t| f["feed"] == t);
    let state = asked(app, "GET", "/api/feed", &Value::Null)?;
    let feeds: Vec<Value> = state["feeds"].as_array().cloned().unwrap_or_default().into_iter().filter(|f| ours(f)).collect();
    if feeds.is_empty() {
        bail!("{}", track.map_or("no feed is open".to_string(), |t| format!("{t} has no feed open")));
    }
    // Already so: said at once.
    if matches!(what.as_deref(), Some("low" | "empty")) {
        for f in &feeds {
            // Judged as the server judges it: by the low mark it says, which a loop can make shorter.
            let (beats, low) = (f["buffer"]["beats"].as_f64().unwrap_or(0.0), f["low_mark"]["beats"].as_f64().unwrap_or(0.0));
            let why = if beats <= 1e-9 {
                Some("empty")
            } else if beats < low - 1e-9 {
                Some("low")
            } else {
                None
            };
            if let Some(why) = why.filter(|w| what.as_deref() == Some("low") || what.as_deref() == Some(*w)) {
                let mut v = f.clone();
                v["t"] = json!("feed");
                v["why"] = json!(why);
                println!("{v}");
                return Ok(());
            }
        }
    }
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            bail!("no feed event came in {secs} s (`--timeout`)");
        }
        socket.wait_at_most(Some(left.min(Duration::from_millis(250))))?;
        match socket.message() {
            Ok(Some(m)) => {
                let Ok(v) = serde_json::from_str::<Value>(&m) else { continue };
                if v["t"] == "feed" && v["session"] == json!(app.session()) && ours(&v) && matches(&v, what.as_deref()) {
                    println!("{v}");
                    return Ok(());
                }
                if v["t"] == "graph" && v.get("session").is_some_and(|s| *s != json!(app.session())) {
                    bail!("the app closed this song");
                }
            }
            Ok(None) => bail!("the app went away"),
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(e) => bail!("couldn't follow the app any more: {e}"),
        }
    }
}
