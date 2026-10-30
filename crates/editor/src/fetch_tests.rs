//! Downloads (NET-1 to NET-6), against a tiny HTTP server on 127.0.0.1 inside each test. No test
//! reaches the internet: an address elsewhere is only ever refused before any connection.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::*;
use crate::testing::Fixture;

/// What the server does with one connection, once it has read the request.
type Answer = Box<dyn FnOnce(&mut TcpStream) + Send>;

/// A server on 127.0.0.1 that answers each connection, in turn, with the next of its answers,
/// then closes it. It keeps each request's head.
struct Server {
    addr: SocketAddr,
    heads: Arc<Mutex<Vec<String>>>,
}

impl Server {
    fn new(answers: Vec<Answer>) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let heads = Arc::new(Mutex::new(vec![]));
        let kept = heads.clone();
        std::thread::spawn(move || {
            for answer in answers {
                let Ok((mut s, _)) = listener.accept() else { return };
                kept.lock().unwrap().push(head(&mut s));
                answer(&mut s);
            }
        });
        Server { addr, heads }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }

    fn host(&self) -> String {
        self.addr.to_string()
    }
}

/// A request's head: its bytes up to the blank line. A client that doesn't start with a method
/// (a TLS hello) is answered at once.
fn head(s: &mut TcpStream) -> String {
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut got = vec![];
    let mut b = [0u8; 1];
    while !got.ends_with(b"\r\n\r\n") {
        match s.read(&mut b) {
            Ok(1) if got.is_empty() && !b[0].is_ascii_uppercase() => break,
            Ok(1) => got.push(b[0]),
            _ => break,
        }
    }
    String::from_utf8_lossy(&got).into_owned()
}

fn raw(bytes: Vec<u8>) -> Answer {
    Box::new(move |s| {
        let _ = s.write_all(&bytes);
    })
}

/// 200 with its length.
fn ok(body: &[u8]) -> Answer {
    let mut r = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).into_bytes();
    r.extend_from_slice(body);
    raw(r)
}

/// 200 with no length: the body ends when the connection closes.
fn ok_unsized(body: &[u8]) -> Answer {
    let mut r = b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n".to_vec();
    r.extend_from_slice(body);
    raw(r)
}

fn status(code: u16) -> Answer {
    raw(format!("HTTP/1.1 {code} Whatever\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").into_bytes())
}

fn redirect(to: &str) -> Answer {
    raw(format!("HTTP/1.1 302 Found\r\nLocation: {to}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").into_bytes())
}

/// A head that promises `len` bytes, then `sent` of them, then whatever `then` does.
fn partial(len: usize, sent: usize, then: fn(&mut TcpStream)) -> Answer {
    Box::new(move |s| {
        let mut r = format!("HTTP/1.1 200 OK\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n").into_bytes();
        r.extend(std::iter::repeat_n(b'x', sent));
        let _ = s.write_all(&r);
        let _ = s.flush();
        then(s);
    })
}

/// Say nothing more until the client goes away (10 s at most).
fn go_quiet(s: &mut TcpStream) {
    let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = s.read(&mut [0u8; 1]);
}

fn hang_up(_: &mut TcpStream) {}

/// A port where nothing listens.
fn closed_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn no_cancel() -> AtomicBool {
    AtomicBool::new(false)
}

fn expect(limit: u64) -> Expect {
    Expect { size: None, sha256: None, limit }
}

/// A folder for a test's files, and its `dest`.
fn folder() -> (Fixture, PathBuf) {
    let f = Fixture::new();
    std::fs::create_dir_all(&f.0).unwrap();
    let dest = f.0.join("blob.wav");
    (f, dest)
}

/// What is in the folder, by name: after a failure, nothing but what was there before (NET-5).
fn names(dir: &Path) -> Vec<String> {
    let mut n: Vec<String> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    n.sort();
    n
}

/// Download `url` to a fresh folder's `dest` with `e`: the result, and that nothing was left
/// when it failed (NET-5).
fn to_file(url: &str, e: &Expect) -> Result<Got, Error> {
    let (f, dest) = folder();
    let got = get_to_file(url, &dest, e, |_| {}, &no_cancel());
    if got.is_err() {
        assert_eq!(names(&f.0), Vec::<String>::new(), "a failed download leaves nothing (NET-5): {got:?}");
    }
    got
}

const BODY: &[u8] = b"a sample's bytes, or a catalog";

// NET-4, NET-5: what came is checked, then put in place, and its size and hash returned.
#[test]
fn a_download_lands_only_when_every_check_passed() {
    let s = Server::new(vec![ok(BODY), ok(BODY)]);
    let (f, dest) = folder();
    std::fs::write(&dest, b"an older copy").unwrap();
    let mut seen = vec![];
    let e = Expect { size: Some(BODY.len() as u64), sha256: Some(Sha::of(BODY)), limit: 1 << 20 };
    let got = get_to_file(&s.url("/blobs/a"), &dest, &e, |p| seen.push(p), &no_cancel()).unwrap();
    assert_eq!(got, Got { size: BODY.len() as u64, sha256: Sha::of(BODY) });
    assert_eq!(std::fs::read(&dest).unwrap(), BODY, "the new bytes replace the old");
    assert_eq!(names(&f.0), ["blob.wav"], "no .part is left");
    let last = seen.last().copied().expect("progress is told");
    assert_eq!(last, Progress { done: BODY.len() as u64, total: Some(BODY.len() as u64) });
    assert!(seen.windows(2).all(|w| w[0].done <= w[1].done));

    // With nothing known but the limit, the size and hash are still returned.
    let got = to_file(&s.url("/blobs/a"), &expect(1 << 20)).unwrap();
    assert_eq!(got, Got { size: BODY.len() as u64, sha256: Sha::of(BODY) });
}

// NET-5: even a caller's progress that panics leaves no `.part`.
#[test]
fn a_progress_that_panics_leaves_nothing() {
    let s = Server::new(vec![ok(BODY)]);
    let (f, dest) = folder();
    let url = s.url("/a");
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        get_to_file(&url, &dest, &expect(1 << 10), |_| panic!("the caller's bug"), &no_cancel())
    }));
    assert!(panicked.is_err());
    assert_eq!(names(&f.0), Vec::<String>::new());
}

#[test]
fn get_returns_the_bytes_within_their_limit() {
    let s = Server::new(vec![ok(BODY), ok_unsized(BODY), ok(BODY)]);
    assert_eq!(get(&s.url("/catalog.json"), 1 << 10).unwrap(), BODY);
    assert_eq!(get(&s.url("/catalog.json"), BODY.len() as u64).unwrap(), BODY, "exactly the limit is allowed");
    let e = get(&s.url("/catalog.json"), 10).unwrap_err();
    assert_eq!(e, Error::TooBig { size: Some(BODY.len() as u64), limit: 10 });
}

// NET-2: a GET of the address, with music2's User-Agent, and nothing about the person.
#[test]
fn a_request_says_only_what_it_asks_for() {
    let s = Server::new(vec![ok(BODY)]);
    get(&s.url("/blobs/sha256/ab?x=1"), 1 << 10).unwrap();
    let heads = s.heads.lock().unwrap().clone();
    assert_eq!(heads.len(), 1);
    let mut lines = heads[0].lines();
    assert_eq!(lines.next(), Some("GET /blobs/sha256/ab?x=1 HTTP/1.1"));
    let mut keys = vec![];
    for line in lines.filter(|l| !l.is_empty()) {
        let (name, value) = line.split_once(':').unwrap();
        let name = name.to_ascii_lowercase();
        if name == "user-agent" {
            assert_eq!(value.trim(), format!("music2/{}", env!("CARGO_PKG_VERSION")));
        }
        keys.push(name);
    }
    keys.sort();
    assert_eq!(keys, ["accept", "host", "user-agent"], "no cookie, referrer, id or anything else: {}", heads[0]);
}

// NET-2, NET-3: what isn't https, this computer's http or a file is refused, before any connection.
#[test]
fn an_address_that_isnt_safe_is_refused_before_any_connection() {
    for ok in [
        "https://packs.example/blobs/sha256/ab",
        "https://127.0.0.1:9/x",
        "http://127.0.0.1:9/x",
        "http://[::1]:9/x",
        "http://localhost:9/x",
        "file:///Volumes/Drive/packs/x",
    ] {
        assert_eq!(check(ok), Ok(()), "{ok}");
    }
    for bad in [
        "http://packs.example/x",
        "http://127.0.0.2/x",
        "http://[::ffff:127.0.0.1]/x",
        "http://0.0.0.0/x",
        "http://localhost.packs.example/x",
        "ftp://packs.example/x",
        "data:text/plain,hi",
        "javascript:alert(1)",
        "file://server/share/x",
        "https://alice:secret@packs.example/x",
        "https://alice@packs.example/x",
        "packs.example/x",
        "",
    ] {
        let e = check(bad).unwrap_err();
        assert!(matches!(e, Error::Refused(_)), "{bad}: {e:?}");
        assert_eq!(get(bad, 1 << 10).unwrap_err(), e, "{bad}");
        assert_eq!(to_file(bad, &expect(1 << 10)).unwrap_err(), e, "{bad}");
    }
    // A server that would answer at another of this computer's addresses never hears from it.
    let listener = TcpListener::bind("0.0.0.0:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    for bad in [format!("http://0.0.0.0:{port}/x"), format!("http://[::ffff:127.0.0.1]:{port}/x")] {
        assert!(matches!(get(&bad, 1 << 10), Err(Error::Refused(_))), "{bad}");
    }
    std::thread::sleep(Duration::from_millis(50));
    assert!(listener.accept().is_err(), "no connection was made");
}

// NET-3: a redirect is checked as an address is, and never goes from https to http.
#[test]
fn a_redirect_to_http_is_refused() {
    let host = |u: &str| Url::parse(u).unwrap();
    let to = |from: &str, loc: &str| next_hop(&host(from), loc);
    assert_eq!(to("https://packs.example/a", "/b").unwrap().as_str(), "https://packs.example/b");
    assert_eq!(to("https://packs.example/a", "https://mirror.example/b").unwrap().as_str(), "https://mirror.example/b");
    assert_eq!(to("http://127.0.0.1:9/a", "https://packs.example/b").unwrap().as_str(), "https://packs.example/b");
    for (from, loc) in [
        ("https://packs.example/a", "http://packs.example/b"),
        ("https://packs.example/a", "http://127.0.0.1:9/b"),
        ("https://packs.example/a", "file:///etc/passwd"),
        ("http://127.0.0.1:9/a", "http://packs.example/b"),
        ("http://127.0.0.1:9/a", "file:///etc/passwd"),
        ("https://packs.example/a", "https://alice:secret@packs.example/b"),
    ] {
        assert!(matches!(to(from, loc), Err(Error::Refused(_))), "{from} → {loc}");
    }

    // Over the wire: this computer's server sending the download on to plain http elsewhere.
    for loc in ["http://packs.example/b", "file:///etc/passwd"] {
        let s = Server::new(vec![redirect(loc)]);
        let e = to_file(&s.url("/a"), &expect(1 << 10)).unwrap_err();
        assert!(matches!(&e, Error::Refused(why) if why.contains(&s.host())), "{loc}: {e:?}");
        assert_eq!(s.heads.lock().unwrap().len(), 1);
    }
}

// NET-3: at most 5 redirects.
#[test]
fn a_few_redirects_are_followed_and_no_more() {
    let chain = |n: usize| {
        let mut answers: Vec<Answer> = (1..=n).map(|i| redirect(&format!("/hop{i}"))).collect();
        answers.push(ok(BODY));
        Server::new(answers)
    };
    let s = chain(5);
    assert_eq!(get(&s.url("/a"), 1 << 10).unwrap(), BODY);
    let heads = s.heads.lock().unwrap().clone();
    assert!(heads[5].starts_with("GET /hop5 "), "{heads:?}");

    let s = chain(6);
    let e = to_file(&s.url("/a"), &expect(1 << 10)).unwrap_err();
    assert!(matches!(&e, Error::Refused(_)), "{e:?}");
    assert_eq!(s.heads.lock().unwrap().len(), 6, "the sixth redirect isn't followed");
}

// NET-4: the server's word on its size is checked before any byte is taken.
#[test]
fn too_big_is_stopped_from_its_content_length() {
    let big = vec![b'x'; 12 << 20];
    let s = Server::new(vec![ok(&big)]);
    let e = to_file(&s.url("/a"), &expect(10 << 20)).unwrap_err();
    assert_eq!(e, Error::TooBig { size: Some(12 << 20), limit: 10 << 20 });
    assert_eq!(e.to_string(), "the file was 12 MB, more than the 10 MB allowed");
}

// NET-4: without a length, the download stops as soon as it is over.
#[test]
fn too_big_is_stopped_while_streaming() {
    let big = vec![b'x'; 4 << 20];
    let s = Server::new(vec![ok_unsized(&big), ok_unsized(&big)]);
    let (f, dest) = folder();
    let mut most = 0;
    let e = get_to_file(&s.url("/a"), &dest, &expect(1 << 20), |p| most = most.max(p.done), &no_cancel()).unwrap_err();
    assert_eq!(e, Error::TooBig { size: None, limit: 1 << 20 });
    assert_eq!(e.to_string(), "the file was more than the 1 MB allowed");
    assert!(most <= 1 << 20, "no byte past the limit is kept: {most}");
    assert_eq!(names(&f.0), Vec::<String>::new());
    assert_eq!(get(&s.url("/a"), 1 << 20).unwrap_err(), Error::TooBig { size: None, limit: 1 << 20 });
}

// NET-4: a size the caller knows, against the server's word and against what came.
#[test]
fn a_wrong_size_is_refused() {
    let n = BODY.len() as u64;
    let s = Server::new(vec![ok(BODY), ok_unsized(BODY), ok_unsized(BODY)]);
    let sized = |size: u64| Expect { size: Some(size), sha256: None, limit: 1 << 20 };
    assert_eq!(to_file(&s.url("/a"), &sized(n - 1)).unwrap_err(), Error::WrongSize { expected: n - 1, got: Some(n) });
    let e = to_file(&s.url("/a"), &sized(n + 1)).unwrap_err();
    assert_eq!(e, Error::WrongSize { expected: n + 1, got: Some(n) });
    assert!(e.to_string().ends_with("it was changed or broken on the way"), "{e}");
    assert_eq!(to_file(&s.url("/a"), &sized(n - 1)).unwrap_err(), Error::WrongSize { expected: n - 1, got: None });
}

// NET-4, NET-5: a wrong hash never reaches `dest`, and what was there stays.
#[test]
fn a_wrong_hash_is_refused() {
    let s = Server::new(vec![ok(BODY)]);
    let (f, dest) = folder();
    std::fs::write(&dest, b"the copy we had").unwrap();
    let e = Expect { size: Some(BODY.len() as u64), sha256: Some(Sha::of(b"other bytes")), limit: 1 << 20 };
    let err = get_to_file(&s.url("/a"), &dest, &e, |_| {}, &no_cancel()).unwrap_err();
    assert_eq!(err, Error::WrongHash);
    assert_eq!(err.to_string(), "the download didn't match its hash: it was changed or broken on the way");
    assert_eq!(std::fs::read(&dest).unwrap(), b"the copy we had", "dest is left as it was");
    assert_eq!(names(&f.0), ["blob.wav"]);
}

#[test]
fn a_missing_file_says_so() {
    let s = Server::new(vec![status(404), status(503), status(403)]);
    let e = to_file(&s.url("/a"), &expect(1 << 10)).unwrap_err();
    assert_eq!(e, Error::Status { host: s.host(), code: 404 });
    assert_eq!(e.to_string(), format!("{} doesn't have that file (it answered 404)", s.host()));
    let e = to_file(&s.url("/a"), &expect(1 << 10)).unwrap_err();
    assert!(e.to_string().contains("try again later"), "{e}");
    let e = to_file(&s.url("/a"), &expect(1 << 10)).unwrap_err();
    assert_eq!(e, Error::Status { host: s.host(), code: 403 });
}

// NET-6: a connection that drops says so, whether before the answer or in the middle.
#[test]
fn a_dropped_connection_says_so() {
    let s = Server::new(vec![partial(1000, 500, hang_up), raw(vec![]), raw(b"HTTP/1.1 200 OK\r\nContent-Le".to_vec())]);
    for _ in 0..3 {
        let e = to_file(&s.url("/a"), &expect(1 << 10)).unwrap_err();
        assert_eq!(e, Error::Broken { host: s.host() });
        assert!(e.to_string().starts_with(&format!("the download from {} broke off", s.host())), "{e}");
    }

    let port = closed_port();
    let e = to_file(&format!("http://127.0.0.1:{port}/a"), &expect(1 << 10)).unwrap_err();
    assert_eq!(e, Error::Unreachable { host: format!("127.0.0.1:{port}") });
    assert_eq!(e.to_string(), format!("couldn't reach 127.0.0.1:{port}: no internet?"));
}

// NET-6: no answer, or no bytes, for too long.
#[test]
fn a_server_that_goes_quiet_times_out() {
    let t = Timeouts { quiet: Duration::from_millis(300), answer: Duration::from_millis(300), ..TIMEOUTS };
    let s = Server::new(vec![Box::new(go_quiet), partial(1000, 10, go_quiet)]);
    for _ in 0..2 {
        let (f, dest) = folder();
        let started = Instant::now();
        let e = get_to_file_with(&t, &s.url("/a"), &dest, &expect(1 << 10), &mut |_| {}, &no_cancel()).unwrap_err();
        assert!(matches!(&e, Error::Quiet { host, .. } if *host == s.host()), "{e:?}");
        assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
        assert_eq!(names(&f.0), Vec::<String>::new());
    }
    assert_eq!(TIMEOUTS.connect, Duration::from_secs(10));
    assert_eq!(TIMEOUTS.answer, Duration::from_secs(30));
    assert_eq!(TIMEOUTS.quiet, Duration::from_secs(30));
}

// NET-6: a cancel is seen within a tick, even while nothing comes, and leaves nothing.
#[test]
fn cancel_stops_at_once_and_leaves_nothing() {
    let s = Server::new(vec![partial(1 << 20, 64 << 10, go_quiet)]);
    let (f, dest) = folder();
    let cancel = Arc::new(AtomicBool::new(false));
    let started = Arc::new(AtomicBool::new(false));
    let (c, st) = (cancel.clone(), started.clone());
    let canceller = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !st.load(Ordering::SeqCst) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(200));
        c.store(true, Ordering::SeqCst);
        Instant::now()
    });
    let e = get_to_file(&s.url("/a"), &dest, &expect(1 << 20), |_| started.store(true, Ordering::SeqCst), &cancel).unwrap_err();
    let after = canceller.join().unwrap().elapsed();
    assert_eq!(e, Error::Cancelled);
    // Far under the 30 s a quiet server is given: the wait saw it, not a timeout (a tick is 50 ms).
    assert!(after < Duration::from_secs(5), "seen {after:?} after the cancel");
    assert_eq!(names(&f.0), Vec::<String>::new());

    // Cancelled before it starts: nothing is asked, nothing is made.
    let s = Server::new(vec![ok(BODY)]);
    let e = get_to_file(&s.url("/a"), &dest, &expect(1 << 10), |_| {}, &AtomicBool::new(true)).unwrap_err();
    assert_eq!(e, Error::Cancelled);
    assert_eq!(names(&f.0), Vec::<String>::new());
}

// NET-3: a file on a drive, read with the same limits and checks.
#[test]
fn file_addresses_read_from_disk_with_the_same_checks() {
    let src = Fixture::new();
    src.write("packs/blob", BODY);
    std::fs::create_dir_all(src.0.join("packs/folder")).unwrap();
    let url = |name: &str| Url::from_file_path(src.0.join(name)).unwrap().to_string();

    let e = Expect { size: Some(BODY.len() as u64), sha256: Some(Sha::of(BODY)), limit: 1 << 10 };
    let (f, dest) = folder();
    let mut seen = vec![];
    let got = get_to_file(&url("packs/blob"), &dest, &e, |p| seen.push(p), &no_cancel()).unwrap();
    assert_eq!(got, Got { size: BODY.len() as u64, sha256: Sha::of(BODY) });
    assert_eq!(std::fs::read(&dest).unwrap(), BODY);
    assert_eq!(names(&f.0), ["blob.wav"]);
    assert_eq!(seen.last(), Some(&Progress { done: BODY.len() as u64, total: Some(BODY.len() as u64) }));
    assert_eq!(get(&url("packs/blob"), 1 << 10).unwrap(), BODY);

    let missing = to_file(&url("packs/none"), &e).unwrap_err();
    assert!(matches!(&missing, Error::Disk(why) if why.contains("isn't there")), "{missing:?}");
    let folder_err = to_file(&url("packs/folder"), &e).unwrap_err();
    assert!(matches!(&folder_err, Error::Disk(why) if why.contains("folder")), "{folder_err:?}");
    assert_eq!(to_file(&url("packs/blob"), &expect(10)).unwrap_err(), Error::TooBig { size: Some(BODY.len() as u64), limit: 10 });
    assert_eq!(get(&url("packs/blob"), 10).unwrap_err(), Error::TooBig { size: Some(BODY.len() as u64), limit: 10 });
    let wrong = Expect { sha256: Some(Sha::of(b"other")), ..e };
    assert_eq!(to_file(&url("packs/blob"), &wrong).unwrap_err(), Error::WrongHash);
    let short = Expect { size: Some(3), ..e };
    assert_eq!(to_file(&url("packs/blob"), &short).unwrap_err(), Error::WrongSize { expected: 3, got: Some(BODY.len() as u64) });
    assert_eq!(get_to_file(&url("packs/blob"), &dest, &e, |_| {}, &AtomicBool::new(true)).unwrap_err(), Error::Cancelled);
    assert_eq!(std::fs::read(&dest).unwrap(), BODY, "a failed copy leaves dest as it was");
    assert_eq!(names(&f.0), ["blob.wav"]);
}

// NET-3: the client for https builds, with the OS's certificates, without connecting anywhere.
#[test]
fn the_https_client_builds_with_the_oss_certificates() {
    client().unwrap();
    // A server that doesn't speak TLS can't pass for one.
    let s = Server::new(vec![ok(BODY)]);
    let e = to_file(&format!("https://{}/a", s.addr), &expect(1 << 10)).unwrap_err();
    assert_eq!(e, Error::Unsafe { host: s.host() });
}

// NET-6: what a failure says names the host only: a token in the path or the query never shows.
#[test]
fn errors_never_show_the_address() {
    let s = Server::new(vec![status(404), partial(100, 10, hang_up)]);
    let secret = "/private-folder/x.json?token=hunter2";
    let port = closed_port();
    let errors = [
        to_file(&s.url(secret), &expect(1 << 10)).unwrap_err(),
        to_file(&s.url(secret), &expect(1 << 10)).unwrap_err(),
        to_file(&format!("http://127.0.0.1:{port}{secret}"), &expect(1 << 10)).unwrap_err(),
        to_file(&format!("http://packs.example{secret}"), &expect(1 << 10)).unwrap_err(),
        to_file(&format!("ftp://packs.example{secret}"), &expect(1 << 10)).unwrap_err(),
        to_file(&format!("https://alice:hunter2@packs.example{secret}"), &expect(1 << 10)).unwrap_err(),
        to_file("not an address hunter2", &expect(1 << 10)).unwrap_err(),
    ];
    for e in errors {
        let (said, debug) = (e.to_string(), format!("{e:?}"));
        for word in ["hunter2", "private-folder", "token", "alice"] {
            assert!(!said.contains(word) && !debug.contains(word), "{word} in {said:?} / {debug}");
        }
    }
}

// NET-6: fetch blocks, so it is called from a thread of its own; async code gets a loud panic.
#[test]
#[should_panic(expected = "Cannot start a runtime from within a runtime")]
fn a_call_from_async_code_panics() {
    let s = Server::new(vec![ok(BODY)]);
    let url = s.url("/a");
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async { get(&url, 1 << 10) }).ok();
}

#[test]
fn a_call_from_a_blocking_thread_of_a_runtime_works() {
    let s = Server::new(vec![ok(BODY)]);
    let url = s.url("/a");
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
    let got = rt.block_on(async move { tokio::task::spawn_blocking(move || get(&url, 1 << 10)).await.unwrap() });
    assert_eq!(got.unwrap(), BODY);
}

#[test]
fn sizes_are_said_in_words() {
    assert_eq!(Error::TooBig { size: Some(900), limit: 512 }.to_string(), "the file was 900 bytes, more than the 512 bytes allowed");
    assert_eq!(Error::TooBig { size: Some(1536), limit: 1024 }.to_string(), "the file was 1.5 KB, more than the 1 KB allowed");
    assert_eq!(Error::TooBig { size: Some(3 << 30), limit: 2 << 30 }.to_string(), "the file was 3 GB, more than the 2 GB allowed");
    // When the two would read the same, the bytes say it.
    assert_eq!(
        Error::TooBig { size: Some((10 << 20) + 1), limit: 10 << 20 }.to_string(),
        "the file was 10485761 bytes, more than the 10485760 bytes allowed"
    );
}

// NET-1: only fetch names the HTTP and TLS crates, and only the editor depends on them.
#[test]
fn only_fetch_reaches_the_network() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap();
    let mut files = vec![];
    for dir in ["core", "crates", "apps", "std/kernels-src"] {
        walk(&root.join(dir), &mut files);
    }
    let mine =
        [root.join("crates/editor/src/fetch.rs"), root.join("crates/editor/src/fetch_tests.rs"), root.join("crates/editor/Cargo.toml")];
    let mut found = vec![];
    for f in files.iter().filter(|f| !mine.contains(f)) {
        let text = std::fs::read_to_string(f).unwrap_or_default();
        for word in ["reqwest", "rustls", "hyper", "ureq", "curl"] {
            if text.lines().any(|l| has_word(l, word)) {
                found.push(format!("{}: {word}", f.strip_prefix(&root).unwrap().display()));
            }
        }
    }
    assert!(found.is_empty(), "only crates/editor/src/fetch.rs reaches the network (NET-1):\n{}", found.join("\n"));
    assert!(files.len() > 100, "the walk found the code: {}", files.len());
}

/// Whether `line` has `word` as a word of its own: `curl`, never `curly`.
fn has_word(line: &str, word: &str) -> bool {
    let part = |c: char| c.is_ascii_alphanumeric() || c == '_';
    line.match_indices(word)
        .any(|(at, _)| !line[..at].chars().next_back().is_some_and(part) && !line[at + word.len()..].chars().next().is_some_and(part))
}

/// Every `.rs` file and `Cargo.toml` under `dir`, but in `target` and `node_modules`.
fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        if p.is_dir() {
            if name != "target" && name != "node_modules" && !name.starts_with('.') {
                walk(&p, out);
            }
        } else if name.ends_with(".rs") || name == "Cargo.toml" {
            out.push(p);
        }
    }
}
