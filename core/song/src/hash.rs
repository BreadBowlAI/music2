//! Content hashes of assets and state blobs, and the cache that keeps compile from reading
//! them again (FMT-15, FMT-16).
//!
//! The cache is keyed by `(path, size, mtime, ctime, inode)`. An entry whose mtime lies
//! within the volume's timestamp resolution of the moment it was cached is unverified and is
//! hashed again on next use (git's racy-timestamp rule): a file rewritten in place with the
//! same length within that window would otherwise keep its old hash. The resolution is taken
//! as 2 seconds, the coarsest of the volumes a project may be on (FAT and exFAT), so a file
//! changed in the last 2 seconds before it was hashed is hashed once more.
//!
//! The cache lives in memory, and on disk only where a host asks ([`HashCache::persist_in`]):
//! `~/Library/Caches/music2/` for the app and the CLI. Nothing here picks a folder by itself,
//! so tests never write outside their own.

use std::collections::HashMap;
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The coarsest timestamp resolution of a volume a project may live on (exFAT).
const RESOLUTION_NS: i128 = 2_000_000_000;

/// The cache's file in its folder.
const FILE: &str = "hashes.json";

/// A SHA-256 of a file's bytes.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Sha([u8; 32]);

impl Sha {
    pub fn of(bytes: &[u8]) -> Sha {
        Sha(Sha256::digest(bytes).into())
    }

    /// Hash a file as it streams by.
    pub fn of_file(path: &Path) -> std::io::Result<Sha> {
        let mut f = std::fs::File::open(path)?;
        let mut h = Sha256::new();
        let mut buf = vec![0u8; 1 << 16];
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                return Ok(Sha(h.finalize().into()));
            }
            h.update(&buf[..n]);
        }
    }

    /// All 64 hex digits.
    pub fn hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// The asset pin: the first 16 hex digits (FMT-15).
    pub fn pin(&self) -> String {
        self.hex()[..16].to_string()
    }

    /// All 64 hex digits, lowercase, back to a hash.
    pub fn parse(hex: &str) -> Option<Sha> {
        if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
            return None;
        }
        let bytes: Vec<u8> = (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok()).collect::<Option<_>>()?;
        Some(Sha(bytes.try_into().ok()?))
    }

    /// `sha256:<64 hex digits>`: how a Tx, a journal and a delta write a hash (WRITE-12).
    pub fn prefixed(&self) -> String {
        format!("sha256:{}", self.hex())
    }

    /// [`Sha::prefixed`]'s form back to a hash.
    pub fn parse_prefixed(s: &str) -> Option<Sha> {
        Sha::parse(s.strip_prefix("sha256:")?)
    }
}

impl fmt::Display for Sha {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.prefixed())
    }
}

impl Serialize for Sha {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.prefixed())
    }
}

impl<'de> Deserialize<'de> for Sha {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Sha, D::Error> {
        let s = String::deserialize(d)?;
        Sha::parse_prefixed(&s).ok_or_else(|| serde::de::Error::custom(format!("`{s}` is not `sha256:` and 64 lowercase hex digits")))
    }
}

impl fmt::Debug for Sha {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Sha({})", self.pin())
    }
}

/// What identifies a file's version on disk without reading it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Stamp {
    size: u64,
    mtime: i128,
    ctime: i128,
    inode: u64,
}

impl Stamp {
    fn of(meta: &std::fs::Metadata) -> Stamp {
        let ns =
            |t: std::io::Result<SystemTime>| t.ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos() as i128);
        #[cfg(unix)]
        let (ctime, inode) = {
            use std::os::unix::fs::MetadataExt;
            (meta.ctime() as i128 * 1_000_000_000 + meta.ctime_nsec() as i128, meta.ino())
        };
        #[cfg(not(unix))]
        let (ctime, inode) = (ns(meta.created()), 0);
        Stamp { size: meta.len(), mtime: ns(meta.modified()), ctime, inode }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Entry {
    stamp: Stamp,
    /// When it was hashed, in nanoseconds since the epoch.
    cached: i128,
    sha: String,
}

impl Entry {
    /// Whether the file can't have changed within its mtime's resolution after it was hashed.
    fn verified(&self) -> bool {
        self.stamp.mtime < self.cached - RESOLUTION_NS
    }
}

/// Hashes by absolute path, checked against each file's stamp before use.
pub struct HashCache {
    entries: Mutex<HashMap<PathBuf, Entry>>,
    /// Where it is saved, if anywhere, and whether it changed since.
    disk: Mutex<(Option<PathBuf>, bool)>,
    /// Files hashed by reading them, for tests.
    reads: std::sync::atomic::AtomicUsize,
}

impl HashCache {
    pub fn new() -> HashCache {
        HashCache { entries: Mutex::default(), disk: Mutex::default(), reads: Default::default() }
    }

    /// The process's cache.
    pub fn global() -> &'static HashCache {
        static CACHE: OnceLock<HashCache> = OnceLock::new();
        CACHE.get_or_init(HashCache::new)
    }

    /// Keep the cache in `dir` (`~/Library/Caches/music2/`): load what is there, and save to it
    /// from now on. A missing or unreadable cache file is an empty cache.
    pub fn persist_in(&self, dir: &Path) {
        let file = dir.join(FILE);
        if let Ok(text) = std::fs::read_to_string(&file)
            && let Ok(saved) = serde_json::from_str::<HashMap<PathBuf, Entry>>(&text)
        {
            let mut entries = self.entries.lock().expect("not poisoned");
            for (p, e) in saved {
                entries.entry(p).or_insert(e);
            }
        }
        *self.disk.lock().expect("not poisoned") = (Some(file), false);
    }

    /// The SHA-256 of the file at `path`, from the cache when its stamp still matches and the
    /// entry is verified, and otherwise by reading it.
    pub fn hash(&self, path: &Path) -> std::io::Result<Sha> {
        let meta = std::fs::metadata(path)?;
        let stamp = Stamp::of(&meta);
        if let Some(e) = self.entries.lock().expect("not poisoned").get(path)
            && e.stamp == stamp
            && e.verified()
            && let Some(sha) = Sha::parse(&e.sha)
        {
            return Ok(sha);
        }
        let cached = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos() as i128);
        let sha = Sha::of_file(path)?;
        self.reads.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // The stamp from before the read: a write during it changes the stamp, so the entry
        // can't match the file it didn't hash.
        self.entries.lock().expect("not poisoned").insert(path.to_path_buf(), Entry { stamp, cached, sha: sha.hex() });
        self.disk.lock().expect("not poisoned").1 = true;
        Ok(sha)
    }

    /// Save the cache, if it is kept on disk and changed. Failing to is not an error: the
    /// cache can always be rebuilt.
    pub fn save(&self) {
        let mut disk = self.disk.lock().expect("not poisoned");
        let (Some(file), true) = (&disk.0, disk.1) else { return };
        let text = serde_json::to_string(&*self.entries.lock().expect("not poisoned")).expect("entries serialise");
        let part = file.with_extension(format!("{}.part", std::process::id()));
        let saved = file.parent().is_some_and(|d| std::fs::create_dir_all(d).is_ok())
            && std::fs::write(&part, text).is_ok()
            && std::fs::rename(&part, file).is_ok();
        if saved {
            disk.1 = false;
        } else {
            std::fs::remove_file(&part).ok();
        }
    }

    #[cfg(test)]
    fn reads(&self) -> usize {
        self.reads.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl Default for HashCache {
    fn default() -> HashCache {
        HashCache::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("m2-hash-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&d).ok();
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn pins_are_the_first_16_hex_digits() {
        let sha = Sha::of(b"abc");
        assert_eq!(sha.hex(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(sha.pin(), "ba7816bf8f01cfea");
        assert_eq!(Sha::parse(&sha.hex()), Some(sha));
        assert_eq!(Sha::parse("ba78"), None);
        assert_eq!(Sha::parse(&sha.hex().to_uppercase()), None, "one spelling");
        assert_eq!(sha.prefixed(), format!("sha256:{}", sha.hex()));
        assert_eq!(Sha::parse_prefixed(&sha.prefixed()), Some(sha));
        assert_eq!(Sha::parse_prefixed(&sha.hex()), None);
        assert_eq!(serde_json::to_string(&sha).unwrap(), format!("\"{}\"", sha.prefixed()));
        assert_eq!(serde_json::from_str::<Sha>(&format!("\"{}\"", sha.prefixed())).unwrap(), sha);
    }

    // FMT-16: an unchanged, verified file is not read again; a changed one is.
    #[test]
    fn cached_by_stamp() {
        let d = dir("stamp");
        let f = d.join("a.wav");
        std::fs::write(&f, b"one").unwrap();
        // Old enough that the entry is verified when cached.
        let old = SystemTime::now() - std::time::Duration::from_secs(60);
        std::fs::File::options().write(true).open(&f).unwrap().set_modified(old).unwrap();
        let cache = HashCache::new();
        assert_eq!(cache.hash(&f).unwrap(), Sha::of(b"one"));
        assert_eq!(cache.hash(&f).unwrap(), Sha::of(b"one"));
        assert_eq!(cache.reads(), 1, "the second use comes from the cache");
        std::fs::write(&f, b"two!").unwrap();
        assert_eq!(cache.hash(&f).unwrap(), Sha::of(b"two!"));
        assert_eq!(cache.reads(), 2);
        std::fs::remove_dir_all(d).ok();
    }

    // FMT-16: git's racy-timestamp rule. A file written in place with the same length and the
    // same mtime right after it was hashed must not keep the old hash.
    #[test]
    fn racy_entries_are_hashed_again() {
        let d = dir("racy");
        let f = d.join("a.wav");
        std::fs::write(&f, b"aaaa").unwrap();
        let cache = HashCache::new();
        assert_eq!(cache.hash(&f).unwrap(), Sha::of(b"aaaa"));
        // Same length, and the mtime put back: only the content changed.
        let mtime = std::fs::metadata(&f).unwrap().modified().unwrap();
        std::fs::write(&f, b"bbbb").unwrap();
        std::fs::File::options().write(true).open(&f).unwrap().set_modified(mtime).unwrap();
        assert_eq!(cache.hash(&f).unwrap(), Sha::of(b"bbbb"), "an entry cached within the resolution is unverified");
        std::fs::remove_dir_all(d).ok();
    }

    // FMT-16: the rule itself. ctime alone catches most rewrites on Unix, but not on volumes
    // whose timestamps are 2 seconds apart.
    #[test]
    fn an_entry_is_verified_only_past_the_resolution() {
        let at = |mtime: i128, cached: i128| Entry { stamp: Stamp { size: 1, mtime, ctime: mtime, inode: 1 }, cached, sha: String::new() };
        let s = 1_000_000_000;
        assert!(!at(10 * s, 10 * s).verified());
        assert!(!at(10 * s, 11 * s).verified());
        assert!(!at(10 * s, 12 * s).verified());
        assert!(at(10 * s, 12 * s + 1).verified());
        assert!(at(10 * s, 60 * s).verified());
    }

    #[test]
    fn persists_where_asked() {
        let d = dir("persist");
        let f = d.join("a.wav");
        std::fs::write(&f, b"x").unwrap();
        let old = SystemTime::now() - std::time::Duration::from_secs(60);
        std::fs::File::options().write(true).open(&f).unwrap().set_modified(old).unwrap();
        let cache = HashCache::new();
        cache.persist_in(&d.join("cache"));
        cache.hash(&f).unwrap();
        cache.save();
        let again = HashCache::new();
        again.persist_in(&d.join("cache"));
        assert_eq!(again.hash(&f).unwrap(), Sha::of(b"x"));
        assert_eq!(again.reads(), 0, "read from the saved cache");
        std::fs::remove_dir_all(d).ok();
    }
}
