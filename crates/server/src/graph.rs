//! The new core in the app: a song-as-program project, shown as a graph.
//!
//! One editing session ([`m2_session::Session`]) and one player ([`m2_play::Player`]).
//! Knob drags preview straight into the playing renderer; releasing commits the value
//! to code through the session. A value commit doesn't rebuild the renderer: the
//! committed graph equals the playing one with that value, which the preview already
//! set. Edits made elsewhere (a text editor, an agent) are picked up by [`Graph::poll`],
//! which rebuilds the renderer at the current position when the accepted revision
//! changes.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use m2_ir::Origin;
use m2_play::Player;
use m2_runtime::{PackageSet, Renderer};
use m2_session::Session;
use parking_lot::Mutex;
use serde_json::{Value, json};

struct Open {
    session: Session,
    /// The accepted revision the player is playing.
    playing: u64,
}

#[derive(Default)]
pub struct Graph {
    open: Mutex<Option<Open>>,
    player: Mutex<Option<Player>>,
}

/// The std package: `$M2_STD`, or the one in this source tree.
fn std_dir() -> PathBuf {
    std::env::var_os("M2_STD").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../std")))
}

fn packages() -> Result<PackageSet> {
    let mut p = PackageSet::default();
    p.load(&std_dir()).context("loading the std package")?;
    Ok(p)
}

/// A renderer for the accepted revision, positioned at `frame`.
fn renderer(session: &Session, sample_rate: u32, frame: u64) -> Result<Renderer> {
    let rev = session.accepted();
    let mut r = Renderer::for_project(&rev.compiled.flat, &rev.project, &session.packages, sample_rate)?;
    r.seek(frame)?;
    Ok(r)
}

impl Graph {
    fn player(&self) -> Result<Player> {
        let mut p = self.player.lock();
        if p.is_none() {
            *p = Some(Player::start()?);
        }
        Ok(p.clone().expect("started"))
    }

    pub fn open(&self, dir: &Path) -> Result<Value> {
        let session = Session::open(dir, packages()?).map_err(|d| anyhow!(d.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n")))?;
        let player = self.player()?;
        player.stop();
        player.load(renderer(&session, player.sample_rate, 0)?);
        let playing = session.accepted().number;
        *self.open.lock() = Some(Open { session, playing });
        self.state().context("no project")
    }

    pub fn close(&self) {
        if let Some(p) = self.player.lock().as_ref() {
            p.stop();
        }
        *self.open.lock() = None;
    }

    /// The view, the project's name and folder, and the transport.
    pub fn state(&self) -> Option<Value> {
        let open = self.open.lock();
        let o = open.as_ref()?;
        let rev = o.session.accepted();
        let (playing, seconds) = self.transport_state();
        Some(json!({
            "name": rev.project.manifest.project.name,
            "dir": rev.project.dir,
            "view": o.session.view(),
            "playing": playing,
            "seconds": seconds,
        }))
    }

    pub fn transport_state(&self) -> (bool, f64) {
        match self.player.lock().as_ref() {
            Some(p) => (p.is_playing(), p.position() as f64 / p.sample_rate as f64),
            None => (false, 0.0),
        }
    }

    /// Change a value in the playing audio only (while a knob is dragged).
    pub fn preview(&self, origin: Origin, value: f64) -> Result<()> {
        self.player()?.set_const(origin, value);
        Ok(())
    }

    /// Commit a value to code. Errors carry what went wrong (stale, conflict, …).
    pub fn set(&self, revision: u64, id: &str, value: f64, expect: Option<f64>) -> Result<Value> {
        let mut open = self.open.lock();
        let o = open.as_mut().context("no project is open")?;
        let before = o.session.accepted().number;
        match o.session.set(revision, id, value, expect) {
            Ok(c) => {
                // The playing graph differs from the committed one only by this value,
                // which a preview usually set already; set it in case there was none.
                if let Some(p) = self.player.lock().as_ref() {
                    p.set_const(c.origin.clone(), c.value);
                }
                if o.playing == before {
                    o.playing = c.revision;
                }
                Ok(json!({"file": c.file, "before": c.before, "after": c.after, "revision": c.revision}))
            }
            Err(e) => Err(anyhow!("{e}")),
        }
    }

    pub fn transport(&self, action: &str) -> Result<()> {
        let player = self.player()?;
        match action {
            "play" => player.play(),
            "stop" => player.stop(),
            "toggle" if player.is_playing() => player.stop(),
            "toggle" => player.play(),
            "restart" => {
                let open = self.open.lock();
                let o = open.as_ref().context("no project is open")?;
                player.load(renderer(&o.session, player.sample_rate, 0)?);
            }
            other => return Err(anyhow!("unknown transport action {other}")),
        }
        Ok(())
    }

    /// Pick up edits made outside the app. Returns whether clients should refresh.
    pub fn poll(&self) -> bool {
        let mut open = self.open.lock();
        let Some(o) = open.as_mut() else { return false };
        if !o.session.refresh() {
            return false;
        }
        let accepted = o.session.accepted().number;
        if accepted != o.playing
            && let Some(p) = self.player.lock().as_ref()
        {
            // Rebuild at the current position (seeking replays from the start).
            match renderer(&o.session, p.sample_rate, p.position()) {
                Ok(r) => {
                    p.load(r);
                    o.playing = accepted;
                }
                Err(e) => eprintln!("music2: couldn't rebuild the graph: {e:#}"),
            }
        }
        true
    }
}
