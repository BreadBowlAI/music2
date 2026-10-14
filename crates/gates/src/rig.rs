//! The gates' only view of an engine: open a song, drive it a device buffer at a time, send it
//! what the app would send, and read the counters it keeps. Time is simulated: frames pass only
//! when the rig is filled, so a run never waits for a clock. What happens off the audio thread
//! (building a renderer, replaying) is timed on the wall clock, reported as `Sent::prepare`, and
//! played through: the device plays on for that many frames meanwhile ([`Deck::change`]).

use std::time::Duration;

use anyhow::Result;
use m2_song::LaneAddr;

use crate::{Lab, Song};

/// Transport, as the app sends it. Beats, so a command means the same in any tempo map.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cmd {
    Play,
    Stop,
    Seek(f64),
    /// A loop over `[start, end)` beats, or none.
    Loop(Option<(f64, f64)>),
    /// Every note off and every kernel reset (RENDER-10's one reset on demand).
    Panic,
}

/// A message on its way to the audio thread.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sent {
    pub id: u64,
    /// Engine frames the device had taken when it was sent.
    pub at: u64,
    /// Wall time spent before it could be sent: building a renderer, preparing lane data, or
    /// at HEAD, replaying the song to the playhead.
    pub prepare: Duration,
    /// A data edit that took `SwapLane` rather than a new renderer (RENDER-26).
    pub swap: bool,
}

/// What the engine says about itself. None: this engine doesn't keep that.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Probe {
    /// Engine frames so far (the engine clock, RENDER-6).
    pub clock: u64,
    /// The transport position, in frames, after the last fill.
    pub transport: Option<u64>,
    pub playing: Option<bool>,
    /// Kernel resets on the running engine, counted where a renderer's graph resets: a panic, or
    /// at HEAD every replay (RENDER-10).
    pub resets: Option<u64>,
    /// How many renderers have been activated after the first.
    pub generation: Option<u64>,
    /// Notes the lane schedulers hold on (RENDER-26a).
    pub held_notes: Option<usize>,
}

/// The plugin instances an engine plays (step 3b), and the process's counts.
#[derive(Clone, Debug, Default)]
pub struct Plugins {
    /// The playing renderer's top-level plugin nodes: their instances, the notes sounding into
    /// each, and the NoteOffs each was sent.
    pub nodes: Vec<m2_runtime::PluginView>,
    /// Instances created and destroyed in this process so far.
    pub created: u64,
    pub destroyed: u64,
    /// Whether the engine idled at any time since the rig opened its song (RENDER-6a).
    pub idled: bool,
    /// Who the rig's instances report to: its count of them outlives the rig.
    pub owner: Option<m2_plugins::Owner>,
}

/// What the playing plan's activation kept and made (step 7), and the engine's own counts.
#[derive(Clone, Debug, Default)]
pub struct Arena {
    pub stats: m2_runtime::Stats,
    /// Activations refused for not fitting what plays, and retired objects forgotten.
    pub refused: u64,
    pub leaked: u64,
    /// Whether its fade window runs.
    pub window: bool,
}

/// A note-on from the engine's trace: where it sounded on both clocks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NoteOn {
    pub engine: u64,
    pub transport: u64,
    pub pitch: f32,
}

/// A change the app makes to what plays, besides transport.
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    /// A new graph: a structural edit (RENDER-25, 25a).
    Structure,
    /// Only the data of these lanes changed: a data edit, which takes `SwapLane` where the engine
    /// can (RENDER-26), else a new renderer.
    Data(Vec<LaneAddr>),
}

pub trait Rig {
    fn name(&self) -> String;
    fn rate(&self) -> u32;
    /// Play `song` on a freshly started engine, stopped at transport 0, nothing heard yet.
    fn open(&mut self, song: &Song) -> Result<()>;
    /// The next `out.len()` device frames, at the engine's rate. A message sent before a fill
    /// takes effect at the engine's next block line.
    fn fill(&mut self, out: &mut [[f32; 2]]) -> Result<()>;
    fn send(&mut self, cmd: Cmd) -> Result<Sent>;
    /// Make what `song` needs to play instead of what plays, off the audio thread, and keep it for
    /// [`Rig::commit`]. Returns how long that took; the device plays on meanwhile.
    fn prepare(&mut self, song: &Song, change: &Change) -> Result<Duration>;
    /// Send what [`Rig::prepare`] made.
    fn commit(&mut self) -> Result<Sent>;
    /// The engine frame `sent` took effect at, once it has.
    fn applied(&mut self, sent: &Sent) -> Option<u64>;
    fn probe(&mut self) -> Probe;
    /// Note-ons since the last call, from the engine's trace. None: it keeps no trace.
    fn ons(&mut self) -> Option<Vec<NoteOn>> {
        None
    }

    /// Assets decoded and clips rendered since the rig opened its song. None: not counted.
    fn audio_work(&self) -> Option<(usize, usize)> {
        None
    }

    /// Its plugin instances. None: this engine doesn't say.
    fn plugins(&mut self) -> Option<Plugins> {
        None
    }

    /// What the playing plan's activation kept and made. None: this engine has no arena.
    fn arena(&mut self) -> Option<Arena> {
        None
    }

    /// The last block's output of top-level node `node` of the playing plan (G7b's taps). None:
    /// this engine doesn't say.
    fn output_of(&mut self, _node: usize) -> Option<Vec<f32>> {
        None
    }
}

/// Which engine a gate measures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// HEAD's: rebuild and replay (see [`crate::replay`]).
    Replay,
    /// Step 3a's live engine (the `engine` feature).
    Engine,
}

impl Kind {
    pub fn name(&self) -> &'static str {
        match self {
            Kind::Replay => "replay",
            Kind::Engine => "engine",
        }
    }

    pub fn make(&self, lab: &Lab) -> Result<Box<dyn Rig>> {
        match self {
            Kind::Replay => Ok(Box::new(crate::replay::Replay::new(lab.packages.clone()))),
            #[cfg(feature = "engine")]
            Kind::Engine => Ok(Box::new(crate::engine::Live::new(lab.packages.clone(), lab.arena)?)),
            #[cfg(not(feature = "engine"))]
            Kind::Engine => anyhow::bail!("this build has no live engine: build with `--features engine` once step 3a lands"),
        }
    }
}

/// A rig driven in device buffers of `period` frames, keeping everything it played.
pub struct Deck {
    pub rig: Box<dyn Rig>,
    pub tape: Vec<[f32; 2]>,
    pub period: usize,
    pub rate: u32,
    buf: Vec<[f32; 2]>,
}

/// The silent player's buffer the gates time with (`M2_AUDIO_FRAMES=512`): 10.7 ms at 48 kHz.
pub const PERIOD: usize = 512;

impl Deck {
    pub fn new(rig: Box<dyn Rig>, period: usize) -> Deck {
        let rate = rig.rate();
        Deck { rig, tape: vec![], period: period.max(1), rate, buf: vec![[0.0; 2]; period.max(1)] }
    }

    /// Engine frames played so far.
    pub fn now(&self) -> u64 {
        self.tape.len() as u64
    }

    pub fn frames(&self, seconds: f64) -> u64 {
        (seconds * self.rate as f64).round() as u64
    }

    /// Play `frames` more, a device buffer at a time (the last one shorter if need be).
    pub fn run(&mut self, frames: u64) -> Result<()> {
        let mut left = frames as usize;
        while left > 0 {
            let n = left.min(self.period);
            self.rig.fill(&mut self.buf[..n])?;
            self.tape.extend_from_slice(&self.buf[..n]);
            left -= n;
        }
        Ok(())
    }

    /// Play on to engine frame `to`.
    pub fn run_to(&mut self, to: u64) -> Result<()> {
        self.run(to.saturating_sub(self.now()))
    }

    /// Change what plays as the app does: prepare it off the audio thread while the device plays
    /// on (in simulated time, the frames the preparing took on the wall clock), then send it. An
    /// engine that builds at the position it read before preparing jumps back by that much.
    pub fn change(&mut self, song: &Song, change: Change) -> Result<Sent> {
        let took = self.rig.prepare(song, &change)?;
        self.run(self.frames(took.as_secs_f64()))?;
        self.rig.commit()
    }

    /// [`Deck::change`], with the device playing `frames` while it prepares rather than what the
    /// preparing took: for a twin whose change must be sent at the engine frame another deck's was.
    pub fn change_after(&mut self, song: &Song, change: Change, frames: u64) -> Result<Sent> {
        self.rig.prepare(song, &change)?;
        self.run(frames)?;
        self.rig.commit()
    }

    /// Play about `seconds` more, ending on a block line (`m2_runtime::BLOCK`), where the engine
    /// takes messages: so a message sent next applies at the frame it was sent at.
    pub fn run_secs(&mut self, seconds: f64) -> Result<()> {
        let block = m2_runtime::BLOCK as u64;
        let to = (self.now() + self.frames(seconds)).div_ceil(block) * block;
        self.run_to(to)
    }

    /// When `sent` was heard, counted from when it was asked for: what preparing it took, then
    /// the frames until the engine applied it, then one device buffer to reach the speaker
    /// (step 3a's design, §15: the silent player's buffer is counted, not subtracted). None until
    /// it has been applied.
    pub fn latency_ms(&mut self, sent: &Sent) -> Option<f64> {
        let at = self.rig.applied(sent)?;
        let frames = at.saturating_sub(sent.at) + self.period as u64;
        Some(sent.prepare.as_secs_f64() * 1e3 + frames as f64 * 1e3 / self.rate as f64)
    }
}
