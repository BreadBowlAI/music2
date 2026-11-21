//! `m2 versions` (VER-5): a notes clip's versions from the terminal. A clip holds several
//! versions of its notes; one plays, the others wait in the clip (VER-1). It lists them, and sends
//! each act of the `versions` clip gesture (VER-3) as one edit that undo takes back.

use std::path::Path;
use std::time::Instant;

use anyhow::{Result, anyhow, bail};
use m2_editor::clips::{Act, ClipRef, Gesture, NewVersion};
use m2_session::Session;
use m2_song::{LaneAddr, LaneData, Note, NoteClip};
use serde_json::{Value, json};

use crate::words::{on, places};

/// `m2 versions <project> …`, with the words `words::read` handed on: its usage took them
/// (AGENT-13).
pub fn run(args: &[String]) -> Result<()> {
    let words = places(args);
    let (dir, act, clip, rest) = match &words[..] {
        [dir, _list] => return all(Path::new(dir), on(args, "--json")),
        [dir, act, clip, rest @ ..] => (Path::new(dir), act.as_str(), clip, rest),
        _ => unreachable!("its usage takes a project and an act at least"),
    };
    let name = || rest[0].clone();
    let act = match act {
        "list" => return list(dir, clip, on(args, "--json")),
        "add" => {
            let text = match rest[1].as_str() {
                "-" => std::io::read_to_string(std::io::stdin())?,
                path => std::fs::read_to_string(path).map_err(|e| anyhow!("{path} can't be read: {e}"))?,
            };
            let notes: Vec<Note> = serde_json::from_str(&text).map_err(|e| {
                anyhow!("the notes don't read ({e}): give a list, such as [{{\"pitch\": \"A1\", \"start\": 0, \"length\": 0.5}}]")
            })?;
            Act::Add { versions: vec![NewVersion { name: name(), notes }], play: false }
        }
        "play" => Act::Play { name: name() },
        "star" => Act::Star { name: name(), star: true },
        "unstar" => Act::Star { name: name(), star: false },
        "remove" => Act::Remove { name: name() },
        "rename" => Act::Rename { name: name(), to: rest[1].clone() },
        "keep" => Act::Keep,
        _ => unreachable!("its usage takes only these"),
    };
    let mut s = super::session(dir)?;
    // A version is added by the agent's name (`--as`), as its undo step is (AGENT-10).
    let by = match m2_server::caller::Caller::agent(crate::app::who().by.as_deref(), None) {
        m2_server::caller::Caller::Agent { name, .. } => Some(name),
        m2_server::caller::Caller::Page => None,
    };
    let g = Gesture::Versions { clip: clip_ref(&s, clip)?, act, by };
    let t = Instant::now();
    let done = m2_editor::clips::edit(&mut s, &g).map_err(|e| anyhow!("{e}"))?;
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    let e = &done.edited;
    match e.before == e.after {
        true => println!("{}: nothing to write", e.after),
        false => println!("{}: {} → {} (revision {}, verified in {ms:.0} ms)", e.file, e.before, e.after, e.revision),
    }
    Ok(())
}

/// `track/lane:id`, or `track/lane@place` (bar.beat.sixteenth) for the first clip that starts
/// there, as `m2 clips` names a clip.
fn clip_ref(s: &Session, text: &str) -> Result<ClipRef> {
    let arr = s.accepted().project.arrangement.clone();
    crate::edits::clip_ref(text, &|at| m2_editor::clips::position(&arr, at).map_err(|e| anyhow!("{e}")))
}

/// The notes clip `text` names in the song as it is, and its lane.
fn found(s: &Session, text: &str) -> Result<(LaneAddr, NoteClip)> {
    let r = clip_ref(s, text)?;
    let (module, lane) = (r.lane.module.as_str(), r.lane.lane.as_str());
    let clips = match s.accepted().compiled.data.lane(module, lane).map(|d| &**d) {
        Some(LaneData::Notes(clips)) => clips,
        Some(_) => bail!("Only a notes clip has versions."),
        None => bail!("the song has no lane {module}/{lane} with clips: `m2 timeline` lists them"),
    };
    let clip = match (&r.id, r.at) {
        (Some(id), _) => clips.iter().find(|c| c.id.as_deref() == Some(id)),
        (None, Some((at, _))) => clips.iter().find(|c| (c.start - at).abs() <= 1e-6),
        (None, None) => None,
    };
    let ids: Vec<String> = clips.iter().filter_map(|c| c.id.clone()).collect();
    let clip = clip.ok_or_else(|| anyhow!("{module}/{lane} has no clip `{text}`: its clips are {}", ids.join(", ")))?;
    Ok((r.lane, clip.clone()))
}

/// A note as the lane's file writes it: its pitch by name where it has one.
fn note(n: &Note) -> Value {
    let pitch = m2_song::lane::pitch_name(n.pitch).map_or(json!(n.pitch), Value::from);
    json!({"pitch": pitch, "start": n.start, "length": n.length, "velocity": n.velocity})
}

/// `module/lane:id`.
fn named(lane: &LaneAddr, c: &NoteClip) -> String {
    format!("{}/{}:{}", lane.module.as_str(), lane.lane.as_str(), c.id.as_deref().unwrap_or("?"))
}

/// `3 versions, `busy` plays`.
fn count(c: &NoteClip) -> String {
    let plays = c.versions.iter().find(|v| v.notes.is_none()).map_or("", |v| v.name.as_str());
    format!("{} versions, `{plays}` plays", c.versions.len())
}

/// A clip's versions as JSON, each with its notes when `notes` is given: the clip's own for the
/// one that plays.
fn versions(c: &NoteClip, notes: bool) -> Vec<Value> {
    c.versions
        .iter()
        .map(|v| {
            let mut one = json!({"name": v.name, "plays": v.notes.is_none(), "star": v.star, "by": v.by});
            if notes {
                one["notes"] = v.notes.as_ref().unwrap_or(&c.notes).iter().map(note).collect();
            }
            one
        })
        .collect()
}

/// `m2 versions <project> list`: the clips that have versions.
fn all(dir: &Path, as_json: bool) -> Result<()> {
    let s = super::reader(dir)?;
    let rev = s.accepted();
    let mut with: Vec<(&LaneAddr, &NoteClip)> = vec![];
    for (lane, data) in rev.compiled.data.lanes() {
        if let LaneData::Notes(clips) = &**data {
            with.extend(clips.iter().filter(|c| !c.versions.is_empty()).map(|c| (lane, c)));
        }
    }
    if as_json {
        let clips: Vec<Value> = with
            .iter()
            .map(|(lane, c)| {
                let plays = c.versions.iter().find(|v| v.notes.is_none()).map(|v| v.name.clone());
                json!({"clip": named(lane, c), "module": lane.module.as_str(), "lane": lane.lane.as_str(), "id": c.id, "start": c.start,
                    "length": c.length, "plays": plays, "versions": versions(c, false)})
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json!({"clips": clips}))?);
        return Ok(());
    }
    if with.is_empty() {
        println!("No clip has versions. Add one: m2 versions <project> add <track>/<lane>:<id> <name> <notes.json | ->");
    }
    for (lane, c) in with {
        let starred = c.versions.iter().filter(|v| v.star).count();
        let stars = if starred > 0 { format!(", {starred} starred") } else { String::new() };
        println!("{}  {}  {}{stars}", named(lane, c), m2_editor::notes::place(&rev.project.arrangement, c.start), count(c));
    }
    Ok(())
}

/// `m2 versions <project> list <clip>`: the clip's versions, the one that plays marked; with
/// `--json`, each with its notes.
fn list(dir: &Path, clip: &str, as_json: bool) -> Result<()> {
    let s = super::reader(dir)?;
    let (lane, c) = found(&s, clip)?;
    if as_json {
        let v = json!({
            "clip": {"module": lane.module.as_str(), "lane": lane.lane.as_str(), "id": c.id, "start": c.start, "length": c.length},
            "notes": c.notes.iter().map(note).collect::<Vec<_>>(),
            "versions": versions(&c, true),
        });
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }
    let notes = |n: usize| format!("{n} note{}", if n == 1 { "" } else { "s" });
    if c.versions.is_empty() {
        println!(
            "{}: no versions ({} play). Add one: m2 versions <project> add {} <name> <notes.json | ->",
            named(&lane, &c),
            notes(c.notes.len()),
            named(&lane, &c)
        );
        return Ok(());
    }
    println!("{}: {}", named(&lane, &c), count(&c));
    let wide = c.versions.iter().map(|v| v.name.chars().count()).max().unwrap_or(0);
    for v in &c.versions {
        let (does, n) = match &v.notes {
            None => ("plays", c.notes.len()),
            Some(n) => ("waits", n.len()),
        };
        let pad = " ".repeat(wide - v.name.chars().count());
        let by = v.by.as_ref().map(|b| format!("  by {b}")).unwrap_or_default();
        println!("  {} {}{pad}  {does}  {}{by}", if v.star { '*' } else { ' ' }, v.name, notes(n));
    }
    Ok(())
}
