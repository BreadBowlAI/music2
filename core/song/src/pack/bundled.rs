//! The packs music2 ships with (PACK-15 to PACK-17): its own sounds, `@m2/synths`, `@m2/drums`
//! and `@m2/effects`, each a pack's folder under a `packs` folder found as std is
//! (`packs/@m2/synths` …). When the app or `m2` starts, each version the store hasn't got is
//! installed into it, as any pack is (D7): one store, one path.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use super::{Install, Listing, PKG_MANIFEST, Store};
use crate::hash::Sha;

/// Every pack folder in `from`, `<from>/@owner/name` with its `m2pkg.toml`, sorted.
pub fn bundled(from: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    for owner in std::fs::read_dir(from).into_iter().flatten().flatten() {
        for pack in std::fs::read_dir(owner.path()).into_iter().flatten().flatten() {
            if pack.path().join(PKG_MANIFEST).is_file() {
                out.push(pack.path());
            }
        }
    }
    out.sort();
    out
}

/// Install every pack bundled in `from` into `store` (PACK-16): a version the store hasn't got is
/// copied in, one there with its hash and its files whole is left as it is (nothing is written),
/// one whose copy changed is put back, and one there with another hash is refused (PACK-17). Each
/// folder with what it did, or why it wasn't installed, in words: the caller says so and goes on.
pub fn install_bundled(from: &Path, store: &Store) -> Vec<(PathBuf, Result<Install, String>)> {
    bundled(from).into_iter().map(|dir| (dir.clone(), store.install(&dir))).collect()
}

/// A store holding the packs this source tree bundles (`packs/`), for tests: a song from the
/// repo builds as it does in the app, and no test reads or writes the store under `$HOME`. One
/// folder in the temp folder for each content of the bundled packs, named by its hash, shared by
/// every test process and installed into once (`Store::install` lets two processes race).
pub fn test_store() -> PathBuf {
    static STORE: OnceLock<PathBuf> = OnceLock::new();
    STORE
        .get_or_init(|| {
            let from = source_packs();
            let hashes: Vec<String> =
                bundled(&from).iter().map(|d| Listing::of(d).expect("a bundled pack lists").hash().to_string()).collect();
            let key = Sha::of(hashes.join("\n").as_bytes()).to_string();
            let key = key.trim_start_matches("sha256:");
            let dir = std::env::temp_dir().join(format!("m2-bundled-{}", &key[..16]));
            for (pack, done) in install_bundled(&from, &Store::at(dir.clone())) {
                if let Err(e) = done {
                    panic!("{} doesn't install into the tests' store {}: {e}", pack.display(), dir.display());
                }
            }
            dir
        })
        .clone()
}

/// The packs this source tree bundles, as installed in [`test_store`]: each by its name, in its
/// version's folder there. For tests that hand a build its packages by name and folder.
pub fn test_packs() -> Vec<(String, PathBuf)> {
    let store = Store::at(test_store());
    bundled(&source_packs())
        .iter()
        .map(|dir| {
            let m = super::PkgManifest::at(dir).expect("a bundled pack reads");
            (m.package.name.as_str().to_string(), store.version_dir(&m.package.name, &m.package.version))
        })
        .collect()
}

/// The source tree's `packs/`.
pub fn source_packs() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs")
}
