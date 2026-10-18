//! Recording what you play (lane B, gaps #24): a take on one notes lane, written as one clip.
//!
//! - **Start** ([`Graph::record_start`]): on a notes lane of what plays. Stopped, the take starts
//!   where play starts: after one bar of clicks at the tempo and meter there (`Player::count_in`),
//!   or at once. Playing, it starts now, with no count. Only the control plane is read, never the
//!   session: R is transport, and never waits for a compile (RENDER-29).
//! - **Notes:** each On and Off audition sends on the take's lane, from a MIDI keyboard that plays
//!   or from a window that marks its notes `record` (the keyboard piano), goes where it was heard
//!   (`Player::heard_frame`, D4). Its time is the engine's clock, which never wraps, placed through
//!   the tempo map that plays from the take's anchor: the clock where the transport was at the
//!   take's start. A note at least 1/64 beat long. A note struck in the count's last beat lands on
//!   the take's first beat; earlier ones are heard, not kept (D27). The count's beat is measured in
//!   time, as its clicks are, whatever the tempo before the start (the review's R4).
//! - **First pass only** (Q27, D5): a note heard past the loop's end isn't kept, and the take ends
//!   there. A loop turned on, moved or turned off while it records moves that end (the review's
//!   R2). A Stop, R, a jump, or a new player ends it where it is heard: a new player where it was
//!   last heard (the review's R5).
//! - **Ending never waits** (D7): held notes end and the end bar is fixed under the control plane's
//!   lock, and the take waits in a queue. The take thread ([`Graph::record_tick`]) writes it: one
//!   `Clips` Tx on the lane (`Gesture::Set`), one undo step, "recording on Bass".
//! - **The code must compile** (the review's R1): R is refused while it doesn't, as every edit is. A
//!   take that ends while it doesn't waits in the queue, said once, and is written once it does.
//! - **The windows hear** `{"t": "take", …}`: the take's phase and notes when they change, then
//!   `written`, `empty`, or `error`.
//! - **An audio lane** (the io plan's slice 2): the same phases and ends; the player's sound input
//!   is opened ([`Player::open_input`]) and written as it records ([`take_audio`]). The clip starts
//!   where recording started (not on a bar line) and is as long as the take: one `Clips` Tx, once
//!   the file is durable. Hearing yourself (MON-8) plays the input on the lane from Record to the
//!   take's end, through the track's effects and fader.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use m2_editor::clips::Gesture;
use m2_play::{InputChoice, Player};
use m2_song::{Arrangement, LaneAddr, TempoMap};
use serde_json::{Value, json};

use super::clips::Refused;
use super::lease::PlaysView;
use super::take_audio::{self, Writer, Written};
use super::{ControlPlane, Device, Graph, Inner};

/// What Record says on a lane it can't record on.
pub const NO_LANE: &str = "Select a notes track or an audio track to record on it.";
/// The shortest note a take keeps, in beats.
pub const SHORTEST: f64 = 1.0 / 64.0;
/// A count whose song hasn't started this long after its bar ended was cancelled (D6).
const LATE_COUNT: Duration = Duration::from_millis(300);
/// A count the engine never took: dropped after this.
const NEVER_COUNTED: Duration = Duration::from_secs(10);

/// What a take is asked to do: count in first, and the grid its starts go to (beats). On an audio
/// lane: the input to record (Settings' Sound input), and whether to hear it (Settings' Hear
/// yourself while recording, MON-8).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Options {
    pub count_in: bool,
    pub quantize: Option<f64>,
    pub input: InputChoice,
    pub hear: bool,
}

/// The take that runs, on the control plane, and the takes that ended, for the take thread.
#[derive(Default)]
pub(super) struct Recorder {
    take: Option<Take>,
    ended: Vec<Ended>,
    /// The windows haven't heard the take as it is now.
    changed: bool,
    /// The windows were told the takes ended wait for the code to compile.
    waiting: bool,
}

/// A note kept, in song beats.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Note {
    pitch: f64,
    start: f64,
    length: f64,
    velocity: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    /// Counting in: the count's message, its whole length in engine frames, its beats and how long
    /// it lasts.
    Count { seq: u64, frames: u64, beats: u32, ms: f64 },
    /// Play was sent, stopped at the start.
    Play { seq: u64 },
    /// Recording.
    Take,
}

struct Take {
    token: u64,
    /// The player it records from: another one ends it (D29).
    epoch: u64,
    module: String,
    lane: String,
    /// The bar line the clip starts on, and where recording starts.
    start: f64,
    from: f64,
    /// How long before the anchor a note still lands on `from`: the count's last beat, in engine
    /// frames.
    lead: f64,
    quantize: Option<f64>,
    tempo: TempoMap,
    arrangement: Arrangement,
    rate: f64,
    phase: Phase,
    /// The engine clock where the transport is at `from`, once known.
    anchor: Option<u64>,
    /// Where the first pass ends: the loop's end, when it loops over `from`.
    until: Option<f64>,
    /// The latest beat heard, by a note or the take thread's tick: where the take ends when its
    /// player is gone (the review's R5).
    last: f64,
    notes: Vec<Note>,
    /// Notes sounding, by source and handle: where each started, its pitch and its velocity.
    held: BTreeMap<(u32, u32), (f64, f64, f64)>,
    began: Instant,
    /// On an audio lane: the input's writer.
    audio: Option<AudioTake>,
}

/// An audio take's writer, and the player that hears its input while it records (MON-8).
struct AudioTake {
    writer: Option<Writer>,
    player: Player,
    hearing: bool,
    /// The input's take ring: the meters read its level, and tests settle the writer by it.
    ring: std::sync::Arc<m2_play::ring::Ring>,
}

impl Drop for AudioTake {
    /// The take ended or was dropped: the input isn't heard any more.
    fn drop(&mut self) {
        if self.hearing {
            let _ = self.player.hear(0, 0, false);
        }
    }
}

/// A take that ended, to be written.
pub(super) struct Ended {
    token: u64,
    module: String,
    lane: String,
    /// The clip, in the lane format; None when nothing was played.
    clip: Option<Value>,
    /// An audio take: where it starts, and its file, being written or written.
    audio: Option<Box<(f64, Audio)>>,
    /// Why it ended, when the windows should hear it: the input went away.
    note: Option<&'static str>,
}

/// An audio take's file once the take ended.
pub(super) enum Audio {
    Writing(Writer),
    Written(Written),
}

/// The bar line at or before `beat`.
fn bar_before(a: &Arrangement, beat: f64) -> f64 {
    let (_, into) = a.bar_at(beat.max(0.0));
    let bar = a.meter_at(beat.max(0.0)).bar();
    if bar - into < 1e-9 { beat.max(0.0) - into + bar } else { beat.max(0.0) - into }
}

/// The bar line at or after `beat`.
fn bar_after(a: &Arrangement, beat: f64) -> f64 {
    let line = bar_before(a, beat);
    if beat - line < 1e-9 { line } else { line + a.meter_at(line).bar() }
}

/// `x` on the grid of `grid` beats from the song's start, as a place in a clip that starts at
/// `start`: exact multiples when the clip starts on the grid. Quantize's own rule (`clips.rs`'
/// `on_grid`), so a take quantized as it is written is what Quantize would make of it.
fn on_grid(x: f64, start: f64, grid: f64) -> f64 {
    let line = ((start + x) / grid).round();
    let k = (start / grid).round();
    match (k * grid - start).abs() < 1e-9 {
        true => (line - k) * grid,
        false => line * grid - start,
    }
}

/// The notes of a clip starting at `start`, each start on the nearest line of `grid`, its length
/// kept: as `Gesture::Quantize` with `ends: false` makes them.
pub(super) fn quantized(notes: &mut [(f64, f64)], start: f64, grid: f64) {
    for (s, _) in notes {
        *s = on_grid(*s, start, grid);
    }
}

impl Take {
    /// The beat heard (`Player::heard_frame`: the engine clock and the transport frame), once the
    /// anchor is known. The clock never wraps, but steps by blocks; the transport's frame is exact.
    /// So the frame says, unless it is more than two blocks from where the clock says the transport
    /// is: then it jumped since (a wrap) or never moved (a count), and the clock says.
    fn beat_of(&self, (clock, pos): (u64, u64)) -> Option<f64> {
        let anchor = self.anchor?;
        let since = clock as f64 - anchor as f64;
        let by_pos = pos as f64 - self.tempo.seconds_at(self.from) * self.rate;
        let frames = if (by_pos - since).abs() <= 2.0 * m2_runtime::BLOCK as f64 { by_pos } else { since };
        Some(self.tempo.beat_at(self.tempo.seconds_at(self.from) + frames / self.rate))
    }

    /// Find the anchor: the block the count or the Play applied at; else, once the song plays,
    /// from where it is heard (the stamp was overtaken by later ones).
    fn resolve(&mut self, player: &Player) {
        self.find_anchor(player);
        // An audio take's writer starts from the frame heard there.
        if let (Some(a), Some(w)) = (self.anchor, self.audio.as_ref().and_then(|t| t.writer.as_ref())) {
            w.anchor(a);
        }
    }

    fn find_anchor(&mut self, player: &Player) {
        if self.anchor.is_some() {
            return;
        }
        self.anchor = match self.phase {
            Phase::Count { seq, frames, .. } => player.applied(seq).map(|a| a.clock + frames),
            Phase::Play { seq } => player.applied(seq).map(|a| a.clock),
            Phase::Take => None,
        };
        if self.anchor.is_none()
            && player.is_playing()
            && let Some((clock, pos)) = player.heard_frame()
        {
            let at = m2_runtime::transport::frame_of(&self.tempo, self.from, self.rate);
            self.anchor = Some(clock.saturating_sub(pos.saturating_sub(at)));
        }
    }

    /// Still counting in at engine clock `clock`.
    fn counting(&self, clock: Option<u64>) -> bool {
        matches!(self.phase, Phase::Count { .. }) && !matches!((self.anchor, clock), (Some(a), Some(c)) if c >= a)
    }

    /// A note's start heard at engine clock `clock`, beat `beat`: on the take's start if it came in
    /// the count's last beat, by the clock; None when it came before that.
    fn admit(&self, clock: u64, beat: f64) -> Option<f64> {
        if beat >= self.from {
            return Some(beat);
        }
        let early = self.anchor.map_or(f64::INFINITY, |a| a as f64 - clock as f64);
        (early <= self.lead + 1.0).then_some(self.from)
    }

    /// `beat` was heard: the latest so far, unless it is earlier.
    fn heard(&mut self, beat: f64) {
        self.last = self.last.max(beat);
    }

    fn on(&mut self, key: (u32, u32), pitch: f64, velocity: f64, (clock, beat): (u64, f64)) -> bool {
        self.heard(beat);
        let Some(start) = self.admit(clock, beat) else { return false };
        self.off(key, beat);
        self.held.insert(key, (start, pitch, velocity));
        true
    }

    fn off(&mut self, key: (u32, u32), beat: f64) -> bool {
        self.heard(beat);
        let Some((start, pitch, velocity)) = self.held.remove(&key) else { return false };
        let end = self.until.map_or(beat, |u| beat.min(u));
        self.notes.push(Note { pitch, start, length: (end - start).max(SHORTEST), velocity });
        true
    }

    /// The take, ended at `beat`: held notes end there, and the clip spans whole bars, at least
    /// one, to the bar line after its last note's end (a note the grid moved later too: the
    /// review's R6).
    fn end(mut self, beat: f64) -> Ended {
        let beat = self.until.map_or(beat, |u| beat.min(u)).max(self.from);
        if let Some(mut a) = self.audio.take() {
            // As long as it played, from where it started: whole frames.
            let frames = ((self.tempo.seconds_at(beat) - self.tempo.seconds_at(self.from)) * self.rate).round().max(0.0) as u64;
            let writer = a.writer.take().expect("an audio take's writer");
            writer.end(frames);
            let audio = Some(Box::new((self.from, Audio::Writing(writer))));
            return Ended { token: self.token, module: self.module, lane: self.lane, clip: None, audio, note: None };
        }
        for key in self.held.keys().copied().collect::<Vec<_>>() {
            self.off(key, beat);
        }
        let clip = (!self.notes.is_empty()).then(|| {
            let mut notes = self.notes.clone();
            notes.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.total_cmp(&b.pitch)));
            let mut placed: Vec<(f64, f64)> = notes.iter().map(|n| (n.start - self.start, n.length)).collect();
            if let Some(grid) = self.quantize {
                quantized(&mut placed, self.start, grid);
            }
            let last = placed.iter().map(|(s, l)| self.start + s + l.max(SHORTEST)).fold(beat, f64::max);
            let end = bar_after(&self.arrangement, last).max(self.start + self.arrangement.meter_at(self.start).bar());
            let notes: Vec<Value> = notes
                .iter()
                .zip(&placed)
                .map(|(n, (s, l))| json!({"pitch": n.pitch, "start": s, "length": l, "velocity": n.velocity}))
                .collect();
            json!({"start": self.start, "length": end - self.start, "notes": notes})
        });
        Ended { token: self.token, module: self.module, lane: self.lane, clip, audio: None, note: None }
    }

    /// What the windows see: its phase, lane, start, and its notes so far (a held one to `now`).
    fn shown(&self, now: Option<f64>) -> Value {
        let mut notes: Vec<Value> =
            self.notes.iter().map(|n| json!({"pitch": n.pitch, "start": n.start, "length": n.length, "velocity": n.velocity})).collect();
        for (start, pitch, velocity) in self.held.values() {
            let length = now.map_or(SHORTEST, |n| (n - start).max(SHORTEST));
            notes.push(json!({"pitch": pitch, "start": start, "length": length, "velocity": velocity, "held": true}));
        }
        let counting = matches!(self.phase, Phase::Count { .. }) && now.is_none_or(|n| n < self.from);
        let count = match self.phase {
            Phase::Count { beats, ms, .. } => json!({"beats": beats, "ms": ms}),
            _ => Value::Null,
        };
        // An audio take says so, and whether its input is heard (the io plan's slice 3).
        json!({
            "phase": if counting { "count" } else { "take" },
            "module": self.module, "lane": self.lane,
            "kind": if self.audio.is_some() { "audio" } else { "notes" },
            "hear": self.audio.as_ref().is_some_and(|a| a.hearing),
            "start": self.start, "from": self.from, "until": self.until,
            "count_in": count, "notes": notes,
        })
    }
}

impl Recorder {
    /// The sound input's take ring while a take records on an audio lane: what the meters read
    /// its level from.
    pub(super) fn input(&self) -> Option<&std::sync::Arc<m2_play::ring::Ring>> {
        self.take.as_ref().and_then(|t| t.audio.as_ref()).map(|a| &a.ring)
    }

    /// The take's lane while it records song `token`: what MIDI keyboards play then (D8).
    pub(super) fn lane(&self, token: Option<u64>) -> Option<(String, String)> {
        self.take.as_ref().filter(|t| Some(t.token) == token).map(|t| (t.module.clone(), t.lane.clone()))
    }

    /// The take's notes heard: called by audition for each On it sends from a source that records
    /// (`source`'s id), on `device`.
    pub(super) fn on(&mut self, device: &Device, lane: (&str, &str), key: (u32, u32), pitch: f64, velocity: f64) {
        let Some(t) = self.take.as_mut().filter(|t| t.epoch == device.epoch && (t.module.as_str(), t.lane.as_str()) == lane) else {
            return;
        };
        t.resolve(&device.player);
        let Some((heard, beat)) = device.player.heard_frame().and_then(|h| Some((h, t.beat_of(h)?))) else { return };
        if t.until.is_some_and(|u| beat >= u) {
            self.end_at(beat);
            return;
        }
        self.changed |= t.on(key, pitch, velocity, (heard.0, beat));
    }

    /// A note let go, as audition sends its Off.
    pub(super) fn off(&mut self, device: &Device, key: (u32, u32)) {
        let Some(t) = self.take.as_mut().filter(|t| t.epoch == device.epoch && t.held.contains_key(&key)) else { return };
        t.resolve(&device.player);
        let Some(beat) = device.player.heard_frame().and_then(|h| t.beat_of(h)) else { return };
        self.changed |= t.off(key, beat);
    }

    /// Every note source `id` holds, let go.
    pub(super) fn release(&mut self, device: Option<&Device>, id: u32) {
        let keys: Vec<(u32, u32)> = self.take.as_ref().map(|t| t.held.keys().filter(|k| k.0 == id).copied().collect()).unwrap_or_default();
        if let Some(d) = device {
            keys.into_iter().for_each(|k| self.off(d, k));
        }
    }

    /// End the take at `beat` (it waits to be written), or drop it while it counts in.
    fn end_at(&mut self, beat: f64) {
        if let Some(t) = self.take.take() {
            self.ended.push(t.end(beat));
            self.changed = true;
        }
    }

    /// End the take where `device` is heard now: Stop, R, a jump. Counting in, it is dropped.
    fn end_now(&mut self, device: Option<&Device>) {
        let Some(t) = self.take.as_mut() else { return };
        let heard = device.filter(|d| d.epoch == t.epoch).and_then(|d| {
            t.resolve(&d.player);
            d.player.heard_frame()
        });
        if t.counting(heard.map(|(c, _)| c)) {
            self.drop_take();
            return;
        }
        let beat = heard.and_then(|h| t.beat_of(h)).unwrap_or(t.last);
        self.end_at(beat);
    }

    fn drop_take(&mut self) {
        self.changed |= self.take.take().is_some();
    }
}

/// The track's name as the timeline shows it, for the undo step.
pub(super) fn label_of(cp: &ControlPlane, module: &str) -> String {
    cp.open
        .as_ref()
        .and_then(|p| p.shown.timeline.tracks.iter().find(|t| t.module == module).and_then(|t| t.head.get("label")?.as_str().map(String::from)))
        .unwrap_or_else(|| module.to_string())
}

/// Lane `(module, lane)` is a notes lane in what plays.
fn plays_notes(plays: &PlaysView, module: &str, lane: &str) -> bool {
    plays.info.lanes.iter().any(|l| l.lane.module == module && l.lane.lane == lane && l.caps.is_some())
}

/// Lane `(module, lane)`'s slot in what plays, if it is an audio lane there.
fn audio_slot(plays: &PlaysView, module: &str, lane: &str) -> Option<u32> {
    plays.info.lanes.iter().position(|l| l.lane.module == module && l.lane.lane == lane && l.audio().is_some()).map(|s| s as u32)
}

impl Graph {
    /// Start a take on lane `(module, lane)` of song `token` (Q19 to Q21): stopped, after a bar of
    /// clicks or at once, from where play starts; playing, from now. Answers the take as the
    /// windows see it. Never waits for the session.
    pub fn record_start(&self, token: u64, module: &str, lane: &str, opts: Options) -> Result<Value> {
        if let Some(g) = opts.quantize
            && !(g.is_finite() && g > 0.0)
        {
            bail!("The grid is a number of beats above 0, such as 0.25 for sixteenths.");
        }
        let device = self.inner.device()?;
        // An audio lane records from the sound input: it is opened, and the take's file made and
        // its writer started, first, never under the control plane's lock (D14, the review's
        // io-r1). A take refused below lets them go, and its part file with them.
        let audio = {
            let cp = self.inner.control.lock();
            let p = cp.open.as_ref();
            let plays = p.and_then(|p| p.engine.plays.as_ref()).filter(|pl| pl.epoch == device.epoch);
            match (p, plays.is_some_and(|pl| audio_slot(pl, module, lane).is_some())) {
                (Some(p), true) => Some((p.shown.dir.clone(), label_of(&cp, module))),
                _ => None,
            }
        };
        let (listen, writer) = match audio {
            Some((dir, label)) => {
                let listen = device.player.open_input(&opts.input)?;
                let file = m2_editor::take::Growing::new(&dir, &label, listen.channels, listen.rate).map_err(|e| anyhow::anyhow!("{e}"))?;
                let writer = Writer::start(listen.ring.clone(), file, format!("{label} take.wav"), listen.channels)?;
                (Some(listen), Some(writer))
            }
            None => (None, None),
        };
        let looped = {
            let timing = self.inner.timing.lock();
            let t = timing.as_ref().context("no project is open")?;
            t.range.filter(|_| t.on && !device.player.loop_refused())
        };
        let mut cp = self.inner.control.lock();
        let p = cp.open.as_ref().context("no project is open")?;
        if p.engine.token != token {
            bail!("the open project changed; refresh the graph before editing");
        }
        if cp.record.take.is_some() {
            bail!("A take is recording already.");
        }
        // A take is written as an edit, which needs the code to compile (the review's R1).
        if p.shown.state["view"]["draft_errors"].as_array().is_some_and(|d| !d.is_empty()) {
            bail!("Fix the code first: it doesn't compile right now.");
        }
        let plays = p.engine.plays.as_ref().filter(|pl| pl.epoch == device.epoch);
        let slot = plays.and_then(|pl| audio_slot(pl, module, lane));
        if !plays.is_some_and(|pl| plays_notes(pl, module, lane)) && (slot.is_none() || listen.is_none()) {
            bail!("{NO_LANE}");
        }
        let generation = plays.map_or(0, |pl| pl.generation);
        let arrangement = p.engine.accepted.project.arrangement.clone();
        let tempo = plays.and_then(|pl| pl.tempo.as_deref().cloned()).unwrap_or_else(|| arrangement.tempo_map());
        let player = &device.player;
        let rate = player.rate() as f64;
        let st = player.status();
        let mut take = Take {
            token,
            epoch: device.epoch,
            module: module.into(),
            lane: lane.into(),
            start: 0.0,
            from: 0.0,
            lead: 0.0,
            quantize: opts.quantize,
            tempo,
            arrangement,
            rate,
            phase: Phase::Take,
            anchor: None,
            until: None,
            last: 0.0,
            notes: vec![],
            held: BTreeMap::new(),
            began: Instant::now(),
            audio: None,
        };
        if st.playing && st.pending.is_some() {
            // A jump the audio thread hasn't taken yet: where it is heard is about to change.
            bail!("The song is moving to another place. Press Record again.");
        }
        // An audio take's writer, and hearing yourself from now, the count included.
        if let (Some(listen), Some(slot), Some(writer)) = (listen, slot, writer) {
            let hearing = opts.hear && player.hear(generation, slot, true).is_ok();
            take.audio = Some(AudioTake { writer: Some(writer), player: player.clone(), hearing, ring: listen.ring });
        }
        if st.playing {
            // From now, with no count.
            let (clock, pos) = player.heard_frame().context("the sound output hasn't started yet")?;
            take.from = take.tempo.beat_at(pos as f64 / rate);
            take.anchor = Some(clock);
        } else {
            take.from = st.pending.unwrap_or(st.beats).max(0.0);
            if opts.count_in {
                let meter = take.arrangement.meter_at(take.from);
                let beat = 4.0 / meter.unit as f64;
                let frames_per_beat = beat * 60.0 / take.tempo.bpm_at(take.from) * rate;
                let seq = player.count_in(meter.beats, frames_per_beat)?;
                let bar = (meter.beats as f64 * frames_per_beat).round().max(1.0) as u64;
                let frames = bar.div_ceil(m2_runtime::BLOCK as u64) * m2_runtime::BLOCK as u64;
                take.phase = Phase::Count { seq, frames, beats: meter.beats, ms: bar as f64 / rate * 1000.0 };
                take.lead = frames_per_beat;
            } else {
                take.phase = Phase::Play { seq: player.transport(m2_runtime::Cmd::Play)? };
            }
        }
        take.start = bar_before(&take.arrangement, take.from);
        take.last = take.from;
        take.until = looped.filter(|(_, end)| take.from < *end).map(|(_, end)| end);
        if take.audio.is_some() {
            // The clip starts where recording starts (D19).
            take.start = take.from;
            take.resolve(player);
        }
        let shown = take.shown(None);
        cp.record.take = Some(take);
        cp.record.changed = true;
        Ok(shown)
    }

    /// What the silent input hears next (D24): `samples`, then a sine `tone` (pitch, seconds, gain),
    /// mono at the song's rate. Says so when no take has opened the input.
    pub fn inject_input(&self, samples: &[f32], tone: Option<(f32, f32, f32)>) -> Result<()> {
        const NONE: &str = "Nothing is recording from the sound input.";
        let device = self.inner.current_device().context(NONE)?;
        let player = &device.player;
        if player.input().is_none() {
            bail!(NONE);
        }
        let mut all = samples.to_vec();
        if let Some((hz, seconds, gain)) = tone {
            let rate = player.rate() as f32;
            let n = (seconds.clamp(0.0, 60.0) * rate) as usize;
            all.extend((0..n).map(|i| gain * (std::f32::consts::TAU * hz * i as f32 / rate).sin()));
        }
        player.inject_input(&all)
    }

    /// R while recording (Q19): the take ends where it is heard, and play goes on. Answers at once:
    /// the take thread writes it.
    pub fn record_stop(&self, token: u64) -> Result<Value> {
        let device = self.inner.current_device();
        let mut cp = self.inner.control.lock();
        if cp.record.take.as_ref().is_none_or(|t| t.token != token) {
            bail!("Nothing is recording.");
        }
        cp.record.end_now(device.as_ref());
        Ok(json!({"ended": true}))
    }

    /// R or Esc while counting in: nothing plays, nothing is kept. While recording: the take is
    /// dropped, and play goes on.
    pub fn record_cancel(&self, token: u64) -> Result<Value> {
        let counting = {
            let mut cp = self.inner.control.lock();
            let Some(t) = cp.record.take.as_ref().filter(|t| t.token == token) else { return Ok(json!({"cancelled": false})) };
            // Counting as the windows were last told, though its bar may have just ended (the
            // review's R7).
            let counting = matches!(t.phase, Phase::Count { .. });
            cp.record.drop_take();
            counting
        };
        // The engine's count ends with a Stop; so does a count that just turned into playing.
        if counting {
            self.transport("stop")?;
        }
        Ok(json!({"cancelled": true}))
    }

    /// Whether a take runs or waits to be written (Export waits for it, Q30).
    pub fn recording(&self) -> bool {
        let cp = self.inner.control.lock();
        cp.record.take.is_some() || !cp.record.ended.is_empty()
    }

    /// A transport action, as a take sees it (B's line in [`Graph::transport`]): while counting
    /// in, anything cancels the count, and a toggle (Space) is a Stop, so nothing plays (D6); while
    /// recording, a Stop or a toggle ends the take where it is heard. Answers the action to take in
    /// its place.
    pub(super) fn record_transport(&self, action: &str) -> Option<&'static str> {
        let device = self.inner.current_device();
        let mut cp = self.inner.control.lock();
        let t = cp.record.take.as_mut()?;
        let clock = device.as_ref().filter(|d| d.epoch == t.epoch).and_then(|d| {
            t.resolve(&d.player);
            d.player.heard_frame().map(|(c, _)| c)
        });
        if t.counting(clock) {
            cp.record.drop_take();
            return (action == "toggle").then_some("stop");
        }
        if matches!(action, "stop" | "toggle") {
            cp.record.end_now(device.as_ref());
        }
        None
    }

    /// The loop changed (B's line in [`Graph::set_loop`], the review's R2): the take's first pass
    /// ends at the new loop's end, while the song is still before it (a song past it plays on); with
    /// no loop, it goes on.
    pub(super) fn record_loop(&self, looped: Option<(f64, f64)>) {
        let device = self.inner.current_device();
        let mut cp = self.inner.control.lock();
        let Some(t) = cp.record.take.as_mut() else { return };
        let now = device.filter(|d| d.epoch == t.epoch).map_or(t.from, |d| d.player.status().beats);
        let until = looped.filter(|&(_, end)| t.from < end && now < end).map(|(_, end)| end);
        if until != t.until {
            t.until = until;
            cp.record.changed = true;
        }
    }

    /// A jump (B's line in [`Graph::seek`]): it ends the take where it is heard (D5).
    pub(super) fn record_jump(&self) {
        let device = self.inner.current_device();
        let mut cp = self.inner.control.lock();
        if cp.record.take.is_some() {
            cp.record.end_now(device.as_ref());
        }
    }

    /// The take thread's tick, every 100 ms (D7, D9): a count that turned into playing, or never
    /// did; a take past its loop's end, or whose player or song went; then each take that ended,
    /// written. Answers what the windows hear, in order: the take as it is when it changed, then
    /// each outcome.
    pub fn record_tick(&self) -> Vec<Value> {
        let mut said = vec![];
        let mut unplugged = None;
        let (ended, labels) = {
            let device = self.inner.current_device();
            let mut cp = self.inner.control.lock();
            let token = cp.open.as_ref().map(|p| p.engine.token);
            let r = &mut cp.record;
            if let Some(t) = r.take.as_mut() {
                let same = device.as_ref().filter(|d| d.epoch == t.epoch);
                if Some(t.token) != token {
                    // The song closed or another opened: nothing is kept.
                    r.drop_take();
                } else if same.is_none() {
                    // A new player (a crash, another output): its clock starts again (D29). The take
                    // ends where it was last heard.
                    let beat = t.last;
                    r.end_at(beat);
                } else if let Some(d) = same.filter(|d| t.audio.is_some() && d.player.input_gone()) {
                    // The input went away (a microphone unplugged): the take ends where it is heard,
                    // with what it has, and the song plays on (the review's io-r2).
                    t.resolve(&d.player);
                    let beat = d.player.heard_frame().and_then(|h| t.beat_of(h)).unwrap_or(t.last);
                    r.end_at(beat);
                    if let Some(e) = r.ended.last_mut() {
                        e.note = Some(take_audio::UNPLUGGED);
                    }
                    unplugged = Some(d.player.clone());
                } else if let Some(d) = same {
                    t.resolve(&d.player);
                    let heard = d.player.heard_frame();
                    let clock = heard.map(|(c, _)| c);
                    let beat = heard.and_then(|h| t.beat_of(h));
                    if let Some(b) = beat {
                        t.heard(b);
                    }
                    let counting = t.counting(clock);
                    let playing = d.player.is_playing();
                    match t.phase {
                        // The bar is over and the song plays: recording.
                        Phase::Count { .. } if !counting && playing => {
                            t.phase = Phase::Take;
                            r.changed = true;
                        }
                        // The bar ended a while ago and nothing plays, or the count never started:
                        // it was cancelled (D6).
                        Phase::Count { .. } => {
                            let late = match (t.anchor, clock) {
                                (Some(a), Some(c)) => c.saturating_sub(a) as f64 / t.rate > LATE_COUNT.as_secs_f64(),
                                _ => t.began.elapsed() > NEVER_COUNTED,
                            };
                            if late && !playing {
                                r.drop_take();
                            }
                        }
                        _ if t.until.zip(beat).is_some_and(|(u, b)| b >= u) => {
                            let at = t.until.unwrap_or(t.from);
                            r.end_at(at);
                        }
                        // Stopped some other way: the take ends there.
                        _ if !playing && t.anchor.is_some() => {
                            let at = beat.unwrap_or(t.last);
                            r.end_at(at);
                        }
                        _ => {}
                    }
                }
            }
            if std::mem::take(&mut r.changed) {
                let now = device.as_ref().and_then(|d| {
                    let t = r.take.as_ref()?;
                    d.player.heard_frame().and_then(|h| t.beat_of(h))
                });
                said.push(match r.take.as_ref() {
                    Some(t) => {
                        let mut v = t.shown(now);
                        v["t"] = json!("take");
                        v
                    }
                    None => json!({"t": "take", "phase": null}),
                });
            }
            // While the code doesn't compile, the takes that ended wait for it (the review's R1).
            let compiles = cp.open.as_ref().is_none_or(|p| p.shown.state["view"]["draft_errors"].as_array().is_none_or(Vec::is_empty));
            let ended = match compiles {
                true => std::mem::take(&mut cp.record.ended),
                false => vec![],
            };
            cp.record.waiting &= !compiles;
            if let Some(said_now) = waits(&mut cp) {
                said.push(said_now);
            }
            let labels: Vec<String> = ended.iter().map(|e| label_of(&cp, &e.module)).collect();
            (ended, labels)
        };
        // Its stream stops; the next R opens it again.
        if let Some(p) = unplugged {
            p.close_input();
        }
        for (e, label) in ended.into_iter().zip(labels) {
            match self.write_take(e, &label) {
                Ok(v) => said.push(v),
                // The code stopped compiling as it was written: it waits.
                Err(e) => {
                    let mut cp = self.inner.control.lock();
                    cp.record.ended.push(e);
                    if let Some(said_now) = waits(&mut cp) {
                        said.push(said_now);
                    }
                }
            }
        }
        said
    }

    /// Write a take that ended: one `Clips` Tx, its undo step named "recording on Bass". Answers what
    /// the windows hear, or the take again when the code doesn't compile, to wait for it.
    fn write_take(&self, mut e: Ended, label: &str) -> Result<Value, Ended> {
        if let Some(a) = e.audio.take() {
            let (from, audio) = *a;
            return self.write_audio(e, from, audio, label);
        }
        let Some(clip) = e.clip.clone() else { return Ok(json!({"t": "take", "empty": true})) };
        let lost = |why: &str| json!({"t": "take", "error": format!("The recording couldn't be kept: {why}")});
        let open = self.inner.control.lock().open.as_ref().map(|p| p.engine.token);
        if open != Some(e.token) {
            return Ok(lost("its song was closed."));
        }
        let Ok(lane) = LaneAddr::new(&e.module, &e.lane) else { return Ok(lost(&format!("{label} is gone."))) };
        match self.clips_why(e.token, Gesture::Set { lane, put: vec![clip], delete: vec![] }, None) {
            (Ok(v), _) => {
                let revision = v["revision"].as_u64().unwrap_or(0);
                if let Err(err) = self.name_step(e.token, revision, format!("recording on {label}"), false) {
                    eprintln!("music2: couldn't name the recording's undo step: {err:#}");
                }
                Ok(json!({"t": "take", "written": {"revision": revision, "made": v["made"], "module": e.module, "lane": e.lane}}))
            }
            (Err(_), Some(Refused::Draft)) => Err(e),
            (Err(_), Some(Refused::Missing)) => Ok(lost(&format!("{label} is gone."))),
            (Err(_), Some(Refused::ReadOnly)) => Ok(lost("this song is open for writing somewhere else.")),
            (Err(err), _) => Ok(lost(&format!("{err:#}"))),
        }
    }
}

impl Graph {
    /// Write an audio take that ended: its file finished and durable first, then one `Clips` Tx
    /// naming it, its undo step "recording on Vox" (WRITE-1). The file stays in `assets/` whatever
    /// becomes of the Tx (music2 never deletes an asset).
    fn write_audio(&self, e: Ended, from: f64, audio: Audio, label: &str) -> Result<Value, Ended> {
        let lost = |why: &str| json!({"t": "take", "error": format!("The recording couldn't be kept: {why}")});
        let written = match audio {
            Audio::Written(w) => w,
            Audio::Writing(w) => match w.done() {
                Ok(Some(w)) => w,
                Ok(None) => return Ok(json!({"t": "take", "empty": true})),
                Err(err) => return Ok(lost(&format!("{err:#}"))),
            },
        };
        let open = self.inner.control.lock().open.as_ref().map(|p| p.engine.token);
        if open != Some(e.token) {
            return Ok(lost("its song was closed."));
        }
        match self.place_take(e.token, &written.0, &e.module, &e.lane, from) {
            (Ok(v), _) => {
                let revision = v["revision"].as_u64().unwrap_or(0);
                if let Err(err) = self.name_step(e.token, revision, format!("recording on {label}"), false) {
                    eprintln!("music2: couldn't name the recording's undo step: {err:#}");
                }
                let mut w = json!({"revision": revision, "made": v["made"], "module": e.module, "lane": e.lane, "asset": written.0.asset.as_str()});
                let notes: Vec<&str> = e.note.into_iter().chain((written.1 > 0).then_some(take_audio::TOO_SLOW)).collect();
                if !notes.is_empty() {
                    w["note"] = json!(notes.join(" "));
                }
                Ok(json!({"t": "take", "written": w}))
            }
            (Err(_), Some(Refused::Draft)) => Err(Ended { audio: Some(Box::new((from, Audio::Written(written)))), ..e }),
            (Err(_), Some(Refused::Missing)) => Ok(lost(&format!("{label} is gone."))),
            (Err(_), Some(Refused::ReadOnly)) => Ok(lost("this song is open for writing somewhere else.")),
            (Err(err), _) => Ok(lost(&format!("{err:#}"))),
        }
    }

    /// An audio take's writer moves nothing, or moves again (tests).
    #[cfg(test)]
    pub(super) fn record_pause(&self, on: bool) {
        let cp = self.inner.control.lock();
        if let Some(w) = cp.record.take.as_ref().and_then(|t| t.audio.as_ref()).and_then(|a| a.writer.as_ref()) {
            w.pause(on);
        }
    }

    /// Wait until an audio take's writer has moved everything its ring holds (tests).
    #[cfg(test)]
    pub(super) fn record_settle(&self) {
        let cp = self.inner.control.lock();
        if let Some(a) = cp.record.take.as_ref().and_then(|t| t.audio.as_ref())
            && let Some(w) = &a.writer
        {
            w.settle(&a.ring);
        }
    }
}

/// What the windows hear when takes wait for the code to compile, once: "The recording waits for the
/// code. Fix the code, and it goes on Bass."
fn waits(cp: &mut ControlPlane) -> Option<Value> {
    let first = cp.record.ended.first()?;
    if cp.record.waiting {
        return None;
    }
    let label = label_of(cp, &first.module);
    cp.record.waiting = true;
    Some(json!({"t": "take", "waiting": format!("The recording waits for the code. Fix the code, and it goes on {label}.")}))
}

impl Inner {
    /// The state's `record`: the take as the windows see it, or null.
    pub(super) fn record_state(&self) -> Value {
        let device = self.current_device();
        let mut cp = self.control.lock();
        let token = cp.open.as_ref().map(|p| p.engine.token);
        match cp.record.take.as_mut().filter(|t| Some(t.token) == token) {
            Some(t) => {
                let now = device.as_ref().filter(|d| d.epoch == t.epoch).and_then(|d| {
                    t.resolve(&d.player);
                    d.player.heard_frame().and_then(|h| t.beat_of(h))
                });
                t.shown(now)
            }
            None => Value::Null,
        }
    }

}

#[cfg(test)]
#[path = "record_tests.rs"]
mod tests;
