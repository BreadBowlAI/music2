use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use m2_song::pack::{Listing, PackName, Store};
use m2_song::registry::{Catalog, Facts, ItemKind, POINTER, Pointer, Registry, blob_path, catalog_path, facts_path};
use m2_song::{Sha, Version};

use super::*;

pub(crate) fn std_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")
}

/// A scratch folder, removed when dropped (read-only files too: an installed pack's).
pub(crate) struct Temp(pub(crate) PathBuf);

impl Temp {
    pub(crate) fn new(tag: &str) -> Temp {
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

pub(crate) fn write(path: &Path, text: impl AsRef<[u8]>) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// A 16-bit mono WAV of a tenth of a second of a tone at 48 kHz: the same bytes every time.
pub(crate) fn tone(path: &Path) {
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

/// A made-up registry, its packs' folders, and where the CI would unpack each pending version:
/// `@alice/drums` 1.0.0, one sample that `samples.toml` names, and `@bob/keys` 0.1.0, one
/// instrument and one sound of it.
pub(crate) struct World {
    pub(crate) t: Temp,
    pub(crate) registry: PathBuf,
    /// Each pack's folder, by name and version: what its `.tar.gz` would hold.
    packs: std::cell::RefCell<BTreeMap<(String, String), PathBuf>>,
    /// Where [`World::stage`] unpacks each pending version, `<hex>/<its folder>`, as the CI does.
    sources: PathBuf,
}

impl World {
    pub(crate) fn new(tag: &str) -> World {
        let t = Temp::new(tag);
        let (registry, sources) = (t.0.join("registry"), t.0.join("sources"));
        let drums = t.0.join("src/drums-1.0.0");
        write(
            &drums.join("m2pkg.toml"),
            "[package]\nname = \"@alice/drums\"\nversion = \"1.0.0\"\nlicense = \"CC0-1.0\"\ndescription = \"One kick\"\n\
             authors = [\"Alice\"]\nrepository = \"https://github.com/alice/drums\"\ntags = [\"drums\"]\n",
        );
        write(&drums.join("samples.toml"), "[folder.\"kicks\"]\ninstrument = \"kick\"\n");
        tone(&drums.join("samples/kicks/Boom.wav"));
        let keys = t.0.join("src/keys/keys-0.1.0");
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
        let w = World { t, registry, packs: Default::default(), sources };
        w.list("@alice/drums", "1.0.0", &drums, "");
        w.list("@bob/keys", "0.1.0", &keys, "");
        w
    }

    pub(crate) fn drums(&self) -> PathBuf {
        self.t.0.join("src/drums-1.0.0")
    }

    pub(crate) fn keys(&self) -> PathBuf {
        self.t.0.join("src/keys/keys-0.1.0")
    }

    /// Write `name`'s pack file with one version, its hash the listing of `dir` now, and `more`
    /// lines in its table; `dir` is that version's source.
    pub(crate) fn list(&self, name: &str, version: &str, dir: &Path, more: &str) {
        let n = PackName::new(name).unwrap();
        let owner = m2_song::registry::github_owner(&n);
        let short = name.rsplit('/').next().unwrap();
        let text = format!(
            "name = \"{name}\"\nrepository = \"https://github.com/{owner}/{short}\"\n\n[[versions]]\nversion = \"{version}\"\nhash = \"{}\"\n\
             source = \"https://github.com/{owner}/{short}/releases/download/v{version}/{short}-{version}.tar.gz\"\n{more}",
            Listing::of(dir).unwrap().hash().prefixed()
        );
        write(&self.registry.join(m2_song::registry::PackFile::path_of(&n)), text);
        self.packs.borrow_mut().insert((name.to_string(), version.to_string()), dir.to_path_buf());
    }

    /// List version `version` of `name` in removed.toml (REG-7), as music2's people would.
    pub(crate) fn take_down(&self, name: &str, version: &str) {
        let reg = self.registry();
        let hash = reg.get(&PackName::new(name).unwrap()).unwrap().get(Version::parse(version).unwrap()).unwrap().hash;
        let file = self.registry.join(m2_song::registry::REMOVED);
        let mut text = std::fs::read_to_string(&file).unwrap_or_default();
        text += &format!(
            "[[removed]]\nname = \"{name}\"\nversion = \"{version}\"\nhash = \"{}\"\nwhy = \"not ours to share\"\n",
            hash.prefixed()
        );
        std::fs::write(file, text).unwrap();
    }

    /// Unpack each listed version's source as the CI does: its folder copied into
    /// `sources/<the listed hash's hex>/`, whatever its files are now.
    fn stage(&self) {
        let _ = std::fs::remove_dir_all(&self.sources);
        for p in &self.registry().packs {
            for v in &p.versions {
                let packs = self.packs.borrow();
                let Some(dir) = packs.get(&(p.name.as_str().to_string(), v.version.to_string())).filter(|d| d.is_dir()) else { continue };
                let to = self.sources.join(v.hash.hex()).join(dir.file_name().unwrap());
                for (rel, bytes) in tree(dir) {
                    write(&to.join(rel), bytes);
                }
            }
        }
    }

    /// Stage the sources, then build.
    pub(crate) fn build(&self, out: &Path, from: Option<&Bucket>) -> Result<Built, Vec<String>> {
        self.stage();
        self.run(out, from, LONGEST)
    }

    /// Build from the sources as they are staged, each version given `limit`.
    fn run(&self, out: &Path, from: Option<&Bucket>, limit: std::time::Duration) -> Result<Built, Vec<String>> {
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
            limit,
        })
    }

    pub(crate) fn registry(&self) -> Registry {
        Registry::read(&self.registry).unwrap()
    }

    /// The listed hash of `name`'s version `version`.
    pub(crate) fn hash(&self, name: &str, version: &str) -> Sha {
        self.registry().get(&PackName::new(name).unwrap()).unwrap().get(Version::parse(version).unwrap()).unwrap().hash
    }
}

/// Copy what a build wrote in `out` into the bucket `live`, as the publish job does: catalog.json
/// replaced, the rest added.
fn publish_into(out: &Path, live: &Path) {
    for (rel, bytes) in tree(out) {
        write(&live.join(rel), bytes);
    }
}

/// Every file under `dir`, by its path there.
pub(crate) fn tree(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    crate::testing::files_under(dir)
        .into_iter()
        .map(|p| (p.strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/"), std::fs::read(&p).unwrap()))
        .collect()
}

fn name(s: &str) -> PackName {
    PackName::new(s).unwrap()
}

/// The catalog `out`'s catalog.json names, checked.
pub(crate) fn catalog_in(out: &BTreeMap<String, Vec<u8>>) -> Catalog {
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

/// `w.build` into the new folder `tag`, where `name` fails and the other version is built: what
/// it says of `name`, and that nothing of it is in the bucket.
fn fails_alone(w: &World, tag: &str, name: &str) -> String {
    let out = w.t.0.join(tag);
    let built = w.build(&out, None).unwrap_or_else(|e| panic!("{tag}: {}", e.join("\n")));
    let [failed] = built.failed.as_slice() else { panic!("{tag}: one failure: {:?}", built.failed) };
    assert_eq!(failed.name.as_str(), name, "{tag}");
    assert_eq!(built.built.len(), 1, "{tag}: the other version is built all the same");
    let kept: std::collections::BTreeSet<Sha> = built.built.iter().flat_map(|f| f.blobs.iter().copied()).collect();
    for (path, _) in tree(&out) {
        if let Some(hex) = path.strip_prefix("blobs/sha256/") {
            assert!(kept.iter().any(|b| b.hex() == hex), "{tag}: {path} is no built version's: a failed version writes nothing");
        }
        assert!(!path.starts_with(&format!("versions/{}", w.hash(name, &failed.version.to_string()).hex())), "{tag}: no facts of {name}");
    }
    let c = catalog_in(&tree(&out));
    assert!(c.get(&self::name(name)).is_none(), "{tag}: {name} isn't in the catalog");
    assert!(pending(&w.registry(), &out, None).iter().any(|(n, _)| n.as_str() == name), "{tag}: {name} stays pending");
    failed.problems.join("\n")
}

// REG-5 (review G5, G8): a source is refused unless its files are the ones the registry lists,
// and unless it passes what the registry asks beyond `m2 pack check`: a licence from the list
// (D6), tags from the word list (P7), its own m2pack.json the listing. Each version is built on
// its own from its own folder: one that fails (refused, missing, a tarball holding another
// version, a pipe, too big) is reported, writes nothing and stays pending, and never stops the
// others.
#[test]
fn a_source_that_isnt_what_is_listed_fails_alone() {
    let w = World::new("refused");
    let boom = w.drums().join("samples/kicks/Boom.wav");
    let good = std::fs::read(&boom).unwrap();
    let mut tampered = good.clone();
    *tampered.last_mut().unwrap() ^= 1;
    std::fs::write(&boom, &tampered).unwrap();
    let e = fails_alone(&w, "tampered", "@alice/drums");
    assert!(e.contains("@alice/drums 1.0.0") && e.contains("aren't the files the registry lists"), "{e}");
    std::fs::write(&boom, &good).unwrap();

    write(&w.keys().join("m2pack.json"), "{\"m2pack\": 1, \"files\": []}\n");
    let e = fails_alone(&w, "own-listing", "@bob/keys");
    assert!(e.contains("@bob/keys 0.1.0: its own m2pack.json isn't the listing of its files"), "{e}");
    std::fs::remove_file(w.keys().join("m2pack.json")).unwrap();

    let manifest = w.drums().join("m2pkg.toml");
    let text = std::fs::read_to_string(&manifest).unwrap();
    std::fs::write(&manifest, text.replace("CC0-1.0", "CC-BY-NC-4.0").replace("[\"drums\"]", "[\"drums\", \"techno\"]")).unwrap();
    w.list("@alice/drums", "1.0.0", &w.drums(), "");
    let e = fails_alone(&w, "licence", "@alice/drums");
    assert!(e.contains("licence CC-BY-NC-4.0 isn't one the registry takes"), "{e}");
    assert!(e.contains("tag `techno` isn't in the registry's word list"), "{e}");
    std::fs::write(&manifest, &text).unwrap();
    w.list("@alice/drums", "1.0.0", &w.drums(), "");

    // Its .tar.gz holds it twice, or holds another version instead: only its own build fails.
    w.stage();
    let hex = w.hash("@alice/drums", "1.0.0").hex();
    for (rel, bytes) in tree(&w.drums()) {
        write(&w.sources.join(&hex).join("again/drums").join(rel), bytes);
    }
    let out = w.t.0.join("twice");
    let built = w.run(&out, None, LONGEST).unwrap();
    assert!(built.failed[0].problems[0].contains("@alice/drums 1.0.0 is in two folders of its source"), "{:?}", built.failed);
    w.stage();
    std::fs::remove_dir_all(w.sources.join(&hex)).unwrap();
    for (rel, bytes) in tree(&w.keys()) {
        write(&w.sources.join(&hex).join("keys").join(rel), bytes);
    }
    let built = w.run(&w.t.0.join("another"), None, LONGEST).unwrap();
    let e = built.failed[0].problems.join("\n");
    assert!(e.contains("@alice/drums 1.0.0: its source, https://github.com/alice/drums/releases/download/v1.0.0/drums-1.0.0.tar.gz, holds @bob/keys 0.1.0, not @alice/drums 1.0.0"), "{e}");
    assert_eq!(built.built.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["@bob/keys"], "keys from its own folder only");

    let real = w.keys();
    std::fs::rename(&real, w.t.0.join("src/keys/away")).unwrap();
    let e = fails_alone(&w, "missing", "@bob/keys");
    assert!(e.contains("@bob/keys 0.1.0 isn't built yet, and its source isn't there"), "{e}");
    assert!(e.contains("unpack https://github.com/bob/keys/releases/download/v0.1.0/keys-0.1.0.tar.gz into"), "says where to get it: {e}");
    std::fs::rename(w.t.0.join("src/keys/away"), &real).unwrap();

    // A pipe is refused before any file is opened (it would wait for ever), and a source over
    // 10 GB by its sizes, before any byte is hashed (a hashed one would say its files aren't the
    // listed ones first).
    w.stage();
    let drums_src = w.sources.join(&hex).join("drums-1.0.0");
    assert!(std::process::Command::new("mkfifo").arg(drums_src.join("samples/kicks/Hang.wav")).status().unwrap().success());
    let built = w.run(&w.t.0.join("pipe"), None, LONGEST).unwrap();
    assert!(built.failed[0].problems[0].contains("samples/kicks/Hang.wav isn't a plain file (a pipe or a device)"), "{:?}", built.failed);
    w.stage();
    let big = std::fs::File::create(drums_src.join("big.txt")).unwrap();
    big.set_len(LARGEST + 1).unwrap();
    let built = w.run(&w.t.0.join("big"), None, LONGEST).unwrap();
    assert!(
        built.failed[0].problems[0].contains("@alice/drums 1.0.0 is 10.0 GB: the registry takes packs up to 10 GB (D9)"),
        "{:?}",
        built.failed
    );
}

/// A kernel module whose kernel `spin` never returns from its `spin_process`: `module_v1` in
/// core/runtime's tests around these exports, made by `wat` once (music2's editor has no `wat`).
///
/// ```text
/// (func (export "spin_new") (param i32) (result i32) (i32.const 4096))
/// (func (export "spin_ports") (result i32) (i32.const 1))
/// (func (export "spin_reset") (param i32))
/// (func (export "spin_active") (param i32) (result i32) (i32.const 0))
/// (func (export "spin_process") (param i32 i32 i32 i32) (loop $l (br $l)))
/// ```
const SPIN: &[u8] = &[
    0, 97, 115, 109, 1, 0, 0, 0, 1, 27, 5, 96, 2, 127, 127, 1, 127, 96, 1, 127, 1, 127, 96, 0, 1, 127, 96, 1, 127, 0, 96, 4, 127, 127, 127,
    127, 0, 3, 7, 6, 0, 1, 2, 3, 1, 4, 5, 3, 1, 0, 1, 6, 8, 1, 127, 1, 65, 128, 192, 0, 11, 7, 87, 7, 6, 109, 101, 109, 111, 114, 121, 2,
    0, 8, 109, 50, 95, 97, 108, 108, 111, 99, 0, 0, 8, 115, 112, 105, 110, 95, 110, 101, 119, 0, 1, 10, 115, 112, 105, 110, 95, 112, 111,
    114, 116, 115, 0, 2, 10, 115, 112, 105, 110, 95, 114, 101, 115, 101, 116, 0, 3, 11, 115, 112, 105, 110, 95, 97, 99, 116, 105, 118, 101,
    0, 4, 12, 115, 112, 105, 110, 95, 112, 114, 111, 99, 101, 115, 115, 0, 5, 10, 97, 6, 68, 1, 2, 127, 35, 0, 32, 1, 65, 1, 107, 106, 65,
    0, 32, 1, 107, 113, 33, 2, 32, 2, 32, 0, 106, 33, 3, 32, 3, 63, 0, 65, 16, 116, 75, 4, 64, 32, 3, 65, 255, 255, 3, 106, 65, 16, 118,
    63, 0, 107, 64, 0, 65, 127, 70, 4, 64, 65, 0, 15, 11, 11, 32, 3, 36, 0, 32, 2, 11, 5, 0, 65, 128, 32, 11, 4, 0, 65, 1, 11, 2, 0, 11, 4,
    0, 65, 0, 11, 7, 0, 3, 64, 12, 0, 11, 11, 0, 11, 6, 109, 50, 95, 97, 98, 105, 1, 0, 0, 0, 0, 49, 4, 110, 97, 109, 101, 2, 25, 1, 0, 4,
    0, 4, 115, 105, 122, 101, 1, 5, 97, 108, 105, 103, 110, 2, 2, 97, 116, 3, 3, 101, 110, 100, 3, 6, 1, 5, 1, 0, 1, 108, 7, 7, 1, 0, 4,
    104, 101, 97, 112,
];

// REG-5 (review G8), ABI-7: a version whose kernel never returns (P4) as its preview renders is
// refused, and the robot goes on: the others are built. Since track `limits` the kernel's call is
// stopped at its limit (ABI-6), so it is refused in seconds, in the limit's words, long before the
// version's own time runs out.
#[test]
fn a_kernel_that_never_returns_is_refused_and_the_rest_are_built() {
    let w = World::new("spin");
    let spin = w.t.0.join("src/spin-0.1.0");
    write(
        &spin.join("m2pkg.toml"),
        "[package]\nname = \"@carol/spin\"\nversion = \"0.1.0\"\nlicense = \"MIT\"\nwasm = \"k.wasm\"\n\n[dependencies]\n\"@m2/core\" = \"1\"\n\
         std = \"0.5\"\n\n[[kernel]]\nname = \"spin\"\ndescription = \"Never returns\"\noutputs = [\"out: audio<1>\"]\n",
    );
    write(&spin.join("k.wasm"), SPIN);
    write(
        &spin.join("src/index.ts"),
        "import { component, kernel, port, signature, type Audio, type Events } from \"@m2/core\";\n\n\
         export const spin = component(\n  {\n    id: \"spin\",\n    label: \"Spin\",\n    signature: signature<(notes: Events) => Audio>(),\n    \
         inputs: { notes: port.notes() },\n    output: port.audio(),\n    editor: { category: \"Keys\", description: \"Never returns\" },\n  },\n  \
         (_notes: Events): Audio => kernel(\"@carol/spin:spin\", {}),\n);\n",
    );
    w.list("@carol/spin", "0.1.0", &spin, "");
    w.stage();
    let started = std::time::Instant::now();
    let built = w.run(&w.t.0.join("bucket"), None, std::time::Duration::from_secs(20)).unwrap_or_else(|e| panic!("{}", e.join("\n")));
    assert!(started.elapsed() < std::time::Duration::from_secs(120), "the robot went on: {:?}", started.elapsed());
    let [failed] = built.failed.as_slice() else { panic!("{:?}", built.failed) };
    assert_eq!(failed.name.as_str(), "@carol/spin");
    let stopped = "@carol/spin 0.1.0: Spin's preview was stopped, so it isn't built: Spin on Play, from @carol/spin, took too long \
                   and was stopped, so the file wasn't made.";
    assert_eq!(failed.problems, [stopped], "stopped by the kernel's limit, not the version's");
    assert!(started.elapsed() < std::time::Duration::from_secs(20), "in seconds: {:?}", started.elapsed());
    assert_eq!(built.built.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["@alice/drums", "@bob/keys"]);
}

// REG-2: check refuses a listed version edited from the base, a file changed by an author not its
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
    assert!(e.contains("packs/@alice/drums.toml is @alice's: mallory may change only packs named @mallory/…"), "{e}");

    let hash = text.lines().find(|l| l.starts_with("hash")).unwrap();
    std::fs::write(&file, text.replace(hash, &format!("hash = \"sha256:{}\"", Sha::of(b"y").hex()))).unwrap();
    let e = ok(Some("alice")).unwrap_err().join("\n");
    assert!(e.contains("@alice/drums 1.0.0 was changed: a listed version is never changed (P5)"), "{e}");
    std::fs::write(&file, format!("{text}yanked = \"a click at the end\"\n")).unwrap();
    assert!(ok(Some("alice")).is_ok(), "a yank is an edit allowed");
    std::fs::write(&file, &text).unwrap();
    w.take_down("@alice/drums", "1.0.0");
    let e = ok(Some("alice")).unwrap_err().join("\n");
    assert!(e.contains("removed.toml is music2's people's to change"), "only music2's people take down: {e}");
    assert!(ok(None).is_ok(), "music2's people may");
    std::fs::remove_file(w.registry.join("removed.toml")).unwrap();

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
// has; a build that reads a bucket builds nothing it has and makes the same catalog; a taken-down
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
    w.take_down("@alice/drums", "1.0.0");
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

// REG-5, REG-7 (review G2, G3): what the bucket publishes never changes, by anyone: a published
// version listed with other files, or a take-down taken back, refuses the whole build, and nothing
// is written; a yank and a new version are built.
#[test]
fn what_is_published_never_changes() {
    let w = World::new("published");
    let live = w.t.0.join("live");
    let out = w.t.0.join("out1");
    w.build(&out, None).unwrap();
    publish_into(&out, &live);
    let bucket = Bucket::Folder(live.clone());

    let boom = w.drums().join("samples/kicks/Boom.wav");
    let mut bytes = std::fs::read(&boom).unwrap();
    *bytes.last_mut().unwrap() ^= 1;
    std::fs::write(&boom, &bytes).unwrap();
    let file = w.registry.join("packs/@alice/drums.toml");
    let was = std::fs::read_to_string(&file).unwrap();
    w.list("@alice/drums", "1.0.0", &w.drums(), "");
    assert!(pending(&w.registry(), &w.t.0.join("out2"), Some(&bucket)).len() == 1, "the new hash isn't in the bucket");
    let e = w.build(&w.t.0.join("out2"), Some(&bucket)).unwrap_err().join("\n");
    assert!(e.contains("@alice/drums 1.0.0 is published with the hash") && e.contains("never changes (P5)"), "{e}");
    assert!(!w.t.0.join("out2").exists(), "a refused build writes nothing");
    std::fs::write(&file, &was).unwrap();

    w.take_down("@alice/drums", "1.0.0");
    let out = w.t.0.join("out3");
    let built = w.build(&out, Some(&bucket)).unwrap();
    assert!(!built.to_delete.is_empty());
    publish_into(&out, &live);
    std::fs::remove_file(w.registry.join("removed.toml")).unwrap();
    let e = w.build(&w.t.0.join("out4"), Some(&bucket)).unwrap_err().join("\n");
    assert!(e.contains("@alice/drums 1.0.0 was taken down, and a take-down is never taken back"), "{e}");
}
