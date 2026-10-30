use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use m2_song::Sha;
use m2_song::pack::{Listing, PackName, Store};
use m2_song::registry::{Catalog, Facts, ItemKind, POINTER, Pointer, Registry, blob_path, catalog_path, facts_path};

use super::*;

fn std_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")
}

/// A scratch folder, removed when dropped (read-only files too: an installed pack's).
struct Temp(PathBuf);

impl Temp {
    fn new(tag: &str) -> Temp {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let d = std::env::temp_dir().join(format!("m2-registry-{tag}-{}-{n}", std::process::id()));
        let t = Temp(d);
        t.clean();
        std::fs::create_dir_all(&t.0).unwrap();
        t
    }

    fn clean(&self) {
        for p in crate::testing::files_under(&self.0).into_iter().chain([self.0.clone()]) {
            for q in p.ancestors().take_while(|a| a.starts_with(&self.0)) {
                if let Ok(m) = std::fs::metadata(q) {
                    let mut perm = m.permissions();
                    #[allow(clippy::permissions_set_readonly_false)]
                    perm.set_readonly(false);
                    let _ = std::fs::set_permissions(q, perm);
                }
            }
        }
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        self.clean();
    }
}

fn write(path: &Path, text: impl AsRef<[u8]>) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// A 16-bit mono WAV of a tenth of a second of a tone at 48 kHz: the same bytes every time.
fn tone(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let spec = hound::WavSpec { channels: 1, sample_rate: 48_000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for i in 0..4_800 {
        w.write_sample(((i as f32 / 10.0).sin() * 8_000.0 * (1.0 - i as f32 / 4_800.0)) as i16).unwrap();
    }
    w.finalize().unwrap();
}

const KEYS: &str = r#"import { component, control, port, signature, type Audio, type Events, type Params } from "@m2/core";
import { synth } from "std/synth";

/** A soft pluck: std's synth pluck, darker. */
export const softKeys = component(
  {
    id: "soft-keys",
    label: "Soft keys",
    signature: signature<(notes: Events) => Audio>(),
    inputs: { notes: port.notes() },
    controls: { brightness: control.hz(1800).range(200, 20000) },
    output: port.audio(),
    editor: { category: "Keys", description: "A soft pluck, from std's synth" },
  },
  (notes: Events, { brightness }: Params): Audio => synth.pluck(notes, { brightness }),
);
"#;

/// A made-up registry and the unpacked sources of its two packs: `@alice/drums` 1.0.0, one
/// sample that `samples.toml` names, and `@bob/keys` 0.1.0, one instrument and one sound of it.
struct World {
    t: Temp,
    registry: PathBuf,
    sources: PathBuf,
}

impl World {
    fn new(tag: &str) -> World {
        let t = Temp::new(tag);
        let (registry, sources) = (t.0.join("registry"), t.0.join("sources"));
        let drums = sources.join("drums-1.0.0");
        write(
            &drums.join("m2pkg.toml"),
            "[package]\nname = \"@alice/drums\"\nversion = \"1.0.0\"\nlicense = \"CC0-1.0\"\ndescription = \"One kick\"\n\
             authors = [\"Alice\"]\nrepository = \"https://github.com/alice/drums\"\ntags = [\"drums\"]\n",
        );
        write(&drums.join("samples.toml"), "[folder.\"kicks\"]\ninstrument = \"kick\"\n");
        tone(&drums.join("samples/kicks/Boom.wav"));
        let keys = sources.join("keys/keys-0.1.0");
        write(
            &keys.join("m2pkg.toml"),
            "[package]\nname = \"@bob/keys\"\nversion = \"0.1.0\"\nlicense = \"MIT\"\ndescription = \"Soft keys\"\nauthors = [\"Bob\"]\n\
             repository = \"https://github.com/bob/keys\"\ntags = [\"keys\"]\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.5\"\n",
        );
        write(&keys.join("src/index.ts"), KEYS);
        write(
            &keys.join("presets/soft-keys/Dark.json"),
            "{\n  \"m2preset\": 1,\n  \"name\": \"Dark\",\n  \"sound\": \"@bob/keys:soft-keys\",\n  \"values\": {\n    \"brightness\": 600\n  }\n}\n",
        );
        write(&registry.join("tags.txt"), "# the words a pack's tags may use\ndrums\nkeys\n");
        write(&registry.join("packs/.gitkeep"), "");
        let w = World { t, registry, sources };
        w.list("@alice/drums", "1.0.0", &w.drums(), "");
        w.list("@bob/keys", "0.1.0", &w.keys(), "");
        w
    }

    fn drums(&self) -> PathBuf {
        self.sources.join("drums-1.0.0")
    }

    fn keys(&self) -> PathBuf {
        self.sources.join("keys/keys-0.1.0")
    }

    /// Write `name`'s pack file with one version, its hash the listing of `dir` now, and `more`
    /// lines in its table.
    fn list(&self, name: &str, version: &str, dir: &Path, more: &str) {
        let n = PackName::new(name).unwrap();
        let owner = m2_song::registry::github_owner(&n);
        let short = name.rsplit('/').next().unwrap();
        let text = format!(
            "name = \"{name}\"\nrepository = \"https://github.com/{owner}/{short}\"\n\n[[versions]]\nversion = \"{version}\"\nhash = \"{}\"\n\
             source = \"https://github.com/{owner}/{short}/releases/download/v{version}/{short}-{version}.tar.gz\"\n{more}",
            Listing::of(dir).unwrap().hash().prefixed()
        );
        write(&self.registry.join(m2_song::registry::PackFile::path_of(&n)), text);
    }

    fn build(&self, out: &Path, from: Option<&Bucket>) -> Result<Built, Vec<String>> {
        let work = self.t.0.join("work");
        build(&Build {
            registry: &self.registry,
            sources: &self.sources,
            out,
            from,
            made: "2026-10-30T12:00:00Z",
            std: &std_dir(),
            work: &work,
            to_delete: None,
        })
    }

    fn registry(&self) -> Registry {
        Registry::read(&self.registry).unwrap()
    }
}

/// Every file under `dir`, by its path there.
fn tree(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    crate::testing::files_under(dir)
        .into_iter()
        .map(|p| (p.strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/"), std::fs::read(&p).unwrap()))
        .collect()
}

fn name(s: &str) -> PackName {
    PackName::new(s).unwrap()
}

/// The catalog `out`'s catalog.json names, checked.
fn catalog_in(out: &BTreeMap<String, Vec<u8>>) -> Catalog {
    let pointer = Pointer::read(&out[POINTER]).unwrap();
    Catalog::read_checked(&out[&catalog_path(&pointer.catalog)], &pointer).unwrap()
}

// REG-3, REG-4, REG-5: a registry builds into a bucket of plain files, each blob named by its own
// hash; the catalog lists both packs, a sample with its file and its preview (the file), an
// instrument and its sound each with a rendered preview. Built again, into a new folder, it is
// the same bytes; into the same folder, nothing is built twice. The instrument's preview is the
// bits the app's ⌘F makes from the installed pack.
#[test]
fn a_registry_builds_into_a_bucket_and_again_to_the_same_bytes() {
    let w = World::new("same");
    let out = w.t.0.join("bucket");
    let built = w.build(&out, None).unwrap_or_else(|e| panic!("{}", e.join("\n")));
    assert_eq!(built.built.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["@alice/drums", "@bob/keys"]);
    let first = tree(&out);
    for (path, bytes) in &first {
        if let Some(hex) = path.strip_prefix("blobs/sha256/") {
            assert_eq!(Sha::of(bytes).hex(), hex, "{path} is named by its hash");
        } else {
            assert!(path == POINTER || path.starts_with("versions/") || path.starts_with("catalog/"), "{path} is no part of a bucket");
        }
    }
    let reg = w.registry();
    for p in &reg.packs {
        let v = &p.versions[0];
        let listing = &first[&blob_path(&v.hash)];
        assert_eq!(Sha::of(listing), v.hash, "{}'s m2pack.json is in the bucket by the version's hash", p.name);
        let f = Facts::read_for(&first[&facts_path(&v.hash)], &p.name, v.version, v.hash).unwrap();
        assert!(Facts::read_for(&first[&facts_path(&v.hash)], &p.name, Version(9, 9, 9), v.hash).is_err(), "another version's facts");
        for b in &f.blobs {
            assert!(first.contains_key(&blob_path(b)), "{} {}: its blob {b} is in the bucket", p.name, v.version);
        }
    }

    let catalog = catalog_in(&first);
    assert_eq!(catalog.made, "2026-10-30T12:00:00Z");
    let drums = catalog.get(&name("@alice/drums")).unwrap();
    assert_eq!(
        (drums.latest.map(|v| v.to_string()), drums.license.as_str(), drums.tags.clone()),
        (Some("1.0.0".into()), "CC0-1.0", vec!["drums".to_string()])
    );
    let [kick] = drums.items.as_slice() else { panic!("one item: {:?}", drums.items) };
    assert_eq!(
        (kick.kind, kick.id.as_str(), kick.component.as_str()),
        (ItemKind::Sample, "sample:@alice/drums:samples/kicks/Boom.wav", "@alice/drums:samples/kicks/Boom.wav")
    );
    let file = kick.file.as_ref().expect("a sample's own file");
    assert_eq!(
        (file.path.as_str(), file.sha256),
        ("samples/kicks/Boom.wav", Sha::of(&std::fs::read(w.drums().join("samples/kicks/Boom.wav")).unwrap()))
    );
    assert_eq!(kick.preview, Some(file.sha256), "a sample's preview is its own file (LIB-6)");
    let s = kick.sample.as_ref().unwrap();
    assert_eq!((s.instrument.as_deref(), s.channels, s.rate), (Some("kick"), 1, 48_000));
    assert!((s.seconds - 0.1).abs() < 1e-9, "{}", s.seconds);

    let keys = catalog.get(&name("@bob/keys")).unwrap();
    let ids: Vec<(&str, ItemKind)> = keys.items.iter().map(|i| (i.id.as_str(), i.kind)).collect();
    assert_eq!(ids, [("@bob/keys:soft-keys", ItemKind::Instrument), ("sound:std/@bob/keys:soft-keys/Dark", ItemKind::Sound)]);
    let mut previews = vec![];
    for i in &keys.items {
        let p = i.preview.unwrap_or_else(|| panic!("{} has a preview: {:?}", i.id, i.no_preview));
        let wav = hound::WavReader::new(std::io::Cursor::new(&first[&blob_path(&p)])).unwrap();
        let spec = wav.spec();
        assert_eq!((spec.channels, spec.sample_rate, spec.bits_per_sample), (2, 48_000, 16), "{}", i.id);
        assert!((wav.duration() as f64 / 48_000.0 - crate::library::preview::SECONDS).abs() < 0.01);
        previews.push(p);
    }
    assert_ne!(previews[0], previews[1], "the sound plays with its preset's values");

    // The app's preview of the installed pack: the same bits.
    let store = Store::at(w.t.0.join("Packs"));
    let installed = match crate::packs::install(&w.keys(), &store, &std_dir()).unwrap() {
        crate::packs::Install::Installed { dir, .. } => dir,
        other => panic!("{other:?}"),
    };
    let packages = vec![("std".to_string(), std_dir()), ("@bob/keys".to_string(), installed)];
    let lib_catalog = m2_script::describe(&packages);
    let items = crate::library::items(&crate::library::Sources {
        catalog: &lib_catalog,
        packages: &packages,
        yours: None,
        plugins: &[],
        samples: &[],
    });
    let item = items.iter().find(|i| i.id == "@bob/keys:soft-keys").unwrap();
    let (song, loads) = crate::library::preview::of(item, &lib_catalog, &packages, None).unwrap();
    let wav = w.t.0.join("app-preview.wav");
    crate::library::preview::render(&song, &loads, &w.t.0.join("app-work"), &wav).unwrap();
    assert_eq!(Sha::of(&std::fs::read(&wav).unwrap()), previews[0], "the robot's preview is the app's");

    let again = w.t.0.join("bucket-again");
    w.build(&again, None).unwrap();
    assert!(tree(&again) == first, "built again into a new folder: the same bytes");
    let twice = w.build(&out, None).unwrap();
    assert!(twice.built.is_empty(), "nothing is built twice");
    assert!(tree(&out) == first);
}

/// `w.build` into a new folder, refused: what it says, and that it wrote nothing.
fn refused(w: &World, tag: &str) -> String {
    let out = w.t.0.join(tag);
    let e = w.build(&out, None).err().unwrap_or_else(|| panic!("{tag}: built, but should be refused")).join("\n");
    assert!(!out.exists() || tree(&out).is_empty(), "{tag}: a refused build writes nothing: {:?}", tree(&out).keys().collect::<Vec<_>>());
    e
}

// REG-5: a source is refused unless its files are the ones the registry lists, and unless it
// passes what the registry asks beyond `m2 pack check`: a licence from the list (D6), tags from
// the word list (P7), its own m2pack.json the listing; each source is found once.
#[test]
fn a_source_that_isnt_what_is_listed_is_refused() {
    let w = World::new("refused");
    let boom = w.drums().join("samples/kicks/Boom.wav");
    let good = std::fs::read(&boom).unwrap();
    let mut tampered = good.clone();
    *tampered.last_mut().unwrap() ^= 1;
    std::fs::write(&boom, &tampered).unwrap();
    let e = refused(&w, "tampered");
    assert!(e.contains("@alice/drums 1.0.0") && e.contains("aren't the files the registry lists"), "{e}");
    std::fs::write(&boom, &good).unwrap();

    write(&w.keys().join("m2pack.json"), "{\"m2pack\": 1, \"files\": []}\n");
    let e = refused(&w, "own-listing");
    assert!(e.contains("@bob/keys 0.1.0: its own m2pack.json isn't the listing of its files"), "{e}");
    std::fs::remove_file(w.keys().join("m2pack.json")).unwrap();

    let manifest = w.drums().join("m2pkg.toml");
    let text = std::fs::read_to_string(&manifest).unwrap();
    std::fs::write(&manifest, text.replace("CC0-1.0", "CC-BY-NC-4.0").replace("[\"drums\"]", "[\"drums\", \"techno\"]")).unwrap();
    w.list("@alice/drums", "1.0.0", &w.drums(), "");
    let e = refused(&w, "licence");
    assert!(e.contains("licence CC-BY-NC-4.0 isn't one the registry takes"), "{e}");
    assert!(e.contains("tag `techno` isn't in the registry's word list"), "{e}");
    std::fs::write(&manifest, &text).unwrap();
    w.list("@alice/drums", "1.0.0", &w.drums(), "");

    let copy = w.sources.join("copy/drums");
    for (rel, bytes) in tree(&w.drums()) {
        write(&copy.join(rel), bytes);
    }
    let e = refused(&w, "twice");
    assert!(e.contains("@alice/drums 1.0.0 is in two folders of the sources"), "{e}");
    std::fs::remove_dir_all(w.sources.join("copy")).unwrap();

    std::fs::remove_dir_all(w.keys()).unwrap();
    let e = refused(&w, "missing");
    assert!(e.contains("@bob/keys 0.1.0 isn't built yet, and its source isn't in"), "{e}");
    assert!(e.contains("https://github.com/bob/keys/releases/download/v0.1.0/keys-0.1.0.tar.gz"), "says where to get it: {e}");
    assert!(e.contains("the sources hold @alice/drums 1.0.0"), "says what is there instead: {e}");
}

// REG-2: check refuses a listed version edited from the base, a pack changed by an author not its
// scope's, and an address outside the scope's GitHub; with nothing wrong it passes.
#[test]
fn check_refuses_an_edited_version_a_wrong_scope_and_a_foreign_source() {
    let w = World::new("check");
    let base = w.t.0.join("base");
    for (rel, bytes) in tree(&w.registry) {
        write(&base.join(rel), bytes);
    }
    let file = w.registry.join("packs/@alice/drums.toml");
    let text = std::fs::read_to_string(&file).unwrap();
    let ok = |author: Option<&str>| check(&Check { registry: &w.registry, base: Some(&base), author });
    assert!(ok(Some("mallory")).is_ok(), "nothing changed: nothing is anyone's to answer for");

    std::fs::write(&file, format!("{text}\n[[versions]]\nversion = \"1.1.0\"\nhash = \"sha256:{}\"\nsource = \"https://github.com/alice/drums/releases/download/v1.1.0/drums-1.1.0.tar.gz\"\n", Sha::of(b"x").hex())).unwrap();
    assert!(ok(Some("ALICE")).is_ok(), "alice adds a version of her own pack");
    let e = ok(Some("mallory")).unwrap_err().join("\n");
    assert!(e.contains("@alice/drums is @alice's: mallory may change only packs named @mallory/…"), "{e}");

    let hash = text.lines().find(|l| l.starts_with("hash")).unwrap();
    std::fs::write(&file, text.replace(hash, &format!("hash = \"sha256:{}\"", Sha::of(b"y").hex()))).unwrap();
    let e = ok(Some("alice")).unwrap_err().join("\n");
    assert!(e.contains("@alice/drums 1.0.0 was changed: a listed version is never changed (P5)"), "{e}");
    std::fs::write(&file, format!("{text}yanked = \"a click at the end\"\n")).unwrap();
    assert!(ok(Some("alice")).is_ok(), "a yank is an edit allowed");

    std::fs::write(&file, text.replace("https://github.com/alice/drums/releases", "https://gitlab.com/alice/drums/releases")).unwrap();
    let e = ok(None).unwrap_err().join("\n");
    assert!(e.contains("packs/@alice/drums.toml: 1.0.0: source `https://gitlab.com/"), "{e}");
    std::fs::remove_file(&file).unwrap();
    let e = ok(None).unwrap_err().join("\n");
    assert!(e.contains("packs/@alice/drums.toml is gone from the registry"), "{e}");
}

// REG-8: add checks the pack, writes its m2pack.json and lists its version: in a new file, or at
// the end of the file there, which keeps its comments; the same again is nothing to do, and the
// same version with other files is refused (P5).
#[test]
fn add_lists_a_version_once_and_never_changes_it() {
    let w = World::new("add");
    std::fs::remove_dir_all(w.registry.join("packs")).unwrap();
    std::fs::create_dir_all(w.registry.join("packs")).unwrap();
    let source = "https://github.com/alice/drums/releases/download/v1.0.0/drums-1.0.0.tar.gz";
    let add_drums = |source: &str| add(&Add { registry: &w.registry, pack: &w.drums(), source, std: &std_dir() });
    let file = w.registry.join("packs/@alice/drums.toml");
    match add_drums(source).unwrap_or_else(|e| panic!("{}", e.join("\n"))) {
        Added::Listed { new: true, hash, .. } => assert_eq!(hash, Listing::of(&w.drums()).unwrap().hash()),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        std::fs::read_to_string(w.drums().join("m2pack.json")).unwrap(),
        Listing::of(&w.drums()).unwrap().text(),
        "its m2pack.json is written"
    );
    let reg = Registry::read(&w.registry).unwrap();
    assert_eq!(reg.packs[0].versions[0].source, source);
    std::fs::write(&file, format!("# Alice's drums, recorded at home\n{}", std::fs::read_to_string(&file).unwrap())).unwrap();
    assert!(matches!(add_drums(source).unwrap(), Added::Already { .. }), "listed already, with these files");

    let boom = w.drums().join("samples/kicks/Boom.wav");
    let mut bytes = std::fs::read(&boom).unwrap();
    *bytes.last_mut().unwrap() ^= 1;
    std::fs::write(&boom, &bytes).unwrap();
    let e = add_drums(source).unwrap_err().join("\n");
    assert!(e.contains("@alice/drums 1.0.0 is listed already with other files"), "{e}");

    let manifest = w.drums().join("m2pkg.toml");
    std::fs::write(&manifest, std::fs::read_to_string(&manifest).unwrap().replace("1.0.0", "1.1.0")).unwrap();
    let e = add_drums("https://github.com/mallory/drums/releases/download/v1.1.0/drums.tar.gz").unwrap_err().join("\n");
    assert!(e.contains("source `https://github.com/mallory/"), "{e}");
    assert!(matches!(add_drums(&source.replace("1.0.0", "1.1.0")).unwrap(), Added::Listed { new: false, .. }));
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.starts_with("# Alice's drums, recorded at home\n"), "{text}");
    let reg = Registry::read(&w.registry).unwrap();
    assert_eq!(reg.packs[0].versions.iter().map(|v| v.version.to_string()).collect::<Vec<_>>(), ["1.0.0", "1.1.0"]);

    let keys = w.keys();
    let m = keys.join("m2pkg.toml");
    std::fs::write(&m, std::fs::read_to_string(&m).unwrap().replace("repository = \"https://github.com/bob/keys\"\n", "")).unwrap();
    let e = add(&Add {
        registry: &w.registry,
        pack: &keys,
        source: "https://github.com/bob/keys/releases/download/v0.1.0/keys.tar.gz",
        std: &std_dir(),
    })
    .unwrap_err()
    .join("\n");
    assert!(e.contains("m2pkg.toml names no repository"), "{e}");
}

// REG-7, REG-9: what is pending is what neither the folder built into nor the bucket read from
// has; a build that reads a bucket builds nothing it has and makes the same catalog; a removed
// version isn't built, is listed with why and nothing else, and its blobs no other version uses
// are what to delete.
#[test]
fn a_removed_version_goes_and_a_bucket_is_read_where_it_is() {
    let w = World::new("removed");
    let first = w.t.0.join("bucket");
    let reg = w.registry();
    assert_eq!(pending(&reg, &first, None).len(), 2);
    w.build(&first, None).unwrap();
    let bucket = Bucket::parse(&format!("file://{}", first.display())).unwrap();
    assert!(matches!(Bucket::parse(first.to_str().unwrap()).unwrap(), Bucket::Folder(_)), "a plain folder path is a bucket too");
    assert!(Bucket::parse("ftp://example.com/bucket").is_err());
    assert!(Bucket::parse("file://relative/bucket").is_err(), "a file:// URL names a whole path");
    let fresh = w.t.0.join("fresh");
    assert!(pending(&reg, &fresh, None).len() == 2 && pending(&reg, &fresh, Some(&bucket)).is_empty());

    let built = w.build(&fresh, Some(&bucket)).unwrap();
    assert!(built.built.is_empty());
    let (a, b) = (tree(&first), tree(&fresh));
    assert_eq!(b.keys().filter(|k| k.starts_with("blobs/") || k.starts_with("versions/")).count(), 0, "nothing it has is built again");
    assert!(b[POINTER] == a[POINTER], "the same catalog, from the bucket's facts");

    let drums_hash = reg.get(&name("@alice/drums")).unwrap().versions[0].hash;
    let drums_facts = Facts::read(&a[&facts_path(&drums_hash)]).unwrap();
    w.list("@alice/drums", "1.0.0", &w.drums(), "removed = \"not ours to share\"\n");
    std::fs::remove_dir_all(w.drums()).unwrap();
    let after = w.t.0.join("after");
    let built = w.build(&after, Some(&bucket)).unwrap();
    assert_eq!(built.to_delete, drums_facts.blobs, "the removed version's blobs, none of them another's");
    let c = catalog_in(&tree(&after));
    let drums = c.get(&name("@alice/drums")).unwrap();
    assert_eq!(
        (drums.latest, drums.items.len(), drums.versions[0].removed.as_deref(), drums.versions[0].size),
        (None, 0, Some("not ours to share"), None)
    );
    assert_eq!(drums.license, "CC0-1.0", "what it said of itself stays, from its facts");
}
