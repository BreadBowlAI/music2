//! What music2 ships with: the std package, the song templates and the examples.
//!
//! Each is found, in order: at its environment override (`$M2_STD`, `$M2_TEMPLATES`,
//! `$M2_EXAMPLES`), in the app bundle's `Contents/Resources` next to the running program
//! (`music2.app/Contents/MacOS/music2`), or in this source tree, for `cargo run` and tests.
//! And the `m2` a new song's `AGENTS.md` names ([`m2_program`], FMT-23).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Where music2 keeps what it can always rebuild, outside every project: the hash cache of
/// assets and plugin states (FMT-16), later decoded and warped renders (RENDER-30). The OS's
/// cache folder (`m2_os::dirs::cache`): `~/Library/Caches/music2` on macOS, `~/.cache/music2` on
/// Linux.
pub fn cache_dir() -> Option<PathBuf> {
    m2_os::dirs::cache()
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

/// The `m2` a new song's `AGENTS.md` names (FMT-23).
#[derive(Clone, Debug, PartialEq)]
pub enum M2 {
    /// This one, built: its real path.
    At(PathBuf),
    /// From source, not built yet: where `cargo build --locked --release -p m2` in `tree` puts it.
    Unbuilt { at: PathBuf, tree: PathBuf },
}

/// The `m2` to name in a new song's `AGENTS.md` (FMT-23), in order: the running program when it
/// is `m2` (`m2 new`); `m2` beside it (`music2.app/Contents/MacOS/m2`, which the app carries, or
/// `target/release/m2` beside `target/release/music2-desktop`); the source tree's
/// `target/release/m2`, then `target/debug/m2`; that release one not built yet. None when there is
/// no source tree either (an app that carries no m2).
pub fn m2_program() -> Option<M2> {
    let exe = std::env::current_exe().ok();
    let real = exe.as_deref().and_then(|e| e.canonicalize().ok());
    find_m2(&[exe.as_deref(), real.as_deref()], &source_tree())
}

/// [`m2_program`], for the running program `exes` (as named, and its real path) and the source
/// tree `tree`.
fn find_m2(exes: &[Option<&Path>], tree: &Path) -> Option<M2> {
    let name = format!("m2{}", std::env::consts::EXE_SUFFIX);
    let real = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let exes: Vec<&Path> = exes.iter().flatten().copied().collect();
    if let Some(me) = exes.iter().find(|e| e.file_name().is_some_and(|n| *n == *name) && e.is_file()) {
        return Some(M2::At(real(me)));
    }
    let beside = exes.iter().filter_map(|e| Some(e.parent()?.join(&name)));
    let built = ["release", "debug"].map(|profile| tree.join("target").join(profile).join(&name));
    if let Some(m2) = beside.chain(built).find(|p| p.is_file()) {
        return Some(M2::At(real(&m2)));
    }
    tree.join("apps/m2/Cargo.toml").is_file().then(|| {
        let tree = real(tree);
        M2::Unbuilt { at: tree.join("target/release").join(&name), tree }
    })
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

    /// An empty file at `path`, its folders made.
    fn touch(path: &Path) -> PathBuf {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "").unwrap();
        path.canonicalize().unwrap()
    }

    /// A folder shaped like the source tree: `apps/m2/Cargo.toml`.
    fn fake_tree(root: &Path) -> PathBuf {
        touch(&root.join("tree/apps/m2/Cargo.toml"));
        root.join("tree").canonicalize().unwrap()
    }

    fn m2_name() -> String {
        format!("m2{}", std::env::consts::EXE_SUFFIX)
    }

    // FMT-23: m2 itself names itself (`m2 new`, `m2 docs agents`), wherever it is.
    #[test]
    fn m2_names_itself() {
        let t = Temp::new("m2-self");
        let me = touch(&t.0.join("bin").join(m2_name()));
        let tree = fake_tree(&t.0);
        touch(&tree.join("target/release").join(m2_name()));
        assert_eq!(find_m2(&[Some(&me)], &tree), Some(M2::At(me)));
    }

    // FMT-23: the app names the m2 music2.app carries beside it, Contents/MacOS/m2.
    #[test]
    fn the_app_names_the_m2_beside_it_in_its_bundle() {
        let t = Temp::new("m2-bundle");
        let exe = fake_bundle(&t.0);
        let tree = fake_tree(&t.0);
        touch(&tree.join("target/release").join(m2_name()));
        let bundled = touch(&exe.parent().unwrap().join(m2_name()));
        assert_eq!(bundled, t.0.join("music2.app/Contents/MacOS").join(m2_name()).canonicalize().unwrap());
        assert_eq!(find_m2(&[Some(&exe), None], &tree), Some(M2::At(bundled)));
    }

    // FMT-23: run from source, the app names the m2 built beside it, then the source tree's
    // release m2, then its debug one, then the release one not built yet, with the line that
    // builds it; with no source tree there, none.
    #[test]
    fn from_source_the_app_names_the_m2_it_finds_in_target() {
        let t = Temp::new("m2-source");
        let tree = fake_tree(&t.0);
        let exe = touch(&t.0.join("elsewhere/release/music2-desktop"));
        assert_eq!(
            find_m2(&[Some(&exe)], &tree),
            Some(M2::Unbuilt { at: tree.join("target/release").join(m2_name()), tree: tree.clone() }),
            "not built yet"
        );
        let debug = touch(&tree.join("target/debug").join(m2_name()));
        assert_eq!(find_m2(&[Some(&exe)], &tree), Some(M2::At(debug)));
        let release = touch(&tree.join("target/release").join(m2_name()));
        assert_eq!(find_m2(&[Some(&exe)], &tree), Some(M2::At(release)), "release before debug");
        let beside = touch(&exe.parent().unwrap().join(m2_name()));
        assert_eq!(find_m2(&[Some(&exe)], &tree), Some(M2::At(beside)), "the one beside the app first");
        // An app that carries no m2, on a computer without the source.
        let bare = fake_bundle(&t.0.join("bare"));
        assert_eq!(find_m2(&[Some(&bare)], &t.0.join("no-tree")), None);
        // This repo is a source tree.
        assert!(source_tree().join("apps/m2/Cargo.toml").is_file());
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
