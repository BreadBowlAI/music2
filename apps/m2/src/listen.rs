//! `m2 listen` (AGENT-7; track 5 of the agents design): what a song's bars measure, for an agent
//! that can't hear them. An offline range render in this process, never the app's engine: at
//! background priority with its plugin host, off the cores playback needs, one at a time on the
//! machine. It reads the song's files (or a Tx it checks and never writes, `--with`), and writes
//! nothing into the song.
//!
//! The range: `--bars 9-16` (counted from 1), else the loop when one is on (the open app's, else
//! `.music2/view.json`'s), else the song to the end of its last clip; 2 bars of preroll, so it is
//! approximate unless the preroll reaches the start (RENDER-5). With `--with`, each revision turns
//! the same bars into its own seconds. `--as-heard` hears the listening mute and solo (the open
//! app's, else `.music2/overlays.json`), given to each revision by its tracks' keys, and says so
//! (MON-4). The open app is asked only for what the range or `--as-heard` need, before the lock,
//! and never waited on for long: a song it holds that it doesn't answer for is read from its files,
//! and the listen says so.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use m2_editor::clips::bar_start;
use m2_editor::listen::{self, Ask, Heard, Overlaid};
use m2_editor::render::Threads;
use m2_server::graph::overlay::{self, Overlays};
use m2_session::{Revision, Session};
use serde_json::{Value, json};

use crate::app;

/// How `m2 listen` is called: its usage in the one table (AGENT-12).
fn usage() -> &'static str {
    crate::commands::find("listen").map_or("", |c| c.usage)
}
/// Bars rendered before the range and dropped (RENDER-4).
const PREROLL_BARS: u64 = 2;
/// How long `m2 listen` waits for the open app's answer before it reads the song's files instead.
const PATIENCE: Duration = Duration::from_secs(5);

/// What `m2 listen` was asked.
struct Args {
    dir: PathBuf,
    bars: Option<String>,
    tracks: Option<Vec<String>>,
    as_heard: bool,
    with: Option<String>,
    png: Option<PathBuf>,
    json: bool,
}

/// The arguments after `listen`: the song, then the flags, each once. A flag it doesn't know, a
/// second song, and a flag where a value should be are refused.
fn args(all: &[String]) -> Result<Args> {
    let mut a = Args { dir: PathBuf::new(), bars: None, tracks: None, as_heard: false, with: None, png: None, json: false };
    let mut dir: Option<PathBuf> = None;
    let mut it = all.iter().skip(1);
    let mut seen: Vec<&str> = vec![];
    while let Some(arg) = it.next() {
        if !arg.starts_with("--") {
            if dir.is_some() {
                bail!("m2 listen takes one song, not `{arg}` too: {}", usage());
            }
            dir = Some(PathBuf::from(arg));
            continue;
        }
        if seen.contains(&arg.as_str()) {
            bail!("{arg} is named twice: {}", usage());
        }
        seen.push(arg);
        let mut value = || -> Result<String> {
            match it.next() {
                Some(v) if !v.starts_with("--") => Ok(v.clone()),
                Some(v) => bail!("{arg} needs a value, not the flag {v}: {}", usage()),
                None => bail!("{arg} needs a value: {}", usage()),
            }
        };
        match arg.as_str() {
            "--bars" => a.bars = Some(value()?),
            "--tracks" => {
                let text = value()?;
                let mut keys: Vec<String> = vec![];
                for k in text.split(',').map(str::trim).filter(|k| !k.is_empty()) {
                    if !keys.iter().any(|x| x == k) {
                        keys.push(k.to_string());
                    }
                }
                a.tracks = Some(keys);
            }
            "--with" => a.with = Some(value()?),
            "--png" => a.png = Some(PathBuf::from(value()?)),
            "--as-heard" => a.as_heard = true,
            "--json" => a.json = true,
            other => bail!("m2 listen doesn't know {other}: {}", usage()),
        }
    }
    a.dir = dir.context(usage())?;
    Ok(a)
}

/// `m2 listen <project> …`.
pub fn listen(all: &[String]) -> Result<()> {
    let a = args(all)?;
    // Its own process at background priority, every thread of it, and its plugin host too: first,
    // so every thread it starts takes it (AGENT-7).
    if let Err(e) = m2_os::process::background() {
        eprintln!("m2: couldn't lower its priority ({e})");
    }
    m2_plugins::background_host();
    // The picture's path, before anything renders: never in the song, and one it can write.
    let mut png = a.png.as_deref().map(|p| Claimed::new(p, &a.dir)).transpose()?;
    // The open app, only for what the range or --as-heard need, before the lock: a busy app holds
    // up no other listen.
    let app = Asked::of(&a.dir, a.bars.is_none(), a.as_heard);
    let _one = one_at_a_time()?;
    let mut s = crate::reader(&a.dir)?;
    let rev = Arc::new(s.accepted().duplicate());
    let range = range(&rev, a.bars.as_deref(), app.state(), &a.dir)?;
    let heard_as = a.as_heard.then(|| listening(app.state(), &a.dir));
    let overlays: Option<Overlaid> = heard_as.as_ref().map(|h| {
        let set = h.set.clone();
        Arc::new(move |rev: &Revision| values(rev, &set)) as Overlaid
    });
    let ask = |rev: &Revision| {
        let span = span(rev, &range);
        let ask = Ask {
            from: span.from,
            to: span.to,
            preroll: span.from - span.start,
            tracks: a.tracks.clone(),
            overlays: overlays.clone(),
            picture: png.is_some(),
            threads: Threads::Beside,
        };
        (span, ask)
    };
    let (span, first) = ask(&rev);
    let mut out = json!({"song": a.dir.canonicalize().unwrap_or(a.dir.clone()), "bars": [range.bars.0, range.bars.1], "beats": [range.beats.0, range.beats.1], "range": range.from});
    let (heard, preroll_bar) = match &a.with {
        None => {
            let heard = listen::hear(rev, &s.packages, &first)?;
            merge(&mut out, heard.json());
            (heard, span.preroll_bar)
        }
        Some(file) => {
            // Checked on the session that read the song as it is: both share one base.
            let candidate = checked(&mut s, file)?;
            let (after_span, second) = ask(&candidate);
            let before = listen::hear(rev, &s.packages, &first)?;
            let after = listen::hear(candidate, &s.packages, &second)?;
            merge(&mut out, json!({"before": before.json(), "after": after.json(), "difference": listen::difference(&before, &after)}));
            for k in ["exact", "reproducible", "seconds"] {
                out[k] = after.json()[k].clone();
            }
            if (span.from - after_span.from).abs() > 5e-4 || (span.to - after_span.to).abs() > 5e-4 {
                out["moved"] = json!(format!(
                    "the change moves these bars from {:.3}-{:.3} s to {:.3}-{:.3} s: its tempo or meter changed, and both measure the same bars",
                    span.from, span.to, after_span.from, after_span.to
                ));
            }
            (after, after_span.preroll_bar)
        }
    };
    if !heard.exact {
        out["approximate"] = json!(format!(
            "it started at bar {preroll_bar} with nothing playing, so notes held from before then and the tails of what played earlier are missing"
        ));
    }
    out["as_heard"] = json!(heard_as.is_some());
    if let Some(h) = &heard_as {
        out["muted"] = json!(h.set.mute);
        out["soloed"] = json!(h.set.solo);
    }
    out["app"] = app.said(a.bars.is_none(), a.as_heard).map_or(Value::Null, Value::from);
    if let Asked::Answered(state) = &app {
        out["unsaved_plugins"] = json!(unsaved_plugins(state));
    }
    if let Some(claimed) = &mut png {
        let blocks: Vec<&listen::Picture> = heard.pictures.iter().map(|p| &p.1).collect();
        let drawn = listen::png::draw(&blocks, &claimed.path)?;
        claimed.done = true;
        out["png"] = json!({
            "path": claimed.path,
            "blocks": heard.pictures.iter().map(|p| &p.0).collect::<Vec<_>>(),
            "columns": drawn.columns,
            "column_ms": 50 * drawn.merged,
            "column_px": drawn.column_px,
            "row_px": listen::png::ROW_PX,
            "gap_px": listen::png::GAP_PX,
            "rows_hz": listen::THIRDS,
        });
    }
    out["priority"] = priority();
    if a.json {
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }
    words(&out, &heard, heard_as.as_ref(), a.with.is_some());
    Ok(())
}

/// The picture's path, claimed before anything renders: outside the song, in a folder that is
/// there, and writable. A file made for it is taken away again unless the picture is drawn.
struct Claimed {
    path: PathBuf,
    made: bool,
    done: bool,
}

impl Claimed {
    fn new(path: &Path, song: &Path) -> Result<Claimed> {
        let said = path.display();
        let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        let folder = parent.canonicalize().with_context(|| format!("--png {said}: its folder isn't there, so nothing was rendered"))?;
        let song = song.canonicalize().with_context(|| format!("reading {}", song.display()))?;
        if folder.starts_with(&song) {
            bail!("--png {said} is in the song, and m2 listen writes nothing into the song: nothing was rendered. Name a path outside it");
        }
        let made = !path.exists();
        File::options()
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .with_context(|| format!("--png {said} can't be written, so nothing was rendered"))?;
        Ok(Claimed { path: path.to_path_buf(), made, done: false })
    }
}

impl Drop for Claimed {
    fn drop(&mut self) {
        if self.made && !self.done {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// The open app, as far as a listen asked it.
enum Asked {
    /// Not asked: it doesn't have the song open, or the listen needs nothing from it.
    Not,
    /// Its state, for this song.
    Answered(Value),
    /// It has the song open, but didn't answer in time or couldn't be reached: its pid.
    Silent(u32),
}

impl Asked {
    /// Ask the app that has the song at `dir` open, when the listen needs its loop (`range`) or
    /// its mute and solo (`as_heard`), waiting no longer than [`PATIENCE`].
    fn of(dir: &Path, range: bool, as_heard: bool) -> Asked {
        if !range && !as_heard {
            return Asked::Not;
        }
        let Some(holder) = app::holder(dir) else { return Asked::Not };
        match app::looking_within(dir, &holder, PATIENCE) {
            Ok(a) => Asked::Answered(a.state().clone()),
            Err(_) => Asked::Silent(holder.pid),
        }
    }

    fn state(&self) -> Option<&Value> {
        match self {
            Asked::Answered(v) => Some(v),
            _ => None,
        }
    }

    /// What was read from the app, or instead of it, in words: None when it wasn't asked.
    fn said(&self, range: bool, as_heard: bool) -> Option<String> {
        let what = match (range, as_heard) {
            (true, true) => "its loop and its listening mute and solo",
            (true, false) => "its loop",
            _ => "its listening mute and solo",
        };
        match self {
            Asked::Not => None,
            Asked::Answered(_) => Some(format!("music2 has this song open: {what} were read from it")),
            Asked::Silent(pid) => Some(format!(
                "music2 has this song open (pid {pid}) but didn't answer in {} s: {what} were read from the song's .music2 files instead, \
                 which may not be what it plays",
                PATIENCE.as_secs()
            )),
        }
    }
}

/// The plugins whose window changed their sound, not saved yet, in the app's state: by their
/// track's and their own names where the view names them, else their node's.
fn unsaved_plugins(state: &Value) -> Vec<String> {
    let named = |node: &str| -> Option<String> {
        state["view"]["modules"].as_array()?.iter().find_map(|m| {
            let n = m["nodes"].as_array()?.iter().find(|n| n["plugin"].as_str() == Some(node))?;
            Some(format!("{}'s {}", m["label"].as_str()?, n["label"].as_str()?))
        })
    };
    let Some(plugins) = state["plugins"].as_object() else { return vec![] };
    plugins.iter().filter(|(_, p)| p["unsaved"] == json!(true)).map(|(node, _)| named(node).unwrap_or_else(|| node.clone())).collect()
}

/// One `m2 listen` at a time on this machine (AGENT-7): `~/.music2/listen.lock`, held while it
/// runs. A second waits, saying so.
fn one_at_a_time() -> Result<File> {
    let home = m2_os::dirs::home().context("no home folder for ~/.music2/listen.lock")?;
    let path = home.join(".music2").join("listen.lock");
    std::fs::create_dir_all(path.parent().expect("in .music2"))?;
    let f = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(&path)?;
    if !m2_os::fs::try_lock(&f).unwrap_or(true) {
        eprintln!("waiting for another m2 listen to finish (one at a time on a machine: {})", path.display());
        f.lock()?;
    }
    Ok(f)
}

/// What a listen renders, in music: its bars (from 1), its beats, and where it came from: `bars`,
/// `loop` or `song`. Each revision turns it into seconds of its own ([`span`]).
struct Range {
    bars: (u64, u64),
    beats: (f64, f64),
    from: &'static str,
}

/// A song with no clips has no bars to listen to.
const NO_CLIPS: &str = "the song has no clips, so it has no bars to listen to: m2 listen measures a song's bars up to the end of its last \
                        clip, or the loop when one is on";

/// The range `--bars` names, else the loop when one is on, else the whole song.
fn range(rev: &Revision, bars: Option<&str>, app: Option<&Value>, dir: &Path) -> Result<Range> {
    let data = &rev.compiled.data;
    let arr = &data.arrangement;
    let end = data.end();
    // The song's bars: as many as start before its last clip ends.
    let mut count = 0;
    while count < 100_000 && bar_start(arr, count) < end - 1e-9 {
        count += 1;
    }
    let ((first, last), beats, from) = match bars {
        Some(_) if count == 0 => bail!("{NO_CLIPS}"),
        Some(text) => {
            let bar = |s: &str| s.trim().parse::<u64>().ok().filter(|n| *n >= 1);
            let (a, b) = match text.split_once('-') {
                Some((a, b)) => (bar(a), bar(b)),
                None => (bar(text), bar(text)),
            };
            let (Some(a), Some(b)) = (a, b) else { bail!("--bars counts from 1: --bars 9-16 or --bars 9, not {text}") };
            if b < a {
                bail!("--bars {text} ends before it starts");
            }
            if b > count {
                let one = if count == 1 { "bar" } else { "bars" };
                bail!("the song has {count} {one}, so --bars {text} is past its end");
            }
            ((a, b), (bar_start(arr, a - 1), bar_start(arr, b)), "bars")
        }
        None => match looped(app, dir) {
            Some((start, stop)) => {
                let bar = |beat: f64| arr.bar_at(beat).0 + 1;
                ((bar(start), bar((stop - 1e-9).max(start))), (start, stop), "loop")
            }
            None if count == 0 => bail!("{NO_CLIPS}"),
            None => ((1, count), (0.0, end), "song"),
        },
    };
    Ok(Range { bars: (first, last), beats, from })
}

/// A range in one revision's seconds: where its render starts (its preroll), and the range.
struct Span {
    start: f64,
    from: f64,
    to: f64,
    preroll_bar: u64,
}

/// `r` in `rev`'s own tempo and meter: bars named are those bars of it, and a loop or the whole
/// song the same beats.
fn span(rev: &Revision, r: &Range) -> Span {
    let data = &rev.compiled.data;
    let arr = &data.arrangement;
    let (beats, first) = match r.from {
        "bars" => ((bar_start(arr, r.bars.0 - 1), bar_start(arr, r.bars.1)), r.bars.0),
        _ => (r.beats, arr.bar_at(r.beats.0).0 + 1),
    };
    let preroll_bar = first.saturating_sub(PREROLL_BARS).max(1);
    let tempo = data.tempo();
    Span {
        start: tempo.seconds_at(bar_start(arr, preroll_bar - 1)),
        from: tempo.seconds_at(beats.0),
        to: tempo.seconds_at(beats.1),
        preroll_bar,
    }
}

/// The loop's beats when one is on: the open app's, else the one `.music2/view.json` keeps.
fn looped(app: Option<&Value>, dir: &Path) -> Option<(f64, f64)> {
    let (start, end, on) = match app {
        Some(state) => {
            let l = &state["loop"];
            (l["start"].as_f64()?, l["end"].as_f64()?, l["on"].as_bool().unwrap_or(false))
        }
        None => {
            let ((start, end), on) = m2_server::graph::view_file::looped(dir)?;
            (start, end, on)
        }
    };
    (on && end > start).then_some((start, end))
}

/// The listening mute and solo, and where they were read.
struct Listening {
    set: Overlays,
    /// From the open app; else `.music2/overlays.json`.
    from_app: bool,
}

/// The listening mute and solo: the open app's, else `.music2/overlays.json`'s.
fn listening(app: Option<&Value>, dir: &Path) -> Listening {
    match app {
        Some(state) => {
            let keys =
                |k: &str| state["overlays"][k].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(str::to_string)).collect();
            Listening { set: Overlays { mute: keys("mute"), solo: keys("solo") }, from_app: true }
        }
        None => Listening { set: overlay::read(dir), from_app: false },
    }
}

/// Each overlay site's value in `rev` that the mute and solo `set` give, by its tracks' keys (MON-3).
fn values(rev: &Revision, set: &Overlays) -> Vec<f32> {
    let flat = &rev.compiled.flat;
    let sites: Vec<overlay::Site> = flat.overlays.iter().map(|s| (s.module.clone(), s.into.clone())).collect();
    overlay::values(&sites, &flat.routes, set, &flat.muted, &overlay::tracks(rev))
}

/// The candidate of the Tx in `file`, checked on `s`, the read-only session that read the song as
/// it is, and written nowhere (AGENT-7). A Tx that doesn't read or apply is refused, as
/// `m2 tx --check` would.
fn checked(s: &mut Session, file: &str) -> Result<Arc<Revision>> {
    let mut tx = match crate::read_tx(file)? {
        Ok(tx) => tx,
        Err(d) => {
            let r = m2_session::Refusal::Rejected { diagnostics: d, unexpected: None };
            bail!("--with {file} isn't a Tx: nothing was rendered: {r}")
        }
    };
    crate::text_bases(s, &mut tx);
    let c = s.check(&tx).map_err(|r| anyhow::anyhow!("--with {file}: nothing was rendered: {r}"))?;
    Ok(Arc::new(c.candidate().duplicate()))
}

/// What `m2 listen` says of its own priority, as the OS has it (AGENT-7): its threads', and its
/// plugin host's when one runs.
fn priority() -> Value {
    let said = |b: Option<bool>| match b {
        Some(true) => json!("background"),
        Some(false) => json!("normal"),
        None => Value::Null,
    };
    json!({
        "threads": said(m2_os::process::in_background(std::process::id())),
        "plugin_host": m2_plugins::host_process().map_or(Value::Null, |pid| said(m2_os::process::in_background(pid))),
    })
}

fn merge(out: &mut Value, more: Value) {
    if let (Some(o), Value::Object(m)) = (out.as_object_mut(), more) {
        o.extend(m);
    }
}

/// In words: the range, how it was heard, the measures, and that they are numbers, not listening.
fn words(out: &Value, heard: &Heard, as_heard: Option<&Listening>, with: bool) {
    let (a, b) = (out["bars"][0].as_u64().unwrap_or(1), out["bars"][1].as_u64().unwrap_or(1));
    let bars = if a == b { format!("bar {a}") } else { format!("bars {a}-{b}") };
    let from = match out["range"].as_str() {
        Some("loop") => " (the loop)",
        Some("song") => " (the whole song)",
        _ => "",
    };
    println!("{bars}{from}: {:.1} s from {:.1} s", heard.to - heard.from, heard.from);
    if let Some(words) = out["approximate"].as_str() {
        println!("approximate: {words}");
    }
    if let Some(words) = out["app"].as_str() {
        println!("app: {words}");
    }
    if let Some(h) = as_heard {
        let mut how = vec![];
        if !h.set.mute.is_empty() {
            how.push(format!("muted {}", h.set.mute.iter().cloned().collect::<Vec<_>>().join(", ")));
        }
        if !h.set.solo.is_empty() {
            how.push(format!("soloed {}", h.set.solo.iter().cloned().collect::<Vec<_>>().join(", ")));
        }
        let how = if how.is_empty() { "nothing muted or soloed".to_string() } else { how.join("; ") };
        let source = if h.from_app { "" } else { " (as .music2/overlays.json keeps it)" };
        println!("as you hear it{source}: {how}");
    }
    let unsaved: Vec<&str> = out["unsaved_plugins"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    if !unsaved.is_empty() {
        println!(
            "unsaved: {} changed in its window and not saved yet: this listen hears the saved state, not what plays in the app",
            unsaved.join(", ")
        );
    }
    if with {
        println!("with the change (--with), checked and never written: as it would be after it");
        if let Some(moved) = out["moved"].as_str() {
            println!("moved: {moved}");
        }
    }
    println!("{}", heard.words());
    if with {
        println!("{}", listen::difference_words(&out["difference"]));
    }
    if let Some(p) = out["png"]["path"].as_str() {
        println!("picture   {p}: {}", out["png"]["blocks"]);
    }
    println!(
        "These are measures, not listening. To hear them, point at the bars (m2 point <song> bars:{a}-{b} \"…\") and ask the musician \
         to play them; after a change, they press V to hear before and after."
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    // The review of ag-listen, F12: the song, then flags it knows, each once, a value after each
    // that takes one; --tracks keeps each key once.
    #[test]
    fn the_arguments_are_what_m2_listen_knows() {
        let of = |s: &str| args(&s.split(' ').map(str::to_string).collect::<Vec<_>>());
        let a = of("listen song --bars 2-3 --tracks kick,bass,kick --as-heard --json").unwrap();
        assert_eq!((a.dir, a.bars.as_deref(), a.tracks), (PathBuf::from("song"), Some("2-3"), Some(vec!["kick".into(), "bass".into()])));
        assert!(a.as_heard && a.json && a.with.is_none() && a.png.is_none());
        for (bad, says) in [
            ("listen song --bras 1-2", "--bras"),
            ("listen song --track kick", "--track"),
            ("listen song --bars 2 --png --json", "--png needs a value"),
            ("listen song --png", "--png needs a value"),
            ("listen song --bars 1 --bars 2", "named twice"),
            ("listen song other", "one song"),
            ("listen --json", "m2 listen <project>"),
        ] {
            let e = of(bad).err().map(|e| e.to_string()).unwrap_or_default();
            assert!(e.contains(says), "{bad}: {e}");
        }
    }

    // The review of ag-listen, F17: the plugins whose window changed their sound, not saved yet, as
    // the app's state says, by their names in the view.
    #[test]
    fn the_unsaved_plugins_are_named() {
        let state = json!({
            "plugins": {"p/n2": {"window": "open", "latched": [], "unsaved": true}, "q/n1": {"window": "closed", "latched": [], "unsaved": false}},
            "view": {"modules": [{"label": "Lead", "nodes": [{"label": "Probe", "plugin": "p/n2"}]}]},
        });
        assert_eq!(unsaved_plugins(&state), ["Lead's Probe"]);
        assert_eq!(unsaved_plugins(&json!({})), Vec::<String>::new());
    }
}
