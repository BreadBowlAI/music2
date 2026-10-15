//! Audio lanes: clips rendered once, then played sample-exactly.
//!
//! Each clip is rendered to a buffer at the song's sample rate when the renderer is
//! built (resampling, varispeed, warping, reverse, gain, fades), so playback is a copy
//! and seeking is exact. Decoded assets and clip renders are kept in a [`Cache`] by content,
//! so a live rebuild shares what it didn't change instead of decoding and warping it again
//! (RENDER-27, RENDER-30 in part).
//!
//! Live, a lane crossfades over 5 ms from its old position to its new one at a seek or a loop
//! wrap, fades out on stop, and fades in on play from inside a clip (RENDER-9); offline it is a
//! plain copy.
//!
//! Time-stretching (WSOLA) lives here for now. The design makes stretching a package
//! kernel so better algorithms can replace it; that needs an offline-kernel ABI
//! (whole buffers in, whole buffers out), which is the next step for this code.
//!
//! Its math goes through the `libm` crate, never the OS's library, so a clip renders the same bits
//! on every OS (PLAT-4; `tests/host_math.rs` keeps it so).

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use m2_song::{AudioClip, DataSet, HashCache, Sha, TempoMap};

use crate::BLOCK;
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
}

/// Decoded assets, by content, and clip renders, by what they depend on. Rendering decodes;
/// compiling never does (FMT-16). A live session keeps one per open project, so an edit
/// shares every clip it didn't change with the renderer it replaces; an offline render uses
/// one of its own.
#[derive(Default)]
pub struct Cache {
    assets: HashMap<Sha, Option<Arc<Asset>>>,
    clips: HashMap<ClipKey, Arc<RenderedClip>>,
    used_assets: HashSet<Sha>,
    used_clips: HashSet<ClipKey>,
    decodes: usize,
    renders: usize,
}

impl Cache {
    /// The decoded file and its hash, or `None` when it can't be read, decoded, or holds no
    /// audio. The hash is a stamp lookup: validating the data already hashed every asset.
    fn asset(&mut self, path: &Path) -> Option<(Sha, Arc<Asset>)> {
        let sha = HashCache::global().hash(path).ok()?;
        self.used_assets.insert(sha);
        let decodes = &mut self.decodes;
        let a = self.assets.entry(sha).or_insert_with(|| {
            *decodes += 1;
            load_wav(path).ok().filter(|a| !a.is_empty()).map(Arc::new)
        });
        Some((sha, a.clone()?))
    }

    fn clip(&mut self, sha: Sha, asset: &Asset, c: &AudioClip, tempo: &TempoMap, rate: f64) -> Arc<RenderedClip> {
        let key = ClipKey::of(sha, c, tempo, rate);
        self.used_clips.insert(key.clone());
        let renders = &mut self.renders;
        self.clips
            .entry(key)
            .or_insert_with(|| {
                *renders += 1;
                Arc::new(render_clip(c, asset, tempo, rate))
            })
            .clone()
    }

    /// Keep only what the builds since the last call used: an edited clip's old render goes
    /// once the renderer that played it is built over.
    pub fn prune(&mut self) {
        let (assets, clips) = (std::mem::take(&mut self.used_assets), std::mem::take(&mut self.used_clips));
        self.assets.retain(|k, _| assets.contains(k));
        self.clips.retain(|k, _| clips.contains(k));
    }

    /// Assets decoded and clips rendered since the cache was made.
    pub fn work(&self) -> (usize, usize) {
        (self.decodes, self.renders)
    }
}

/// Read `x` at fractional position `pos` (cubic Hermite); silence outside.
fn read(x: &[f32], pos: f64) -> f32 {
    if pos < 0.0 || pos > (x.len() as f64 - 1.0) {
        return 0.0;
    }
    let i = pos.floor() as isize;
    let t = (pos - i as f64) as f32;
    let at = |k: isize| if k < 0 || k as usize >= x.len() { 0.0 } else { x[k as usize] };
    let (y0, y1, y2, y3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
    let c1 = 0.5 * (y2 - y0);
    let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
    let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
    ((c3 * t + c2) * t + c1) * t + y1
}

const FRAME: usize = 2048;
const HOP: usize = FRAME / 2;
const SEARCH: isize = 384;

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

/// Render one clip at `sample_rate`: its samples from the clip's start, for its length.
pub fn render_clip(clip: &AudioClip, asset: &Asset, tempo: &TempoMap, sample_rate: f64) -> Vec<Vec<f32>> {
    let t0 = tempo.seconds_at(clip.start);
    let out_len = ((tempo.seconds_at(clip.start + clip.length) - t0) * sample_rate).round() as usize;
    // exp2 by name, so debug and release builds call one function: a release build turns
    // `2f64.powf(x)` into exp2 where a debug build calls pow, and libm's two differ in the last bit.
    let rate = libm::exp2(clip.pitch / 12.0);
    let src_rate = asset.sample_rate;
    let offset = clip.offset * src_rate;

    let mut out: Vec<Vec<f32>> = if clip.warp.is_empty() {
        // Varispeed: source time runs `rate` times faster.
        let step = rate * src_rate / sample_rate;
        asset.channels.iter().map(|ch| (0..out_len).map(|i| read(ch, offset + i as f64 * step)).collect()).collect()
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
        let step = rate * src_rate / sample_rate;
        z.iter().map(|ch| (0..out_len).map(|i| read(ch, i as f64 * step)).collect()).collect()
    };
    let gain = libm::powf(10.0, clip.gain_db as f32 / 20.0);
    let fade_in = (clip.fade_in * sample_rate) as usize;
    let fade_out = (clip.fade_out * sample_rate) as usize;
    for ch in &mut out {
        if clip.reverse {
            ch.reverse();
        }
        let n = ch.len();
        for (i, x) in ch.iter_mut().enumerate() {
            let mut g = gain;
            if i < fade_in {
                g *= 0.5 - 0.5 * libm::cosf(std::f32::consts::PI * i as f32 / fade_in as f32);
            }
            if n - i <= fade_out {
                g *= 0.5 - 0.5 * libm::cosf(std::f32::consts::PI * (n - i) as f32 / fade_out as f32);
            }
            *x *= g;
        }
    }
    out
}

/// An audio lane, ready to play: rendered clips at their start samples.
pub struct AudioLane {
    clips: Vec<(u64, Arc<RenderedClip>)>,
    channels: usize,
}

impl AudioLane {
    /// Render `clips`, in the order given (canonical: by start and id, so overlapping clips
    /// always sum in one order). A clip whose asset may not be played (`asset.missing`) or
    /// can't be decoded is silent; it never silences the rest (FMT-15).
    pub fn build(clips: &[AudioClip], data: &DataSet, cache: &mut Cache, tempo: &TempoMap, sample_rate: f64, channels: usize) -> AudioLane {
        let mut out = vec![];
        for c in clips {
            let Some((sha, a)) = data.asset_path(&c.asset).and_then(|p| cache.asset(&p)) else { continue };
            let start = (tempo.seconds_at(c.start) * sample_rate).round() as u64;
            out.push((start, cache.clip(sha, &a, c, tempo, sample_rate)));
        }
        AudioLane { clips: out, channels }
    }

    /// Whether transport frame `pos` is strictly inside a clip: playing from there starts
    /// mid-sound.
    fn inside(&self, pos: u64) -> bool {
        self.clips.iter().any(|(at, data)| *at < pos && pos < at + data[0].len() as u64)
    }

    /// Fill frames `off..off + n` of `out` (channel-major, `channels × block`) with the
    /// lane's samples `start..start + n`. Overlapping clips are summed.
    pub fn fill(&self, out: &mut [f32], block: usize, off: usize, start: u64, n: usize) {
        for c in 0..self.channels {
            out[c * block + off..c * block + off + n].fill(0.0);
        }
        let end = start + n as u64;
        for (at, data) in &self.clips {
            let len = data[0].len() as u64;
            if *at >= end || at + len <= start {
                continue;
            }
            let from = start.max(*at);
            let to = end.min(at + len);
            for c in 0..self.channels {
                let src = &data[c.min(data.len() - 1)];
                let mono_down = self.channels == 1 && data.len() > 1;
                for t in from..to {
                    let i = (t - at) as usize;
                    let v = if mono_down { 0.5 * (data[0][i] + data[1][i]) } else { src[i] };
                    out[c * block + off + (t - start) as usize] += v;
                }
            }
        }
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

/// An audio lane node's declicking (RENDER-9 item 5): at a seek or a wrap the old position
/// fades out while the new one fades in, equal-power over 5 ms; stop fades out; play fades in
/// only from inside a clip, so playing from 0 or from a clip's start is exact (RENDER-33).
/// Nothing is faded at a renderer's first block: the whole-output crossfade covers it.
pub struct AudioNode {
    pub(crate) slot: usize,
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

impl AudioNode {
    pub(crate) fn new(slot: usize, sample_rate: f64) -> AudioNode {
        let len = (0.005 * sample_rate).round().max(1.0) as u32;
        AudioNode { slot, taps: [Tap::default(); TAPS], ntaps: 0, fade_in: None, len, prev_moving: false, cont: 0, dropped: 0 }
    }

    /// Taps let go before their fade ended (a small step each).
    pub(crate) fn dropped_taps(&self) -> u32 {
        self.dropped
    }

    pub(crate) fn reset(&mut self) {
        (self.ntaps, self.fade_in, self.prev_moving, self.cont) = (0, None, false, 0);
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
    pub(crate) fn process(&mut self, lane: &AudioLane, runs: &Runs, out: &mut [f32]) {
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
                Jump::Enter => self.fade_in = None,
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

    // RENDER-9 item 5, RENDER-28: at every project rate, the most fades jumps can start at once
    // fit. A seek on every block line with a wrap mid-block after it, block after block, is the
    // most the engine's message loop allows; no fade may be let go before it ends (a step).
    #[test]
    fn jumps_never_outrun_the_taps() {
        for rate in [8_000u32, 44_100, 48_000, 96_000, 192_000, 384_000] {
            let material: Vec<f32> = (0..rate as usize).map(|i| (i as f32 * 0.01).sin()).collect();
            let lane = AudioLane { clips: vec![(0, Arc::new(vec![material]))], channels: 1 };
            let mut node = AudioNode::new(0, rate as f64);
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
            assert_eq!(node.dropped_taps(), 0, "{rate} Hz: {most} fades at once, room for {TAPS}");
            assert!(most + 3 >= 2 * node.len.div_ceil(BLOCK as u32) as usize, "{rate} Hz: the jumps overlapped ({most} fades)");
        }
    }
}
