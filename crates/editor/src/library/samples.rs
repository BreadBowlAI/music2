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
//! read. Only WAV and FLAC files are samples for now (FMT-24); other audio files are counted ("WAV
//! and FLAC only for now").
//!
//! Adding one to a song copies its file into `assets/` with a note beside it ([`note`],
//! [`write_note`]), and places a clip, with Follow tempo for a loop whose tempo is known
//! ([`follows`]).

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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

/// Audio files that are neither WAV nor FLAC, by extension: skipped and counted (LIB-4).
const NOT_WAV: [&str; 8] = ["mp3", "aif", "aiff", "ogg", "m4a", "aac", "opus", "wma"];

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

/// A word of a name ([`words`]), and whether the next word was run into it at a capital
/// (`Fm` of `FmPiano`).
#[derive(Clone, Debug)]
struct Word {
    text: String,
    run_on: bool,
}

/// A name's words: its runs of letters, digits and the signs `#`, `♯` and `♭`, split again where
/// letters meet digits (`120bpm` is `120` and `bpm`, `Kick01` is `Kick` and `01`) and where a
/// small letter meets a capital (`PianoChords` is `Piano` and `Chords`). Case kept.
fn words(text: &str) -> Vec<Word> {
    let mut out = vec![];
    let mut word = String::new();
    let mut digits: Option<bool> = None;
    let mut small = false;
    for c in text.chars() {
        let sign = matches!(c, '#' | '♯' | '♭');
        if !(c.is_alphanumeric() || sign) {
            if !word.is_empty() {
                out.push(Word { text: std::mem::take(&mut word), run_on: false });
            }
            digits = None;
            small = false;
            continue;
        }
        let digit = c.is_ascii_digit();
        let capital = small && c.is_uppercase();
        if digits.is_some_and(|d| d != digit) || capital {
            out.push(Word { text: std::mem::take(&mut word), run_on: capital });
        }
        word.push(c);
        digits = Some(digit);
        small = c.is_lowercase();
    }
    if !word.is_empty() {
        out.push(Word { text: word, run_on: false });
    }
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
/// `Bb maj`, `A min`). Words of a sentence aren't one: `Am` after `I` (`I Am Free`), or a key word
/// run into a capitalised word (`FmPiano`: FM synthesis).
fn key_in(words: &[Word]) -> Option<Key> {
    for (i, w) in words.iter().enumerate() {
        if i > 0 && words[i - 1].text == "I" {
            continue;
        }
        let mode = words.get(i + 1).map(|n| n.text.to_ascii_lowercase());
        let minor = match mode.as_deref() {
            Some("minor" | "min") => Some(true),
            Some("major" | "maj") => Some(false),
            _ => None,
        };
        if let (Some(minor), Some((root, _, _, ""))) = (minor, root_of(&w.text)) {
            return Some(Key { root, minor });
        }
        if w.run_on {
            continue;
        }
        if let Some(k) = key_word(&w.text) {
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

/// Units a number in a name may be followed by, which make it no tempo: `96k`, `24bit`, `100 Hz`,
/// `90s`, `-6dB`.
const UNITS: [&str; 15] = ["k", "khz", "hz", "bit", "bits", "kbps", "ms", "s", "sec", "secs", "db", "st", "x", "ch", "khz"];

/// The tempo `words` say: a number beside `bpm` (`90 bpm`, `120bpm`, `bpm 95`) in [`TEMPOS`], or,
/// when `bare`, a number alone in [`BARE_TEMPOS`] with no leading zero (not `01` or `045`), and
/// no unit after it ([`UNITS`]).
fn tempo_in(words: &[Word], bare: bool) -> Option<f64> {
    let lower: Vec<String> = words.iter().map(|w| w.text.to_ascii_lowercase()).collect();
    for (i, w) in lower.iter().enumerate() {
        let beside = lower.get(i + 1).is_some_and(|n| n == "bpm") || i > 0 && lower[i - 1] == "bpm";
        if let Some(n) = number(w).filter(|n| beside && TEMPOS.contains(n)) {
            return Some(n);
        }
    }
    let unit = |i: usize| lower.get(i + 1).is_some_and(|n| UNITS.contains(&n.as_str()));
    bare.then(|| {
        lower
            .iter()
            .enumerate()
            .filter(|(i, w)| !w.starts_with('0') && !unit(*i))
            .find_map(|(_, w)| number(w).filter(|n| BARE_TEMPOS.contains(n)))
    })?
}

/// The instrument the lowercase `words` name: the first of [`INSTRUMENTS`] one of them says
/// (`bass drum` and `hi hat` as one). A folder's `808` names none: a folder of 808s is a drum kit
/// as often as basses.
fn instrument_in(words: &[String], folder: bool) -> Option<&'static str> {
    if words.windows(2).any(|p| p[0] == "bass" && p[1] == "drum") {
        return Some("kick");
    }
    let says = |w: &String, names: &[&str]| names.contains(&w.as_str()) && !(folder && w == "808");
    INSTRUMENTS.iter().zip(NAMES).find(|(_, names)| words.iter().any(|w| says(w, names))).map(|(i, _)| *i)
}

/// The type the lowercase `words` of one name say, if any: a one-shot word (`one-shot`, `oneshot`,
/// `one shots`, `hit`, `hits`, `shot`, `shots`) before a loop word (`loop`, `loops`).
fn type_in(words: &[String]) -> Option<SampleType> {
    let one_shot = words.windows(2).any(|p| p[0] == "one" && (p[1] == "shot" || p[1] == "shots"))
        || words.iter().any(|w| matches!(w.as_str(), "oneshot" | "oneshots" | "hit" | "hits" | "shot" | "shots"));
    if one_shot {
        return Some(SampleType::OneShot);
    }
    words.iter().any(|w| w == "loop" || w == "loops").then_some(SampleType::Loop)
}

/// How many beats of a bar a loop that is a loop by its tempo alone must last, at least: one bar
/// of 4 beats, less 5%.
const LOOP_BEATS: f64 = 4.0 * 0.95;

/// What a sample's path inside its folder says of it (LIB-4), `/` between folders, as if it were
/// long enough to be a loop ([`guess_with`] knows its length): `loops/dusty keys 90 Am.wav` is a
/// loop of keys at 90 bpm in A minor.
pub fn guess(path: &str) -> Guess {
    guess_with(path, None)
}

/// What a sample's path inside its folder, and its length in seconds when known, say of it
/// (LIB-4).
/// - A tempo: a number beside `bpm` in the name, else in a folder (deepest first); else a bare
///   number of the name from 60 to 200 when the path has a key or a loop word, never one with a
///   unit after it ([`UNITS`]).
/// - A key: the name's, else a folder's ([`key_in`]).
/// - Its type, from evidence of its own: the nearest of its name and its folders (deepest first)
///   with a type word ([`type_in`]); with none, a loop when its own name has a tempo and it lasts
///   at least [`LOOP_BEATS`] at it; else a one-shot. A folder's tempo alone makes no loop.
/// - Its instrument: from the name's words, else a folder's, deepest first ([`instrument_in`]).
pub fn guess_with(path: &str, seconds: Option<f64>) -> Guess {
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    let Some((file, folders)) = parts.split_last() else { return Guess::default() };
    let stem = file.rsplit_once('.').map_or(*file, |(s, _)| s);
    let name = words(stem);
    let dirs: Vec<Vec<Word>> = folders.iter().rev().map(|f| words(f)).collect();
    let lower = |w: &[Word]| w.iter().map(|x| x.text.to_lowercase()).collect::<Vec<_>>();
    let typed = std::iter::once(&name).chain(&dirs).find_map(|w| type_in(&lower(w)));
    let key = key_in(&name).or_else(|| dirs.iter().find_map(|d| key_in(d)));
    let bare = typed != Some(SampleType::OneShot) && (typed == Some(SampleType::Loop) || key.is_some());
    let own = tempo_in(&name, false).or_else(|| tempo_in(&name, bare));
    let bpm = tempo_in(&name, false).or_else(|| dirs.iter().find_map(|d| tempo_in(d, false))).or(own);
    let long_enough = |bpm: f64| seconds.is_none_or(|s| s * bpm / 60.0 >= LOOP_BEATS);
    let kind = match typed {
        Some(t) => t,
        None if own.is_some_and(long_enough) => SampleType::Loop,
        None => SampleType::OneShot,
    };
    let instrument = instrument_in(&lower(&name), false).or_else(|| dirs.iter().find_map(|d| instrument_in(&lower(d), true)));
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

/// What `samples.toml` writes for a key or an instrument to take back a guess: there is none.
pub const NONE: &str = "none";

/// What `samples.toml` says of some samples: each field it gives wins over the guess; `none` for a
/// key or an instrument takes back the guess.
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
            if let Some(i) = s.instrument.as_deref().filter(|i| *i != NONE && !INSTRUMENTS.contains(i)) {
                problems.push(format!("{SAMPLES_TOML} {at}: instrument {i} isn't one of {} (or none)", INSTRUMENTS.join(", ")));
            }
            if let Some(b) = s.bpm.filter(|b| !TEMPOS.contains(b)) {
                problems.push(format!("{SAMPLES_TOML} {at}: bpm is 20 to 300, not {b}"));
            }
            if let Some(k) = s.key.as_deref().filter(|k| *k != NONE && key_said(k).is_none()) {
                problems.push(format!("{SAMPLES_TOML} {at}: key {k} doesn't read: write it as Am, F#, Bb minor or C (or none)"));
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
                out.push(format!("{SAMPLES_TOML} names the folder {f}, but samples/{f} holds no WAV or FLAC file"));
            }
        }
        for f in self.file.keys().filter(|f| !found.contains(f)) {
            out.push(format!("{SAMPLES_TOML} names the file {f}, but there is no WAV or FLAC file samples/{f}"));
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

/// Measure the WAV or FLAC file at `path` (told by its first bytes, FMT-24), reading it once to its
/// end as it will play: why it can't be, in words after its name (`doesn't read as a WAV file
/// (…)`). A file the library can't offer is refused as an import refuses it: a rate outside 8,000
/// to 384,000 Hz, over 3 hours, a sample that isn't a number, a FLAC music2 doesn't read.
pub fn measure(path: &Path) -> Result<Measured, String> {
    use m2_runtime::decode::Kind;
    match m2_runtime::decode::kind_of(path).map_err(|e| format!("can't be read ({e})"))? {
        Some(Kind::Wav) => measure_wav(path).map_err(|why| format!("doesn't read as a WAV file ({why})")),
        Some(Kind::Flac) => measure_flac(path).map_err(|why| format!("doesn't read as a FLAC file ({why})")),
        None => Err("isn't a WAV or FLAC file".into()),
    }
}

/// Whether a file of `frames` at `rate` is one the library offers, as an import takes it: why not.
fn offered(frames: u64, rate: u32) -> Result<(), String> {
    if frames == 0 {
        return Err("it holds no samples".into());
    }
    if !crate::import::RATES.contains(&rate) {
        return Err(format!("it plays at {rate} Hz: music2 takes files from 8,000 to 384,000 Hz"));
    }
    if frames as f64 / rate as f64 > crate::import::LONGEST {
        return Err("it is longer than 3 hours: music2 takes files up to 3 hours".into());
    }
    Ok(())
}

/// [`measure`] of a FLAC: its header checked before a sample is decoded, then each block in turn,
/// its first two channels' peak as the WAV of it would give.
fn measure_flac(path: &Path) -> Result<Measured, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut f = m2_runtime::decode::Flac::new(file).map_err(|e| format!("{e:#}"))?;
    offered(f.frames, f.rate)?;
    let (scale, keep) = (m2_runtime::decode::scale(f.bits), (f.channels as usize).min(2));
    let (mut block, mut peak) = (vec![], 0f32);
    while f.next(&mut block).map_err(|e| format!("{e:#}"))? > 0 {
        peak = block[..keep].iter().flatten().fold(peak, |p, &v| p.max((v as f32 * scale).abs()));
    }
    let hash = m2_song::Sha::of_file(path).map_err(|e| e.to_string())?.hex();
    Ok(Measured { seconds: f.frames as f64 / f.rate as f64, channels: keep as u16, rate: f.rate, peak: peak as f64, hash })
}

/// [`measure`] of a WAV, read in pieces as it plays.
fn measure_wav(path: &Path) -> Result<Measured, String> {
    let wav = m2_runtime::stream::Wav::open(path).map_err(|e| format!("{e:#}"))?;
    let rate = wav.rate as u32;
    offered(wav.frames(), rate)?;
    let seconds = wav.frames() as f64 / wav.rate;
    let (mut bytes, mut piece) = (vec![], vec![vec![]; wav.channels()]);
    let (mut at, mut peak) = (0, 0f32);
    while at < wav.frames() {
        let to = wav.frames().min(at + PIECE);
        wav.read(at, to, &mut bytes, &mut piece).map_err(|e| format!("{e:#}"))?;
        peak = piece.iter().flatten().fold(peak, |p, s| p.max(s.abs()));
        if !peak.is_finite() {
            return Err("it holds a sample that isn't a number".into());
        }
        at = to;
    }
    let hash = m2_song::Sha::of_file(path).map_err(|e| e.to_string())?.hex();
    Ok(Measured { seconds, channels: wav.channels() as u16, rate, peak: peak as f64, hash })
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
    /// Audio files that are neither WAV nor FLAC, skipped: "WAV and FLAC only for now".
    pub not_wav: usize,
    /// WAV and FLAC files that don't read, and why.
    pub unreadable: Vec<String>,
    /// What is wrong with the package's `samples.toml`, in words.
    pub problems: Vec<String>,
    /// How many files were read this time; the rest came from the cache.
    pub measured: usize,
    /// The folder isn't there, or can't be read (a drive unplugged): nothing was indexed, and the
    /// cache was kept.
    pub missing: bool,
    /// It was asked to stop (the folder was removed): it is unfinished, and wrote no cache.
    pub stopped: bool,
}

/// The cache's file for a folder (LIB-5): the folder, whether it is a package's, and its measured
/// files by their exact paths ([`path_key`]), each with its size and modified time.
#[derive(Default, Serialize)]
struct Kept {
    root: String,
    package: bool,
    files: BTreeMap<String, KeptFile>,
}

#[derive(Clone, Serialize, Deserialize)]
struct KeptFile {
    size: u64,
    modified: u64,
    measured: Measured,
}

impl Kept {
    /// The cache file at `file`, read entry by entry: an entry that doesn't read (or measures that
    /// aren't numbers) costs only itself. Empty when there is none.
    fn read(file: &Path) -> BTreeMap<String, KeptFile> {
        let Some(v) = std::fs::read(file).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) else { return BTreeMap::new() };
        let files = v.get("files").and_then(Value::as_object).cloned().unwrap_or_default();
        files
            .into_iter()
            .filter_map(|(k, e)| serde_json::from_value::<KeptFile>(e).ok().map(|e| (k, e)))
            .filter(|(_, e)| e.measured.seconds.is_finite() && e.measured.peak.is_finite())
            .collect()
    }
}

/// A file found under a root: its path inside it (`/` between folders), its path, size and
/// modified time (nanoseconds since 1970).
struct Found {
    rel: String,
    path: PathBuf,
    size: u64,
    modified: u64,
}

/// What [`files_in`] found under a folder.
#[derive(Default)]
struct Walked {
    /// The WAV and FLAC files, by sorted path.
    wavs: Vec<Found>,
    /// Other audio files, by extension ([`NOT_WAV`]).
    not_wav: usize,
    /// Files whose name isn't UTF-8, under a package's folder: no `samples.toml` can name them.
    unnamed: Vec<String>,
    /// The folders under it that couldn't be read, by their paths inside it.
    unread: Vec<String>,
}

/// The WAV and FLAC files under `dir`, by sorted path, and the other audio files counted: only folders and
/// plain files, never inside a hidden folder (`.git`), never through a link, never a pipe or a
/// device (which would block a read for good). None, if `dir` can't be read.
fn files_in(dir: &Path) -> Option<Walked> {
    let mut out = Walked::default();
    let mut stack = vec![(dir.to_path_buf(), String::new())];
    while let Some((d, prefix)) = stack.pop() {
        let entries = match std::fs::read_dir(&d) {
            Ok(e) => e,
            Err(_) if prefix.is_empty() => return None,
            Err(_) => {
                out.unread.push(prefix);
                continue;
            }
        };
        for e in entries.flatten() {
            let os_name = e.file_name();
            let name = os_name.to_string_lossy().into_owned();
            let Ok(t) = e.file_type() else { continue };
            if name.starts_with('.') || !(t.is_dir() || t.is_file()) {
                continue;
            }
            let rel = if prefix.is_empty() { name } else { format!("{prefix}/{}", name) };
            if t.is_dir() {
                stack.push((e.path(), rel));
                continue;
            }
            if !is_audio_name(&rel) {
                out.not_wav += usize::from(NOT_WAV.contains(&extension(&rel).as_str()));
                continue;
            }
            if os_name.to_str().is_none() {
                out.unnamed.push(rel.clone());
            }
            if let Ok(m) = e.metadata() {
                let modified = m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos() as u64);
                out.wavs.push(Found { rel, path: e.path(), size: m.len(), modified });
            }
        }
    }
    out.wavs.sort_by(|a, b| a.rel.cmp(&b.rel));
    Some(out)
}

/// A file's extension, lowercase.
fn extension(rel: &str) -> String {
    Path::new(rel).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

/// Whether a file may be a sample, by its name in any capitals: a WAV's (`.wav`, `.WAV`, `.wave`)
/// or a FLAC's (`.flac`). What it is, is told by its first bytes when it is measured (FMT-24).
pub fn is_audio_name(rel: &str) -> bool {
    matches!(extension(rel).as_str(), "wav" | "wave" | "flac")
}

/// A path as an id and the cache write it, exactly: the path itself when it is UTF-8; else as
/// shown, then `#` and a short hash of its exact bytes, so two names that differ only in bytes
/// that aren't UTF-8 are two samples (LIB-4).
pub fn path_key(path: &Path) -> String {
    match path.to_str() {
        Some(p) => p.to_string(),
        None => format!("{}#{}", path.to_string_lossy(), &m2_song::Sha::of(path.as_os_str().as_encoded_bytes()).hex()[..16]),
    }
}

/// The cache's file for the folder `base`, in `cache`: named by a hash of its exact path.
pub fn cache_file(cache: &Path, base: &Path) -> PathBuf {
    let named = m2_song::Sha::of(base.as_os_str().as_encoded_bytes()).hex();
    cache.join(format!("{}.json", &named[..16]))
}

/// Where a root's samples are: a package's `samples/`, or the folder itself.
pub fn base_of(root: Root) -> PathBuf {
    match root {
        Root::Package { dir, .. } => dir.join(SAMPLES_DIR),
        Root::Folder(dir) => dir.to_path_buf(),
    }
}

/// The samples of `root` (LIB-4, LIB-5): every WAV and FLAC file under its folder (a package's `samples/`),
/// measured, or found in the cache folder `cache` by its exact path, size and modified time; then
/// what the package's `samples.toml` says of each, and the guess for the rest. The cache is written
/// again when a file was measured or is gone, keeping the entries of a subfolder that couldn't be
/// read. Other audio files are counted, and a file that doesn't read is named. A folder of yours
/// that isn't there is `missing`: nothing indexed, the cache kept.
pub fn index(root: Root, cache: Option<&Path>) -> Indexed {
    index_until(root, cache, &AtomicBool::new(false))
}

/// [`index`], stopping between two files once `stop` is set: then it is `stopped`, and writes no
/// cache.
pub fn index_until(root: Root, cache: Option<&Path>, stop: &AtomicBool) -> Indexed {
    let base = base_of(root);
    let mut out = Indexed::default();
    let (said, origin) = match root {
        Root::Package { name, dir } => {
            // A package's samples are in a folder of its own, never one a link points at (which
            // could be anywhere: `/`).
            if !std::fs::symlink_metadata(&base).is_ok_and(|m| m.is_dir()) {
                if std::fs::symlink_metadata(&base).is_ok_and(|m| m.file_type().is_symlink()) {
                    out.problems.push(format!("{SAMPLES_DIR} is a link: a pack's samples are listed only from a folder of its own"));
                }
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
    let Some(walked) = files_in(&base) else {
        out.missing = true;
        return out;
    };
    let listed = |rel: &str| matches!(root, Root::Folder(_)) || said.names(rel);
    let kept_at = cache.map(|c| cache_file(c, &base));
    let mut kept = kept_at.as_deref().map(Kept::read).unwrap_or_default();
    let mut now = Kept { root: path_key(&base), package: matches!(root, Root::Package { .. }), files: BTreeMap::new() };
    out.not_wav = walked.not_wav;
    let mut wavs = vec![];
    for f in &walked.wavs {
        if stop.load(Ordering::Relaxed) {
            out.stopped = true;
            return out;
        }
        wavs.push(f.rel.clone());
        if !listed(&f.rel) {
            continue;
        }
        if matches!(root, Root::Package { .. }) && walked.unnamed.contains(&f.rel) {
            out.unreadable.push(format!("{} has a name that isn't UTF-8", f.rel));
            continue;
        }
        let key = path_key(&f.path);
        let measured = match kept.remove(&key).filter(|k| k.size == f.size && k.modified == f.modified) {
            Some(k) => k.measured,
            None => {
                out.measured += 1;
                match measure(&f.path) {
                    Ok(m) => m,
                    Err(why) => {
                        out.unreadable.push(format!("{} {why}", f.rel));
                        continue;
                    }
                }
            }
        };
        now.files.insert(key, KeptFile { size: f.size, modified: f.modified, measured: measured.clone() });
        let mut item = item(&origin, &f.rel, &f.path, &measured, &said.of(&f.rel));
        if let Some(s) = item.sample.as_mut() {
            s.stamp = (f.size, f.modified);
        }
        out.items.push(item);
    }
    if matches!(root, Root::Package { .. }) {
        out.problems.extend(said.missing(&wavs));
    }
    // A subfolder that couldn't be read this time keeps its files' entries.
    let unread: Vec<String> = walked.unread.iter().map(|r| format!("{}/", path_key(&base.join(r)))).collect();
    let (gone, held): (Vec<_>, Vec<_>) = std::mem::take(&mut kept).into_iter().partition(|(k, _)| !unread.iter().any(|u| k.starts_with(u)));
    now.files.extend(held);
    // Written again only when something changed: a file measured, or one gone.
    if let Some(file) = kept_at.filter(|_| out.measured > 0 || !gone.is_empty()) {
        let _ = keep(&file, &now);
    }
    out
}

/// Delete the cache files in `cache` (LIB-5) of a folder of yours that isn't one of `folders` any
/// more, of a package's folder that is gone (a pack's version removed), or that don't read; and
/// part files an index stopped mid-way left over an hour ago. What it deleted.
pub fn prune(cache: &Path, folders: &[PathBuf]) -> usize {
    let yours: Vec<String> = folders.iter().map(|f| path_key(f)).collect();
    let mut gone = 0;
    for e in std::fs::read_dir(cache).into_iter().flatten().flatten() {
        let (path, name) = (e.path(), e.file_name().to_string_lossy().into_owned());
        let old = || e.metadata().and_then(|m| m.modified()).is_ok_and(|t| t.elapsed().is_ok_and(|a| a > STALE_PART));
        let drop = if name.ends_with(".part") {
            old()
        } else if name.ends_with(".json") {
            let v = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok());
            let root = v.as_ref().and_then(|v| v["root"].as_str()).filter(|r| !r.is_empty());
            match (root, v.as_ref().and_then(|v| v["package"].as_bool())) {
                (Some(root), Some(true)) => !Path::new(root).is_dir(),
                (Some(root), Some(false)) => !yours.iter().any(|y| y == root),
                _ => true,
            }
        } else {
            false
        };
        if drop && std::fs::remove_file(&path).is_ok() {
            gone += 1;
        }
    }
    gone
}

/// How old a part file of the cache must be to be one an index left when it stopped mid-way.
const STALE_PART: std::time::Duration = std::time::Duration::from_secs(3600);

/// Delete the cache file of the folder `dir` of yours from `cache` (LIB-5): it was removed.
pub fn forget(cache: &Path, dir: &Path) {
    let _ = std::fs::remove_file(cache_file(cache, dir));
}

/// What `m2 pack check` finds wrong with the samples of the pack in `dir` (PACK-12, LIB-4), in
/// words: every WAV and FLAC under `samples/` decodes, and `samples.toml` reads strictly and names only
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
    for f in files_in(&dir.join(SAMPLES_DIR)).map(|w| w.wavs).unwrap_or_default() {
        if let Err(why) = measure(&f.path) {
            out.push(format!("{SAMPLES_DIR}/{} {why}", f.rel));
        }
        wavs.push(f.rel);
    }
    out.extend(said.missing(&wavs));
    let any = wavs.iter().any(|w| said.names(w));
    (out, any)
}

/// Write the cache's file whole, through a part file of its own (the process's and a count's in
/// its name, so two writers never share one) renamed over it.
fn keep(file: &Path, kept: &Kept) -> std::io::Result<()> {
    static N: AtomicUsize = AtomicUsize::new(0);
    std::fs::create_dir_all(file.parent().expect("a file in the cache"))?;
    let part = file.with_extension(format!("json.{}.{}.part", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let written = (|| {
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&part)?;
        f.write_all(&serde_json::to_vec(kept).map_err(std::io::Error::other)?)?;
        drop(f);
        std::fs::rename(&part, file)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    written
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
    let g = guess_with(rel, Some(m.seconds));
    let kind = match said.kind.as_deref() {
        Some("loop") => SampleType::Loop,
        Some("one-shot") => SampleType::OneShot,
        _ => g.kind,
    };
    // `none` takes back the guess.
    let instrument = match said.instrument.as_deref() {
        Some(NONE) => None,
        Some(i) => Some(i.to_string()),
        None => g.instrument.map(String::from),
    };
    let bpm = said.bpm.or(g.bpm).filter(|_| kind == SampleType::Loop || said.bpm.is_some());
    let key = match said.key.as_deref() {
        Some(NONE) => None,
        Some(k) => key_said(k),
        None => g.key,
    };
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
        Origin::Folder => (format!("sample:folder:{}", path_key(path)), "yours".to_string(), String::new(), json!({ "folder": true })),
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
            stamp: (0, 0),
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

/// Whether `note` is a note of yours, which credits no one: `{"from": {"folder": true}}`, or an
/// import's that says only what its WAV was made from (FMT-24), with that or alone.
fn yours(note: &Value) -> bool {
    let Some(o) = note.as_object() else { return false };
    !o.is_empty() && o.keys().all(|k| k == "from" || k == "made_from") && o.get("from").is_none_or(|f| f == &json!({ "folder": true }))
}

/// Write `note` beside asset `asset` in the song in `dir` (LIB-7): created only if it isn't there
/// (FMT-14), its bytes pushed and its folder synced before anything names the asset (WRITE-1). A
/// note already there is kept, but for a note of yours, which a package's note replaces (the same
/// bytes came from a pack: its credit is never lost), through a part file renamed over it.
/// Whether it was written.
pub fn write_note(dir: &Path, asset: &str, note: &Value) -> std::io::Result<bool> {
    let assets = dir.join(m2_song::path::ASSETS);
    let path = assets.join(note_name(asset));
    let there = match std::fs::read(&path) {
        Ok(b) => Some(serde_json::from_slice::<Value>(&b).ok()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    let replace = match &there {
        None => false,
        Some(Some(old)) if yours(old) && note["from"].get("pack").is_some() => true,
        Some(_) => return Ok(false),
    };
    // A fresh name of its own, create-only: hidden, so no sweep or import takes it for an asset.
    static N: AtomicUsize = AtomicUsize::new(0);
    let to = if replace {
        assets.join(format!(".{}.{}.{}.part", note_name(asset), std::process::id(), N.fetch_add(1, Ordering::Relaxed)))
    } else {
        path.clone()
    };
    let mut f = match std::fs::OpenOptions::new().write(true).create_new(true).open(&to) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && !replace => return Ok(false),
        Err(e) => return Err(e),
    };
    let text = serde_json::to_string_pretty(note).map_err(std::io::Error::other)? + "\n";
    let written = f.write_all(text.as_bytes()).and_then(|()| m2_os::fs::push(&f));
    drop(f);
    let written = written.and_then(|()| if replace { std::fs::rename(&to, &path) } else { Ok(()) });
    if let Err(e) = written {
        let _ = std::fs::remove_file(&to);
        return Err(e);
    }
    m2_os::fs::sync_dir(&assets)?;
    Ok(true)
}

/// The tempo a sample's clip follows when added (LIB-7): a loop's own, when it is known, at the
/// song's tempo too, so it follows a later change of tempo.
pub fn follows(sample: &Sample) -> Option<f64> {
    sample.bpm.filter(|_| sample.kind == SampleType::Loop)
}

/// How many beats a loop of `seconds` at `bpm` lasts, played at `rate`: its exact length, or a
/// whole number of beats when that is within one sample of it.
pub fn loop_beats(seconds: f64, bpm: f64, rate: u32) -> f64 {
    let beats = seconds * bpm / 60.0;
    let whole = beats.round();
    // How far it is from the whole beats, in samples.
    let off = (beats - whole).abs() * 60.0 / bpm * rate as f64;
    if whole >= 1.0 && off <= 1.0 + 1e-6 { whole } else { beats }
}

/// The longest file a sample's preview is (LIB-6), in bytes: the page holds a preview whole.
pub const PREVIEW_BYTES: u64 = 64 * 1024 * 1024;

/// Whether sample `s`'s file is still as it was indexed (LIB-6): a plain file at its path, never a
/// link (one put there since, which could point anywhere) nor a pipe. Its stamp, links not
/// followed; why not, in words.
pub fn still_plain(s: &Sample) -> Result<m2_os::fs::Stamp, String> {
    let gone = || format!("{} isn't there any more", s.file.display());
    let m = std::fs::symlink_metadata(&s.file).map_err(|_| gone())?;
    if !m.file_type().is_file() {
        return Err(format!("{} isn't a plain file any more: a sample is only ever read where it was found", s.file.display()));
    }
    Ok(m2_os::fs::Stamp::of(&m))
}

/// A FLAC sample's preview (LIB-6, FMT-24): the WAV of its samples, made now, so the page needs no
/// FLAC decoder; none for a WAV, which is its own preview. The file opened must be the one stamped
/// `stamp` when it was checked (never a link put there since), and the WAV it makes at most
/// [`PREVIEW_BYTES`], from the FLAC's header, before a sample is decoded. Why not, in words.
pub fn flac_preview(s: &Sample, stamp: m2_os::fs::Stamp) -> Result<Option<Vec<u8>>, String> {
    use m2_runtime::decode::{self, Flac, Kind};
    let file = std::fs::File::open(&s.file).map_err(|e| e.to_string())?;
    if file.metadata().map(|m| m2_os::fs::Stamp::of(&m)).ok() != Some(stamp) {
        return Err(format!("{} changed as it was asked for: a sample is only ever read where it was found", s.file.display()));
    }
    let mut head = [0u8; decode::HEAD];
    let n = m2_os::fs::read_full_at(&file, &mut head, 0, decode::HEAD).map_err(|e| e.to_string())?;
    if decode::kind(&head[..n]) != Some(Kind::Flac) {
        return Ok(None);
    }
    let flac = Flac::new(std::io::BufReader::new(file)).map_err(|e| format!("{e:#}"))?;
    let wav = flac.frames * flac.channels as u64 * (flac.bits as u64 / 8) + 44;
    if wav > PREVIEW_BYTES {
        return Err(format!(
            "as a WAV it is {} MB, too long to preview here (up to {} MB): add it to the song to hear it",
            wav / 1_000_000,
            PREVIEW_BYTES / 1_000_000
        ));
    }
    let mut out = std::io::Cursor::new(Vec::with_capacity(wav as usize));
    decode::write_wav(flac, &mut out).map_err(|e| format!("{e:#}"))?;
    Ok(Some(out.into_inner()))
}

#[cfg(test)]
#[path = "samples_tests.rs"]
mod tests;
