//! The registry (REG-1 to REG-7): the list of packs anyone can get, and the bucket its robot
//! (`m2 registry`, `m2_editor::registry`) fills from it. Here are their formats, each read strictly
//! and written in one form, so the robot that writes them and the app that reads them agree to the
//! byte, and the rules of the list itself.
//!
//! ```text
//! PackFile    packs/@owner/name.toml: a pack's versions, each with its hash and source (REG-1)
//! Registry    the registry's folder: every pack file, and tags.txt (REG-1); what may change (REG-2)
//! Pointer     catalog.json: which catalog is current (REG-3)
//! Facts       versions/<hash>.json: what the robot found in one version (REG-4)
//! Catalog     catalog/<hash>.json: every listed pack, its versions and its latest's items (REG-4)
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::hash::Sha;
use crate::manifest::Version;
use crate::pack::{Listed, PackName};

/// The registry's folder of pack files.
pub const PACKS: &str = "packs";
/// The registry's word list: the words a pack's tags may use (design P7).
pub const TAGS: &str = "tags.txt";
/// The bucket's one file that changes: which catalog is current.
pub const POINTER: &str = "catalog.json";
/// The format of every file the robot writes in the bucket.
pub const FORMAT: u32 = 1;
/// The GitHub account whose repos `@m2/` packs come from.
pub const OURS: &str = "BreadBowlAI";
/// music2's own scope.
pub const OUR_SCOPE: &str = "m2";

/// Where a blob is in the bucket: `blobs/sha256/<hex>`.
pub fn blob_path(sha: &Sha) -> String {
    format!("blobs/sha256/{}", sha.hex())
}

/// Where a version's facts are in the bucket, by the version's hash: `versions/<hex>.json`.
pub fn facts_path(hash: &Sha) -> String {
    format!("versions/{}.json", hash.hex())
}

/// Where a catalog is in the bucket, by its own hash: `catalog/<hex>.json`.
pub fn catalog_path(sha: &Sha) -> String {
    format!("catalog/{}.json", sha.hex())
}

/// The scope of a pack's name: `alice` of `@alice/felt-piano`.
pub fn scope(name: &PackName) -> &str {
    name.as_str().strip_prefix('@').and_then(|r| r.split_once('/')).map_or("", |(owner, _)| owner)
}

/// The GitHub account a pack's repository and sources must be under (REG-1): its scope's, and
/// [`OURS`] for `@m2/`.
pub fn github_owner(name: &PackName) -> &str {
    match scope(name) {
        OUR_SCOPE => OURS,
        s => s,
    }
}

/// The parts of `url` after `https://github.com/<owner>/`, when it is a plain URL there: `owner`
/// in any capitals (GitHub's names don't care), only letters, digits and `-._~%+/`, and no empty,
/// `.` or `..` part. Why not, in words naming `what` it is.
fn under_github<'a>(url: &'a str, owner: &str, what: &str) -> Result<Vec<&'a str>, String> {
    let want = format!("https://github.com/{owner}/");
    let rest = url.get(want.len()..).unwrap_or_default();
    let plain = rest.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._~%+/".contains(&b));
    let parts: Vec<&str> = rest.split('/').collect();
    let at = url.get(..want.len()).is_some_and(|p| p.eq_ignore_ascii_case(&want)) && url.starts_with("https://github.com/");
    if !at || !plain || parts.iter().any(|p| p.is_empty() || *p == "." || *p == "..") {
        return Err(format!(
            "{what} `{url}` must be a plain address under {want} (only letters, digits and - . _ ~ % + /, no empty or `..` \
             part): a pack comes from its owner's own GitHub"
        ));
    }
    Ok(parts)
}

/// Whether `url` is a repository of `owner`'s: `https://github.com/<owner>/<repo>`.
fn repository_ok(url: &str, owner: &str) -> Result<(), String> {
    match under_github(url, owner, "repository")?.len() {
        1 => Ok(()),
        _ => Err(format!("repository `{url}` must be a repository's address, https://github.com/{owner}/<repo>, and no more")),
    }
}

/// Whether `url` is a source of `owner`'s: a `.tar.gz` (or `.tgz`) under its GitHub, in a repo.
fn source_ok(url: &str, owner: &str) -> Result<(), String> {
    let parts = under_github(url, owner, "source")?;
    let tarball = url.ends_with(".tar.gz") || url.ends_with(".tgz");
    if parts.len() < 3 || !tarball {
        return Err(format!(
            "source `{url}` must be a .tar.gz file in one of {owner}'s repos, such as \
             https://github.com/{owner}/<repo>/releases/download/v1.0.0/<pack>-1.0.0.tar.gz"
        ));
    }
    Ok(())
}

/// A pack's file in the registry, `packs/@owner/name.toml` (REG-1), read strictly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackFile {
    pub name: PackName,
    pub repository: String,
    /// As the file lists them.
    pub versions: Vec<ListedVersion>,
}

/// One version a pack file lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListedVersion {
    pub version: Version,
    /// Its `m2pack.json`'s hash (PACK-6): what a song's `music2.lock` pins (PACK-8).
    pub hash: Sha,
    /// Where the robot gets its files: a `.tar.gz` of the pack's folder.
    pub source: String,
    /// Why it is hidden from search; a song that locks it still gets it (design §9.3).
    pub yanked: Option<String>,
    /// Why it was taken down: its files are deleted from the bucket.
    pub removed: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPackFile {
    name: PackName,
    repository: String,
    #[serde(default)]
    versions: Vec<RawVersion>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawVersion {
    version: String,
    hash: Sha,
    source: String,
    yanked: Option<String>,
    removed: Option<String>,
}

/// A string as TOML (and JSON) writes it, quoted and escaped.
fn quoted(s: &str) -> String {
    serde_json::to_string(s).expect("a string serialises")
}

impl PackFile {
    /// Read a pack file's text strictly (REG-1): an unknown key anywhere, a name that isn't a
    /// pack's, no version, a version or a hash listed twice, a version both yanked and removed, a
    /// reason left empty, or an address outside the scope's GitHub is refused, in words.
    pub fn read(text: &str) -> Result<PackFile, String> {
        let raw: RawPackFile = toml::from_str(text).map_err(|e| e.message().trim().to_string())?;
        let name = raw.name;
        if name.is_std() {
            return Err("std comes with music2 and is never listed: a pack is named @owner/name".into());
        }
        let owner = github_owner(&name);
        repository_ok(&raw.repository, owner)?;
        if raw.versions.is_empty() {
            return Err(format!("{name} lists no version: add a [[versions]] table (`m2 registry add` writes one)"));
        }
        let mut versions: Vec<ListedVersion> = vec![];
        for v in raw.versions {
            let version =
                Version::parse(&v.version).ok_or_else(|| format!("`version = \"{}\"` must be a version such as \"1.2.0\"", v.version))?;
            source_ok(&v.source, owner).map_err(|e| format!("{version}: {e}"))?;
            for (key, why) in [("yanked", &v.yanked), ("removed", &v.removed)] {
                if why.as_deref().is_some_and(|w| w.trim().is_empty()) {
                    return Err(format!("{version}: `{key}` says why, in words: it can't be empty"));
                }
            }
            if v.yanked.is_some() && v.removed.is_some() {
                return Err(format!("{version} is both yanked and removed: a removed version is gone, so take out its `yanked`"));
            }
            if versions.iter().any(|w| w.version == version) {
                return Err(format!("{version} is listed twice"));
            }
            if let Some(w) = versions.iter().find(|w| w.hash == v.hash) {
                return Err(format!("{version} has the hash of {}: two versions are never the same files", w.version));
            }
            versions.push(ListedVersion { version, hash: v.hash, source: v.source, yanked: v.yanked, removed: v.removed });
        }
        Ok(PackFile { name, repository: raw.repository, versions })
    }

    /// Where the pack file of `name` is in the registry, relative: `packs/@alice/felt-piano.toml`.
    pub fn path_of(name: &PackName) -> PathBuf {
        let mut p = Path::new(PACKS).join(name.folder());
        p.set_extension("toml");
        p
    }

    /// A new pack file's text: its name and repository, then each version.
    pub fn text(&self) -> String {
        let mut out = format!("name = {}\nrepository = {}\n", quoted(self.name.as_str()), quoted(&self.repository));
        for v in &self.versions {
            out.push('\n');
            out.push_str(&PackFile::version_text(v));
        }
        out
    }

    /// One version's `[[versions]]` table, as `m2 registry add` appends it.
    pub fn version_text(v: &ListedVersion) -> String {
        let mut out =
            format!("[[versions]]\nversion = \"{}\"\nhash = \"{}\"\nsource = {}\n", v.version, v.hash.prefixed(), quoted(&v.source));
        for (key, why) in [("yanked", &v.yanked), ("removed", &v.removed)] {
            if let Some(why) = why {
                out.push_str(&format!("{key} = {}\n", quoted(why)));
            }
        }
        out
    }

    pub fn get(&self, version: Version) -> Option<&ListedVersion> {
        self.versions.iter().find(|v| v.version == version)
    }

    /// Its newest version that is neither yanked nor removed: what search offers (design §9.3).
    pub fn latest(&self) -> Option<&ListedVersion> {
        self.versions.iter().filter(|v| v.yanked.is_none() && v.removed.is_none()).max_by_key(|v| v.version)
    }
}

/// The registry's folder (REG-1): every pack file under `packs/`, by name, and the word list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Registry {
    pub packs: Vec<PackFile>,
    /// The words a pack's tags may use (design P7), from `tags.txt`.
    pub tags: BTreeSet<String>,
}

/// Whether `w` is a tag: 1 to 39 of `a-z`, `0-9` and inner `-`, as a pack name's part.
fn is_word(w: &str) -> bool {
    (1..=39).contains(&w.len())
        && w.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !w.starts_with('-')
        && !w.ends_with('-')
}

/// `tags.txt`'s words: one a line; blank lines and lines starting `#` aside.
pub fn read_tags(text: &str) -> Result<BTreeSet<String>, Vec<String>> {
    let mut words = BTreeSet::new();
    let mut problems = vec![];
    for (n, line) in text.lines().enumerate() {
        let w = line.trim();
        if w.is_empty() || w.starts_with('#') {
            continue;
        }
        match is_word(w) {
            true => drop(words.insert(w.to_string())),
            false => problems.push(format!("{TAGS}:{}: `{w}` isn't a tag: lowercase letters, digits and inner hyphens", n + 1)),
        }
    }
    match problems.is_empty() {
        true => Ok(words),
        false => Err(problems),
    }
}

impl Registry {
    /// Read the registry in `dir` strictly (REG-1): `tags.txt`, and under `packs/` only folders
    /// `@owner` holding pack files `name.toml` (dot files aside), each read with
    /// [`PackFile::read`] and naming its own path. Every problem, in words naming the file.
    pub fn read(dir: &Path) -> Result<Registry, Vec<String>> {
        let mut problems = vec![];
        let tags = match std::fs::read_to_string(dir.join(TAGS)) {
            Ok(text) => read_tags(&text).unwrap_or_else(|p| {
                problems.extend(p);
                BTreeSet::new()
            }),
            Err(e) => {
                problems.push(format!("{TAGS} can't be read ({e}): the registry's word list, one tag a line"));
                BTreeSet::new()
            }
        };
        let mut packs = vec![];
        let listed = |d: &Path| -> std::io::Result<Vec<(String, PathBuf, std::fs::FileType)>> {
            let mut v = vec![];
            for e in std::fs::read_dir(d)? {
                let e = e?;
                let name = e.file_name().to_string_lossy().into_owned();
                if !name.starts_with('.') {
                    v.push((name, e.path(), std::fs::symlink_metadata(e.path())?.file_type()));
                }
            }
            v.sort_by(|a, b| a.0.cmp(&b.0));
            Ok(v)
        };
        let owners = listed(&dir.join(PACKS)).unwrap_or_else(|e| {
            problems.push(format!("{PACKS}/ can't be read ({e}): the registry keeps one file a pack there"));
            vec![]
        });
        for (owner, path, kind) in owners {
            if !kind.is_dir() || !owner.starts_with('@') {
                problems.push(format!("{PACKS}/{owner}: only folders named @owner are in {PACKS}/, each holding name.toml files"));
                continue;
            }
            for (file, path, kind) in listed(&path).unwrap_or_default() {
                let at = format!("{PACKS}/{owner}/{file}");
                let Some(stem) = file.strip_suffix(".toml").filter(|_| kind.is_file()) else {
                    problems.push(format!("{at}: only pack files, name.toml, are in {PACKS}/{owner}/"));
                    continue;
                };
                let read = std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| PackFile::read(&t));
                match read {
                    Ok(p) if p.name.as_str() == format!("{owner}/{stem}") => packs.push(p),
                    Ok(p) => problems.push(format!("{at} names {}: a pack's file is {}", p.name, PackFile::path_of(&p.name).display())),
                    Err(e) => problems.push(format!("{at}: {e}")),
                }
            }
        }
        packs.sort_by(|a, b| a.name.cmp(&b.name));
        match problems.is_empty() {
            true => Ok(Registry { packs, tags }),
            false => Err(problems),
        }
    }

    pub fn get(&self, name: &PackName) -> Option<&PackFile> {
        self.packs.iter().find(|p| p.name == *name)
    }

    /// What this registry changed of `base`, the registry as it was, that a version's promise
    /// forbids (REG-2, design P5): a pack file gone, a version gone, or a listed version's hash or
    /// source changed. Adding `yanked` or `removed`, or taking one back, is allowed. In words.
    pub fn broken_promises(&self, base: &Registry) -> Vec<String> {
        let mut out = vec![];
        for was in &base.packs {
            let Some(now) = self.get(&was.name) else {
                out.push(format!(
                    "{} is gone from the registry: a listed pack stays listed. Mark its versions `removed = \"why\"` instead",
                    PackFile::path_of(&was.name).display()
                ));
                continue;
            };
            for v in &was.versions {
                match now.get(v.version) {
                    None => out.push(format!(
                        "{} {} is gone from its file: a listed version stays listed. Mark it `yanked` or `removed` instead",
                        was.name, v.version
                    )),
                    Some(n) if n.hash != v.hash || n.source != v.source => out.push(format!(
                        "{} {} was changed: a listed version is never changed (P5), only yanked or removed. Give the new \
                         files a new version",
                        was.name, v.version
                    )),
                    Some(_) => {}
                }
            }
        }
        out
    }

    /// The packs whose files differ from `base`'s (all of them with no base): what a pull request
    /// changed.
    pub fn changed_from<'a>(&'a self, base: Option<&Registry>) -> Vec<&'a PackFile> {
        self.packs.iter().filter(|p| base.and_then(|b| b.get(&p.name)) != Some(*p)).collect()
    }

    /// What the GitHub account `author` may not change (REG-2): of the packs changed from `base`,
    /// each whose scope isn't `author`'s (in any capitals). `@m2/` is never an author's: its files
    /// are changed by music2's own people, whose pull requests are checked without an author.
    pub fn not_authors(&self, base: Option<&Registry>, author: &str) -> Vec<String> {
        let mut out = vec![];
        for p in self.changed_from(base) {
            let s = scope(&p.name);
            if s == OUR_SCOPE {
                out.push(format!("{} is music2's own (@m2/): only music2's people change it", p.name));
            } else if !s.eq_ignore_ascii_case(author) {
                out.push(format!("{} is @{s}'s: {author} may change only packs named @{}/…", p.name, author.to_ascii_lowercase()));
            }
        }
        out
    }

    /// The blobs of its removed versions that no version still listed uses (REG-7), from each
    /// version's facts (`facts`, by the version's hash): what to delete from the bucket. A removed
    /// version whose facts aren't there is named in the second list, its blobs unknown.
    pub fn removed_blobs(&self, facts: &dyn Fn(&Sha) -> Option<Facts>) -> (Vec<Sha>, Vec<String>) {
        let (mut kept, mut gone, mut unknown) = (BTreeSet::new(), BTreeSet::new(), vec![]);
        for p in &self.packs {
            for v in &p.versions {
                match (facts(&v.hash), v.removed.is_some()) {
                    (Some(f), true) => gone.extend(f.blobs),
                    (Some(f), false) => kept.extend(f.blobs),
                    (None, true) => unknown.push(format!("{} {}", p.name, v.version)),
                    (None, false) => {}
                }
            }
        }
        (gone.difference(&kept).copied().collect(), unknown)
    }
}

/// `v` as JSON in RFC 8785 form (`crate::jcs`): the one form the robot writes, so the same value
/// is always the same bytes and the same hash (REG-3).
fn jcs<T: Serialize>(v: &T) -> Vec<u8> {
    crate::jcs::canonical(&serde_json::to_value(v).expect("the registry's files serialise")).into_bytes()
}

/// `bytes` as JSON of `T`, read strictly, in words naming `what`.
fn strict<T: for<'de> Deserialize<'de>>(bytes: &[u8], what: &str) -> Result<T, String> {
    // The format first, so a newer file says to update music2, not which field it doesn't know.
    let v: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| format!("{what} isn't JSON: {e}"))?;
    match v.get("format").and_then(serde_json::Value::as_u64) {
        Some(n) if n == u64::from(FORMAT) => {}
        Some(n) => return Err(format!("{what} is format {n}, and this music2 reads format {FORMAT}: update music2")),
        None => return Err(format!("{what} has no `format`")),
    }
    serde_json::from_value(v).map_err(|e| format!("{what} can't be read: {e}"))
}

/// Whether `s` is a time as the robot writes one: UTC to the second, `2026-10-30T12:00:00Z`.
pub fn is_time(s: &str) -> bool {
    let b = s.as_bytes();
    let digits = |r: std::ops::Range<usize>| b.get(r.clone()).is_some_and(|d| d.iter().all(u8::is_ascii_digit));
    let n = |r: std::ops::Range<usize>| s.get(r).and_then(|d| d.parse::<u32>().ok()).unwrap_or(99);
    b.len() == 20
        && digits(0..4)
        && b[4] == b'-'
        && digits(5..7)
        && b[7] == b'-'
        && digits(8..10)
        && b[10] == b'T'
        && digits(11..13)
        && b[13] == b':'
        && digits(14..16)
        && b[16] == b':'
        && digits(17..19)
        && b[19] == b'Z'
        && (1..=12).contains(&n(5..7))
        && (1..=31).contains(&n(8..10))
        && n(11..13) < 24
        && n(14..16) < 60
        && n(17..19) < 60
}

/// `secs` after 1970 as a time the robot writes, in UTC (Howard Hinnant's days-to-civil).
pub fn utc(secs: u64) -> String {
    let (days, rest) = (secs / 86_400, secs % 86_400);
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", rest / 3600, rest % 3600 / 60, rest % 60)
}

/// `catalog.json` (REG-3): which catalog is current, its size, and when it was made.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pointer {
    pub format: u32,
    /// The catalog's hash: it is at [`catalog_path`] of it.
    pub catalog: Sha,
    /// The catalog's size, in bytes.
    pub size: u64,
    /// When it was made ([`is_time`]).
    pub made: String,
}

impl Pointer {
    /// Read `catalog.json` strictly: another format, an unknown field or a time not written as
    /// the robot writes one is refused.
    pub fn read(bytes: &[u8]) -> Result<Pointer, String> {
        let p: Pointer = strict(bytes, POINTER)?;
        match is_time(&p.made) {
            true => Ok(p),
            false => Err(format!("{POINTER}'s `made` is `{}`, not a time such as 2026-10-30T12:00:00Z", p.made)),
        }
    }

    pub fn bytes(&self) -> Vec<u8> {
        jcs(self)
    }
}

/// What the robot found in one version (REG-4), `versions/<hash>.json`: written once.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Facts {
    pub format: u32,
    pub name: PackName,
    pub version: Version,
    /// The version's hash: its `m2pack.json`'s.
    pub hash: Sha,
    /// What its `m2pkg.toml` says of it.
    pub about: About,
    /// The bytes of all its files, its `m2pack.json` among them: what getting it all fetches.
    pub size: u64,
    /// How many files it has, its `m2pack.json` among them.
    pub files: u64,
    /// Every blob it put in the bucket (its files, its `m2pack.json`, its previews), sorted: what
    /// a removal deletes, but those another version uses (REG-7).
    pub blobs: Vec<Sha>,
    /// Its items, by id.
    pub items: Vec<ItemFacts>,
    /// What made its previews: `music2 0.1.0, @m2/core 1.8.0`.
    pub made_by: String,
}

/// What a version's `m2pkg.toml` says of it (PACK-2).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct About {
    pub description: String,
    pub license: String,
    pub authors: Vec<String>,
    pub tags: Vec<String>,
}

/// What an item is (LIB-1's kinds, but plugins, which no pack holds).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemKind {
    Instrument,
    Sound,
    Effect,
    Sample,
}

/// One item of a version, as the library would list it from the installed pack (LIB-1).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemFacts {
    pub kind: ItemKind,
    /// Its id in the library: `@alice/felt-piano:felt-piano`, `sound:std/@alice/felt-piano:felt-piano/Dark`,
    /// `sample:@alice/felt-piano:samples/kick.wav`.
    pub id: String,
    pub label: String,
    pub category: String,
    pub description: String,
    pub tags: Vec<String>,
    /// What adding it names: a ComponentId; for a sound, its sound's; for a sample its package
    /// file, `@alice/felt-piano:samples/kick.wav`.
    pub component: String,
    /// Adding it makes a track that plays notes.
    pub instrument: bool,
    /// It goes on a track's sound.
    pub effect: bool,
    /// A sound's preset: its name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    /// A sample's facts (LIB-4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample: Option<SampleFacts>,
    /// Its preview's blob (REG-5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<Sha>,
    /// Why it has no preview, in words, when it has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_preview: Option<String>,
    /// A sample's own file, as its version lists it: all that getting the sample alone fetches.
    /// None for an instrument, an effect or a sound, which come with their whole version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<Listed>,
}

/// A sample's type (LIB-4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SampleKind {
    #[serde(rename = "loop")]
    Loop,
    #[serde(rename = "one-shot")]
    OneShot,
}

/// What is said and measured of a sample (LIB-4).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SampleFacts {
    #[serde(rename = "type")]
    pub kind: SampleKind,
    /// A word of the library's list (`m2_editor::library::samples::INSTRUMENTS`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instrument: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bpm: Option<f64>,
    /// As the library writes a key: `Am`, `F#`, `Bbm`, `C` (read back with `samples::key_said`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    pub seconds: f64,
    pub channels: u16,
    pub rate: u32,
    /// Its loudest sample: 1 is full scale.
    pub peak: f64,
}

/// Whether `items` are by id, each once.
fn by_id(items: &[ItemFacts]) -> bool {
    items.windows(2).all(|w| w[0].id < w[1].id)
}

/// Whether every number of `items` is a number: a NaN can't be JSON.
fn finite(items: &[ItemFacts]) -> bool {
    items.iter().filter_map(|i| i.sample.as_ref()).all(|s| s.seconds.is_finite() && s.peak.is_finite() && s.bpm.is_none_or(f64::is_finite))
}

impl Facts {
    /// Read a version's facts strictly: another format, an unknown field, blobs or items out of
    /// order, or a sample with no facts or file is refused.
    pub fn read(bytes: &[u8]) -> Result<Facts, String> {
        let f: Facts = strict(bytes, "a version's facts")?;
        let at = format!("the facts of {} {}", f.name, f.version);
        if !f.blobs.windows(2).all(|w| w[0] < w[1]) || !by_id(&f.items) {
            return Err(format!("{at} list their blobs or items out of order"));
        }
        if f.items.iter().any(|i| (i.kind == ItemKind::Sample) != (i.sample.is_some() && i.file.is_some())) {
            return Err(format!("{at}: a sample, and only a sample, has its sample facts and its file"));
        }
        Ok(f)
    }

    /// [`Facts::read`], for version `version` of `name` with `hash` (as the registry or the
    /// catalog lists it): refused when they are another's.
    pub fn read_for(bytes: &[u8], name: &PackName, version: Version, hash: Sha) -> Result<Facts, String> {
        let f = Facts::read(bytes)?;
        match (&f.name, f.version, f.hash) == (name, version, hash) {
            true => Ok(f),
            false => Err(format!("the facts at {} are of {} {}, not {name} {version}", facts_path(&hash), f.name, f.version)),
        }
    }

    pub fn bytes(&self) -> Vec<u8> {
        jcs(self)
    }
}

/// A catalog (REG-4), `catalog/<hash>.json`: every listed pack, by name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub format: u32,
    pub made: String,
    pub packs: Vec<CatalogPack>,
}

/// A listed pack in the catalog: what its latest version's `m2pkg.toml` says, its versions, and
/// its latest's items.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogPack {
    pub name: PackName,
    pub repository: String,
    pub description: String,
    pub license: String,
    pub authors: Vec<String>,
    pub tags: Vec<String>,
    /// Its newest version that is neither yanked nor removed: what search offers. None when every
    /// version is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest: Option<Version>,
    /// Oldest first.
    pub versions: Vec<CatalogVersion>,
    /// The latest's items, by id.
    pub items: Vec<ItemFacts>,
}

/// One listed version in the catalog. A removed one has no size and no files: it can't be got.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogVersion {
    pub version: Version,
    pub hash: Sha,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub yanked: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub removed: Option<String>,
}

impl Catalog {
    /// The catalog of `registry` as of `made` (REG-4), from each listed version's facts (`facts`,
    /// by the version's hash; a removed version's may be missing): packs by name, versions oldest
    /// first, the latest's items. The same registry, facts and time give the same bytes. Each
    /// version that isn't removed and has no facts, in words.
    pub fn make(registry: &Registry, facts: &dyn Fn(&Sha) -> Option<Facts>, made: &str) -> Result<Catalog, Vec<String>> {
        let mut packs = vec![];
        let mut problems = vec![];
        for p in &registry.packs {
            let mut listed: Vec<&ListedVersion> = p.versions.iter().collect();
            listed.sort_by_key(|v| v.version);
            let mut versions = vec![];
            // Every version's facts there, a removed one's too: what the pack says of itself.
            let mut found: BTreeMap<Version, Facts> = BTreeMap::new();
            for v in listed {
                let f = facts(&v.hash);
                if f.is_none() && v.removed.is_none() {
                    problems.push(format!("{} {} has no facts ({}): build it first", p.name, v.version, facts_path(&v.hash)));
                }
                let kept = v.removed.is_none();
                versions.push(CatalogVersion {
                    version: v.version,
                    hash: v.hash,
                    size: f.as_ref().filter(|_| kept).map(|f| f.size),
                    files: f.as_ref().filter(|_| kept).map(|f| f.files),
                    yanked: v.yanked.clone(),
                    removed: v.removed.clone(),
                });
                if let Some(f) = f {
                    found.insert(v.version, f);
                }
            }
            let latest = p.latest().map(|v| v.version);
            // What it says of itself: its latest's, else its newest's there.
            let about = latest.and_then(|l| found.get(&l)).or_else(|| found.values().next_back());
            let a = about.map(|f| f.about.clone()).unwrap_or_default();
            let items = latest.and_then(|l| found.get(&l)).map(|f| f.items.clone()).unwrap_or_default();
            packs.push(CatalogPack {
                name: p.name.clone(),
                repository: p.repository.clone(),
                description: a.description,
                license: a.license,
                authors: a.authors,
                tags: a.tags,
                latest,
                versions,
                items,
            });
        }
        packs.sort_by(|a, b| a.name.cmp(&b.name));
        match problems.is_empty() {
            true => Ok(Catalog { format: FORMAT, made: made.to_string(), packs }),
            false => Err(problems),
        }
    }

    /// Read a catalog strictly: another format, an unknown field, packs, versions or items out of
    /// order, a latest that isn't one of its versions or is yanked or removed, or a removed
    /// version with a size (or one kept without) is refused.
    pub fn read(bytes: &[u8]) -> Result<Catalog, String> {
        let c: Catalog = strict(bytes, "the catalog")?;
        if !is_time(&c.made) {
            return Err(format!("the catalog's `made` is `{}`, not a time such as 2026-10-30T12:00:00Z", c.made));
        }
        if !c.packs.windows(2).all(|w| w[0].name < w[1].name) {
            return Err("the catalog's packs are out of order".into());
        }
        for p in &c.packs {
            if !p.versions.windows(2).all(|w| w[0].version < w[1].version) || !by_id(&p.items) || !finite(&p.items) {
                return Err(format!("the catalog's {} lists its versions or items out of order", p.name));
            }
            if let Some(l) = p.latest
                && !p.versions.iter().any(|v| v.version == l && v.yanked.is_none() && v.removed.is_none())
            {
                return Err(format!("the catalog's {} says its latest is {l}, which it doesn't offer", p.name));
            }
            if p.versions.iter().any(|v| v.removed.is_some() == (v.size.is_some() || v.files.is_some())) {
                return Err(format!("the catalog's {}: a removed version has no size or files, and every other has both", p.name));
            }
        }
        Ok(c)
    }

    /// [`Catalog::read`] of the catalog `pointer` names: refused unless `bytes` are its size and
    /// hash, so a broken or changed one is never used.
    pub fn read_checked(bytes: &[u8], pointer: &Pointer) -> Result<Catalog, String> {
        if bytes.len() as u64 != pointer.size || Sha::of(bytes) != pointer.catalog {
            return Err(format!("the catalog fetched isn't the one {POINTER} names ({}): it was changed or cut short", pointer.catalog));
        }
        Catalog::read(bytes)
    }

    pub fn bytes(&self) -> Vec<u8> {
        jcs(self)
    }

    /// Its pointer, `catalog.json`.
    pub fn pointer(&self) -> Pointer {
        let bytes = self.bytes();
        Pointer { format: FORMAT, catalog: Sha::of(&bytes), size: bytes.len() as u64, made: self.made.clone() }
    }

    pub fn get(&self, name: &PackName) -> Option<&CatalogPack> {
        self.packs.iter().find(|p| p.name == *name)
    }
}

impl CatalogPack {
    pub fn get(&self, version: Version) -> Option<&CatalogVersion> {
        self.versions.iter().find(|v| v.version == version)
    }
}

#[cfg(test)]
mod tests;
