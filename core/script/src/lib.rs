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

pub mod component;
pub mod sites;

pub use component::{Catalog, Descriptor, UnitCtor};

use m2_ir::{Body, Diagnostic, Ir};
use oxc::allocator::Allocator;
use oxc::allocator::TakeIn;
use oxc::ast::ast::{
    AssignmentExpression, AssignmentOperator, AssignmentTarget, BinaryExpression, BinaryOperator, CallExpression, Expression,
    ImportDeclarationSpecifier, Statement,
};
use oxc::ast::builder::AstBuilder;
use oxc::ast_visit::{VisitMut, walk_mut};
use oxc::codegen::{Codegen, CodegenOptions};
use oxc::parser::Parser;
use oxc::semantic::SemanticBuilder;
use oxc::span::{SPAN, SourceType};
use oxc::transformer::{TransformOptions, Transformer};
use rquickjs::context::EvalOptions;
use rquickjs::function::{Opt, Rest};
use rquickjs::loader::{ImportAttributes, Loader, Resolver};
use rquickjs::{CatchResultExt, CaughtError, Coerced, Context, Ctx, Function, Module, Object, Runtime};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The `@m2/core` SDK, shipped inside the core.
pub const SDK: &str = include_str!("../sdk/core.ts");

/// The version of `@m2/core` that [`SDK`] is. A project's `[dependencies] "@m2/core"`
/// requirement is checked against it (FMT-2, FMT-3). From 2.0.0 it is the pack contract: a change
/// that breaks a pack's code is a new major (PACK-22).
pub const SDK_VERSION: &str = "2.0.0";

const CORE: &str = "@m2/core";

/// Project modules are named by their snapshot path behind this prefix, so a project file
/// is never confused with a package file or the SDK. A package file is named as people see it,
/// `std/src/dsp.ts`, never by its absolute path: QuickJS cuts a module's name to 63 bytes in its
/// messages, which would lose the file in a long checkout path.
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

/// Runs before anything else, once `Math` is on libm: remove what would make a build
/// non-deterministic. `__m2s` records the site of the call about to run and what its arguments
/// end with (see `sites`): `ends` is 0 for its last argument, 1 for none and 2 for a trailing
/// spread's elements, kept as an array so the SDK can compare them without iterating the spread
/// twice. `__m2_pow` is `**` as JS defines it, which song code's `**` is lowered to (see `strip`):
/// each side converted once (`-(-x)` is JS's ToNumeric), numbers through libm's `Math.pow`, BigInts
/// through QuickJS's own `**`, which is exact and never calls the C library. Code made from text
/// (`eval`, the `Function` constructors) would skip that lowering, so song code can't make any.
const PRELUDE: &str = r#"
globalThis.__m2_site = -1;
globalThis.__m2s = function (site, value, ends) {
  if (ends === 2 && !Array.isArray(value)) value = [...value];
  globalThis.__m2_site = site;
  globalThis.__m2_ends = ends === undefined ? 0 : ends;
  globalThis.__m2_last = value;
  return value;
};
delete globalThis.Date;
delete globalThis.performance;
// When garbage is collected depends on the bytes the OS's malloc counts, so a song must not see it.
delete globalThis.WeakRef;
delete globalThis.FinalizationRegistry;
Math.random = () => { throw new Error("Math.random isn't available: songs must render the same every time. Use a seeded generator."); };
{
  const pow = Math.pow;
  globalThis.__m2_pow = (a, b) => {
    a = -(-a);
    b = -(-b);
    return typeof a === "bigint" || typeof b === "bigint" ? a ** b : pow(a, b);
  };
  const refuse = (what) => function () {
    throw new Error(`${what} isn't available: songs run only the code in their files, so they render the same on every computer. Write the code in a file instead.`);
  };
  globalThis.eval = refuse("eval");
  // Each kind of function reaches its constructor as `.constructor`. The refusal keeps the
  // constructor's name and prototype, so `instanceof Function` still works.
  for (const f of [function () {}, async function () {}, function* () {}, async function* () {}]) {
    const proto = Object.getPrototypeOf(f);
    const name = proto.constructor.name;
    const no = refuse(`The ${name} constructor`);
    Object.defineProperty(no, "name", { value: name });
    no.prototype = proto;
    Object.defineProperty(proto, "constructor", { value: no });
  }
  globalThis.Function = Function.prototype.constructor;
}
"#;

/// The prelude's name in stack frames: an error it throws is placed at the code that called it.
const PRELUDE_FILE: &str = "<prelude>";

/// A package module made from data (PACK-13): the TypeScript of a pack's `instruments/<id>.toml`,
/// laid out on the file's own lines, and the line of each of its regions (from 1), so a mistake
/// `@m2/core` finds in region 3 is placed at region 3's line.
pub struct DataModule {
    pub code: String,
    pub regions: Vec<u32>,
}

/// Why a data file makes no module: in words, at its line and column (from 1).
pub struct DataProblem {
    pub message: String,
    pub line: u32,
    pub col: u32,
}

/// What reads a pack's instruments written as data (PACK-13). The build has no TOML reader of its
/// own, and makes no name in code of its own; `m2_session` gives it both.
#[derive(Clone, Copy)]
pub struct DataReader {
    /// The module of one `instruments/<id>.toml`: given the package's name, the id (the file's
    /// name without `.toml`) and the file's text.
    pub module: fn(package: &str, id: &str, text: &str) -> Result<DataModule, DataProblem>,
    /// The name an instrument is exported by, given its id, or why the file's name can't be an
    /// id: asked before any code is made from the name.
    pub binding: fn(id: &str) -> Result<String, String>,
}

/// Where a pack's instruments written as data are, and the module that exports them (PACK-13).
const DATA: &str = "instruments";

/// A package's kernels' source, at its top: no part of the pack, never installed, so no import
/// reaches it (PACK-20). `m2_song::pack::KERNELS_SRC`, and its words, `NOT_IN_KERNELS_SRC` (this crate
/// doesn't depend on m2-song; a test in m2-session holds them equal).
pub const KERNELS_SRC: &str = "kernels-src";
pub const NOT_IN_KERNELS_SRC: &str =
    "kernels-src/ is no part of the pack (the source of its kernels, never installed): put the built wasm in kernels/";

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
    /// The CPU time a build may use before it is stopped as an endless loop: the build thread's
    /// own work, not the time it waited for a CPU; by the clock, ten times this at most (see
    /// [`Limit`]).
    pub time_limit: Duration,
    /// What reads the packs' instruments written as data; without one, importing them is an error.
    pub data: Option<DataReader>,
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
    /// The descriptor of every component the modules this build loaded declare, which covers
    /// every ComponentId a frame names (PKG-9, PKG-11).
    pub components: Vec<Descriptor>,
    /// The unit constructors of `@m2/core`, generated from its table: the one copy editors
    /// and gestures use (PKG-11).
    pub units: Vec<UnitCtor>,
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
            data: None,
        }
    }
}

/// Maps positions in generated JavaScript back to the TypeScript source.
#[derive(Clone, Default)]
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
    /// Every package file read, by the path it was read from, with the SHA-256 of its bytes.
    read: Vec<(PathBuf, String)>,
    /// Why an import failed, with its code: the engine passes on only the text.
    failed: Option<Diagnostic>,
    /// What reads the packs' instruments written as data (PACK-13).
    data: Option<DataReader>,
    /// The line of each region of each data module loaded, by the module's name.
    data_regions: HashMap<String, Vec<u32>>,
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
        // A data module's columns are its code's, not its file's (PACK-13).
        let col = if self.data_regions.contains_key(file) { 1 } else { col };
        format!("{}:{line}:{col}", self.display(file))
    }

    /// `file:line:col` of a site's start.
    fn site_src(&self, site: u32) -> Option<String> {
        let s = self.sites.iter().find(|s| s.id == site)?;
        let f = self.files.values().find(|f| f.instrumented.as_ref().is_some_and(|i| i.sites.iter().any(|x| x.id == site)))?;
        let (l, c) = line_col(&f.original, s.start as usize);
        let c = if self.data_regions.contains_key(&s.file) { 1 } else { c };
        Some(format!("{}:{l}:{c}", s.file))
    }

    /// QuickJS's error for an import of a name its module doesn't export (`Could not find export
    /// 'bar' in module './lib/a.ts'`, or the circular and ambiguous forms) cuts the module's name
    /// to 63 bytes and says nothing of where the import is. This one names the module as people
    /// see it, at the first import of that name from it (files in bytewise order: the project's
    /// first).
    fn export_error(&self, text: &str) -> Option<Diagnostic> {
        let (head, rest) = text.split_once(" in module '")?;
        let (cut, tail) = rest.rsplit_once('\'')?;
        let name = head.split_once('\'')?.1.strip_suffix('\'')?;
        let mut files: Vec<&String> = self.files.keys().collect();
        files.sort();
        let longer: Vec<&&String> = files.iter().filter(|f| f.starts_with(cut)).collect();
        let module = files.iter().find(|f| f.as_str() == cut).or((longer.len() == 1).then(|| longer[0]))?;
        let at = files.iter().find_map(|file| self.import_at(file, name, module));
        // One of std/fx's effects that moved to a pack (PKG-17) says which.
        if let Some(why) =
            self.display(module).strip_prefix("std/src/").and_then(|m| m.strip_suffix(".ts")).and_then(|m| moved_from_std(m, Some(name)))
        {
            return Some(Diagnostic::error("pkg.missing", why).at(at));
        }
        Some(Diagnostic::error("code.build", format!("{head} in module '{}'{tail}", self.display(module))).at(at))
    }

    /// One of std/fx's effects that moved to a pack (PKG-17), called through a namespace import
    /// (`import * as fx from "std/fx"`, then `fx.reverb(…)`), where a "not a function" was thrown at
    /// `at` (a frame: its file's key, line and column): the words a named import gets, at the name.
    fn moved_through_namespace(&self, at: &str) -> Option<Diagnostic> {
        let file = at.rsplitn(3, ':').nth(2)?;
        let source = &self.files.get(file)?.original;
        let line: usize = self.remap(at).rsplit(':').nth(1)?.parse().ok()?;
        let text = source.lines().nth(line.checked_sub(1)?)?;
        let allocator = Allocator::default();
        let program = Parser::new(&allocator, source, SourceType::ts()).parse().program;
        let spaces: Vec<String> = program
            .body
            .iter()
            .filter_map(|s| match s {
                Statement::ImportDeclaration(d) if d.source.value == "std/fx" => d.specifiers.as_ref(),
                _ => None,
            })
            .flatten()
            .filter_map(|s| match s {
                ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => Some(s.local.name.to_string()),
                _ => None,
            })
            .collect();
        let (_, names, _) = MOVED.iter().find(|(m, ..)| *m == "fx")?;
        let ident = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
        for ns in &spaces {
            for name in *names {
                let call = format!("{ns}.{name}");
                for (i, _) in text.match_indices(&call) {
                    let alone = !text[..i].ends_with(ident) && !text[i + call.len()..].starts_with(ident);
                    if alone {
                        let col = text[..i + ns.len() + 1].chars().count() + 1;
                        let why = moved_from_std("fx", Some(name))?;
                        return Some(Diagnostic::error("pkg.missing", why).at(Some(format!("{}:{line}:{col}", self.display(file)))));
                    }
                }
            }
        }
        None
    }

    /// `file:line:col` of the first import or re-export in `file` that takes `name` from `module`
    /// and that the build keeps: stripping TypeScript drops one that only types use, which never fails.
    fn import_at(&self, file: &str, name: &str, module: &str) -> Option<String> {
        let source = &self.files.get(file)?.original;
        let allocator = Allocator::default();
        let mut program = Parser::new(&allocator, source, SourceType::ts()).parse().program;
        let scoping = SemanticBuilder::new().build(&program).semantic.into_scoping();
        Transformer::new(&allocator, Path::new(file), &TransformOptions::default()).build_with_scoping(scoping, &mut program);
        for statement in &program.body {
            let (from, at) = match statement {
                Statement::ImportDeclaration(d) => {
                    let at = d.specifiers.iter().flatten().find_map(|s| match s {
                        ImportDeclarationSpecifier::ImportSpecifier(s) if s.imported.name() == name => Some(s.span),
                        ImportDeclarationSpecifier::ImportDefaultSpecifier(s) if name == "default" => Some(s.span),
                        _ => None,
                    });
                    (d.source.value, at)
                }
                Statement::ExportFromDeclaration(d) => {
                    let at = d.specifiers.iter().find(|s| s.local.name() == name).map(|s| s.span);
                    (d.source.value, at)
                }
                _ => continue,
            };
            // A specifier with a lone surrogate names no file, so it never takes from `module`.
            if let Some(at) = at
                && let Some(from) = from.as_str()
                && self.resolve(file, from).is_ok_and(|m| m == module)
            {
                let (l, c) = line_col(source, at.start as usize);
                return Some(format!("{}:{l}:{c}", self.display(file)));
            }
        }
        None
    }

    /// The package a module file belongs to, as `@m2/core`'s `component()` asks for it
    /// (`__m2_home`): `{"package": "std", "module": "std/fx", "file": "std/src/fx.ts"}`, or
    /// `{"package": null, "module": "lib/fx.ts", "file": "lib/fx.ts"}` for a project file.
    fn home(&self, file: &str) -> serde_json::Value {
        let display = self.display(file);
        if file.starts_with(PROJECT) || file == "<main>" {
            return serde_json::json!({"package": null, "module": display, "file": display});
        }
        for (name, _) in &self.packages {
            if display == format!("{name}/{DATA}") || display.strip_prefix(&format!("{name}/{DATA}/")).is_some_and(|f| f.ends_with(".toml"))
            {
                return serde_json::json!({"package": name, "module": format!("{name}/{DATA}"), "file": display});
            }
            if let Some(sub) = display.strip_prefix(&format!("{name}/src/")) {
                let sub = sub.strip_suffix(".ts").unwrap_or(sub);
                let sub = sub.strip_suffix("/index").or_else(|| (sub == "index").then_some("")).unwrap_or(sub);
                let module = if sub.is_empty() { name.clone() } else { format!("{name}/{sub}") };
                return serde_json::json!({"package": name, "module": module, "file": display});
            }
        }
        serde_json::json!({"package": null, "module": display, "file": display})
    }

    /// A module or a file as people see it: `tracks/bass.ts`, `std/src/dsp.ts`. A package
    /// module is named so already; a path on disk is shortened to its package's name.
    fn display(&self, file: &str) -> String {
        if let Some(rel) = file.strip_prefix(PROJECT) {
            return rel.to_string();
        }
        let p = Path::new(file);
        for (name, dir) in &self.packages {
            if let Ok(rel) = p.strip_prefix(dir) {
                // `/` on every OS: the name is a module's, and home() reads it.
                let parts: Vec<_> = rel.components().map(|c| c.as_os_str().to_string_lossy()).collect();
                return format!("{name}/{}", parts.join("/"));
            }
        }
        file.to_string()
    }

    /// The file on disk that a package module (`std/src/dsp.ts`) names.
    fn package_path(&self, module: &str) -> Option<PathBuf> {
        self.package_module(module).map(|(_, file)| file)
    }

    /// The folder of the package a package module (`std/src/dsp.ts`) is in, and its file there.
    fn package_module(&self, module: &str) -> Option<(&Path, PathBuf)> {
        self.packages.iter().find_map(|(name, dir)| Some((dir.as_path(), dir.join(module.strip_prefix(name.as_str())?.strip_prefix('/')?))))
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
        // Inside one package's folder (PACK-3): a relative import's own module's package, a bare
        // import's named one.
        let found = if relative {
            match self.package_module(base) {
                Some((own, file)) => {
                    let at = file.parent().map(Path::to_path_buf).unwrap_or_default();
                    self.package_file(own, &normalize(&at.join(name)))
                }
                None => Err(("import.outside-code", "it names no file of an installed package".into())),
            }
        } else {
            let (pkg, sub) = package_of(name);
            if in_project && !self.dependencies.iter().any(|d| d == pkg) {
                let e = format!("`{name}` is from the package `{pkg}`, which music2.toml doesn't list: add it under [dependencies]");
                return Err(fail("pkg.undeclared", e));
            }
            match self.packages.iter().find(|(n, _)| n == pkg) {
                // The pack's instruments written as data, unless its code has that module (PACK-13).
                Some((_, dir)) if sub == DATA && !data_files(dir).is_empty() => {
                    match self.package_file(dir, &normalize(&dir.join("src").join(sub))) {
                        Err((_, m)) if m == NOT_FOUND => Ok(format!("{pkg}/{DATA}")),
                        found => found,
                    }
                }
                Some((_, dir)) => match self.package_file(dir, &normalize(&dir.join("src").join(sub))) {
                    // A std module that moved to a pack (PKG-17) says which.
                    Err((_, m))
                        if m == NOT_FOUND
                            && pkg == "std"
                            && let Some(why) = moved_from_std(sub, None) =>
                    {
                        return Err(fail("pkg.missing", why));
                    }
                    found => found,
                },
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

    /// The module of a file inside the installed package in `dir`: the path, then with `.ts`, then
    /// `/index.ts`, spelled exactly as in its folder's listing, so every disk finds the same
    /// files. One outside `dir` (another package's folder too: PACK-3), or reached through a
    /// symbolic link (PACK-5), is no file it may import. An error comes with its code.
    fn package_file(&self, dir: &Path, p: &Path) -> Result<String, (&'static str, String)> {
        if !p.starts_with(dir) {
            // Short: QuickJS cuts a resolving error's message, and a cut one loses its code.
            return Err((
                "import.outside-code",
                "it leaves its package (songs import from their own project or from installed packages)".into(),
            ));
        }
        let tried = [p.to_path_buf(), PathBuf::from(format!("{}.ts", p.display())), p.join("index.ts")];
        let listed: Vec<(&PathBuf, PathBuf)> = tried.iter().filter_map(|c| Some((c, listed(dir, c)?))).collect();
        if let Some((c, real)) = listed.iter().find(|(c, real)| *c == real) {
            if through_link(dir, real) {
                return Err(("import.outside-code", "it is reached through a symbolic link, and a package's file never is".into()));
            }
            // A hidden file is no part of a package (its listing leaves it out, PACK-6).
            if real.strip_prefix(dir).is_ok_and(|rel| rel.components().any(|c| c.as_os_str().to_string_lossy().starts_with('.'))) {
                return Err(("import.outside-code", "it is a hidden file, and no part of its package".into()));
            }
            // Nor is its kernels' source, which is never installed (PACK-20).
            if real.strip_prefix(dir).is_ok_and(|rel| rel.components().next().is_some_and(|c| c.as_os_str() == KERNELS_SRC)) {
                return Err(("import.outside-code", NOT_IN_KERNELS_SRC.into()));
            }
            return Ok(self.display(&c.to_string_lossy()));
        }
        match listed.first() {
            Some((_, real)) => Err((
                "import.outside-code",
                format!("must be spelled as the file is named, `{}`, with the same capitals", self.display(&real.to_string_lossy())),
            )),
            None => Err(("code.build", NOT_FOUND.into())),
        }
    }

    /// A mistake `@m2/core` found in "region N" of a data module the stack passes through, placed
    /// at that region's line in its file (PACK-13): the file is what to fix, not the call in the song.
    fn region_at(&self, stack: &str, message: &str) -> Option<String> {
        let n: usize = message.split("region ").skip(1).find_map(|r| r.split(|c: char| !c.is_ascii_digit()).next()?.parse().ok())?;
        stack.lines().filter_map(frame_at).find_map(|at| {
            let (file, _) = at.rsplitn(3, ':').nth(2).zip(Some(()))?;
            let line = self.data_regions.get(file)?.get(n.checked_sub(1)?)?;
            Some(format!("{file}:{line}:1"))
        })
    }

    /// The module of a pack's instruments written as data, when `name` is one (PACK-13): the
    /// package's folder, and the file's id for one instrument (`None` for `<package>/instruments`,
    /// which exports them all).
    fn data_module(&self, name: &str) -> Option<(&str, &Path, Option<String>)> {
        self.packages.iter().find_map(|(pkg, dir)| {
            let rest = name.strip_prefix(pkg.as_str())?.strip_prefix('/')?.strip_prefix(DATA)?;
            if rest.is_empty() {
                return Some((pkg.as_str(), dir.as_path(), None));
            }
            let id = rest.strip_prefix('/')?.strip_suffix(".toml")?;
            (!id.contains('/')).then(|| (pkg.as_str(), dir.as_path(), Some(id.to_string())))
        })
    }
}

/// What a package file not there is called: a build error like any other.
const NOT_FOUND: &str = "file not found";

/// What moved out of std into the packs music2 ships, in std 0.7 (PKG-17): a std module, whole or
/// only the names listed, and the pack it is in now. The one table: an import of one says where.
pub const MOVED: &[(&str, &[&str], &str)] = &[
    ("instruments", &[], "@m2/synths"),
    ("synth", &[], "@m2/synths"),
    ("fm", &[], "@m2/synths"),
    ("drums", &[], "@m2/drums"),
    ("fx", &["bitcrush", "chorus", "delay", "drive", "phaser", "reverb", "tremolo", "widen"], "@m2/effects"),
];

/// Why an import of `std/<sub>`, or of `name` from it, finds nothing, when what it names moved to
/// a pack (PKG-17): the pack, what to import, and the line `[dependencies]` needs.
fn moved_from_std(sub: &str, name: Option<&str>) -> Option<String> {
    let (_, names, pack) =
        MOVED.iter().find(|(m, names, _)| *m == sub && name.is_none_or(|n| names.contains(&n)) && name.is_some() != names.is_empty())?;
    let what = match name {
        Some(n) => format!("`{n}` is no longer in std/{sub}: since std 0.7 it is in the pack {pack}"),
        None => format!("`std/{sub}` is no longer std's: since std 0.7 its sounds are in the pack {pack}"),
    };
    let it = if names.is_empty() { "them" } else { "it" };
    let mono = if sub == "synth" && name.is_none() { " (`mono` and `Voice` stay std's, in std/dsp)" } else { "" };
    Some(format!(
        "{what}, which comes with music2. Import {it} from \"{pack}\"{mono}, and add \"{pack}\" = \"1\" under [dependencies] in music2.toml"
    ))
}

/// The module `name` (`<package>/instruments`): `export { <binding> } from "./instruments/<file>";`
/// for each of `files`, its binding asked of `binding` before any code is made (PACK-13). A file
/// whose name isn't an id, or whose binding an earlier file has, is refused at its file:
/// `(file:1:1, why)`.
fn data_exports(name: &str, files: &[String], binding: fn(&str) -> Result<String, String>) -> Result<String, (String, String)> {
    let mut taken: BTreeMap<String, &str> = BTreeMap::new();
    let mut code = String::new();
    for f in files {
        let at = format!("{name}/{f}");
        let b = binding(f.strip_suffix(".toml").unwrap_or(f)).map_err(|why| (format!("{at}:1:1"), why))?;
        let plain =
            b.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') && b.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !plain {
            return Err((format!("{at}:1:1"), format!("`{b}` can't be a name in code")));
        }
        if let Some(other) = taken.insert(b.clone(), f) {
            return Err((
                format!("{at}:1:1"),
                format!("`{name}/{other}` and `{at}` would both be exported as `{b}`, so both are refused: rename one of them"),
            ));
        }
        code.push_str(&format!("export {{ {b} }} from {};\n", serde_json::to_string(&format!("./{DATA}/{f}")).expect("a string")));
    }
    Ok(code)
}

/// A package's instruments written as data (PACK-13): the `.toml` files right in its `instruments/`
/// folder, by name in bytewise order, never through a link and never a hidden one.
pub fn data_files(dir: &Path) -> Vec<String> {
    let folder = dir.join(DATA);
    if !std::fs::symlink_metadata(&folder).is_ok_and(|m| m.is_dir()) {
        return vec![];
    }
    let mut names: Vec<String> = std::fs::read_dir(&folder)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| e.file_name().to_str().map(String::from))
        .filter(|n| n.ends_with(".toml") && !n.starts_with('.'))
        .collect();
    names.sort();
    names
}

/// A bare import's package and the module in it (PACK-3): its first segment, or its first two when
/// it starts with `@`, and `index` when it names nothing more. `"std/dsp"` is std's `dsp`,
/// `"@alice/felt-piano/keys"` is `@alice/felt-piano`'s `keys`.
pub fn package_of(spec: &str) -> (&str, &str) {
    let cut = match spec.starts_with('@') {
        true => spec.match_indices('/').nth(1).map(|(i, _)| i),
        false => spec.find('/'),
    };
    match cut {
        Some(i) => (&spec[..i], &spec[i + 1..]),
        None => (spec, "index"),
    }
}

/// Whether any part of `path` below `root` is a symbolic link.
fn through_link(root: &Path, path: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(root) else { return true };
    let mut at = root.to_path_buf();
    rel.components().any(|c| {
        at.push(c);
        std::fs::symlink_metadata(&at).is_ok_and(|m| m.file_type().is_symlink())
    })
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

/// The SDK stripped of its TypeScript, once per process: every build and describe run loads it.
fn sdk_js() -> (String, LineMap) {
    static JS: std::sync::OnceLock<(String, LineMap)> = std::sync::OnceLock::new();
    JS.get_or_init(|| strip(CORE, SDK).expect("the SDK is valid TypeScript")).clone()
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
    // oxc lowers `**=` before it strips the types around its target: `x! **= y` would be left to
    // QuickJS's own `**`.
    Unwrap(&allocator).visit_program(&mut program);
    let scoping = SemanticBuilder::new().build(&program).semantic.into_scoping();
    // `**` becomes `__m2_pow`, which computes numbers with libm like the rest of `Math` (PLAT-4);
    // QuickJS's own `**` calls the OS's C library. oxc lowers it to `Math.pow`, which `Pow` renames.
    let mut options = TransformOptions::default();
    options.env.es2016.exponentiation_operator = true;
    let tr = Transformer::new(&allocator, Path::new(path), &options).build_with_scoping(scoping, &mut program);
    if let Some(e) = tr.diagnostics.first() {
        return Err(format!("{path}: {}", e.message));
    }
    let mut pow = Pow { ast: AstBuilder::new(&allocator), left: None };
    pow.visit_program(&mut program);
    if let Some(at) = pow.left {
        // Every `**` on numbers is lowered: one that isn't would use the OS's `pow`.
        let (line, _) = line_col(source, at as usize);
        return Err(format!("{path}:{line}: this `**` can't be made to compute alike on every computer: write Math.pow(a, b)"));
    }
    let out =
        Codegen::new().with_options(CodegenOptions { source_map_path: Some(PathBuf::from(path)), ..Default::default() }).build(&program);
    let mut map = LineMap::default();
    if let Some(sm) = &out.map {
        map.tokens = sm.get_tokens().map(|t| (t.get_dst_line(), t.get_dst_col(), t.get_src_line(), t.get_src_col())).collect();
        map.tokens.sort_unstable();
    }
    Ok((out.code, map))
}

/// Points each `Math.pow` call that oxc's `**` lowering made at the prelude's `__m2_pow`, which
/// takes BigInts too, as `**` does, and which no `Math` the song declares can stand in for. The
/// lowering's `Math` and `pow` have no place in the source; a `Math.pow` the code wrote has. `left`
/// is where a `**` on something other than a BigInt literal was left unlowered, if one was.
struct Pow<'a> {
    ast: AstBuilder<'a>,
    left: Option<u32>,
}

impl<'a> VisitMut<'a> for Pow<'a> {
    fn visit_call_expression(&mut self, call: &mut CallExpression<'a>) {
        if let Expression::StaticMemberExpression(m) = &call.callee
            && let Expression::Identifier(math) = &m.object
            && (math.name == "Math" && math.span.is_empty() && m.property.name == "pow" && m.property.span.is_empty())
        {
            call.callee = Expression::new_identifier(SPAN, "__m2_pow", &self.ast);
        }
        walk_mut::walk_call_expression(self, call);
    }

    // oxc leaves a `**` with a BigInt literal to QuickJS, whose BigInt `**` is exact.
    fn visit_binary_expression(&mut self, b: &mut BinaryExpression<'a>) {
        if b.operator == BinaryOperator::Exponential && !b.left.is_big_int_literal() && !b.right.is_big_int_literal() {
            self.left = self.left.or(Some(b.span.start));
        }
        walk_mut::walk_binary_expression(self, b);
    }

    fn visit_assignment_expression(&mut self, a: &mut AssignmentExpression<'a>) {
        if a.operator == AssignmentOperator::Exponential && !a.right.is_big_int_literal() {
            self.left = self.left.or(Some(a.span.start));
        }
        walk_mut::walk_assignment_expression(self, a);
    }
}

/// Takes the types off the target of each `**=` (`x! **= y`, `(x as number) **= y`), as the
/// TypeScript strip would, before oxc lowers `**`: it lowers only plain names and members.
struct Unwrap<'a>(&'a Allocator);

impl<'a> VisitMut<'a> for Unwrap<'a> {
    fn visit_assignment_expression(&mut self, a: &mut AssignmentExpression<'a>) {
        if a.operator == AssignmentOperator::Exponential
            && let Some(wrapped) = a.left.get_expression_mut()
        {
            let inner = wrapped.get_inner_expression_mut();
            if let Expression::Identifier(_) = inner {
                let Expression::Identifier(name) = inner.take_in(&self.0) else { unreachable!() };
                a.left = AssignmentTarget::AssignmentTargetIdentifier(name);
            } else if inner.is_member_expression() {
                a.left = AssignmentTarget::from(inner.take_in(&self.0).into_member_expression());
            }
        }
        walk_mut::walk_assignment_expression(self, a);
    }
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
    fn load<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        name: &str,
        _: Option<ImportAttributes<'js>>,
    ) -> rquickjs::Result<Module<'js, rquickjs::module::Declared>> {
        let fail = |e: String| rquickjs::Error::new_loading_message(name, e);
        let mut st = self.0.borrow_mut();
        let (original, instrumented) = if name == CORE {
            (SDK.to_string(), None)
        } else {
            let data = st.data_module(name).map(|(pkg, dir, id)| (pkg.to_string(), dir.to_path_buf(), id));
            let source = match (name.strip_prefix(PROJECT), data) {
                (Some(rel), _) => st.code.get(rel).cloned().ok_or_else(|| fail("not a code file of the project".into()))?,
                // `<package>/instruments`: each data instrument by its name in code, in bytewise order
                // of the files. What the folder lists is recorded, so a describe run sees an
                // instrument added (PACK-13).
                (None, Some((_, dir, None))) => {
                    let files = data_files(&dir);
                    st.read.push((dir.join(DATA), sha256_hex(files.join("\n").as_bytes())));
                    let Some(reader) = st.data else {
                        let e = format!("`{name}` is instruments written as data, which this program can't read");
                        st.failed.get_or_insert(Diagnostic::error("code.build", e.clone()).at(Some(name.to_string())));
                        return Err(fail(e));
                    };
                    match data_exports(name, &files, reader.binding) {
                        Ok(code) => code,
                        Err((at, e)) => {
                            st.failed.get_or_insert(Diagnostic::error("code.build", e.clone()).at(Some(at)));
                            return Err(fail(e));
                        }
                    }
                }
                (None, data) => {
                    let file = st.package_path(name).ok_or_else(|| fail("not a file of an installed package".into()))?;
                    let bytes = std::fs::read(&file).map_err(|e| fail(e.to_string()))?;
                    let sha256 = sha256_hex(&bytes);
                    st.read.push((file, sha256.clone()));
                    st.package_files.push(PackageFile { path: name.to_string(), sha256 });
                    let text = String::from_utf8(bytes).map_err(|e| fail(e.to_string()))?;
                    match data {
                        Some((pkg, _, Some(id))) => {
                            let Some(reader) = st.data else {
                                let e = format!("`{name}` is an instrument written as data, which this program can't read");
                                st.failed.get_or_insert(Diagnostic::error("code.build", e.clone()).at(Some(name.to_string())));
                                return Err(fail(e));
                            };
                            match (reader.module)(&pkg, &id, &text) {
                                Ok(m) => {
                                    st.data_regions.insert(name.to_string(), m.regions);
                                    m.code
                                }
                                Err(p) => {
                                    let at = format!("{name}:{}:{}", p.line, p.col);
                                    st.failed.get_or_insert(Diagnostic::error("code.build", p.message.clone()).at(Some(at.clone())));
                                    return Err(fail(format!("{at}: {}", p.message)));
                                }
                            }
                        }
                        _ => text,
                    }
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
        let (js, map) = if name == CORE { sdk_js() } else { strip(name, code).map_err(fail)? };
        st.files.insert(name.to_string(), LoadedFile { original, instrumented, map });
        drop(st);
        let module = Module::declare(ctx.clone(), name, js)?;
        // `import.meta.url` is the module's name, as people see it: `@m2/core`'s `file()` reads
        // its package from it (PACK-5).
        module.meta()?.set("url", name)?;
        Ok(module)
    }
}

/// Run the song and return its IR, or what went wrong.
pub fn build(opts: &Options) -> Result<Ir, Vec<Diagnostic>> {
    build_full(opts).map(|b| b.ir)
}

/// How many times this thread has run a song's TypeScript ([`build_full`]): what shows that a
/// lane edit took the data path, which never runs QuickJS (PLANE-1, WRITE-4). Per thread, so
/// tests running side by side don't count each other's.
pub fn builds() -> u64 {
    BUILDS.with(|b| b.get())
}

thread_local! {
    static BUILDS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// The C stack each build and describe run has. QuickJS recurses in C for each call in song code,
/// so how deep a song may recurse depends on how each compiler lays out its frames (at 1 MB, about
/// 1,300 calls with gcc and 950 with clang). Only address space is reserved: pages are used as the
/// stack grows.
const STACK: usize = 32 << 20;
/// What QuickJS may use of it, about 15,000 calls or more: far deeper than a real song goes. The
/// rest is room for the host's own calls under it. rquickjs takes no more than 16 MB: a larger limit
/// means none, and endless recursion would overflow the thread.
const JS_STACK: usize = 16 << 20;

/// Run `f` on a thread of its own, with [`STACK`] and numbers read as C reads them whatever the
/// process's locale (`m2_os::thread::c_numbers`), and wait for it: the same build on every OS and
/// in every app (PLAT-4).
fn on_own_thread<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|s| {
        let run = move || {
            m2_os::thread::c_numbers();
            f()
        };
        let t = std::thread::Builder::new().name("m2-script".into()).stack_size(STACK).spawn_scoped(s, run).expect("a thread to build on");
        t.join().unwrap_or_else(|e| std::panic::resume_unwind(e))
    })
}

/// Like [`build`], also returning the call sites the IR's origins refer to.
pub fn build_full(opts: &Options) -> Result<Build, Vec<Diagnostic>> {
    BUILDS.with(|b| b.set(b.get() + 1));
    on_own_thread(|| build_here(opts))
}

/// [`build_full`], on this thread.
fn build_here(opts: &Options) -> Result<Build, Vec<Diagnostic>> {
    let err = |m: String| vec![Diagnostic::error("code.build", m)];
    if !opts.code.contains_key(&opts.entry) {
        return Err(err(format!("the entry `{}` is not a code file of the project", opts.entry)));
    }
    let packages: Vec<(String, PathBuf)> = opts.packages.iter().filter_map(|(n, d)| Some((n.clone(), d.canonicalize().ok()?))).collect();
    let state = Rc::new(RefCell::new(State {
        code: opts.code.clone(),
        packages: packages.clone(),
        dependencies: opts.dependencies.clone(),
        data: opts.data,
        ..Default::default()
    }));

    let (_rt, ctx, limit) = sandbox(&state, opts.memory_limit, opts.time_limit).map_err(|e| err(e.to_string()))?;

    let result: Result<[String; 4], Vec<Diagnostic>> = ctx.with(|ctx| {
        let run = || -> rquickjs::Result<[String; 4]> {
            host(&ctx, &state, &packages)?;
            let g = ctx.globals();
            let entry = serde_json::to_string(&format!("{PROJECT}{}", opts.entry)).expect("string");
            let main = format!("import {{ __build }} from \"{CORE}\";\nimport song from {entry};\nglobalThis.__m2_out = __build(song);\n");
            Module::evaluate(ctx.clone(), "<main>", main)?.finish::<()>()?;
            Ok([g.get("__m2_out")?, g.get("__m2_outputs")?, g.get("__m2_components")?, g.get("__m2_units")?])
        };
        run().catch(&ctx).map_err(|e| vec![caught(&mut state.borrow_mut(), e, limit.passed())])
    });
    let [json, outputs, components, units] = result?;
    let outputs: Vec<Output> = serde_json::from_str(&outputs).map_err(|e| err(format!("the SDK produced invalid outputs: {e}")))?;
    let components: Vec<Descriptor> =
        serde_json::from_str(&components).map_err(|e| err(format!("the SDK produced invalid descriptors: {e}")))?;
    let units: Vec<UnitCtor> = serde_json::from_str(&units).map_err(|e| err(format!("the SDK produced an invalid unit table: {e}")))?;
    let mut ir: Ir = serde_json::from_str(&json).map_err(|e| err(format!("the SDK produced invalid IR: {e}")))?;
    let st = state.borrow();
    for m in &mut ir.modules {
        m.src = m.src.as_deref().map(|s| st.remap(s));
        remap_nodes(&st, &mut m.nodes);
    }
    Ok(Build { ir, sites: st.sites.clone(), outputs, package_files: st.package_files.clone(), components, units })
}

/// How long a run may work before it is stopped as an endless loop: CPU time of the thread it runs
/// on, from its start. Not the clock: a build at background priority on a busy machine (an agent's
/// `m2 listen` on a Mac's efficiency cores, AGENT-7) waits for a CPU far longer than it works, and
/// its wait is no loop. The thread's CPU time is read only once the clock passed the limit too (a
/// thread never works longer than the clock runs), so the many checks before cost a clock read.
/// The clock is a backstop at [`BACKSTOP`] times the limit: an endless loop that gets almost no CPU
/// still ends, and an agent's listen doesn't hold `~/.music2/listen.lock` for good.
#[derive(Clone, Copy)]
struct Limit {
    started: Instant,
    worked: Duration,
    limit: Duration,
}

/// How many times its limit a run may take by the clock, whatever CPU it got.
const BACKSTOP: u32 = 10;

impl Limit {
    /// From now, on the calling thread: the thread that runs the sandbox.
    fn new(limit: Duration) -> Limit {
        Limit { started: Instant::now(), worked: m2_os::thread::cpu_time(), limit }
    }

    /// Whether the run worked longer than its limit, or took the backstop's time by the clock:
    /// asked on its own thread.
    fn passed(&self) -> bool {
        let took = self.started.elapsed();
        took > self.limit * BACKSTOP || (took > self.limit && m2_os::thread::cpu_time().saturating_sub(self.worked) > self.limit)
    }
}

/// A sandbox for one run, on the thread that runs it: QuickJS with the prelude, the loader over
/// `state` and the limits.
fn sandbox(state: &Rc<RefCell<State>>, memory_limit: usize, time_limit: Duration) -> rquickjs::Result<(Runtime, Context, Limit)> {
    let rt = Runtime::new()?;
    rt.set_memory_limit(memory_limit);
    rt.set_max_stack_size(JS_STACK);
    let limit = Limit::new(time_limit);
    rt.set_interrupt_handler(Some(Box::new(move || limit.passed())));
    rt.set_loader(Resolve(state.clone()), Load(state.clone()));
    let ctx = Context::full(&rt)?;
    Ok((rt, ctx, limit))
}

/// Put `Math` on libm and run the prelude, then give the SDK what it asks the host while it runs:
/// where a call or a stack frame is in the TypeScript, whether a key is valid (one rule, in
/// m2_ir::key), and the package a module file belongs to.
fn host<'js>(ctx: &Ctx<'js>, state: &Rc<RefCell<State>>, packages: &[(String, PathBuf)]) -> rquickjs::Result<()> {
    math(ctx)?;
    let mut prelude = EvalOptions::default();
    prelude.filename = Some(PRELUDE_FILE.into());
    ctx.eval_with_options::<(), _>(PRELUDE, prelude)?;
    let g = ctx.globals();
    g.set("__m2_project", PROJECT)?;
    g.set("__m2_packages", packages.iter().map(|(n, _)| format!("{n}/")).collect::<Vec<_>>())?;
    let st = state.clone();
    let place = move |at: String| st.try_borrow().map_or_else(|_| at.clone(), |st| st.remap(&at));
    g.set("__m2_where", Function::new(ctx.clone(), place)?)?;
    let st = state.clone();
    g.set("__m2_site_src", Function::new(ctx.clone(), move |site: u32| st.try_borrow().ok().and_then(|st| st.site_src(site)))?)?;
    g.set("__m2_key", Function::new(ctx.clone(), |what: String, k: String| m2_ir::key::invalid(&what, &k))?)?;
    let st = state.clone();
    let home = move |file: String| st.try_borrow().map_or_else(|_| "null".to_string(), |st| st.home(&file).to_string());
    g.set("__m2_home", Function::new(ctx.clone(), home)?)?;
    Ok(())
}

/// `Math`'s transcendental functions over the `libm` crate, never the OS's C library, whose last
/// bit differs between macOS and Linux, so song code builds the same constants on every OS (PLAT-4,
/// RENDER-2). Each takes its arguments as QuickJS's own: converted to numbers, a missing one NaN,
/// extras ignored.
fn math(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    let m: Object = ctx.globals().get("Math")?;
    let num = |x: Opt<Coerced<f64>>| x.0.map_or(f64::NAN, |c| c.0);
    let one = [
        ("sin", libm::sin as fn(f64) -> f64),
        ("cos", libm::cos),
        ("tan", libm::tan),
        ("asin", libm::asin),
        ("acos", libm::acos),
        ("atan", libm::atan),
        ("sinh", libm::sinh),
        ("cosh", libm::cosh),
        ("tanh", libm::tanh),
        ("asinh", libm::asinh),
        ("acosh", libm::acosh),
        ("atanh", libm::atanh),
        ("exp", libm::exp),
        ("expm1", libm::expm1),
        ("log", libm::log),
        ("log1p", libm::log1p),
        ("log2", libm::log2),
        ("log10", libm::log10),
        ("cbrt", libm::cbrt),
    ];
    for (name, f) in one {
        set(&m, name, 1, Function::new(ctx.clone(), move |x| f(num(x)))?)?;
    }
    // JS's pow is NaN for ±1 to an infinite or NaN power, where C's gives 1 (QuickJS's js_math_pow).
    let pow = |a: f64, b: f64| if !b.is_finite() && a.abs() == 1.0 { f64::NAN } else { libm::pow(a, b) };
    let two = [("atan2", libm::atan2 as fn(f64, f64) -> f64), ("pow", pow)];
    for (name, f) in two {
        set(&m, name, 2, Function::new(ctx.clone(), move |a, b| f(num(a), num(b)))?)?;
    }
    // Folded as QuickJS folds it: |x| for one argument, else hypot(hypot(x0, x1), x2) and on.
    let hypot = |xs: Rest<Coerced<f64>>| match xs.0.split_first() {
        None => 0.0,
        Some((x, [])) => x.0.abs(),
        Some((x, rest)) => rest.iter().fold(x.0, |r, y| libm::hypot(r, y.0)),
    };
    set(&m, "hypot", 2, Function::new(ctx.clone(), hypot)?)
}

/// Put `f` on `object` as `name`, with the `name` and `length` a built-in function has.
fn set<'js>(object: &Object<'js>, name: &str, length: usize, f: Function<'js>) -> rquickjs::Result<()> {
    f.set_length(length)?;
    object.set(name, f.with_name(name)?)
}

/// Every component the packages declare, and where each is exported, for the effect browser and
/// the effect planner (PKG-2, PKG-9). Each module under a package's `src/`, in bytewise order of
/// its path, is described in its own sandboxed run, so a module that throws affects only itself
/// and the modules that import it. It never fails: a module that doesn't load, an invalid
/// descriptor or one id declared at two places is `pkg.invalid`, and those components are left
/// out, so a broken package never stops a project from opening (WRITE-20). Runs are cached for
/// the process, by the content of every file each read.
pub fn describe(packages: &[(String, PathBuf)]) -> Catalog {
    describe_with(packages, None)
}

/// [`describe`], reading the packs' instruments written as data with `data` (PACK-13): each pack's
/// `"<package>/instruments"` is one more module, unless its code has a module of that name.
pub fn describe_with(packages: &[(String, PathBuf)], data: Option<DataReader>) -> Catalog {
    let packages: Vec<(String, PathBuf)> = packages.iter().filter_map(|(n, d)| Some((n.clone(), d.canonicalize().ok()?))).collect();
    let mut runs = vec![];
    for (name, dir) in &packages {
        let src = dir.join("src");
        let mut files = vec![];
        let mut stack = vec![src.clone()];
        while let Some(d) = stack.pop() {
            // A link is never followed (PACK-5): a folder that links to one above it would never end.
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                let (path, kind) = (e.path(), e.file_type());
                if kind.as_ref().is_ok_and(|k| k.is_dir()) {
                    stack.push(path);
                } else if kind.is_ok_and(|k| k.is_file()) && path.extension().is_some_and(|x| x == "ts") {
                    files.push(path);
                }
            }
        }
        let mut subs: Vec<String> =
            files.iter().filter_map(|f| Some(f.strip_prefix(&src).ok()?.to_string_lossy().replace('\\', "/"))).collect();
        subs.sort();
        for sub in subs {
            // A package's `index` is the package itself (PACK-3): `@alice/felt-piano`, not `…/index`.
            let stem = sub.strip_suffix(".ts").unwrap_or(&sub);
            let spec = match stem.strip_suffix("/index").or_else(|| (stem == "index").then_some("")) {
                Some("") => name.clone(),
                Some(folder) => format!("{name}/{folder}"),
                None => format!("{name}/{stem}"),
            };
            let file = format!("{name}/src/{sub}");
            runs.push((file, described(&packages, &spec, data)));
        }
        let spec = format!("{name}/{DATA}");
        if !data_files(dir).is_empty()
            && !runs.iter().any(|(f, _)| f == &format!("{name}/src/{DATA}.ts") || f == &format!("{name}/src/{DATA}/index.ts"))
        {
            runs.push((spec.clone(), described(&packages, &spec, data)));
        }
    }
    merge(runs)
}

/// What one describe run found: the components it registered and where its module exports them.
#[derive(Clone, Deserialize)]
struct Described {
    components: Vec<Descriptor>,
    exports: Vec<DescribedExport>,
}

#[derive(Clone, Deserialize)]
struct DescribedExport {
    component: String,
    module: String,
    name: String,
}

type DescribeCache = HashMap<(Vec<(String, PathBuf)>, String, bool), (Vec<(PathBuf, String)>, Result<Described, Diagnostic>)>;

/// The SHA-256 of `bytes` in 64 lowercase hex digits, as a build records a package file's (FMT-6a).
fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Describe the package module `spec` (`std/fx`), or take a cached run whose files are unchanged.
// A diagnostic is 128 bytes since it carries `inner` (the studio plan's Q46); this runs once a module.
#[allow(clippy::result_large_err)]
fn described(packages: &[(String, PathBuf)], spec: &str, data: Option<DataReader>) -> Result<Described, Diagnostic> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<DescribeCache>> = std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let key = (packages.to_vec(), spec.to_string(), data.is_some());
    // A file's bytes; for a pack's `instruments/` folder, what it lists (PACK-13).
    let now = |p: &Path| match p.file_name().is_some_and(|n| n == DATA) && p.is_dir() {
        true => p.parent().map(|dir| sha256_hex(data_files(dir).join("\n").as_bytes())),
        false => std::fs::read(p).ok().map(|b| sha256_hex(&b)),
    };
    let same = |read: &[(PathBuf, String)]| read.iter().all(|(p, sha)| now(p).is_some_and(|h| h == *sha));
    if let Some((read, result)) = cache.lock().expect("the describe cache").get(&key)
        && same(read)
    {
        return result.clone();
    }
    let (read, result) = describe_run(packages, spec, data);
    cache.lock().expect("the describe cache").insert(key, (read, result.clone()));
    result
}

/// One sandboxed run that imports `spec` and asks `@m2/core` what it registered.
fn describe_run(
    packages: &[(String, PathBuf)],
    spec: &str,
    data: Option<DataReader>,
) -> (Vec<(PathBuf, String)>, Result<Described, Diagnostic>) {
    on_own_thread(|| describe_here(packages, spec, data))
}

/// [`describe_run`], on this thread.
#[allow(clippy::result_large_err)]
fn describe_here(
    packages: &[(String, PathBuf)],
    spec: &str,
    data: Option<DataReader>,
) -> (Vec<(PathBuf, String)>, Result<Described, Diagnostic>) {
    let state = Rc::new(RefCell::new(State {
        packages: packages.to_vec(),
        dependencies: packages.iter().map(|(n, _)| n.clone()).collect(),
        data,
        ..Default::default()
    }));
    let invalid = |m: String| Diagnostic::error("pkg.invalid", m);
    let (_rt, ctx, limit) = match sandbox(&state, 256 << 20, Duration::from_secs(10)) {
        Ok(s) => s,
        Err(e) => return (vec![], Err(invalid(e.to_string()))),
    };
    let result: Result<String, Diagnostic> = ctx.with(|ctx| {
        let run = || -> rquickjs::Result<String> {
            host(&ctx, &state, packages)?;
            let spec_js = serde_json::to_string(spec).expect("a string");
            let main = format!("import * as ns from {spec_js};\nimport {{ __describe }} from \"{CORE}\";\nglobalThis.__m2_out = __describe({spec_js}, ns);\n");
            Module::evaluate(ctx.clone(), "<main>", main)?.finish::<()>()?;
            ctx.globals().get("__m2_out")
        };
        run().catch(&ctx).map_err(|e| caught(&mut state.borrow_mut(), e, limit.passed()))
    });
    let read = state.borrow().read.clone();
    let result = match result {
        Ok(json) => serde_json::from_str::<Described>(&json).map_err(|e| invalid(format!("`{spec}`: the SDK described it wrongly: {e}"))),
        Err(d) => {
            let mut e = invalid(format!("the package module `{spec}` doesn't load, so its components are left out: {}", d.message));
            e.src = d.src;
            Err(e)
        }
    };
    (read, result)
}

/// One catalog from the runs: a component found by several (a module and those importing it)
/// is one, by id and definition site; one id at two sites is `pkg.invalid` and left out.
fn merge(runs: Vec<(String, Result<Described, Diagnostic>)>) -> Catalog {
    let mut catalog = Catalog::default();
    let mut found: BTreeMap<String, Vec<Descriptor>> = BTreeMap::new();
    let mut exports: BTreeMap<String, Vec<component::Export>> = BTreeMap::new();
    for (file, run) in runs {
        match run {
            Ok(d) => {
                for c in d.components {
                    let at = found.entry(c.id.clone()).or_default();
                    if !at.iter().any(|x| x.at == c.at) {
                        at.push(c);
                    }
                }
                for e in d.exports {
                    exports.entry(e.component).or_default().push(component::Export { module: e.module, name: e.name });
                }
            }
            Err(mut d) => {
                d.src = d.src.or(Some(file));
                catalog.problems.push(d);
            }
        }
    }
    for (id, mut at) in found {
        if at.len() > 1 {
            let places: Vec<&str> = at.iter().map(|d| d.at.as_str()).collect();
            let message = format!(
                "two components have the id `{id}`: at {}. Ids are unique in their package (PKG-9), so both are left out",
                places.join(" and ")
            );
            catalog.problems.push(Diagnostic::error("pkg.invalid", message).at(Some(places[0].to_string())));
            continue;
        }
        let d = at.remove(0);
        let mut e = exports.remove(&id).unwrap_or_default();
        // First where code imports it from: its package's own name when that exports it (a pack's
        // `src/index.ts`, PACK-3: `import { synth } from "@m2/synths"`), else where it is defined.
        let root = id.split_once(':').map(|(package, _)| package);
        let rank = |x: &component::Export| (Some(x.module.as_str()) != root, x.module != d.module);
        e.sort_by(|a, b| (rank(a), a).cmp(&(rank(b), b)));
        e.dedup();
        catalog.exports.insert(id.clone(), e);
        catalog.components.insert(id, std::sync::Arc::new(d));
    }
    catalog
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
    // The engine cuts a long message short, so the start of one is enough.
    let head = |m: &str| m.char_indices().nth(48).map_or(m, |(i, _)| &m[..i]).to_string();
    if let Some(d) = st.failed.take().filter(|d| text.contains(&head(&d.message))) {
        return d;
    }
    if let Some(d) = st.export_error(&text) {
        return d;
    }
    match e {
        CaughtError::Exception(ex) => {
            let message = ex.message().unwrap_or_else(|| "error".into());
            let field = |k: &str| ex.as_object().get::<_, Option<String>>(k).ok().flatten();
            let code = field("code").and_then(|c| CODES.iter().copied().find(|k| *k == c)).unwrap_or("code.build");
            let stack = ex.stack().unwrap_or_default();
            let frame = stack.lines().filter_map(frame_at).find(|at| at.starts_with(PROJECT));
            let other = || stack.lines().filter_map(frame_at).find(|at| ![CORE, "<main>", PRELUDE_FILE].iter().any(|f| at.starts_with(f)));
            if message == "not a function"
                && let Some(d) = frame.as_deref().and_then(|at| st.moved_through_namespace(at))
            {
                return d;
            }
            let src = frame.or_else(other).map(|at| st.remap(&at));
            let src = st.region_at(&stack, &message).or(src);
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
mod bus_tests;
#[cfg(test)]
mod kernel_settings_tests;
#[cfg(test)]
mod kinds_tests;
#[cfg(test)]
mod mute_tests;
#[cfg(test)]
mod poly_tests;
#[cfg(test)]
mod tests;
