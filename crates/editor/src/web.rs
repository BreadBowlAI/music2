//! The web bundle (WEB-1 to WEB-6): what `m2 export <song> --web <folder>` writes so a web page
//! can play the song: the compiled graph, the song's data files as they are, the samples it plays
//! and the kernels of the packages it plays, with a manifest of sizes and hashes, `m2web.json`. No
//! song code: the page runs none (the web player note, `docs/designs/live/web-player.md`).
//!
//! [`plan`] reads the accepted revision and refuses what a page can't play, in words, before
//! anything is written; [`Plan::write`] copies the files into a new, empty folder, hashing each
//! as it goes, and writes the manifest last, so a folder without one is no bundle.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use m2_ir::{Ir, Node, Op};
use m2_runtime::Package;
use m2_session::{Revision, Session};
use m2_song::pack::{self, PackName};
use m2_song::{DataPath, Severity, Sha};
use serde_json::{Value, json};

/// The bundle's format: `@music2/web` reads this one, and refuses another in words.
pub const FORMAT: u32 = 1;
/// The manifest's name, at the bundle's top.
pub const MANIFEST: &str = "m2web.json";

/// The words for a folder export won't write into (WEB-5).
pub const NOT_EMPTY: &str = "Export to a new, empty folder outside the song.";

/// A sample the bundle carries, and where its note says it came from (WEB-6).
#[derive(Clone, Debug, PartialEq)]
pub struct Credit {
    /// Its path in the bundle.
    pub path: String,
    /// Its note's `from`: a pack's name, version, file, author and licence, or `{"folder": true}`
    /// for one of yours. None when it has no note, or its note says no `from` (a bounce).
    pub from: Option<Value>,
}

impl Credit {
    /// Its licence in words, for the warning: the pack, its version, author and licence, or "no
    /// licence on record" when it credits no pack (one of yours).
    pub fn words(&self) -> String {
        let Some(pack) = self.from.as_ref().and_then(|f| f.get("pack")).and_then(Value::as_str) else {
            return "no licence on record (your own)".into();
        };
        let from = self.from.as_ref().expect("it names a pack");
        let field = |k: &str| from.get(k).and_then(Value::as_str);
        let mut words = pack.to_string();
        if let Some(v) = field("version") {
            words.push_str(&format!(" {v}"));
        }
        if let Some(a) = field("author") {
            words.push_str(&format!(", by {a}"));
        }
        words.push_str(&match field("license") {
            Some(l) => format!(", licence {l}"),
            None => ", no licence on record".into(),
        });
        words
    }
}

/// Where a file of the bundle comes from.
enum Source {
    /// Bytes made here: the IR, the controls, a lane file's text as the revision read it.
    Bytes(Vec<u8>),
    /// A file of the song or of a package, and the SHA-256 the revision read it with, when it
    /// knows one: a file that changed since is refused, never carried.
    File(PathBuf, Option<Sha>),
}

/// A bundle planned on one revision: every file but the manifest by its path in the bundle, and
/// what the manifest says besides.
pub struct Plan {
    files: BTreeMap<String, Source>,
    /// The manifest without its `files`, which [`Plan::write`] adds as it copies them.
    head: serde_json::Map<String, Value>,
    pub credits: Vec<Credit>,
}

/// What [`Plan::write`] wrote.
#[derive(Debug)]
pub struct Written {
    /// Files, the manifest one of them, and their bytes.
    pub files: usize,
    pub bytes: u64,
    /// The manifest's SHA-256 as a page's `integrity` takes it: `sha256-<base64>`.
    pub integrity: String,
}

/// Plan the bundle of `s`'s accepted revision, or refuse it in words (WEB-4, WEB-5): a plugin, a
/// kernel of a pack that isn't music2's own, a std that isn't as music2 ships it, a problem that
/// is an error. Nothing is written.
pub fn plan(s: &Session) -> Result<Plan> {
    let rev = s.accepted();
    if !rev.has_code() {
        bail!("The song's code doesn't build: `m2 check` says why");
    }
    let c = &rev.compiled;
    refuse_plugins(rev, s)?;
    let errors: Vec<&m2_song::Diagnostic> = c.problems.iter().filter(|d| d.severity == Severity::Error).collect();
    if !errors.is_empty() {
        let each: Vec<String> = errors.iter().map(|d| format!("  {d}")).collect();
        bail!(
            "The song has {} a web page would play otherwise than the app, as `m2 check` says:\n{}\nFix {} first, then export again.",
            if errors.len() == 1 { "a problem" } else { "problems" },
            each.join("\n"),
            if errors.len() == 1 { "it" } else { "them" }
        );
    }

    let mut files: BTreeMap<String, Source> = BTreeMap::new();
    let mut ir = serde_json::to_vec(&c.ir)?;
    ir.push(b'\n');
    files.insert("ir.json".into(), Source::Bytes(ir));
    files.insert("controls.json".into(), Source::Bytes(controls(s)?));
    let dir = &rev.project.dir;
    for name in [m2_song::path::MANIFEST, m2_song::path::LOCK] {
        if dir.join(name).is_file() {
            files.insert(name.into(), Source::File(dir.join(name), None));
        }
    }
    let arrangement = DataPath::Arrangement.to_string();
    if dir.join(&arrangement).is_file() {
        files.insert(arrangement.clone(), Source::File(dir.join(&arrangement), None));
    }
    // Every lane file, read by a lane or not: the song's length counts them all (DataSet::end).
    for (addr, _, text) in rev.project.lane_files() {
        files.insert(DataPath::lane(addr).to_string(), Source::Bytes(text.as_bytes().to_vec()));
    }

    // The packages whose kernels the IR names: their code runs in the page (WEB-4).
    let view = &c.view;
    let mut kernels: BTreeSet<String> = BTreeSet::new();
    each_node(&c.ir, &mut |_, n| {
        if let Op::Kernel { kernel, .. } = &n.op
            && let Some((package, _)) = pack::split_id(kernel)
        {
            kernels.insert(package.to_string());
        }
    });
    let mut packs: BTreeMap<String, Value> = BTreeMap::new();
    for name in &kernels {
        let p = view.get(name).ok_or_else(|| anyhow!("The song plays {name}, which its build didn't load"))?;
        let at = folder(p);
        let (listing, hash) = trusted(p, &rev.project.lock)?;
        let wasm = pack::PkgManifest::at(&p.dir).map_err(|e| anyhow!("{name}: {e}"))?.package.wasm;
        let read = |file: &str| p.files.get(&format!("{name}/{file}")).and_then(|hex| Sha::parse(hex));
        files.insert(format!("{at}/{}", pack::PKG_MANIFEST), Source::File(p.dir.join(pack::PKG_MANIFEST), read(pack::PKG_MANIFEST)));
        if let Some(w) = wasm {
            files.insert(format!("{at}/{w}"), Source::File(p.dir.join(&w), read(&w)));
        }
        files.insert(format!("{at}/{}", pack::LISTING), listing);
        packs.insert(name.clone(), package_entry(p, Some(hash)));
    }

    // The samples: each asset a kernel reads or a clip plays, and each package file a kernel reads.
    let data = &c.data;
    let mut assets: BTreeMap<String, Option<Sha>> = data.kernel_assets().iter().map(|(n, sha)| (n.clone(), Some(*sha))).collect();
    for (_, lane) in data.lanes() {
        if let m2_song::LaneData::Audio(clips) = &**lane {
            for clip in clips {
                assets.entry(clip.asset.clone()).or_insert(None);
            }
        }
    }
    let mut credits = vec![];
    for (name, sha) in &assets {
        let Some(path) = data.asset_path(name) else { continue };
        let (at, from) = match pack::AssetRef::parse(name) {
            Ok(pack::AssetRef::Package { package, path: file }) => {
                let p = view.get(package.as_str()).ok_or_else(|| anyhow!("The song reads {name}, from a package its build didn't load"))?;
                if !packs.contains_key(package.as_str()) {
                    packs.insert(package.as_str().to_string(), package_entry(p, locked(&rev.project.lock, &package)));
                }
                let mut from = crate::library::samples::package_from(package.as_str(), &p.dir);
                from["file"] = json!(file);
                (format!("{}/{file}", folder(p)), Some(from))
            }
            _ => {
                let at = format!("{}/{name}", m2_song::path::ASSETS);
                let note = format!("{at}.json");
                let from = match std::fs::read(dir.join(&note)) {
                    Ok(bytes) => {
                        let read: Option<Value> = serde_json::from_slice(&bytes).ok();
                        files.insert(note, Source::Bytes(bytes));
                        read.and_then(|n| n.get("from").cloned())
                    }
                    Err(_) => None,
                };
                (at, from)
            }
        };
        files.insert(at.clone(), Source::File(path, *sha));
        credits.push(Credit { path: at, from });
    }

    let mut head = serde_json::Map::new();
    head.insert("format".into(), json!(FORMAT));
    head.insert("music2".into(), json!(env!("CARGO_PKG_VERSION")));
    head.insert("ir".into(), json!(c.ir.ir));
    head.insert("abi".into(), json!(m2_runtime::packages::KERNEL_ABI));
    let seconds = data.tempo().seconds_at(data.end());
    head.insert(
        "song".into(),
        json!({"name": rev.project.manifest.project.name, "sample_rate": rev.project.sample_rate(), "seconds": seconds}),
    );
    head.insert("tracks".into(), tracks(&c.ir));
    head.insert("packs".into(), Value::Array(packs.into_values().collect()));
    let credited: Vec<Value> = credits.iter().map(|c| json!({"path": c.path, "from": c.from})).collect();
    head.insert("credits".into(), Value::Array(credited));
    Ok(Plan { files, head, credits })
}

/// Refuse a song with a node that hosts a CLAP plugin, naming each and where it is (WEB-5).
fn refuse_plugins(rev: &Revision, s: &Session) -> Result<()> {
    let ir = &rev.compiled.ir;
    let mut found: Vec<(String, String)> = vec![];
    each_node(ir, &mut |m, n| {
        if let Op::Plugin { plugin, .. } = &n.op {
            let id = || plugin.strip_prefix("clap:").unwrap_or(plugin).to_string();
            let name = s.packages.plugin_info(plugin).map_or_else(id, |i| i.found.name.clone());
            let place = match m.track {
                true => format!("on the track {}", m.label()),
                false => "in the song".to_string(),
            };
            if !found.contains(&(name.clone(), place.clone())) {
                found.push((name, place));
            }
        }
    });
    if found.is_empty() {
        return Ok(());
    }
    // By place: "Surge XT and Consolidated, on the track Lead; Vital, in the song". Bounce to audio
    // keeps the track, muted in the song (RENDER-40, MON-5), and a muted track is still built, its
    // plugins with it: so the track goes too, in a copy.
    let mut places: Vec<(String, Vec<String>)> = vec![];
    for (name, place) in found.iter().cloned() {
        match places.iter_mut().find(|(p, _)| *p == place) {
            Some((_, names)) => names.push(name),
            None => places.push((place, vec![name])),
        }
    }
    let said: Vec<String> = places.iter().map(|(place, names)| format!("{}, {place}", and(names))).collect();
    let tracks = places.iter().filter(|(p, _)| p.starts_with("on the track")).count();
    bail!(
        "This song plays {} ({}). A web page can't load plugins. In a copy of the song (Save As), bounce {} to audio (Bounce to audio) and delete {} with {}, then export the copy.",
        if found.len() == 1 { "a plugin" } else { "plugins" },
        said.join("; "),
        if tracks > 1 { "those tracks" } else { "the track" },
        if tracks > 1 { "the tracks" } else { "the track" },
        if found.len() == 1 { "the plugin" } else { "the plugins" }
    )
}

/// `a`, `a and b`, `a, b and c`.
fn and(names: &[String]) -> String {
    match names {
        [rest @ .., last] if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => names.concat(),
    }
}

/// Every node of `ir`, with its module, inside `poly` bodies too.
fn each_node(ir: &Ir, f: &mut impl FnMut(&m2_ir::Module, &Node)) {
    fn walk(m: &m2_ir::Module, ns: &[Node], f: &mut impl FnMut(&m2_ir::Module, &Node)) {
        for n in ns {
            f(m, n);
            if let Op::Poly { body, .. } = &n.op {
                walk(m, &body.nodes, f);
            }
        }
    }
    for m in &ir.modules {
        walk(m, &m.nodes, f);
    }
}

/// A package's folder in the bundle: `packs/<name>/<version>`.
fn folder(p: &Package) -> String {
    format!("packs/{}/{}", p.name, p.version)
}

/// The lock's hash of pack `name`.
fn locked(lock: &pack::Lock, name: &PackName) -> Option<Sha> {
    lock.get(name).map(|l| l.hash)
}

/// The manifest's entry for package `p` (WEB-2): informative only.
fn package_entry(p: &Package, hash: Option<Sha>) -> Value {
    json!({"name": p.name, "version": p.version, "hash": hash, "license": p.license})
}

/// The listing a page decides package `p`'s trust by, and its hash, when its module runs as std's
/// (WEB-4, ABI-11); else the refusal, in words. std's is made from its files (PACK-17a), and must
/// be one the record holds; a pack's is its installed listing, and `packages::checked` must not
/// check it.
fn trusted(p: &Package, lock: &pack::Lock) -> Result<(Source, Sha)> {
    if p.name == pack::STD {
        let listing = pack::std_listing(&p.dir).map_err(|e| anyhow!("std: {e}"))?;
        let hash = listing.hash();
        let version = m2_song::Version::parse(&p.version).ok_or_else(|| anyhow!("std's version {} doesn't read", p.version))?;
        if !pack::own(&PackName::new(pack::STD).expect("std is a name"), &version, &hash) {
            bail!(
                "The std this music2 plays (std {version}) isn't std as music2 ships it: its m2pkg.toml or its kernels changed. A web \
                 page trusts only std as music2 ships it, by its hash. Export with the std that came with music2."
            );
        }
        // The listing's text, made here, must be the one hashed: what the record holds.
        return Ok((Source::Bytes(listing.text().into_bytes()), hash));
    }
    if p.checked {
        bail!(
            "This song plays {}, which isn't one of music2's own packs. A web bundle can hold only std and music2's own packs until \
             the web has a time limit for other packs' code.",
            p.name
        );
    }
    let listing = p.dir.join(pack::LISTING);
    let hash = Sha::of_file(&listing).with_context(|| format!("reading {}", listing.display()))?;
    let name = PackName::new(&p.name).map_err(|e| anyhow!("{e}"))?;
    if locked(lock, &name).is_some_and(|l| l != hash) {
        bail!("{} isn't the version the song's lock names: build the song again (`m2 check`), then export", p.name);
    }
    Ok((Source::File(listing, Some(hash)), hash))
}

/// The manifest's `tracks` (WEB-2): each track's key, label and lanes, and the notes lane a feed
/// opens on when it isn't told one (FEED-1): null when it has none or several.
fn tracks(ir: &Ir) -> Value {
    let tracks = ir.modules.iter().filter(|m| m.track).map(|m| {
        let lanes: Vec<Value> =
            m.lanes.iter().map(|l| json!({"key": l.key.as_str(), "kind": kind(&m2_runtime::lane_kind(l.kind))})).collect();
        let notes: Vec<&str> =
            m.lanes.iter().filter(|l| m2_runtime::lane_kind(l.kind) == m2_song::LaneKind::Notes).map(|l| l.key.as_str()).collect();
        let feed = match notes[..] {
            [one] => json!(one),
            _ => Value::Null,
        };
        json!({"key": m.key, "label": m.label(), "lanes": lanes, "feed": feed})
    });
    Value::Array(tracks.collect())
}

/// A lane's kind in a word.
fn kind(k: &m2_song::LaneKind) -> &'static str {
    match k {
        m2_song::LaneKind::Notes => "notes",
        m2_song::LaneKind::Control { .. } => "control",
        m2_song::LaneKind::Choice { .. } => "choice",
        m2_song::LaneKind::Switch => "switch",
        m2_song::LaneKind::Audio => "audio",
    }
}

/// `controls.json` (WEB-3): every control `m2 controls` lists, with where it is in the IR.
fn controls(s: &Session) -> Result<Vec<u8>> {
    let listed: Vec<Value> = crate::controls::controls(s)
        .into_iter()
        .map(|c| {
            let declared = c.declared.first();
            json!({
                "id": c.id,
                "label": declared.map(|d| d.label.clone()),
                "unit": c.unit.map(|u| u.name()),
                "value": c.value,
                "range": declared.and_then(|d| d.range).map(|(lo, hi)| [lo, hi]),
                "log": declared.is_some_and(|d| d.log),
                "default": declared.and_then(|d| d.default),
                "origin": c.origin,
            })
        })
        .collect();
    let mut out = serde_json::to_vec_pretty(&json!({ "controls": listed }))?;
    out.push(b'\n');
    Ok(out)
}

impl Plan {
    /// The paths of the files it writes, the manifest last.
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.files.keys().map(String::as_str).chain([MANIFEST])
    }

    /// Write the bundle into `to`, a new or empty folder outside the song in `song` (WEB-5), else
    /// refused before anything is written. Each file is copied create-only, hashed as it goes; a
    /// file whose bytes aren't the ones the revision read is refused, and everything written so
    /// far is taken away again. The manifest goes last.
    pub fn write(self, song: &Path, to: &Path) -> Result<Written> {
        let created = fresh(song, to)?;
        let written = self.write_into(to);
        if written.is_err() {
            match created {
                true => drop(std::fs::remove_dir_all(to)),
                false => {
                    for e in std::fs::read_dir(to).into_iter().flatten().flatten() {
                        let _ = if e.path().is_dir() { std::fs::remove_dir_all(e.path()) } else { std::fs::remove_file(e.path()) };
                    }
                }
            }
        }
        written
    }

    fn write_into(self, to: &Path) -> Result<Written> {
        let mut listed = vec![];
        let mut bytes = 0;
        for (path, source) in &self.files {
            let dest = to.join(path);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).with_context(|| format!("making {}", parent.display()))?;
            }
            let mut out =
                std::fs::OpenOptions::new().write(true).create_new(true).open(&dest).with_context(|| format!("writing {path}"))?;
            let (size, sha) = match source {
                Source::Bytes(b) => {
                    out.write_all(b).with_context(|| format!("writing {path}"))?;
                    (b.len() as u64, Sha::of(b))
                }
                Source::File(from, expect) => {
                    let file = std::fs::File::open(from).with_context(|| format!("reading {}", from.display()))?;
                    let mut reading = Sha::reading(file);
                    let size = std::io::copy(&mut reading, &mut out).with_context(|| format!("copying {}", from.display()))?;
                    let sha = reading.sha();
                    if expect.is_some_and(|e| e != sha) {
                        bail!("{path} changed while it was exported: export again");
                    }
                    (size, sha)
                }
            };
            out.flush()?;
            bytes += size;
            listed.push(json!({"path": path, "size": size, "sha256": sha}));
        }
        let files = listed.len() + 1;
        let Plan { mut head, .. } = self;
        // In the order the contract says them: files before credits.
        let credits = head.remove("credits").unwrap_or(Value::Array(vec![]));
        head.insert("files".into(), Value::Array(listed));
        head.insert("credits".into(), credits);
        let mut manifest = serde_json::to_vec_pretty(&Value::Object(head))?;
        manifest.push(b'\n');
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(to.join(MANIFEST))
            .and_then(|mut f| f.write_all(&manifest).and_then(|()| f.flush()))
            .with_context(|| format!("writing {MANIFEST}"))?;
        Ok(Written { files, bytes: bytes + manifest.len() as u64, integrity: integrity(&manifest) })
    }
}

/// Make sure `to` is a new or empty folder outside the song in `song` (WEB-5), making it when it
/// isn't there: whether it was made here.
fn fresh(song: &Path, to: &Path) -> Result<bool> {
    let song = song.canonicalize().with_context(|| format!("reading {}", song.display()))?;
    // Where `to` is, though it isn't there yet: its nearest folder that is, and the rest.
    let mut there = to.to_path_buf();
    let mut rest = vec![];
    let found = loop {
        if let Ok(c) = there.canonicalize() {
            break c;
        }
        match (there.file_name(), there.parent()) {
            (Some(name), Some(parent)) => {
                rest.push(name.to_os_string());
                there = if parent.as_os_str().is_empty() { PathBuf::from(".") } else { parent.to_path_buf() };
            }
            _ => bail!("{} can't be made", to.display()),
        }
    };
    let whole = rest.iter().rev().fold(found, |p, n| p.join(n));
    if whole.starts_with(&song) {
        bail!("{NOT_EMPTY}");
    }
    match std::fs::symlink_metadata(to) {
        Ok(m) if m.is_dir() => {
            if std::fs::read_dir(to)?.next().is_some() {
                bail!("{NOT_EMPTY}");
            }
            Ok(false)
        }
        Ok(_) => bail!("{NOT_EMPTY}"),
        Err(_) => {
            std::fs::create_dir_all(to).with_context(|| format!("making {}", to.display()))?;
            Ok(true)
        }
    }
}

/// A size in words: `363 KB`, `1.8 MB`.
pub fn size(bytes: u64) -> String {
    match bytes {
        b if b < 1000 => format!("{b} bytes"),
        b if b < 1_000_000 => format!("{} KB", (b as f64 / 1000.0).round()),
        b => format!("{:.1} MB", b as f64 / 1e6),
    }
}

/// `bytes`' SHA-256 as a page's `integrity` takes it (Subresource Integrity): `sha256-<base64>`.
pub fn integrity(bytes: &[u8]) -> String {
    let hex = Sha::of(bytes).hex();
    let raw: Vec<u8> = (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex")).collect();
    format!("sha256-{}", base64(&raw))
}

/// Standard base64, padded.
fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            match i <= chunk.len() {
                true => out.push(ABC[((n >> (18 - 6 * i)) & 63) as usize] as char),
                false => out.push('='),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // WEB-2: the integrity a page checks is the SHA-256 in standard base64.
    #[test]
    fn integrity_is_sha256_in_base64() {
        assert_eq!(integrity(b""), "sha256-47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=");
        assert_eq!(integrity(b"abc"), "sha256-ungWv48Bz+pBQUDeXa4iI7ADYaOWF3qctBD/YfIAFa0=");
        assert_eq!(base64(b"ab"), "YWI=");
        assert_eq!(base64(b"a"), "YQ==");
    }
}
