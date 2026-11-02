//! Audio lanes: clips at the song's sample rate (resampling, varispeed, warping, reverse, gain,
//! fades), played sample-exactly from the disk (RENDER-30, [`stream`]): playback is a copy from a
//! ring the disk thread fills, and seeking is exact. Opened files, warped renders and whole lanes
//! are kept in a [`Cache`] by content, so a live rebuild shares what it didn't change, its ring
//! included (RENDER-27).
//!
//! Live, a lane crossfades over 5 ms from its old position to its new one at a seek or a loop
//! wrap, fades out on stop, and fades in on play from inside a clip (RENDER-9); offline it is a
//! plain copy. Its player persists across plan activations, and one that brings new renders (an
//! edited clip, a tempo change) crossfades linearly from the old renders to them (RENDER-27).
//!
//! A warped clip is stretched by std's offline kernel `std:stretch` ([`crate::offline`]), once, in
//! pieces, straight into a file in the cache folder, which then streams as any clip; its source is
//! read from the disk. When the folder can't take it (none, a full disk, no permission), it is made
//! in memory instead (review sx-r1). Gain, fades and reverse are the host's, worked out as the file is read, as
//! for a plain clip (the sx plan's D7): clips that differ only in them share one stretch.
//!
//! Its math goes through the `libm` crate, never the OS's library, so a clip renders the same bits
//! on every OS (PLAT-4; `tests/host_math.rs` keeps it so).

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex, Weak};

use anyhow::{Context, Result};
use m2_song::{AudioClip, DataSet, HashCache, Sha, TempoMap};

use crate::BLOCK;
use crate::fade::xfade;
use crate::fault::{Code, Failed, Reporter, Subject};
use crate::offline::Kernel;
use crate::renders::{self, Stretcher};
use crate::sandbox::Held;
use crate::stream::{self, Clip, Making, Raw, Ring, Shape, Source, Wav};
use crate::transport::{Jump, Runs};

/// Decoded audio, planar, at its own sample rate.
#[derive(Clone, Debug)]
pub struct Asset {
    pub channels: Vec<Vec<f32>>,
    pub sample_rate: f64,
}

impl Asset {
    pub fn len(&self) -> usize {
        self.channels.first().map_or(0, Vec::len)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn seconds(&self) -> f64 {
        self.len() as f64 / self.sample_rate
    }
}

/// Write planar audio as a 32-bit float WAV (lossless for anything the engine makes).
pub fn write_wav(path: &Path, channels: &[Vec<f32>], sample_rate: u32) -> Result<()> {
    let file = std::fs::File::create(path).with_context(|| format!("creating {}", path.display()))?;
    wav_to(&file, channels, sample_rate)
}

/// [`write_wav`] to a file that must not exist yet: assets are never overwritten, so a new one
/// is created with create-only semantics (FMT-14). A failed write leaves nothing behind. Its
/// bytes are pushed and its folder synced before it returns, so a Tx that names it makes them
/// durable by its barrier at the latest (WRITE-1).
pub fn write_new_wav(path: &Path, channels: &[Vec<f32>], sample_rate: u32) -> Result<()> {
    let file =
        std::fs::OpenOptions::new().write(true).create_new(true).open(path).with_context(|| format!("creating {}", path.display()))?;
    let folder = m2_os::fs::folder_of(path);
    wav_to(&file, channels, sample_rate)
        .and_then(|()| m2_os::fs::push(&file).with_context(|| format!("writing {}", path.display())))
        .and_then(|()| m2_os::fs::sync_dir(folder).with_context(|| format!("syncing {}", folder.display())))
        .inspect_err(|_| drop(std::fs::remove_file(path)))
}

fn wav_to(file: &std::fs::File, channels: &[Vec<f32>], sample_rate: u32) -> Result<()> {
    let spec =
        hound::WavSpec { channels: channels.len() as u16, sample_rate, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let mut w = hound::WavWriter::new(std::io::BufWriter::new(file), spec)?;
    for i in 0..channels.first().map_or(0, Vec::len) {
        for c in channels {
            w.write_sample(c[i])?;
        }
    }
    w.finalize()?;
    Ok(())
}

/// A live sound input the engine hears on an audio lane (MON-8, RENDER-30a): its frames at the
/// engine's rate, numbered as they came, written by the input's callback. Read on the audio thread:
/// neither call allocates, locks or waits.
pub trait LiveInput: Send + Sync {
    /// Frames written so far, and the last write's length (the input's buffer).
    fn head(&self) -> (u64, u32);
    /// Frames from number `at` into `out`, those written: how many. A frame gone is silence.
    fn read(&self, at: u64, out: &mut [[f32; 2]]) -> usize;
}

/// An input heard on a lane: where its reader is.
struct Hearing {
    input: Arc<dyn LiveInput>,
    at: Option<u64>,
}

impl Hearing {
    /// Add this block of the input to `out` (channel-major, `channels × BLOCK`): about one input
    /// buffer and one block behind the input, so a late callback still finds its frames. A reader
    /// that falls further behind (the input's clock is faster) or ahead jumps back to that lag;
    /// frames not there yet are silence.
    fn add(&mut self, out: &mut [f32], channels: usize) {
        let (head, buffer) = self.input.head();
        let lag = buffer as u64 + BLOCK as u64;
        let at = match self.at {
            Some(a) if a <= head && head - a <= lag + 4 * (buffer as u64 + BLOCK as u64) => a,
            _ => head.saturating_sub(lag),
        };
        let mut block = [[0.0f32; 2]; BLOCK];
        let n = self.input.read(at, &mut block);
        self.at = Some(at + n as u64);
        for (f, x) in block.iter().enumerate() {
            match channels {
                1 => out[f] += 0.5 * (x[0] + x[1]),
                _ => {
                    out[f] += x[0];
                    out[BLOCK + f] += x[1];
                }
            }
        }
    }
}

/// A clip rendered at the song's rate: one buffer per channel, from the clip's start.
pub type RenderedClip = Vec<Vec<f32>>;

/// What a clip's render depends on: the asset's bytes, every field that shapes the sound, the
/// tempo map and the rate; and for a warped clip, the module of the kernel that stretches it (D8).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ClipKey {
    asset: Sha,
    clip: Vec<u64>,
    tempo: Vec<u64>,
    rate: u64,
    stretch: Option<Sha>,
}

impl ClipKey {
    fn of(asset: Sha, c: &AudioClip, tempo: &TempoMap, rate: f64, stretch: Option<&Kernel>) -> ClipKey {
        let mut clip = vec![c.start, c.length, c.offset, c.gain_db, c.fade_in, c.fade_out, c.pitch, c.reverse as u8 as f64];
        clip.extend(c.warp.iter().flat_map(|&(a, b)| [a, b]));
        let tempo = tempo.segments().iter().flat_map(|t| [t.beat, t.bpm]).map(f64::to_bits).collect();
        let stretch = stretch.filter(|_| !c.warp.is_empty()).map(|k| k.sha);
        ClipKey { asset, clip: clip.into_iter().map(f64::to_bits).collect(), tempo, rate: rate.to_bits(), stretch }
    }
}

/// What a warped clip's stretch depends on: everything its kernel is given (D8). The source's
/// bytes, the kernel's module, the time map, the pitch, the rate and the length; never the clip's
/// gain, fades or reverse, which are worked out as it is read.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct WarpKey {
    asset: Sha,
    stretch: Sha,
    numbers: Vec<u64>,
}

impl WarpKey {
    fn of(asset: Sha, stretch: Sha, map: &[[f64; 2]], pitch: f64, rate: f64, frames: usize) -> WarpKey {
        let mut numbers = vec![pitch.to_bits(), rate.to_bits(), frames as u64, map.len() as u64];
        numbers.extend(map.iter().flatten().map(|v| v.to_bits()));
        WarpKey { asset, stretch, numbers }
    }

    /// Its bytes, for a file name.
    fn bytes(&self) -> Vec<u8> {
        let mut b = format!("{} {}\n", self.asset.hex(), self.stretch.hex()).into_bytes();
        for n in &self.numbers {
            b.extend_from_slice(&n.to_le_bytes());
        }
        b
    }
}

/// What a lane plays: its clips' keys in order, its channels.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct LaneKey {
    clips: Vec<ClipKey>,
    channels: usize,
}

/// Opened assets, by content; warped clips' stretches, by what the kernel is given; lanes, by what
/// they play; and the files kernels read, decoded whole, by content. Building opens files and reads
/// their headers, fills the lanes short enough to stay whole, asks for warped clips' stretches and
/// decodes kernels' files; compiling never does (FMT-16).
///
/// A live session keeps a [`Cache::live`] one per open project, so an edit shares every lane it
/// didn't change with the renderer it replaces, and its ring with it; it never waits for a stretch:
/// its stretch thread makes each new one in the background, and lanes play it as it is made
/// ([`crate::renders`], the sx plan's D10). An offline render uses one of its own, the
/// [`Default`] kind, so its rings are its own too, and it makes each stretch whole as it builds:
/// its bits never depend on timing.
#[derive(Default)]
pub struct Cache {
    wavs: HashMap<Sha, Found<Arc<Wav>>>,
    warps: HashMap<WarpKey, Arc<Raw>>,
    lanes: HashMap<LaneKey, Arc<AudioLane>>,
    /// The files kernels read (kernel ABI v1), decoded whole, kept while a build uses them.
    kernels: HashMap<Sha, Found<Arc<Asset>>>,
    used_wavs: HashSet<Sha>,
    used_kernels: HashSet<Sha>,
    used_warps: HashSet<WarpKey>,
    used_lanes: HashSet<LaneKey>,
    decodes: usize,
    renders: usize,
    folder: Renders,
    /// A live cache's stretch thread; none offline.
    live: Option<Stretcher>,
    /// The ring the plans built with it report faults to (RENDER-25): a live session's is the
    /// app's; an offline cache has its own.
    faults: Arc<crate::fault::Faults>,
}

/// Where stretches are kept: `<cache>/renders` (FMT-16's folder); none, in memory.
struct Renders(Option<std::path::PathBuf>);

impl Default for Renders {
    fn default() -> Renders {
        Renders(m2_os::dirs::cache().map(|c| c.join("renders")))
    }
}

/// What a file that may be played gave: its audio, none (it holds no audio), or an error (it is
/// there, but can't be read or decoded: `asset.unreadable`; or a clip's isn't a WAV, FMT-24).
#[derive(Clone)]
pub(crate) enum Found<T> {
    Plays(T),
    Empty,
    Unreadable,
    NotWav,
    TooLong,
}

impl<T> Found<T> {
    fn of(read: Result<T>, empty: impl FnOnce(&T) -> bool) -> Found<T> {
        match read {
            Ok(t) if empty(&t) => Found::Empty,
            Ok(t) => Found::Plays(t),
            Err(e) if e.downcast_ref::<stream::NotWav>().is_some() => Found::NotWav,
            Err(e) if e.downcast_ref::<crate::decode::TooLong>().is_some() => Found::TooLong,
            Err(_) => Found::Unreadable,
        }
    }

    fn map<U>(self, f: impl FnOnce(T) -> U) -> Found<U> {
        match self {
            Found::Plays(t) => Found::Plays(f(t)),
            Found::Empty => Found::Empty,
            Found::Unreadable => Found::Unreadable,
            Found::NotWav => Found::NotWav,
            Found::TooLong => Found::TooLong,
        }
    }
}

impl Cache {
    /// The opened file at `path` and its hash: it may be played. The hash is a stamp lookup:
    /// validating the data already hashed every asset.
    fn wav(&mut self, path: &Path) -> Found<(Sha, Arc<Wav>)> {
        let Ok(sha) = HashCache::global().hash(path) else { return Found::Unreadable };
        self.used_wavs.insert(sha);
        let decodes = &mut self.decodes;
        let w = self.wavs.entry(sha).or_insert_with(|| {
            *decodes += 1;
            Found::of(Wav::open(path), |w| w.frames() == 0).map(Arc::new)
        });
        w.clone().map(|w| (sha, w))
    }

    /// The asset named `name` that a kernel reads (kernel ABI v1), decoded whole at its own rate,
    /// once while builds use it: a live edit shares it with the renderer it replaces. `None` when
    /// it may not be played (`asset.missing`, which validation finds), so the kernel reads silence.
    pub(crate) fn kernel_asset(&mut self, data: &DataSet, name: &str) -> Option<Found<(Sha, Arc<Asset>)>> {
        let path = data.asset_path(name)?;
        let Ok(sha) = HashCache::global().hash(&path) else { return Some(Found::Unreadable) };
        self.used_kernels.insert(sha);
        let decodes = &mut self.decodes;
        let a = self.kernels.entry(sha).or_insert_with(|| {
            *decodes += 1;
            Found::of(crate::decode::load(&path), Asset::is_empty).map(Arc::new)
        });
        Some(a.clone().map(|a| (sha, a)))
    }

    /// A live session's cache: warped clips' stretches are made in the background, and their lanes
    /// play them as they are made. The plans built with it report faults to `faults`, the app's
    /// ring.
    pub fn live(faults: Arc<crate::fault::Faults>) -> Cache {
        Cache { live: Some(Stretcher::default()), faults, ..Cache::default() }
    }

    /// The ring the plans built with it report faults to.
    pub fn faults(&self) -> &Arc<crate::fault::Faults> {
        &self.faults
    }

    /// Whether it is a live session's: its ring is the app's, which the session reads.
    pub fn is_live(&self) -> bool {
        self.live.is_some()
    }

    /// An offline cache whose warped renders are kept in `folder` (tests); none, in memory.
    #[doc(hidden)]
    pub fn with_renders_in(folder: Option<std::path::PathBuf>) -> Cache {
        Cache { folder: Renders(folder), ..Cache::default() }
    }

    /// A live cache whose warped renders are kept in `folder` (tests).
    #[doc(hidden)]
    pub fn live_in(folder: std::path::PathBuf) -> Cache {
        Cache { folder: Renders(Some(folder)), ..Cache::live(Arc::default()) }
    }

    /// A test seam: a live cache's stretches each make at most `frames` frames, then wait until
    /// this changes; none, all they need.
    #[doc(hidden)]
    pub fn hold_renders(&self, frames: Option<usize>) {
        if let Some(s) = &self.live {
            s.hold(frames);
        }
    }

    /// Wait until a live cache's stretches are made, or wait at a hold, for `cap` at most (tests).
    /// True when so.
    #[doc(hidden)]
    pub fn settle_renders(&self, cap: std::time::Duration) -> bool {
        self.live.as_ref().is_none_or(|s| s.settle(cap))
    }

    /// Warped clip `c`'s stretch, made by `stretch` from its file `wav`, the asset `asset` (D7),
    /// and its key: from the cache folder when it is there, else asked for now. A live cache's
    /// stretch thread makes it in the background, and lanes play it as it is made (D10); one that
    /// fails is silent, and each lane that plays it reports `stretch.failed`. An offline cache makes
    /// it here, with the same maker ([`renders::make_now`]); one that fails fails the build with
    /// that fault, `at` naming the clip (RENDER-1). Either makes it into its file in the folder, or
    /// in memory when there is no folder or it can't take it (review sx-r1). None without a
    /// stretch kernel.
    #[allow(clippy::too_many_arguments)]
    fn warp(
        &mut self,
        wav: &Arc<Wav>,
        asset: Sha,
        c: &AudioClip,
        tempo: &TempoMap,
        rate: f64,
        stretch: Option<&Kernel>,
        at: (&Tell, u32),
    ) -> Result<Option<(WarpKey, Arc<Raw>)>> {
        let Some(k) = stretch else { return Ok(None) };
        let map = time_map(c, tempo);
        let frames = stream::clip_len(c, tempo, rate);
        let key = WarpKey::of(asset, k.sha, &map, c.pitch, rate, frames);
        self.used_warps.insert(key.clone());
        if let Some(s) = self.warps.get(&key) {
            return Ok(Some((key, s.clone())));
        }
        let path = self.folder.0.as_deref().map(|f| stream::render_path(f, &key.bytes()));
        let ch = wav.channels();
        let raw = match path.as_deref().and_then(|p| Raw::open(p, ch, frames)) {
            Some(whole) => Arc::new(whole),
            None => {
                self.renders += 1;
                let out = Making::new(path.as_deref(), ch, frames);
                let raw = out.raw().clone();
                let source = Held::Disk(wav.clone());
                let job = renders::Job { kernel: k.clone(), source, values: vec![c.pitch], map, rate, out };
                match &mut self.live {
                    Some(live) => live.push(job),
                    None => renders::make_now(job).map_err(|e| {
                        let why = e.context(format!("the warped clip of {} at beat {} can't be stretched", c.asset, c.start));
                        Failed::new(at.0.fault(Code::StretchFailed, at.1), Some(why))
                    })?,
                }
                raw
            }
        };
        self.warps.insert(key.clone(), raw.clone());
        Ok(Some((key, raw)))
    }

    /// Keep only what the builds since the last call used: an edited lane's old ring goes once
    /// the renderer that played it is built over.
    pub fn prune(&mut self) {
        let wavs = std::mem::take(&mut self.used_wavs);
        let warps = std::mem::take(&mut self.used_warps);
        let lanes = std::mem::take(&mut self.used_lanes);
        let kernels = std::mem::take(&mut self.used_kernels);
        self.wavs.retain(|k, _| wavs.contains(k));
        self.kernels.retain(|k, _| kernels.contains(k));
        // A stretch no build asked for any more stops, and its part file goes (D10).
        self.warps.retain(|k, r| {
            let keep = warps.contains(k);
            if !keep {
                r.stop();
            }
            keep
        });
        if let Some(s) = &self.live {
            s.forget_stopped();
        }
        self.lanes.retain(|k, _| lanes.contains(k));
    }

    /// Assets opened (kernels' files decoded included) and warped clips rendered (or asked for, live)
    /// since the cache was made.
    pub fn work(&self) -> (usize, usize) {
        (self.decodes, self.renders)
    }

    /// Every lane it holds, weakly: tests check that what no plan plays any more is freed.
    pub fn lanes(&self) -> Vec<Weak<AudioLane>> {
        self.lanes.values().map(Arc::downgrade).collect()
    }
}

/// The assets a graph's kernels read, by name, each looked up once per build: its hash, which is
/// part of its kernel's shape (RENDER-25), and its decoded audio, which its sandbox shares
/// (`m2.asset_read`). `None` for one that is missing or can't be decoded: it reads silence.
#[derive(Default)]
pub(crate) struct KernelAssets(HashMap<String, Option<(Sha, Arc<Asset>)>>);

impl KernelAssets {
    /// Every asset a kernel of `flat` names, voices' kernels included, through `cache`. One that is
    /// there but can't be read or decoded is reported through `report`, once per build, as the
    /// first node that reads it (`asset.unreadable`): it reads silence.
    pub(crate) fn of(flat: &m2_ir::Flat, data: &DataSet, cache: &mut Cache, report: &Reporter) -> KernelAssets {
        let mut out = HashMap::new();
        for (i, n) in flat.nodes.iter().enumerate() {
            for (slot, a) in assets_of(n).into_iter().enumerate() {
                if out.contains_key(a) {
                    continue;
                }
                let found = match cache.kernel_asset(data, a) {
                    Some(Found::Plays(x)) => Some(x),
                    Some(f @ (Found::Unreadable | Found::NotWav | Found::TooLong)) => {
                        let code = if matches!(f, Found::TooLong) { Code::TooLong } else { Code::Unreadable };
                        report.report(code, Subject::Asset { node: i as u32, slot: slot as u32 });
                        None
                    }
                    Some(Found::Empty) | None => None,
                };
                out.insert(a.to_string(), found);
            }
        }
        KernelAssets(out)
    }

    /// The hash of the asset named `name`, when it plays.
    pub(crate) fn sha(&self, name: &str) -> Option<Sha> {
        self.0.get(name)?.as_ref().map(|(sha, _)| *sha)
    }

    /// Its decoded audio, when it plays.
    pub(crate) fn get(&self, name: &str) -> Option<Arc<Asset>> {
        self.0.get(name)?.as_ref().map(|(_, a)| a.clone())
    }
}

/// The names of the files top-level node `n`'s kernels read, in order: a kernel's own, a `poly`'s
/// voice's kernels' (kernel ABI v1). A fault names one by its place here ([`Subject::Asset`]).
pub fn assets_of(n: &m2_ir::FNode) -> Vec<&str> {
    fn own(n: &m2_ir::FNode) -> Vec<&str> {
        match &n.op {
            m2_ir::FOp::Kernel { assets, .. } => assets.iter().map(String::as_str).collect(),
            _ => vec![],
        }
    }
    match &n.op {
        m2_ir::FOp::Poly { voice, .. } => voice.nodes.iter().flat_map(own).collect(),
        _ => own(n),
    }
}

/// Where a lane reports its faults (RENDER-25): its plan's reporter, and its place among the
/// plan's lanes ([`crate::lane_slots`]).
#[derive(Clone, Debug)]
pub(crate) struct Tell {
    pub(crate) report: Reporter,
    pub(crate) lane: u32,
}

impl Tell {
    fn fault(&self, code: Code, clip: u32) -> crate::fault::Fault {
        self.report.fault(code, Subject::Clip { lane: self.lane, clip })
    }
}

/// What a lane told of its faults: the plan it tells them to now, what it told that plan, and every
/// fault it met. A lane is shared by the plans that play it (RENDER-27): each new one is told again
/// what the lane met, as a chunk that failed holds silence until it is worked out again.
#[derive(Default)]
struct Told {
    tell: Option<Tell>,
    said: Vec<(Code, u32)>,
    met: Vec<(Code, u32)>,
}

/// Read `x` at fractional position `pos` (cubic Hermite); silence outside.
#[cfg(test)]
fn read(x: &[f32], pos: f64) -> f32 {
    stream::read_in(x, 0, x.len(), pos)
}

/// A warped clip's time map (D5), for `std:stretch`: pairs of (source seconds, output seconds from
/// the clip's start) at its start, its end, and each warp marker and tempo change inside it. Source
/// time runs in a straight line between markers in beats, and output time between tempo changes,
/// so between two pairs both do, and the map is exact. Past the markers, their nearest segment goes
/// on, in beats; one marker pins a moment, and the audio runs at its own speed around it.
pub(crate) fn time_map(c: &AudioClip, tempo: &TempoMap) -> Vec<[f64; 2]> {
    let t0 = tempo.seconds_at(c.start);
    let w = &c.warp;
    let src_at_beat = |b: f64| -> f64 {
        if w.len() == 1 {
            return w[0].0 + tempo.seconds_at(c.start + b) - tempo.seconds_at(c.start + w[0].1);
        }
        let i = w.partition_point(|m| m.1 <= b).clamp(1, w.len() - 1);
        let (a, z) = (w[i - 1], w[i]);
        a.0 + (b - a.1) * (z.0 - a.0) / (z.1 - a.1)
    };
    let inside = |b: &f64| *b > 0.0 && *b < c.length;
    let mut beats = vec![0.0, c.length.max(0.0)];
    beats.extend(w.iter().map(|m| m.1).filter(inside));
    beats.extend(tempo.segments().iter().map(|t| t.beat - c.start).filter(inside));
    beats.sort_by(f64::total_cmp);
    beats.dedup();
    beats.into_iter().map(|b| [src_at_beat(b), tempo.seconds_at(c.start + b) - t0]).collect()
}

/// What `k` makes as `run` says, whole in memory (tests' reference).
#[cfg(test)]
fn whole(k: &Kernel, run: &crate::offline::Run) -> Result<RenderedClip> {
    let mut out: RenderedClip = vec![Vec::with_capacity(run.frames); run.channels];
    crate::offline::run(k, run, &mut |b| {
        for (o, x) in out.iter_mut().zip(b) {
            o.extend_from_slice(x);
        }
        Ok(())
    })?;
    Ok(out)
}

/// Render one clip at `sample_rate`: its samples from the clip's start, for its length, worked out
/// whole. A clip with no warp is worked out the same way piece by piece from its file as it plays
/// ([`stream`]); a warped one is stretched whole by `stretch` (silent without it), then reversed,
/// faded and gained as its file is when read. This is the reference their pieces must equal (tests).
#[cfg(test)]
pub(crate) fn render_clip(
    clip: &AudioClip,
    asset: &Arc<Asset>,
    tempo: &TempoMap,
    sample_rate: f64,
    stretch: Option<&Kernel>,
) -> Result<RenderedClip> {
    let sh = Shape::of(clip, asset.sample_rate, tempo, sample_rate);
    let mut out: RenderedClip = if clip.warp.is_empty() {
        // Varispeed: source time runs `rate` times faster.
        asset.channels.iter().map(|ch| (0..sh.len).map(|i| read(ch, sh.offset + i as f64 * sh.step)).collect()).collect()
    } else {
        let map = time_map(clip, tempo);
        let channels = asset.channels.len();
        let run = crate::offline::Run {
            source: Held::Whole(asset.clone()),
            values: &[clip.pitch],
            map: &map,
            rate: sample_rate,
            channels,
            frames: sh.len,
        };
        match stretch {
            Some(k) => whole(k, &run)?,
            None => vec![vec![0.0; sh.len]; channels],
        }
    };
    for ch in &mut out {
        if clip.reverse {
            ch.reverse();
        }
        let n = ch.len();
        for (i, x) in ch.iter_mut().enumerate() {
            *x *= stream::gain_at(i, n, sh.gain, sh.fade_in, sh.fade_out);
        }
    }
    Ok(out)
}

/// An audio lane, ready to play: its clips at their start samples, and the ring the sound thread
/// copies them from ([`stream`]).
pub struct AudioLane {
    /// What it plays, for [`AudioLane::same`]: none for a lane made from renders.
    key: Option<LaneKey>,
    /// Its warped clips' stretches: a build that keeps it keeps them too.
    warps: Vec<WarpKey>,
    pub(crate) clips: Vec<Clip>,
    pub(crate) channels: usize,
    pub(crate) ring: Ring,
    pub(crate) tempo: TempoMap,
    pub(crate) rate: f64,
    /// Where it reports its faults, and what it told: never locked on the sound thread.
    told: Mutex<Told>,
}

impl std::fmt::Debug for AudioLane {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("AudioLane").field("clips", &self.clips.len()).field("channels", &self.channels).finish()
    }
}

impl AudioLane {
    /// Whether it plays what `o` plays, at the same samples: what a plan keeps.
    pub(crate) fn same(&self, o: &AudioLane) -> bool {
        std::ptr::eq(self, o) || (self.key.is_some() && self.key == o.key)
    }

    /// The lane for `clips`, in the order given (canonical: by start and id, so overlapping clips
    /// always sum in one order), from `cache` when it holds it. A clip whose asset may not be
    /// played (`asset.missing`) or holds no audio is silent; it never silences the rest (FMT-15).
    /// One whose file is there but can't be read or decoded is silent too, and reported through
    /// `tell` (`asset.unreadable`), as the lane reports what it can't read later. A lane short
    /// enough to stay whole is filled here. Warped clips are stretched by `stretch` (the song's
    /// `std:stretch`); without it they are silent. An error only offline: a stretch that failed
    /// ([`Cache::warp`]).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn build(
        clips: &[AudioClip],
        data: &DataSet,
        cache: &mut Cache,
        tempo: &TempoMap,
        sample_rate: f64,
        channels: usize,
        stretch: Option<&Kernel>,
        tell: &Tell,
    ) -> Result<Arc<AudioLane>> {
        AudioLane::make(clips, &|a| data.asset_path(a), cache, tempo, sample_rate, channels, stretch, tell)
    }

    /// [`AudioLane::build`] with each asset's path from `asset_path`, its faults reported to
    /// `cache`'s ring as lane 0 of plan 0 (tests).
    #[cfg(test)]
    pub(crate) fn build_with(
        clips: &[AudioClip],
        asset_path: &dyn Fn(&str) -> Option<std::path::PathBuf>,
        cache: &mut Cache,
        tempo: &TempoMap,
        sample_rate: f64,
        channels: usize,
        stretch: Option<&Kernel>,
    ) -> Result<Arc<AudioLane>> {
        let tell = Tell { report: Reporter::new(cache.faults().clone(), 0), lane: 0 };
        AudioLane::make(clips, asset_path, cache, tempo, sample_rate, channels, stretch, &tell)
    }

    #[allow(clippy::too_many_arguments)]
    fn make(
        clips: &[AudioClip],
        asset_path: &dyn Fn(&str) -> Option<std::path::PathBuf>,
        cache: &mut Cache,
        tempo: &TempoMap,
        sample_rate: f64,
        channels: usize,
        stretch: Option<&Kernel>,
        tell: &Tell,
    ) -> Result<Arc<AudioLane>> {
        let mut found = vec![];
        for (i, c) in clips.iter().enumerate() {
            let Some(path) = asset_path(&c.asset) else { continue };
            match cache.wav(&path) {
                Found::Plays((sha, w)) => found.push((i as u32, c, ClipKey::of(sha, c, tempo, sample_rate, stretch), w)),
                Found::Empty => {}
                Found::Unreadable => tell.report.faults().report(tell.fault(Code::Unreadable, i as u32)),
                Found::NotWav => tell.report.faults().report(tell.fault(Code::NotWav, i as u32)),
                // Never from a clip's file, which streams: no whole decode.
                Found::TooLong => tell.report.faults().report(tell.fault(Code::Unreadable, i as u32)),
            }
        }
        let key = LaneKey { clips: found.iter().map(|f| f.2.clone()).collect(), channels };
        cache.used_lanes.insert(key.clone());
        if let Some(l) = cache.lanes.get(&key) {
            cache.used_warps.extend(l.warps.iter().cloned());
            l.tell(tell.clone());
            return Ok(l.clone());
        }
        let (mut out, mut warps) = (vec![], vec![]);
        for (i, c, k, w) in found {
            let at = (tempo.seconds_at(c.start) * sample_rate).round() as u64;
            let sh = Shape::of(c, w.rate, tempo, sample_rate);
            let src = if c.warp.is_empty() {
                Source::Wav(w.clone(), sh)
            } else {
                let Some((key, raw)) = cache.warp(&w, k.asset, c, tempo, sample_rate, stretch, (tell, i))? else { continue };
                warps.push(key);
                Source::Render(raw, sh)
            };
            out.push(Clip::new(at, src, stream::clip_len(c, tempo, sample_rate), i));
        }
        let mut lane = AudioLane::of(out, channels, tempo.clone(), sample_rate, Some(key.clone()), Some(tell.clone()));
        lane.warps = warps;
        let lane = Arc::new(lane);
        cache.lanes.insert(key, lane.clone());
        Ok(lane)
    }

    fn of(clips: Vec<Clip>, channels: usize, tempo: TempoMap, rate: f64, key: Option<LaneKey>, tell: Option<Tell>) -> AudioLane {
        let ring = Ring::new(&clips, channels, rate);
        let told = Mutex::new(Told { tell, ..Told::default() });
        let lane = AudioLane { key, warps: vec![], clips, channels, ring, tempo, rate, told };
        lane.fill_all();
        lane
    }

    /// Report its faults to `tell` from now on: a new plan plays it. What it met before is told
    /// to that plan at once.
    fn tell(&self, tell: Tell) {
        let mut t = self.told.lock().unwrap_or_else(|e| e.into_inner());
        for &(code, clip) in &t.met {
            tell.report.faults().report(tell.fault(code, clip));
        }
        t.said = t.met.clone();
        t.tell = Some(tell);
    }

    /// `code` happened to clip `clip` (its place in the song's data): reported once to the plan
    /// it tells. Off the sound thread only: it locks.
    pub(crate) fn fault(&self, code: Code, clip: u32) {
        let mut t = self.told.lock().unwrap_or_else(|e| e.into_inner());
        if !t.met.contains(&(code, clip)) {
            t.met.push((code, clip));
        }
        if t.said.contains(&(code, clip)) {
            return;
        }
        if let Some(tell) = &t.tell {
            tell.report.faults().report(tell.fault(code, clip));
        }
        t.said.push((code, clip));
    }

    /// A lane of renders in memory at their start frames, in a song at `rate` with no tempo
    /// change (tests).
    #[doc(hidden)]
    pub fn of_renders(clips: Vec<(u64, Arc<RenderedClip>)>, channels: usize, rate: f64) -> AudioLane {
        let clips = clips.into_iter().map(|(at, r)| {
            let len = r[0].len();
            let sh = Shape { offset: 0.0, step: 1.0, len, reverse: false, gain: 1.0, fade_in: 0, fade_out: 0 };
            Clip::new(at, Source::Mem(r, sh), len, 0)
        });
        AudioLane::of(clips.collect(), channels, TempoMap::constant(120.0), rate, None, None)
    }

    /// Whether transport frame `pos` is strictly inside a clip: playing from there starts
    /// mid-sound.
    fn inside(&self, pos: u64) -> bool {
        self.clips.iter().any(|c| c.at < pos && pos < c.at + c.len)
    }
}

/// The most places one block of a player reads: a fade's start, each fade, and its runs.
const NEED: usize = 1 + TAPS + crate::transport::MAX_RUNS;

/// Fill what `lane` doesn't hold of the frames `at` names, on this thread, never taking the slot of
/// a chunk another of them reads.
fn ensure(lane: &AudioLane, at: &[(u64, u64)]) {
    let c = lane.chunk();
    let mut block = [0u64; 2 * NEED];
    let mut m = 0;
    for &(a, len) in at {
        for k in a / c..(a + len).div_ceil(c) {
            if m < block.len() && !block[..m].contains(&k) {
                block[m] = k;
                m += 1;
            }
        }
    }
    for &(a, len) in at {
        lane.ensure(a, len, &block[..m]);
    }
}

/// A position fading out: the lane's material from `pos` on, from gain `g0` to silence.
#[derive(Clone, Copy, Debug, Default)]
struct Tap {
    pos: u64,
    g0: f32,
    done: u32,
}

/// The highest project rate (`m2_song`'s manifest takes 8 to 384 kHz), whose 5 ms fade is the
/// longest: 1920 frames, 15 blocks.
const MAX_RATE: u64 = 384_000;

/// Taps that can run at once at any project rate. A block starts at most two (a seek or a stop on
/// its line, and a wrap mid-block: loops are at least two blocks long), and each lasts one fade:
/// so two for each block a fade spans, one more for a wrap in the block before those, and the new
/// one. A seek on every block line, each with a wrap just after, runs 30 of them at 384 kHz.
const TAPS: usize = 2 * (MAX_RATE as usize * 5 / 1000).div_ceil(BLOCK) + 2;

/// An audio lane as the engine plays it: its clips' renders and its declicking, filled once per
/// block before the graph runs; every node that reads the lane copies [`AudioPlayer::out`]. Two
/// nodes on one lane computed the same output from the same runs, so one player gives their bits.
/// It moves from plan to plan with its lane; an activation that brings new renders swaps them in,
/// and for one fade window the old ones play on beside them (RENDER-27).
pub(crate) struct AudioPlayer {
    cur: Arc<AudioLane>,
    /// The renders an activation replaced: read for one window, then retired with the window.
    prev: Option<Arc<AudioLane>>,
    /// `prev`'s transport frame minus the transport's, fixed at the swap: not 0 only when the
    /// activation retimed the transport (a tempo change).
    shift: i64,
    /// Frames since the swap.
    since: u32,
    decl: Declick,
    /// The live input heard on this lane while a take records (MON-8): added to its block, so the
    /// track's effects and fader play it. Only a player's engine is ever sent one, so no export
    /// has it.
    hearing: Option<Hearing>,
    /// This block, channel-major, `channels × BLOCK`.
    out: Box<[f32]>,
    /// `prev`'s side of the block, while it fades out.
    old: Box<[f32]>,
}

impl AudioPlayer {
    pub(crate) fn new(lane: Arc<AudioLane>, sample_rate: f64) -> AudioPlayer {
        let out = vec![0.0; lane.channels * BLOCK].into_boxed_slice();
        AudioPlayer { old: out.clone(), out, cur: lane, prev: None, shift: 0, since: 0, decl: Declick::new(sample_rate), hearing: None }
    }

    /// This block, for `runs`. During a swap's window both renders play the same runs, `prev`
    /// through a copy of the declick state, and each sample crossfades from it to `cur`: a clip
    /// the edit didn't change is the same bits on both sides, so it stays exact (RENDER-27). Never
    /// allocates.
    pub(crate) fn fill(&mut self, runs: &Runs) {
        let len = self.decl.len;
        if let Some(prev) = self.prev.as_ref().filter(|_| self.since < len) {
            let mut decl = self.decl;
            decl.retime(self.shift);
            decl.process(prev, &runs.shifted(self.shift), &mut self.old);
            self.decl.process(&self.cur, runs, &mut self.out);
            for c in 0..self.cur.channels {
                for f in 0..BLOCK {
                    let i = c * BLOCK + f;
                    self.out[i] = xfade(self.old[i], self.out[i], self.since + f as u32, len);
                }
            }
            self.since += BLOCK as u32;
        } else {
            self.decl.process(&self.cur, runs, &mut self.out);
        }
        if let Some(h) = &mut self.hearing {
            h.add(&mut self.out, self.cur.channels);
        }
    }

    /// Hear `input` on this lane from the next block, or stop hearing: what it heard before, to
    /// retire. Never allocates.
    pub(crate) fn hear(&mut self, input: Option<Arc<dyn LiveInput>>) -> Option<Arc<dyn LiveInput>> {
        let old = self.hearing.take().map(|h| h.input);
        self.hearing = input.map(|input| Hearing { input, at: None });
        old
    }

    /// Whether it hears an input: the engine never idles then.
    pub(crate) fn hearing(&self) -> bool {
        self.hearing.is_some()
    }

    /// Work out on this thread what this block of `runs` reads that the ring doesn't hold: an
    /// offline render's fill before each block (D10). Live, the disk thread fills the ring and
    /// this is never called.
    pub(crate) fn prefetch(&self, runs: &Runs) {
        // Where the block reads: its runs, the position a jump would fade out from, and each fade
        // under way; the old renders too while a swap fades, all at their own position.
        let mut at = [(0u64, 0u64); NEED];
        at[0] = (self.decl.cont, BLOCK as u64);
        let mut n = 1;
        for t in &self.decl.taps[..self.decl.ntaps] {
            at[n] = (t.pos, BLOCK as u64);
            n += 1;
        }
        for r in runs.as_slice().iter().filter(|r| r.moving) {
            at[n] = (r.at, r.n as u64);
            n += 1;
        }
        ensure(&self.cur, &at[..n]);
        if let Some(prev) = self.prev.as_ref().filter(|_| self.since < self.decl.len) {
            for a in &mut at[..n] {
                a.0 = a.0.saturating_add_signed(self.shift);
            }
            ensure(prev, &at[..n]);
        }
    }

    /// Play `lane` from the next block, crossfading from what it played over one window. `shift`
    /// is where the old renders play relative to the transport. Never allocates: the window it
    /// fades in is the plan's, so no other swap is still fading.
    pub(crate) fn swap(&mut self, lane: Arc<AudioLane>, shift: i64) {
        debug_assert!(self.prev.is_none(), "a swap while another fades");
        self.prev = Some(std::mem::replace(&mut self.cur, lane));
        (self.shift, self.since) = (shift, 0);
        // A fade under way goes on where the new renders have its material.
        self.decl.retime(-shift);
    }

    /// The renders a swap replaced, at the window's end.
    pub(crate) fn take_prev(&mut self) -> Option<Arc<AudioLane>> {
        self.prev.take()
    }

    pub(crate) fn fading(&self) -> bool {
        self.prev.is_some()
    }

    pub(crate) fn out(&self) -> &[f32] {
        &self.out
    }

    pub(crate) fn reset(&mut self) {
        self.decl.reset();
    }

    pub(crate) fn dropped_taps(&self) -> u32 {
        self.decl.dropped
    }
}

/// An audio lane's declicking (RENDER-9 item 5): at a seek or a wrap the old position fades out
/// while the new one fades in, equal-power over 5 ms; stop fades out; play fades in only from
/// inside a clip, so playing from 0 or from a clip's start is exact (RENDER-33). A plan's first
/// block (Enter) changes nothing: a player moved into it goes on with its own fade-in, so a no-op
/// activation is exact, and a new one has none.
#[derive(Clone, Copy)]
struct Declick {
    taps: [Tap; TAPS],
    ntaps: usize,
    /// Frames of the fade-in done, while one runs.
    fade_in: Option<u32>,
    /// The fade's length, `F`.
    len: u32,
    prev_moving: bool,
    /// Where the lane would have gone on reading.
    cont: u64,
    /// Taps let go mid-ramp for want of room: only above [`MAX_RATE`].
    dropped: u32,
}

impl Declick {
    fn new(sample_rate: f64) -> Declick {
        let len = (0.005 * sample_rate).round().max(1.0) as u32;
        Declick { taps: [Tap::default(); TAPS], ntaps: 0, fade_in: None, len, prev_moving: false, cont: 0, dropped: 0 }
    }

    fn reset(&mut self) {
        (self.ntaps, self.fade_in, self.prev_moving, self.cont) = (0, None, false, 0);
    }

    /// Its positions `by` frames on: into another tempo map's frames at a swap (RENDER-27).
    fn retime(&mut self, by: i64) {
        self.cont = self.cont.saturating_add_signed(by);
        for t in &mut self.taps[..self.ntaps] {
            t.pos = t.pos.saturating_add_signed(by);
        }
    }

    fn gain_in(&self, k: u32) -> f32 {
        if k >= self.len { 1.0 } else { libm::sinf(std::f32::consts::FRAC_PI_2 * (k + 1) as f32 / self.len as f32) }
    }

    fn gain_out(&self, g0: f32, k: u32) -> f32 {
        if k >= self.len { 0.0 } else { g0 * libm::cosf(std::f32::consts::FRAC_PI_2 * (k + 1) as f32 / self.len as f32) }
    }

    /// The current material becomes a tap at the gain it has now.
    fn tap_out(&mut self) {
        if !self.prev_moving {
            return;
        }
        let g0 = match self.fade_in {
            Some(k) if k < self.len => libm::sinf(std::f32::consts::FRAC_PI_2 * k as f32 / self.len as f32),
            _ => 1.0,
        };
        if self.ntaps == TAPS {
            // Unreachable at a project rate (see TAPS). No assert: a panic here would silence the
            // player, where this is a small step (RENDER-28).
            self.dropped = self.dropped.saturating_add(1);
            // The quietest goes.
            let quietest = (0..TAPS).min_by(|&a, &b| {
                let g = |t: &Tap| self.gain_out(t.g0, t.done);
                g(&self.taps[a]).total_cmp(&g(&self.taps[b]))
            });
            let i = quietest.unwrap_or(0);
            self.taps[i] = self.taps[TAPS - 1];
            self.ntaps -= 1;
        }
        self.taps[self.ntaps] = Tap { pos: self.cont, g0, done: 0 };
        self.ntaps += 1;
    }

    /// Fill `out` (channel-major, `channels × BLOCK`) for one block of `runs`. Never allocates.
    fn process(&mut self, lane: &AudioLane, runs: &Runs, out: &mut [f32]) {
        let rs = runs.as_slice();
        // Offline, and live between jumps: today's copy, bit for bit.
        if let [r] = rs
            && r.moving
            && r.jump == Jump::None
            && self.ntaps == 0
            && self.fade_in.is_none()
        {
            lane.fill(out, BLOCK, 0, r.at, BLOCK);
            (self.prev_moving, self.cont) = (true, r.at + BLOCK as u64);
            return;
        }
        let mut scratch = [0.0f32; 2 * BLOCK];
        let channels = lane.channels.min(2);
        for r in rs {
            let (off, n) = (r.off as usize, r.n as usize);
            match r.jump {
                Jump::Seek | Jump::Wrap if r.moving => {
                    self.tap_out();
                    self.fade_in = Some(0);
                }
                Jump::Stop => {
                    self.tap_out();
                    self.fade_in = None;
                }
                Jump::Start => self.fade_in = (r.moving && lane.inside(r.at)).then_some(0),
                _ => {}
            }
            if r.moving {
                lane.fill(out, BLOCK, off, r.at, n);
                if let Some(k0) = self.fade_in {
                    for c in 0..channels {
                        for (k, x) in out[c * BLOCK + off..c * BLOCK + off + n].iter_mut().enumerate() {
                            *x *= self.gain_in(k0 + k as u32);
                        }
                    }
                    let k = k0 + n as u32;
                    self.fade_in = (k < self.len).then_some(k);
                }
                self.cont = r.at + n as u64;
            } else {
                for c in 0..lane.channels {
                    out[c * BLOCK + off..c * BLOCK + off + n].fill(0.0);
                }
            }
            for t in 0..self.ntaps {
                let tap = self.taps[t];
                lane.fill(&mut scratch, BLOCK, off, tap.pos, n);
                for c in 0..channels {
                    let (o, s) = (&mut out[c * BLOCK + off..c * BLOCK + off + n], &scratch[c * BLOCK + off..c * BLOCK + off + n]);
                    for (k, (x, y)) in o.iter_mut().zip(s).enumerate() {
                        *x += y * self.gain_out(tap.g0, tap.done + k as u32);
                    }
                }
                self.taps[t] = Tap { pos: tap.pos + n as u64, done: tap.done + n as u32, ..tap };
            }
            // Taps whose ramp has ended go; the rest keep their order.
            let mut kept = 0;
            for t in 0..self.ntaps {
                if self.taps[t].done < self.len {
                    self.taps[kept] = self.taps[t];
                    kept += 1;
                }
            }
            self.ntaps = kept;
            self.prev_moving = r.moving;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::Run;

    // RENDER-27: an activation that retimed the transport (a tempo change) swaps a player's renders
    // with the old ones read at their own position: for the window each sample crossfades from the
    // old renders at its frame plus the shift to the new ones at its frame, then the new ones alone.
    #[test]
    fn a_swap_reads_the_old_renders_at_their_own_position() {
        let ramp =
            |k: f32| Arc::new(AudioLane::of_renders(vec![(0, Arc::new(vec![(0..96_000).map(|i| k * i as f32).collect()]))], 1, 48_000.0));
        let (old, new) = (ramp(1e-5), ramp(-1e-5));
        let mut p = AudioPlayer::new(old, 48_000.0);
        let run = |at: u64| Runs::one(Run { off: 0, n: BLOCK as u32, at, moving: true, jump: Jump::None });
        p.fill(&run(10_000));
        p.swap(new, 3_000);
        let mut out = vec![];
        for b in 0..3u64 {
            p.fill(&run(10_128 + b * BLOCK as u64));
            out.extend_from_slice(&p.out()[..BLOCK]);
        }
        for (k, x) in out.iter().enumerate() {
            let at = 10_128.0 + k as f32;
            let want = xfade(1e-5 * (at + 3_000.0), -1e-5 * at, k as u32, 240);
            assert_eq!(x.to_bits(), want.to_bits(), "frame {k}");
        }
        assert!(p.fading() && p.take_prev().is_some() && !p.fading(), "the old renders wait for the window's end");
    }

    // RENDER-9 item 5, RENDER-28: at every project rate, the most fades jumps can start at once
    // fit. A seek on every block line with a wrap mid-block after it, block after block, is the
    // most the engine's message loop allows; no fade may be let go before it ends (a step).
    #[test]
    fn jumps_never_outrun_the_taps() {
        for rate in [8_000u32, 44_100, 48_000, 96_000, 192_000, 384_000] {
            let material: Vec<f32> = (0..rate as usize).map(|i| (i as f32 * 0.01).sin()).collect();
            let lane = AudioLane::of_renders(vec![(0, Arc::new(vec![material]))], 1, rate as f64);
            let mut node = Declick::new(rate as f64);
            let mut out = [0.0f32; BLOCK];
            node.process(&lane, &Runs::one(Run { off: 0, n: BLOCK as u32, at: 0, moving: true, jump: Jump::None }), &mut out);
            let mut most = 0;
            for k in 0..4 * node.len.div_ceil(BLOCK as u32) {
                // The wrap anywhere in the block.
                let w = 1 + (k * 37) % (BLOCK as u32 - 1);
                let mut runs = Runs::one(Run { off: 0, n: w, at: 1000 + 7 * k as u64, moving: true, jump: Jump::Seek });
                runs.push(Run { off: w, n: BLOCK as u32 - w, at: 500, moving: true, jump: Jump::Wrap });
                node.process(&lane, &runs, &mut out);
                most = most.max(node.ntaps);
            }
            assert_eq!(node.dropped, 0, "{rate} Hz: {most} fades at once, room for {TAPS}");
            assert!(most + 3 >= 2 * node.len.div_ceil(BLOCK as u32) as usize, "{rate} Hz: the jumps overlapped ({most} fades)");
        }
    }
}
