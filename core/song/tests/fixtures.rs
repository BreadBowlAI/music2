//! The fixtures' data files are in their canonical form (FMT-8a): reading each and writing it
//! again gives the same bytes. `M2_FIXTURES=<dir>` checks the projects in another folder (such
//! as a conversion's output) instead of the repo's.

use std::path::{Path, PathBuf};

use m2_song::{Arrangement, DataPath, Holds, LaneData, LaneKind, Sidecar, canonical};

fn projects() -> Vec<PathBuf> {
    let roots: Vec<PathBuf> = match std::env::var_os("M2_FIXTURES") {
        Some(d) => vec![PathBuf::from(d)],
        None => ["examples", "templates"].iter().map(|r| Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(r)).collect(),
    };
    let mut out = vec![];
    for root in roots {
        let mut stack = vec![root];
        while let Some(d) = stack.pop() {
            if d.join("music2.toml").exists() {
                out.push(d);
                continue;
            }
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                if e.file_type().is_ok_and(|t| t.is_dir()) {
                    stack.push(e.path());
                }
            }
        }
    }
    out
}

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            files(&e.path(), out);
        } else {
            out.push(e.path());
        }
    }
}

// FMT-8a gate: reading and writing each canonical data file gives the same bytes.
#[test]
fn fixture_data_files_are_canonical() {
    let mut checked = 0;
    for project in projects() {
        let data = project.join("data");
        let mut all = vec![];
        files(&data, &mut all);
        for f in all {
            let rel = f.strip_prefix(&project).unwrap();
            let text = std::fs::read_to_string(&f).unwrap_or_default();
            checked += 1;
            match DataPath::parse(rel) {
                DataPath::Arrangement => assert_eq!(Arrangement::read(&text).unwrap().canonical(), text, "{}", f.display()),
                DataPath::Lane { .. } => {
                    let unit = serde_json::from_str::<serde_json::Value>(&text).unwrap()["unit"].as_str().map(str::to_string);
                    let kind = match Holds::of(&text) {
                        Holds::Notes => LaneKind::Notes,
                        Holds::Audio => LaneKind::Audio,
                        Holds::Control | Holds::Empty => LaneKind::Control { unit: unit.clone() },
                        Holds::Unknown => panic!("{}: no lane can read it", f.display()),
                    };
                    let read = LaneData::read(&text, &kind).unwrap_or_else(|e| panic!("{}: {e:?}", f.display()));
                    assert_eq!(canonical(&read), text, "{}", f.display());
                }
                DataPath::PluginState { sidecar: true, .. } => {
                    assert_eq!(Sidecar::read(&text).unwrap().canonical(), text, "{}", f.display())
                }
                DataPath::PluginState { sidecar: false, .. } => {}
                other => panic!("{}: not a data file ({other:?})", f.display()),
            }
        }
    }
    assert!(std::env::var_os("M2_FIXTURES").is_none() || checked > 0, "no data files under M2_FIXTURES");
}
