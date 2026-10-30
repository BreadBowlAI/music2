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
    let offered: Vec<String> = m2_session::offered(&s.packages).into_iter().map(|(n, ..)| n).collect();
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

/// `@test/tiny` at `version` in the store at `<f>/Packs`, as an install leaves it, with a kernel of
/// its own: `@test/tiny:sine`, std's module under its own name (`tag` as a custom section, so each
/// version's module has its own hash), played by its instrument `tinyTone`. Its module's hash.
fn install_with_kernel(f: &Fixture, version: &str, tag: &str) -> m2_song::Sha {
    let v = Version::parse(version).unwrap();
    let dir = Store::at(f.0.join("Packs")).version_dir(&PackName::new("@test/tiny").unwrap(), &v);
    copy(&tiny(), &dir);
    let toml = std::fs::read_to_string(dir.join("m2pkg.toml")).unwrap();
    let toml = toml.replace("version = \"0.1.0\"", &format!("version = \"{version}\"\nwasm = \"k.wasm\""))
        + "\n[[kernel]]\nname = \"sine\"\ndescription = \"A sine\"\ninputs = [\"freq: control<hz> = 440\"]\noutputs = [\"out: audio<1>\"]\n";
    std::fs::write(dir.join("m2pkg.toml"), toml).unwrap();
    let mut wasm = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std/kernels/std.wasm")).unwrap();
    let name = b"m2-test";
    wasm.extend([0u8, (1 + name.len() + tag.len()) as u8, name.len() as u8]);
    wasm.extend(name);
    wasm.extend(tag.as_bytes());
    std::fs::write(dir.join("k.wasm"), &wasm).unwrap();
    let index = std::fs::read_to_string(dir.join("src/index.ts")).unwrap();
    let tone = "\nimport { hz, kernel } from \"@m2/core\";\n\n/** A sine of the pack's own kernel. */\nexport const tinyTone = component(\n  {\n    \
                id: \"tiny-tone\",\n    label: \"Tiny tone\",\n    signature: signature<(notes: Events) => Audio>(),\n    \
                inputs: { notes: port.notes() },\n    output: port.audio(),\n    editor: { category: \"Keys\", description: \"A sine\" },\n  },\n  \
                (_notes: Events): Audio => kernel(\"@test/tiny:sine\", { freq: hz(440) }),\n);\n";
    std::fs::write(dir.join("src/index.ts"), index + tone).unwrap();
    let listing = Listing::of(&dir).unwrap();
    std::fs::write(dir.join(LISTING), listing.text()).unwrap();
    m2_song::Sha::of(&wasm)
}

const SINE: &str = "@test/tiny:sine";

/// A song at `f` whose track `tone` plays `@test/tiny`'s own kernel, `"@test/tiny" = "0.1"` and
/// locked to 0.1.0, with 0.1.0 (module `one`) installed: its session, and the module's hash.
fn tone_song(f: &Fixture) -> (Session, m2_song::Sha) {
    let m1 = install_with_kernel(f, "0.1.0", "one");
    f.write("music2.toml", &format!("{MANIFEST}\"@test/tiny\" = \"0.1\"\n"));
    f.write(
        "song.ts",
        "import { song } from \"@m2/core\";\nimport { master, mixer } from \"std/mix\";\nimport tone from \"./tracks/tone\";\n\nexport default song(() => master(mixer({ tone: tone.out })));\n",
    );
    f.write(
        "tracks/tone.ts",
        "import { track } from \"@m2/core\";\nimport { tinyTone } from \"@test/tiny\";\n\nexport default track(\"tone\", { name: \"Tone\" }, ({ lane }) => tinyTone(lane.notes(\"notes\")));\n",
    );
    let tiny = PackName::new("@test/tiny").unwrap();
    let hash = Store::at(f.0.join("Packs")).hash(&tiny, &Version(0, 1, 0)).unwrap();
    f.write("music2.lock", &Lock::default().with(tiny, Version(0, 1, 0), hash).canonical());
    let s = open(f);
    assert!(s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    assert_eq!(s.packages.module_sha(SINE), Some(m1));
    (s, m1)
}

/// The module hash `info`'s plan gives kernel `id`.
fn module_of(info: &m2_runtime::PlanInfo, id: &str) -> Option<m2_song::Sha> {
    info.shapes.iter().flatten().find_map(|s| match s {
        m2_runtime::Shape::Kernel { id: k, wasm, .. } if k == id => Some(*wasm),
        _ => None,
    })
}

// PACK-9, RENDER-2: a stem renders with the packs of the revision it was made from, though the
// song moved to another version before it rendered (stems render one at a time, edits go on).
#[test]
fn a_stem_renders_the_view_it_was_planned_on() {
    use crate::render::{Length, Parts, Render, Threads};
    let f = Fixture::new();
    let (mut s, m1) = tone_song(&f);
    let rev = s.accepted_arc();
    let ir = crate::render::rooted(&rev.compiled.ir, "tone", "out").unwrap();
    let stem = std::sync::Arc::new(Parts::of(&*rev, ir, &s.packages).unwrap_or_else(|d| panic!("{d:?}")));
    // `m2 pack update` while the stem waits: the song moves to 0.1.1.
    let m2 = install_with_kernel(&f, "0.1.1", "two");
    write_lock(&mut s, pack::Move::All).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(s.packages.module_sha(SINE), Some(m2), "the song moved");
    let r = Render::new(stem, &s.packages, 48_000, 0.0, Length::Seconds(0.1), Threads::Exactly(1)).unwrap_or_else(|e| panic!("{e:#}"));
    assert_eq!(module_of(&r.info(), SINE), Some(m1), "the stem plays the version it was made with");
}

// PACK-9, PACK-11: the lists follow the accepted revision's packs: `m2 pack update` while the song
// is open moves its items to the new version's, as the list names it.
#[test]
fn the_lists_follow_the_accepted_view() {
    let f = Fixture::new();
    song_and_store(&f);
    f.write("music2.toml", &format!("{MANIFEST}\"@test/tiny\" = \"0.1\"\n"));
    let mut s = open(&f);
    write_lock(&mut s, pack::Move::Needed).unwrap_or_else(|e| panic!("{e}"));
    let tone = "@test/tiny:tiny-tone";
    assert!(!s.catalog().components.contains_key(tone), "0.1.0 has no tone");
    install_with_kernel(&f, "0.1.1", "two");
    write_lock(&mut s, pack::Move::All).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(s.accepted().compiled.packs[0].version, Version(0, 1, 1));
    assert!(s.catalog().components.contains_key(tone), "the lists have 0.1.1's items");
    assert!(crate::tracks::instruments(&s).iter().any(|i| i.component == tone));
    assert_eq!(from(&s, tone).map(|p| p.version), Some("0.1.1".into()));
}

// PACK-11, PACK-10: an add takes the version the lists offer the item from, not one installed
// since the lists were made.
#[test]
fn an_add_takes_the_version_the_lists_show() {
    let f = Fixture::new();
    let (mut s, hash) = song_and_store(&f);
    assert_eq!(from(&s, "@test/tiny:tiny-keys").map(|p| p.version), Some("0.1.0".into()));
    install_with_kernel(&f, "0.1.1", "two");
    assert_eq!(from(&s, "@test/tiny:tiny-keys").map(|p| p.version), Some("0.1.0".into()), "the list says what it shows");
    crate::tracks::add(&mut s, "@test/tiny:tiny-keys", None, None, None).unwrap_or_else(|e| panic!("{e}"));
    let lock = Lock::read(&f.read("music2.lock")).unwrap();
    assert_eq!(lock.get(&PackName::new("@test/tiny").unwrap()).map(|l| (l.version, l.hash)), Some((Version(0, 1, 0), hash)));
}

// PACK-6: a repair whose new copy can't go in (a scanner holding a file just written, on Windows)
// puts the copy before back, read-only, and leaves nothing aside.
#[test]
fn a_repair_that_cant_put_the_new_copy_in_puts_the_old_one_back() {
    let f = Fixture::new();
    let pack = f.0.join("Packs/@a/x");
    let (target, temp, old) = (pack.join("1.0.0"), pack.join(".1.0.0.1.tmp"), pack.join(".1.0.0.1.old"));
    f.write("Packs/@a/x/1.0.0/m2pack.json", "before");
    f.write("Packs/@a/x/.1.0.0.1.tmp/m2pack.json", "new");
    read_only_inside(&target).unwrap();
    set_read_only(&target, true).unwrap();
    let mut rename = |a: &Path, b: &Path| match a == temp {
        true => Err(std::io::Error::other("held by another program")),
        false => std::fs::rename(a, b),
    };
    assert!(swap(&temp, &target, &old, &mut rename).is_err());
    assert_eq!(std::fs::read_to_string(target.join("m2pack.json")).unwrap(), "before", "the version is as it was");
    assert!(!old.exists(), "nothing is left aside");
    assert!(std::fs::metadata(&target).unwrap().permissions().readonly(), "read-only again");
    remove_tree(&f.0.join("Packs")).unwrap();
}

// PACK-6: the copy a stopped install left aside goes with the next install of that version.
#[test]
fn an_install_removes_a_copy_a_stopped_install_left_aside() {
    let f = Fixture::new();
    let folder = f.0.join("tiny");
    copy(&tiny(), &folder);
    let store = Store::at(f.0.join("Packs"));
    let std = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std");
    install(&folder, &store, &std).unwrap_or_else(|e| panic!("{e:?}"));
    let left = store.dir.join("@test/tiny/.0.1.0.999999.old");
    f.write("Packs/@test/tiny/.0.1.0.999999.old/src/index.ts", "// the changed copy\n");
    read_only_inside(&left).unwrap();
    assert!(matches!(install(&folder, &store, &std), Ok(Install::Already { .. })));
    assert!(!left.exists(), "the copy left aside is gone");
    remove_tree(&store.dir).unwrap();
}
