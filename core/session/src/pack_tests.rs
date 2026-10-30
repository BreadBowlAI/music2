//! Packs in a song (PACK-5, PACK-9): the fixture pack `@test/tiny` (`tests/fixtures/packs/tiny`),
//! used from its folder and from a scratch store with a lock.

use std::path::{Path, PathBuf};

use m2_runtime::{PackageSet, Renderer};
use m2_song::pack::{LISTING, Listing, Lock, PackName, Store};
use m2_song::{Project, Version};

use super::*;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/packs")
}

/// A fresh folder for one test.
fn scratch(what: &str) -> PathBuf {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let d = std::env::temp_dir().join(format!("m2-session-packs-{what}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if e.file_type().unwrap().is_dir() { copy(&src, &dst) } else { std::fs::copy(&src, &dst).map(drop).unwrap() }
    }
}

/// std, and the store at `store`.
fn packages(store: &Path) -> PackageSet {
    let mut p = PackageSet::default().with_store(store.to_path_buf());
    p.load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")).unwrap();
    p
}

/// `@test/tiny` installed in `store` as an install leaves it: its files and its listing.
fn install_tiny(store: &Path) -> m2_song::Sha {
    let s = Store::at(store.to_path_buf());
    let dir = s.version_dir(&PackName::new("@test/tiny").unwrap(), &Version(0, 1, 0));
    copy(&fixtures().join("tiny"), &dir);
    let listing = Listing::of(&dir).unwrap();
    std::fs::write(dir.join(LISTING), listing.text()).unwrap();
    listing.hash()
}

fn codes(d: &[Diagnostic]) -> Vec<&'static str> {
    d.iter().map(|d| d.code).collect()
}

// PACK-5, PACK-9: a song builds with a pack from a folder; its ids carry the pack's name, the
// revision records the pack's files and the recording its code reads, and it sounds.
#[test]
fn a_song_builds_with_a_pack_from_its_folder() {
    let root = scratch("path");
    copy(&fixtures(), &root);
    let mut pk = packages(&root.join("no-store"));
    let c = compile(&Project::open(&root.join("song-path")).unwrap(), &mut pk).unwrap_or_else(|d| panic!("{d:?}"));
    assert!(c.problems.is_empty(), "{:?}", c.problems);
    let components: Vec<&str> = c.ir.modules.iter().flat_map(|m| m.frames.iter().filter_map(|f| f.component.as_deref())).collect();
    for id in ["@test/tiny:tiny-keys", "@test/tiny:tiny-crunch", "@test/tiny:tiny-sampler"] {
        assert!(components.contains(&id), "{id} in {components:?}");
    }
    assert!(c.package_files.iter().any(|f| f.path == "@test/tiny/src/index.ts"), "{:?}", c.package_files);
    let wav = std::fs::read(root.join("tiny/samples/pluck.wav")).unwrap();
    assert_eq!(c.data.kernel_assets().get("@test/tiny:samples/pluck.wav"), Some(&m2_song::Sha::of(&wav)), "the recording's hash");
    assert_eq!(c.packs.iter().map(|p| (p.name.as_str(), p.path)).collect::<Vec<_>>(), [("@test/tiny", true)]);
    let out = Renderer::new(&c.flat, &c.data, &pk, 48_000).unwrap().render(2.0).unwrap();
    let peak =
        |from: f64, to: f64| out[(from * 48_000.0) as usize..(to * 48_000.0) as usize].iter().map(|f| f[0].abs()).fold(0.0, f32::max);
    assert!(peak(0.0, 0.2) > 0.01, "the keys sound");
    assert!(peak(0.25, 0.45) > 0.01, "the recorded pluck sounds");
    std::fs::remove_dir_all(&root).ok();
}

// PACK-8, PACK-9: from the store, the lock keeps the version; another hash is pkg.changed, a
// version not installed pkg.missing; each blocks, and what loaded before stays loaded.
#[test]
fn a_song_builds_with_a_locked_pack_from_the_store() {
    let root = scratch("store");
    copy(&fixtures(), &root);
    let store = root.join("Packs");
    let hash = install_tiny(&store);
    let song = root.join("song-store");
    let lock = Lock::default().with(PackName::new("@test/tiny").unwrap(), Version(0, 1, 0), hash);
    std::fs::write(song.join(m2_song::path::LOCK), lock.canonical()).unwrap();
    let fixture_lock = std::fs::read_to_string(fixtures().join("song-store").join(m2_song::path::LOCK)).unwrap_or_default();
    assert_eq!(fixture_lock, lock.canonical(), "the fixture's music2.lock pins the fixture pack: write this there");
    let mut pk = packages(&store);
    let c = compile(&Project::open(&song).unwrap(), &mut pk).unwrap_or_else(|d| panic!("{d:?}"));
    assert_eq!(c.packs.iter().map(|p| (p.version, p.hash)).collect::<Vec<_>>(), [(Version(0, 1, 0), Some(hash))]);
    assert!(pk.get("@test/tiny").is_some());

    // Another hash in the lock: pkg.changed, and the set keeps what the last build loaded.
    let other = Lock::default().with(PackName::new("@test/tiny").unwrap(), Version(0, 1, 0), m2_song::Sha::of(b"other"));
    std::fs::write(song.join(m2_song::path::LOCK), other.canonical()).unwrap();
    let e = compile(&Project::open(&song).unwrap(), &mut pk).err().unwrap();
    assert_eq!(codes(&e), ["pkg.changed"]);
    assert!(pk.get("@test/tiny").is_some(), "what the accepted revision resolved stays loaded");

    // A version that isn't installed: pkg.missing, saying how to install it.
    let gone = Lock::default().with(PackName::new("@test/tiny").unwrap(), Version(0, 1, 9), hash);
    std::fs::write(song.join(m2_song::path::LOCK), gone.canonical()).unwrap();
    let e = compile(&Project::open(&song).unwrap(), &mut pk).err().unwrap();
    assert_eq!(codes(&e), ["pkg.missing"]);
    assert!(e[0].message.contains("m2 pack install"), "{}", e[0].message);
    std::fs::remove_dir_all(&root).ok();
}

// PACK-2: a pack that needs another std than the app's is pkg.broken, naming what it needs; a
// song that doesn't list it can't import it (pkg.undeclared).
#[test]
fn a_pack_that_needs_another_std_is_broken() {
    let root = scratch("needs");
    copy(&fixtures(), &root);
    let manifest = root.join("tiny/m2pkg.toml");
    let text = std::fs::read_to_string(&manifest).unwrap().replace("std = \"0.4\"", "std = \"0.9\"");
    std::fs::write(&manifest, text).unwrap();
    let mut pk = packages(&root.join("no-store"));
    let e = compile(&Project::open(&root.join("song-path")).unwrap(), &mut pk).err().unwrap();
    assert_eq!(codes(&e), ["pkg.broken"]);
    assert!(e[0].message.contains("std \"0.9\""), "{}", e[0].message);
    assert!(pk.get("@test/tiny").is_none(), "a pack that can't be used isn't kept");
    std::fs::remove_dir_all(&root).ok();
}
