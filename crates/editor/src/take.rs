//! A recorded take's file (the io plan's D17 and D18; WRITE-1, FMT-14): a 32-bit float WAV that
//! grows in the song's `assets/` while it records.
//!
//! [`Growing::new`] makes a hidden part file there, create-only, and locks it at once, as an import
//! does (`.Vox take.wav.<pid>.part`), so no sweep takes it. [`Growing::append`] adds frames; every
//! write leaves a WAV whose header says what it holds so far. [`Growing::finish`] finishes the
//! header, pushes the bytes to the disk, names the file create-only (`Vox take.wav`, numbered when
//! taken in any capitals: `Vox take 2.wav`) and syncs the folder. Only then may a Tx name it. A
//! take let go unfinished (cancelled, or its song closed) leaves nothing.

use std::fs::File;
use std::io::{BufWriter, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use m2_song::path::ASSETS;
use m2_song::{AssetName, Sha};

use crate::EditError;
use crate::import::{Imported, assets_in};
use crate::render::{name_new, part_path, sweep};

type Writer = hound::WavWriter<BufWriter<File>>;

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
        let part = part_path(&folder.join(wanted.as_str())).map_err(io)?;
        sweep(&folder);
        let file = std::fs::OpenOptions::new().write(true).create_new(true).open(&part).map_err(io)?;
        let made = (|| {
            let lock = file.try_clone()?;
            let _ = m2_os::fs::try_lock(&lock);
            let spec = hound::WavSpec {
                channels: channels.clamp(1, 2),
                sample_rate: rate,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            };
            let writer = hound::WavWriter::new(BufWriter::new(file), spec).map_err(std::io::Error::other)?;
            Ok::<_, std::io::Error>((lock, writer))
        })();
        let (lock, writer) = made.map_err(|e| {
            let _ = std::fs::remove_file(&part);
            io(e)
        })?;
        Ok(Growing { folder, part, wanted, writer: Some(writer), _lock: lock, channels: channels.clamp(1, 2), rate, frames: 0 })
    }

    /// Add `frames` (left and right; a mono take keeps the left), and leave the header saying so.
    pub fn append(&mut self, frames: &[[f32; 2]]) -> Result<(), EditError> {
        let w = self.writer.as_mut().ok_or_else(|| io("it is finished"))?;
        for f in frames {
            w.write_sample(f[0]).map_err(io)?;
            if self.channels == 2 {
                w.write_sample(f[1]).map_err(io)?;
            }
        }
        w.flush().map_err(io)?;
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
    pub fn finish(mut self, name: &str) -> Result<Imported, EditError> {
        let w = self.writer.take().ok_or_else(|| io("it is finished"))?;
        w.finalize().map_err(io)?;
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
        Ok(Imported { asset, name: name.into(), hash, seconds, rate: self.rate, dropped_channels: 0, reused: false })
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

/// What a test watches: each step of each take's finish, by its `assets/` folder, and the Tx's.
static STEPS: Mutex<Vec<(PathBuf, &'static str)>> = Mutex::new(vec![]);
static WATCHING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Note step `what` of a take in the `assets/` folder `folder`, while a test watches.
pub fn seen(folder: &Path, what: &'static str) {
    if WATCHING.load(std::sync::atomic::Ordering::Relaxed) {
        STEPS.lock().unwrap_or_else(|e| e.into_inner()).push((folder.to_path_buf(), what));
    }
}

/// The steps seen in `dir`'s `assets/` so far, in order, watching from now on (tests:
/// `a_take_is_durable_before_the_song_names_it`).
#[doc(hidden)]
pub fn steps(dir: &Path) -> Vec<&'static str> {
    WATCHING.store(true, std::sync::atomic::Ordering::Relaxed);
    let folder = dir.join(ASSETS);
    STEPS.lock().unwrap_or_else(|e| e.into_inner()).iter().filter(|(f, _)| *f == folder).map(|(_, w)| *w).collect()
}
