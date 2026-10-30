//! The registry (REG-1 to REG-7): the list of packs anyone can get, and the bucket its robot
//! (`m2 registry`, `m2_editor::registry`) fills from it. Here are their formats, each read strictly
//! and written in one form, so the robot that writes them and the app that reads them agree to the
//! byte, and the rules of the list itself.
//!
//! ```text
//! PackFile    packs/@owner/name.toml: a pack's versions, each with its hash and source (REG-1)
//! Removal     removed.toml: the versions taken down, which only music2's people list (REG-7)
//! Registry    the registry's folder: every pack file, tags.txt, removed.toml (REG-1); what a change
//!             may not do (REG-2), and what may never change once published (REG-5)
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

/// The parts of `url` after `https://github.com/<owner>/`, when it is a plain address there:
/// `owner` in any capitals (GitHub's names don't care), then only letters, digits and `-._/`, with
/// no empty, `.` or `..` part. No `%`: an encoded part (`%2e%2e`, `%2F`) could name another
/// owner's repo once the server decodes it. Why not, in words naming `what` it is.
fn under_github<'a>(url: &'a str, owner: &str, what: &str) -> Result<Vec<&'a str>, String> {
    let want = format!("https://github.com/{owner}/");
    let rest = url.get(want.len()..).unwrap_or_default();
    let plain = rest.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._/".contains(&b));
    let parts: Vec<&str> = rest.split('/').collect();
    let at = url.get(..want.len()).is_some_and(|p| p.eq_ignore_ascii_case(&want)) && url.starts_with("https://github.com/");
    if !at || !plain || parts.iter().any(|p| p.is_empty() || *p == "." || *p == "..") {
        return Err(format!(
            "{what} `{url}` must be a plain address under {want} (only letters, digits and - . _ /, no empty, `.` or `..` part): \
             a pack comes from its owner's own GitHub"
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

/// One version a pack file lists. Whether it was taken down is `removed.toml`'s ([`Removal`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListedVersion {
    pub version: Version,
    /// Its `m2pack.json`'s hash (PACK-6): what a song's `music2.lock` pins (PACK-8).
    pub hash: Sha,
    /// Where the robot gets its files: a `.tar.gz` of the pack's folder.
    pub source: String,
    /// Why it is hidden from search; a song that locks it still gets it (design §9.3).
    pub yanked: Option<String>,
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
    /// Kept only to refuse it in words: a take-down is `removed.toml`'s, which only music2's people
    /// change (REG-7).
    removed: Option<toml::Value>,
}

/// A string as TOML (and JSON) writes it, quoted and escaped.
fn quoted(s: &str) -> String {
    serde_json::to_string(s).expect("a string serialises")
}

impl PackFile {
    /// Read a pack file's text strictly (REG-1): an unknown key anywhere, a name that isn't a
    /// pack's, no version, a version or a hash listed twice, a reason left empty, a take-down
    /// (which is `removed.toml`'s), or an address outside the scope's GitHub is refused, in words.
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
            if v.removed.is_some() {
                return Err(format!(
                    "{version}: a take-down isn't written in a pack's file but in {REMOVED}, which only music2's people change. \
                     To hide a version from search, write `yanked = \"why\"`"
                ));
            }
            if v.yanked.as_deref().is_some_and(|w| w.trim().is_empty()) {
                return Err(format!("{version}: `yanked` says why, in words: it can't be empty"));
            }
            if versions.iter().any(|w| w.version == version) {
                return Err(format!("{version} is listed twice"));
            }
            if let Some(w) = versions.iter().find(|w| w.hash == v.hash) {
                return Err(format!("{version} has the hash of {}: two versions are never the same files", w.version));
            }
            versions.push(ListedVersion { version, hash: v.hash, source: v.source, yanked: v.yanked });
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
        if let Some(why) = &v.yanked {
            out.push_str(&format!("yanked = {}\n", quoted(why)));
        }
        out
    }

    pub fn get(&self, version: Version) -> Option<&ListedVersion> {
        self.versions.iter().find(|v| v.version == version)
    }
}

/// The registry's file of take-downs (REG-7), at its root: only music2's people change it, and an
/// entry is never taken out.
pub const REMOVED: &str = "removed.toml";

/// A version taken down, as `removed.toml` lists it: `[[removed]]` with its pack's `name`, its
/// `version`, its `hash` (the one its pack file lists) and `why`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Removal {
    pub name: PackName,
    pub version: Version,
    pub hash: Sha,
    pub why: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRemoved {
    #[serde(default)]
    removed: Vec<Removal>,
}

/// `removed.toml`'s take-downs, read strictly: an unknown key, an empty reason or one version twice
/// is refused, in words.
pub fn read_removed(text: &str) -> Result<Vec<Removal>, String> {
    let raw: RawRemoved = toml::from_str(text).map_err(|e| format!("{REMOVED}: {}", e.message().trim()))?;
    let mut out: Vec<Removal> = vec![];
    for r in raw.removed {
        if r.why.trim().is_empty() {
            return Err(format!("{REMOVED}: {} {} says no why: a take-down says why, in words", r.name, r.version));
        }
        if out.iter().any(|o| o.name == r.name && o.version == r.version) {
            return Err(format!("{REMOVED}: {} {} is taken down twice", r.name, r.version));
        }
        out.push(r);
    }
    Ok(out)
}

/// The registry's folder (REG-1): every pack file under `packs/`, by name, the word list, the
/// take-downs, and the bytes of every file a change can touch.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Registry {
    pub packs: Vec<PackFile>,
    /// The words a pack's tags may use (design P7), from `tags.txt`.
    pub tags: BTreeSet<String>,
    /// The versions taken down, from `removed.toml` (REG-7).
    pub removed: Vec<Removal>,
    /// Every file under `packs/` (dot files too), `tags.txt` and `removed.toml`, by its path in
    /// the registry, with its bytes: what [`Registry::not_authors`] compares (REG-2).
    pub files: BTreeMap<String, Vec<u8>>,
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

/// Every file under `dir`, by its path from the registry's root (`packs/@alice/drums.toml`), dot
/// files and folders too, with its bytes. A link, a pipe or a device anywhere is a problem, never
/// followed or read.
fn files_under(root: &Path, rel: &str, out: &mut BTreeMap<String, Vec<u8>>, problems: &mut Vec<String>) {
    let entries = match std::fs::read_dir(root.join(rel)) {
        Ok(e) => e,
        Err(e) => return problems.push(format!("{rel}/ can't be read ({e})")),
    };
    for e in entries.flatten() {
        let path = format!("{rel}/{}", e.file_name().to_string_lossy());
        match std::fs::symlink_metadata(e.path()).map(|m| m.file_type()) {
            Ok(t) if t.is_dir() => files_under(root, &path, out, problems),
            Ok(t) if t.is_file() => match std::fs::read(e.path()) {
                Ok(bytes) => drop(out.insert(path, bytes)),
                Err(e) => problems.push(format!("{path} can't be read ({e})")),
            },
            Ok(t) if t.is_symlink() => problems.push(format!("{path} is a link: the registry holds only plain files")),
            Ok(_) => problems.push(format!("{path} isn't a plain file: the registry holds only plain files")),
            Err(e) => problems.push(format!("{path} can't be read ({e})")),
        }
    }
}

impl Registry {
    /// Read the registry in `dir` strictly (REG-1): `tags.txt`; under `packs/` only folders
    /// `@owner` holding pack files `name.toml` (dot files aside), each read with
    /// [`PackFile::read`] and naming its own path, and no link, pipe or device anywhere; each hash
    /// listed once in the whole registry; and `removed.toml`, if it is there, naming only listed
    /// versions by their listed hashes. Every problem, in words naming the file.
    pub fn read(dir: &Path) -> Result<Registry, Vec<String>> {
        let mut problems = vec![];
        let mut files = BTreeMap::new();
        for top in [TAGS, REMOVED] {
            match std::fs::symlink_metadata(dir.join(top)) {
                Ok(m) if m.file_type().is_file() => match std::fs::read(dir.join(top)) {
                    Ok(bytes) => drop(files.insert(top.to_string(), bytes)),
                    Err(e) => problems.push(format!("{top} can't be read ({e})")),
                },
                Ok(_) => problems.push(format!("{top} isn't a plain file: the registry holds only plain files")),
                Err(_) if top == REMOVED => {}
                Err(e) => problems.push(format!("{TAGS} can't be read ({e}): the registry's word list, one tag a line")),
            }
        }
        let tags = match files.get(TAGS).map(|b| String::from_utf8_lossy(b).into_owned()) {
            Some(text) => read_tags(&text).unwrap_or_else(|p| {
                problems.extend(p);
                BTreeSet::new()
            }),
            None => BTreeSet::new(),
        };
        let removed = match files.get(REMOVED).map(|b| String::from_utf8_lossy(b).into_owned()) {
            Some(text) => read_removed(&text).unwrap_or_else(|p| {
                problems.push(p);
                vec![]
            }),
            None => vec![],
        };
        match std::fs::symlink_metadata(dir.join(PACKS)) {
            Ok(m) if m.file_type().is_dir() => files_under(dir, PACKS, &mut files, &mut problems),
            _ => problems.push(format!("{PACKS}/ isn't a folder: the registry keeps one file a pack there")),
        }
        let mut packs = vec![];
        for (path, bytes) in &files {
            let Some(rest) = path.strip_prefix(&format!("{PACKS}/")) else { continue };
            // Dot files and folders, at any depth, are no pack's; they are compared all the same.
            if rest.split('/').any(|p| p.starts_with('.')) {
                continue;
            }
            let (owner, file) = rest.split_once('/').unwrap_or(("", rest));
            if !owner.starts_with('@') {
                problems.push(format!("{path}: only folders named @owner are in {PACKS}/, each holding name.toml files"));
                continue;
            }
            let Some(stem) = file.strip_suffix(".toml").filter(|f| !f.contains('/')) else {
                problems.push(format!("{path}: only pack files, name.toml, are in {PACKS}/{owner}/"));
                continue;
            };
            match std::str::from_utf8(bytes).map_err(|e| e.to_string()).and_then(PackFile::read) {
                Ok(p) if p.name.as_str() == format!("{owner}/{stem}") => packs.push(p),
                Ok(p) => problems.push(format!("{path} names {}: a pack's file is {}", p.name, PackFile::path_of(&p.name).display())),
                Err(e) => problems.push(format!("{path}: {e}")),
            }
        }
        packs.sort_by(|a, b| a.name.cmp(&b.name));
        let mut hashes: BTreeMap<Sha, (&PackName, Version)> = BTreeMap::new();
        for p in &packs {
            for v in &p.versions {
                if let Some((n, w)) = hashes.insert(v.hash, (&p.name, v.version)) {
                    problems.push(format!(
                        "{} {} and {n} {w} list one hash: two versions are never the same files, so each hash is listed once",
                        p.name, v.version
                    ));
                }
            }
        }
        for r in &removed {
            let listed = packs.iter().find(|p| p.name == r.name).and_then(|p| p.get(r.version));
            if listed.is_none_or(|v| v.hash != r.hash) {
                problems
                    .push(format!("{REMOVED}: {} {} isn't listed with that hash: a take-down names a listed version", r.name, r.version));
            }
        }
        match problems.is_empty() {
            true => Ok(Registry { packs, tags, removed, files }),
            false => Err(problems),
        }
    }

    pub fn get(&self, name: &PackName) -> Option<&PackFile> {
        self.packs.iter().find(|p| p.name == *name)
    }

    /// Version `version` of `name`'s take-down, if it was taken down (REG-7).
    pub fn removal(&self, name: &PackName, version: Version) -> Option<&Removal> {
        self.removed.iter().find(|r| r.name == *name && r.version == version)
    }

    /// Its newest version of `p` that is neither yanked nor taken down: what search offers.
    pub fn latest<'a>(&self, p: &'a PackFile) -> Option<&'a ListedVersion> {
        p.versions.iter().filter(|v| v.yanked.is_none() && self.removal(&p.name, v.version).is_none()).max_by_key(|v| v.version)
    }

    /// What this registry changed of `base`, the registry as it was, that a version's promise
    /// forbids (REG-2, design P5): a pack file gone, a version gone, a listed version's hash or
    /// source changed, or a take-down taken back. Adding `yanked`, or taking it back, is allowed.
    /// In words.
    pub fn broken_promises(&self, base: &Registry) -> Vec<String> {
        let mut out = vec![];
        for was in &base.packs {
            let Some(now) = self.get(&was.name) else {
                out.push(format!(
                    "{} is gone from the registry: a listed pack stays listed. Yank its versions instead",
                    PackFile::path_of(&was.name).display()
                ));
                continue;
            };
            for v in &was.versions {
                match now.get(v.version) {
                    None => out
                        .push(format!("{} {} is gone from its file: a listed version stays listed. Yank it instead", was.name, v.version)),
                    Some(n) if n.hash != v.hash || n.source != v.source => out.push(format!(
                        "{} {} was changed: a listed version is never changed (P5), only yanked. Give the new files a new version",
                        was.name, v.version
                    )),
                    Some(_) => {}
                }
            }
        }
        for r in base.removed.iter().filter(|r| !self.removed.contains(r)) {
            out.push(format!(
                "{REMOVED}: {} {} was taken down, and a take-down is never taken back: its files are gone",
                r.name, r.version
            ));
        }
        out
    }

    /// What the GitHub account `author` may not change (REG-2): every file whose bytes differ from
    /// `base`'s (every file, with no base) that isn't in a folder of the author's own scope,
    /// `packs/@<author>/` in any capitals. `tags.txt`, `removed.toml` and any other file are music2's
    /// people's, and `@m2/` is never an author's: those change only in pull requests music2's
    /// people make, which are checked without an author. In words, a file a line.
    pub fn not_authors(&self, base: Option<&Registry>, author: &str) -> Vec<String> {
        let none = BTreeMap::new();
        let was = base.map_or(&none, |b| &b.files);
        let changed: BTreeSet<&String> =
            self.files.keys().chain(was.keys()).filter(|p| base.is_none() || self.files.get(*p) != was.get(*p)).collect();
        let mut out = vec![];
        for path in changed {
            let owner = path.strip_prefix(&format!("{PACKS}/@")).and_then(|r| r.split_once('/')).map(|(o, _)| o);
            match owner {
                Some(OUR_SCOPE) => out.push(format!("{path} is music2's own (@m2/): only music2's people change it")),
                Some(o) if o.eq_ignore_ascii_case(author) => {}
                Some(o) => out.push(format!("{path} is @{o}'s: {author} may change only packs named @{}/…", author.to_ascii_lowercase())),
                None => out.push(format!("{path} is music2's people's to change: an author changes only their own packs' files")),
            }
        }
        out
    }

    /// What this registry would change of what is published already, `published` (the bucket's
    /// catalog), that may never change (REG-5): a published version gone or listed with another
    /// hash or source, a take-down taken back, or a hash a published version has listed for
    /// another version. In words; none is allowed, by anyone.
    pub fn breaks(&self, published: &Catalog) -> Vec<String> {
        let mut out = vec![];
        let mut hashes: BTreeMap<Sha, (&PackName, Version)> = BTreeMap::new();
        for cp in &published.packs {
            let now = self.get(&cp.name);
            for cv in &cp.versions {
                hashes.insert(cv.hash, (&cp.name, cv.version));
                let at = format!("{} {}", cp.name, cv.version);
                match now.and_then(|p| p.get(cv.version)) {
                    None => out.push(format!("{at} is published, and gone from the registry: a published version stays listed (P5)")),
                    Some(v) if v.hash != cv.hash || v.source != cv.source => out.push(format!(
                        "{at} is published with the hash {} and the source {}, and the registry now lists it with others: a \
                         published version never changes (P5). Put it back as it was, and give new files a new version",
                        cv.hash, cv.source
                    )),
                    Some(_) => {}
                }
                if cv.removed.is_some() && self.removal(&cp.name, cv.version).is_none() {
                    out.push(format!("{at} was taken down, and a take-down is never taken back: its files are gone from the bucket"));
                }
            }
        }
        for p in &self.packs {
            for v in &p.versions {
                if let Some((n, w)) = hashes.get(&v.hash).filter(|(n, w)| (*n, *w) != (&p.name, v.version)) {
                    out.push(format!("{} {} lists the hash of the published {n} {w}: each hash is one version's", p.name, v.version));
                }
            }
        }
        out
    }

    /// The blobs of its taken-down versions that no other listed version uses (REG-7), from each
    /// version's facts (`facts`, by the version's hash): what to delete from the bucket. A
    /// taken-down version whose facts aren't there is named in the second list, its blobs unknown.
    pub fn removed_blobs(&self, facts: &dyn Fn(&Sha) -> Option<Facts>) -> (Vec<Sha>, Vec<String>) {
        let (mut kept, mut gone, mut unknown) = (BTreeSet::new(), BTreeSet::new(), vec![]);
        for p in &self.packs {
            for v in &p.versions {
                match (facts(&v.hash), self.removal(&p.name, v.version).is_some()) {
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
    /// Where its files came from, as the registry listed it: never changes once published (REG-5).
    pub source: String,
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
    /// by the version's hash): packs by name, versions oldest first, the latest's items. A version
    /// with no facts (one not built yet, or whose build failed) is left out, and a pack none of
    /// whose versions is in; a taken-down version is in, with why, whatever facts there are. The
    /// same registry, facts and time give the same bytes. The versions left out, in words.
    pub fn make(registry: &Registry, facts: &dyn Fn(&Sha) -> Option<Facts>, made: &str) -> (Catalog, Vec<String>) {
        let mut packs = vec![];
        let mut left_out = vec![];
        for p in &registry.packs {
            let mut listed: Vec<&ListedVersion> = p.versions.iter().collect();
            listed.sort_by_key(|v| v.version);
            let mut versions = vec![];
            // Every version's facts there, a removed one's too: what the pack says of itself.
            let mut found: BTreeMap<Version, Facts> = BTreeMap::new();
            for v in listed {
                let f = facts(&v.hash);
                let removed = registry.removal(&p.name, v.version).map(|r| r.why.clone());
                if f.is_none() && removed.is_none() {
                    left_out.push(format!("{} {}", p.name, v.version));
                    continue;
                }
                let kept = removed.is_none();
                versions.push(CatalogVersion {
                    version: v.version,
                    hash: v.hash,
                    source: v.source.clone(),
                    size: f.as_ref().filter(|_| kept).map(|f| f.size),
                    files: f.as_ref().filter(|_| kept).map(|f| f.files),
                    yanked: v.yanked.clone(),
                    removed,
                });
                if let Some(f) = f {
                    found.insert(v.version, f);
                }
            }
            if versions.is_empty() {
                continue;
            }
            let offered = |c: &&CatalogVersion| c.yanked.is_none() && c.removed.is_none();
            let latest = versions.iter().filter(offered).map(|c| c.version).max();
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
        (Catalog { format: FORMAT, made: made.to_string(), packs }, left_out)
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
