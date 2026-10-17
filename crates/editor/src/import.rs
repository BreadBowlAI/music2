//! Bringing audio files into a song (lane A's A2, FMT-14, WRITE-1).
//!
//! [`audio`] copies one file into the song's `assets/`: a WAV only (by its `RIFF`/`WAVE` header,
//! not its name), up to 15 minutes, through a hidden part file that is locked at once, pushed,
//! decoded once (so what imports is what plays), then named create-only and its folder synced
//! (Q3, D2). Its name is the file's own, made safe ([`AssetName::for_import`]), with a number when
//! taken in any capitals. The same bytes already in `assets/` are used again, with no copy (Q2).
//! A failure leaves nothing. music2 never deletes an asset (Q4).
//!
//! [`place`] puts the files' clips in the song as one Tx, one undo step, after taking the new
//! files in (D1): onto an audio lane, one after another; or onto a new audio track each (P0's
//! `tracks::add_tracks`). A clip plays its file at its own speed from where it lands: it is as
//! long as the file at the song's tempo there (Q5).

use std::collections::BTreeSet;
use std::fs::File;
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};

use m2_runtime::audio::Asset;
use m2_session::Session;
use m2_song::path::ASSETS;
use m2_song::{AssetName, HashCache, Key, LaneAddr, LaneKind, Sha, Snapshot, TempoMap};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::clips::{self, Gesture};
use crate::render::{name_new, part_path, sweep};
use crate::{EditError, Edited};

/// The longest file an import takes, in seconds: audio is kept in memory for now (RENDER-30).
pub const LONGEST: f64 = 15.0 * 60.0;
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
/// plays at a rate music2 takes and is at most 15 minutes long (D31).
fn header(name: &str, mut r: Box<dyn Read + '_>) -> Result<Header, EditError> {
    let mut head = [0u8; 12];
    let n = read_up_to(&mut r, &mut head);
    if !is_wav(&head[..n]) {
        return Err(refused(not_wav(name)));
    }
    let reader = hound::WavReader::new(std::io::Cursor::new(head).chain(r)).map_err(|e| refused(format!("{name} doesn't read as a WAV file ({e})")))?;
    let spec = reader.spec();
    let h = Header { channels: spec.channels, rate: spec.sample_rate, frames: reader.duration() };
    if !RATES.contains(&h.rate) {
        return Err(refused(format!("{name} plays at {} Hz: music2 takes files from 8,000 to 384,000 Hz.", h.rate)));
    }
    if h.frames == 0 || h.channels == 0 {
        return Err(refused(format!("{name} has no sound in it.")));
    }
    if h.frames as f64 / h.rate as f64 > LONGEST {
        return Err(refused(format!(
            "{name} is longer than 15 minutes: music2 keeps audio in memory for now, so it takes files up to 15 minutes."
        )));
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
fn assets_in(folder: &Path) -> Vec<(String, PathBuf)> {
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

/// Copy the file `from` into `dir`'s `assets/` (Q2, Q3, D2): the asset it is now, and its decode
/// (none when the same bytes were there: nothing was copied or decoded). Checks all it can before
/// copying; a failure leaves nothing.
pub fn audio(dir: &Path, from: Source) -> Result<(Imported, Option<Asset>), EditError> {
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
            return Ok((Imported { asset, name, hash, seconds, dropped_channels, reused: true }, None));
        }
    }
    let wanted = AssetName::for_import(&name);
    let part = part_path(&folder.join(wanted.as_str())).map_err(|e| EditError::Io(e.to_string()))?;
    sweep(&folder);
    let copied = copy_in(&from, &part, &folder, &wanted, there.iter().map(|(n, _)| n.as_str()));
    if copied.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    let (asset, decoded) = copied.map_err(|e| match e {
        Copy::Io(e) => io(e),
        Copy::Decode(why) => refused(format!("{name} doesn't read as a WAV file ({why})")),
    })?;
    Ok((Imported { asset, name, hash, seconds, dropped_channels, reused: false }, Some(decoded)))
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
/// pushed, decoded, then named the first free name and its folder synced. The lock goes after.
fn copy_in<'a>(from: &Source, part: &Path, folder: &Path, wanted: &AssetName, there: impl Iterator<Item = &'a str>) -> Result<(AssetName, Asset), Copy> {
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(part)?;
    let _ = m2_os::fs::try_lock(&f);
    match from {
        Source::Path(p) => {
            std::io::copy(&mut File::open(p)?, &mut f)?;
        }
        Source::Bytes { bytes, .. } => f.write_all(bytes)?,
    }
    m2_os::fs::push(&f)?;
    #[cfg(test)]
    tests::DURING.with(|d| d.borrow().as_ref().map(|during| during(part)));
    let decoded = m2_runtime::audio::load_wav(part).map_err(|e| Copy::Decode(e.root_cause().to_string()))?;
    if decoded.is_empty() {
        return Err(Copy::Decode("it holds no samples".into()));
    }
    let mut taken: Vec<String> = there.map(str::to_string).collect();
    let asset = loop {
        let name = wanted.untaken_folded(taken.iter().map(String::as_str));
        match name_new(part, &folder.join(name.as_str())) {
            Ok(()) => break name,
            // Made a moment ago by someone else: the next number.
            Err(e) if e.kind() == ErrorKind::AlreadyExists => taken.push(name.as_str().to_string()),
            Err(e) => return Err(e.into()),
        }
    };
    m2_os::fs::sync_dir(folder)?;
    drop(f);
    Ok((asset, decoded))
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
    if files.is_empty() {
        return Err(refused("No file to bring in.".into()));
    }
    take_in(s, files)?;
    let tempo = s.accepted().compiled.data.tempo().clone();
    let beat = at.beat();
    if !(beat.is_finite() && beat >= 0.0) {
        return Err(refused("A clip can't start before the song starts.".into()));
    }
    match at {
        Target::Lane { module, lane, .. } => {
            let addr = LaneAddr { module: Key::new(module).map_err(refused)?, lane: Key::new(lane).map_err(refused)? };
            audio_lane(s, &addr)?;
            let mut start = beat;
            let mut put = vec![];
            for f in files {
                let length = length_at(&tempo, start, f.seconds);
                put.push(clip_json(f, start, length));
                start += length;
            }
            let done = clips::edit(s, &Gesture::Set { lane: addr, put, delete: vec![] })?;
            Ok(Placed { edited: done.edited, made: done.made, tracks: vec![] })
        }
        Target::Track { .. } | Target::Below { .. } => {
            Err(refused("A new audio track for a file waits for P0's tracks::add_tracks.".into()))
        }
    }
}

/// Whether `addr` is an audio lane of the accepted revision, in words.
fn audio_lane(s: &Session, addr: &LaneAddr) -> Result<(), EditError> {
    let rev = s.accepted();
    let m = rev.compiled.ir.modules.iter().find(|m| m.key == addr.module.as_str());
    let m = m.ok_or_else(|| EditError::Missing(format!("There's no track `{}`.", addr.module)))?;
    let decl = m.lanes.iter().find(|l| l.key == addr.lane.as_str()).ok_or_else(|| EditError::Missing(format!("{} has no lane `{}`.", m.label(), addr.lane)))?;
    match m2_runtime::lane_kind(decl.kind) {
        LaneKind::Audio => Ok(()),
        _ => Err(refused(format!("{}'s {} lane plays notes or automation: drop audio on an audio track.", m.label(), decl.label()))),
    }
}

#[cfg(test)]
#[path = "import_tests.rs"]
mod tests;
