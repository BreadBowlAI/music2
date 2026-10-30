//! Packs in the editor (PACK-10 to PACK-12): what adding a pack's item writes besides its import,
//! which pack an item is from, and `m2 pack`'s check, build and install.

use std::path::{Path, PathBuf};

use m2_session::Session;
use m2_song::pack::{self, Listing, Lock, PackName, Store};
use m2_song::tx::{Declared, Op, Payload, Range, TxPath};
use m2_song::{DataPath, Dependency, Requirement, Sha, Version};
use serde::Serialize;

use crate::EditError;
use crate::plan::{self, path};

/// What adding an item of a pack writes besides its import (PACK-10): its `[dependencies]` line
/// when the song doesn't list the pack, its `music2.lock` entry when the lock lacks one or the song
/// didn't list it; and the files those writes may change, for the Tx's expectation.
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

/// The pack an item is from, as the lists show it (PACK-11): its name and version.
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
    let (version, _) = s.offered_one(package)?;
    Some(From { name: package.to_string(), version: version.to_string() })
}

/// The version and hash of `package` as the lists offer it from the store (PACK-11): the version
/// they were made from, its copy checked against its listing (PACK-6).
fn offered_version(s: &Session, package: &str) -> Option<(Version, Result<Sha, String>)> {
    let name = PackName::new(package).ok()?;
    let (v, _) = s.offered_one(package)?;
    Some((v, s.packages.store()?.check(&name, &v)))
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
        // Listed and locked, or taken from a folder: the song has it. A lock entry for a pack the
        // manifest doesn't list is left from before, and is replaced with the line (PACK-10).
        if matches!(listed, Some(Dependency::Path(_))) || (listed.is_some() && lock.get(&name).is_some()) {
            continue;
        }
        // The version the song resolves now, else the one the lists offer.
        let (version, hash) = match rev.compiled.packs.iter().find(|r| r.name == name) {
            Some(r) => (r.version, r.hash.ok_or_else(|| format!("{package} {} has no m2pack.json", r.version))),
            None => offered_version(s, package)
                .ok_or_else(|| EditError::Rejected(format!("{package} isn't installed: install it with `m2 pack install <folder>`")))?,
        };
        let hash = hash.map_err(|why| {
            EditError::Rejected(format!(
                "{package} {version} can't be added as it is installed ({why}): install it again with `m2 pack install <folder>`"
            ))
        })?;
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

/// `music2.lock` as `m2 pack lock` and `m2 pack update` write it (PACK-8): from the manifest, the
/// lock it replaces and the store, as the project's files are now, never from a build, so it works
/// while the song doesn't build. What moves is `moving`'s. A lock that doesn't read is replaced
/// whole.
pub fn lock_now(s: &Session, moving: pack::Move) -> Result<Lock, EditError> {
    let files = &s.draft().files;
    let text = files.get(Path::new(m2_song::path::MANIFEST)).ok_or_else(|| EditError::NotFound(m2_song::path::MANIFEST.into()))?;
    let manifest = m2_song::Manifest::read(text).map_err(|d| EditError::Rejected(d.message))?;
    let old = files.get(Path::new(m2_song::path::LOCK)).and_then(|t| Lock::read(t).ok()).unwrap_or_default();
    pack::relock(&manifest, &old, s.packages.store(), &m2_session::app(&s.packages), moving)
        .map_err(|d| EditError::Rejected(d.into_iter().map(|d| d.message).collect::<Vec<_>>().join("\n")))
}

/// Write `music2.lock` (`m2 pack lock`, `m2 pack update`, PACK-8) as [`lock_now`] makes it; nothing
/// when it says that already. While the song builds, one Tx, one undo step. While it doesn't (a
/// new requirement its old lock doesn't meet), saved as `m2 edit` saves (AGENT-1): written only
/// when the song then builds, as one undo step; otherwise refused with why, and nothing written.
pub fn write_lock(s: &mut Session, moving: pack::Move) -> Result<Option<crate::Edited>, EditError> {
    let lock = lock_now(s, moving)?;
    let text = lock.canonical();
    let now = s.draft().files.get(Path::new(m2_song::path::LOCK)).cloned();
    if now.as_deref() == Some(text.as_str()) || (now.is_none() && lock.packs.is_empty()) {
        return Ok(None);
    }
    let said: Vec<String> = lock.packs.iter().map(|l| format!("{} {}", l.name, l.version)).collect();
    let label = (m2_song::path::LOCK.to_string(), if said.is_empty() { "no packs locked".into() } else { said.join(", ") });
    let file = path(m2_song::path::LOCK)?;
    let revision = match s.draft_errors.is_empty() {
        true => {
            let ops = vec![Op::Write { file, bytes: Payload::Text(text.into()) }];
            let tx = plan::planned(s, label.clone(), ops, m2_song::Expect::Identical);
            plan::run(s, tx, |_, _| Ok(()))?.number
        }
        false => {
            let base = match &now {
                Some(was) => m2_song::tx::Base::text(was),
                None => m2_song::tx::Base::Absent,
            };
            s.save_built(Path::new(m2_song::path::LOCK), base, &text)?;
            s.landed().filter(|l| l.number > 0).map_or(s.accepted().number, |l| l.number)
        }
    };
    Ok(Some(crate::Edited { revision, file: m2_song::path::LOCK.into(), before: label.0, after: label.1 }))
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

/// The file types a pack may hold (design P3), besides `LICENSE` with no extension.
pub const FILE_TYPES: &[&str] = &["ts", "wasm", "toml", "json", "md", "txt", "wav"];

/// What `m2 pack check` finds in the pack in `dir` (PACK-12), in words: empty when it is good.
/// `std` is the std this app has, which the pack builds against.
pub fn check(dir: &Path, std: &Path) -> Vec<String> {
    let mut out = vec![];
    let m = match pack::PkgManifest::at(dir) {
        Ok(m) => m,
        Err(e) => return vec![e],
    };
    let name = m.package.name.clone();
    if name.is_std() {
        return vec!["std comes with music2; a pack is named @owner/name".into()];
    }
    match m.package.license.as_deref().map(str::trim) {
        Some(l) if !l.is_empty() => {}
        _ => out.push(
            "m2pkg.toml names no licence: add `license = \"CC-BY-4.0\"` (or another that lets anyone share it) under [package]".into(),
        ),
    }
    let mut std_set = m2_runtime::PackageSet::default();
    let std_version = std_set.load(std).ok().and_then(|p| Version::parse(&p.version));
    let sdk = Version::parse(m2_script::SDK_VERSION).expect("the SDK's version");
    if let Err(e) = m.needs(sdk, std_version.unwrap_or(Version(0, 0, 0))) {
        out.push(e);
    }
    match pack::pack_files(dir) {
        Ok(files) => {
            for (rel, _) in files {
                // In any capitals: `.WAV` is a WAV.
                let ext = Path::new(&rel).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
                if rel != "LICENSE" && !FILE_TYPES.contains(&ext.as_str()) {
                    let types: Vec<String> = FILE_TYPES.iter().map(|t| format!(".{t}")).collect();
                    out.push(format!("{rel}: a pack holds only {} files and LICENSE", types.join(" ")));
                }
            }
        }
        Err(e) => out.push(format!("{} can't be read: {e}", dir.display())),
    }
    // Its kernels load, each module checked as a song would load it.
    let mut set = m2_runtime::PackageSet::default();
    let loaded = set.load(std).map(drop).and_then(|()| set.load(dir).map(drop));
    if let Err(e) = loaded {
        out.push(format!("{e:#}"));
        return out;
    }
    let catalog = m2_script::describe(&[(pack::STD.to_string(), std.to_path_buf()), (name.as_str().to_string(), dir.to_path_buf())]);
    let prefix = format!("{name}:");
    for d in catalog.problems.iter().filter(|d| d.message.contains(name.as_str())) {
        out.push(d.message.clone());
    }
    let ours: Vec<_> = catalog.components.values().filter(|d| d.id.starts_with(&prefix)).cloned().collect();
    // Its samples (LIB-4): a pack of samples alone needs no component.
    let (samples, any_samples) = crate::library::samples::check(dir);
    out.extend(samples);
    if ours.is_empty() && !any_samples {
        out.push(format!("{name} declares no component and no sample: an instrument or an effect is a component(…) in src/, a sample a WAV file under samples/ that samples.toml names"));
    }
    for d in &ours {
        if d.editor.category.is_none() {
            out.push(format!("{} ({}) has no category: give it `editor: {{ category: \"Keys\" }}` or another", d.id, d.at));
        } else if !d.is_instrument() && !d.is_effect() {
            out.push(format!("{} ({}) is neither an instrument (notes in, sound out) nor an effect (sound in, sound out)", d.id, d.at));
        }
    }
    out.extend(presets_problems(dir, &name, &ours));
    out.extend(builds(dir, std, &name, &ours, &catalog));
    out
}

/// Every preset in `presets/<component>/` is a preset of that component of the pack, names only
/// its controls, and keeps each value inside its control's range where that range is strict
/// (PKG-8): what loading it on its sound needs (PACK-12).
fn presets_problems(dir: &Path, name: &PackName, ours: &[std::sync::Arc<m2_script::Descriptor>]) -> Vec<String> {
    let mut out = vec![];
    for folder in std::fs::read_dir(dir.join("presets")).into_iter().flatten().flatten() {
        let id = folder.file_name().to_string_lossy().into_owned();
        let sound = format!("{name}:{id}");
        let desc = ours.iter().find(|d| d.id == sound);
        for f in std::fs::read_dir(folder.path()).into_iter().flatten().flatten() {
            let rel = format!("presets/{id}/{}", f.file_name().to_string_lossy());
            let preset = std::fs::read_to_string(f.path()).map_err(|e| e.to_string()).and_then(|t| crate::presets::Preset::parse(&t));
            match (preset, desc) {
                (Err(e), _) => out.push(format!("{rel}: {e}")),
                (Ok(_), None) => out.push(format!("{rel}: {name} has no component `{id}` for it")),
                (Ok(p), Some(_)) if p.sound != sound => out.push(format!("{rel} is for {}, but it is in {sound}'s folder", p.sound)),
                (Ok(p), Some(d)) => {
                    for (slot, v) in &p.values {
                        match d.control(slot) {
                            None => out.push(format!("{rel}: {} has no control `{slot}`", d.label)),
                            Some(c) => match (c.strict, c.range) {
                                (true, Some((lo, hi))) if !(lo..=hi).contains(v) => {
                                    out.push(format!("{rel}: `{slot}` is {v}, outside {}'s range for it, {lo} to {hi}", d.label))
                                }
                                _ => {}
                            },
                        }
                    }
                }
            }
        }
    }
    out
}

/// Every instrument and effect of the pack builds in a song, and every package file its code
/// names is there: a song in a scratch folder that plays each (PACK-12).
fn builds(
    dir: &Path,
    std: &Path,
    name: &PackName,
    ours: &[std::sync::Arc<m2_script::Descriptor>],
    catalog: &m2_script::Catalog,
) -> Vec<String> {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    // In music2's cache folder, under `$HOME` (PLAT-7), and gone when the check ends.
    let song = m2_os::dirs::cache().unwrap_or_else(std::env::temp_dir).join(format!("pack-check-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&song);
    let (mut imports, mut plays) = (std::collections::BTreeSet::new(), vec![]);
    for (i, d) in ours.iter().enumerate() {
        let Some(e) = catalog.exports.get(&d.id).and_then(|e| e.first()) else { continue };
        let root = e.name.split('.').next().unwrap_or(&e.name);
        imports.insert(format!("import {{ {root} }} from {};", crate::tracks::quoted(&e.module)));
        if d.is_instrument() {
            plays.push(format!("const t{i} = track(\"t{i}\", ({{ lane }}) => {}(lane.notes(\"notes\")));", e.name));
        } else if d.is_effect() {
            plays.push(format!("const t{i} = track(\"t{i}\", () => {}(osc.saw(hz(110))));", e.name));
        }
    }
    let channels: Vec<String> = (0..ours.len())
        .filter(|i| plays.iter().any(|p| p.starts_with(&format!("const t{i} "))))
        .map(|i| format!("t{i}: t{i}.out"))
        .collect();
    let code = format!(
        "import {{ hz, song, track }} from \"@m2/core\";\nimport {{ osc }} from \"std/dsp\";\nimport {{ master, mixer }} from \"std/mix\";\n{}\n{}\nexport default song(() => master(mixer({{ {} }})));\n",
        imports.into_iter().collect::<Vec<_>>().join("\n"),
        plays.join("\n"),
        channels.join(", ")
    );
    let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let manifest = format!(
        "[project]\nname = \"check\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"{}\"\n\"{name}\" = {{ path = {} }}\n",
        std_requirement(std),
        serde_json::to_string(&dir.to_string_lossy()).expect("a string")
    );
    let write = || -> std::io::Result<()> {
        std::fs::create_dir_all(&song)?;
        std::fs::write(song.join("music2.toml"), manifest)?;
        std::fs::write(song.join("song.ts"), code)
    };
    let mut out = vec![];
    if let Err(e) = write() {
        return vec![format!("a scratch song to build the pack in couldn't be made: {e}")];
    }
    let mut set = m2_runtime::PackageSet::default();
    let built = match set.load(std) {
        Err(e) => Err(vec![format!("{e:#}")]),
        Ok(_) => m2_song::Project::open(&song)
            .map_err(|d| d.into_iter().map(|d| d.message).collect::<Vec<_>>())
            .and_then(|project| m2_session::compile(&project, &mut set).map_err(|d| d.into_iter().map(|d| d.message).collect())),
    };
    match built {
        Ok(c) => out.extend(c.problems.iter().filter(|d| d.severity == m2_song::Severity::Error).map(|d| d.message.clone())),
        Err(e) => out.extend(e.into_iter().map(|m| format!("its components don't build in a song: {m}"))),
    }
    let _ = std::fs::remove_dir_all(&song);
    out
}

/// The requirement that admits the std in `std`.
fn std_requirement(std: &Path) -> String {
    let v = pack::PkgManifest::at(std).map(|m| m.package.version).unwrap_or(Version(0, 0, 0));
    Requirement::minor(v).to_string()
}

/// Write `m2pack.json` into the pack in `dir` (`m2 pack build`, PACK-6): its listing, and the
/// version's hash.
pub fn build(dir: &Path) -> std::io::Result<(Listing, Sha)> {
    let listing = Listing::of(dir)?;
    std::fs::write(dir.join(pack::LISTING), listing.text())?;
    let hash = listing.hash();
    Ok((listing, hash))
}

/// What an install did.
#[derive(Clone, Debug, PartialEq)]
pub enum Install {
    /// Copied into the store, at its folder, with its hash.
    Installed { name: PackName, version: Version, dir: PathBuf, hash: Sha },
    /// That version was there with this very hash and its files whole: nothing to do.
    Already { name: PackName, version: Version, hash: Sha },
    /// That version was there with this very hash, but its copy no longer matched its listing
    /// (`why`): the copy was replaced with these files (PACK-6).
    Repaired { name: PackName, version: Version, why: String },
}

/// Check the pack in `dir`, build its listing and copy it into `store`, its files and folders
/// read-only (`m2 pack install`, PACK-6): refused when it doesn't check, or when its version is
/// there with another hash (design P5). With the same hash it does nothing when the installed copy
/// still matches its listing, and replaces the copy when it doesn't. A link is never followed: a
/// pack holding one doesn't check.
pub fn install(dir: &Path, store: &Store, std: &Path) -> Result<Install, Vec<String>> {
    let problems = check(dir, std);
    if !problems.is_empty() {
        return Err(problems);
    }
    let io = |e: std::io::Error| vec![e.to_string()];
    let m = pack::PkgManifest::at(dir).map_err(|e| vec![e])?;
    let (name, version) = (m.package.name, m.package.version);
    let (listing, hash) = build(dir).map_err(io)?;
    let target = store.version_dir(&name, &version);
    let parent = target.parent().expect("a version's folder is in its pack's");
    // The changed copy an install of this version that stopped left aside goes, so no build takes
    // the version for one being installed again (PACK-6).
    for (_, old) in store.aside(&name).into_iter().filter(|(v, _)| *v == version) {
        let _ = remove_tree(&old);
    }
    let mut repair = None;
    if std::fs::symlink_metadata(&target).is_ok() {
        match store.hash(&name, &version) {
            Ok(h) if h == hash => match store.check(&name, &version) {
                Ok(_) => return Ok(Install::Already { name, version, hash }),
                Err(why) => repair = Some(why),
            },
            _ => {
                return Err(vec![format!(
                    "{name} {version} is installed already with other files. A version is never changed (P5): give this one a \
                     new version in m2pkg.toml, or remove the installed one first with `m2 pack remove {name} {version}`"
                )]);
            }
        }
    }
    std::fs::create_dir_all(parent).map_err(io)?;
    let temp = parent.join(format!(".{version}.{}.tmp", std::process::id()));
    let _ = remove_tree(&temp);
    let copied = (|| -> std::io::Result<()> {
        for f in &listing.files {
            let (from, to) = (dir.join(&f.path), temp.join(&f.path));
            // Listed with no link, and copied only as a plain file, never through one.
            if !std::fs::symlink_metadata(&from)?.file_type().is_file() {
                return Err(std::io::Error::other(format!("{} is no longer a plain file: install it again", f.path)));
            }
            std::fs::create_dir_all(to.parent().expect("inside the pack"))?;
            std::fs::copy(&from, &to)?;
        }
        std::fs::write(temp.join(pack::LISTING), listing.text())?;
        // What is copied must be what was listed.
        if Listing::of(&temp)? != listing {
            return Err(std::io::Error::other("the pack's files changed while it was copied: install it again"));
        }
        read_only_inside(&temp)?;
        match repair {
            Some(_) => {
                swap(&temp, &target, &parent.join(format!(".{version}.{}.old", std::process::id())), &mut |a, b| std::fs::rename(a, b))?
            }
            None => std::fs::rename(&temp, &target)?,
        }
        set_read_only(&target, true)
    })();
    if let Err(e) = copied {
        let _ = remove_tree(&temp);
        return Err(io(e));
    }
    Ok(match repair {
        Some(why) => Install::Repaired { name, version, why },
        None => Install::Installed { name, version, dir: target, hash },
    })
}

/// Put the copy in `temp` in place of the installed copy at `target` (a repair, PACK-6), through
/// `rename` (`std::fs::rename` but in tests). The changed copy is moved aside to `old` first, so
/// the version is never half one and half the other, and removed once the new one is in. Should
/// the new one not go in, the changed one is put back as it was, read-only: a failed repair
/// leaves the version installed, never missing.
fn swap(temp: &Path, target: &Path, old: &Path, rename: &mut dyn FnMut(&Path, &Path) -> std::io::Result<()>) -> std::io::Result<()> {
    // A folder is renamed while it can be written, on every OS, and made read-only after.
    set_read_only(target, false)?;
    if let Err(e) = rename(target, old) {
        let _ = set_read_only(target, true);
        return Err(e);
    }
    if let Err(e) = rename(temp, target) {
        return match rename(old, target).and_then(|()| set_read_only(target, true)) {
            Ok(()) => Err(e),
            Err(back) => {
                Err(std::io::Error::other(format!("{e}; and the copy before couldn't be put back ({back}): it is at {}", old.display())))
            }
        };
    }
    remove_tree(old)
}

/// Make every file and folder in `dir` read-only (PACK-6): its folders last, deepest first, so
/// nothing can be added to, removed from or renamed in an installed version.
fn read_only_inside(dir: &Path) -> std::io::Result<()> {
    let mut folders = vec![];
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d)? {
            let e = e?;
            let kind = e.file_type()?;
            if kind.is_dir() {
                folders.push(e.path());
                stack.push(e.path());
            } else if kind.is_file() {
                set_read_only(&e.path(), true)?;
            }
        }
    }
    for d in folders.iter().rev() {
        set_read_only(d, true)?;
    }
    Ok(())
}

fn set_read_only(path: &Path, on: bool) -> std::io::Result<()> {
    let mut perm = std::fs::symlink_metadata(path)?.permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perm.set_readonly(on);
    std::fs::set_permissions(path, perm)
}

/// Take version `version` of `name` out of `store` (`m2 pack remove`). A song that locks it then
/// says `pkg.missing`: the store can't know every song.
pub fn remove(store: &Store, name: &PackName, version: &Version) -> std::io::Result<()> {
    let dir = store.version_dir(name, version);
    if !dir.join(pack::LISTING).is_file() {
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, format!("{name} {version} isn't installed")));
    }
    remove_tree(&dir)?;
    // The pack's folder, and its owner's, go when nothing is left in them.
    let pack = dir.parent().expect("a version is in its pack's folder");
    if std::fs::remove_dir(pack).is_ok() {
        let _ = std::fs::remove_dir(pack.parent().expect("a pack is in its owner's folder"));
    }
    Ok(())
}

/// Remove `dir` and everything in it, read-only files and folders included. A link is removed,
/// never followed.
fn remove_tree(dir: &Path) -> std::io::Result<()> {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        if !std::fs::symlink_metadata(&d).is_ok_and(|m| m.is_dir()) {
            continue;
        }
        let _ = set_read_only(&d, false);
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push(e.path()),
                Ok(t) if t.is_file() => drop(set_read_only(&e.path(), false)),
                _ => {}
            }
        }
    }
    std::fs::remove_dir_all(dir)
}

/// A new pack in `folder` named `name` (`m2 pack new`): `m2pkg.toml`, `src/index.ts` with one
/// instrument on std's synth, an empty `presets/`, a README and no licence yet. Refused when the
/// folder is there.
pub fn new_pack(folder: &Path, name: &PackName, std: &Path) -> Result<(), String> {
    if name.is_std() {
        return Err("a pack is named @owner/name".into());
    }
    if folder.exists() {
        return Err(format!("{} exists already: pick a new folder", folder.display()));
    }
    let short = name.as_str().rsplit('/').next().unwrap_or("pack");
    let binding = crate::sites::camel(short);
    let label = short.replace('-', " ");
    let manifest = format!(
        "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nlicense = \"CC0-1.0\"\ndescription = \"\"\nauthors = []\ntags = []\n\n\
         [dependencies]\n\"@m2/core\" = \"1\"\nstd = \"{}\"\n",
        std_requirement(std)
    );
    let index = format!(
        "import {{ component, control, port, signature, type Audio, type Events, type Params }} from \"@m2/core\";\n\
         import {{ synth }} from \"std/synth\";\n\n\
         /** An instrument: notes in, sound out. Its id is {name}:{short}. */\n\
         export const {binding} = component(\n  {{\n    id: \"{short}\",\n    label: \"{label}\",\n    \
         signature: signature<(notes: Events) => Audio>(),\n    inputs: {{ notes: port.notes() }},\n    \
         controls: {{ brightness: control.hz(2400).range(200, 20000) }},\n    output: port.audio(),\n    \
         editor: {{ category: \"Keys\", description: \"A pluck, from std's synth\" }},\n  }},\n  \
         (notes: Events, {{ brightness }}: Params): Audio => synth.pluck(notes, {{ brightness }}),\n);\n"
    );
    let readme = format!(
        "# {name}\n\n**In short:** a music2 pack. `m2 pack check .` checks it, and `m2 pack install .` puts it where songs find it.\n\n\
         - `src/index.ts`: its instruments and effects, each a `component(…)`.\n\
         - `presets/<component>/<name>.json`: its sounds.\n\
         - `samples/`: recordings its code plays, named `file(import.meta, \"samples/…\")`.\n"
    );
    let made = (|| -> std::io::Result<()> {
        std::fs::create_dir_all(folder.join("src"))?;
        std::fs::create_dir_all(folder.join("presets"))?;
        std::fs::write(folder.join(pack::PKG_MANIFEST), manifest)?;
        std::fs::write(folder.join("src/index.ts"), index)?;
        std::fs::write(folder.join("README.md"), readme)
    })();
    made.map_err(|e| format!("{} couldn't be made: {e}", folder.display()))
}

#[cfg(test)]
#[path = "packs_tests.rs"]
mod tests;
