//! The app's WebSocket as a client reads it (RFC 6455): `m2 watch` follows the open song through it
//! (AGENT-6), and the tests read what a socket hears. Small on purpose: text messages in, a pong
//! for each ping, and a close; plain TCP to 127.0.0.1, as `m2` speaks HTTP to the app. No client
//! crate: one would change `Cargo.lock`. A length it can't hold or past [`LARGEST`] is an error, not
//! a panic or a wait; a message's parts are kept across a read that timed out. The answer's
//! `Sec-WebSocket-Accept` isn't checked (gaps: a SHA-1 this crate doesn't have).

use std::hash::{BuildHasher, Hasher};
use std::io::{self, Read, Write};
use std::net::TcpStream;

/// An open socket to the app's `/api/ws`.
pub struct Socket {
    stream: TcpStream,
    /// Bytes read past what was taken: the next frame's start.
    buf: Vec<u8>,
    /// The parts of a message read so far, kept when a read times out before its last.
    message: Vec<u8>,
}

/// The largest message read, and so the largest frame: 16 MiB, far more than any event the app
/// sends (a step names its files; a graph event its problems).
pub const LARGEST: usize = 16 << 20;

/// A frame's opcodes (RFC 6455, 5.2).
const TEXT: u8 = 0x1;
const CLOSE: u8 = 0x8;
const PING: u8 = 0x9;

impl Socket {
    /// Open `path` (with its query: `/api/ws?token=…`) on the app at `url` (`http://127.0.0.1:port`).
    pub fn open(url: &str, path: &str) -> io::Result<Socket> {
        let addr = url.trim_start_matches("http://").trim_end_matches('/');
        let mut stream = TcpStream::connect(addr)?;
        let key = base64(&random_bytes());
        let req = format!(
            "GET {path} HTTP/1.1\r\nHost: {addr}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
        );
        stream.write_all(req.as_bytes())?;
        // The answer's head, up to its blank line; what follows it is the first frames.
        let mut buf = vec![];
        let mut chunk = [0u8; 4096];
        let end = loop {
            if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break i + 4;
            }
            let n = stream.read(&mut chunk)?;
            if n == 0 {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "the app closed the socket before it answered"));
            }
            buf.extend_from_slice(&chunk[..n]);
        };
        let head = String::from_utf8_lossy(&buf[..end]).to_string();
        if !head.starts_with("HTTP/1.1 101") {
            let first = head.lines().next().unwrap_or_default().to_string();
            return Err(io::Error::other(format!("the app didn't open a socket: {first}")));
        }
        buf.drain(..end);
        Ok(Socket { stream, buf, message: vec![] })
    }

    /// The next text message, or None once the socket closed (a close frame, or the connection
    /// ending: the app went away). A ping is answered with a pong on the way.
    pub fn message(&mut self) -> io::Result<Option<String>> {
        loop {
            let Some((fin, op, payload)) = self.frame()? else { return Ok(None) };
            match op {
                CLOSE => {
                    let _ = self.send(CLOSE, &[]);
                    return Ok(None);
                }
                PING => self.send(0xA, &payload)?,
                // A text message, or the rest of one (a continuation, 0x0): binary is never sent.
                0x0 | TEXT => {
                    if self.message.len() + payload.len() > LARGEST {
                        self.message.clear();
                        return Err(too_big());
                    }
                    self.message.extend_from_slice(&payload);
                    if fin {
                        let whole = std::mem::take(&mut self.message);
                        return Ok(Some(String::from_utf8_lossy(&whole).into_owned()));
                    }
                }
                _ => {}
            }
        }
    }

    /// How long [`Socket::message`] waits for a message before it fails (None: for ever).
    pub fn wait_at_most(&self, most: Option<std::time::Duration>) -> io::Result<()> {
        self.stream.set_read_timeout(most)
    }

    /// Close it, saying so to the app.
    pub fn close(mut self) {
        let _ = self.send(CLOSE, &[]);
    }

    /// The next frame: whether it ends its message, its opcode and its payload. None when the
    /// connection ended.
    fn frame(&mut self) -> io::Result<Option<(bool, u8, Vec<u8>)>> {
        if !self.fill(2)? {
            return Ok(None);
        }
        let (fin, op, masked) = (self.buf[0] & 0x80 != 0, self.buf[0] & 0x0F, self.buf[1] & 0x80 != 0);
        let (len, mut at) = match self.buf[1] & 0x7F {
            126 => {
                if !self.fill(4)? {
                    return Ok(None);
                }
                (u16::from_be_bytes([self.buf[2], self.buf[3]]) as usize, 4)
            }
            127 => {
                if !self.fill(10)? {
                    return Ok(None);
                }
                let mut b = [0u8; 8];
                b.copy_from_slice(&self.buf[2..10]);
                // Past what a message may be, or what this machine holds: refused before any wait.
                (usize::try_from(u64::from_be_bytes(b)).ok().filter(|n| *n <= LARGEST).ok_or_else(too_big)?, 10)
            }
            n => (n as usize, 2),
        };
        if len > LARGEST {
            return Err(too_big());
        }
        let mask = if masked {
            if !self.fill(at + 4)? {
                return Ok(None);
            }
            let m = [self.buf[at], self.buf[at + 1], self.buf[at + 2], self.buf[at + 3]];
            at += 4;
            Some(m)
        } else {
            None
        };
        if !self.fill(at + len)? {
            return Ok(None);
        }
        let mut payload: Vec<u8> = self.buf[at..at + len].to_vec();
        if let Some(m) = mask {
            payload.iter_mut().enumerate().for_each(|(i, b)| *b ^= m[i % 4]);
        }
        self.buf.drain(..at + len);
        Ok(Some((fin, op, payload)))
    }

    /// Read until `n` bytes wait in the buffer: false when the connection ended first.
    fn fill(&mut self, n: usize) -> io::Result<bool> {
        let mut chunk = [0u8; 1 << 16];
        while self.buf.len() < n {
            match self.stream.read(&mut chunk) {
                Ok(0) => return Ok(false),
                Ok(k) => self.buf.extend_from_slice(&chunk[..k]),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) if e.kind() == io::ErrorKind::ConnectionReset => return Ok(false),
                Err(e) => return Err(e),
            }
        }
        Ok(true)
    }

    /// Send one frame, masked, as a client must (RFC 6455, 5.3).
    fn send(&mut self, op: u8, payload: &[u8]) -> io::Result<()> {
        let mut frame = vec![0x80 | op];
        match payload.len() {
            n if n < 126 => frame.push(0x80 | n as u8),
            n if n <= u16::MAX as usize => {
                frame.push(0x80 | 126);
                frame.extend_from_slice(&(n as u16).to_be_bytes());
            }
            n => {
                frame.push(0x80 | 127);
                frame.extend_from_slice(&(n as u64).to_be_bytes());
            }
        }
        let mask: [u8; 4] = random_bytes()[..4].try_into().expect("four bytes");
        frame.extend_from_slice(&mask);
        frame.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
        self.stream.write_all(&frame)
    }
}

/// A frame or a message longer than [`LARGEST`]: the socket can't go on.
fn too_big() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("the app sent a message longer than {} MiB", LARGEST >> 20))
}

/// 16 bytes no one can guess ahead: the handshake's key and each frame's mask. From the standard
/// library's random hash keys.
fn random_bytes() -> [u8; 16] {
    let half = || std::collections::hash_map::RandomState::new().build_hasher().finish().to_le_bytes();
    let mut out = [0u8; 16];
    out[..8].copy_from_slice(&half());
    out[8..].copy_from_slice(&half());
    out
}

/// `bytes` in base64, with padding.
fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            match i <= c.len() {
                true => out.push(ABC[(n >> (18 - 6 * i) & 63) as usize] as char),
                false => out.push('='),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// A server on 127.0.0.1 that answers the handshake, then sends `frames`, each after its wait
    /// (ms), and keeps the connection open a while: its url.
    fn serving(frames: Vec<(u64, Vec<u8>)>) -> String {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut c, _) = l.accept().unwrap();
            let mut buf = [0u8; 4096];
            let _ = c.read(&mut buf);
            c.write_all(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n").unwrap();
            for (wait, bytes) in frames {
                std::thread::sleep(Duration::from_millis(wait));
                let _ = c.write_all(&bytes);
            }
            std::thread::sleep(Duration::from_secs(3));
        });
        format!("http://{addr}")
    }

    /// An unmasked server frame: `fin`, `op` and `payload`.
    fn frame(fin: bool, op: u8, payload: &[u8]) -> Vec<u8> {
        let mut f = vec![if fin { 0x80 | op } else { op }];
        match payload.len() {
            n if n < 126 => f.push(n as u8),
            n => {
                f.push(127);
                f.extend_from_slice(&(n as u64).to_be_bytes());
            }
        }
        f.extend_from_slice(payload);
        f
    }

    // The review of ag-see, F7: a frame that says it is longer than anything is an error, never a
    // panic, and never a wait for it.
    #[test]
    fn any_length_is_an_error_or_a_message_never_a_panic() {
        for len in [u64::MAX, u64::MAX - 9, 1 << 63, (usize::MAX as u64) - 4] {
            let mut head = vec![0x81, 127];
            head.extend_from_slice(&len.to_be_bytes());
            let mut s = Socket::open(&serving(vec![(0, head)]), "/api/ws").unwrap();
            s.wait_at_most(Some(Duration::from_secs(1))).unwrap();
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| s.message()));
            let r = r.unwrap_or_else(|_| panic!("a length of {len} panicked"));
            assert_eq!(r.map_err(|e| e.kind()), Err(io::ErrorKind::InvalidData), "a length of {len}");
        }
    }

    // F7: a frame or a message bigger than the limit is refused as soon as its header says so,
    // without waiting for it or growing to hold it.
    #[test]
    fn a_message_bigger_than_the_limit_is_refused_at_once() {
        let mut head = vec![0x81, 127];
        head.extend_from_slice(&(1u64 << 30).to_be_bytes());
        let mut s = Socket::open(&serving(vec![(0, head)]), "/api/ws").unwrap();
        s.wait_at_most(Some(Duration::from_secs(2))).unwrap();
        let t = Instant::now();
        assert_eq!(s.message().map_err(|e| e.kind()), Err(io::ErrorKind::InvalidData), "a 1 GiB frame");
        assert!(t.elapsed() < Duration::from_secs(1), "at once: {:?}", t.elapsed());
        // Fragments that add up to more than it.
        let part = vec![b'x'; LARGEST / 4 + 1];
        let frames = (0..5).map(|i| (0, frame(false, if i == 0 { TEXT } else { 0 }, &part))).collect();
        let mut s = Socket::open(&serving(frames), "/api/ws").unwrap();
        s.wait_at_most(Some(Duration::from_secs(5))).unwrap();
        assert_eq!(s.message().map_err(|e| e.kind()), Err(io::ErrorKind::InvalidData), "fragments past the limit");
    }

    // F7: a message in two fragments with a wait between them longer than the read timeout is read
    // whole once its second part comes: the first part is kept, not lost.
    #[test]
    fn a_message_split_across_a_wait_is_kept_whole() {
        let url = serving(vec![(0, frame(false, TEXT, b"hello ")), (600, frame(true, 0, b"world"))]);
        let mut s = Socket::open(&url, "/api/ws").unwrap();
        s.wait_at_most(Some(Duration::from_millis(200))).unwrap();
        let first = s.message();
        assert!(first.as_ref().is_err_and(|e| matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut)), "{first:?}");
        std::thread::sleep(Duration::from_millis(700));
        assert_eq!(s.message().unwrap(), Some("hello world".to_string()));
    }

    #[test]
    fn base64_is_rfc_4648s() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        // RFC 6455's sample key: 16 bytes are 24 characters.
        assert_eq!(base64(b"the sample nonce"), "dGhlIHNhbXBsZSBub25jZQ==");
    }
}
