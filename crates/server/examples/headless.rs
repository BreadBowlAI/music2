//! The app's server with no window, for automated UI tests.
//!
//!   M2_AUDIO=none cargo run -p m2-server --example headless -- <project-folder | -> <session.json> <token>
//!
//! Opens the project (`-`: none, for the start screen) and writes `{url, token, pid}` to the session file.
//!
//! It serves the `web/dist` of the tree it was built in, or with `M2_WEB=<folder>` that folder: one
//! server build then serves another checkout's page. std, the templates and the examples still come
//! from the tree it was built in.
//!
//! With `M2_AUDIO_CAPTURE=<file>`, what the app plays goes into that file instead of a device, in
//! real time, so a test can listen to it: interleaved stereo `f32` at 48 kHz, little-endian,
//! appended a device buffer at a time (`M2_AUDIO_FRAMES` frames, 512 unless set) as each is played.

use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn main() -> anyhow::Result<()> {
    // HOST-2: the server runs on a worker while the main thread serves plugins as their main
    // thread (the OS's main loop), and destroys what they created before the process exits.
    m2_plugins::run_main(serve)
}

fn serve() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [project, session, token] = &args[..] else { anyhow::bail!("usage: headless <project-folder | -> <session.json> <token>") };
    let opts = m2_server::ServeOptions {
        port: 0,
        web_dir: std::env::var_os("M2_WEB")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../web/dist"))),
        token: token.clone(),
    };
    let running = match std::env::var_os("M2_AUDIO_CAPTURE") {
        Some(file) => m2_server::start_with(opts, m2_server::graph::Graph::with_player(capture(file.into())?))?,
        None => m2_server::start(opts)?,
    };
    if project != "-" {
        running.state.graph.open(std::path::Path::new(project))?;
    }
    let body = serde_json::json!({"url": running.url, "token": token, "pid": std::process::id()});
    std::fs::write(session, serde_json::to_string_pretty(&body)?)?;
    println!("{}", running.url);
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}

/// A player whose device is `file`: a clock calls it back a device buffer at a time, as the silent
/// player does, and each buffer it played is appended to the file. What it retires is dropped
/// there, after the buffer is written.
fn capture(file: PathBuf) -> anyhow::Result<m2_play::Player> {
    const RATE: u32 = 48_000;
    let frames: usize = std::env::var("M2_AUDIO_FRAMES").ok().and_then(|v| v.parse().ok()).filter(|&n| n > 0).unwrap_or(512);
    let (player, mut driver) = m2_play::Player::driven(RATE);
    let mut out = std::io::BufWriter::new(std::fs::File::create(&file)?);
    std::thread::Builder::new().name("m2-play-capture".into()).spawn(move || {
        let mut buf = vec![0.0f32; frames * 2];
        let mut bytes = Vec::with_capacity(buf.len() * 4);
        let period = Duration::from_secs_f64(frames as f64 / RATE as f64);
        let mut next = Instant::now();
        loop {
            driver.fill(&mut buf, 2);
            bytes.clear();
            bytes.extend(buf.iter().flat_map(|x| x.to_le_bytes()));
            if out.write_all(&bytes).and_then(|()| out.flush()).is_err() {
                eprintln!("headless: couldn't write the capture; it stops");
                return;
            }
            drop(driver.drain_retired());
            next += period;
            // A callback that ran late doesn't make the next ones early.
            next = next.max(Instant::now().checked_sub(period).unwrap_or(next));
            std::thread::sleep(next.saturating_duration_since(Instant::now()));
        }
    })?;
    Ok(player)
}
