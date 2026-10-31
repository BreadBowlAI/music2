//! What music2 ships with: the std package, its own packs (PACK-15), the song templates and the
//! examples.
//!
//! Each is found, in order (PLAT-8): at its environment override (`$M2_STD`, `$M2_PACKS`,
//! `$M2_TEMPLATES`, `$M2_EXAMPLES`); in the bundle the running program is in, where Tauri puts its resources:
//! `Contents/Resources` beside `music2.app/Contents/MacOS/music2`, or `lib/music2` beside
//! `bin/` in the Linux packages (`/usr/bin/music2` and `/usr/lib/music2` in the `.deb`, the same
//! inside the AppImage, `bin/m2` and `lib/music2` in m2's `.tar.gz`); or in this source tree, for
//! `cargo run` and tests. And the `m2` a new song's `AGENTS.md` names ([`m2_program`], FMT-23).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Where music2 keeps what it can always rebuild, outside every project: the hash cache of
/// assets and plugin states (FMT-16), later decoded and warped renders (RENDER-30). The OS's
/// cache folder (`m2_os::dirs::cache`): `~/Library/Caches/music2` on macOS, `~/.cache/music2` on
/// Linux.
pub fn cache_dir() -> Option<PathBuf> {
    m2_os::dirs::cache()
}

/// Keep the cache folder from now on (PLAT-7): a program's `main` calls it first, never a test
/// ([`m2_os::dirs::keep_cache`]).
pub fn keep_cache() {
    m2_os::dirs::keep_cache();
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

/// The packs music2 ships (PACK-15): `@m2/synths`, `@m2/drums` and `@m2/effects`, each a pack's
/// folder, `packs/@m2/<name>`. Nothing reads them here but [`install_packs`]: songs and the lists
/// read them from the store, as any pack (PACK-16).
pub fn packs_dir() -> PathBuf {
    resolve("M2_PACKS", "packs")
}

/// Install the packs music2 ships into the store at `store` (PACK-16): each version it hasn't got;
/// one it holds with its hash and its files whole is left alone, so a start that finds them all
/// writes nothing. One it holds with another hash is refused (PACK-17), and the log says what puts
/// music2's back; the others go on. `quiet` leaves out what went as it should, as `m2` does: its
/// output is its answer. Only a program's start calls it: the app, the headless server, `m2`.
pub fn install_packs(store: &Path, quiet: bool) {
    for line in installing_packs(&packs_dir(), store, quiet) {
        eprintln!("music2: {line}");
    }
}

/// [`install_packs`] from `from`: what it says, a line each.
pub(crate) fn installing_packs(from: &Path, store: &Path, quiet: bool) -> Vec<String> {
    use m2_song::pack::{Install, Listing, PkgManifest, Store};
    let into = Store::at(store.to_path_buf());
    let from = std::fs::canonicalize(from).unwrap_or_else(|_| from.to_path_buf());
    let mut said = vec![];
    for (dir, done) in m2_song::pack::install_bundled(&from, &into) {
        // Another copy under music2's version: what the user does, not what a pack's author would.
        let theirs = || {
            let m = PkgManifest::at(&dir).ok()?;
            let installed = into.hash(&m.package.name, &m.package.version).ok()?;
            (Listing::of(&dir).ok()?.hash() != installed).then_some(m.package)
        };
        match done {
            Ok(Install::Installed { name, version, .. }) if !quiet => {
                said.push(format!("installed {name} {version}, which comes with music2, into {}", store.display()))
            }
            Ok(Install::Repaired { name, version, why }) if !quiet => {
                said.push(format!("put {name} {version} back as it comes with music2: its installed copy was changed ({why})"))
            }
            Ok(_) => {}
            Err(e) => said.push(match theirs() {
                Some(p) => format!(
                    "{} {} in your packs folder isn't the one music2 comes with. To use music2's: `m2 pack remove {} {}`, then start \
                     music2 again",
                    p.name, p.version, p.name, p.version
                ),
                None => format!("{}, which comes with music2, wasn't installed: {e}", dir.display()),
            }),
        }
    }
    said
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

/// Where Tauri puts the resources, from the folder above the program's: `Resources` in a Mac
/// app's `Contents`, `lib/<productName>` on Linux (`tauri.conf.json`'s productName, music2).
const BUNDLE_LAYOUTS: [&str; 2] = ["Resources", "lib/music2"];

/// `X.app/Contents/MacOS/prog` → `X.app/Contents/Resources/<name>`, or `<prefix>/bin/prog` →
/// `<prefix>/lib/music2/<name>`, whichever is there.
fn bundled(exe: &Path, name: &str) -> Option<PathBuf> {
    let above = exe.parent()?.parent()?;
    BUNDLE_LAYOUTS.iter().map(|layout| above.join(layout).join(name)).find(|dir| dir.is_dir())
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
/// is `m2` (`m2 new`); `m2` beside it (`music2.app/Contents/MacOS/m2` and `/usr/bin/m2`, which the
/// app and the `.deb` carry, or `target/release/m2` beside `target/release/music2-desktop`), but
/// not inside a mounted AppImage (`$APPDIR`), which goes away when the app quits; the source tree's
/// `target/release/m2`, then `target/debug/m2`; that release one not built yet. None when there is
/// no source tree either (an app that carries no m2).
pub fn m2_program() -> Option<M2> {
    let exe = std::env::current_exe().ok();
    let real = exe.as_deref().and_then(|e| e.canonicalize().ok());
    find_m2(&[exe.as_deref(), real.as_deref()], std::env::var_os("APPDIR").as_deref().map(Path::new), &source_tree())
}

/// [`m2_program`], for the running program `exes` (as named, and its real path), the mounted
/// AppImage it may run from (`appdir`) and the source tree `tree`.
fn find_m2(exes: &[Option<&Path>], appdir: Option<&Path>, tree: &Path) -> Option<M2> {
    let name = format!("m2{}", std::env::consts::EXE_SUFFIX);
    let real = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let mounted = appdir.map(real);
    let exes: Vec<&Path> = exes.iter().flatten().copied().filter(|e| !mounted.as_ref().is_some_and(|m| real(e).starts_with(m))).collect();
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
        std::fs::create_dir_all(contents.join("Resources/packs/@m2/synths")).unwrap();
        exe
    }

    #[test]
    fn the_app_bundle_finds_its_resources_next_to_the_program() {
        let t = Temp::new("bundle");
        let exe = fake_bundle(&t.0);
        let resources = t.0.join("music2.app/Contents/Resources");
        assert_eq!(find(None, &[Some(&exe)], "std"), resources.join("std"));
        assert_eq!(find(None, &[Some(&exe)], "templates"), resources.join("templates"));
        // The packs music2 ships, beside std (PACK-15).
        assert_eq!(find(None, &[Some(&exe)], "packs"), resources.join("packs"));
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
        // The source tree's std is the real package, and its packs the real ones.
        assert!(std_dir().join("m2pkg.toml").is_file() || std::env::var_os("M2_STD").is_some());
        assert!(packs_dir().join("@m2/synths/m2pkg.toml").is_file() || std::env::var_os("M2_PACKS").is_some());
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
        assert_eq!(find_m2(&[Some(&me)], None, &tree), Some(M2::At(me)));
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
        assert_eq!(find_m2(&[Some(&exe), None], None, &tree), Some(M2::At(bundled)));
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
            find_m2(&[Some(&exe)], None, &tree),
            Some(M2::Unbuilt { at: tree.join("target/release").join(m2_name()), tree: tree.clone() }),
            "not built yet"
        );
        let debug = touch(&tree.join("target/debug").join(m2_name()));
        assert_eq!(find_m2(&[Some(&exe)], None, &tree), Some(M2::At(debug)));
        let release = touch(&tree.join("target/release").join(m2_name()));
        assert_eq!(find_m2(&[Some(&exe)], None, &tree), Some(M2::At(release)), "release before debug");
        let beside = touch(&exe.parent().unwrap().join(m2_name()));
        assert_eq!(find_m2(&[Some(&exe)], None, &tree), Some(M2::At(beside)), "the one beside the app first");
        // An app that carries no m2, on a computer without the source.
        let bare = fake_bundle(&t.0.join("bare"));
        assert_eq!(find_m2(&[Some(&bare)], None, &t.0.join("no-tree")), None);
        // This repo is a source tree.
        assert!(source_tree().join("apps/m2/Cargo.toml").is_file());
    }

    /// A fake Linux package laid out as `tauri build` makes the `.deb` and the AppImage:
    /// `usr/bin/music2` beside `usr/bin/m2`, and the resources in `usr/lib/music2`, with std and
    /// templates but no examples.
    fn fake_linux_package(root: &Path) -> PathBuf {
        let usr = root.join("usr");
        let exe = touch(&usr.join("bin/music2"));
        touch(&usr.join("lib/music2/std/m2pkg.toml"));
        std::fs::create_dir_all(usr.join("lib/music2/templates/starter")).unwrap();
        std::fs::create_dir_all(usr.join("lib/music2/packs/@m2/synths")).unwrap();
        exe
    }

    // PLAT-8: the `.deb` (`/usr/bin/music2`, `/usr/lib/music2/`), the AppImage (the same inside
    // it) and the `.tar.gz` of m2 (`bin/m2`, `lib/music2/`) find what they carry in
    // `<program's folder>/../lib/music2`, where Tauri puts its resources on Linux.
    #[test]
    fn a_linux_package_finds_its_resources_in_lib_music2() {
        let t = Temp::new("linux");
        let exe = fake_linux_package(&t.0);
        let lib = t.0.join("usr/lib/music2").canonicalize().unwrap();
        let found = |name| find(None, &[Some(&exe)], name).canonicalize().unwrap();
        assert_eq!(found("std"), lib.join("std"));
        assert_eq!(found("templates"), lib.join("templates"));
        assert_eq!(found("packs"), lib.join("packs"));
        // Not carried: the source tree's.
        assert_eq!(find(None, &[Some(&exe)], "examples"), source_tree().join("examples"));
        // m2 in the .tar.gz: bin/m2 and lib/music2.
        let m2 = touch(&t.0.join("m2-0.1.0-linux-x86_64/bin").join(m2_name()));
        std::fs::create_dir_all(t.0.join("m2-0.1.0-linux-x86_64/lib/music2/examples/first-song")).unwrap();
        let tar = t.0.join("m2-0.1.0-linux-x86_64/lib/music2").canonicalize().unwrap();
        assert_eq!(find(None, &[Some(&m2)], "examples").canonicalize().unwrap(), tar.join("examples"));
        // The program's own folder is never a bundle: a lib/music2 beside it isn't looked at.
        std::fs::create_dir_all(t.0.join("usr/bin/lib/music2/examples")).unwrap();
        assert_eq!(find(None, &[Some(&exe)], "examples"), source_tree().join("examples"));
    }

    // FMT-23, PLAT-8: the .deb names the m2 beside the app, /usr/bin/m2.
    #[test]
    fn a_linux_package_names_the_m2_beside_the_app() {
        let t = Temp::new("m2-deb");
        let exe = fake_linux_package(&t.0);
        let m2 = touch(&t.0.join("usr/bin").join(m2_name()));
        assert_eq!(find_m2(&[Some(&exe)], None, &t.0.join("no-tree")), Some(M2::At(m2.clone())));
        // $APPDIR set, but the program isn't under it: still named.
        assert_eq!(find_m2(&[Some(&exe)], Some(&t.0.join("other")), &t.0.join("no-tree")), Some(M2::At(m2)));
    }

    // FMT-23: inside a mounted AppImage ($APPDIR, gone when the app quits), the app names no m2
    // there: the source tree's, else none, so the song keeps the template's line.
    #[test]
    fn an_appimage_names_no_m2_inside_it() {
        let t = Temp::new("m2-appimage");
        let appdir = t.0.join("mount");
        let exe = fake_linux_package(&appdir);
        touch(&appdir.join("usr/bin").join(m2_name()));
        let real = exe.canonicalize().unwrap();
        assert_eq!(find_m2(&[Some(&exe), Some(&real)], Some(&appdir), &t.0.join("no-tree")), None);
        let tree = fake_tree(&t.0);
        let release = touch(&tree.join("target/release").join(m2_name()));
        assert_eq!(find_m2(&[Some(&exe), Some(&real)], Some(&appdir), &tree), Some(M2::At(release)));
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

    // PACK-17, PACK-19 (review S7): a store holding another copy under music2's version tells the
    // user what puts music2's back, in their words: no pack author's advice, no path with `..`.
    #[test]
    fn a_start_that_finds_another_copy_says_what_puts_music2s_back() {
        let t = Temp::new("theirs");
        let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs");
        let theirs = t.0.join("theirs");
        let ours = from.join("@m2/synths");
        let mut stack = vec![(ours.clone(), theirs.clone())];
        while let Some((a, b)) = stack.pop() {
            std::fs::create_dir_all(&b).unwrap();
            for e in std::fs::read_dir(&a).unwrap().flatten() {
                match e.file_type().unwrap().is_dir() {
                    true => stack.push((e.path(), b.join(e.file_name()))),
                    false => drop(std::fs::copy(e.path(), b.join(e.file_name())).unwrap()),
                }
            }
        }
        std::fs::write(theirs.join("README.md"), "theirs\n").unwrap();
        let store = m2_song::pack::Store::at(t.0.join("Packs"));
        store.install(&theirs).unwrap();
        let said = installing_packs(&from, &store.dir, true);
        assert_eq!(
            said,
            ["@m2/synths 1.0.0 in your packs folder isn't the one music2 comes with. To use music2's: `m2 pack remove @m2/synths \
              1.0.0`, then start music2 again"],
        );
        let loud = installing_packs(&from, &t.0.join("Packs-2"), false);
        assert_eq!(loud.len(), 4, "{loud:?}");
        assert!(loud.iter().all(|l| !l.contains("..")), "{loud:?}");
        m2_song::pack::remove_tree(&t.0).unwrap();
    }
}
