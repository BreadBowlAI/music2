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

pub mod arena;
pub mod baseline;
pub mod gates;
pub mod paired;
pub mod plugins;
pub mod realtime;
pub mod replay;
pub mod report;
pub mod rig;
pub mod server;
pub mod signal;
pub mod songs;
pub mod store;

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
use m2_song::{DataSet, Project, Snapshot};

/// A compiled song, ready to play.
#[derive(Clone)]
pub struct Song {
    pub dir: PathBuf,
    pub flat: Flat,
    pub data: DataSet,
    pub rate: u32,
    /// How long compiling it took: what the contract's edit budgets are counted after.
    pub compile: Duration,
    /// Its build and the snapshot it read: what names its nodes against another song's, as the
    /// server names a revision's against what plays (`m2_session::tx::continues`).
    pub code: Arc<(m2_session::Compiled, Snapshot)>,
}

/// What a render of it names its faults from (RENDER-1).
impl m2_editor::faults::Song for Song {
    fn ir(&self) -> &m2_ir::Ir {
        &self.code.0.ir
    }
    fn flat(&self) -> &Flat {
        &self.flat
    }
    fn data(&self) -> &DataSet {
        &self.data
    }
}

impl Song {
    /// A song of build `c`, read from `snapshot` of the project in `dir`.
    pub fn of(dir: &Path, c: m2_session::Compiled, snapshot: Snapshot, rate: u32, compile: Duration) -> Song {
        let (flat, data) = (c.flat.clone(), c.data.clone());
        Song { dir: dir.to_path_buf(), flat, data, rate, compile, code: Arc::new((c, snapshot)) }
    }

    /// For each of its top-level nodes, the node of `old` it continues: what an activation from
    /// `old` may keep (ID-15), as the server names them.
    pub fn continues(&self, old: &Song) -> m2_runtime::Continues {
        let ((a, sa), (b, sb)) = (&*old.code, &*self.code);
        m2_session::tx::continues::between((a, sa), (b, sb))
    }

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
    /// Whether the engine rig keeps the instances that continue across activations (RENDER-25);
    /// off, every one is new and the whole old graph plays out as ghosts: the negative reference
    /// (`--no-arena`).
    pub arena: bool,
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
        Ok(Lab { work: work.to_path_buf(), compiler: load_std()?, packages: Arc::new(load_std()?), arena: true, n: AtomicUsize::new(0) })
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
        let (project, snapshot) = open(dir)?;
        let c = m2_session::compile(&project, &mut self.compiler).map_err(|d| anyhow!("{} doesn't compile: {d:?}", dir.display()))?;
        let compile = t.elapsed();
        if let Some(e) = c.problems.iter().find(|d| d.severity == m2_song::Severity::Error) {
            return Err(anyhow!("{}: {e}", dir.display()));
        }
        Ok(Song::of(dir, c, snapshot, project.sample_rate(), compile))
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
    Song::of(&rev.project.dir, rev.compiled.clone(), rev.snapshot.clone(), rev.project.sample_rate(), compile)
}

/// The project in `dir`, and the snapshot it was read from.
pub fn open(dir: &Path) -> Result<(Project, Snapshot)> {
    let snapshot = Snapshot::read(dir).with_context(|| format!("reading {}", dir.display()))?;
    let project = Project::from_snapshot(&snapshot).map_err(|d| anyhow!("{} doesn't open: {d:?}", dir.display()))?;
    Ok((project, snapshot))
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
