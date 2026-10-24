//! `m2 listen` (AGENT-7; track 5 of the agents design): what a song's bars measure, for an agent
//! that can't hear them. An offline range render in this process, never the app's engine: at
//! background priority with its plugin host, on the cores playback doesn't need, one at a time on
//! the machine. It reads the song's files (or a Tx it checks and never writes, `--with`), and
//! writes nothing into the song.
//!
//! The range: `--bars 9-16` (counted from 1), else the loop when one is on (the open app's, else
//! `.music2/view.json`'s), else the song to the end of its last clip; 2 bars of preroll, so it is
//! approximate unless the preroll reaches the start (RENDER-5). `--as-heard` hears the listening
//! mute and solo (the open app's, else `.music2/overlays.json`), and says so (MON-4).

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use m2_editor::clips::bar_start;
use m2_editor::listen::{self, Ask, Heard};
use m2_editor::render::Threads;
use m2_session::{Revision, Session};
use m2_server::graph::overlay::{self, Overlays};
use serde_json::{Value, json};

use crate::{app, flag, report};

const USAGE: &str =
    "m2 listen <project> [--bars 9-16] [--tracks kick,bass] [--as-heard] [--with tx.json] [--png out.png] [--json]";
/// Bars rendered before the range and dropped (RENDER-4).
const PREROLL_BARS: u64 = 2;

/// `m2 listen <project> …`.
pub fn listen(args: &[String]) -> Result<()> {
    let dir = PathBuf::from(args.get(1).filter(|a| !a.starts_with("--")).context(USAGE)?);
    // Its own process at background priority, every thread of it, and its plugin host too: first,
    // so every thread it starts takes it (AGENT-7).
    if let Err(e) = m2_os::process::background() {
        eprintln!("m2: couldn't lower its priority ({e})");
    }
    m2_plugins::background_host();
    let _one = one_at_a_time()?;
    let s = crate::reader(&dir)?;
    let rev = Arc::new(s.accepted().duplicate());
    // The open app, when it has this song: its loop, and its mute and solo. Read, never written.
    let app = app::holder(&dir).and_then(|_| app::looking(&dir).ok()).and_then(|a| a.looked("GET", "/api/graph", None).ok()).map(|v| v.1);
    let range = range(&rev, flag(args, "--bars").as_deref(), app.as_ref(), &dir)?;
    let as_heard = args.iter().any(|a| a == "--as-heard").then(|| listening(&rev, app.as_ref(), &dir));
    let png = flag(args, "--png").map(PathBuf::from);
    let tracks = flag(args, "--tracks").map(|t| t.split(',').map(|k| k.trim().to_string()).filter(|k| !k.is_empty()).collect());
    let ask = Ask {
        from: range.seconds.0,
        to: range.seconds.1,
        preroll: range.seconds.0 - range.preroll,
        tracks,
        overlays: as_heard.as_ref().map(|h| h.values.clone()),
        picture: png.is_some(),
        threads: Threads::Beside,
    };
    let mut out = json!({"song": dir.canonicalize().unwrap_or(dir.clone()), "bars": [range.bars.0, range.bars.1], "beats": [range.beats.0, range.beats.1], "range": range.from});
    let heard = match flag(args, "--with") {
        None => {
            let heard = listen::hear(rev, &s.packages, &ask)?;
            merge(&mut out, heard.json());
            heard
        }
        Some(file) => {
            let candidate = checked(&dir, &file)?;
            let before = listen::hear(rev, &s.packages, &ask)?;
            let after = listen::hear(candidate, &s.packages, &ask)?;
            merge(&mut out, json!({"before": before.json(), "after": after.json(), "difference": listen::difference(&before, &after)}));
            for k in ["exact", "reproducible", "seconds"] {
                out[k] = after.json()[k].clone();
            }
            after
        }
    };
    if !heard.exact {
        out["approximate"] = json!(format!(
            "it started at bar {} with nothing playing, so notes held from before then and the tails of what played earlier are missing",
            range.preroll_bar
        ));
    }
    out["as_heard"] = json!(as_heard.is_some());
    if let Some(h) = &as_heard {
        out["muted"] = json!(h.set.mute);
        out["soloed"] = json!(h.set.solo);
    }
    if let Some(path) = &png {
        let blocks: Vec<&listen::Picture> = heard.pictures.iter().map(|p| &p.1).collect();
        let drawn = listen::png::draw(&blocks, path)?;
        out["png"] = json!({
            "path": path,
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
    if args.iter().any(|a| a == "--json") {
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }
    words(&out, &heard, as_heard.as_ref(), flag(args, "--with").is_some());
    Ok(())
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

/// What a listen renders: its bars (from 1), its beats and seconds, and where its preroll starts.
struct Range {
    bars: (u64, u64),
    beats: (f64, f64),
    seconds: (f64, f64),
    /// Where the render starts: the seconds, and the bar.
    preroll: f64,
    preroll_bar: u64,
    /// Where the range came from: `bars`, `loop` or `song`.
    from: &'static str,
}

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
            None if count == 0 => bail!("the song has no clips to listen to: name bars, as --bars 1-4"),
            None => ((1, count), (0.0, end), "song"),
        },
    };
    let tempo = data.tempo();
    let preroll_bar = first.saturating_sub(PREROLL_BARS).max(1);
    Ok(Range {
        bars: (first, last),
        beats,
        seconds: (tempo.seconds_at(beats.0), tempo.seconds_at(beats.1)),
        preroll: tempo.seconds_at(bar_start(arr, preroll_bar - 1)),
        preroll_bar,
        from,
    })
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

/// The listening mute and solo, and each overlay site's value they give (MON-3).
struct Listening {
    set: Overlays,
    values: Vec<f32>,
}

/// The listening mute and solo: the open app's, else `.music2/overlays.json`'s.
fn listening(rev: &Revision, app: Option<&Value>, dir: &Path) -> Listening {
    let set = match app {
        Some(state) => {
            let keys = |k: &str| state["overlays"][k].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(str::to_string)).collect();
            Overlays { mute: keys("mute"), solo: keys("solo") }
        }
        None => overlay::read(dir),
    };
    let flat = &rev.compiled.flat;
    let sites: Vec<overlay::Site> = flat.overlays.iter().map(|s| (s.module.clone(), s.into.clone())).collect();
    let values = overlay::values(&sites, &flat.routes, &set, &flat.muted, &overlay::tracks(rev));
    Listening { set, values }
}

/// The candidate of the Tx in `file`, checked on a read-only session of the song at `dir` and
/// written nowhere (AGENT-7). A Tx that doesn't read or apply is refused, as `m2 tx --check` would.
fn checked(dir: &Path, file: &str) -> Result<Arc<Revision>> {
    let mut tx = match crate::read_tx(file)? {
        Ok(tx) => tx,
        Err(d) => {
            let r = m2_session::Refusal::Rejected { diagnostics: d, unexpected: None };
            bail!("--with {file} isn't a Tx: nothing was rendered: {r}")
        }
    };
    let mut s = Session::open_read_only(dir, crate::packages()?).map_err(report)?;
    crate::text_bases(&s, &mut tx);
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
    if let Some(h) = as_heard {
        let mut how = vec![];
        if !h.set.mute.is_empty() {
            how.push(format!("muted {}", h.set.mute.iter().cloned().collect::<Vec<_>>().join(", ")));
        }
        if !h.set.solo.is_empty() {
            how.push(format!("soloed {}", h.set.solo.iter().cloned().collect::<Vec<_>>().join(", ")));
        }
        let how = if how.is_empty() { "nothing muted or soloed".to_string() } else { how.join("; ") };
        println!("as you hear it: {how}");
    }
    if with {
        println!("with the change (--with), checked and never written: as it would be after it");
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
