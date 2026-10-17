//! The audio device: the one place the core touches hardware.
//!
//! The audio callback owns a [`Live`]: the persistent [`Engine`] (RENDER-6) and the converter
//! to the device's rate. Everything else talks to it through a bounded channel and reads where
//! it is from atomics. Whatever the callback replaces leaves over a second channel and is
//! dropped on another thread.
//!
//! The engine runs at the project's `sample_rate`, whatever the device's rate is (RENDER-31):
//! the device is opened at its own rate, and when the two differ the engine's output is
//! converted at the device boundary ([`Resampler`]). Positions are always engine frames.
//!
//! RENDER-28: the callback never allocates, locks or waits. No thread ever blocks on these
//! channels (crossbeam's channels lock only to wake a blocked thread): senders `try_send`
//! and retry for a bounded time, and retired objects are polled for. A panic in the engine is
//! caught at the callback, so it never unwinds into the device's frame. Cell values never use
//! the queue: they are written into the slots of the renderer they are for ([`Player::set_cells`]).
//!
//! Live mode: a NaN from a broken kernel is silenced, not an error.
//!
//! The device's errors are sorted by kind ([`DeviceEvent`]): an xrun is counted and play goes on,
//! a change of output device asks for a new player, and only a device that is gone stalls it.

pub mod alloc;
mod cells;
mod engines;
pub mod outputs;
mod resample;
mod timing;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering, fence};
use std::sync::{Arc, OnceLock, Weak};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use cpal::traits::{DeviceTrait, StreamTrait};
pub use cpal::{Error as DeviceError, ErrorKind as DeviceErrorKind};
use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
pub use engines::{BETWEEN, Between, EngineThreads, Handover, LiveThreads, live_count};
use m2_runtime::engine::{APPLIED, Msg as EngineMsg};
pub use m2_runtime::engine::{Applied, CallbackStamp, Engine, Envelope, Heard, Retired};
use m2_runtime::{CellTarget, Cmd, LaneSwap, LiveEv, Renderer, Runner, Transport, fpenv};
pub use outputs::{Choice, Freed, Output, default_output, output_ready, outputs};
pub use resample::Resampler;
pub use timing::{BUCKET_NS, Timing};

use crate::cells::{CELL_SEQ, Writer};

/// How long a sender waits for room before it reports the player stalled.
const STALL: Duration = Duration::from_millis(500);
/// A device that hasn't called back for this long has stopped.
const SILENT_DEVICE: Duration = Duration::from_secs(1);
/// Messages the queue holds, and retired objects.
const QUEUE: usize = 1024;
const RETIRED: usize = 64;
/// Frames the callback renders at a time: device buffers are cut into pieces this big.
const SCRATCH: usize = 8192;
/// Objects the callback keeps while the retired channel is full.
const GONE: usize = 8;

/// The process's clock for callback stamps, in nanoseconds: one for every player, read without a
/// lock, so a caller can stamp a request before it waits for anything.
pub fn now_ns() -> u64 {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    EPOCH.get_or_init(Instant::now).elapsed().as_nanos() as u64
}

/// What the audio thread plays: the engine at the project's rate, and the converter to the
/// device's rate when they differ. Both are made off the audio thread.
pub struct Live {
    engine: Engine,
    resampler: Option<Resampler>,
}

impl Live {
    fn new(renderer: Renderer, device_rate: u32) -> Live {
        let rate = renderer.sample_rate();
        Live { engine: Engine::with(Box::new(renderer)), resampler: (rate != device_rate).then(|| Resampler::new(rate, device_rate)) }
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Fill `out` at the device's rate, each block rendered on `runner`'s threads.
    fn fill(
        &mut self,
        out: &mut [[f32; 2]],
        inbox: &mut dyn FnMut() -> Option<Envelope>,
        retire: &mut dyn FnMut(Retired) -> Result<(), Retired>,
        stamp: CallbackStamp,
        runner: &mut Runner,
    ) {
        let Live { engine, resampler } = self;
        match resampler {
            Some(rs) => {
                let _ = rs.fill(out, &mut |buf| {
                    engine.process_on(buf, inbox, retire, stamp, runner);
                    Ok(())
                });
            }
            None => engine.process_on(out, inbox, retire, stamp, runner),
        }
    }
}

/// What a new [`Live`] takes from the one it replaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keep {
    /// Nothing: a project just opened plays from its renderer's transport.
    Nothing,
    /// The beat, the play state and the loop (a rate change).
    Position,
}

enum Msg {
    Engine(Envelope),
    Replace { live: Box<Live>, keep: Keep },
    Unload,
    /// The player's engine threads (8a's live half), handed over once they are made.
    Threads(Box<Handover>),
}

/// What leaves the audio thread, to be dropped elsewhere.
pub enum Gone {
    Engine(Retired),
    Live(Box<Live>),
    /// The handover, holding the runner the callback had before.
    Runner(Box<Handover>),
}

/// An error a sender returns when the audio thread hasn't taken a message for [`STALL`]: the
/// device stopped calling back. The message was not sent.
#[derive(Debug)]
pub struct Stalled;

impl std::fmt::Display for Stalled {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("the audio device stopped responding")
    }
}

impl std::error::Error for Stalled {}

/// What an error the device reports means for the player. cpal gives every OS's errors one set of
/// kinds, its xruns included (ALSA's `EPIPE` and suspends, CoreAudio's overloads, PipeWire's and
/// JACK's), so this is the same on every OS (platform survey-later §2.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceEvent {
    /// A buffer underrun or overrun: a dropout. Counted, and play goes on.
    Xrun,
    /// The output moved to another device, as cpal follows the default one. It plays on there,
    /// but the new device has its own rate and period, so a new player should take over.
    Changed,
    /// The stream was invalidated or the device is unavailable (unplugged, or no default output
    /// is left): the player is stalled.
    Gone,
    /// Anything else: counted, and play goes on. A device that stops calling back after one is
    /// stalled a second later, as any quiet device is.
    Other,
}

impl DeviceEvent {
    /// What an error of `kind` means for the player.
    pub fn of(kind: DeviceErrorKind) -> DeviceEvent {
        match kind {
            DeviceErrorKind::Xrun => DeviceEvent::Xrun,
            DeviceErrorKind::DeviceChanged => DeviceEvent::Changed,
            DeviceErrorKind::StreamInvalidated | DeviceErrorKind::DeviceNotAvailable => DeviceEvent::Gone,
            _ => DeviceEvent::Other,
        }
    }
}

/// The errors a player's device reported since it started, counted by kind ([`DeviceEvent`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeviceErrors {
    pub xruns: u64,
    pub changes: u64,
    pub gone: u64,
    pub other: u64,
}

impl DeviceErrors {
    /// What the device reported since `before`, in words for the log, if anything.
    pub fn since(self, before: DeviceErrors) -> Option<String> {
        let n = |now: u64, then: u64| now.saturating_sub(then);
        let s = |n: u64| if n == 1 { "" } else { "s" };
        let mut said = vec![];
        match n(self.xruns, before.xruns) {
            0 => {}
            x => said.push(format!("{x} dropout{}", s(x))),
        }
        if n(self.changes, before.changes) > 0 {
            said.push("the output moved to another device".into());
        }
        if n(self.gone, before.gone) > 0 {
            said.push("the device is gone".into());
        }
        match n(self.other, before.other) {
            0 => {}
            x => said.push(format!("{x} other error{}", s(x))),
        }
        (!said.is_empty()).then(|| format!("the audio device reported {}", said.join(", ")))
    }
}

/// One [`Applied`] stamp, as atomics.
#[derive(Default)]
struct AppliedSlot {
    seq: AtomicU64,
    clock: AtomicU64,
    callback_ns: AtomicU64,
    frame: AtomicU64,
    refused: AtomicBool,
}

/// One [`Heard`] stamp, as atomics, and its place among them all.
#[derive(Default)]
struct HeardSlot {
    n: AtomicU64,
    to: AtomicU64,
    clock: AtomicU64,
    callback_ns: AtomicU64,
    frame: AtomicU64,
}

/// What the audio thread publishes. The position block is a seqlock: the audio thread is its
/// only writer, and readers retry until they see one whole write.
#[derive(Default)]
struct Shared {
    seq: AtomicU64,
    pos: AtomicU64,
    beats: AtomicU64,
    seconds: AtomicU64,
    playing: AtomicBool,
    generation: AtomicU64,
    applied: [AppliedSlot; APPLIED],
    applied_n: AtomicU64,
    heard: [HeardSlot; APPLIED],
    heard_n: AtomicU64,
    // Written outside the seqlock.
    clock: AtomicU64,
    last_callback_ns: AtomicU64,
    /// The last callback's device buffer, in frames: 0 before the first (Settings' Now row).
    frames: AtomicU32,
    /// The output chosen is gone, so this plays on the default ([`Player::start_on`]).
    fell_back: AtomicBool,
    /// The engine rate of the project last sent: what positions count in.
    rate: AtomicU32,
    crashed: AtomicBool,
    /// Activations the engines refused, in all: the sender recovers with a fresh engine.
    refused: AtomicU64,
    loop_refused: AtomicBool,
    dropped_ons: AtomicU64,
    /// Live notes held in the renderer playing (MON-7).
    live: AtomicU32,
    /// The engine renders: a project plays, and it doesn't idle (RENDER-6a).
    running: AtomicBool,
    /// The metronome is on: each callback tells the engine (gaps #19).
    click: AtomicBool,
    /// The longest callback since [`Player::load`] last read it, as a share of its period (the
    /// bits of a non-negative `f32`), and the callbacks longer than their period so far (8b.8).
    longest: AtomicU32,
    late: AtomicU64,
    /// The device's errors so far, one count per [`DeviceEvent`].
    device_errors: [AtomicU64; 4],
    stalled: AtomicBool,
    /// Held for an export, published after the callback's work, with its stamps (RENDER-1).
    held: AtomicBool,
    /// A device (or the silent clock) calls back: a quiet one has stopped.
    device: AtomicBool,
    /// The last callback played no engine and faded none out: an unload's fade out is done.
    quiet: AtomicBool,
    /// Its device is free again: the stream stopped, or the silent clock ended (next track, NX-S5:
    /// [`Player::freed`]).
    freed: AtomicBool,
    /// Seeks and loads sent, and taken by the audio thread, and the last target: until the
    /// audio thread takes them, they are where playback is.
    seeks_sent: AtomicU64,
    seeks_applied: AtomicU64,
    seek_target: AtomicU64,
    /// Plays and stops the same way: until the audio thread takes the last, it says whether
    /// playback plays.
    plays_sent: AtomicU64,
    plays_applied: AtomicU64,
    play_target: AtomicBool,
    loads_sent: AtomicU64,
    loads_applied: AtomicU64,
    load_pos: AtomicU64,
    /// Where a load not yet taken starts: its beat (f64 bits), and whether it plays.
    load_beat: AtomicU64,
    load_playing: AtomicBool,
    /// Its engine threads, as made and as the callback found them (8a's live half).
    engines: engines::Report,
    /// The device hears zeros: G12 plays muted ([`Player::mute_output`]).
    muted: AtomicBool,
    /// Every callback's time, for G12 (`timing.rs`).
    timing: timing::Record,
}

impl Shared {
    /// cpal's error callback: count `e` by kind, and nothing else. On ALSA it runs on the audio
    /// thread, and CoreAudio's overloads come from its realtime thread, so it never allocates,
    /// locks or prints (RENDER-28): the stream's thread logs the counts. cpal makes an xrun's
    /// error with no text of its own, so dropping it frees nothing.
    fn device_error(&self, e: DeviceError) {
        self.device_errors[DeviceEvent::of(e.kind()) as usize].fetch_add(1, Ordering::Relaxed);
    }

    fn device_errors(&self) -> DeviceErrors {
        let n = |e: DeviceEvent| self.device_errors[e as usize].load(Ordering::Relaxed);
        DeviceErrors {
            xruns: n(DeviceEvent::Xrun),
            changes: n(DeviceEvent::Changed),
            gone: n(DeviceEvent::Gone),
            other: n(DeviceEvent::Other),
        }
    }
}

/// The DSP load (8b.8): how long the audio callback takes, against how long it may. Measured with
/// the clock alone, around the whole callback.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Load {
    /// The longest callback since the last read, as a share of its period: over 1 was late.
    pub longest: f32,
    /// Callbacks that took longer than their period since the player started: each one a dropout.
    pub late: u64,
}

/// Where playback is, as the audio thread last published it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Status {
    /// Transport frame, at the engine's rate.
    pub pos: u64,
    pub beats: f64,
    pub seconds: f64,
    pub playing: bool,
    /// The renderer playing.
    pub generation: u64,
    /// A seek the audio thread hasn't taken yet: its target beat, which is where playback is.
    pub pending: Option<f64>,
}

struct LiveFade {
    old: Box<Live>,
    done: u32,
    len: u32,
}

struct Callback {
    live: Option<Box<Live>>,
    /// The live replaced or unloaded, fading out over 5 ms of the device.
    fade: Option<LiveFade>,
    /// A replace or unload the inbox met: taken at the start of the next callback, so the
    /// messages after it wait for it.
    held: Option<Msg>,
    gone: [Option<Gone>; GONE],
    rx: Receiver<Msg>,
    retired: Sender<Gone>,
    shared: Arc<Shared>,
    a: Vec<[f32; 2]>,
    b: Vec<[f32; 2]>,
    device_rate: u32,
    /// The engine's applied stamps, heard stamps and refused activations published so far.
    published: u64,
    published_heard: u64,
    refused: u64,
    seeks: u64,
    plays: u64,
    loads: u64,
    /// Where each block renders: the player's engine threads once handed over, this thread alone
    /// before (8a's live half).
    runner: Runner,
    between: Between,
}

impl Callback {
    fn new(rx: Receiver<Msg>, retired: Sender<Gone>, shared: Arc<Shared>, device_rate: u32) -> Callback {
        Callback {
            live: None,
            fade: None,
            held: None,
            gone: Default::default(),
            rx,
            retired,
            shared,
            a: vec![[0.0; 2]; SCRATCH],
            b: vec![[0.0; 2]; SCRATCH],
            device_rate,
            published: 0,
            published_heard: 0,
            refused: 0,
            seeks: 0,
            plays: 0,
            loads: 0,
            runner: Runner::one(),
            between: BETWEEN,
        }
    }

    /// Send `g` to be dropped elsewhere, or keep it until there is room. Never drops it here.
    fn retire(&mut self, g: Gone) {
        let g = match self.retired.try_send(g) {
            Ok(()) => return,
            Err(e) => e.into_inner(),
        };
        match self.gone.iter_mut().find(|s| s.is_none()) {
            Some(slot) => *slot = Some(g),
            None => {
                // More than GONE replacements while the dropper is stuck: leak rather than free.
                debug_assert!(false, "nowhere to keep a retired object");
                std::mem::forget(g);
            }
        }
    }

    /// The audio callback: fill `data`, interleaved with `channels` channels.
    fn fill(&mut self, data: &mut [f32], channels: usize) {
        // The device thread's own floating-point mode, as found; then the one mode for all the
        // callback mixes (the resampler, the fades, the click), its own back at the end (RENDER-2a).
        self.shared.engines.found(fpenv::mode());
        let _mode = fpenv::Canon::enter();
        // Marked for the allocation counter in debug builds only, the only ones that install it:
        // a release callback touches no thread-local, whose first touch may allocate (macOS).
        #[cfg(debug_assertions)]
        alloc::set_in_callback(true);
        self.fill_in(data, channels);
        if self.shared.muted.load(Ordering::Relaxed) {
            data.fill(0.0);
        }
        engines::after_callback(&mut self.runner, self.between);
        #[cfg(debug_assertions)]
        alloc::set_in_callback(false);
    }

    fn fill_in(&mut self, data: &mut [f32], channels: usize) {
        let ns = now_ns();
        self.shared.last_callback_ns.store(ns, Ordering::Relaxed);
        self.shared.frames.store((data.len() / channels.max(1)) as u32, Ordering::Relaxed);
        for i in 0..GONE {
            if let Some(g) = self.gone[i].take()
                && let Err(e) = self.retired.try_send(g)
            {
                self.gone[i] = Some(e.into_inner());
                break;
            }
        }
        self.take_held();
        let channels = channels.max(1);
        let frames = data.len() / channels;
        let mut done = 0;
        while done < frames {
            let n = (frames - done).min(SCRATCH);
            if catch_unwind(AssertUnwindSafe(|| self.render(n, CallbackStamp { ns }))).is_err() {
                self.crash();
                self.a[..n].fill([0.0; 2]);
            }
            for (i, f) in self.a[..n].iter().enumerate() {
                let o = (done + i) * channels;
                data[o] = f[0];
                if channels > 1 {
                    data[o + 1] = f[1];
                    data[o + 2..o + channels].fill(0.0);
                }
            }
            done += n;
        }
        self.publish();
        // The DSP load (8b.8): this callback's time against its period, by the clock alone.
        let took = now_ns().saturating_sub(ns);
        let period = frames as u64 * 1_000_000_000 / self.device_rate.max(1) as u64;
        if period > 0 {
            self.shared.longest.fetch_max((took as f32 / period as f32).to_bits(), Ordering::Relaxed);
            if took > period {
                self.shared.late.fetch_add(1, Ordering::Relaxed);
            }
        }
        self.shared.timing.callback(ns, took, frames, period);
    }

    /// Apply a held replace or unload; with nothing playing, take messages until one arrives.
    fn take_held(&mut self) {
        loop {
            let msg = match self.held.take() {
                Some(m) => m,
                None if self.live.is_none() => match self.rx.try_recv() {
                    Ok(m) => m,
                    Err(_) => return,
                },
                None => return,
            };
            match msg {
                Msg::Replace { mut live, keep } => {
                    let old = self.live.take();
                    if keep == Keep::Position
                        && let (Some(n), Some(o)) = (live.engine.current_mut(), old.as_ref().and_then(|o| o.engine.current()))
                    {
                        n.enter_from(o);
                    }
                    self.loads += 1;
                    (self.published, self.published_heard, self.refused) = (0, 0, 0);
                    self.live = Some(live);
                    if let Some(old) = old {
                        self.fade_out(old);
                    }
                }
                Msg::Unload => {
                    if let Some(old) = self.live.take() {
                        self.fade_out(old);
                    }
                }
                // Between runs, like a replace: from now on blocks render on the engine threads.
                // This thread's own set-up comes once, here.
                Msg::Threads(mut h) => {
                    std::mem::swap(&mut h.runner, &mut self.runner);
                    self.between = h.between;
                    self.shared.engines.device(m2_os::rt::on_device_thread(&h.group));
                    self.retire(Gone::Runner(h));
                }
                Msg::Engine(env) => {
                    // Nothing plays: what the message carries is retired unapplied.
                    match env.msg {
                        EngineMsg::Activate { renderer } => self.retire(Gone::Engine(Retired::Renderer(renderer))),
                        EngineMsg::SwapLane { data, .. } => self.retire(Gone::Engine(Retired::Lane(data))),
                        EngineMsg::Transport(Cmd::Seek(_)) => self.seeks += 1,
                        EngineMsg::Transport(Cmd::Play | Cmd::Stop) => self.plays += 1,
                        EngineMsg::Transport(_) | EngineMsg::Panic | EngineMsg::Hold(_) | EngineMsg::Live { .. } => {}
                    }
                }
            }
        }
    }

    /// Fade `old` out over 5 ms of the device; one still fading from before is cut.
    fn fade_out(&mut self, old: Box<Live>) {
        if let Some(f) = self.fade.take() {
            self.retire(Gone::Live(f.old));
        }
        let len = (0.005 * self.device_rate as f64).round().max(1.0) as u32;
        self.fade = Some(LiveFade { old, done: 0, len });
    }

    fn render(&mut self, n: usize, stamp: CallbackStamp) {
        let Callback { live, fade, held, rx, retired, a, b, seeks, plays, shared, runner, .. } = self;
        let mut retire = |r: Retired| match retired.try_send(Gone::Engine(r)) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(Gone::Engine(r)) | TrySendError::Disconnected(Gone::Engine(r))) => Err(r),
            Err(_) => unreachable!("only engine objects are sent here"),
        };
        match live {
            Some(l) => {
                let mut inbox = || {
                    if held.is_some() {
                        return None;
                    }
                    match rx.try_recv() {
                        Ok(Msg::Engine(env)) => {
                            // Transport commands apply as soon as the engine reads them.
                            match env.msg {
                                EngineMsg::Transport(Cmd::Seek(_)) => *seeks += 1,
                                EngineMsg::Transport(Cmd::Play | Cmd::Stop) => *plays += 1,
                                _ => {}
                            }
                            Some(env)
                        }
                        Ok(other) => {
                            *held = Some(other);
                            None
                        }
                        Err(_) => None,
                    }
                };
                l.engine.set_click(shared.click.load(Ordering::Relaxed));
                l.fill(&mut a[..n], &mut inbox, &mut retire, stamp, runner);
            }
            None => a[..n].fill([0.0; 2]),
        }
        if let Some(f) = fade {
            f.old.fill(&mut b[..n], &mut || None, &mut retire, stamp, runner);
            for (k, (x, o)) in a[..n].iter_mut().zip(&b[..n]).enumerate() {
                let i = f.done + k as u32;
                let g = if i < f.len { (i + 1) as f32 / f.len as f32 } else { 1.0 };
                *x = [o[0] * (1.0 - g) + x[0] * g, o[1] * (1.0 - g) + x[1] * g];
            }
            f.done = f.done.saturating_add(n as u32);
        }
        if self.fade.as_ref().is_some_and(|f| f.done >= f.len) {
            let f = self.fade.take().expect("a fade");
            self.retire(Gone::Live(f.old));
        }
    }

    /// What a message never taken holds, to drop elsewhere.
    fn leftover(m: Msg) -> Option<Gone> {
        match m {
            Msg::Replace { live, .. } => Some(Gone::Live(live)),
            Msg::Engine(Envelope { msg: EngineMsg::Activate { renderer }, .. }) => Some(Gone::Engine(Retired::Renderer(renderer))),
            Msg::Engine(Envelope { msg: EngineMsg::SwapLane { data, .. }, .. }) => Some(Gone::Engine(Retired::Lane(data))),
            Msg::Threads(h) => Some(Gone::Runner(h)),
            Msg::Engine(_) | Msg::Unload => None,
        }
    }

    /// A panic in the engine: what played is retired, never dropped here, and the callback
    /// plays silence until a project is opened again.
    fn crash(&mut self) {
        self.shared.crashed.store(true, Ordering::Relaxed);
        if let Some(l) = self.live.take() {
            self.retire(Gone::Live(l));
        }
        if let Some(f) = self.fade.take() {
            self.retire(Gone::Live(f.old));
        }
    }

    fn publish(&mut self) {
        let s = &self.shared;
        let st = self.live.as_ref().map(|l| l.engine.status()).unwrap_or_default();
        let seq = s.seq.load(Ordering::Relaxed);
        s.seq.store(seq + 1, Ordering::Relaxed);
        fence(Ordering::Release);
        s.pos.store(st.pos, Ordering::Relaxed);
        s.beats.store(st.beats.to_bits(), Ordering::Relaxed);
        s.seconds.store(st.seconds.to_bits(), Ordering::Relaxed);
        s.playing.store(st.playing, Ordering::Relaxed);
        s.generation.store(st.generation, Ordering::Relaxed);
        if let Some(l) = &self.live {
            let (stamps, n) = l.engine.applied();
            // Stamps of an engine replaced since are gone; the new engine counts from 0.
            let from = self.published.min(n).max(n.saturating_sub(APPLIED as u64));
            for k in from..n {
                let a = stamps[(k % APPLIED as u64) as usize];
                let slot = &s.applied[(s.applied_n.load(Ordering::Relaxed) % APPLIED as u64) as usize];
                slot.seq.store(a.seq, Ordering::Relaxed);
                slot.clock.store(a.clock, Ordering::Relaxed);
                slot.callback_ns.store(a.callback_ns, Ordering::Relaxed);
                slot.frame.store(a.frame as u64, Ordering::Relaxed);
                slot.refused.store(a.refused, Ordering::Relaxed);
                s.applied_n.fetch_add(1, Ordering::Relaxed);
            }
            self.published = n;
            let (stamps, n) = l.engine.heard();
            let from = self.published_heard.min(n).max(n.saturating_sub(APPLIED as u64));
            for k in from..n {
                let h = stamps[(k % APPLIED as u64) as usize];
                let at = s.heard_n.load(Ordering::Relaxed);
                let slot = &s.heard[(at % APPLIED as u64) as usize];
                slot.n.store(at + 1, Ordering::Relaxed);
                slot.to.store(h.to, Ordering::Relaxed);
                slot.clock.store(h.clock, Ordering::Relaxed);
                slot.callback_ns.store(h.callback_ns, Ordering::Relaxed);
                slot.frame.store(h.frame as u64, Ordering::Relaxed);
                s.heard_n.store(at + 1, Ordering::Relaxed);
            }
            self.published_heard = n;
            let refused = l.engine.refused_activations();
            s.refused.fetch_add(refused - self.refused, Ordering::Relaxed);
            self.refused = refused;
        }
        // Once the callback's work is done: a reader that sees it held may call the instances. Before
        // the stamps are released, so a reader that sees a hold's stamp sees this callback's
        // `held` or a later one, never the one before (an export's wait reads both).
        s.held.store(st.held, Ordering::Release);
        s.quiet.store(self.live.is_none() && self.fade.is_none(), Ordering::Release);
        s.seq.store(seq + 2, Ordering::Release);
        if let Some(l) = &self.live {
            s.clock.store(l.engine.clock(), Ordering::Relaxed);
            let cur = l.engine.current();
            s.timing.engine(l.engine.crashes() + cur.map_or(0, Renderer::stops), cur.map_or(0, Renderer::non_finite));
        }
        s.loop_refused.store(st.loop_refused, Ordering::Relaxed);
        s.dropped_ons.store(st.dropped_ons as u64, Ordering::Relaxed);
        s.live.store(st.live, Ordering::Relaxed);
        s.running.store(self.live.as_ref().is_some_and(|l| !l.engine.is_idle()), Ordering::Relaxed);
        // After the position: a reader that sees a seek, a play or a stop, or a load taken sees
        // where it went.
        s.seeks_applied.store(self.seeks, Ordering::Release);
        s.plays_applied.store(self.plays, Ordering::Release);
        s.loads_applied.store(self.loads, Ordering::Release);
    }
}

impl Drop for Callback {
    /// Its player was let go and its stream stopped (or its silent clock): what it plays, what it
    /// fades out, what waited for it and what it kept leaves for the dropper, as anything it
    /// retires does. So no engine is freed on the audio thread, even by a stream that drops its
    /// callback there (cpal's ALSA stream does). It is no callback any more: what the dropper has no
    /// room for is dropped here.
    fn drop(&mut self) {
        let mut rest = vec![self.live.take().map(Gone::Live), self.fade.take().map(|f| Gone::Live(f.old))];
        rest.extend(self.held.take().map(Callback::leftover));
        rest.extend(self.gone.iter_mut().map(Option::take));
        rest.extend(self.rx.try_iter().map(Callback::leftover));
        for g in rest.into_iter().flatten() {
            let _ = self.retired.try_send(g);
        }
    }
}

/// A handle to the output stream, which lives on its own thread (cpal streams can't
/// move between threads on every platform). Cheap to clone.
#[derive(Clone)]
pub struct Player {
    tx: Sender<Msg>,
    shared: Arc<Shared>,
    next_seq: Arc<AtomicU64>,
    cells: Arc<Writer>,
    /// Held by every clone: once the last is let go, its stream stops and its threads end.
    alive: Arc<()>,
    /// The device's own rate. The engine runs at the project's.
    pub device_rate: u32,
    pub device: String,
}

/// Plays the audio thread's part for tests and tools: [`Driver::fill`] is the device callback.
pub struct Driver {
    cb: Callback,
    retired: Receiver<Gone>,
}

impl Driver {
    /// One device callback, interleaved `channels`.
    pub fn fill(&mut self, data: &mut [f32], channels: usize) {
        self.cb.fill(data, channels);
    }

    /// What the callback retired so far: drop it off the audio thread.
    pub fn drain_retired(&self) -> Vec<Gone> {
        self.retired.try_iter().collect()
    }

    /// The engine playing, if a project is open.
    pub fn engine(&mut self) -> Option<&mut Engine> {
        self.cb.live.as_mut().map(|l| &mut l.engine)
    }

    /// Whether a replaced engine is still fading out.
    pub fn fading(&self) -> bool {
        self.cb.fade.is_some()
    }

    /// One error from the device, as cpal's error callback gets it.
    pub fn error(&self, e: DeviceError) {
        self.cb.shared.device_error(e);
    }

    /// The runner the callback renders on: its counters, spread and trip (tests).
    #[doc(hidden)]
    pub fn runner(&mut self) -> &mut Runner {
        &mut self.cb.runner
    }
}

/// How often a device's thread looks whether its player was let go.
const LOOK: Duration = Duration::from_millis(50);
/// How long a device's thread waits, once its player is let go, for the callback to go quiet: one
/// that stopped calling back never does.
const LINGER: Duration = Duration::from_millis(500);
/// Then how long it lets the device play the buffers the callback filled before.
const PLAYED: Duration = Duration::from_millis(50);

/// A device's thread while `stream` plays: say what the device reported, once a second while it
/// reports anything, until every clone of the player is let go (`alive` is gone: the server lets
/// go of one that stalled, crashed or whose output moved). Then, once the device has played the
/// unload's fade out, stop the stream, which drops the callback: what that played leaves for the
/// dropper ([`Callback`]'s drop), and the dropper ends.
fn serve<S>(stream: S, shared: Arc<Shared>, alive: Weak<()>) {
    let (mut seen, mut said) = (DeviceErrors::default(), Instant::now());
    while alive.strong_count() > 0 {
        std::thread::sleep(LOOK);
        if said.elapsed() >= Duration::from_secs(1) {
            let now = shared.device_errors();
            if let Some(words) = now.since(seen) {
                eprintln!("music2: {words}");
            }
            (seen, said) = (now, Instant::now());
        }
    }
    // The server unloads a player before it lets go of it, and the engine fades out over 5 ms in
    // the next callback: a stream stopped before the device played that cuts the sound off with a
    // click (an output that moved may still sound).
    let t = Instant::now();
    while !shared.quiet.load(Ordering::Acquire) && t.elapsed() < LINGER {
        std::thread::sleep(Duration::from_millis(2));
    }
    if shared.quiet.load(Ordering::Acquire) {
        std::thread::sleep(PLAYED);
    }
    drop(stream);
    shared.freed.store(true, Ordering::Release);
}

/// Drop what the audio thread retires, polling: no thread ever blocks on the channel. Ends once
/// the callback is gone and everything it retired is dropped.
fn drop_retired(retired: Receiver<Gone>) {
    std::thread::Builder::new()
        .name("m2-play-drop".into())
        .spawn(move || {
            loop {
                loop {
                    match retired.try_recv() {
                        Ok(g) => drop(g),
                        Err(crossbeam_channel::TryRecvError::Empty) => break,
                        Err(crossbeam_channel::TryRecvError::Disconnected) => return,
                    }
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        })
        .expect("a thread for dropping");
}

impl Player {
    fn parts(device_rate: u32, device: &str) -> (Player, Callback, Receiver<Gone>) {
        now_ns();
        let (tx, rx) = bounded::<Msg>(QUEUE);
        let (retired_tx, retired) = bounded::<Gone>(RETIRED);
        let shared = Arc::new(Shared::default());
        let cb = Callback::new(rx, retired_tx, shared.clone(), device_rate);
        let player = Player {
            tx,
            shared,
            next_seq: Arc::default(),
            cells: Arc::default(),
            alive: Arc::default(),
            device_rate,
            device: device.into(),
        };
        (player, cb, retired)
    }

    /// Open the default output device. With `M2_AUDIO=none`, run silently on a clock
    /// instead (for automated tests).
    pub fn start() -> Result<Player> {
        Player::start_on(&Choice::default())
    }

    /// Open the output `choice` names at its buffer size: the default when it names none or one
    /// that is gone (then [`Player::fell_back`]), and the device's own buffer size when it can't
    /// take that one. With `M2_AUDIO=none`, a silent player named after the choice
    /// ([`outputs::outputs`]); with `M2_AUDIO=fail`, no output starts (a test seam).
    pub fn start_on(choice: &Choice) -> Result<Player> {
        Player::start_in(outputs::Audio::from_env(), choice)
    }

    /// The default output device whatever `M2_AUDIO` says, its buffers `frames` frames (when it
    /// takes them), on the engine threads `live` asks for: G12's live run on a Mac.
    #[doc(hidden)]
    pub fn start_device(frames: u32, live: LiveThreads) -> Result<Player> {
        Player::start_live(outputs::Audio::Device, &Choice { device: None, frames: Some(frames) }, live)
    }

    fn start_in(audio: outputs::Audio, choice: &Choice) -> Result<Player> {
        Player::start_live(audio, choice, LiveThreads::from_env()?)
    }

    fn start_live(audio: outputs::Audio, choice: &Choice, live: LiveThreads) -> Result<Player> {
        match audio {
            outputs::Audio::Silent(names) => {
                let (name, fell_back, frames) = outputs::Audio::silent_pick(&names, choice);
                let player = Player::silent_as(&name, frames, live)?;
                player.shared.fell_back.store(fell_back, Ordering::Relaxed);
                return Ok(player);
            }
            outputs::Audio::Fail => return Err(anyhow!(outputs::NO_OUTPUT)),
            outputs::Audio::Device => {}
        }
        let choice = choice.clone();
        let (ready_tx, ready) = bounded::<Result<Player>>(1);
        std::thread::Builder::new().name("m2-play".into()).spawn(move || {
            type Opened = (cpal::Stream, Player, Receiver<Gone>, Option<String>);
            let open_on = |device: cpal::Device, fell_back: bool| -> Result<Opened> {
                let name = outputs::name_of(&device).unwrap_or_else(|| "output".into());
                let uid = device.id().ok().map(|id| id.id().to_string());
                let supported = device.default_output_config()?;
                let mut config = supported.config();
                config.buffer_size = outputs::buffer(choice.frames, supported.buffer_size());
                let channels = config.channels as usize;
                let (player, mut cb, retired) = Player::parts(config.sample_rate, &name);
                player.shared.fell_back.store(fell_back, Ordering::Relaxed);
                let shared = player.shared.clone();
                let stream = device.build_output_stream::<f32, _, _>(
                    config,
                    move |data: &mut [f32], _| cb.fill(data, channels),
                    move |e| shared.device_error(e),
                    None,
                )?;
                stream.play()?;
                player.shared.device.store(true, Ordering::Relaxed);
                Ok((stream, player, retired, uid))
            };
            // The output chosen, else the default when it is gone or won't open (NX-S1).
            let open = || {
                let mut failed = anyhow!(outputs::NO_OUTPUT);
                for (device, fell_back) in outputs::tries(&choice) {
                    match open_on(device, fell_back) {
                        Ok(opened) => return Ok(opened),
                        Err(e) => failed = e,
                    }
                }
                Err(failed)
            };
            match open() {
                Ok((stream, player, retired, uid)) => {
                    let (shared, alive) = (player.shared.clone(), Arc::downgrade(&player.alive));
                    // Engine threads once the device calls back, handed over before any project
                    // (8a's live half). A cancelled workgroup asks for a new player.
                    let first = || shared.frames.load(Ordering::Relaxed);
                    let engines = engines::for_device(uid.as_deref(), player.device_rate, live, &first, &shared.engines);
                    if engines.cancelled {
                        shared.device_errors[DeviceEvent::Changed as usize].fetch_add(1, Ordering::Relaxed);
                    }
                    let _ = player.send(Msg::Threads(engines.handover));
                    let _ = ready_tx.send(Ok(player));
                    drop_retired(retired);
                    // The stream stops first, then the pool joins its workers (each leaving the
                    // workgroup), then the device is free.
                    serve((stream, engines.pool), shared, alive);
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                }
            }
        })?;
        ready.recv()?
    }

    /// The same player, rendering into nothing in real time: no device, no sound. Device
    /// buffers are `M2_AUDIO_FRAMES` frames (480 unless set) at 48 kHz.
    pub fn silent() -> Result<Player> {
        Player::silent_as("none", None, LiveThreads::from_env()?)
    }

    /// [`Player::silent`] with its buffers `frames` frames, on the engine threads `live` asks for
    /// (tests, and G12 on a machine with no device).
    #[doc(hidden)]
    pub fn silent_with(frames: u32, live: LiveThreads) -> Result<Player> {
        Player::silent_as("none", Some(frames), live)
    }

    /// [`Player::silent`], named `name`, its buffers `frames` frames when given, rendering on the
    /// engine threads `live` asks for: plain threads, one unless it says more.
    fn silent_as(name: &str, frames: Option<u32>, live: LiveThreads) -> Result<Player> {
        const SR: u32 = outputs::SILENT_RATE;
        let frames: usize = match frames {
            Some(n) => n as usize,
            None => std::env::var("M2_AUDIO_FRAMES").ok().and_then(|v| v.parse().ok()).filter(|&n| n > 0).unwrap_or(480),
        };
        let (player, mut cb, retired) = Player::parts(SR, name);
        // Handed over as a device's are: the first message, taken at the first callback.
        let engines = engines::for_clock(frames, SR, live, &player.shared.engines);
        player.send(Msg::Threads(engines.handover))?;
        let pool = engines.pool;
        player.shared.device.store(true, Ordering::Relaxed);
        let alive = Arc::downgrade(&player.alive);
        let shared = player.shared.clone();
        std::thread::Builder::new().name("m2-play-silent".into()).spawn(move || {
            let mut buf = vec![0.0f32; frames * 2];
            let period = Duration::from_secs_f64(frames as f64 / SR as f64);
            let mut next = Instant::now();
            // Until the player is let go; then `cb` drops here, its engine leaving for the dropper.
            while alive.strong_count() > 0 {
                cb.fill(&mut buf, 2);
                next += period;
                // A callback that ran late doesn't make the next ones early.
                next = next.max(Instant::now().checked_sub(period).unwrap_or(next));
                std::thread::sleep(next.saturating_duration_since(Instant::now()));
            }
            // The callback first, then its engine threads, each worker leaving as it ends.
            drop(cb);
            drop(pool);
            shared.freed.store(true, Ordering::Release);
        })?;
        drop_retired(retired);
        Ok(player)
    }

    /// A player whose callback the caller runs ([`Driver::fill`]), at `device_rate`.
    pub fn driven(device_rate: u32) -> (Player, Driver) {
        let (player, cb, retired) = Player::parts(device_rate, "driven");
        // No device of its own: free at once.
        player.shared.freed.store(true, Ordering::Relaxed);
        (player, Driver { cb, retired })
    }

    /// [`Player::driven`], each block rendered on `runner`'s threads: tests keep their own pool.
    pub fn driven_on(device_rate: u32, runner: Runner) -> (Player, Driver) {
        let (player, mut d) = Player::driven(device_rate);
        player.shared.engines.made(runner.threads(), None, None, BETWEEN, None);
        d.cb.runner = runner;
        (player, d)
    }

    /// What its engine threads are and did (G12's set-up line).
    pub fn engine_threads(&self) -> EngineThreads {
        self.shared.engines.read()
    }

    /// Send `msg`, waiting for room at most [`STALL`]; then the player is stalled and the
    /// message is dropped here, never silently (the caller gets [`Stalled`]).
    fn send(&self, msg: Msg) -> Result<()> {
        let start = Instant::now();
        let mut msg = msg;
        loop {
            match self.tx.try_send(msg) {
                Ok(()) => return Ok(()),
                Err(TrySendError::Full(m)) => {
                    if start.elapsed() > STALL {
                        self.shared.stalled.store(true, Ordering::Relaxed);
                        return Err(Stalled.into());
                    }
                    msg = m;
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(TrySendError::Disconnected(_)) => {
                    // Its callback is gone: as stalled as one that stopped taking messages.
                    self.shared.stalled.store(true, Ordering::Relaxed);
                    return Err(anyhow!("the audio player stopped"));
                }
            }
        }
    }

    fn envelope(&self, msg: EngineMsg) -> Result<u64> {
        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed) + 1;
        self.send(Msg::Engine(Envelope { seq, msg }))?;
        Ok(seq)
    }

    fn replace_with(&self, mut renderer: Renderer, keep: Keep) -> Result<()> {
        self.cells.sending(&mut renderer);
        self.shared.rate.store(renderer.sample_rate(), Ordering::Relaxed);
        self.shared.loads_sent.fetch_add(1, Ordering::Release);
        let sent = self.send(Msg::Replace { live: Box::new(Live::new(renderer, self.device_rate)), keep });
        if sent.is_err() {
            // Never taken: the position is the published one again.
            self.shared.loads_sent.fetch_sub(1, Ordering::Release);
        }
        sent
    }

    /// Play a newly opened project: a fresh engine for `renderer`, stopped at its start. What
    /// played before fades out.
    pub fn open(&self, mut renderer: Renderer) -> Result<()> {
        renderer.set_strict(false);
        renderer.set_transport(Transport::new(0, false));
        self.loading(0, 0.0, false);
        self.replace_with(renderer, Keep::Nothing)
    }

    /// A fresh engine for `renderer` (a rate change), at the same beat, play state and loop.
    pub fn replace(&self, mut renderer: Renderer) -> Result<()> {
        renderer.set_strict(false);
        // The same beat, in the new renderer's frames.
        let now = self.status();
        let beat = now.pending.unwrap_or(now.beats);
        let pos = m2_runtime::transport::frame_of(renderer.tempo(), beat, renderer.sample_rate() as f64);
        self.loading(pos, beat, now.playing);
        self.replace_with(renderer, Keep::Position)
    }

    /// Where a load about to be sent starts, which [`Self::status`] reports until it is taken.
    fn loading(&self, pos: u64, beat: f64, playing: bool) {
        self.shared.load_pos.store(pos, Ordering::Relaxed);
        self.shared.load_beat.store(beat.to_bits(), Ordering::Relaxed);
        self.shared.load_playing.store(playing, Ordering::Relaxed);
    }

    /// Nothing plays any more (the project closed): the engine fades out and is dropped.
    pub fn unload(&self) -> Result<()> {
        self.send(Msg::Unload)
    }

    /// Play `renderer`, prepared against the plan sent last, from where the engine is: what it
    /// keeps moves into it, and what changed crossfades, with no jump back (RENDER-25a). Returns
    /// its generation and the message's sequence number.
    pub fn activate(&self, mut renderer: Renderer) -> Result<(u64, u64)> {
        renderer.set_strict(false);
        self.cells.sending(&mut renderer);
        let generation = renderer.generation();
        Ok((generation, self.envelope(EngineMsg::Activate { renderer: Box::new(renderer) })?))
    }

    /// Replace lane `slot`'s data in the renderer `generation` (RENDER-26).
    pub fn swap_lane(&self, generation: u64, slot: u32, data: LaneSwap) -> Result<u64> {
        self.envelope(EngineMsg::SwapLane { generation, slot, data })
    }

    /// A transport command, applied at the next block line (RENDER-9). Never waits for an edit.
    /// Until the audio thread takes it, [`Self::status`] reports a seek, a play or a stop as done.
    pub fn transport(&self, cmd: Cmd) -> Result<u64> {
        let s = &self.shared;
        let sent = match cmd {
            Cmd::Seek(beat) => {
                s.seek_target.store(beat.max(0.0).to_bits(), Ordering::Relaxed);
                Some(&s.seeks_sent)
            }
            Cmd::Play | Cmd::Stop => {
                s.play_target.store(cmd == Cmd::Play, Ordering::Relaxed);
                Some(&s.plays_sent)
            }
            Cmd::Loop(_) => None,
        };
        if let Some(n) = sent {
            n.fetch_add(1, Ordering::Release);
        }
        let result = self.envelope(EngineMsg::Transport(cmd));
        if let (Some(n), Err(_)) = (sent, &result) {
            n.fetch_sub(1, Ordering::Release);
        }
        result
    }

    /// Move playback to `beat`, playing or stopped. Notes sounding there aren't chased.
    pub fn seek(&self, beat: f64) -> Result<u64> {
        if !beat.is_finite() {
            return Err(anyhow!("can't seek to beat {beat}"));
        }
        self.transport(Cmd::Seek(beat))
    }

    /// Loop between two beats, or stop looping.
    pub fn set_loop(&self, beats: Option<(f64, f64)>) -> Result<u64> {
        self.transport(Cmd::Loop(beats))
    }

    /// A live note's On or Off, or its source's release, for lane `slot` of the renderer
    /// `generation` (MON-7): in order behind the activations sent before it, so it reaches the plan
    /// it names. Never refused here: the engine refuses what doesn't fit, and counts it.
    pub fn live(&self, generation: u64, slot: u32, ev: LiveEv) -> Result<u64> {
        self.envelope(EngineMsg::Live { generation, slot, ev })
    }

    /// Every note off and every instance reset (RENDER-10).
    pub fn panic(&self) -> Result<u64> {
        self.envelope(EngineMsg::Panic)
    }

    /// Pause for an export that borrows the live plugin instances, or resume (RENDER-1): the
    /// engine fades out over 5 ms and processes nothing until it resumes where it held, fading in.
    /// A hold applies in order with activations and swaps. A resume applies as soon as the engine
    /// reads it, never behind an activation that waits for it, but never before its hold.
    pub fn hold(&self, on: bool) -> Result<u64> {
        self.envelope(EngineMsg::Hold(on))
    }

    /// Whether the engine is held: published after the callback that held it returned, so once
    /// this is true no plugin of what it plays is called until it resumes (RENDER-1).
    pub fn held(&self) -> bool {
        self.shared.held.load(Ordering::Acquire)
    }

    /// The metronome on or off: a click on each beat while the song plays, from the next callback,
    /// whatever plays then (a project opened later too). Never in an export: exports never make an
    /// engine (RENDER-1).
    pub fn set_click(&self, on: bool) {
        self.shared.click.store(on, Ordering::Relaxed);
    }

    pub fn click(&self) -> bool {
        self.shared.click.load(Ordering::Relaxed)
    }

    /// Start playing, for tests and tools; [`Player::transport`] says whether it was sent.
    pub fn play(&self) {
        let _ = self.transport(Cmd::Play);
    }

    /// Stop: notes end, tails ring on (RENDER-6a). For tests and tools, as [`Player::play`].
    pub fn stop(&self) {
        let _ = self.transport(Cmd::Stop);
    }

    /// Write `value` into `target`'s cells (a `SetCell`, RENDER-22): heard at the renderer's next
    /// block line, with no rebuild, never through the queue, so never dropped or kept waiting
    /// (RENDER-28). The latest write to a cell wins. `value` is in the constant's own units.
    /// Returns the write's seq, for [`Player::applied`].
    pub fn set_cells(&self, target: &CellTarget, value: f64) -> u64 {
        self.cells.set(target, value as f32)
    }

    /// The rate positions count in: the engine's, which is the project's (RENDER-31). The
    /// device's until something is loaded.
    pub fn rate(&self) -> u32 {
        match self.shared.rate.load(Ordering::Relaxed) {
            0 => self.device_rate,
            r => r,
        }
    }

    /// The last whole publication of the audio thread.
    fn published(&self) -> Status {
        let s = &self.shared;
        loop {
            let a = s.seq.load(Ordering::Acquire);
            if a & 1 == 1 {
                std::hint::spin_loop();
                continue;
            }
            let st = Status {
                pos: s.pos.load(Ordering::Relaxed),
                beats: f64::from_bits(s.beats.load(Ordering::Relaxed)),
                seconds: f64::from_bits(s.seconds.load(Ordering::Relaxed)),
                playing: s.playing.load(Ordering::Relaxed),
                generation: s.generation.load(Ordering::Relaxed),
                pending: None,
            };
            fence(Ordering::Acquire);
            if s.seq.load(Ordering::Relaxed) == a {
                return st;
            }
        }
    }

    /// Where playback is. Never waits for anything: a project loaded, or a seek, a play or a stop
    /// sent, that the audio thread hasn't taken yet is reported as where it goes. So a stop then a
    /// seek back reads stopped there at once, never playing on from there for a block.
    pub fn status(&self) -> Status {
        let s = &self.shared;
        let mut st = self.published();
        if s.loads_sent.load(Ordering::Acquire) > s.loads_applied.load(Ordering::Acquire) {
            let (pos, beat) = (s.load_pos.load(Ordering::Relaxed), f64::from_bits(s.load_beat.load(Ordering::Relaxed)));
            st = Status {
                pos,
                beats: beat,
                seconds: pos as f64 / self.rate() as f64,
                playing: s.load_playing.load(Ordering::Relaxed),
                pending: Some(beat),
                ..st
            };
        }
        if s.seeks_sent.load(Ordering::Acquire) > s.seeks_applied.load(Ordering::Acquire) {
            st.pending = Some(f64::from_bits(s.seek_target.load(Ordering::Relaxed)));
        }
        if s.plays_sent.load(Ordering::Acquire) > s.plays_applied.load(Ordering::Acquire) {
            st.playing = s.play_target.load(Ordering::Relaxed);
        }
        st
    }

    /// Engine frames: where the transport is. A renderer loaded and not yet taken is already
    /// where it starts.
    pub fn position(&self) -> u64 {
        let s = &self.shared;
        if s.loads_sent.load(Ordering::Acquire) > s.loads_applied.load(Ordering::Acquire) {
            s.load_pos.load(Ordering::Relaxed)
        } else {
            self.published().pos
        }
    }

    pub fn is_playing(&self) -> bool {
        self.published().playing
    }

    /// The engine clock after the last callback.
    pub fn clock(&self) -> u64 {
        self.shared.clock.load(Ordering::Relaxed)
    }

    /// When message or cell write `seq` took effect, while it is among the last few applied.
    pub fn applied(&self, seq: u64) -> Option<Applied> {
        if seq > CELL_SEQ {
            return self.heard(seq);
        }
        let s = &self.shared;
        loop {
            let a = s.seq.load(Ordering::Acquire);
            if a & 1 == 1 {
                std::hint::spin_loop();
                continue;
            }
            let found = s.applied.iter().find(|x| x.seq.load(Ordering::Relaxed) == seq).map(|x| Applied {
                seq,
                clock: x.clock.load(Ordering::Relaxed),
                callback_ns: x.callback_ns.load(Ordering::Relaxed),
                frame: x.frame.load(Ordering::Relaxed) as u32,
                refused: x.refused.load(Ordering::Relaxed),
            });
            fence(Ordering::Acquire);
            if s.seq.load(Ordering::Relaxed) == a {
                return found;
            }
        }
    }

    /// When cell write `seq` was heard: the first block, among the last few stamped, that applied
    /// it or a later write. It may be a block late, never early (see `m2_runtime::CellSlots`).
    pub fn heard(&self, seq: u64) -> Option<Applied> {
        let s = &self.shared;
        loop {
            let a = s.seq.load(Ordering::Acquire);
            if a & 1 == 1 {
                std::hint::spin_loop();
                continue;
            }
            let found = s
                .heard
                .iter()
                .filter(|x| x.n.load(Ordering::Relaxed) > 0 && x.to.load(Ordering::Relaxed) >= seq)
                .min_by_key(|x| x.n.load(Ordering::Relaxed))
                .map(|x| Applied {
                    seq,
                    clock: x.clock.load(Ordering::Relaxed),
                    callback_ns: x.callback_ns.load(Ordering::Relaxed),
                    frame: x.frame.load(Ordering::Relaxed) as u32,
                    refused: false,
                });
            fence(Ordering::Acquire);
            if s.seq.load(Ordering::Relaxed) == a {
                return found;
            }
        }
    }

    /// Nanoseconds of [`Applied::callback_ns`]'s clock, now.
    pub fn now_ns(&self) -> u64 {
        now_ns()
    }

    /// The device's buffer, in frames, as its last callback asked: None before the first.
    pub fn frames(&self) -> Option<u32> {
        Some(self.shared.frames.load(Ordering::Relaxed)).filter(|&n| n > 0)
    }

    /// Whether the output chosen is gone, so this plays on the default ([`Player::start_on`]).
    pub fn fell_back(&self) -> bool {
        self.shared.fell_back.load(Ordering::Relaxed)
    }

    /// Whether the audio thread stopped taking messages: a sender timed out, the device is gone
    /// ([`DeviceEvent::Gone`]), or it hasn't called back for a second. Nothing else the device
    /// reports stalls the player: an xrun is only counted ([`Player::device_errors`]).
    pub fn stalled(&self) -> bool {
        let s = &self.shared;
        let quiet = s.device.load(Ordering::Relaxed)
            && now_ns().saturating_sub(s.last_callback_ns.load(Ordering::Relaxed)) > SILENT_DEVICE.as_nanos() as u64
            && s.last_callback_ns.load(Ordering::Relaxed) > 0;
        let gone = s.device_errors[DeviceEvent::Gone as usize].load(Ordering::Relaxed) > 0;
        s.stalled.load(Ordering::Relaxed) || gone || quiet
    }

    /// Whether the output moved to another device ([`DeviceEvent::Changed`]): this player plays
    /// on there, but a new one should take over, opened at the new device's rate and period.
    pub fn changed(&self) -> bool {
        self.shared.device_errors[DeviceEvent::Changed as usize].load(Ordering::Relaxed) > 0
    }

    /// The errors the device reported since this player started, by kind.
    pub fn device_errors(&self) -> DeviceErrors {
        self.shared.device_errors()
    }

    /// Activations the audio thread refused, in all: each was prepared against a plan that wasn't
    /// playing when it arrived, which only a bug does. The sender recovers with a fresh engine.
    pub fn refused(&self) -> u64 {
        self.shared.refused.load(Ordering::Relaxed)
    }

    /// Whether the engine panicked: it plays silence until a project is opened again.
    pub fn crashed(&self) -> bool {
        self.shared.crashed.load(Ordering::Relaxed)
    }

    /// Whether the last loop was refused, or a tempo change made it too short.
    pub fn loop_refused(&self) -> bool {
        self.shared.loop_refused.load(Ordering::Relaxed)
    }

    /// Ons dropped by full event buffers in the renderer playing.
    pub fn dropped_ons(&self) -> u64 {
        self.shared.dropped_ons.load(Ordering::Relaxed)
    }

    /// Live notes held in the renderer playing, after the last callback (MON-7).
    pub fn live_notes(&self) -> u32 {
        self.shared.live.load(Ordering::Relaxed)
    }

    /// The DSP load since the last call (8b.8): reading it starts the longest callback again, so
    /// one reader only (the server's meters).
    pub fn load(&self) -> Load {
        Load { longest: f32::from_bits(self.shared.longest.swap(0, Ordering::Relaxed)), late: self.shared.late.load(Ordering::Relaxed) }
    }

    /// Whether the engine renders nothing: it idles once stopped and silent (RENDER-6a), or no
    /// project plays. As of the last callback.
    pub fn idle(&self) -> bool {
        !self.shared.running.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
#[path = "load_tests.rs"]
mod load_tests;
