use std::path::{Path, PathBuf};

use m2_runtime::PackageSet;
use m2_session::Session;
use m2_song::Version;
use m2_song::pack::{LISTING, Listing, Lock, PackName, Store, read_only_inside, remove_tree};

use super::*;
use crate::testing::Fixture;

/// A song's manifest that needs std alone: these songs play `@test/tiny` from a store of their own.
const MANIFEST: &str = "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"2\"\nstd = \"0.9\"\n";

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
/// 0.1.0 and the packs music2 ships, as every store does (PACK-16): its session, and tiny's hash.
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
    for (pack, done) in m2_song::pack::install_bundled(&m2_song::pack::source_packs(), &Store::at(store.clone())) {
        done.unwrap_or_else(|e| panic!("{}: {e}", pack.display()));
    }
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
    assert_eq!(from(&s, "std:sampler"), None, "std's items are from no pack");
    // music2's own sounds are a pack's, in the store as any (PACK-15).
    let synths = m2_song::pack::PkgManifest::at(&m2_song::pack::source_packs().join("@m2/synths")).unwrap().package.version;
    assert_eq!(from(&s, "@m2/synths:synth-pluck"), Some(From { name: "@m2/synths".into(), version: synths.to_string() }));
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
    assert!(manifest.ends_with("std = \"0.9\"\n\"@test/tiny\" = \"0.1\"\n"), "{manifest}");
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
    assert_eq!(
        (offered.module.as_str(), offered.name.as_str(), offered.category.as_str()),
        ("@test/tiny/instruments", "tinyPiano", "Keys")
    );
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
    let made = crate::tracks::add(&mut s, "@m2/synths:synth-pluck", None, None, None).unwrap();
    // A pack music2 ships is added as any pack is (PACK-15, PACK-10).
    let v = m2_song::pack::PkgManifest::at(&m2_song::pack::source_packs().join("@m2/synths")).unwrap().package.version;
    let line = format!("\"@m2/synths\" = \"{}.{}\"", v.0, v.1);
    assert!(f.read("music2.toml").contains(&line) && f.read("music2.lock").contains("@m2/synths"), "{line}");
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
    let text = "[project]\nname = \"x\"\n\n[dependencies]\n\"@m2/core\" = \"2\" # the SDK\nstd = \"0.4\"\n\n[other]\n";
    let (at, line) = dependency_line(text, &name, &req).unwrap();
    let mut out = text.to_string();
    out.insert_str(at as usize, &line);
    assert_eq!(
        out,
        "[project]\nname = \"x\"\n\n[dependencies]\n\"@m2/core\" = \"2\" # the SDK\nstd = \"0.4\"\n\"@a/b\" = \"1.2\"\n\n[other]\n"
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

// PACK-10: two items of one pack added in one Tx (two drum tracks of a MIDI file) write its
// dependency line once, and its lock entry once.
#[test]
fn two_items_of_one_pack_need_it_once() {
    let f = Fixture::new();
    let (s, hash) = song_and_store(&f);
    let needs = needs_all(&s, &["@test/tiny:tiny-keys", "@test/tiny:tiny-crunch", "@test/tiny:tiny-keys"]).unwrap();
    assert_eq!(needs.ops.len(), 2, "one manifest edit and one lock: {:?}", needs.ops);
    let Op::TextEdit { edits, .. } = &needs.ops[0] else { panic!("{:?}", needs.ops[0]) };
    assert_eq!(edits.iter().map(|e| e.text.as_str()).collect::<String>().matches("\"@test/tiny\"").count(), 1, "{edits:?}");
    let Op::Write { bytes: Payload::Text(text), .. } = &needs.ops[1] else { panic!("{:?}", needs.ops[1]) };
    let lock = Lock::read(text).unwrap();
    assert_eq!((lock.packs.len(), lock.packs[0].hash), (1, hash));
}

// PACK-10: changing a track's instrument to a pack's writes its dependency and lock with the swap,
// in its one Tx.
#[test]
fn changing_an_instrument_to_a_packs_writes_its_dependency() {
    let f = Fixture::new();
    let (mut s, hash) = song_and_store(&f);
    let made = crate::tracks::add(&mut s, "@m2/synths:synth-pluck", None, None, None).unwrap();
    let before = f.read("music2.toml");
    crate::swap::swap(&mut s, &made.module, "@test/tiny:tiny-keys").unwrap_or_else(|e| panic!("{e}"));
    assert!(f.read("music2.toml").contains("\"@test/tiny\" = \"0.1\""), "{}", f.read("music2.toml"));
    let lock = Lock::read(&f.read("music2.lock")).unwrap();
    assert_eq!(lock.get(&PackName::new("@test/tiny").unwrap()).map(|l| l.hash), Some(hash));
    assert!(f.read(&format!("tracks/{}.ts", made.module)).contains("tinyKeys("));
    // One undo takes the swap away whole.
    s.undo().unwrap();
    assert_eq!(f.read("music2.toml"), before);
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
    install_as(&f, "@test/tiny", "0.2.0", |t| t.replace("\"@m2/core\" = \"2\"", "\"@m2/core\" = \"9\""), None);
    // A pack whose every version needs a std this app doesn't have.
    install_as(&f, "@test/later", "1.0.0", |t| t.replace("std = \"0.4\"", "std = \"9.0\""), None);
    let s = open(&f);
    assert_eq!(from(&s, "@test/tiny:tiny-keys").map(|p| p.version), Some("0.1.0".into()));
    // Beside the packs music2 ships, which every store holds (PACK-16).
    let offered: Vec<String> = m2_session::offered(&s.packages).into_iter().map(|(n, ..)| n).filter(|n| !n.starts_with("@m2/")).collect();
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
    let tone = "\nimport { hz } from \"@m2/core\";\n\n/** A sine of the pack's own kernel. */\nexport const tinyTone = component(\n  {\n    \
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
    f.write("music2.toml", format!("{MANIFEST}\"@test/tiny\" = \"0.1\"\n"));
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
    f.write("music2.lock", Lock::default().with(tiny, Version(0, 1, 0), hash).canonical());
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
    f.write("music2.toml", format!("{MANIFEST}\"@test/tiny\" = \"0.1\"\n"));
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

// PACK-15, PACK-12: each pack music2 ships passes `m2 pack check`, as the registry's robot will
// check it (PACK-19).
#[test]
fn the_bundled_packs_pass_check() {
    let std_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std");
    let packs = m2_song::pack::bundled(&m2_song::pack::source_packs());
    assert_eq!(packs.len(), 4, "@m2/drums, @m2/effects, @m2/mixing and @m2/synths: {packs:?}");
    for dir in packs {
        assert_eq!(check(&dir, &std_dir), Vec::<String>::new(), "{}", dir.display());
        assert!(!dir.join(LISTING).exists(), "{}: checking writes nothing where it ships", dir.display());
    }
}

// PACK-20 (review X4): a pack's `kernels-src/` is never installed, so nothing of the pack is read
// from it: `m2 pack check` refuses a module there, and code that imports from there, as a song
// would once the pack is installed.
#[test]
fn check_refuses_a_pack_that_reads_its_kernel_source() {
    let f = Fixture::new();
    let std_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std");
    // Its module in kernels-src/.
    let fx = f.0.join("fx");
    copy(&m2_song::pack::source_packs().join("@m2/effects"), &fx);
    std::fs::rename(fx.join("kernels/effects.wasm"), fx.join("kernels-src/fx.wasm")).unwrap();
    std::fs::remove_dir_all(fx.join("presets")).unwrap();
    let toml = std::fs::read_to_string(fx.join("m2pkg.toml")).unwrap();
    let toml = toml.replace("name = \"@m2/effects\"", "name = \"@alice/fx\"").replace("kernels/effects.wasm", "kernels-src/fx.wasm");
    std::fs::write(fx.join("m2pkg.toml"), toml).unwrap();
    let said = check(&fx, &std_dir).join("\n");
    assert!(said.contains("`kernels-src/fx.wasm`") && said.contains("kernels-src/ is no part of the pack"), "{said}");
    // Code that imports from it.
    let small = f.0.join("tiny");
    copy(&tiny(), &small);
    std::fs::create_dir_all(small.join("kernels-src")).unwrap();
    std::fs::write(small.join("kernels-src/extra.ts"), "export const extra = 1;\n").unwrap();
    let code = std::fs::read_to_string(small.join("src/index.ts")).unwrap();
    std::fs::write(
        small.join("src/index.ts"),
        format!("import {{ extra }} from \"../kernels-src/extra\";\nexport const e = extra;\n{code}"),
    )
    .unwrap();
    let said = check(&small, &std_dir).join("\n");
    assert!(said.contains("../kernels-src/extra") && said.contains("kernels-src/ is no part of the pack"), "{said}");
}

// PACK-12, PKG-7: a component with no category is named, as one its author forgot; one that says
// `unlisted: true` is code's on purpose, and no list offers it.
#[test]
fn a_component_with_no_category_is_named_unless_it_says_unlisted() {
    let f = Fixture::new();
    let dir = f.0.join("tiny");
    copy(&tiny(), &dir);
    let std_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std");
    let plain = |editor: &str| {
        format!(
            "\n/** A plain sine, for code. */\nexport const tinyPlain = component(\n  {{\n    id: \"tiny-plain\",\n    label: \"Tiny plain\",\n    \
             signature: signature<(notes: Events) => Audio>(),\n    inputs: {{ notes: port.notes() }},\n    output: port.audio(),{editor}\n  }},\n  \
             (notes: Events): Audio => poly(notes, (n) => vca(osc.sine(n.pitch), adsr(n.gate))),\n);\n"
        )
    };
    // The pack imports std/dsp's blocks already.
    let index = std::fs::read_to_string(dir.join("src/index.ts")).unwrap();
    assert!(index.contains("import { adsr, osc, svf, vca } from \"std/dsp\";"));
    std::fs::write(dir.join("src/index.ts"), format!("{index}{}", plain(""))).unwrap();
    let problems = check(&dir, &std_dir);
    assert!(problems.iter().any(|p| p.starts_with("@test/tiny:tiny-plain") && p.contains("has no category")), "{problems:?}");
    std::fs::write(dir.join("src/index.ts"), format!("{index}{}", plain("\n    editor: { unlisted: true },"))).unwrap();
    assert_eq!(check(&dir, &std_dir), Vec::<String>::new(), "said on purpose");
    let s = song_and_store_with(&f, &[("src/index.ts", &std::fs::read_to_string(dir.join("src/index.ts")).unwrap())]).0;
    assert!(s.catalog().components.contains_key("@test/tiny:tiny-plain"), "code can play it");
    assert!(!crate::tracks::instruments(&s).iter().any(|i| i.component == "@test/tiny:tiny-plain"), "no list offers it");
}

// PACK-19 (review S9, S13): music2's own is a pack version music2 comes with, by its files, never
// by its name: a folder's `@m2/fake` is everyone else's. Every list ranks by that one rule, std's
// and music2's own first: + Effect as the search; and the library marks music2's alone `own`.
#[test]
fn music2s_own_is_by_its_files_and_every_list_ranks_by_it() {
    let f = Fixture::new();
    song_and_store(&f);
    // Named to sort before `@m2/` and inside it.
    install_as(&f, "@aa/tiny", "0.1.0", |t| t, None);
    install_as(&f, "@m2/fake", "0.1.0", |t| t, None);
    let s = open(&f);
    let order: Vec<String> = crate::effect::effects(&s).into_iter().map(|e| e.component).collect();
    let at = |id: &str| order.iter().position(|c| c == id).unwrap_or_else(|| panic!("{id}: {order:?}"));
    for theirs in ["@aa/tiny:tiny-crunch", "@m2/fake:tiny-crunch", "@test/tiny:tiny-crunch"] {
        assert!(at("std:lowpass") < at(theirs) && at("@m2/effects:delay") < at(theirs), "{theirs}: {order:?}");
    }
    let std_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std");
    let mut packages = vec![("std".to_string(), std_dir.clone())];
    packages.extend(crate::library::store_packs(&f.0.join("Packs"), &std_dir));
    let lib = crate::library::without_song(&packages, None, &[], &[]);
    let own = |id: &str| lib.get(id).unwrap_or_else(|| panic!("{id}")).own;
    assert!(own("std:lowpass") && own("@m2/effects:delay") && own("@m2/synths:synth-pluck"));
    assert!(!own("@m2/fake:tiny-crunch") && !own("@aa/tiny:tiny-crunch"), "its name alone isn't music2's");
    let all: Vec<&str> = lib.items.iter().map(|i| i.id.as_str()).collect();
    let first_theirs = all.iter().position(|id| id.starts_with("@m2/fake:")).unwrap();
    assert!(all.iter().rposition(|id| id.starts_with("@m2/effects:")).unwrap() < first_theirs, "{all:?}");
    assert!(
        all.iter().position(|id| id.starts_with("@aa/tiny:")).unwrap() > all.iter().rposition(|id| id.starts_with("@m2/synths:")).unwrap()
    );
}

// PACK-12 (review G12): every word a pack gives the lists (its m2pkg.toml's, its components'
// labels, descriptions, categories and notes, its sounds' names, its samples' tags, its files'
// names) is on one line, with no control character: a line break or an escape could pass for
// something else where it is shown, a CI's log among them.
#[test]
fn a_packs_words_hold_no_control_characters() {
    let f = Fixture::new();
    let dir = f.0.join("tiny");
    copy(&tiny(), &dir);
    let std_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std");
    assert_eq!(check(&dir, &std_dir), Vec::<String>::new(), "the fixture checks");
    let edit = |rel: &str, from: &str, to: &str| {
        let text = std::fs::read_to_string(dir.join(rel)).unwrap();
        assert!(text.contains(from), "{rel} has {from}");
        std::fs::write(dir.join(rel), text.replace(from, to)).unwrap();
    };
    edit("m2pkg.toml", "description = \"A tiny pack", "description = \"\\u001b[32mA tiny pack");
    edit("m2pkg.toml", "authors = [\"music2\"]", "authors = [\"mu\\nsic2\"]");
    edit("m2pkg.toml", "tags = [\"test\"]", "tags = [\"te\\tst\"]");
    edit("src/index.ts", "label: \"Tiny keys\"", "label: \"Tiny\\n::error::keys\"");
    edit("src/index.ts", "description: \"std's drive, harder\"", "description: \"std's drive\\r harder\"");
    edit("presets/tiny-keys/Dark.json", "\"name\": \"Dark\"", "\"name\": \"Da\\u0007rk\"");
    std::fs::write(dir.join("samples.toml"), "[file.\"pluck.wav\"]\ntags = [\"bright\\u0000\"]\n").unwrap();
    std::fs::write(dir.join("notes\nhere.txt"), "x").unwrap();
    let problems = check(&dir, &std_dir).join("\n");
    for want in [
        "m2pkg.toml: its description holds a control character",
        "m2pkg.toml: its authors hold a control character",
        "m2pkg.toml: its tags hold a control character",
        "@test/tiny:tiny-keys: its label holds a control character",
        "@test/tiny:tiny-crunch: its description holds a control character",
        // A preset's own reader refuses it already (FMT-22).
        "presets/tiny-keys/Dark.json: This preset's name won't do: a preset's name is 1 to 40 characters on one line",
        "samples.toml: its tags hold a control character",
        "\"notes\\nhere.txt\": its name holds a control character",
    ] {
        assert!(problems.contains(want), "says `{want}`:\n{problems}");
    }
}
