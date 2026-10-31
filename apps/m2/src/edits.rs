//! The 8b back ends' commands, each a Tx builder of `crates/editor` taking the project's lock
//! (WRITE-18): mute in song, clip gestures, section gestures, and Remove automation.

use std::path::Path;
use std::time::Instant;

use anyhow::{Result, anyhow, bail};

use crate::words::{on, places, value};

/// Whether `cmd` is one of these.
pub fn handles(cmd: &str) -> bool {
    matches!(cmd, "mute-in-song" | "clips" | "sections" | "unautomate")
}

/// Run `cmd`, with the words `words::read` handed on (`args[0]` is `cmd`): its usage took them
/// (AGENT-13).
pub fn run(cmd: &str, args: &[String]) -> Result<()> {
    match cmd {
        "mute-in-song" => mute_in_song(args),
        "clips" => clips(args),
        "sections" => sections(args),
        "unautomate" => unautomate(args),
        _ => unreachable!("handled: {cmd}"),
    }
}

/// `m2 mute-in-song <project> <track>… on|off`: one Tx, one undo step (MON-5).
fn mute_in_song(args: &[String]) -> Result<()> {
    let [_, dir, rest @ .., last] = args else { unreachable!("its usage takes three words at least") };
    let muted = last == "on";
    let mut s = super::session(Path::new(dir))?;
    let t = Instant::now();
    let e = m2_editor::mute::mute_in_song(&mut s, rest, muted).map_err(|e| anyhow!("{e}"))?;
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    match e.before == e.after {
        true => println!("{}: nothing to write", e.after),
        false => println!("{}: {} → {} (revision {}, verified in {ms:.0} ms)", e.file, e.before, e.after, e.revision),
    }
    Ok(())
}

/// `m2 unautomate <project> <track>/<lane> [--at <place>] [--beats]`: the lane's value at the
/// place (else its fallback) written back where it is read, and its file removed (HOME-2).
fn unautomate(args: &[String]) -> Result<()> {
    let [dir, lane] = &places(args)[..] else { unreachable!("its usage takes two words") };
    let lane = lane_addr(lane)?;
    let mut s = super::session(Path::new(dir))?;
    let arr = s.accepted().project.arrangement.clone();
    let at = match value(args, "--at") {
        Some(t) if on(args, "--beats") => Some(t.parse::<f64>().map_err(|_| anyhow!("`{t}` isn't a number of beats"))?),
        Some(t) => Some(m2_editor::clips::position(&arr, &t).map_err(|e| anyhow!("{e}"))?),
        None => None,
    };
    let t = Instant::now();
    let e = m2_editor::automate::remove(&mut s, lane.module.as_str(), lane.lane.as_str(), at).map_err(|e| anyhow!("{e}"))?;
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    println!("{}: {} → {} (revision {}, verified in {ms:.0} ms)", e.file, e.before, e.after, e.revision);
    Ok(())
}

/// `m2 clips <project> <gesture> …`: one clip gesture, one Tx, one undo step (8b.5). Clips are
/// `track/lane:id`, or `track/lane@place` for the first clip that starts there. Places are
/// bar.beat.sixteenth (`9.3.1`), or beats with `--beats`; distances and lengths are beats.
fn clips(args: &[String]) -> Result<()> {
    use m2_editor::clips::{Gesture, edit, position};
    let [dir, op, words @ ..] = &places(args)[..] else { unreachable!("its usage takes two words at least") };
    let mut s = super::session(Path::new(dir))?;
    let arr = s.accepted().project.arrangement.clone();
    // Every gesture but `json` names its clips or places, in beats with --beats.
    let beats = op != "json" && on(args, "--beats");
    let place = |text: &str| match beats {
        true => text.parse::<f64>().map_err(|_| anyhow!("`{text}` isn't a number of beats")),
        false => position(&arr, text).map_err(|e| anyhow!("{e}")),
    };
    let need = |name: &str| value(args, name).ok_or_else(|| anyhow!("say {name}"));
    let refs = || words.iter().map(|w| clip_ref(w, &place)).collect::<Result<Vec<_>>>();
    let g = match op.as_str() {
        "create" => Gesture::Create { lane: lane_addr(&words[0])?, start: place(&need("--at")?)?, length: number(args, "--length")? },
        "move" => Gesture::Move {
            clips: refs()?,
            by: number(args, "--by")?.unwrap_or(0.0),
            to: value(args, "--to").map(|v| lane_addr(&v)).transpose()?,
            copy: on(args, "--copy"),
        },
        "resize" => {
            let length = number(args, "--length")?.ok_or_else(|| anyhow!("say how long, in beats: --length 4"))?;
            Gesture::Resize { clip: clip_ref(&words[0], &place)?, start: place(&need("--start")?)?, length }
        }
        "duplicate" => Gesture::Duplicate { clips: refs()?, range: None, grid: number(args, "--grid")?, times: None },
        "split" => Gesture::Split { clips: refs()?, at: place(&need("--at")?)? },
        "join" => Gesture::Join { clips: refs()? },
        "quantize" => {
            let grid = number(args, "--grid")?.ok_or_else(|| anyhow!("say the grid, in beats: --grid 0.25 for sixteenths"))?;
            Gesture::Quantize { clips: refs()?, notes: None, grid, ends: on(args, "--ends") }
        }
        "delete" => Gesture::Delete { clips: refs()? },
        "json" => {
            let text = match words[0].as_str() {
                "-" => std::io::read_to_string(std::io::stdin())?,
                path => std::fs::read_to_string(path)?,
            };
            serde_json::from_str(&text).map_err(|e| anyhow!("the gesture doesn't read: {e}"))?
        }
        _ => unreachable!("its usage takes only these"),
    };
    let t = Instant::now();
    let done = edit(&mut s, &g).map_err(|e| anyhow!("{e}"))?;
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    let e = &done.edited;
    match e.before == e.after {
        true => println!("{}: nothing to write", e.after),
        false => println!("{}: {} → {} (revision {}, verified in {ms:.0} ms)", e.file, e.before, e.after, e.revision),
    }
    for (lane, id) in &done.made {
        println!("  made {lane}:{id}");
    }
    Ok(())
}

/// `m2 sections <project> <gesture> …`: one section gesture on every track, one Tx, one undo step
/// (ARR-9). Bars are counted from 1 (`9-16`, `9`), or named by a marker: from it to the next one,
/// or to the end of the last bar that plays.
fn sections(args: &[String]) -> Result<()> {
    use m2_editor::sections::{Gesture, edit};
    let [dir, op, words @ ..] = &places(args)[..] else { unreachable!("its usage takes two words at least") };
    let mut s = super::session(Path::new(dir))?;
    let arr = s.accepted().project.arrangement.clone();
    let span = || bars(&s, &words[0]);
    let to = |name: &str| value(args, name).map(|t| m2_editor::clips::position(&arr, &t).map_err(|e| anyhow!("{name}: {e}"))).transpose();
    let g = match op.as_str() {
        "duplicate" => {
            let (start, end) = span()?;
            Gesture::Duplicate { start, end, to: to("--to")? }
        }
        "move" => {
            let (start, end) = span()?;
            Gesture::Move { start, end, to: to("--to")?.ok_or_else(|| anyhow!("say where the bars go: --to 17"))? }
        }
        "delete" => {
            let (start, end) = span()?;
            Gesture::Delete { start, end }
        }
        "insert-bars" => {
            let (start, end) = span()?;
            let count = arr.bar_at(end).0 - arr.bar_at(start).0;
            Gesture::InsertBars { at: start, bars: u32::try_from(count).unwrap_or(u32::MAX) }
        }
        "delete-bars" => {
            let (start, end) = span()?;
            Gesture::DeleteBars { start, end }
        }
        "json" => {
            let text = match words[0].as_str() {
                "-" => std::io::read_to_string(std::io::stdin())?,
                path => std::fs::read_to_string(path)?,
            };
            serde_json::from_str(&text).map_err(|e| anyhow!("the gesture doesn't read: {e}"))?
        }
        _ => unreachable!("its usage takes only these"),
    };
    let t = Instant::now();
    let done = edit(&mut s, &g).map_err(|e| anyhow!("{e}"))?;
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    let e = &done.edited;
    match e.before == e.after {
        true => println!("{}: nothing to write", e.after),
        false => println!("{}: {} → {} (revision {}, verified in {ms:.0} ms)", e.file, e.before, e.after, e.revision),
    }
    for (lane, id) in &done.made {
        println!("  made {lane}:{id}");
    }
    Ok(())
}

/// The bars `text` names, as beats `[start, end)`: `9-16` (bars 9 to 16), `9` (bar 9), or a
/// marker's name: from it to the next marker, or to the end of the last bar that plays.
fn bars(s: &m2_session::Session, text: &str) -> Result<(f64, f64)> {
    let arr = &s.accepted().project.arrangement;
    let bar = |w: &str| w.trim().parse::<u64>().ok().filter(|n| *n >= 1);
    let (first, last) = match text.split_once('-') {
        Some((a, b)) => (bar(a), bar(b)),
        None => (bar(text), bar(text)),
    };
    if let (Some(first), Some(last)) = (first, last) {
        if last < first {
            bail!("`{text}`: the last bar comes before the first; write them in order, such as 9-16");
        }
        return Ok((m2_editor::clips::bar_start(arr, first - 1), m2_editor::clips::bar_start(arr, last)));
    }
    let marks = m2_editor::markers::on_disk(&s.accepted().project.dir);
    let Some(i) = marks.iter().position(|m| m.name == text).or_else(|| marks.iter().position(|m| m.name.eq_ignore_ascii_case(text))) else {
        let names: Vec<&str> = marks.iter().map(|m| m.name.as_str()).collect();
        bail!(
            "`{text}` isn't bars or a marker: write bars counted from 1, such as 9-16, or a marker's name{}",
            if names.is_empty() { " (the song has none)".to_string() } else { format!(" ({})", names.join(", ")) }
        );
    };
    let start = marks[i].beat;
    let end = match marks.get(i + 1) {
        Some(next) => next.beat,
        None => {
            // The end of the last bar that plays.
            let last = m2_editor::timeline::timeline(s).end;
            let (n, into) = arr.bar_at(last);
            m2_editor::clips::bar_start(arr, if into > 0.0 { n + 1 } else { n })
        }
    };
    if end <= start {
        bail!("nothing plays after the {} marker, so it marks no bars", marks[i].name);
    }
    Ok((start, end))
}

/// `track/lane`.
fn lane_addr(text: &str) -> Result<m2_song::LaneAddr> {
    let (m, l) = text.split_once('/').ok_or_else(|| anyhow!("`{text}` isn't a lane: write track/lane, by keys, such as bass/notes"))?;
    m2_song::LaneAddr::new(m, l).map_err(|e| anyhow!("`{text}`: {e}"))
}

/// `track/lane:id`, or `track/lane@place` for the first clip that starts there.
fn clip_ref(text: &str, place: &dyn Fn(&str) -> Result<f64>) -> Result<m2_editor::clips::ClipRef> {
    let (lane, id, at) = match (text.split_once(':'), text.split_once('@')) {
        (Some((lane, id)), _) => (lane, Some(id.to_string()), None),
        (None, Some((lane, at))) => (lane, None, Some((place(at)?, 0))),
        _ => bail!("`{text}` isn't a clip: write track/lane:id, or track/lane@place for the clip that starts there"),
    };
    Ok(m2_editor::clips::ClipRef { lane: lane_addr(lane)?, id, at, sha: None })
}

/// Flag `name`'s value, a number of beats.
fn number(args: &[String], name: &str) -> Result<Option<f64>> {
    value(args, name).map(|v| v.parse::<f64>().map_err(|_| anyhow!("{name} takes a number of beats, not `{v}`"))).transpose()
}
