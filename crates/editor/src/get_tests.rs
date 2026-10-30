//! The registry in the app (REG-11 to REG-17), against a bucket the robot built from two made-up
//! packs (the robot's own test world: `@alice/drums`, one sample; `@bob/keys`, an instrument and a
//! sound), served as `file://` and over http on this computer. No test reaches the internet.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use m2_song::pack::{Listing, Lock, PackName, Store};
use m2_song::registry::{Catalog, POINTER, Pointer, blob_path, catalog_path};
use m2_song::{Sha, Version};
use serde_json::json;

use super::*;
use crate::registry::tests::{Temp, World, catalog_in, std_dir, tree, write};

/// A bucket built from the robot's test world, in `<world>/bucket`.
fn built(tag: &str) -> (World, PathBuf) {
    let w = World::new(tag);
    let out = w.t.0.join("bucket");
    w.build(&out, None).unwrap_or_else(|e| panic!("{}", e.join("\n")));
    (w, out)
}

/// A folder served over http on 127.0.0.1, each request on a connection of its own; the paths
/// asked for are kept. A path starting with `hold` is answered with half its bytes, then nothing
/// until the client goes.
struct Serve {
    url: String,
    asked: Arc<Mutex<Vec<String>>>,
    hold: Arc<Mutex<Option<String>>>,
}

impl Serve {
    fn folder(dir: &Path) -> Serve {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let asked = Arc::new(Mutex::new(vec![]));
        let hold: Arc<Mutex<Option<String>>> = Arc::default();
        let (dir, kept, held) = (dir.to_path_buf(), asked.clone(), hold.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { return };
                let (dir, kept, held) = (dir.clone(), kept.clone(), held.clone());
                std::thread::spawn(move || {
                    s.set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
                    let mut head = vec![];
                    let mut b = [0u8; 1];
                    while !head.ends_with(b"\r\n\r\n") && s.read(&mut b).is_ok_and(|n| n == 1) {
                        head.push(b[0]);
                    }
                    let head = String::from_utf8_lossy(&head).to_string();
                    let path = head.split(' ').nth(1).unwrap_or("/").trim_start_matches('/').to_string();
                    kept.lock().unwrap().push(path.clone());
                    let Ok(body) = std::fs::read(dir.join(&path)) else {
                        let _ = s.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                        return;
                    };
                    let _ = s.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).as_bytes());
                    if held.lock().unwrap().as_ref().is_some_and(|h| path.starts_with(h.as_str())) {
                        let _ = s.write_all(&body[..body.len() / 2]);
                        let _ = s.flush();
                        let _ = s.read(&mut [0u8; 1]);
                        return;
                    }
                    let _ = s.write_all(&body);
                });
            }
        });
        Serve { url, asked, hold }
    }

    /// The paths asked for since the last call.
    fn asked(&self) -> Vec<String> {
        std::mem::take(&mut *self.asked.lock().unwrap())
    }
}

fn name(s: &str) -> PackName {
    PackName::new(s).unwrap()
}

/// The bucket in `dir` as a `file://` address.
fn file_url(dir: &Path) -> String {
    crate::fetch::file_address(dir).unwrap()
}

/// What is under `dir`, by path: none when it isn't there.
fn files(dir: &Path) -> Vec<String> {
    tree(dir).into_keys().collect()
}

/// The folders a Get leaves beside the store: none, when it is done.
fn getting(store: &Store) -> Vec<String> {
    std::fs::read_dir(&store.dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(".getting"))
        .collect()
}

// REG-11: the environment's registry, else the settings', else the default, which is empty until
// the owner picks the bucket's address (D10); an empty one is none.
#[test]
fn the_registry_is_the_one_the_environment_or_the_settings_name() {
    assert_eq!(address(Some("https://a.example"), Some("https://b.example")).as_deref(), Some("https://a.example"));
    assert_eq!(address(None, Some("https://b.example")).as_deref(), Some("https://b.example"));
    assert_eq!(address(Some(" "), Some(" https://b.example ")).as_deref(), Some("https://b.example"));
    assert_eq!(DEFAULT_REGISTRY, "", "no default until the owner picks the bucket's address");
    assert_eq!(address(None, None), None);
    assert_eq!(address(Some(""), Some("")), None);
    // An address music2 doesn't download from is refused in words; a folder is a file:// one.
    let t = Temp::new("get-address");
    assert!(Remote::new("http://packs.example", &t.0).unwrap_err().contains("only over https"));
    assert!(Remote::new("ftp://packs.example", &t.0).is_err());
    assert!(Remote::new("bucket", &t.0).unwrap_err().contains("whole path"));
    let folder = Remote::new(t.0.join("my bucket").to_str().unwrap(), &t.0).unwrap();
    assert!(folder.at.starts_with("file:///") && folder.at.ends_with("my%20bucket"), "{}", folder.at);
    assert_eq!(Remote::new("https://packs.example/", &t.0).unwrap().at, "https://packs.example");
}

// REG-12: catalog.json, then the catalog it names, checked and kept in the cache; asked again,
// only catalog.json is fetched, as the catalog is kept. The same from a folder (file://).
#[test]
fn the_catalog_is_fetched_checked_and_kept() {
    let (w, bucket) = built("get-catalog");
    let serve = Serve::folder(&bucket);
    let cache = w.t.0.join("cache");
    let remote = Remote::new(&serve.url, &cache).unwrap();
    assert!(remote.kept().is_none(), "nothing kept yet");
    let got = remote.fetch(&AtomicBool::new(false)).unwrap();
    let pointer = Pointer::read(&std::fs::read(bucket.join(POINTER)).unwrap()).unwrap();
    assert_eq!(serve.asked(), [POINTER.to_string(), catalog_path(&pointer.catalog)]);
    assert_eq!(*got.catalog, catalog_in(&tree(&bucket)));
    let kept = remote.kept().expect("kept");
    assert_eq!(kept.catalog, got.catalog);
    let key = &Sha::of(remote.at.as_bytes()).hex()[..16];
    assert_eq!(files(&cache.join(key)), [POINTER.to_string(), catalog_path(&pointer.catalog)], "kept by the address's short hash");
    remote.fetch(&AtomicBool::new(false)).unwrap();
    assert_eq!(serve.asked(), [POINTER], "the catalog is kept: only catalog.json again");

    let from_folder = Remote::new(&file_url(&bucket), &cache).unwrap();
    assert_eq!(*from_folder.fetch(&AtomicBool::new(false)).unwrap().catalog, *got.catalog);
    assert!(serve.asked().is_empty());
}

// REG-12: `m2` with the app closed fetches only a catalog a day old; offline, the one kept.
#[test]
fn a_catalog_is_fetched_again_only_when_a_day_old() {
    let (w, bucket) = built("get-fresh");
    let serve = Serve::folder(&bucket);
    let remote = Remote::new(&serve.url, &w.t.0.join("cache")).unwrap();
    let (first, why) = remote.current(FRESH);
    assert!(first.is_some() && why.is_none(), "{why:?}");
    assert_eq!(serve.asked().len(), 2);
    let (again, _) = remote.current(FRESH);
    assert!(again.is_some());
    assert!(serve.asked().is_empty(), "fetched within a day: nothing asked");
    remote.current(std::time::Duration::ZERO);
    assert_eq!(serve.asked(), [POINTER], "older than asked: catalog.json again");
}

// REG-12: a catalog.json that doesn't read, or a catalog that isn't the one it names (changed on
// the way, or by the host), is never used and never replaces the one kept.
#[test]
fn a_broken_catalog_never_replaces_a_good_one() {
    let (w, bucket) = built("get-broken");
    let remote = Remote::new(&file_url(&bucket), &w.t.0.join("cache")).unwrap();
    let good = remote.fetch(&AtomicBool::new(false)).unwrap().catalog;
    // A newer catalog, changed after it was named.
    let mut newer = (*good).clone();
    newer.made = "2026-10-31T12:00:00Z".into();
    let pointer = newer.pointer();
    let mut bytes = newer.bytes();
    let at = bytes.windows(9).position(|w| w == b"Soft keys").unwrap();
    bytes[at..at + 4].copy_from_slice(b"Hard");
    write(&bucket.join(catalog_path(&pointer.catalog)), &bytes);
    write(&bucket.join(POINTER), pointer.bytes());
    let why = remote.fetch(&AtomicBool::new(false)).unwrap_err();
    assert!(why.contains("changed"), "{why}");
    assert_eq!(remote.kept().unwrap().catalog, good, "the good one stays");
    write(&bucket.join(POINTER), b"{\"format\": 1}");
    assert!(remote.fetch(&AtomicBool::new(false)).is_err());
    assert_eq!(remote.kept().unwrap().catalog, good, "the good one stays");
    // Once a good one is named, it is taken.
    write(&bucket.join(catalog_path(&pointer.catalog)), newer.bytes());
    write(&bucket.join(POINTER), pointer.bytes());
    assert_eq!(remote.fetch(&AtomicBool::new(false)).unwrap().catalog.made, "2026-10-31T12:00:00Z");
    assert_eq!(remote.kept().unwrap().catalog.made, "2026-10-31T12:00:00Z");
}

// REG-12: offline (the bucket can't be reached) the catalog kept is used, saying why.
#[test]
fn offline_the_kept_catalog_is_used() {
    let (w, bucket) = built("get-offline");
    let remote = Remote::new(&file_url(&bucket), &w.t.0.join("cache")).unwrap();
    let good = remote.fetch(&AtomicBool::new(false)).unwrap().catalog;
    std::fs::rename(&bucket, w.t.0.join("gone")).unwrap();
    assert!(remote.fetch(&AtomicBool::new(false)).is_err());
    let (kept, why) = remote.current(std::time::Duration::ZERO);
    assert_eq!(kept.map(|k| k.catalog), Some(good));
    assert!(why.is_some_and(|w| !w.is_empty()));
}

// REG-13: From the library is the latest's items of each pack this computer hasn't got, or has
// an older version of; each with its id under `library:`, found and ranked by the one search.
#[test]
fn from_the_library_is_what_you_havent_got() {
    let (_w, bucket) = built("get-offers");
    let catalog = catalog_in(&tree(&bucket));
    let all = offers(&catalog, &|_| None);
    let ids: Vec<&str> = all.iter().map(|o| o.item.id.as_str()).collect();
    assert_eq!(
        ids,
        ["library:sample:@alice/drums:samples/kicks/Boom.wav", "library:@bob/keys:soft-keys", "library:sound:std/@bob/keys:soft-keys/Dark"]
    );
    let keys = all.iter().find(|o| o.item.id == "library:@bob/keys:soft-keys").unwrap();
    assert_eq!(
        (keys.item.kind, keys.item.label.as_str(), keys.item.package.as_str(), keys.item.component.as_str(), keys.item.preview),
        (crate::library::Kind::Instrument, "Soft keys", "@bob/keys", "@bob/keys:soft-keys", true)
    );
    let v = catalog.get(&name("@bob/keys")).unwrap().get(Version(0, 1, 0)).unwrap();
    assert_eq!(keys.size(), v.size.unwrap(), "an instrument comes with its whole version");
    let j = keys.json();
    assert_eq!(j["id"], json!("library:@bob/keys:soft-keys"));
    assert_eq!(
        j["library"],
        json!({"pack": "@bob/keys", "version": "0.1.0", "license": "MIT", "authors": ["Bob"], "size": v.size, "have": null})
    );
    let kick = &all[0];
    let file = kick.facts.file.as_ref().unwrap();
    assert_eq!(kick.size(), file.size, "a sample comes alone");
    let s = kick.item.sample.as_ref().unwrap();
    assert_eq!((s.kind, s.instrument.as_deref(), s.hash.as_str()), (crate::library::SampleType::OneShot, Some("kick"), file.sha256.hex().as_str()));
    assert_eq!(
        s.from,
        json!({"pack": "@alice/drums", "version": "1.0.0", "file": "samples/kicks/Boom.wav", "author": "Alice", "license": "CC0-1.0"})
    );
    assert_eq!(kick.json()["sample"]["from"]["pack"], json!("@alice/drums"));
    let dark = all.iter().find(|o| o.item.kind == crate::library::Kind::Sound).unwrap();
    assert_eq!(dark.item.preset.as_ref().map(|p| (p.from.as_str(), p.name.as_str())), Some(("std", "Dark")));

    // What this computer has: none of a pack it has, but of one it has an older version of.
    let had = offers(&catalog, &|n| (n.as_str() == "@bob/keys").then_some(Version(0, 1, 0)));
    assert!(had.iter().all(|o| o.pack.as_str() == "@alice/drums"), "you have @bob/keys 0.1.0");
    let older = offers(&catalog, &|n| (n.as_str() == "@bob/keys").then_some(Version(0, 0, 9)));
    assert_eq!(older.iter().filter(|o| o.have == Some(Version(0, 0, 9))).count(), 2, "a newer version is offered, saying what you have");

    let found: Vec<&str> = find(&all, &Query::parse("soft")).into_iter().map(|o| o.item.id.as_str()).collect();
    assert_eq!(found, ["library:@bob/keys:soft-keys", "library:sound:std/@bob/keys:soft-keys/Dark"]);
    let found: Vec<&str> = find(&all, &Query::parse("kick one-shot")).into_iter().map(|o| o.item.id.as_str()).collect();
    assert_eq!(found, ["library:sample:@alice/drums:samples/kicks/Boom.wav"]);
}

// REG-14: a preview is fetched by its hash and kept by it, so a second play fetches nothing; a kept
// one that no longer matches is fetched again, and a blob changed in the bucket is refused.
#[test]
fn a_preview_is_fetched_by_its_hash_and_kept() {
    let (w, bucket) = built("get-preview");
    let serve = Serve::folder(&bucket);
    let remote = Remote::new(&serve.url, &w.t.0.join("cache")).unwrap();
    let catalog = catalog_in(&tree(&bucket));
    let keys = catalog.get(&name("@bob/keys")).unwrap();
    let sha = keys.items[0].preview.unwrap();
    let no = AtomicBool::new(false);
    let file = remote.blob(sha, 64 << 20, &no).unwrap();
    assert_eq!(std::fs::read(&file).unwrap(), std::fs::read(bucket.join(blob_path(&sha))).unwrap());
    assert_eq!(serve.asked(), [blob_path(&sha)]);
    assert_eq!(remote.blob(sha, 64 << 20, &no).unwrap(), file);
    assert!(serve.asked().is_empty(), "kept: nothing fetched");
    std::fs::write(&file, b"not a preview").unwrap();
    assert_eq!(Sha::of_file(&remote.blob(sha, 64 << 20, &no).unwrap()).unwrap(), sha, "fetched again");
    assert_eq!(serve.asked(), [blob_path(&sha)]);
    std::fs::remove_file(&file).unwrap();
    std::fs::write(bucket.join(blob_path(&sha)), b"changed").unwrap();
    assert!(remote.blob(sha, 64 << 20, &no).unwrap_err().contains("didn't match its hash"));
    assert!(!file.exists(), "nothing kept");
}

/// The catalog of `bucket`, and its version `version` of `pack`.
fn version_of(bucket: &Path, pack: &str, version: Version) -> m2_song::registry::CatalogVersion {
    catalog_in(&tree(bucket)).get(&name(pack)).unwrap().get(version).unwrap().clone()
}

// REG-15: Get fetches m2pack.json by the version's hash, then each file by its hash, and installs
// the version checked, as `m2 pack install` does: its hash is the listed one, its progress ends at
// the version's size, and no folder of its own is left.
#[test]
fn get_installs_a_whole_version_checked() {
    let (w, bucket) = built("get-pack");
    let serve = Serve::folder(&bucket);
    let remote = Remote::new(&serve.url, &w.t.0.join("cache")).unwrap();
    let store = Store::at(w.t.0.join("Packs"));
    let v = version_of(&bucket, "@bob/keys", Version(0, 1, 0));
    let mut seen = vec![];
    let done = pack(&remote, &name("@bob/keys"), &v, &store, &std_dir(), &mut |p| seen.push(p), &AtomicBool::new(false)).unwrap();
    assert!(matches!(done, Install::Installed { hash, .. } if hash == v.hash), "{done:?}");
    assert_eq!(store.check(&name("@bob/keys"), &Version(0, 1, 0)).unwrap(), v.hash);
    let last = seen.last().unwrap();
    assert_eq!((last.done, last.total), (v.size.unwrap(), v.size.unwrap()));
    assert!(seen.windows(2).all(|p| p[0].done <= p[1].done));
    let asked = serve.asked();
    assert_eq!(asked[0], blob_path(&v.hash), "its m2pack.json first");
    assert_eq!(asked.len() as u64, v.files.unwrap(), "each file once");
    assert!(getting(&store).is_empty());
    // Got again: it is there with these very files.
    let again = pack(&remote, &name("@bob/keys"), &v, &store, &std_dir(), &mut |_| {}, &AtomicBool::new(false)).unwrap();
    assert!(matches!(again, Install::Already { .. }), "{again:?}");
}

// REG-15: a blob changed in the bucket makes Get fail, saying why, and leaves nothing behind.
#[test]
fn a_changed_blob_fails_get_and_leaves_nothing() {
    let (w, bucket) = built("get-changed");
    let remote = Remote::new(&file_url(&bucket), &w.t.0.join("cache")).unwrap();
    let store = Store::at(w.t.0.join("Packs"));
    let v = version_of(&bucket, "@bob/keys", Version(0, 1, 0));
    let listing = Listing::of(&w.keys()).unwrap();
    let index = listing.files.iter().find(|f| f.path == "src/index.ts").unwrap();
    let blob = bucket.join(blob_path(&index.sha256));
    let text = std::fs::read_to_string(&blob).unwrap();
    std::fs::write(&blob, text.replace("1800", "1900")).unwrap();
    let why = pack(&remote, &name("@bob/keys"), &v, &store, &std_dir(), &mut |_| {}, &AtomicBool::new(false)).unwrap_err();
    assert!(why.contains("didn't match its hash") && why.contains("@bob/keys 0.1.0"), "{why}");
    assert!(store.versions(&name("@bob/keys")).is_empty(), "nothing installed");
    assert!(files(&store.dir).is_empty() && getting(&store).is_empty(), "nothing left: {:?}", files(&store.dir));
}

// REG-15: a cancel stops a Get, and leaves nothing behind; over http too, while a file comes.
#[test]
fn a_cancelled_get_leaves_nothing() {
    let (w, bucket) = built("get-cancel");
    let store = Store::at(w.t.0.join("Packs"));
    let v = version_of(&bucket, "@bob/keys", Version(0, 1, 0));
    let remote = Remote::new(&file_url(&bucket), &w.t.0.join("cache")).unwrap();
    let cancel = AtomicBool::new(false);
    let why = pack(&remote, &name("@bob/keys"), &v, &store, &std_dir(), &mut |_| cancel.store(true, Ordering::Relaxed), &cancel).unwrap_err();
    assert!(why.contains("cancelled"), "{why}");
    assert!(store.versions(&name("@bob/keys")).is_empty() && getting(&store).is_empty());

    let serve = Serve::folder(&bucket);
    let remote = Remote::new(&serve.url, &w.t.0.join("cache")).unwrap();
    let index = Listing::of(&w.keys()).unwrap().files.iter().find(|f| f.path == "src/index.ts").unwrap().sha256;
    *serve.hold.lock().unwrap() = Some(blob_path(&index));
    let cancel = Arc::new(AtomicBool::new(false));
    let later = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        later.store(true, Ordering::Relaxed);
    });
    let t = std::time::Instant::now();
    let why = pack(&remote, &name("@bob/keys"), &v, &store, &std_dir(), &mut |_| {}, &cancel).unwrap_err();
    assert!(why.contains("cancelled") && t.elapsed() < std::time::Duration::from_secs(5), "{why} in {:?}", t.elapsed());
    assert!(store.versions(&name("@bob/keys")).is_empty() && getting(&store).is_empty());
}

/// A bucket in `dir` holding a version made by hand: `files` (path, bytes) and its `m2pack.json`
/// listing them as `listed` (its paths as given), each blob by its hash. The catalog's version of it.
fn by_hand(dir: &Path, files: &[(&str, &[u8])], listed: &[&str]) -> m2_song::registry::CatalogVersion {
    let mut listing = Listing { files: vec![] };
    for ((_, bytes), path) in files.iter().zip(listed) {
        let sha = Sha::of(bytes);
        write(&dir.join(blob_path(&sha)), bytes);
        listing.files.push(m2_song::pack::Listed { path: path.to_string(), size: bytes.len() as u64, sha256: sha });
    }
    let text = listing.text();
    write(&dir.join(blob_path(&listing.hash())), &text);
    m2_song::registry::CatalogVersion {
        version: Version(0, 1, 0),
        hash: listing.hash(),
        source: String::new(),
        size: Some(files.iter().map(|(_, b)| b.len() as u64).sum::<u64>() + text.len() as u64),
        files: Some(files.len() as u64 + 1),
        yanked: None,
        removed: None,
    }
}

const TINY: &[u8] = b"[package]\nname = \"@alice/tiny\"\nversion = \"0.1.0\"\nlicense = \"CC0-1.0\"\n";

// REG-15: a listing whose path leaves the Get's folder (or is no plain path) is refused before
// any file is fetched, and nothing is written anywhere.
#[test]
fn a_listing_that_leaves_its_folder_is_refused() {
    let t = Temp::new("get-escape");
    let store = Store::at(t.0.join("Packs"));
    for bad in ["../escape.txt", "/etc/escape.txt", "a//b.txt", "a\\b.txt", "./a.txt", ".hidden.txt"] {
        let bucket = t.0.join(format!("bucket-{}", Sha::of(bad.as_bytes()).hex()));
        let v = by_hand(&bucket, &[("m2pkg.toml", TINY), ("x", b"escaped")], &["m2pkg.toml", bad]);
        let remote = Remote::new(&file_url(&bucket), &t.0.join("cache")).unwrap();
        let why = pack(&remote, &name("@alice/tiny"), &v, &store, &std_dir(), &mut |_| {}, &AtomicBool::new(false)).unwrap_err();
        assert!(why.contains("isn't a plain path"), "{bad}: {why}");
        assert!(!t.0.join("escape.txt").exists() && files(&store.dir).is_empty() && getting(&store).is_empty(), "{bad}");
    }
}

// REG-15: a version whose m2pkg.toml needs more than this app has stops the Get once that file
// is in, saying to update music2: nothing else is fetched (its other file isn't in the bucket).
#[test]
fn a_pack_for_a_newer_music2_stops_before_its_files() {
    let t = Temp::new("get-newer");
    let bucket = t.0.join("bucket");
    let manifest = b"[package]\nname = \"@alice/tiny\"\nversion = \"0.1.0\"\nlicense = \"CC0-1.0\"\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"9.0\"\n";
    let v = by_hand(&bucket, &[("m2pkg.toml", manifest), ("src/index.ts", b"export {};\n")], &["m2pkg.toml", "src/index.ts"]);
    std::fs::remove_file(bucket.join(blob_path(&Sha::of(b"export {};\n")))).unwrap();
    let remote = Remote::new(&file_url(&bucket), &t.0.join("cache")).unwrap();
    let store = Store::at(t.0.join("Packs"));
    let why = pack(&remote, &name("@alice/tiny"), &v, &store, &std_dir(), &mut |_| {}, &AtomicBool::new(false)).unwrap_err();
    assert!(why.contains("newer music2"), "{why}");
    assert!(files(&store.dir).is_empty() && getting(&store).is_empty());
    // Another pack's m2pkg.toml under this version's hash: refused too.
    let other = t.0.join("other");
    let v = by_hand(&other, &[("m2pkg.toml", TINY)], &["m2pkg.toml"]);
    let remote = Remote::new(&file_url(&other), &t.0.join("cache")).unwrap();
    let why = pack(&remote, &name("@alice/other"), &v, &store, &std_dir(), &mut |_| {}, &AtomicBool::new(false)).unwrap_err();
    assert!(why.contains("@alice/tiny 0.1.0") && why.contains("not @alice/other"), "{why}");
}

// REG-15: the folder a Get that crashed left beside the store is removed by the next Get; one of
// a Get still running (this process's) is not.
#[test]
fn a_crashed_gets_folder_is_removed() {
    let (w, bucket) = built("get-crashed");
    let store = Store::at(w.t.0.join("Packs"));
    let dead = store.dir.join(".getting-4000000000-0");
    let ours = store.dir.join(format!(".getting-{}-999999", std::process::id()));
    write(&dead.join("src/index.ts"), "half");
    write(&ours.join("src/index.ts"), "still coming");
    let remote = Remote::new(&file_url(&bucket), &w.t.0.join("cache")).unwrap();
    let v = version_of(&bucket, "@alice/drums", Version(1, 0, 0));
    pack(&remote, &name("@alice/drums"), &v, &store, &std_dir(), &mut |_| {}, &AtomicBool::new(false)).unwrap();
    assert!(!dead.exists(), "a crashed Get's folder goes");
    assert!(ours.exists(), "a running Get's stays");
}

// REG-16: Get of a sample fetches its one file, by its hash, under its own name, with its note;
// one kept as a preview is copied from the cache.
#[test]
fn get_a_sample_fetches_its_one_file() {
    let (w, bucket) = built("get-sample");
    let serve = Serve::folder(&bucket);
    let remote = Remote::new(&serve.url, &w.t.0.join("cache")).unwrap();
    let catalog = catalog_in(&tree(&bucket));
    let kick = offers(&catalog, &|_| None).into_iter().find(|o| o.item.kind == crate::library::Kind::Sample).unwrap();
    let into = w.t.0.join("into");
    std::fs::create_dir_all(&into).unwrap();
    let mut seen = vec![];
    let s = sample(&remote, &kick, &into, &mut |p| seen.push(p), &AtomicBool::new(false)).unwrap();
    assert_eq!(s.file, into.join("Boom.wav"));
    assert_eq!(std::fs::read(&s.file).unwrap(), std::fs::read(w.drums().join("samples/kicks/Boom.wav")).unwrap());
    assert_eq!(s.from["pack"], json!("@alice/drums"));
    let file = kick.facts.file.clone().unwrap();
    assert_eq!(serve.asked(), [blob_path(&file.sha256)], "its one file, nothing else");
    assert_eq!(seen.last().map(|p| (p.done, p.total)), Some((file.size, file.size)));
    // Played first, it is kept: getting it fetches nothing.
    remote.blob(file.sha256, 64 << 20, &AtomicBool::new(false)).unwrap();
    serve.asked();
    std::fs::remove_file(&s.file).unwrap();
    sample(&remote, &kick, &into, &mut |_| {}, &AtomicBool::new(false)).unwrap();
    assert!(serve.asked().is_empty());
}

// REG-17: a song's locked versions that aren't installed, each gettable when the catalog lists it
// with that very hash: yanked too; a taken-down one, another hash and an unlisted pack say why.
#[test]
fn missing_locked_versions_are_got_by_their_hash() {
    let (w, bucket) = built("get-missing");
    let mut catalog: Catalog = catalog_in(&tree(&bucket));
    let store = Store::at(w.t.0.join("Packs"));
    let (keys, drums) = (w.hash("@bob/keys", "0.1.0"), w.hash("@alice/drums", "1.0.0"));
    let lock = |entries: &[(&str, &str, Sha)]| {
        let mut l = Lock::default();
        for (n, v, h) in entries {
            l = l.with(name(n), Version::parse(v).unwrap(), *h);
        }
        l
    };
    let all = missing(&lock(&[("@bob/keys", "0.1.0", keys), ("@alice/drums", "1.0.0", drums)]), &store, Some(&catalog));
    assert_eq!(all.len(), 2);
    assert!(all.iter().all(|m| m.got.as_ref().is_ok_and(|v| v.hash == m.hash)), "{all:?}");
    // Installed: not missing.
    crate::packs::install(&w.drums(), &store, &std_dir()).unwrap();
    let left = missing(&lock(&[("@bob/keys", "0.1.0", keys), ("@alice/drums", "1.0.0", drums)]), &store, Some(&catalog));
    assert_eq!(left.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["@bob/keys"]);
    let one = |l: &Lock, c: &Catalog| missing(l, &store, Some(c)).remove(0).got;
    let other = one(&lock(&[("@bob/keys", "0.1.0", Sha::of(b"other"))]), &catalog).unwrap_err();
    assert!(other.contains("other files"), "{other}");
    let unlisted = one(&lock(&[("@carol/none", "1.0.0", keys)]), &catalog).unwrap_err();
    assert!(unlisted.contains("isn't in the library"), "{unlisted}");
    assert!(missing(&lock(&[("@bob/keys", "0.1.0", keys)]), &store, None)[0].got.as_ref().unwrap_err().contains("library"));
    let edit = |c: &mut Catalog, change: &dyn Fn(&mut m2_song::registry::CatalogVersion)| {
        let p = c.packs.iter_mut().find(|p| p.name.as_str() == "@bob/keys").unwrap();
        p.latest = None;
        change(&mut p.versions[0]);
    };
    edit(&mut catalog, &|v| v.yanked = Some("a bug in its release".into()));
    assert!(one(&lock(&[("@bob/keys", "0.1.0", keys)]), &catalog).is_ok(), "a yanked version is still got for a song that locks it");
    edit(&mut catalog, &|v| {
        v.removed = Some("the recordings weren't the author's".into());
        (v.size, v.files) = (None, None);
    });
    let gone = one(&lock(&[("@bob/keys", "0.1.0", keys)]), &catalog).unwrap_err();
    assert!(gone.contains("taken down") && gone.contains("the recordings weren't the author's"), "{gone}");
}

// REG-17: `m2 pack get <name>` gets the latest, or the version named; a version taken down, or a
// pack not listed, is said in words.
#[test]
fn the_version_got_is_the_latest_or_the_one_named() {
    let (_w, bucket) = built("get-chosen");
    let mut catalog = catalog_in(&tree(&bucket));
    assert_eq!(chosen(&catalog, &name("@bob/keys"), None).unwrap().version, Version(0, 1, 0));
    assert_eq!(chosen(&catalog, &name("@bob/keys"), Some(Version(0, 1, 0))).unwrap().version, Version(0, 1, 0));
    assert!(chosen(&catalog, &name("@bob/keys"), Some(Version(9, 0, 0))).unwrap_err().contains("0.1.0"), "it says which there are");
    assert!(chosen(&catalog, &name("@carol/none"), None).unwrap_err().contains("isn't in the library"));
    let p = catalog.packs.iter_mut().find(|p| p.name.as_str() == "@bob/keys").unwrap();
    p.latest = None;
    p.versions[0].yanked = Some("a bug".into());
    assert!(chosen(&catalog, &name("@bob/keys"), None).unwrap_err().contains("yanked"));
    assert!(chosen(&catalog, &name("@bob/keys"), Some(Version(0, 1, 0))).is_ok(), "named, a yanked version is got");
    let p = catalog.packs.iter_mut().find(|p| p.name.as_str() == "@bob/keys").unwrap();
    p.versions[0].removed = Some("not ours".into());
    assert!(chosen(&catalog, &name("@bob/keys"), Some(Version(0, 1, 0))).unwrap_err().contains("taken down"));
}
