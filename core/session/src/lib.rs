//! Turns a project folder into something playable.
//!
//! [`compile`] is the one operation every host uses before playing or rendering: if it
//! succeeds, the song renders. It runs the TypeScript, checks the graph, and validates
//! every lane's timeline data against the lane's declared kind.
//!
//! [`Session`] (in `edit`) is the editing session built on it: a source snapshot,
//! revisions, verified value edits and undo.

pub mod edit;
pub mod view;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use m2_ir::{Diagnostic, Flat, Ir, Kind};
use m2_runtime::PackageSet;
use m2_song::{Project, slug};

pub use edit::{Audition, Committed, Control, EditError, Edited, Inlet, Outlet, Revision, Session};
pub use view::{GraphView, Knob, ModuleView, ViewNode};

/// A project that is ready to render.
pub struct Compiled {
    pub ir: Ir,
    pub flat: Flat,
    /// The call sites that origins in the IR refer to.
    pub sites: Vec<m2_script::sites::Site>,
    /// What each call returned, when it was a signal.
    pub outputs: Vec<m2_script::Output>,
    /// Problems that don't stop the song from playing, such as unused lane files.
    pub warnings: Vec<Diagnostic>,
}

pub fn compile(project: &Project, packages: &mut PackageSet) -> Result<Compiled, Vec<Diagnostic>> {
    compile_sources(project, packages, HashMap::new())
}

/// [`compile`], with some files' contents supplied instead of read from disk.
pub fn compile_sources(project: &Project, packages: &mut PackageSet, sources: HashMap<PathBuf, String>) -> Result<Compiled, Vec<Diagnostic>> {
    let mut opts = m2_script::Options::new(&project.dir, &project.entry());
    opts.packages = packages.packages().map(|p| (p.name.clone(), p.dir.clone())).collect();
    opts.sources = sources;
    let m2_script::Build { ir, sites, outputs } = m2_script::build_full(&opts)?;

    let mut diags = names(&ir);
    diags.extend(plugins(project, &ir, packages));
    let flat = match m2_ir::check(&ir, packages) {
        Ok(f) => Some(f),
        Err(d) => {
            diags.extend(d);
            None
        }
    };
    diags.extend(timeline(project, &ir));
    if !diags.is_empty() {
        return Err(diags);
    }

    let used: Vec<(String, String)> = ir.modules.iter().flat_map(|m| m.lanes.iter().map(|l| (m.name.clone(), l.id.clone()))).collect();
    let warnings = project
        .timeline
        .unused(&used)
        .into_iter()
        .map(|f| Diagnostic { message: "no lane uses this file".into(), src: Some(relative(&project.dir, &f)), ..Default::default() })
        .collect();
    Ok(Compiled { ir, flat: flat.expect("no diagnostics means the graph checked"), sites, outputs, warnings })
}

/// Module names become file names (`Lead Vox` → `timeline/lead-vox.*.json`), so two
/// names that normalise the same would share files.
fn names(ir: &Ir) -> Vec<Diagnostic> {
    let mut diags = vec![];
    let mut seen: HashMap<String, &str> = HashMap::new();
    for m in &ir.modules {
        let s = slug(&m.name);
        let d = |message: String| Diagnostic { message, module: Some(m.name.clone()), src: m.src.clone(), ..Default::default() };
        if s.is_empty() {
            diags.push(d(format!("the name `{}` needs at least one letter or digit", m.name)));
        } else if let Some(other) = seen.insert(s.clone(), &m.name) {
            diags.push(d(format!("`{}` and `{other}` would share the timeline files `timeline/{s}.*.json`; rename one", m.name)));
        }
    }
    diags
}

/// Describe the hosted plugins the song uses, and check their saved states exist.
fn plugins(project: &Project, ir: &Ir, packages: &mut PackageSet) -> Vec<Diagnostic> {
    let mut diags = vec![];
    let mut ids = vec![];
    for m in &ir.modules {
        for n in &m.nodes {
            if let m2_ir::Op::Plugin { plugin, state, .. } = &n.op {
                ids.push(plugin.as_str());
                if let Some(s) = state {
                    let path = m2_runtime::plugin_state_path(&project.dir.join("assets"), s);
                    if !path.exists() {
                        diags.push(Diagnostic {
                            message: format!("no saved plugin state `{s}` ({}); create it with `m2 plugin-state`", relative(&project.dir, &path)),
                            module: Some(m.name.clone()),
                            node: Some(n.id.clone()),
                            src: n.src.clone(),
                        });
                    }
                }
            }
        }
    }
    diags.extend(packages.describe_plugins(ids).into_iter().map(Diagnostic::new));
    diags
}

/// Every lane's data must parse as the lane's kind.
fn timeline(project: &Project, ir: &Ir) -> Vec<Diagnostic> {
    let tl = &project.timeline;
    let mut diags = vec![];
    if let Err(e) = tl.tempo() {
        diags.push(Diagnostic { message: e.to_string(), src: Some("timeline/song.json".into()), ..Default::default() });
    }
    for m in &ir.modules {
        for lane in &m.lanes {
            let result = match lane.kind {
                Kind::Events(_) => tl.notes(&m.name, &lane.id).map(drop),
                Kind::Control(_) => tl.control(&m.name, &lane.id).map(drop),
                Kind::Audio(_) => tl.audio(&m.name, &lane.id).and_then(|clips| {
                    for c in clips {
                        let path = project.dir.join("assets").join(&c.asset);
                        m2_runtime::audio::load_wav(&path).map_err(|e| anyhow::anyhow!("clip at beat {}: {e:#}", c.start))?;
                    }
                    Ok(())
                }),
            };
            if let Err(e) = result {
                diags.push(Diagnostic {
                    message: format!("lane `{}` ({}): {}", lane.id, lane.kind, e.root_cause()),
                    module: Some(m.name.clone()),
                    src: tl.lane_file(&m.name, &lane.id).map(|f| relative(&project.dir, f)),
                    ..Default::default()
                });
            }
        }
    }
    diags
}

fn relative(dir: &Path, file: &Path) -> String {
    file.strip_prefix(dir).unwrap_or(file).display().to_string()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn std_packages() -> PackageSet {
        let mut p = PackageSet::default();
        p.load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")).unwrap();
        p
    }

    fn project(files: &[(&str, &str)]) -> PathBuf {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("m2-session-test-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
        for (name, text) in files {
            let p = dir.join(name);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        dir
    }

    const SONG: &str = r#"import { song } from "@m2/core";
import { kick } from "std/instruments";
import { track } from "@m2/core";
const k = track("Kick", ({ lane }) => kick(lane.notes("hits")));
export default song(() => k.out);
"#;

    fn compile_dir(dir: &Path) -> Result<Compiled, Vec<Diagnostic>> {
        compile(&Project::open(dir).unwrap(), &mut std_packages())
    }

    #[test]
    fn the_example_compiles() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song");
        let c = compile_dir(&dir).unwrap_or_else(|e| panic!("{e:?}"));
        assert!(c.warnings.is_empty(), "{:?}", c.warnings);
    }

    #[test]
    fn bad_timeline_data_fails_compile_not_render() {
        let dir = project(&[("song.ts", SONG), ("timeline/kick.hits.json", r#"{"clips": [{"start": 0, "length": 4, "notes": [{"pitch": "H9", "start": 0, "length": 1}]}]}"#)]);
        let e = compile_dir(&dir).err().unwrap();
        assert!(e[0].message.contains("lane `hits`") && e[0].message.contains("bad pitch `H9`"), "{}", e[0]);
        assert_eq!(e[0].src.as_deref(), Some("timeline/kick.hits.json"));
        std::fs::remove_dir_all(dir).ok();

        // Control data in a notes lane is the wrong shape.
        let dir = project(&[("song.ts", SONG), ("timeline/kick.hits.json", r#"{"clips": [{"start": 0, "length": 4, "points": [[0, 1]]}]}"#)]);
        assert!(compile_dir(&dir).is_err());
        std::fs::remove_dir_all(dir).ok();

        let dir = project(&[("song.ts", SONG), ("timeline/song.json", r#"{"tempo": [{"beat": 0, "bpm": 5}]}"#)]);
        let e = compile_dir(&dir).err().unwrap();
        assert!(e[0].message.contains("out of range"), "{}", e[0]);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn names_that_share_files_are_rejected() {
        let src = r#"import { song, track } from "@m2/core";
import { kick } from "std/instruments";
import { add } from "std/math";
const a = track("Bass", ({ lane }) => kick(lane.notes("n")));
const b = track("bass", ({ lane }) => kick(lane.notes("n")));
export default song(() => add(a.out, b.out));
"#;
        let dir = project(&[("song.ts", src)]);
        let e = compile_dir(&dir).err().unwrap();
        assert!(e.iter().any(|d| d.message.contains("would share the timeline files `timeline/bass.*.json`")), "{e:?}");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn unused_lane_files_are_warnings() {
        let dir = project(&[("song.ts", SONG), ("timeline/kick.old.json", r#"{"clips": []}"#)]);
        let c = compile_dir(&dir).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(c.warnings.len(), 1);
        assert_eq!(c.warnings[0].src.as_deref(), Some("timeline/kick.old.json"));
        std::fs::remove_dir_all(dir).ok();
    }
}
