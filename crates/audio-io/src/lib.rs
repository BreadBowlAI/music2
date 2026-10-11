//! L5 — realtime output. The audio callback owns a [`Renderer`] and the transport;
//! everything else talks to it through a lock-free message channel and atomics.
//! Audio is the master clock: the playhead is the callback's sample counter.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};

use anyhow::{Result, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{Receiver, Sender, bounded};
use m2_dsp::Frame;
use m2_project::{Id, Tick};
use m2_render::Renderer;

pub enum Msg {
    /// A freshly compiled renderer for a new song snapshot.
    Renderer(Box<Renderer>),
    Play,
    Stop,
    Seek(Tick),
    /// Loop region in ticks.
    Loop(Option<(Tick, Tick)>),
    LiveNote {
        track: Id,
        pitch: u8,
        velocity: u8,
        on: bool,
    },
    /// Silence everything immediately.
    Panic,
}

/// State the UI reads every frame.
pub struct Shared {
    pub playhead: AtomicI64,
    pub playing: AtomicBool,
    master_peak: AtomicU32,
    /// Peaks by track index (up to 64 tracks).
    track_peaks: [AtomicU32; 64],
    track_ids: [AtomicI64; 64],
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            playhead: AtomicI64::new(0),
            playing: AtomicBool::new(false),
            master_peak: AtomicU32::new(0),
            track_peaks: std::array::from_fn(|_| AtomicU32::new(0)),
            track_ids: std::array::from_fn(|_| AtomicI64::new(-1)),
        }
    }
}

impl Shared {
    pub fn master_peak(&self) -> f32 {
        f32::from_bits(self.master_peak.load(Ordering::Relaxed))
    }

    pub fn track_peak(&self, id: Id) -> f32 {
        for i in 0..64 {
            if self.track_ids[i].load(Ordering::Relaxed) == id as i64 {
                return f32::from_bits(self.track_peaks[i].load(Ordering::Relaxed));
            }
        }
        0.0
    }
}

struct Engine {
    renderer: Option<Box<Renderer>>,
    pos: i64,
    playing: bool,
    looping: Option<(Tick, Tick)>,
    scratch: Vec<Frame>,
    rx: Receiver<Msg>,
    trash: Sender<Box<Renderer>>,
    shared: Arc<Shared>,
}

impl Engine {
    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Renderer(mut new) => {
                if let Some(mut old) = self.renderer.take() {
                    new.adopt_from(&mut old);
                    // Keep the musical position when the tempo changes.
                    let (ot, nt) = (old.tempo(), new.tempo());
                    if ot != nt {
                        let tick = ot.sample_to_tick_f(self.pos, old.sample_rate() as f64);
                        self.pos = (tick * nt.seconds_per_tick() * new.sample_rate() as f64).round() as i64;
                    }
                    // Freeing happens on the UI thread, never in the callback.
                    if let Err(e) = self.trash.try_send(old) {
                        std::mem::forget(e.into_inner());
                    }
                }
                self.renderer = Some(new);
            }
            Msg::Play => self.playing = true,
            Msg::Stop => {
                self.playing = false;
                if let Some(r) = &mut self.renderer {
                    r.all_notes_off(false);
                }
            }
            Msg::Seek(tick) => {
                if let Some(r) = &mut self.renderer {
                    self.pos = r.tempo().tick_to_sample(tick.max(0), r.sample_rate() as f64);
                    r.all_notes_off(true);
                }
            }
            Msg::Loop(l) => self.looping = l.filter(|(a, b)| b > a),
            Msg::LiveNote { track, pitch, velocity, on } => {
                if let Some(r) = &mut self.renderer {
                    r.live_note(track, pitch, velocity, on);
                }
            }
            Msg::Panic => {
                if let Some(r) = &mut self.renderer {
                    r.all_notes_off(true);
                }
            }
        }
    }

    fn fill(&mut self, data: &mut [f32], channels: usize) {
        while let Ok(m) = self.rx.try_recv() {
            self.handle(m);
        }
        let frames = data.len() / channels.max(1);
        let Some(r) = &mut self.renderer else {
            data.fill(0.0);
            return;
        };
        let sr = r.sample_rate() as f64;
        let tempo = r.tempo();
        let mut done = 0;
        while done < frames {
            let mut n = (frames - done).min(self.scratch.len());
            let mut wrap = None;
            if self.playing
                && let Some((a, b)) = self.looping
            {
                let end = tempo.tick_to_sample(b, sr);
                if self.pos >= end {
                    // Playhead is past the loop (e.g. loop set behind it): jump now.
                    self.pos = tempo.tick_to_sample(a, sr);
                    r.all_notes_off(false);
                } else if self.pos + n as i64 > end {
                    n = (end - self.pos) as usize;
                    wrap = Some(tempo.tick_to_sample(a, sr));
                }
            }
            let buf = &mut self.scratch[..n];
            r.render(self.pos, self.playing, buf);
            for (i, f) in buf.iter().enumerate() {
                let o = (done + i) * channels;
                data[o] = f[0];
                if channels > 1 {
                    data[o + 1] = f[1];
                    for c in 2..channels {
                        data[o + c] = 0.0;
                    }
                }
            }
            done += n;
            if self.playing {
                self.pos += n as i64;
            }
            if let Some(start) = wrap {
                self.pos = start;
                r.all_notes_off(false);
            }
        }
        self.shared.playhead.store(tempo.sample_to_tick(self.pos, sr), Ordering::Relaxed);
        self.shared.playing.store(self.playing, Ordering::Relaxed);
        self.shared.master_peak.store(r.master_peak.to_bits(), Ordering::Relaxed);
        for (i, (id, p)) in r.track_peaks().into_iter().take(64).enumerate() {
            self.shared.track_ids[i].store(id as i64, Ordering::Relaxed);
            self.shared.track_peaks[i].store(p.to_bits(), Ordering::Relaxed);
        }
    }
}

pub struct AudioOut {
    _stream: cpal::Stream,
    tx: Sender<Msg>,
    trash: Receiver<Box<Renderer>>,
    pub shared: Arc<Shared>,
    pub sample_rate: u32,
    pub device_name: String,
}

impl AudioOut {
    pub fn start() -> Result<AudioOut> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or_else(|| anyhow!("no audio output device"))?;
        let device_name = device.description().map(|d| d.name().to_string()).unwrap_or_else(|_| "output".into());
        let supported = device.default_output_config()?;
        let mut config = supported.config();
        config.buffer_size = cpal::BufferSize::Default;
        let channels = config.channels as usize;
        let sample_rate = config.sample_rate;
        let (tx, rx) = bounded::<Msg>(256);
        let (trash_tx, trash) = bounded::<Box<Renderer>>(16);
        let shared = Arc::new(Shared::default());
        let mut engine = Engine {
            renderer: None,
            pos: 0,
            playing: false,
            looping: None,
            scratch: vec![[0.0; 2]; 4096],
            rx,
            trash: trash_tx,
            shared: shared.clone(),
        };
        let stream = device.build_output_stream::<f32, _, _>(
            config,
            move |data: &mut [f32], _| engine.fill(data, channels),
            |e| eprintln!("audio stream error: {e}"),
            None,
        )?;
        stream.play()?;
        Ok(AudioOut { _stream: stream, tx, trash, shared, sample_rate, device_name })
    }

    pub fn send(&self, msg: Msg) {
        let _ = self.tx.try_send(msg);
    }

    /// Drop renderers the audio thread has retired. Call regularly from the UI thread.
    pub fn collect_garbage(&self) {
        while self.trash.try_recv().is_ok() {}
    }

    pub fn playhead(&self) -> Tick {
        self.shared.playhead.load(Ordering::Relaxed)
    }

    pub fn is_playing(&self) -> bool {
        self.shared.playing.load(Ordering::Relaxed)
    }
}

/// A `Send + Clone` handle to an output stream that lives on its own thread
/// (cpal streams can't move between threads on every platform).
#[derive(Clone)]
pub struct AudioHandle {
    tx: Sender<Msg>,
    pub shared: Arc<Shared>,
    pub sample_rate: u32,
    pub device_name: String,
}

impl AudioHandle {
    /// Open the default output on a dedicated thread, which also frees retired renderers.
    pub fn spawn() -> Result<AudioHandle> {
        let (tx, rx) = bounded::<Result<AudioHandle>>(1);
        std::thread::Builder::new().name("music2-audio".into()).spawn(move || match AudioOut::start() {
            Ok(out) => {
                let handle = AudioHandle {
                    tx: out.tx.clone(),
                    shared: out.shared.clone(),
                    sample_rate: out.sample_rate,
                    device_name: out.device_name.clone(),
                };
                let _ = tx.send(Ok(handle));
                loop {
                    out.collect_garbage();
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
            Err(e) => {
                let _ = tx.send(Err(e));
            }
        })?;
        rx.recv()?
    }

    pub fn send(&self, msg: Msg) {
        let _ = self.tx.try_send(msg);
    }

    pub fn playhead(&self) -> Tick {
        self.shared.playhead.load(Ordering::Relaxed)
    }

    pub fn is_playing(&self) -> bool {
        self.shared.playing.load(Ordering::Relaxed)
    }
}
