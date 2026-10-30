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

const SONG: &str = "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.4\"\n";

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
    assert_eq!(split_id("std:synth-pluck"), Some(("std", "synth-pluck")));
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
                [dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.4\"\n";
    let m = PkgManifest::read(good).unwrap();
    assert_eq!((m.package.name.as_str(), m.package.version), ("@alice/felt-piano", Version(1, 2, 0)));
    assert_eq!((m.package.authors.len(), m.package.tags.len(), m.package.license.as_deref()), (1, 1, Some("CC-BY-4.0")));
    assert!(m.needs(Version(1, 6, 0), Version(0, 4, 1)).is_ok());
    assert!(m.needs(Version(1, 6, 0), Version(0, 5, 0)).unwrap_err().contains("std"), "std 0.5 doesn't meet 0.4");
    assert!(m.needs(Version(2, 0, 0), Version(0, 4, 0)).unwrap_err().contains("@m2/core"));
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
    let r = resolve(&song, &m, &Lock::default(), &store);
    assert_eq!(codes(&r), Vec::<&str>::new());
    assert_eq!((r.packs[0].version, r.packs[0].hash), (Version(1, 3, 0), Some(h130)));
    assert_eq!(r.packs[0].dir, store.version_dir(&name, &Version(1, 3, 0)));
    let r = resolve(&song, &song_with("\"@alice/felt-piano\" = \"1.2.5\"\n"), &Lock::default(), &store);
    assert_eq!(r.packs[0].version, Version(1, 3, 0));

    // The lock's version, when its hash is the lock's.
    let lock = Lock::default().with(name.clone(), Version(1, 2, 0), h120);
    let r = resolve(&song, &m, &lock, &store);
    assert_eq!((codes(&r), r.packs[0].version), (vec![], Version(1, 2, 0)));

    // Another hash: pkg.changed.
    let lock = Lock::default().with(name.clone(), Version(1, 2, 0), h125);
    let r = resolve(&song, &m, &lock, &store);
    assert_eq!(codes(&r), ["pkg.changed"]);
    assert!(r.packs.is_empty());

    // A locked version not installed: pkg.missing, saying what to install and how.
    let lock = Lock::default().with(name.clone(), Version(1, 2, 9), h120);
    let r = resolve(&song, &m, &lock, &store);
    assert_eq!(codes(&r), ["pkg.missing"]);
    let said = &r.problems[0].message;
    assert!(said.contains("@alice/felt-piano") && said.contains("1.2.9") && said.contains("m2 pack install"), "{said}");

    // A locked version the requirement doesn't admit: pkg.unsatisfied.
    let lock = Lock::default().with(name.clone(), Version(1, 2, 5), h125);
    let r = resolve(&song, &song_with("\"@alice/felt-piano\" = \"1.3\"\n"), &lock, &store);
    assert_eq!(codes(&r), ["pkg.unsatisfied"]);
    assert!(r.problems[0].message.contains("m2 pack lock"), "{}", r.problems[0].message);

    // No installed version admitted: pkg.unsatisfied, naming those there are.
    let r = resolve(&song, &song_with("\"@alice/felt-piano\" = \"3\"\n"), &Lock::default(), &store);
    assert_eq!(codes(&r), ["pkg.unsatisfied"]);
    assert!(r.problems[0].message.contains("1.3.0, 2.0.0"), "{}", r.problems[0].message);
    let r = resolve(&song, &song_with("\"@bob/none\" = \"1\"\n"), &Lock::default(), &store);
    assert_eq!(codes(&r), ["pkg.unsatisfied"]);

    // A path dependency: that folder, never locked, never hashed.
    let mine = root.join("felt-piano");
    pack_at(&mine, "@alice/felt-piano", "1.4.0");
    let m = song_with("\"@alice/felt-piano\" = { path = \"../felt-piano\" }\n");
    let r = resolve(&song, &m, &lock, &store);
    assert_eq!(codes(&r), Vec::<&str>::new());
    assert_eq!((r.packs[0].version, r.packs[0].hash, r.packs[0].path), (Version(1, 4, 0), None, true));
    assert_eq!(r.packs[0].dir, mine.canonicalize().unwrap());
    // A folder that holds another pack, or none: pkg.missing.
    pack_at(&root.join("other"), "@bob/other", "1.0.0");
    let m = song_with("\"@alice/felt-piano\" = { path = \"../other\" }\n");
    assert_eq!(codes(&resolve(&song, &m, &Lock::default(), &store)), ["pkg.missing"]);
    let m = song_with("\"@alice/felt-piano\" = { path = \"../nowhere\" }\n");
    assert_eq!(codes(&resolve(&song, &m, &Lock::default(), &store)), ["pkg.missing"]);

    // std and the SDK are never resolved here: the app has them.
    assert!(resolve(&song, &song_with(""), &Lock::default(), &store).packs.is_empty());
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
