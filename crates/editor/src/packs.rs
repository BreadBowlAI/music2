//! Packs in the editor (PACK-10 to PACK-12): what adding a pack's item writes besides its import,
//! which pack an item is from, and `m2 pack`'s check, build and install.

use std::path::{Path, PathBuf};

use m2_session::Session;
use m2_song::pack::{self, Lock, PackName, Store};
use m2_song::tx::{Declared, Op, Payload, Range, TxPath};
use m2_song::{DataPath, Dependency, Requirement, Sha, Version};
use serde::Serialize;

use crate::EditError;
use crate::plan::{self, path};

/// What adding an item of a pack writes besides its import (PACK-10): its `[dependencies]` line
/// when the song doesn't list the pack, its `music2.lock` entry when the lock lacks one; and the
/// files those writes may change, for the Tx's expectation.
#[derive(Clone, Debug, Default)]
pub struct Needs {
    pub ops: Vec<Op>,
    pub ranges: Vec<(TxPath, Declared)>,
}

impl Needs {
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }
}

/// The pack an item is from, as the lists show it (PACK-11): its name and version, and whether it
/// is the song's own folder.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct From {
    pub name: String,
    pub version: String,
}

/// Which pack ComponentId `component` is from, with its version: none for std's and the
/// project's own (PACK-4).
pub fn from(s: &Session, component: &str) -> Option<From> {
    let (package, _) = pack::split_id(component)?;
    if package == pack::STD {
        return None;
    }
    let p = s.packages.get(package).map(|p| p.version.clone());
    let version = p.or_else(|| offered_version(s, package).map(|(v, _)| v.to_string()))?;
    Some(From { name: package.to_string(), version })
}

/// The version and hash of `package` as the lists offer it from the store: its newest installed.
fn offered_version(s: &Session, package: &str) -> Option<(Version, Option<Sha>)> {
    let name = PackName::new(package).ok()?;
    let store = s.packages.store()?;
    let v = *store.versions(&name).last()?;
    Some((v, store.hash(&name, &v).ok()))
}

/// What adding an item of ComponentId `component` writes besides its import (PACK-10). Nothing for
/// std's, the project's own, or a pack the song lists and locks (or takes from a folder).
pub fn needs(s: &Session, component: &str) -> Result<Needs, EditError> {
    needs_all(s, &[component])
}

/// [`needs`] for several items added in one Tx: one edit of `[dependencies]` and one lock for all
/// of their packs.
pub fn needs_all(s: &Session, components: &[&str]) -> Result<Needs, EditError> {
    let rev = s.accepted();
    let (manifest, mut lock) = (&rev.project.manifest, rev.project.lock.clone());
    let (mut lines, mut locked) = (vec![], false);
    for component in components {
        let Some((package, _)) = pack::split_id(component) else { continue };
        if package == pack::STD || package == "clap" {
            continue;
        }
        let name = PackName::new(package).map_err(EditError::Rejected)?;
        let listed = manifest.dependencies.get(package);
        if matches!(listed, Some(Dependency::Path(_))) || lock.get(&name).is_some() {
            continue;
        }
        // The version the song resolves now, else the store's newest.
        let used = match rev.compiled.packs.iter().find(|r| r.name == name) {
            Some(r) => Some((r.version, r.hash)),
            None => offered_version(s, package),
        };
        let (version, hash) =
            used.ok_or_else(|| EditError::Rejected(format!("{package} isn't installed: install it with `m2 pack install <folder>`")))?;
        let hash = hash.ok_or_else(|| EditError::Rejected(format!("{package} {version} has no m2pack.json: install it again")))?;
        if listed.is_none() {
            lines.push((name.clone(), Requirement::minor(version)));
        }
        lock = lock.with(name, version, hash);
        locked = true;
    }
    let mut out = Needs::default();
    if !lines.is_empty() {
        let file = path(m2_song::path::MANIFEST)?;
        let text = rev.snapshot.files.get(&DataPath::Manifest.path()).ok_or_else(|| EditError::NotFound(file.to_string()))?;
        let mut at_all = None;
        let mut written = String::new();
        for (name, req) in &lines {
            let (at, line) = dependency_line(text, name, req)
                .ok_or_else(|| EditError::Rejected(format!("{} has no [dependencies] to add {name} to", m2_song::path::MANIFEST)))?;
            // Every line goes to one place; only the first may need a newline before it.
            written.push_str(if at_all.is_some() { line.trim_start_matches('\n') } else { &line });
            at_all = Some(at);
        }
        let at = at_all.expect("a line");
        out.ops.push(plan::text_edit(&file, text, &[(at, at, written)]));
        out.ranges.push((file, Declared::Ranges(vec![Range::bytes(at, at)])));
    }
    if locked {
        let file = path(m2_song::path::LOCK)?;
        out.ops.push(Op::Write { file: file.clone(), bytes: Payload::Text(lock.canonical().into()) });
        out.ranges.push((file, Declared::Whole));
    }
    Ok(out)
}

/// Where a `"<name>" = "<req>"` line goes in a manifest's `[dependencies]` table, and the line:
/// after the table's last entry, so its comments and blank lines stay where they are.
pub fn dependency_line(text: &str, name: &PackName, req: &Requirement) -> Option<(u32, String)> {
    let mut offset = 0;
    let mut in_table = false;
    let mut at = None;
    for line in text.split_inclusive('\n') {
        let bare = line.split('#').next().unwrap_or("").trim();
        if bare.starts_with('[') {
            if in_table {
                break;
            }
            in_table = bare == "[dependencies]";
            if in_table {
                at = Some(offset + line.len());
            }
        } else if in_table && !bare.is_empty() {
            at = Some(offset + line.len());
        }
        offset += line.len();
    }
    let at = at?;
    let newline = if at > 0 && !text[..at].ends_with('\n') { "\n" } else { "" };
    Some((at as u32, format!("{newline}\"{}\" = \"{req}\"\n", name.as_str())))
}

/// `music2.lock` as `m2 pack lock` writes it (PACK-8): every pack the song resolves now from the
/// store, by name, with its version and hash. Path packs and std aren't locked.
pub fn lock_now(s: &Session) -> Result<Lock, EditError> {
    let rev = s.accepted();
    let resolution = m2_session::resolve_packs(&rev.project, &s.packages);
    if let Some(d) = resolution.problems.first() {
        return Err(EditError::Rejected(d.message.clone()));
    }
    let mut lock = Lock::default();
    for r in resolution.packs.iter().filter(|r| !r.path) {
        let hash = r.hash.ok_or_else(|| EditError::Rejected(format!("{} {} has no m2pack.json: install it again", r.name, r.version)))?;
        lock = lock.with(r.name.clone(), r.version, hash);
    }
    Ok(lock)
}

/// Write `music2.lock` from what resolves now (`m2 pack lock`, PACK-8): one Tx, one undo step;
/// nothing when it says that already.
pub fn write_lock(s: &mut Session) -> Result<Option<crate::Edited>, EditError> {
    crate::ready(s)?;
    let lock = lock_now(s)?;
    let text = lock.canonical();
    let rev = s.accepted();
    let file = path(m2_song::path::LOCK)?;
    let now = rev.snapshot.files.get(Path::new(m2_song::path::LOCK)).map(|t| t.to_string());
    if now.as_deref() == Some(text.as_str()) || (now.is_none() && lock.packs.is_empty()) {
        return Ok(None);
    }
    let label = ("music2.lock".to_string(), format!("{} packs locked", lock.packs.len()));
    let ops = vec![Op::Write { file, bytes: Payload::Text(text.into()) }];
    let tx = plan::planned(s, label.clone(), ops, m2_song::Expect::Identical);
    let done = plan::run(s, tx, |_, _| Ok(()))?;
    Ok(Some(crate::Edited { revision: done.number, file: m2_song::path::LOCK.into(), before: label.0, after: label.1 }))
}

/// Every installed pack, newest version last, with what each holds: what `m2 pack list` and the
/// Packs list show (PACK-11).
#[derive(Clone, Debug, Serialize)]
pub struct Installed {
    pub name: String,
    pub versions: Vec<String>,
    pub description: String,
    pub license: Option<String>,
    /// The folder of its newest version.
    pub dir: PathBuf,
}

pub fn installed(store: &Store) -> Vec<Installed> {
    let mut out = vec![];
    for (name, versions) in store.list() {
        let Some(newest) = versions.last() else { continue };
        let dir = store.version_dir(&name, newest);
        let m = pack::PkgManifest::at(&dir).ok();
        out.push(Installed {
            name: name.as_str().to_string(),
            versions: versions.iter().map(Version::to_string).collect(),
            description: m.as_ref().map(|m| m.package.description.clone()).unwrap_or_default(),
            license: m.and_then(|m| m.package.license),
            dir,
        });
    }
    out
}

#[cfg(test)]
#[path = "packs_tests.rs"]
mod tests;
