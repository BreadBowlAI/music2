//! Bringing audio files into a song (lane A's A2, FMT-14, WRITE-1).
//!
//! [`audio`] copies one file into the song's `assets/`: a WAV only (by its `RIFF`/`WAVE` header,
//! not its name), up to 3 hours, through a hidden part file that is locked at once, pushed,
//! decoded once (so what imports is what plays), then named create-only and its folder synced
//! (Q3, D2). Its name is the file's own, made safe ([`AssetName::for_import`]), with a number when
//! taken in any capitals. The same bytes already in `assets/` are used again, with no copy (Q2).
//! A failure leaves nothing. music2 never deletes an asset (Q4).
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

use m2_session::Session;
use m2_song::path::ASSETS;
use m2_song::{AssetName, HashCache, Key, LaneAddr, LaneKind, Sha, Snapshot, TempoMap};
use serde::{Deserialize, Serialize};
use serde_json::json;

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
}

/// What a WAV's header says.
struct Header {
    channels: u16,
    rate: u32,
    frames: u32,
}

fn refused(why: String) -> EditError {
    EditError::Rejected(why)
}

/// The words for a file that isn't a WAV (Q1).
pub fn not_wav(name: &str) -> String {
    format!("{name} isn't a WAV file. music2 reads WAV files. Save it as a WAV first: most audio apps can (Export, then WAV).")
}

/// Whether `head` (a file's first bytes) starts as a WAV does.
pub fn is_wav(head: &[u8]) -> bool {
    head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WAVE"
}

/// Everything an import can know from the file before copying it: a WAV, that reads, has sound,
/// plays at a rate music2 takes and is at most 3 hours long (D31).
fn header(name: &str, mut r: Box<dyn Read + '_>) -> Result<Header, EditError> {
    let mut head = [0u8; 12];
    let n = read_up_to(&mut r, &mut head);
    if !is_wav(&head[..n]) {
        return Err(refused(not_wav(name)));
    }
    let reader = hound::WavReader::new(std::io::Cursor::new(head).chain(r))
        .map_err(|e| refused(format!("{name} doesn't read as a WAV file ({e})")))?;
    let spec = reader.spec();
    let h = Header { channels: spec.channels, rate: spec.sample_rate, frames: reader.duration() };
    if !RATES.contains(&h.rate) {
        return Err(refused(format!("{name} plays at {} Hz: music2 takes files from 8,000 to 384,000 Hz.", h.rate)));
    }
    if h.frames == 0 || h.channels == 0 {
        return Err(refused(format!("{name} has no sound in it.")));
    }
    if h.frames as f64 / h.rate as f64 > LONGEST {
        return Err(refused(format!("{name} is longer than 3 hours: music2 takes files up to 3 hours.")));
    }
    Ok(h)
}

fn read_up_to(r: &mut dyn Read, buf: &mut [u8]) -> usize {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    n
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
    /// goes after.
    pub fn name(mut self) -> Result<Imported, EditError> {
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
    let seconds = h.frames as f64 / h.rate as f64;
    let dropped_channels = h.channels.saturating_sub(2);
    let io = |e: std::io::Error| EditError::Io(format!("couldn't bring {name} in: {e}"));
    let hash = match &from {
        Source::Path(p) => Sha::of_file(p).map_err(io)?,
        Source::Bytes { bytes, .. } => Sha::of(bytes),
    };
    let size = match &from {
        Source::Path(p) => std::fs::metadata(p).map_err(io)?.len(),
        Source::Bytes { bytes, .. } => bytes.len() as u64,
    };
    let folder = dir.join(ASSETS);
    m2_os::fs::make_dirs(&folder).map_err(io)?;
    let there = assets_in(&folder);
    // The same bytes already there: that asset, with no copy (size first, then the hash).
    for (rel, p) in &there {
        let same_size = std::fs::metadata(p).is_ok_and(|m| m.len() == size);
        if same_size
            && HashCache::global().hash(p).is_ok_and(|s| s == hash)
            && let Ok(asset) = AssetName::new(rel)
        {
            let file = Imported { asset, name, hash, seconds, rate: h.rate, dropped_channels, reused: true };
            return Ok(Staged { file, part: None, folder });
        }
    }
    let wanted = AssetName::for_import(&name);
    let part = part_path(&folder.join(wanted.as_str())).map_err(|e| EditError::Io(e.to_string()))?;
    sweep(&folder);
    let copied = copy_in(&from, &part, each);
    if copied.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    let (lock, hash) = copied.map_err(|e| match e {
        Copy::Io(e) => io(e),
        Copy::Decode(why) => refused(format!("{name} doesn't read as a WAV file ({why})")),
    })?;
    let file = Imported { asset: wanted, name, hash, seconds, rate: h.rate, dropped_channels, reused: false };
    Ok(Staged { file, part: Some((part, lock)), folder })
}

enum Copy {
    Io(std::io::Error),
    Decode(String),
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
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(part)?;
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
