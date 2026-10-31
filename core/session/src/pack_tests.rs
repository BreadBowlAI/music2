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

// PACK-2: a pack's std is a lower bound: one made for an older std builds with the app's.
#[test]
fn a_pack_made_for_an_older_std_still_builds() {
    let root = scratch("older");
    copy(&fixtures(), &root);
    let manifest = root.join("tiny/m2pkg.toml");
    let text = std::fs::read_to_string(&manifest).unwrap().replace("std = \"0.4\"", "std = \"0.3\"");
    std::fs::write(&manifest, text).unwrap();
    let mut pk = packages(&root.join("no-store"));
    let c = compile(&Project::open(&root.join("song-path")).unwrap(), &mut pk).unwrap_or_else(|d| panic!("{d:?}"));
    assert_eq!(c.packs.len(), 1);
    std::fs::remove_dir_all(&root).ok();
}

/// `@test/tiny` at `version` in the store at `store`, as an install leaves it, with a kernel of its
/// own: `@test/tiny:sine`, std's sine in the pack's module (std's, with `tag` as a custom section,
/// so each version's module has its own hash), played by its instrument `tinyTone`. Its hash, and
/// its module's.
fn install_tiny_with_kernel(store: &Path, version: &str, tag: &str) -> (m2_song::Sha, m2_song::Sha) {
    let v = Version::parse(version).unwrap();
    let dir = Store::at(store.to_path_buf()).version_dir(&PackName::new("@test/tiny").unwrap(), &v);
    copy(&fixtures().join("tiny"), &dir);
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
    (listing.hash(), m2_song::Sha::of(&wasm))
}

/// A Tx of `ops` in the song at `dir`, each file based on what it holds now.
fn based(dir: &Path, ops: Vec<m2_song::tx::Op>) -> m2_song::tx::Tx {
    use m2_song::tx::{Base, Expect, Tx};
    let mut tx = Tx::new("pack-test", ("before".into(), "after".into()), ops, Expect::Seen(m2_song::Sha::of(b"")));
    for path in tx.touched() {
        let base = match std::fs::read_to_string(dir.join(path.path())) {
            Ok(text) => Base::text(&text),
            Err(_) => Base::Absent,
        };
        tx.base.insert(path, base);
    }
    tx
}

/// Commit `tx` as `m2 tx --seen` does: checked, then committed on the delta it showed.
fn commit_seen(s: &mut crate::Session, mut tx: m2_song::tx::Tx) -> u64 {
    let checked = s.check(&tx).unwrap_or_else(|r| panic!("{r}"));
    tx.expect = m2_song::tx::Expect::Seen(checked.hash);
    s.commit(tx).unwrap_or_else(|r| panic!("{r}")).number
}

fn write(file: &str, text: String) -> m2_song::tx::Op {
    m2_song::tx::Op::Write { file: m2_song::tx::TxPath::new(file).unwrap(), bytes: m2_song::tx::Payload::Text(text.into()) }
}

/// The module hash the plan of `rev` built from `packages` gives kernel `id`.
fn planned_module(rev: &crate::Revision, packages: &PackageSet, id: &str) -> Option<m2_song::Sha> {
    let r = Renderer::new(&rev.compiled.flat, &rev.compiled.data, packages, 48_000).unwrap_or_else(|e| panic!("{e:#}"));
    r.info().shapes.iter().flatten().find_map(|s| match s {
        m2_runtime::Shape::Kernel { id: k, wasm, .. } if k == id => Some(*wasm),
        _ => None,
    })
}

// PACK-9: a dry run that moves the song to another version, then an unrelated edit, leave the
// accepted revision listing and playing the version it was built with; committing the move
// moves it.
#[test]
fn a_dry_run_never_moves_what_the_accepted_revision_plays() {
    let root = scratch("view");
    copy(&fixtures(), &root);
    let store = root.join("Packs");
    let (h1, m1) = install_tiny_with_kernel(&store, "0.1.0", "one");
    let (h2, m2) = install_tiny_with_kernel(&store, "0.2.0", "two");
    assert_ne!(m1, m2);
    let song = root.join("song-store");
    let tiny = PackName::new("@test/tiny").unwrap();
    std::fs::write(song.join(m2_song::path::LOCK), Lock::default().with(tiny.clone(), Version(0, 1, 0), h1).canonical()).unwrap();
    std::fs::write(
        song.join("tracks/tone.ts"),
        "import { track } from \"@m2/core\";\nimport { tinyTone } from \"@test/tiny\";\n\nexport default track(\"tone\", { name: \"Tone\" }, ({ lane }) => tinyTone(lane.notes(\"notes\")));\n",
    )
    .unwrap();
    let main = std::fs::read_to_string(song.join("song.ts")).unwrap();
    let main = main
        .replace("import pluck", "import tone from \"./tracks/tone\";\nimport pluck")
        .replace("pluck: pluck.out", "pluck: pluck.out, tone: tone.out");
    std::fs::write(song.join("song.ts"), main).unwrap();
    let mut s = crate::Session::open(&song, packages(&store)).unwrap_or_else(|d| panic!("{d:?}"));
    assert!(s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    let sine = "@test/tiny:sine";
    let version = |s: &crate::Session| s.packages.get("@test/tiny").map(|p| p.version.clone());
    assert_eq!((version(&s), s.packages.module_sha(sine)), (Some("0.1.0".into()), Some(m1)));

    // A dry run of the move: it builds, and writes nothing.
    let manifest = std::fs::read_to_string(song.join("music2.toml")).unwrap();
    let lock2 = Lock::default().with(tiny.clone(), Version(0, 2, 0), h2).canonical();
    let moving = || based(&song, vec![write("music2.toml", manifest.replace("\"0.1\"", "\"0.2\"")), write("music2.lock", lock2.clone())]);
    s.check(&moving()).unwrap_or_else(|r| panic!("{r}"));
    // An unrelated edit, committed: one note louder.
    let notes = std::fs::read_to_string(song.join("data/keys/notes.json")).unwrap();
    commit_seen(&mut s, based(&song, vec![write("data/keys/notes.json", notes.replacen("\"velocity\": 0.8", "\"velocity\": 0.9", 1))]));
    assert!(std::fs::read_to_string(song.join("music2.toml")).unwrap().contains("\"0.1\""));
    assert_eq!(version(&s), Some("0.1.0".into()), "the lists name the version the accepted revision uses");
    assert_eq!(s.packages.module_sha(sine), Some(m1));
    let accepted = s.accepted_arc();
    assert_eq!(planned_module(&accepted, &s.packages, sine), Some(m1), "a plan of the accepted revision runs its own version's kernel");

    // Committed, the move moves what plays.
    commit_seen(&mut s, moving());
    assert_eq!((version(&s), s.packages.module_sha(sine)), (Some("0.2.0".into()), Some(m2)));
    assert_eq!(planned_module(&s.accepted_arc(), &s.packages, sine), Some(m2));
    std::fs::remove_dir_all(&root).ok();
}

/// The song-path fixture, taking `@test/tiny` from `pack` (a folder beside it), with the kernel
/// pack's instrument playing too.
fn song_from_folder(root: &Path, pack: &str) -> PathBuf {
    let song = root.join("song-path");
    let manifest = std::fs::read_to_string(song.join("music2.toml")).unwrap();
    let at = manifest.find("\"@test/tiny\"").unwrap();
    std::fs::write(song.join("music2.toml"), format!("{}\"@test/tiny\" = {{ path = \"../{pack}\" }}\n", &manifest[..at])).unwrap();
    std::fs::write(
        song.join("tracks/tone.ts"),
        "import { track } from \"@m2/core\";\nimport { tinyTone } from \"@test/tiny\";\n\nexport default track(\"tone\", { name: \"Tone\" }, ({ lane }) => tinyTone(lane.notes(\"notes\")));\n",
    )
    .unwrap();
    let main = std::fs::read_to_string(song.join("song.ts")).unwrap();
    if !main.contains("tone") {
        let main = main
            .replace("import pluck", "import tone from \"./tracks/tone\";\nimport pluck")
            .replace("pluck: pluck.out", "pluck: pluck.out, tone: tone.out");
        std::fs::write(song.join("song.ts"), main).unwrap();
    }
    song
}

/// `@test/tiny` with its kernel, in the folder `dir` (not the store): as its author has it.
fn tiny_with_kernel_in(root: &Path, dir: &str, tag: &str) -> m2_song::Sha {
    let store = root.join(format!("make-{dir}"));
    install_tiny_with_kernel(&store, "0.1.0", tag);
    let made = Store::at(store.clone()).version_dir(&PackName::new("@test/tiny").unwrap(), &Version(0, 1, 0));
    let _ = std::fs::remove_dir_all(root.join(dir));
    copy(&made, &root.join(dir));
    std::fs::remove_dir_all(&store).unwrap();
    m2_song::Sha::of(&std::fs::read(root.join(dir).join("k.wasm")).unwrap())
}

// PACK-9: a path pack's rebuilt wasm (and m2pkg.toml) reaches the song's next build, as its
// TypeScript does: a loaded version is known by the hashes of both.
#[test]
fn a_path_packs_new_wasm_reaches_the_next_build() {
    let root = scratch("rebuilt");
    copy(&fixtures(), &root);
    let m1 = tiny_with_kernel_in(&root, "mine", "one");
    let song = song_from_folder(&root, "mine");
    let mut pk = packages(&root.join("no-store"));
    let c = compile(&Project::open(&song).unwrap(), &mut pk).unwrap_or_else(|d| panic!("{d:?}"));
    assert_eq!(pk.module_sha("@test/tiny:sine"), Some(m1));
    drop(c);
    let m2 = tiny_with_kernel_in(&root, "mine", "two");
    assert_ne!(m1, m2);
    let c = compile(&Project::open(&song).unwrap(), &mut pk).unwrap_or_else(|d| panic!("{d:?}"));
    assert_eq!(pk.module_sha("@test/tiny:sine"), Some(m2), "the rebuilt module plays");
    let out = Renderer::new(&c.flat, &c.data, &pk, 48_000).unwrap().render(0.5).unwrap();
    assert!(out.iter().any(|f| f[0].abs() > 0.01), "the song sounds");
    std::fs::remove_dir_all(&root).ok();
}

// PACK-9: a pack that doesn't load is pkg.broken, never a panic, even when what loaded before
// is gone from the disk since.
#[test]
fn a_pack_that_fails_to_load_never_panics_the_build() {
    let root = scratch("gone");
    copy(&fixtures(), &root);
    tiny_with_kernel_in(&root, "first", "one");
    let song = song_from_folder(&root, "first");
    let mut pk = packages(&root.join("no-store"));
    compile(&Project::open(&song).unwrap(), &mut pk).unwrap_or_else(|d| panic!("{d:?}"));
    // Its author moves to another folder, whose wasm is broken, and the first is gone.
    tiny_with_kernel_in(&root, "second", "two");
    std::fs::write(root.join("second/k.wasm"), b"not wasm").unwrap();
    std::fs::remove_dir_all(root.join("first")).unwrap();
    let song = song_from_folder(&root, "second");
    let e = compile(&Project::open(&song).unwrap(), &mut pk).err().unwrap();
    assert_eq!(codes(&e), ["pkg.broken"]);
    std::fs::remove_dir_all(&root).ok();
}

/// `@test/tiny` in `root` with `instruments/tiny-piano.toml` (PACK-13), and the song from its
/// folder playing it on its pluck track.
fn tiny_with_data(root: &Path, toml: &str) -> PathBuf {
    copy(&fixtures(), root);
    std::fs::create_dir_all(root.join("tiny/instruments")).unwrap();
    std::fs::write(root.join("tiny/instruments/tiny-piano.toml"), toml).unwrap();
    let pluck = "import { track } from \"@m2/core\";\nimport { tinyPiano } from \"@test/tiny/instruments\";\n\n\
                 export default track(\"pluck\", { name: \"Pluck\" }, ({ lane }) => tinyPiano(lane.notes(\"notes\")));\n";
    std::fs::write(root.join("song-path/tracks/pluck.ts"), pluck).unwrap();
    root.join("song-path")
}

const TINY_PIANO: &str = "# Two layers of one pluck.\nlabel = \"Tiny piano\"\ncategory = \"Keys\"\ndescription = \"A pluck in two layers\"\n\
                          engine = \"std:sampler\"\nmode = \"gate\"\nrelease = 0.2\nvoices = 4\n\n\
                          [[region]]\nfile = \"samples/pluck.wav\"\nroot = 48\nkeys = [0, 59]\n\n\
                          [[region]]\nfile = \"samples/pluck.wav\"\nroot = 60\nkeys = [60, 127]\nvolume = -6\n";

// PACK-13: an instrument written as data is a component of its pack, as if written in TypeScript:
// its id, its descriptor with the file's defaults, exported from "<package>/instruments" by its id
// in camelCase, and listed with the pack's other components. The build records the file read, and
// it sounds.
#[test]
fn a_data_instrument_is_a_component_of_its_pack() {
    let root = scratch("data");
    let song = tiny_with_data(&root, TINY_PIANO);
    let mut pk = packages(&root.join("no-store"));
    let c = compile(&Project::open(&song).unwrap(), &mut pk).unwrap_or_else(|d| panic!("{d:?}"));
    assert!(c.problems.is_empty(), "{:?}", c.problems);
    let components: Vec<&str> = c.ir.modules.iter().flat_map(|m| m.frames.iter().filter_map(|f| f.component.as_deref())).collect();
    assert!(components.contains(&"@test/tiny:tiny-piano"), "{components:?}");
    let toml = std::fs::read(root.join("tiny/instruments/tiny-piano.toml")).unwrap();
    let read = c.package_files.iter().find(|f| f.path == "@test/tiny/instruments/tiny-piano.toml").expect("the file is recorded");
    assert_eq!(format!("sha256:{}", read.sha256), m2_song::Sha::of(&toml).to_string());
    let out = Renderer::new(&c.flat, &c.data, &pk, 48_000).unwrap().render(2.0).unwrap();
    let peak =
        |from: f64, to: f64| out[(from * 48_000.0) as usize..(to * 48_000.0) as usize].iter().map(|f| f[0].abs()).fold(0.0, f32::max);
    assert!(peak(0.25, 0.45) > 0.01, "the data instrument sounds");

    let std = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std");
    let catalog = describe(&[("std".into(), std), ("@test/tiny".into(), root.join("tiny"))]);
    assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);
    let d = &catalog.components["@test/tiny:tiny-piano"];
    assert!(d.is_instrument());
    assert_eq!(
        (d.label.as_str(), d.editor.category.as_deref(), d.editor.description.as_str()),
        ("Tiny piano", Some("Keys"), "A pluck in two layers")
    );
    assert_eq!(d.module, "@test/tiny/instruments");
    assert_eq!(d.at, "@test/tiny/instruments/tiny-piano.toml:1:1", "defined at its file");
    assert_eq!(d.control("release").and_then(|c| c.default.as_ref()).map(|v| v.value), Some(0.2));
    assert_eq!(d.control("level").and_then(|c| c.default.as_ref()).map(|v| v.value), Some(0.0), "left out, the Sampler's");
    let exports: Vec<(&str, &str)> =
        catalog.exports["@test/tiny:tiny-piano"].iter().map(|e| (e.module.as_str(), e.name.as_str())).collect();
    assert_eq!(exports, [("@test/tiny/instruments", "tinyPiano")]);
    assert!(catalog.components.contains_key("@test/tiny:tiny-keys"), "listed with the pack's coded ones");
    std::fs::remove_dir_all(&root).ok();
}

// PACK-13, PKG-15: a mistake in a data instrument names its file and line: one the reader finds,
// and one @m2/core finds in a region's numbers, at that region's line.
#[test]
fn a_data_instruments_region_error_names_its_line() {
    let root = scratch("data-err");
    let song = tiny_with_data(&root, &TINY_PIANO.replace("keys = [60, 127]", "keys = [127, 60]"));
    let mut pk = packages(&root.join("no-store"));
    let Err(e) = compile(&Project::open(&song).unwrap(), &mut pk) else { panic!("refused") };
    assert_eq!(e[0].src.as_deref(), Some("@test/tiny/instruments/tiny-piano.toml:15:1"), "{e:?}");
    assert!(e[0].message.contains("region 2: keys 127 to 60: the lowest is above the highest"), "{}", e[0].message);

    std::fs::write(root.join("tiny/instruments/tiny-piano.toml"), TINY_PIANO.replace("root = 48", "roots = 48")).unwrap();
    let Err(e) = compile(&Project::open(&song).unwrap(), &mut pk) else { panic!("refused") };
    assert_eq!(e[0].src.as_deref(), Some("@test/tiny/instruments/tiny-piano.toml:12:1"), "{e:?}");
    assert!(e[0].message.contains("`roots` in region 1: no such key; did you mean `root`?"), "{}", e[0].message);
    std::fs::remove_dir_all(&root).ok();
}

// PACK-13: a data file's name is the instrument's id, checked before any code is made from it: a
// name that is code, or a word JavaScript keeps for itself, is refused at its file, and never runs.
#[test]
fn a_data_file_named_as_code_is_refused_at_its_file() {
    let root = scratch("data-name");
    let song = tiny_with_data(&root, TINY_PIANO);
    let std = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std");
    let evil = "a=(()=>{throw new Error(\"INJECTED CODE RAN\")})(),b.toml";
    for name in [evil, "new.toml", "static.toml", "default.toml", "Felt Piano.toml"] {
        let file = root.join("tiny/instruments").join(name);
        std::fs::write(&file, TINY_PIANO).unwrap();
        let mut pk = packages(&root.join("no-store"));
        let Err(e) = compile(&Project::open(&song).unwrap(), &mut pk) else { panic!("{name} is refused") };
        assert!(e.iter().all(|d| !d.message.contains("INJECTED")), "the name never runs: {e:?}");
        assert_eq!(e[0].src.as_deref(), Some(format!("@test/tiny/instruments/{name}:1:1").as_str()), "{e:?}");
        assert!(e[0].message.contains("the file's name is the instrument's id"), "{name}: {}", e[0].message);
        let catalog = describe(&[("std".into(), std.clone()), ("@test/tiny".into(), root.join("tiny"))]);
        assert!(catalog.problems.iter().all(|d| !d.message.contains("INJECTED")), "{:?}", catalog.problems);
        let at = format!("@test/tiny/instruments/{name}:1:1");
        assert!(
            catalog
                .problems
                .iter()
                .any(|d| d.src.as_deref() == Some(at.as_str()) && d.message.contains("the file's name is the instrument's id")),
            "{name}: {:?}",
            catalog.problems
        );
        std::fs::remove_file(&file).unwrap();
    }
    std::fs::remove_dir_all(&root).ok();
}

// PACK-13: two files whose names make one name in code are both refused, naming both, rather than
// one import failing as "ambiguous" in a song.
#[test]
fn two_files_with_one_name_in_code_are_both_refused() {
    let root = scratch("data-twins");
    let song = tiny_with_data(&root, TINY_PIANO);
    for (a, b, both) in [("a-1", "a1", "a1"), ("a--b", "a-b", "aB")] {
        std::fs::write(root.join(format!("tiny/instruments/{a}.toml")), TINY_PIANO).unwrap();
        std::fs::write(root.join(format!("tiny/instruments/{b}.toml")), TINY_PIANO).unwrap();
        let mut pk = packages(&root.join("no-store"));
        let Err(e) = compile(&Project::open(&song).unwrap(), &mut pk) else { panic!("{a} and {b} are refused") };
        let m = &e[0].message;
        assert!(m.contains(&format!("{a}.toml")) && m.contains(&format!("{b}.toml")) && m.contains(&format!("`{both}`")), "{m}");
        std::fs::remove_file(root.join(format!("tiny/instruments/{a}.toml"))).unwrap();
        std::fs::remove_file(root.join(format!("tiny/instruments/{b}.toml"))).unwrap();
    }
    std::fs::remove_dir_all(&root).ok();
}

// PACK-13: an instrument may be named as the module made from it names its own imports and values
// (`sampler`, `regions`, `component` …): those are names no instrument can have. Every file of
// the folder is loaded with the module `<package>/instruments`, so each is built here.
#[test]
fn the_modules_own_names_are_no_instruments() {
    let root = scratch("data-own-names");
    let song = tiny_with_data(&root, TINY_PIANO);
    for id in ["sampler", "regions", "component", "control", "signature", "notes", "p"] {
        std::fs::write(root.join(format!("tiny/instruments/{id}.toml")), TINY_PIANO).unwrap();
    }
    let pluck = "import { track } from \"@m2/core\";\nimport { sampler } from \"@test/tiny/instruments\";\n\n\
                 export default track(\"pluck\", { name: \"Pluck\" }, ({ lane }) => sampler(lane.notes(\"notes\")));\n";
    std::fs::write(root.join("song-path/tracks/pluck.ts"), pluck).unwrap();
    let mut pk = packages(&root.join("no-store"));
    let c = compile(&Project::open(&song).unwrap(), &mut pk).unwrap_or_else(|d| panic!("{d:?}"));
    assert!(c.problems.is_empty(), "{:?}", c.problems);
    let out = Renderer::new(&c.flat, &c.data, &pk, 48_000).unwrap().render(2.0).unwrap();
    let peak = out[(0.25 * 48_000.0) as usize..(0.45 * 48_000.0) as usize].iter().map(|f| f[0].abs()).fold(0.0, f32::max);
    assert!(peak > 0.01, "`sampler`, the data instrument, sounds");
    std::fs::remove_dir_all(&root).ok();
}

// PACK-13: a data instrument's controls and settings are std's Sampler's, so one that gives no
// defaults is the Sampler with its regions.
#[test]
fn sampler_controls_are_stds() {
    let std = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std");
    let catalog = describe(&[("std".into(), std)]);
    let sampler = &catalog.components["std:sampler"];
    for (name, _, default, (lo, hi)) in m2_song::instrument::CONTROLS {
        let c = sampler.control(name).unwrap_or_else(|| panic!("std's sampler has {name}"));
        assert_eq!((c.default.as_ref().map(|v| v.value), c.range), (Some(*default), Some((*lo, *hi))), "{name}");
    }
    let setting = |n: &str| sampler.settings.iter().find(|s| s.name == n).unwrap_or_else(|| panic!("std's sampler has {n}")).clone();
    assert_eq!(setting("voices").default, Some(serde_json::json!(m2_song::instrument::VOICES as i64)));
    assert_eq!(setting("mode").default, Some(serde_json::json!(m2_song::instrument::MODE)));
    assert_eq!(setting("mode").options, Some(m2_song::instrument::MODES.iter().map(|m| m.to_string()).collect()));
    assert_eq!(setting("turns").default, Some(serde_json::json!(m2_song::instrument::TURN)));
    assert_eq!(setting("turns").options, Some(m2_song::instrument::TURNS.iter().map(|m| m.to_string()).collect()));
}

// PACK-11, REG-15: a pack installed while a song is open (a Get) is offered once the session is
// told the store changed, at once, with no edit of the song.
#[test]
fn the_lists_offer_a_pack_installed_once_the_store_changed() {
    let root = scratch("store-changed");
    copy(&fixtures(), &root);
    let store = root.join("Packs");
    let song = root.join("song-path");
    let manifest = std::fs::read_to_string(song.join("music2.toml")).unwrap();
    let plain: String = manifest.lines().filter(|l| !l.contains("@test/tiny")).map(|l| format!("{l}\n")).collect();
    std::fs::write(song.join("music2.toml"), plain).unwrap();
    std::fs::write(
        song.join("song.ts"),
        "import { hz, song } from '@m2/core';\nimport { osc } from 'std/dsp';\nexport default song(() => osc.sine(hz(800)));\n",
    )
    .unwrap();
    let _ = std::fs::remove_dir_all(song.join("tracks"));
    let mut s = crate::Session::open(&song, packages(&store)).unwrap_or_else(|d| panic!("{d:?}"));
    let offered = |s: &crate::Session| s.offered().iter().any(|(n, ..)| n == "@test/tiny");
    assert!(!offered(&s));
    install_tiny(&store);
    assert!(!offered(&s), "the lists are made as the song opens, and when its packs change");
    let taken = s.store_changed();
    assert!(offered(&s), "told the store changed, they offer it");
    assert!(s.catalog().components.contains_key("@test/tiny:tiny-keys"));
    assert!(taken.accepted.is_none(), "a song that built isn't built again");
    std::fs::remove_dir_all(&root).ok();
}

// PACK-9, REG-17: a song whose lock names a version that isn't installed waits (pkg.missing);
// once it is installed and the session told, the song is built again and plays.
#[test]
fn a_song_missing_a_locked_pack_builds_once_it_is_installed() {
    let root = scratch("missing-locked");
    copy(&fixtures(), &root);
    let store = root.join("Packs");
    let song = root.join("song-store");
    let mut s = crate::Session::open(&song, packages(&store)).unwrap_or_else(|d| panic!("{d:?}"));
    assert!(!s.accepted().has_code());
    assert_eq!(codes(&s.draft_errors), ["pkg.missing"]);
    install_tiny(&store);
    let taken = s.store_changed();
    assert!(taken.accepted.is_some() && s.accepted().has_code(), "{:?}", s.draft_errors);
    assert!(s.draft_errors.is_empty());
    assert_eq!(s.accepted().compiled.packs.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["@test/tiny"]);
    std::fs::remove_dir_all(&root).ok();
}

// AGENT-2, REG-17: an outside write that waited for a pack (pkg.missing), built once a Get
// installs it, keeps its one undo step, source outside, as when its own batch built.
#[test]
fn an_outside_edit_that_waited_for_a_pack_keeps_its_undo_step() {
    let root = scratch("rv-outside");
    copy(&fixtures(), &root);
    let store = root.join("Packs");
    let song = root.join("song-store");
    let rd = |p: &str| std::fs::read_to_string(song.join(p)).unwrap();
    let (manifest, lock, code) = (rd("music2.toml"), rd("music2.lock"), rd("song.ts"));
    let plain: String = manifest.lines().filter(|l| !l.contains("@test/tiny")).map(|l| format!("{l}\n")).collect();
    std::fs::write(song.join("music2.toml"), &plain).unwrap();
    std::fs::remove_file(song.join("music2.lock")).unwrap();
    std::fs::write(
        song.join("song.ts"),
        "import { hz, song } from '@m2/core';\nimport { osc } from 'std/dsp';\nexport default song(() => osc.sine(hz(800)));\n",
    )
    .unwrap();
    std::fs::rename(song.join("tracks"), root.join("tracks-aside")).unwrap();
    let mut s = crate::Session::open(&song, packages(&store)).unwrap_or_else(|d| panic!("{d:?}"));
    assert!(s.accepted().has_code());
    // An agent writes the pack in: its dependency, its lock, and code that plays it.
    std::fs::write(song.join("music2.toml"), &manifest).unwrap();
    std::fs::write(song.join("music2.lock"), &lock).unwrap();
    std::fs::write(song.join("song.ts"), &code).unwrap();
    std::fs::rename(root.join("tracks-aside"), song.join("tracks")).unwrap();
    let paths: std::collections::BTreeSet<PathBuf> =
        ["music2.toml", "music2.lock", "song.ts", "tracks/keys.ts", "tracks/pluck.ts"].iter().map(PathBuf::from).collect();
    let (_, out) = s.take_in_batch(m2_song::Snapshot::read(&song).unwrap(), &paths, false);
    assert_eq!(codes(&s.draft_errors), ["pkg.missing"], "{out:?}");
    // Then a Get installs the pack, and the session is told.
    install_tiny(&store);
    let taken = s.store_changed();
    assert!(taken.accepted.is_some(), "{:?}", s.draft_errors);
    let top = s.step_names().0;
    assert!(
        top.as_ref().is_some_and(|n| n.source == crate::Source::Outside),
        "the agent's edit, built once the pack came, has no undo step: {:?}",
        top.map(|n| (n.id, n.source, n.files))
    );
    // One undo takes the agent's whole edit back.
    s.undo().unwrap();
    assert_eq!(std::fs::read_to_string(song.join("music2.toml")).unwrap(), plain);
    std::fs::remove_dir_all(&root).ok();
}

// AGENT-2: the same outside write, with the pack installed first: built by its own batch, one
// undo step, source outside.
#[test]
fn an_outside_edit_with_its_pack_there_is_one_undo_step() {
    let root = scratch("rv-control");
    copy(&fixtures(), &root);
    let store = root.join("Packs");
    let song = root.join("song-store");
    let rd = |p: &str| std::fs::read_to_string(song.join(p)).unwrap();
    let (manifest, lock, code) = (rd("music2.toml"), rd("music2.lock"), rd("song.ts"));
    let plain: String = manifest.lines().filter(|l| !l.contains("@test/tiny")).map(|l| format!("{l}\n")).collect();
    std::fs::write(song.join("music2.toml"), &plain).unwrap();
    std::fs::remove_file(song.join("music2.lock")).unwrap();
    std::fs::write(
        song.join("song.ts"),
        "import { hz, song } from '@m2/core';\nimport { osc } from 'std/dsp';\nexport default song(() => osc.sine(hz(800)));\n",
    )
    .unwrap();
    std::fs::rename(song.join("tracks"), root.join("tracks-aside")).unwrap();
    install_tiny(&store);
    let mut s = crate::Session::open(&song, packages(&store)).unwrap_or_else(|d| panic!("{d:?}"));
    assert!(s.accepted().has_code());
    // An agent writes the pack in: its dependency, its lock, and code that plays it.
    std::fs::write(song.join("music2.toml"), &manifest).unwrap();
    std::fs::write(song.join("music2.lock"), &lock).unwrap();
    std::fs::write(song.join("song.ts"), &code).unwrap();
    std::fs::rename(root.join("tracks-aside"), song.join("tracks")).unwrap();
    let paths: std::collections::BTreeSet<PathBuf> =
        ["music2.toml", "music2.lock", "song.ts", "tracks/keys.ts", "tracks/pluck.ts"].iter().map(PathBuf::from).collect();
    let (_, out) = s.take_in_batch(m2_song::Snapshot::read(&song).unwrap(), &paths, false);
    assert!(matches!(out, crate::take::Outside::Step(_)), "{out:?} {:?}", s.draft_errors);
    let top = s.step_names().0;
    assert!(
        top.as_ref().is_some_and(|n| n.source == crate::Source::Outside),
        "the agent's edit, built once the pack came, has no undo step: {:?}",
        top.map(|n| (n.id, n.source, n.files))
    );
    std::fs::remove_dir_all(&root).ok();
}

// CARD-10, CARD-11, CARD-12: a choice a song writes by its name reaches a pack's kernel as the
// option's place, and a switch as 1 or 0: heard end to end, through `@test/kinds`, whose kernels
// play what they read (`tests/fixtures/packs/kinds`).
#[test]
fn a_choice_in_a_pack_plays_its_place() {
    let root = scratch("kinds");
    copy(&fixtures().join("kinds"), &root.join("kinds"));
    let song = root.join("song");
    std::fs::create_dir_all(&song).unwrap();
    let manifest = "[project]\nname = \"Kinds\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.7\"\n";
    std::fs::write(song.join("music2.toml"), format!("{manifest}\"@test/kinds\" = {{ path = \"../kinds\" }}\n")).unwrap();
    let mut pk = packages(&root.join("no-store"));
    for (call, heard) in [
        ("shape({ shape: \"sine\" })", 2.0),
        ("shape({ shape: \"square\" })", 1.0),
        ("shape()", 0.0),
        ("sub({ on: true })", 1.0),
        ("sub()", 0.0),
    ] {
        let code = format!(
            "import {{ song }} from \"@m2/core\";\nimport {{ shape, sub }} from \"@test/kinds\";\nexport default song(() => {call});\n"
        );
        std::fs::write(song.join("song.ts"), code).unwrap();
        let c = compile(&Project::open(&song).unwrap(), &mut pk).unwrap_or_else(|d| panic!("{call}: {d:?}"));
        let out = Renderer::new(&c.flat, &c.data, &pk, 48_000).unwrap().render(0.05).unwrap();
        assert!(out.iter().all(|f| f[0] == heard as f32), "{call}: {:?}", &out[..2]);
    }
    std::fs::remove_dir_all(&root).ok();
}
