//! Disk streaming (RENDER-30): an audio lane's samples, worked out from its files off the sound
//! thread, a little ahead of where they play.
//!
//! A lane's time is cut into **chunks** of a quarter second (`rate / 4` frames). A chunk with a clip
//! in it is worked out whole by a **filler**: the player's disk thread live (`m2-play`'s `disk.rs`),
//! the render's own thread offline. The filler sums the lane's clips in their canonical order, each
//! read from its WAV in place (plain, pitched, reversed, faded) or from its warped render in the
//! cache folder, exactly as [`render_clip`](crate::audio::render_clip) and the old copy did, so
//! every sample keeps its bits (RENDER-1, RENDER-2). Files are read with `m2_os::fs::read_at` alone,
//! never mapped: a mapped file another program cuts short kills the app.
//!
//! The sound thread only copies, from a ring of **slots** that each hold one chunk. A slot's tag
//! says which chunk it holds, or that it is being written; its samples are `AtomicU32` bits, so a
//! read torn by a write is never undefined. The reader checks the tag before and after it copies:
//! a chunk that isn't there whole plays silence for that block and counts as a **miss**. The sound
//! thread never waits, locks or reads a file.
//!
//! A lane has `min(16, chunks with audio)` slots. A chunk with no clip in it is silence and takes no
//! slot, so a short lane is wholly resident, filled when it is built, as its renders were.

use std::fs::File;
use std::io::Read;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering, fence};
use std::sync::{Arc, Mutex, OnceLock};
use std::task::Waker;

use anyhow::{Context, Result, bail};
use hound::Sample;
use m2_song::{AudioClip, Sha, TempoMap};

use crate::audio::{AudioLane, RenderedClip};
use crate::fault::Code;

/// The most slots a lane has: 4 s of audio, 1.5 MB of stereo at 48 kHz.
pub const SLOTS: usize = 16;

/// The largest single read: a disk reads well in pieces of this size, and a piece never holds much.
pub const READ: usize = 64 * 1024;

/// A slot's tag while it holds no chunk, and while it is being written.
const EMPTY: u64 = u64::MAX;
const BUSY: u64 = u64::MAX - 1;

/// Frames in one chunk at `rate`: a quarter second.
pub fn chunk_frames(rate: f64) -> u64 {
    ((rate / 4.0).round() as u64).max(1)
}

/// A WAV file opened to be read in place: where its samples start, and how each is stored.
pub struct Wav {
    file: File,
    data: u64,
    frames: u64,
    channels: usize,
    bytes: u16,
    bits: u16,
    format: hound::SampleFormat,
    pub rate: f64,
}

/// A reader at a position of a file, through `read_at`: what `hound` parses a header from.
struct At<'a> {
    file: &'a File,
    pos: u64,
}

impl Read for At<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = m2_os::fs::read_at(self.file, buf, self.pos)?;
        self.pos += n as u64;
        Ok(n)
    }
}

impl Wav {
    /// Open `path` and read its header. A file shorter than its header says is refused, as a
    /// whole decode refused it.
    pub fn open(path: &Path) -> Result<Wav> {
        let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
        let r = hound::WavReader::new(At { file: &file, pos: 0 }).with_context(|| format!("reading {}", path.display()))?;
        let spec = r.spec();
        let (frames, samples) = (r.duration() as u64, r.len() as u64);
        let data = r.into_inner().pos;
        // The data chunk's size is the 4 bytes before its first: each sample's width comes from it,
        // as hound works it out.
        let mut size = [0u8; 4];
        if data < 4 || m2_os::fs::read_full_at(&file, &mut size, data - 4, 4)? != 4 {
            bail!("{} has no data", path.display());
        }
        let data_len = u32::from_le_bytes(size) as u64;
        let bytes = data_len.checked_div(samples).unwrap_or(spec.bits_per_sample.div_ceil(8) as u64);
        if file.metadata()?.len() < data + data_len {
            bail!("{} is cut short", path.display());
        }
        // A sample's bits must fit in its bytes, at most 32: a crafted header would make the
        // decode's scale overflow.
        let bits = spec.bits_per_sample as u64;
        if bits == 0 || bits > 32 || bits > 8 * bytes {
            bail!("{} says each sample has {bits} bits, in {bytes} bytes", path.display());
        }
        Ok(Wav {
            file,
            data,
            frames,
            channels: spec.channels.max(1) as usize,
            bytes: bytes as u16,
            bits: spec.bits_per_sample,
            format: spec.sample_format,
            rate: spec.sample_rate as f64,
        })
    }

    /// Frames in the file.
    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// The channels it plays: its first two.
    pub fn channels(&self) -> usize {
        self.channels.min(2)
    }

    /// Frames `from..to` decoded as [`load_wav`](crate::audio::load_wav) decodes them, by hound's
    /// own sample reader, into the first [`Self::channels`] of `out` (each cleared first). `buf`
    /// holds the bytes; it grows to the largest piece asked for.
    pub fn read(&self, from: u64, to: u64, buf: &mut Vec<u8>, out: &mut [Vec<f32>]) -> Result<()> {
        let frame = self.channels * self.bytes as usize;
        let n = to.saturating_sub(from) as usize;
        buf.resize(n * frame, 0);
        if m2_os::fs::read_full_at(&self.file, buf, self.data + from * frame as u64, READ)? < buf.len() {
            bail!("the file is cut short");
        }
        let keep = self.channels();
        for o in out[..keep].iter_mut() {
            o.clear();
        }
        let mut cur: &[u8] = buf;
        let (format, bytes, bits) = (self.format, self.bytes, self.bits);
        // Each frame's samples in turn: its first two kept, the rest read past.
        match format {
            hound::SampleFormat::Float => {
                for _ in 0..n {
                    for o in out[..keep].iter_mut() {
                        o.push(<f32 as Sample>::read(&mut cur, format, bytes, bits)?);
                    }
                    for _ in keep..self.channels {
                        <f32 as Sample>::read(&mut cur, format, bytes, bits)?;
                    }
                }
            }
            hound::SampleFormat::Int => {
                let scale = 1.0 / (1u64 << (bits - 1)) as f32;
                for _ in 0..n {
                    for o in out[..keep].iter_mut() {
                        o.push(<i32 as Sample>::read(&mut cur, format, bytes, bits)? as f32 * scale);
                    }
                    for _ in keep..self.channels {
                        <i32 as Sample>::read(&mut cur, format, bytes, bits)?;
                    }
                }
            }
        }
        Ok(())
    }
}

/// A warped clip's render: interleaved 32-bit floats, in the cache folder (little-endian, no
/// header), or in memory when there is no folder or it can't take the render (a full disk, no
/// permission: review sx-r1). Whole when it was found there or an offline cache made it. A live
/// cache's is made by its stretch thread while it plays ([`crate::renders`], the sx plan's D10):
/// read from its part file, which is renamed to its name once whole, or from memory, its frames
/// done so far counted, published only once they are there. A chunk is filled only once every
/// frame it reads is done.
pub(crate) struct Raw {
    /// Its file (its part file while it is made); none when it was made in memory from the start.
    file: Option<File>,
    /// Its frames in memory, read from here once set: set when it is made in memory, or when the
    /// folder stopped taking it while it was made, after the frames in the file were copied in.
    mem: OnceLock<Box<[AtomicU32]>>,
    channels: usize,
    frames: usize,
    /// Frames in the file that may be read: all of them once it is whole.
    done: AtomicUsize,
    /// It will never be whole: stopped (a prune dropped it) or failed. What isn't done is silence.
    over: AtomicBool,
    /// Its stretch failed: each lane that plays it reports `stretch.failed` (RENDER-25).
    failed: AtomicBool,
    /// Asked to stop: no lane the cache keeps plays it any more.
    stop: AtomicBool,
    /// Who to wake as more is done: the disk threads of the players that play it.
    wakers: Mutex<Vec<Waker>>,
}

impl Raw {
    fn of(file: Option<File>, channels: usize, frames: usize, done: usize) -> Raw {
        let (over, failed, stop, wakers) = (AtomicBool::new(false), AtomicBool::new(false), AtomicBool::new(false), Mutex::default());
        Raw { file, mem: OnceLock::new(), channels, frames, done: AtomicUsize::new(done), over, failed, stop, wakers }
    }

    /// The render at `path`, when it is there whole.
    pub(crate) fn open(path: &Path, channels: usize, frames: usize) -> Option<Raw> {
        let file = File::open(path).ok()?;
        (file.metadata().ok()?.len() == (frames * channels * 4) as u64).then(|| Raw::of(Some(file), channels, frames, frames))
    }

    /// The render `part` is making, read as it is made: none of it done yet. (Windows, later: the
    /// part file is opened with delete sharing in `m2-os`, so it can be renamed while read, PLAT-2.)
    fn reading(part: &Part) -> Result<Raw> {
        let file = File::open(&part.part).with_context(|| format!("opening {}", part.part.display()))?;
        Ok(Raw::of(Some(file), part.channels, part.frames, 0))
    }

    /// A render made in memory: none of it done yet.
    fn in_memory(channels: usize, frames: usize) -> Raw {
        let raw = Raw::of(None, channels, frames, 0);
        drop(raw.mem.set((0..frames * channels).map(|_| AtomicU32::new(0)).collect()));
        raw
    }

    /// Its channels and frames.
    pub(crate) fn size(&self) -> (usize, usize) {
        (self.channels, self.frames)
    }

    /// Whether its frames before `to` can be read: done, or never to be (then silence).
    pub(crate) fn has(&self, to: usize) -> bool {
        to <= self.done.load(Ordering::Acquire) || self.over.load(Ordering::Acquire)
    }

    /// Whether it is whole.
    pub(crate) fn whole(&self) -> bool {
        self.done.load(Ordering::Acquire) == self.frames
    }

    /// Its first `done` frames are in the file: readers may read them, and are woken.
    pub(crate) fn made(&self, done: usize) {
        self.done.store(done, Ordering::Release);
        self.wake(done == self.frames);
    }

    /// It will never be whole: what isn't done plays silence from now on.
    pub(crate) fn end(&self) {
        self.over.store(true, Ordering::Release);
        self.wake(true);
    }

    /// Its stretch failed: it ends, and the lanes that play it say so.
    pub(crate) fn fail(&self) {
        self.failed.store(true, Ordering::Release);
        self.end();
    }

    pub(crate) fn failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }

    fn wake(&self, last: bool) {
        let mut w = self.wakers.lock().unwrap_or_else(|e| e.into_inner());
        w.iter().for_each(Waker::wake_by_ref);
        if last {
            w.clear();
        }
    }

    /// Wake `w` as more of it is done, till it is whole or over.
    pub(crate) fn watch(&self, w: &Waker) {
        let mut ws = self.wakers.lock().unwrap_or_else(|e| e.into_inner());
        if !self.whole() && !self.over.load(Ordering::Acquire) && !ws.iter().any(|x| x.will_wake(w)) {
            ws.push(w.clone());
        }
    }

    /// Ask it to stop: its stretch thread ends it, and removes its part file.
    pub(crate) fn stop(&self) {
        self.stop.store(true, Ordering::Release);
    }

    pub(crate) fn stopped(&self) -> bool {
        self.stop.load(Ordering::Acquire)
    }

    /// Frames `from..to` into `out`, a channel each; those not done (only when it is over) are
    /// silence.
    pub(crate) fn read(&self, from: usize, to: usize, buf: &mut Vec<u8>, out: &mut [Vec<f32>]) -> Result<()> {
        let (ch, frame) = (self.channels, self.channels * 4);
        let have = to.min(self.done.load(Ordering::Acquire)).max(from);
        for o in out[..ch].iter_mut() {
            o.clear();
        }
        if let Some(m) = self.mem.get() {
            for f in m[from * ch..have * ch].chunks_exact(ch) {
                for (o, s) in out.iter_mut().zip(f) {
                    o.push(f32::from_bits(s.load(Ordering::Relaxed)));
                }
            }
        } else {
            let file = self.file.as_ref().context("the render has no file")?;
            buf.resize((have - from) * frame, 0);
            if m2_os::fs::read_full_at(file, buf, (from * frame) as u64, READ)? < buf.len() {
                bail!("the render is cut short");
            }
            for f in buf.chunks_exact(frame) {
                for (c, s) in f.as_chunks::<4>().0.iter().enumerate() {
                    out[c].push(f32::from_le_bytes(*s));
                }
            }
        }
        for o in out[..self.channels].iter_mut() {
            o.resize(to - from, 0.0);
        }
        Ok(())
    }
}

/// A render being written to the cache folder, a piece at a time, never whole in memory: to a part
/// file, renamed to its name once whole, so a reader never meets half of one. Two programs that
/// render the same clip write the same bytes. A part left unfinished is removed.
struct Part {
    file: File,
    part: PathBuf,
    path: PathBuf,
    channels: usize,
    frames: usize,
    /// Frames put so far, and the bytes of those not yet written.
    put: usize,
    bytes: Vec<u8>,
    done: bool,
}

impl Part {
    /// A part file for the render at `path`: `frames` frames of `channels`, put in order.
    fn create(path: &Path, channels: usize, frames: usize) -> Result<Part> {
        static N: AtomicUsize = AtomicUsize::new(0);
        m2_os::fs::make_dirs(m2_os::fs::folder_of(path))?;
        let part = path.with_extension(format!("f32.{}.{}.part", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let file = File::create(&part).with_context(|| format!("creating {}", part.display()))?;
        Ok(Part { file, part, path: path.to_path_buf(), channels, frames, put: 0, bytes: Vec::with_capacity(READ), done: false })
    }

    /// Frames in the file so far: those put, less those not yet written.
    fn written(&self) -> usize {
        self.put - self.bytes.len() / (4 * self.channels)
    }

    /// The next frames, a slice a channel, all as long ([`Making::put`] checks them). A write that
    /// fails keeps them, and every frame not yet written, in `bytes`.
    fn put(&mut self, block: &[&[f32]]) -> Result<()> {
        let n = block.first().map_or(0, |c| c.len());
        for i in 0..n {
            for c in block {
                self.bytes.extend_from_slice(&c[i].to_le_bytes());
            }
        }
        self.put += n;
        if self.bytes.len() >= READ {
            self.flush()?;
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        use std::io::Write;
        self.file.write_all(&self.bytes).with_context(|| format!("writing {}", self.part.display()))?;
        self.bytes.clear();
        Ok(())
    }

    /// Done: the rest written, and the file renamed to its name. A reader of the part file reads
    /// on, the same file.
    fn finish(&mut self) -> Result<()> {
        self.flush()?;
        std::fs::rename(&self.part, &self.path).with_context(|| format!("naming {}", self.path.display()))?;
        self.done = true;
        Ok(())
    }
}

impl Drop for Part {
    fn drop(&mut self) {
        if !self.done {
            drop(std::fs::remove_file(&self.part));
        }
    }
}

/// A render being made, a block at a time, and where it goes: its part file in the cache folder,
/// or memory when there is no folder or the folder can't take it, at the start or partway (a full
/// disk, no permission: review sx-r1). Its [`Raw`] reads it as it is made.
pub(crate) struct Making {
    part: Option<Part>,
    /// Frames put so far, in memory.
    put: usize,
    raw: Arc<Raw>,
}

impl Making {
    /// A render of `frames` frames of `channels` for `path` in the cache folder; none, in memory.
    pub(crate) fn new(path: Option<&Path>, channels: usize, frames: usize) -> Making {
        let file = path.map(|p| Part::create(p, channels, frames).and_then(|part| Ok((Raw::reading(&part)?, part))));
        let (part, raw) = match file {
            Some(Ok((raw, part))) => (Some(part), raw),
            Some(Err(e)) => {
                eprintln!("music2: the cache folder can't take a stretch ({e:#}): it is made in memory");
                (None, Raw::in_memory(channels, frames))
            }
            None => (None, Raw::in_memory(channels, frames)),
        };
        Making { part, put: 0, raw: Arc::new(raw) }
    }

    /// What reads it as it is made.
    pub(crate) fn raw(&self) -> &Arc<Raw> {
        &self.raw
    }

    /// Frames there so far, which readers may be told of: those in the file, or in memory.
    pub(crate) fn written(&self) -> usize {
        self.part.as_ref().map_or(self.put, Part::written)
    }

    /// Frames put so far.
    pub(crate) fn frames_put(&self) -> usize {
        self.part.as_ref().map_or(self.put, |p| p.put)
    }

    /// The next frames, a slice a channel, all as long.
    pub(crate) fn put(&mut self, block: &[&[f32]]) -> Result<()> {
        let (ch, frames) = self.raw.size();
        let n = block.first().map_or(0, |c| c.len());
        let at = self.frames_put();
        if at + n > frames || block.len() != ch {
            bail!("a render of {frames} frames of {ch} channels was given {} of {}", at + n, block.len());
        }
        if let Some(p) = &mut self.part {
            return match p.put(block) {
                Ok(()) => Ok(()),
                Err(e) => self.go_in_memory(e),
            };
        }
        let m = self.raw.mem.get().context("a render with no file is in memory")?;
        for (c, b) in block.iter().enumerate() {
            for (i, x) in b.iter().enumerate() {
                m[(at + i) * ch + c].store(x.to_bits(), Ordering::Relaxed);
            }
        }
        self.put = at + n;
        Ok(())
    }

    /// Every frame put is there, so [`Self::written`] counts them all.
    pub(crate) fn flush(&mut self) -> Result<()> {
        match self.part.as_mut().map(Part::flush) {
            Some(Err(e)) => self.go_in_memory(e),
            _ => Ok(()),
        }
    }

    /// Done: every frame there, its file renamed to its name; readers may read it all.
    pub(crate) fn finish(mut self) -> Result<()> {
        let frames = self.raw.frames;
        if self.frames_put() != frames {
            bail!("a render of {frames} frames ended at {}", self.frames_put());
        }
        if let Some(Err(e)) = self.part.as_mut().map(Part::finish) {
            self.go_in_memory(e)?;
        }
        self.raw.made(frames);
        Ok(())
    }

    /// The folder stopped taking it (`why`): what is there so far goes to memory, the frames in
    /// the file read back, then those not written yet; the part file goes, and the rest is made in
    /// memory. A reader reads the same frames from either.
    fn go_in_memory(&mut self, why: anyhow::Error) -> Result<()> {
        let Some(part) = self.part.take() else { return Err(why) };
        eprintln!("music2: the cache folder stopped taking a stretch ({why:#}): the rest is made in memory");
        let (ch, written) = (part.channels, part.written());
        let file = self.raw.file.as_ref().context("a render with a part file reads it")?;
        let mut m: Vec<AtomicU32> = Vec::with_capacity(part.frames * ch);
        let (mut buf, total) = (vec![0u8; READ], (written * ch * 4) as u64);
        let mut at = 0;
        while at < total {
            let n = (total - at).min(READ as u64) as usize;
            if m2_os::fs::read_full_at(file, &mut buf[..n], at, READ)? < n {
                bail!("the part file is cut short");
            }
            m.extend(buf[..n].as_chunks::<4>().0.iter().map(|b| AtomicU32::new(u32::from_le_bytes(*b))));
            at += n as u64;
        }
        m.extend(part.bytes.as_chunks::<4>().0.iter().map(|b| AtomicU32::new(u32::from_le_bytes(*b))));
        m.resize_with(part.frames * ch, AtomicU32::default);
        self.put = part.put;
        if self.raw.mem.set(m.into_boxed_slice()).is_err() {
            bail!("a render with a part file was in memory already");
        }
        Ok(())
    }

    /// A test seam: from now on, writing its part file fails, as on a full disk.
    #[cfg(test)]
    pub(crate) fn break_disk(&mut self) {
        if let Some(p) = &mut self.part {
            p.file = File::open(&p.part).unwrap();
        }
    }
}

/// Where a warped clip's stretch is kept in `folder` (`<cache>/renders`, FMT-16's), named by
/// everything the kernel was given: its key holds the stretch kernel's module hash (the sx plan's
/// D8), so a new std makes new renders, and the time map itself, so a new way of making the map
/// does too.
pub(crate) fn render_path(folder: &Path, key: &[u8]) -> PathBuf {
    let mut named = b"music2 stretch\n".to_vec();
    named.extend_from_slice(key);
    folder.join(format!("{}.f32", Sha::of(&named).hex()))
}

/// How a clip is worked out from its file, or from its stretch: [`render_clip`]'s numbers (a
/// stretch reads only its length, reverse, gain and fades).
///
/// [`render_clip`]: crate::audio::render_clip
#[derive(Clone, Copy, Debug)]
pub(crate) struct Shape {
    pub(crate) offset: f64,
    pub(crate) step: f64,
    pub(crate) len: usize,
    pub(crate) reverse: bool,
    pub(crate) gain: f32,
    pub(crate) fade_in: usize,
    pub(crate) fade_out: usize,
}

impl Shape {
    pub(crate) fn of(clip: &AudioClip, src_rate: f64, tempo: &TempoMap, sample_rate: f64) -> Shape {
        // exp2 by name, so debug and release builds call one function: a release build turns
        // `2f64.powf(x)` into exp2 where a debug build calls pow, and libm's two differ in the last bit.
        let rate = libm::exp2(clip.pitch / 12.0);
        Shape {
            offset: clip.offset * src_rate,
            step: rate * src_rate / sample_rate,
            len: clip_len(clip, tempo, sample_rate),
            reverse: clip.reverse,
            gain: libm::powf(10.0, clip.gain_db as f32 / 20.0),
            fade_in: (clip.fade_in * sample_rate) as usize,
            fade_out: (clip.fade_out * sample_rate) as usize,
        }
    }

    /// The source position output sample `j` (before reversing) reads.
    fn pos(&self, j: usize) -> f64 {
        self.offset + j as f64 * self.step
    }

    /// The frames of a made render (a stretch) its samples `i0..i1` read: from its end, reversed.
    fn span(&self, i0: usize, i1: usize) -> (usize, usize) {
        if self.reverse { (self.len - i1, self.len - i0) } else { (i0, i1) }
    }

    /// Its samples `at` into `out`, a channel each, from `win`, the render's frames from `j0` on:
    /// reversed, faded and gained, as a whole render would have them.
    fn place(&self, win: &[Vec<f32>], j0: usize, at: Range<usize>, out: &mut [Vec<f32>]) {
        let n = self.len;
        for (win, out) in win.iter().zip(out.iter_mut()) {
            out.clear();
            for i in at.clone() {
                let j = if self.reverse { n - 1 - i } else { i };
                out.push(win[j - j0] * gain_at(i, n, self.gain, self.fade_in, self.fade_out));
            }
        }
    }
}

/// A clip's length in frames at `sample_rate`.
pub(crate) fn clip_len(clip: &AudioClip, tempo: &TempoMap, sample_rate: f64) -> usize {
    let t0 = tempo.seconds_at(clip.start);
    ((tempo.seconds_at(clip.start + clip.length) - t0) * sample_rate).round() as usize
}

/// The gain of output sample `i` of `n`: the clip's gain and its fades.
pub(crate) fn gain_at(i: usize, n: usize, gain: f32, fade_in: usize, fade_out: usize) -> f32 {
    let mut g = gain;
    if i < fade_in {
        g *= 0.5 - 0.5 * libm::cosf(std::f32::consts::PI * i as f32 / fade_in as f32);
    }
    if n - i <= fade_out {
        g *= 0.5 - 0.5 * libm::cosf(std::f32::consts::PI * (n - i) as f32 / fade_out as f32);
    }
    g
}

/// Read a signal of `len` samples at fractional position `pos` (cubic Hermite), silence outside,
/// from `x`, its samples from `base` on: every sample the read needs must be in `x`.
pub(crate) fn read_in(x: &[f32], base: usize, len: usize, pos: f64) -> f32 {
    if pos < 0.0 || pos > (len as f64 - 1.0) {
        return 0.0;
    }
    let i = pos.floor() as isize;
    let t = (pos - i as f64) as f32;
    let at = |k: isize| if k < 0 || k as usize >= len { 0.0 } else { x[k as usize - base] };
    let (y0, y1, y2, y3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
    let c1 = 0.5 * (y2 - y0);
    let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
    let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
    ((c3 * t + c2) * t + c1) * t + y1
}

/// Where a clip's samples come from.
pub(crate) enum Source {
    /// Its WAV, worked out in place.
    Wav(Arc<Wav>, Shape),
    /// A warped clip's stretch in the cache folder, reversed, faded and gained in place.
    Render(Arc<Raw>, Shape),
    /// A render given whole in memory, reversed, faded and gained in place: tests' renders
    /// ([`AudioLane::of_renders`]).
    Mem(Arc<RenderedClip>, Shape),
}

/// One clip of a lane: where it starts, in the lane's frames, how long it is and what it reads.
pub(crate) struct Clip {
    pub(crate) at: u64,
    pub(crate) len: u64,
    pub(crate) channels: usize,
    pub(crate) src: Source,
    /// Its place among the lane's clips in the song's data: what a fault names ([`crate::fault::Subject::Clip`]).
    pub(crate) index: u32,
}

impl Clip {
    pub(crate) fn new(at: u64, src: Source, len: usize, index: u32) -> Clip {
        let channels = match &src {
            Source::Wav(w, _) => w.channels(),
            Source::Render(r, _) => r.channels,
            Source::Mem(m, _) => m.len(),
        };
        Clip { at, len: len as u64, channels, src, index }
    }

    /// Whether it plays a stretch that failed.
    fn stretch_failed(&self) -> bool {
        matches!(&self.src, Source::Render(r, _) if r.failed())
    }

    /// Whether its samples `i0..i1` can be worked out now: a render being made has made every
    /// frame they read.
    fn has(&self, i0: usize, i1: usize) -> bool {
        match &self.src {
            Source::Render(r, sh) => r.has(sh.span(i0, i1).1),
            Source::Wav(..) | Source::Mem(..) => true,
        }
    }

    /// Its samples `i0..i1` (from its start), each channel into `s.vals`, as the clip's whole
    /// render has them.
    fn piece(&self, i0: usize, i1: usize, s: &mut Scratch) -> Result<()> {
        let ch = self.channels;
        match &self.src {
            Source::Render(r, sh) => {
                let (j0, j1) = sh.span(i0, i1);
                r.read(j0, j1, &mut s.bytes, &mut s.win)?;
                sh.place(&s.win[..ch], j0, i0..i1, &mut s.vals);
            }
            Source::Mem(d, sh) => {
                let (j0, j1) = sh.span(i0, i1);
                for (w, d) in s.win.iter_mut().zip(d.iter()) {
                    w.clear();
                    w.extend_from_slice(&d[j0..j1]);
                }
                sh.place(&s.win[..ch], j0, i0..i1, &mut s.vals);
            }
            Source::Wav(w, sh) => {
                let n = sh.len;
                let (j0, j1) = if sh.reverse { (n - i1, n - i0) } else { (i0, i1) };
                // The source frames the cubic reads at these positions touch, within the file.
                let frames = w.frames as i64;
                let lo = (sh.pos(j0).floor() as i64).saturating_sub(1).clamp(0, frames);
                let hi = (sh.pos(j1 - 1).floor() as i64).saturating_add(3).clamp(lo, frames);
                w.read(lo as u64, hi as u64, &mut s.bytes, &mut s.win)?;
                for (win, out) in s.win.iter().zip(s.vals.iter_mut()).take(ch) {
                    out.clear();
                    for i in i0..i1 {
                        let j = if sh.reverse { n - 1 - i } else { i };
                        let x = read_in(win, lo as usize, w.frames as usize, sh.pos(j));
                        out.push(x * gain_at(i, n, sh.gain, sh.fade_in, sh.fade_out));
                    }
                }
            }
        }
        Ok(())
    }
}

/// A filler's working buffers: one lane's, kept between chunks.
#[derive(Default)]
pub(crate) struct Scratch {
    bytes: Vec<u8>,
    win: [Vec<f32>; 2],
    vals: [Vec<f32>; 2],
    chunk: Vec<f32>,
}

impl Scratch {
    fn bytes(&self) -> usize {
        self.bytes.capacity() + 4 * (self.win.iter().chain(&self.vals).map(Vec::capacity).sum::<usize>() + self.chunk.capacity())
    }
}

/// One chunk's room in the ring.
struct Slot {
    tag: AtomicU64,
    /// Channel-major, `channels × chunk`.
    data: Box<[AtomicU32]>,
}

/// A lane's ring of slots, and what the sound thread and the fillers share about it.
pub(crate) struct Ring {
    chunk: u64,
    channels: usize,
    slots: Box<[Slot]>,
    /// The chunks with audio, as ranges of chunk numbers in order.
    spans: Vec<Range<u64>>,
    audio_chunks: u64,
    /// The next slot an offline filler takes when every slot holds a chunk.
    cursor: AtomicUsize,
    misses: AtomicU64,
    /// The fillers' lock: never taken by the sound thread.
    fill: Mutex<Scratch>,
}

impl Ring {
    pub(crate) fn new(clips: &[Clip], channels: usize, rate: f64) -> Ring {
        let chunk = chunk_frames(rate);
        let mut spans: Vec<Range<u64>> = clips.iter().filter(|c| c.len > 0).map(|c| c.at / chunk..(c.at + c.len).div_ceil(chunk)).collect();
        spans.sort_by_key(|r| r.start);
        let mut merged: Vec<Range<u64>> = vec![];
        for s in spans {
            match merged.last_mut() {
                Some(m) if s.start <= m.end => m.end = m.end.max(s.end),
                _ => merged.push(s),
            }
        }
        let audio_chunks = merged.iter().map(|r| r.end - r.start).sum::<u64>();
        let n = (audio_chunks as usize).min(SLOTS);
        let slots = (0..n)
            .map(|_| Slot { tag: AtomicU64::new(EMPTY), data: (0..channels * chunk as usize).map(|_| AtomicU32::new(0)).collect() })
            .collect();
        Ring {
            chunk,
            channels,
            slots,
            spans: merged,
            audio_chunks,
            cursor: AtomicUsize::new(0),
            misses: AtomicU64::new(0),
            fill: Mutex::new(Scratch::default()),
        }
    }

    pub(crate) fn has_audio(&self, k: u64) -> bool {
        let i = self.spans.partition_point(|r| r.end <= k);
        self.spans.get(i).is_some_and(|r| r.start <= k)
    }

    pub(crate) fn resident(&self) -> bool {
        self.audio_chunks as usize <= SLOTS
    }

    fn slot_of(&self, k: u64) -> Option<&Slot> {
        self.slots.iter().find(|s| s.tag.load(Ordering::Acquire) == k)
    }

    /// Copy frames `from..from + n` of chunk `k` into `out` (channel-major, `block` frames a
    /// channel) at frame `off`. False, with what it copied left to be cleared, when the chunk
    /// isn't there whole. Never waits, locks or allocates.
    fn copy(&self, k: u64, from: usize, n: usize, out: &mut [f32], block: usize, off: usize) -> bool {
        let Some(slot) = self.slot_of(k) else { return false };
        let chunk = self.chunk as usize;
        for c in 0..self.channels {
            let src = &slot.data[c * chunk + from..c * chunk + from + n];
            for (o, s) in out[c * block + off..c * block + off + n].iter_mut().zip(src) {
                *o = f32::from_bits(s.load(Ordering::Relaxed));
            }
        }
        fence(Ordering::Acquire);
        slot.tag.load(Ordering::Relaxed) == k
    }

    /// Put `samples` (channel-major) in slot `i` as chunk `k`: marked busy first, so a reader that
    /// meets it mid-write sees a miss.
    fn store(&self, i: usize, k: u64, samples: &[f32]) {
        let slot = &self.slots[i];
        slot.tag.store(BUSY, Ordering::Relaxed);
        fence(Ordering::Release);
        for (d, s) in slot.data.iter().zip(samples) {
            d.store(s.to_bits(), Ordering::Relaxed);
        }
        slot.tag.store(k, Ordering::Release);
    }
}

impl AudioLane {
    /// Frames in a chunk.
    pub fn chunk(&self) -> u64 {
        self.ring.chunk
    }

    /// Whether chunk `k` has a clip in it: one without is silence, and never takes a slot.
    pub fn has_audio(&self, k: u64) -> bool {
        self.ring.has_audio(k)
    }

    /// Whether every chunk with audio fits its slots: the whole lane stays.
    pub fn resident(&self) -> bool {
        self.ring.resident()
    }

    /// How many chunks it can hold at once.
    pub fn slots(&self) -> usize {
        self.ring.slots.len()
    }

    /// Whether chunk `k` can be played: silence, or in a slot whole.
    pub fn ready(&self, k: u64) -> bool {
        !self.has_audio(k) || self.ring.slot_of(k).is_some()
    }

    /// Whether frames `start..start + n` can be played.
    pub fn ready_for(&self, start: u64, n: u64) -> bool {
        let c = self.ring.chunk;
        (start / c..(start + n).div_ceil(c)).all(|k| self.ready(k))
    }

    /// Lane frame of `beat`, in the tempo map it was built in.
    pub fn frame_of(&self, beat: f64) -> u64 {
        crate::transport::frame_of(&self.tempo, beat, self.rate)
    }

    /// The rate it plays at.
    pub fn rate(&self) -> f64 {
        self.rate
    }

    /// Blocks that met a chunk not there, in all: each played silence (D7).
    pub fn misses(&self) -> u64 {
        self.ring.misses.load(Ordering::Relaxed)
    }

    /// The memory its samples take: the ring, and its filler's buffers.
    pub fn memory(&self) -> usize {
        let ring = self.ring.slots.iter().map(|s| s.data.len() * 4).sum::<usize>();
        ring + self.ring.fill.lock().map_or(0, |s| s.bytes())
    }

    /// Whether chunk `k` can be worked out now: every render being made that it reads has made
    /// the frames it needs (the sx plan's D10). Till then it stays out of the ring: the sound thread
    /// plays silence there and counts a miss.
    pub fn can_fill(&self, k: u64) -> bool {
        let chunk = self.ring.chunk;
        let (start, end) = (k * chunk, (k + 1) * chunk);
        self.clips.iter().all(|c| {
            c.at >= end || c.at + c.len <= start || c.has((start.max(c.at) - c.at) as usize, (end.min(c.at + c.len) - c.at) as usize)
        })
    }

    /// Wake `w` each time a render it plays makes more frames, till each is whole: a player's disk
    /// thread then fills what it couldn't before.
    pub fn watch(&self, w: &std::task::Waker) {
        for c in &self.clips {
            if let Source::Render(r, _) = &c.src {
                r.watch(w);
            }
        }
    }

    /// Work out chunk `k` and put it in a slot: an empty one, else one whose chunk `keep` doesn't
    /// want (from the ring's cursor on). False when every slot holds a chunk it wants. A chunk that
    /// can't be worked out yet ([`Self::can_fill`]) is left out. A file that can't be read gives
    /// silence there, and `asset.unreadable` is reported for its clip; a stretch that failed,
    /// `stretch.failed` ([`Self::fault`]). Off the sound thread only: the disk thread's, an offline
    /// render's, or a build's.
    pub fn fill_chunk(&self, k: u64, keep: &dyn Fn(u64) -> bool) -> bool {
        let r = &self.ring;
        if r.slots.is_empty() || !r.has_audio(k) || !self.can_fill(k) {
            return true;
        }
        let mut s = r.fill.lock().unwrap_or_else(|e| e.into_inner());
        if r.slot_of(k).is_some() {
            return true;
        }
        let n = r.slots.len();
        let victim = r.slots.iter().position(|s| s.tag.load(Ordering::Relaxed) == EMPTY).or_else(|| {
            let from = r.cursor.load(Ordering::Relaxed);
            (0..n).map(|d| (from + d) % n).find(|&i| !keep(r.slots[i].tag.load(Ordering::Relaxed)))
        });
        let Some(i) = victim else { return false };
        r.cursor.store((i + 1) % n, Ordering::Relaxed);
        if let Err(clip) = self.sum(k, &mut s) {
            s.chunk.fill(0.0);
            self.fault(Code::Unreadable, clip);
        }
        let (start, end) = (k * r.chunk, (k + 1) * r.chunk);
        for c in self.clips.iter().filter(|c| c.at < end && c.at + c.len > start && c.stretch_failed()) {
            self.fault(Code::StretchFailed, c.index);
        }
        r.store(i, k, &s.chunk);
        true
    }

    /// Sum chunk `k` of every clip, in order, into `s.chunk` (channel-major): what the old copy
    /// added up for each of its frames. Err: the place of the clip whose file couldn't be read.
    fn sum(&self, k: u64, s: &mut Scratch) -> Result<(), u32> {
        let chunk = self.ring.chunk;
        let (start, end) = (k * chunk, (k + 1) * chunk);
        let mut acc = std::mem::take(&mut s.chunk);
        acc.clear();
        acc.resize(self.channels * chunk as usize, 0.0);
        let summed = (|| {
            for clip in &self.clips {
                if clip.at >= end || clip.at + clip.len <= start {
                    continue;
                }
                let (from, to) = (start.max(clip.at), end.min(clip.at + clip.len));
                clip.piece((from - clip.at) as usize, (to - clip.at) as usize, s).map_err(|_| clip.index)?;
                let data = &s.vals;
                let mono_down = self.channels == 1 && clip.channels > 1;
                for c in 0..self.channels {
                    let src = &data[c.min(clip.channels - 1)];
                    let out = &mut acc[c * chunk as usize..];
                    for t in from..to {
                        let i = (t - from) as usize;
                        let v = if mono_down { 0.5 * (data[0][i] + data[1][i]) } else { src[i] };
                        out[(t - start) as usize] += v;
                    }
                }
            }
            Ok(())
        })();
        s.chunk = acc;
        summed
    }

    /// Make sure frames `start..start + n` are in the ring, working out what isn't, on this thread:
    /// an offline render's fill before each block (D10). A chunk taken for it is one no other
    /// part of the block reads (`block`).
    pub(crate) fn ensure(&self, start: u64, n: u64, block: &[u64]) {
        let c = self.ring.chunk;
        for k in start / c..(start + n).div_ceil(c) {
            if self.has_audio(k) && self.ring.slot_of(k).is_none() {
                self.fill_chunk(k, &|x| block.contains(&x));
            }
        }
    }

    /// Fill every chunk with audio: a resident lane, as it is built.
    pub(crate) fn fill_all(&self) {
        if !self.resident() {
            return;
        }
        for r in self.ring.spans.clone() {
            for k in r {
                self.fill_chunk(k, &|_| true);
            }
        }
    }

    /// Fill frames `off..off + n` of `out` (channel-major, `channels × block`) with the lane's
    /// samples `start..start + n`: copied from the ring, silence where a chunk isn't there (a
    /// miss). Never waits, locks, reads a file or allocates.
    pub fn fill(&self, out: &mut [f32], block: usize, off: usize, start: u64, n: usize) {
        for c in 0..self.channels {
            out[c * block + off..c * block + off + n].fill(0.0);
        }
        let chunk = self.ring.chunk;
        let end = start + n as u64;
        let mut t = start;
        while t < end {
            let k = t / chunk;
            let to = end.min((k + 1) * chunk);
            let (o, m) = (off + (t - start) as usize, (to - t) as usize);
            if self.has_audio(k) && !self.ring.copy(k, (t - k * chunk) as usize, m, out, block, o) {
                for c in 0..self.channels {
                    out[c * block + o..c * block + o + m].fill(0.0);
                }
                self.ring.misses.fetch_add(1, Ordering::Relaxed);
            }
            t = to;
        }
    }
}
