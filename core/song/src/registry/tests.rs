use std::path::{Path, PathBuf};

use super::*;

fn scratch(tag: &str) -> PathBuf {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let d = std::env::temp_dir().join(format!("m2-song-registry-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn sha(n: u8) -> Sha {
    Sha::of(&[n])
}

/// A pack file of `@alice/drums` listing `versions`, each `(version, hash, more lines)`.
fn drums(versions: &[(&str, Sha, &str)]) -> String {
    let mut out = "name = \"@alice/drums\"\nrepository = \"https://github.com/alice/drums\"\n".to_string();
    for (v, h, more) in versions {
        out += &format!(
            "\n[[versions]]\nversion = \"{v}\"\nhash = \"{}\"\nsource = \"https://github.com/alice/drums/releases/download/v{v}/drums-{v}.tar.gz\"\n{more}",
            h.prefixed()
        );
    }
    out
}

fn refused(text: &str, why: &str) {
    match PackFile::read(text) {
        Ok(p) => panic!("{text}\nread as {p:?}, but should be refused for {why}"),
        Err(e) => assert!(e.contains(why), "refused, but says `{e}`, not `{why}`:\n{text}"),
    }
}

// REG-1: a pack file reads strictly: an unknown key anywhere, std, no version, a version or a hash
// twice, an empty reason, or a take-down (removed.toml's, REG-7) is refused, in words.
#[test]
fn a_pack_file_is_read_strictly() {
    let good = drums(&[("1.0.0", sha(1), ""), ("1.1.0", sha(2), "yanked = \"a click at the end of the kick\"\n")]);
    let p = PackFile::read(&good).unwrap();
    assert_eq!(p.name.as_str(), "@alice/drums");
    assert_eq!(p.versions.len(), 2);
    let reg = Registry { packs: vec![p.clone()], ..Default::default() };
    assert_eq!(reg.latest(&p).map(|v| v.version), Some(Version(1, 0, 0)), "a yanked version is never the latest");
    assert_eq!(PackFile::read(&p.text()).unwrap(), p, "its own text reads back as itself");

    refused(&format!("{good}colour = \"red\"\n"), "colour");
    refused(&drums(&[("1.0.0", sha(1), "notes = \"x\"\n")]), "notes");
    refused(&good.replace("@alice/drums\"\nrepo", "std\"\nrepo"), "never listed");
    refused("name = \"@alice/drums\"\nrepository = \"https://github.com/alice/drums\"\n", "lists no version");
    refused(&drums(&[("1.0.0", sha(1), ""), ("1.0.0", sha(2), "")]), "listed twice");
    refused(&drums(&[("1.0.0", sha(1), ""), ("1.1.0", sha(1), "")]), "never the same files");
    refused(&drums(&[("1.0.0", sha(1), "removed = \"not ours\"\n")]), "a take-down isn't written in a pack's file but in removed.toml");
    refused(&drums(&[("1.0.0", sha(1), "yanked = \" \"\n")]), "can't be empty");
    refused(&drums(&[("1.0", sha(1), "")]), "must be a version");
    refused(&good.replace("sha256:", "md5:"), "sha256:");
}

// REG-1: a pack's repository and sources are under its scope's own GitHub, in any capitals, and
// `@m2/`'s under BreadBowlAI's; a source is a .tar.gz; an address is plain.
#[test]
fn a_pack_files_addresses_are_its_scopes_github() {
    let good = drums(&[("1.0.0", sha(1), "")]);
    PackFile::read(&good.replace("github.com/alice/", "github.com/Alice/")).expect("GitHub's names are in any capitals");
    refused(
        &good.replace("repository = \"https://github.com/alice/drums\"", "repository = \"https://github.com/mallory/drums\""),
        "repository",
    );
    refused(&good.replace("github.com/alice/drums/releases", "github.com/mallory/drums/releases"), "source");
    refused(&good.replace("https://github.com/alice/drums/releases", "https://example.com/alice/drums/releases"), "source");
    refused(&good.replace("https://github.com/alice/drums/releases", "http://github.com/alice/drums/releases"), "source");
    refused(&good.replace("drums-1.0.0.tar.gz", "drums-1.0.0.zip"), ".tar.gz");
    refused(&good.replace("download/v1.0.0", "download/../v1.0.0"), "`..`");
    refused(&good.replace("drums-1.0.0.tar.gz", "drums-1.0.0.tar.gz?x=1"), "plain address");
    refused(&good.replace("drums-1.0.0.tar.gz", "drums-1.0.0;rm -rf.tar.gz"), "plain address");
    // An encoded part could name another owner's repo once the server decodes it (G10).
    refused(&good.replace("releases/download/v1.0.0", "%2e%2e/mallory/evil/releases/download/v1.0.0"), "plain address");
    refused(&good.replace("https://github.com/alice/drums\"", "https://github.com/alice/%2e%2e\""), "plain address");
    refused(&good.replace("download/v1.0.0/", "download/v1.0.0%2F"), "plain address");
    refused(&good.replace("drums-1.0.0.tar.gz", "drums~1.0.0.tar.gz"), "plain address");
    refused(&good.replace("drums-1.0.0.tar.gz", "drums+1.0.0.tar.gz"), "plain address");
    refused(&good.replace("download/v1.0.0/", "download/./v1.0.0/"), "plain address");
    refused(&good.replace("https://github.com/alice/drums\"", "https://github.com/alice/drums/tree/main\""), "no more");
    refused(
        &good.replace("https://github.com/alice/drums/releases/download/v1.0.0/", "https://github.com/alice/"),
        ".tar.gz file in one of",
    );

    let ours = "name = \"@m2/grand-piano\"\nrepository = \"https://github.com/BreadBowlAI/m2-grand-piano\"\n\n[[versions]]\n\
                version = \"0.1.0\"\nhash = \"sha256:{h}\"\nsource = \"https://github.com/BreadBowlAI/m2-grand-piano/releases/download/v0.1.0/grand-piano-0.1.0.tar.gz\"\n"
        .replace("{h}", &sha(1).hex());
    PackFile::read(&ours).expect("@m2/ is BreadBowlAI's");
    refused(&ours.replace("BreadBowlAI/m2-grand-piano\"", "m2/m2-grand-piano\""), "repository");
}

fn write(dir: &Path, rel: &str, text: &str) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

/// A registry folder with the word list and `files`.
fn registry(files: &[(&str, &str)]) -> PathBuf {
    let d = scratch("reg");
    write(&d, TAGS, "# the words a pack's tags may use\ndrums\nlofi\n\nkeys\n");
    std::fs::create_dir_all(d.join(PACKS)).unwrap();
    write(&d, "packs/.gitkeep", "");
    for (rel, text) in files {
        write(&d, rel, text);
    }
    d
}

// REG-1: the registry's folder holds the word list and, under packs/, only pack files at their
// own names; each problem is named with its file.
#[test]
fn a_registry_holds_only_pack_files_at_their_names() {
    let good = drums(&[("1.0.0", sha(1), "")]);
    let d = registry(&[("packs/@alice/drums.toml", &good)]);
    let r = Registry::read(&d).unwrap();
    assert_eq!(r.packs.len(), 1);
    assert_eq!(r.tags.iter().map(String::as_str).collect::<Vec<_>>(), ["drums", "keys", "lofi"]);
    assert_eq!(PackFile::path_of(&r.packs[0].name), Path::new("packs/@alice/drums.toml"));

    write(&d, "packs/@alice/.note", "read me");
    assert_eq!(Registry::read(&d).unwrap().files["packs/@alice/.note"], b"read me", "a dot file is no pack's, but its bytes are kept");

    let bad = registry(&[
        ("packs/@alice/kicks.toml", &good),
        ("packs/@alice/notes.txt", "hello"),
        ("packs/stray.toml", &good),
        ("packs/@bob/x.toml", "name = 1\n"),
    ]);
    let problems = Registry::read(&bad).unwrap_err().join("\n");
    for want in [
        "packs/@alice/kicks.toml names @alice/drums: a pack's file is packs/@alice/drums.toml",
        "packs/@alice/notes.txt: only pack files",
        "packs/stray.toml: only folders named @owner",
        "packs/@bob/x.toml:",
    ] {
        assert!(problems.contains(want), "says `{want}`:\n{problems}");
    }

    std::os::unix::fs::symlink(d.join(TAGS), d.join("packs/@alice/link.toml")).unwrap();
    assert!(Registry::read(&d).unwrap_err().join("\n").contains("packs/@alice/link.toml is a link"), "a link is never followed");
    std::fs::remove_file(d.join("packs/@alice/link.toml")).unwrap();

    std::fs::write(d.join(TAGS), "drums\nLo-Fi\n").unwrap();
    assert!(Registry::read(&d).unwrap_err().join("\n").contains("tags.txt:2: `Lo-Fi` isn't a tag"));
    std::fs::remove_file(d.join(TAGS)).unwrap();
    assert!(Registry::read(&d).unwrap_err().join("\n").contains("tags.txt can't be read"));
}

fn read(text: &str) -> Registry {
    Registry { packs: vec![PackFile::read(text).unwrap()], ..Default::default() }
}

/// `r` with version `version` of `@alice/drums` taken down, its hash `hash`.
fn taken_down(mut r: Registry, version: &str, hash: Sha) -> Registry {
    let name = PackName::new("@alice/drums").unwrap();
    r.removed.push(Removal { name, version: Version::parse(version).unwrap(), hash, why: "not ours to share".into() });
    r
}

// REG-2: a listed version never changes (P5): a pack file or a version gone, a hash or source
// changed, or a take-down taken back breaks the promise; yanking, taking down and adding a
// version don't.
#[test]
fn a_listed_version_never_changes() {
    let base = read(&drums(&[("1.0.0", sha(1), ""), ("1.1.0", sha(2), "")]));
    let fine = taken_down(
        read(&drums(&[("1.0.0", sha(1), ""), ("1.1.0", sha(2), "yanked = \"a click\"\n"), ("1.2.0", sha(3), "")])),
        "1.0.0",
        sha(1),
    );
    assert_eq!(fine.broken_promises(&base), Vec::<String>::new());
    assert_eq!(
        base.broken_promises(&fine),
        vec![
            "@alice/drums 1.2.0 is gone from its file: a listed version stays listed. Yank it instead".to_string(),
            "removed.toml: @alice/drums 1.0.0 was taken down, and a take-down is never taken back: its files are gone".to_string(),
        ]
    );

    let edited = read(&drums(&[("1.0.0", sha(9), ""), ("1.1.0", sha(2), "")]));
    assert!(edited.broken_promises(&base)[0].contains("@alice/drums 1.0.0 was changed"));
    let moved = read(&drums(&[("1.0.0", sha(1), ""), ("1.1.0", sha(2), "")]).replace("v1.1.0/drums-1.1.0", "v1.1.0/other"));
    assert!(moved.broken_promises(&base)[0].contains("1.1.0 was changed"), "a new source is a change too");
    let gone = Registry::default();
    assert!(gone.broken_promises(&base)[0].contains("packs/@alice/drums.toml is gone from the registry"));
}

// REG-1, REG-7: a take-down is in removed.toml, read strictly, naming a listed version by its
// listed hash; a hash is listed once in the whole registry, so no pack can take another's.
#[test]
fn take_downs_and_hashes_are_the_registrys_own() {
    let good = drums(&[("1.0.0", sha(1), ""), ("1.1.0", sha(2), "")]);
    let removal = |version: &str, hash: Sha| {
        format!(
            "[[removed]]\nname = \"@alice/drums\"\nversion = \"{version}\"\nhash = \"{}\"\nwhy = \"not ours to share\"\n",
            hash.prefixed()
        )
    };
    let d = registry(&[("packs/@alice/drums.toml", &good), (REMOVED, &removal("1.0.0", sha(1)))]);
    let r = Registry::read(&d).unwrap();
    let name = PackName::new("@alice/drums").unwrap();
    assert_eq!(r.removal(&name, Version(1, 0, 0)).map(|t| t.why.as_str()), Some("not ours to share"));
    assert_eq!(r.latest(&r.packs[0]).map(|v| v.version), Some(Version(1, 1, 0)), "a version taken down is never the latest");

    std::fs::write(d.join(REMOVED), removal("1.0.0", sha(9))).unwrap();
    assert!(Registry::read(&d).unwrap_err().join("\n").contains("removed.toml: @alice/drums 1.0.0 isn't listed with that hash"));
    std::fs::write(d.join(REMOVED), removal("1.0.0", sha(1)) + &removal("1.0.0", sha(1))).unwrap();
    assert!(Registry::read(&d).unwrap_err().join("\n").contains("taken down twice"));
    std::fs::write(d.join(REMOVED), removal("1.0.0", sha(1)).replace("why", "because")).unwrap();
    assert!(Registry::read(&d).unwrap_err().join("\n").contains("because"));
    std::fs::remove_file(d.join(REMOVED)).unwrap();

    let theirs = good.replace("alice/drums", "mallory/x").replace("1.1.0", "0.0.1");
    write(&d, "packs/@mallory/x.toml", &theirs);
    let e = Registry::read(&d).unwrap_err().join("\n");
    assert!(e.contains("list one hash: two versions are never the same files"), "{e}");
}

// REG-2: an author may change only the files in their own scope's folder, compared byte for byte
// (a comment and a dot file too), in any capitals, and never @m2/'s, tags.txt or removed.toml;
// what isn't changed from the base isn't theirs to answer for.
#[test]
fn an_author_changes_only_their_own_files() {
    let alice = drums(&[("1.0.0", sha(1), "")]);
    let bob = alice.replace("alice", "bob").replace(&sha(1).prefixed(), &sha(2).prefixed());
    let ours = "name = \"@m2/kit\"\nrepository = \"https://github.com/BreadBowlAI/m2-kit\"\n\n[[versions]]\nversion = \"0.1.0\"\n\
                hash = \"sha256:{h}\"\nsource = \"https://github.com/BreadBowlAI/m2-kit/releases/download/v0.1.0/kit.tar.gz\"\n"
        .replace("{h}", &sha(5).hex());
    let base_dir = registry(&[("packs/@bob/drums.toml", &bob)]);
    let base = Registry::read(&base_dir).unwrap();
    let d = registry(&[("packs/@bob/drums.toml", &bob), ("packs/@alice/drums.toml", &alice)]);
    let now = Registry::read(&d).unwrap();
    assert_eq!(now.not_authors(Some(&base), "Alice"), Vec::<String>::new(), "bob's file is as it was");
    assert_eq!(
        now.not_authors(Some(&base), "mallory"),
        vec!["packs/@alice/drums.toml is @alice's: mallory may change only packs named @mallory/…"]
    );
    assert_eq!(now.not_authors(None, "alice").len(), 3, "with no base every file is checked: bob's, tags.txt, .gitkeep");

    // The same pack, read the same, but not the same bytes: a comment, and a note beside it.
    write(&d, "packs/@bob/drums.toml", &format!("# WARNING: abandoned, use @mallory/drums\n{bob}"));
    write(&d, "packs/@bob/.note", "use @mallory/drums");
    let now = Registry::read(&d).unwrap();
    assert_eq!(now.get(&PackName::new("@bob/drums").unwrap()), base.get(&PackName::new("@bob/drums").unwrap()));
    assert_eq!(
        now.not_authors(Some(&base), "alice"),
        vec![
            "packs/@bob/.note is @bob's: alice may change only packs named @alice/…".to_string(),
            "packs/@bob/drums.toml is @bob's: alice may change only packs named @alice/…".to_string(),
        ]
    );

    write(&d, "tags.txt", "drums\nmalware\n");
    write(&d, "packs/@m2/kit.toml", &ours);
    let now = Registry::read(&d).unwrap();
    let e = now.not_authors(Some(&base), "bob");
    assert!(e.contains(&"packs/@m2/kit.toml is music2's own (@m2/): only music2's people change it".to_string()), "{e:?}");
    assert!(e.contains(&"tags.txt is music2's people's to change: an author changes only their own packs' files".to_string()), "{e:?}");
    assert_eq!(now.not_authors(Some(&base), "m2").len(), e.len() + 2, "a login named m2 owns nothing: bob's files are refused to it too");
}

/// The facts of `@alice/drums` at `version` with `hash`: one sample, whose file and preview are
/// the blobs `blobs`.
fn facts(version: &str, hash: Sha, blobs: &[Sha]) -> Facts {
    let file = Listed { path: "samples/kick.wav".into(), size: 100, sha256: blobs[0] };
    let mut sorted = blobs.to_vec();
    sorted.sort();
    Facts {
        format: FORMAT,
        name: PackName::new("@alice/drums").unwrap(),
        version: Version::parse(version).unwrap(),
        hash,
        about: About { description: "Drums".into(), license: "CC0-1.0".into(), authors: vec!["Alice".into()], tags: vec!["drums".into()] },
        size: 300,
        files: 3,
        blobs: sorted,
        items: vec![ItemFacts {
            kind: ItemKind::Sample,
            id: "sample:@alice/drums:samples/kick.wav".into(),
            label: "kick".into(),
            category: "Kick".into(),
            description: "A one-shot, 0.42 s, mono".into(),
            tags: vec!["kick".into()],
            component: "@alice/drums:samples/kick.wav".into(),
            instrument: false,
            effect: false,
            preset: None,
            sample: Some(SampleFacts {
                kind: SampleKind::OneShot,
                instrument: Some("kick".into()),
                bpm: None,
                key: None,
                seconds: 0.42,
                channels: 1,
                rate: 48_000,
                peak: 0.891,
            }),
            preview: Some(blobs[0]),
            no_preview: None,
            file: Some(file),
        }],
        made_by: "music2 0.1.0, @m2/core 1.0.0".into(),
    }
}

/// `text` with `field` set to `value` in its JSON, as bytes.
fn with(bytes: &[u8], path: &[&str], value: serde_json::Value) -> Vec<u8> {
    let mut v: serde_json::Value = serde_json::from_slice(bytes).unwrap();
    let mut at = &mut v;
    for p in &path[..path.len() - 1] {
        at = match p.parse::<usize>() {
            Ok(i) => &mut at[i],
            Err(_) => &mut at[*p],
        };
    }
    at[path[path.len() - 1]] = value;
    serde_json::to_vec(&v).unwrap()
}

// REG-3: every file the robot writes is JSON in one form, read back strictly: an unknown field or
// another format is refused, and a format first, so a newer file says to update music2.
#[test]
fn the_buckets_files_are_read_strictly() {
    let f = facts("1.0.0", sha(1), &[sha(2), sha(3)]);
    let bytes = f.bytes();
    assert_eq!(Facts::read(&bytes).unwrap(), f);
    assert_eq!(bytes, crate::jcs::canonical(&serde_json::from_slice(&bytes).unwrap()).into_bytes(), "written in RFC 8785 form");
    let e = Facts::read(&with(&bytes, &["format"], 2.into())).unwrap_err();
    assert!(e.contains("format 2, and this music2 reads format 1: update music2"), "{e}");
    let e = Facts::read(&with(&bytes, &["items", "0", "rating"], 5.into())).unwrap_err();
    assert!(e.contains("rating"), "{e}");
    let e = Facts::read(&with(&bytes, &["blobs"], serde_json::json!([f.blobs[1], f.blobs[0]]))).unwrap_err();
    assert!(e.contains("out of order"), "{e}");

    let reg = read(&drums(&[("1.0.0", sha(1), "")]));
    let (c, _) = Catalog::make(&reg, &|h| (*h == sha(1)).then(|| f.clone()), "2026-10-30T12:00:00Z");
    let bytes = c.bytes();
    assert!(
        Catalog::read(&with(&bytes, &["packs", "0", "versions", "0", "source"], serde_json::Value::Null)).is_err(),
        "a version says its source"
    );
    assert_eq!(Catalog::read(&bytes).unwrap(), c);
    assert!(Catalog::read(&with(&bytes, &["format"], 2.into())).unwrap_err().contains("update music2"));
    assert!(Catalog::read(&with(&bytes, &["packs", "0", "downloads"], 7.into())).unwrap_err().contains("downloads"));
    assert!(Catalog::read(&with(&bytes, &["packs", "0", "items", "0", "sample", "loudness"], 7.into())).unwrap_err().contains("loudness"));
    assert!(Catalog::read(&with(&bytes, &["made"], "yesterday".into())).unwrap_err().contains("not a time"));
    assert!(Catalog::read(&with(&bytes, &["packs", "0", "latest"], "9.9.9".into())).unwrap_err().contains("doesn't offer"));

    let p = c.pointer();
    assert_eq!((p.catalog, p.size, p.made.as_str()), (Sha::of(&bytes), bytes.len() as u64, "2026-10-30T12:00:00Z"));
    assert_eq!(Pointer::read(&p.bytes()).unwrap(), p);
    assert!(Pointer::read(&with(&p.bytes(), &["format"], 3.into())).unwrap_err().contains("catalog.json is format 3"));
    assert!(Pointer::read(&with(&p.bytes(), &["mirror"], "x".into())).unwrap_err().contains("mirror"));
    assert_eq!(Catalog::read_checked(&bytes, &p).unwrap(), c);
    let mut changed = bytes.clone();
    changed[10] ^= 1;
    assert!(Catalog::read_checked(&changed, &p).unwrap_err().contains("changed or cut short"));
    assert!(Catalog::read_checked(&bytes[..bytes.len() - 1], &p).is_err());
}

// REG-4: the catalog is made from the registry and the facts alone: the same inputs, in any
// order, give the same bytes; packs by name, versions oldest first with their sources, the
// latest's items; a yanked version is listed but never the latest, and a taken-down one has no
// size. A version with no facts (not built yet, or its build failed) is left out, and named.
#[test]
fn the_same_registry_and_facts_make_the_same_catalog() {
    let text = drums(&[("1.2.0", sha(3), "yanked = \"a click\"\n"), ("1.0.0", sha(1), ""), ("1.1.0", sha(2), "")]);
    let mut keys = PackFile::read(&text.replace("alice/drums", "alice/keys")).unwrap();
    keys.versions.truncate(1);
    keys.versions[0].hash = sha(9);
    keys.versions[0].yanked = None;
    let reg = taken_down(Registry { packs: vec![PackFile::read(&text).unwrap(), keys], ..Default::default() }, "1.0.0", sha(1));
    let all = [
        (sha(1), facts("1.0.0", sha(1), &[sha(11)])),
        (sha(2), facts("1.1.0", sha(2), &[sha(12)])),
        (sha(3), facts("1.2.0", sha(3), &[sha(13)])),
    ];
    let mut keys_facts = facts("1.2.0", sha(9), &[sha(19)]);
    keys_facts.name = PackName::new("@alice/keys").unwrap();
    let lookup = |h: &Sha| all.iter().find(|(k, _)| k == h).map(|(_, f)| f.clone()).or_else(|| (*h == sha(9)).then(|| keys_facts.clone()));
    let (c, left_out) = Catalog::make(&reg, &lookup, "2026-10-30T12:00:00Z");
    assert!(left_out.is_empty());
    let mut flipped = reg.clone();
    flipped.packs.reverse();
    flipped.packs[1].versions.reverse();
    assert_eq!(Catalog::make(&flipped, &lookup, "2026-10-30T12:00:00Z").0.bytes(), c.bytes(), "the same bytes, whatever the order");

    let drums = c.get(&PackName::new("@alice/drums").unwrap()).unwrap();
    assert_eq!(c.packs.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["@alice/drums", "@alice/keys"]);
    assert_eq!(drums.versions.iter().map(|v| v.version.to_string()).collect::<Vec<_>>(), ["1.0.0", "1.1.0", "1.2.0"]);
    assert_eq!(drums.versions[1].source, "https://github.com/alice/drums/releases/download/v1.1.0/drums-1.1.0.tar.gz");
    assert_eq!(drums.latest, Some(Version(1, 1, 0)), "1.2.0 is yanked");
    assert_eq!(drums.items, all[1].1.items, "the latest's items");
    assert_eq!((drums.license.as_str(), drums.description.as_str()), ("CC0-1.0", "Drums"));
    let removed = drums.get(Version(1, 0, 0)).unwrap();
    assert_eq!((removed.size, removed.files, removed.removed.as_deref()), (None, None, Some("not ours to share")));
    assert_eq!(drums.get(Version(1, 2, 0)).unwrap().size, Some(300), "a yanked version can still be got");

    let failed = |h: &Sha| (*h != sha(2) && *h != sha(9)).then(|| lookup(h)).flatten();
    let (c, left_out) = Catalog::make(&reg, &failed, "2026-10-30T12:00:00Z");
    assert_eq!(left_out, ["@alice/drums 1.1.0", "@alice/keys 1.2.0"]);
    let drums = c.get(&PackName::new("@alice/drums").unwrap()).unwrap();
    assert_eq!(drums.versions.iter().map(|v| v.version.to_string()).collect::<Vec<_>>(), ["1.0.0", "1.2.0"], "1.1.0 isn't built");
    assert_eq!((drums.latest, drums.items.len()), (None, 0), "the one left is yanked");
    assert!(c.get(&PackName::new("@alice/keys").unwrap()).is_none(), "a pack with no version built isn't in the catalog yet");
    let gone = |h: &Sha| (*h != sha(1)).then(|| lookup(h)).flatten();
    assert!(Catalog::make(&reg, &gone, "2026-10-30T12:00:00Z").1.is_empty(), "a taken-down version's facts may be gone");
}

// REG-5, REG-7: what is published never changes, by anyone: a published version gone, or listed
// with another hash or source, a take-down taken back, or a hash a published version has under
// another name, is refused; yanking and adding versions aren't.
#[test]
fn a_published_version_never_changes() {
    let reg = taken_down(read(&drums(&[("1.0.0", sha(1), ""), ("1.1.0", sha(2), "")])), "1.0.0", sha(1));
    let lookup = |h: &Sha| match h {
        h if *h == sha(1) => Some(facts("1.0.0", sha(1), &[sha(11)])),
        h if *h == sha(2) => Some(facts("1.1.0", sha(2), &[sha(12)])),
        _ => None,
    };
    let (published, _) = Catalog::make(&reg, &lookup, "2026-10-30T12:00:00Z");
    assert_eq!(reg.breaks(&published), Vec::<String>::new());
    let more = taken_down(
        read(&drums(&[("1.0.0", sha(1), ""), ("1.1.0", sha(2), "yanked = \"a click\"\n"), ("1.2.0", sha(3), "")])),
        "1.0.0",
        sha(1),
    );
    assert_eq!(more.breaks(&published), Vec::<String>::new(), "a yank and a new version are fine");

    let changed = taken_down(read(&drums(&[("1.0.0", sha(1), ""), ("1.1.0", sha(8), "")])), "1.0.0", sha(1));
    let e = changed.breaks(&published).join("\n");
    assert!(e.contains("@alice/drums 1.1.0 is published with the hash") && e.contains("a published version never changes (P5)"), "{e}");
    let moved = taken_down(
        read(&drums(&[("1.0.0", sha(1), ""), ("1.1.0", sha(2), "")]).replace("v1.1.0/drums-1.1.0", "v1.1.0/x")),
        "1.0.0",
        sha(1),
    );
    assert!(moved.breaks(&published).join("\n").contains("@alice/drums 1.1.0 is published"), "a source is published too");
    let back = read(&drums(&[("1.0.0", sha(1), ""), ("1.1.0", sha(2), "")]));
    assert!(back.breaks(&published).join("\n").contains("@alice/drums 1.0.0 was taken down, and a take-down is never taken back"));
    let gone = taken_down(read(&drums(&[("1.0.0", sha(1), "")])), "1.0.0", sha(1));
    assert!(gone.breaks(&published).join("\n").contains("@alice/drums 1.1.0 is published, and gone from the registry"));

    let mut claimed = reg.clone();
    claimed.packs.push(PackFile::read(&drums(&[("0.0.1", sha(2), "")]).replace("alice/drums", "mallory/x")).unwrap());
    let e = claimed.breaks(&published).join("\n");
    assert!(e.contains("@mallory/x 0.0.1 lists the hash of the published @alice/drums 1.1.0"), "{e}");
}

// REG-7: the blobs to delete are the taken-down versions' that no other listed version uses; a
// taken-down version with no facts is named, its blobs unknown.
#[test]
fn a_removed_versions_blobs_are_those_no_other_version_uses() {
    let reg = taken_down(
        taken_down(read(&drums(&[("1.0.0", sha(1), ""), ("1.1.0", sha(2), ""), ("1.2.0", sha(3), "")])), "1.0.0", sha(1)),
        "1.2.0",
        sha(3),
    );
    let lookup = |h: &Sha| match h {
        h if *h == sha(1) => Some(facts("1.0.0", sha(1), &[sha(10), sha(11), sha(12)])),
        h if *h == sha(2) => Some(facts("1.1.0", sha(2), &[sha(11), sha(20)])),
        _ => None,
    };
    let (gone, unknown) = reg.removed_blobs(&lookup);
    let mut want = vec![sha(10), sha(12)];
    want.sort();
    assert_eq!(gone, want, "sha(11) is 1.1.0's too");
    assert_eq!(unknown, vec!["@alice/drums 1.2.0"]);
}

// REG-3: a time is written one way: UTC, to the second.
#[test]
fn times_are_utc_to_the_second() {
    assert_eq!(utc(0), "1970-01-01T00:00:00Z");
    assert_eq!(utc(1_793_361_600), "2026-10-30T12:00:00Z");
    assert!(is_time("2026-10-30T12:00:00Z"));
    for bad in ["2026-10-30T12:00:00+01:00", "2026-10-30 12:00:00Z", "2026-13-30T12:00:00Z", "2026-10-30T12:00:00.5Z", ""] {
        assert!(!is_time(bad), "{bad}");
    }
}
