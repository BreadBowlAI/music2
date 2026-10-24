//! The app's WebSocket as a client reads it (RFC 6455): `m2 watch` follows the open song through it
//! (AGENT-6), and the tests read what a socket hears. `tungstenite`'s blocking client, on plain TCP
//! to 127.0.0.1, as `m2` speaks HTTP to the app: it checks the answer's `Sec-WebSocket-Accept`,
//! answers each ping, and keeps a message's parts across a read that timed out. A frame or a message
//! past [`LARGEST`] is an error, refused as soon as its header says so: not a panic or a wait.

use std::io;
use std::net::TcpStream;
use std::time::Duration;

use tungstenite::error::ProtocolError;
use tungstenite::handshake::HandshakeError;
use tungstenite::protocol::WebSocketConfig;
use tungstenite::{Error, Message, WebSocket};

/// An open socket to the app's `/api/ws`.
pub struct Socket(WebSocket<TcpStream>);

/// The largest message read, and so the largest frame: 16 MiB, far more than any event the app
/// sends (a step names its files; a graph event its problems).
pub const LARGEST: usize = 16 << 20;

impl Socket {
    /// Open `path` (with its query: `/api/ws?token=…`) on the app at `url` (`http://127.0.0.1:port`).
    pub fn open(url: &str, path: &str) -> io::Result<Socket> {
        let addr = url.trim_start_matches("http://").trim_end_matches('/');
        let stream = TcpStream::connect(addr)?;
        let config = WebSocketConfig::default().max_message_size(Some(LARGEST)).max_frame_size(Some(LARGEST));
        match tungstenite::client::client_with_config(format!("ws://{addr}{path}"), stream, Some(config)) {
            Ok((ws, _)) => Ok(Socket(ws)),
            Err(HandshakeError::Failure(Error::Io(e))) => Err(e),
            Err(HandshakeError::Failure(Error::Http(answer))) => {
                Err(io::Error::other(format!("the app didn't open a socket: {}", answer.status())))
            }
            Err(e) => Err(io::Error::other(format!("the app didn't open a socket: {e}"))),
        }
    }

    /// The next text message, or None once the socket closed (a close frame, or the connection
    /// ending: the app went away). A ping is answered with a pong on the way.
    pub fn message(&mut self) -> io::Result<Option<String>> {
        loop {
            match self.0.read() {
                Ok(Message::Text(text)) => return Ok(Some(text.as_str().to_owned())),
                // Its answer goes out now; tungstenite queued it.
                Ok(Message::Close(_)) => {
                    let _ = self.0.flush();
                    return Ok(None);
                }
                // A ping's pong goes out on the next read. Binary is never sent.
                Ok(_) => {}
                Err(Error::ConnectionClosed | Error::AlreadyClosed | Error::Protocol(ProtocolError::ResetWithoutClosingHandshake)) => {
                    return Ok(None);
                }
                Err(Error::Io(e)) if e.kind() == io::ErrorKind::ConnectionReset => return Ok(None),
                // A read that timed out (WouldBlock or TimedOut) among them: what came of the
                // message so far stays in the socket.
                Err(Error::Io(e)) => return Err(e),
                Err(Error::Capacity(_)) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("the app sent a message longer than {} MiB", LARGEST >> 20),
                    ));
                }
                Err(e) => return Err(io::Error::new(io::ErrorKind::InvalidData, e)),
            }
        }
    }

    /// How long [`Socket::message`] waits for a message before it fails (None: for ever).
    pub fn wait_at_most(&self, most: Option<Duration>) -> io::Result<()> {
        self.0.get_ref().set_read_timeout(most)
    }

    /// Close it, saying so to the app.
    pub fn close(mut self) {
        let _ = self.0.close(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::time::Instant;

    /// A text frame's opcode (RFC 6455, 5.2); 0x0 continues a message.
    const TEXT: u8 = 0x1;

    /// A server on 127.0.0.1 that answers the handshake, then sends `frames`, each after its wait
    /// (ms), and keeps the connection open a while: its url.
    fn serving(frames: Vec<(u64, Vec<u8>)>) -> String {
        serving_with(|key| switching(Some(&tungstenite::handshake::derive_accept_key(key.as_bytes()))), frames)
    }

    /// A 101 that says `accept` as its `Sec-WebSocket-Accept`, or nothing.
    fn switching(accept: Option<&str>) -> String {
        let accept = accept.map(|a| format!("Sec-WebSocket-Accept: {a}\r\n")).unwrap_or_default();
        format!("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n{accept}\r\n")
    }

    /// As [`serving`], answering the handshake with what `answer` makes of the key it was sent.
    fn serving_with(answer: impl FnOnce(&str) -> String + Send + 'static, frames: Vec<(u64, Vec<u8>)>) -> String {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut c, _) = l.accept().unwrap();
            // The handshake, up to its blank line, a byte at a time: nothing past it is taken.
            let (mut head, mut b) = (vec![], [0u8; 1]);
            while !head.ends_with(b"\r\n\r\n") && c.read(&mut b).unwrap_or(0) == 1 {
                head.push(b[0]);
            }
            let head = String::from_utf8_lossy(&head).into_owned();
            let key = head.lines().filter_map(|l| l.split_once(':')).find(|(k, _)| k.trim().eq_ignore_ascii_case("sec-websocket-key"));
            c.write_all(answer(key.map_or("", |(_, v)| v.trim())).as_bytes()).unwrap();
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

    // AGENT-6: the answer must accept the key this socket sent (RFC 6455, 4.1): a 101 without
    // `Sec-WebSocket-Accept`, or with another key's, opens nothing, whatever follows it.
    #[test]
    fn an_answer_that_doesnt_accept_its_key_opens_nothing() {
        // RFC 6455's sample: the accept of the key "dGhlIHNhbXBsZSBub25jZQ==", never this socket's.
        for accept in [None, Some("s3pPLMBiTxaQ9kYGzzhZRbK+xOo=")] {
            let url = serving_with(move |_| switching(accept), vec![(0, frame(true, TEXT, b"hello"))]);
            let opened = Socket::open(&url, "/api/ws");
            assert!(opened.is_err(), "a 101 with {accept:?} as its Sec-WebSocket-Accept opened a socket");
        }
    }
}
