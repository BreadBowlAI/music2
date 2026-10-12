//! Audio lanes: clips rendered once, then played sample-exactly.
//!
//! Each clip is rendered to a buffer at the song's sample rate when the renderer is
//! built (resampling, varispeed, warping, reverse, gain, fades), so playback is a copy
//! and seeking is exact.
//!
//! Time-stretching (WSOLA) lives here for now. The design makes stretching a package
//! kernel so better algorithms can replace it; that needs an offline-kernel ABI
//! (whole buffers in, whole buffers out), which is the next step for this code.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use m2_song::{AudioClip, DataSet, TempoMap};

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
    wav_to(file, channels, sample_rate)
}

/// [`write_wav`] to a file that must not exist yet: assets are never overwritten, so a new one
/// is created with create-only semantics (FMT-14). A failed write leaves nothing behind.
pub fn write_new_wav(path: &Path, channels: &[Vec<f32>], sample_rate: u32) -> Result<()> {
    let file =
        std::fs::OpenOptions::new().write(true).create_new(true).open(path).with_context(|| format!("creating {}", path.display()))?;
    wav_to(file, channels, sample_rate).inspect_err(|_| drop(std::fs::remove_file(path)))
}

fn wav_to(file: std::fs::File, channels: &[Vec<f32>], sample_rate: u32) -> Result<()> {
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

/// Assets decoded once per renderer, by path. Rendering decodes; compiling never does (FMT-16).
#[derive(Default)]
pub struct Assets {
    loaded: HashMap<PathBuf, Option<Arc<Asset>>>,
}

impl Assets {
    /// The decoded file, or `None` when it can't be decoded or holds no audio.
    pub fn get(&mut self, path: &Path) -> Option<Arc<Asset>> {
        self.loaded.entry(path.to_path_buf()).or_insert_with(|| load_wav(path).ok().filter(|a| !a.is_empty()).map(Arc::new)).clone()
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
    let window: Vec<f32> = (0..FRAME).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / FRAME as f32).cos()).collect();
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
    let rate = 2f64.powf(clip.pitch / 12.0);
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
    let gain = 10f32.powf(clip.gain_db as f32 / 20.0);
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
                g *= 0.5 - 0.5 * (std::f32::consts::PI * i as f32 / fade_in as f32).cos();
            }
            if n - i <= fade_out {
                g *= 0.5 - 0.5 * (std::f32::consts::PI * (n - i) as f32 / fade_out as f32).cos();
            }
            *x *= g;
        }
    }
    out
}

/// An audio lane, ready to play: rendered clips at their start samples.
pub struct AudioLane {
    clips: Vec<(u64, Vec<Vec<f32>>)>,
    channels: usize,
}

impl AudioLane {
    /// Render `clips`, in the order given (canonical: by start and id, so overlapping clips
    /// always sum in one order). A clip whose asset may not be played (`asset.missing`) or
    /// can't be decoded is silent; it never silences the rest (FMT-15).
    pub fn build(
        clips: &[AudioClip],
        data: &DataSet,
        assets: &mut Assets,
        tempo: &TempoMap,
        sample_rate: f64,
        channels: usize,
    ) -> AudioLane {
        let mut out = vec![];
        for c in clips {
            let Some(a) = data.asset_path(&c.asset).and_then(|p| assets.get(&p)) else { continue };
            let start = (tempo.seconds_at(c.start) * sample_rate).round() as u64;
            out.push((start, render_clip(c, &a, tempo, sample_rate)));
        }
        AudioLane { clips: out, channels }
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
