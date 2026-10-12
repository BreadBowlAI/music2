//! What music2 ships with: the std package, the song templates and the examples.
//!
//! Each is found, in order: at its environment override (`$M2_STD`, `$M2_TEMPLATES`,
//! `$M2_EXAMPLES`), in the app bundle's `Contents/Resources` next to the running program
//! (`music2.app/Contents/MacOS/music2`), or in this source tree, for `cargo run` and tests.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Where music2 keeps what it can always rebuild, outside every project: the hash cache of
/// assets and plugin states (FMT-16), later decoded and warped renders (RENDER-30).
/// `~/Library/Caches/music2`.
pub fn cache_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").filter(|h| !h.is_empty())?;
    Some(PathBuf::from(home).join("Library").join("Caches").join("music2"))
}

/// Keep the hash cache on disk, so a project's assets are hashed once, not at every open.
pub fn keep_hashes() {
    if let Some(dir) = cache_dir() {
        m2_song::HashCache::global().persist_in(&dir);
    }
}

/// The std package: `m2pkg.toml`, `kernels/*.wasm` and `src/*.ts`.
pub fn std_dir() -> PathBuf {
    resolve("M2_STD", "std")
}

/// Song templates, one complete project per folder, each with a `template.toml`.
pub fn templates_dir() -> PathBuf {
    resolve("M2_TEMPLATES", "templates")
}

/// Example songs, one project per folder.
pub fn examples_dir() -> PathBuf {
    resolve("M2_EXAMPLES", "examples")
}

fn resolve(var: &str, name: &str) -> PathBuf {
    let exe = std::env::current_exe().ok();
    // A symlink to the program (say in ~/bin) still finds the bundle it points into.
    let real = exe.as_deref().and_then(|e| e.canonicalize().ok());
    find(std::env::var_os(var), &[exe.as_deref(), real.as_deref()], name)
}

/// The override if set, else the first bundle with `name` in its Resources, else the source tree.
fn find(over: Option<OsString>, exes: &[Option<&Path>], name: &str) -> PathBuf {
    if let Some(dir) = over.filter(|v| !v.is_empty()) {
        return dir.into();
    }
    exes.iter().flatten().find_map(|exe| bundled(exe, name)).unwrap_or_else(|| source_tree().join(name))
}

/// `X.app/Contents/MacOS/prog` → `X.app/Contents/Resources/<name>`, if it is there.
fn bundled(exe: &Path, name: &str) -> Option<PathBuf> {
    let contents = exe.parent()?.parent()?;
    let dir = contents.join("Resources").join(name);
    dir.is_dir().then_some(dir)
}

/// The repository this was built from.
fn source_tree() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Temp(PathBuf);

    impl Temp {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("m2-resources-{tag}-{}", std::process::id()));
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

    /// A fake `music2.app` laid out as `tauri build` makes it, with std and templates
    /// but no examples.
    fn fake_bundle(root: &Path) -> PathBuf {
        let contents = root.join("music2.app/Contents");
        let exe = contents.join("MacOS/music2");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, "").unwrap();
        std::fs::create_dir_all(contents.join("Resources/std/kernels")).unwrap();
        std::fs::write(contents.join("Resources/std/m2pkg.toml"), "").unwrap();
        std::fs::create_dir_all(contents.join("Resources/templates/starter")).unwrap();
        exe
    }

    #[test]
    fn the_app_bundle_finds_its_resources_next_to_the_program() {
        let t = Temp::new("bundle");
        let exe = fake_bundle(&t.0);
        let resources = t.0.join("music2.app/Contents/Resources");
        assert_eq!(find(None, &[Some(&exe)], "std"), resources.join("std"));
        assert_eq!(find(None, &[Some(&exe)], "templates"), resources.join("templates"));
        // Not bundled: the source tree's.
        assert_eq!(find(None, &[Some(&exe)], "examples"), source_tree().join("examples"));
    }

    #[test]
    fn an_override_wins_and_an_empty_one_is_ignored() {
        let t = Temp::new("override");
        let exe = fake_bundle(&t.0);
        assert_eq!(find(Some("/elsewhere/std".into()), &[Some(&exe)], "std"), PathBuf::from("/elsewhere/std"));
        assert_eq!(find(Some("".into()), &[Some(&exe)], "std"), t.0.join("music2.app/Contents/Resources/std"));
    }

    #[test]
    fn a_program_outside_a_bundle_uses_the_source_tree() {
        let t = Temp::new("plain");
        let exe = t.0.join("target/debug/music2-desktop");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        assert_eq!(find(None, &[Some(&exe), None], "std"), source_tree().join("std"));
        assert_eq!(find(None, &[], "templates"), source_tree().join("templates"));
        // The source tree's std is the real package.
        assert!(std_dir().join("m2pkg.toml").is_file() || std::env::var_os("M2_STD").is_some());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_program_finds_the_bundle_it_points_into() {
        let t = Temp::new("link");
        let exe = fake_bundle(&t.0);
        let link = t.0.join("bin/music2");
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&exe, &link).unwrap();
        let real = link.canonicalize().unwrap();
        let found = find(None, &[Some(&link), Some(&real)], "std");
        assert_eq!(found.canonicalize().unwrap(), t.0.join("music2.app/Contents/Resources/std").canonicalize().unwrap());
    }
}
