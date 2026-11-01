//! Bringing audio files into a song (lane A's A2, FMT-14, WRITE-1).
//!
//! [`audio`] copies one file into the song's `assets/`: a WAV or a FLAC (told by its first bytes,
//! not its name: FMT-24), up to 3 hours, through a hidden part file that is locked at once, pushed,
//! decoded once (so what imports is what plays), then named create-only and its folder synced
//! (Q3, D2). A FLAC is written there as the WAV of its samples, with its bits, as `<name>.wav`, and
//! a note beside it says what it was made from: a song's clips stream WAV, which seeks exactly. Its
//! name is the file's own, made safe ([`AssetName::for_import`]), with a number when taken in any
//! capitals. The same bytes already in `assets/` are used again, with no copy (Q2). A failure
//! leaves nothing. music2 never deletes an asset (Q4).
//!
//! The app does it in two parts ([`stage`], then [`Staged::name`]), so the copy is made before the
//! session is asked, but becomes an asset only inside the change that places it, after that
//! change's check (AGENT-3): a part file is no file of the song, and one dropped unnamed goes.
//!
//! [`place`] puts the files' clips in the song as one Tx, one undo step, after taking the new
//! files in (D1): onto an audio lane, one after another; or onto a new audio track each
//! (`tracks::add_tracks`). A clip plays its file at its own speed from where it lands: it is as
//! long as the file at the song's tempo there (Q5).

use std::collections::BTreeSet;
use std::fs::File;
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};

use m2_runtime::decode::{self, Flac, Kind};
use m2_session::Session;
use m2_song::path::ASSETS;
use m2_song::{AssetName, HashCache, Key, LaneAddr, LaneKind, Sha, Snapshot, TempoMap};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::clips::{self, Gesture};
use crate::render::{name_new, part_path, sweep};
use crate::tracks::{self, AUDIO_LANE, NewTrack, Play};
use crate::{EditError, Edited};

/// The longest file an import takes, in seconds: audio plays from the disk (RENDER-30), so this
/// only keeps an import's check and its waveform to a few minutes' work.
pub const LONGEST: f64 = 3.0 * 3600.0;
/// Frames an import's check decodes at a time: the file never sits whole in memory.
const PIECE: u64 = 65_536;
/// The sample rates an import takes.
pub const RATES: std::ops::RangeInclusive<u32> = 8_000..=384_000;

/// Where an imported file comes from: a file on this computer (the app, `m2`), or its bytes (a
/// browser's upload), with its own name.
pub enum Source {
    Path(PathBuf),
    Bytes { name: String, bytes: Vec<u8> },
}

impl Source {
    /// The file's own name, as the words say it.
    pub fn name(&self) -> String {
        match self {
            Source::Path(p) => p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned()),
            Source::Bytes { name, .. } => name.clone(),
        }
    }
}

/// A file in the song's `assets/`, ready for a clip to name.
#[derive(Clone, Debug, PartialEq)]
pub struct Imported {
    pub asset: AssetName,
    /// What the file was called where it came from.
    pub name: String,
    pub hash: Sha,
    pub seconds: f64,
    /// Its sample rate.
    pub rate: u32,
    /// Channels past the first two, which nothing plays (Q1).
    pub dropped_channels: u16,
    /// Whether the same bytes were in `assets/` already, so nothing was copied.
    pub reused: bool,
    /// The FLAC its WAV was made from (FMT-24); none for a WAV, copied as it was.
    pub made_from: Option<MadeFrom>,
}

/// The FLAC a WAV in `assets/` was made from (FMT-24): its name where it came from, and the hash
/// of its bytes. Its note says so.
#[derive(Clone, Debug, PartialEq)]
pub struct MadeFrom {
    pub file: String,
    pub hash: Sha,
}

impl MadeFrom {
    /// As the note says it: `{"file": "kick.flac", "format": "FLAC", "sha256": "…"}`.
    pub fn note(&self) -> Value {
        json!({ "file": self.file, "format": "FLAC", "sha256": self.hash.hex() })
    }
}

/// What a file's header says: its kind, and what plays.
struct Header {
    kind: Kind,
    channels: u16,
    rate: u32,
    frames: u64,
}

fn refused(why: String) -> EditError {
    EditError::Rejected(why)
}

/// The words for a file that is neither a WAV nor a FLAC (Q1, FMT-24).
pub fn not_wav(name: &str) -> String {
    format!(
        "{name} isn't a WAV or FLAC file. music2 reads WAV and FLAC files. Save it as a WAV first: most audio apps can (Export, then WAV)."
    )
}

/// The words for a file of `kind` that doesn't read, `why` after.
fn doesnt_read(name: &str, kind: Kind, why: &str) -> String {
    let kind = match kind {
        Kind::Wav => "WAV",
        Kind::Flac => "FLAC",
    };
    format!("{name} doesn't read as a {kind} file ({why})")
}

/// The name a FLAC called `name` comes in as (FMT-24): its WAV's, `<name>.wav`.
fn as_wav(name: &str) -> String {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.contains(['/', '\\']) => format!("{stem}.wav"),
        _ => format!("{name}.wav"),
    }
}

/// Everything an import can know from the file before copying it: a WAV or a FLAC (FMT-24), that
/// reads, has sound, plays at a rate music2 takes and is at most 3 hours long (D31).
fn header(name: &str, r: Box<dyn Read + '_>) -> Result<Header, EditError> {
    let (kind, r) = decode::sniff(r).map_err(|e| refused(format!("{name} can't be read ({e})")))?;
    let h = match kind {
        Some(Kind::Wav) => {
            let reader = hound::WavReader::new(r).map_err(|e| refused(doesnt_read(name, Kind::Wav, &e.to_string())))?;
            let spec = reader.spec();
            Header { kind: Kind::Wav, channels: spec.channels, rate: spec.sample_rate, frames: reader.duration() as u64 }
        }
        Some(Kind::Flac) => {
            let f = Flac::new(r).map_err(|e| refused(doesnt_read(name, Kind::Flac, &format!("{e:#}"))))?;
            Header { kind: Kind::Flac, channels: f.channels, rate: f.rate, frames: f.frames }
        }
        None => return Err(refused(not_wav(name))),
    };
    takes(name, &h)?;
    Ok(h)
}

/// Whether a file whose header is `h` is one an import takes (D31): it has sound, plays at a rate
/// music2 takes and is at most 3 hours long.
fn takes(name: &str, h: &Header) -> Result<(), EditError> {
    if !RATES.contains(&h.rate) {
        return Err(refused(format!("{name} plays at {} Hz: music2 takes files from 8,000 to 384,000 Hz.", h.rate)));
    }
    if h.frames == 0 || h.channels == 0 {
        return Err(refused(format!("{name} has no sound in it.")));
    }
    if h.frames as f64 / h.rate as f64 > LONGEST {
        return Err(refused(format!("{name} is longer than 3 hours: music2 takes files up to 3 hours.")));
    }
    Ok(())
}

/// Every file under `assets/` that could be an asset, by its name there.
pub(crate) fn assets_in(folder: &Path) -> Vec<(String, PathBuf)> {
    let mut out = vec![];
    let mut todo = vec![(String::new(), folder.to_path_buf())];
    while let Some((prefix, dir)) = todo.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let rel = format!("{prefix}{name}");
            match e.file_type() {
                Ok(t) if t.is_dir() => todo.push((format!("{rel}/"), e.path())),
                Ok(t) if t.is_file() => out.push((rel, e.path())),
                _ => {}
            }
        }
    }
    out
}

/// Copy the file `from` into `dir`'s `assets/` (Q2, Q3, D2): the asset it is now. Its check
/// decodes it in pieces (D12), each handed to `each` (its first two channels, in order), so a
/// waveform is made from the same decode; none when the same bytes were there (`reused`: nothing
/// was copied or decoded). Checks all it can before copying; a failure leaves nothing.
pub fn audio(dir: &Path, from: Source, each: &mut dyn FnMut(&[Vec<f32>])) -> Result<Imported, EditError> {
    stage(dir, from, each)?.name()
}

/// A file on its way into a song's `assets/` (AGENT-3): copied to a hidden part file there, locked
/// (so no sweep takes it), pushed and decoded; or the same bytes found there already. It is no
/// file of the song yet: [`Staged::name`] makes it an asset, inside the change that places it.
/// Dropped unnamed, its part goes, and nothing of it is left.
pub struct Staged {
    /// What it will be: the asset's name it asks for, until it is named.
    file: Imported,
    /// Its part file, and the lock held on it until it is named.
    part: Option<(PathBuf, File)>,
    folder: PathBuf,
}

impl Staged {
    /// What it will be once named; its asset's name may take a number then.
    pub fn file(&self) -> &Imported {
        &self.file
    }

    /// Make it an asset: its part given the first free name, create-only, and its folder synced (a
    /// file whose bytes were there already is that asset). A name whose note's name
    /// (`kick.wav.json`, LIB-7) is taken is taken too, so no file inherits another's note. The lock
    /// goes after. A FLAC's WAV gets its note, saying what it was made from (FMT-24).
    pub fn name(self) -> Result<Imported, EditError> {
        self.name_noted(None).map(|(file, _)| file)
    }

    /// [`Self::name`], then its note written beside it, before anything names the file (LIB-7,
    /// WRITE-1): `from`, a sample's credit, and what a FLAC's WAV was made from (FMT-24). Whether
    /// a note was written now (one there already is kept, but for a note of yours).
    pub fn name_noted(self, from: Option<&Value>) -> Result<(Imported, bool), EditError> {
        let song = self.folder.parent().map(Path::to_path_buf).unwrap_or_default();
        let file = self.name_only()?;
        let mut note = serde_json::Map::new();
        if let Some(from) = from {
            note.insert("from".into(), from.clone());
        }
        if let Some(m) = &file.made_from {
            note.insert("made_from".into(), m.note());
        }
        if note.is_empty() {
            return Ok((file, false));
        }
        let noted = crate::library::samples::write_note(&song, file.asset.as_str(), &Value::Object(note))
            .map_err(|e| EditError::Io(format!("couldn't write the note of {}: {e}", file.asset)))?;
        Ok((file, noted))
    }

    fn name_only(mut self) -> Result<Imported, EditError> {
        let Some((part, lock)) = self.part.take() else { return Ok(self.file.clone()) };
        let named = (|| -> std::io::Result<AssetName> {
            let there: Vec<String> = assets_in(&self.folder).into_iter().map(|(n, _)| n).collect();
            // A note's name taken takes its asset's: `kick.wav.json` takes `kick.wav`.
            let notes = there.iter().filter_map(|n| n.strip_suffix(".json")).map(String::from).collect::<Vec<_>>();
            let mut taken: Vec<String> = there.into_iter().chain(notes).collect();
            let asset = loop {
                let name = self.file.asset.untaken_folded(taken.iter().map(String::as_str));
                match name_new(&part, &self.folder.join(name.as_str())) {
                    Ok(()) => break name,
                    // Made a moment ago by someone else: the next number.
                    Err(e) if e.kind() == ErrorKind::AlreadyExists => taken.push(name.as_str().to_string()),
                    Err(e) => return Err(e),
                }
            };
            m2_os::fs::sync_dir(&self.folder)?;
            Ok(asset)
        })();
        drop(lock);
        match named {
            Ok(asset) => Ok(Imported { asset, ..self.file.clone() }),
            Err(e) => {
                let _ = std::fs::remove_file(&part);
                Err(EditError::Io(format!("couldn't bring {} in: {e}", self.file.name)))
            }
        }
    }
}

impl Drop for Staged {
    fn drop(&mut self) {
        if let Some((part, lock)) = self.part.take() {
            let _ = std::fs::remove_file(&part);
            drop(lock);
        }
    }
}

/// The first part of [`audio`]: `from` checked, copied to a hidden part file in `dir`'s `assets/`
/// and decoded (each piece to `each`), or the same bytes found there; not yet an asset.
pub fn stage(dir: &Path, from: Source, each: &mut dyn FnMut(&[Vec<f32>])) -> Result<Staged, EditError> {
    let name = from.name();
    let h = match &from {
        Source::Path(p) => header(&name, Box::new(File::open(p).map_err(|e| refused(format!("{name} can't be read ({e})")))?)),
        Source::Bytes { bytes, .. } => header(&name, Box::new(&bytes[..])),
    }?;
    let io = |e: std::io::Error| EditError::Io(format!("couldn't bring {name} in: {e}"));
    let folder = dir.join(ASSETS);
    m2_os::fs::make_dirs(&folder).map_err(io)?;
    let copy_err = |e: Copy| match e {
        Copy::Io(e) => io(e),
        Copy::Decode(why) => refused(doesnt_read(&name, h.kind, &why)),
        Copy::Refused(e) => e,
    };
    if h.kind == Kind::Flac {
        // FMT-24: the WAV of its samples, made first, from one read of the FLAC: its hash, its
        // header and its audio are the same bytes (review F6). Then the same bytes there are that
        // asset.
        let wanted = AssetName::for_import(&as_wav(&name));
        let part = part_path(&folder.join(wanted.as_str())).map_err(|e| EditError::Io(e.to_string()))?;
        sweep(&folder);
        let written = match &from {
            Source::Path(p) => File::open(p).map_err(Copy::Io).and_then(|f| flac_in(&name, f, &part)),
            Source::Bytes { bytes, .. } => flac_in(&name, &bytes[..], &part),
        };
        let (lock, hash, h, source) = written.map_err(copy_err)?;
        let (seconds, dropped_channels) = (h.frames as f64 / h.rate as f64, h.channels.saturating_sub(2));
        let made_from = Some(MadeFrom { file: name.clone(), hash: source });
        let wav_size = lock.metadata().map_err(io)?.len();
        let file = Imported { asset: wanted, name: name.clone(), hash, seconds, rate: h.rate, dropped_channels, reused: false, made_from };
        if let Some(asset) = same_bytes(&folder, wav_size, hash) {
            drop(lock);
            let _ = std::fs::remove_file(&part);
            return Ok(Staged { file: Imported { asset, reused: true, ..file }, part: None, folder });
        }
        decode(&part, each)
            .map_err(|e| Copy::Decode(e.root_cause().to_string()))
            .inspect_err(|_| drop(std::fs::remove_file(&part)))
            .map_err(copy_err)?;
        return Ok(Staged { file, part: Some((part, lock)), folder });
    }
    let (seconds, dropped_channels) = (h.frames as f64 / h.rate as f64, h.channels.saturating_sub(2));
    let hash = match &from {
        Source::Path(p) => Sha::of_file(p).map_err(io)?,
        Source::Bytes { bytes, .. } => Sha::of(bytes),
    };
    let size = match &from {
        Source::Path(p) => std::fs::metadata(p).map_err(io)?.len(),
        Source::Bytes { bytes, .. } => bytes.len() as u64,
    };
    // The same bytes already there: that asset, with no copy.
    if let Some(asset) = same_bytes(&folder, size, hash) {
        let file = Imported { asset, name, hash, seconds, rate: h.rate, dropped_channels, reused: true, made_from: None };
        return Ok(Staged { file, part: None, folder });
    }
    let wanted = AssetName::for_import(&name);
    let part = part_path(&folder.join(wanted.as_str())).map_err(|e| EditError::Io(e.to_string()))?;
    sweep(&folder);
    let (lock, hash) = copy_in(&from, &part, each).map_err(copy_err)?;
    let file = Imported { asset: wanted, name, hash, seconds, rate: h.rate, dropped_channels, reused: false, made_from: None };
    Ok(Staged { file, part: Some((part, lock)), folder })
}

/// The asset in `folder` (`assets/`) whose bytes are `size` long and hash to `hash`, if any: size
/// first, then the hash.
fn same_bytes(folder: &Path, size: u64, hash: Sha) -> Option<AssetName> {
    assets_in(folder).into_iter().find_map(|(rel, p)| {
        let same = std::fs::metadata(&p).is_ok_and(|m| m.len() == size) && HashCache::global().hash(&p).is_ok_and(|s| s == hash);
        same.then(|| AssetName::new(&rel).ok()).flatten()
    })
}

/// The FLAC `name`, read from `from` once, written into the part file `part` as the WAV of its
/// samples (FMT-24), made create-only and locked at once (so no sweep takes it), then pushed: its
/// lock, held until it is named, the hash of the WAV's bytes, the FLAC's header as it was read and
/// checked (as an import checks one, D31), and the hash of every byte read (a tag after its frames
/// too). A failure removes the part this made, and only it.
fn flac_in(name: &str, from: impl Read, part: &Path) -> Result<(File, Sha, Header, Sha), Copy> {
    let f = std::fs::OpenOptions::new().read(true).write(true).create_new(true).open(part)?;
    let made = (|| {
        let _ = m2_os::fs::try_lock(&f);
        #[cfg(test)]
        tests::DURING.with(|d| d.borrow().as_ref().map(|during| during(part)));
        let flac = Flac::new(Sha::reading(from)).map_err(|e| Copy::Decode(format!("{e:#}")))?;
        let h = Header { kind: Kind::Flac, channels: flac.channels, rate: flac.rate, frames: flac.frames };
        takes(name, &h).map_err(Copy::Refused)?;
        // A write that fails (a full disk) is the disk's; anything else is the FLAC's.
        let flac = decode::write_wav(flac, std::io::BufWriter::new(&f)).map_err(|e| match e.downcast::<hound::Error>() {
            Ok(hound::Error::IoError(e)) => Copy::Io(e),
            Ok(e) => Copy::Decode(e.to_string()),
            Err(e) => match e.downcast::<std::io::Error>() {
                Ok(e) => Copy::Io(e),
                Err(e) => Copy::Decode(format!("{e:#}")),
            },
        })?;
        // The rest of the file (a tag after its frames), so the hash is of every byte read.
        let mut rest = flac.into_inner();
        std::io::copy(&mut rest, &mut std::io::sink())?;
        m2_os::fs::push(&f)?;
        Ok((Sha::of_file(part)?, h, rest.sha()))
    })();
    match made {
        Ok((hash, h, source)) => Ok((f, hash, h, source)),
        Err(e) => {
            drop(f);
            let _ = std::fs::remove_file(part);
            Err(e)
        }
    }
}

enum Copy {
    Io(std::io::Error),
    Decode(String),
    /// Refused in words of its own.
    Refused(EditError),
}

impl From<std::io::Error> for Copy {
    fn from(e: std::io::Error) -> Copy {
        Copy::Io(e)
    }
}

/// The copy itself: the part made create-only and locked at once (so no sweep takes it), filled,
/// pushed and decoded in pieces (each to `each`): its lock, held until it is named, and the hash of
/// the bytes written.
fn copy_in(from: &Source, part: &Path, each: &mut dyn FnMut(&[Vec<f32>])) -> Result<(File, Sha), Copy> {
    let f = std::fs::OpenOptions::new().write(true).create_new(true).open(part)?;
    // A failure removes the part this made, and only it (review F2).
    fill(from, f, part, each).inspect_err(|_| drop(std::fs::remove_file(part)))
}

fn fill(from: &Source, mut f: File, part: &Path, each: &mut dyn FnMut(&[Vec<f32>])) -> Result<(File, Sha), Copy> {
    let _ = m2_os::fs::try_lock(&f);
    match from {
        Source::Path(p) => {
            std::io::copy(&mut File::open(p)?, &mut f)?;
        }
        Source::Bytes { bytes, .. } => f.write_all(bytes)?,
    }
    m2_os::fs::push(&f)?;
    // The bytes written, as the clip pins them: a file still being written elsewhere may have
    // changed since it was first read.
    let hash = Sha::of_file(part)?;
    #[cfg(test)]
    tests::DURING.with(|d| d.borrow().as_ref().map(|during| during(part)));
    decode(part, each).map_err(|e| Copy::Decode(e.root_cause().to_string()))?;
    Ok((f, hash))
}

/// Decode the WAV at `path` in pieces, as it will play, each to `each`: its rate, once every
/// sample read.
pub fn decode(path: &Path, each: &mut dyn FnMut(&[Vec<f32>])) -> anyhow::Result<u32> {
    let wav = m2_runtime::stream::Wav::open(path)?;
    if wav.frames() == 0 {
        anyhow::bail!("it holds no samples");
    }
    let (mut bytes, mut piece) = (vec![], vec![vec![]; wav.channels()]);
    let mut at = 0;
    while at < wav.frames() {
        let to = wav.frames().min(at + PIECE);
        wav.read(at, to, &mut bytes, &mut piece)?;
        each(&piece);
        at = to;
    }
    Ok(wav.rate as u32)
}

/// Where imported files go (Q6, Q7), as the drop layer says it: onto an audio lane at a beat; onto
/// a new audio track after track `module`; or onto new audio tracks at the end.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Target {
    Lane { module: String, lane: String, beat: f64 },
    Track { module: String, beat: f64 },
    Below { beat: f64 },
}

impl Target {
    pub fn beat(&self) -> f64 {
        match self {
            Target::Lane { beat, .. } | Target::Track { beat, .. } | Target::Below { beat } => *beat,
        }
    }
}

/// What [`place`] did: the edit, the clips it made (to select), and the tracks it added.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub edited: Edited,
    pub made: Vec<(LaneAddr, String)>,
    /// Each track added: its key and its name.
    pub tracks: Vec<(String, String)>,
}

/// How long a clip of a file of `seconds` is, placed at `beat` (Q5): the file at its own speed,
/// under the song's tempo there.
pub fn length_at(tempo: &TempoMap, beat: f64, seconds: f64) -> f64 {
    tempo.beat_at(tempo.seconds_at(beat) + seconds) - beat
}

/// A clip playing `f` from `start` for `length` beats, as the lane format writes it.
pub fn clip_json(f: &Imported, start: f64, length: f64) -> serde_json::Value {
    json!({
        "start": start, "length": length, "asset": f.asset.as_str(), "hash": f.hash.pin(),
        "offset": 0, "gain_db": 0, "fade_in": 0, "fade_out": 0, "reverse": false, "pitch": 0, "warp": [],
    })
}

/// "Lead vox" for `lead vox.wav`: a file's name as a track's.
pub fn track_label(f: &Imported) -> String {
    let stem = f.name.rsplit_once('.').map_or(f.name.as_str(), |(s, _)| s).trim();
    let mut c = stem.chars();
    match c.next() {
        Some(first) => first.to_uppercase().chain(c).collect(),
        None => "Audio".into(),
    }
}

/// Take the imported files in, before a Tx names them (D1): the accepted revision then lists each
/// by its hash, so the clip plays it from the start, with no `asset.missing` until the folder's
/// watcher catches up.
pub fn take_in(s: &mut Session, files: &[Imported]) -> Result<(), EditError> {
    let paths: BTreeSet<PathBuf> = files.iter().map(|f| Path::new(ASSETS).join(f.asset.as_str())).collect();
    let dir = s.accepted().project.dir.clone();
    let snapshot = Snapshot::read_paths(&dir, &paths, &|_| true, BTreeSet::new()).map_err(|e| EditError::Io(format!("{e:#}")))?;
    s.take_in_only(snapshot, &paths);
    Ok(())
}

/// Put the clips of `files` in the song as one Tx, one undo step (Q4, Q6): onto lane `at` one
/// after another, or each onto a new audio track named after it.
pub fn place(s: &mut Session, files: &[Imported], at: &Target) -> Result<Placed, EditError> {
    place_with(s, files, at, &[])
}

/// A clip of `f` from `start`: as long as the file at the song's tempo there; or, following a
/// loop's own tempo `bpm` (LIB-7), as long as its beats, with Follow tempo's warp markers made
/// from that tempo, so it plays in time with the song.
fn clip_at(f: &Imported, start: f64, tempo: &TempoMap, bpm: Option<f64>) -> serde_json::Value {
    match bpm {
        None => clip_json(f, start, length_at(tempo, start, f.seconds)),
        Some(bpm) => {
            let beats = crate::library::samples::loop_beats(f.seconds, bpm, f.rate);
            let mut clip = clip_json(f, start, beats);
            clip["warp"] = json!([[0.0, 0.0], [f.seconds, beats]]);
            clip
        }
    }
}

/// [`place`], each file following the tempo `follow` gives it, if any (by its index): a loop added
/// from the library (LIB-7).
pub fn place_with(s: &mut Session, files: &[Imported], at: &Target, follow: &[Option<f64>]) -> Result<Placed, EditError> {
    let follows = |i: usize| follow.get(i).copied().flatten();
    if files.is_empty() {
        return Err(refused("No file to bring in.".into()));
    }
    check_target(s, at)?;
    take_in(s, files)?;
    let tempo = s.accepted().compiled.data.tempo().clone();
    let beat = at.beat();
    match at {
        Target::Lane { module, lane, .. } => {
            let addr = LaneAddr { module: Key::new(module).map_err(refused)?, lane: Key::new(lane).map_err(refused)? };
            let mut start = beat;
            let mut put = vec![];
            for (i, f) in files.iter().enumerate() {
                let clip = clip_at(f, start, &tempo, follows(i));
                start += clip["length"].as_f64().expect("a clip's length");
                put.push(clip);
            }
            let done = clips::edit(s, &Gesture::Set { lane: addr, put, delete: vec![] })?;
            Ok(Placed { edited: done.edited, made: done.made, tracks: vec![] })
        }
        Target::Track { .. } | Target::Below { .. } => {
            // A new audio track for each file, named after it, its clip at the beat: one Tx.
            let after = match at {
                Target::Track { module, .. } => Some(module.as_str()),
                _ => None,
            };
            let new: Vec<NewTrack> = files
                .iter()
                .enumerate()
                .map(|(i, f)| NewTrack {
                    play: Play::Audio,
                    label: Some(track_label(f)),
                    key: None,
                    clips: vec![clip_at(f, beat, &tempo, follows(i))],
                    ..NewTrack::default()
                })
                .collect();
            let made = tracks::add_tracks(s, &new, after, None)?;
            let data = &s.accepted().compiled.data;
            let mut clips = vec![];
            for m in &made {
                let lane = LaneAddr { module: Key::new(&m.module).map_err(refused)?, lane: Key::new(AUDIO_LANE).map_err(refused)? };
                let ids = data
                    .lane(&m.module, AUDIO_LANE)
                    .map(|d| d.ids().into_iter().flatten().map(str::to_string).collect::<Vec<_>>())
                    .unwrap_or_default();
                clips.extend(ids.into_iter().map(|id| (lane.clone(), id)));
            }
            let edited = made[0].edited.clone();
            Ok(Placed { edited, made: clips, tracks: made.into_iter().map(|m| (m.module, m.label)).collect() })
        }
    }
}

/// Whether clips can go at `at` in the accepted revision, in words: a beat in the song, on an audio
/// lane, or after a track that is there. Checked before a file is named into `assets/` (LIB-7), so a
/// refusal leaves nothing behind.
pub fn check_target(s: &Session, at: &Target) -> Result<(), EditError> {
    let beat = at.beat();
    if !(beat.is_finite() && beat >= 0.0) {
        return Err(refused("A clip can't start before the song starts.".into()));
    }
    match at {
        Target::Lane { module, lane, .. } => {
            audio_lane(s, &LaneAddr { module: Key::new(module).map_err(refused)?, lane: Key::new(lane).map_err(refused)? })
        }
        Target::Track { module, .. } if !s.accepted().compiled.ir.modules.iter().any(|m| m.key == *module) => {
            Err(EditError::Missing(format!("There's no track `{module}`.")))
        }
        _ => Ok(()),
    }
}

/// Whether `addr` is an audio lane of the accepted revision, in words.
fn audio_lane(s: &Session, addr: &LaneAddr) -> Result<(), EditError> {
    let rev = s.accepted();
    let m = rev.compiled.ir.modules.iter().find(|m| m.key == addr.module.as_str());
    let m = m.ok_or_else(|| EditError::Missing(format!("There's no track `{}`.", addr.module)))?;
    let decl = m
        .lanes
        .iter()
        .find(|l| l.key == addr.lane.as_str())
        .ok_or_else(|| EditError::Missing(format!("{} has no lane `{}`.", m.label(), addr.lane)))?;
    match m2_runtime::lane_kind(decl.kind) {
        LaneKind::Audio => Ok(()),
        _ => Err(refused(format!("{}'s {} lane plays notes or automation: drop audio on an audio track.", m.label(), decl.label()))),
    }
}

#[cfg(test)]
#[path = "import_tests.rs"]
mod tests;
