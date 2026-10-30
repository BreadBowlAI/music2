//! A recorded take's file (the io plan's D17 and D18; WRITE-1, FMT-14): a 32-bit float WAV that
//! grows in the song's `assets/` while it records.
//!
//! [`Growing::new`] makes a hidden part file there, create-only, and locks it at once, as an import
//! does (`.Vox take.wav.<pid>.part`, or `.Vox take.wav.2.<pid>.part` while the take before on the
//! track is still written), so no sweep takes it. [`Growing::append`] adds frames; every
//! write leaves a WAV whose header says what it holds so far. [`Growing::finish`] finishes the
//! header, pushes the bytes to the disk, names the file create-only (`Vox take.wav`, numbered when
//! taken in any capitals: `Vox take 2.wav`) and syncs the folder. Only then may a Tx name it. A
//! take let go unfinished (cancelled, or its song closed) leaves nothing. After a failed write (a
//! full disk), [`Growing::finish`] keeps what was written before it; and a take is never longer
//! than a WAV can say ([`Growing::room`]).

use std::fs::File;
use std::io::{BufWriter, ErrorKind, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use m2_song::path::ASSETS;
use m2_song::{AssetName, Sha};

use crate::EditError;
use crate::import::{Imported, assets_in};
use crate::render::{name_new, part_path, sweep};

type Writer = hound::WavWriter<BufWriter<Out>>;

/// Bytes kept for the header under a WAV's 32-bit sizes.
const HEADROOM: u64 = 4096;

/// The part file as hound writes it: one that can't grow past `limit` bytes, as a full disk can't
/// (tests set it; it is never reached otherwise).
struct Out {
    file: File,
    at: u64,
    limit: Arc<AtomicU64>,
}

impl Write for Out {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let room = self.limit.load(Ordering::Relaxed).saturating_sub(self.at);
        if room == 0 && !buf.is_empty() {
            return Err(ErrorKind::StorageFull.into());
        }
        let n = self.file.write(&buf[..buf.len().min(usize::try_from(room).unwrap_or(usize::MAX))])?;
        self.at += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

impl Seek for Out {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        self.at = self.file.seek(to)?;
        Ok(self.at)
    }
}

/// A take's file while it records.
pub struct Growing {
    folder: PathBuf,
    part: PathBuf,
    wanted: AssetName,
    /// Unset once finished.
    writer: Option<Writer>,
    /// The part's lock, held until it is named.
    _lock: File,
    channels: u16,
    rate: u32,
    frames: u64,
    limit: Arc<AtomicU64>,
    /// A write failed: nothing more is added.
    broken: bool,
}

fn io(e: impl std::fmt::Display) -> EditError {
    EditError::Io(format!("couldn't write the recording: {e}"))
}

impl Growing {
    /// A new take of `channels` (1 or 2) at `rate` for the track `label`, in `dir`'s `assets/`.
    pub fn new(dir: &Path, label: &str, channels: u16, rate: u32) -> Result<Growing, EditError> {
        let folder = dir.join(ASSETS);
        m2_os::fs::make_dirs(&folder).map_err(io)?;
        let wanted = AssetName::for_import(&format!("{} take.wav", label.trim()));
        sweep(&folder);
        // The take before on the track may still be written, or be on its way out: its part file
        // is there, and this one is numbered past it.
        let (part, file) = 'made: {
            for n in 1..100 {
                let name = if n == 1 { wanted.as_str().to_string() } else { format!("{}.{n}", wanted.as_str()) };
                let part = part_path(&folder.join(name)).map_err(io)?;
                match std::fs::OpenOptions::new().write(true).create_new(true).open(&part) {
                    Ok(file) => break 'made (part, file),
                    Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
                    Err(e) => return Err(io(e)),
                }
            }
            return Err(io("too many takes of this track are being written at once"));
        };
        let made = (|| {
            let lock = file.try_clone()?;
            let _ = m2_os::fs::try_lock(&lock);
            let spec = hound::WavSpec {
                channels: channels.clamp(1, 2),
                sample_rate: rate,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            };
            let limit = Arc::new(AtomicU64::new(u64::MAX));
            let out = Out { file, at: 0, limit: limit.clone() };
            let writer = hound::WavWriter::new(BufWriter::new(out), spec).map_err(std::io::Error::other)?;
            Ok::<_, std::io::Error>((lock, writer, limit))
        })();
        let (lock, writer, limit) = made.map_err(|e| {
            let _ = std::fs::remove_file(&part);
            io(e)
        })?;
        let channels = channels.clamp(1, 2);
        Ok(Growing { folder, part, wanted, writer: Some(writer), _lock: lock, channels, rate, frames: 0, limit, broken: false })
    }

    /// The length its file can't grow past, in bytes: what a full disk does (tests).
    #[doc(hidden)]
    pub fn limit(&self) -> Arc<AtomicU64> {
        self.limit.clone()
    }

    /// Frames it can still take: a WAV counts its bytes in 32 bits, header and all, so a take
    /// ends a little before 4 GiB (about 3.1 hours of 48 kHz stereo, 6.2 of mono). Past that the
    /// header would wrap, and the clip would play its start only (the review's io-r3).
    pub fn room(&self) -> u64 {
        (u32::MAX as u64 - HEADROOM) / (4 * self.channels as u64) - self.frames
    }

    /// Add `frames` (left and right; a mono take keeps the left), and leave the header saying so.
    /// At most [`Self::room`] of them. Once a write fails (a full disk), nothing more is added:
    /// [`Self::finish`] keeps what was added before it.
    pub fn append(&mut self, frames: &[[f32; 2]]) -> Result<(), EditError> {
        if self.broken {
            return Err(io("a write failed before"));
        }
        if frames.len() as u64 > self.room() {
            return Err(io("a WAV file can't be that long"));
        }
        let w = self.writer.as_mut().ok_or_else(|| io("it is finished"))?;
        let stereo = self.channels == 2;
        let wrote = (|| {
            for f in frames {
                w.write_sample(f[0])?;
                if stereo {
                    w.write_sample(f[1])?;
                }
            }
            w.flush()
        })();
        if let Err(e) = wrote {
            self.broken = true;
            return Err(io(e));
        }
        self.frames += frames.len() as u64;
        Ok(())
    }

    /// Frames written so far.
    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// Its rate.
    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// The finished file, ready for a Tx to name (WRITE-1): its header finished, its bytes pushed,
    /// named create-only, its folder synced, in that order. `name` is what the take is called.
    /// After a failed write, it keeps the frames added before it: the bytes the failure left after
    /// them are cut off, and the header says what is left. Should that fail too, the part file
    /// stays, and a later sweep takes it.
    pub fn finish(mut self, name: &str) -> Result<Imported, EditError> {
        let w = self.writer.take().ok_or_else(|| io("it is finished"))?;
        if self.broken {
            // hound writes its header again as it drops, failing or not: `cut` sets it right.
            drop(w);
            self.cut().map_err(io)?;
        } else {
            w.finalize().map_err(io)?;
        }
        let file = File::open(&self.part).map_err(io)?;
        m2_os::fs::push(&file).map_err(io)?;
        seen(&self.folder, "push");
        let hash = Sha::of_file(&self.part).map_err(io)?;
        let mut taken: Vec<String> = assets_in(&self.folder).into_iter().map(|(n, _)| n).collect();
        let asset = loop {
            let name = self.wanted.untaken_folded(taken.iter().map(String::as_str));
            match name_new(&self.part, &self.folder.join(name.as_str())) {
                Ok(()) => break name,
                Err(e) if e.kind() == ErrorKind::AlreadyExists => taken.push(name.as_str().to_string()),
                Err(e) => return Err(io(e)),
            }
        };
        seen(&self.folder, "name");
        m2_os::fs::sync_dir(&self.folder).map_err(io)?;
        seen(&self.folder, "sync");
        let seconds = self.frames as f64 / self.rate as f64;
        Ok(Imported { asset, name: name.into(), hash, seconds, rate: self.rate, dropped_channels: 0, reused: false, made_from: None })
    }

    /// Cut the part file to the frames added whole, and set its header's two sizes to them. It
    /// only shrinks the file and writes over 8 bytes of it: a full disk has room for that.
    fn cut(&self) -> std::io::Result<()> {
        let mut f = std::fs::OpenOptions::new().read(true).write(true).open(&self.part)?;
        let mut head = [0u8; 512];
        let mut n = 0;
        while n < head.len() {
            match std::io::Read::read(&mut f, &mut head[n..])? {
                0 => break,
                k => n += k,
            }
        }
        // The data chunk: after "RIFF", the size and "WAVE", chunks of an id, a size and a body.
        let mut at = 12;
        let data = loop {
            if at + 8 > n {
                return Err(std::io::Error::new(ErrorKind::InvalidData, "the take's header is broken"));
            }
            let size = u32::from_le_bytes(head[at + 4..at + 8].try_into().unwrap_or_default()) as usize;
            if &head[at..at + 4] == b"data" {
                break at as u64;
            }
            at += 8 + size + size % 2;
        };
        let bytes = self.frames * 4 * self.channels as u64;
        let len = data + 8 + bytes;
        f.set_len(len)?;
        f.seek(SeekFrom::Start(4))?;
        f.write_all(&((len - 8) as u32).to_le_bytes())?;
        f.seek(SeekFrom::Start(data + 4))?;
        f.write_all(&(bytes as u32).to_le_bytes())?;
        f.flush()
    }

    /// The part file, while it records (tests).
    #[doc(hidden)]
    pub fn part(&self) -> &Path {
        &self.part
    }
}

impl Drop for Growing {
    /// Let go unfinished: nothing is left.
    fn drop(&mut self) {
        if self.writer.take().is_some() {
            let _ = std::fs::remove_file(&self.part);
        }
    }
}

/// What a test watches: each step of each take's finish, by its `assets/` folder's real path, and
/// the Tx's.
static STEPS: Mutex<Vec<(PathBuf, &'static str)>> = Mutex::new(vec![]);
static WATCHING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// `p` by its real path, links followed (a Mac's temporary folder is reached through /var, a link
/// to /private/var), or as given when it can't be read.
fn real(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

/// Note step `what` of a take in the `assets/` folder `folder`, while a test watches.
pub fn seen(folder: &Path, what: &'static str) {
    if WATCHING.load(std::sync::atomic::Ordering::Relaxed) {
        STEPS.lock().unwrap_or_else(|e| e.into_inner()).push((real(folder), what));
    }
}

/// The steps seen in `dir`'s `assets/` so far, in order, watching from now on (tests:
/// `a_take_is_durable_before_the_song_names_it`). Folders are compared by their real paths: `dir`
/// may be spelled through a link.
#[doc(hidden)]
pub fn steps(dir: &Path) -> Vec<&'static str> {
    WATCHING.store(true, std::sync::atomic::Ordering::Relaxed);
    let folder = real(dir).join(ASSETS);
    STEPS.lock().unwrap_or_else(|e| e.into_inner()).iter().filter(|(f, _)| *f == folder).map(|(_, w)| *w).collect()
}
