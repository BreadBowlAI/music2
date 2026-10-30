//! What is in an audio file, and decoding it whole (FMT-24): a WAV or a FLAC, told apart by its
//! first bytes ([`kind`], [`kind_of`]), never by its name; a FLAC's ID3v2 tag before it is passed
//! over.
//!
//! [`header_of`] reads what a file's header says before a sample of it is decoded: the one place
//! the budget of a whole decode is checked from ([`WHOLE`], [`budget`]), for a WAV as for a FLAC,
//! since a FLAC of silence can say a long length in few bytes. [`load`] decodes a file whole (a
//! kernel's: a Sampler's recording, a pack's), held to that budget. A FLAC is read by the `claxon`
//! crate, block by block ([`Flac`]); its integers are scaled as a WAV's are ([`scale`]), so a FLAC
//! gives the very bits of the WAV of its samples. A song's own clips stay WAV, which seeks exactly
//! ([`crate::stream`], read in pieces there): a FLAC coming into a song is written as that WAV
//! ([`write_wav`]).

use std::io::{Chain, Cursor, Read, Seek, Write};
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
/// `fLaC` a FLAC. None for anything else, a FLAC behind an ID3v2 tag too: [`kind_of_file`] and
/// [`sniff`] look past one.
pub fn kind(head: &[u8]) -> Option<Kind> {
    if head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WAVE" {
        return Some(Kind::Wav);
    }
    (head.len() >= 4 && &head[..4] == b"fLaC").then_some(Kind::Flac)
}

/// The length of the ID3v2 tag `head` (at least its first 10 bytes) starts with, if it does: its
/// header, the size it says (four 7-bit bytes) and its footer when it has one.
fn id3v2_len(head: &[u8]) -> Option<u64> {
    let h = head.get(..10)?;
    if &h[..3] != b"ID3" || h[3] == 0xFF || h[4] == 0xFF || h[6..10].iter().any(|b| b & 0x80 != 0) {
        return None;
    }
    let size = h[6..10].iter().fold(0u64, |n, &b| (n << 7) | b as u64);
    Some(10 + size + if h[5] & 0x10 != 0 { 10 } else { 0 })
}

/// [`kind`] of the open file `f`, from its first bytes: a FLAC behind an ID3v2 tag is a FLAC.
pub fn kind_of_file(f: &std::fs::File) -> std::io::Result<Option<Kind>> {
    let mut head = [0u8; HEAD];
    let n = m2_os::fs::read_full_at(f, &mut head, 0, HEAD)?;
    let Some(tag) = id3v2_len(&head[..n]) else { return Ok(kind(&head[..n])) };
    let n = m2_os::fs::read_full_at(f, &mut head[..4], tag, 4)?;
    Ok((n == 4 && &head[..4] == b"fLaC").then_some(Kind::Flac))
}

/// [`kind_of_file`] of the file at `path`.
pub fn kind_of(path: &Path) -> std::io::Result<Option<Kind>> {
    kind_of_file(&std::fs::File::open(path)?)
}

/// A stream after [`sniff`] looked at its first bytes: those bytes, then the rest.
pub type Sniffed<R> = Chain<Cursor<Vec<u8>>, R>;

/// What a stream is, from its first bytes, past an ID3v2 tag before a FLAC: its kind, and the
/// stream to read it from (a WAV's from its start; a FLAC's from `fLaC`).
pub fn sniff<R: Read>(mut r: R) -> std::io::Result<(Option<Kind>, Sniffed<R>)> {
    let mut head = vec![0u8; HEAD];
    let n = read_up_to(&mut r, &mut head)?;
    head.truncate(n);
    if let Some(tag) = id3v2_len(&head) {
        std::io::copy(&mut (&mut r).take(tag.saturating_sub(n as u64)), &mut std::io::sink())?;
        let mut after = vec![0u8; 4];
        let n = read_up_to(&mut r, &mut after)?;
        after.truncate(n);
        let k = (after == b"fLaC").then_some(Kind::Flac);
        return Ok((k, Cursor::new(after).chain(r)));
    }
    Ok((kind(&head), Cursor::new(head).chain(r)))
}

fn read_up_to(r: &mut impl Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(n)
}

/// The most bytes of samples a WAV holds: its sizes are 32-bit, less room for its header.
pub const WAV_BYTES: u64 = u32::MAX as u64 - 4096;

/// The most samples (frames × channels) one whole decode may make (FMT-24): 2^27, about 23 minutes
/// of stereo at 48 kHz, 512 MiB as the engine holds it (32-bit floats). A recording longer than
/// that waits for streaming (gaps #22).
pub const WHOLE: u64 = 1 << 27;

/// The most samples a pack's audio files may hold in all (FMT-24, PACK-12): 2^28, 1 GiB as the
/// engine holds them. Std's piano (the lite Salamander grand) holds 102,024,134.
pub const PACK: u64 = 1 << 28;

/// Why a file isn't decoded whole: it holds more samples than [`WHOLE`] (FMT-24).
#[derive(Debug)]
pub struct TooLong {
    pub samples: u64,
}

impl std::fmt::Display for TooLong {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "it holds {} samples (frames × channels), more than music2 decodes whole ({WHOLE}, about 23 minutes of stereo at 48 kHz): \
             cut it shorter or into parts; longer recordings wait for music2 to stream them from the disk",
            self.samples
        )
    }
}

impl std::error::Error for TooLong {}

/// What a file's header says, read before a sample of it is decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub kind: Kind,
    pub channels: u16,
    pub rate: u32,
    /// Bits in a sample, as stored (32 for a float WAV).
    pub bits: u16,
    pub frames: u64,
}

impl Header {
    /// Its samples: frames × channels.
    pub fn samples(&self) -> u64 {
        self.frames.saturating_mul(self.channels as u64)
    }
}

/// Whether a file whose header is `h` may be decoded whole: held to [`WHOLE`] (FMT-24).
pub fn budget(h: &Header) -> Result<()> {
    match h.samples() {
        n if n > WHOLE => Err(TooLong { samples: n }.into()),
        _ => Ok(()),
    }
}

/// What the header of the WAV or FLAC file at `path` says (FMT-24): no sample is decoded.
pub fn header_of(path: &Path) -> Result<Header> {
    match kind_of(path)? {
        Some(Kind::Wav) => {
            let r = hound::WavReader::open(path)?;
            let spec = r.spec();
            let frames = r.len() as u64 / spec.channels.max(1) as u64;
            Ok(Header { kind: Kind::Wav, channels: spec.channels, rate: spec.sample_rate, bits: spec.bits_per_sample, frames })
        }
        Some(Kind::Flac) => Ok(Flac::new(std::fs::File::open(path)?)?.header()),
        None => bail!("it isn't a WAV or FLAC file"),
    }
}

/// What an integer sample of `bits` bits is multiplied by to play: `1 / 2^(bits − 1)`, a WAV's
/// scale and a FLAC's, so a full-scale number is ±1.
pub fn scale(bits: u16) -> f32 {
    1.0 / (1u64 << (bits - 1)) as f32
}

/// Decode the WAV or FLAC file at `path` whole (integer or float, any channels; past the first two
/// dropped), at its own rate: one whose header says more than [`WHOLE`] samples is refused before
/// a sample is decoded ([`TooLong`]).
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
    let frames = r.len() as u64 / ch as u64;
    let h = Header { kind: Kind::Wav, channels: spec.channels, rate: spec.sample_rate, bits: spec.bits_per_sample, frames };
    budget(&h).with_context(|| format!("reading {}", path.display()))?;
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

/// A FLAC's stream as claxon is given it: its Vorbis comment and application blocks read as padding,
/// which claxon passes over. music2 reads neither, and claxon refuses a comment that isn't UTF-8,
/// as older taggers write them (review F4).
struct Meta<R> {
    inner: R,
    at: u64,
    /// Where the next metadata block's header starts; none once the last block began.
    next: Option<u64>,
    /// A block header read and changed, still to give.
    pending: Vec<u8>,
}

impl<R: Read> Read for Meta<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.pending.is_empty()
            && let Some(next) = self.next.filter(|&n| n == self.at)
        {
            let mut h = [0u8; 4];
            let n = read_up_to(&mut self.inner, &mut h)?;
            if n == 4 {
                let (last, kind) = (h[0] & 0x80, h[0] & 0x7F);
                if kind == 2 || kind == 4 {
                    h[0] = last | 1;
                }
                let len = u32::from_be_bytes([0, h[1], h[2], h[3]]) as u64;
                self.next = (last == 0).then_some(next + 4 + len);
            } else {
                self.next = None;
            }
            self.pending = h[..n].to_vec();
        }
        if !self.pending.is_empty() {
            let n = buf.len().min(self.pending.len());
            buf[..n].copy_from_slice(&self.pending[..n]);
            self.pending.drain(..n);
            self.at += n as u64;
            return Ok(n);
        }
        let most = match self.next {
            Some(next) => buf.len().min((next - self.at) as usize),
            None => buf.len(),
        };
        let n = self.inner.read(&mut buf[..most])?;
        self.at += n as u64;
        Ok(n)
    }
}

/// claxon's error in music2's words: a part of FLAC it doesn't decode is just that; anything else
/// is a damaged file.
fn claxon_said(e: claxon::Error, damaged: &str) -> anyhow::Error {
    match e {
        claxon::Error::Unsupported(what) => anyhow::anyhow!("it uses a part of FLAC that music2 can't decode yet ({what})"),
        claxon::Error::IoError(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => anyhow::anyhow!("it is cut short ({e})"),
        claxon::Error::IoError(e) => anyhow::Error::new(e),
        e => anyhow::anyhow!("{damaged} ({e})"),
    }
}

/// A FLAC read from its start to its end, a block at a time: its header checked when it is opened,
/// each block as it is decoded. It ends at the frames its header says: what follows (an ID3v1 tag)
/// is no audio.
pub struct Flac<R: Read> {
    reader: claxon::FlacReader<Meta<Sniffed<R>>>,
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
    /// Read the FLAC's header from `r` (past an ID3v2 tag before it), and check it (FMT-24): 8, 16
    /// or 24 bits in a sample, its length said, and no more samples than a WAV holds. Why not, in
    /// words.
    pub fn new(r: R) -> Result<Flac<R>> {
        let (_, r) = sniff(r)?;
        let meta = Meta { inner: r, at: 0, next: Some(4), pending: vec![] };
        let opts = claxon::FlacReaderOptions { metadata_only: false, read_vorbis_comment: false };
        let reader = claxon::FlacReader::new_ext(meta, opts).map_err(|e| claxon_said(e, "its header is damaged"))?;
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

    /// What its header says.
    pub fn header(&self) -> Header {
        Header { kind: Kind::Flac, channels: self.channels, rate: self.rate, bits: self.bits, frames: self.frames }
    }

    /// The next block's samples into `out`, one list for each channel (each cleared first): how many
    /// frames, 0 at the end: once the frames its header says are read, or the stream ends. One
    /// that ends before them, a block past them or with other channels, or a sample outside its
    /// bits is damaged: refused, never played.
    pub fn next(&mut self, out: &mut Vec<Vec<i32>>) -> Result<usize> {
        if self.done == self.frames {
            return Ok(0);
        }
        let buf = std::mem::take(&mut self.buf);
        let block = self.reader.blocks().read_next_or_eof(buf).map_err(|e| claxon_said(e, "it is damaged"))?;
        let Some(block) = block else {
            bail!("it is cut short: it holds {} of the {} frames its header says", self.done, self.frames);
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

    /// The stream it was read from, past what it read (a tag after the frames, still to read).
    pub fn into_inner(self) -> R {
        self.reader.into_inner().inner.into_inner().1
    }
}

fn load_flac(path: &Path) -> Result<Asset> {
    let file = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut f = Flac::new(file).with_context(|| format!("reading {}", path.display()))?;
    budget(&f.header()).with_context(|| format!("reading {}", path.display()))?;
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
/// pushed to the disk. The FLAC is given back, read to its last frame.
pub fn write_wav<R: Read, W: Write + Seek>(mut flac: Flac<R>, out: W) -> Result<Flac<R>> {
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
    Ok(flac)
}
