//! The sound inputs there are, and the one a player reads (RENDER-30a; the io plan's D13 to D16,
//! D20 and D24).
//!
//! A player opens an input at the first Record on an audio track and keeps it open until the song
//! closes or Settings changes it ([`Player::open_input`]). It is the output's own device, or the one
//! an [`InputChoice`] names. Its callback ([`Feed::device`]) takes the first two channels (a mono
//! input stays mono), converts them to the engine's rate with the player's [`Resampler`], and
//! copies them into two [`Ring`]s made when it opened: one the engine hears on a lane (MON-8), one
//! a take writes from. It never allocates, locks or waits. Each buffer is stamped with the engine
//! clock heard when its first frame was captured: the device's capture time, by cpal's
//! timestamps (else one buffer before its callback), placed on the output's clock as
//! [`Player::heard_frame`] places a key.
//!
//! Under `M2_AUDIO=none` (and on a driven player) the input is silent unless samples are injected
//! ([`Player::inject_input`]): the output's callback hands them over as it plays, one engine frame
//! for each it renders, each buffer stamped as a device's would be. So a test knows where every
//! injected sample lands, to the sample. `M2_AUDIO_INPUTS` names the inputs listed then.
//!
//! What goes wrong with the input is reported into the app's fault ring ([`Player::report_to`],
//! RENDER-25), numbers only, about the input's opening ([`Listen::open`]): the input chosen isn't
//! there (`input.missing`, here); the session reports a take's input that went away
//! (`input.lost`) and one the OS refuses ([`Player::input_refused`], `input.refused`).

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use m2_runtime::fault::{Code, Fault, Faults, Subject};

use crate::outputs::{self, Audio};
use crate::ring::{Queue, Ring};
use crate::{DeviceError, DeviceErrors, DeviceEvent, Msg, Player, Resampler, Shared, now_ns};

/// What a player says when there is no input to open (D23).
pub const NO_INPUT: &str = "No sound input found. Plug in a microphone or an audio interface.";
/// What it says when macOS doesn't let it use the microphone (D23).
pub const NO_MICROPHONE: &str = "music2 can't use the microphone. Allow it in System Settings ▸ Privacy & Security ▸ Microphone.";

/// How long a take's ring holds, in seconds of the engine's rate (D22), and the engine's.
const TAKE_RING: f64 = 4.0;
const ENGINE_RING: f64 = 0.5;
/// Frames a device buffer may have: longer ones are taken in pieces this long.
const BUFFER: usize = 8192;
/// Samples an injected input queues ahead of the callback.
const QUEUED: usize = 1 << 20;

/// A sound input: its id (what an [`InputChoice`] names), its name, whether it is the system's
/// default now, its rate and its channels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Input {
    pub id: String,
    pub name: String,
    pub default: bool,
    pub rate: u32,
    pub channels: u16,
}

/// The input a player opens: None is the output's own device (the default in Settings, D13). Its
/// name, as it was listed when chosen, says which it was when it isn't there.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InputChoice {
    pub device: Option<String>,
    pub name: Option<String>,
}

impl InputChoice {
    /// How it is said: its name as listed, else its id.
    fn said(&self) -> Option<String> {
        self.name.clone().or_else(|| self.device.clone())
    }
}

/// Each input opened in the app, counted: a fault names its input by this ([`Listen::open`]),
/// whichever player opened it.
static OPENED: AtomicU32 = AtomicU32::new(0);

/// The inputs there are, the default first: the OS's input devices, or under `M2_AUDIO=none` those
/// `M2_AUDIO_INPUTS` names (a comma list; one called "none" when it is unset), each mono at 48 kHz.
pub fn inputs() -> Vec<Input> {
    match Audio::from_env() {
        Audio::Silent(_) => silent_names()
            .into_iter()
            .enumerate()
            .map(|(i, name)| Input { id: name.clone(), name, default: i == 0, rate: outputs::SILENT_RATE, channels: 1 })
            .collect(),
        Audio::Fail => vec![],
        Audio::Device => device_inputs(),
    }
}

fn silent_names() -> Vec<String> {
    let listed = std::env::var("M2_AUDIO_INPUTS").unwrap_or_default();
    let names: Vec<String> = listed.split(',').map(str::trim).filter(|n| !n.is_empty()).map(String::from).collect();
    if names.is_empty() { vec!["none".into()] } else { names }
}

/// The OS's input devices, each name once, the default first.
fn device_inputs() -> Vec<Input> {
    let host = cpal::default_host();
    let default = host.default_input_device().and_then(|d| outputs::id_of(&d));
    let devices: Vec<(String, String, cpal::Device)> =
        host.input_devices().into_iter().flatten().filter_map(|d| Some((outputs::id_of(&d)?, outputs::name_of(&d)?, d))).collect();
    let seen: Vec<(String, String)> = devices.iter().map(|(id, name, _)| (id.clone(), name.clone())).collect();
    let mut channels = vec![0u16; devices.len()];
    let listed = outputs::listed(&seen, default.as_deref(), |i| {
        let config = devices[i].2.default_input_config().ok()?;
        channels[i] = config.channels();
        Some((config.sample_rate(), None))
    });
    listed
        .into_iter()
        .map(|o| {
            let ch = seen.iter().position(|(id, _)| *id == o.id).map_or(2, |i| channels[i]);
            Input { id: o.id, name: o.name, default: o.default, rate: o.rate, channels: ch }
        })
        .collect()
}

/// What a take reads from the input a player has open: its ring, its channels (1 or 2), the rate
/// its frames are at (the engine's when it opened) and its name. `open` numbers this opening in the
/// app (a fault about it says so); `missing` is the input chosen when it isn't there, so this is
/// another (io r7).
#[derive(Clone)]
pub struct Listen {
    pub ring: Arc<Ring>,
    pub channels: u16,
    pub rate: u32,
    pub name: String,
    pub open: u32,
    pub missing: Option<String>,
}

/// The input callback's work: frames into both rings, stamped. Made whole when the input opens.
pub(crate) struct Feed {
    engine: Arc<Ring>,
    take: Arc<Ring>,
    /// The device's rate to the engine's, when they differ.
    convert: Option<Resampler>,
    /// One piece of a buffer as frames, and the same at the engine's rate.
    frames: Box<[[f32; 2]]>,
    made: Box<[[f32; 2]]>,
    shared: Arc<Shared>,
    device_rate: u32,
    rate: u32,
}

impl Feed {
    fn new(engine: Arc<Ring>, take: Arc<Ring>, shared: Arc<Shared>, device_rate: u32, rate: u32) -> Feed {
        let made = BUFFER * rate.max(device_rate) as usize / device_rate.max(1) as usize + 64;
        Feed {
            engine,
            take,
            convert: (device_rate != rate).then(|| Resampler::new(device_rate, rate)),
            frames: vec![[0.0; 2]; BUFFER].into_boxed_slice(),
            made: vec![[0.0; 2]; made].into_boxed_slice(),
            shared,
            device_rate,
            rate,
        }
    }

    /// The device's callback: `data`, `channels` interleaved, its first frame captured `latency`
    /// before now. Never allocates, locks or waits (RENDER-30a).
    pub(crate) fn device(&mut self, data: &[f32], channels: usize, latency: Duration) {
        #[cfg(debug_assertions)]
        crate::alloc::set_in_callback(true);
        let channels = channels.max(1);
        let captured = now_ns().saturating_sub(latency.as_nanos() as u64);
        // The engine clock heard then; each piece after it by its length at the engine's rate.
        let mut clock = self.shared.heard_clock_at(captured, self.rate, self.device_rate);
        for piece in data.chunks(BUFFER * channels) {
            let n = piece.len() / channels;
            for (f, x) in self.frames[..n].iter_mut().zip(piece.chunks(channels)) {
                *f = [x[0], if channels > 1 { x[1] } else { x[0] }];
            }
            let made = match &mut self.convert {
                Some(c) => c.push(&self.frames[..n], &mut self.made),
                None => {
                    self.made[..n].copy_from_slice(&self.frames[..n]);
                    n
                }
            };
            self.engine.write(&self.made[..made], clock);
            self.take.write(&self.made[..made], clock);
            clock = clock.map(|c| c + made as u64);
        }
        #[cfg(debug_assertions)]
        crate::alloc::set_in_callback(false);
    }
}

/// The silent input: injected samples, handed over by the output's callback, one engine frame for
/// each it renders (D24).
pub struct Silent {
    engine: Arc<Ring>,
    take: Arc<Ring>,
    queue: Arc<Queue>,
    samples: Box<[f32]>,
    frames: Box<[[f32; 2]]>,
}

impl Silent {
    /// `n` frames, the first heard at engine clock `clock`. Never allocates, locks or waits.
    pub(crate) fn deliver(&mut self, n: usize, clock: u64) {
        let mut done = 0;
        while done < n {
            let k = (n - done).min(self.samples.len());
            self.queue.take(&mut self.samples[..k]);
            for (f, s) in self.frames[..k].iter_mut().zip(&self.samples[..k]) {
                *f = [*s, *s];
            }
            let at = Some(clock + done as u64);
            self.engine.write(&self.frames[..k], at);
            self.take.write(&self.frames[..k], at);
            done += k;
        }
    }
}

/// What an input's device reported, counted by kind: the input's own, never the output's, so a
/// microphone unplugged never stops playback, and its overruns are no dropouts of the output (the
/// review's io-r2).
#[derive(Default)]
pub(crate) struct Errors([AtomicU64; 4]);

impl Errors {
    /// The input's error callback: count `e` by kind, and nothing else. It never allocates, locks
    /// or prints (RENDER-28), as the output's doesn't: the input's thread logs the counts.
    fn add(&self, e: &DeviceError) {
        self.0[DeviceEvent::of(e.kind()) as usize].fetch_add(1, Ordering::Relaxed);
    }

    fn read(&self) -> DeviceErrors {
        let n = |e: DeviceEvent| self.0[e as usize].load(Ordering::Relaxed);
        DeviceErrors { xruns: n(DeviceEvent::Xrun), changes: n(DeviceEvent::Changed), gone: n(DeviceEvent::Gone), other: n(DeviceEvent::Other) }
    }
}

/// The input a player has open, on its control side.
pub(crate) struct Opened {
    choice: InputChoice,
    listen: Listen,
    /// What its device reported.
    errors: Arc<Errors>,
    /// The engine's ring, for hearing.
    engine: Arc<Ring>,
    /// An injected input's samples.
    queue: Option<Arc<Queue>>,
    /// Held while a device's stream plays: let go, its thread stops it.
    _stream: Option<Arc<()>>,
}

/// How a player meets its input: the silent one (injected), or a device's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Silent,
    Device,
}

/// The player's side of its input.
#[derive(Default)]
pub(crate) struct Inputs {
    open: Mutex<Option<Opened>>,
    /// The app's fault ring ([`Player::report_to`]).
    faults: OnceLock<Arc<Faults>>,
    /// The inject seam's Don't Allow ([`Player::refuse_input`]).
    refused: AtomicBool,
    /// The silent inputs, as `M2_AUDIO_INPUTS` names them, set for this player ([`Player::name_inputs`]).
    names: Mutex<Option<Vec<String>>>,
}

fn rings(rate: u32) -> (Arc<Ring>, Arc<Ring>) {
    let len = |s: f64| (s * rate as f64).ceil() as usize;
    (Arc::new(Ring::new(len(ENGINE_RING))), Arc::new(Ring::new(len(TAKE_RING))))
}

impl Player {
    /// Report what goes wrong with its input into `faults`, the app's fault ring (RENDER-25). Once:
    /// a player belongs to one app.
    pub fn report_to(&self, faults: Arc<Faults>) {
        let _ = self.inputs.faults.set(faults);
    }

    /// Whether the OS refuses music2 the microphone now: a Mac whose Privacy & Security says no
    /// (`m2_os::host::microphone_refused`), or the inject seam's Don't Allow. Its input gives
    /// silence then, not an error. The session asks at each record tick (the ex plan's D11).
    pub fn input_refused(&self) -> bool {
        self.inputs.refused.load(Ordering::Relaxed) || (self.input_kind == Kind::Device && m2_os::host::microphone_refused())
    }

    /// The OS refuses the microphone from now, as a Mac's Don't Allow does, or allows it again
    /// (tests, and the inject API's `refuse`).
    #[doc(hidden)]
    pub fn refuse_input(&self, on: bool) {
        self.inputs.refused.store(on, Ordering::Relaxed);
    }

    /// The silent inputs this player lists, as `M2_AUDIO_INPUTS` names them (tests: the
    /// environment is the whole process's).
    #[doc(hidden)]
    pub fn name_inputs(&self, names: &[&str]) {
        *self.inputs.names.lock().unwrap_or_else(|e| e.into_inner()) = Some(names.iter().map(|n| n.to_string()).collect());
    }

    /// Open the input `choice` names, or keep the one open when it is that one at this rate:
    /// what a take reads. Under `M2_AUDIO=none` (or driven), the silent input, named after the
    /// choice when `M2_AUDIO_INPUTS` lists it. Says [`NO_INPUT`] when there is none, and
    /// [`NO_MICROPHONE`] when macOS doesn't let music2 use it. Plain words only: what the device
    /// said is logged. When the input chosen isn't there, it opens another, and reports
    /// `input.missing` about it, each time ([`Listen::missing`], the ex plan's D8).
    pub fn open_input(&self, choice: &InputChoice) -> Result<Listen> {
        // A Mac whose Privacy & Security says no gives silence, not an error: said in words (D23).
        if self.input_refused() {
            return Err(anyhow!(NO_MICROPHONE));
        }
        let rate = self.rate();
        let mut open = self.inputs.open.lock().unwrap_or_else(|e| e.into_inner());
        // The same input, unless its device went away, or the one chosen wasn't there: then it
        // opens again, and finds the one chosen once it is plugged in.
        if let Some(o) =
            open.as_ref().filter(|o| o.choice == *choice && o.listen.rate == rate && o.errors.read().gone == 0 && o.listen.missing.is_none())
        {
            return Ok(o.listen.clone());
        }
        self.close_locked(&mut open);
        let (engine, take) = rings(rate);
        let errors = Arc::new(Errors::default());
        let at = OPENED.fetch_add(1, Ordering::Relaxed) + 1;
        let opened = match self.input_kind {
            Kind::Silent => {
                let names = match Audio::from_env() {
                    Audio::Fail => return Err(anyhow!(NO_INPUT)),
                    _ => self.inputs.names.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_else(silent_names),
                };
                let found = choice.device.clone().filter(|d| names.contains(d));
                let missing = if found.is_none() { choice.said() } else { None };
                let name = found.unwrap_or_else(|| names[0].clone());
                let queue = Arc::new(Queue::new(QUEUED));
                let silent = Silent {
                    engine: engine.clone(),
                    take: take.clone(),
                    queue: queue.clone(),
                    samples: vec![0.0; BUFFER].into_boxed_slice(),
                    frames: vec![[0.0; 2]; BUFFER].into_boxed_slice(),
                };
                self.send(Msg::Input(Some(Box::new(silent))))?;
                let listen = Listen { ring: take, channels: 1, rate, name, open: at, missing };
                Opened { choice: choice.clone(), listen, errors, engine, queue: Some(queue), _stream: None }
            }
            Kind::Device => {
                let (name, channels, stream, found) = self.open_device(choice, engine.clone(), take.clone(), rate, errors.clone())?;
                let missing = if found { None } else { choice.said() };
                let listen = Listen { ring: take, channels, rate, name, open: at, missing };
                Opened { choice: choice.clone(), listen, errors, engine, queue: None, _stream: Some(stream) }
            }
        };
        let listen = opened.listen.clone();
        *open = Some(opened);
        if listen.missing.is_some()
            && let Some(faults) = self.inputs.faults.get()
        {
            faults.report(Fault { code: Code::InputMissing, plan: 0, subject: Subject::Input(listen.open) });
        }
        Ok(listen)
    }

    /// Close the input (the song closed, or Settings chose another): its stream stops.
    pub fn close_input(&self) {
        let mut open = self.inputs.open.lock().unwrap_or_else(|e| e.into_inner());
        self.close_locked(&mut open);
    }

    fn close_locked(&self, open: &mut Option<Opened>) {
        if let Some(o) = open.take()
            && o.queue.is_some()
        {
            let _ = self.send(Msg::Input(None));
        }
    }

    /// The input open, if one is: what a take reads.
    pub fn input(&self) -> Option<Listen> {
        self.inputs.open.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|o| o.listen.clone())
    }

    /// Whether the open input's device went away (unplugged, or its stream invalidated): a take
    /// from it ends, and the next [`Self::open_input`] opens it again. The output plays on: it is
    /// never [`Self::stalled`] for it (the review's io-r2).
    pub fn input_gone(&self) -> bool {
        self.inputs.open.lock().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|o| o.errors.read().gone > 0)
    }

    /// An error the open input's device reported, counted as its error callback counts it (tests,
    /// and the inject API's unplug).
    #[doc(hidden)]
    pub fn input_error(&self, e: DeviceError) {
        if let Some(o) = self.inputs.open.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            o.errors.add(&e);
        }
    }

    /// Queue mono `samples` at the engine's rate for the silent input to deliver, in order, one for
    /// each engine frame the player renders from the next callback on (tests, and a browser suite's
    /// input). Waits while the queue is full, at most a second.
    pub fn inject_input(&self, samples: &[f32]) -> Result<()> {
        let queue = self.inputs.open.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|o| o.queue.clone());
        let queue = queue.ok_or_else(|| anyhow!("no injected input is open"))?;
        match queue.put(samples, Duration::from_secs(1)) {
            true => Ok(()),
            false => Err(anyhow!("the injected input is full")),
        }
    }

    /// Hear the input on audio lane `slot` of the renderer `generation` (MON-8), through the track's
    /// effects and fader, or with `on` false stop hearing it on any lane. In order behind the
    /// activations sent before it, as a live note is.
    pub fn hear(&self, generation: u64, slot: u32, on: bool) -> Result<u64> {
        let input = match on {
            true => {
                let open = self.inputs.open.lock().unwrap_or_else(|e| e.into_inner());
                let ring = open.as_ref().map(|o| o.engine.clone()).ok_or_else(|| anyhow!("no sound input is open"))?;
                Some(ring as Arc<dyn m2_runtime::LiveInput>)
            }
            false => None,
        };
        self.envelope(m2_runtime::Msg::Hear { generation, slot, input })
    }

    /// Open a device's input stream on a thread of its own (cpal streams can't move between threads
    /// on every OS), its callback a [`Feed`]: its name, its channels (1 or 2), what keeps it, and
    /// whether it is the one chosen (always, when none is).
    fn open_device(&self, choice: &InputChoice, engine: Arc<Ring>, take: Arc<Ring>, rate: u32, errors: Arc<Errors>) -> Result<(String, u16, Arc<()>, bool)> {
        let host = cpal::default_host();
        let by_id = |id: &str| host.device_by_id(&id.parse().ok()?);
        // The one chosen; else the output's own device, when it records; else the default input.
        let chosen = choice.device.as_deref().and_then(by_id);
        let found = choice.device.is_none() || chosen.is_some();
        let device = chosen
            .or_else(|| self.output_id.as_deref().and_then(by_id).filter(|d| d.default_input_config().is_ok()))
            .or_else(|| host.default_input_device())
            .ok_or_else(|| anyhow!(NO_INPUT))?;
        let config = device.default_input_config().map_err(|e| no_input(&e))?;
        let name = outputs::name_of(&device).unwrap_or_else(|| "input".into());
        let channels = config.channels() as usize;
        let mut feed = Feed::new(engine, take, self.shared.clone(), config.sample_rate(), rate);
        let alive = Arc::new(());
        let weak: Weak<()> = Arc::downgrade(&alive);
        let player = Arc::downgrade(&self.alive);
        let (ready_tx, ready) = std::sync::mpsc::sync_channel::<Result<()>>(1);
        let counted = errors.clone();
        std::thread::Builder::new().name("m2-input".into()).spawn(move || {
            let stream = device.build_input_stream::<f32, _, _>(
                config.config(),
                move |data: &[f32], info: &cpal::InputCallbackInfo| {
                    let t = info.timestamp();
                    let latency = t.callback.duration_since(t.capture);
                    feed.device(data, channels, latency)
                },
                move |e| counted.add(&e),
                None,
            );
            let stream = match stream.map_err(anyhow::Error::from).and_then(|s| s.play().map(|()| s).map_err(anyhow::Error::from)) {
                Ok(s) => s,
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            let _ = ready_tx.send(Ok(()));
            // What the input's device reported, logged here once a second, never by its callback.
            // Its going away isn't: a take from it ends, and the session tells it (`input.lost`).
            let (mut seen, mut said) = (DeviceErrors::default(), Instant::now());
            while weak.strong_count() > 0 && player.strong_count() > 0 {
                std::thread::sleep(Duration::from_millis(50));
                if said.elapsed() >= Duration::from_secs(1) {
                    let now = DeviceErrors { gone: 0, ..errors.read() };
                    if let Some(words) = now.since(seen) {
                        eprintln!("music2: the sound input: {words}");
                    }
                    (seen, said) = (now, Instant::now());
                }
            }
            drop(stream);
        })?;
        ready.recv().map_err(|_| anyhow!(NO_INPUT))?.map_err(|e| no_input(&e))?;
        Ok((name, channels.min(2) as u16, alive, found))
    }
}

/// [`NO_INPUT`], the device's own words logged.
fn no_input(e: &dyn std::fmt::Display) -> anyhow::Error {
    eprintln!("music2: the sound input didn't open: {e:#}");
    anyhow!(NO_INPUT)
}

/// An input's callback driven by a test, as a device would call it: [`InputDriver::fill`].
pub struct InputDriver {
    feed: Feed,
    channels: usize,
}

impl InputDriver {
    /// One device buffer, `channels` interleaved, its first frame captured `latency` ago.
    pub fn fill(&mut self, data: &[f32], latency: Duration) {
        self.feed.device(data, self.channels, latency);
    }
}

impl Player {
    /// A device input's callback for `device_rate` and `channels`, into fresh rings, for a test to
    /// call (`the_input_callback_never_allocates`): its rings, the engine's and the take's.
    #[doc(hidden)]
    pub fn driven_input(&self, device_rate: u32, channels: usize) -> (InputDriver, Arc<Ring>, Arc<Ring>) {
        let (engine, take) = rings(self.rate());
        let feed = Feed::new(engine.clone(), take.clone(), self.shared.clone(), device_rate, self.rate());
        (InputDriver { feed, channels }, engine, take)
    }

    /// The silent input's frames delivered so far, for tests: its take ring's head.
    #[doc(hidden)]
    pub fn input_frames(&self) -> u64 {
        self.input().map_or(0, |l| l.ring.head())
    }
}
