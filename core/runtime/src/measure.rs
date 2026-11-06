//! Measures (VOX-41): offline kernels that read a whole file and write numbers about it, one frame
//! of them each hop, rather than sound. `@m2/vocals:notes` finds a recorded voice's notes.
//!
//! A measure runs over the file at the file's own rate, its source as asset 0 (read from the disk,
//! never whole), its values, and no time map, for ⌈frames / hop⌉ frames. What it writes is kept in
//! the cache folder ([FMT-16]), `<cache>/measures/<hash>.f32`, each frame's numbers little-endian in
//! the order of its outputs, named by everything it was given: the file's hash, the kernel's id, its
//! module's hash, its hop and its values. It is written to a part file renamed once whole, so a
//! reader never meets half of one; one the folder can't take is made again when next asked. Nothing
//! that plays runs one: Find notes and the measure route do, off the session's lock.
//!
//! [FMT-16]: ../../../docs/architecture.md

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use m2_song::{HashCache, Sha};

use crate::offline::{self, Kernel};
use crate::packages::PackageSet;
use crate::sandbox::Held;
use crate::stream::Wav;

/// What a measure found in a file: its hop (frames of the file), the file's rate, its outputs'
/// names, and each output's numbers, one a hop.
#[derive(Clone, Debug, PartialEq)]
pub struct Measured {
    pub hop: usize,
    pub rate: f64,
    pub outputs: Vec<String>,
    pub values: Vec<Vec<f32>>,
}

impl Measured {
    /// Output `name`'s numbers, one a hop.
    pub fn output(&self, name: &str) -> Option<&[f32]> {
        let i = self.outputs.iter().position(|o| o == name)?;
        Some(&self.values[i])
    }

    /// Hops it holds.
    pub fn hops(&self) -> usize {
        self.values.first().map_or(0, Vec::len)
    }

    /// Where hop `k` is in the file, in seconds.
    pub fn seconds(&self, k: usize) -> f64 {
        (k * self.hop) as f64 / self.rate
    }
}

/// The folder measures are kept in: `<cache>/measures`; none, in memory only.
fn folder() -> Option<PathBuf> {
    m2_os::dirs::cache().map(|c| c.join("measures"))
}

/// Measure `file` (a WAV) by measure `id` of `packages`, with `values`: from the cache folder when
/// it is there, else made now and kept (VOX-41).
pub fn measure(packages: &PackageSet, id: &str, file: &Path, values: &[f64]) -> Result<Measured> {
    measure_in(folder().as_deref(), packages, id, file, values)
}

/// [`measure`], kept in `folder` (tests); none, made in memory each time.
#[doc(hidden)]
pub fn measure_in(folder: Option<&Path>, packages: &PackageSet, id: &str, file: &Path, values: &[f64]) -> Result<Measured> {
    let Some(decl) = packages.offline_decl(id).filter(|d| d.hop > 0) else {
        bail!("`{id}` isn't a measure: a measure is an offline kernel with a `hop` (VOX-41)");
    };
    let kernel = Kernel::of(packages, id).with_context(|| format!("`{id}` has no module"))?;
    let wav = Arc::new(Wav::open(file)?);
    let sha = HashCache::global().hash(file).with_context(|| format!("reading {}", file.display()))?;
    let (hop, outputs) = (decl.hop, decl.outputs.clone());
    let frames = (wav.frames() as usize).div_ceil(hop);
    let rate = wav.rate;
    let path = folder.map(|f| f.join(format!("{}.f32", key(sha, id, &kernel, hop, values).hex())));
    if let Some(found) = path.as_deref().and_then(|p| read(p, outputs.len(), frames)) {
        return Ok(Measured { hop, rate, outputs, values: found });
    }
    let mut made: Vec<Vec<f32>> = vec![Vec::with_capacity(frames); outputs.len()];
    let run = offline::Run { source: Held::Disk(wav), values, map: &[], rate, channels: outputs.len(), frames, measure: true };
    offline::run(&kernel, &run, &mut |b| {
        for (m, x) in made.iter_mut().zip(b) {
            m.extend_from_slice(x);
        }
        Ok(())
    })
    .with_context(|| format!("measuring {} by `{id}`", file.display()))?;
    if let Some(p) = &path
        && let Err(e) = keep(p, &made)
    {
        eprintln!("music2: the cache folder can't keep a measure ({e:#}): it is made again when next asked");
    }
    Ok(Measured { hop, rate, outputs, values: made })
}

/// What names a measure: everything it was given.
fn key(file: Sha, id: &str, kernel: &Kernel, hop: usize, values: &[f64]) -> Sha {
    let mut b = format!("music2 measure\n{id}\n{} {}\n{hop}\n", file.hex(), kernel.sha.hex()).into_bytes();
    for v in values {
        b.extend_from_slice(&v.to_bits().to_le_bytes());
    }
    Sha::of(&b)
}

/// The measure kept at `path`, when it is there whole.
fn read(path: &Path, outputs: usize, frames: usize) -> Option<Vec<Vec<f32>>> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() != outputs * frames * 4 {
        return None;
    }
    let mut out = vec![Vec::with_capacity(frames); outputs];
    for (i, b) in bytes.as_chunks::<4>().0.iter().enumerate() {
        out[i % outputs].push(f32::from_le_bytes(*b));
    }
    Some(out)
}

/// Keep `made` at `path`: written to a part file, renamed once whole.
fn keep(path: &Path, made: &[Vec<f32>]) -> Result<()> {
    m2_os::fs::make_dirs(m2_os::fs::folder_of(path))?;
    let frames = made.first().map_or(0, Vec::len);
    let mut bytes = Vec::with_capacity(4 * frames * made.len());
    for i in 0..frames {
        for m in made {
            bytes.extend_from_slice(&m[i].to_le_bytes());
        }
    }
    let part = path.with_extension(format!("f32.{}.part", std::process::id()));
    std::fs::write(&part, &bytes).with_context(|| format!("writing {}", part.display()))?;
    std::fs::rename(&part, path).inspect_err(|_| drop(std::fs::remove_file(&part))).with_context(|| format!("naming {}", path.display()))
}
