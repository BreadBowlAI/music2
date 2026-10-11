//! The audio device: the one place the core touches hardware.
//!
//! The audio callback owns a [`Renderer`]. Everything else talks to it through a
//! bounded channel (swap in a new renderer, play, stop, change a written value) and
//! reads its position and state from atomics. Renderers being replaced go back over a
//! second channel and are dropped off the audio thread.
//!
//! Live mode: a NaN from a broken kernel is silenced, not an error.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use anyhow::{Result, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{Receiver, Sender, bounded};
use m2_ir::Origin;
use m2_runtime::Renderer;

enum Msg {
    Swap(Box<Renderer>),
    Play,
    Stop,
    SetConst(Origin, f64),
}

#[derive(Default)]
struct Shared {
    /// Frames rendered by the current renderer (its playback position).
    position: AtomicU64,
    playing: AtomicBool,
}

struct Callback {
    renderer: Option<Box<Renderer>>,
    playing: bool,
    scratch: Vec<[f32; 2]>,
    rx: Receiver<Msg>,
    retired: Sender<Box<Renderer>>,
    shared: Arc<Shared>,
}

impl Callback {
    fn fill(&mut self, data: &mut [f32], channels: usize) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Swap(r) => {
                    if let Some(old) = self.renderer.replace(r) {
                        let _ = self.retired.try_send(old);
                    }
                }
                Msg::Play => self.playing = true,
                Msg::Stop => self.playing = false,
                Msg::SetConst(origin, v) => {
                    if let Some(r) = &mut self.renderer {
                        r.set_const(&origin, v);
                    }
                }
            }
        }
        let frames = data.len() / channels.max(1);
        let (Some(r), true) = (&mut self.renderer, self.playing) else {
            data.fill(0.0);
            self.shared.playing.store(false, Ordering::Relaxed);
            return;
        };
        let mut done = 0;
        while done < frames {
            let n = (frames - done).min(self.scratch.len());
            if r.process(&mut self.scratch[..n]).is_err() {
                // A kernel crashed: stop rather than make noise.
                self.playing = false;
                data[done * channels..].fill(0.0);
                break;
            }
            for (i, f) in self.scratch[..n].iter().enumerate() {
                let o = (done + i) * channels;
                data[o] = f[0];
                if channels > 1 {
                    data[o + 1] = f[1];
                    data[o + 2..o + channels].fill(0.0);
                }
            }
            done += n;
        }
        self.shared.position.store(r.position(), Ordering::Relaxed);
        self.shared.playing.store(self.playing, Ordering::Relaxed);
    }
}

/// A handle to the output stream, which lives on its own thread (cpal streams can't
/// move between threads on every platform). Cheap to clone.
#[derive(Clone)]
pub struct Player {
    tx: Sender<Msg>,
    shared: Arc<Shared>,
    pub sample_rate: u32,
    pub device: String,
}

impl Player {
    /// Open the default output device. With `M2_AUDIO=none`, run silently on a clock
    /// instead (for automated tests).
    pub fn start() -> Result<Player> {
        if std::env::var("M2_AUDIO").as_deref() == Ok("none") {
            return Player::silent();
        }
        let (ready_tx, ready) = bounded::<Result<Player>>(1);
        std::thread::Builder::new().name("m2-play".into()).spawn(move || {
            let open = || -> Result<(cpal::Stream, Player, Receiver<Box<Renderer>>)> {
                let device = cpal::default_host().default_output_device().ok_or_else(|| anyhow!("no audio output device"))?;
                let name = device.description().map(|d| d.name().to_string()).unwrap_or_else(|_| "output".into());
                let config = device.default_output_config()?.config();
                let channels = config.channels as usize;
                let (tx, rx) = bounded::<Msg>(1024);
                let (retired_tx, retired) = bounded::<Box<Renderer>>(8);
                let shared = Arc::new(Shared::default());
                let mut cb = Callback {
                    renderer: None,
                    playing: false,
                    scratch: vec![[0.0; 2]; 8192],
                    rx,
                    retired: retired_tx,
                    shared: shared.clone(),
                };
                let sample_rate = config.sample_rate;
                let stream = device.build_output_stream::<f32, _, _>(
                    config,
                    move |data: &mut [f32], _| cb.fill(data, channels),
                    |e| eprintln!("music2: audio stream error: {e}"),
                    None,
                )?;
                stream.play()?;
                Ok((stream, Player { tx, shared, sample_rate, device: name }, retired))
            };
            match open() {
                Ok((_stream, player, retired)) => {
                    let _ = ready_tx.send(Ok(player));
                    // Keep the stream alive, and drop retired renderers here.
                    while let Ok(r) = retired.recv() {
                        drop(r);
                    }
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                }
            }
        })?;
        ready.recv()?
    }

    /// The same player, rendering into nothing in real time: no device, no sound.
    pub fn silent() -> Result<Player> {
        const SR: u32 = 48_000;
        const FRAMES: usize = 480;
        let (tx, rx) = bounded::<Msg>(1024);
        let (retired_tx, retired) = bounded::<Box<Renderer>>(8);
        let shared = Arc::new(Shared::default());
        let mut cb =
            Callback { renderer: None, playing: false, scratch: vec![[0.0; 2]; FRAMES], rx, retired: retired_tx, shared: shared.clone() };
        std::thread::Builder::new().name("m2-play-silent".into()).spawn(move || {
            let mut buf = vec![0.0f32; FRAMES * 2];
            loop {
                cb.fill(&mut buf, 2);
                while retired.try_recv().is_ok() {}
                std::thread::sleep(std::time::Duration::from_secs_f64(FRAMES as f64 / SR as f64));
            }
        })?;
        Ok(Player { tx, shared, sample_rate: SR, device: "none".into() })
    }

    /// Play `renderer` from its current position (seek it first if needed).
    pub fn load(&self, mut renderer: Renderer) {
        renderer.set_strict(false);
        let _ = self.tx.send(Msg::Swap(Box::new(renderer)));
    }

    pub fn play(&self) {
        let _ = self.tx.send(Msg::Play);
    }

    pub fn stop(&self) {
        let _ = self.tx.send(Msg::Stop);
    }

    /// Change a written value in the playing renderer, from the next block.
    pub fn set_const(&self, origin: Origin, value: f64) {
        let _ = self.tx.try_send(Msg::SetConst(origin, value));
    }

    /// Deliver an accepted value or rollback even when previews filled the queue.
    /// Call from the host's worker thread; this waits for room in the audio queue.
    pub fn commit_const(&self, origin: Origin, value: f64) -> Result<()> {
        self.tx.send(Msg::SetConst(origin, value)).map_err(|_| anyhow!("the audio player stopped"))
    }

    /// Frames the playing renderer has rendered.
    pub fn position(&self) -> u64 {
        self.shared.position.load(Ordering::Relaxed)
    }

    pub fn is_playing(&self) -> bool {
        self.shared.playing.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_values_wait_for_room_instead_of_being_dropped() {
        let (tx, rx) = bounded(1);
        tx.send(Msg::Play).unwrap();
        let player = Player { tx, shared: Arc::new(Shared::default()), sample_rate: 48_000, device: "test".into() };
        let origin = Origin { site: 3, arg: vec!["0".into()], scale: None };
        let sent = origin.clone();
        let (started_tx, started) = bounded(1);
        let (done_tx, done) = bounded(1);
        let commit = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            let result = player.commit_const(sent, 950.0);
            done_tx.send(()).unwrap();
            result
        });
        started.recv().unwrap();
        assert!(done.recv_timeout(std::time::Duration::from_millis(20)).is_err(), "a full queue must wait instead of dropping the commit");
        assert!(matches!(rx.recv().unwrap(), Msg::Play));
        let Msg::SetConst(actual, value) = rx.recv_timeout(std::time::Duration::from_secs(1)).unwrap() else {
            panic!("expected the committed value")
        };
        assert_eq!(actual, origin);
        assert_eq!(value, 950.0);
        commit.join().unwrap().unwrap();
    }
}
