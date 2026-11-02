//! What an agent sees beside the musician (AGENT-6): `m2 focus`, `m2 watch` and `m2 point`. Each
//! reaches the app that has the song open, as a write does (`app::looking`), and writes nothing:
//! what the musician looks at and what an agent points at are attention, not song.

use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};

use crate::app;

/// `m2 focus <song>`: what the musician looks at in the open app, as JSON.
pub fn focus(args: &[String]) -> Result<()> {
    let dir = song(args);
    let app = app::looking(dir.as_ref())?;
    let (status, v) = app.looked("GET", "/api/focus", None)?;
    if status != 200 {
        bail!("{}", app::words(&v));
    }
    println!("{}", serde_json::to_string_pretty(&v)?);
    Ok(())
}

/// `m2 point <song> <target> <words>`, or `m2 point <song> --clear`: a note at a track, a clip, a
/// lane, a knob or bars in the open app, by the agent's name (`--as`, `M2_AS`). The musician
/// dismisses it with OK; it never moves their view or changes their selection.
pub fn point(args: &[String]) -> Result<()> {
    // Its usage took them (AGENT-13): `m2 point <song> --clear` is the form with no target.
    let (dir, target, words) = match &crate::words::places(args)[..] {
        [dir] => (dir.clone(), None, String::new()),
        [dir, target, words @ ..] => (dir.clone(), Some(target.clone()), words.first().cloned().unwrap_or_default()),
        [] => unreachable!("its usage takes a song first"),
    };
    let clear = target.is_none();
    let app = app::looking(dir.as_ref())?;
    let body = json!({"session": app.session(), "target": target, "words": words, "clear": clear, "as": app::who().by});
    let (status, v) = app.looked("POST", "/api/point", Some(&body))?;
    if status != 200 {
        bail!("{}", app::words(&v));
    }
    match v["kind"].as_str() {
        None => println!("the pointer is gone from the open app"),
        Some(_) => println!("pointing at {} in the open app: {}", shown(&v), v["words"].as_str().unwrap_or_default()),
    }
    Ok(())
}

/// A pointer's target in words: "bars 9 to 16", "lane bass/cutoff".
fn shown(p: &Value) -> String {
    let s = |k: &str| p[k].as_str().unwrap_or_default();
    match s("kind") {
        "bars" => match (p["bars"][0].as_u64(), p["bars"][1].as_u64()) {
            (Some(a), Some(b)) if a == b => format!("bar {a}"),
            (Some(a), Some(b)) => format!("bars {a} to {b}"),
            _ => "bars".into(),
        },
        "track" => format!("track {}", s("track")),
        "lane" => format!("lane {}/{}", s("track"), s("lane")),
        "clip" => format!("clip {}/{}:{}", s("track"), s("lane"), s("clip")),
        _ => format!("knob {}", s("knob")),
    }
}

/// How often `m2 watch` reads the transport, as the page does: no event says it.
const TRANSPORT_EVERY: Duration = Duration::from_millis(250);

/// How many events `m2 watch` keeps while whoever reads it doesn't: beyond them it drops what comes,
/// and says how many it dropped (`lagged`, `by: m2`) once it is read again.
const WAITING: usize = 1024;

/// `m2 watch <song> [--meters]`: every event the page hears about that song, one JSON object a line,
/// `t` first, as it comes, and the transport (`transport`: playing, the beat, the loop) when it
/// plays or stops, the loop changes or the playhead jumps. First, the song it follows (`watch`, by
/// its session); every line says its session. Meters (30 a second) only with `--meters`. When the
/// app goes away it prints `closed` (`why: app`) and exits 0; when the app opens another song, or
/// closes this one, `closed` with `why: song`. When whoever reads it goes away, it ends quietly.
pub fn watch(args: &[String]) -> Result<()> {
    let dir = song(args);
    let meters = crate::words::on(args, "--meters");
    let app = app::looking(dir.as_ref())?;
    let session = app.session();
    // The meters only when asked: the app runs them only for a socket that listens (TAP-4).
    let listen = if meters { "&meters=1" } else { "" };
    let mut socket = m2_server::socket::Socket::open(app.url(), &format!("/api/ws?token={}{listen}", app.key()))
        .map_err(|e| anyhow!("couldn't follow the app: {e}"))?;
    // The socket is read on a thread of its own, always, so the app never waits for this one; what
    // waits for whoever reads this one is kept up to WAITING, and the rest counted.
    let (tx, rx) = mpsc::sync_channel::<Heard>(WAITING);
    let dropped = Arc::new(AtomicU64::new(0));
    let counted = dropped.clone();
    std::thread::spawn(move || {
        let ended = loop {
            let m = match socket.message() {
                Ok(Some(m)) => m,
                Ok(None) => break None,
                Err(e) => break Some(e.to_string()),
            };
            let Ok(v) = serde_json::from_str::<Value>(&m) else { continue };
            if v["t"] == "meters" && !meters {
                continue;
            }
            match tx.try_send(Heard::Event(v)) {
                Ok(()) => {}
                Err(mpsc::TrySendError::Full(_)) => {
                    counted.fetch_add(1, Ordering::Relaxed);
                }
                Err(mpsc::TrySendError::Disconnected(_)) => return,
            }
        };
        let _ = tx.send(Heard::End(ended));
    });
    let mut out = Out(std::io::stdout().lock());
    let ours = json!(session);
    let mine = |mut v: Value| {
        v["session"] = ours.clone();
        v
    };
    if !out.say(&mine(json!({"t": "watch", "song": dir})))? {
        return Ok(());
    }
    let mut transport = Transport::default();
    let mut polled = Instant::now() - TRANSPORT_EVERY;
    loop {
        if polled.elapsed() >= TRANSPORT_EVERY {
            polled = Instant::now();
            if let Ok((200, status)) = app.looked("GET", "/api/graph/status", None) {
                // Another song, or none: this one went.
                if status.get("session").is_some_and(|s| *s != ours) {
                    out.say(&mine(json!({"t": "closed", "why": "song"})))?;
                    return Ok(());
                }
                if let Some(said) = transport.heard(&status)
                    && !out.say(&mine(said))?
                {
                    return Ok(());
                }
            }
        }
        match rx.recv_timeout(TRANSPORT_EVERY.saturating_sub(polled.elapsed())) {
            Ok(Heard::Event(v)) => {
                let missed = dropped.swap(0, Ordering::Relaxed);
                if missed > 0 && !out.say(&mine(json!({"t": "lagged", "missed": missed, "by": "m2"})))? {
                    return Ok(());
                }
                // An event of another song, or of none: this one went.
                if v.get("session").is_some_and(|s| *s != ours) {
                    out.say(&mine(json!({"t": "closed", "why": "song"})))?;
                    return Ok(());
                }
                if !out.say(&v)? {
                    return Ok(());
                }
            }
            Ok(Heard::End(None)) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                out.say(&mine(json!({"t": "closed", "why": "app"})))?;
                return Ok(());
            }
            // The socket said what it can't read (a message past its limit): it can't go on.
            Ok(Heard::End(Some(e))) => {
                out.say(&mine(json!({"t": "closed", "why": "error", "error": e})))?;
                bail!("couldn't follow the app any more: {e}");
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
}

/// What `m2 watch`'s reader heard: an event, or the end of the socket (with why, when it was an
/// error).
enum Heard {
    Event(Value),
    End(Option<String>),
}

/// Where `m2 watch` writes its lines.
struct Out<W: Write>(W);

impl<W: Write> Out<W> {
    /// Write `v` as one line, `t` first. False when whoever read it went away (a broken pipe):
    /// then `m2 watch` ends quietly.
    fn say(&mut self, v: &Value) -> Result<bool> {
        match writeln!(self.0, "{}", line(v)).and_then(|()| self.0.flush()) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(false),
            Err(e) => Err(e.into()),
        }
    }
}

/// The transport as `m2 watch` last said it, and when it read it.
#[derive(Default)]
struct Transport {
    last: Option<(bool, f64, Value, Instant)>,
}

/// How far the playhead may be from where it would be before a move counts as a jump, in beats.
const JUMP: f64 = 0.5;

impl Transport {
    /// The `transport` event to print for `status`, if anything changed: the first time; playing
    /// or stopping; the loop; or the playhead away from where it would be (a seek, Return), the
    /// loop's turn aside.
    fn heard(&mut self, status: &Value) -> Option<Value> {
        let (playing, beat, looping) = (status["playing"].as_bool()?, status["beat"].as_f64()?, status["loop"].clone());
        let now = Instant::now();
        let changed = match &self.last {
            None => true,
            Some((was_playing, was_beat, was_loop, at)) => {
                let bpm = status["bpm"].as_f64().unwrap_or(120.0);
                let mut expected = if *was_playing { was_beat + at.elapsed().as_secs_f64() * bpm / 60.0 } else { *was_beat };
                // Around the loop, as it plays.
                if let (Some(start), Some(end), Some(true)) = (looping["start"].as_f64(), looping["end"].as_f64(), looping["on"].as_bool())
                    && *was_playing
                    && end > start
                    && expected >= end
                {
                    expected = start + (expected - start) % (end - start);
                }
                *was_playing != playing || *was_loop != looping || (beat - expected).abs() > JUMP
            }
        };
        self.last = Some((playing, beat, looping.clone(), now));
        changed.then(|| json!({"t": "transport", "playing": playing, "beat": beat, "loop": looping}))
    }
}

/// `v` on one line, its `t` first: what an agent reads to tell events apart.
fn line(v: &Value) -> String {
    let mut fields = v.as_object().cloned().unwrap_or_default();
    let t = fields.remove("t").unwrap_or(Value::Null);
    match Value::Object(fields).to_string().strip_prefix('{').unwrap_or("}") {
        "}" => format!("{{\"t\":{t}}}"),
        rest => format!("{{\"t\":{t},{rest}"),
    }
}

/// The song folder these commands name first: their usage takes it (AGENT-13).
fn song(args: &[String]) -> String {
    crate::words::places(args).swap_remove(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_says_its_t_first() {
        assert_eq!(line(&json!({"t": "step", "a": 1, "z": [2]})), r#"{"t":"step","a":1,"z":[2]}"#);
        assert_eq!(line(&json!({"t": "closed"})), r#"{"t":"closed"}"#);
    }

    #[test]
    fn the_transport_is_said_when_it_changes() {
        let mut t = Transport::default();
        let at = |playing: bool, beat: f64| json!({"playing": playing, "beat": beat, "bpm": 120.0, "loop": null});
        assert!(t.heard(&at(false, 0.0)).is_some(), "the first time");
        assert!(t.heard(&at(false, 0.0)).is_none(), "nothing moved");
        assert!(t.heard(&at(false, 8.0)).is_some(), "a seek while stopped");
        assert!(t.heard(&at(true, 8.0)).is_some(), "play");
        assert!(t.heard(&at(true, 8.1)).is_none(), "playing on, as it would");
        assert!(t.heard(&at(true, 32.0)).is_some(), "a jump while playing");
        let looped = json!({"playing": true, "beat": 32.0, "bpm": 120.0, "loop": {"start": 0.0, "end": 4.0, "on": true}});
        assert!(t.heard(&looped).is_some(), "the loop changed");
    }
}
