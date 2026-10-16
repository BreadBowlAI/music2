//! S's runner, for G7f (RENDER-32): plays S, HEAD `8aa301e`, the commit before step 7, in a process
//! of its own, while m2-gates measures the app beside it in turns over the same music (`wire.rs`).
//!
//! Built only in a copy of S, as an example of S's m2-gates (`build.sh` in this folder), so it uses
//! S's API alone: `Lab::compile`, `Renderer::new`, `set_strict`, `set_transport`, `render_block`,
//! `Player::driven`, `open`, `seek`, `play`, `Driver::fill` and `drain_retired`, each as m2-gates
//! calls the same thing on HEAD's side. Its kernels are S's own std's.
//!
//!   s_runner <work folder>
//!
//! It reads commands on stdin and answers on stdout; what goes wrong goes to stderr as well.

// The runner answers: the client's half is m2-gates'.
#[allow(dead_code)]
mod wire;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use m2_gates::{Lab, Song};
use m2_ir::{FNode, FOp};
use m2_play::{Driver, Player};
use m2_runtime::{BLOCK, Renderer, Transport};

/// What plays.
enum Plays {
    Nothing,
    /// A player whose callback this runs, and its device buffer.
    Player {
        _player: Player,
        d: Driver,
        buf: Vec<f32>,
    },
    /// A renderer, and the next block's index.
    Renderer {
        r: Renderer,
        k: u64,
    },
}

struct S {
    lab: Lab,
    /// Each project compiled once, by its folder.
    songs: HashMap<PathBuf, Song>,
    plays: Plays,
}

fn say(e: anyhow::Error) -> String {
    format!("{e:#}")
}

impl S {
    fn song(&mut self, dir: &Path) -> Result<Song, String> {
        if !self.songs.contains_key(dir) {
            let song = self.lab.compile(dir).map_err(say)?;
            self.songs.insert(dir.to_path_buf(), song);
        }
        Ok(self.songs[dir].clone())
    }

    /// One callback or block, timed in µs; what it played is then added to `heard`, and what the
    /// callback retired dropped, off the clock, as m2-gates does on its side.
    fn step(&mut self, heard: &mut wire::Heard) -> Result<f64, String> {
        let t = Instant::now();
        match &mut self.plays {
            Plays::Nothing => Err("nothing plays: send player or renderer first".into()),
            Plays::Player { d, buf, .. } => {
                d.fill(buf, 2);
                let us = t.elapsed().as_secs_f64() * 1e6;
                drop(d.drain_retired());
                heard.add(buf);
                Ok(us)
            }
            Plays::Renderer { r, k } => {
                let out = r.render_block(*k * BLOCK as u64, false).map_err(say)?;
                let us = t.elapsed().as_secs_f64() * 1e6;
                heard.add_frames(out);
                *k += 1;
                Ok(us)
            }
        }
    }
}

impl wire::Deck for S {
    fn player(&mut self, dir: &Path, from: f64, frames: usize) -> Result<(), String> {
        self.plays = Plays::Nothing;
        let song = self.song(dir)?;
        let mut r = Renderer::new(&song.flat, &song.data, &self.lab.packages, song.rate).map_err(say)?;
        r.set_strict(false);
        let (player, d) = Player::driven(song.rate);
        player.open(r).map_err(say)?;
        player.seek(song.beat((from * song.rate as f64) as u64)).map_err(say)?;
        player.play();
        self.plays = Plays::Player { _player: player, d, buf: vec![0.0; 2 * frames] };
        Ok(())
    }

    fn renderer(&mut self, dir: &Path, from: f64) -> Result<Vec<(String, f64)>, String> {
        self.plays = Plays::Nothing;
        let song = self.song(dir)?;
        let t = Instant::now();
        let mut r = Renderer::new(&song.flat, &song.data, &self.lab.packages, song.rate).map_err(say)?;
        let ms = t.elapsed().as_secs_f64() * 1e3;
        // Live: non-finite samples silenced, as the engine plays.
        r.set_strict(false);
        r.set_transport(Transport::starting((from * song.rate as f64).round() as u64));
        self.plays = Plays::Renderer { r, k: 0 };
        Ok(vec![("build_ms".into(), ms), ("count".into(), instances(&song.flat.nodes, &song.flat.order) as f64)])
    }

    fn warm(&mut self, n: usize) -> Result<(), String> {
        let mut nobody = wire::Heard::default();
        for _ in 0..n {
            self.step(&mut nobody)?;
        }
        Ok(())
    }

    fn turn(&mut self, lead: usize, n: usize, times: &mut Vec<f64>, heard: &mut wire::Heard) -> Result<(), String> {
        let mut nobody = wire::Heard::default();
        for _ in 0..lead {
            times.push(self.step(&mut nobody)?);
        }
        for _ in 0..n {
            times.push(self.step(heard)?);
        }
        Ok(())
    }

    fn drop_it(&mut self) -> f64 {
        let t = Instant::now();
        self.plays = Plays::Nothing;
        t.elapsed().as_secs_f64() * 1e3
    }
}

/// Kernel instances a renderer of `nodes` builds, counted as m2-gates counts them.
fn instances(nodes: &[FNode], order: &[usize]) -> usize {
    order
        .iter()
        .map(|&i| match &nodes[i].op {
            FOp::Kernel { .. } => 1,
            FOp::Poly { voices, voice, .. } => *voices as usize * instances(&voice.nodes, &voice.order),
            _ => 0,
        })
        .sum()
}

fn main() {
    let Some(work) = std::env::args_os().nth(1) else {
        eprintln!("s_runner: name a work folder; m2-gates starts this runner itself (crates/gates/s/README.md)");
        std::process::exit(2);
    };
    let lab = match Lab::new(Path::new(&work)) {
        Ok(lab) => lab,
        Err(e) => {
            eprintln!("s_runner: {e:#}");
            std::process::exit(2);
        }
    };
    let mut s = S { lab, songs: HashMap::new(), plays: Plays::Nothing };
    // Which code this is, and which commit it was built at (`build.sh` says).
    let ready = wire::Reply::Ready {
        code: wire::code(include_bytes!("main.rs"), include_bytes!("wire.rs")),
        s: option_env!("M2_S_COMMIT").unwrap_or("unknown").to_string(),
    };
    let stdin = std::io::stdin();
    if let Err(e) = wire::serve(ready, stdin.lock(), std::io::stdout().lock(), &mut s) {
        eprintln!("s_runner: {e}");
        std::process::exit(1);
    }
}
