//! The TypeScript sandbox: runs a song's modules and returns its graph IR.
//!
//! TypeScript is stripped with oxc and run in QuickJS. The sandbox has no file
//! system, network, clock or randomness: imports are limited to the project's own
//! files, `@m2/core` and installed packages, and running a module can only
//! describe a graph. Source locations in the IR point at the original `.ts` lines.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

pub mod sites;

use m2_ir::{Body, Diagnostic, Ir};
use oxc::allocator::Allocator;
use oxc::codegen::{Codegen, CodegenOptions};
use oxc::parser::Parser;
use oxc::semantic::SemanticBuilder;
use oxc::span::SourceType;
use oxc::transformer::{TransformOptions, Transformer};
use rquickjs::loader::{ImportAttributes, Loader, Resolver};
use rquickjs::{CatchResultExt, CaughtError, Context, Ctx, Module, Runtime};

/// The `@m2/core` SDK, shipped inside the core.
pub const SDK: &str = include_str!("../sdk/core.ts");

const CORE: &str = "@m2/core";

/// Runs before anything else: remove what would make a build non-deterministic.
const PRELUDE: &str = r#"
globalThis.__m2_site = -1;
globalThis.__m2s = function (site, value) { globalThis.__m2_site = site; return value; };
delete globalThis.Date;
Math.random = () => { throw new Error("Math.random isn't available: songs must render the same every time. Use a seeded generator."); };
"#;

pub struct Options {
    /// The project folder; its files may import each other.
    pub project: PathBuf,
    /// The root module, usually `song.ts` in the project.
    pub entry: PathBuf,
    /// Installed packages by name, e.g. `std` → its folder. `import "std/dsp"`
    /// loads `<folder>/src/dsp.ts`.
    pub packages: Vec<(String, PathBuf)>,
    pub memory_limit: usize,
    pub time_limit: Duration,
    /// File contents to use instead of what is on disk, by canonical path. Lets the
    /// session build a candidate edit without writing it.
    pub sources: HashMap<PathBuf, String>,
}

/// What a build produces.
pub struct Build {
    pub ir: Ir,
    /// Every call site in the project and package files that ran.
    pub sites: Vec<sites::Site>,
}

impl Options {
    pub fn new(project: &Path, entry: &Path) -> Options {
        Options {
            project: project.to_path_buf(),
            entry: entry.to_path_buf(),
            packages: vec![],
            memory_limit: 256 << 20,
            time_limit: Duration::from_secs(10),
            sources: HashMap::new(),
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
    project: PathBuf,
    packages: Vec<(String, PathBuf)>,
    sources: HashMap<PathBuf, String>,
    files: HashMap<String, LoadedFile>,
    sites: Vec<sites::Site>,
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
        let p = Path::new(file);
        if let Ok(rel) = p.strip_prefix(&self.project) {
            return rel.display().to_string();
        }
        for (name, dir) in &self.packages {
            if let Ok(rel) = p.strip_prefix(dir) {
                return format!("{name}/{}", rel.display());
            }
        }
        file.to_string()
    }
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
        let st = self.0.borrow();
        let fail = |msg: String| rquickjs::Error::new_resolving_message(base, name, msg);
        if name == CORE {
            return Ok(CORE.into());
        }
        let file = |p: PathBuf| -> Option<PathBuf> {
            let p = normalize(&p);
            [p.clone(), p.with_extension("ts"), PathBuf::from(format!("{}.ts", p.display())), p.join("index.ts")].into_iter().find(|c| c.is_file())
        };
        let found = if name.starts_with("./") || name.starts_with("../") {
            let dir = if base == "<main>" { st.project.clone() } else { Path::new(base).parent().map(Path::to_path_buf).unwrap_or_default() };
            file(dir.join(name))
        } else if Path::new(name).is_absolute() {
            file(PathBuf::from(name))
        } else {
            let (pkg, sub) = name.split_once('/').unwrap_or((name, "index"));
            match st.packages.iter().find(|(n, _)| n == pkg) {
                Some((_, dir)) => file(dir.join("src").join(sub)),
                None => {
                    let have: Vec<&str> = st.packages.iter().map(|(n, _)| n.as_str()).collect();
                    return Err(fail(format!("no package `{pkg}` is installed (installed: {})", have.join(", "))));
                }
            }
        };
        let Some(path) = found else { return Err(fail("file not found".into())) };
        let path = path.canonicalize().map_err(|e| fail(e.to_string()))?;
        let inside = path.starts_with(&st.project) || st.packages.iter().any(|(_, d)| path.starts_with(d));
        if !inside {
            return Err(fail("songs can only import files from their own project or from installed packages".into()));
        }
        Ok(path.to_string_lossy().into_owned())
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
            let source = match st.sources.get(Path::new(name)) {
                Some(s) => s.clone(),
                None => std::fs::read_to_string(name).map_err(|e| fail(e.to_string()))?,
            };
            let first = st.sites.last().map_or(0, |s| s.id + 1);
            let mut inst = sites::instrument(&st.display(name), &source, first).map_err(fail)?;
            let in_project = Path::new(name).starts_with(&st.project);
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
    let err = |m: String| vec![Diagnostic::new(m)];
    let project = opts.project.canonicalize().map_err(|e| err(format!("{}: {e}", opts.project.display())))?;
    let entry = opts.entry.canonicalize().map_err(|e| err(format!("{}: {e}", opts.entry.display())))?;
    let packages: Vec<(String, PathBuf)> = opts.packages.iter().filter_map(|(n, d)| Some((n.clone(), d.canonicalize().ok()?))).collect();
    let state = Rc::new(RefCell::new(State {
        project: project.clone(),
        packages: packages.clone(),
        sources: opts.sources.clone(),
        files: HashMap::new(),
        sites: vec![],
    }));

    let rt = Runtime::new().map_err(|e| err(e.to_string()))?;
    rt.set_memory_limit(opts.memory_limit);
    rt.set_max_stack_size(1 << 20);
    let deadline = Instant::now() + opts.time_limit;
    rt.set_interrupt_handler(Some(Box::new(move || Instant::now() > deadline)));
    rt.set_loader(Resolve(state.clone()), Load(state.clone()));
    let ctx = Context::full(&rt).map_err(|e| err(e.to_string()))?;

    let result: Result<String, Diagnostic> = ctx.with(|ctx| {
        let run = || -> rquickjs::Result<String> {
            ctx.eval::<(), _>(PRELUDE)?;
            let g = ctx.globals();
            g.set("__m2_project", format!("{}/", project.display()))?;
            g.set("__m2_packages", packages.iter().map(|(_, d)| format!("{}/", d.display())).collect::<Vec<_>>())?;
            let main = format!(
                "import {{ __build }} from \"{CORE}\";\nimport song from {};\nglobalThis.__m2_out = __build(song);\n",
                serde_json::to_string(&entry.to_string_lossy()).expect("string")
            );
            Module::evaluate(ctx.clone(), "<main>", main)?.finish::<()>()?;
            g.get("__m2_out")
        };
        run().catch(&ctx).map_err(|e| caught(&state.borrow(), e, Instant::now() > deadline))
    });
    let json = result.map_err(|d| vec![d])?;
    let mut ir: Ir = serde_json::from_str(&json).map_err(|e| err(format!("the SDK produced invalid IR: {e}")))?;
    let st = state.borrow();
    for m in &mut ir.modules {
        m.src = m.src.as_deref().map(|s| st.remap(s));
        remap_nodes(&st, &mut m.nodes);
    }
    Ok(Build { ir, sites: st.sites.clone() })
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

/// Turn a JavaScript error into a diagnostic pointing at the user's code.
fn caught(st: &State, e: CaughtError, timed_out: bool) -> Diagnostic {
    if timed_out {
        return Diagnostic::new("the song took too long to build (is there an endless loop?)");
    }
    match e {
        CaughtError::Exception(ex) => {
            let message = ex.message().unwrap_or_else(|| "error".into());
            let stack = ex.stack().unwrap_or_default();
            let frame = stack.lines().filter_map(frame_at).find(|at| at.starts_with(&st.project.to_string_lossy().to_string()));
            let src = frame.or_else(|| stack.lines().filter_map(frame_at).find(|at| !at.starts_with(CORE))).map(|at| st.remap(&at));
            Diagnostic { message, src, ..Default::default() }
        }
        CaughtError::Value(v) => Diagnostic::new(format!("the song threw {v:?}")),
        CaughtError::Error(e) => Diagnostic::new(e.to_string()),
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
