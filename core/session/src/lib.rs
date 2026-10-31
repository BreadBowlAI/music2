//! Turns a project folder into something playable.
//!
//! [`compile`] is the one operation every host uses before playing or rendering: if it
//! succeeds, the song renders. It checks the manifest's dependencies, runs the TypeScript,
//! binds the hosted plugins, checks the graph, and reads every declared lane's data against its
//! declaration. What doesn't stop the song comes back as `problems` (DIAG-1): data that doesn't
//! read (the lane plays what it can), data no lane reads, a plugin that isn't installed.
//!
//! [`Session`] is the editing session built on it: a source snapshot, revisions numbered
//! `(code_rev, data_rev)`, and undo. Every write goes through the write path of step 6: `tx`
//! checks and commits verified edits (bases, the baseline, the delta and the expectations, the
//! journal and staging, recovery), `take` takes in what changed on disk and the code editor's
//! saves, `lock` keeps one writer per project, and `watch` settles outside changes. Core holds no
//! gesture (P10, WRITE-9): `crates/editor` plans each one as a Tx and asserts what it means.

pub mod forward;
pub mod history;
pub mod lock;
pub mod reference;
pub mod session;
pub mod take;
pub mod text;
pub mod texts;
pub mod tx;
pub mod unclaimed;
pub mod watch;

use std::collections::BTreeMap;
use std::sync::Arc;

use m2_ir::{Diagnostic, Flat, Ir, Severity};
use m2_runtime::PackageSet;
use m2_script::{Descriptor, UnitCtor};
use m2_song::{DataSet, Manifest, Project, Version};
use serde::Serialize;

pub use history::{Bound, End};
pub use session::{Past, Revision, Session, Source, StepError, StepName, Stepped, journal_problems};
pub use tx::{BeforeWrite, Checked, Committed, ConflictFile, Patch, Refusal, Uncommitted, Writing};
pub use unclaimed::{Seen, SeenLane};

/// Where a value lives (HOME-1): what editing it writes, and what the UI says it is. Every
/// value the editor's controls, its view and gesture leases return names its home.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Home {
    /// A number written in the code, or a declared control's default that the code leaves out:
    /// editing it writes the code.
    Code,
    /// A lane's data, which changes over time.
    Lane,
    /// A plugin's state blob.
    State,
    /// `layout.json`: shared presentation.
    Layout,
    /// `data/arrangement.json`: the tempo and the time signature (FMT-13), which the song bar
    /// edits.
    Arrangement,
    /// `.music2/`: this machine's monitoring and view state.
    Local,
}

/// What QuickJS, the binder and the checker made of a project's code: everything a revision has
/// that only the code decides (RENDER-15), shared by every revision with the same code, so a data
/// revision builds nothing (WRITE-13, PLANE-1).
pub struct Code {
    pub ir: Ir,
    pub flat: Flat,
    /// The call sites that origins in the IR refer to, shared with the past revisions that name
    /// controls by them.
    pub sites: Arc<[m2_script::sites::Site]>,
    /// What each call returned, when it was a signal.
    pub outputs: Vec<m2_script::Output>,
    /// Every package file the build read (FMT-6a, RENDER-2).
    pub package_files: Vec<m2_script::PackageFile>,
    /// The packs the build resolved (PACK-9): what it was built with, beside std.
    pub packs: Vec<m2_song::pack::Resolved>,
    /// The packages it was built with: std and one loaded version of each of its packs. Its plans
    /// and renders run this view's kernels (PACK-9); the session shows the accepted revision's.
    pub view: Arc<m2_runtime::View>,
    /// The descriptor of every component the build's modules declare, by ComponentId: what a
    /// frame's `component` names (PKG-9, PKG-11).
    pub components: BTreeMap<String, Arc<Descriptor>>,
    /// The unit constructors of `@m2/core`, as the build generated them from its table (PKG-11):
    /// the one table gestures write units with.
    pub units: Arc<[UnitCtor]>,
    /// What the code declares that reads data: lanes and stateful plugins.
    pub decls: m2_song::Decls,
    /// Build problems that don't stop the song: a plugin that isn't installed, a parameter the
    /// plugin renamed (FMT-18, FMT-18a).
    pub kept: Vec<Diagnostic>,
}

impl Code {
    /// No code: what a project whose code doesn't build opens with (WRITE-20). It plays nothing,
    /// and no renderer is ever built from it.
    pub fn none() -> Code {
        let root = m2_ir::Root { module: m2_song::ROOT.into(), export: "out".into() };
        let flat = Flat {
            nodes: vec![],
            order: vec![],
            out: m2_ir::PortRef { node: 0, port: 0 },
            out_kind: m2_ir::Kind::Audio(2),
            sidechain: vec![],
            overlays: vec![],
            muted: vec![],
            routes: vec![],
        };
        Code {
            ir: Ir { ir: m2_ir::IR_VERSION, modules: vec![], root },
            flat,
            sites: Arc::new([]),
            outputs: vec![],
            package_files: vec![],
            packs: vec![],
            view: Arc::default(),
            components: BTreeMap::new(),
            units: Arc::new([]),
            decls: m2_song::Decls::default(),
            kept: vec![],
        }
    }
}

/// A project that is ready to render: its code's build, and its data read against it.
pub struct Compiled {
    code: Arc<Code>,
    /// What the graph plays: each declared lane and plugin state, read against its declaration.
    pub data: DataSet,
    /// Everything that needs attention but doesn't stop the song (DIAG-1, FMT-9, FMT-18a), with
    /// the fixes each offers: a lane that doesn't read, clip ids, asset pins, plugin states,
    /// unknown files under `data/`, data no lane or plugin reads, a plugin that isn't
    /// installed, a parameter the plugin renamed. Errors first, then Warnings, then Notices.
    pub problems: Vec<m2_song::Diagnostic>,
}

impl std::ops::Deref for Compiled {
    type Target = Code;

    fn deref(&self) -> &Code {
        &self.code
    }
}

impl Compiled {
    /// The build this revision's data was read against: one `Arc` for every revision with the
    /// same code.
    pub fn code(&self) -> &Arc<Code> {
        &self.code
    }

    /// No code and no data (WRITE-20).
    pub fn none() -> Compiled {
        Compiled { code: Arc::new(Code::none()), data: DataSet::empty(), problems: vec![] }
    }
}

impl Clone for Compiled {
    fn clone(&self) -> Compiled {
        Compiled { code: self.code.clone(), data: self.data.clone(), problems: self.problems.clone() }
    }
}

/// Build `project` and read its data, as a host that plays or renders it once does: `packages`
/// then shows what it was built with (PACK-9), so a renderer made from them plays it.
pub fn compile(project: &Project, packages: &mut PackageSet) -> Result<Compiled, Vec<Diagnostic>> {
    let c = compile_sources(project, packages, &Seen::default(), None)?;
    packages.show(&c.view);
    Ok(c)
}

/// [`compile`], given what earlier revisions declared (`seen`, for messages about data a rename
/// left behind, see [`unclaimed::unclaimed`]) and the data last accepted (`previous`), which a
/// lane whose file fails keeps playing (FMT-9). The build reads the project's code from its
/// snapshot only; the data never stops it.
pub fn compile_sources(
    project: &Project,
    packages: &mut PackageSet,
    seen: &Seen,
    previous: Option<&DataSet>,
) -> Result<Compiled, Vec<Diagnostic>> {
    let code = build_code(project, packages)?;
    Ok(read_data(&code, project, seen, previous))
}

/// The packs `project` builds with (PACK-9), from its manifest, its lock and `packages`' store.
pub fn resolve_packs(project: &Project, packages: &PackageSet) -> m2_song::pack::Resolution {
    m2_song::pack::resolve(&project.dir, &project.manifest, &project.lock, packages.store(), &app(packages))
}

/// The packs the project in `snapshot` resolves (PACK-9), by name, with their folders: each that
/// resolves, though another doesn't. None when it isn't a project that reads.
pub(crate) fn resolved(snapshot: &m2_song::Snapshot, packages: &PackageSet) -> Option<Vec<(String, std::path::PathBuf)>> {
    let project = Project::from_snapshot(snapshot).ok()?;
    Some(resolve_packs(&project, packages).packs.into_iter().map(|r| (r.name.as_str().to_string(), r.dir)).collect())
}

/// Build a project's code: check its dependencies and resolve its packs (PACK-9), run the
/// TypeScript (QuickJS), bind the hosted plugins and check the graph. What only the code decides;
/// the data is [`read_data`]'s. The build gets its own view of the packages, std and the packs it
/// resolved, which its code keeps; what `packages` shows never changes, so a check, a refused Tx
/// or a probe never moves what the accepted revision lists or plays.
pub fn build_code(project: &Project, packages: &mut PackageSet) -> Result<Arc<Code>, Vec<Diagnostic>> {
    let resolution = resolve_packs(project, packages);
    let mut unmet = dependencies(&project.manifest, packages);
    unmet.extend(resolution.problems.into_iter().map(diagnostic));
    if !unmet.is_empty() {
        return Err(unmet);
    }
    let packs = resolution.packs;
    let listed: Vec<(String, std::path::PathBuf)> = packs.iter().map(|r| (r.name.as_str().to_string(), r.dir.clone())).collect();
    let broken = |name: &str, why: String| {
        let message = format!("{name}, which this song uses, can't be used: {why}");
        vec![Diagnostic::error("pkg.broken", message).at(Some(m2_song::path::MANIFEST.to_string()))]
    };
    let view = packages.view_of(&listed).map_err(|e| broken("A pack", format!("{e:#}")))?;
    let app = app(packages);
    for r in &packs {
        let Some(p) = view.get(r.name.as_str()) else { continue };
        app.meets(&p.needs).map_err(|why| {
            // One of music2's own that it no longer comes with is moved on first (review Y5).
            let asked = project.manifest.dependencies.get(r.name.as_str()).and_then(|d| d.requirement());
            match asked.and_then(|req| m2_song::pack::move_on(&r.name, req)) {
                Some(first) => broken(&format!("{first}. {}", r.name), format!("it {why}")),
                None => broken(r.name.as_str(), format!("it {why}")),
            }
        })?;
    }
    let code = project.code.iter().map(|(p, t)| (p.clone(), t.to_string())).collect();
    let mut opts = m2_script::Options::new(code, &project.manifest.project.entry);
    opts.packages = view.packages().map(|p| (p.name.clone(), p.dir.clone())).collect();
    opts.dependencies = project.manifest.dependencies.keys().filter(|d| *d != m2_song::SDK).cloned().collect();
    opts.data = Some(DATA);
    let m2_script::Build { mut ir, sites, outputs, package_files, components, units } = m2_script::build_full(&opts)?;
    let components = components.into_iter().map(|d| (d.id.clone(), Arc::new(d))).collect();

    // A missing plugin and a renamed parameter never stop the song (FMT-18, FMT-18a).
    let (blocking, kept): (Vec<_>, Vec<_>) =
        packages.bind_plugins(&mut ir).into_iter().partition(|d| d.severity == Severity::Error && d.code != "plugin.missing");
    if !blocking.is_empty() {
        return Err(blocking);
    }
    let flat = m2_ir::check(&ir, &*packages.viewing(&view))
        .map_err(|e| e.into_iter().map(|d| at_project_call(d, &ir, &sites, project)).collect::<Vec<_>>())?;
    let mut decls = m2_runtime::decls(&ir);
    // A package's own files are read from the packages it was built with (PACK-5).
    decls.packages = view.packages().map(|p| (p.name.clone(), p.dir.clone())).collect();
    let (sites, units) = (sites.into(), units.into());
    Ok(Arc::new(Code { ir, flat, sites, outputs, package_files, packs, view, components, units, decls, kept }))
}

/// A graph-check error at a node made inside a package, placed at the innermost call in the
/// project whose frames made it, with the package's place kept as `inner` (the studio plan's Q46):
/// `lowpass(x, db(-3))` is a mistake on your own line, not in `std/src/filter.ts`. An error
/// already in the project, or one whose frames reach no call in it, keeps its place.
fn at_project_call(mut d: Diagnostic, ir: &Ir, sites: &[m2_script::sites::Site], project: &Project) -> Diagnostic {
    let ours = |src: &str| src.rsplitn(3, ':').nth(2).is_some_and(|file| project.code.contains_key(file));
    if d.code != "code.check" || d.src.as_deref().is_some_and(ours) {
        return d;
    }
    let (Some(module), Some(node)) = (d.module.as_deref(), d.node.as_deref()) else { return d };
    let Some(m) = ir.modules.iter().find(|m| m.key == module) else { return d };
    let Some(n) = m.nodes.iter().find(|n| n.id == node) else { return d };
    let call = m.chain(n.frame).into_iter().rev().find_map(|f| sites.iter().find(|s| s.id == f.site && s.in_project));
    let Some(site) = call else { return d };
    // The column, in characters, from the call's first byte in its file.
    let col = project.code.get(&site.file).and_then(|text| {
        let start = (site.start as usize).min(text.len());
        let line = text[..start].rfind('\n').map_or(0, |i| i + 1);
        text.get(line..start).map(|s| s.chars().count() as u32 + 1)
    });
    d.inner = d.src.take().map(Box::new);
    d.src = Some(match col {
        Some(col) => format!("{}:{}:{col}", site.file, site.line),
        None => format!("{}:{}:1", site.file, site.line),
    });
    d
}

/// Read a project's data against `code`'s declarations: every declared lane and plugin state,
/// and the problems found on the way, data no lane reads among them. No QuickJS run: what a
/// change to data files alone costs (PLANE-1, WRITE-4, WRITE-14). `previous` is the data last
/// accepted, which a lane whose file fails keeps playing (FMT-9).
pub fn read_data(code: &Arc<Code>, project: &Project, seen: &Seen, previous: Option<&DataSet>) -> Compiled {
    let (data, mut problems) = project.data(&code.decls, previous);
    problems.extend(code.kept.iter().cloned().map(problem));
    problems.extend(unclaimed::unclaimed(project, &code.ir, &code.decls, seen));
    problems.sort_by_key(|d| d.severity);
    Compiled { code: code.clone(), data, problems }
}

/// Every component `packages` declare and offer, and where each is exported (PKG-11, PACK-11):
/// std's, the loaded packs' and those of the newest version of every other pack in the store. One
/// sandboxed run per package module, cached for the process. It never fails (WRITE-20): a module
/// that doesn't load, an invalid descriptor or one id twice is `pkg.invalid` in its `problems`,
/// and only those components are left out.
pub fn catalog(packages: &PackageSet) -> m2_script::Catalog {
    catalog_of(packages, &offered(packages))
}

/// The catalog of std, as `packages` shows it, and of the packs `offered` lists.
pub(crate) fn catalog_of(packages: &PackageSet, offered: &[Offered]) -> m2_script::Catalog {
    let mut installed: Vec<(String, std::path::PathBuf)> =
        packages.packages().filter(|p| p.name == m2_song::pack::STD).map(|p| (p.name.clone(), p.dir.clone())).collect();
    installed.extend(offered.iter().map(|(name, _, dir)| (name.clone(), dir.clone())));
    describe(&installed)
}

/// Every component `packages` declare and where each is exported (`m2_script::describe`), their
/// instruments written as data included (PACK-13).
pub fn describe(packages: &[(String, std::path::PathBuf)]) -> m2_script::Catalog {
    m2_script::describe_with(packages, Some(DATA))
}

/// What reads a pack's instruments written as data for the build (PACK-13).
pub const DATA: m2_script::DataReader = m2_script::DataReader { module: data_instrument, binding: data_binding };

/// A pack's `instruments/<id>.toml` read and made into its module (PACK-13): its name checked as an
/// id first, so code is made only from one that passed.
pub fn data_instrument(package: &str, id: &str, text: &str) -> Result<m2_script::DataModule, m2_script::DataProblem> {
    let problem = |message: String, line: u32, col: u32| m2_script::DataProblem { message, line, col };
    let id = m2_song::instrument::Id::new(id).map_err(|why| problem(file_name(&why), 1, 1))?;
    let i = m2_song::instrument::DataInstrument::read(text).map_err(|p| problem(p.message, p.line, p.col))?;
    Ok(m2_script::DataModule { code: i.module(package, &id), regions: i.regions.iter().map(|r| r.line).collect() })
}

/// The name a data instrument is exported by (PACK-13), or why its file's name can't be an id.
fn data_binding(id: &str) -> Result<String, String> {
    m2_song::instrument::Id::new(id).map(|id| id.binding()).map_err(|why| file_name(&why))
}

/// Why a data file's name can't be its instrument's id, in words at the file.
fn file_name(why: &str) -> String {
    format!("the file's name is the instrument's id, and it can't be one: {why}")
}

/// A pack the lists offer (PACK-11): its name, and the version and folder they offer it from.
pub type Offered = (String, Version, std::path::PathBuf);

/// The packs the lists offer (PACK-11): those `packages` shows (the accepted revision's), then of
/// every other pack in the store, the newest version this app can run: one whose `[dependencies]`
/// its SDK and std meet (PACK-2).
pub fn offered(packages: &PackageSet) -> Vec<Offered> {
    packages.offered(&app(packages))
}

/// What this app gives a pack (PACK-2): its SDK, and the std `packages` shows. Whatever asks whether
/// it runs a pack asks this.
pub fn app(packages: &PackageSet) -> m2_song::pack::App {
    m2_song::pack::App {
        sdk: Version::parse(m2_script::SDK_VERSION),
        std: packages.get(m2_song::pack::STD).and_then(|p| Version::parse(&p.version)),
    }
}

/// The SDK and std entries of `[dependencies]` against what this app has (FMT-3). A pack's are
/// its resolution's to check (PACK-9).
fn dependencies(manifest: &Manifest, packages: &PackageSet) -> Vec<Diagnostic> {
    let mut ours = manifest.clone();
    ours.dependencies.retain(|name, _| name == m2_song::SDK || name == m2_song::pack::STD);
    let installed = |name: &str| match name {
        m2_song::SDK => Version::parse(m2_script::SDK_VERSION),
        _ => packages.packages().find(|p| p.name == name).and_then(|p| Version::parse(&p.version)),
    };
    ours.check_dependencies(installed).into_iter().map(diagnostic).collect()
}

/// A project diagnostic from `core/song` as the build's diagnostics are shaped (no fixes).
pub fn diagnostic(d: m2_song::Diagnostic) -> Diagnostic {
    let mut out = Diagnostic::error(d.code, d.message).at(d.src);
    out.module = d.module;
    out.severity = match d.severity {
        m2_song::Severity::Error => Severity::Error,
        m2_song::Severity::Warning => Severity::Warning,
        m2_song::Severity::Notice => Severity::Notice,
    };
    out
}

/// A build diagnostic that doesn't stop the song, as the revision's problems are shaped. Its
/// `src` stays `file:line:col` when it has one.
pub fn problem(d: Diagnostic) -> m2_song::Diagnostic {
    let severity = match d.severity {
        Severity::Error => m2_song::Severity::Error,
        Severity::Warning => m2_song::Severity::Warning,
        Severity::Notice => m2_song::Severity::Notice,
    };
    let mut out = m2_song::Diagnostic::new(severity, d.code, d.message);
    (out.src, out.module) = (d.src, d.module);
    out
}

#[cfg(test)]
mod bus_tests;
#[cfg(test)]
mod gate_tests;
#[cfg(test)]
mod history_tests;
#[cfg(test)]
mod pack_tests;

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;

    fn std_packages() -> PackageSet {
        let mut p = PackageSet::default().with_store(m2_song::pack::test_store());
        p.load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")).unwrap();
        p
    }

    pub(crate) const MANIFEST: &str = "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"2\"\nstd = \"0.9\"\n\"@m2/drums\" = \"2\"\n\"@m2/effects\" = \"1\"\n\"@m2/synths\" = \"2\"\n";

    fn project(files: &[(&str, &str)]) -> PathBuf {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "m2-session-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        for (name, text) in [("music2.toml", MANIFEST)].iter().chain(files) {
            let p = dir.join(name);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        dir
    }

    const SONG: &str = r#"import { song } from "@m2/core";
import { drums } from "@m2/drums";
import { track } from "@m2/core";
const k = track("kick", ({ lane }) => drums.drum(lane.notes("hits")));
export default song(() => k.out);
"#;

    fn compile_dir(dir: &Path) -> Result<Compiled, Vec<Diagnostic>> {
        compile(&Project::open(dir).unwrap(), &mut std_packages())
    }

    #[test]
    fn the_example_compiles() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song");
        let c = compile_dir(&dir).unwrap_or_else(|e| panic!("{e:?}"));
        assert!(c.problems.is_empty(), "{:?}", c.problems);
    }

    // FMT-9: a lane file that fails never stops the compile; it is a problem naming the file and
    // the field, and that lane plays nothing.
    #[test]
    fn bad_lane_data_is_a_problem_not_a_failure() {
        let hits = r#"{"clips": [{"id": "a", "start": 0, "length": 4, "notes": [{"pitch": "H9", "start": 0, "length": 1}]}]}"#;
        let dir = project(&[("song.ts", SONG), ("data/kick/hits.json", hits)]);
        let c = compile_dir(&dir).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(c.problems.iter().map(|d| d.code).collect::<Vec<_>>(), ["lane.invalid"]);
        assert!(c.problems[0].message.contains("bad pitch `H9`"), "{}", c.problems[0]);
        assert_eq!(c.problems[0].src.as_deref(), Some("data/kick/hits.json"));
        assert!(c.data.notes("kick", "hits").is_empty());
        std::fs::remove_dir_all(dir).ok();

        // FMT-9: the arrangement does block.
        let dir = project(&[("song.ts", SONG), ("data/arrangement.json", r#"{"tempo": [{"beat": 0, "bpm": 5}]}"#)]);
        let e = Project::open(&dir).unwrap_err();
        assert_eq!(e[0].code, "arrangement.invalid");
        std::fs::remove_dir_all(dir).ok();
    }

    // FMT-16: compile never decodes audio: an asset that isn't audio at all compiles.
    #[test]
    fn compile_never_decodes_assets() {
        let src = r#"import { song, track } from "@m2/core";
const v = track("vox", ({ lane }) => lane.audio("takes"));
export default song(() => v.out);
"#;
        let takes = r#"{"clips": [{"id": "a", "start": 0, "length": 4, "asset": "take.wav"}]}"#;
        let dir = project(&[("song.ts", src), ("data/vox/takes.json", takes), ("assets/take.wav", "not a wav file")]);
        let c = compile_dir(&dir).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(c.problems.iter().map(|d| d.code).collect::<Vec<_>>(), ["asset.unpinned"]);
        std::fs::remove_dir_all(dir).ok();
    }

    /// [`MANIFEST`], asking for std `req`.
    fn asking_for_std(req: &str) -> String {
        MANIFEST.lines().map(|l| if l.starts_with("std = ") { format!("std = \"{req}\"\n") } else { format!("{l}\n") }).collect()
    }

    // FMT-3, PACK-24: an unmet dependency is refused before any code runs. A song asking for an
    // older std is told how to move to the one bundled (L7), and so is one asking for an older major
    // of @m2/core; one asking for a newer std or @m2/core is not.
    #[test]
    fn unmet_dependencies_block() {
        let dir = project(&[("song.ts", SONG)]);
        std::fs::write(dir.join("music2.toml"), asking_for_std("9")).unwrap();
        let e = compile_dir(&dir).err().unwrap();
        assert_eq!(e.iter().map(|d| d.code).collect::<Vec<_>>(), ["pkg.unsatisfied"]);
        assert!(!e[0].message.contains("This app has"), "{}", e[0].message);
        std::fs::write(dir.join("music2.toml"), asking_for_std("0.0")).unwrap();
        let e = compile_dir(&dir).err().unwrap();
        assert_eq!(e.iter().map(|d| d.code).collect::<Vec<_>>(), ["pkg.unsatisfied"]);
        // std is 0.x, so a song asks for it by its first two numbers.
        let Version(major, minor, _) = Version::parse(&std_packages().get("std").unwrap().version).unwrap();
        let hint = format!(
            "{}This app has std {major}.{minor}. To use it, change the std line in music2.toml to std = \"{major}.{minor}\". The song",
            m2_song::UPGRADE
        );
        assert!(e[0].message.starts_with(&hint), "{}", e[0].message);
        // A newer @m2/core than this app's: no way to move but a newer music2.
        std::fs::write(dir.join("music2.toml"), MANIFEST.replace("\"@m2/core\" = \"2\"", "\"@m2/core\" = \"3\"")).unwrap();
        let e = compile_dir(&dir).err().unwrap();
        assert_eq!(e.iter().map(|d| d.code).collect::<Vec<_>>(), ["sdk.unsatisfied"]);
        assert!(!e[0].message.contains("This app has"), "{}", e[0].message);
        // PACK-24: an older major is told first how to move, and that the code may need changes.
        std::fs::write(dir.join("music2.toml"), MANIFEST.replace("\"@m2/core\" = \"2\"", "\"@m2/core\" = \"1\"")).unwrap();
        let e = compile_dir(&dir).err().unwrap();
        assert_eq!(e.iter().map(|d| d.code).collect::<Vec<_>>(), ["sdk.unsatisfied"]);
        let Version(major, _, _) = Version::parse(m2_script::SDK_VERSION).unwrap();
        let hint = format!(
            "{}This app has @m2/core {major}. To use it, change the @m2/core line in music2.toml to \"@m2/core\" = \"{major}\". The song's \
             code may need changes. music2.toml asks for @m2/core \"1\"",
            m2_song::UPGRADE
        );
        assert!(e[0].message.starts_with(&hint), "{}", e[0].message);
        std::fs::remove_dir_all(dir).ok();
    }

    // FMT-3, PACK-2, PACK-23: every project the repo ships asks for what this build has: the SDK and
    // the bundled std; and every pack it ships needs no more than that, asking for the SDK's major. So a std bump that leaves a
    // manifest behind fails here, with no grep needed.
    #[test]
    fn every_shipped_manifest_is_met() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let packages = std_packages();
        let mut unmet = vec![];
        let (sdk, std) = (Version::parse(m2_script::SDK_VERSION), packages.get("std").and_then(|p| Version::parse(&p.version)));
        let mut packs = 0;
        // PACK-23: and each pack that runs code asks for this app's major of @m2/core alone
        // (`"@m2/core" = "2"`); a pack of recordings alone may ask for none.
        let major = sdk.map(|v| v.0.to_string());
        for root in ["examples", "templates", "core/session/tests/fixtures", "e2e/fixtures", "packs"] {
            let mut found = 0;
            let mut stack = vec![repo.join(root)];
            while let Some(d) = stack.pop() {
                let pack = d.join(m2_song::pack::PKG_MANIFEST);
                if pack.exists() {
                    packs += 1;
                    let name = pack.strip_prefix(&repo).unwrap().display().to_string();
                    let m = m2_song::pack::PkgManifest::at(&d).unwrap_or_else(|e| panic!("{name}: {e}"));
                    unmet.extend(m2_song::pack::meets(&m.dependencies, sdk, std).err().map(|why| format!("{name}: {why}")));
                    let core = m.dependencies.get("@m2/core").map(|r| r.to_string());
                    if core.is_some() && core != major || core.is_none() && d.join("src").exists() {
                        unmet.push(format!(
                            "{name}: asks for @m2/core {core:?}, not \"{}\" alone (PACK-23)",
                            major.as_deref().unwrap_or("?")
                        ));
                    }
                }
                let file = d.join("music2.toml");
                if file.exists() {
                    found += 1;
                    let name = file.strip_prefix(&repo).unwrap().display().to_string();
                    let manifest = Manifest::read(&std::fs::read_to_string(&file).unwrap()).unwrap_or_else(|e| panic!("{name}: {e:?}"));
                    unmet.extend(dependencies(&manifest, &packages).into_iter().map(|d| format!("{name}: {}", d.message)));
                    continue;
                }
                for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                    if e.file_type().is_ok_and(|t| t.is_dir()) {
                        stack.push(e.path());
                    }
                }
            }
            // Finding none fails: a test built in a folder that has since moved would pass having
            // checked nothing. The bundled packs (PACK-15) are packs, not songs.
            assert!(found > 0 || root == "packs", "no music2.toml under {}", repo.join(root).display());
        }
        assert!(packs > 0, "no m2pkg.toml under the fixtures: the packs' check checked nothing");
        assert!(unmet.is_empty(), "shipped projects and packs this build can't build:\n{}", unmet.join("\n"));
    }

    // PACK-18: every song the repo ships that imports a pack music2 ships (PACK-15) lists it in
    // [dependencies] and locks it at the bundled version and hash; and no song locks another
    // version of one. A bundled pack whose files changed under its version fails here, as in
    // `every_bundled_pack_is_the_version_its_record_names`: it moves to a new version (PACK-17), then
    // the songs move to it.
    #[test]
    fn every_shipped_song_locks_the_bundled_packs_it_plays() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let store = m2_song::pack::Store::at(m2_song::pack::test_store());
        let bundled: Vec<(m2_song::pack::PackName, Version, m2_song::Sha)> = m2_song::pack::bundled(&m2_song::pack::source_packs())
            .iter()
            .map(|dir| {
                let m = m2_song::pack::PkgManifest::at(dir).unwrap();
                let hash = store.hash(&m.package.name, &m.package.version).unwrap();
                (m.package.name, m.package.version, hash)
            })
            .collect();
        assert_eq!(bundled.len(), 4, "@m2/drums, @m2/effects, @m2/mixing and @m2/synths");
        let (mut songs, mut wrong) = (0, vec![]);
        let mut stack: Vec<PathBuf> = ["examples", "templates", "core/session/tests/fixtures", "e2e/fixtures"].map(|r| repo.join(r)).into();
        while let Some(d) = stack.pop() {
            if !d.join("music2.toml").is_file() {
                stack.extend(std::fs::read_dir(&d).into_iter().flatten().flatten().filter(|e| e.path().is_dir()).map(|e| e.path()));
                continue;
            }
            songs += 1;
            let name = d.strip_prefix(&repo).unwrap().display().to_string();
            let project = Project::open(&d).unwrap_or_else(|e| panic!("{name}: {e:?}"));
            let code: String = project.code.values().map(|t| t.to_string()).collect();
            for (pack, version, hash) in &bundled {
                let plays = code.contains(&format!("from \"{pack}\"")) || code.contains(&format!("from \"{pack}/"));
                let listed = project.manifest.dependencies.contains_key(pack.as_str());
                let locked = project.lock.get(pack).map(|l| (l.version, l.hash));
                match (plays, listed, locked) {
                    (false, false, None) => {}
                    (true, true, Some((v, h))) if v == *version && h == *hash => {}
                    (true, false, _) => wrong.push(format!("{name}: plays {pack} but music2.toml doesn't list it")),
                    (_, _, Some((v, h))) if v == *version && h != *hash => wrong.push(format!(
                        "{name}: locks {pack} {v} ({h}), but music2 ships {v} with other files ({hash}). A version's files never change \
                         (PACK-17): move {pack} to {} in packs/{pack}/m2pkg.toml, record it in packs/versions.toml, and relock the \
                         songs (`m2 pack update {pack}` in each)",
                        Version(v.0, v.1, v.2 + 1)
                    )),
                    (_, _, Some((v, _))) if v != *version => wrong
                        .push(format!("{name}: locks {pack} {v}, but music2 ships {version}: move it with `m2 pack update {pack}` in it")),
                    _ => wrong.push(format!("{name}: {pack} is listed but not locked, or locked but not played")),
                }
            }
        }
        assert!(songs > 10, "found {songs} songs");
        assert!(wrong.is_empty(), "songs the repo ships that don't lock the packs music2 ships (PACK-18):\n{}", wrong.join("\n"));
    }

    // PACK-17 (review S6): the packs whose hashes a lock or packs/versions.toml pins keep their
    // bytes on every checkout: .gitattributes leaves their files as committed (`-text`, never a
    // CRLF on Windows), and none holds a CR, which a checkout that turned LF into CRLF would.
    #[test]
    fn pinned_packs_keep_their_bytes_on_every_checkout() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let attributes = std::fs::read_to_string(repo.join(".gitattributes")).unwrap_or_default();
        let (mut files, mut wrong) = (0, vec![]);
        for root in ["packs", "core/session/tests/fixtures/packs", "e2e/fixtures/library"] {
            if !attributes.lines().any(|l| l.split_whitespace().collect::<Vec<_>>() == [&format!("{root}/**"), "-text"]) {
                wrong.push(format!(".gitattributes doesn't say `{root}/** -text`"));
            }
            let mut stack = vec![repo.join(root)];
            while let Some(d) = stack.pop() {
                for e in std::fs::read_dir(&d).unwrap_or_else(|e| panic!("{}: {e}", d.display())).flatten() {
                    let path = e.path();
                    if e.file_type().unwrap().is_dir() {
                        stack.push(path);
                    } else if path.extension().is_some_and(|x| ["ts", "toml", "json", "md", "txt", "sfz"].contains(&x.to_str().unwrap())) {
                        files += 1;
                        if std::fs::read(&path).unwrap().contains(&b'\r') {
                            wrong.push(format!(
                                "{} holds a CR: its hash isn't the one its locks name",
                                path.strip_prefix(&repo).unwrap().display()
                            ));
                        }
                    }
                }
            }
        }
        assert!(files > 20, "found {files} files");
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    // DIAG-1: data no lane reads is a Warning among the revision's problems.
    #[test]
    fn unused_lane_files_are_warnings() {
        let dir = project(&[("song.ts", SONG), ("data/kick/old.json", r#"{"clips": []}"#)]);
        let c = compile_dir(&dir).unwrap_or_else(|e| panic!("{e:?}"));
        let found: Vec<_> = c.problems.iter().map(|d| (d.severity, d.code, d.src.as_deref())).collect();
        assert_eq!(found, [(m2_song::Severity::Warning, "data.unclaimed", Some("data/kick/old.json"))]);
        std::fs::remove_dir_all(dir).ok();
    }

    // FMT-18a: a plugin that isn't installed doesn't stop the song; it is an Error among the
    // revision's problems, and its node plays silence.
    #[test]
    fn a_missing_plugin_is_a_problem_not_a_failure() {
        let src = r#"import { plugin, song, track } from "@m2/core";
import { drums } from "@m2/drums";
import { sum } from "std/math";
const k = track("kick", ({ lane }) => drums.drum(lane.notes("hits")));
const s = track("synth", { name: "Synth" }, ({ lane }) =>
  plugin("clap:org.example.not-installed", { notes: lane.notes("notes"), cutoff_7: 0.5 }));
export default song(() => sum(k.out, s.out));
"#;
        let dir = project(&[("song.ts", src)]);
        let c = compile_dir(&dir).unwrap_or_else(|e| panic!("{e:?}"));
        let found: Vec<_> = c.problems.iter().map(|d| (d.severity, d.code, d.module.as_deref())).collect();
        assert_eq!(found, [(m2_song::Severity::Error, "plugin.missing", Some("synth"))]);
        assert!(c.problems[0].src.as_deref().is_some_and(|s| s.starts_with("song.ts:6:")), "{:?}", c.problems[0]);
        std::fs::remove_dir_all(dir).ok();
    }

    // Q46 (the studio plan): a graph-check error points at the call in the project that made
    // the node, never inside a package; the place inside the package is its `inner`. Through a
    // component of the song's own in lib/, it is the innermost call in the project.
    #[test]
    fn a_check_error_points_at_the_call_in_the_project() {
        let song = r#"import { song } from "@m2/core";
import bass from "./tracks/bass";
export default song(() => bass.out);
"#;
        let bass = r#"import { db, track } from "@m2/core";
import { bass } from "@m2/synths";
import { lowpass } from "std/filter";

export default track("bass", ({ lane }) => {
  return lowpass(bass(lane.notes("notes")), db(-3));
});
"#;
        let dir = project(&[("song.ts", song), ("tracks/bass.ts", bass)]);
        let e = compile_dir(&dir).err().expect("db(-3) as a cutoff doesn't check");
        assert_eq!(e.iter().map(|d| d.code).collect::<Vec<_>>(), ["code.check"], "{e:?}");
        assert!(e[0].message.contains("expects control<hz>, got control<db>"), "{}", e[0]);
        assert_eq!(e[0].src.as_deref(), Some("tracks/bass.ts:6:10"), "{e:?}");
        assert!(e[0].inner.as_deref().is_some_and(|s| s.contains("filter.ts:")), "{e:?}");
        std::fs::remove_dir_all(dir).ok();

        // Through the song's own component: at its call of lowpass, in lib/dark.ts.
        let dark = r#"import { component, db, port, signature, type Audio } from "@m2/core";
import { lowpass } from "std/filter";

export const dark = component(
  { id: "dark", label: "Dark", signature: signature<(x: Audio) => Audio>(), inputs: { in: port.audio() }, controls: {}, output: port.audio() },
  (x: Audio) => lowpass(x, db(-3)),
);
"#;
        let bass = r#"import { track } from "@m2/core";
import { bass } from "@m2/synths";
import { dark } from "../lib/dark";

export default track("bass", ({ lane }) => dark(bass(lane.notes("notes"))));
"#;
        let dir = project(&[("song.ts", song), ("tracks/bass.ts", bass), ("lib/dark.ts", dark)]);
        let e = compile_dir(&dir).err().expect("db(-3) as a cutoff doesn't check");
        assert_eq!(e[0].src.as_deref(), Some("lib/dark.ts:6:17"), "{e:?}");
        assert!(e[0].inner.as_deref().is_some_and(|s| s.contains("filter.ts:")), "{e:?}");
        std::fs::remove_dir_all(dir).ok();
    }
}
