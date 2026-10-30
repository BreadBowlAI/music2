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
    song_and_store_with(f, &[])
}

/// [`song_and_store`], with these files added to the pack before it is listed.
fn song_and_store_with(f: &Fixture, files: &[(&str, &str)]) -> (Session, m2_song::Sha) {
    f.write("music2.toml", MANIFEST);
    f.write(
        "song.ts",
        "import { song } from \"@m2/core\";\nimport { master, mixer } from \"std/mix\";\n\nexport default song(() => master(mixer({})));\n",
    );
    let store = f.0.join("Packs");
    let dir = Store::at(store.clone()).version_dir(&PackName::new("@test/tiny").unwrap(), &Version(0, 1, 0));
    copy(&tiny(), &dir);
    for (path, text) in files {
        std::fs::create_dir_all(dir.join(path).parent().unwrap()).unwrap();
        std::fs::write(dir.join(path), text).unwrap();
    }
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
    assert_eq!(keys.pack, Some(From { name: "@test/tiny".into(), version: "0.1.0".into() }), "the list knows its pack");
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
    assert!(manifest.ends_with("std = \"0.5\"\n\"@test/tiny\" = \"0.1\"\n"), "{manifest}");
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

// PACK-13, PACK-10, PACK-11: an instrument written as data is offered with the pack's others, and
// + Track adds it as one written in code: its import from "<package>/instruments", its dependency
// and its lock, and the song builds and plays it.
#[test]
fn a_data_instrument_is_offered_and_added_as_any_other() {
    let f = Fixture::new();
    let piano = "label = \"Tiny piano\"\ncategory = \"Keys\"\nengine = \"std:sampler\"\nmode = \"gate\"\n\n\
                 [[region]]\nfile = \"samples/pluck.wav\"\nroot = 60\nkeys = [0, 127]\n";
    let (mut s, _) = song_and_store_with(&f, &[("instruments/tiny-piano.toml", piano)]);
    let offered = crate::tracks::instruments(&s).into_iter().find(|i| i.component == "@test/tiny:tiny-piano").expect("offered");
    assert_eq!((offered.module.as_str(), offered.name.as_str(), offered.category.as_str()), ("@test/tiny/instruments", "tinyPiano", "Keys"));
    assert_eq!(offered.pack, Some(From { name: "@test/tiny".into(), version: "0.1.0".into() }));
    let made = crate::tracks::add(&mut s, "@test/tiny:tiny-piano", None, None, None).unwrap_or_else(|e| panic!("{e}"));
    let track = f.read(&format!("tracks/{}.ts", made.module));
    assert!(track.contains("import { tinyPiano } from \"@test/tiny/instruments\";"), "{track}");
    assert!(f.read("music2.toml").contains("\"@test/tiny\" = \"0.1\""));
    assert!(f.read("music2.lock").contains("@test/tiny"));
    let rev = s.accepted();
    assert!(rev.compiled.problems.iter().all(|d| d.severity != m2_song::Severity::Error), "{:?}", rev.compiled.problems);
    let frames: Vec<&str> = rev.compiled.ir.modules.iter().flat_map(|m| m.frames.iter().filter_map(|f| f.component.as_deref())).collect();
    assert!(frames.contains(&"@test/tiny:tiny-piano"), "{frames:?}");
}

// PACK-10: adding a pack's effect writes the same, in the effect's one Tx.
#[test]
fn adding_a_pack_effect_writes_its_dependency_and_lock() {
    let f = Fixture::new();
    let (mut s, _) = song_and_store(&f);
    let made = crate::tracks::add(&mut s, "std:synth-pluck", None, None, None).unwrap();
    assert!(!f.0.join("music2.lock").exists());
    let file = format!("tracks/{}.ts", made.module);
    let rev = s.accepted();
    let site = rev.compiled.sites.iter().find(|x| x.file == file && x.callee.ends_with("pluck")).expect("the pluck's call");
    let (call, at) = (format!("{file}#{}", site.ordinal), rev.number);
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

// PACK-10: changing a track's instrument to a pack's writes its dependency and lock with the swap,
// in its one Tx.
#[test]
fn changing_an_instrument_to_a_packs_writes_its_dependency() {
    let f = Fixture::new();
    let (mut s, hash) = song_and_store(&f);
    let made = crate::tracks::add(&mut s, "std:synth-pluck", None, None, None).unwrap();
    crate::swap::swap(&mut s, &made.module, "@test/tiny:tiny-keys").unwrap_or_else(|e| panic!("{e}"));
    assert!(f.read("music2.toml").contains("\"@test/tiny\" = \"0.1\""), "{}", f.read("music2.toml"));
    let lock = Lock::read(&f.read("music2.lock")).unwrap();
    assert_eq!(lock.get(&PackName::new("@test/tiny").unwrap()).map(|l| l.hash), Some(hash));
    assert!(f.read(&format!("tracks/{}.ts", made.module)).contains("tinyKeys("));
    // One undo takes the swap away whole.
    s.undo().unwrap();
    assert_eq!(f.read("music2.toml"), MANIFEST);
}

// PACK-10: a lock entry left from before, for a pack the manifest no longer lists, doesn't stop
// adding the pack's item: its line is written again, and its entry replaced.
#[test]
fn a_lock_left_from_before_gets_its_line_again() {
    let f = Fixture::new();
    let (_, hash) = song_and_store(&f);
    let left = Lock::default().with(PackName::new("@test/tiny").unwrap(), Version(0, 1, 0), m2_song::Sha::of(b"before"));
    f.write("music2.lock", left.canonical());
    let mut p = PackageSet::default().with_store(f.0.join("Packs"));
    p.load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")).unwrap();
    let mut s = Session::open(&f.0, p).unwrap_or_else(|d| panic!("{d:?}"));
    assert!(s.draft_errors.is_empty(), "a lock entry no line asks for stops nothing: {:?}", s.draft_errors);
    crate::tracks::add(&mut s, "@test/tiny:tiny-keys", None, None, None).unwrap_or_else(|e| panic!("{e}"));
    assert!(f.read("music2.toml").contains("\"@test/tiny\" = \"0.1\""), "{}", f.read("music2.toml"));
    let lock = Lock::read(&f.read("music2.lock")).unwrap();
    assert_eq!(lock.get(&PackName::new("@test/tiny").unwrap()).map(|l| l.hash), Some(hash), "the entry from before is replaced");
}

/// `@test/tiny` installed in the store at `<f>/Packs` as `name` at `version`, its `m2pkg.toml`
/// changed by `edit` and its index replaced by `index` when given.
fn install_as(f: &Fixture, name: &str, version: &str, edit: impl Fn(String) -> String, index: Option<&str>) {
    let dir = Store::at(f.0.join("Packs")).version_dir(&PackName::new(name).unwrap(), &Version::parse(version).unwrap());
    copy(&tiny(), &dir);
    let toml = std::fs::read_to_string(dir.join("m2pkg.toml")).unwrap();
    let toml = toml
        .replace("name = \"@test/tiny\"", &format!("name = \"{name}\""))
        .replace("version = \"0.1.0\"", &format!("version = \"{version}\""));
    std::fs::write(dir.join("m2pkg.toml"), edit(toml)).unwrap();
    if let Some(index) = index {
        std::fs::write(dir.join("src/index.ts"), index).unwrap();
    }
    // Its presets are tiny's, for tiny's components: none of this pack's.
    std::fs::remove_dir_all(dir.join("presets")).unwrap();
    let listing = Listing::of(&dir).unwrap();
    std::fs::write(dir.join(LISTING), listing.text()).unwrap();
}

/// A session of the song at `f`, with the store at `<f>/Packs`.
fn open(f: &Fixture) -> Session {
    let mut p = PackageSet::default().with_store(f.0.join("Packs"));
    p.load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")).unwrap();
    Session::open(&f.0, p).unwrap_or_else(|d| panic!("{d:?}"))
}

// PACK-11, PACK-2: the lists offer the newest version of a pack this app can run, and none of a
// pack that needs a newer SDK or std than it has.
#[test]
fn a_pack_this_app_cant_run_isnt_offered() {
    let f = Fixture::new();
    song_and_store(&f);
    // A newer @test/tiny for an SDK this app doesn't have: 0.1.0 is still offered.
    install_as(&f, "@test/tiny", "0.2.0", |t| t.replace("\"@m2/core\" = \"1\"", "\"@m2/core\" = \"9\""), None);
    // A pack whose every version needs a std this app doesn't have.
    install_as(&f, "@test/later", "1.0.0", |t| t.replace("std = \"0.4\"", "std = \"9.0\""), None);
    let s = open(&f);
    assert_eq!(from(&s, "@test/tiny:tiny-keys").map(|p| p.version), Some("0.1.0".into()));
    let offered: Vec<String> = m2_session::offered(&s.packages).into_iter().map(|(n, _)| n).collect();
    assert_eq!(offered, ["@test/tiny"], "@test/later needs std 9.0");
}

// PACK-11: a store pack the song doesn't use, whose module doesn't load, leaves its components out
// of the lists and puts no problem on the song; std's and the song's own packs' still would.
#[test]
fn a_store_pack_the_song_doesnt_use_puts_no_problem_on_it() {
    let f = Fixture::new();
    song_and_store(&f);
    install_as(&f, "@test/broken", "1.0.0", |t| t, Some("throw new Error(\"broken on purpose\");\n"));
    let s = open(&f);
    assert!(!s.catalog().problems.is_empty(), "the catalog knows the module doesn't load");
    let invalid: Vec<&str> = s.problems().iter().filter(|d| d.code == "pkg.invalid").map(|d| d.message.as_str()).collect();
    assert!(invalid.is_empty(), "{invalid:?}");
    assert!(crate::tracks::instruments(&s).iter().any(|i| i.component == "@test/tiny:tiny-keys"), "the rest is still offered");
}
