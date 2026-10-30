//! Samples (LIB-4 to LIB-7): one-shots and loops, from a package's `samples/` folder (std's kit, a
//! pack's) or a folder of yours, as library items.
//!
//! What a sample is comes from three places. What can be measured is read from the file
//! ([`measure`]): its length, channels, rate, peak and hash. What a package says is read from its
//! `samples.toml` ([`Said`]), strictly. The rest is guessed from the file's path ([`guess`]): a
//! tempo, a key, loop or one-shot, and an instrument from [`INSTRUMENTS`].
//!
//! [`index`] lists a folder's samples, measuring only the files the cache folder doesn't know by
//! their path, size and modified time, so a folder indexed before is indexed again with no file
//! read. Only WAV files are samples for now; other audio files are counted ("WAV only for now").
//!
//! Adding one to a song copies its file into `assets/` with a note beside it ([`note`],
//! [`write_note`]), and places a clip, with Follow tempo for a loop at another tempo
//! ([`follow_at`]).

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Item, Key, Kind, Sample, SampleType};

/// The words a sample's instrument may be (LIB-4), in the order a name's words are tried: a drum
/// before a sound a drum's name may also say (`808 kick` is a kick, `synth bass` a bass, `pad synth`
/// a pad). The one list: `samples.toml`, the guess and `m2 pack check` read it here.
pub const INSTRUMENTS: [&str; 19] = [
    "kick", "snare", "clap", "hat", "cymbal", "tom", "perc", "bass", "keys", "guitar", "vocal", "strings", "brass", "pad", "lead", "synth",
    "fx", "texture", "foley",
];

/// The words of a file's name that say each instrument, in [`INSTRUMENTS`]' order.
const NAMES: [&[&str]; 19] = [
    &["kick", "kicks", "kik", "bd", "bassdrum"],
    &["snare", "snares", "snr", "sd"],
    &["clap", "claps", "handclap", "handclaps", "snap", "snaps"],
    &["hat", "hats", "hihat", "hihats", "hh"],
    &["cymbal", "cymbals", "crash", "crashes", "ride", "rides", "splash", "china"],
    &["tom", "toms", "floortom"],
    &[
        "perc",
        "percs",
        "percussion",
        "shaker",
        "shakers",
        "tambourine",
        "tamb",
        "cowbell",
        "cowbells",
        "conga",
        "congas",
        "bongo",
        "bongos",
        "rim",
        "rims",
        "rimshot",
        "clave",
        "claves",
        "woodblock",
        "triangle",
        "guiro",
        "cabasa",
    ],
    &["bass", "basses", "sub", "808", "reese"],
    &["keys", "piano", "pianos", "rhodes", "epiano", "organ", "wurli", "wurlitzer", "clav", "keyboard", "keyboards"],
    &["guitar", "guitars", "gtr", "strum", "strums"],
    &["vocal", "vocals", "vox", "voice", "voices", "chop", "chops", "adlib", "adlibs", "choir", "acapella"],
    &["string", "strings", "violin", "violins", "viola", "cello", "cellos", "orchestra", "orchestral"],
    &["brass", "horn", "horns", "trumpet", "trumpets", "trombone", "sax", "saxophone", "tuba"],
    &["pad", "pads"],
    &["lead", "leads"],
    &["synth", "synths", "arp", "arps", "pluck", "plucks", "stab", "stabs"],
    &["fx", "sfx", "riser", "risers", "uplifter", "downlifter", "sweep", "sweeps", "impact", "impacts", "whoosh", "transition"],
    &["texture", "textures", "atmos", "atmosphere", "ambience", "ambient", "drone", "drones", "vinyl", "crackle", "noise"],
    &["foley", "footstep", "footsteps"],
];

/// Audio files that aren't WAV, by extension: skipped and counted (LIB-4).
const NOT_WAV: [&str; 9] = ["flac", "mp3", "aif", "aiff", "ogg", "m4a", "aac", "opus", "wma"];

/// A loop's tempo, as `samples.toml` may say it and a guess may find it, in bpm.
pub const TEMPOS: std::ops::RangeInclusive<f64> = 20.0..=300.0;
/// A bare number in a name is a tempo only in this range, and only beside a key or a loop word.
const BARE_TEMPOS: std::ops::RangeInclusive<f64> = 60.0..=200.0;

/// What a sample's path says of it ([`guess`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Guess {
    pub kind: SampleType,
    pub instrument: Option<&'static str>,
    pub bpm: Option<f64>,
    pub key: Option<Key>,
}

/// A name's words: its runs of letters, digits and the signs `#`, `♯` and `♭`, split again where
/// letters meet digits (`120bpm` is `120` and `bpm`, `Kick01` is `Kick` and `01`) and where a
/// small letter meets a capital (`PianoChords` is `Piano` and `Chords`). Case kept.
fn tokens(text: &str) -> Vec<String> {
    let mut out = vec![];
    let mut word = String::new();
    let mut digits: Option<bool> = None;
    let mut small = false;
    for c in text.chars() {
        let sign = matches!(c, '#' | '♯' | '♭');
        if !(c.is_alphanumeric() || sign) {
            out.extend((!word.is_empty()).then(|| std::mem::take(&mut word)));
            digits = None;
            small = false;
            continue;
        }
        let digit = c.is_ascii_digit();
        if digits.is_some_and(|d| d != digit) || small && c.is_uppercase() {
            out.push(std::mem::take(&mut word));
        }
        word.push(c);
        digits = Some(digit);
        small = c.is_lowercase();
    }
    out.extend((!word.is_empty()).then_some(word));
    out
}

/// A root at the start of `word`: a letter A to G in either case, then a sign if any (`#`, `♯`,
/// `b`, `♭`). Its pitch class, whether the letter was a capital, whether a sign followed, and the
/// rest of the word.
fn root_of(word: &str) -> Option<(u8, bool, bool, &str)> {
    let letter = word.chars().next()?;
    let base: i8 = match letter.to_ascii_uppercase() {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let rest = &word[letter.len_utf8()..];
    let (shift, rest) = match rest.chars().next() {
        Some(c @ ('#' | '♯')) => (1i8, &rest[c.len_utf8()..]),
        Some(c @ ('b' | '♭')) => (-1, &rest[c.len_utf8()..]),
        _ => (0, rest),
    };
    Some(((base + 12 + shift) as u8 % 12, letter.is_ascii_uppercase(), shift != 0, rest))
}

/// A key written as one word of a name: `Am`, `C#m`, `F#`, `Bb`, `Ebmin`, `Cmaj`, `f#m`,
/// `amin`. A bare letter isn't one, `m` alone needs a capital or a sign (`am` is a word), and a
/// sign alone needs a capital.
fn key_word(word: &str) -> Option<Key> {
    let (root, capital, sign, rest) = root_of(word)?;
    let minor = match rest.to_ascii_lowercase().as_str() {
        _ if rest == "M" => return None,
        "m" | "min" | "minor" => true,
        "" | "maj" | "major" => false,
        _ => return None,
    };
    let ok = match rest.len() {
        0 => capital && sign,
        1 => capital || sign,
        _ => true,
    };
    ok.then_some(Key { root, minor })
}

/// The first key in `words`: one word ([`key_word`]), or a root and its mode (`F# minor`,
/// `Bb maj`, `A min`).
fn key_in(words: &[String]) -> Option<Key> {
    for (i, w) in words.iter().enumerate() {
        let mode = words.get(i + 1).map(|n| n.to_ascii_lowercase());
        let minor = match mode.as_deref() {
            Some("minor" | "min") => Some(true),
            Some("major" | "maj") => Some(false),
            _ => None,
        };
        if let (Some(minor), Some((root, _, _, ""))) = (minor, root_of(w)) {
            return Some(Key { root, minor });
        }
        if let Some(k) = key_word(w) {
            return Some(k);
        }
    }
    None
}

/// A whole number of two or three digits.
fn number(word: &str) -> Option<f64> {
    (2..=3).contains(&word.len()).then_some(())?;
    word.bytes().all(|b| b.is_ascii_digit()).then(|| word.parse().ok())?
}

/// The tempo `words` say: a number beside `bpm` (`90 bpm`, `120bpm`, `bpm 95`) in [`TEMPOS`], or,
/// when `bare`, a number alone in [`BARE_TEMPOS`] with no leading zero (not `01` or `045`).
fn tempo_in(words: &[String], bare: bool) -> Option<f64> {
    let lower: Vec<String> = words.iter().map(|w| w.to_ascii_lowercase()).collect();
    for (i, w) in lower.iter().enumerate() {
        let beside = lower.get(i + 1).is_some_and(|n| n == "bpm") || i > 0 && lower[i - 1] == "bpm";
        if let Some(n) = number(w).filter(|n| beside && TEMPOS.contains(n)) {
            return Some(n);
        }
    }
    bare.then(|| lower.iter().filter(|w| !w.starts_with('0')).find_map(|w| number(w).filter(|n| BARE_TEMPOS.contains(n))))?
}

/// The instrument the lowercase `words` name: the first of [`INSTRUMENTS`] one of them says
/// (`bass drum` and `hi hat` as one).
fn instrument_in(words: &[String]) -> Option<&'static str> {
    if words.windows(2).any(|p| p[0] == "bass" && p[1] == "drum") {
        return Some("kick");
    }
    INSTRUMENTS.iter().zip(NAMES).find(|(_, names)| words.iter().any(|w| names.contains(&w.as_str()))).map(|(i, _)| *i)
}

/// What a sample's path inside its folder says of it (LIB-4), `/` between folders:
/// `loops/dusty keys 90 Am.wav` is a loop of keys at 90 bpm in A minor.
/// - A tempo: a number beside `bpm` in the name, else in a folder (deepest first); else a bare
///   number of the name from 60 to 200 when the path has a key or a loop word.
/// - A key: the name's, else a folder's.
/// - Its type: a one-shot when the path says so (`one-shot`, `oneshot`, `one shots`); else a loop
///   with a tempo or a loop word (`loop`, `loops`); else a one-shot.
/// - Its instrument: from the name's words, else a folder's, deepest first ([`instrument_in`]).
pub fn guess(path: &str) -> Guess {
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    let Some((file, folders)) = parts.split_last() else { return Guess::default() };
    let stem = file.rsplit_once('.').map_or(*file, |(s, _)| s);
    let name = tokens(stem);
    let dirs: Vec<Vec<String>> = folders.iter().rev().map(|f| tokens(f)).collect();
    let lower = |w: &[String]| w.iter().map(|x| x.to_lowercase()).collect::<Vec<_>>();
    let every: Vec<String> = std::iter::once(&name).chain(&dirs).flat_map(|w| lower(w)).collect();
    let one_shot = every.windows(2).any(|p| p[0] == "one" && (p[1] == "shot" || p[1] == "shots"))
        || every.iter().any(|w| w == "oneshot" || w == "oneshots");
    let looped = every.iter().any(|w| w == "loop" || w == "loops");
    let key = key_in(&name).or_else(|| dirs.iter().find_map(|d| key_in(d)));
    let bpm = tempo_in(&name, false)
        .or_else(|| dirs.iter().find_map(|d| tempo_in(d, false)))
        .or_else(|| tempo_in(&name, !one_shot && (looped || key.is_some())));
    let kind = if !one_shot && (bpm.is_some() || looped) { SampleType::Loop } else { SampleType::OneShot };
    let instrument = instrument_in(&lower(&name)).or_else(|| dirs.iter().find_map(|d| instrument_in(&lower(d))));
    Guess { kind, instrument, bpm, key }
}

/// A key as `samples.toml` writes it: `Am`, `F#`, `Bbm`, `Cmaj`, a bare capital `C` (major), or
/// a root and a mode, `A minor`, `F# major`.
pub fn key_said(text: &str) -> Option<Key> {
    let text = text.trim();
    if let Some(k) = Key::parse(text) {
        return Some(k);
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    match words.as_slice() {
        [root] => root_of(root).filter(|r| r.1 && r.3.is_empty()).map(|r| Key { root: r.0, minor: false }),
        [root, mode] => {
            let minor = match mode.to_ascii_lowercase().as_str() {
                "minor" | "min" => true,
                "major" | "maj" => false,
                _ => return None,
            };
            root_of(root).filter(|r| r.1 && r.3.is_empty()).map(|r| Key { root: r.0, minor })
        }
        _ => None,
    }
}

/// What `samples.toml` says of some samples: each field it gives wins over the guess.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Said {
    /// `loop` or `one-shot`.
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub instrument: Option<String>,
    pub bpm: Option<f64>,
    pub key: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// A package's `samples.toml` (LIB-4): what it says of the files under a folder of `samples/`, and
/// of one file, by their paths inside `samples/`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SamplesToml {
    #[serde(default)]
    pub folder: BTreeMap<String, Said>,
    #[serde(default)]
    pub file: BTreeMap<String, Said>,
}

/// The file a package's samples are said in, at its root.
pub const SAMPLES_TOML: &str = "samples.toml";
/// The folder a package's samples are in.
pub const SAMPLES_DIR: &str = "samples";

impl SamplesToml {
    /// Read `samples.toml`'s text, strictly: an unknown key, a type that isn't `loop` or
    /// `one-shot`, an instrument not in [`INSTRUMENTS`], a tempo outside [`TEMPOS`] or a key that
    /// doesn't read is refused, each in words.
    pub fn parse(text: &str) -> Result<SamplesToml, Vec<String>> {
        match SamplesToml::read(text) {
            (Some(t), problems) if problems.is_empty() => Ok(t),
            (_, problems) => Err(problems),
        }
    }

    /// [`SamplesToml::parse`], with what it says even when a word in it is wrong (none when it
    /// doesn't read as TOML, or has a key it doesn't know): `m2 pack check` checks its paths too.
    pub fn read(text: &str) -> (Option<SamplesToml>, Vec<String>) {
        let t: SamplesToml = match toml::from_str(text) {
            Ok(t) => t,
            Err(e) => return (None, vec![format!("{SAMPLES_TOML} doesn't read: {}", e.message().trim())]),
        };
        let mut problems = vec![];
        let tables = t
            .folder
            .iter()
            .map(|(p, s)| (format!("[folder.\"{p}\"]"), s))
            .chain(t.file.iter().map(|(p, s)| (format!("[file.\"{p}\"]"), s)));
        for (at, s) in tables {
            if let Some(k) = s.kind.as_deref().filter(|k| !matches!(*k, "loop" | "one-shot")) {
                problems.push(format!("{SAMPLES_TOML} {at}: type is loop or one-shot, not {k}"));
            }
            if let Some(i) = s.instrument.as_deref().filter(|i| !INSTRUMENTS.contains(i)) {
                problems.push(format!("{SAMPLES_TOML} {at}: instrument {i} isn't one of {}", INSTRUMENTS.join(", ")));
            }
            if let Some(b) = s.bpm.filter(|b| !TEMPOS.contains(b)) {
                problems.push(format!("{SAMPLES_TOML} {at}: bpm is 20 to 300, not {b}"));
            }
            if let Some(k) = s.key.as_deref().filter(|k| key_said(k).is_none()) {
                problems.push(format!("{SAMPLES_TOML} {at}: key {k} doesn't read: write it as Am, F#, Bb minor or C"));
            }
        }
        (Some(t), problems)
    }

    /// The package in `dir`'s, if it has one: none when there is no file; problems in words.
    pub fn of_package(dir: &Path) -> Result<SamplesToml, Vec<String>> {
        match std::fs::read_to_string(dir.join(SAMPLES_TOML)) {
            Ok(text) => SamplesToml::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(SamplesToml::default()),
            Err(e) => Err(vec![format!("{SAMPLES_TOML} can't be read: {e}")]),
        }
    }

    /// Whether it names the file at `rel` (inside `samples/`), by a table of its own or of a folder
    /// it is under: a package's loose samples are the files it names (LIB-4); the rest of
    /// `samples/` is its instruments' recordings.
    pub fn names(&self, rel: &str) -> bool {
        self.file.contains_key(rel) || self.folder.keys().any(|f| rel.starts_with(&format!("{}/", f.trim_end_matches('/'))))
    }

    /// What it says of the file at `rel` (inside `samples/`): its folders' tables, the shallowest
    /// first so the deepest wins, then its own; tags from all of them.
    fn of(&self, rel: &str) -> Said {
        let mut said = Said::default();
        let mut tables: Vec<(&String, &Said)> =
            self.folder.iter().filter(|(f, _)| rel.starts_with(&format!("{}/", f.trim_end_matches('/')))).collect();
        tables.sort_by_key(|(f, _)| f.len());
        tables.extend(self.file.get_key_value(rel));
        for (_, s) in tables {
            said.kind = s.kind.clone().or(said.kind);
            said.instrument = s.instrument.clone().or(said.instrument);
            said.bpm = s.bpm.or(said.bpm);
            said.key = s.key.clone().or(said.key);
            said.tags.extend(s.tags.iter().cloned());
        }
        said
    }

    /// The paths it names that aren't under `samples/` in `found` (the WAVs there and their
    /// folders), in words.
    pub fn missing(&self, found: &[String]) -> Vec<String> {
        let mut out = vec![];
        for f in self.folder.keys() {
            let prefix = format!("{}/", f.trim_end_matches('/'));
            if !found.iter().any(|p| p.starts_with(&prefix)) {
                out.push(format!("{SAMPLES_TOML} names the folder {f}, but samples/{f} holds no WAV file"));
            }
        }
        for f in self.file.keys().filter(|f| !found.contains(f)) {
            out.push(format!("{SAMPLES_TOML} names the file {f}, but there is no WAV file samples/{f}"));
        }
        out
    }
}

/// What is measured from a sample's file (LIB-4): never typed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Measured {
    pub seconds: f64,
    pub channels: u16,
    pub rate: u32,
    /// Its loudest sample: 1 is full scale.
    pub peak: f64,
    /// The SHA-256 of its bytes, in hex.
    pub hash: String,
}

/// Frames read at a time while measuring.
const PIECE: u64 = 65_536;

/// Measure the WAV file at `path`, reading it once to its end as it will play: why it can't be, in
/// words.
pub fn measure(path: &Path) -> Result<Measured, String> {
    let wav = m2_runtime::stream::Wav::open(path).map_err(|e| format!("{e:#}"))?;
    if wav.frames() == 0 {
        return Err("it holds no samples".into());
    }
    let (mut bytes, mut piece) = (vec![], vec![vec![]; wav.channels()]);
    let (mut at, mut peak) = (0, 0f32);
    while at < wav.frames() {
        let to = wav.frames().min(at + PIECE);
        wav.read(at, to, &mut bytes, &mut piece).map_err(|e| format!("{e:#}"))?;
        peak = piece.iter().flatten().fold(peak, |p, s| p.max(s.abs()));
        at = to;
    }
    let hash = m2_song::Sha::of_file(path).map_err(|e| e.to_string())?.hex();
    let seconds = wav.frames() as f64 / wav.rate;
    Ok(Measured { seconds, channels: wav.channels() as u16, rate: wav.rate as u32, peak: peak as f64, hash })
}

/// Where samples are found: a package's `samples/` folder, or a folder of yours.
#[derive(Clone, Copy, Debug)]
pub enum Root<'a> {
    Package { name: &'a str, dir: &'a Path },
    Folder(&'a Path),
}

/// What [`index`] found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Indexed {
    pub items: Vec<Item>,
    /// Audio files that aren't WAV, skipped: "WAV only for now".
    pub not_wav: usize,
    /// WAV files that don't read, and why.
    pub unreadable: Vec<String>,
    /// What is wrong with the package's `samples.toml`, in words.
    pub problems: Vec<String>,
    /// How many files were read this time; the rest came from the cache.
    pub measured: usize,
}

/// The cache's file for a root: its measured files by path, each with its size and modified time.
#[derive(Default, Serialize, Deserialize)]
struct Kept {
    files: BTreeMap<String, KeptFile>,
}

#[derive(Clone, Serialize, Deserialize)]
struct KeptFile {
    size: u64,
    modified: u64,
    measured: Measured,
}

/// A file found under a root: its path inside it (`/` between folders), its path, size and
/// modified time (nanoseconds since 1970).
struct Found {
    rel: String,
    path: PathBuf,
    size: u64,
    modified: u64,
}

/// Every file under `dir`, by sorted path: never inside a hidden folder (`.git`), never through a
/// link.
fn files_in(dir: &Path) -> Vec<Found> {
    let mut out = vec![];
    let mut stack = vec![(dir.to_path_buf(), String::new())];
    while let Some((d, prefix)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Ok(t) = e.file_type() else { continue };
            if name.starts_with('.') || t.is_symlink() {
                continue;
            }
            let rel = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
            if t.is_dir() {
                stack.push((e.path(), rel));
            } else if let Ok(m) = e.metadata() {
                let modified = m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos() as u64);
                out.push(Found { rel, path: e.path(), size: m.len(), modified });
            }
        }
    }
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    out
}

/// A file's extension, lowercase.
fn extension(rel: &str) -> String {
    Path::new(rel).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

/// Whether a file is a WAV, by its name in any capitals (`.wav`, `.WAV`, `.wave`).
pub fn is_wav_name(rel: &str) -> bool {
    matches!(extension(rel).as_str(), "wav" | "wave")
}

/// The cache's file for the folder `base`, in `cache`: named by a hash of its path.
fn cache_file(cache: &Path, base: &Path) -> PathBuf {
    let named = m2_song::Sha::of(base.to_string_lossy().as_bytes()).hex();
    cache.join(format!("{}.json", &named[..16]))
}

/// The samples of `root` (LIB-4, LIB-5): every WAV file under its folder (a package's `samples/`),
/// measured, or found in the cache folder `cache` by its path, size and modified time; then what
/// the package's `samples.toml` says of each, and the guess for the rest. The cache is written
/// again when a file was measured or is gone. Other audio files are counted, and a WAV that
/// doesn't read is named.
pub fn index(root: Root, cache: Option<&Path>) -> Indexed {
    let base = match root {
        Root::Package { dir, .. } => dir.join(SAMPLES_DIR),
        Root::Folder(dir) => dir.to_path_buf(),
    };
    let mut out = Indexed::default();
    let (said, origin) = match root {
        Root::Package { name, dir } => {
            if !base.is_dir() {
                return out;
            }
            // A `samples.toml` that doesn't read lists nothing until it is fixed.
            let said = SamplesToml::of_package(dir).unwrap_or_else(|p| {
                out.problems = p;
                SamplesToml::default()
            });
            (said, Origin::package(name, dir))
        }
        Root::Folder(_) => (SamplesToml::default(), Origin::Folder),
    };
    let listed = |rel: &str| matches!(root, Root::Folder(_)) || said.names(rel);
    let kept_at = cache.map(|c| cache_file(c, &base));
    let mut kept: Kept =
        kept_at.as_ref().and_then(|f| std::fs::read(f).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    let mut now = Kept::default();
    let files = files_in(&base);
    let mut wavs = vec![];
    for f in &files {
        if !is_wav_name(&f.rel) {
            out.not_wav += usize::from(NOT_WAV.contains(&extension(&f.rel).as_str()));
            continue;
        }
        wavs.push(f.rel.clone());
        if !listed(&f.rel) {
            continue;
        }
        let key = f.path.to_string_lossy().into_owned();
        let measured = match kept.files.remove(&key).filter(|k| k.size == f.size && k.modified == f.modified) {
            Some(k) => k.measured,
            None => {
                out.measured += 1;
                match measure(&f.path) {
                    Ok(m) => m,
                    Err(why) => {
                        out.unreadable.push(format!("{} doesn't read as a WAV file ({why})", f.rel));
                        continue;
                    }
                }
            }
        };
        now.files.insert(key, KeptFile { size: f.size, modified: f.modified, measured: measured.clone() });
        out.items.push(item(&origin, &f.rel, &f.path, &measured, &said.of(&f.rel)));
    }
    if matches!(root, Root::Package { .. }) {
        out.problems.extend(said.missing(&wavs));
    }
    // Written again only when something changed: a file measured, or one gone.
    if let Some(file) = kept_at.filter(|_| out.measured > 0 || !kept.files.is_empty()) {
        let _ = keep(&file, &now);
    }
    out
}

/// What `m2 pack check` finds wrong with the samples of the pack in `dir` (PACK-12, LIB-4), in
/// words: every WAV under `samples/` decodes, and `samples.toml` reads strictly and names only
/// files and folders there. Whether it lists any sample, too: a pack of samples alone needs no
/// component.
pub fn check(dir: &Path) -> (Vec<String>, bool) {
    let mut out = vec![];
    let said = match std::fs::read_to_string(dir.join(SAMPLES_TOML)) {
        Ok(text) => {
            let (said, problems) = SamplesToml::read(&text);
            out.extend(problems);
            said.unwrap_or_default()
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => SamplesToml::default(),
        Err(e) => {
            out.push(format!("{SAMPLES_TOML} can't be read: {e}"));
            SamplesToml::default()
        }
    };
    let mut wavs = vec![];
    for f in files_in(&dir.join(SAMPLES_DIR)).into_iter().filter(|f| is_wav_name(&f.rel)) {
        if let Err(why) = measure(&f.path) {
            out.push(format!("{SAMPLES_DIR}/{} doesn't read as a WAV file ({why})", f.rel));
        }
        wavs.push(f.rel);
    }
    out.extend(said.missing(&wavs));
    let any = wavs.iter().any(|w| said.names(w));
    (out, any)
}

/// Write the cache's file whole, through a part file renamed over it.
fn keep(file: &Path, kept: &Kept) -> std::io::Result<()> {
    std::fs::create_dir_all(file.parent().expect("a file in the cache"))?;
    let part = file.with_extension(format!("json.{}.part", std::process::id()));
    let mut f = std::fs::File::create(&part)?;
    f.write_all(&serde_json::to_vec(kept).map_err(std::io::Error::other)?)?;
    drop(f);
    std::fs::rename(&part, file)
}

/// Where a root's samples come from, as their notes say it.
enum Origin {
    Package { name: String, from: Value },
    Folder,
}

impl Origin {
    /// A package's: its name, version, author (its `authors`; music2 for std) and licence.
    fn package(name: &str, dir: &Path) -> Origin {
        let m = m2_song::pack::PkgManifest::at(dir).ok();
        let mut from = json!({ "pack": name });
        if let Some(m) = &m {
            from["version"] = json!(m.package.version.to_string());
        }
        let authors = m.as_ref().map(|m| m.package.authors.join(", ")).filter(|a| !a.is_empty());
        if let Some(a) = authors.or_else(|| (name == m2_song::pack::STD).then(|| "music2".to_string())) {
            from["author"] = json!(a);
        }
        if let Some(l) = m.and_then(|m| m.package.license) {
            from["license"] = json!(l);
        }
        Origin::Package { name: name.to_string(), from }
    }
}

/// A word with its first letter a capital: `Kick`.
fn titled(word: &str) -> String {
    let mut c = word.chars();
    c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
}

/// `10.7 s`, `0.42 s`.
fn seconds_words(s: f64) -> String {
    if s < 1.0 { format!("{s:.2} s") } else { format!("{s:.1} s") }
}

/// The library's item for the sample at `rel` under `origin`'s folder, its file at `path`: what
/// was measured, what `said` says, and the guess for the rest.
fn item(origin: &Origin, rel: &str, path: &Path, m: &Measured, said: &Said) -> Item {
    let g = guess(rel);
    let kind = match said.kind.as_deref() {
        Some("loop") => SampleType::Loop,
        Some("one-shot") => SampleType::OneShot,
        _ => g.kind,
    };
    let instrument = said.instrument.clone().or_else(|| g.instrument.map(String::from));
    let bpm = said.bpm.or(g.bpm).filter(|_| kind == SampleType::Loop || said.bpm.is_some());
    let key = said.key.as_deref().and_then(key_said).or(g.key);
    let stem = Path::new(rel).file_stem().map_or_else(|| rel.to_string(), |s| s.to_string_lossy().into_owned());
    let mut tags: Vec<String> = instrument.iter().cloned().collect();
    tags.extend(rel.split('/').rev().skip(1).map(String::from));
    tags.extend(said.tags.iter().cloned());
    let mut words = vec![match kind {
        SampleType::Loop => "A loop".to_string(),
        SampleType::OneShot => "A one-shot".to_string(),
    }];
    if let Some(b) = bpm {
        words.push(format!("{} bpm", m2_song::lane::number(b)));
    }
    if let Some(k) = key {
        words.push(k.words());
    }
    words.push(seconds_words(m.seconds));
    words.push(if m.channels == 1 { "mono".into() } else { "stereo".into() });
    let (id, package, component, from) = match origin {
        Origin::Package { name, from } => {
            let file = format!("{SAMPLES_DIR}/{rel}");
            let mut from = from.clone();
            from["file"] = json!(file);
            (format!("sample:{name}:{file}"), name.clone(), format!("{name}:{file}"), from)
        }
        Origin::Folder => (format!("sample:folder:{}", path.display()), "yours".to_string(), String::new(), json!({ "folder": true })),
    };
    Item {
        kind: Kind::Sample,
        id,
        label: stem,
        category: instrument.as_deref().map_or_else(|| "Sample".to_string(), titled),
        description: words.join(", "),
        tags,
        package,
        component,
        instrument: false,
        effect: false,
        preview: true,
        no_preview: None,
        preset: None,
        sample: Some(Sample {
            kind,
            bpm,
            key,
            seconds: Some(m.seconds),
            instrument,
            channels: m.channels,
            rate: m.rate,
            peak: m.peak,
            hash: m.hash.clone(),
            from,
            file: path.to_path_buf(),
        }),
    }
}

/// A sample's note (LIB-7), as `assets/<file>.json` holds it: `{"from": …}`.
pub fn note(sample: &Sample) -> Value {
    json!({ "from": sample.from })
}

/// The note's name for asset `asset`: `kick.wav.json` for `kick.wav`.
pub fn note_name(asset: &str) -> String {
    format!("{asset}.json")
}

/// Write `note` beside asset `asset` in the song in `dir` (LIB-7): created only if it isn't there
/// (FMT-14), its bytes pushed and its folder synced before anything names the asset (WRITE-1).
/// Whether it was written: false when a note was there, which is kept.
pub fn write_note(dir: &Path, asset: &str, note: &Value) -> std::io::Result<bool> {
    let path = dir.join(m2_song::path::ASSETS).join(note_name(asset));
    let mut f = match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Ok(false),
        Err(e) => return Err(e),
    };
    let text = serde_json::to_string_pretty(note).map_err(std::io::Error::other)? + "\n";
    let written = f.write_all(text.as_bytes()).and_then(|()| m2_os::fs::push(&f));
    if let Err(e) = written {
        drop(f);
        let _ = std::fs::remove_file(&path);
        return Err(e);
    }
    m2_os::fs::sync_dir(path.parent().expect("inside assets/"))?;
    Ok(true)
}

/// The tempo a sample's clip follows when added where the song plays at `song_bpm` (LIB-7): a
/// loop's own, when it is known and differs from the song's.
pub fn follow_at(sample: &Sample, song_bpm: f64) -> Option<f64> {
    sample.bpm.filter(|b| sample.kind == SampleType::Loop && (b - song_bpm).abs() > 1e-6)
}

/// How many beats a loop of `seconds` at `bpm` lasts: a whole number of beats when within 5% of
/// one (a name's tempo is rounded), else exactly.
pub fn loop_beats(seconds: f64, bpm: f64) -> f64 {
    let beats = seconds * bpm / 60.0;
    let whole = beats.round();
    if whole >= 1.0 && (beats - whole).abs() <= 0.05 * whole { whole } else { beats }
}

#[cfg(test)]
#[path = "samples_tests.rs"]
mod tests;
