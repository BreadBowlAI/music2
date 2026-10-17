//! An audio file's waveform peaks (lane A's A1, Q9): the lowest and highest sample of each 256
//! frames, per channel, as `i8`, for the timeline to draw a clip's waveform from.
//!
//! Made once per file content from its decode, and kept by content (FMT-16): in memory (the
//! last [`KEPT`]) and in `<cache>/peaks/<sha256>.m2pk` beside the hash cache, so a song opened
//! again draws at once. A cache file is only ever a copy of what a decode makes: one that doesn't
//! read is made again. Made on a blocking task of the route (or the import's thread), never on
//! the session's actor, never on the audio thread.
//!
//! The file (and the route's answer) is little-endian: `M2PK`, the version (u16), channels (u16),
//! the rate (u32), frames (u64), the bucket (u32), then for each bucket, for each channel, its
//! lowest and highest sample (i8 each, a sample × 127, rounded).

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock};

use anyhow::{Context, Result, bail};
use m2_runtime::audio::Asset;
use m2_song::{HashCache, Sha};
use parking_lot::Mutex;

/// Frames a pair stands for.
pub const BUCKET: u32 = 256;
const MAGIC: &[u8; 4] = b"M2PK";
const VERSION: u16 = 1;
const HEADER: usize = 24;
/// How many files' peaks stay in memory.
const KEPT: usize = 32;

/// A file's peaks (Q9).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Peaks {
    pub channels: u16,
    pub rate: u32,
    pub frames: u64,
    pub bucket: u32,
    /// For each bucket, for each channel: its lowest, then its highest sample.
    pub data: Vec<i8>,
}

impl Peaks {
    /// From a decoded file.
    pub fn of_asset(a: &Asset) -> Peaks {
        let channels = a.channels.len().max(1);
        let frames = a.len();
        let buckets = frames.div_ceil(BUCKET as usize);
        let mut data = Vec::with_capacity(buckets * channels * 2);
        let q = |x: f32| (x.clamp(-1.0, 1.0) * 127.0).round() as i8;
        for b in 0..buckets {
            let span = b * BUCKET as usize..((b + 1) * BUCKET as usize).min(frames);
            for c in 0..channels {
                let s = a.channels.get(c).map_or(&[][..], |ch| &ch[span.clone()]);
                let (lo, hi) = s.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), &x| (lo.min(x), hi.max(x)));
                let (lo, hi) = if s.is_empty() { (0.0, 0.0) } else { (lo, hi) };
                data.extend([q(lo), q(hi)]);
            }
        }
        Peaks { channels: channels as u16, rate: a.sample_rate.round() as u32, frames: frames as u64, bucket: BUCKET, data }
    }

    /// The file's bytes, and the route's answer.
    pub fn bytes(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(HEADER + self.data.len());
        b.extend_from_slice(MAGIC);
        b.extend_from_slice(&VERSION.to_le_bytes());
        b.extend_from_slice(&self.channels.to_le_bytes());
        b.extend_from_slice(&self.rate.to_le_bytes());
        b.extend_from_slice(&self.frames.to_le_bytes());
        b.extend_from_slice(&self.bucket.to_le_bytes());
        b.extend(self.data.iter().map(|&v| v as u8));
        b
    }

    /// [`Peaks::bytes`] back, or why they aren't peaks of this version.
    pub fn read(b: &[u8]) -> Result<Peaks> {
        if b.len() < HEADER || &b[..4] != MAGIC {
            bail!("not a peaks file");
        }
        let u16_at = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
        let u32_at = |i: usize| u32::from_le_bytes(b[i..i + 4].try_into().expect("4 bytes"));
        if u16_at(4) != VERSION {
            bail!("peaks of another version");
        }
        let (channels, rate, bucket) = (u16_at(6), u32_at(8), u32_at(20));
        let frames = u64::from_le_bytes(b[12..20].try_into().expect("8 bytes"));
        if channels == 0 || bucket == 0 {
            bail!("peaks with no channels");
        }
        let want = (frames.div_ceil(bucket as u64)).checked_mul(channels as u64 * 2).context("too many peaks")?;
        if (b.len() - HEADER) as u64 != want {
            bail!("peaks cut short");
        }
        Ok(Peaks { channels, rate, frames, bucket, data: b[HEADER..].iter().map(|&v| v as i8).collect() })
    }
}

/// Peaks by content: in memory, then on disk, else made from a decode.
pub struct Store {
    /// `<cache>/peaks`, or none: kept in memory only.
    dir: Option<PathBuf>,
    kept: Mutex<VecDeque<(Sha, Arc<Peaks>)>>,
    /// One file decoded at a time: two asks for one file decode it once.
    making: Mutex<()>,
    decodes: AtomicUsize,
}

static GLOBAL: LazyLock<Store> = LazyLock::new(|| Store::new(disk()));

/// Where the server keeps peaks. Tests keep them in memory, so none writes outside its own folder.
fn disk() -> Option<PathBuf> {
    if cfg!(test) { None } else { folder() }
}

/// `<cache>/peaks`, beside the hash cache (PLAT-7: under `$HOME`).
fn folder() -> Option<PathBuf> {
    crate::resources::cache_dir().map(|d| d.join("peaks"))
}

impl Store {
    pub fn new(dir: Option<PathBuf>) -> Store {
        Store { dir, kept: Mutex::default(), making: Mutex::new(()), decodes: AtomicUsize::new(0) }
    }

    /// The server's.
    pub fn global() -> &'static Store {
        &GLOBAL
    }

    /// The peaks of the file at `path`: by its hash, from memory, the cache folder, or a decode.
    pub fn of(&self, path: &Path) -> Result<Arc<Peaks>> {
        let sha = HashCache::global().hash(path).with_context(|| format!("couldn't read {}", name(path)))?;
        if let Some(p) = self.find(sha) {
            return Ok(p);
        }
        let _one = self.making.lock();
        if let Some(p) = self.find(sha) {
            return Ok(p);
        }
        self.decodes.fetch_add(1, Ordering::Relaxed);
        let asset = m2_runtime::audio::load_wav(path).map_err(|e| anyhow::anyhow!("{} doesn't read as a WAV file ({})", name(path), e.root_cause()))?;
        Ok(self.put(sha, &asset))
    }

    /// Keep the peaks of `asset`, a decode of the file whose bytes hash to `sha` (an import
    /// decodes once: D31).
    pub fn put(&self, sha: Sha, asset: &Asset) -> Arc<Peaks> {
        let p = Arc::new(Peaks::of_asset(asset));
        if let Some(file) = self.file(sha) {
            write_whole(&file, &p.bytes());
        }
        self.keep(sha, p.clone());
        p
    }

    /// Files decoded since it was made.
    #[cfg(test)]
    pub fn decodes(&self) -> usize {
        self.decodes.load(Ordering::Relaxed)
    }

    fn file(&self, sha: Sha) -> Option<PathBuf> {
        Some(self.dir.as_ref()?.join(format!("{}.m2pk", sha.hex())))
    }

    fn find(&self, sha: Sha) -> Option<Arc<Peaks>> {
        if let Some((_, p)) = self.kept.lock().iter().find(|(s, _)| *s == sha) {
            return Some(p.clone());
        }
        let p = Arc::new(Peaks::read(&std::fs::read(self.file(sha)?).ok()?).ok()?);
        self.keep(sha, p.clone());
        Some(p)
    }

    fn keep(&self, sha: Sha, p: Arc<Peaks>) {
        let mut kept = self.kept.lock();
        kept.retain(|(s, _)| *s != sha);
        kept.push_front((sha, p));
        kept.truncate(KEPT);
    }
}

/// Write `bytes` as `file`, whole or not at all: through a file beside it, then a rename. A cache
/// file that can't be written is only made again next time.
fn write_whole(file: &Path, bytes: &[u8]) {
    let Some(dir) = file.parent() else { return };
    let part = file.with_extension(format!("{}.part", std::process::id()));
    let written = std::fs::create_dir_all(dir).is_ok() && std::fs::write(&part, bytes).is_ok() && std::fs::rename(&part, file).is_ok();
    if !written {
        let _ = std::fs::remove_file(&part);
    }
}

/// A file's name, as the words say it.
fn name(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

#[cfg(test)]
#[path = "peaks_tests.rs"]
mod tests;
