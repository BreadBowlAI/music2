//! What keeps core small (WRITE-9, WRITE-1, P10; the step 6 gate): no core crate depends on the
//! editor, core defines no gesture, and the write bypasses are closed.

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every `.rs` file under `dir`, recursively.
fn sources(dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(sources(&p));
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
    out
}

/// What `file` says outside its tests: the code up to its `#[cfg(test)] mod tests`, if it has one
/// inline, and nothing for a file of tests.
fn code(file: &Path) -> String {
    let name = file.file_name().unwrap().to_string_lossy();
    if name.ends_with("_tests.rs") || name == "tests.rs" || name == "props.rs" || name == "testing.rs" {
        return String::new();
    }
    let text = std::fs::read_to_string(file).unwrap();
    match text.find("#[cfg(test)]\nmod tests {") {
        Some(at) => text[..at].to_string(),
        None => text,
    }
}

// WRITE-9: adding a gesture never changes core: no core crate depends on the editor, in any
// dependency table.
#[test]
fn core_never_depends_on_the_editor() {
    for e in std::fs::read_dir(root().join("core")).unwrap().flatten() {
        let manifest = e.path().join("Cargo.toml");
        let Ok(text) = std::fs::read_to_string(&manifest) else { continue };
        assert!(!text.contains("m2-editor"), "{} depends on the editor", manifest.display());
    }
}

// WRITE-9, P10: core defines no gesture; the editor builds them as Tx.
#[test]
fn core_defines_no_gesture() {
    for file in sources(&root().join("core/session/src")) {
        let text = code(&file);
        for gesture in ["fn connect(", "fn disconnect(", "fn add_effect(", "fn remove_effect(", "fn set(", "fn write_notes(", "fn view("] {
            assert!(!text.contains(gesture), "{} defines `{gesture}`", file.display());
        }
    }
}

// WRITE-1, the step 6 gate: `write_whole` is no public writer in core, the server's guess at a
// value edit from revision numbers is gone, the CLI writes project files only through the
// session, and no builder takes in the folder itself (R2).
#[test]
fn the_write_bypasses_are_closed() {
    for file in sources(&root().join("core")) {
        let text = code(&file);
        assert!(!text.contains("pub fn write_whole") && !text.contains("pub(crate) fn write_whole"), "{}", file.display());
    }
    for file in sources(&root().join("crates/server/src")) {
        let text = code(&file);
        assert!(!text.contains("fn single(") && !text.contains("revision == before + 1"), "{} guesses", file.display());
    }
    for file in sources(&root().join("apps/m2/src")) {
        assert!(!code(&file).contains("std::fs::write"), "{} writes a file itself", file.display());
    }
    for file in sources(&root().join("crates/editor/src")) {
        assert!(!code(&file).contains(".refresh()"), "{} takes in the folder", file.display());
    }
    let actor = code(&root().join("crates/server/src/graph/actor.rs"));
    let gestures = &actor[actor.find("pub(super) fn commit(").unwrap()..actor.find("pub(super) fn edit(").unwrap()];
    assert!(!gestures.contains(".refresh()"), "a value edit takes in the folder first");
}
