//! The audio device: the one place the core touches hardware.
//!
//! The audio callback owns a [`Renderer`]. Everything else talks to it through a
//! bounded channel (swap in a new renderer, play, stop, change a written value) and
//! reads its position and state from atomics. Renderers being replaced go back over a
//! second channel and are dropped off the audio thread.
//!
//! The engine runs at the project's `sample_rate`, whatever the device's rate is (RENDER-31):
//! the device is opened at its own rate, and when the two differ the engine's output is
//! converted at the device boundary ([`Resampler`]). Positions are always engine frames.
//!
//! Live mode: a NaN from a broken kernel is silenced, not an error.

mod resample;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use anyhow::{Result, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{Receiver, Sender, bounded};
use m2_ir::Origin;
use m2_runtime::Renderer;
pub use resample::Resampler;

/// What the audio thread plays: a renderer at the engine's rate, and the converter to the
/// device's rate when they differ. Both are made off the audio thread.
struct Playing {
    renderer: Renderer,
    resampler: Option<Resampler>,
}

enum Msg {
    Swap(Box<Playing>),
    Play,
    Stop,
    SetConst(Origin, f64),
}

#[derive(Default)]
struct Shared {
    /// Frames rendered by the current renderer (its playback position).
    position: AtomicU64,
    /// Renderers `load` sent that the audio thread hasn't taken yet, and where the last
    /// one starts. Until it is taken, that is the position: a block of the renderer it
    /// replaces may still finish and store its own.
    loading: AtomicU64,
    loaded: AtomicU64,
    playing: AtomicBool,
    /// The engine rate of the renderer last loaded: what positions count in.
    rate: AtomicU32,
}

struct Callback {
    renderer: Option<Box<Playing>>,
    playing: bool,
    scratch: Vec<[f32; 2]>,
    rx: Receiver<Msg>,
    retired: Sender<Box<Playing>>,
    shared: Arc<Shared>,
}

impl Callback {
    fn fill(&mut self, data: &mut [f32], channels: usize) {
        self.receive();
        self.render(data, channels);
    }

    fn receive(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Swap(r) => {
                    self.shared.position.store(r.renderer.position(), Ordering::Relaxed);
                    self.shared.loading.fetch_sub(1, Ordering::Release);
                    if let Some(old) = self.renderer.replace(r) {
                        let _ = self.retired.try_send(old);
                    }
                }
                Msg::Play => self.playing = true,
                Msg::Stop => self.playing = false,
                Msg::SetConst(origin, v) => {
                    if let Some(r) = &mut self.renderer {
                        r.renderer.set_const(&origin, v);
                    }
                }
            }
        }
    }

    fn render(&mut self, data: &mut [f32], channels: usize) {
        let frames = data.len() / channels.max(1);
        let (Some(r), true) = (&mut self.renderer, self.playing) else {
            data.fill(0.0);
            self.shared.playing.store(false, Ordering::Relaxed);
            return;
        };
        let Playing { renderer: r, resampler } = &mut **r;
        let mut done = 0;
        while done < frames {
            let n = (frames - done).min(self.scratch.len());
            let rendered = match resampler {
                Some(rs) => rs.fill(&mut self.scratch[..n], &mut |buf| r.process(buf)),
                None => r.process(&mut self.scratch[..n]),
            };
            if rendered.is_err() {
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
    /// The device's own rate. The engine runs at the project's.
    pub device_rate: u32,
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
            let open = || -> Result<(cpal::Stream, Player, Receiver<Box<Playing>>)> {
                let device = cpal::default_host().default_output_device().ok_or_else(|| anyhow!("no audio output device"))?;
                let name = device.description().map(|d| d.name().to_string()).unwrap_or_else(|_| "output".into());
                let config = device.default_output_config()?.config();
                let channels = config.channels as usize;
                let (tx, rx) = bounded::<Msg>(1024);
                let (retired_tx, retired) = bounded::<Box<Playing>>(8);
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
                Ok((stream, Player { tx, shared, device_rate: sample_rate, device: name }, retired))
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
        let (retired_tx, retired) = bounded::<Box<Playing>>(8);
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
        Ok(Player { tx, shared, device_rate: SR, device: "none".into() })
    }

    /// Play `renderer` from its current position (seek it first if needed). That is the
    /// position from now on, playing or stopped. It renders at its own rate, the project's; the
    /// device hears it converted to its rate when they differ (RENDER-31).
    pub fn load(&self, mut renderer: Renderer) {
        renderer.set_strict(false);
        let rate = renderer.sample_rate();
        let resampler = (rate != self.device_rate).then(|| Resampler::new(rate, self.device_rate));
        self.shared.loaded.store(renderer.position(), Ordering::Relaxed);
        self.shared.rate.store(rate, Ordering::Relaxed);
        self.shared.loading.fetch_add(1, Ordering::Release);
        let _ = self.tx.send(Msg::Swap(Box::new(Playing { renderer, resampler })));
    }

    /// The rate positions count in: the engine's, which is the project's (RENDER-31). The
    /// device's until something is loaded.
    pub fn rate(&self) -> u32 {
        match self.shared.rate.load(Ordering::Relaxed) {
            0 => self.device_rate,
            r => r,
        }
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

    /// Engine frames the playing renderer has rendered.
    pub fn position(&self) -> u64 {
        if self.shared.loading.load(Ordering::Acquire) > 0 {
            self.shared.loaded.load(Ordering::Relaxed)
        } else {
            self.shared.position.load(Ordering::Relaxed)
        }
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
        let player = Player { tx, shared: Arc::new(Shared::default()), device_rate: 48_000, device: "test".into() };
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

    /// A renderer of silence, at `frame`.
    fn silence(frame: u64) -> Renderer {
        let ir: m2_ir::Ir =
            serde_json::from_value(serde_json::json!({"ir": m2_ir::IR_VERSION, "root": {"module": "song", "export": "out"},
            "modules": [{"key": "song", "lanes": [{"key": "a", "kind": "audio<2>"}],
                "nodes": [{"id": "x", "op": "lane", "lane": "a"}], "exports": {"out": "x"}}]}))
            .unwrap();
        let packages = m2_runtime::PackageSet::default();
        let flat = m2_ir::check(&ir, &packages).unwrap();
        let mut r = Renderer::new(&flat, &m2_song::DataSet::empty(), &packages, 48_000).unwrap();
        r.seek(frame).unwrap();
        r
    }

    /// A renderer of a 1 kHz sine at `rate`.
    fn sine(rate: u32) -> Renderer {
        let ir: m2_ir::Ir =
            serde_json::from_value(serde_json::json!({"ir": m2_ir::IR_VERSION, "root": {"module": "song", "export": "out"},
            "modules": [{"key": "song", "nodes": [
                {"id": "f", "op": "const", "value": 1000, "unit": "hz"},
                {"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "f"}}], "exports": {"out": "o"}}]}))
            .unwrap();
        let mut packages = m2_runtime::PackageSet::default();
        packages.load(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")).unwrap();
        let flat = m2_ir::check(&ir, &packages).unwrap();
        Renderer::new(&flat, &m2_song::DataSet::empty(), &packages, rate).unwrap()
    }

    /// A callback playing into a device at `device_rate`, and its player.
    fn device(device_rate: u32) -> (Player, Callback) {
        let (tx, rx) = bounded(8);
        let (retired, _old) = bounded(8);
        let shared = Arc::new(Shared::default());
        let player = Player { tx, shared: shared.clone(), device_rate, device: "test".into() };
        (player, Callback { renderer: None, playing: false, scratch: vec![[0.0; 2]; 256], rx, retired, shared })
    }

    /// Play `seconds` through the callback in device buffers of uneven sizes, stereo.
    fn hear(cb: &mut Callback, frames: usize) -> Vec<[f32; 2]> {
        let mut out = vec![];
        let sizes = [1, 7, 128, 300, 64, 1000];
        for n in sizes.iter().cycle() {
            let n = (*n).min(frames - out.len());
            if n == 0 {
                break;
            }
            let mut data = vec![0.0; n * 2];
            cb.fill(&mut data, 2);
            out.extend(data.chunks(2).map(|f| [f[0], f[1]]));
        }
        out
    }

    /// Upward zero crossings per second, near enough the frequency.
    fn freq(x: &[[f32; 2]], rate: u32) -> f64 {
        let ups = x.windows(2).filter(|w| w[0][0] < 0.0 && w[1][0] >= 0.0).count();
        ups as f64 * rate as f64 / x.len() as f64
    }

    // RENDER-31, RENDER-33: at the device's own rate, live playback is the offline render bit
    // for bit, whatever the device's buffer sizes.
    #[test]
    fn at_the_devices_rate_live_is_the_offline_render() {
        let (player, mut cb) = device(48_000);
        player.load(sine(48_000));
        player.play();
        let live = hear(&mut cb, 9_600);
        assert_eq!(live, sine(48_000).render(0.2).unwrap());
        assert_eq!((player.rate(), player.position()), (48_000, 9_600));
    }

    // RENDER-31: the engine runs at the project's rate, and the device hears it converted to its
    // own: a 1 kHz sine is 1 kHz either way, and positions count engine frames.
    #[test]
    fn the_engine_runs_at_the_projects_rate_on_any_device() {
        for (project, dev) in [(44_100, 48_000), (96_000, 48_000), (48_000, 44_100)] {
            let (player, mut cb) = device(dev);
            player.load(sine(project));
            player.play();
            let heard = hear(&mut cb, dev as usize / 2);
            assert_eq!(player.rate(), project);
            let f = freq(&heard[dev as usize / 10..], dev);
            assert!((f - 1000.0).abs() < 5.0, "{project} on {dev}: {f} Hz");
            // Half a second of the device is half a second of the engine, give or take the
            // converter's lookahead (its filter's half, and a chunk pulled at a time).
            let ahead = project as i64 / 2 - player.position() as i64;
            assert!((-96..=0).contains(&ahead), "{project} on {dev}: {} frames ahead", -ahead);
            // As loud as it was: no gain from converting.
            let peak = |x: &[[f32; 2]]| x.iter().fold(0.0f32, |m, f| m.max(f[0].abs()));
            let engine = peak(&sine(project).render(0.5).unwrap());
            assert!((peak(&heard[dev as usize / 10..]) - engine).abs() < 0.02 * engine, "{project} on {dev}");
        }
    }

    #[test]
    fn a_block_of_the_replaced_renderer_does_not_undo_a_load() {
        let (tx, rx) = bounded(8);
        let (retired, _old) = bounded(8);
        let shared = Arc::new(Shared::default());
        let player = Player { tx, shared: shared.clone(), device_rate: 48_000, device: "test".into() };
        let mut cb = Callback { renderer: None, playing: false, scratch: vec![[0.0; 2]; 64], rx, retired, shared };
        let mut data = vec![0.0; 128];
        player.load(silence(0));
        player.play();
        cb.fill(&mut data, 2);
        assert_eq!(player.position(), 64);
        // The load lands while the audio thread renders a block of the old renderer.
        cb.receive();
        player.load(silence(4800));
        cb.render(&mut data, 2);
        assert_eq!(player.position(), 4800);
        cb.fill(&mut data, 2);
        assert_eq!(player.position(), 4864);
        // Stopped, the loaded position holds at once and stays.
        player.stop();
        cb.fill(&mut data, 2);
        player.load(silence(96_000));
        assert_eq!(player.position(), 96_000);
        cb.fill(&mut data, 2);
        assert_eq!(player.position(), 96_000);
    }
}
