use std::path::{Path, PathBuf};

use m2_runtime::PackageSet;
use m2_session::Session;
use m2_song::Version;
use m2_song::pack::{LISTING, Listing, Lock, PackName, Store};

use super::*;
use crate::testing::{Fixture, MANIFEST};

/// The fixture pack `@test/tiny`.
fn tiny() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../core/session/tests/fixtures/packs/tiny")
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if e.file_type().unwrap().is_dir() { copy(&src, &dst) } else { std::fs::copy(&src, &dst).map(drop).unwrap() }
    }
}

/// A song with std's mixer and no tracks, and a store at `<song>/../Packs` holding `@test/tiny`
/// 0.1.0: its session, and the pack's hash.
fn song_and_store(f: &Fixture) -> (Session, m2_song::Sha) {
    f.write("music2.toml", MANIFEST);
    f.write(
        "song.ts",
        "import { song } from \"@m2/core\";\nimport { master, mixer } from \"std/mix\";\n\nexport default song(() => master(mixer({})));\n",
    );
    let store = f.0.join("Packs");
    let dir = Store::at(store.clone()).version_dir(&PackName::new("@test/tiny").unwrap(), &Version(0, 1, 0));
    copy(&tiny(), &dir);
    let listing = Listing::of(&dir).unwrap();
    std::fs::write(dir.join(LISTING), listing.text()).unwrap();
    let mut p = PackageSet::default().with_store(store);
    p.load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")).unwrap();
    (Session::open(&f.0, p).unwrap_or_else(|d| panic!("{d:?}")), listing.hash())
}

// PACK-11: the store's packs are offered with std's, each knowing its pack.
#[test]
fn a_pack_in_the_store_is_offered() {
    let f = Fixture::new();
    let (s, _) = song_and_store(&f);
    let keys = crate::tracks::instruments(&s).into_iter().find(|i| i.component == "@test/tiny:tiny-keys").expect("offered");
    assert_eq!((keys.module.as_str(), keys.name.as_str(), keys.category.as_str()), ("@test/tiny", "tinyKeys", "Keys"));
    assert_eq!(from(&s, "@test/tiny:tiny-keys"), Some(From { name: "@test/tiny".into(), version: "0.1.0".into() }));
    assert_eq!(from(&s, "std:synth-pluck"), None);
    assert!(crate::effect::effects(&s).iter().any(|e| e.component == "@test/tiny:tiny-crunch"));
}

// PACK-10: adding a pack's instrument writes its import, its [dependencies] line and its lock
// entry in one Tx, one undo step; adding another of its items writes neither again.
#[test]
fn adding_a_pack_instrument_writes_its_dependency_and_lock_in_one_undo() {
    let f = Fixture::new();
    let (mut s, hash) = song_and_store(&f);
    let made = crate::tracks::add(&mut s, "@test/tiny:tiny-keys", None, None, None).unwrap_or_else(|e| panic!("{e}"));
    let manifest = f.read("music2.toml");
    assert!(manifest.ends_with("std = \"0.4\"\n\"@test/tiny\" = \"0.1\"\n"), "{manifest}");
    let lock = Lock::read(&f.read("music2.lock")).unwrap();
    assert_eq!(lock.get(&PackName::new("@test/tiny").unwrap()).map(|l| (l.version, l.hash)), Some((Version(0, 1, 0), hash)));
    let track = f.read(&format!("tracks/{}.ts", made.module));
    assert!(track.contains("import { tinyKeys } from \"@test/tiny\";"), "{track}");
    assert_eq!(s.accepted().compiled.packs.len(), 1, "the song builds with the pack");
    // A second item of the pack writes only its own import.
    crate::tracks::add(&mut s, "@test/tiny:tiny-sampler", None, None, None).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(f.read("music2.toml"), manifest);
    // One undo takes the second away; another takes the first, its dependency and its lock.
    s.undo().unwrap();
    s.undo().unwrap();
    assert_eq!(f.read("music2.toml"), MANIFEST);
    assert!(!f.0.join("music2.lock").exists(), "the lock was new, so it goes");
}

// PACK-10: adding a pack's effect writes the same, in the effect's one Tx.
#[test]
fn adding_a_pack_effect_writes_its_dependency_and_lock() {
    let f = Fixture::new();
    let (mut s, _) = song_and_store(&f);
    let made = crate::tracks::add(&mut s, "std:synth-pluck", None, None, None).unwrap();
    assert!(!f.0.join("music2.lock").exists());
    let file = format!("tracks/{}.ts", made.module);
    let rev = s.accepted().clone();
    let site = rev.compiled.sites.iter().find(|x| x.file == file && x.callee.ends_with("pluck")).expect("the pluck's call");
    let call = format!("{file}#{}", site.ordinal);
    let at = rev.number;
    crate::effect::add_effect(&mut s, at, &call, "@test/tiny:tiny-crunch").unwrap_or_else(|e| panic!("{e}"));
    assert!(f.read("music2.toml").contains("\"@test/tiny\" = \"0.1\""));
    assert!(f.read("music2.lock").contains("@test/tiny"));
    assert!(f.read(&file).contains("tinyCrunch("), "{}", f.read(&file));
}

// PACK-10: the line goes after the table's last entry, wherever the table is.
#[test]
fn the_dependency_line_goes_at_the_end_of_its_table() {
    let name = PackName::new("@a/b").unwrap();
    let req = Requirement::parse("1.2").unwrap();
    let text = "[project]\nname = \"x\"\n\n[dependencies]\n\"@m2/core\" = \"1\" # the SDK\nstd = \"0.4\"\n\n[other]\n";
    let (at, line) = dependency_line(text, &name, &req).unwrap();
    let mut out = text.to_string();
    out.insert_str(at as usize, &line);
    assert_eq!(
        out,
        "[project]\nname = \"x\"\n\n[dependencies]\n\"@m2/core\" = \"1\" # the SDK\nstd = \"0.4\"\n\"@a/b\" = \"1.2\"\n\n[other]\n"
    );
    let (at, line) = dependency_line("[dependencies]\nstd = \"0.4\"", &name, &req).unwrap();
    assert_eq!((at, line.as_str()), (26, "\n\"@a/b\" = \"1.2\"\n"), "a last line with no newline gets one");
    assert!(dependency_line("[project]\nname = \"x\"\n", &name, &req).is_none());
}

// PACK-11, FMT-22: a pack's own presets are in its folder and load as std's do; yours of a scoped
// sound go in one flat folder per sound, never nested.
#[test]
fn a_packs_presets_are_its_own_and_yours_are_never_nested() {
    assert_eq!(crate::presets::folder("@alice/felt-piano:felt-piano"), Path::new("_alice_felt-piano").join("felt-piano"));
    let f = Fixture::new();
    let (mut s, _) = song_and_store(&f);
    let made = crate::tracks::add(&mut s, "@test/tiny:tiny-keys", None, None, None).unwrap();
    let sound = "@test/tiny:tiny-keys";
    let dir = crate::presets::package_dir(&s, sound).expect("the pack's folder");
    let own = crate::presets::factory(&dir, sound).unwrap();
    let listed: Vec<String> = crate::presets::list(&own, sound).into_iter().map(|l| l.name).collect();
    assert_eq!(listed, ["Dark"]);
    let dark = crate::presets::read(&own, sound, "Dark").unwrap();
    let file = format!("tracks/{}.ts", made.module);
    let site = s.accepted().compiled.sites.iter().rfind(|x| x.file == file && x.callee == "tinyKeys").expect("the call").clone();
    let at = s.accepted().number;
    crate::presets::load(&mut s, at, &format!("{file}#{}", site.ordinal), &dark).unwrap_or_else(|e| panic!("{e}"));
    assert!(f.read(&file).contains("brightness: 600"), "{}", f.read(&file));
}
