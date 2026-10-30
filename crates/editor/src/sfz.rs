//! SFZ instruments (PACK-14): one parser for SFZ, the open text format of free sample instruments,
//! and `m2 pack import-sfz`, which makes a pack of one: its regions as a data instrument (PACK-13).
//!
//! ```text
//! parse      the file, its #include files and #define names, into regions, each with every opcode
//!            of the headers above it (<control> <global> <master> <group> <region>)
//! map        each region into PKG-15's fields, or why it is left out; and what is ignored
//! import     the pack: m2pkg.toml, instruments/<id>.toml, samples/, README.md, LICENSE
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use m2_song::pack::PackName;

/// One region as the file gives it: every opcode of the headers above it, its own last, and where
/// it is.
#[derive(Clone, Debug, PartialEq)]
pub struct SfzRegion {
    pub opcodes: BTreeMap<String, String>,
    /// `default_path` from `<control>` when the region was read.
    pub default_path: String,
    /// The file and line of its `<region>` header.
    pub at: String,
}

/// A parsed SFZ file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sfz {
    pub regions: Vec<SfzRegion>,
    /// The `<control>` opcodes in force at the end (`set_cc…`, `default_path`).
    pub control: BTreeMap<String, String>,
    /// The comment lines before the first header of the main file: the credits, often.
    pub comments: Vec<String>,
    /// Sections music2 doesn't read (`<curve>`, `<effect>` …), by header, with how many.
    pub other_headers: BTreeMap<String, usize>,
}

/// How deep `#include` may go: deeper is a loop.
const DEPTH: usize = 16;

/// Parse the SFZ file at `path`, with what it includes (relative to its folder).
pub fn parse(path: &Path) -> Result<Sfz, String> {
    let root = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let mut p = Parser {
        root,
        defines: vec![],
        sfz: Sfz::default(),
        level: Level::None,
        maps: Default::default(),
        region: None,
        seen_header: false,
    };
    p.file(path, 0)?;
    p.end_region();
    Ok(p.sfz)
}

/// Where opcodes go: the header last read.
#[derive(Clone, Copy, PartialEq)]
enum Level {
    None,
    Control,
    Global,
    Master,
    Group,
    Region,
    Other,
}

struct Parser {
    root: PathBuf,
    /// `#define $NAME value`, in order: a later one replaces an earlier one.
    defines: Vec<(String, String)>,
    sfz: Sfz,
    level: Level,
    /// `<control>`, `<global>`, `<master>`, `<group>`.
    maps: [BTreeMap<String, String>; 4],
    region: Option<SfzRegion>,
    seen_header: bool,
}

impl Parser {
    fn file(&mut self, path: &Path, depth: usize) -> Result<(), String> {
        if depth > DEPTH {
            return Err(format!("{}: #include goes {DEPTH} files deep: does it include itself?", path.display()));
        }
        let bytes = std::fs::read(path).map_err(|e| format!("{} can't be read: {e}", path.display()))?;
        let text = String::from_utf8_lossy(&bytes).replace("\r\n", "\n").replace('\r', "\n");
        let text = without_block_comments(&text);
        let shown = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
        for (n, raw) in text.lines().enumerate() {
            let (line, comment) = match raw.find("//") {
                Some(i) => (&raw[..i], Some(raw[i + 2..].trim())),
                None => (raw, None),
            };
            self.line(line, &format!("{shown}:{}", n + 1), depth)?;
            if depth == 0
                && !self.seen_header
                && let Some(c) = comment
            {
                self.sfz.comments.push(c.to_string());
            }
        }
        Ok(())
    }

    /// One line, its comment cut: headers, `#define`, `#include` and `name=value` opcodes, any
    /// number of each, in order.
    fn line(&mut self, line: &str, at: &str, depth: usize) -> Result<(), String> {
        let mut rest = line.trim_start();
        while !rest.is_empty() {
            if let Some(r) = rest.strip_prefix('<') {
                let end = r.find('>').ok_or_else(|| format!("{at}: a header `<{r}` has no `>`"))?;
                self.header(r[..end].trim(), at);
                rest = r[end + 1..].trim_start();
            } else if let Some(r) = rest.strip_prefix("#define") {
                let mut parts = r.split_whitespace();
                let (Some(name), Some(value)) = (parts.next(), parts.next()) else {
                    return Err(format!("{at}: `#define` needs a name and a value, as `#define $VEL 100`"));
                };
                let value = self.defined(value);
                self.defines.retain(|(n, _)| n != name);
                self.defines.push((name.to_string(), value));
                let used = r.find(name).map_or(r.len(), |i| i + name.len());
                let after = r[used..].trim_start();
                rest = after.get(after.find(char::is_whitespace).unwrap_or(after.len())..).unwrap_or("").trim_start();
            } else if let Some(r) = rest.strip_prefix("#include") {
                let r = r.trim_start();
                let quoted = r.strip_prefix('"').and_then(|q| q.find('"').map(|e| (&q[..e], &q[e + 1..])));
                let Some((file, after)) = quoted else {
                    return Err(format!("{at}: `#include` needs a file in quotes, as `#include \"notes.sfz\"`"));
                };
                let file = self.defined(file).replace('\\', "/");
                let path = self.root.join(&file);
                self.file(&path, depth + 1).map_err(|e| format!("{at}: {e}"))?;
                rest = after.trim_start();
            } else {
                let eq = rest.find('=').ok_or_else(|| format!("{at}: `{}` isn't an opcode (`name=value`) or a header", rest.trim()))?;
                let name = self.defined(rest[..eq].trim());
                let after = &rest[eq + 1..];
                let end = value_end(after);
                let value = self.defined(after[..end].trim());
                self.opcode(name, value);
                rest = after[end..].trim_start();
            }
        }
        Ok(())
    }

    /// `text` with every `$NAME` defined so far replaced, the longest names first.
    fn defined(&self, text: &str) -> String {
        if !text.contains('$') {
            return text.to_string();
        }
        let mut defs: Vec<&(String, String)> = self.defines.iter().collect();
        defs.sort_by_key(|(n, _)| std::cmp::Reverse(n.len()));
        let mut out = text.to_string();
        for (n, v) in defs {
            out = out.replace(n.as_str(), v);
        }
        out
    }

    fn header(&mut self, name: &str, at: &str) {
        self.end_region();
        self.seen_header = true;
        let (level, clear) = match name {
            "control" => (Level::Control, 0),
            "global" => (Level::Global, 1),
            "master" => (Level::Master, 2),
            "group" => (Level::Group, 3),
            "region" => (Level::Region, 4),
            other => {
                *self.sfz.other_headers.entry(format!("<{other}>")).or_default() += 1;
                (Level::Other, 4)
            }
        };
        // A header starts its level afresh, and every level under it.
        for m in self.maps.iter_mut().skip(clear) {
            m.clear();
        }
        if level == Level::Region {
            let mut opcodes = BTreeMap::new();
            for m in &self.maps[1..] {
                opcodes.extend(m.iter().map(|(k, v)| (k.clone(), v.clone())));
            }
            let default_path = self.maps[0].get("default_path").cloned().unwrap_or_default();
            self.region = Some(SfzRegion { opcodes, default_path, at: at.to_string() });
        }
        self.level = level;
    }

    fn opcode(&mut self, name: String, value: String) {
        match self.level {
            Level::Region => {
                if let Some(r) = &mut self.region {
                    r.opcodes.insert(name, value);
                }
            }
            Level::Control => {
                self.sfz.control.insert(name.clone(), value.clone());
                self.maps[0].insert(name, value);
            }
            Level::Global => drop(self.maps[1].insert(name, value)),
            Level::Master => drop(self.maps[2].insert(name, value)),
            Level::Group => drop(self.maps[3].insert(name, value)),
            Level::None | Level::Other => {}
        }
    }

    fn end_region(&mut self) {
        if let Some(r) = self.region.take() {
            self.sfz.regions.push(r);
        }
    }
}

/// Where an opcode's value ends in `after` (what follows its `=`): at the next `name=`, header,
/// `#define` or `#include`, so a sample's name may hold spaces (`sample=Grand A0.wav`).
fn value_end(after: &str) -> usize {
    let bytes = after.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' || (bytes[i] == b'#' && (after[i..].starts_with("#define") || after[i..].starts_with("#include"))) {
            return i;
        }
        if bytes[i].is_ascii_whitespace() {
            let word = after[i..].trim_start();
            let skip = after[i..].len() - word.len();
            let name_len = word.bytes().take_while(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'$').count();
            if name_len > 0 && word.as_bytes().get(name_len) == Some(&b'=') {
                return i;
            }
            if word.starts_with('<') || word.starts_with("#define") || word.starts_with("#include") {
                return i + skip;
            }
        }
        i += 1;
    }
    after.len()
}

/// `text` with each `/* … */` comment blanked, its line breaks kept so lines keep their numbers.
fn without_block_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("/*") {
        // A `//` comment before it on its line wins: `// see /* here` is one comment.
        let line_start = rest[..start].rfind('\n').map_or(0, |i| i + 1);
        if rest[line_start..start].contains("//") {
            let next = rest[start..].find('\n').map_or(rest.len(), |i| start + i);
            out.push_str(&rest[..next]);
            rest = &rest[next..];
            continue;
        }
        out.push_str(&rest[..start]);
        let end = rest[start + 2..].find("*/").map_or(rest.len(), |e| start + 2 + e + 2);
        out.extend(rest[start..end].chars().filter(|c| *c == '\n'));
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// A note as SFZ writes it: a number, or a name, `c4` = 60, `c#4` = `db4` = 61, `c-1` = 0.
pub fn note(text: &str) -> Option<i64> {
    let t = text.trim();
    if let Ok(n) = t.parse::<i64>() {
        return Some(n);
    }
    let mut chars = t.chars();
    let pc = match chars.next()?.to_ascii_lowercase() {
        'c' => 0,
        'd' => 2,
        'e' => 4,
        'f' => 5,
        'g' => 7,
        'a' => 9,
        'b' => 11,
        _ => return None,
    };
    let rest: &str = chars.as_str();
    let (acc, octave) = match rest.chars().next() {
        Some('#') | Some('♯') => (1, &rest[rest.chars().next()?.len_utf8()..]),
        Some('b') | Some('♭') if rest.len() > 1 => (-1, &rest[rest.chars().next()?.len_utf8()..]),
        _ => (0, rest),
    };
    let octave: i64 = octave.parse().ok()?;
    Some((octave + 1) * 12 + pc + acc)
}

/// One region of the data instrument (PKG-15's fields), as the import writes it.
#[derive(Clone, Debug, PartialEq)]
pub struct Mapped {
    /// The recording, relative to the SFZ's folder, `/`-separated.
    pub source: String,
    pub root: i64,
    pub keys: (i64, i64),
    pub velocity: (i64, i64),
    pub round_robin: Option<(i64, i64)>,
    pub release: bool,
    pub loop_: Option<(i64, i64)>,
    pub tune: f64,
    pub volume: f64,
    pub pan: f64,
    pub offset: i64,
    pub one_shot: bool,
    /// For the instrument: its `amp_veltrack`, `ampeg_attack` and `ampeg_release`, when given.
    pub veltrack: Option<f64>,
    pub attack: Option<f64>,
    pub release_time: Option<f64>,
    /// Opcodes music2 doesn't play, which this region has.
    pub ignored: Vec<String>,
    /// Said of this region though it is kept (`loop_continuous` plays as `loop_sustain` …).
    pub notes: Vec<String>,
}

/// The opcodes [`map`] reads; any other a region has is ignored, and counted.
const READ: &[&str] = &[
    "sample",
    "key",
    "lokey",
    "hikey",
    "pitch_keycenter",
    "lovel",
    "hivel",
    "seq_length",
    "seq_position",
    "trigger",
    "loop_mode",
    "loopmode",
    "loop_start",
    "loopstart",
    "loop_end",
    "loopend",
    "tune",
    "transpose",
    "volume",
    "pan",
    "offset",
    "amp_veltrack",
    "ampeg_attack",
    "ampeg_release",
    "sw_last",
    "sw_lokey",
    "sw_hikey",
    "sw_default",
    "sw_label",
];

/// What the region plays as, or why it is left out (in words that read after "N regions").
pub fn map(r: &SfzRegion, control: &BTreeMap<String, String>) -> Result<Mapped, String> {
    let o = &r.opcodes;
    let get = |k: &str| o.get(k).map(String::as_str);
    let num = |k: &str| get(k).and_then(|v| v.trim().parse::<f64>().ok());
    let key = |k: &str| get(k).and_then(note);
    let Some(sample) = get("sample") else { return Err("name no sample".into()) };
    if sample.starts_with('*') {
        return Err(format!("play a sound SFZ makes ({sample}) rather than a recording"));
    }
    // Key switches: only the default one's regions.
    let default_sw = get("sw_default").and_then(note);
    if let Some(last) = key("sw_last")
        && default_sw != Some(last)
    {
        return Err(format!("belong to the key switch {}, not the default", note_name(last)));
    }
    for k in ["sw_down", "sw_up", "sw_previous", "sw_lolast", "sw_hilast"] {
        if o.contains_key(k) {
            return Err(format!("need a key held or played before them ({k})"));
        }
    }
    // Conditions on a controller: met at the controller's starting value, else left out.
    for k in o.keys() {
        let cc = |prefix: &str| k.strip_prefix(prefix).and_then(|n| n.parse::<u32>().ok());
        if let Some(n) = cc("on_locc").or_else(|| cc("on_hicc")) {
            return Err(format!("play when a controller moves (CC {n})"));
        }
        if let Some(n) = cc("locc").or_else(|| cc("hicc")) {
            let at_rest = cc_start(control, n);
            let lo = o.get(&format!("locc{n}")).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
            let hi = o.get(&format!("hicc{n}")).and_then(|v| v.parse::<f64>().ok()).unwrap_or(127.0);
            if !(lo..=hi).contains(&at_rest) {
                return Err(format!("play only with a controller moved (CC {n})"));
            }
        }
    }
    // Regions chosen at random: the one for a random value of 0 is kept, the others left out.
    if num("lorand").unwrap_or(0.0) > 0.0 {
        return Err("play at random, and aren't the first choice (lorand, hirand)".into());
    }
    let release = match get("trigger").unwrap_or("attack") {
        "attack" => false,
        "release" => true,
        other => return Err(format!("play on `trigger={other}`, which music2 doesn't")),
    };
    let lo = key("lokey").or_else(|| key("key")).unwrap_or(0);
    let hi = key("hikey").or_else(|| key("key")).unwrap_or(127);
    if lo < 0 || hi < 0 || lo > hi {
        return Err("play on no key".into());
    }
    let root = key("pitch_keycenter").or_else(|| key("key")).unwrap_or(60);
    let lovel = num("lovel").map_or(1, |v| v as i64).max(1);
    let hivel = num("hivel").map_or(127, |v| v as i64);
    if lovel > hivel {
        return Err("play at no velocity".into());
    }
    let mut notes = vec![];
    let round_robin = match (num("seq_length"), num("seq_position")) {
        (Some(len), pos) if len > 1.0 => Some((pos.unwrap_or(1.0) as i64, len as i64)),
        (None, Some(pos)) if pos > 1.0 => return Err("are a later take of a round robin with no `seq_length`".into()),
        _ => None,
    };
    let mode = get("loop_mode").or_else(|| get("loopmode")).unwrap_or("no_loop");
    let points = match (num("loop_start").or_else(|| num("loopstart")), num("loop_end").or_else(|| num("loopend"))) {
        (Some(s), Some(e)) => Some((s as i64, e as i64)),
        _ => None,
    };
    let loop_ = match mode {
        "no_loop" | "one_shot" => None,
        "loop_continuous" | "loop_sustain" => {
            if points.is_none() {
                notes.push("loop on the recording's own loop points, which music2 doesn't read yet: they play without a loop".into());
            } else if mode == "loop_continuous" {
                notes.push("loop on after the note ends (loop_continuous): here their loop ends with the note".into());
            }
            points
        }
        other => return Err(format!("have `loop_mode={other}`, which SFZ doesn't define")),
    };
    let tune = num("tune").unwrap_or(0.0) + num("transpose").unwrap_or(0.0) * 100.0;
    if tune.abs() > 2400.0 {
        return Err("are tuned more than two octaves away".into());
    }
    let volume = num("volume").unwrap_or(0.0).clamp(-120.0, 24.0);
    let pan = (num("pan").unwrap_or(0.0) / 100.0).clamp(-1.0, 1.0);
    if let Some(t) = num("pitch_keytrack")
        && t != 100.0
        && lo != hi
    {
        notes.push("don't follow the key in SFZ (pitch_keytrack): here they are pitched by it".into());
    }
    let ignored = o
        .keys()
        .filter(|k| !READ.contains(&k.as_str()) && !k.starts_with("locc") && !k.starts_with("hicc") && *k != "lorand" && *k != "hirand")
        .cloned()
        .collect();
    Ok(Mapped {
        source: format!("{}{}", r.default_path, sample).replace('\\', "/"),
        root,
        keys: (lo, hi.min(127)),
        velocity: (lovel, hivel.min(127)),
        round_robin,
        release,
        loop_,
        tune,
        volume,
        pan,
        offset: num("offset").map_or(0, |v| v.max(0.0) as i64),
        one_shot: mode == "one_shot",
        veltrack: num("amp_veltrack"),
        attack: num("ampeg_attack"),
        release_time: num("ampeg_release"),
        ignored,
        notes,
    })
}

/// A controller's value before it moves: `set_ccN` (or `set_hdccN`, 0 to 1) in `<control>`, else 0.
fn cc_start(control: &BTreeMap<String, String>, n: u32) -> f64 {
    let read = |k: String| control.get(&k).and_then(|v| v.trim().parse::<f64>().ok());
    read(format!("set_cc{n}")).or_else(|| read(format!("set_hdcc{n}")).map(|v| v * 127.0)).unwrap_or(0.0)
}

/// A note's name as SFZ writes it: 60 is `c4`.
fn note_name(n: i64) -> String {
    const NAMES: [&str; 12] = ["c", "c#", "d", "d#", "e", "f", "f#", "g", "g#", "a", "a#", "b"];
    format!("{}{}", NAMES[n.rem_euclid(12) as usize], n.div_euclid(12) - 1)
}

/// What an import is asked to make.
pub struct Import<'a> {
    pub sfz: &'a Path,
    pub folder: &'a Path,
    pub name: &'a PackName,
    pub label: Option<&'a str>,
    pub category: Option<&'a str>,
    pub license: Option<&'a str>,
    /// The std the pack is made for: its `[dependencies]` asks for it.
    pub std: &'a Path,
}

/// What an import made, and what it couldn't keep, in words.
#[derive(Debug)]
pub struct Imported {
    pub id: String,
    pub regions: usize,
    pub samples: usize,
    pub left_out: Vec<String>,
}

/// Make a pack in `folder` from an SFZ file (PACK-14). Nothing is written until every sample it keeps
/// is found and is a WAV.
pub fn import(i: &Import) -> Result<Imported, String> {
    if i.folder.exists() {
        return Err(format!("{} exists already: name a new folder for the pack", i.folder.display()));
    }
    let sfz = parse(i.sfz)?;
    let dir = i.sfz.parent().unwrap_or(Path::new(".")).to_path_buf();
    let mut kept = vec![];
    let mut dropped: BTreeMap<String, usize> = BTreeMap::new();
    for r in &sfz.regions {
        match map(r, &sfz.control) {
            Ok(m) => kept.push(m),
            Err(why) => *dropped.entry(why).or_default() += 1,
        }
    }
    if kept.is_empty() {
        return Err(format!("{} has no region music2 can play", i.sfz.display()));
    }
    // Every recording: found (in any capitals, as SFZ made on Windows names them), a WAV, and given
    // its path in the pack.
    let mut files: BTreeMap<String, (PathBuf, String)> = BTreeMap::new();
    let mut taken: BTreeMap<String, String> = BTreeMap::new();
    for m in &kept {
        if files.contains_key(&m.source) {
            continue;
        }
        let ext = Path::new(&m.source).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        if ext != "wav" {
            return Err(format!("{} is a {} file: convert it to WAV first; music2 reads only WAV for now", m.source, ext.to_uppercase()));
        }
        let found =
            find(&dir, &m.source).ok_or_else(|| format!("{}, which the SFZ plays, isn't there (looked in {})", m.source, dir.display()))?;
        hound::WavReader::open(&found).map_err(|e| format!("{} isn't a WAV music2 can read: {e}", m.source))?;
        // Under `samples/`, by its path from the SFZ's folder: no `..`, and a first folder that is
        // already called samples (in any capitals) not twice.
        let mut inside: Vec<&str> = m.source.split('/').filter(|s| !s.is_empty() && *s != "." && *s != "..").collect();
        if inside.len() > 1 && inside[0].eq_ignore_ascii_case("samples") {
            inside.remove(0);
        }
        let path = format!("samples/{}", inside.join("/"));
        if let Some(other) = taken.insert(path.to_lowercase(), m.source.clone()) {
            return Err(format!("{} and {other} would both be {path} in the pack", m.source));
        }
        files.insert(m.source.clone(), (found, path));
    }
    let id = i.name.as_str().rsplit('/').next().unwrap_or("instrument").to_string();
    let label =
        i.label.map(String::from).unwrap_or_else(|| i.sfz.file_stem().map_or_else(|| id.clone(), |s| s.to_string_lossy().into_owned()));
    let category = i.category.unwrap_or("Sampler");
    let license_file = ["LICENSE", "LICENSE.txt", "LICENSE.md", "License.txt", "license.txt", "COPYING", "COPYING.txt"]
        .iter()
        .map(|n| dir.join(n))
        .find(|p| p.is_file());
    let license = match (i.license, &license_file) {
        (Some(l), _) => l.to_string(),
        (None, Some(f)) => recognise(&std::fs::read_to_string(f).unwrap_or_default())
            .ok_or_else(|| format!("say the pack's licence: add --license with the one in {} (such as CC-BY-4.0)", f.display()))?
            .to_string(),
        (None, None) => return Err("say the pack's licence, the one its author gave: add --license (such as CC-BY-4.0 or CC0-1.0)".into()),
    };

    // What the instrument takes from its regions: the value most of them have.
    let most = |f: &dyn Fn(&Mapped) -> Option<f64>| -> Option<f64> {
        let mut count: BTreeMap<String, (usize, f64)> = BTreeMap::new();
        for m in kept.iter().filter_map(f) {
            count.entry(format!("{m}")).or_insert((0, m)).0 += 1;
        }
        count.into_values().max_by(|a, b| a.0.cmp(&b.0).then(b.1.total_cmp(&a.1))).map(|(_, v)| v)
    };
    let veltrack = most(&|m| m.veltrack);
    let attack = most(&|m| m.attack);
    let release_time = most(&|m| m.release_time);
    let attacks: Vec<&Mapped> = kept.iter().filter(|m| !m.release).collect();
    let one_shot = !attacks.is_empty() && attacks.iter().all(|m| m.one_shot);

    let mut left_out = vec![];
    for (why, n) in &dropped {
        left_out.push(format!("left out: {n} region{} {}", plural(*n), agree(*n, why)));
    }
    let mut said: BTreeMap<String, usize> = BTreeMap::new();
    for m in &kept {
        for n in &m.notes {
            *said.entry(n.clone()).or_default() += 1;
        }
        if veltrack.is_some() && m.veltrack != veltrack {
            *said
                .entry(format!(
                    "have their own amp_veltrack ({}%): the instrument's velocity is {}% for all",
                    show(m.veltrack.unwrap_or(100.0)),
                    show(veltrack.unwrap_or(100.0))
                ))
                .or_default() += 1;
        }
        if attack.is_some() && m.attack != attack {
            *said
                .entry(format!("have their own ampeg_attack: the instrument's attack is {} s for all", show(attack.unwrap_or(0.0))))
                .or_default() += 1;
        }
        if release_time.is_some() && m.release_time != release_time {
            *said
                .entry(format!("have their own ampeg_release: the instrument's release is {} s for all", show(release_time.unwrap_or(0.0))))
                .or_default() += 1;
        }
        if !m.release && m.one_shot != one_shot {
            let how = if one_shot {
                "stop with their note in SFZ: here they play to their end"
            } else {
                "play to their end in SFZ (one_shot): here they stop with their note"
            };
            *said.entry(how.into()).or_default() += 1;
        }
    }
    for (what, n) in &said {
        left_out.push(format!("changed: {n} region{} {}", plural(*n), agree(*n, what)));
    }
    let mut ignored: BTreeMap<&str, usize> = BTreeMap::new();
    for m in &kept {
        for k in &m.ignored {
            *ignored.entry(k).or_default() += 1;
        }
    }
    if !ignored.is_empty() {
        let list: Vec<String> = ignored.iter().map(|(k, n)| format!("{k} ({n})")).collect();
        left_out.push(format!("ignored, as music2 doesn't play them yet (regions that have each): {}", list.join(", ")));
    }
    for (h, n) in &sfz.other_headers {
        left_out.push(format!("ignored: {n} {h} section{}", plural(*n)));
    }
    if let Some(sw) = sfz.regions.iter().find_map(|r| r.opcodes.get("sw_default")).and_then(|v| note(v)) {
        left_out.push(format!("key switches: kept the default one's regions ({})", note_name(sw)));
    }

    // The instrument.
    let mut t = format!(
        "# Made by `m2 pack import-sfz` from {}.\nlabel = {}\ncategory = {}\ndescription = {}\nengine = \"std:sampler\"\n",
        i.sfz.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
        quote(&label),
        quote(category),
        quote(&format!("{label}, from an SFZ instrument")),
    );
    t.push_str(&format!("mode = {}\n", quote(if one_shot { "oneshot" } else { "gate" })));
    if let Some(a) = attack {
        t.push_str(&format!("attack = {}\n", show(a)));
    }
    if let Some(r) = release_time {
        t.push_str(&format!("release = {}\n", show(r)));
    }
    if let Some(v) = veltrack {
        t.push_str(&format!("velocity = {}\n", show((v / 100.0).clamp(0.0, 1.0))));
    }
    t.push_str("voices = 32\n");
    for m in &kept {
        t.push_str(&format!(
            "\n[[region]]\nfile = {}\nroot = {}\nkeys = [{}, {}]\n",
            quote(&files[&m.source].1),
            m.root,
            m.keys.0,
            m.keys.1
        ));
        if m.velocity != (1, 127) {
            t.push_str(&format!("velocity = [{}, {}]\n", m.velocity.0, m.velocity.1));
        }
        if let Some((p, n)) = m.round_robin {
            t.push_str(&format!("round_robin = [{p}, {n}]\n"));
        }
        if m.release {
            t.push_str("trigger = \"release\"\n");
        }
        if let Some((s, e)) = m.loop_ {
            t.push_str(&format!("loop = [{s}, {e}]\n"));
        }
        for (k, v) in [("tune", m.tune), ("volume", m.volume), ("pan", m.pan)] {
            if v != 0.0 {
                t.push_str(&format!("{k} = {}\n", show(v)));
            }
        }
        if m.offset != 0 {
            t.push_str(&format!("offset = {}\n", m.offset));
        }
    }
    // It must read as a data instrument does.
    m2_song::instrument::DataInstrument::read(&t)
        .map_err(|p| format!("the instrument made isn't right, line {}: {}", p.line, p.message))?;

    let std_req =
        m2_song::Requirement::minor(m2_song::pack::PkgManifest::at(i.std).map(|m| m.package.version).unwrap_or(m2_song::Version(0, 5, 0)));
    let manifest = format!(
        "[package]\nname = \"{}\"\nversion = \"0.1.0\"\nlicense = {}\ndescription = {}\nauthors = []\ntags = []\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"{std_req}\"\n",
        i.name,
        quote(&license),
        quote(&format!("{label}, from an SFZ instrument")),
    );
    let mut readme = format!(
        "# {label}\n\n**In short:** a music2 pack made by `m2 pack import-sfz` from `{}`: one instrument, `{}:{id}`, \
         in `instruments/{id}.toml`, playing {} recordings in `samples/`.\n\nIn a song: `import {{ {} }} from \"{}/instruments\";`\n",
        i.sfz.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
        i.name,
        files.len(),
        m2_song::instrument::Id::new(&id).map(|i| i.binding()).unwrap_or_default(),
        i.name,
    );
    let credits: Vec<&String> =
        sfz.comments.iter().filter(|c| !c.trim().is_empty() && !c.chars().all(|ch| "-=*/_ ".contains(ch))).collect();
    if !credits.is_empty() {
        readme.push_str("\n## Credits\n\nFrom the SFZ file:\n\n");
        for c in credits {
            readme.push_str(&format!("> {c}\n"));
        }
    }
    readme.push_str(&format!("\nLicence: {license}{}.\n", if license_file.is_some() { ", in `LICENSE`" } else { "" }));
    if !left_out.is_empty() {
        readme.push_str("\n## What the import couldn't keep\n\n");
        for l in &left_out {
            readme.push_str(&format!("- {l}\n"));
        }
    }

    let write = || -> std::io::Result<()> {
        std::fs::create_dir_all(i.folder.join("instruments"))?;
        for (from, to) in files.values() {
            let to = i.folder.join(to);
            std::fs::create_dir_all(to.parent().expect("in samples/"))?;
            std::fs::copy(from, &to)?;
        }
        if let Some(l) = &license_file {
            std::fs::copy(l, i.folder.join("LICENSE"))?;
        }
        std::fs::write(i.folder.join(m2_song::pack::PKG_MANIFEST), manifest)?;
        std::fs::write(i.folder.join(format!("instruments/{id}.toml")), t)?;
        std::fs::write(i.folder.join("README.md"), readme)
    };
    if let Err(e) = write() {
        let _ = std::fs::remove_dir_all(i.folder);
        return Err(format!("{} couldn't be written: {e}", i.folder.display()));
    }
    Ok(Imported { id, regions: kept.len(), samples: files.len(), left_out })
}

/// The licence a `LICENSE` file is, when it is one of the common ones for recordings.
fn recognise(text: &str) -> Option<&'static str> {
    let t = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let cc = |kind: &str, v: &str| t.contains(&format!("{kind} {v}"));
    [
        ("Attribution-ShareAlike 4.0", "CC-BY-SA-4.0"),
        ("Attribution-ShareAlike 3.0", "CC-BY-SA-3.0"),
        ("Attribution 4.0", "CC-BY-4.0"),
        ("Attribution 3.0", "CC-BY-3.0"),
    ]
    .iter()
    .find(|(s, _)| cc(s.rsplit_once(' ').map_or(s, |x| x.0), s.rsplit_once(' ').map_or("", |x| x.1)))
    .map(|(_, id)| *id)
    .or_else(|| (t.contains("CC0 1.0") || t.contains("CC0-1.0")).then_some("CC0-1.0"))
}

/// The file at `rel` (`/`-separated) under `dir`, matching each part exactly, else in any capitals.
fn find(dir: &Path, rel: &str) -> Option<PathBuf> {
    let mut at = dir.to_path_buf();
    for part in rel.split('/').filter(|p| !p.is_empty() && *p != ".") {
        if part == ".." {
            at.pop();
            continue;
        }
        let exact = at.join(part);
        if exact.exists() {
            at = exact;
            continue;
        }
        let names: Vec<std::ffi::OsString> = std::fs::read_dir(&at).ok()?.flatten().map(|e| e.file_name()).collect();
        let name = names.iter().find(|n| n.to_string_lossy().eq_ignore_ascii_case(part))?;
        at.push(name);
    }
    at.is_file().then_some(at)
}

/// `why`, whose first word is a verb said of several regions, said of `n`: "play" of one is "plays".
fn agree(n: usize, why: &str) -> String {
    if n != 1 {
        return why.to_string();
    }
    let (verb, rest) = why.split_once(' ').unwrap_or((why, ""));
    let one = match verb {
        "are" => "is".to_string(),
        "have" => "has".to_string(),
        "don't" => "doesn't".to_string(),
        v => format!("{v}s"),
    };
    let rest =
        rest.replace("they are ", "it is ").replace("they play ", "it plays ").replace("they stop ", "it stops ").replace("their ", "its ");
    format!("{one} {rest}")
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// A number as TOML and people read it: whole ones without a point.
fn show(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 { format!("{}", v as i64) } else { format!("{v}") }
}

fn quote(s: &str) -> String {
    serde_json::to_string(s).expect("a string")
}

#[cfg(test)]
#[path = "sfz_tests.rs"]
mod tests;
