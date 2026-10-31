use std::path::{Path, PathBuf};

use super::*;
use crate::manifest::Manifest;

fn scratch(tag: &str) -> PathBuf {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let d = std::env::temp_dir().join(format!("m2-song-pack-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A pack folder named `name` at `version`, with one source file.
fn pack_at(dir: &Path, name: &str, version: &str) {
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join(PKG_MANIFEST), format!("[package]\nname = \"{name}\"\nversion = \"{version}\"\nlicense = \"CC0-1.0\"\n"))
        .unwrap();
    std::fs::write(dir.join("src/index.ts"), format!("export const v = \"{version}\";\n")).unwrap();
}

/// `pack_at` inside `store`, with its listing written, as an install leaves it.
fn installed(store: &Store, name: &str, version: &str) -> Sha {
    let dir = store.version_dir(&PackName::new(name).unwrap(), &Version::parse(version).unwrap());
    pack_at(&dir, name, version);
    let listing = Listing::of(&dir).unwrap();
    std::fs::write(dir.join(LISTING), listing.text()).unwrap();
    listing.hash()
}

const SONG: &str = "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"2\"\nstd = \"0.4\"\n";

fn song_with(deps: &str) -> Manifest {
    Manifest::read(&format!("{SONG}{deps}")).unwrap()
}

// PACK-1: one rule for every name.
#[test]
fn pack_names() {
    let long = "a".repeat(39);
    for good in ["std", "@alice/felt-piano", "@m2/drums", "@a/b", "@0/9", "@a-b/c-d", &format!("@{long}/{long}")] {
        let n = PackName::new(good).unwrap_or_else(|e| panic!("{good}: {e}"));
        assert_eq!(n.as_str(), good);
    }
    let too_long = "a".repeat(40);
    for bad in [
        "",
        "drums",
        "Std",
        "@alice",
        "@alice/",
        "@/x",
        "@Alice/x",
        "@alice/X",
        "@alice/x/y",
        "@alice/-x",
        "@alice/x-",
        "@-alice/x",
        "@alice/x_y",
        "@alice/x:y",
        "@alice/felt piano",
        "@alice//x",
        "std/x",
        "@m2/core",
        &format!("@{too_long}/x"),
        &format!("@x/{too_long}"),
    ] {
        assert!(PackName::new(bad).is_err(), "{bad:?} is no package name");
    }
    assert!(PackName::new("@m2/core").unwrap_err().contains("SDK"), "the SDK is never a pack");
    assert!(PackName::new("std").unwrap().is_std() && !PackName::new("@a/b").unwrap().is_std());
    assert_eq!(PackName::new("@alice/felt-piano").unwrap().folder(), Path::new("@alice").join("felt-piano"));
}

// PACK-4: ids split at their first `:`, as a package name never holds one.
#[test]
fn ids_split_at_the_first_colon() {
    assert_eq!(split_id("@alice/felt-piano:felt-piano"), Some(("@alice/felt-piano", "felt-piano")));
    assert_eq!(split_id("std:svf-lowpass"), Some(("std", "svf-lowpass")));
    assert_eq!(split_id("@m2/synths:synth-pluck"), Some(("@m2/synths", "synth-pluck")));
    assert_eq!(split_id("clap:org.example:odd"), Some(("clap", "org.example:odd")));
    assert_eq!(split_id("delay"), None, "a project's own component has no package");
}

// PACK-5: a reference with a `:` is a package's file, checked as package files are.
#[test]
fn asset_refs_name_the_song_or_a_package() {
    assert_eq!(AssetRef::parse("kick.wav"), Ok(AssetRef::Song("kick.wav")));
    assert_eq!(AssetRef::parse("drums/kick.wav"), Ok(AssetRef::Song("drums/kick.wav")));
    let p = AssetRef::parse("@alice/felt-piano:samples/C4.wav").unwrap();
    assert_eq!(p, AssetRef::Package { package: PackName::new("@alice/felt-piano").unwrap(), path: "samples/C4.wav" });
    assert!(matches!(AssetRef::parse("std:kits/808/kick.wav"), Ok(AssetRef::Package { .. })));
    for bad in [
        "@alice/felt-piano:../x.wav",
        "@alice/felt-piano:samples/../../x.wav",
        "@alice/felt-piano:/etc/passwd",
        "@alice/felt-piano:samples//x.wav",
        "@alice/felt-piano:samples\\x.wav",
        "@alice/felt-piano:",
        "@alice/felt-piano:.hidden.wav",
        "@alice/felt-piano:./x.wav",
        "drums:x.wav",
        "@m2/core:x.wav",
    ] {
        assert!(AssetRef::parse(bad).is_err(), "{bad:?} names no file in a package");
    }
}

// PACK-2: m2pkg.toml is strict, and names only the SDK and std.
#[test]
fn a_package_manifest_is_read_strictly() {
    let good = "[package]\nname = \"@alice/felt-piano\"\nversion = \"1.2.0\"\nlicense = \"CC-BY-4.0\"\ndescription = \"soft\"\n\
                authors = [\"Alice\"]\nrepository = \"https://github.com/alice/felt-piano\"\ntags = [\"keys\"]\n\n\
                [dependencies]\n\"@m2/core\" = \"2\"\nstd = \"0.4\"\n";
    let m = PkgManifest::read(good).unwrap();
    assert_eq!((m.package.name.as_str(), m.package.version), ("@alice/felt-piano", Version(1, 2, 0)));
    assert_eq!((m.package.authors.len(), m.package.tags.len(), m.package.license.as_deref()), (1, 1, Some("CC-BY-4.0")));
    assert!(m.needs(Version(2, 6, 0), Version(0, 4, 1)).is_ok());
    // std is a lower bound (PACK-2): a std that grew since meets it, an older one doesn't.
    assert!(m.needs(Version(2, 6, 0), Version(0, 5, 0)).is_ok(), "std 0.5 meets 0.4");
    assert!(m.needs(Version(2, 6, 0), Version(1, 0, 0)).is_ok(), "std 1.0 meets 0.4");
    assert!(m.needs(Version(2, 6, 0), Version(0, 3, 9)).unwrap_err().contains("std"), "std 0.3.9 doesn't meet 0.4");
    // The SDK keeps the caret rule.
    assert!(m.needs(Version(3, 0, 0), Version(0, 4, 0)).unwrap_err().contains("@m2/core"));
    for bad in [
        good.replace("tags =", "colour = \"red\"\ntags ="),
        good.replace("[dependencies]", "[other]\nx = 1\n[dependencies]"),
        good.replace("std = \"0.4\"", "std = \"0.4\"\n\"@bob/strings\" = \"1\""),
        good.replace("name = \"@alice/felt-piano\"", "name = \"felt-piano\""),
        good.replace("version = \"1.2.0\"", "version = \"1.2\""),
        good.replace("version = \"1.2.0\"\n", ""),
        good.replace("[package]", "[package]\n[[effect]]\nname = \"x\"\n[package]"),
    ] {
        assert!(PkgManifest::read(&bad).is_err(), "{bad}");
    }
    // std's own still loads.
    let std = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std/m2pkg.toml")).unwrap();
    let s = PkgManifest::read(&std).unwrap();
    assert!(s.package.name.is_std() && !s.kernel.is_empty());
}

// PACK-8: the lock is strict, sorted, and has one canonical form.
#[test]
fn the_lock_has_one_canonical_form() {
    let a = Sha::of(b"a");
    let b = Sha::of(b"b");
    let lock = Lock::default().with(PackName::new("@zed/x").unwrap(), Version(0, 1, 0), b).with(
        PackName::new("@alice/felt-piano").unwrap(),
        Version(1, 2, 0),
        a,
    );
    let text = lock.canonical();
    assert!(text.find("@alice/felt-piano").unwrap() < text.find("@zed/x").unwrap(), "sorted by name:\n{text}");
    assert!(text.contains(&format!("hash = \"{}\"", a.prefixed())), "{text}");
    let read = Lock::read(&text).unwrap();
    assert_eq!(read, lock);
    assert_eq!(read.canonical(), text, "reading then writing gives the same bytes");
    assert_eq!(read.get(&PackName::new("@zed/x").unwrap()).map(|l| (l.version, l.hash)), Some((Version(0, 1, 0), b)));
    // Replacing an entry keeps one per name.
    let moved = read.with(PackName::new("@zed/x").unwrap(), Version(0, 2, 0), a);
    assert_eq!(moved.packs.len(), 2);
    assert_eq!(Lock::read("").unwrap(), Lock::default(), "an empty lock locks nothing");
    for bad in [
        text.replace("version = \"1.2.0\"", "version = \"1.2\""),
        text.replace("name = \"@zed/x\"", "name = \"zed\""),
        text.replace("[[pack]]", "[[pack]]\nwhy = 1"),
        format!("{text}\n[other]\n"),
        text.replace(&a.prefixed(), "sha256:xyz"),
        format!("{text}\n[[pack]]\nname = \"@zed/x\"\nversion = \"0.3.0\"\nhash = \"{}\"\n", a.prefixed()),
        text.replace("name = \"@zed/x\"", "name = \"std\""),
    ] {
        let e = Lock::read(&bad).unwrap_err();
        assert_eq!((e.code, e.src.as_deref()), ("manifest.invalid", Some(LOCK)), "{bad}");
    }
}

// PACK-6: the listing names every file but itself, sorted, with its size and hash; its own hash is the version's.
#[test]
fn a_listing_names_every_file_with_its_size_and_hash() {
    let dir = scratch("listing");
    pack_at(&dir, "@alice/x", "1.0.0");
    std::fs::create_dir_all(dir.join("samples")).unwrap();
    std::fs::write(dir.join("samples/c4.wav"), b"RIFF").unwrap();
    std::fs::create_dir_all(dir.join(".git")).unwrap();
    std::fs::write(dir.join(".git/HEAD"), b"x").unwrap();
    let l = Listing::of(&dir).unwrap();
    let paths: Vec<&str> = l.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["m2pkg.toml", "samples/c4.wav", "src/index.ts"], "sorted, dot folders left out");
    assert_eq!((l.files[1].size, l.files[1].sha256), (4, Sha::of(b"RIFF")));
    std::fs::write(dir.join(LISTING), l.text()).unwrap();
    let again = Listing::of(&dir).unwrap();
    assert_eq!(again, l, "the listing never lists itself");
    assert_eq!(Listing::read(&l.text()).unwrap(), l);
    assert_eq!(l.hash(), Sha::of(l.text().as_bytes()));
    std::fs::remove_dir_all(&dir).unwrap();
}

// PACK-20: a pack's kernels' source, `kernels-src/` at its top, is no part of it: not listed, so
// not in its hash, and never installed; a build left in it changes nothing. A folder of that name
// deeper in the pack is the pack's like any other.
#[test]
fn a_packs_kernel_source_is_no_part_of_it() {
    let dir = scratch("kernels-src");
    pack_at(&dir, "@alice/x", "1.0.0");
    let bare = Listing::of(&dir).unwrap();
    for (file, text) in
        [("kernels-src/Cargo.toml", "[package]\n"), ("kernels-src/src/lib.rs", "//! x\n"), ("kernels-src/target/x.wasm", "x")]
    {
        std::fs::create_dir_all(dir.join(file).parent().unwrap()).unwrap();
        std::fs::write(dir.join(file), text).unwrap();
    }
    assert_eq!(Listing::of(&dir).unwrap(), bare, "its source changes neither its listing nor its hash");
    std::fs::create_dir_all(dir.join("src/kernels-src")).unwrap();
    std::fs::write(dir.join("src/kernels-src/a.ts"), "export const a = 1;\n").unwrap();
    let paths: Vec<String> = Listing::of(&dir).unwrap().files.into_iter().map(|f| f.path).collect();
    assert_eq!(paths, ["m2pkg.toml", "src/index.ts", "src/kernels-src/a.ts"], "only the top's is the source");
    // Nothing of the pack is read from it (review X4): no file a song names, no module.
    assert_eq!(package_file(&dir, "kernels-src/src/lib.rs").unwrap_err(), NOT_IN_KERNELS_SRC);
    assert!(package_file(&dir, "src/kernels-src/a.ts").is_ok());
    let with = |wasm: &str| PkgManifest::read(&format!("[package]\nname = \"@alice/x\"\nversion = \"1.0.0\"\nwasm = \"{wasm}\"\n"));
    assert!(with("kernels/x.wasm").is_ok());
    assert!(with("./kernels-src/x.wasm").unwrap_err().ends_with(NOT_IN_KERNELS_SRC));
    assert!(with("../x.wasm").unwrap_err().contains("leaves the pack"));
    let store = Store::at(scratch("kernels-src-store"));
    store.install(&dir).unwrap();
    let at = store.version_dir(&PackName::new("@alice/x").unwrap(), &Version(1, 0, 0));
    assert!(!at.join("kernels-src").exists(), "never installed");
    store.check(&PackName::new("@alice/x").unwrap(), &Version(1, 0, 0)).unwrap();
    remove_tree(&store.dir).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
}

// PACK-9: every way a song's packs resolve, and what each problem says.
#[test]
fn every_resolution_case_says_what_it_is() {
    let root = scratch("resolve");
    let store = Store::at(root.join("Packs"));
    let song = root.join("song");
    std::fs::create_dir_all(&song).unwrap();
    let h120 = installed(&store, "@alice/felt-piano", "1.2.0");
    let h125 = installed(&store, "@alice/felt-piano", "1.2.5");
    let h130 = installed(&store, "@alice/felt-piano", "1.3.0");
    installed(&store, "@alice/felt-piano", "2.0.0");
    let name = PackName::new("@alice/felt-piano").unwrap();
    let codes = |r: &Resolution| r.problems.iter().map(|d| d.code).collect::<Vec<_>>();

    // No lock entry: the highest installed version the requirement admits (Cargo's caret rule).
    let m = song_with("\"@alice/felt-piano\" = \"1.2\"\n");
    let r = resolve(&song, &m, &Lock::default(), Some(&store), &App::default());
    assert_eq!(codes(&r), Vec::<&str>::new());
    assert_eq!((r.packs[0].version, r.packs[0].hash), (Version(1, 3, 0), Some(h130)));
    assert_eq!(r.packs[0].dir, store.version_dir(&name, &Version(1, 3, 0)));
    let r = resolve(&song, &song_with("\"@alice/felt-piano\" = \"1.2.5\"\n"), &Lock::default(), Some(&store), &App::default());
    assert_eq!(r.packs[0].version, Version(1, 3, 0));

    // The lock's version, when its hash is the lock's.
    let lock = Lock::default().with(name.clone(), Version(1, 2, 0), h120);
    let r = resolve(&song, &m, &lock, Some(&store), &App::default());
    assert_eq!((codes(&r), r.packs[0].version), (vec![], Version(1, 2, 0)));

    // Another hash: pkg.changed.
    let lock = Lock::default().with(name.clone(), Version(1, 2, 0), h125);
    let r = resolve(&song, &m, &lock, Some(&store), &App::default());
    assert_eq!(codes(&r), ["pkg.changed"]);
    assert!(r.packs.is_empty());

    // A locked version not installed: pkg.missing, saying what to install and how.
    let lock = Lock::default().with(name.clone(), Version(1, 2, 9), h120);
    let r = resolve(&song, &m, &lock, Some(&store), &App::default());
    assert_eq!(codes(&r), ["pkg.missing"]);
    let said = &r.problems[0].message;
    assert!(said.contains("@alice/felt-piano") && said.contains("1.2.9") && said.contains("m2 pack install"), "{said}");

    // A locked version the requirement doesn't admit: pkg.unsatisfied.
    let lock = Lock::default().with(name.clone(), Version(1, 2, 5), h125);
    let r = resolve(&song, &song_with("\"@alice/felt-piano\" = \"1.3\"\n"), &lock, Some(&store), &App::default());
    assert_eq!(codes(&r), ["pkg.unsatisfied"]);
    assert!(r.problems[0].message.contains("m2 pack lock"), "{}", r.problems[0].message);

    // No installed version admitted: pkg.unsatisfied, naming those there are.
    let r = resolve(&song, &song_with("\"@alice/felt-piano\" = \"3\"\n"), &Lock::default(), Some(&store), &App::default());
    assert_eq!(codes(&r), ["pkg.unsatisfied"]);
    assert!(r.problems[0].message.contains("1.3.0, 2.0.0"), "{}", r.problems[0].message);
    let r = resolve(&song, &song_with("\"@bob/none\" = \"1\"\n"), &Lock::default(), Some(&store), &App::default());
    assert_eq!(codes(&r), ["pkg.unsatisfied"]);

    // A path dependency: that folder, never locked, never hashed.
    let mine = root.join("felt-piano");
    pack_at(&mine, "@alice/felt-piano", "1.4.0");
    let m = song_with("\"@alice/felt-piano\" = { path = \"../felt-piano\" }\n");
    let r = resolve(&song, &m, &lock, Some(&store), &App::default());
    assert_eq!(codes(&r), Vec::<&str>::new());
    assert_eq!((r.packs[0].version, r.packs[0].hash, r.packs[0].path), (Version(1, 4, 0), None, true));
    assert_eq!(r.packs[0].dir, mine.canonicalize().unwrap());
    // A folder that holds another pack, or none: pkg.missing.
    pack_at(&root.join("other"), "@bob/other", "1.0.0");
    let m = song_with("\"@alice/felt-piano\" = { path = \"../other\" }\n");
    assert_eq!(codes(&resolve(&song, &m, &Lock::default(), Some(&store), &App::default())), ["pkg.missing"]);
    let m = song_with("\"@alice/felt-piano\" = { path = \"../nowhere\" }\n");
    assert_eq!(codes(&resolve(&song, &m, &Lock::default(), Some(&store), &App::default())), ["pkg.missing"]);

    // std and the SDK are never resolved here: the app has them.
    assert!(resolve(&song, &song_with(""), &Lock::default(), Some(&store), &App::default()).packs.is_empty());

    // No store (no home folder): a pack from the store is unsatisfied, and nothing else is looked
    // in, not even a folder of that name where the process runs.
    let r = resolve(&song, &song_with("\"@alice/felt-piano\" = \"1.2\"\n"), &Lock::default(), None, &App::default());
    assert_eq!(codes(&r), ["pkg.unsatisfied"]);
    assert!(r.problems[0].message.contains("no store"), "{}", r.problems[0].message);
    std::fs::remove_dir_all(&root).unwrap();
}

// PACK-6: the store lists each pack's versions, oldest first.
#[test]
fn the_store_lists_what_is_installed() {
    let root = scratch("store");
    let store = Store::at(root.clone());
    installed(&store, "@alice/felt-piano", "1.10.0");
    installed(&store, "@alice/felt-piano", "1.9.0");
    installed(&store, "@bob/kit", "0.1.0");
    std::fs::create_dir_all(root.join("@bob/kit/not-a-version")).unwrap();
    std::fs::create_dir_all(root.join("@bob/kit/0.2.0")).unwrap();
    let all = store.list();
    let shown: Vec<(String, Vec<String>)> =
        all.iter().map(|(n, vs)| (n.as_str().to_string(), vs.iter().map(|v| v.to_string()).collect())).collect();
    assert_eq!(
        shown,
        [("@alice/felt-piano".to_string(), vec!["1.9.0".to_string(), "1.10.0".to_string()]), ("@bob/kit".into(), vec!["0.1.0".into()])],
        "a folder that isn't an installed version is no version"
    );
    std::fs::remove_dir_all(&root).unwrap();
}

// PACK-4: the editor names a kernel, a component and a preset folder by the same split.
#[test]
fn a_scoped_id_splits_into_its_whole_package() {
    for (id, package, rest) in [("@alice/felt-piano:grain", "@alice/felt-piano", "grain"), ("std:svf", "std", "svf")] {
        assert_eq!(split_id(id), Some((package, rest)));
        assert!(PackName::new(package).is_ok(), "{package} is a package's name");
    }
}

/// Make `path` (a file in an installed version) writable, as someone changing it would.
fn writable(path: &Path) {
    for p in [path.parent().unwrap(), path] {
        if let Ok(m) = std::fs::metadata(p) {
            let mut perm = m.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            perm.set_readonly(false);
            std::fs::set_permissions(p, perm).unwrap();
        }
    }
}

// PACK-6, PACK-9: a store version is checked file by file against its listing before it is used:
// a changed, missing or added file, or a link, is pkg.changed naming it, though the listing (and
// so the lock's hash) is untouched.
#[test]
fn a_store_version_is_checked_file_by_file() {
    let root = scratch("checked");
    let store = Store::at(root.join("Packs"));
    let song = root.join("song");
    std::fs::create_dir_all(&song).unwrap();
    let name = PackName::new("@alice/felt-piano").unwrap();
    let hash = installed(&store, "@alice/felt-piano", "1.2.0");
    let dir = store.version_dir(&name, &Version(1, 2, 0));
    let m = song_with("\"@alice/felt-piano\" = \"1.2\"\n");
    let lock = Lock::default().with(name.clone(), Version(1, 2, 0), hash);
    let codes = |r: &Resolution| r.problems.iter().map(|d| d.code).collect::<Vec<_>>();
    assert_eq!(store.check(&name, &Version(1, 2, 0)), Ok(hash));
    assert_eq!(codes(&resolve(&song, &m, &lock, Some(&store), &App::default())), Vec::<&str>::new());

    let index = dir.join("src/index.ts");
    let was = std::fs::read(&index).unwrap();
    writable(&index);
    // The same size, other bytes: only the hash can tell.
    std::fs::write(&index, String::from_utf8(was.clone()).unwrap().replace("1.2.0", "9.9.9")).unwrap();
    for lock in [&lock, &Lock::default()] {
        let r = resolve(&song, &m, lock, Some(&store), &App::default());
        assert_eq!(codes(&r), ["pkg.changed"], "{:?}", r.problems);
        let said = &r.problems[0].message;
        assert!(said.contains("src/index.ts") && said.contains("m2 pack install"), "{said}");
    }
    std::fs::write(&index, &was).unwrap();
    assert_eq!(store.check(&name, &Version(1, 2, 0)), Ok(hash), "put back, it is whole again");

    std::fs::remove_file(&index).unwrap();
    assert!(store.check(&name, &Version(1, 2, 0)).unwrap_err().contains("src/index.ts"));
    std::fs::write(&index, &was).unwrap();

    std::fs::write(dir.join("src/extra.ts"), "export const x = 1;\n").unwrap();
    let why = store.check(&name, &Version(1, 2, 0)).unwrap_err();
    assert!(why.contains("src/extra.ts"), "{why}");
    std::fs::remove_file(dir.join("src/extra.ts")).unwrap();

    #[cfg(unix)]
    {
        std::fs::remove_file(&index).unwrap();
        let outside = root.join("outside.ts");
        std::fs::write(&outside, &was).unwrap();
        std::os::unix::fs::symlink(&outside, &index).unwrap();
        let why = store.check(&name, &Version(1, 2, 0)).unwrap_err();
        assert!(why.contains("src/index.ts") && why.contains("link"), "a link to the very bytes is still refused: {why}");
    }
    std::fs::remove_dir_all(&root).ok();
}

// PACK-5, PACK-6: nothing of a pack is reached through a link: its listing, its tools and its
// package files refuse one, a file or a folder.
#[cfg(unix)]
#[test]
fn a_package_file_is_never_reached_through_a_link() {
    let root = scratch("links");
    let dir = root.join("pack");
    pack_at(&dir, "@alice/x", "1.0.0");
    std::fs::create_dir_all(dir.join("samples")).unwrap();
    std::fs::write(dir.join("samples/c4.wav"), b"RIFF").unwrap();
    assert!(package_file(&dir, "samples/c4.wav").is_ok());
    let outside = root.join("secret");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("token.txt"), b"SECRET").unwrap();
    std::os::unix::fs::symlink(outside.join("token.txt"), dir.join("samples/notes.wav")).unwrap();
    std::os::unix::fs::symlink(&outside, dir.join("samples/more")).unwrap();
    for path in ["samples/notes.wav", "samples/more/token.txt"] {
        let why = package_file(&dir, path).unwrap_err();
        assert!(why.contains("link"), "{path}: {why}");
    }
    let why = Listing::of(&dir).unwrap_err().to_string();
    assert!(why.contains("link") && why.contains("samples/"), "{why}");
    assert!(pack_files(&dir).is_err(), "the tools never list through a link");
    std::fs::remove_dir_all(&root).ok();
}

// PACK-8: lock keeps what still resolves and moves only what must; update moves to the newest
// admitted; a kept version whose copy isn't the one locked is refused, saying how to put it back.
#[test]
fn relocking_moves_only_what_must_move() {
    let root = scratch("relock");
    let store = Store::at(root.join("Packs"));
    let song = root.join("song");
    std::fs::create_dir_all(&song).unwrap();
    let name = PackName::new("@alice/felt-piano").unwrap();
    let h120 = installed(&store, "@alice/felt-piano", "1.2.0");
    let h125 = installed(&store, "@alice/felt-piano", "1.2.5");
    let h130 = installed(&store, "@alice/felt-piano", "1.3.0");
    let h200 = installed(&store, "@alice/felt-piano", "2.0.0");
    let kit = PackName::new("@bob/kit").unwrap();
    let hkit = installed(&store, "@bob/kit", "0.1.0");
    let locked = |l: &Lock, n: &PackName| l.get(n).map(|l| (l.version, l.hash));
    let m = song_with("\"@alice/felt-piano\" = \"1.2\"\n\"@bob/kit\" = \"0.1\"\n");

    // No lock: the highest each admits.
    let l = relock(&m, &Lock::default(), Some(&store), &App::default(), Move::Needed).unwrap();
    assert_eq!((locked(&l, &name), locked(&l, &kit)), (Some((Version(1, 3, 0), h130)), Some((Version(0, 1, 0), hkit))));

    // An entry still admitted and installed stays, though a newer is there; update moves it.
    let old = Lock::default().with(name.clone(), Version(1, 2, 0), h120).with(kit.clone(), Version(0, 1, 0), hkit);
    let l = relock(&m, &old, Some(&store), &App::default(), Move::Needed).unwrap();
    assert_eq!(l, old, "nothing moves that needn't");
    let l = relock(&m, &old, Some(&store), &App::default(), Move::All).unwrap();
    assert_eq!(locked(&l, &name), Some((Version(1, 3, 0), h130)));
    let l = relock(&m, &old, Some(&store), &App::default(), Move::Only(&kit)).unwrap();
    assert_eq!(locked(&l, &name), Some((Version(1, 2, 0), h120)), "update of another pack leaves this one");

    // The requirement moved on: the entry it no longer admits moves to the highest it does.
    let m2 = song_with("\"@alice/felt-piano\" = \"2\"\n\"@bob/kit\" = \"0.1\"\n");
    let l = relock(&m2, &old, Some(&store), &App::default(), Move::Needed).unwrap();
    assert_eq!((locked(&l, &name), locked(&l, &kit)), (Some((Version(2, 0, 0), h200)), Some((Version(0, 1, 0), hkit))));

    // A locked version that isn't installed, while another admitted one is: that one.
    let gone = Lock::default().with(name.clone(), Version(1, 2, 9), h125);
    let l = relock(&m, &gone, Some(&store), &App::default(), Move::Needed).unwrap();
    assert_eq!(locked(&l, &name), Some((Version(1, 3, 0), h130)));

    // The installed copy isn't the one locked: refused, saying how to put it back and how to move.
    let other = Lock::default().with(name.clone(), Version(1, 2, 0), h125);
    let e = relock(&m, &other, Some(&store), &App::default(), Move::Needed).unwrap_err();
    assert_eq!(e.iter().map(|d| d.code).collect::<Vec<_>>(), ["pkg.changed"]);
    assert!(e[0].message.contains("m2 pack install") && e[0].message.contains("m2 pack update"), "{}", e[0].message);
    // Update to the installed copy is the way to move to it.
    let l = relock(&m, &other, Some(&store), &App::default(), Move::Only(&name)).unwrap();
    assert_eq!(locked(&l, &name), Some((Version(1, 3, 0), h130)));

    // A pack the manifest no longer lists leaves the lock; a path pack and std are never locked.
    let mine = root.join("mine");
    pack_at(&mine, "@bob/kit", "0.1.0");
    let m3 = song_with("\"@alice/felt-piano\" = \"1.2\"\n\"@bob/kit\" = { path = \"../mine\" }\n");
    let l = relock(&m3, &old, Some(&store), &App::default(), Move::Needed).unwrap();
    assert_eq!(l, Lock::default().with(name.clone(), Version(1, 2, 0), h120));

    // Nothing admitted is installed: unsatisfied, naming what is.
    let e =
        relock(&song_with("\"@alice/felt-piano\" = \"3\"\n"), &Lock::default(), Some(&store), &App::default(), Move::Needed).unwrap_err();
    assert_eq!(e.iter().map(|d| d.code).collect::<Vec<_>>(), ["pkg.unsatisfied"]);
    assert!(relock(&m, &Lock::default(), None, &App::default(), Move::Needed).is_err(), "no store, nothing to lock to");
    std::fs::remove_dir_all(&root).ok();
}

// PACK-6: while `m2 pack install` repairs a version, its changed copy moved aside and the new one
// not in place yet, a build says the version is being installed again: never that it isn't
// installed, nor that an older one does instead.
#[test]
fn a_build_during_a_repair_says_it_is_being_installed() {
    let root = scratch("aside");
    let song = root.join("song");
    std::fs::create_dir_all(&song).unwrap();
    let store = Store::at(root.join("Packs"));
    installed(&store, "@alice/felt-piano", "1.2.0");
    let h125 = installed(&store, "@alice/felt-piano", "1.2.5");
    let name = PackName::new("@alice/felt-piano").unwrap();
    let dir = store.version_dir(&name, &Version(1, 2, 5));
    std::fs::rename(&dir, dir.with_file_name(".1.2.5.4242.old")).unwrap();
    let m = song_with("\"@alice/felt-piano\" = \"1.2\"\n");
    let being = |r: &Resolution| r.problems.iter().any(|d| d.message.contains("being installed again"));
    let lock = Lock::default().with(name.clone(), Version(1, 2, 5), h125);
    let r = resolve(&song, &m, &lock, Some(&store), &App::default());
    assert!(being(&r), "{:?}", r.problems);
    let r = resolve(&song, &m, &Lock::default(), Some(&store), &App::default());
    assert!(being(&r) && r.packs.is_empty(), "1.2.0 isn't taken instead: {:?} {:?}", r.problems, r.packs);
    std::fs::remove_dir_all(&root).unwrap();
}

// PACK-6: anything in a pack that isn't a folder or a plain file (a pipe, a device) is refused,
// named, before any file is opened: reading a pipe would wait for ever (review G8).
#[test]
fn a_pipe_in_a_pack_is_refused_before_anything_is_read() {
    let d = scratch("pipe");
    pack_at(&d, "@alice/drums", "1.0.0");
    std::fs::create_dir_all(d.join("samples")).unwrap();
    let made = std::process::Command::new("mkfifo").arg(d.join("samples/hang.wav")).status().unwrap();
    assert!(made.success(), "mkfifo");
    let e = Listing::of(&d).unwrap_err().to_string();
    assert!(e.contains("samples/hang.wav isn't a plain file (a pipe or a device)"), "{e}");
    assert!(pack_files(&d).is_err());
}

// PACK-6: a repair whose new copy can't go in (a scanner holding a file just written, on Windows)
// puts the copy before back, read-only, and leaves nothing aside.
#[test]
fn a_repair_that_cant_put_the_new_copy_in_puts_the_old_one_back() {
    let f = scratch("swap");
    let pack = f.join("Packs/@a/x");
    let (target, temp, old) = (pack.join("1.0.0"), pack.join(".1.0.0.1.tmp"), pack.join(".1.0.0.1.old"));
    for (dir, text) in [(&target, "before"), (&temp, "new")] {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("m2pack.json"), text).unwrap();
    }
    read_only_inside(&target).unwrap();
    set_read_only(&target, true).unwrap();
    let mut rename = |a: &Path, b: &Path| match a == temp {
        true => Err(std::io::Error::other("held by another program")),
        false => std::fs::rename(a, b),
    };
    assert!(install::swap(&temp, &target, &old, &mut rename).is_err());
    assert_eq!(std::fs::read_to_string(target.join("m2pack.json")).unwrap(), "before", "the version is as it was");
    assert!(!old.exists(), "nothing is left aside");
    assert!(std::fs::metadata(&target).unwrap().permissions().readonly(), "read-only again");
    remove_tree(&f).unwrap();
}

/// Every file and folder under `dir` with when it was last changed: what a write would move.
fn stamps(dir: &Path) -> Vec<(PathBuf, std::time::SystemTime)> {
    let mut out = vec![];
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        out.push((d.clone(), std::fs::metadata(&d).unwrap().modified().unwrap()));
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            match e.file_type().unwrap().is_dir() {
                true => stack.push(e.path()),
                false => out.push((e.path(), e.metadata().unwrap().modified().unwrap())),
            }
        }
    }
    out.sort();
    out
}

// PACK-15, PACK-16: the packs music2 ships install into an empty store, each as the version its
// m2pkg.toml names, read-only; installing them again, as every start does, writes nothing.
#[test]
fn installing_the_bundled_packs_twice_writes_nothing() {
    let d = scratch("bundled");
    let store = Store::at(d.join("Packs"));
    let first = install_bundled(&source_packs(), &store);
    let names: Vec<String> = first
        .iter()
        .map(|(_, done)| match done {
            Ok(Install::Installed { name, version, .. }) => format!("{name} {version}"),
            other => panic!("{other:?}"),
        })
        .collect();
    let bundled: Vec<String> =
        bundled(&source_packs()).iter().map(|d| PkgManifest::at(d).unwrap().package).map(|p| format!("{} {}", p.name, p.version)).collect();
    assert_eq!(names, bundled);
    assert_eq!(names.len(), 4, "@m2/drums, @m2/effects, @m2/mixing and @m2/synths");
    for (dir, _) in &first {
        let m = PkgManifest::at(dir).unwrap();
        let hash = store.check(&m.package.name, &m.package.version).unwrap();
        assert_eq!(hash, Listing::of(dir).unwrap().hash(), "{}: the store holds the bundled files", dir.display());
        assert!(!dir.join(LISTING).exists(), "nothing is written where it ships");
        assert!(std::fs::metadata(store.version_dir(&m.package.name, &m.package.version)).unwrap().permissions().readonly());
    }
    let before = stamps(&store.dir);
    std::thread::sleep(std::time::Duration::from_millis(20));
    for (dir, done) in install_bundled(&source_packs(), &store) {
        assert!(matches!(done, Ok(Install::Already { .. })), "{}: {done:?}", dir.display());
    }
    assert_eq!(stamps(&store.dir), before, "a second start writes nothing");
    remove_tree(&d).unwrap();
}

// PACK-17: a version of an `@m2/` pack the store holds with other files is someone else's: the
// bundled one is refused, in words, and the others install all the same.
#[test]
fn a_bundled_version_installed_with_other_files_is_refused_and_the_others_go_on() {
    let d = scratch("clash");
    let store = Store::at(d.join("Packs"));
    let v = PkgManifest::at(&source_packs().join("@m2/synths")).unwrap().package.version;
    let theirs = installed(&store, "@m2/synths", &v.to_string());
    let done = install_bundled(&source_packs(), &store);
    let said: Vec<(String, Result<String, String>)> = done
        .into_iter()
        .map(|(dir, r)| {
            (dir.file_name().unwrap().to_string_lossy().into_owned(), r.map(|i| format!("{i:?}").split(' ').next().unwrap().to_string()))
        })
        .collect();
    assert_eq!(said[0], ("drums".into(), Ok("Installed".into())));
    assert_eq!(said[1], ("effects".into(), Ok("Installed".into())));
    assert_eq!(said[2], ("mixing".into(), Ok("Installed".into())));
    let Err(why) = &said[3].1 else { panic!("{said:?}") };
    assert!(why.starts_with(&format!("@m2/synths {v} is installed already with other files")), "{why}");
    assert_eq!(store.check(&PackName::new("@m2/synths").unwrap(), &v).unwrap(), theirs, "theirs is left as it was");
    remove_tree(&d).unwrap();
}

// PACK-17: every pack the source tree bundles is a version packs/versions.toml records, with the
// hash it records. A pack whose files changed under its version fails here, saying to move it on;
// a new version is told the line to add.
#[test]
fn every_bundled_pack_is_the_version_its_record_names() {
    let mut wrong = vec![];
    let packs = bundled(&source_packs());
    assert_eq!(packs.len(), 4, "@m2/drums, @m2/effects, @m2/mixing and @m2/synths");
    for dir in &packs {
        let m = PkgManifest::at(dir).unwrap();
        let (name, v) = (&m.package.name, m.package.version);
        let hash = Listing::of(dir).unwrap().hash();
        let next = Version(v.0, v.1, v.2 + 1);
        match recorded().iter().find(|(n, rv, _)| n == name && *rv == v) {
            Some((.., h)) if *h == hash => assert!(own(name, &v, &hash)),
            Some((.., h)) => wrong.push(format!(
                "{name} changed under {v}: packs/versions.toml records {h}, its files are now {hash}. A version's files never \
                 change (PACK-17): move {name} to {next} in packs/{name}/m2pkg.toml, record {next}, and relock the songs \
                 (`m2 pack update {name}` in each)"
            )),
            None => wrong.push(format!("packs/versions.toml doesn't record {name} {v}: add the line \"{name} {v}\" = \"{hash}\"")),
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    assert!(!own(&PackName::new("@m2/synths").unwrap(), &Version(1, 0, 0), &Sha::of(b"another")), "the name alone isn't music2's");
}

// PACK-6 (review S3): a version's folder with no listing in it is no installed version: an install
// puts the version in its place, and a remove takes it out. Install and remove agree.
#[test]
fn a_version_folder_with_no_listing_is_replaced_by_an_install() {
    let d = scratch("unlisted");
    let store = Store::at(d.join("Packs"));
    let name = PackName::new("@alice/keys").unwrap();
    let left = store.version_dir(&name, &Version(1, 0, 0));
    std::fs::create_dir_all(left.join("src")).unwrap();
    std::fs::write(left.join("src/half.ts"), "export const half = 1;\n").unwrap();
    set_read_only(&left, true).unwrap();
    let src = d.join("src-pack");
    pack_at(&src, "@alice/keys", "1.0.0");
    assert!(matches!(store.install(&src), Ok(Install::Installed { .. })), "an empty or unlisted folder is no version");
    assert_eq!(store.check(&name, &Version(1, 0, 0)).unwrap(), Listing::of(&src).unwrap().hash());
    assert!(!left.join("src/half.ts").exists(), "what was there went");
    // Remove takes out an unlisted folder too.
    let other = store.version_dir(&name, &Version(2, 0, 0));
    std::fs::create_dir_all(&other).unwrap();
    store.remove(&name, &Version(2, 0, 0)).unwrap();
    assert!(!other.exists());
    assert!(store.remove(&name, &Version(3, 0, 0)).is_err(), "a version with no folder isn't installed");
    remove_tree(&d).unwrap();
}

// PACK-6 (review S10): two threads of one process installing one version at once each see it
// installed: their copies in progress never share a name.
#[test]
fn two_threads_installing_one_version_both_see_it_installed() {
    let d = scratch("threads");
    let src = d.join("src-pack");
    pack_at(&src, "@alice/keys", "1.0.0");
    // Enough files that each copy takes a while.
    for i in 0..300 {
        std::fs::write(src.join(format!("src/f{i}.ts")), format!("export const f{i} = {i};\n")).unwrap();
    }
    for round in 0..8 {
        let store = Store::at(d.join(format!("Packs-{round}")));
        let done: Vec<Result<Install, String>> = std::thread::scope(|s| {
            let a = s.spawn(|| store.install(&src));
            let b = s.spawn(|| store.install(&src));
            vec![a.join().unwrap(), b.join().unwrap()]
        });
        for r in &done {
            assert!(matches!(r, Ok(Install::Installed { .. } | Install::Already { .. })), "round {round}: {done:?}");
        }
        let left: Vec<_> = std::fs::read_dir(store.dir.join("@alice/keys")).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(left, ["1.0.0"], "round {round}: nothing left beside it");
    }
    remove_tree(&d).unwrap();
}

// PACK-19 (review S7): a song locked to one of music2's own packs is told what puts music2's back,
// never to install from a folder it hasn't got: another copy under that version, a changed copy,
// and none at all.
#[test]
fn a_song_locked_to_music2s_own_pack_is_told_how_to_get_it_back() {
    let root = scratch("own");
    let store = Store::at(root.join("Packs"));
    let song = root.join("song");
    std::fs::create_dir_all(&song).unwrap();
    let bundled = PkgManifest::at(&source_packs().join("@m2/synths")).unwrap().package.version;
    let (name, v, hash) = recorded().iter().find(|(n, v, _)| n.as_str() == "@m2/synths" && *v == bundled).cloned().expect("recorded");
    let m = song_with(&format!("\"{name}\" = \"{v}\"\n"));
    let lock = Lock::default().with(name.clone(), v, hash);
    let said = |store: &Store| {
        let r = resolve(&song, &m, &lock, Some(store), &App::default());
        assert_eq!(r.problems.len(), 1, "{:?}", r.problems);
        (r.problems[0].code, r.problems[0].message.clone())
    };
    let (code, missing) = said(&store);
    assert_eq!(code, "pkg.missing");
    assert!(missing.contains("It comes with music2: start music2 again") && !missing.contains("m2 pack install"), "{missing}");
    // Another copy under its version, installed from a folder.
    installed(&store, name.as_str(), &v.to_string());
    let (code, other) = said(&store);
    assert_eq!(code, "pkg.changed");
    assert!(other.contains("isn't the one music2 comes with"), "{other}");
    assert!(
        other.contains(&format!("`m2 pack remove {name} {v}`, then start music2 again")) && !other.contains("m2 pack install"),
        "{other}"
    );
    // music2's own copy, changed after it was installed.
    remove_tree(&store.version_dir(&name, &v)).unwrap();
    store.install(&source_packs().join(name.as_str())).unwrap();
    let file = store.version_dir(&name, &v).join("README.md");
    let mut perm = std::fs::metadata(&file).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perm.set_readonly(false);
    std::fs::set_permissions(&file, perm).unwrap();
    std::fs::write(&file, "changed\n").unwrap();
    let (code, changed) = said(&store);
    assert_eq!(code, "pkg.changed");
    assert!(changed.contains("comes with music2: start music2 again, and it puts it back"), "{changed}");
    // Someone else's pack says what it said.
    let theirs = PackName::new("@alice/keys").unwrap();
    let h = installed(&store, "@alice/keys", "1.0.0");
    let m = song_with("\"@alice/keys\" = \"1.0.0\"\n");
    let lock = Lock::default().with(theirs, Version(1, 0, 0), Sha::of(h.to_string().as_bytes()));
    let r = resolve(&song, &m, &lock, Some(&store), &App::default());
    assert!(r.problems[0].message.contains("m2 pack install <its folder>"), "{:?}", r.problems);
    remove_tree(&root).unwrap();
}
