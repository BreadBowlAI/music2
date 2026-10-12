//! The TypeScript sandbox: runs a song's modules and returns its graph IR.
//!
//! TypeScript is stripped with oxc and run in QuickJS. The sandbox has no file
//! system, network, clock or randomness. Project code comes only from the snapshot the
//! host passes in, and imports resolve against it byte for byte (FMT-6a); packages are
//! read from their folders, and project code may import only those its manifest lists
//! (FMT-3). Running a module can only describe a graph. Source locations in the IR
//! point at the original `.ts` lines.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

pub mod sites;

use m2_ir::{Body, Diagnostic, Ir};
use oxc::allocator::Allocator;
use serde::{Deserialize, Serialize};
use oxc::codegen::{Codegen, CodegenOptions};
use oxc::parser::Parser;
use oxc::semantic::SemanticBuilder;
use oxc::span::SourceType;
use oxc::transformer::{TransformOptions, Transformer};
use rquickjs::loader::{ImportAttributes, Loader, Resolver};
use rquickjs::{CatchResultExt, CaughtError, Context, Ctx, Function, Module, Runtime};
use sha2::{Digest, Sha256};

/// The `@m2/core` SDK, shipped inside the core.
pub const SDK: &str = include_str!("../sdk/core.ts");

/// The version of `@m2/core` that [`SDK`] is. A project's `[dependencies] "@m2/core"`
/// requirement is checked against it (FMT-2, FMT-3).
pub const SDK_VERSION: &str = "1.0.0";

const CORE: &str = "@m2/core";

/// Project modules are named by their snapshot path behind this prefix, so a project file
/// is never confused with a package file (an absolute path) or the SDK.
const PROJECT: &str = "./";

/// The codes a build raises (DIAG-2). An error from the SDK naming any other is `code.build`.
const CODES: &[&str] = &[
    "code.build",
    "import.outside-code",
    "key.invalid",
    "key.duplicate",
    "lane.conflict",
    "lane.shared",
    "pkg.undeclared",
    "pkg.unsatisfied",
];

const OUTSIDE: &str = "is outside the project: code imports only the project's own .ts files and its packages";

/// Runs before anything else: remove what would make a build non-deterministic.
const PRELUDE: &str = r#"
globalThis.__m2_site = -1;
globalThis.__m2s = function (site, value) { globalThis.__m2_site = site; return value; };
delete globalThis.Date;
Math.random = () => { throw new Error("Math.random isn't available: songs must render the same every time. Use a seeded generator."); };
"#;

pub struct Options {
    /// The project's code: every Code file of the snapshot, by its path in the project,
    /// `/`-separated (`song.ts`, `tracks/bass.ts`). Nothing of the project is read from
    /// disk, so a build is a function of its snapshot (FMT-6a).
    pub code: BTreeMap<String, String>,
    /// The root module: a path in `code`, usually `song.ts`.
    pub entry: String,
    /// Installed packages by name, e.g. `std` → its folder. `import "std/dsp"`
    /// loads `<folder>/src/dsp.ts`.
    pub packages: Vec<(String, PathBuf)>,
    /// The packages the manifest's `[dependencies]` lists besides `@m2/core`: the only ones
    /// project code may import (FMT-3).
    pub dependencies: Vec<String>,
    pub memory_limit: usize,
    pub time_limit: Duration,
}

/// What a build produces.
pub struct Build {
    pub ir: Ir,
    /// Every call site in the project and package files that ran.
    pub sites: Vec<sites::Site>,
    /// What each call that ran returned, when it was a signal.
    pub outputs: Vec<Output>,
    /// Every package file the build read, so a revision records what ran (FMT-6a, RENDER-2).
    pub package_files: Vec<PackageFile>,
}

/// A package file a build read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageFile {
    /// As shown to people: `std/src/dsp.ts`.
    pub path: String,
    /// The SHA-256 of its bytes, in lowercase hex.
    pub sha256: String,
}

/// A signal returned by a call: what `const wobble = lfo(...)` holds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Output {
    pub site: u32,
    /// The module's key.
    pub module: String,
    /// The port reference in that module's nodes.
    #[serde(rename = "ref")]
    pub port: String,
    /// Whether it is at the module's top level (not inside a `poly` voice).
    pub top: bool,
}

impl Options {
    pub fn new(code: BTreeMap<String, String>, entry: &str) -> Options {
        Options {
            code,
            entry: entry.to_string(),
            packages: vec![],
            dependencies: vec![],
            memory_limit: 256 << 20,
            time_limit: Duration::from_secs(10),
        }
    }
}

/// Maps positions in generated JavaScript back to the TypeScript source.
#[derive(Default)]
struct LineMap {
    /// `(js_line, js_col, ts_line, ts_col)`, all 0-based, sorted.
    tokens: Vec<(u32, u32, u32, u32)>,
}

impl LineMap {
    fn lookup(&self, line: u32, col: u32) -> Option<(u32, u32)> {
        let i = self.tokens.partition_point(|t| (t.0, t.1) <= (line, col));
        let t = self.tokens.get(i.checked_sub(1)?)?;
        (t.0 == line).then_some((t.2, t.3))
    }
}

struct LoadedFile {
    original: String,
    instrumented: Option<sites::Instrumented>,
    map: LineMap,
}

#[derive(Default)]
struct State {
    code: BTreeMap<String, String>,
    packages: Vec<(String, PathBuf)>,
    dependencies: Vec<String>,
    files: HashMap<String, LoadedFile>,
    sites: Vec<sites::Site>,
    package_files: Vec<PackageFile>,
    /// Why an import failed, with its code: the engine passes on only the text.
    failed: Option<Diagnostic>,
}

impl State {
    /// `file:line:col` in generated JS → `relative/file.ts:line:col` in the source (1-based).
    fn remap(&self, at: &str) -> String {
        let mut parts = at.rsplitn(3, ':');
        let (col, line, file) = match (parts.next(), parts.next(), parts.next()) {
            (Some(c), Some(l), Some(f)) => (c.parse::<u32>().ok(), l.parse::<u32>().ok(), f),
            _ => return at.to_string(),
        };
        let (Some(line), Some(col)) = (line, col) else { return at.to_string() };
        let Some(f) = self.files.get(file) else { return format!("{}:{line}:{col}", self.display(file)) };
        let (line, col) = match f.map.lookup(line.saturating_sub(1), col.saturating_sub(1)) {
            // Map from the instrumented text back to the original.
            Some((l, c)) => match &f.instrumented {
                Some(inst) => {
                    let off = offset_of(&inst.code, l as usize, c as usize);
                    let (l, c) = line_col(&f.original, inst.original_offset(off as u32) as usize);
                    (l as u32, c as u32)
                }
                None => (l + 1, c + 1),
            },
            None => (line, col),
        };
        format!("{}:{line}:{col}", self.display(file))
    }

    /// `file:line:col` of a site's start.
    fn site_src(&self, site: u32) -> Option<String> {
        let s = self.sites.iter().find(|s| s.id == site)?;
        let f = self.files.values().find(|f| f.instrumented.as_ref().is_some_and(|i| i.sites.iter().any(|x| x.id == site)))?;
        let (l, c) = line_col(&f.original, s.start as usize);
        Some(format!("{}:{l}:{c}", s.file))
    }

    fn display(&self, file: &str) -> String {
        if let Some(rel) = file.strip_prefix(PROJECT) {
            return rel.to_string();
        }
        let p = Path::new(file);
        for (name, dir) in &self.packages {
            if let Ok(rel) = p.strip_prefix(dir) {
                return format!("{name}/{}", rel.display());
            }
        }
        file.to_string()
    }

    /// The module that `name`, imported from the module `base`, names.
    fn resolve(&self, base: &str, name: &str) -> Result<String, Box<Diagnostic>> {
        if name == CORE {
            return Ok(CORE.into());
        }
        let in_project = base == "<main>" || base.starts_with(PROJECT);
        let from = in_project.then(|| self.display(base)).filter(|_| base != "<main>");
        let fail = |code: &'static str, message: String| Box::new(Diagnostic::error(code, message).at(from.clone()));
        let relative = name.starts_with("./") || name.starts_with("../");
        if Path::new(name).is_absolute() || name.starts_with('/') {
            return Err(fail("import.outside-code", format!("`{name}` {OUTSIDE}")));
        }
        if in_project && relative {
            let dir = base.strip_prefix(PROJECT).and_then(|b| b.rsplit_once('/')).map_or("", |(d, _)| d);
            return self.project_file(dir, name).map(|p| format!("{PROJECT}{p}")).map_err(|m| fail("import.outside-code", m));
        }
        let found = if relative {
            let dir = Path::new(base).parent().map(Path::to_path_buf).unwrap_or_default();
            self.package_file(&normalize(&dir.join(name)))
        } else {
            let (pkg, sub) = name.split_once('/').unwrap_or((name, "index"));
            if in_project && !self.dependencies.iter().any(|d| d == pkg) {
                let e = format!("`{name}` is from the package `{pkg}`, which music2.toml doesn't list: add it under [dependencies]");
                return Err(fail("pkg.undeclared", e));
            }
            match self.packages.iter().find(|(n, _)| n == pkg) {
                Some((_, dir)) => self.package_file(&normalize(&dir.join("src").join(sub))),
                None => {
                    let have: Vec<&str> = self.packages.iter().map(|(n, _)| n.as_str()).collect();
                    let code = if in_project { "pkg.unsatisfied" } else { "code.build" };
                    return Err(fail(code, format!("no package `{pkg}` is installed (installed: {})", have.join(", "))));
                }
            }
        };
        // A path out of the package, or a spelling in other capitals, is no file the build may
        // read (FMT-6a); a file the package doesn't have is a build error like any other.
        found.map_err(|(code, m)| fail(code, format!("`{name}`: {m}")))
    }

    /// The snapshot's code file that `spec`, imported from the folder `dir`, names: the
    /// path, then with `.ts`, then `/index.ts`, byte for byte, with no case folding (FMT-6a).
    fn project_file(&self, dir: &str, spec: &str) -> Result<String, String> {
        let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
        for seg in spec.split('/') {
            match seg {
                "" | "." => {}
                ".." if parts.pop().is_none() => return Err(format!("`{spec}` {OUTSIDE}")),
                ".." => {}
                s => parts.push(s),
            }
        }
        let path = parts.join("/");
        let tried = match path.as_str() {
            "" => vec!["index.ts".to_string()],
            _ => vec![path.clone(), format!("{path}.ts"), format!("{path}/index.ts")],
        };
        if let Some(p) = tried.iter().find(|p| self.code.contains_key(p.as_str())) {
            return Ok(p.clone());
        }
        let folded: Vec<String> = tried.iter().map(|p| p.to_lowercase()).collect();
        if let Some(k) = self.code.keys().find(|k| folded.contains(&k.to_lowercase())) {
            return Err(format!("`{spec}` must be spelled as the file is named, `{k}`, with the same capitals"));
        }
        if path.ends_with(".js") || path.ends_with(".mjs") || path.ends_with(".cjs") {
            return Err(format!("`{spec}` names `{path}`, which isn't TypeScript: code imports only the project's .ts files"));
        }
        Err(format!("`{spec}` names no code file of the project (looked for {})", tried.join(", ")))
    }

    /// A file inside an installed package: the path, then with `.ts`, then `/index.ts`,
    /// spelled exactly as in its folder's listing, so every disk finds the same files. An error
    /// comes with its code.
    fn package_file(&self, p: &Path) -> Result<String, (&'static str, String)> {
        let Some((_, dir)) = self.packages.iter().find(|(_, d)| p.starts_with(d)) else {
            return Err(("import.outside-code", "songs can only import files from their own project or from installed packages".into()));
        };
        let tried = [p.to_path_buf(), PathBuf::from(format!("{}.ts", p.display())), p.join("index.ts")];
        let listed: Vec<(&PathBuf, PathBuf)> = tried.iter().filter_map(|c| Some((c, listed(dir, c)?))).collect();
        if let Some((c, _)) = listed.iter().find(|(c, real)| *c == real) {
            return Ok(c.to_string_lossy().into_owned());
        }
        match listed.first() {
            Some((_, real)) => Err((
                "import.outside-code",
                format!("must be spelled as the file is named, `{}`, with the same capitals", self.display(&real.to_string_lossy())),
            )),
            None => Err(("code.build", "file not found".into())),
        }
    }
}

/// The file at `path` below `root` as its folders' listings spell it, matching each part
/// exactly or else ignoring case; `None` when there is no such file.
fn listed(root: &Path, path: &Path) -> Option<PathBuf> {
    let rel = path.strip_prefix(root).ok()?;
    let mut at = root.to_path_buf();
    for c in rel.components() {
        let names: Vec<std::ffi::OsString> = std::fs::read_dir(&at).ok()?.flatten().map(|e| e.file_name()).collect();
        let want = c.as_os_str();
        let name = names.iter().find(|n| *n == want).or_else(|| names.iter().find(|n| n.eq_ignore_ascii_case(want)))?;
        at.push(name);
    }
    at.is_file().then_some(at)
}

/// Strip TypeScript, keeping a map back to the source lines.
fn strip(path: &str, source: &str) -> Result<(String, LineMap), String> {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
    if let Some(e) = ret.diagnostics.first() {
        let at = e.labels.first().map(|l| l.offset()).unwrap_or(0);
        let (line, col) = line_col(source, at as usize);
        return Err(format!("{path}:{line}:{col}: {}", e.message));
    }
    let mut program = ret.program;
    let scoping = SemanticBuilder::new().build(&program).semantic.into_scoping();
    let options = TransformOptions::default();
    let tr = Transformer::new(&allocator, Path::new(path), &options).build_with_scoping(scoping, &mut program);
    if let Some(e) = tr.diagnostics.first() {
        return Err(format!("{path}: {}", e.message));
    }
    let out = Codegen::new().with_options(CodegenOptions { source_map_path: Some(PathBuf::from(path)), ..Default::default() }).build(&program);
    let mut map = LineMap::default();
    if let Some(sm) = &out.map {
        map.tokens = sm.get_tokens().map(|t| (t.get_dst_line(), t.get_dst_col(), t.get_src_line(), t.get_src_col())).collect();
        map.tokens.sort_unstable();
    }
    Ok((out.code, map))
}

/// Byte offset of a 0-based line and column (in characters).
fn offset_of(text: &str, line: usize, col: usize) -> usize {
    let start: usize = text.split_inclusive('\n').take(line).map(str::len).sum();
    let rest = &text[start.min(text.len())..];
    start + rest.char_indices().nth(col).map_or(rest.len(), |(i, _)| i)
}

fn line_col(source: &str, offset: usize) -> (usize, usize) {
    let before = &source[..offset.min(source.len())];
    let line = before.matches('\n').count() + 1;
    let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    (line, col)
}

/// Lexically normalise `a/b/../c` without touching the file system.
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            c => out.push(c),
        }
    }
    out
}

struct Resolve(Rc<RefCell<State>>);

impl Resolver for Resolve {
    fn resolve<'js>(&mut self, _ctx: &Ctx<'js>, base: &str, name: &str, _: Option<ImportAttributes<'js>>) -> rquickjs::Result<String> {
        let mut st = self.0.borrow_mut();
        st.resolve(base, name).map_err(|d| {
            let e = rquickjs::Error::new_resolving_message(base, name, d.message.clone());
            st.failed.get_or_insert(*d);
            e
        })
    }
}

struct Load(Rc<RefCell<State>>);

impl Loader for Load {
    fn load<'js>(&mut self, ctx: &Ctx<'js>, name: &str, _: Option<ImportAttributes<'js>>) -> rquickjs::Result<Module<'js, rquickjs::module::Declared>> {
        let fail = |e: String| rquickjs::Error::new_loading_message(name, e);
        let mut st = self.0.borrow_mut();
        let (original, instrumented) = if name == CORE {
            (SDK.to_string(), None)
        } else {
            let source = match name.strip_prefix(PROJECT) {
                Some(rel) => st.code.get(rel).cloned().ok_or_else(|| fail("not a code file of the project".into()))?,
                None => {
                    let bytes = std::fs::read(name).map_err(|e| fail(e.to_string()))?;
                    let sha256 = format!("{:x}", Sha256::digest(&bytes));
                    let path = st.display(name);
                    st.package_files.push(PackageFile { path, sha256 });
                    String::from_utf8(bytes).map_err(|e| fail(e.to_string()))?
                }
            };
            let first = st.sites.last().map_or(0, |s| s.id + 1);
            let inst = sites::instrument(&st.display(name), &source, first);
            let mut inst = inst.map_err(|e| {
                // `file:line:col: message`: report it at that place.
                let (at, message) = e.split_once(": ").map_or((None, e.as_str()), |(a, m)| (Some(a.to_string()), m));
                st.failed.get_or_insert(Diagnostic::error("code.build", message).at(at));
                fail(e.clone())
            })?;
            let in_project = name.starts_with(PROJECT);
            for s in &mut inst.sites {
                s.in_project = in_project;
            }
            st.sites.extend(inst.sites.iter().cloned());
            (source, Some(inst))
        };
        let code = instrumented.as_ref().map_or(original.as_str(), |i| i.code.as_str());
        let (js, map) = strip(name, code).map_err(fail)?;
        st.files.insert(name.to_string(), LoadedFile { original, instrumented, map });
        drop(st);
        Module::declare(ctx.clone(), name, js)
    }
}

/// Run the song and return its IR, or what went wrong.
pub fn build(opts: &Options) -> Result<Ir, Vec<Diagnostic>> {
    build_full(opts).map(|b| b.ir)
}

/// Like [`build`], also returning the call sites the IR's origins refer to.
pub fn build_full(opts: &Options) -> Result<Build, Vec<Diagnostic>> {
    let err = |m: String| vec![Diagnostic::error("code.build", m)];
    if !opts.code.contains_key(&opts.entry) {
        return Err(err(format!("the entry `{}` is not a code file of the project", opts.entry)));
    }
    let packages: Vec<(String, PathBuf)> = opts.packages.iter().filter_map(|(n, d)| Some((n.clone(), d.canonicalize().ok()?))).collect();
    let state = Rc::new(RefCell::new(State {
        code: opts.code.clone(),
        packages: packages.clone(),
        dependencies: opts.dependencies.clone(),
        ..Default::default()
    }));

    let rt = Runtime::new().map_err(|e| err(e.to_string()))?;
    rt.set_memory_limit(opts.memory_limit);
    rt.set_max_stack_size(1 << 20);
    let deadline = Instant::now() + opts.time_limit;
    rt.set_interrupt_handler(Some(Box::new(move || Instant::now() > deadline)));
    rt.set_loader(Resolve(state.clone()), Load(state.clone()));
    let ctx = Context::full(&rt).map_err(|e| err(e.to_string()))?;

    let result: Result<(String, String), Vec<Diagnostic>> = ctx.with(|ctx| {
        let run = || -> rquickjs::Result<(String, String)> {
            ctx.eval::<(), _>(PRELUDE)?;
            let g = ctx.globals();
            g.set("__m2_project", PROJECT)?;
            g.set("__m2_packages", packages.iter().map(|(_, d)| format!("{}/", d.display())).collect::<Vec<_>>())?;
            // What the SDK asks the host while it runs: where a call or a stack frame is in
            // the TypeScript, and whether a key is valid (one rule, in m2_ir::key).
            let st = state.clone();
            let place = move |at: String| st.try_borrow().map_or_else(|_| at.clone(), |st| st.remap(&at));
            g.set("__m2_where", Function::new(ctx.clone(), place)?)?;
            let st = state.clone();
            g.set("__m2_site_src", Function::new(ctx.clone(), move |site: u32| st.try_borrow().ok().and_then(|st| st.site_src(site)))?)?;
            g.set("__m2_key", Function::new(ctx.clone(), |what: String, k: String| m2_ir::key::invalid(&what, &k))?)?;
            let entry = serde_json::to_string(&format!("{PROJECT}{}", opts.entry)).expect("string");
            let main = format!("import {{ __build }} from \"{CORE}\";\nimport song from {entry};\nglobalThis.__m2_out = __build(song);\n");
            Module::evaluate(ctx.clone(), "<main>", main)?.finish::<()>()?;
            Ok((g.get("__m2_out")?, g.get("__m2_outputs")?))
        };
        run().catch(&ctx).map_err(|e| vec![caught(&mut state.borrow_mut(), e, Instant::now() > deadline)])
    });
    let (json, outputs) = result?;
    let outputs: Vec<Output> = serde_json::from_str(&outputs).map_err(|e| err(format!("the SDK produced invalid outputs: {e}")))?;
    let mut ir: Ir = serde_json::from_str(&json).map_err(|e| err(format!("the SDK produced invalid IR: {e}")))?;
    let st = state.borrow();
    for m in &mut ir.modules {
        m.src = m.src.as_deref().map(|s| st.remap(s));
        remap_nodes(&st, &mut m.nodes);
    }
    Ok(Build { ir, sites: st.sites.clone(), outputs, package_files: st.package_files.clone() })
}

fn remap_nodes(st: &State, nodes: &mut [m2_ir::Node]) {
    for n in nodes {
        // A node made by a known call points exactly at it; otherwise use the stack.
        n.src = n.origin.as_ref().and_then(|o| st.site_src(o.site)).or_else(|| n.src.as_deref().map(|s| st.remap(s)));
        if let m2_ir::Op::Poly { body: Body { nodes, .. }, .. } = &mut n.op {
            remap_nodes(st, nodes);
        }
    }
}

/// Turn a JavaScript error into a diagnostic pointing at the user's code. An error the
/// SDK threw with a known `code` keeps it, and the key of the module it was building.
fn caught(st: &mut State, e: CaughtError, timed_out: bool) -> Diagnostic {
    if timed_out {
        return Diagnostic::error("code.build", "the song took too long to build (is there an endless loop?)");
    }
    // An import or a load that failed is the error, unless the failure was caught (a
    // dynamic import) and this is another one.
    let text = match &e {
        CaughtError::Exception(ex) => ex.message().unwrap_or_default(),
        CaughtError::Error(e) => e.to_string(),
        CaughtError::Value(_) => String::new(),
    };
    if let Some(d) = st.failed.take().filter(|d| text.contains(&d.message)) {
        return d;
    }
    match e {
        CaughtError::Exception(ex) => {
            let message = ex.message().unwrap_or_else(|| "error".into());
            let field = |k: &str| ex.as_object().get::<_, Option<String>>(k).ok().flatten();
            let code = field("code").and_then(|c| CODES.iter().copied().find(|k| *k == c)).unwrap_or("code.build");
            let stack = ex.stack().unwrap_or_default();
            let frame = stack.lines().filter_map(frame_at).find(|at| at.starts_with(PROJECT));
            let other = || stack.lines().filter_map(frame_at).find(|at| !at.starts_with(CORE) && !at.starts_with("<main>"));
            let src = frame.or_else(other).map(|at| st.remap(&at));
            let mut d = Diagnostic::error(code, message).at(src);
            d.module = field("module");
            d
        }
        CaughtError::Value(v) => Diagnostic::error("code.build", format!("the song threw {v:?}")),
        CaughtError::Error(e) => Diagnostic::error("code.build", e.to_string()),
    }
}

/// `"    at f (/a/b.ts:3:9)"` → `"/a/b.ts:3:9"`.
fn frame_at(line: &str) -> Option<String> {
    let s = line.trim().strip_prefix("at ")?;
    let s = s.rsplit_once('(').map_or(s, |(_, r)| r).trim_end_matches(')');
    let mut it = s.rsplitn(3, ':');
    let (c, l, f) = (it.next()?, it.next()?, it.next()?);
    (c.parse::<u32>().is_ok() && l.parse::<u32>().is_ok()).then(|| format!("{f}:{l}:{c}"))
}

#[cfg(test)]
mod tests;
