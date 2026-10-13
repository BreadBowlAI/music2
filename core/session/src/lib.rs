//! Turns a project folder into something playable.
//!
//! [`compile`] is the one operation every host uses before playing or rendering: if it
//! succeeds, the song renders. It checks the manifest's dependencies, runs the TypeScript,
//! binds the hosted plugins, checks the graph, and reads every declared lane's data against its
//! declaration. What doesn't stop the song comes back as `problems` (DIAG-1): data that doesn't
//! read (the lane plays what it can), data no lane reads, a plugin that isn't installed.
//!
//! [`Session`] (in `edit`) is the editing session built on it: a source snapshot,
//! revisions, verified value edits and undo.

pub mod edit;
pub mod lanes;
pub mod plane;
pub mod reference;
pub mod render;
pub mod timeline;
pub mod unclaimed;
pub mod view;

use std::collections::BTreeMap;
use std::sync::Arc;

use m2_ir::{Diagnostic, Flat, Ir, Severity};
use m2_runtime::PackageSet;
use m2_script::{Descriptor, UnitCtor};
use m2_song::{DataSet, Project, Version};
use serde::Serialize;

pub use edit::{Audition, Committed, Control, Declared, EditError, Edited, Inlet, Outlet, Revision, Session};
pub use edit::effect::EffectInfo;
/// What [`Session::write_notes`] writes.
pub use m2_song::{Note, NoteClip};
pub use timeline::{TimelineLane, TimelineTrack, TimelineView};
pub use unclaimed::{Seen, SeenLane};
pub use view::{GraphView, Knob, ModuleView, ReadOnly, ViewNode};

/// Where a value lives (HOME-1): what editing it writes, and what the UI says it is. Every
/// value [`Session::controls`], the view and gesture leases return names its home.
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
    /// `.music2/`: this machine's monitoring and view state.
    Local,
}

/// A project that is ready to render.
pub struct Compiled {
    pub ir: Ir,
    pub flat: Flat,
    /// The call sites that origins in the IR refer to.
    pub sites: Vec<m2_script::sites::Site>,
    /// What each call returned, when it was a signal.
    pub outputs: Vec<m2_script::Output>,
    /// Every package file the build read (FMT-6a, RENDER-2).
    pub package_files: Vec<m2_script::PackageFile>,
    /// The descriptor of every component the build's modules declare, by ComponentId: what a
    /// frame's `component` names (PKG-9, PKG-11).
    pub components: BTreeMap<String, Arc<Descriptor>>,
    /// The unit constructors of `@m2/core`, as the build generated them from its table (PKG-11):
    /// the one table gestures write units with.
    pub units: Arc<[UnitCtor]>,
    /// What the graph plays: each declared lane and plugin state, read against its declaration.
    pub data: DataSet,
    /// Everything that needs attention but doesn't stop the song (DIAG-1, FMT-9, FMT-18a), with
    /// the fixes each offers: a lane that doesn't read, clip ids, asset pins, plugin states,
    /// unknown files under `data/`, data no lane or plugin reads, a plugin that isn't
    /// installed, a parameter the plugin renamed. Errors first, then Warnings, then Notices.
    pub problems: Vec<m2_song::Diagnostic>,
}

pub fn compile(project: &Project, packages: &mut PackageSet) -> Result<Compiled, Vec<Diagnostic>> {
    compile_sources(project, packages, &Seen::default(), None)
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
    let unmet = dependencies(project, packages);
    if !unmet.is_empty() {
        return Err(unmet);
    }
    let mut opts = m2_script::Options::new(project.code.clone(), &project.manifest.project.entry);
    opts.packages = packages.packages().map(|p| (p.name.clone(), p.dir.clone())).collect();
    opts.dependencies = project.manifest.dependencies.keys().filter(|d| *d != m2_song::SDK).cloned().collect();
    let m2_script::Build { mut ir, sites, outputs, package_files, components, units } = m2_script::build_full(&opts)?;
    let components = components.into_iter().map(|d| (d.id.clone(), Arc::new(d))).collect();

    // A missing plugin and a renamed parameter never stop the song (FMT-18, FMT-18a).
    let (blocking, kept): (Vec<_>, Vec<_>) =
        packages.bind_plugins(&mut ir).into_iter().partition(|d| d.severity == Severity::Error && d.code != "plugin.missing");
    if !blocking.is_empty() {
        return Err(blocking);
    }
    let flat = m2_ir::check(&ir, packages)?;
    let decls = m2_runtime::decls(&ir);
    let (data, mut problems) = project.data(&decls, previous);
    problems.extend(kept.into_iter().map(problem));
    problems.extend(unclaimed::unclaimed(project, &ir, &decls, seen));
    problems.sort_by_key(|d| d.severity);
    Ok(Compiled { ir, flat, sites, outputs, package_files, components, units: units.into(), data, problems })
}

/// Every component `packages` declare, and where each is exported (PKG-11): one sandboxed run
/// per package module, cached for the process. It never fails (WRITE-20): a module that doesn't
/// load, an invalid descriptor or one id twice is `pkg.invalid` in its `problems`, and only those
/// components are left out.
pub fn catalog(packages: &PackageSet) -> m2_script::Catalog {
    let installed: Vec<(String, std::path::PathBuf)> = packages.packages().map(|p| (p.name.clone(), p.dir.clone())).collect();
    m2_script::describe(&installed)
}

/// Every `[dependencies]` entry against what is installed (FMT-3): the SDK this build runs, and
/// each package's own version.
fn dependencies(project: &Project, packages: &PackageSet) -> Vec<Diagnostic> {
    let installed = |name: &str| match name {
        m2_song::SDK => Version::parse(m2_script::SDK_VERSION),
        _ => packages.packages().find(|p| p.name == name).and_then(|p| Version::parse(&p.version)),
    };
    project.manifest.check_dependencies(installed).into_iter().map(diagnostic).collect()
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
mod gate_tests;

#[cfg(test)]
mod gate5_tests;

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;

    fn std_packages() -> PackageSet {
        let mut p = PackageSet::default();
        p.load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")).unwrap();
        p
    }

    pub(crate) const MANIFEST: &str =
        "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.1\"\n";

    fn project(files: &[(&str, &str)]) -> PathBuf {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("m2-session-test-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
        for (name, text) in [("music2.toml", MANIFEST)].iter().chain(files) {
            let p = dir.join(name);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        dir
    }

    const SONG: &str = r#"import { song } from "@m2/core";
import { kick } from "std/instruments";
import { track } from "@m2/core";
const k = track("kick", ({ lane }) => kick(lane.notes("hits")));
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

    // FMT-3: an unmet dependency is refused before any code runs.
    #[test]
    fn unmet_dependencies_block() {
        let dir = project(&[("song.ts", SONG)]);
        std::fs::write(dir.join("music2.toml"), MANIFEST.replace("std = \"0.1\"", "std = \"9\"")).unwrap();
        let e = compile_dir(&dir).err().unwrap();
        assert_eq!(e.iter().map(|d| d.code).collect::<Vec<_>>(), ["pkg.unsatisfied"]);
        std::fs::write(dir.join("music2.toml"), MANIFEST.replace("\"@m2/core\" = \"1\"", "\"@m2/core\" = \"2\"")).unwrap();
        let e = compile_dir(&dir).err().unwrap();
        assert_eq!(e.iter().map(|d| d.code).collect::<Vec<_>>(), ["sdk.unsatisfied"]);
        std::fs::remove_dir_all(dir).ok();
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
import { kick } from "std/instruments";
import { sum } from "std/math";
const k = track("kick", ({ lane }) => kick(lane.notes("hits")));
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
}
