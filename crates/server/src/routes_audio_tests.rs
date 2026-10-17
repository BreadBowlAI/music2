//! The audio routes through a running server: a waveform's peaks (Q9).

use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::Duration;

use crate::graph::Graph;
use crate::peaks::Peaks;
use crate::{ServeOptions, start_with};

const SONG: &str = "import { song, track } from '@m2/core';\n\
                    const vox = track('vox', ({ lane }) => lane.audio('clips'));\nexport default song(() => vox.out);\n";

/// A song with one audio track playing `assets/tone.wav`, in a folder of its own.
fn project(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("m2-routes-audio-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("data/vox")).unwrap();
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    let manifest = "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.2\"\n";
    std::fs::write(dir.join("music2.toml"), manifest).unwrap();
    std::fs::write(dir.join("song.ts"), SONG).unwrap();
    let spec = hound::WavSpec { channels: 1, sample_rate: 48_000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(dir.join("assets/tone.wav"), spec).unwrap();
    for i in 0..4_800 {
        w.write_sample(((i as f32 / 10.0).sin() * 16_000.0) as i16).unwrap();
    }
    w.finalize().unwrap();
    std::fs::write(dir.join("data/vox/clips.json"), r#"{"clips": [{"id": "a", "start": 0, "length": 4, "asset": "tone.wav"}]}"#).unwrap();
    dir
}

/// GET `path` with the token: the status and the body's bytes. Fails rather than wait past a minute.
fn get(url: &str, path: &str) -> (u16, Vec<u8>) {
    let addr = url.trim_start_matches("http://");
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
    let req = format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nx-m2-token: t0k\r\nConnection: close\r\n\r\n");
    s.write_all(req.as_bytes()).unwrap();
    let mut answer = vec![];
    s.read_to_end(&mut answer).expect("an answer, not a wait");
    let head_end = answer.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let head = String::from_utf8_lossy(&answer[..head_end]).to_string();
    let status = head[9..12].parse().unwrap();
    let mut body = answer[head_end + 4..].to_vec();
    if head.to_ascii_lowercase().contains("transfer-encoding: chunked") {
        body = unchunk(&body);
    }
    (status, body)
}

fn unchunk(mut b: &[u8]) -> Vec<u8> {
    let mut out = vec![];
    loop {
        let eol = b.windows(2).position(|w| w == b"\r\n").unwrap();
        let n = usize::from_str_radix(std::str::from_utf8(&b[..eol]).unwrap().trim(), 16).unwrap();
        if n == 0 {
            return out;
        }
        out.extend_from_slice(&b[eol + 2..eol + 2 + n]);
        b = &b[eol + 2 + n + 2..];
    }
}

// Q9: the peaks of an asset, by the open song's session; a name that isn't an asset's, or another
// session, is refused in words.
#[test]
fn peaks_of_an_asset() {
    let dir = project("peaks");
    let running = start_with(ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "t0k".into() }, Graph::silent()).unwrap();
    let session = running.state.graph.open(&dir).unwrap()["session"].as_u64().unwrap();
    let (status, body) = get(&running.url, &format!("/api/graph/audio/peaks?session={session}&asset=tone.wav"));
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    let p = Peaks::read(&body).unwrap();
    assert_eq!((p.channels, p.rate, p.frames), (1, 48_000, 4_800));
    assert_eq!(p.data.iter().max(), Some(&62), "the tone at half level: 16,000 of 32,768");
    let (status, body) = get(&running.url, &format!("/api/graph/audio/peaks?session={session}&asset=..%2Fsong.ts"));
    assert_eq!(status, 400, "{}", String::from_utf8_lossy(&body));
    let (status, body) = get(&running.url, &format!("/api/graph/audio/peaks?session={}&asset=tone.wav", session + 1));
    assert!(status == 400 && String::from_utf8_lossy(&body).contains("changed"), "{status} {}", String::from_utf8_lossy(&body));
    let (status, body) = get(&running.url, &format!("/api/graph/audio/peaks?session={session}&asset=gone.wav"));
    assert!(status == 400 && String::from_utf8_lossy(&body).contains("gone.wav"), "{status} {}", String::from_utf8_lossy(&body));
    let _ = std::fs::remove_dir_all(&dir);
}

// Guard: the peaks answer while the session is busy (held as a long compile would hold it).
#[test]
fn peaks_never_wait_for_the_actor() {
    let dir = project("busy");
    let running = start_with(ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "t0k".into() }, Graph::silent()).unwrap();
    let session = running.state.graph.open(&dir).unwrap()["session"].as_u64().unwrap();
    let held = running.state.graph.hold();
    let (status, body) = get(&running.url, &format!("/api/graph/audio/peaks?session={session}&asset=tone.wav"));
    drop(held);
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    assert_eq!(Peaks::read(&body).unwrap().frames, 4_800);
    let _ = std::fs::remove_dir_all(&dir);
}
