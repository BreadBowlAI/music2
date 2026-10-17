//! The 8b back ends' commands, each a Tx builder of `crates/editor` taking the project's lock
//! (WRITE-18): mute in song, and clip gestures.

use std::path::Path;
use std::time::Instant;

use anyhow::{Result, anyhow, bail};

/// Whether `cmd` is one of these.
pub fn handles(cmd: &str) -> bool {
    matches!(cmd, "mute-in-song" | "clips")
}

/// Run `cmd`, with the command line's arguments (`args[0]` is `cmd`).
pub fn run(cmd: &str, args: &[String]) -> Result<()> {
    match cmd {
        "mute-in-song" => mute_in_song(args),
        "clips" => clips(args),
        _ => unreachable!("handled: {cmd}"),
    }
}

/// `m2 mute-in-song <project> <track>… on|off`: one Tx, one undo step (MON-5).
fn mute_in_song(args: &[String]) -> Result<()> {
    let usage = "say which tracks, then on or off: m2 mute-in-song <project> <track>… on|off";
    let [_, dir, rest @ .., last] = args else { bail!("{usage}") };
    let muted = match last.as_str() {
        "on" => true,
        "off" => false,
        _ => bail!("{usage}"),
    };
    if rest.is_empty() {
        bail!("{usage}");
    }
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

const CLIPS: &str = "m2 clips <project> create|move|resize|duplicate|delete|json …; see m2 --help";

/// `m2 clips <project> <gesture> …`: one clip gesture, one Tx, one undo step (8b.5). Clips are
/// `track/lane:id`, or `track/lane@place` for the first clip that starts there. Places are
/// bar.beat.sixteenth (`9.3.1`), or beats with `--beats`; distances and lengths are beats.
fn clips(args: &[String]) -> Result<()> {
    use m2_editor::clips::{Gesture, edit, position};
    let [_, dir, op, rest @ ..] = args else { bail!("{CLIPS}") };
    let mut s = super::session(Path::new(dir))?;
    let arr = s.accepted().project.arrangement.clone();
    let mut f = Flags::read(rest)?;
    let beats = f.on("--beats");
    let place = |text: &str| match beats {
        true => text.parse::<f64>().map_err(|_| anyhow!("`{text}` isn't a number of beats")),
        false => position(&arr, text).map_err(|e| anyhow!("{e}")),
    };
    let refs = |f: &Flags| f.words.iter().map(|w| clip_ref(w, &place)).collect::<Result<Vec<_>>>();
    let g = match op.as_str() {
        "create" => {
            let [lane] = &f.words[..] else { bail!("say which lane: m2 clips <project> create <track>/<lane> --at <place>") };
            Gesture::Create { lane: lane_addr(lane)?, start: place(&f.need("--at")?)?, length: f.number("--length")? }
        }
        "move" => Gesture::Move { clips: refs(&f)?, by: f.number("--by")?.unwrap_or(0.0), to: f.lane("--to")?, copy: f.on("--copy") },
        "resize" => {
            let [clip] = &refs(&f)?[..] else { bail!("say which clip: m2 clips <project> resize <clip> --start <place> --length <beats>") };
            let length = f.number("--length")?.ok_or_else(|| anyhow!("say how long, in beats: --length 4"))?;
            Gesture::Resize { clip: clip.clone(), start: place(&f.need("--start")?)?, length }
        }
        "duplicate" => Gesture::Duplicate { clips: refs(&f)?, range: None, grid: f.number("--grid")? },
        "delete" => Gesture::Delete { clips: refs(&f)? },
        "json" => {
            let [file] = &f.words[..] else { bail!("give the gesture as JSON: m2 clips <project> json <gesture.json | ->") };
            let text = match file.as_str() {
                "-" => std::io::read_to_string(std::io::stdin())?,
                path => std::fs::read_to_string(path)?,
            };
            serde_json::from_str(&text).map_err(|e| anyhow!("the gesture doesn't read: {e}"))?
        }
        _ => bail!("{CLIPS}"),
    };
    f.done()?;
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

/// A command's words and `--flag value` pairs; a flag left unread is refused.
struct Flags {
    words: Vec<String>,
    flags: Vec<(String, Option<String>)>,
}

impl Flags {
    const SWITCHES: [&str; 3] = ["--copy", "--beats", "--ends"];

    fn read(args: &[String]) -> Result<Flags> {
        let (mut words, mut flags) = (vec![], vec![]);
        let mut it = args.iter();
        while let Some(a) = it.next() {
            match a.strip_prefix("--") {
                Some(_) if Self::SWITCHES.contains(&a.as_str()) => flags.push((a.clone(), None)),
                Some(_) => flags.push((a.clone(), Some(it.next().ok_or_else(|| anyhow!("{a} needs a value"))?.clone()))),
                None => words.push(a.clone()),
            }
        }
        Ok(Flags { words, flags })
    }

    fn take(&mut self, name: &str) -> Option<Option<String>> {
        let i = self.flags.iter().position(|(f, _)| f == name)?;
        Some(self.flags.remove(i).1)
    }

    fn on(&mut self, name: &str) -> bool {
        self.take(name).is_some()
    }

    fn need(&mut self, name: &str) -> Result<String> {
        self.take(name).flatten().ok_or_else(|| anyhow!("say {name}"))
    }

    fn number(&mut self, name: &str) -> Result<Option<f64>> {
        self.take(name).flatten().map(|v| v.parse::<f64>().map_err(|_| anyhow!("{name} takes a number of beats, not `{v}`"))).transpose()
    }

    fn lane(&mut self, name: &str) -> Result<Option<m2_song::LaneAddr>> {
        self.take(name).flatten().map(|v| lane_addr(&v)).transpose()
    }

    fn done(&self) -> Result<()> {
        match self.flags.first() {
            Some((f, _)) => bail!("{f} isn't something this gesture takes"),
            None => Ok(()),
        }
    }
}
