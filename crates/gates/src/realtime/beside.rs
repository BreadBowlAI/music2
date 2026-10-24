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
//! - **Listens**: `m2 listen` back to back on 8 bars of the song, each at background priority on
//!   the slow cores, its plugin host too (AGENT-7).
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

/// The bars each listen measures: 8 bars from bar 33, past the song's start, so each is the range
/// render agents listen with (RENDER-4).
pub const BARS: &str = "33-40";

/// One agent's command that ran to its end.
#[derive(Clone, Debug)]
pub struct Ran {
    pub ok: bool,
    pub seconds: f64,
    /// Its error's first line, when it failed.
    pub said: String,
    /// A listen's: whether it said its threads ran at background priority.
    pub background: Option<bool>,
}

/// The agents at work beside a live run.
pub struct Agents {
    stop: Arc<AtomicBool>,
    listens: JoinHandle<Vec<Ran>>,
    edits: JoinHandle<Vec<Ran>>,
    /// The app: kept open until they stop.
    _app: m2_server::Running,
    pub knob: String,
}

/// What the agents did.
#[derive(Clone, Debug)]
pub struct Did {
    pub listens: Vec<Ran>,
    pub edits: Vec<Ran>,
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

/// Start the agents on the song in `dir`, their scratch files in `work`.
pub fn start(dir: &Path, work: &Path) -> Result<Agents> {
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
        std::thread::Builder::new().name("g13-listens".into()).spawn(move || {
            // What it says of its own priority, in its JSON: `"threads": "background"`.
            agent(
                &stop,
                |_| command(&m2, &home, &["listen", &song, "--bars", BARS, "--json"]),
                |o| Some(String::from_utf8_lossy(&o.stdout).contains(r#""threads": "background""#)),
            )
        })?
    };
    let edits = {
        let (m2, home, knob, stop) = (m2.clone(), home.clone(), knob.clone(), stop.clone());
        std::thread::Builder::new().name("g13-edits".into()).spawn(move || {
            let value = |n: usize| if n.is_multiple_of(2) { "-11" } else { "-12" };
            agent(&stop, |n| command(&m2, &home, &["set", &song, &knob, value(n), "--as", "G13", "--why", "an agent's edit"]), |_| None)
        })?
    };
    Ok(Agents { stop, listens, edits, _app: app, knob })
}

/// Run `make(n)` for n = 0, 1, … one after another until `stop`: what each that ran to its end did.
/// The one at work when `stop` comes is ended, and not counted.
fn agent(stop: &AtomicBool, make: impl Fn(usize) -> Command, background: impl Fn(&Output) -> Option<bool>) -> Vec<Ran> {
    let mut ran = vec![];
    for n in 0.. {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let t = Instant::now();
        let Ok(mut child) = make(n).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() else {
            ran.push(Ran { ok: false, seconds: 0.0, said: "it didn't start".into(), background: None });
            break;
        };
        let Some(out) = until_done(&mut child, stop) else { break };
        let said = String::from_utf8_lossy(&out.stderr).lines().find(|l| l.starts_with("error")).unwrap_or_default().to_string();
        ran.push(Ran { ok: out.status.success(), seconds: t.elapsed().as_secs_f64(), said, background: background(&out) });
    }
    ran
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
        Did { listens: self.listens.join().unwrap_or_default(), edits: self.edits.join().unwrap_or_default() }
    }
}

impl Did {
    /// What landed, in words: how many of each, how long they took, and the first failure.
    pub fn said(runs: &[Ran]) -> String {
        let ok: Vec<f64> = runs.iter().filter(|r| r.ok).map(|r| r.seconds).collect();
        let mean = ok.iter().sum::<f64>() / ok.len().max(1) as f64;
        let longest = ok.iter().copied().fold(0.0, f64::max);
        let failed =
            runs.iter().find(|r| !r.ok).map(|r| format!("; {} failed, the first: {}", runs.iter().filter(|r| !r.ok).count(), r.said));
        format!("{} landed, {mean:.1} s each, the longest {longest:.1} s{}", ok.len(), failed.unwrap_or_default())
    }
}
