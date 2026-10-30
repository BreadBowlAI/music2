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
// twice, both yanked and removed, or an empty reason is refused, in words.
#[test]
fn a_pack_file_is_read_strictly() {
    let good = drums(&[("1.0.0", sha(1), ""), ("1.1.0", sha(2), "yanked = \"a click at the end of the kick\"\n")]);
    let p = PackFile::read(&good).unwrap();
    assert_eq!(p.name.as_str(), "@alice/drums");
    assert_eq!(p.versions.len(), 2);
    assert_eq!(p.latest().map(|v| v.version), Some(Version(1, 0, 0)), "a yanked version is never the latest");
    assert_eq!(PackFile::read(&p.text()).unwrap(), p, "its own text reads back as itself");

    refused(&format!("{good}colour = \"red\"\n"), "colour");
    refused(&drums(&[("1.0.0", sha(1), "notes = \"x\"\n")]), "notes");
    refused(&good.replace("@alice/drums\"\nrepo", "std\"\nrepo"), "never listed");
    refused("name = \"@alice/drums\"\nrepository = \"https://github.com/alice/drums\"\n", "lists no version");
    refused(&drums(&[("1.0.0", sha(1), ""), ("1.0.0", sha(2), "")]), "listed twice");
    refused(&drums(&[("1.0.0", sha(1), ""), ("1.1.0", sha(1), "")]), "never the same files");
    refused(&drums(&[("1.0.0", sha(1), "yanked = \"x\"\nremoved = \"y\"\n")]), "both yanked and removed");
    refused(&drums(&[("1.0.0", sha(1), "removed = \" \"\n")]), "can't be empty");
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

    std::fs::write(d.join(TAGS), "drums\nLo-Fi\n").unwrap();
    assert!(Registry::read(&d).unwrap_err().join("\n").contains("tags.txt:2: `Lo-Fi` isn't a tag"));
    std::fs::remove_file(d.join(TAGS)).unwrap();
    assert!(Registry::read(&d).unwrap_err().join("\n").contains("tags.txt can't be read"));
}

fn read(text: &str) -> Registry {
    Registry { packs: vec![PackFile::read(text).unwrap()], tags: Default::default() }
}

// REG-2: a listed version never changes (P5): a pack file or a version gone, or a hash or source
// changed, breaks the promise; yanking, removing and adding a version don't.
#[test]
fn a_listed_version_never_changes() {
    let base = read(&drums(&[("1.0.0", sha(1), ""), ("1.1.0", sha(2), "")]));
    let fine = read(&drums(&[
        ("1.0.0", sha(1), "removed = \"not ours to share\"\n"),
        ("1.1.0", sha(2), "yanked = \"a click\"\n"),
        ("1.2.0", sha(3), ""),
    ]));
    assert_eq!(fine.broken_promises(&base), Vec::<String>::new());
    assert_eq!(
        base.broken_promises(&fine),
        vec!["@alice/drums 1.2.0 is gone from its file: a listed version stays listed. Mark it `yanked` or `removed` instead"]
    );

    let edited = read(&drums(&[("1.0.0", sha(9), ""), ("1.1.0", sha(2), "")]));
    assert!(edited.broken_promises(&base)[0].contains("@alice/drums 1.0.0 was changed"));
    let moved = read(&drums(&[("1.0.0", sha(1), ""), ("1.1.0", sha(2), "")]).replace("v1.1.0/drums-1.1.0", "v1.1.0/other"));
    assert!(moved.broken_promises(&base)[0].contains("1.1.0 was changed"), "a new source is a change too");
    let gone = Registry::default();
    assert!(gone.broken_promises(&base)[0].contains("packs/@alice/drums.toml is gone from the registry"));
}

// REG-2: an author may change only packs of their own scope, in any capitals, and never @m2/'s;
// what isn't changed from the base isn't theirs to answer for.
#[test]
fn an_author_changes_only_their_own_scope() {
    let alice = drums(&[("1.0.0", sha(1), "")]);
    let bob = alice.replace("alice", "bob");
    let ours = "name = \"@m2/kit\"\nrepository = \"https://github.com/BreadBowlAI/m2-kit\"\n\n[[versions]]\nversion = \"0.1.0\"\n\
                hash = \"sha256:{h}\"\nsource = \"https://github.com/BreadBowlAI/m2-kit/releases/download/v0.1.0/kit.tar.gz\"\n"
        .replace("{h}", &sha(5).hex());
    let base = Registry { packs: vec![PackFile::read(&bob).unwrap()], tags: Default::default() };
    let mut now = base.clone();
    now.packs.push(PackFile::read(&alice).unwrap());
    assert_eq!(now.not_authors(Some(&base), "Alice"), Vec::<String>::new(), "bob's file is as it was");
    assert_eq!(now.not_authors(Some(&base), "mallory"), vec!["@alice/drums is @alice's: mallory may change only packs named @mallory/…"]);
    assert_eq!(now.not_authors(None, "alice").len(), 1, "with no base every pack is checked: bob's too");
    now.packs.push(PackFile::read(&ours).unwrap());
    assert_eq!(
        now.not_authors(Some(&base), "m2"),
        vec![
            "@alice/drums is @alice's: m2 may change only packs named @m2/…".to_string(),
            "@m2/kit is music2's own (@m2/): only music2's people change it".to_string(),
        ]
    );
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
    let c = Catalog::make(&reg, &|h| (*h == sha(1)).then(|| f.clone()), "2026-10-30T12:00:00Z").unwrap();
    let bytes = c.bytes();
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
// order, give the same bytes; packs by name, versions oldest first, the latest's items; a yanked
// version is listed but never the latest, and a removed one has no size.
#[test]
fn the_same_registry_and_facts_make_the_same_catalog() {
    let text = drums(&[("1.2.0", sha(3), "yanked = \"a click\"\n"), ("1.0.0", sha(1), "removed = \"not ours\"\n"), ("1.1.0", sha(2), "")]);
    let mut keys = PackFile::read(&text.replace("alice/drums", "alice/keys")).unwrap();
    keys.versions.truncate(1);
    keys.versions[0].hash = sha(9);
    keys.versions[0].yanked = None;
    let reg = Registry { packs: vec![PackFile::read(&text).unwrap(), keys], tags: Default::default() };
    let all = [
        (sha(1), facts("1.0.0", sha(1), &[sha(11)])),
        (sha(2), facts("1.1.0", sha(2), &[sha(12)])),
        (sha(3), facts("1.2.0", sha(3), &[sha(13)])),
    ];
    let mut keys_facts = facts("1.2.0", sha(9), &[sha(19)]);
    keys_facts.name = PackName::new("@alice/keys").unwrap();
    let lookup = |h: &Sha| all.iter().find(|(k, _)| k == h).map(|(_, f)| f.clone()).or_else(|| (*h == sha(9)).then(|| keys_facts.clone()));
    let c = Catalog::make(&reg, &lookup, "2026-10-30T12:00:00Z").unwrap();
    let mut flipped = reg.clone();
    flipped.packs.reverse();
    flipped.packs[1].versions.reverse();
    assert_eq!(Catalog::make(&flipped, &lookup, "2026-10-30T12:00:00Z").unwrap().bytes(), c.bytes(), "the same bytes, whatever the order");

    let drums = c.get(&PackName::new("@alice/drums").unwrap()).unwrap();
    assert_eq!(c.packs.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["@alice/drums", "@alice/keys"]);
    assert_eq!(drums.versions.iter().map(|v| v.version.to_string()).collect::<Vec<_>>(), ["1.0.0", "1.1.0", "1.2.0"]);
    assert_eq!(drums.latest, Some(Version(1, 1, 0)), "1.2.0 is yanked");
    assert_eq!(drums.items, all[1].1.items, "the latest's items");
    assert_eq!((drums.license.as_str(), drums.description.as_str()), ("CC0-1.0", "Drums"));
    let removed = drums.get(Version(1, 0, 0)).unwrap();
    assert_eq!((removed.size, removed.files, removed.removed.as_deref()), (None, None, Some("not ours")));
    assert_eq!(drums.get(Version(1, 2, 0)).unwrap().size, Some(300), "a yanked version can still be got");

    let none = |h: &Sha| (*h != sha(2)).then(|| lookup(h)).flatten();
    let e = Catalog::make(&reg, &none, "2026-10-30T12:00:00Z").unwrap_err();
    assert_eq!(e, vec![format!("@alice/drums 1.1.0 has no facts ({}): build it first", facts_path(&sha(2)))]);
    let gone = |h: &Sha| (*h != sha(1)).then(|| lookup(h)).flatten();
    Catalog::make(&reg, &gone, "2026-10-30T12:00:00Z").expect("a removed version's facts may be gone");
}

// REG-7: the blobs to delete are the removed versions' that no version still listed uses; a
// removed version with no facts is named, its blobs unknown.
#[test]
fn a_removed_versions_blobs_are_those_no_other_version_uses() {
    let reg =
        read(&drums(&[("1.0.0", sha(1), "removed = \"not ours\"\n"), ("1.1.0", sha(2), ""), ("1.2.0", sha(3), "removed = \"also\"\n")]));
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
