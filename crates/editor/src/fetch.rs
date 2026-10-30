//! Downloads (NET-1 to NET-6): the one way music2 reaches another computer.
//!
//! [`get`] fetches something small into memory (a catalog, a JSON file); [`get_to_file`] streams
//! something big to a file, through a `.part` file of its own beside it, checked by size and hash
//! before it is put in place. Both take an address that is `https://`, `http://` to this computer
//! only, or `file://` (NET-3); a size limit and a deadline for the whole download (NET-4, NET-6);
//! and a cancel. Neither retries nor caches: the caller decides.
//!
//! Both block. Call them from a thread of their own (in the server, `spawn_blocking`), never from
//! async code: there they panic, as tokio's `block_on` does, so a blocked runtime is a loud bug
//! and never a quiet stall of the app's server. Inside, fetch drives reqwest's client on a small
//! runtime of its own, one thread for the whole process, made at the first download: so while it
//! waits on the network a cancel or the deadline is seen within 50 ms, a quiet server is timed by
//! the stretch of silence, not by each read, and connections are kept for the next download from
//! the same host.
//!
//! TLS is rustls with the ring provider, handed to fetch's own clients, so nothing is installed for
//! the whole process and nothing else can change it. Certificates are checked by the OS's verifier
//! (rustls-platform-verifier: Apple's on macOS, the system's store on Linux), loaded only for an
//! https address: http to this computer and `file://` never need one. No check can be turned off
//! here, no proxy is used, and no compressed answer is taken.

use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::future::Future;
use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use m2_song::Sha;
use reqwest::Url;
use reqwest::header::{ACCEPT_ENCODING, CONTENT_ENCODING, HeaderMap, HeaderValue, LOCATION};
use rustls::pki_types::CertificateDer;
use rustls_platform_verifier::BuilderVerifierExt;
use sha2::{Digest, Sha256};

/// What a download must be (NET-4, NET-6): its size and SHA-256 when the caller knows them, the
/// most it may be, and the longest the whole download may take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Expect {
    pub size: Option<u64>,
    pub sha256: Option<Sha>,
    pub limit: u64,
    pub within: Duration,
}

/// What came: its size and SHA-256.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Got {
    pub size: u64,
    pub sha256: Sha,
}

/// How far a download is: the bytes so far, and how many there will be when the server (or the
/// disk) said.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub done: u64,
    pub total: Option<u64>,
}

/// Why a download failed, in words for a musician (NET-6). Each names the host (`packs.example`,
/// `127.0.0.1:4100`), or the file for a `file://` address, never the whole address: a token in a
/// link is never shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The address isn't one music2 downloads from, or a redirect to one was refused (NET-2,
    /// NET-3): why. Nothing was sent to it.
    Refused(String),
    /// The host couldn't be reached: no internet, a wrong name, or it is down.
    Unreachable { host: String },
    /// No safe connection could be made: a certificate this computer doesn't trust, one for
    /// another name, or a server that doesn't speak TLS.
    Unsafe { host: String },
    /// Nothing came for this many seconds: no answer, or no bytes.
    Quiet { host: String, secs: u64 },
    /// The connection broke off before the end.
    Broken { host: String },
    /// The server answered this, not the file.
    Status { host: String, code: u16 },
    /// The server sent the file compressed (`Content-Encoding`), which music2 doesn't read.
    Compressed { host: String },
    /// Over the limit: its size when the server or the disk said it.
    TooBig { size: Option<u64>, limit: u64 },
    /// Not the size the caller expected: what came, or `None` when more came than expected and
    /// the download stopped there.
    WrongSize { expected: u64, got: Option<u64> },
    /// Not the SHA-256 the caller expected.
    WrongHash,
    /// The whole download took longer than the caller's `within`, in seconds.
    TooSlow { secs: u64 },
    /// A file couldn't be read (`file://`) or the download couldn't be saved: why, naming it.
    Disk(String),
    /// This computer's trusted certificates couldn't be loaded, so no https download can be
    /// checked: why. Tried again at the next https download.
    NoCertificates(String),
    /// The caller cancelled it.
    Cancelled,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Refused(why) | Error::Disk(why) => write!(f, "{why}"),
            Error::Unreachable { host } => write!(f, "couldn't reach {host}: no internet?"),
            Error::Unsafe { host } => {
                write!(
                    f,
                    "couldn't connect to {host} safely: this computer doesn't trust its certificate, or something on the way changed the connection"
                )
            }
            Error::Quiet { host, secs } => {
                write!(f, "nothing came from {host} for {secs} s: the connection is too slow, or the server stopped")
            }
            Error::Broken { host } => write!(f, "the download from {host} broke off before its end: try again"),
            Error::Status { host, code: code @ (404 | 410) } => write!(f, "{host} doesn't have that file (it answered {code})"),
            Error::Status { host, code: code @ (401 | 403) } => write!(f, "{host} won't give that file (it answered {code})"),
            Error::Status { host, code: code @ (429 | 500..=599) } => {
                write!(f, "{host} can't answer right now (it answered {code}): try again later")
            }
            Error::Status { host, code } => write!(f, "{host} answered {code}, not the file"),
            Error::Compressed { host } => write!(f, "{host} sent the file compressed, which music2 doesn't read"),
            Error::TooBig { size: Some(size), limit } => {
                let (size, limit) = sizes(*size, *limit);
                write!(f, "the file was {size}, more than the {limit} allowed")
            }
            Error::TooBig { size: None, limit } => write!(f, "the file was more than the {} allowed", size_text(*limit)),
            Error::WrongSize { expected, got: Some(got) } => {
                let (got, expected) = sizes(*got, *expected);
                write!(f, "the download was {got}, not the {expected} it should be: it was changed or broken on the way")
            }
            Error::WrongSize { expected, got: None } => {
                write!(f, "the download was more than the {} it should be: it was changed or broken on the way", size_text(*expected))
            }
            Error::WrongHash => write!(f, "the download didn't match its hash: it was changed or broken on the way"),
            Error::TooSlow { secs } => {
                write!(f, "the download took more than the {secs} s allowed, so music2 stopped it: the connection is too slow")
            }
            Error::NoCertificates(why) => {
                write!(f, "couldn't load this computer's trusted certificates, so no download can be checked: {why}")
            }
            Error::Cancelled => write!(f, "the download was cancelled"),
        }
    }
}

impl std::error::Error for Error {}

/// Whether music2 downloads from `url` (NET-2, NET-3), without sending anything.
pub fn check(url: &str) -> Result<(), Error> {
    source(url).map(drop)
}

/// The bytes at `url`, checked against `expect` (NET-4): at most `expect.limit` of them, of its
/// size and hash when known, within `expect.within`. `cancel` is looked at as for
/// [`get_to_file`].
pub fn get(url: &str, expect: &Expect, cancel: &AtomicBool) -> Result<Vec<u8>, Error> {
    get_with(&SETUP, url, expect, cancel)
}

/// Download `url` to `dest` (NET-4, NET-5): into a `.part` file of this call's own beside it,
/// hashed as it is written, stopped as soon as it is over `expect.limit` or past `expect.within`,
/// then checked against `expect`'s size and hash, put on the drive, renamed onto `dest`, and its
/// folder synced. On any failure or a cancel the `.part` is removed and `dest` is left as it was,
/// but when the folder can't be synced after the rename: then nothing is left at `dest`.
/// `progress` is told after each piece, on the caller's thread. `cancel` is looked at every 50 ms
/// while the download waits on the network, and between pieces read from a file; not while the
/// file is put on the drive. `dest`'s folder must be there. Two downloads to one `dest` never mix:
/// each lands whole, the last one last.
pub fn get_to_file(url: &str, dest: &Path, expect: &Expect, mut progress: impl FnMut(Progress), cancel: &AtomicBool) -> Result<Got, Error> {
    get_to_file_with(&SETUP, url, dest, expect, &mut progress, cancel)
}

/// How long a download waits (NET-6).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Timeouts {
    /// To connect: a name looked up, a connection made, TLS agreed.
    pub(crate) connect: Duration,
    /// For the answer, once asked.
    pub(crate) answer: Duration,
    /// With no bytes, once the answer came.
    pub(crate) quiet: Duration,
}

pub(crate) const TIMEOUTS: Timeouts =
    Timeouts { connect: Duration::from_secs(10), answer: Duration::from_secs(30), quiet: Duration::from_secs(30) };

/// How often a wait looks at its cancel and its deadline.
pub(crate) const TICK: Duration = Duration::from_millis(50);

/// The most redirects one download follows (NET-3).
const REDIRECTS: usize = 5;

/// A file's pieces as they are read from the disk.
const PIECE: usize = 64 << 10;

/// The most `.part` names one download tries before it gives up: each taken name (a part a crash
/// left, a link someone put there) is passed over.
const PART_TRIES: usize = 64;

/// What a download runs with: the app's always, the tests' own where they say.
pub(crate) struct Setup<'a> {
    pub(crate) timeouts: Timeouts,
    /// Certificates trusted besides the OS's: a test's own CA, never one in the app.
    pub(crate) roots: &'a [CertificateDer<'static>],
    /// The disk calls that put a download in place.
    pub(crate) disk: &'a dyn Disk,
}

pub(crate) const SETUP: Setup<'static> = Setup { timeouts: TIMEOUTS, roots: &[], disk: &OsDisk };

/// The disk calls that put a download in place (NET-5), in one seam so the tests see their order.
pub(crate) trait Disk: Sync {
    /// The file's bytes made durable: `F_FULLFSYNC` on macOS, `fsync` on Linux.
    fn barrier(&self, f: &File) -> std::io::Result<()>;
    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()>;
    /// A folder's new entries pushed to the drive (WRITE-1).
    fn sync_dir(&self, dir: &Path) -> std::io::Result<()>;
}

/// The OS's own calls, through `m2_os` (PLAT-2).
pub(crate) struct OsDisk;

impl Disk for OsDisk {
    fn barrier(&self, f: &File) -> std::io::Result<()> {
        m2_os::fs::barrier(f)
    }

    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        std::fs::rename(from, to)
    }

    fn sync_dir(&self, dir: &Path) -> std::io::Result<()> {
        m2_os::fs::sync_dir(dir)
    }
}

/// [`get`] with `setup`.
pub(crate) fn get_with(setup: &Setup, url: &str, expect: &Expect, cancel: &AtomicBool) -> Result<Vec<u8>, Error> {
    let source = source(url)?;
    let ctl = Control::new(expect.within, cancel);
    ctl.check()?;
    let mut sink = Sink::Memory(vec![]);
    run(setup, &source, &mut sink, expect, &mut |_| {}, &ctl)?;
    match sink {
        Sink::Memory(bytes) => Ok(bytes),
        Sink::File(..) => unreachable!("get keeps the bytes"),
    }
}

/// [`get_to_file`] with `setup`.
pub(crate) fn get_to_file_with(
    setup: &Setup,
    url: &str,
    dest: &Path,
    expect: &Expect,
    progress: &mut dyn FnMut(Progress),
    cancel: &AtomicBool,
) -> Result<Got, Error> {
    let source = source(url)?;
    let ctl = Control::new(expect.within, cancel);
    ctl.check()?;
    let (file, path) = open_part(dest)?;
    let mut part = Part { path, landed: false };
    let saving = |e: std::io::Error| Error::Disk(format!("couldn't save {}: {e}", part.path.display()));
    let mut sink = Sink::File(file, part.path.clone());
    let got = run(setup, &source, &mut sink, expect, progress, &ctl)?;
    let Sink::File(file, _) = sink else { unreachable!("get_to_file writes a file") };
    // The bytes are on the drive before the name is, so a crash never leaves `dest` empty or half.
    setup.disk.barrier(&file).map_err(saving)?;
    drop(file);
    setup.disk.rename(&part.path, dest).map_err(|e| Error::Disk(format!("couldn't put the download at {}: {e}", dest.display())))?;
    part.landed = true;
    // The new name pushed to the drive: durable on Linux now, on macOS at its next flush. If that
    // fails, the file it replaced is gone already, so nothing is left at `dest` (NET-5).
    if let Err(e) = setup.disk.sync_dir(m2_os::fs::folder_of(dest)) {
        let _ = std::fs::remove_file(dest);
        return Err(Error::Disk(format!("couldn't make sure {} was saved: {e}", dest.display())));
    }
    Ok(got)
}

/// The `.part` names handed out in this process, one each (NET-5).
pub(crate) static PARTS: AtomicU64 = AtomicU64::new(0);

/// The `n`th `.part` name for `dest`: `<dest>.<this process>-<n>.part`, beside it.
pub(crate) fn part_of(dest: &Path, n: u64) -> PathBuf {
    let mut name = OsString::from(dest.file_name().unwrap_or_default());
    name.push(format!(".{}-{n}.part", std::process::id()));
    dest.with_file_name(name)
}

/// A `.part` of this call's own, made new: never one another call writes, never through a link,
/// which `create_new` refuses as it refuses any name that is taken (NET-5).
fn open_part(dest: &Path) -> Result<(File, PathBuf), Error> {
    if dest.file_name().is_none() {
        return Err(Error::Disk(format!("{} isn't a file's name", dest.display())));
    }
    for _ in 0..PART_TRIES {
        let path = part_of(dest, PARTS.fetch_add(1, Ordering::Relaxed));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((file, path)),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(Error::Disk(format!("couldn't save {}: {e}", path.display()))),
        }
    }
    Err(Error::Disk(format!("couldn't make a new file beside {}: every name tried is taken", dest.display())))
}

/// The `.part` being written: removed when dropped, unless it landed (NET-5). So a failure, a
/// cancel, or a caller's `progress` that panics, all leave nothing behind.
struct Part {
    path: PathBuf,
    landed: bool,
}

impl Drop for Part {
    fn drop(&mut self) {
        if !self.landed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// What may end a download early: the caller's cancel and its deadline (NET-6).
struct Control<'a> {
    cancel: &'a AtomicBool,
    /// None when `within` is beyond what the clock can say.
    deadline: Option<Instant>,
    within: Duration,
}

impl Control<'_> {
    fn new(within: Duration, cancel: &AtomicBool) -> Control<'_> {
        Control { cancel, deadline: Instant::now().checked_add(within), within }
    }

    /// Cancelled, or past the deadline.
    fn check(&self) -> Result<(), Error> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        match self.deadline {
            Some(at) if Instant::now() >= at => {
                Err(Error::TooSlow { secs: (self.within.as_secs() + u64::from(self.within.subsec_nanos() > 0)).max(1) })
            }
            _ => Ok(()),
        }
    }

    /// The time left before the deadline.
    fn left(&self) -> Option<Duration> {
        self.deadline.map(|at| at.saturating_duration_since(Instant::now()))
    }
}

/// Where a download comes from, once its address passed NET-3.
enum Source {
    Web(Url),
    Disk(PathBuf),
}

/// `url`, checked (NET-2, NET-3): `https://`, `http://` to this computer, or `file://` on this
/// computer; no name or password in it. Says why not in words that never repeat the address.
fn source(url: &str) -> Result<Source, Error> {
    let parsed = Url::parse(url).map_err(|_| Error::Refused("that isn't a web address: it should start with https://".into()))?;
    if parsed.scheme() == "file" {
        // A host is another computer's share (`file://server/share`): never read.
        return match parsed.host().is_none().then(|| parsed.to_file_path()) {
            Some(Ok(path)) => Ok(Source::Disk(path)),
            _ => Err(Error::Refused("music2 reads file:// addresses on this computer only".into())),
        };
    }
    web(&parsed)?;
    Ok(Source::Web(parsed))
}

/// A web address music2 may ask (NET-2, NET-3).
fn web(url: &Url) -> Result<(), Error> {
    let host = host_of(url);
    if !url.username().is_empty() || url.password().is_some() {
        return Err(Error::Refused(format!("the address for {host} holds a name or password, and music2 never sends those")));
    }
    match url.scheme() {
        "https" if url.host().is_some() => Ok(()),
        "http" if this_computer(url) => Ok(()),
        "http" => Err(Error::Refused(format!("music2 downloads only over https, so it won't ask {host} over plain http"))),
        scheme => {
            Err(Error::Refused(format!("music2 can't download {scheme}: addresses, only https://, http:// on this computer, and file://")))
        }
    }
}

/// Whether `url`'s host is this computer: `127.0.0.1`, `::1` or `localhost`, which the clients
/// always send to 127.0.0.1 (NET-3). The parser writes an address one way only (`0x7f.1` is
/// `127.0.0.1`, `[0:0::1]` is `[::1]`), so its words are compared.
fn this_computer(url: &Url) -> bool {
    matches!(url.host_str(), Some("127.0.0.1" | "[::1]" | "localhost"))
}

/// What a message calls `url`'s host: its name and port, never its path or query (NET-6).
fn host_of(url: &Url) -> String {
    let host = url.host_str().unwrap_or("the server");
    match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    }
}

/// Where a redirect from `from` to `location` goes, checked as an address is, and never from
/// https to http (NET-3).
pub(crate) fn next_hop(from: &Url, location: &str) -> Result<Url, Error> {
    let host = host_of(from);
    let next = from.join(location).map_err(|_| Error::Refused(format!("{host} sent the download on to an address that isn't one")))?;
    match next.scheme() {
        "https" => {}
        "http" if from.scheme() == "https" => {
            return Err(Error::Refused(format!("{host} sent the download on to an address that isn't https, so music2 stopped")));
        }
        "http" => {}
        scheme => return Err(Error::Refused(format!("{host} sent the download on to a {scheme}: address, which music2 never follows"))),
    }
    web(&next).map_err(|e| match e {
        Error::Refused(why) => Error::Refused(format!("{host} sent the download on, and {why}")),
        e => e,
    })?;
    Ok(next)
}

/// Where the bytes go: memory for [`get`], the `.part` for [`get_to_file`].
enum Sink {
    Memory(Vec<u8>),
    File(File, PathBuf),
}

/// What came so far, checked as it comes (NET-4).
struct Intake<'a> {
    expect: &'a Expect,
    sink: &'a mut Sink,
    progress: &'a mut dyn FnMut(Progress),
    hash: Sha256,
    done: u64,
    total: Option<u64>,
}

impl Intake<'_> {
    /// The size the server or the disk says, checked before any byte.
    fn total(&mut self, total: Option<u64>) -> Result<(), Error> {
        if let Some(n) = total {
            if n > self.expect.limit {
                return Err(Error::TooBig { size: Some(n), limit: self.expect.limit });
            }
            if let Some(want) = self.expect.size
                && n != want
            {
                return Err(Error::WrongSize { expected: want, got: Some(n) });
            }
            if let Sink::Memory(bytes) = self.sink {
                bytes.reserve(n as usize);
            }
        }
        self.total = total;
        Ok(())
    }

    /// The next piece: refused before it is kept when it would pass the limit or the size.
    fn take(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let done = self.done + bytes.len() as u64;
        if done > self.expect.limit {
            return Err(Error::TooBig { size: None, limit: self.expect.limit });
        }
        if let Some(want) = self.expect.size
            && done > want
        {
            return Err(Error::WrongSize { expected: want, got: None });
        }
        match self.sink {
            Sink::Memory(kept) => kept.extend_from_slice(bytes),
            Sink::File(f, part) => f.write_all(bytes).map_err(|e| Error::Disk(format!("couldn't save {}: {e}", part.display())))?,
        }
        self.hash.update(bytes);
        self.done = done;
        (self.progress)(Progress { done, total: self.total });
        Ok(())
    }

    /// Every byte came: the size and hash, checked.
    fn end(self) -> Result<Got, Error> {
        if let Some(want) = self.expect.size
            && self.done != want
        {
            return Err(Error::WrongSize { expected: want, got: Some(self.done) });
        }
        let digest: [u8; 32] = self.hash.finalize().into();
        let sha256 = Sha::from(digest);
        if self.expect.sha256.is_some_and(|want| want != sha256) {
            return Err(Error::WrongHash);
        }
        Ok(Got { size: self.done, sha256 })
    }
}

/// One download, from `source` into `sink`, on fetch's runtime: blocks the caller, and panics in
/// async code (NET-6).
fn run(
    setup: &Setup,
    source: &Source,
    sink: &mut Sink,
    expect: &Expect,
    progress: &mut dyn FnMut(Progress),
    ctl: &Control,
) -> Result<Got, Error> {
    let mut intake = Intake { expect, sink, progress, hash: Sha256::new(), done: 0, total: None };
    runtime().block_on(async {
        match source {
            Source::Web(url) => from_web(setup, url, &mut intake, ctl).await?,
            Source::Disk(path) => from_disk(path, &mut intake, ctl)?,
        }
        ctl.check()?;
        intake.end()
    })
}

/// fetch's runtime: one worker thread for the whole process, which drives the connections while
/// each caller's thread waits on its own download.
fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("music2-fetch")
            .enable_all()
            .build()
            .expect("music2 can't start a thread for downloads")
    })
}

/// Whose certificates a client trusts.
enum Trust<'a> {
    /// The OS's: every https download.
    Os,
    /// The OS's and these: a test's own CA.
    OsAnd(&'a [CertificateDer<'static>]),
    /// None: the client for http to this computer, which never meets a certificate.
    Nothing,
}

/// The client for `url`'s scheme: https with the OS's certificates, http to this computer with
/// none, so a computer without any still reaches a local registry (NET-3).
fn client_for(url: &Url, roots: &[CertificateDer<'static>]) -> Result<reqwest::Client, Error> {
    match url.scheme() {
        "https" => tls_client(roots),
        _ => {
            static PLAIN: Mutex<Option<reqwest::Client>> = Mutex::new(None);
            kept(&PLAIN, Trust::Nothing).map_err(|why| Error::Disk(format!("couldn't start downloading: {why}")))
        }
    }
}

/// The https client, made at the first https download with this computer's certificates, and
/// kept once made. A failure isn't kept: the next download tries again.
pub(crate) fn tls_client(roots: &[CertificateDer<'static>]) -> Result<reqwest::Client, Error> {
    static TLS: Mutex<Option<reqwest::Client>> = Mutex::new(None);
    match roots.is_empty() {
        true => kept(&TLS, Trust::Os),
        false => make_client(&TIMEOUTS, Trust::OsAnd(roots)),
    }
    .map_err(Error::NoCertificates)
}

/// The client in `slot`, made with `trust` when there is none yet.
fn kept(slot: &Mutex<Option<reqwest::Client>>, trust: Trust) -> Result<reqwest::Client, String> {
    let mut slot = slot.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(client) = &*slot {
        return Ok(client.clone());
    }
    let client = make_client(&TIMEOUTS, trust)?;
    *slot = Some(client.clone());
    Ok(client)
}

fn make_client(t: &Timeouts, trust: Trust) -> Result<reqwest::Client, String> {
    // ring, handed to this client alone: no default is installed for the process (NET-3).
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder =
        rustls::ClientConfig::builder_with_provider(provider.clone()).with_safe_default_protocol_versions().map_err(|e| e.to_string())?;
    let builder = match trust {
        Trust::Os => builder.with_platform_verifier().map_err(|e| e.to_string())?,
        // The OS's verifier still, given one more root: rustls calls any verifier it is handed
        // "dangerous", as it can't tell this one checks as strictly.
        Trust::OsAnd(roots) => {
            let verifier =
                rustls_platform_verifier::Verifier::new_with_extra_roots(roots.iter().cloned(), provider).map_err(|e| e.to_string())?;
            builder.dangerous().with_custom_certificate_verifier(Arc::new(verifier))
        }
        Trust::Nothing => builder.with_root_certificates(rustls::RootCertStore::empty()),
    };
    let mut tls = builder.with_no_client_auth();
    tls.alpn_protocols = vec![b"http/1.1".to_vec()];
    // The file as it is: never compressed on the way (NET-4).
    let headers = HeaderMap::from_iter([(ACCEPT_ENCODING, HeaderValue::from_static("identity"))]);
    reqwest::Client::builder()
        .tls_backend_preconfigured(tls)
        .user_agent(concat!("music2/", env!("CARGO_PKG_VERSION")))
        .default_headers(headers)
        // fetch follows redirects itself, checking each (NET-3), and sends no referrer (NET-2).
        .redirect(reqwest::redirect::Policy::none())
        .referer(false)
        .no_proxy()
        .connect_timeout(t.connect)
        .resolve("localhost", SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
        .build()
        .map_err(|e| e.to_string())
}

async fn from_web(setup: &Setup<'_>, url: &Url, intake: &mut Intake<'_>, ctl: &Control<'_>) -> Result<(), Error> {
    let t = &setup.timeouts;
    let mut url = url.clone();
    let mut hops = 0;
    let mut answer = loop {
        let host = host_of(&url);
        let client = client_for(&url, setup.roots)?;
        let answer = wait(client.get(url.clone()).send(), t.answer, ctl, &host).await?.map_err(|e| failed(&e, &host))?;
        let location = answer.headers().get(LOCATION).filter(|_| answer.status().is_redirection());
        let Some(location) = location else { break answer };
        if hops == REDIRECTS {
            return Err(Error::Refused(format!("{host} sent the download on more than {REDIRECTS} times, so music2 stopped")));
        }
        let location =
            location.to_str().map_err(|_| Error::Refused(format!("{host} sent the download on to an address that isn't one")))?;
        url = next_hop(&url, location)?;
        hops += 1;
    };
    let host = host_of(&url);
    if !answer.status().is_success() {
        return Err(Error::Status { host, code: answer.status().as_u16() });
    }
    let encoded = answer.headers().get(CONTENT_ENCODING).map(|v| v.as_bytes().trim_ascii());
    if encoded.is_some_and(|v| !v.is_empty() && !v.eq_ignore_ascii_case(b"identity")) {
        return Err(Error::Compressed { host });
    }
    intake.total(answer.content_length())?;
    while let Some(piece) = wait(answer.chunk(), t.quiet, ctl, &host).await?.map_err(|e| failed(&e, &host))? {
        intake.take(&piece)?;
    }
    Ok(())
}

/// A `file://` download: a plain file read from the disk with the same checks (NET-3). Anything
/// else (a pipe, a device) is refused before it is opened, since opening or reading it could block
/// where no cancel is seen.
fn from_disk(path: &Path, intake: &mut Intake<'_>, ctl: &Control) -> Result<(), Error> {
    let unread = |e: std::io::Error| match e.kind() {
        ErrorKind::NotFound => Error::Disk(format!("{} isn't there", path.display())),
        ErrorKind::PermissionDenied => Error::Disk(format!("music2 isn't allowed to read {}", path.display())),
        _ => Error::Disk(format!("couldn't read {}: {e}", path.display())),
    };
    let plain = |meta: &std::fs::Metadata| match (meta.is_dir(), meta.is_file()) {
        (true, _) => Err(Error::Disk(format!("{} is a folder, not a file", path.display()))),
        (_, false) => Err(Error::Disk(format!("{} isn't a plain file: music2 reads only files from a drive", path.display()))),
        _ => Ok(()),
    };
    plain(&std::fs::metadata(path).map_err(unread)?)?;
    let mut file = File::open(path).map_err(unread)?;
    let meta = file.metadata().map_err(unread)?;
    plain(&meta)?;
    intake.total(Some(meta.len()))?;
    let mut buf = vec![0u8; PIECE];
    loop {
        ctl.check()?;
        match file.read(&mut buf) {
            Ok(0) => return Ok(()),
            Ok(n) => intake.take(&buf[..n])?,
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => return Err(unread(e)),
        }
    }
}

/// `fut`, waited for at most `limit`, looking at the cancel and the deadline every [`TICK`]
/// (NET-6).
async fn wait<F: Future>(fut: F, limit: Duration, ctl: &Control<'_>, host: &str) -> Result<F::Output, Error> {
    let start = Instant::now();
    let mut fut = std::pin::pin!(fut);
    loop {
        ctl.check()?;
        let left = limit.saturating_sub(start.elapsed());
        if left.is_zero() {
            return Err(Error::Quiet { host: host.to_string(), secs: limit.as_secs().max(1) });
        }
        let slice = ctl.left().map_or(left, |l| l.min(left)).min(TICK);
        if let Ok(out) = tokio::time::timeout(slice, &mut fut).await {
            return Ok(out);
        }
    }
}

/// What a failure of reqwest's means, in NET-6's words. Never its own message, which holds the
/// whole address.
fn failed(e: &reqwest::Error, host: &str) -> Error {
    let host = host.to_string();
    if tls_failed(e) {
        Error::Unsafe { host }
    } else if e.is_connect() {
        Error::Unreachable { host }
    } else {
        Error::Broken { host }
    }
}

/// Whether TLS refused the connection: rustls's error anywhere in the chain. An I/O error's
/// `source` skips the error it wraps, so the chain goes through what it holds instead.
fn tls_failed(e: &(dyn std::error::Error + 'static)) -> bool {
    let mut at = Some(e);
    while let Some(e) = at {
        if e.is::<rustls::Error>() {
            return true;
        }
        at = match e.downcast_ref::<std::io::Error>().and_then(|io| io.get_ref()) {
            Some(inner) => Some(inner as &(dyn std::error::Error + 'static)),
            None => e.source(),
        };
    }
    false
}

/// Two sizes in words, in bytes whenever the rounded words would read the same.
fn sizes(a: u64, b: u64) -> (String, String) {
    let (ta, tb) = (size_text(a), size_text(b));
    match ta == tb {
        true => (format!("{a} bytes"), format!("{b} bytes")),
        false => (ta, tb),
    }
}

/// A size in words: `900 bytes`, `1.5 KB`, `12 MB`, `3 GB` (1 KB is 1,024 bytes, as the
/// sandbox's memory says it).
fn size_text(n: u64) -> String {
    const UNITS: [(&str, u64); 3] = [("GB", 1 << 30), ("MB", 1 << 20), ("KB", 1 << 10)];
    let Some((unit, size)) = UNITS.iter().find(|(_, size)| n >= *size) else { return format!("{n} bytes") };
    let x = n as f64 / *size as f64;
    match x < 10.0 && (x * 10.0).round() % 10.0 != 0.0 {
        true => format!("{x:.1} {unit}"),
        false => format!("{} {unit}", x.round()),
    }
}

#[cfg(test)]
#[path = "fetch_tests.rs"]
mod tests;
