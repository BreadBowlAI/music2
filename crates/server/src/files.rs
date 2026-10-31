//! The project's TypeScript as text, for the code editor.
//!
//! The editor opens the accepted revision's files, type-checks them against the SDK and
//! the packages' sources, and saves one file at a time. A save is the session's `save()`
//! (WRITE-1, WRITE-2): it names the text it was based on and is written only if the file on
//! disk still says that, so it never overwrites a change made since (a knob, a cable, another
//! editor). Code that doesn't compile is saved too: the session plays on the last revision
//! that did, and the draft's problems come back as [`diagnostics`].

use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use m2_ir::{Diagnostic, Severity};
use m2_runtime::PackageSet;
use m2_session::{Refusal, Session};
use m2_song::{Base, DataPath, Snapshot};
use serde::Serialize;
use serde_json::{Value, json};

/// A save refused because the file changed since the editor read it.
#[derive(Debug)]
pub struct Conflict {
    pub path: String,
    /// What the file says now.
    pub current: String,
}

impl fmt::Display for Conflict {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} changed since it was opened", self.path)
    }
}

impl std::error::Error for Conflict {}

/// An agent's save the song wouldn't take in as written (WRITE-2, AGENT-1): nothing was written.
/// Why, as the build said it.
#[derive(Debug)]
pub struct Unbuilt {
    pub path: String,
    pub diagnostics: Vec<m2_song::Diagnostic>,
}

impl fmt::Display for Unbuilt {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let why: Vec<String> = self.diagnostics.iter().map(|d| d.message.clone()).collect();
        write!(f, "{} wasn't saved: the song doesn't build with it ({})", self.path, why.join("; "))
    }
}

impl std::error::Error for Unbuilt {}

/// One reason the draft doesn't compile, placed in a file when it can be told where.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Problem {
    /// Its stable code from the contract's table (DIAG-2), such as `code.build`.
    pub code: &'static str,
    pub severity: Severity,
    /// Relative to the project (`tracks/bass.ts`), or to a package's parent (`std/src/dsp.ts`).
    pub file: Option<String>,
    /// 1-based, like the compiler's messages.
    pub line: Option<u32>,
    /// 1-based, in UTF-16 code units as editors count them (an emoji is two), where the
    /// compiler counts characters.
    pub col: Option<u32>,
    pub message: String,
    /// The key of the module whose graph the problem is in, if it is about one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    /// What went wrong in plain words, for a problem a rule knows (`m2_editor::explain`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plain: Option<String>,
    /// What to do about it, in plain words.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
    /// Where inside a package it happened, when `file` is the call in the project (Q46).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inner: Option<String>,
}

impl Default for Problem {
    fn default() -> Problem {
        Problem {
            code: "code.build",
            severity: Severity::Error,
            file: None,
            line: None,
            col: None,
            message: String::new(),
            module: None,
            plain: None,
            fix: None,
            inner: None,
        }
    }
}

/// The file a request names: a path relative to the project that `DataPath` classes as Code
/// (FMT-5), and which stays inside the project once links are followed. Returns where to write.
pub fn resolve(dir: &Path, path: &str) -> Result<PathBuf> {
    let rel = Path::new(path);
    let bad = || anyhow!("`{path}` is not one of the project's TypeScript files");
    if path.contains('\\') || !matches!(DataPath::parse(rel), DataPath::Code(_)) {
        return Err(bad());
    }
    let dir = dir.canonicalize().with_context(|| format!("opening {}", dir.display()))?;
    let file = dir.join(rel).canonicalize().map_err(|_| anyhow!("there is no {path} in the project"))?;
    if !file.starts_with(&dir) || !file.is_file() || file.extension().is_none_or(|x| x != "ts") {
        return Err(bad());
    }
    Ok(file)
}

/// Save `text` to the project's file `path` if it still says `base`, through the session's
/// `save()` (WRITE-2): compared and swapped, staged and renamed whole, then taken in. Otherwise
/// fail with a [`Conflict`] carrying what it says now. A path that is a link within the project
/// saves the file it leads to, so the link stays. A file that isn't UTF-8 is shown as its lossy
/// text, which stands for its bytes here, so saving it as UTF-8 is how it is fixed (WRITE-2).
/// `built`: an agent's save, written only when the song takes it in as written, else [`Unbuilt`]
/// (AGENT-1).
pub fn save(session: &mut Session, path: &str, base: &str, text: &str, built: bool) -> Result<m2_session::take::Saved> {
    let dir = session.accepted().project.dir.clone();
    let file = resolve(&dir, path)?;
    let root = dir.canonicalize().with_context(|| format!("opening {}", dir.display()))?;
    let rel = file.strip_prefix(&root).map_err(|_| anyhow!("`{path}` is not one of the project's TypeScript files"))?.to_path_buf();
    let read = || std::fs::read(&file).with_context(|| format!("reading {path}"));
    let bytes = read()?;
    let current = String::from_utf8_lossy(&bytes);
    if current != base {
        return Err(Conflict { path: path.into(), current: current.into_owned() }.into());
    }
    let base = match std::str::from_utf8(&bytes) {
        Ok(_) => Base::text(base),
        Err(_) => Base::Sha(m2_song::Sha::of(&bytes)),
    };
    let saved = if built { session.save_built(&rel, base, text) } else { session.save(&rel, base, text) };
    match saved {
        Ok(saved) => Ok(saved),
        Err(Refusal::Conflict { .. }) => {
            Err(Conflict { path: path.into(), current: String::from_utf8_lossy(&read()?).into_owned() }.into())
        }
        Err(Refusal::Rejected { diagnostics, .. }) if built => Err(Unbuilt { path: path.into(), diagnostics }.into()),
        Err(r) => Err(anyhow!("{path} wasn't saved: {r}")),
    }
}

/// The accepted revision's TypeScript files and modules, as the session's actor publishes them
/// once per change, so listing them never waits for the session.
pub struct Accepted {
    revision: u64,
    dir: PathBuf,
    files: Vec<(PathBuf, std::sync::Arc<str>)>,
    modules: Vec<Value>,
}

/// What [`list`] needs of the accepted revision.
pub fn accepted(session: &Session) -> Accepted {
    let rev = session.accepted();
    let ts = |p: &&PathBuf| matches!(DataPath::parse(p), DataPath::Code(_));
    let files = rev.snapshot.files.iter().filter(|(p, _)| ts(p)).map(|(p, t)| (p.clone(), t.clone())).collect();
    let modules = rev
        .compiled
        .ir
        .modules
        .iter()
        .map(|m| {
            let file = m.src.as_deref().and_then(located).map(|(f, ..)| f).filter(|f| rev.snapshot.files.contains_key(Path::new(f)));
            // Addressed by key, shown by label (ID-10, ID-11).
            json!({"key": m.key, "label": m.label(), "file": file})
        })
        .collect();
    Accepted { revision: rev.number, dir: rev.project.dir.clone(), files, modules }
}

/// The accepted revision's TypeScript files with the module each one defines. A file the
/// disk has differently (saved, but not compiling) also carries that text as `draft`
/// (null if it is gone), and one only on disk is listed with `accepted: false`, so the
/// editor shows and saves against what is really there.
pub fn list(accepted: &Accepted) -> Result<Value> {
    let disk = Snapshot::read(&accepted.dir)?;
    let ts = |p: &&PathBuf| matches!(DataPath::parse(p), DataPath::Code(_));
    let mut files = vec![];
    for (path, text) in &accepted.files {
        let mut f = json!({"path": slashes(path), "text": text});
        match disk.files.get(path) {
            Some(d) if d == text => {}
            d => f["draft"] = json!(d),
        }
        files.push(f);
    }
    for (path, text) in disk.files.iter().filter(|(p, _)| ts(p) && !accepted.files.iter().any(|(a, _)| a == *p)) {
        files.push(json!({"path": slashes(path), "text": text, "accepted": false}));
    }
    Ok(json!({"revision": accepted.revision, "files": files, "modules": accepted.modules}))
}

/// The TypeScript the editor checks imports against, by the path each import resolves to
/// under `node_modules`: `@m2/core/index.ts` for the SDK, `std/fx.ts` for `std/fx`.
pub fn types(packages: &PackageSet) -> Value {
    let mut files = vec![json!({"path": "@m2/core/index.ts", "text": m2_script::SDK})];
    for p in packages.packages() {
        let src = p.dir.join("src");
        let mut stack = vec![src.clone()];
        while let Some(d) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&d) else { continue };
            for path in entries.flatten().map(|e| e.path()) {
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|x| x == "ts")
                    && let Ok(text) = std::fs::read_to_string(&path)
                {
                    let rel = path.strip_prefix(&src).expect("under src");
                    files.push(json!({"path": format!("{}/{}", p.name, slashes(rel)), "text": text}));
                }
            }
        }
        // Its instruments written as data, as one module, unless its code has that module (PACK-13).
        let data = m2_script::data_files(&p.dir);
        if !data.is_empty() && !src.join("instruments.ts").is_file() && !src.join("instruments/index.ts").is_file() {
            let read: Vec<(String, String)> = data
                .iter()
                .filter_map(|f| {
                    Some((f.strip_suffix(".toml")?.to_string(), std::fs::read_to_string(p.dir.join("instruments").join(f)).ok()?))
                })
                .collect();
            files.push(json!({"path": format!("{}/instruments.ts", p.name), "text": m2_song::instrument::types(&read)}));
        }
    }
    files.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    json!({ "files": files })
}

/// The draft's problems, each placed where its message or source says, with paths made
/// relative so they read like the editor's file list.
pub fn diagnostics(session: &Session) -> Vec<Problem> {
    let dir = &session.accepted().project.dir;
    let mut roots = vec![(format!("{}/", dir.display()), String::new())];
    // The draft's own packages (PACK-9): one it brings in is named too, though nothing plays it yet.
    for (name, folder) in session.draft_packages() {
        // The build names package files by the folder as given; the runtime may by the real one.
        let at = folder.canonicalize().unwrap_or_else(|_| folder.clone());
        roots.push((format!("{}/", folder.display()), format!("{name}/")));
        roots.push((format!("{}/", at.display()), format!("{name}/")));
    }
    let relative = |s: &str| roots.iter().fold(s.to_string(), |s, (from, to)| s.replace(from.as_str(), to));
    // A relative path back to the file on disk: a package's by its name, else the project's.
    let absolute = |file: &str| {
        let (from, to) = roots.iter().rev().find(|(_, to)| file.starts_with(to.as_str())).expect("the project's root matches any path");
        PathBuf::from(format!("{from}{}", &file[to.len()..]))
    };
    let mut problems: Vec<Problem> = session.draft_errors.iter().map(|d| problem(d, &relative, dir)).collect();
    // Each in plain words, where a rule knows it (the studio plan's D2), from its place in the
    // file as the compiler counts it: in characters.
    let song = if problems.is_empty() { Default::default() } else { m2_editor::explain::Song::of(session) };
    for p in &mut problems {
        let text = p.file.as_deref().zip(p.line).and_then(|(file, line)| {
            let text = std::fs::read_to_string(absolute(file)).ok()?;
            text.lines().nth(line.checked_sub(1)? as usize).map(str::to_string)
        });
        let at = m2_editor::explain::At {
            file: p.file.as_deref(),
            line: p.line,
            col: p.col,
            text: text.as_deref(),
            module: p.module.as_deref(),
        };
        if let Some(plain) = m2_editor::explain::explain(p.code, &p.message, &song, &at) {
            (p.plain, p.fix) = (Some(plain.words), plain.fix);
        }
    }
    for p in &mut problems {
        if let (Some(file), Some(line), Some(col)) = (&p.file, p.line, p.col)
            && let Ok(text) = std::fs::read_to_string(absolute(file))
        {
            p.col = Some(utf16_col(&text, line, col));
        }
    }
    problems
}

/// The revision's problems as the window shows them (DIAG-4), each with its words in plain words
/// and a fix (`plain`, `fix`) when a rule knows it (the studio plan's D2).
pub fn plain_problems(session: &Session, problems: &[m2_song::Diagnostic]) -> Vec<Value> {
    if problems.is_empty() {
        return vec![];
    }
    let song = m2_editor::explain::Song::named(session);
    problems
        .iter()
        .map(|d| {
            let mut v = serde_json::to_value(d).expect("a diagnostic is JSON");
            let src = d.src.as_deref();
            let (file, line, col) = match src.and_then(located) {
                Some((file, line, col)) => (Some(file), Some(line), Some(col)),
                None => (src.map(str::to_string), None, None),
            };
            let at = m2_editor::explain::At { file: file.as_deref(), line, col, text: None, module: d.module.as_deref() };
            if let Some(plain) = m2_editor::explain::explain(d.code, &d.message, &song, &at) {
                v["plain"] = plain.words.into();
                if let Some(fix) = plain.fix {
                    v["fix"] = fix.into();
                }
            }
            v
        })
        .collect()
}

/// A 1-based column counted in characters on 1-based `line` of `text`, counted in UTF-16 code units.
fn utf16_col(text: &str, line: u32, col: u32) -> u32 {
    let Some(l) = text.lines().nth(line.saturating_sub(1) as usize) else { return col };
    let before: usize = l.chars().take(col.saturating_sub(1) as usize).map(char::len_utf16).sum();
    // Past the line's end, as many columns past it as the compiler said.
    let past = (col.saturating_sub(1) as usize).saturating_sub(l.chars().count());
    (before + past) as u32 + 1
}

fn problem(d: &Diagnostic, relative: &impl Fn(&str) -> String, dir: &Path) -> Problem {
    let message = relative(&d.message);
    let mut p = Problem {
        code: d.code,
        severity: d.severity,
        module: d.module.clone(),
        inner: d.inner.as_deref().map(|i| relative(i)),
        ..Default::default()
    };
    // A parse error names its place in the message: `Error loading module '…': tracks/bass.ts:3:1: Expected …`.
    if let Some((at, rest)) = find_location(&message) {
        let (file, line, col) = at;
        (p.file, p.line, p.col, p.message) = (Some(file), Some(line), Some(col), rest.to_string());
        return p;
    }
    p.message = message;
    let src = d.src.as_deref().map(relative);
    if let Some((file, line, col)) = src.as_deref().and_then(located) {
        (p.file, p.line, p.col) = (Some(file), Some(line), Some(col));
    } else if let Some(file) = src.filter(|f| f.ends_with(".ts")) {
        // About a whole file, such as an import that resolves to nothing (`std/nope`: …): at the
        // import, when the message names one.
        let spec = p.message.strip_prefix('`').and_then(|m| m.split_once('`')).map(|(spec, _)| spec.to_string());
        let at = spec.and_then(|spec| std::fs::read_to_string(dir.join(&file)).ok().and_then(|text| find_import(&text, &spec)));
        (p.file, p.line, p.col) = (Some(file), at.map(|a| a.0), at.map(|a| a.1));
    } else if let Some((spec, from)) = unresolved(&p.message) {
        // An import that resolves to nothing: point at it in the importing file.
        let at = std::fs::read_to_string(dir.join(&from)).ok().and_then(|text| find_import(&text, &spec));
        p.file = Some(from);
        (p.line, p.col) = (at.map(|a| a.0), at.map(|a| a.1));
    }
    p
}

/// `file.ts:line:col` → its parts.
fn located(s: &str) -> Option<(String, u32, u32)> {
    let mut parts = s.rsplitn(3, ':');
    let (col, line, file) = (parts.next()?.parse().ok()?, parts.next()?.parse().ok()?, parts.next()?);
    file.ends_with(".ts").then(|| (file.to_string(), line, col))
}

/// The first `file.ts:line:col: ` in a message, and what follows it.
fn find_location(message: &str) -> Option<((String, u32, u32), &str)> {
    let mut from = 0;
    while let Some(i) = message[from..].find(".ts:") {
        let end = from + i + 3;
        let start = message[..end].rfind([' ', '\'', '"', '(']).map_or(0, |s| s + 1);
        let tail = &message[end + 1..];
        let mut nums = tail.splitn(3, ':');
        if let (Some(line), Some(col), Some(rest)) = (nums.next(), nums.next(), nums.next())
            && let (Ok(line), Ok(col)) = (line.parse(), col.parse())
        {
            return Some(((message[start..end].to_string(), line, col), rest.trim_start()));
        }
        from = end;
    }
    None
}

/// `Error resolving module 'std/nope' from 'tracks/bass.ts': …` → the specifier and the importing file.
fn unresolved(message: &str) -> Option<(String, String)> {
    let rest = message.strip_prefix("Error resolving module '")?;
    let (spec, rest) = rest.split_once("' from '")?;
    let (from, _) = rest.split_once('\'')?;
    from.ends_with(".ts").then(|| (spec.to_string(), from.to_string()))
}

/// Where `spec` is written as a quoted import specifier: 1-based line and column.
fn find_import(text: &str, spec: &str) -> Option<(u32, u32)> {
    text.lines().enumerate().find_map(|(i, line)| {
        let at = [format!("'{spec}'"), format!("\"{spec}\"")].iter().find_map(|q| line.find(q.as_str()))?;
        Some((i as u32 + 1, line[..at].chars().count() as u32 + 2))
    })
}

fn slashes(p: &Path) -> String {
    p.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Project(PathBuf);

    impl Project {
        fn new(files: &[(&str, &str)]) -> Project {
            static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let dir = std::env::temp_dir().join(format!("m2-files-{}-{n}", std::process::id()));
            let p = Project(dir);
            p.write(
                "music2.toml",
                "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.8\"\n\"@m2/drums\" = \"1\"\n\"@m2/effects\" = \"1\"\n\"@m2/synths\" = \"1\"\n",
            );
            for (name, text) in files {
                p.write(name, text);
            }
            p
        }

        fn write(&self, name: &str, text: &str) {
            let p = self.0.join(name);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }

        fn read(&self, name: &str) -> String {
            std::fs::read_to_string(self.0.join(name)).unwrap()
        }

        fn session(&self) -> Session {
            let mut packages = PackageSet::default().with_store(m2_song::pack::test_store());
            packages.load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")).unwrap();
            Session::open(&self.0, packages).unwrap()
        }
    }

    impl Drop for Project {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const SONG: &str = "import { song } from '@m2/core';\nimport lead from './tracks/lead';\nexport default song(() => lead.out);\n";
    const LEAD: &str =
        "import { hz, track } from '@m2/core';\nimport { osc } from 'std/dsp';\nexport default track('lead', () => osc.sine(hz(300)));\n";

    fn lead() -> Project {
        Project::new(&[("song.ts", SONG), ("tracks/lead.ts", LEAD)])
    }

    #[test]
    fn only_typescript_files_inside_the_project_resolve() {
        let p = lead();
        p.write("notes.json", "{}");
        p.write("assets/x.ts", "");
        p.write(".hidden/x.ts", "");
        let outside = Project::new(&[("secret.ts", "")]);
        assert!(resolve(&p.0, "tracks/lead.ts").unwrap().ends_with("tracks/lead.ts"));
        assert!(resolve(&p.0, "song.ts").is_ok());
        for bad in [
            "",
            "tracks",
            "notes.json",
            "music2.toml",
            "../secret.ts",
            "tracks/../../secret.ts",
            "tracks/../song.ts",
            "./song.ts",
            "assets/x.ts",
            ".hidden/x.ts",
            "tracks\\lead.ts",
            "missing.ts",
        ] {
            assert!(resolve(&p.0, bad).is_err(), "{bad:?} resolves");
        }
        let absolute = outside.0.join("secret.ts");
        assert!(resolve(&p.0, absolute.to_str().unwrap()).is_err(), "an absolute path resolves");
        assert!(resolve(&p.0, p.0.join("song.ts").to_str().unwrap()).is_err(), "an absolute path into the project resolves");
    }

    #[cfg(unix)]
    #[test]
    fn a_link_may_lead_within_the_project_never_out_of_it() {
        let p = lead();
        let outside = Project::new(&[("secret.ts", "// not the project's")]);
        std::os::unix::fs::symlink(outside.0.join("secret.ts"), p.0.join("tracks/escape.ts")).unwrap();
        std::os::unix::fs::symlink(&outside.0, p.0.join("linked")).unwrap();
        std::os::unix::fs::symlink(p.0.join("tracks/lead.ts"), p.0.join("alias.ts")).unwrap();
        assert!(resolve(&p.0, "tracks/escape.ts").is_err());
        assert!(resolve(&p.0, "linked/secret.ts").is_err());
        assert!(save(&mut p.session(), "tracks/escape.ts", "// not the project's", "// overwritten", false).is_err());
        assert_eq!(std::fs::read_to_string(outside.0.join("secret.ts")).unwrap(), "// not the project's");
        assert!(resolve(&p.0, "alias.ts").unwrap().ends_with("tracks/lead.ts"));
    }

    #[test]
    fn a_save_is_written_only_over_the_text_it_was_based_on() {
        let p = lead();
        let mut s = p.session();
        let changed = LEAD.replace("300", "310");
        save(&mut s, "tracks/lead.ts", LEAD, &changed, false).unwrap();
        assert_eq!(p.read("tracks/lead.ts"), changed);

        // Saved again from the old text: refused, and told what the file says now.
        let e = save(&mut s, "tracks/lead.ts", LEAD, &LEAD.replace("300", "320"), false).unwrap_err();
        let conflict = e.downcast_ref::<Conflict>().expect("a conflict");
        assert_eq!(conflict.current, changed);
        assert_eq!(p.read("tracks/lead.ts"), changed);
        assert!(!p.0.join("tracks/lead.m2tmp").exists());
    }

    // WRITE-2: a code file that isn't UTF-8 opens as its lossy text, and saving that text's edit
    // writes it as UTF-8: how it is fixed. Any other base is still a conflict.
    #[test]
    fn a_file_that_isnt_utf8_is_saved_from_its_lossy_text() {
        let p = lead();
        std::fs::write(p.0.join("tracks/lead.ts"), [b"// caf\xe9\n".as_slice(), LEAD.as_bytes()].concat()).unwrap();
        let mut s = p.session();
        let shown = format!("// caf\u{fffd}\n{LEAD}");
        assert!(save(&mut s, "tracks/lead.ts", LEAD, LEAD, false).unwrap_err().downcast_ref::<Conflict>().is_some());
        save(&mut s, "tracks/lead.ts", &shown, &format!("// café\n{LEAD}"), false).unwrap();
        assert_eq!(p.read("tracks/lead.ts"), format!("// café\n{LEAD}"));
    }

    #[cfg(unix)]
    #[test]
    fn a_link_planted_beside_a_file_is_never_written_through() {
        let p = lead();
        let outside = Project::new(&[("victim.txt", "victim data\n")]);
        for name in ["tracks/lead.m2tmp", &format!("tracks/.lead.ts.{}-0.m2tmp", std::process::id())] {
            std::os::unix::fs::symlink(outside.0.join("victim.txt"), p.0.join(name)).unwrap();
        }
        save(&mut p.session(), "tracks/lead.ts", LEAD, "// saved\n", false).unwrap();
        assert_eq!(outside.read("victim.txt"), "victim data\n");
        assert!(!std::fs::symlink_metadata(p.0.join("tracks/lead.ts")).unwrap().file_type().is_symlink());
        assert_eq!(p.read("tracks/lead.ts"), "// saved\n");
    }

    #[cfg(unix)]
    #[test]
    fn a_save_through_a_link_keeps_the_link() {
        let p = lead();
        std::os::unix::fs::symlink(p.0.join("tracks/lead.ts"), p.0.join("alias.ts")).unwrap();
        save(&mut p.session(), "alias.ts", LEAD, "// saved\n", false).unwrap();
        assert!(std::fs::symlink_metadata(p.0.join("alias.ts")).unwrap().file_type().is_symlink());
        assert_eq!(p.read("tracks/lead.ts"), "// saved\n");
    }

    #[test]
    fn files_are_the_accepted_revision_with_what_the_disk_says_differently() {
        let p = lead();
        p.write("tracks/notes.ts", "// unused\n");
        let mut s = p.session();
        let listed = list(&accepted(&s)).unwrap();
        let paths: Vec<&str> = listed["files"].as_array().unwrap().iter().map(|f| f["path"].as_str().unwrap()).collect();
        assert_eq!(paths, ["song.ts", "tracks/lead.ts", "tracks/notes.ts"]);
        assert_eq!(listed["files"][1]["text"], LEAD);
        assert!(listed["files"][1].get("draft").is_none());
        // ID-11: modules by key, with their label.
        let lead = listed["modules"].as_array().unwrap().iter().find(|m| m["key"] == "lead").unwrap();
        assert_eq!((&lead["file"], &lead["label"]), (&json!("tracks/lead.ts"), &json!("lead")));
        assert_eq!(listed["modules"].as_array().unwrap().iter().find(|m| m["key"] == "song").unwrap()["file"], "song.ts");

        // Saved but broken: the accepted text stays, and the draft is what's on disk.
        p.write("tracks/lead.ts", "export default (");
        p.write("tracks/new.ts", "// new\n");
        std::fs::remove_file(p.0.join("tracks/notes.ts")).unwrap();
        s.refresh();
        let listed = list(&accepted(&s)).unwrap();
        let files = listed["files"].as_array().unwrap();
        assert_eq!((&files[1]["text"], &files[1]["draft"]), (&json!(LEAD), &json!("export default (")));
        assert_eq!(files[2], json!({"path": "tracks/notes.ts", "text": "// unused\n", "draft": null}));
        assert_eq!(files[3], json!({"path": "tracks/new.ts", "text": "// new\n", "accepted": false}));
    }

    #[test]
    fn types_are_the_sdk_and_the_package_sources_by_import_path() {
        let p = lead();
        let s = p.session();
        let t = types(&s.packages);
        let paths: Vec<&str> = t["files"].as_array().unwrap().iter().map(|f| f["path"].as_str().unwrap()).collect();
        assert!(paths.contains(&"@m2/core/index.ts") && paths.contains(&"std/dsp.ts") && paths.contains(&"std/fx.ts"), "{paths:?}");
        let dsp = t["files"].as_array().unwrap().iter().find(|f| f["path"] == "std/dsp.ts").unwrap();
        assert!(dsp["text"].as_str().unwrap().contains("export const osc"));
    }

    /// The problems with `tracks/lead.ts` saying `text`, in a session opened before it did.
    fn problems(p: &Project, text: &str) -> Vec<Problem> {
        p.write("tracks/lead.ts", LEAD);
        let mut s = p.session();
        p.write("tracks/lead.ts", text);
        s.refresh();
        diagnostics(&s)
    }

    #[test]
    fn diagnostics_say_where_in_the_project_each_problem_is() {
        let p = lead();
        // A syntax error, from the parser's message.
        let syntax = problems(&p, "import { hz, track } from '@m2/core';\nexport default track('lead', () => hz(300)\n");
        assert_eq!(syntax.len(), 1);
        assert_eq!((syntax[0].file.as_deref(), syntax[0].line, syntax[0].col), (Some("tracks/lead.ts"), Some(3), Some(1)));
        assert!(syntax[0].message.starts_with("Expected `)`"), "{}", syntax[0].message);

        // An error thrown while the song is built, from its source.
        let thrown = problems(&p, "import { hz, track } from '@m2/core';\nexport default track('lead', () => osc.sine(hz(300)));\n");
        assert_eq!((thrown[0].file.as_deref(), thrown[0].line, thrown[0].col), (Some("tracks/lead.ts"), Some(2), Some(36)));
        assert_eq!(thrown[0].message, "osc is not defined");

        // An import of nothing, at the import.
        let import = problems(&p, &LEAD.replace("std/dsp", "std/nope"));
        assert_eq!((import[0].file.as_deref(), import[0].line, import[0].col), (Some("tracks/lead.ts"), Some(2), Some(22)));
        // DIAG-1: with its code and severity.
        assert_eq!((import[0].code, import[0].severity), ("code.build", m2_ir::Severity::Error));
        assert!(!import[0].message.contains(&p.0.display().to_string()), "paths are relative: {}", import[0].message);

        // A graph that doesn't check: its module, and the package line that made the node.
        let graph = problems(
            &p,
            "import { hz, track } from '@m2/core';\nimport { osc, vca } from 'std/dsp';\nexport default track('lead', () => vca(osc.sine(hz(300)), osc.sine(hz(3))));\n",
        );
        assert_eq!(graph[0].module.as_deref(), Some("lead"));
        // At the call in the project that made the node (the studio plan's Q46); the package line as `inner`.
        assert_eq!((graph[0].file.as_deref(), graph[0].line), (Some("tracks/lead.ts"), Some(3)), "{:?}", graph[0]);
        assert!(graph[0].inner.as_deref().is_some_and(|f| f.starts_with("std/src/")), "{:?}", graph[0]);
        assert!(graph[0].message.contains("expects control<gain>"), "{}", graph[0].message);

        // Columns count as an editor does: each emoji before the error is two.
        let emoji =
            problems(&p, "import { hz, track } from '@m2/core';\nconst s = \"🎸🎸\"; export default track('lead', () => hz(300);\n");
        let line = "const s = \"🎸🎸\"; export default track('lead', () => hz(300);";
        let col = emoji[0].col.unwrap() as usize;
        let units: Vec<u16> = line.encode_utf16().collect();
        assert_eq!(emoji[0].line, Some(2));
        assert_eq!(String::from_utf16(&units[col - 1..col]).unwrap(), ";", "{:?} at {col}", emoji[0].message);
        assert_eq!(utf16_col("a🎸b", 1, 3), 4);
        assert_eq!(utf16_col("ab", 1, 5), 5, "past the end stays past it");

        // An import of a name its module doesn't export, at the import.
        let export = problems(&p, &LEAD.replace("{ osc }", "{ nope }").replace("osc.sine", "nope"));
        assert_eq!((export[0].file.as_deref(), export[0].line, export[0].col), (Some("tracks/lead.ts"), Some(2), Some(10)));
        assert!(export[0].message.contains("'std/src/dsp.ts'"), "package paths are shortened: {}", export[0].message);

        // No location at all: a thrown value that isn't an error has no stack.
        let none = problems(&p, &LEAD.replace("export default", "throw 1;\nexport default"));
        assert_eq!((none[0].file.as_deref(), none[0].line), (None, None));
        assert!(none[0].message.starts_with("the song threw"), "{}", none[0].message);
    }

    // The studio plan's D2: a problem a rule knows carries plain words and a fix beside its own
    // message, at the call in the project, with the place inside std as `inner`; one no rule
    // knows has none of them. The revision's problems get theirs too.
    #[test]
    fn diagnostics_carry_plain_words_and_the_technical_text() {
        let p = lead();
        let unit = problems(
            &p,
            "import { db, hz, track } from '@m2/core';\nimport { osc } from 'std/dsp';\nimport { lowpass } from 'std/filter';\nexport default track('lead', () => lowpass(osc.sine(hz(110)), db(-3)));\n",
        );
        assert_eq!((unit[0].file.as_deref(), unit[0].line, unit[0].col), (Some("tracks/lead.ts"), Some(4), Some(36)), "{:?}", unit[0]);
        assert_eq!(unit[0].plain.as_deref(), Some("Cutoff wants a frequency (Hz), but gets a level (dB)."));
        assert_eq!(unit[0].fix.as_deref(), Some("Write it in Hz, such as hz(800)."));
        assert_eq!(unit[0].message, "input `cutoff` expects control<hz>, got control<db>");
        assert!(unit[0].inner.as_deref().is_some_and(|i| i.starts_with("std/src/filter.ts:")), "{:?}", unit[0]);
        let json = serde_json::to_value(&unit[0]).unwrap();
        assert!(["plain", "fix", "inner", "message", "code"].iter().all(|k| json.get(k).is_some()), "{json}");

        let none = problems(&p, &LEAD.replace("export default", "throw 1;\nexport default"));
        assert_eq!((none[0].plain.as_deref(), none[0].fix.as_deref(), none[0].inner.as_deref()), (None, None, None));
        let json = serde_json::to_value(&none[0]).unwrap();
        assert!(json.get("plain").is_none() && json.get("fix").is_none() && json.get("inner").is_none(), "{json}");

        // A revision's problem: a clip's file isn't in assets/.
        let vox = Project::new(&[
            (
                "song.ts",
                "import { song, track } from '@m2/core';\nconst v = track('vox', { name: 'Vox' }, ({ lane }) => lane.audio('takes'));\nexport default song(() => v.out);\n",
            ),
            ("data/vox/takes.json", r#"{"clips": [{"id": "a", "start": 0, "length": 4, "asset": "vox.wav"}]}"#),
        ]);
        let s = vox.session();
        let shown = plain_problems(&s, s.problems());
        let missing = shown.iter().find(|v| v["code"] == "asset.missing").unwrap_or_else(|| panic!("{shown:?}"));
        assert_eq!(missing["plain"], "vox.wav isn't in the song's assets folder, so its clip plays nothing.");
        assert_eq!(missing["fix"], "Put the file back, or delete the clip.");
        assert!(missing["message"].as_str().is_some_and(|m| m.contains("`vox.wav` isn't in assets/")), "{missing}");
    }

    /// A copy of the fixture pack `@test/tiny` at `to`.
    fn tiny_at(to: &Path) {
        fn copy(from: &Path, to: &Path) {
            std::fs::create_dir_all(to).unwrap();
            for e in std::fs::read_dir(from).unwrap().flatten() {
                let (f, t) = (e.path(), to.join(e.file_name()));
                if f.is_dir() { copy(&f, &t) } else { std::fs::copy(&f, &t).map(drop).unwrap() }
            }
        }
        copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../core/session/tests/fixtures/packs/tiny"), to);
    }

    // PACK-9: a draft that brings in a pack and doesn't build is explained with that pack, which the
    // accepted revision doesn't have: a missing module of it named among the pack's modules, and
    // the pack's files named by the pack, never by where they are.
    #[test]
    fn a_failed_draft_is_explained_with_its_own_packs() {
        let p = lead();
        let tiny = PathBuf::from(format!("{}-tiny", p.0.display()));
        tiny_at(&tiny);
        let mut s = p.session();
        let name = tiny.file_name().unwrap().to_str().unwrap();
        p.write("music2.toml", &format!("{}\"@test/tiny\" = {{ path = \"../{name}\" }}\n", p.read("music2.toml")));
        let importing = |line: &str| LEAD.replace("import { osc } from 'std/dsp';", &format!("import {{ osc }} from 'std/dsp';\n{line}"));
        // Used, so the import isn't left out as one only of types.
        p.write("tracks/lead.ts", &importing("import { tinyKeys } from '@test/tiny/nope';\nconst used = [tinyKeys];"));
        s.refresh();
        let missing = diagnostics(&s);
        assert_eq!(missing[0].fix.as_deref(), Some("Check its name: @test/tiny has @test/tiny."), "{:?}", missing[0]);
        // A problem with the pack's own files names them by the pack, never by where they are.
        let toml = std::fs::read_to_string(tiny.join("m2pkg.toml")).unwrap();
        std::fs::write(tiny.join("m2pkg.toml"), toml.replace("version = \"0.1.0\"", "version = \"0.1.0\"\nwasm = \"k.wasm\"")).unwrap();
        p.write("tracks/lead.ts", LEAD);
        s.refresh();
        let broken = diagnostics(&s);
        let message = &broken[0].message;
        assert!(message.contains("@test/tiny/k.wasm") && !message.contains(&*tiny.to_string_lossy()), "{message}");
        std::fs::remove_dir_all(&tiny).ok();
    }
}
