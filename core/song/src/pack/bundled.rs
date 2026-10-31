//! The packs music2 ships with (PACK-15 to PACK-17): its own sounds, `@m2/synths`, `@m2/drums`
//! and `@m2/effects`, each a pack's folder under a `packs` folder found as std is
//! (`packs/@m2/synths` …). When the app or `m2` starts, each version the store hasn't got is
//! installed into it, as any pack is (D7): one store, one path.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use super::{Install, PKG_MANIFEST, PackName, Store};
use crate::hash::Sha;
use crate::manifest::Version;

/// `packs/versions.toml`: every version of music2's own packs the source tree has bundled, with
/// its hash, compiled in (PACK-17, PACK-19).
const VERSIONS: &str = include_str!("../../../../packs/versions.toml");

/// Each version `packs/versions.toml` records, with its hash: `"@m2/synths 1.0.0" = "sha256:…"`.
pub fn recorded() -> &'static [(PackName, Version, Sha)] {
    static RECORDED: OnceLock<Vec<(PackName, Version, Sha)>> = OnceLock::new();
    RECORDED.get_or_init(|| {
        let table: toml::Table = VERSIONS.parse().expect("packs/versions.toml is TOML");
        table
            .iter()
            .map(|(key, hash)| {
                let read = || -> Option<(PackName, Version, Sha)> {
                    let (name, version) = key.split_once(' ')?;
                    Some((PackName::new(name).ok()?, Version::parse(version)?, Sha::parse_prefixed(hash.as_str()?)?))
                };
                read().unwrap_or_else(|| panic!("packs/versions.toml: \"{key}\" isn't `\"<pack> <version>\" = \"sha256:…\"`"))
            })
            .collect()
    })
}

/// Whether version `version` of `name`, with these files (`hash`), is music2's own (PACK-19): one
/// it bundles, or bundled before. The name alone says nothing: anyone can name a folder's pack
/// `@m2/…`.
pub fn own(name: &PackName, version: &Version, hash: &Sha) -> bool {
    recorded().iter().any(|(n, v, h)| n == name && v == version && h == hash)
}

/// The `m2pkg.toml` of each pack this source tree bundles, compiled in: what music2 comes with.
const BUNDLES: [&str; 4] = [
    include_str!("../../../../packs/@m2/drums/m2pkg.toml"),
    include_str!("../../../../packs/@m2/effects/m2pkg.toml"),
    include_str!("../../../../packs/@m2/mixing/m2pkg.toml"),
    include_str!("../../../../packs/@m2/synths/m2pkg.toml"),
];

/// The version of `name` this music2 comes with (PACK-15), if it is one of its own packs: the one
/// it bundles, not every version it ever bundled ([`recorded`]).
pub fn comes_with(name: &PackName) -> Option<Version> {
    BUNDLES
        .iter()
        .map(|t| super::PkgManifest::read(t).expect("a bundled m2pkg.toml reads"))
        .find(|m| &m.package.name == name)
        .map(|m| m.package.version)
}

/// What to do first when a song asks for (or keeps) `asked` of one of music2's own packs, which
/// music2 no longer comes with: the line to write for the version it comes with now (review Y5).
/// None for another pack, or one music2 comes with that `asked` already is.
// The upgrade track (FMT-3a) puts "run `m2 upgrade <song>`" first here once it exists.
pub fn move_on(name: &PackName, asked: &super::Requirement) -> Option<String> {
    let now = comes_with(name).filter(|v| !asked.matches(*v))?;
    let line = super::Requirement::admitting(now);
    Some(format!(
        "music2 now comes with {name} {now}: change its line in music2.toml to \"{name}\" = \"{line}\", then run `m2 pack lock <song>`. \
         The song's code may need changes"
    ))
}

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

/// A scratch home folder for tests, `HOME` for a program they run: its store of packs
/// (`Music/music2/Packs`, PLAT-7) is [`test_store`], with the packs this source tree bundles
/// installed, so the program finds them there and writes nothing. One for each content of the
/// bundled packs, named by its hash, so none holds another tree's bytes under a version.
#[cfg(any(test, feature = "testing"))]
pub fn test_home() -> PathBuf {
    static HOME: OnceLock<PathBuf> = OnceLock::new();
    HOME.get_or_init(|| {
        let from = source_packs();
        let hashes: Vec<String> =
            bundled(&from).iter().map(|d| super::Listing::of(d).expect("a bundled pack lists").hash().to_string()).collect();
        let key = Sha::of(hashes.join("\n").as_bytes()).to_string();
        let key = key.trim_start_matches("sha256:");
        let home = std::env::temp_dir().join(format!("m2-tests-{}", &key[..16]));
        let store = m2_os::dirs::packs_under(&home);
        for (pack, done) in install_bundled(&from, &Store::at(store.clone())) {
            if let Err(e) = done {
                panic!("{} doesn't install into the tests' store {}: {e}", pack.display(), store.display());
            }
        }
        home
    })
    .clone()
}

/// A store holding the packs this source tree bundles (`packs/`), for tests: a song from the
/// repo builds as it does in the app, and no test reads or writes the store under `$HOME`. The
/// store of [`test_home`], shared by every test process and installed into once (`Store::install`
/// lets two processes race).
#[cfg(any(test, feature = "testing"))]
pub fn test_store() -> PathBuf {
    m2_os::dirs::packs_under(&test_home())
}

/// The packs this source tree bundles, as installed in [`test_store`]: each by its name, in its
/// version's folder there. For tests that hand a build its packages by name and folder.
#[cfg(any(test, feature = "testing"))]
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

/// The source tree's `packs/`, for tests and the tools run from it.
#[cfg(any(test, feature = "testing"))]
pub fn source_packs() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs")
}
