//! Step 3's acceptance gates, measured (contract, Acceptance gates; step 3a's design, §15).
//!
//! Each gate plays a song on a [`rig::Rig`], an engine driven in simulated time, and judges what
//! it hears with the detectors in [`signal`], plus the engine's own counters where it keeps them:
//!
//! - the **replay** rig ([`replay`]) is the engine as it is at HEAD: every seek, edit and loop
//!   wrap rebuilds the renderer and replays the song to the playhead. It runs today, and it is
//!   the negative reference: it must fail the gates that RENDER-8 to RENDER-11 exist for.
//! - the **engine** rig (`engine`, behind the `engine` feature) is step 3a's live engine, through
//!   `m2_play::Player::driven`. It must pass them.
//!
//! [`server`] measures what the contract times on the reference projects through the app's own
//! [`m2_server::graph::Graph`], in real time, with the silent player.
//!
//! Run them with `cargo run -p m2-gates --release -- --help`; see this crate's README.

pub mod gates;
pub mod plugins;
pub mod replay;
pub mod report;
pub mod rig;
pub mod server;
pub mod signal;
pub mod songs;

#[cfg(feature = "engine")]
pub mod engine;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use m2_ir::Flat;
use m2_runtime::PackageSet;
use m2_session::reference::Reference;
use m2_song::{DataSet, Project};

/// A compiled song, ready to play.
#[derive(Clone)]
pub struct Song {
    pub dir: PathBuf,
    pub flat: Flat,
    pub data: DataSet,
    pub rate: u32,
    /// How long compiling it took: what the contract's edit budgets are counted after.
    pub compile: Duration,
}

impl Song {
    /// The transport frame of `beat`, by the formula notes use (step 3a's design, §3).
    pub fn frame(&self, beat: f64) -> u64 {
        (self.data.tempo().seconds_at(beat) * self.rate as f64).round() as u64
    }

    pub fn beat(&self, frame: u64) -> f64 {
        self.data.tempo().beat_at(frame as f64 / self.rate as f64)
    }

    /// The frame the last clip ends at.
    pub fn end(&self) -> u64 {
        self.frame(self.data.end())
    }

    /// Every note-on the song has, played from 0 without a jump: `(transport frame, pitch bits)`
    /// for each note of each notes lane its graph reads, in order. Worked out here from the lane
    /// files' notes, not by the engine, so a gate can hold the engine's trace against it.
    pub fn note_ons(&self) -> Vec<(u64, u32)> {
        let mut ons = vec![];
        for lane in m2_runtime::lane_slots(&self.flat).iter().filter(|l| matches!(l.kind, m2_ir::Kind::Events(_))) {
            for n in self.data.notes(&lane.module, &lane.lane) {
                let (start, end) = (self.frame(n.start), self.frame(n.end));
                // A note shorter than a frame never sounds.
                if end > start {
                    ons.push((start, (n.pitch as f32).to_bits()));
                }
            }
        }
        ons.sort_unstable();
        ons
    }
}

/// Where gate runs keep their projects, and the std package they compile and build with.
pub struct Lab {
    pub work: PathBuf,
    /// Compiling binds plugins, so it needs a set of its own.
    compiler: PackageSet,
    pub packages: Arc<PackageSet>,
    /// Whether the engine rig carries plugin instances across rebuilds (HOST-5); off, it is the
    /// negative reference (`--no-carry`).
    pub carry: bool,
    n: AtomicUsize,
}

/// The std package: `$M2_STD`, else this source tree's.
pub fn std_dir() -> PathBuf {
    match std::env::var_os("M2_STD").filter(|v| !v.is_empty()) {
        Some(dir) => dir.into(),
        None => Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std"),
    }
}

/// A fixture of this repository, such as `examples/first-song`.
pub fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel)
}

pub fn load_std() -> Result<PackageSet> {
    let mut p = PackageSet::default();
    p.load(&std_dir()).with_context(|| format!("loading std from {}", std_dir().display()))?;
    Ok(p)
}

impl Lab {
    /// A lab in `work`, which is created if missing.
    pub fn new(work: &Path) -> Result<Lab> {
        std::fs::create_dir_all(work).with_context(|| format!("creating {}", work.display()))?;
        Ok(Lab { work: work.to_path_buf(), compiler: load_std()?, packages: Arc::new(load_std()?), carry: true, n: AtomicUsize::new(0) })
    }

    /// A new folder in the lab, never used before.
    pub fn dir(&self, name: &str) -> PathBuf {
        self.work.join(format!("{name}-{}", self.n.fetch_add(1, Ordering::Relaxed)))
    }

    /// A new project made of `files`.
    pub fn project(&self, name: &str, files: &songs::Files) -> Result<PathBuf> {
        let dir = self.dir(name);
        songs::write(&dir, files)?;
        Ok(dir)
    }

    /// A reference project (p16, p50) in a new folder.
    pub fn reference(&self, r: &Reference) -> Result<PathBuf> {
        let dir = self.dir(&r.name());
        m2_session::reference::write(&dir, r)?;
        Ok(dir)
    }

    /// A copy of the project in `from` (a fixture, or another lab project), to edit.
    pub fn copy(&self, from: &Path, name: &str) -> Result<PathBuf> {
        let dir = self.dir(name);
        copy_tree(from, &dir)?;
        Ok(dir)
    }

    /// Compile the project in `dir`; it must have nothing that stops it playing as written.
    pub fn compile(&mut self, dir: &Path) -> Result<Song> {
        let t = Instant::now();
        let project = Project::open(dir).map_err(|d| anyhow!("{} doesn't open: {d:?}", dir.display()))?;
        let c = m2_session::compile(&project, &mut self.compiler).map_err(|d| anyhow!("{} doesn't compile: {d:?}", dir.display()))?;
        let compile = t.elapsed();
        if let Some(e) = c.problems.iter().find(|d| d.severity == m2_song::Severity::Error) {
            return Err(anyhow!("{}: {e}", dir.display()));
        }
        Ok(Song { dir: dir.to_path_buf(), flat: c.flat, data: c.data, rate: project.sample_rate(), compile })
    }

    /// A fresh session on the project in `dir`, as the app opens it: for edits made the way the
    /// app makes them (`add_effect`).
    pub fn session(&self, dir: &Path) -> Result<m2_session::Session> {
        m2_session::Session::open(dir, load_std()?).map_err(|d| anyhow!("{} doesn't open: {d:?}", dir.display()))
    }
}

/// The accepted revision of `session`, as a [`Song`] whose compile took `compile`.
pub fn accepted(session: &m2_session::Session, compile: Duration) -> Song {
    let rev = session.accepted();
    Song {
        dir: rev.project.dir.clone(),
        flat: rev.compiled.flat.clone(),
        data: rev.compiled.data.clone(),
        rate: rev.project.sample_rate(),
        compile,
    }
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from).with_context(|| format!("reading {}", from.display()))? {
        let entry = entry?;
        let (src, dst) = (entry.path(), to.join(entry.file_name()));
        if entry.file_type()?.is_dir() {
            // A project's own state (journal, layout) never comes along.
            if entry.file_name() != ".music2" {
                copy_tree(&src, &dst)?;
            }
        } else {
            std::fs::copy(&src, &dst).with_context(|| format!("copying {}", src.display()))?;
        }
    }
    Ok(())
}
