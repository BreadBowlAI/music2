//! Rendering a song offline to a WAV file: `m2 render` and the app's Export Audio.
//!
//! Both build a [`Render`] for one compiled revision and write it ([`Render::write_for`]), so the
//! same project rendered to the same length gives the same file, byte for byte. It renders the
//! way the app's player plays: a NaN or infinity on the way is silenced, not an error.
//!
//! [`Render::range`] is the range render agents listen with (RENDER-4): it starts the engine a
//! preroll before the range, as a live jump does, instead of replaying the song from its start.
//! It says it is approximate unless the preroll reaches the start (RENDER-5).
//!
//! A render is written to a hidden part file beside the one asked for and named when it is
//! complete. The part is locked while it's written, so one left by a render that never
//! finished (the program was killed) is found unlocked and removed by the next render there.
//!
//! A render runs on as many threads as its [`Threads`] say, with the same bits on any number
//! (RENDER-2). A song that hosts a plugin renders on one for now: plugins on worker threads come
//! with live playback on all cores.
//!
//! A render never finishes with a part silent (RENDER-1): any fault that changed its sound stops
//! it, a node's error or one its ring holds (a file it can't read, a stretch that failed), named by
//! its [`Namer`] as one error, [`Faulted`].

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, anyhow, bail};
use m2_ir::{FOp, Flat};
use m2_os::process::Cores;
use m2_runtime::{PackageSet, PluginInstance, Pool, Renderer, Runner};
use m2_song::DataSet;

use crate::faults::Namer;
pub use crate::faults::{Faulted, Parts, Song};

/// The longest render. A WAV file's sizes are 32-bit, so it can't hold much over 4 hours of
/// 24-bit stereo at 48 kHz; and a song this long is most likely a clip put far past the rest.
pub const MAX_SECONDS: f64 = 3.0 * 3600.0;
/// After the last clip the song plays on (reverb, releases) until it has been silent
/// for this long, which the file keeps…
pub const TAIL_SILENCE: f64 = 0.5;
/// …or for this long at most.
pub const TAIL_MAX: f64 = 10.0;
/// Silent: quieter than -90 dBFS, about the quietest step a 16-bit file can hold.
const SILENT: f32 = 3.2e-5;
/// Frames rendered between progress reports.
const CHUNK: usize = 4096;
/// Of the progress bar, the share the song takes before its tail.
const SONG_SHARE: f64 = 0.9;

/// How much to render.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Length {
    /// To the end of the last clip, then on while it still sounds: until it has been
    /// silent for [`TAIL_SILENCE`], or [`TAIL_MAX`] past the end at most.
    Song,
    /// Exactly this many seconds.
    Seconds(f64),
}

/// How many threads a render uses (8a). The same bits on any number (RENDER-2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Threads {
    /// Nothing else plays: the fast cores that share the last cache before memory
    /// (`Cores::per_last_cache`). Every fast core on a Mac: 8 threads rendered p50 22% faster than
    /// 4 there (8a's Mac sweep). One die of a two-die CPU: a render over both dies was no faster
    /// than over one (8a's spike).
    Alone,
    /// Beside playback: the slow cores (one at least), at background priority.
    Beside,
    /// Exactly this many, 1 to 64: `--threads`, `M2_THREADS`, and tests.
    Exactly(usize),
}

impl Threads {
    /// What `M2_THREADS` says, if it is set.
    pub fn from_env() -> Result<Option<Threads>> {
        Ok(m2_runtime::threads::from_env()?.map(Threads::Exactly))
    }

    /// A count of threads, 1 to 64, as `--threads` and `M2_THREADS` give it.
    pub fn parse(text: &str) -> Result<Threads> {
        m2_runtime::threads::parse(text).map(Threads::Exactly)
    }

    /// How many threads this is on a machine with `cores`: never more than a pool has.
    pub fn count(self, cores: &Cores) -> usize {
        match self {
            Threads::Alone => cores.per_last_cache.clamp(1, Pool::MAX_THREADS),
            Threads::Beside => cores.slow.clamp(1, Pool::MAX_THREADS),
            Threads::Exactly(n) => n,
        }
    }
}

/// The threads to render `renderer` on, as `threads` says: one for a song that hosts a plugin,
/// and one when the OS won't start more. The pool's workers stop when it drops.
pub fn threads_for(renderer: &Renderer, threads: Threads) -> (Runner, Option<Pool>) {
    let n = if renderer.hosts_plugins() { 1 } else { threads.count(&m2_os::process::cores()) };
    if n <= 1 {
        return (Runner::one(), None);
    }
    match Pool::new(n, m2_runtime::threads::setup(threads == Threads::Beside)) {
        Ok((pool, runner)) => (runner, Some(pool)),
        Err(_) => (Runner::one(), None),
    }
}

/// A song ready to render: one revision's renderer, at the start of what's rendered.
pub struct Render {
    renderer: Renderer,
    /// Names what stopped it, when a fault did.
    namer: Namer,
    /// The threads it renders on; the pool's workers stop when it drops.
    runner: Runner,
    _pool: Option<Pool>,
    sample_rate: u32,
    /// The song's frame where the file starts.
    first: u64,
    /// The frame where the last clip ends.
    end: u64,
    length: Length,
    /// Set to stop rendering: the write fails and leaves nothing behind.
    stop: Arc<AtomicBool>,
    exact: bool,
    reproducible: bool,
    /// Written as `m2 bounce` writes: 32-bit float, nothing clipped ([`Render::write_float`]).
    float: bool,
}

/// What a render says when a plugin's latency changed while it rendered (RENDER-17).
pub const LATENCY_CHANGED: &str = "a plugin's latency changed while the song rendered, so that plugin played out of time with \
     the rest from then on. Render again: the next render makes up for the new latency (latency.changed).";

/// What a render wrote.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rendered {
    pub frames: u64,
    pub sample_rate: u32,
    /// The loudest sample, in dBFS, before anything above 0 dB was clipped.
    pub peak_db: f64,
    /// Samples silenced because a kernel made a NaN or an infinity.
    pub non_finite: u64,
    /// Whether it is what an export holds there: rendered from the song's start (RENDER-5). A
    /// range render whose preroll starts later is approximate in its tails.
    pub exact: bool,
    /// Whether the same files render the same bits again: false when the song hosts a plugin
    /// (RENDER-2).
    pub reproducible: bool,
    /// The threads it rendered on.
    pub threads: usize,
    /// Whether it hosts a plugin, so it rendered on one thread.
    pub hosts_plugins: bool,
    /// Plugins whose latency changed while it rendered: it kept what each had at its start, so
    /// they played misaligned from then on (RENDER-17, `latency.changed`).
    pub latency_changed: usize,
}

impl Rendered {
    pub fn seconds(&self) -> f64 {
        self.frames as f64 / self.sample_rate as f64
    }
}

/// Whether `flat` renders the same bits every time: it plays no hosted plugin (RENDER-2).
pub(crate) fn pure(flat: &Flat) -> bool {
    !flat.order.iter().any(|&i| matches!(flat.nodes[i].op, FOp::Plugin { .. }))
}

/// Where a render from `from` seconds starts and where the song's last clip ends, in frames;
/// refused when it can't be rendered.
fn bounds(data: &DataSet, sample_rate: u32, from: f64, length: Length) -> Result<(u64, u64)> {
    if !(from >= 0.0 && from.is_finite()) {
        bail!("can't start rendering at {from} seconds");
    }
    if let Length::Seconds(s) = length
        && !(s > 0.0 && s.is_finite())
    {
        bail!("can't render {s} seconds");
    }
    let tempo = data.tempo();
    let last = data.end();
    let end = (tempo.seconds_at(last) * sample_rate as f64).round() as u64;
    let first = (from * sample_rate as f64).round() as u64;
    match length {
        Length::Seconds(s) if s > MAX_SECONDS => bail!("can't render {s} seconds: {} is the most music2 renders", hours(MAX_SECONDS)),
        Length::Song if end.saturating_sub(first) as f64 / sample_rate as f64 + TAIL_MAX > MAX_SECONDS => {
            // The bar the last clip ends in: one ending on a bar line ends the bar before it.
            let bar = match data.arrangement.bar_at(last) {
                (b, at) if at == 0.0 && b > 0 => b,
                (b, _) => b + 1,
            };
            bail!(
                "its last clip ends in bar {bar}, {} in, past the {} music2 can export. Is a clip far past the rest?",
                hours(end as f64 / sample_rate as f64),
                hours(MAX_SECONDS)
            );
        }
        _ => {}
    }
    Ok((first, end))
}

impl Render {
    /// Render a compiled song (its graph and the data it plays) from `from` seconds in (replaying
    /// up to there first), on `threads`: exact (RENDER-1).
    pub fn new(
        song: Arc<dyn Song>,
        packages: &PackageSet,
        sample_rate: u32,
        from: f64,
        length: Length,
        threads: Threads,
    ) -> Result<Render> {
        let namer = Namer::new(0, song.clone(), packages);
        let (flat, data) = (song.flat(), song.data());
        let (first, end) = bounds(data, sample_rate, from, length)?;
        let mut renderer = Renderer::new(flat, data, packages, sample_rate).map_err(|e| namer.faulted(e))?;
        renderer.set_strict(false);
        let (mut runner, pool) = threads_for(&renderer, threads);
        renderer.seek_on(first, &mut runner).map_err(|e| namer.faulted(e))?;
        Ok(Render::of(renderer, namer, (runner, pool), sample_rate, (first, end), length, true, pure(flat)))
    }

    #[allow(clippy::too_many_arguments)]
    fn of(
        renderer: Renderer,
        namer: Namer,
        (runner, pool): (Runner, Option<Pool>),
        sample_rate: u32,
        (first, end): (u64, u64),
        length: Length,
        exact: bool,
        reproducible: bool,
    ) -> Render {
        Render {
            renderer,
            namer,
            runner,
            _pool: pool,
            sample_rate,
            first,
            end,
            length,
            stop: Arc::default(),
            exact,
            reproducible,
            float: false,
        }
    }

    /// An export from the song's start, as [`Render::new`] makes one, that plays the live engine's
    /// plugin instances `lent` names, by node (RENDER-1): the app's export of a song with plugins.
    /// Each lent instance is reset at its first call; kernels start from init, and any other plugin
    /// is created for it. The live engine must be held while it renders, and each lent instance
    /// loaded with its committed state, set to offline mode and activated first.
    pub fn lending(
        song: Arc<dyn Song>,
        packages: &PackageSet,
        sample_rate: u32,
        length: Length,
        lent: &HashMap<usize, Arc<PluginInstance>>,
        threads: Threads,
    ) -> Result<Render> {
        let namer = Namer::new(0, song.clone(), packages);
        let (flat, data) = (song.flat(), song.data());
        let (first, end) = bounds(data, sample_rate, 0.0, length)?;
        let mut renderer = Renderer::lending(flat, data, packages, sample_rate, lent).map_err(|e| namer.faulted(e))?;
        renderer.set_strict(false);
        let threads = threads_for(&renderer, threads);
        Ok(Render::of(renderer, namer, threads, sample_rate, (first, end), length, true, pure(flat)))
    }

    /// Play `inst`, a new instance, at node `node` in place of the live one lent to this export
    /// ([`Render::lending`]): taking that one offline failed, or playback didn't pause (RENDER-1).
    /// Only before it renders. False when the node plays no lent instance.
    pub fn unlend(&mut self, node: usize, inst: Arc<PluginInstance>) -> bool {
        self.renderer.unlend(node, inst)
    }

    /// A range render (RENDER-4): from `from` seconds in, after `preroll` seconds rendered and
    /// dropped. The engine starts from init where the preroll starts, with the transport there,
    /// as a live jump does: notes sounding there aren't chased, and tails from before it are
    /// missing, so it is approximate (RENDER-5). A preroll that reaches the song's start makes it
    /// the exact render. For listening only: an export or a bounce is always exact (RENDER-3).
    pub fn range(
        song: Arc<dyn Song>,
        packages: &PackageSet,
        sample_rate: u32,
        from: f64,
        preroll: f64,
        length: Length,
        threads: Threads,
    ) -> Result<Render> {
        if !(preroll >= 0.0 && preroll.is_finite()) {
            bail!("can't start {preroll} seconds before the range");
        }
        let (first, end) = bounds(song.data(), sample_rate, from, length)?;
        let t0 = first.saturating_sub((preroll * sample_rate as f64).round() as u64);
        if t0 == 0 {
            return Render::new(song, packages, sample_rate, from, length, threads);
        }
        let namer = Namer::new(0, song.clone(), packages);
        let (flat, data) = (song.flat(), song.data());
        let mut renderer = Renderer::starting_at(flat, data, packages, sample_rate, t0).map_err(|e| namer.faulted(e))?;
        renderer.set_strict(false);
        let (mut runner, pool) = threads_for(&renderer, threads);
        let mut scratch = vec![[0.0f32; 2]; CHUNK];
        let mut left = first - t0;
        while left > 0 {
            let n = left.min(CHUNK as u64) as usize;
            renderer.process_on(&mut scratch[..n], &mut runner).map_err(|e| namer.faulted(e))?;
            left -= n as u64;
        }
        Ok(Render::of(renderer, namer, (runner, pool), sample_rate, (first, end), length, false, pure(flat)))
    }

    /// Whether it renders what an export holds from where it starts (see [`Rendered::exact`]).
    pub fn exact(&self) -> bool {
        self.exact
    }

    /// Stop rendering when `stop` is set (the app is quitting): the write then fails, and
    /// leaves no file behind.
    pub fn stop_when(mut self, stop: Arc<AtomicBool>) -> Render {
        self.stop = stop;
        self
    }

    /// Render to `path` as a 24-bit stereo WAV, replacing what is there. It is written beside
    /// `path` and renamed into place when complete, so `path` is never a half-written file.
    /// `progress` is told how far along it is (0..1) after every few thousand frames.
    pub fn write(self, path: &Path, progress: impl FnMut(f64)) -> Result<Rendered> {
        let r = self.write_as(path, progress, |part| std::fs::rename(part, path).map(|()| path.to_path_buf()))?;
        Ok(r.1)
    }

    /// Render to `out` as `m2 render` and Export Audio write for the project at `project`: into its
    /// `assets/` only an exact render (RENDER-5), and only as a new file, never over one that is
    /// there (WRITE-1: an asset is created, never replaced, FMT-14); anywhere else as
    /// [`Render::write`] does. Both refusals come before anything is rendered.
    pub fn write_for(self, project: &Path, out: &Path, progress: impl FnMut(f64)) -> Result<Rendered> {
        if !in_assets(project, out)? {
            return self.write(out, progress);
        }
        if !self.exact {
            return Err(approximate(out));
        }
        let taken = || {
            anyhow!(
                "{} is there already: a render goes into the project's {}/ as a new file, never over one (WRITE-1). Name it anew",
                out.display(),
                m2_song::path::ASSETS
            )
        };
        if out.symlink_metadata().is_ok() {
            return Err(taken());
        }
        // Named only if the name is still free when it is done (a create-only rename never replaces).
        match self.write_new([out.to_path_buf()], progress) {
            Ok((_, r)) => Ok(r),
            Err(_) if out.symlink_metadata().is_ok() => Err(taken()),
            Err(e) => Err(e),
        }
    }

    /// Render to `path` as `m2 bounce` writes its file: 32-bit float, two channels, every sample
    /// as rendered, nothing clipped (D15), and only as a new file. Written beside it and named
    /// create-only when complete, as [`Render::write_new`] does. A stem is `m2 bounce --track`'s
    /// bits (C3): [`rooted`], from 0 for [`bounce_seconds`].
    pub fn write_float(mut self, path: &Path, progress: impl FnMut(f64)) -> Result<Rendered> {
        self.float = true;
        let r = self.write_as(path, progress, |part| name_new(part, path).map(|()| path.to_path_buf()))?;
        Ok(r.1)
    }

    /// Render like [`Render::write`] to the first of `paths` that is free when the render is
    /// done. It never replaces a file, not even one that appeared while it rendered. Returns
    /// the file written.
    pub fn write_new(self, paths: impl IntoIterator<Item = PathBuf>, progress: impl FnMut(f64)) -> Result<(PathBuf, Rendered)> {
        let mut paths = paths.into_iter().peekable();
        let first = paths.peek().cloned().context("no file to write to")?;
        self.write_as(&first, progress, |part| {
            for path in paths {
                match name_new(part, &path) {
                    Ok(()) => return Ok(path),
                    Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
                    Err(e) => return Err(e),
                }
            }
            Err(std::io::Error::other("every name for it is taken"))
        })
    }

    /// Render into a part file beside `path`, then `name` it. The part is gone either way. Its
    /// bytes are pushed before it gets its name, and its folder synced after, so a Tx that names
    /// it (a clip placed on a lane) makes it durable by its barrier at the latest (WRITE-1).
    fn write_as(
        mut self,
        path: &Path,
        mut progress: impl FnMut(f64),
        name: impl FnOnce(&Path) -> std::io::Result<PathBuf>,
    ) -> Result<(PathBuf, Rendered)> {
        let part = part_path(path)?;
        sweep(m2_os::fs::folder_of(&part));
        let written = self.write_to(&part, &mut progress).and_then(|(lock, r)| {
            m2_os::fs::push(&lock)?;
            let named = name(&part)?;
            drop(lock);
            m2_os::fs::sync_dir(m2_os::fs::folder_of(&named))?;
            Ok((named, r))
        });
        if written.is_err() {
            let _ = std::fs::remove_file(&part);
        }
        match written {
            // A fault says what stopped it, and that the file wasn't made.
            Err(e) if Faulted::of(&e).is_some() => Err(e),
            // Named by the file asked for: the one beside it is a detail.
            w => w.with_context(|| format!("couldn't write {}", path.display())),
        }
    }

    /// Write the WAV to `part`. Returns the part's lock, held until it is named.
    fn write_to(&mut self, part: &Path, progress: &mut dyn FnMut(f64)) -> Result<(File, Rendered)> {
        let spec = match self.float {
            true => hound::WavSpec {
                channels: 2,
                sample_rate: self.sample_rate,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
            false => {
                hound::WavSpec { channels: 2, sample_rate: self.sample_rate, bits_per_sample: 24, sample_format: hound::SampleFormat::Int }
            }
        };
        let file = File::create(part)?;
        // A second handle on the same open file keeps the lock after the writer closes its own.
        // A disk that can't lock just goes unswept.
        let lock = file.try_clone()?;
        let _ = m2_os::fs::try_lock(&lock);
        let mut out = Out { wav: hound::WavWriter::new(BufWriter::new(file), spec)?, frames: 0, peak: 0.0, float: self.float };
        let mut chunk = vec![[0.0f32; 2]; CHUNK];
        let start = self.first;
        match self.length {
            Length::Seconds(s) => {
                let total = (s * self.sample_rate as f64).round() as u64;
                self.stream(&mut out, &mut chunk, total, &mut |done| progress(done as f64 / total as f64))?;
            }
            Length::Song => {
                let song = self.end.saturating_sub(start);
                self.stream(&mut out, &mut chunk, song, &mut |done| progress(SONG_SHARE * done as f64 / song as f64))?;
                // The tail is held back until it's clear where it ends.
                let room = (TAIL_MAX * self.sample_rate as f64).round() as usize;
                let quiet = (TAIL_SILENCE * self.sample_rate as f64).round() as usize;
                let mut tail: Vec<[f32; 2]> = Vec::with_capacity(room.min(CHUNK * 16));
                // Up to and including the tail's last sample that isn't silent.
                let mut sounding = 0;
                while tail.len() < (sounding + quiet).min(room) {
                    let n = CHUNK.min(room - tail.len());
                    self.process(&mut chunk[..n])?;
                    if let Some(last) = chunk[..n].iter().rposition(|f| f[0].abs() > SILENT || f[1].abs() > SILENT) {
                        sounding = tail.len() + last + 1;
                    }
                    tail.extend_from_slice(&chunk[..n]);
                    progress(SONG_SHARE + (1.0 - SONG_SHARE) * tail.len() as f64 / room as f64);
                }
                out.push(&tail[..(sounding + quiet).min(room)])?;
            }
        }
        // No block follows soon: the workers sleep now.
        self.runner.rest();
        let Out { wav, frames, peak, .. } = out;
        wav.finalize()?;
        progress(1.0);
        let rendered = Rendered {
            frames,
            sample_rate: self.sample_rate,
            peak_db: 20.0 * (peak.max(1e-9) as f64).log10(),
            non_finite: self.renderer.non_finite(),
            latency_changed: self.renderer.latency_moved().len(),
            exact: self.exact,
            reproducible: self.reproducible,
            threads: self.runner.threads(),
            hosts_plugins: self.renderer.hosts_plugins(),
        };
        Ok((lock, rendered))
    }

    /// Render the next frames, unless asked to stop. A fault that stops it is named.
    fn process(&mut self, frames: &mut [[f32; 2]]) -> Result<()> {
        if self.stop.load(Ordering::Relaxed) {
            bail!("the render was stopped");
        }
        self.renderer.process_on(frames, &mut self.runner).map_err(|e| self.namer.faulted(e))
    }

    /// Render `frames` frames straight into the file.
    fn stream(&mut self, out: &mut Out, chunk: &mut [[f32; 2]], frames: u64, progress: &mut dyn FnMut(u64)) -> Result<()> {
        let mut done = 0;
        while done < frames {
            let n = (frames - done).min(chunk.len() as u64) as usize;
            self.process(&mut chunk[..n])?;
            out.push(&chunk[..n])?;
            done += n as u64;
            progress(done);
        }
        Ok(())
    }
}

struct Out {
    wav: hound::WavWriter<BufWriter<File>>,
    frames: u64,
    peak: f32,
    /// 32-bit float as rendered; else 24-bit, clipped at 0 dBFS.
    float: bool,
}

impl Out {
    fn push(&mut self, frames: &[[f32; 2]]) -> Result<()> {
        for f in frames {
            for &s in f {
                self.peak = self.peak.max(s.abs());
                match self.float {
                    true => self.wav.write_sample(s)?,
                    false => self.wav.write_sample((s.clamp(-1.0, 1.0) * 8_388_607.0).round() as i32)?,
                }
            }
        }
        self.frames += frames.len() as u64;
        Ok(())
    }
}

/// The song `ir` heard through export `export` of track `track` alone: what `m2 bounce --track`
/// renders, and a stem (C3). The root module stays `song` (ID-3) and plays the track's export
/// through a node of its own. Refused when there is no such track.
pub fn rooted(ir: &m2_ir::Ir, track: &str, export: &str) -> Result<m2_ir::Ir> {
    if !ir.modules.iter().any(|m| m.key == track) {
        bail!("there's no track `{track}`");
    }
    let mut ir = ir.clone();
    let root = ir.modules.iter_mut().find(|m| m.key == m2_song::ROOT).context("the song has no root module")?;
    // A reference is `node.port`, so the node's id has no dot.
    let id = "m2-bounce".to_string();
    let op = m2_ir::Op::Import { module: track.to_string(), export: export.to_string() };
    root.nodes.push(m2_ir::Node { id: id.clone(), op, src: None, origin: None, frame: None });
    root.exports.insert(id.clone(), id.clone());
    ir.root.export = id;
    Ok(ir)
}

/// How long `m2 bounce` renders a song by default, and a stem: to 2 s past where its last clip ends.
pub fn bounce_seconds(data: &DataSet) -> f64 {
    data.tempo().seconds_at(data.end()) + 2.0
}

/// Refuse to write a render that isn't exact into the project at `project`'s `assets/`
/// (RENDER-5): an approximation must never become part of the song. `out` is where it would go,
/// relative to the working directory or absolute; neither it nor `assets/` need exist yet. Paths
/// are compared by the folders they name, so another spelling, a `..` or a link to `assets/` is
/// refused too.
pub fn refuse_in_assets(project: &Path, out: &Path) -> Result<()> {
    match in_assets(project, out)? {
        true => Err(approximate(out)),
        false => Ok(()),
    }
}

/// Why an approximate render isn't written to `out`, in the project's `assets/`.
fn approximate(out: &Path) -> anyhow::Error {
    anyhow!(
        "{} is in the project's {}/: a render that isn't exact never goes there (RENDER-5). Render it exactly (without a preroll), or \
         write it somewhere else",
        out.display(),
        m2_song::path::ASSETS
    )
}

/// Whether `out` is in the project at `project`'s `assets/`, however the path spells it (see
/// [`refuse_in_assets`]).
pub fn in_assets(project: &Path, out: &Path) -> Result<bool> {
    let out = std::path::absolute(out)?;
    let root = project.canonicalize().with_context(|| format!("{} can't be read", project.display()))?;
    // The nearest of `out`'s folders that exists, and the names after it.
    let mut rest = vec![];
    let mut at = out.as_path();
    let base = loop {
        if let Ok(p) = at.canonicalize() {
            break p;
        }
        match (at.parent(), at.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name);
                at = parent;
            }
            // A `..` after a folder that doesn't exist: nothing can be written there.
            _ => return Ok(false),
        }
    };
    let assets = file_id(&root.join(m2_song::path::ASSETS));
    let inside = assets.is_some() && base.ancestors().any(|a| file_id(a) == assets);
    // `assets/` not made yet, in any case of letters (macOS and Windows don't tell them apart).
    let made =
        file_id(&base) == file_id(&root) && rest.last().is_some_and(|c| c.to_string_lossy().eq_ignore_ascii_case(m2_song::path::ASSETS));
    Ok(inside || made)
}

/// What a path names, whatever it is called.
fn file_id(p: &Path) -> Option<m2_os::fs::FileId> {
    m2_os::fs::file_id(p).ok()
}

/// Where a render is written until it's complete: hidden, beside the file it becomes.
pub(crate) fn part_path(path: &Path) -> Result<PathBuf> {
    let name = path.file_name().with_context(|| format!("{} is not a file name", path.display()))?;
    Ok(path.with_file_name(format!(".{}.{}.part", name.to_string_lossy(), std::process::id())))
}

/// A [`part_path`] name: `.<file>.<process id>.part`.
fn is_part(name: &str) -> bool {
    let rest = name.strip_prefix('.').and_then(|n| n.strip_suffix(".part"));
    rest.and_then(|r| r.rsplit_once('.'))
        .is_some_and(|(file, pid)| !file.is_empty() && !pid.is_empty() && pid.bytes().all(|b| b.is_ascii_digit()))
}

/// Remove the part files in `dir` left by renders that never finished: a render holds its
/// part's lock until the part is named, and a killed program's locks are let go.
pub(crate) fn sweep(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        if !e.file_type().is_ok_and(|t| t.is_file()) || !e.file_name().to_str().is_some_and(is_part) {
            continue;
        }
        if File::open(e.path()).is_ok_and(|f| m2_os::fs::try_lock(&f).is_ok_and(|taken| taken)) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Give the finished `part` the name `path` only if nothing has it: a create-only rename fails if
/// something does, even a file made a moment ago (OS-07).
pub(crate) fn name_new(part: &Path, path: &Path) -> std::io::Result<()> {
    match m2_os::fs::rename(part, path, m2_os::fs::Rename::Excl) {
        // A volume that can't rename that way: look, then rename.
        Err(e) if e.kind() == ErrorKind::Unsupported && path.symlink_metadata().is_err() => std::fs::rename(part, path),
        named => named,
    }
}

/// `3 hours`, `4 h 10 min`, `25 min`.
fn hours(seconds: f64) -> String {
    let minutes = (seconds / 60.0).round() as u64;
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} min"),
        (1, 0) => "1 hour".into(),
        (h, 0) => format!("{h} hours"),
        (h, m) => format!("{h} h {m} min"),
    }
}

#[cfg(test)]
mod tests {
    use m2_song::Project;

    use super::*;

    /// The rate these tests render at; a project renders at its own (RENDER-31a).
    const SAMPLE_RATE: u32 = 48_000;

    fn std_packages() -> PackageSet {
        let mut p = PackageSet::default();
        p.load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")).unwrap();
        p
    }

    struct Dir(PathBuf);

    impl Dir {
        fn new(files: &[(&str, &str)]) -> Dir {
            static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let dir = std::env::temp_dir().join(format!("m2-render-test-{}-{n}", std::process::id()));
            for (name, text) in [("music2.toml", crate::testing::MANIFEST)].iter().chain(files) {
                let p = dir.join(name);
                std::fs::create_dir_all(p.parent().unwrap()).unwrap();
                std::fs::write(p, text).unwrap();
            }
            Dir(dir)
        }

        /// Render the project in it to `out.wav`; the samples read back, left channel.
        fn render(&self, length: Length) -> (Rendered, Vec<f32>) {
            let project = Project::open(&self.0).unwrap();
            let mut packages = std_packages();
            let compiled = Arc::new(m2_session::compile(&project, &mut packages).unwrap_or_else(|e| panic!("{e:?}")));
            let out = self.0.join("out.wav");
            let r = Render::new(compiled.clone(), &packages, SAMPLE_RATE, 0.0, length, Threads::Exactly(1))
                .unwrap()
                .write(&out, |_| {})
                .unwrap();
            let mut wav = hound::WavReader::open(&out).unwrap();
            assert_eq!((wav.spec().channels, wav.spec().sample_rate, wav.spec().bits_per_sample), (2, SAMPLE_RATE, 24));
            assert_eq!(wav.duration() as u64, r.frames);
            let left = wav.samples::<i32>().step_by(2).map(|s| s.unwrap() as f32 / 8_388_607.0).collect();
            (r, left)
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const SR: usize = SAMPLE_RATE as usize;
    /// 120 bpm, so a beat is half a second.
    const TEMPO: (&str, &str) = ("data/arrangement.json", r#"{"tempo": [{"beat": 0, "bpm": 120}]}"#);

    fn pad_song(clips: &str) -> Dir {
        Dir::new(&[
            (
                "song.ts",
                "import { song, track } from '@m2/core';\nimport { pad } from 'std/instruments';\nconst p = track('pad', ({ lane }) => pad(lane.notes('chords')));\nexport default song(() => p.out);\n",
            ),
            ("data/pad/chords.json", clips),
            TEMPO,
        ])
    }

    #[test]
    fn a_song_renders_to_its_last_clip_and_on_until_its_release_falls_silent() {
        // A note held to the clip's end at beat 4 (2 s): its 0.8 s release rings past it.
        let dir = pad_song(r#"{"clips": [{"start": 0, "length": 4, "notes": [{"pitch": 57, "start": 0, "length": 4}]}]}"#);
        let (r, left) = dir.render(Length::Song);
        let (end, quiet) = (2 * SR, SR / 2);
        assert!(r.frames as usize > end + quiet, "the release is kept: {} frames", r.frames);
        assert!(r.frames as usize <= end + 10 * SR);
        // Sample-exact: it ends half a second after the last sample that isn't silent.
        let last = r.frames as usize - quiet;
        assert!(left[last - 1].abs() > SILENT, "sounding until {last}");
        assert!(left[last..].iter().all(|s| s.abs() <= SILENT), "then silent");
        assert!(r.peak_db > -30.0 && r.peak_db < 0.0, "peak {}", r.peak_db);
    }

    #[test]
    fn a_tail_that_never_falls_silent_stops_ten_seconds_past_the_end() {
        let dir = Dir::new(&[
            (
                "song.ts",
                "import { hz, song } from '@m2/core';\nimport { osc } from 'std/dsp';\nexport default song(() => osc.sine(hz(440)));\n",
            ),
            TEMPO,
        ]);
        // No clips: the song ends where it starts, and only the tail is rendered.
        let (r, _) = dir.render(Length::Song);
        assert_eq!(r.frames as usize, 10 * SR);
    }

    #[test]
    fn a_song_with_no_clips_and_no_sound_is_half_a_second_of_silence() {
        let dir = pad_song(r#"{"clips": []}"#);
        let (r, left) = dir.render(Length::Song);
        assert_eq!(r.frames as usize, SR / 2);
        assert!(left.iter().all(|&s| s == 0.0));
        assert!(r.peak_db < -170.0);
    }

    #[test]
    fn a_song_silent_at_its_end_keeps_half_a_second_after_the_end() {
        // The note stops long before the clip does.
        let dir = pad_song(r#"{"clips": [{"start": 0, "length": 8, "notes": [{"pitch": 57, "start": 0, "length": 1}]}]}"#);
        let (r, _) = dir.render(Length::Song);
        assert_eq!(r.frames as usize, 4 * SR + SR / 2);
    }

    #[test]
    fn a_length_in_seconds_is_exact() {
        let dir = pad_song(r#"{"clips": [{"start": 0, "length": 4, "notes": [{"pitch": 57, "start": 0, "length": 4}]}]}"#);
        let (r, _) = dir.render(Length::Seconds(1.25));
        assert_eq!(r.frames as usize, SR + SR / 4);
        let project = Project::open(&dir.0).unwrap();
        let mut packages = std_packages();
        let compiled = Arc::new(m2_session::compile(&project, &mut packages).unwrap());
        for (from, length) in [(0.0, Length::Seconds(0.0)), (0.0, Length::Seconds(f64::NAN)), (-1.0, Length::Song)] {
            assert!(Render::new(compiled.clone(), &packages, SAMPLE_RATE, from, length, Threads::Exactly(1)).is_err(), "{from} {length:?}");
        }
    }

    #[test]
    fn a_failed_render_leaves_neither_the_file_nor_its_part() {
        let dir = pad_song(r#"{"clips": []}"#);
        let project = Project::open(&dir.0).unwrap();
        let mut packages = std_packages();
        let compiled = Arc::new(m2_session::compile(&project, &mut packages).unwrap());
        let render = Render::new(compiled.clone(), &packages, SAMPLE_RATE, 0.0, Length::Song, Threads::Exactly(1)).unwrap();
        let out = dir.0.join("no-such-folder/out.wav");
        assert!(render.write(&out, |_| {}).is_err());
        assert!(!out.exists());
        let render = Render::new(compiled.clone(), &packages, SAMPLE_RATE, 0.0, Length::Song, Threads::Exactly(1)).unwrap();
        let out = dir.0.join("out.wav");
        let mut reports = vec![];
        render.write(&out, |p| reports.push(p)).unwrap();
        assert_eq!(reports.last(), Some(&1.0));
        assert!(reports.windows(2).all(|w| w[0] <= w[1]), "progress only goes forward: {reports:?}");
        let names: Vec<String> =
            std::fs::read_dir(&dir.0).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert!(!names.iter().any(|n| n.ends_with(".part")), "{names:?}");
    }

    /// A held note: two seconds of sound, then its release.
    const NOTE: &str = r#"{"clips": [{"start": 0, "length": 4, "notes": [{"pitch": 57, "start": 0, "length": 4}]}]}"#;

    fn render_of(dir: &Dir, length: Length) -> Result<Render> {
        let project = Project::open(&dir.0).unwrap();
        let mut packages = std_packages();
        let compiled = Arc::new(m2_session::compile(&project, &mut packages).unwrap());
        Render::new(compiled.clone(), &packages, SAMPLE_RATE, 0.0, length, Threads::Exactly(1))
    }

    fn parts(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).filter(|n| is_part(n)).collect()
    }

    #[test]
    fn a_song_too_long_for_a_wav_file_is_refused_in_plain_words() {
        // A clip typed far out: beat 30000 at 120 bpm is over 4 hours in, past what a WAV holds.
        let dir = pad_song(r#"{"clips": [{"start": 30000, "length": 4, "notes": []}]}"#);
        let e = render_of(&dir, Length::Song).err().unwrap().to_string();
        assert_eq!(e, "its last clip ends in bar 7501, 4 h 10 min in, past the 3 hours music2 can export. Is a clip far past the rest?");
        let e = render_of(&dir, Length::Seconds(MAX_SECONDS + 1.0)).err().unwrap().to_string();
        assert!(e.contains("3 hours is the most"), "{e}");
        // Under the limit, tail and all: fine.
        let dir = pad_song(r#"{"clips": [{"start": 21000, "length": 4, "notes": []}]}"#);
        assert!(render_of(&dir, Length::Song).is_ok());
        assert_eq!((hours(MAX_SECONDS), hours(1500.0), hours(3600.0)), ("3 hours".into(), "25 min".into(), "1 hour".into()));
    }

    #[test]
    fn a_new_file_never_replaces_one_not_even_one_made_while_it_rendered() {
        let dir = pad_song(NOTE);
        let named = |n: u32| dir.0.join(format!("song {n}.wav"));
        std::fs::write(named(1), "mine").unwrap();
        let mut taken = false;
        let (path, r) = render_of(&dir, Length::Song)
            .unwrap()
            .write_new((1..).map(named), |_| {
                // Someone else takes the next free name meanwhile.
                if !taken {
                    std::fs::write(named(2), "theirs").unwrap();
                    taken = true;
                }
            })
            .unwrap();
        assert_eq!(path, named(3));
        assert_eq!(std::fs::read_to_string(named(1)).unwrap(), "mine");
        assert_eq!(std::fs::read_to_string(named(2)).unwrap(), "theirs");
        assert_eq!(hound::WavReader::open(&path).unwrap().duration() as u64, r.frames);
        assert!(parts(&dir.0).is_empty());
    }

    #[test]
    fn a_stopped_render_leaves_nothing_behind() {
        let dir = pad_song(NOTE);
        let (stop, out) = (Arc::new(AtomicBool::new(false)), dir.0.join("out.wav"));
        let flag = stop.clone();
        let e = render_of(&dir, Length::Song)
            .unwrap()
            .stop_when(stop)
            .write(&out, |p| {
                if p > 0.2 {
                    flag.store(true, Ordering::Relaxed);
                }
            })
            .unwrap_err();
        assert!(format!("{e:#}").contains("the render was stopped"), "{e:#}");
        assert!(!out.exists());
        assert!(parts(&dir.0).is_empty());
    }

    #[test]
    fn parts_left_by_renders_that_never_finished_are_swept_but_not_ones_being_written() {
        let dir = pad_song(r#"{"clips": []}"#);
        // Left by a render whose program was killed: nothing holds its lock.
        let stale = dir.0.join(".old.wav.4242.part");
        std::fs::write(&stale, "left behind").unwrap();
        // Another render's, being written.
        let live = dir.0.join(".other.wav.4343.part");
        std::fs::write(&live, "being written").unwrap();
        let held = File::open(&live).unwrap();
        held.lock().unwrap();
        // Not a render's part at all.
        let other = dir.0.join(".notes.part");
        std::fs::write(&other, "").unwrap();
        render_of(&dir, Length::Song).unwrap().write(&dir.0.join("out.wav"), |_| {}).unwrap();
        assert!(!stale.exists());
        assert!(live.exists() && other.exists());
        drop(held);
        assert!(is_part(".a.wav.12.part") && !is_part(".a.wav.part") && !is_part("a.wav.12.part") && !is_part("..12.part"));
    }

    fn compiled(dir: &Dir) -> (Arc<m2_session::Compiled>, PackageSet) {
        let mut packages = std_packages();
        let c = m2_session::compile(&Project::open(&dir.0).unwrap(), &mut packages).unwrap_or_else(|e| panic!("{e:?}"));
        (Arc::new(c), packages)
    }

    // RENDER-4, RENDER-5: a range render starts a preroll before the range, as a live jump does.
    // It gives the same bits every time and says it is approximate; one whose preroll reaches the
    // start is the exact render, and says so.
    #[test]
    fn a_range_render_is_the_same_every_time_and_says_whether_it_is_exact() {
        // A pad held over 8 s, and a note on every beat.
        let beats: Vec<String> = (0..16).map(|b| format!(r#"{{"pitch": 64, "start": {b}, "length": 0.5}}"#)).collect();
        let clips = format!(
            r#"{{"clips": [{{"start": 0, "length": 16, "notes": [{{"pitch": 57, "start": 0, "length": 16}}, {}]}}]}}"#,
            beats.join(", ")
        );
        let dir = pad_song(&clips);
        let (c, packages) = compiled(&dir);
        let second = Length::Seconds(1.0);
        let write = |r: Render, name: &str| {
            let out = dir.0.join(name);
            let rendered = r.write(&out, |_| {}).unwrap();
            (rendered, std::fs::read(out).unwrap())
        };
        let range = |from, preroll| Render::range(c.clone(), &packages, SAMPLE_RATE, from, preroll, second, Threads::Exactly(1)).unwrap();
        let (a, a_bytes) = write(range(4.0, 1.0), "a.wav");
        let (b, b_bytes) = write(range(4.0, 1.0), "b.wav");
        assert_eq!((a.exact, a.reproducible, a.frames), (false, true, SR as u64));
        assert!(a == b && a_bytes == b_bytes, "the same bits every time");
        let exact = || Render::new(c.clone(), &packages, SAMPLE_RATE, 4.0, second, Threads::Exactly(1)).unwrap();
        let (x, x_bytes) = write(exact(), "x.wav");
        assert!(x.exact && x_bytes != a_bytes, "the held pad isn't chased, so it differs from the export");
        // A preroll back to the start is the exact render.
        let (e, e_bytes) = write(range(4.0, 4.0), "e.wav");
        assert!(e.exact && e_bytes == x_bytes);
        assert!(range(4.0, 9.0).exact());
        for preroll in [-1.0, f64::NAN, f64::INFINITY] {
            assert!(Render::range(c.clone(), &packages, SAMPLE_RATE, 4.0, preroll, second, Threads::Exactly(1)).is_err(), "{preroll}");
        }
    }

    // RENDER-5: an approximate render never goes into assets/, however the path spells it.
    #[test]
    fn an_approximate_render_never_goes_into_assets() {
        let dir = pad_song(r#"{"clips": []}"#);
        let p = &dir.0;
        let refused = |out: &str| refuse_in_assets(p, &p.join(out)).is_err();
        // Not made yet: by its name, in any case.
        for out in ["assets/x.wav", "Assets/x.wav", "data/../assets/x.wav", "data/pad/../../assets/new/x.wav"] {
            assert!(refused(out), "{out}");
        }
        std::fs::create_dir_all(p.join("assets/new")).unwrap();
        for out in ["assets/x.wav", "ASSETS/x.wav", "data/../assets/x.wav", "assets/new/x.wav", "assets/later/x.wav"] {
            assert!(refused(out), "{out}");
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(p.join("assets"), p.join("link")).unwrap();
            assert!(refused("link/x.wav"), "a link to it");
        }
        for out in ["assetsx/x.wav", "x.wav", "data/assets/x.wav", "out/assets.wav"] {
            assert!(!refused(out), "{out}");
        }
        assert!(refuse_in_assets(p, &std::env::temp_dir().join("x.wav")).is_ok());
    }

    // WRITE-1, FMT-14, RENDER-5: a render goes into the project's assets/ only exactly and only as
    // a new file; anywhere else it replaces what is there, as an export the user picked does.
    #[test]
    fn a_render_goes_into_assets_only_as_a_new_exact_file() {
        let dir = pad_song(r#"{"clips": []}"#);
        let (c, packages) = compiled(&dir);
        let second = Length::Seconds(0.1);
        let exact = || Render::new(c.clone(), &packages, SAMPLE_RATE, 0.0, second, Threads::Exactly(1)).unwrap();
        std::fs::create_dir_all(dir.0.join("assets")).unwrap();
        let wav = dir.0.join("assets/bounce.wav");
        assert!(exact().write_for(&dir.0, &wav, |_| {}).is_ok() && wav.exists(), "made new");
        let before = std::fs::read(&wav).unwrap();
        let again = exact().write_for(&dir.0, &wav, |_| {}).unwrap_err().to_string();
        assert!(again.contains("is there already") && std::fs::read(&wav).unwrap() == before, "{again}");
        let approximate = Render::range(c.clone(), &packages, SAMPLE_RATE, 4.0, 1.0, second, Threads::Exactly(1)).unwrap();
        let near = dir.0.join("assets/near.wav");
        assert!(approximate.write_for(&dir.0, &near, |_| {}).is_err() && !near.exists(), "never approximate");
        let elsewhere = dir.0.join("out.wav");
        std::fs::write(&elsewhere, "old").unwrap();
        assert!(exact().write_for(&dir.0, &elsewhere, |_| {}).is_ok() && std::fs::read(&elsewhere).unwrap() == before, "replaced");
    }

    /// `examples/first-song`, compiled: five tracks and the song, so a render has partitions to
    /// spread.
    fn first_song() -> (Arc<m2_session::Compiled>, PackageSet) {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song");
        let mut packages = std_packages();
        let c = m2_session::compile(&Project::open(&dir).unwrap(), &mut packages).unwrap_or_else(|e| panic!("{e:?}"));
        (Arc::new(c), packages)
    }

    // RENDER-2 (8a): a render on 4 threads writes the same file as on one, and says how many it
    // used.
    #[test]
    fn a_render_on_4_threads_writes_the_same_file() {
        let (c, packages) = first_song();
        let dir = Dir::new(&[]);
        let write = |threads: usize| {
            let out = dir.0.join(format!("on{threads}.wav"));
            let r = Render::new(c.clone(), &packages, SAMPLE_RATE, 1.0, Length::Seconds(4.0), Threads::Exactly(threads)).unwrap();
            let r = r.write(&out, |_| {}).unwrap();
            (r.threads, std::fs::read(out).unwrap())
        };
        let ((one, a), (four, b)) = (write(1), write(4));
        assert_eq!((one, four), (1, 4));
        assert!(a == b, "the same bytes");
    }

    // 8a: a song that hosts a plugin renders on one thread for now, whatever it is asked, and says
    // so.
    #[test]
    fn a_song_with_plugins_renders_on_one_thread() {
        m2_plugins::test_main_thread();
        m2_plugins::probe::install();
        let dir = Dir::new(&[(
            "song.ts",
            "import { plugin, song } from '@m2/core';\nimport { osc } from 'std/dsp';\n\
             export default song(() => plugin('clap:m2.probe', { in: osc.sine(440) }));\n",
        )]);
        let mut packages = std_packages();
        let c = Arc::new(m2_session::compile(&Project::open(&dir.0).unwrap(), &mut packages).unwrap_or_else(|e| panic!("{e:?}")));
        let r = Render::new(c.clone(), &packages, SAMPLE_RATE, 0.0, Length::Seconds(0.1), Threads::Exactly(4)).unwrap();
        let r = r.write(&dir.0.join("out.wav"), |_| {}).unwrap();
        assert_eq!((r.threads, r.hosts_plugins), (1, true));
    }

    // RENDER-1 (the ex plan's slice 2, D5): a plugin that reports an error while a render plays it
    // stops the render, strict or not (an export isn't strict): the error says `plugin.failed`,
    // naming the plugin and its track, and no file is made. Before, it played silence there and
    // the export said it was done.
    #[test]
    fn a_render_whose_plugin_fails_fails() {
        m2_plugins::test_main_thread();
        m2_plugins::probe::install();
        let state = m2_plugins::probe::state("fails-process-render", 1.0);
        let dir = Dir::new(&[
            (
                "song.ts",
                "import { plugin, song, track } from '@m2/core';\nimport { osc } from 'std/dsp';\n\
                 const p = track('p', () => plugin('clap:m2.probe', { in: osc.sine(440), key: 'lead' }));\n\
                 export default song(() => p.out);\n",
            ),
            ("data/p/lead.state.json", r#"{"plugin": "clap:m2.probe", "plugin_version": "1.0.0"}"#),
        ]);
        std::fs::write(dir.0.join("data/p/lead.state"), state).unwrap();
        let (c, packages) = compiled(&dir);
        let out = dir.0.join("out.wav");
        let e = Render::new(c, &packages, SAMPLE_RATE, 0.0, Length::Seconds(0.1), Threads::Exactly(1))
            .unwrap()
            .write(&out, |_| {})
            .unwrap_err();
        let f = Faulted::of(&e).unwrap_or_else(|| panic!("a fault: {e:#}"));
        assert_eq!(f.code, "plugin.failed");
        assert_eq!(
            format!("{e:#}"),
            "m2 probe (p) stopped working, so the file wasn't made. Try again. If it stops again, the plugin may be broken."
        );
        assert!(!out.exists() && parts(&dir.0).is_empty(), "no file");
    }

    // ab r4, RENDER-1 (the ex plan's slice 2): a file in assets/ that a Sampler plays, there but
    // damaged so it won't decode, stops the render before its first block: the error says
    // `asset.unreadable`, naming the file and what plays it. Before, the Sampler played silence
    // and the export said it was done.
    #[test]
    fn a_sampler_file_that_wont_decode_fails_the_export_naming_it() {
        let dir = Dir::new(&[
            (
                "song.ts",
                "import { song, track } from '@m2/core';\nimport { sampler } from 'std/sampler';\n\
                 const d = track('drums', { name: 'Drums' }, ({ lane }) => sampler(lane.notes('hits'), { zones: { 36: 'kick.wav' } }));\n\
                 export default song(() => d.out);\n",
            ),
            (
                "data/drums/hits.json",
                r#"{"clips": [{"id": "a", "start": 0, "length": 4, "notes": [{"pitch": "C2", "start": 0, "length": 1}]}]}"#,
            ),
            ("assets/kick.wav", "RIFF\u{0}\u{0}\u{0}\u{0}WAVEnot a sound at all"),
            TEMPO,
        ]);
        let (c, packages) = compiled(&dir);
        let e = Render::new(c, &packages, SAMPLE_RATE, 0.0, Length::Song, Threads::Exactly(1))
            .and_then(|r| r.write(&dir.0.join("out.wav"), |_| {}))
            .map(drop)
            .unwrap_err();
        assert_eq!(Faulted::of(&e).map(|f| f.code), Some("asset.unreadable"), "{e:#}");
        assert_eq!(
            format!("{e:#}"),
            "kick.wav, which the instrument on Drums plays, can't be read, so the file wasn't made. It may be damaged."
        );
        assert!(parts(&dir.0).is_empty(), "no file");
    }

    // C3, RENDER-3: a stem is `m2 bounce --track`'s file, bit for bit: each of first-song's four
    // tracks rendered by `Render` through `rooted` and written by `write_float` equals what bounce's
    // own loop writes (`Renderer::new`, `seek_on`, `render_on`, `write_new_wav`).
    #[test]
    fn a_stem_renders_what_m2_bounce_renders() {
        let (c, packages) = first_song();
        let dir = Dir::new(&[]);
        let seconds = bounce_seconds(&c.data);
        assert!((seconds - (32.0 * 60.0 / 124.0 + 2.0)).abs() < 1e-9, "{seconds}");
        for track in ["kick", "hats", "bass", "keys"] {
            let ir = rooted(&c.ir, track, "out").unwrap();
            let flat = m2_ir::check(&ir, &packages).unwrap();
            let stem = dir.0.join(format!("{track}.wav"));
            let parts = Arc::new(Parts { ir, flat: flat.clone(), data: c.data.clone() });
            let r = Render::new(parts, &packages, SAMPLE_RATE, 0.0, Length::Seconds(seconds), Threads::Exactly(2))
                .unwrap()
                .write_float(&stem, |_| {})
                .unwrap();
            assert_eq!(r.frames, (seconds * SAMPLE_RATE as f64).round() as u64);
            // `m2 bounce`'s loop.
            let mut renderer = Renderer::new(&flat, &c.data, &packages, SAMPLE_RATE).unwrap();
            let (mut runner, _pool) = threads_for(&renderer, Threads::Exactly(1));
            renderer.seek_on(0, &mut runner).unwrap();
            let frames = renderer.render_on(seconds, &mut runner).unwrap();
            let bounce = dir.0.join(format!("{track}-bounce.wav"));
            let channels = vec![frames.iter().map(|f| f[0]).collect::<Vec<f32>>(), frames.iter().map(|f| f[1]).collect()];
            m2_runtime::audio::write_new_wav(&bounce, &channels, SAMPLE_RATE).unwrap();
            let (a, b) = (std::fs::read(&stem).unwrap(), std::fs::read(&bounce).unwrap());
            assert!(a == b, "{track}: the stem and the bounce differ ({} and {} bytes)", a.len(), b.len());
            assert!(frames.iter().any(|f| f[0] != 0.0), "{track} makes a sound");
        }
        // Another track's sound isn't in it: the kick's stem isn't the song.
        assert!(rooted(&c.ir, "nobody", "out").unwrap_err().to_string().contains("there's no track `nobody`"));
    }

    // D15: a float render keeps what is over 0 dBFS, as `m2 bounce` does, and is made only as a new file.
    #[test]
    fn a_float_render_is_never_clipped_and_never_replaces_a_file() {
        let dir = Dir::new(&[(
            "song.ts",
            "import { hz, song } from '@m2/core';\nimport { osc } from 'std/dsp';\nimport { scale } from 'std/math';\n\
             export default song(() => scale(osc.sine(hz(440)), 3));\n",
        )]);
        let (c, packages) = compiled(&dir);
        let render = || Render::new(c.clone(), &packages, SAMPLE_RATE, 0.0, Length::Seconds(0.1), Threads::Exactly(1)).unwrap();
        let out = dir.0.join("loud.wav");
        let r = render().write_float(&out, |_| {}).unwrap();
        assert!(r.peak_db > 9.0, "{}", r.peak_db);
        let mut wav = hound::WavReader::open(&out).unwrap();
        assert_eq!((wav.spec().bits_per_sample, wav.spec().sample_format), (32, hound::SampleFormat::Float));
        let peak = wav.samples::<f32>().map(|s| s.unwrap().abs()).fold(0.0f32, f32::max);
        assert!(peak > 2.9, "{peak}");
        let before = std::fs::read(&out).unwrap();
        assert!(render().write_float(&out, |_| {}).is_err());
        assert_eq!(std::fs::read(&out).unwrap(), before);
        assert!(parts(&dir.0).is_empty());
    }

    // 8a: `M2_THREADS` and `--threads` take 1 to 64, and refuse anything else in plain words; the
    // defaults count the fast cores of the last cache alone (one die here, every fast core on a
    // Mac), and the slow ones (one at least) beside playback, 64 at most.
    #[test]
    fn m2_threads_is_read_and_nonsense_refused() {
        assert_eq!(Threads::parse("4").unwrap(), Threads::Exactly(4));
        assert_eq!(Threads::parse(" 64\n").unwrap(), Threads::Exactly(64));
        for bad in ["0", "65", "x", "", "-1", "2.5"] {
            let e = Threads::parse(bad).unwrap_err().to_string();
            assert!(e.contains("a number from 1 to 64"), "{bad}: {e}");
        }
        let server = Cores { fast: 16, slow: 0, per_cache: 8, per_last_cache: 8 };
        let mac = Cores { fast: 8, slow: 2, per_cache: 4, per_last_cache: 8 };
        assert_eq!([Threads::Alone, Threads::Beside, Threads::Exactly(3)].map(|t| t.count(&server)), [8, 1, 3]);
        assert_eq!([Threads::Alone, Threads::Beside].map(|t| t.count(&mac)), [8, 2], "every fast core on a Mac");
        // More cores behind one cache than a pool has threads: as many as it has, never one.
        let big = Cores { fast: 96, slow: 80, per_cache: 72, per_last_cache: 96 };
        assert_eq!([Threads::Alone, Threads::Beside].map(|t| t.count(&big)), [64, 64]);
    }
}
