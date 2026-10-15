//! New song projects: a template, or a copy of an example, in a new folder.
//!
//! Songs go in the songs folder (`~/Music/music2`) unless the user picks another. A new
//! folder never replaces anything: a name that is taken gets " 2", " 3"… instead.

use std::collections::{BTreeMap, HashSet};
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::graph::Graph;
use crate::resources;

/// What a song is called when the name given is empty.
pub const DEFAULT_NAME: &str = "My song";
/// The template a new song starts from unless another is asked for.
pub const DEFAULT_TEMPLATE: &str = "starter";
/// Longest song name, in characters: folder names stay readable in Finder.
const MAX_NAME: usize = 80;

/// Where songs are made: `$HOME/Music/music2` (`m2_os::dirs::songs`). From `$HOME`, so tests can
/// point it elsewhere. It is created when a song is first made there ([`make_songs_dir`]).
pub fn songs_dir() -> Result<PathBuf> {
    m2_os::dirs::songs().ok_or_else(|| anyhow!("can't find your home folder ($HOME isn't set)"))
}

/// [`songs_dir`], made if it isn't there, each folder made synced in the one above it, as every
/// folder a new song's files are in (WRITE-1).
pub fn make_songs_dir() -> Result<PathBuf> {
    let songs = songs_dir()?;
    m2_os::fs::make_dirs(&songs).with_context(|| format!("making {}", songs.display()))?;
    Ok(songs)
}

/// A template or an example, as the New Song dialog and the start screen list them.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Starter {
    pub id: String,
    pub name: String,
    pub description: String,
}

/// `template.toml` (or `example.toml`): how the folder is listed. Not copied into the song.
#[derive(Deserialize, Default)]
struct About {
    name: Option<String>,
    #[serde(default)]
    description: String,
    /// Lower comes first; the rest by name.
    #[serde(default = "later")]
    order: i64,
    /// The CLAP plugins an example plays, by id, with the names people know them by
    /// (`"org.surge-synth-team.surge-xt" = "Surge XT"`).
    #[serde(default)]
    plugins: BTreeMap<String, String>,
}

fn later() -> i64 {
    100
}

const TEMPLATE_TOML: &str = "template.toml";
const EXAMPLE_TOML: &str = "example.toml";

/// What a new song can start from, and where it goes by default (`GET /api/project/templates`).
/// `home` lets the UI write the songs folder as `~/Music/music2`.
pub fn catalog() -> Value {
    json!({"templates": templates(), "examples": examples(), "songs": songs_dir().ok(), "home": m2_os::dirs::home()})
}

/// A New Song request (`POST /api/project/new`).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct NewSong {
    /// What to call it; empty is [`DEFAULT_NAME`], or the example's name.
    #[serde(default)]
    pub name: String,
    /// A template's id ([`DEFAULT_TEMPLATE`] if neither this nor `example` is given).
    #[serde(default)]
    pub template: Option<String>,
    /// Copy this example instead of a template.
    #[serde(default)]
    pub example: Option<String>,
    /// The folder to make it in (a full path); the songs folder if not given.
    #[serde(default)]
    pub dir: Option<PathBuf>,
}

/// Make the song `req` asks for and open it, like `/api/graph/open`. Returns the new song's
/// state. An example needing a plugin that isn't installed says so before anything is made.
/// A song that can't be opened isn't kept, and the project open before stays open.
pub fn new_song(graph: &Graph, req: &NewSong) -> Result<Value> {
    let (source, name) = match (&req.example, &req.template) {
        (Some(id), _) => {
            let shown = examples().into_iter().find(|e| &e.id == id).map(|e| e.name);
            (Source::Example(id.clone()), if req.name.trim().is_empty() { shown.unwrap_or_default() } else { req.name.clone() })
        }
        (None, id) => (Source::Template(id.clone().unwrap_or_else(|| DEFAULT_TEMPLATE.into())), req.name.clone()),
    };
    let from = source.dir()?;
    if let Source::Example(_) = source {
        let (found, _) = m2_plugins::scan();
        let plugins = about(&from.join(EXAMPLE_TOML)).map(|a| a.plugins).unwrap_or_default();
        if let Some(why) = missing_plugins(&plugins, |id| found.iter().any(|f| f.id == id)) {
            bail!("{why}");
        }
    }
    let parent = match &req.dir {
        Some(dir) if !dir.is_absolute() => bail!("the folder to make the song in must be a full path, not {}", dir.display()),
        Some(dir) => dir.clone(),
        None => make_songs_dir()?,
    };
    open_new(graph, &parent, &name, &from)
}

/// [`create`] the song and open it. One that doesn't open isn't kept: its folder only holds the copy.
fn open_new(graph: &Graph, parent: &Path, name: &str, from: &Path) -> Result<Value> {
    let dir = create(parent, name, from)?;
    graph.open(&dir).inspect_err(|_| {
        let _ = std::fs::remove_dir_all(&dir);
    })
}

/// Why a song playing `plugins` (id → name) can't be opened, if one isn't `installed`:
/// "it needs Surge XT, which isn't installed. Install it, then try again."
fn missing_plugins(plugins: &BTreeMap<String, String>, installed: impl Fn(&str) -> bool) -> Option<String> {
    let names: Vec<&str> = plugins.iter().filter(|(id, _)| !installed(id)).map(|(_, name)| name.as_str()).collect();
    let (last, rest) = names.split_last()?;
    let list = if rest.is_empty() { last.to_string() } else { format!("{} and {last}", rest.join(", ")) };
    let (s, is, them) = if rest.is_empty() { ("", "isn't", "it") } else { ("s", "aren't", "them") };
    Some(format!("it needs the {list} plugin{s}, which {is} installed. Install {them}, then try again."))
}

/// A `template.toml` or `example.toml`.
fn about(file: &Path) -> Result<About> {
    Ok(toml::from_str(&std::fs::read_to_string(file)?)?)
}

/// The bundled templates.
pub fn templates() -> Vec<Starter> {
    list(&resources::templates_dir(), TEMPLATE_TOML, true)
}

/// The bundled examples.
pub fn examples() -> Vec<Starter> {
    list(&resources::examples_dir(), EXAMPLE_TOML, false)
}

/// The song projects in `dir`. A template must say what it is (`about` file); an example
/// may, else its folder's name is shown (`first-song` → "First song").
fn list(dir: &Path, about: &str, required: bool) -> Vec<Starter> {
    let Ok(entries) = std::fs::read_dir(dir) else { return vec![] };
    let mut found: Vec<(i64, Starter)> = entries
        .flatten()
        .filter_map(|e| {
            let id = e.file_name().to_str()?.to_string();
            let path = e.path();
            if id.starts_with('.') || !path.join("song.ts").is_file() {
                return None;
            }
            let info = match std::fs::read_to_string(path.join(about)) {
                Ok(text) => toml::from_str::<About>(&text).ok()?,
                Err(_) if !required => About { order: later(), ..About::default() },
                Err(_) => return None,
            };
            let name = info.name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| title(&id));
            Some((info.order, Starter { id, name, description: info.description }))
        })
        .collect();
    found.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.name.to_lowercase().cmp(&b.1.name.to_lowercase())));
    found.into_iter().map(|(_, s)| s).collect()
}

/// `first-song` → "First song".
fn title(id: &str) -> String {
    let words = id.replace(['-', '_'], " ");
    let mut chars = words.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// What a new song starts from.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    Template(String),
    Example(String),
}

impl Source {
    /// The folder to copy, if it is one of the bundled templates or examples.
    pub fn dir(&self) -> Result<PathBuf> {
        let (root, id, what) = match self {
            Source::Template(id) => (resources::templates_dir(), id, "template"),
            Source::Example(id) => (resources::examples_dir(), id, "example"),
        };
        // An id is a folder's name, never a path.
        if id.is_empty() || id.starts_with('.') || id.contains(['/', '\\']) {
            bail!("there's no {what} called “{id}”");
        }
        let dir = root.join(id);
        if !dir.join("song.ts").is_file() {
            bail!("there's no {what} called “{id}”");
        }
        Ok(dir)
    }
}

/// `name` without control characters, trimmed, at most [`MAX_NAME`] characters.
fn trimmed(name: &str) -> String {
    let cleaned: String = name.chars().filter(|c| !c.is_control()).collect();
    let short: String = cleaned.trim().chars().take(MAX_NAME).collect();
    short.trim().into()
}

/// What of a [`trimmed`] name a folder can be called: no path separators (`AC/DC` →
/// `AC-DC`), no leading dots (a hidden folder). Empty if nothing is left.
fn folder_part(trimmed: &str) -> String {
    let swapped: String = trimmed.chars().map(|c| if matches!(c, '/' | '\\' | ':') { '-' } else { c }).collect();
    swapped.trim_start_matches(|c: char| c == '.' || c.is_whitespace()).trim_end().into()
}

/// The name a song is shown by: `name` without control characters, trimmed, at most
/// [`MAX_NAME`] characters. A name no folder can have (empty, or only dots) is
/// [`DEFAULT_NAME`], as its folder is.
pub fn clean_name(name: &str) -> String {
    let t = trimmed(name);
    if folder_part(&t).is_empty() { DEFAULT_NAME.into() } else { t }
}

/// A folder name for a song called `name`: no path separators, no leading dots, at most
/// [`MAX_NAME`] characters; [`DEFAULT_NAME`] if nothing is left.
pub fn folder_name(name: &str) -> String {
    match folder_part(&trimmed(name)) {
        s if s.is_empty() => DEFAULT_NAME.into(),
        s => s,
    }
}

/// The songs folder's `Exports` holds exported audio, so no song is made there. Any case:
/// macOS doesn't tell `exports` from `Exports`. Either folder may not be made yet (`m2 new`
/// checks before it makes one), so each is compared as [`resolved`].
fn reserved(songs: Option<&Path>, parent: &Path, folder: &str) -> bool {
    let same = |a: &Path, b: &Path| a == b || matches!((resolved(a), resolved(b)), (Some(x), Some(y)) if x == y);
    folder.to_lowercase() == crate::export::EXPORTS.to_lowercase() && songs.is_some_and(|s| same(s, parent))
}

/// `p` with its deepest folder that is there resolved (links, `..`, a path relative to where m2
/// runs), and the names after it, not made yet, as written. None when it can't be told.
fn resolved(p: &Path) -> Option<PathBuf> {
    let mut rest = vec![];
    let mut at = p;
    loop {
        if let Ok(there) = (if at.as_os_str().is_empty() { Path::new(".") } else { at }).canonicalize() {
            return Some(rest.iter().rev().fold(there, |p, name| p.join(name)));
        }
        rest.push(at.file_name()?);
        at = at.parent()?;
    }
}

/// Make a new song called `name` in `parent`, copied from `from` (a template or an
/// example folder). The folder is `name` made safe, or "<name> 2", "<name> 3"… if that
/// is taken in any capitals; the song's name in music2.toml says the same. Returns the new folder.
pub fn create(parent: &Path, name: &str, from: &Path) -> Result<PathBuf> {
    make(parent, name, from, songs_dir().ok().as_deref())
}

/// [`create`], with `songs` as the songs folder.
fn make(parent: &Path, name: &str, from: &Path, songs: Option<&Path>) -> Result<PathBuf> {
    if !parent.is_dir() {
        bail!("{} isn't a folder", parent.display());
    }
    let (base, shown) = (folder_name(name), clean_name(name));
    let taken = taken(parent)?.unwrap_or_default();
    for n in 1..=999 {
        let (folder, name) = if n == 1 { (base.clone(), shown.clone()) } else { (format!("{base} {n}"), format!("{shown} {n}")) };
        if reserved(songs, parent, &folder) || taken.contains(&folder.to_lowercase()) {
            continue;
        }
        let dir = parent.join(&folder);
        // Creating the folder claims the name: one that appeared meanwhile is never written into.
        match std::fs::create_dir(&dir) {
            Ok(()) => {
                fill(&dir, &name, from)?;
                return Ok(dir);
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e).with_context(|| format!("making {}", dir.display())),
        }
    }
    bail!("{} already has songs called “{shown}” up to “{shown} 999”", parent.display())
}

/// The names in `parent`, in lower case. A name is taken in any capitals, on every OS (as macOS
/// has it), so songs stay safe to copy between computers (PLAT-7): "exports 2" beside "Exports 2"
/// is taken. None when `parent` can be written but not listed: then only the exact name is checked,
/// by the create-only `create_dir`.
fn taken(parent: &Path) -> Result<Option<HashSet<String>>> {
    let entries = match std::fs::read_dir(parent) {
        Err(e) if e.kind() == ErrorKind::PermissionDenied => return Ok(None),
        entries => entries.with_context(|| format!("reading {}", parent.display()))?,
    };
    let mut taken = HashSet::new();
    for e in entries {
        taken.insert(e?.file_name().to_string_lossy().to_lowercase());
    }
    Ok(Some(taken))
}

/// `dir` with each `..` that follows a folder not there yet taken out with that folder
/// (`NIGHT/../night` is `night` while there is no `NIGHT`): such a folder would only be made to be
/// left behind. A `..` after a folder that is there stays, since that folder may be a link.
fn without_new_parents(dir: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in dir.components() {
        if c == Component::ParentDir && matches!(out.components().next_back(), Some(Component::Normal(_))) && !out.exists() {
            out.pop();
        } else {
            out.push(c);
        }
    }
    out
}

/// Make a new song at exactly `dir`, named after its folder (`m2 new`). Refuses if
/// something is there already, in any capitals ([`taken`]), as macOS does. Every check comes
/// before any folder is made, so a refusal leaves nothing behind.
pub fn create_at(dir: &Path, from: &Path) -> Result<PathBuf> {
    let dir = &without_new_parents(dir);
    let name = dir.file_name().and_then(|n| n.to_str()).ok_or_else(|| anyhow!("{} needs a folder name", dir.display()))?.to_string();
    let parent = dir.parent().filter(|p| !p.as_os_str().is_empty());
    if let Some(parent) = parent
        && reserved(songs_dir().ok().as_deref(), parent, &name)
    {
        bail!("{} is where music2 puts exported audio: give the song another name", dir.display());
    }
    // A folder not made yet holds nothing.
    let above = m2_os::fs::folder_of(dir);
    if above.is_dir() && taken(above)?.is_some_and(|t| t.contains(&name.to_lowercase())) {
        bail!("{} already exists", dir.display());
    }
    if let Some(parent) = parent {
        // Each folder made for it is synced in the one above, as the song's own folder is in
        // `fill` (WRITE-1).
        m2_os::fs::make_dirs(parent).with_context(|| format!("making {}", parent.display()))?;
    }
    match std::fs::create_dir(dir) {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::AlreadyExists => bail!("{} already exists", dir.display()),
        Err(e) => return Err(e).with_context(|| format!("making {}", dir.display())),
    }
    fill(dir, &clean_name(&name), from)?;
    Ok(dir.to_path_buf())
}

/// Copy `from` into the new, empty `dir` and name the song. On failure `dir` is removed
/// again: it only holds what was copied. Every file is pushed and every folder synced, `dir`'s
/// own entry last, so the song's first Tx makes them durable by its barrier at the latest
/// (WRITE-1).
fn fill(dir: &Path, name: &str, from: &Path) -> Result<()> {
    let result = copy_tree(from, dir, true)
        .and_then(|()| set_name(dir, name))
        .and_then(|()| ignore_local(dir))
        .and_then(|()| push_tree(dir))
        .and_then(|()| {
            let parent = m2_os::fs::folder_of(dir);
            m2_os::fs::sync_dir(parent).with_context(|| format!("syncing {}", parent.display()))
        });
    if result.is_err() {
        let _ = std::fs::remove_dir_all(dir);
    }
    result.with_context(|| format!("making {}", dir.display()))
}

/// Push every file under `dir`, then sync each folder, so each of their names and bytes reaches
/// the disk.
fn push_tree(dir: &Path) -> Result<()> {
    for e in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let e = e?;
        if e.file_type()?.is_dir() {
            push_tree(&e.path())?;
        } else {
            let file = std::fs::File::open(e.path()).with_context(|| format!("reading {}", e.path().display()))?;
            m2_os::fs::push(&file).with_context(|| format!("writing {}", e.path().display()))?;
        }
    }
    m2_os::fs::sync_dir(dir).with_context(|| format!("syncing {}", dir.display()))
}

/// Copy a song folder's files: not hidden ones (`.DS_Store`), links, or at the top the
/// template's own `template.toml`.
fn copy_tree(from: &Path, to: &Path, top: bool) -> Result<()> {
    for e in std::fs::read_dir(from).with_context(|| format!("reading {}", from.display()))? {
        let e = e?;
        let name = e.file_name();
        let text = name.to_string_lossy();
        if text.starts_with('.') || (top && (text == TEMPLATE_TOML || text == EXAMPLE_TOML)) {
            continue;
        }
        let kind = e.file_type()?;
        let (src, dst) = (e.path(), to.join(&name));
        if kind.is_dir() {
            std::fs::create_dir(&dst).with_context(|| format!("making {}", dst.display()))?;
            copy_tree(&src, &dst, false)?;
        } else if kind.is_file() {
            std::fs::copy(&src, &dst).with_context(|| format!("copying {}", src.display()))?;
            // The copy is the user's to change, even if what it was copied from (an installed app)
            // is read-only.
            m2_os::fs::make_writable(&dst).with_context(|| format!("making {} writable", dst.display()))?;
        }
    }
    Ok(())
}

/// `.music2/` is this machine's, never the song's: a new project keeps it out of git (FMT-20).
fn ignore_local(dir: &Path) -> Result<()> {
    std::fs::write(dir.join(".gitignore"), format!("{}/\n", m2_song::path::LOCAL)).context("writing .gitignore")
}

/// Write the song's name into the copy's `music2.toml`, a format 1 manifest: only the `name`
/// line of `[project]` changes, so the rest (`format`, `sample_rate`, `[dependencies]`, comments)
/// stays as the template wrote it.
fn set_name(dir: &Path, name: &str) -> Result<()> {
    let path = dir.join(m2_song::path::MANIFEST);
    let text = std::fs::read_to_string(&path).context("reading music2.toml")?;
    let line = format!("name = {}\n", toml::Value::String(name.into()));
    let (mut out, mut table, mut done) = (String::new(), None::<String>, false);
    for l in text.split_inclusive('\n') {
        let t = l.split('#').next().unwrap_or("").trim();
        if t.starts_with('[') {
            // Leaving [project] without a name: it gets one at its end.
            if table.as_deref() == Some("[project]") && !done {
                out += &line;
                done = true;
            }
            table = Some(t.to_string());
        } else if table.as_deref() == Some("[project]") && !done && t.split('=').next().map(str::trim) == Some("name") {
            out += &line;
            done = true;
            continue;
        }
        out += l;
    }
    if table.as_deref() == Some("[project]") && !done {
        out += if out.ends_with('\n') || out.is_empty() { "" } else { "\n" };
        out += &line;
        done = true;
    }
    if !done {
        // Not a format 1 manifest; opening it says so.
        out += &format!("{}[project]\n{line}", if out.is_empty() || out.ends_with('\n') { "" } else { "\n" });
    }
    std::fs::write(&path, out).context("writing music2.toml")
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    struct Temp(PathBuf);

    impl Temp {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let dir = std::env::temp_dir().join(format!("m2-project-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn template(id: &str) -> PathBuf {
        Source::Template(id.into()).dir().unwrap()
    }

    fn manifest_name(dir: &Path) -> String {
        let doc: toml::Table = toml::from_str(&std::fs::read_to_string(dir.join("music2.toml")).unwrap()).unwrap();
        doc["project"]["name"].as_str().unwrap().to_string()
    }

    #[test]
    fn names_become_safe_folder_names() {
        assert_eq!(folder_name("My song"), "My song");
        assert_eq!(folder_name("  Summer  "), "Summer");
        assert_eq!(folder_name("AC/DC: live"), "AC-DC- live");
        assert_eq!(folder_name(r"back\slash"), "back-slash");
        assert_eq!(folder_name(".hidden"), "hidden");
        assert_eq!(folder_name("../../etc"), "-..-etc");
        assert_eq!(folder_name("..."), DEFAULT_NAME);
        assert_eq!(folder_name(""), DEFAULT_NAME);
        assert_eq!(folder_name("a\nb\u{7}c"), "abc");
        assert_eq!(folder_name("Café ☕ 2"), "Café ☕ 2");
        let long = "x".repeat(300);
        assert_eq!(folder_name(&long).chars().count(), MAX_NAME);
        // Cut by characters, never inside one.
        assert_eq!(folder_name(&"é".repeat(100)).chars().count(), MAX_NAME);
        // The shown name keeps what a folder name can't.
        assert_eq!(clean_name("AC/DC"), "AC/DC");
        assert_eq!(clean_name("  "), DEFAULT_NAME);
        // …but a name no folder can have is the default, as its folder is.
        for nothing in [".", "..", "...", ". . ", " .\n"] {
            assert_eq!((clean_name(nothing), folder_name(nothing)), (DEFAULT_NAME.into(), DEFAULT_NAME.into()), "{nothing:?}");
        }
        assert_eq!((clean_name("/"), folder_name("/")), ("/".into(), "-".into()));
    }

    #[test]
    fn a_name_no_folder_can_have_is_numbered_as_the_default() {
        let t = Temp::new();
        std::fs::create_dir(t.0.join(DEFAULT_NAME)).unwrap();
        let dir = create(&t.0, "..", &template("empty")).unwrap();
        assert_eq!((dir.file_name().unwrap().to_str().unwrap(), manifest_name(&dir)), ("My song 2", "My song 2".into()));
    }

    #[test]
    fn no_song_takes_the_songs_folders_exports() {
        let (songs, elsewhere) = (Temp::new(), Temp::new());
        for name in ["Exports", "exports", "EXPORTS"] {
            let dir = make(&songs.0, name, &template("empty"), Some(&songs.0)).unwrap();
            assert_eq!(dir, songs.0.join(format!("{name} 2")));
            assert_eq!(manifest_name(&dir), format!("{name} 2"));
            std::fs::remove_dir_all(&dir).unwrap();
        }
        assert!(!songs.0.join("Exports").exists());
        // Only there: anywhere else it's a name like any other.
        let dir = make(&elsewhere.0, "Exports", &template("empty"), Some(&songs.0)).unwrap();
        assert_eq!(dir, elsewhere.0.join("Exports"));
        assert!(reserved(Some(&songs.0), &songs.0.join("."), "Exports") && !reserved(None, &songs.0, "Exports"));
        assert!(!reserved(Some(&songs.0), &songs.0, "Exports 2"));
    }

    // PLAT-7 (decision D3 of the platform review, checked again): `m2 new` checks before it makes
    // any folder, so the songs folder may not be there yet. Its `Exports` is still refused when
    // the path names it another way: through a link, or relative to where m2 runs (`m2 new
    // music2/Exports` in `~/Music`).
    #[cfg(unix)]
    #[test]
    fn exports_is_reserved_before_the_songs_folder_is_made() {
        let t = Temp::new();
        std::fs::create_dir(t.0.join("Music")).unwrap();
        std::os::unix::fs::symlink(t.0.join("Music"), t.0.join("M")).unwrap();
        let songs = t.0.join("Music/music2");
        assert!(reserved(Some(&songs), &t.0.join("M/music2"), "Exports"), "through a link");
        // From this test's folder up to `/`, then down to the songs folder.
        let up: PathBuf = std::env::current_dir().unwrap().components().skip(1).map(|_| "..").collect();
        assert!(reserved(Some(&songs), &up.join(songs.strip_prefix("/").unwrap()), "exports"), "relative");
        assert!(!reserved(Some(&songs), &t.0.join("M/music3"), "Exports") && !reserved(Some(&songs), &t.0.join("M/music2"), "Night"));
        assert!(!songs.exists(), "nothing made");
        assert_eq!(resolved(&t.0.join("M/music2/a")), Some(t.0.canonicalize().unwrap().join("Music/music2/a")));
    }

    // PLAT-7, OS-41 (the owner's Q1): a name is taken in any capitals on every OS, as on macOS,
    // whatever the disk tells apart. Files count too.
    #[test]
    fn a_name_taken_in_other_capitals_is_taken() {
        let t = Temp::new();
        std::fs::create_dir(t.0.join("Exports 2")).unwrap();
        std::fs::write(t.0.join("NIGHT DRIVE"), "a file").unwrap();
        let dir = create(&t.0, "exports 2", &template("empty")).unwrap();
        assert_eq!((dir.file_name().unwrap().to_str().unwrap(), manifest_name(&dir)), ("exports 2 2", "exports 2 2".into()));
        let dir = create(&t.0, "Night drive", &template("empty")).unwrap();
        assert_eq!((dir.file_name().unwrap().to_str().unwrap(), manifest_name(&dir)), ("Night drive 2", "Night drive 2".into()));
        // Across the numbers too: "Café 2" is taken by "CAFÉ 2".
        std::fs::create_dir(t.0.join("café")).unwrap();
        std::fs::create_dir(t.0.join("CAFÉ 2")).unwrap();
        assert_eq!(create(&t.0, "Café", &template("empty")).unwrap(), t.0.join("Café 3"));
        assert_eq!(std::fs::read_to_string(t.0.join("NIGHT DRIVE")).unwrap(), "a file");
    }

    #[test]
    fn missing_plugins_are_named_plainly() {
        let plugins: BTreeMap<String, String> =
            [("org.surge-synth-team.surge-xt", "Surge XT"), ("com.airwindows.consolidated", "Airwindows Consolidated")]
                .map(|(id, name)| (id.into(), name.into()))
                .into();
        assert_eq!(missing_plugins(&plugins, |_| true), None);
        assert_eq!(
            missing_plugins(&plugins, |_| false).unwrap(),
            "it needs the Airwindows Consolidated and Surge XT plugins, which aren't installed. Install them, then try again."
        );
        assert_eq!(
            missing_plugins(&plugins, |id| id.starts_with("com.")).unwrap(),
            "it needs the Surge XT plugin, which isn't installed. Install it, then try again."
        );
        // The example that plays them says so.
        let about = about(&Source::Example("with-plugins".into()).dir().unwrap().join(EXAMPLE_TOML)).unwrap();
        assert_eq!(about.plugins, plugins);
    }

    #[test]
    fn songs_made_at_once_each_answer_with_their_own() {
        let t = Temp::new();
        let graph = std::sync::Arc::new(Graph::silent());
        for round in 0..3 {
            let name = format!("Pair {round}");
            let made: Vec<Value> = (0..4)
                .map(|_| {
                    let (graph, dir, name) = (graph.clone(), t.0.clone(), name.clone());
                    let req = NewSong { name, template: Some("empty".into()), dir: Some(dir), ..NewSong::default() };
                    std::thread::spawn(move || new_song(&graph, &req).unwrap())
                })
                .collect::<Vec<_>>()
                .into_iter()
                .map(|h| h.join().unwrap())
                .collect();
            let mut names: Vec<&str> = made.iter().map(|s| s["name"].as_str().unwrap()).collect();
            names.sort();
            assert_eq!(names, [name.clone(), format!("{name} 2"), format!("{name} 3"), format!("{name} 4")], "round {round}");
            // Each answer is the song it made: its name is its folder's.
            assert!(made.iter().all(|s| Path::new(s["dir"].as_str().unwrap()).ends_with(s["name"].as_str().unwrap())), "{made:?}");
        }
    }

    #[test]
    fn a_taken_name_gets_a_number_and_nothing_is_overwritten() {
        let t = Temp::new();
        // Something already called "My song", which must survive.
        std::fs::create_dir(t.0.join("My song")).unwrap();
        std::fs::write(t.0.join("My song/keep.txt"), "mine").unwrap();
        // A file of the next name is taken too.
        std::fs::write(t.0.join("My song 2"), "a file").unwrap();
        let a = create(&t.0, "My song", &template("empty")).unwrap();
        assert_eq!(a, t.0.join("My song 3"));
        assert_eq!(manifest_name(&a), "My song 3");
        let b = create(&t.0, "My song", &template("empty")).unwrap();
        assert_eq!(b, t.0.join("My song 4"));
        assert_eq!(std::fs::read_to_string(t.0.join("My song/keep.txt")).unwrap(), "mine");
        assert_eq!(std::fs::read_to_string(t.0.join("My song 2")).unwrap(), "a file");
        assert!(!t.0.join("My song/song.ts").exists());
    }

    #[test]
    fn a_new_song_is_the_templates_files_with_its_name() {
        let t = Temp::new();
        let dir = create(&t.0, "Night/drive", &template("starter")).unwrap();
        assert_eq!(dir, t.0.join("Night-drive"));
        assert_eq!(manifest_name(&dir), "Night/drive");
        assert!(dir.join("song.ts").is_file());
        assert!(dir.join("tracks").is_dir() && dir.join("data/arrangement.json").is_file());
        // `.music2/` is kept out of git (FMT-20).
        assert_eq!(std::fs::read_to_string(dir.join(".gitignore")).unwrap(), ".music2/\n");
        // The template's own description isn't part of the song.
        assert!(!dir.join(TEMPLATE_TOML).exists());
        let project = m2_song::Project::open(&dir).unwrap();
        assert_eq!(project.manifest.project.name, "Night/drive");
        assert_eq!(project.manifest.project.entry, "song.ts");
    }

    // FMT-4: a new song's manifest is the template's, format 1, with only its name changed.
    #[test]
    fn naming_a_song_changes_only_the_name_line_of_its_manifest() {
        let t = Temp::new();
        let manifest = "# The starter.\n[project]\nname = \"Starter beat\"   # its label\nformat = 1\nsample_rate = 44100\n\n\
                        [dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.1\"\n";
        std::fs::write(t.0.join("music2.toml"), manifest).unwrap();
        let name = "Night \"drive\" \\ ☕";
        set_name(&t.0, name).unwrap();
        let written = std::fs::read_to_string(t.0.join("music2.toml")).unwrap();
        assert_eq!(
            written,
            manifest.replace("name = \"Starter beat\"   # its label", &format!("name = {}", toml::Value::String(name.into())))
        );
        let m = m2_song::Manifest::read(&written).unwrap();
        assert_eq!((m.project.name.as_str(), m.project.sample_rate), (name, 44_100));
        // A [project] without a name gets one at its end.
        std::fs::write(t.0.join("music2.toml"), "[project]\nformat = 1\n[dependencies]\n").unwrap();
        set_name(&t.0, "x").unwrap();
        assert_eq!(std::fs::read_to_string(t.0.join("music2.toml")).unwrap(), "[project]\nformat = 1\nname = \"x\"\n[dependencies]\n");
    }

    #[test]
    fn create_at_makes_exactly_that_folder_and_refuses_one_that_exists() {
        let t = Temp::new();
        let dir = create_at(&t.0.join("deep/er/beat-one"), &template("empty")).unwrap();
        assert_eq!(manifest_name(&dir), "beat-one");
        let again = create_at(&dir, &template("starter")).unwrap_err().to_string();
        assert!(again.contains("already exists"), "{again}");
        // Untouched by the refusal: still the empty template.
        assert!(!dir.join("tracks").exists());
    }

    // PLAT-7, OS-41 (the owner's Q1) for `m2 new`: a name there in other capitals is taken on every
    // OS, and refused in the words macOS gives; nothing is made, nothing there changes.
    #[test]
    fn create_at_refuses_a_name_taken_in_other_capitals() {
        let t = Temp::new();
        let night = create_at(&t.0.join("Night"), &template("empty")).unwrap();
        std::fs::write(t.0.join("DRIVE"), "a file").unwrap();
        for asked in ["night", "NIGHT", "drive"] {
            let e = create_at(&t.0.join(asked), &template("starter")).unwrap_err().to_string();
            assert_eq!(e, format!("{} already exists", t.0.join(asked).display()));
        }
        let mut names: Vec<String> = std::fs::read_dir(&t.0).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        names.sort();
        assert_eq!(names, ["DRIVE", "Night"]);
        assert!(!night.join("tracks").exists() && std::fs::read_to_string(t.0.join("DRIVE")).unwrap() == "a file");
    }

    // PLAT-7 (decision D3 of the platform review): `m2 new NIGHT/../night` beside `Night` is
    // refused before any folder is made, so no empty `NIGHT` is left behind; with no `Night`, the
    // song is made at `night`, and still no `NIGHT`. A disk that ignores capitals (a Mac's) finds
    // `NIGHT` there, so its refusal names the path as written.
    #[test]
    fn create_at_checks_before_it_makes_a_folder() {
        let t = Temp::new();
        std::fs::create_dir(t.0.join("Night")).unwrap();
        let e = create_at(&t.0.join("NIGHT/../night"), &template("empty")).unwrap_err().to_string();
        let asked = [t.0.join("night"), t.0.join("NIGHT/../night")].map(|p| format!("{} already exists", p.display()));
        assert!(asked.contains(&e), "{e}");
        let names: Vec<String> = std::fs::read_dir(&t.0).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        assert_eq!(names, ["Night"]);
        std::fs::remove_dir(t.0.join("Night")).unwrap();
        assert_eq!(create_at(&t.0.join("NIGHT/../night"), &template("empty")).unwrap(), t.0.join("night"));
        let names: Vec<String> = std::fs::read_dir(&t.0).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        assert_eq!(names, ["night"]);
    }

    // PLAT-7, WRITE-1 (decisions D2 and D3 of the platform review): New Song and `m2 new` in a
    // folder that can be written but not listed make the song. Its name is checked exactly, by the
    // create-only `create_dir`, as its capitals can't be; the folder's sync is skipped, since the
    // folder can't be opened for it.
    #[cfg(unix)]
    #[test]
    fn a_song_is_made_in_a_folder_that_cant_be_listed() {
        use std::os::unix::fs::PermissionsExt;
        let t = Temp::new();
        let shut = t.0.join("shut");
        std::fs::create_dir(&shut).unwrap();
        std::fs::create_dir(shut.join("Taken")).unwrap();
        std::fs::set_permissions(&shut, std::fs::Permissions::from_mode(0o300)).unwrap();
        let made = (create(&shut, "Night", &template("empty")), create(&shut, "Taken", &template("empty")));
        let at = (create_at(&shut.join("Drive"), &template("empty")), create_at(&shut.join("Taken"), &template("empty")));
        std::fs::set_permissions(&shut, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(made.0.unwrap(), shut.join("Night"));
        assert_eq!(made.1.unwrap(), shut.join("Taken 2"), "the exact name is taken");
        assert_eq!(at.0.unwrap(), shut.join("Drive"));
        assert_eq!(at.1.unwrap_err().to_string(), format!("{} already exists", shut.join("Taken").display()));
        assert_eq!(manifest_name(&shut.join("Drive")), "Drive");
    }

    #[test]
    fn a_failed_copy_leaves_no_folder_behind() {
        let t = Temp::new();
        let missing = t.0.join("no-such-template");
        assert!(create(&t.0, "Broken", &missing).is_err());
        assert!(!t.0.join("Broken").exists());
        assert!(create(&t.0.join("not-a-folder"), "x", &template("empty")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_copy_of_read_only_files_can_be_changed() {
        use std::os::unix::fs::PermissionsExt;
        let (src, t) = (Temp::new(), Temp::new());
        std::fs::create_dir(src.0.join("tracks")).unwrap();
        for f in ["song.ts", "music2.toml", "tracks/a.ts"] {
            std::fs::write(src.0.join(f), "").unwrap();
            std::fs::set_permissions(src.0.join(f), std::fs::Permissions::from_mode(0o444)).unwrap();
        }
        let dir = create(&t.0, "Mine", &src.0).unwrap();
        for f in ["song.ts", "music2.toml", "tracks/a.ts"] {
            assert_ne!(std::fs::metadata(dir.join(f)).unwrap().permissions().mode() & 0o200, 0, "{f}");
        }
        assert_eq!(manifest_name(&dir), "Mine");
    }

    #[test]
    fn ids_name_bundled_folders_only() {
        for bad in ["", ".", "..", "../examples/first-song", "a/b", ".hidden", "nope"] {
            assert!(Source::Template(bad.into()).dir().is_err(), "{bad}");
            assert!(Source::Example(bad.into()).dir().is_err(), "{bad}");
        }
        assert!(Source::Example("first-song".into()).dir().is_ok());
    }

    #[test]
    fn templates_and_examples_are_listed_with_names() {
        let t = templates();
        let ids: Vec<&str> = t.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(&ids[..2], ["starter", "empty"], "{t:?}");
        assert!(t.iter().all(|s| !s.name.is_empty() && !s.description.is_empty()), "{t:?}");
        let e = examples();
        assert!(e.iter().any(|s| s.id == "first-song" && !s.name.is_empty()), "{e:?}");
        assert_eq!(title("first-song"), "First song");
    }

    #[test]
    fn an_example_copy_brings_its_assets() {
        let t = Temp::new();
        let dir = create(&t.0, "Resample", &Source::Example("resample".into()).dir().unwrap()).unwrap();
        assert!(dir.join("assets/keys-bounce.wav").is_file());
        assert_eq!(
            std::fs::metadata(dir.join("assets/keys-bounce.wav")).unwrap().len(),
            std::fs::metadata(resources::examples_dir().join("resample/assets/keys-bounce.wav")).unwrap().len()
        );
        assert_eq!(manifest_name(&dir), "Resample");
    }

    #[test]
    fn new_song_makes_the_folder_and_opens_it() {
        let t = Temp::new();
        let graph = Graph::silent();
        let req = |name: &str, template: Option<&str>| NewSong {
            name: name.into(),
            template: template.map(Into::into),
            dir: Some(t.0.clone()),
            ..NewSong::default()
        };
        let state = new_song(&graph, &req("Friday", None)).unwrap();
        assert_eq!(state["name"], "Friday");
        assert_eq!(graph.dir().unwrap(), t.0.join("Friday").canonicalize().unwrap());
        // The starter's six tracks, each with its lane, are in the timeline.
        let tracks = state["timeline"]["tracks"].as_array().unwrap();
        let named: Vec<&str> = tracks.iter().filter(|t| t["track"] == true).map(|t| t["module"].as_str().unwrap()).collect();
        assert_eq!(named.len(), 6, "{named:?}");
        assert!(tracks.iter().filter(|t| t["track"] == true).all(|t| t["lanes"].as_array().is_some_and(|l| !l.is_empty())));
        assert_eq!(state["timeline"]["end"], 32.0);

        // Again: a second folder, and the new song is the one open.
        let empty = new_song(&graph, &req("Friday", Some("empty"))).unwrap();
        assert_eq!(empty["name"], "Friday 2");
        assert!(empty["timeline"]["tracks"].as_array().unwrap().iter().all(|t| t["track"] != true));
        assert!(t.0.join("Friday/song.ts").is_file() && t.0.join("Friday 2/song.ts").is_file());

        // An example copy is named after the example unless a name is given.
        let ex = new_song(&graph, &NewSong { example: Some("first-song".into()), dir: Some(t.0.clone()), ..NewSong::default() }).unwrap();
        assert_eq!(ex["name"], "First song");
        assert!(t.0.join("First song/data/kick/hits.json").is_file());
    }

    #[test]
    fn new_song_refuses_bad_requests_and_keeps_nothing_it_couldnt_open() {
        let t = Temp::new();
        let graph = Graph::silent();
        let bad = |req: NewSong| new_song(&graph, &NewSong { dir: req.dir.or(Some(t.0.clone())), ..req }).unwrap_err().to_string();
        assert!(bad(NewSong { template: Some("../../std".into()), ..NewSong::default() }).contains("no template"));
        assert!(bad(NewSong { example: Some("nope".into()), ..NewSong::default() }).contains("no example"));
        assert!(bad(NewSong { dir: Some("relative/dir".into()), ..NewSong::default() }).contains("full path"));
        assert!(bad(NewSong { dir: Some(t.0.join("missing")), ..NewSong::default() }).contains("isn't a folder"));
        assert_eq!(std::fs::read_dir(&t.0).unwrap().count(), 0);
        assert!(graph.dir().is_none());

        // A song that doesn't open (FMT-3: a newer format): the folder made for it is removed
        // again, and the song open before stays open.
        let opened = open_new(&graph, &t.0, "Fine", &template("empty")).unwrap();
        let templates = Temp::new();
        let manifest = "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.1\"\n";
        let newer = templates.0.join("newer");
        std::fs::create_dir_all(&newer).unwrap();
        std::fs::write(newer.join("music2.toml"), manifest.replace("format = 1", "format = 2")).unwrap();
        std::fs::write(newer.join("song.ts"), "export default song(() => 0);\n").unwrap();
        let e = format!("{:#}", open_new(&graph, &t.0, "Newer", &newer).unwrap_err());
        assert!(e.contains("format"), "{e}");
        assert!(!t.0.join("Newer").exists());
        assert_eq!(graph.state().unwrap()["session"], opened["session"]);
        // One whose code doesn't compile opens (WRITE-20), with its problem, and is kept.
        let broken = templates.0.join("broken");
        std::fs::create_dir_all(&broken).unwrap();
        std::fs::write(broken.join("music2.toml"), manifest).unwrap();
        std::fs::write(broken.join("song.ts"), "export default song(() => oops);\n").unwrap();
        let state = open_new(&graph, &t.0, "Broken", &broken).unwrap();
        assert_eq!((&state["diagnostics"][0]["file"], &state["diagnostics"][0]["line"]), (&json!("song.ts"), &json!(1)), "{state:#}");
        assert!(t.0.join("Broken/song.ts").exists());
    }

    #[test]
    fn the_songs_folder_is_under_home() {
        let songs = songs_dir().unwrap();
        assert!(songs.ends_with("Music/music2"), "{}", songs.display());
        assert!(songs.starts_with(std::env::var("HOME").unwrap()));
    }

    #[test]
    fn every_template_compiles_and_renders_sound_or_silence_as_it_says() {
        let mut packages = m2_runtime::PackageSet::default();
        packages.load(&resources::std_dir()).unwrap();
        for s in templates() {
            let t = Temp::new();
            let dir = create(&t.0, &s.name, &template(&s.id)).unwrap();
            let project = m2_song::Project::open(&dir).unwrap();
            let c = m2_session::compile(&project, &mut packages).unwrap_or_else(|d| panic!("{}: {d:?}", s.id));
            let mut r = m2_runtime::Renderer::new(&c.flat, &c.data, &packages, project.sample_rate()).unwrap();
            let frames = r.render(4.0).unwrap();
            let peak = frames.iter().fold(0.0f32, |m, f| m.max(f[0].abs()).max(f[1].abs()));
            if s.id == "empty" {
                assert_eq!(peak, 0.0, "the empty song plays silence");
            } else {
                assert!(peak > 0.1 && peak < 1.0, "{}: peak {peak}", s.id);
            }
        }
    }
}
