//! What an agent sees beside the musician (AGENT-6): `m2 focus`, `m2 watch` and `m2 point`. Each
//! reaches the app that has the song open, as a write does (`app::looking`), and writes nothing:
//! what the musician looks at and what an agent points at are attention, not song.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};

use crate::app;

/// `m2 focus <song>`: what the musician looks at in the open app, as JSON.
pub fn focus(args: &[String]) -> Result<()> {
    let dir = song(args, "m2 focus <song>")?;
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
    const USE: &str = "m2 point <song> <target> \"<words>\" | --clear (a target is track:bass, clip:bass/notes:<id>, lane:bass/cutoff, knob:<id> or bars:9-16)";
    let dir = song(args, USE)?;
    let clear = args.iter().skip(2).any(|a| a == "--clear");
    let (target, words) = match (clear, args.get(2), args.get(3)) {
        (true, ..) => (None, String::new()),
        (false, Some(target), words) => (Some(target.clone()), words.cloned().unwrap_or_default()),
        (false, None, _) => bail!("say where to point: {USE}"),
    };
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

/// `m2 watch <song> [--meters]`: every event the page hears, one JSON object a line, `t` first, as
/// it comes, and the transport (`transport`: playing, the beat, the loop) when it plays or stops,
/// the loop changes or the playhead jumps. Meters (30 a second) only with `--meters`. When the app
/// goes away it prints `closed` and exits 0.
pub fn watch(args: &[String]) -> Result<()> {
    let dir = song(args, "m2 watch <song> [--meters]")?;
    let meters = args.iter().any(|a| a == "--meters");
    let app = app::looking(dir.as_ref())?;
    let mut socket = m2_server::socket::Socket::open(app.url(), &format!("/api/ws?token={}", app.key()))
        .map_err(|e| anyhow!("couldn't follow the app: {e}"))?;
    let (tx, rx) = mpsc::channel::<Option<String>>();
    std::thread::spawn(move || {
        loop {
            match socket.message() {
                Ok(Some(m)) => {
                    if tx.send(Some(m)).is_err() {
                        return;
                    }
                }
                _ => {
                    let _ = tx.send(None);
                    return;
                }
            }
        }
    });
    let mut transport = Transport::default();
    let mut polled = Instant::now() - TRANSPORT_EVERY;
    loop {
        if polled.elapsed() >= TRANSPORT_EVERY {
            polled = Instant::now();
            if let Ok((200, status)) = app.looked("GET", "/api/graph/status", None)
                && let Some(said) = transport.heard(&status)
            {
                println!("{}", line(&said));
            }
        }
        match rx.recv_timeout(TRANSPORT_EVERY.saturating_sub(polled.elapsed())) {
            Ok(Some(m)) => {
                let Ok(v) = serde_json::from_str::<Value>(&m) else { continue };
                if v["t"] != "meters" || meters {
                    println!("{}", line(&v));
                }
            }
            Ok(None) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                println!("{}", line(&json!({"t": "closed"})));
                return Ok(());
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
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

/// The song folder these commands name first.
fn song(args: &[String], usage: &str) -> Result<String> {
    match args.get(1) {
        Some(dir) if !dir.starts_with("--") => Ok(dir.clone()),
        _ => bail!("name the song first: {usage}"),
    }
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
