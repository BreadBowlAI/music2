//! G13 (the agents design's track 5; AGENT-7): G12's live run while agents work beside it, as they
//! would beside the musician. Each agent is a process of its own, the `m2` built beside `m2-gates`,
//! with a scratch `HOME` (nothing reads the real `~/.music2`):
//!
//! - **An app** that has the song open: this process's own server, on the silent player
//!   (`M2_AUDIO=none`, as the gates set it), holding the song's lock, its `session.json` in that
//!   `HOME`. It takes the edits `m2` forwards to it (AGENT-1): its compiles run here, beside
//!   playback, as the app's run beside its own.
//! - **Edits**: `m2 set` of a channel's fader, one after another, alternating two values, each one
//!   undo step in the app.
//! - **Listens**: `m2 listen` back to back on 2 bars of the song, each at background priority
//!   (AGENT-7); every other one on 4 bars of `examples/with-plugins` when the test plugins are
//!   there, so its plugin host, at background priority too, runs beside playback.
//!
//! When the run ends, the agent at work is ended, and what landed is counted: each listen, each
//! edit, how long each took, and whether each listen said it ran in the background.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

/// The bars each listen measures: 2 bars from bar 33, past the song's start, so each is the range
/// render agents listen with (RENDER-4), with its 2 bars of preroll. Two, so that several land in
/// the minute on the slowest machine it is judged on: on the Mac's efficiency cores, at background
/// priority, one listen of 8 bars of p50 (13.6 s of one fast core on Linux) didn't end in the
/// minute (the Mac's first full check); 2 bars take 5.0 s there.
pub const BARS: &str = "33-34";
/// The bars each listen of the song with plugins measures.
pub const PLUGIN_BARS: &str = "1-4";

/// One agent's command that ran to its end.
#[derive(Clone, Debug)]
pub struct Ran {
    pub ok: bool,
    pub seconds: f64,
    /// Its error's first line, when it failed.
    pub said: String,
    /// A listen's: whether it said its threads ran at background priority.
    pub background: Option<bool>,
    /// A listen's of the song with plugins: whether it said its plugin host did.
    pub host: Option<bool>,
}

/// What one agent did: each command that ran to its end, and how long the one at work when the run
/// ended had run (it was ended, and isn't counted).
type Runs = (Vec<Ran>, Option<f64>);

/// The agents at work beside a live run.
pub struct Agents {
    stop: Arc<AtomicBool>,
    listens: JoinHandle<Runs>,
    edits: JoinHandle<Runs>,
    /// The app: kept open until they stop.
    _app: m2_server::Running,
    pub knob: String,
}

/// What the agents did.
#[derive(Clone, Debug)]
pub struct Did {
    pub listens: Vec<Ran>,
    pub edits: Vec<Ran>,
    /// How long the listen at work when the run ended had run, when one was.
    pub listening: Option<f64>,
    /// How long the edit at work when the run ended had run, when one was.
    pub editing: Option<f64>,
}

/// The `m2` beside this program.
fn m2() -> Result<PathBuf> {
    let me = std::env::current_exe().context("this program's path")?;
    let m2 = me.with_file_name(format!("m2{}", std::env::consts::EXE_SUFFIX));
    if !m2.exists() {
        bail!("G13 runs the m2 beside {}: build it first (cargo build --release -p m2)", me.display());
    }
    Ok(m2)
}

/// `m2 args…` with `home` as HOME, no device and no MIDI.
fn command(m2: &Path, home: &Path, args: &[&str]) -> Command {
    let mut c = Command::new(m2);
    c.args(args).env("HOME", home).env("M2_AUDIO", "none").env("M2_MIDI", "none").env_remove("M2_AS").stdin(Stdio::null());
    c
}

/// Start the agents on the song in `dir`, their scratch files in `work`; every other listen on the
/// song with plugins in `plugins`, when there is one.
pub fn start(dir: &Path, work: &Path, plugins: Option<&Path>) -> Result<Agents> {
    let m2 = m2()?;
    let home = work.join("g13-home");
    std::fs::create_dir_all(home.join(".music2"))?;
    // The app: the song open in this process's server, holding its lock as `music2` does.
    let key = format!("g13-{}", std::process::id());
    let options =
        m2_server::ServeOptions { port: 0, web_dir: work.join("g13-no-web"), token: key.clone(), page_token: format!("{key}-page") };
    let app = m2_server::start(options).context("G13's app")?;
    app.state.graph.open(dir).context("G13's app opening the song")?;
    // The url and the key are plain ASCII: written as they are.
    let published = format!(r#"{{"url": "{}", "token": "{key}", "pid": {}}}"#, app.url, std::process::id());
    std::fs::write(home.join(".music2/session.json"), published)?;
    // The knob the edits set: the first channel fader the code writes a number for.
    let controls = command(&m2, &home, &["controls", &dir.to_string_lossy()]).output()?;
    let knob = String::from_utf8_lossy(&controls.stdout)
        .lines()
        .find(|l| l.contains(" db ") && l.contains('`') && !l.contains('@'))
        .and_then(|l| l.split_whitespace().next().map(str::to_string))
        .context("G13 found no channel fader to set in the song")?;
    let stop = Arc::new(AtomicBool::new(false));
    let song = dir.to_string_lossy().into_owned();
    let listens = {
        let (m2, home, song, stop) = (m2.clone(), home.clone(), song.clone(), stop.clone());
        let plugins = plugins.map(|p| p.to_string_lossy().into_owned());
        std::thread::Builder::new().name("g13-listens".into()).spawn(move || {
            // What it says of its own priority, and its plugin host's, in its JSON's `priority`.
            agent(
                &stop,
                |n| match &plugins {
                    Some(p) if n % 2 == 1 => command(&m2, &home, &["listen", p, "--bars", PLUGIN_BARS, "--json"]),
                    _ => command(&m2, &home, &["listen", &song, "--bars", BARS, "--json"]),
                },
                |o| {
                    // As its JSON says it: `"threads": "background"`, and `"plugin_host"` the same,
                    // `"normal"`, or null with no host.
                    let text = String::from_utf8_lossy(&o.stdout);
                    let said = |who: &str| match () {
                        _ if text.contains(&format!(r#""{who}": "background""#)) => Some(true),
                        _ if text.contains(&format!(r#""{who}": "normal""#)) => Some(false),
                        _ => None,
                    };
                    (Some(said("threads") == Some(true)), said("plugin_host"))
                },
            )
        })?
    };
    let edits = {
        let (m2, home, knob, stop) = (m2.clone(), home.clone(), knob.clone(), stop.clone());
        std::thread::Builder::new().name("g13-edits".into()).spawn(move || {
            let value = |n: usize| if n.is_multiple_of(2) { "-11" } else { "-12" };
            agent(
                &stop,
                |n| command(&m2, &home, &["set", &song, &knob, value(n), "--as", "G13", "--why", "an agent's edit"]),
                |_| (None, None),
            )
        })?
    };
    Ok(Agents { stop, listens, edits, _app: app, knob })
}

/// Run `make(n)` for n = 0, 1, … one after another until `stop`: what each that ran to its end did,
/// with what `background` says of its priority and its plugin host's. The one at work when `stop`
/// comes is ended, and not counted: how long it had run is returned beside them.
fn agent(stop: &AtomicBool, make: impl Fn(usize) -> Command, background: impl Fn(&Output) -> (Option<bool>, Option<bool>)) -> Runs {
    let mut ran = vec![];
    for n in 0.. {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let t = Instant::now();
        let Ok(mut child) = make(n).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() else {
            ran.push(Ran { ok: false, seconds: 0.0, said: "it didn't start".into(), background: None, host: None });
            break;
        };
        let Some(out) = until_done(&mut child, stop) else { return (ran, Some(t.elapsed().as_secs_f64())) };
        let said = String::from_utf8_lossy(&out.stderr).lines().find(|l| l.starts_with("error")).unwrap_or_default().to_string();
        let (background, host) = background(&out);
        ran.push(Ran { ok: out.status.success(), seconds: t.elapsed().as_secs_f64(), said, background, host });
    }
    (ran, None)
}

/// `child`'s output once it ends; None when `stop` came first (it is ended then).
fn until_done(child: &mut Child, stop: &AtomicBool) -> Option<Output> {
    // Its pipes are read as it runs, so a long answer never blocks it.
    let read = |p: Option<Box<dyn std::io::Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = vec![];
            if let Some(mut p) = p {
                let _ = p.read_to_end(&mut buf);
            }
            buf
        })
    };
    let out = read(child.stdout.take().map(|p| Box::new(p) as Box<dyn std::io::Read + Send>));
    let err = read(child.stderr.take().map(|p| Box::new(p) as Box<dyn std::io::Read + Send>));
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            return Some(Output { status, stdout: out.join().unwrap_or_default(), stderr: err.join().unwrap_or_default() });
        }
        if stop.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

impl Agents {
    /// Stop: the agent at work is ended; what landed before.
    pub fn stop(self) -> Did {
        self.stop.store(true, Ordering::Relaxed);
        let (listens, listening) = self.listens.join().unwrap_or_default();
        let (edits, editing) = self.edits.join().unwrap_or_default();
        Did { listens, edits, listening, editing }
    }
}

impl Did {
    /// What landed, in words: how many, how long they took, the first failure, and how long the one
    /// at work when the run ended (`cut`) had run.
    pub fn said(runs: &[Ran], cut: Option<f64>) -> String {
        let ok: Vec<f64> = runs.iter().filter(|r| r.ok).map(|r| r.seconds).collect();
        let mut words = format!("{} landed", ok.len());
        if !ok.is_empty() {
            let mean = ok.iter().sum::<f64>() / ok.len() as f64;
            let longest = ok.iter().copied().fold(0.0, f64::max);
            words += &format!(", {mean:.1} s each, the longest {longest:.1} s");
        }
        if let Some(r) = runs.iter().find(|r| !r.ok) {
            words += &format!("; {} failed, the first: {}", runs.iter().filter(|r| !r.ok).count(), r.said);
        }
        if let Some(cut) = cut {
            words += &format!("; one was still at work after {cut:.1} s, and was ended");
        }
        words
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ran(ok: bool, seconds: f64) -> Ran {
        Ran { ok, seconds, said: if ok { String::new() } else { "error: it broke".into() }, background: Some(true), host: None }
    }

    // G13 on the Mac's first full check said "0 landed, -0.0 s each, the longest 0.0 s": its one
    // listen was still at work when the run ended, and nothing said so. What was at work then is
    // said, with how long it had run; with nothing landed there is no mean to give.
    #[test]
    fn what_was_still_at_work_when_the_run_ended_is_said() {
        assert_eq!(Did::said(&[], Some(61.24)), "0 landed; one was still at work after 61.2 s, and was ended");
        assert_eq!(Did::said(&[], None), "0 landed");
        assert_eq!(
            Did::said(&[ran(true, 20.0), ran(true, 22.0)], Some(5.0)),
            "2 landed, 21.0 s each, the longest 22.0 s; one was still at work after 5.0 s, and was ended"
        );
        assert_eq!(
            Did::said(&[ran(true, 2.0), ran(false, 1.0)], None),
            "1 landed, 2.0 s each, the longest 2.0 s; 1 failed, the first: error: it broke"
        );
    }
}
