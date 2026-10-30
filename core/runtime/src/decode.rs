//! The one door every whole audio file music2 decodes goes through (FMT-24): a WAV or a FLAC, told
//! apart by its first bytes ([`kind`]), never by its name.
//!
//! A FLAC is read by the `claxon` crate, block by block ([`Flac`]). The limits a WAV has hold before
//! a sample of it is decoded: its frames × channels × bytes must fit a WAV's 32-bit sizes
//! ([`WAV_BYTES`]). Its integers are scaled as a WAV's are ([`scale`]), so a FLAC gives the very
//! bits of the WAV of its samples. A song's own clips stay WAV, which seeks exactly
//! ([`crate::stream`]): a FLAC coming into a song is written as that WAV ([`write_wav`]).

use std::io::{Read, Seek, Write};
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::audio::Asset;

/// What a file's first bytes say it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Wav,
    Flac,
}

/// How many first bytes [`kind`] needs.
pub const HEAD: usize = 12;

/// The kind of audio file whose first bytes are `head`: `RIFF`, then `WAVE` at byte 8, is a WAV;
/// `fLaC` a FLAC. None for anything else.
pub fn kind(head: &[u8]) -> Option<Kind> {
    if head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WAVE" {
        return Some(Kind::Wav);
    }
    (head.len() >= 4 && &head[..4] == b"fLaC").then_some(Kind::Flac)
}

/// [`kind`] of the file at `path`, from its first bytes.
pub fn kind_of(path: &Path) -> std::io::Result<Option<Kind>> {
    let mut f = std::fs::File::open(path)?;
    let mut head = [0u8; HEAD];
    let mut n = 0;
    while n < HEAD {
        match f.read(&mut head[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(kind(&head[..n]))
}

/// The most bytes of samples a WAV holds: its sizes are 32-bit, less room for its header.
pub const WAV_BYTES: u64 = u32::MAX as u64 - 4096;

/// What an integer sample of `bits` bits is multiplied by to play: `1 / 2^(bits − 1)`, a WAV's
/// scale and a FLAC's, so a full-scale number is ±1.
pub fn scale(bits: u16) -> f32 {
    1.0 / (1u64 << (bits - 1)) as f32
}

/// Decode the WAV or FLAC file at `path` whole (integer or float, any channels; past the first two
/// dropped), at its own rate.
pub fn load(path: &Path) -> Result<Asset> {
    match kind_of(path).with_context(|| format!("opening {}", path.display()))? {
        Some(Kind::Wav) => load_wav(path),
        Some(Kind::Flac) => load_flac(path),
        None => bail!("{} isn't a WAV or FLAC file", path.display()),
    }
}

fn load_wav(path: &Path) -> Result<Asset> {
    let mut r = hound::WavReader::open(path).with_context(|| format!("opening {}", path.display()))?;
    let spec = r.spec();
    let ch = spec.channels.max(1) as usize;
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => r.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = scale(spec.bits_per_sample);
            r.samples::<i32>().map(|s| s.map(|v| v as f32 * scale)).collect::<Result<_, _>>()?
        }
    };
    let keep = ch.min(2);
    let channels = (0..keep).map(|c| interleaved.iter().skip(c).step_by(ch).copied().collect()).collect();
    Ok(Asset { channels, sample_rate: spec.sample_rate as f64 })
}

/// A FLAC read from its start to its end, a block at a time: its header checked when it is opened,
/// each block as it is decoded.
pub struct Flac<R: Read> {
    reader: claxon::FlacReader<R>,
    pub channels: u16,
    pub rate: u32,
    pub bits: u16,
    /// Frames, as its header says: what it must hold.
    pub frames: u64,
    /// Frames decoded so far.
    done: u64,
    /// The last block's buffer, kept for the next.
    buf: Vec<i32>,
}

impl<R: Read> Flac<R> {
    /// Read the FLAC's header from `r`, and check it (FMT-24): 8, 16 or 24 bits in a sample, its
    /// length said, and no more samples than a WAV holds. Why not, in words.
    pub fn new(r: R) -> Result<Flac<R>> {
        let opts = claxon::FlacReaderOptions { metadata_only: false, read_vorbis_comment: false };
        let reader = claxon::FlacReader::new_ext(r, opts).map_err(|e| anyhow::anyhow!("its header is damaged ({e})"))?;
        let info = reader.streaminfo();
        let bits = info.bits_per_sample;
        if !matches!(bits, 8 | 16 | 24) {
            bail!("it has {bits} bits in each sample: music2 reads FLAC files of 8, 16 or 24 bits");
        }
        let Some(frames) = info.samples else { bail!("its header doesn't say how long it is, which music2 needs to read a FLAC") };
        let bytes = frames.saturating_mul(info.channels as u64).saturating_mul(bits as u64 / 8);
        if bytes > WAV_BYTES {
            bail!("it is too long: as a WAV it would be over 4 GiB, more than a WAV can hold");
        }
        Ok(Flac { reader, channels: info.channels as u16, rate: info.sample_rate, bits: bits as u16, frames, done: 0, buf: vec![] })
    }

    /// The next block's samples into `out`, one list for each channel (each cleared first): how many
    /// frames, 0 at the end. A file that holds more or fewer frames than its header says, a block
    /// with other channels, or a sample outside its bits is damaged: refused, never played.
    pub fn next(&mut self, out: &mut Vec<Vec<i32>>) -> Result<usize> {
        let buf = std::mem::take(&mut self.buf);
        let block = self.reader.blocks().read_next_or_eof(buf).map_err(|e| anyhow::anyhow!("it is damaged: {e}"))?;
        let Some(block) = block else {
            if self.done != self.frames {
                bail!("it is cut short: it holds {} of the {} frames its header says", self.done, self.frames);
            }
            return Ok(0);
        };
        if block.channels() != self.channels as u32 {
            bail!("it is damaged: a block of it has {} channels, not {}", block.channels(), self.channels);
        }
        let n = block.duration() as usize;
        self.done += n as u64;
        if self.done > self.frames {
            bail!("it is damaged: it holds more samples than its header says");
        }
        let top = 1i32 << (self.bits - 1);
        out.resize_with(self.channels as usize, Vec::new);
        for (c, o) in out.iter_mut().enumerate() {
            let samples = block.channel(c as u32);
            if samples.iter().any(|&v| v < -top || v >= top) {
                bail!("it is damaged: a sample is outside its {} bits", self.bits);
            }
            o.clear();
            o.extend_from_slice(samples);
        }
        self.buf = block.into_buffer();
        Ok(n)
    }
}

fn load_flac(path: &Path) -> Result<Asset> {
    let file = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut f = Flac::new(file).with_context(|| format!("reading {}", path.display()))?;
    let (keep, scale) = ((f.channels as usize).min(2), scale(f.bits));
    // Grown as it decodes, never reserved on the header's word alone.
    let mut channels: Vec<Vec<f32>> = vec![vec![]; keep];
    let mut block = vec![];
    while f.next(&mut block).with_context(|| format!("reading {}", path.display()))? > 0 {
        for (c, b) in channels.iter_mut().zip(&block) {
            c.extend(b.iter().map(|&v| v as f32 * scale));
        }
    }
    Ok(Asset { channels, sample_rate: f.rate as f64 })
}

/// Write `flac` to `out` as the WAV of its samples (FMT-24): its channels, its rate and its bits
/// (8 → 8, 16 → 16, 24 → 24), so the WAV decodes to the FLAC's very bits. `out` is finished, not
/// pushed to the disk.
pub fn write_wav<R: Read, W: Write + Seek>(mut flac: Flac<R>, out: W) -> Result<()> {
    let spec = hound::WavSpec {
        channels: flac.channels,
        sample_rate: flac.rate,
        bits_per_sample: flac.bits,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::new(out, spec)?;
    let mut block = vec![];
    loop {
        let n = flac.next(&mut block)?;
        if n == 0 {
            break;
        }
        for i in 0..n {
            for c in &block {
                w.write_sample(c[i])?;
            }
        }
    }
    w.finalize()?;
    Ok(())
}
