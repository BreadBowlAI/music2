//! SFZ instruments (PACK-14): one parser for SFZ, the open text format of free sample instruments,
//! and `m2 pack import-sfz`, which makes a pack of one: its regions as a data instrument (PACK-13).
//!
//! ```text
//! parse      the file, its #include files and #define names, into regions, each with every opcode
//!            of the headers above it (<control> <global> <master> <group> <region>)
//! map        each region into PKG-15's fields, or why it is left out; and what is ignored
//! import     the pack: m2pkg.toml, instruments/<id>.toml, samples/, README.md, LICENSE
//! ```
//!
//! An SFZ folder comes from a stranger. Everything it names is a plain file in its own folder,
//! found by [`Folder`] (never through a link, never above it), and the work it can ask for is
//! bounded: the size of a file, the text once `#define` names are filled in, how deep `#include`
//! goes, how many regions and opcodes.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::fs::FileType;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use m2_song::instrument::Id;
use m2_song::pack::PackName;

/// One region as the file gives it: every opcode of the headers above it, its own last, and where
/// it is.
#[derive(Clone, Debug, PartialEq)]
pub struct SfzRegion {
    pub opcodes: BTreeMap<String, String>,
    /// Where each opcode was set (`file:line`): on the region, or on a header above it.
    pub places: BTreeMap<String, String>,
    /// `default_path` from `<control>` when the region was read, and where it was set.
    pub default_path: String,
    pub default_path_at: String,
    /// What `<control>`'s `octave_offset` and `note_offset` add to every note of the region:
    /// 12 × `octave_offset` + `note_offset`, as they were when it was read.
    pub shift: i64,
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
    /// Opcodes before the first header, which no region has.
    pub loose: Vec<String>,
}

/// How deep `#include` may go.
const DEPTH: usize = 16;
/// The most an SFZ file (one it includes, too) may hold.
const FILE_MAX: u64 = 4 << 20;
/// The most text all the files read hold, each as often as it is included, once `#define` names
/// are filled in.
const TEXT_MAX: usize = 8 << 20;
/// The most regions, and opcodes in all of them (each with what it inherits).
const REGIONS_MAX: usize = 100_000;
const OPCODES_MAX: usize = 1_000_000;
/// The most a licence file may hold.
const LICENCE_MAX: u64 = 1 << 20;

/// Parse the SFZ file at `path`, with what it includes (relative to its folder).
pub fn parse(path: &Path) -> Result<Sfz, String> {
    read(path).map(|(sfz, _)| sfz)
}

/// [`parse`], and the SFZ's folder, which everything it names is found in.
fn read(path: &Path) -> Result<(Sfz, Folder), String> {
    let (folder, name) = Folder::of(path)?;
    let (file, spelt) = folder.find(&name).map_err(|r| match r {
        Refused::Link(_) => format!("{} is a link: name the SFZ file itself", path.display()),
        Refused::NotPlain(_) => format!("{} isn't a plain file", path.display()),
        _ => format!("{} isn't there", path.display()),
    })?;
    let mut p = Parser {
        folder,
        defines: vec![],
        sfz: Sfz::default(),
        level: Level::None,
        maps: Default::default(),
        region: None,
        seen_header: false,
        reading: vec![],
        text: 0,
        opcodes: 0,
    };
    p.file(&file, &spelt, 0)?;
    p.end_region();
    Ok((p.sfz, p.folder))
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
    folder: Folder,
    /// `#define $NAME value`, in order: a later one replaces an earlier one.
    defines: Vec<(String, String)>,
    sfz: Sfz,
    level: Level,
    /// `<control>`, `<global>`, `<master>`, `<group>`: each opcode's value, and where it was set.
    maps: [BTreeMap<String, (String, String)>; 4],
    region: Option<SfzRegion>,
    seen_header: bool,
    /// The files being read, the main one first: one that includes one of these is a loop.
    reading: Vec<String>,
    /// The text read so far, as [`TEXT_MAX`] counts it, and the opcodes the regions hold.
    text: usize,
    opcodes: usize,
}

impl Parser {
    /// The file at `path`, `spelt` from the SFZ's folder: a plain file there, found by [`Folder`].
    fn file(&mut self, path: &Path, spelt: &str, depth: usize) -> Result<(), String> {
        if let Some(i) = self.reading.iter().position(|r| r == spelt) {
            let chain: Vec<&str> = self.reading[i..].iter().map(String::as_str).chain([spelt]).collect();
            return Err(format!("{spelt} would include itself ({})", chain.join(", ")));
        }
        if depth > DEPTH {
            return Err(format!("{spelt}: `#include` goes more than {DEPTH} files deep"));
        }
        let text = read_text(path, spelt, FILE_MAX, "an SFZ file is text, and music2 reads")?;
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        self.text += text.len();
        if self.text > TEXT_MAX {
            return Err(format!(
                "{spelt}: the SFZ's text, with what it includes, is over {} MB: music2 reads at most that",
                TEXT_MAX >> 20
            ));
        }
        let text = without_block_comments(&text);
        let shown = spelt.rsplit('/').next().unwrap_or(spelt).to_string();
        self.reading.push(spelt.to_string());
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
        self.reading.pop();
        Ok(())
    }

    /// One line, its comment cut: headers, `#define`, `#include` and `name=value` opcodes, any
    /// number of each, in order.
    fn line(&mut self, line: &str, at: &str, depth: usize) -> Result<(), String> {
        let mut rest = line.trim_start();
        while !rest.is_empty() {
            if let Some(r) = rest.strip_prefix('<') {
                let end = r.find('>').ok_or_else(|| format!("{at}: a header `<{r}` has no `>`"))?;
                self.header(r[..end].trim(), at)?;
                rest = r[end + 1..].trim_start();
            } else if let Some(r) = rest.strip_prefix("#define") {
                let mut parts = r.split_whitespace();
                let (Some(name), Some(value)) = (parts.next(), parts.next()) else {
                    return Err(format!("{at}: `#define` needs a name and a value, as `#define $VEL 100`"));
                };
                let value = self.defined(value, at)?;
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
                let what = format!("{at}: `#include \"{file}\"`");
                let name = self.defined(file, at)?;
                let (path, spelt) = self.folder.find(&name).map_err(|r| r.said(&what, &name))?;
                self.file(&path, &spelt, depth + 1).map_err(|e| format!("{what}: {e}"))?;
                rest = after.trim_start();
            } else {
                let eq = rest.find('=').ok_or_else(|| format!("{at}: `{}` isn't an opcode (`name=value`) or a header", rest.trim()))?;
                let name = self.defined(rest[..eq].trim(), at)?;
                if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                    return Err(format!("{at}: `{name}` isn't an opcode's name: letters, digits and `_`"));
                }
                let after = &rest[eq + 1..];
                let end = value_end(after);
                let value = self.defined(after[..end].trim(), at)?;
                self.opcode(name, value, at)?;
                rest = after[end..].trim_start();
            }
        }
        Ok(())
    }

    /// `text` with every `$NAME` defined so far replaced, the longest names first; a `$NAME` no
    /// `#define` made is an error. What the names add counts towards [`TEXT_MAX`], before it is made.
    fn defined(&mut self, text: &str, at: &str) -> Result<String, String> {
        if !text.contains('$') {
            return Ok(text.to_string());
        }
        let mut defs: Vec<(String, String)> = self.defines.clone();
        defs.sort_by_key(|(n, _)| std::cmp::Reverse(n.len()));
        let mut out = text.to_string();
        for (n, v) in &defs {
            let uses = out.matches(n.as_str()).count();
            if uses == 0 {
                continue;
            }
            self.text = self.text.saturating_add(uses.saturating_mul(v.len().saturating_sub(n.len())));
            if self.text > TEXT_MAX {
                return Err(format!(
                    "{at}: the SFZ's text grows past {} MB as its #define names are filled in: music2 reads at most that",
                    TEXT_MAX >> 20
                ));
            }
            out = out.replace(n.as_str(), v);
        }
        for (i, _) in out.match_indices('$') {
            let name: String = out[i + 1..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
            if !name.is_empty() {
                return Err(format!("{at}: `${name}` isn't defined: add `#define ${name} …` above it"));
            }
        }
        Ok(out)
    }

    fn header(&mut self, name: &str, at: &str) -> Result<(), String> {
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
            if self.sfz.regions.len() >= REGIONS_MAX {
                return Err(format!("{at}: the SFZ has more than {} regions: music2 reads at most that many", thousands(REGIONS_MAX)));
            }
            let (mut opcodes, mut places) = (BTreeMap::new(), BTreeMap::new());
            for m in &self.maps[1..] {
                for (k, (v, a)) in m {
                    opcodes.insert(k.clone(), v.clone());
                    places.insert(k.clone(), a.clone());
                }
            }
            self.count(opcodes.len(), at)?;
            let (default_path, default_path_at) = self.maps[0].get("default_path").cloned().unwrap_or_default();
            let offset = |k: &str| self.maps[0].get(k).and_then(|(v, _)| v.trim().parse::<i64>().ok()).unwrap_or(0);
            let shift = 12 * offset("octave_offset") + offset("note_offset");
            self.region = Some(SfzRegion { opcodes, places, default_path, default_path_at, shift, at: at.to_string() });
        }
        self.level = level;
        Ok(())
    }

    /// `n` more opcodes held by the regions, as [`OPCODES_MAX`] counts them.
    fn count(&mut self, n: usize, at: &str) -> Result<(), String> {
        self.opcodes += n;
        if self.opcodes > OPCODES_MAX {
            return Err(format!(
                "{at}: the SFZ's regions hold more than {} opcodes in all (each with what it inherits): music2 reads at most that many",
                thousands(OPCODES_MAX)
            ));
        }
        Ok(())
    }

    /// An opcode, under its main name (`loopmode` is `loop_mode`); `key` is `lokey`, `hikey` and
    /// `pitch_keycenter` at once, where it is written, as SFZ has it.
    fn opcode(&mut self, name: String, value: String, at: &str) -> Result<(), String> {
        let name = match name.as_str() {
            "loopmode" => "loop_mode".to_string(),
            "loopstart" => "loop_start".to_string(),
            "loopend" => "loop_end".to_string(),
            _ => name,
        };
        if self.level == Level::Control && (name == "octave_offset" || name == "note_offset") && value.trim().parse::<i64>().is_err() {
            return Err(format!("{at}: `{name}={value}` isn't a whole number"));
        }
        let names = if name == "key" { vec!["lokey".to_string(), "hikey".into(), "pitch_keycenter".into()] } else { vec![name] };
        for name in names {
            match self.level {
                Level::Region => {
                    self.count(1, at)?;
                    if let Some(r) = &mut self.region {
                        r.opcodes.insert(name.clone(), value.clone());
                        r.places.insert(name, at.to_string());
                    }
                }
                Level::Control => {
                    self.sfz.control.insert(name.clone(), value.clone());
                    self.maps[0].insert(name, (value.clone(), at.to_string()));
                }
                Level::Global => drop(self.maps[1].insert(name, (value.clone(), at.to_string()))),
                Level::Master => drop(self.maps[2].insert(name, (value.clone(), at.to_string()))),
                Level::Group => drop(self.maps[3].insert(name, (value.clone(), at.to_string()))),
                Level::None => self.sfz.loose.push(name),
                Level::Other => {}
            }
        }
        Ok(())
    }

    fn end_region(&mut self) {
        if let Some(r) = self.region.take() {
            self.sfz.regions.push(r);
        }
    }
}

/// `n` as people read a big number: `100 000`.
fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

/// The text of the plain file at `path` (`shown` in words), at most `max` bytes, its UTF-8 byte
/// order mark skipped. `what` follows "over N MB: " when it is bigger.
fn read_text(path: &Path, shown: &str, max: u64, what: &str) -> Result<String, String> {
    use std::io::Read;
    let mut bytes = vec![];
    std::fs::File::open(path).and_then(|f| f.take(max + 1).read_to_end(&mut bytes)).map_err(|e| format!("{shown} can't be read: {e}"))?;
    if bytes.len() as u64 > max {
        let size = if max >= 1 << 20 { format!("{} MB", max >> 20) } else { format!("{} KB", max >> 10) };
        return Err(format!("{shown} is over {size}: {what} at most {size} of one"));
    }
    let text = String::from_utf8_lossy(&bytes);
    Ok(text.strip_prefix('\u{FEFF}').unwrap_or(&text).to_string())
}

/// The SFZ's own folder, the one place an import reads (PACK-14). A path the SFZ names is found
/// in it part by part, among what each folder lists: only plain folders and files, never a link
/// (never followed), never above the folder, never an absolute path. Capitals are matched, when
/// the exact name isn't there, among those alone.
pub struct Folder {
    root: PathBuf,
    listed: RefCell<HashMap<PathBuf, Listing>>,
}

/// What a folder lists: each name and its kind, sorted by name.
type Listing = Rc<Vec<(String, FileType)>>;

/// Why a path isn't a plain file in the SFZ's folder: the part that isn't, from the folder.
enum Refused {
    Absolute,
    Above,
    Link(String),
    NotPlain(String),
    Missing,
}

impl Refused {
    /// In words after `what` (`i.sfz:3: \`sample=a.wav\``), naming `rel` when it isn't there.
    fn said(&self, what: &str, rel: &str) -> String {
        const ONLY: &str = "music2 imports only the files in the SFZ's folder and the folders in it";
        match self {
            Refused::Absolute => format!("{what} is an absolute path: {ONLY}"),
            Refused::Above => format!("{what} leaves the SFZ's folder: {ONLY}"),
            Refused::Link(p) => format!("{what}: {p} is a link, which music2 never follows: put the file itself in the SFZ's folder"),
            Refused::NotPlain(p) => format!("{what}: {p} isn't a plain file"),
            Refused::Missing => format!("{what}: {rel} isn't there"),
        }
    }
}

impl Folder {
    /// The folder `sfz` is in (`i.sfz` alone is in the working folder), made whole, and `sfz`'s
    /// name in it.
    fn of(sfz: &Path) -> Result<(Folder, String), String> {
        let parent = sfz.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        let root = parent.canonicalize().map_err(|e| format!("{} can't be read: {e}", parent.display()))?;
        let name = sfz.file_name().and_then(|n| n.to_str()).ok_or_else(|| format!("{} isn't an SFZ file", sfz.display()))?;
        Ok((Folder { root, listed: Default::default() }, name.to_string()))
    }

    /// What the folder at `dir` lists: each name and its kind, a link a link.
    fn list(&self, dir: &Path) -> Listing {
        self.listed
            .borrow_mut()
            .entry(dir.to_path_buf())
            .or_insert_with(|| {
                let mut out: Vec<(String, FileType)> = std::fs::read_dir(dir)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .filter_map(|e| Some((e.file_name().into_string().ok()?, e.file_type().ok()?)))
                    .collect();
                out.sort_by(|a, b| a.0.cmp(&b.0));
                Rc::new(out)
            })
            .clone()
    }

    /// The plain file at `rel` (`/`-separated, `\` as `/`, as an SFZ names files): its path, and
    /// its path from the folder as the folder spells it, so two spellings of one file are one.
    fn find(&self, rel: &str) -> Result<(PathBuf, String), Refused> {
        let rel = rel.replace('\\', "/");
        if rel.starts_with('/') || rel.as_bytes().get(1) == Some(&b':') {
            return Err(Refused::Absolute);
        }
        let mut parts: Vec<&str> = vec![];
        for p in rel.split('/') {
            match p {
                "" | "." => {}
                ".." => {
                    parts.pop().ok_or(Refused::Above)?;
                }
                p => parts.push(p),
            }
        }
        if parts.is_empty() {
            return Err(Refused::Missing);
        }
        let (mut at, mut spelt) = (self.root.clone(), Vec::<String>::new());
        for (i, part) in parts.iter().enumerate() {
            let last = i + 1 == parts.len();
            let listed = self.list(&at);
            let plain = |t: &FileType| if last { t.is_file() } else { t.is_dir() };
            let exact = listed.iter().filter(|(n, _)| n == part);
            let other = listed.iter().filter(|(n, _)| n != part && n.eq_ignore_ascii_case(part));
            let named = || exact.clone().chain(other.clone());
            let shown = |n: &str| spelt.iter().map(String::as_str).chain([n]).collect::<Vec<_>>().join("/");
            match named().find(|(_, t)| plain(t)) {
                Some((n, _)) => {
                    at.push(n);
                    spelt.push(n.clone());
                }
                None => match named().next() {
                    Some((n, t)) if t.is_symlink() => return Err(Refused::Link(shown(n))),
                    Some((n, _)) => return Err(Refused::NotPlain(shown(n))),
                    None => return Err(Refused::Missing),
                },
            }
        }
        // As the file is now: still a plain file, not a link.
        let spelt = spelt.join("/");
        match std::fs::symlink_metadata(&at) {
            Ok(m) if m.is_file() => Ok((at, spelt)),
            Ok(m) if m.file_type().is_symlink() => Err(Refused::Link(spelt)),
            Ok(_) => Err(Refused::NotPlain(spelt)),
            Err(_) => Err(Refused::Missing),
        }
    }

    /// The licence file beside the SFZ, if there is one: its name and its text. One that is a link
    /// or isn't a plain file, over [`LICENCE_MAX`] or not UTF-8 text stops the import.
    fn licence(&self) -> Result<Option<(String, String)>, String> {
        let listed = self.list(&self.root);
        let rank = |n: &str| LICENCE_FILES.iter().position(|l| n.eq_ignore_ascii_case(l));
        let mut found: Vec<&(String, FileType)> = listed.iter().filter(|(n, _)| rank(n).is_some()).collect();
        found.sort_by_key(|(n, _)| (rank(n), n.clone()));
        for (n, t) in &found {
            if t.is_symlink() {
                return Err(format!("{n} is a link, which music2 never follows: put the licence itself beside the SFZ"));
            }
            if !t.is_file() {
                return Err(format!("{n} isn't a plain file: the licence beside the SFZ must be one"));
            }
        }
        let Some((name, _)) = found.first() else { return Ok(None) };
        let (path, _) = self.find(name).map_err(|r| r.said(name, name))?;
        let text = read_text(&path, name, LICENCE_MAX, "a licence is text, and music2 copies")?;
        if text.contains('\u{FFFD}') {
            return Err(format!("{name} isn't text (UTF-8): save it as UTF-8 text, or remove it and give --license"));
        }
        Ok(Some((name.clone(), text)))
    }
}

/// The names of a licence file beside an SFZ, in any capitals, the first found first.
const LICENCE_FILES: &[&str] =
    &["LICENSE", "LICENSE.txt", "LICENSE.md", "LICENCE", "LICENCE.txt", "LICENCE.md", "COPYING", "COPYING.txt", "COPYING.md"];

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
    /// The recording, relative to the SFZ's folder, `/`-separated: `default_path` and `sample`.
    pub source: String,
    /// What names it, in words: "i.sfz:3: `sample=a.wav`", and `default_path` when there is one.
    pub named: String,
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
    /// The file and line of its `<region>` header.
    pub at: String,
}

/// The opcodes [`map`] reads; any other a region has is ignored, and counted. (`key`, `loopmode`,
/// `loopstart` and `loopend` are stored as the opcodes they stand for.)
const READ: &[&str] = &[
    "sample",
    "lokey",
    "hikey",
    "pitch_keycenter",
    "lovel",
    "hivel",
    "seq_length",
    "seq_position",
    "trigger",
    "loop_mode",
    "loop_start",
    "loop_end",
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

/// What the region plays as, or why it is left out (in words that read after "N regions"). A value
/// it reads that isn't one (`tune=+5c`, a note `h4`) leaves it out: never read as 0.
pub fn map(r: &SfzRegion, control: &BTreeMap<String, String>) -> Result<Mapped, String> {
    let o = &r.opcodes;
    let get = |k: &str| o.get(k).map(String::as_str);
    let num = |k: &str| -> Result<Option<f64>, String> {
        match get(k) {
            None => Ok(None),
            Some(v) => match v.trim().parse::<f64>() {
                Ok(n) if n.is_finite() => Ok(Some(n)),
                _ => Err(format!("have `{k}={v}`, which isn't a number")),
            },
        }
    };
    // A note, moved by `<control>`'s offsets.
    let key = |k: &str| -> Result<Option<i64>, String> {
        match get(k) {
            None => Ok(None),
            Some(v) if k == "pitch_keycenter" && v.trim() == "sample" => {
                Err("have `pitch_keycenter=sample`, the recording's own root note, which music2 can't read yet".into())
            }
            Some(v) => note(v).map(|n| Some(n + r.shift)).ok_or_else(|| format!("have `{k}={v}`, which isn't a note")),
        }
    };
    let Some(sample) = get("sample") else { return Err("name no sample".into()) };
    if sample.starts_with('*') {
        return Err(format!("play a sound SFZ makes ({sample}) rather than a recording"));
    }
    // Key switches: only the default one's regions.
    let default_sw = key("sw_default")?;
    if let Some(last) = key("sw_last")?
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
            let at_rest = cc_start(control, n)?;
            let lo = num(&format!("locc{n}"))?.unwrap_or(0.0);
            let hi = num(&format!("hicc{n}"))?.unwrap_or(127.0);
            if !(lo..=hi).contains(&at_rest) {
                return Err(format!("play only with a controller moved (CC {n})"));
            }
        }
    }
    // Regions chosen at random: the one for a random value of 0 is kept, the others left out.
    if num("lorand")?.unwrap_or(0.0) > 0.0 {
        return Err("play at random, and aren't the first choice (lorand, hirand)".into());
    }
    let release = match get("trigger").unwrap_or("attack") {
        "attack" => false,
        "release" => true,
        other => return Err(format!("play on `trigger={other}`, which music2 doesn't")),
    };
    let lo = key("lokey")?.unwrap_or(0).max(0);
    let hi = key("hikey")?.unwrap_or(127).min(127);
    if lo > hi {
        return Err("play on no key".into());
    }
    let root = key("pitch_keycenter")?.unwrap_or(60);
    if !(0..=127).contains(&root) {
        return Err("have a root note outside 0 to 127 (pitch_keycenter)".into());
    }
    let lovel = num("lovel")?.map_or(1, |v| v as i64).max(1);
    let hivel = num("hivel")?.map_or(127, |v| v as i64).min(127);
    if lovel > hivel {
        return Err("play at no velocity".into());
    }
    let mut notes = vec![];
    let round_robin = match (num("seq_length")?, num("seq_position")?) {
        (Some(len), _) if len > 128.0 => return Err("are in a round robin of more than 128 takes".into()),
        (Some(len), pos) if len > 1.0 => {
            let pos = pos.unwrap_or(1.0);
            if !(1.0..=len).contains(&pos) {
                return Err("are a take of a round robin that has no such take (seq_position)".into());
            }
            Some((pos as i64, len as i64))
        }
        (None, Some(pos)) if pos > 1.0 => return Err("are a later take of a round robin with no `seq_length`".into()),
        _ => None,
    };
    let mode = get("loop_mode").unwrap_or("no_loop");
    let points = match (num("loop_start")?, num("loop_end")?) {
        (Some(s), Some(e)) if s >= 0.0 && s < e => Some((s as i64, e as i64)),
        (Some(_), Some(_)) => {
            notes.push("have a loop that doesn't start before it ends: they play without one".into());
            None
        }
        _ => None,
    };
    let loop_ = match mode {
        "no_loop" | "one_shot" => None,
        "loop_continuous" | "loop_sustain" => {
            if num("loop_start")?.is_none() || num("loop_end")?.is_none() {
                notes.push("loop on the recording's own loop points, which music2 doesn't read yet: they play without a loop".into());
            } else if mode == "loop_continuous" && points.is_some() {
                notes.push("loop on after the note ends (loop_continuous): here their loop ends with the note".into());
            }
            points
        }
        other => return Err(format!("have `loop_mode={other}`, which SFZ doesn't define")),
    };
    let tune = num("tune")?.unwrap_or(0.0) + num("transpose")?.unwrap_or(0.0) * 100.0;
    if tune.abs() > 2400.0 {
        return Err("are tuned more than two octaves away".into());
    }
    let volume = num("volume")?.unwrap_or(0.0).clamp(-120.0, 24.0);
    let pan = (num("pan")?.unwrap_or(0.0) / 100.0).clamp(-1.0, 1.0);
    if let Some(t) = num("pitch_keytrack")?
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
    let place = |k: &str| r.places.get(k).map_or(r.at.as_str(), String::as_str).to_string();
    let mut named = format!("{}: `sample={sample}`", place("sample"));
    if !r.default_path.is_empty() {
        named.push_str(&format!(", after `default_path={}` ({})", r.default_path, r.default_path_at));
    }
    Ok(Mapped {
        source: format!("{}{}", r.default_path, sample).replace('\\', "/"),
        named,
        root,
        keys: (lo, hi),
        velocity: (lovel, hivel),
        round_robin,
        release,
        loop_,
        tune,
        volume,
        pan,
        offset: num("offset")?.map_or(0, |v| v.max(0.0) as i64),
        one_shot: mode == "one_shot",
        veltrack: num("amp_veltrack")?,
        attack: num("ampeg_attack")?,
        release_time: num("ampeg_release")?,
        ignored,
        notes,
        at: r.at.clone(),
    })
}

/// A controller's value before it moves: `set_ccN` (or `set_hdccN`, 0 to 1) in `<control>`, else 0.
fn cc_start(control: &BTreeMap<String, String>, n: u32) -> Result<f64, String> {
    let read = |k: String| -> Result<Option<f64>, String> {
        match control.get(&k) {
            None => Ok(None),
            Some(v) => v.trim().parse::<f64>().map(Some).map_err(|_| format!("depend on `{k}={v}`, which isn't a number")),
        }
    };
    Ok(read(format!("set_cc{n}"))?.or(read(format!("set_hdcc{n}"))?.map(|v| v * 127.0)).unwrap_or(0.0))
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
    /// What `m2 pack check` finds in the pack written (PACK-12), given its folder: the pack is kept
    /// only when this finds nothing.
    pub check: &'a dyn Fn(&Path) -> Vec<String>,
}

/// What an import made, and what it couldn't keep, in words.
#[derive(Debug)]
pub struct Imported {
    pub id: String,
    pub regions: usize,
    pub samples: usize,
    pub left_out: Vec<String>,
    /// The licence, when it was recognised in a licence file: in words, with how.
    pub licence: Option<String>,
}

/// How many of the regions that play at the attack must say one value of the instrument's for it
/// to be the instrument's: 9 in 10 (PACK-14).
const AGREE: f64 = 0.9;

/// One of the instrument's values that SFZ gives each region: the opcode, the instrument's name
/// for it, its unit, SFZ's default, the Sampler's (in the opcode's unit), the opcode's unit in the
/// Sampler's (100 for a percentage), and each region's own.
struct Shared {
    opcode: &'static str,
    name: &'static str,
    unit: &'static str,
    sfz: f64,
    sampler: f64,
    scale: f64,
    own: fn(&Mapped) -> Option<f64>,
    /// Whether release regions play by it too (the envelope is only the attack regions').
    releases: bool,
}

const SHARED: [Shared; 3] = [
    Shared {
        opcode: "amp_veltrack",
        name: "velocity",
        unit: "%",
        sfz: 100.0,
        sampler: 100.0,
        scale: 100.0,
        own: |m| m.veltrack,
        releases: true,
    },
    Shared { opcode: "ampeg_attack", name: "attack", unit: " s", sfz: 0.0, sampler: 0.0, scale: 1.0, own: |m| m.attack, releases: false },
    Shared {
        opcode: "ampeg_release",
        name: "release",
        unit: " s",
        sfz: 0.0,
        sampler: 0.05,
        scale: 1.0,
        own: |m| m.release_time,
        releases: false,
    },
];

/// Make a pack in `folder` from an SFZ file (PACK-14). What `m2 pack check` would refuse is refused
/// first; the pack is written into a new folder beside `folder`, checked, and renamed to `folder`
/// only when it passes, so a failed import leaves nothing.
pub fn import(i: &Import) -> Result<Imported, String> {
    if i.folder.exists() {
        return Err(format!("{} exists already: name a new folder for the pack", i.folder.display()));
    }
    let part = i.name.as_str().rsplit('/').next().unwrap_or("");
    let id = Id::new(part).map_err(|why| {
        format!("--name {}: the instrument's id is the name after the `/`, and `{part}` can't be an instrument's id: {why}", i.name)
    })?;
    let (sfz, folder) = read(i.sfz)?;
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
    turns_whole(&kept)?;
    let licence_file = folder.licence()?;
    // Every recording: a plain file in the SFZ's folder (in any capitals, as SFZ made on Windows
    // names them), a WAV, and given its path in the pack, one path for each file however it is named.
    let mut files: BTreeMap<String, (PathBuf, String)> = BTreeMap::new();
    let mut by_source: BTreeMap<String, String> = BTreeMap::new();
    let mut taken: BTreeMap<String, String> = BTreeMap::new();
    for m in &kept {
        if by_source.contains_key(&m.source) {
            continue;
        }
        let ext = Path::new(&m.source).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        if ext != "wav" {
            return Err(format!("{} is a {} file: convert it to WAV first; music2 reads only WAV for now", m.source, ext.to_uppercase()));
        }
        let (found, spelt) = folder.find(&m.source).map_err(|r| match r {
            Refused::Missing => format!("{}, which the SFZ plays, isn't there (looked in {})", m.source, folder.root.display()),
            r => r.said(&m.named, &m.source),
        })?;
        by_source.insert(m.source.clone(), spelt.clone());
        if files.contains_key(&spelt) {
            continue;
        }
        hound::WavReader::open(&found).map_err(|e| format!("{} isn't a WAV music2 can read: {e}", m.source))?;
        // Under `samples/`, by its path from the SFZ's folder, a first folder that is already
        // called samples (in any capitals) not twice.
        let mut inside: Vec<&str> = spelt.split('/').collect();
        if inside.len() > 1 && inside[0].eq_ignore_ascii_case("samples") {
            inside.remove(0);
        }
        let path = format!("samples/{}", inside.join("/"));
        if let Some(other) = taken.insert(path.to_lowercase(), spelt.clone()) {
            return Err(format!("{spelt} and {other} are two files that would both be {path} in the pack"));
        }
        files.insert(spelt, (found, path));
    }
    let pack_path = |m: &Mapped| &files[&by_source[&m.source]].1;
    let label = i
        .label
        .map(String::from)
        .unwrap_or_else(|| i.sfz.file_stem().map_or_else(|| id.as_str().to_string(), |s| s.to_string_lossy().into_owned()));
    let category = i.category.unwrap_or("Sampler");
    let (license, licence) = match (i.license, &licence_file) {
        (Some(l), _) => (l.to_string(), None),
        (None, Some((name, text))) => match recognise(text) {
            Some((l, how)) => (l.clone(), Some(format!("licence: {l}, recognised in {name} ({how})"))),
            None => {
                return Err(format!(
                    "say the pack's licence: {name} beside the SFZ isn't one music2 recognises by itself (an SPDX line, or the whole \
                     text of a Creative Commons licence), so read it and add --license with the one it gives (such as CC-BY-4.0)"
                ));
            }
        },
        (None, None) => return Err("say the pack's licence, the one its author gave: add --license (such as CC-BY-4.0 or CC0-1.0)".into()),
    };

    let mut left_out = vec![];
    for (why, n) in &dropped {
        left_out.push(format!("left out: {n} region{} {}", plural(*n), agree(*n, why)));
    }
    // What the instrument takes from its regions: what nearly every one that plays at the attack
    // says, one that doesn't set it saying SFZ's default; else the Sampler's, said.
    let attacks: Vec<&Mapped> = kept.iter().filter(|m| !m.release).collect();
    let mut said: BTreeMap<String, usize> = BTreeMap::new();
    let mut disagree = vec![];
    let mut values: [Option<f64>; 3] = [None; 3];
    for (n, v) in SHARED.iter().enumerate() {
        let shown = |x: f64| format!("{}{}", show(x), v.unit);
        let effective = |m: &Mapped| (v.own)(m).unwrap_or(v.sfz);
        let mut votes: Vec<(f64, usize)> = vec![];
        for m in &attacks {
            match votes.iter_mut().find(|(x, _)| *x == effective(m)) {
                Some(vote) => vote.1 += 1,
                None => votes.push((effective(m), 1)),
            }
        }
        votes.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.total_cmp(&b.0)));
        let Some(&(top, count)) = votes.first() else { continue };
        if (count as f64) < AGREE * attacks.len() as f64 {
            // Each value as the regions give it: "8 don't set it, 2 have 4 s".
            let mut given: Vec<(Option<f64>, usize)> = vec![];
            for m in &attacks {
                match given.iter_mut().find(|(x, _)| *x == (v.own)(m)) {
                    Some(g) => g.1 += 1,
                    None => given.push(((v.own)(m), 1)),
                }
            }
            given.sort_by_key(|g| std::cmp::Reverse(g.1));
            let each: Vec<String> = given
                .iter()
                .map(|(x, c)| match x {
                    None => format!("{c} {} set it (SFZ: {})", if *c == 1 { "doesn't" } else { "don't" }, shown(v.sfz)),
                    Some(x) => format!("{c} {} {}", if *c == 1 { "has" } else { "have" }, shown(*x)),
                })
                .collect();
            disagree.push(format!(
                "changed: the regions that play at the attack don't agree on {} ({}): the instrument's {} is the Sampler's, {}, for all",
                v.opcode,
                each.join(", "),
                v.name,
                shown(v.sampler)
            ));
            continue;
        }
        // Inside the Sampler's range for it (PACK-13), else at its end, said.
        let (lo, hi) = m2_song::instrument::CONTROLS.iter().find(|c| c.0 == v.name).map(|c| c.3).expect("a control of the Sampler's");
        let (lo, hi) = (lo * v.scale, hi * v.scale);
        let top_in = top.clamp(lo, hi);
        if top_in != top {
            disagree.push(format!(
                "changed: the regions' {} is {}, beyond the Sampler's {} to {}: the instrument's {} is {} for all",
                v.opcode,
                shown(top),
                shown(lo),
                shown(hi),
                v.name,
                shown(top_in)
            ));
        }
        let value = if top == v.sfz { v.sampler } else { top_in };
        if top != v.sfz {
            values[n] = Some(top_in);
        }
        for m in kept.iter().filter(|m| !m.release || v.releases) {
            if effective(m) == top {
                continue;
            }
            let what = match (v.own)(m) {
                Some(x) => format!("have their own {} ({})", v.opcode, shown(x)),
                None => format!("don't set {} (SFZ: {})", v.opcode, shown(v.sfz)),
            };
            *said.entry(format!("{what}: the instrument's {} is {} for all", v.name, shown(value))).or_default() += 1;
        }
    }
    let [veltrack, attack, release_time] = values;
    let one_shot = !attacks.is_empty() && attacks.iter().all(|m| m.one_shot);
    for m in &kept {
        for n in &m.notes {
            *said.entry(n.clone()).or_default() += 1;
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
    left_out.extend(disagree);
    // The Sampler's velocity is a straight line; SFZ's amp_veltrack follows the velocity squared.
    let amount = veltrack.unwrap_or(100.0).clamp(0.0, 100.0) / 100.0;
    if amount > 0.0 {
        let louder = (1..=127)
            .map(|k| {
                let x = k as f64 / 127.0;
                20.0 * ((1.0 - amount + amount * x) / (1.0 - amount + amount * x * x)).log10()
            })
            .fold(0.0, f64::max);
        left_out.push(format!(
            "changed: velocity changes the level on a straight line here, where SFZ's amp_veltrack ({}%) follows a curve: soft notes \
             are up to {louder:.1} dB louder than in SFZ",
            show(amount * 100.0)
        ));
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
    let control: Vec<&str> = sfz
        .control
        .keys()
        .map(String::as_str)
        .filter(|k| !["default_path", "octave_offset", "note_offset"].contains(k) && !k.starts_with("set_cc") && !k.starts_with("set_hdcc"))
        .collect();
    if !control.is_empty() {
        left_out.push(format!("ignored: <control> opcodes music2 doesn't read: {}", control.join(", ")));
    }
    if !sfz.loose.is_empty() {
        let n = sfz.loose.len();
        let names: std::collections::BTreeSet<&str> = sfz.loose.iter().map(String::as_str).collect();
        let mut shown: Vec<&str> = names.iter().copied().take(20).collect();
        if names.len() > shown.len() {
            shown.push("…");
        }
        left_out.push(format!("ignored: {n} opcode{} before the first header, which no region has: {}", plural(n), shown.join(", ")));
    }
    for (h, n) in &sfz.other_headers {
        left_out.push(format!("ignored: {n} {h} section{}", plural(*n)));
    }
    if let Some(sw) = sfz.regions.iter().find_map(|r| r.opcodes.get("sw_default").and_then(|v| note(v)).map(|n| n + r.shift)) {
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
        t.push_str(&format!("\n[[region]]\nfile = {}\nroot = {}\nkeys = [{}, {}]\n", quote(pack_path(m)), m.root, m.keys.0, m.keys.1));
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
        id.binding(),
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
    readme.push_str(&format!("\nLicence: {license}{}.\n", if licence_file.is_some() { ", in `LICENSE`" } else { "" }));
    if !left_out.is_empty() {
        readme.push_str("\n## What the import couldn't keep\n\n");
        for l in &left_out {
            readme.push_str(&format!("- {l}\n"));
        }
    }

    // Written beside the folder named, checked, and only then given its name.
    let leaf = i.folder.file_name().ok_or_else(|| format!("{} isn't a folder's name", i.folder.display()))?;
    let parent = i.folder.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let temp = parent.join(format!(".{}.import-{}", leaf.to_string_lossy(), std::process::id()));
    let write = || -> std::io::Result<()> {
        std::fs::create_dir_all(parent)?;
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir(&temp)?;
        std::fs::create_dir_all(temp.join("instruments"))?;
        for (from, to) in files.values() {
            let to = temp.join(to);
            std::fs::create_dir_all(to.parent().expect("in samples/"))?;
            std::fs::copy(from, &to)?;
        }
        if let Some((_, text)) = &licence_file {
            std::fs::write(temp.join("LICENSE"), text)?;
        }
        std::fs::write(temp.join(m2_song::pack::PKG_MANIFEST), &manifest)?;
        std::fs::write(temp.join(format!("instruments/{id}.toml")), &t)?;
        std::fs::write(temp.join("README.md"), &readme)
    };
    if let Err(e) = write() {
        let _ = std::fs::remove_dir_all(&temp);
        return Err(format!("{} couldn't be written: {e}", i.folder.display()));
    }
    let problems = (i.check)(&temp);
    if !problems.is_empty() {
        let _ = std::fs::remove_dir_all(&temp);
        return Err(format!("the pack made doesn't pass m2 pack check, so nothing is kept:\n{}", problems.join("\n")));
    }
    if i.folder.exists() {
        let _ = std::fs::remove_dir_all(&temp);
        return Err(format!("{} exists already: name a new folder for the pack", i.folder.display()));
    }
    if let Err(e) = std::fs::rename(&temp, i.folder) {
        let _ = std::fs::remove_dir_all(&temp);
        return Err(format!("{} couldn't be written: {e}", i.folder.display()));
    }
    Ok(Imported { id: id.as_str().to_string(), regions: kept.len(), samples: files.len(), left_out, licence })
}

/// Every round robin whole (PKG-15): wherever regions of one trigger in a round robin of `n` takes
/// play a note at a velocity, a region of each take plays it there. SFZ plays nothing on a missing
/// take's turn; music2 refuses that, so the import stops, naming a region.
fn turns_whole(kept: &[Mapped]) -> Result<(), String> {
    // For each trigger and `n`: each take's velocities (bits 1 to 127) on each note.
    let mut groups: BTreeMap<(bool, i64), Vec<[u128; 128]>> = BTreeMap::new();
    for m in kept {
        let Some((p, n)) = m.round_robin else { continue };
        let takes = groups.entry((m.release, n)).or_insert_with(|| vec![[0u128; 128]; n as usize]);
        let velocities = (m.velocity.0..=m.velocity.1).fold(0u128, |b, v| b | 1u128 << v);
        for k in m.keys.0..=m.keys.1 {
            takes[(p - 1) as usize][k as usize] |= velocities;
        }
    }
    for ((release, n), takes) in &groups {
        for k in 0..128 {
            let all = takes.iter().fold(0u128, |a, t| a | t[k]);
            for (q, t) in takes.iter().enumerate() {
                let missing = all & !t[k];
                if missing == 0 {
                    continue;
                }
                let v = i64::from(missing.trailing_zeros());
                let plays = |m: &&Mapped| {
                    m.release == *release
                        && m.round_robin.is_some_and(|(_, of)| of == *n)
                        && (m.keys.0..=m.keys.1).contains(&(k as i64))
                        && (m.velocity.0..=m.velocity.1).contains(&v)
                };
                let at = kept.iter().find(plays).map_or("", |m| m.at.as_str());
                return Err(format!(
                    "{at}: a round robin of {n} takes (seq_length) plays note {} at velocity {v}, but no region there is take {} \
                     (seq_position): SFZ plays nothing on that turn, and music2 needs every take; add take {}, or make the round \
                     robin shorter",
                    note_name(k as i64),
                    q + 1,
                    q + 1
                ));
            }
        }
    }
    Ok(())
}

/// The licence a licence file gives, and how it was recognised, when the file is clear (PACK-14):
/// its first line `SPDX-License-Identifier: <licence>`, or the whole text of a Creative Commons
/// licence. A file that only mentions one is not: sample libraries often put several in one file.
fn recognise(text: &str) -> Option<(String, String)> {
    let first = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    if let Some(id) = first.strip_prefix("SPDX-License-Identifier:") {
        let id = id.trim();
        let plain = !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || " .-+()".contains(c));
        return plain.then(|| (id.to_string(), "its SPDX line".to_string()));
    }
    let fingerprint = fingerprint(text);
    KNOWN.iter().find(|(_, _, f)| *f == fingerprint).map(|(id, title, _)| (id.to_string(), format!("the whole text of {title}")))
}

/// The licences recognised by their whole text: SPDX id, title, and [`fingerprint`] of the text
/// Creative Commons publishes (`legalcode.txt`).
const KNOWN: &[(&str, &str, &str)] = &[
    ("CC0-1.0", "CC0 1.0", "sha256:05d9f1a0af61535887a399e161c9d14d1f898043b61d05fe4854ed8c6460179c"),
    ("CC-BY-3.0", "CC BY 3.0", "sha256:b5d5f34c2061eb6b70957933ee0ff2cb22eadb9c3b3dd88c8d4da54428b83433"),
    ("CC-BY-4.0", "CC BY 4.0", "sha256:9eb27da074ff286d34e70c6df4e7f2418976ff00d3a9395f6acbc00e985df939"),
    ("CC-BY-SA-3.0", "CC BY-SA 3.0", "sha256:0a9fac48bdb081c4258265339f9218ad059ed47ed3c789d8cfbce5574d876743"),
    ("CC-BY-SA-4.0", "CC BY-SA 4.0", "sha256:48b8d3efc79d5af9639cbc84ee98ab206d9d584241462698983e36ea38b37128"),
];

/// A text's hash, its spaces and line breaks aside: each run of them one space, curly quotes
/// straight, `http://` as `https://`.
fn fingerprint(text: &str) -> String {
    let t = text.split_whitespace().collect::<Vec<_>>().join(" ").replace("http://", "https://");
    let t = t.replace(['\u{201C}', '\u{201D}'], "\"").replace(['\u{2018}', '\u{2019}'], "'");
    m2_song::Sha::of(t.as_bytes()).to_string()
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
