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
//! Time-stretching (WSOLA) lives here for now. The design makes stretching a package
//! kernel so better algorithms can replace it; that needs an offline-kernel ABI
//! (whole buffers in, whole buffers out), which is the next step for this code.
//!
//! Its math goes through the `libm` crate, never the OS's library, so a clip renders the same bits
//! on every OS (PLAT-4; `tests/host_math.rs` keeps it so).

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Weak};

use anyhow::{Context, Result};
use m2_song::{AudioClip, DataSet, HashCache, Sha, TempoMap};

use crate::BLOCK;
use crate::fade::xfade;
use crate::stream::{self, Clip, Ring, Shape, Source, Wav, gain_at};
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

/// Read a WAV file (integer or float, mono or stereo; extra channels are dropped).
pub fn load_wav(path: &Path) -> Result<Asset> {
    let mut r = hound::WavReader::open(path).with_context(|| format!("opening {}", path.display()))?;
    let spec = r.spec();
    let ch = spec.channels.max(1) as usize;
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => r.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample - 1)) as f32;
            r.samples::<i32>().map(|s| s.map(|v| v as f32 * scale)).collect::<Result<_, _>>()?
        }
    };
    let keep = ch.min(2);
    let channels = (0..keep).map(|c| interleaved.iter().skip(c).step_by(ch).copied().collect()).collect();
    Ok(Asset { channels, sample_rate: spec.sample_rate as f64 })
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
/// tempo map and the rate.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ClipKey {
    asset: Sha,
    clip: Vec<u64>,
    tempo: Vec<u64>,
    rate: u64,
}

impl ClipKey {
    fn of(asset: Sha, c: &AudioClip, tempo: &TempoMap, rate: f64) -> ClipKey {
        let mut clip = vec![c.start, c.length, c.offset, c.gain_db, c.fade_in, c.fade_out, c.pitch, c.reverse as u8 as f64];
        clip.extend(c.warp.iter().flat_map(|&(a, b)| [a, b]));
        let tempo = tempo.segments().iter().flat_map(|t| [t.beat, t.bpm]).map(f64::to_bits).collect();
        ClipKey { asset, clip: clip.into_iter().map(f64::to_bits).collect(), tempo, rate: rate.to_bits() }
    }

    /// Its bytes, for a file name.
    fn bytes(&self) -> Vec<u8> {
        let mut b = self.asset.hex().into_bytes();
        for n in [&[self.clip.len() as u64][..], &self.clip, &[self.tempo.len() as u64], &self.tempo, &[self.rate]] {
            for x in n {
                b.extend_from_slice(&x.to_le_bytes());
            }
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

/// Opened assets, by content; warped renders, by what they depend on; and lanes, by what they
/// play. Building opens files and reads their headers, and fills the lanes short enough to stay
/// whole; compiling never does (FMT-16). A live session keeps one per open project, so an edit
/// shares every lane it didn't change with the renderer it replaces, and its ring with it; an
/// offline render uses one of its own, so its rings are its own too.
#[derive(Default)]
pub struct Cache {
    wavs: HashMap<Sha, Option<Arc<Wav>>>,
    warps: HashMap<ClipKey, Option<Arc<Source>>>,
    lanes: HashMap<LaneKey, Arc<AudioLane>>,
    /// Assets decoded whole for a warp's one render (D2), until the next prune.
    decoded: HashMap<Sha, Option<Arc<Asset>>>,
    used_wavs: HashSet<Sha>,
    used_warps: HashSet<ClipKey>,
    used_lanes: HashSet<LaneKey>,
    decodes: usize,
    renders: usize,
}

impl Cache {
    /// The opened file and its hash, or `None` when it can't be read, decoded, or holds no
    /// audio. The hash is a stamp lookup: validating the data already hashed every asset.
    fn wav(&mut self, path: &Path) -> Option<(Sha, Arc<Wav>)> {
        let sha = HashCache::global().hash(path).ok()?;
        self.used_wavs.insert(sha);
        let decodes = &mut self.decodes;
        let w = self.wavs.entry(sha).or_insert_with(|| {
            *decodes += 1;
            Wav::open(path).ok().filter(|w| w.frames() > 0).map(Arc::new)
        });
        Some((sha, w.clone()?))
    }

    /// A warped clip's render (D1): from the cache folder, else made once, there or in memory.
    fn warp(&mut self, key: ClipKey, path: &Path, c: &AudioClip, tempo: &TempoMap, rate: f64) -> Option<Arc<Source>> {
        self.used_warps.insert(key.clone());
        if let Some(s) = self.warps.get(&key) {
            return s.clone();
        }
        let frames = stream::clip_len(c, tempo, rate);
        let (decoded, renders) = (&mut self.decoded, &mut self.renders);
        let render = || {
            let asset = decoded.entry(key.asset).or_insert_with(|| load_wav(path).ok().filter(|a| !a.is_empty()).map(Arc::new)).clone()?;
            *renders += 1;
            Some(render_clip(c, &asset, tempo, rate))
        };
        let channels = self.wavs.get(&key.asset).cloned().flatten().map_or(1, |w| w.channels());
        let s = stream::warped(stream::render_path(&key.bytes()), render, channels, frames).map(Arc::new);
        self.warps.insert(key, s.clone());
        s
    }

    /// Keep only what the builds since the last call used: an edited lane's old ring goes once
    /// the renderer that played it is built over.
    pub fn prune(&mut self) {
        let wavs = std::mem::take(&mut self.used_wavs);
        let warps = std::mem::take(&mut self.used_warps);
        let lanes = std::mem::take(&mut self.used_lanes);
        self.wavs.retain(|k, _| wavs.contains(k));
        self.warps.retain(|k, _| warps.contains(k));
        self.lanes.retain(|k, _| lanes.contains(k));
        self.decoded.clear();
    }

    /// Assets opened and warped clips rendered since the cache was made.
    pub fn work(&self) -> (usize, usize) {
        (self.decodes, self.renders)
    }

    /// Every lane it holds, weakly: tests check that what no plan plays any more is freed.
    pub fn lanes(&self) -> Vec<Weak<AudioLane>> {
        self.lanes.values().map(Arc::downgrade).collect()
    }
}

/// Read `x` at fractional position `pos` (cubic Hermite); silence outside.
fn read(x: &[f32], pos: f64) -> f32 {
    stream::read_in(x, 0, x.len(), pos)
}

const FRAME: usize = 2048;
const HOP: usize = FRAME / 2;
const SEARCH: isize = 384;

/// The warp's version, in the name of every warped render kept in the cache folder
/// ([`stream::render_path`]). Any change to what [`stretch`] or [`render_clip`] makes MUST bump it,
/// or users' caches go on playing (and exporting) the old renders (the review's io-r5).
pub(crate) const WARP_VERSION: u32 = 1;

/// WSOLA time-stretch with an arbitrary monotonic time map: output sample `t` comes
/// from around source position `map(t)`. Keeps pitch; channels share one alignment.
fn stretch(src: &[Vec<f32>], out_len: usize, map: impl Fn(f64) -> f64) -> Vec<Vec<f32>> {
    let window: Vec<f32> = (0..FRAME).map(|i| 0.5 - 0.5 * libm::cosf(std::f32::consts::TAU * i as f32 / FRAME as f32)).collect();
    let mono: Vec<f32> = (0..src[0].len()).map(|i| src.iter().map(|c| c[i]).sum::<f32>()).collect();
    let mut out = vec![vec![0.0f32; out_len + FRAME]; src.len()];
    let mut prev: Option<isize> = None;
    let mut k = 0usize;
    while k * HOP < out_len {
        let t = (k * HOP) as f64;
        let nominal = map(t + FRAME as f64 / 2.0) as isize - FRAME as isize / 2;
        let pos = match prev {
            None => nominal,
            Some(p) => {
                // The natural continuation of the previous frame, and the candidate near
                // `nominal` that resembles it most (cross-correlation over the overlap).
                let cont = p + HOP as isize;
                let score = |cand: isize| -> f32 {
                    let mut s = 0.0;
                    for i in (0..HOP).step_by(2) {
                        let (a, b) = (cont + i as isize, cand + i as isize);
                        if a >= 0 && b >= 0 && (a as usize) < mono.len() && (b as usize) < mono.len() {
                            s += mono[a as usize] * mono[b as usize];
                        }
                    }
                    s
                };
                let mut best = (nominal, f32::MIN);
                for d in (-SEARCH..=SEARCH).step_by(8) {
                    let sc = score(nominal + d);
                    if sc > best.1 {
                        best = (nominal + d, sc);
                    }
                }
                let coarse = best.0;
                for d in -8..=8 {
                    let sc = score(coarse + d);
                    if sc > best.1 {
                        best = (coarse + d, sc);
                    }
                }
                best.0
            }
        };
        for (c, ch) in src.iter().enumerate() {
            for i in 0..FRAME {
                let s = pos + i as isize;
                if s >= 0 && (s as usize) < ch.len() {
                    out[c][k * HOP + i] += ch[s as usize] * window[i];
                }
            }
        }
        prev = Some(pos);
        k += 1;
    }
    for c in &mut out {
        c.truncate(out_len);
    }
    out
}

/// Render one clip at `sample_rate`: its samples from the clip's start, for its length. A warped
/// clip is made this way, once (D2); a clip with no warp is worked out the same way piece by piece
/// from its file as it plays ([`stream`]), and this is the reference its pieces must equal.
pub fn render_clip(clip: &AudioClip, asset: &Asset, tempo: &TempoMap, sample_rate: f64) -> Vec<Vec<f32>> {
    let t0 = tempo.seconds_at(clip.start);
    let src_rate = asset.sample_rate;
    let sh = Shape::of(clip, src_rate, tempo, sample_rate);
    let out_len = sh.len;
    let rate = libm::exp2(clip.pitch / 12.0);

    let mut out: Vec<Vec<f32>> = if clip.warp.is_empty() {
        // Varispeed: source time runs `rate` times faster.
        asset.channels.iter().map(|ch| (0..out_len).map(|i| read(ch, sh.offset + i as f64 * sh.step)).collect()).collect()
    } else {
        // Warp: output time → beat → source seconds, piecewise linear through the
        // markers (extended past the ends by their nearest segment).
        let w = &clip.warp;
        let src_at_beat = |b: f64| -> f64 {
            if w.len() == 1 {
                // One marker pins a moment; the audio runs at its own speed around it.
                return w[0].0 + tempo.seconds_at(clip.start + b) - tempo.seconds_at(clip.start + w[0].1);
            }
            let i = w.partition_point(|m| m.1 <= b).clamp(1, w.len() - 1);
            let (a, z) = (w[i - 1], w[i]);
            a.0 + (b - a.1) * (z.0 - a.0) / (z.1 - a.1)
        };
        let src_seconds = |out_sec: f64| src_at_beat(tempo.beat_at(t0 + out_sec) - clip.start);
        // The stretch copies source frames, so its output runs at the source's sample
        // rate. Pitch with warp: stretch to `rate` × the duration, then read it back
        // `rate` times faster (which also converts to the song's sample rate).
        let z_len = (out_len as f64 * rate * src_rate / sample_rate).ceil() as usize + 4;
        let z = stretch(&asset.channels, z_len, |u| src_seconds(u / src_rate / rate) * src_rate);
        z.iter().map(|ch| (0..out_len).map(|i| read(ch, i as f64 * sh.step)).collect()).collect()
    };
    for ch in &mut out {
        if clip.reverse {
            ch.reverse();
        }
        let n = ch.len();
        for (i, x) in ch.iter_mut().enumerate() {
            *x *= gain_at(i, n, sh.gain, sh.fade_in, sh.fade_out);
        }
    }
    out
}

/// An audio lane, ready to play: its clips at their start samples, and the ring the sound thread
/// copies them from ([`stream`]).
pub struct AudioLane {
    /// What it plays, for [`AudioLane::same`]: none for a lane made from renders.
    key: Option<LaneKey>,
    pub(crate) clips: Vec<Clip>,
    pub(crate) channels: usize,
    pub(crate) ring: Ring,
    pub(crate) tempo: TempoMap,
    pub(crate) rate: f64,
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
    /// played (`asset.missing`) or can't be decoded is silent; it never silences the rest
    /// (FMT-15). A lane short enough to stay whole is filled here.
    pub fn build(clips: &[AudioClip], data: &DataSet, cache: &mut Cache, tempo: &TempoMap, sample_rate: f64, channels: usize) -> Arc<AudioLane> {
        AudioLane::build_with(clips, &|a| data.asset_path(a), cache, tempo, sample_rate, channels)
    }

    /// [`AudioLane::build`] with each asset's path from `asset_path` (tests and tools).
    pub fn build_with(
        clips: &[AudioClip],
        asset_path: &dyn Fn(&str) -> Option<std::path::PathBuf>,
        cache: &mut Cache,
        tempo: &TempoMap,
        sample_rate: f64,
        channels: usize,
    ) -> Arc<AudioLane> {
        let mut found = vec![];
        for c in clips {
            let Some(path) = asset_path(&c.asset) else { continue };
            let Some((sha, w)) = cache.wav(&path) else { continue };
            found.push((c, ClipKey::of(sha, c, tempo, sample_rate), w, path));
        }
        let key = LaneKey { clips: found.iter().map(|f| f.1.clone()).collect(), channels };
        cache.used_lanes.insert(key.clone());
        if let Some(l) = cache.lanes.get(&key) {
            return l.clone();
        }
        let mut out = vec![];
        for (c, k, w, path) in found {
            let at = (tempo.seconds_at(c.start) * sample_rate).round() as u64;
            let src = if c.warp.is_empty() {
                Source::Wav(w.clone(), Shape::of(c, w.rate, tempo, sample_rate))
            } else {
                match cache.warp(k, &path, c, tempo, sample_rate) {
                    Some(s) => match &*s {
                        Source::Render(r) => Source::Render(r.clone()),
                        Source::Mem(m) => Source::Mem(m.clone()),
                        Source::Wav(..) => continue,
                    },
                    None => continue,
                }
            };
            out.push(Clip::new(at, src, stream::clip_len(c, tempo, sample_rate)));
        }
        let lane = Arc::new(AudioLane::of(out, channels, tempo.clone(), sample_rate, Some(key.clone())));
        cache.lanes.insert(key, lane.clone());
        lane
    }

    fn of(clips: Vec<Clip>, channels: usize, tempo: TempoMap, rate: f64, key: Option<LaneKey>) -> AudioLane {
        let ring = Ring::new(&clips, channels, rate);
        let lane = AudioLane { key, clips, channels, ring, tempo, rate };
        lane.fill_all();
        lane
    }

    /// A lane of renders in memory at their start frames, in a song at `rate` with no tempo
    /// change (tests).
    #[doc(hidden)]
    pub fn of_renders(clips: Vec<(u64, Arc<RenderedClip>)>, channels: usize, rate: f64) -> AudioLane {
        let clips = clips.into_iter().map(|(at, r)| {
            let len = r[0].len();
            Clip::new(at, Source::Mem(r), len)
        });
        AudioLane::of(clips.collect(), channels, TempoMap::constant(120.0), rate, None)
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
        let ramp = |k: f32| Arc::new(AudioLane::of_renders(vec![(0, Arc::new(vec![(0..96_000).map(|i| k * i as f32).collect()]))], 1, 48_000.0));
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
