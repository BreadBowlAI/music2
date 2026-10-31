//! Recording what you play (lane B, gaps #24 and #25): a take on the armed tracks, or the selected
//! one, each lane's written as its clip.
//!
//! - **Arm** ([`Graph::arm`], MON-9): a track's R. Armed tracks are what Record records on; with
//!   none, the lane a window names (the selected track's). One notes track is armed at a time (the
//!   keyboards play it); audio tracks, any number, each from its own channels of the input
//!   (MON-13). Arms live on the control plane while the song is open: no file, no undo step.
//! - **Hearing yourself** (MON-8): every audio lane a take records on, and every armed audio track,
//!   hears its channels while Settings says to. What each lane hears is told to the engine as it
//!   changes ([`send_hearing`]): at an arm, a take's start and end, and on the take thread's tick,
//!   so a new plan's lanes hear too.
//! - **Start** ([`Graph::record_start`]): stopped, the take starts where play starts: after one bar
//!   of clicks at the tempo and meter there (`Player::count_in`), or at once. Playing, it starts
//!   now, with no count. With Punch on, or the loop on, recording starts at the loop's start, the
//!   song playing up to it first (MON-10, MON-11). Only the control plane is read, never the
//!   session: R is transport, and never waits for a compile (RENDER-29).
//! - **Where it is heard**: the take's anchor is the engine clock where the transport is at the
//!   take's start. Each note, and the take's end, is placed by the clock, which never wraps, and by
//!   the transport's frame, which is exact: in a pass of the loop and a frame of it ([`Take::place`]).
//! - **Notes:** each On and Off audition sends on the take's notes lane, from a MIDI keyboard or a
//!   window that marks its notes `record` (the keyboard piano), goes where it was heard
//!   (`Player::heard_frame`, D4). A note at least 1/64 beat long. A note struck in the count's last
//!   beat lands on the take's first beat; earlier ones are heard, not kept (D27).
//! - **Passes** (MON-11): over the loop, each time round is a pass. A notes pass is written as it
//!   ends, as its own clip (or into the clip under it, with Overdub, MON-12), so the next pass
//!   plays it. An audio lane writes one file of every pass; at the take's end its clip covers the
//!   loop, playing the newest whole pass, and the file's note lists them all.
//! - **Punch** (MON-10): the take covers only the loop's range, and replaces what was there.
//! - **Ending never waits** (D7): held notes end and the end is fixed under the control plane's
//!   lock, and the take waits in a queue. The take thread ([`Graph::record_tick`]) writes it: one Tx
//!   for every lane's clip, one undo step, "recording on Bass" (MON-14).
//! - **The code must compile** (the review's R1): R is refused while it doesn't, as every edit is. A
//!   take that ends while it doesn't waits in the queue, said once, and is written once it does.
//! - **The windows hear** `{"t": "take", …}`: the take's phase and notes when they change, then
//!   `written`, `empty`, or `error`.
//! - **The input's problems** (the ex plan's slice 3) go through the fault ring (RENDER-25), named
//!   by the input's [`InputNamer`]: an input chosen that isn't there (the player reports it as it
//!   opens), an input gone during a take, and the OS refusing the microphone during one, which the
//!   take thread asks at each tick ([`Player::input_refused`]): that take is only silence, so it
//!   isn't kept.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use m2_editor::clips::{How, Taken};
use m2_play::{Channels, InputChoice, Listen, Player};
use m2_runtime::fault::{Code, Fault, Subject};
use m2_song::{Arrangement, LaneAddr, TempoMap};
use serde_json::{Value, json};

use super::clips::Refused;
use super::faults::InputNamer;
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

/// What a take is asked to do: count in first, and the grid its starts go to (beats). On audio
/// lanes: the input to record (Settings' Sound input), and whether to hear it (Settings' Hear
/// yourself while recording, MON-8). Punch: only between the loop's edges, replacing what was there
/// (MON-10). Overdub: a notes take into the clip under it (MON-12).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Options {
    pub count_in: bool,
    pub quantize: Option<f64>,
    pub input: InputChoice,
    pub hear: bool,
    pub punch: bool,
    pub overdub: bool,
}

/// The take that runs, on the control plane, the takes that ended, for the take thread, and the
/// arms.
#[derive(Default)]
pub(super) struct Recorder {
    take: Option<Take>,
    ended: Vec<Ended>,
    /// The windows haven't heard the take as it is now.
    changed: bool,
    /// The windows were told the takes ended wait for the code to compile.
    waiting: bool,
    arms: Arms,
    /// What the engine was last told each lane hears (MON-8).
    heard: Heard,
}

/// The armed tracks of the song open, each audio track's channels, and the input (MON-9, MON-13).
#[derive(Default)]
struct Arms {
    token: u64,
    /// Each armed track's lane, and whether it is an audio lane.
    armed: BTreeMap<String, (String, bool)>,
    /// Each audio track's channels, once chosen.
    channels: BTreeMap<String, Channels>,
    /// The input Settings chose, and whether you hear yourself, as last said.
    input: InputChoice,
    hear: bool,
    /// The input as last opened here, and the player it was opened in.
    listen: Option<(u64, Listen)>,
}

/// What the engine was last told each audio lane hears: in which player and plan, from which
/// opening of the input, by slot.
#[derive(Clone, Default, PartialEq)]
struct Heard {
    epoch: u64,
    generation: u64,
    open: u32,
    lanes: BTreeMap<u32, Channels>,
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
    /// Recording, or playing up to where it records.
    Take,
}

/// How far a take goes.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Span {
    /// Until it stops.
    Open,
    /// To `end` (beats), whose transport frame is `e`: a punch with the loop off (MON-10).
    To { end: f64, e: u64 },
    /// Round the loop `[start, end)` (beats), whose frames are `s` and `e` (MON-11).
    Loop { start: f64, end: f64, s: u64, e: u64 },
}

/// Where in a take a moment is: its pass (0 is the first), and the transport's frame there.
type Place = (u32, f64);

struct Take {
    token: u64,
    /// The player it records from: another one ends it (D29).
    epoch: u64,
    /// Where recording starts, in beats, and the transport's frame there.
    from: f64,
    frame: u64,
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
    /// Frames the song plays before `from`: a punch's way in, or a loop's (MON-10, MON-11).
    skip: u64,
    span: Span,
    punch: bool,
    overdub: bool,
    /// The latest place heard, by a note or the take thread's tick: where the take ends when its
    /// player is gone (the review's R5).
    last: Place,
    /// The pass recording now.
    pass: u32,
    /// Where the notes clip of this pass starts: a bar line, or `from` itself for a punch.
    start: f64,
    notes: Option<NotesPart>,
    audio: Vec<AudioPart>,
    began: Instant,
}

/// The take's notes lane: its notes kept this pass, and those sounding, by source and handle:
/// where each started, its pitch and its velocity.
struct NotesPart {
    module: String,
    lane: String,
    notes: Vec<Note>,
    held: BTreeMap<(u32, u32), (f64, f64, f64)>,
}

/// An audio lane's take: its channels, their ring, and its writer.
struct AudioPart {
    module: String,
    lane: String,
    channels: Channels,
    writer: Option<Writer>,
    /// The ring of its channels: the meters read its level, and tests settle the writer by it.
    ring: Arc<m2_play::ring::Ring>,
    /// The input's opening: what a fault of it is about.
    input: u32,
    /// Frames of silence its file starts with: from the loop's start to the take's (MON-11).
    lead: u64,
}

/// A take that ended, or a pass of one, to be written: each lane's part.
pub(super) struct Ended {
    token: u64,
    tempo: TempoMap,
    parts: Vec<Part>,
}

/// One lane's part of a take that ended.
enum Part {
    /// A notes clip, in the lane format.
    Notes { module: String, lane: String, clip: Value, how: How },
    /// An audio take: its file, being written or written, and where it goes.
    Audio { module: String, lane: String, how: How, at: Placing, file: Audio },
}

/// Where an audio take's clip goes: its start (beats), where it starts in the file (seconds), how
/// long it is (beats; None: to the file's end), and the file's takes over a loop (seconds, MON-11).
#[derive(Clone, Debug)]
struct Placing {
    start: f64,
    offset: f64,
    length: Option<f64>,
    takes: Vec<f64>,
}

/// An audio take's file once the take ended.
pub(super) enum Audio {
    Writing(Writer),
    Written(Written),
    /// No frame was written: nothing was played.
    Empty,
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
    /// The beat at transport frame `frame`.
    fn beat(&self, frame: f64) -> f64 {
        self.tempo.beat_at(frame / self.rate)
    }

    /// Where recording starts in pass `pass`: `from` in the first, the loop's start after.
    fn pass_from(&self, pass: u32) -> f64 {
        match self.span {
            Span::Loop { start, .. } if pass > 0 => start,
            _ => self.from,
        }
    }

    /// Where the take or its pass ends at the latest: the loop's end, or a punch's.
    fn until(&self) -> Option<f64> {
        match self.span {
            Span::Open => None,
            Span::To { end, .. } | Span::Loop { end, .. } => Some(end),
        }
    }

    /// Where `(clock, pos)` (`Player::heard_frame`: the engine clock and the transport frame) is
    /// in the take, once the anchor is known. The clock never wraps, but steps by blocks; the
    /// transport's frame is exact. So the frame says, in the pass the clock says or one next to it,
    /// when it is within two blocks of where the clock puts the transport; else it jumped since or
    /// never moved (a count), and the clock says.
    fn place(&self, (clock, pos): (u64, u64)) -> Option<Place> {
        let anchor = self.anchor? as i64;
        let (c, f, pos) = (clock as i64, self.frame as i64, pos as i64);
        let near = 2 * m2_runtime::BLOCK as i64;
        let (k, expect): (i64, Box<dyn Fn(i64) -> i64>) = match self.span {
            Span::Loop { s, e, .. } if e > s => {
                let (s, e) = (s as i64, e as i64);
                let wrap = anchor + (e - f);
                let k = if c < wrap { 0 } else { 1 + (c - wrap) / (e - s) };
                (k, Box::new(move |k| if k == 0 { f + (c - anchor) } else { s + (c - wrap) - (k - 1) * (e - s) }))
            }
            _ => (0, Box::new(move |_| f + (c - anchor))),
        };
        for kk in [k, k + 1, k - 1] {
            if kk >= 0 && (expect(kk) - pos).abs() <= near && (kk == 0 || matches!(self.span, Span::Loop { .. })) {
                return Some((kk as u32, pos as f64));
            }
        }
        Some((k as u32, expect(k) as f64))
    }

    /// Frames from the anchor to `at`, through the passes before it.
    fn since(&self, (pass, frame): Place) -> f64 {
        let f = self.frame as f64;
        match self.span {
            Span::Loop { s, e, .. } if pass > 0 => (e as f64 - f) + (pass - 1) as f64 * (e - s) as f64 + (frame - s as f64),
            _ => frame - f,
        }
    }

    /// Find the anchor: the block the count or the Play applied at, and the way in after it; else,
    /// once the song plays, from where it is heard (the stamp was overtaken by later ones).
    fn resolve(&mut self, player: &Player) {
        self.find_anchor(player);
        // An audio take's writers start from the frame heard there.
        if let Some(a) = self.anchor {
            for p in &self.audio {
                if let Some(w) = &p.writer {
                    w.anchor(a, p.lead);
                }
            }
        }
    }

    fn find_anchor(&mut self, player: &Player) {
        if self.anchor.is_some() {
            return;
        }
        self.anchor = match self.phase {
            Phase::Count { seq, frames, .. } => player.applied(seq).map(|a| a.clock + frames + self.skip),
            Phase::Play { seq } => player.applied(seq).map(|a| a.clock + self.skip),
            Phase::Take => None,
        };
        if self.anchor.is_none()
            && player.is_playing()
            && let Some((clock, pos)) = player.heard_frame()
        {
            self.anchor = Some((clock as i64 - (pos as i64 - self.frame as i64)).max(0) as u64);
        }
    }

    /// Still counting in at engine clock `clock`: the count's bar hasn't ended.
    fn counting(&self, clock: Option<u64>) -> bool {
        matches!(self.phase, Phase::Count { .. }) && !matches!((self.anchor, clock), (Some(a), Some(c)) if c + self.skip >= a)
    }

    /// Nothing recorded yet at engine clock `clock`: counting in, or playing up to where it records.
    fn before(&self, clock: Option<u64>) -> bool {
        self.counting(clock) || matches!((self.anchor, clock), (Some(a), Some(c)) if c < a)
    }

    /// A note's start heard at engine clock `clock`, at `at`: on the take's start if it came in the
    /// count's last beat, by the clock; None when it came before that.
    fn admit(&self, clock: u64, (pass, frame): Place) -> Option<f64> {
        let beat = self.beat(frame);
        if pass > 0 || beat >= self.from {
            return Some(beat);
        }
        let early = self.anchor.map_or(f64::INFINITY, |a| a as f64 - clock as f64);
        (early <= self.lead + 1.0).then_some(self.from)
    }

    /// `at` was heard: the latest so far, unless it is earlier.
    fn heard(&mut self, at: Place) {
        if (at.0, at.1) > (self.last.0, self.last.1) {
            self.last = at;
        }
    }

    fn on(&mut self, key: (u32, u32), pitch: f64, velocity: f64, clock: u64, at: Place) -> bool {
        self.heard(at);
        let Some(start) = self.admit(clock, at) else { return false };
        self.off(key, at);
        let Some(n) = self.notes.as_mut() else { return false };
        n.held.insert(key, (start, pitch, velocity));
        true
    }

    fn off(&mut self, key: (u32, u32), at: Place) -> bool {
        self.heard(at);
        let beat = self.beat(at.1);
        let end = self.until().map_or(beat, |u| beat.min(u));
        let Some(n) = self.notes.as_mut() else { return false };
        let Some((start, pitch, velocity)) = n.held.remove(&key) else { return false };
        n.notes.push(Note { pitch, start, length: (end - start).max(SHORTEST), velocity });
        true
    }

    /// How this pass's notes go on their lane: into the clip under them with Overdub; over the
    /// range with Punch, in the first pass; else a clip of their own (MON-10 to MON-12).
    fn how(&self) -> How {
        match () {
            _ if self.overdub => How::Into,
            _ if self.punch && self.pass == 0 => How::Replace,
            _ => How::New,
        }
    }

    /// This pass's notes clip, ended at `beat`: held notes end there, and the clip spans whole bars,
    /// at least one, to the bar line after its last note's end (a note the grid moved later too:
    /// the review's R6); a punch's covers exactly what was recorded. None when nothing was played.
    fn notes_clip(&mut self, beat: f64) -> Option<Part> {
        let how = self.how();
        let n = self.notes.as_mut()?;
        for (start, pitch, velocity) in std::mem::take(&mut n.held).into_values() {
            n.notes.push(Note { pitch, start, length: (beat - start).max(SHORTEST), velocity });
        }
        if n.notes.is_empty() {
            return None;
        }
        let mut notes = std::mem::take(&mut n.notes);
        notes.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.total_cmp(&b.pitch)));
        let start = self.start;
        let mut placed: Vec<(f64, f64)> = notes.iter().map(|n| (n.start - start, n.length)).collect();
        if let Some(grid) = self.quantize {
            quantized(&mut placed, start, grid);
        }
        let end = match how {
            How::Replace => beat.max(start + SHORTEST),
            _ => {
                let last = placed.iter().map(|(s, l)| start + s + l.max(SHORTEST)).fold(beat, f64::max);
                bar_after(&self.arrangement, last).max(start + self.arrangement.meter_at(start).bar())
            }
        };
        let notes: Vec<Value> = notes
            .iter()
            .zip(&placed)
            .map(|(n, (s, l))| json!({"pitch": n.pitch, "start": s, "length": l, "velocity": n.velocity}))
            .collect();
        let clip = json!({"start": start, "length": end - start, "notes": notes});
        Some(Part::Notes { module: n.module.clone(), lane: n.lane.clone(), clip, how })
    }

    /// The pass ended at the loop's end: its notes clip, to be written now, so the next pass plays
    /// it (MON-11). The next pass starts at the loop's start.
    fn next_pass(&mut self) -> Option<Ended> {
        let Span::Loop { start, end, .. } = self.span else { return None };
        let part = self.notes_clip(end);
        self.pass += 1;
        self.start = if self.punch && !self.overdub { start } else { bar_before(&self.arrangement, start) };
        part.map(|p| Ended { token: self.token, tempo: self.tempo.clone(), parts: vec![p] })
    }

    /// The take, ended at `at`: its last notes pass, and each audio lane's file, told how long it is.
    fn end(mut self, at: Place) -> Ended {
        let mut at = at;
        if let Some(u) = self.until() {
            let beat = self.beat(at.1);
            if beat > u {
                at.1 = m2_runtime::transport::frame_of(&self.tempo, u, self.rate) as f64;
            }
        }
        let beat = self.beat(at.1).max(self.pass_from(self.pass));
        let mut parts: Vec<Part> = self.notes_clip(beat).into_iter().collect();
        // As long as it played, from where it started: whole frames.
        let frames = self.since(at).round().max(0.0) as u64;
        let how = if self.punch { How::Replace } else { How::New };
        for mut p in std::mem::take(&mut self.audio) {
            let Some(w) = p.writer.take() else { continue };
            w.end(frames);
            let at = self.placing(at, p.lead);
            parts.push(Part::Audio { module: p.module, lane: p.lane, how, at, file: Audio::Writing(w) });
        }
        Ended { token: self.token, tempo: self.tempo, parts }
    }

    /// Where an audio take ended at `at` goes (MON-11), its file led by `lead` frames of silence:
    /// over the loop when it went round, playing the newest whole pass, with every pass listed;
    /// else from where it started, to the file's end.
    fn placing(&self, at: Place, lead: u64) -> Placing {
        let (rate, lead) = (self.rate, lead as f64);
        match self.span {
            Span::Loop { start, end, s, e } if at.0 > 0 => {
                let f = self.frame as f64;
                let len = (e - s) as f64;
                // Each pass's start in the file, from the anchor's frame.
                let starts: Vec<f64> =
                    (0..=at.0).map(|k| if k == 0 { s as f64 - f } else { (e as f64 - f) + (k - 1) as f64 * len }).collect();
                let total = self.since(at);
                // A pass that starts before the file does (the loop turned on after the take began) is
                // no take of the loop's.
                let kept: Vec<(f64, bool)> =
                    starts.iter().map(|&x| (x, total - x >= len - 1.0)).filter(|(x, _)| total > *x && x + lead > -1.0).collect();
                let plays = kept.iter().rposition(|(_, whole)| *whole).unwrap_or(kept.len().saturating_sub(1));
                let takes: Vec<f64> = kept.iter().map(|(x, _)| ((x + lead) / rate).max(0.0)).collect();
                Placing { start, offset: takes.get(plays).copied().unwrap_or(0.0), length: Some(end - start), takes }
            }
            // A punch that ran to its end covers its range exactly; else as long as the file.
            Span::To { end, e } if at.1 >= e as f64 => {
                Placing { start: self.from, offset: lead / rate, length: Some(end - self.from), takes: vec![] }
            }
            _ => Placing { start: self.from, offset: lead / rate, length: None, takes: vec![] },
        }
    }

    /// What the windows see: its phase, lanes, start, and its notes so far (a held one to `now`).
    fn shown(&self, now: Option<f64>) -> Value {
        let mut notes = vec![];
        if let Some(n) = &self.notes {
            notes.extend(n.notes.iter().map(|n| json!({"pitch": n.pitch, "start": n.start, "length": n.length, "velocity": n.velocity})));
            for (start, pitch, velocity) in n.held.values() {
                let length = now.map_or(SHORTEST, |n| (n - start).max(SHORTEST));
                notes.push(json!({"pitch": pitch, "start": start, "length": length, "velocity": velocity, "held": true}));
            }
        }
        let from = self.pass_from(self.pass);
        let counting = matches!(self.phase, Phase::Count { .. }) && now.is_none_or(|n| n < from);
        let count = match self.phase {
            Phase::Count { beats, ms, .. } => json!({"beats": beats, "ms": ms}),
            _ => Value::Null,
        };
        // An audio take's clip starts where recording starts; over a loop, at its start.
        let audio_start = match self.span {
            Span::Loop { start, .. } if self.pass > 0 => start,
            _ => from,
        };
        let lanes: Vec<Value> =
            self.notes
                .iter()
                .map(|n| json!({"module": n.module, "lane": n.lane, "kind": "notes", "start": self.start}))
                .chain(self.audio.iter().map(
                    |a| json!({"module": a.module, "lane": a.lane, "kind": "audio", "start": audio_start, "channels": a.channels.name()}),
                ))
                .collect();
        let first = &lanes[0];
        let start = first["start"].as_f64().unwrap_or(from);
        json!({
            "phase": if counting { "count" } else { "take" },
            "module": first["module"], "lane": first["lane"], "kind": first["kind"],
            "lanes": lanes,
            "hear": false,
            "start": start, "from": from, "until": self.until(), "pass": self.pass,
            "punch": self.punch, "overdub": self.overdub,
            "count_in": count, "notes": notes,
        })
    }
}

impl Recorder {
    /// The arms of song `token`: none when another song's were kept.
    fn arms(&mut self, token: u64) -> &mut Arms {
        if self.arms.token != token {
            let (input, hear, listen) = (self.arms.input.clone(), self.arms.hear, self.arms.listen.take());
            self.arms = Arms { token, input, hear, listen, ..Arms::default() };
        }
        &mut self.arms
    }

    /// The input was closed: nothing is heard from it, and the next arm or take opens it again.
    pub(super) fn forget_input(&mut self) {
        self.arms.listen = None;
    }

    /// The ring of the first audio lane while a take records on one: what the meters read the
    /// input's level from.
    pub(super) fn input(&self) -> Option<&Arc<m2_play::ring::Ring>> {
        self.take.as_ref().and_then(|t| t.audio.first()).map(|a| &a.ring)
    }

    /// What MIDI keyboards play in song `token`, whatever a window selects: the take's notes lane
    /// while one records (D8), else the armed notes track's (MON-9).
    pub(super) fn lane(&self, token: Option<u64>) -> Option<(String, String)> {
        if let Some(t) = self.take.as_ref().filter(|t| Some(t.token) == token) {
            return t.notes.as_ref().map(|n| (n.module.clone(), n.lane.clone()));
        }
        let a = &self.arms;
        (Some(a.token) == token).then(|| a.armed.iter().find(|(_, (_, audio))| !audio).map(|(m, (l, _))| (m.clone(), l.clone()))).flatten()
    }

    /// The take's notes heard: called by audition for each On it sends from a source that records
    /// (`source`'s id), on `device`.
    pub(super) fn on(&mut self, device: &Device, lane: (&str, &str), key: (u32, u32), pitch: f64, velocity: f64) {
        let Some(t) = self
            .take
            .as_mut()
            .filter(|t| t.epoch == device.epoch && t.notes.as_ref().is_some_and(|n| (n.module.as_str(), n.lane.as_str()) == lane))
        else {
            return;
        };
        t.resolve(&device.player);
        let Some((heard, at)) = device.player.heard_frame().and_then(|h| Some((h, t.place(h)?))) else { return };
        if self.passed(at) {
            return;
        }
        let t = self.take.as_mut().expect("a take");
        self.changed |= t.on(key, pitch, velocity, heard.0, at);
    }

    /// A note let go, as audition sends its Off.
    pub(super) fn off(&mut self, device: &Device, key: (u32, u32)) {
        let Some(t) = self.take.as_mut().filter(|t| t.epoch == device.epoch && t.notes.as_ref().is_some_and(|n| n.held.contains_key(&key)))
        else {
            return;
        };
        t.resolve(&device.player);
        let Some(at) = device.player.heard_frame().and_then(|h| t.place(h)) else { return };
        if self.passed(at) {
            return;
        }
        let t = self.take.as_mut().expect("a take");
        self.changed |= t.off(key, at);
    }

    /// Every note source `id` holds, let go.
    pub(super) fn release(&mut self, device: Option<&Device>, id: u32) {
        let keys: Vec<(u32, u32)> = self
            .take
            .as_ref()
            .and_then(|t| t.notes.as_ref())
            .map(|n| n.held.keys().filter(|k| k.0 == id).copied().collect())
            .unwrap_or_default();
        if let Some(d) = device {
            keys.into_iter().for_each(|k| self.off(d, k));
        }
    }

    /// The take heard at `at`: each pass that ended before it goes to be written (MON-11), and a
    /// punch past its end ends the take there (MON-10). Whether the take ended.
    fn passed(&mut self, at: Place) -> bool {
        let Some(t) = self.take.as_mut() else { return true };
        while t.pass < at.0 && matches!(t.span, Span::Loop { .. }) {
            if let Some(e) = t.next_pass() {
                self.ended.push(e);
            }
            self.changed = true;
        }
        if let Span::To { e, .. } = t.span
            && at.1 >= e as f64
        {
            self.end_at((0, e as f64));
            return true;
        }
        false
    }

    /// End the take at `at` (it waits to be written), or drop it while it counts in.
    fn end_at(&mut self, at: Place) {
        if let Some(t) = self.take.take() {
            self.ended.push(t.end(at));
            self.changed = true;
        }
    }

    /// End the take where `device` is heard now: Stop, R, a jump. Before it records anything
    /// (counting in, or on its way in), it is dropped.
    fn end_now(&mut self, device: Option<&Device>) {
        let Some(t) = self.take.as_mut() else { return };
        let heard = device.filter(|d| d.epoch == t.epoch).and_then(|d| {
            t.resolve(&d.player);
            d.player.heard_frame()
        });
        if t.before(heard.map(|(c, _)| c)) {
            self.drop_take();
            return;
        }
        let (at, last) = (heard.and_then(|h| t.place(h)), t.last);
        let at = match at {
            Some(at) if !self.passed(at) => at,
            Some(_) => return,
            None => last,
        };
        self.end_at(at);
    }

    fn drop_take(&mut self) {
        self.changed |= self.take.take().is_some();
    }

    /// The lanes that hear the input in song `token`, with their channels: the take's audio lanes,
    /// and the armed audio tracks, while you hear yourself (MON-8, MON-9). None of a song not open.
    fn wanted(&self, token: u64) -> BTreeMap<(String, String), Option<Channels>> {
        let mut out = BTreeMap::new();
        if !self.arms.hear || self.arms.token != token {
            return out;
        }
        for (m, (l, audio)) in &self.arms.armed {
            if *audio {
                out.insert((m.clone(), l.clone()), self.arms.channels.get(m).copied());
            }
        }
        if let Some(t) = self.take.as_ref().filter(|t| t.token == token) {
            for a in &t.audio {
                out.insert((a.module.clone(), a.lane.clone()), Some(a.channels));
            }
        }
        out
    }
}

/// Tell the engine what each audio lane hears where it differs from what it was last told (MON-8):
/// under the control plane's lock, with no wait. A new plan or a new opening of the input is told
/// whole: what moved into it hearing, and shouldn't, stops.
fn send_hearing(cp: &mut ControlPlane, device: Option<&Device>) {
    let (Some(d), Some(p)) = (device, cp.open.as_ref()) else { return };
    let token = p.engine.token;
    let Some(plays) = p.engine.plays.as_ref().filter(|pl| pl.epoch == d.epoch) else { return };
    let r = &mut cp.record;
    let listen = r.arms.listen.as_ref().filter(|(epoch, _)| *epoch == d.epoch).map(|(_, l)| l.clone());
    let mut lanes = BTreeMap::new();
    if let Some(l) = &listen {
        for ((m, lane), ch) in r.wanted(token) {
            let ch = ch.unwrap_or_else(|| Channels::first(l.channels));
            if let Some(slot) = audio_slot(plays, &m, &lane).filter(|_| ch.fits(l.channels)) {
                lanes.insert(slot, ch);
            }
        }
    }
    let now = Heard { epoch: d.epoch, generation: plays.generation, open: listen.as_ref().map_or(0, |l| l.open), lanes };
    if r.heard == now {
        return;
    }
    let was = std::mem::replace(&mut r.heard, now.clone());
    let same = (was.epoch, was.generation, was.open) == (now.epoch, now.generation, now.open);
    let stop: Vec<u32> = match (same, was.epoch == now.epoch && !was.lanes.is_empty()) {
        (true, _) => was.lanes.keys().filter(|s| !now.lanes.contains_key(s)).copied().collect(),
        // What heard in the plan before moved with its lanes: each lane not wanted stops.
        (false, true) => (0..plays.info.lanes.len() as u32)
            .filter(|s| plays.info.lanes[*s as usize].audio().is_some() && !now.lanes.contains_key(s))
            .collect(),
        (false, false) => vec![],
    };
    for slot in stop {
        let _ = d.player.hear(now.generation, slot, None);
    }
    if let Some(l) = &listen {
        for (slot, ch) in &now.lanes {
            if !same || was.lanes.get(slot) != Some(ch) {
                let _ = d.player.hear(now.generation, *slot, Some((l, *ch)));
            }
        }
    }
}

/// The track a take records on in project `token` now, counting in or recording (AGENT-3): its
/// first.
pub(super) fn recording_on(cp: &ControlPlane, token: u64) -> Option<String> {
    let t = cp.record.take.as_ref().filter(|t| t.token == token)?;
    t.notes.as_ref().map(|n| n.module.clone()).or_else(|| t.audio.first().map(|a| a.module.clone()))
}

/// The track's name as the timeline shows it, for the undo step.
pub(super) fn label_of(cp: &ControlPlane, module: &str) -> String {
    cp.open
        .as_ref()
        .and_then(|p| {
            p.shown.timeline.tracks.iter().find(|t| t.module == module).and_then(|t| t.head.get("label")?.as_str().map(String::from))
        })
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

/// The lanes a take records on: each `(module, lane, audio)`.
type Lanes = Vec<(String, String, bool)>;

impl Graph {
    /// Arm track `module` for recording, or disarm it, on lane `lane`, and choose the channels it
    /// records (MON-9, MON-13): `input` and `hear` are Settings'. No edit; refused while a take
    /// records. Opens the input when an armed audio track should hear it. Answers the arms.
    #[allow(clippy::too_many_arguments)]
    pub fn arm(
        &self,
        token: u64,
        module: &str,
        lane: Option<&str>,
        armed: Option<bool>,
        channels: Option<Channels>,
        input: InputChoice,
        hear: bool,
    ) -> Result<Value> {
        let device = self.inner.device()?;
        {
            let mut cp = self.inner.control.lock();
            let p = cp.open.as_ref().context("no project is open")?;
            if p.engine.token != token {
                bail!("the open project changed; refresh the graph before editing");
            }
            if cp.record.take.as_ref().is_some_and(|t| t.token == token) && (armed.is_some() || channels.is_some()) {
                bail!("A take is recording: arm tracks before you record.");
            }
            let label = label_of(&cp, module);
            // Whether the lane is an audio lane, a notes lane, or neither, in the song as the
            // windows show it (what plays may be a moment behind a track just added).
            let kind = lane.map(|lane| {
                let l = p.shown.timeline.tracks.iter().find(|t| t.module == module)?.lanes.iter().find(|l| l.key == lane)?;
                match l.json["kind"].as_str() {
                    Some("notes") => Some(false),
                    Some("audio") => Some(true),
                    _ => None,
                }
            });
            let a = cp.record.arms(token);
            (a.input, a.hear) = (input, hear);
            match armed {
                Some(true) => {
                    let lane = lane.context("Say which lane the track records on.")?;
                    let audio = kind.flatten().with_context(|| format!("{label} has no notes or audio lane `{lane}` to record on."))?;
                    // One notes track at a time: the keyboards play it.
                    if !audio {
                        a.armed.retain(|_, (_, audio)| *audio);
                    }
                    a.armed.insert(module.into(), (lane.into(), audio));
                }
                Some(false) => {
                    a.armed.remove(module);
                }
                None => {}
            }
            if let Some(ch) = channels {
                if let Some((_, l)) = a.listen.as_ref().filter(|(_, l)| !ch.fits(l.channels)) {
                    bail!("{} has no input {}: it has {}.", l.name, ch.name().replace('-', "–"), inputs_said(l.channels));
                }
                a.channels.insert(module.into(), ch);
            }
        }
        // An armed audio track hears its input: it opens now, off the lock (D14).
        if hear && !self.inner.control.lock().record.wanted(token).is_empty() {
            self.open_input(token, &device)?;
        }
        let mut cp = self.inner.control.lock();
        send_hearing(&mut cp, Some(&device));
        Ok(arm_state(&cp.record, token))
    }

    /// What Settings says of the input now (MON-8): the next take records it, and armed audio
    /// tracks hear it at once when they should.
    pub fn set_input(&self, input: InputChoice, hear: bool) {
        let token = {
            let mut cp = self.inner.control.lock();
            let token = cp.open.as_ref().map(|p| p.engine.token);
            let a = &mut cp.record.arms;
            (a.input, a.hear) = (input, hear);
            token
        };
        let Some(device) = self.inner.current_device() else { return };
        let Some(token) = token else { return };
        let (wants, recording) = {
            let cp = self.inner.control.lock();
            (!cp.record.wanted(token).is_empty(), cp.record.take.is_some())
        };
        if wants
            && !recording
            && let Err(e) = self.open_input(token, &device)
        {
            eprintln!("music2: the sound input didn't open for the armed tracks: {e:#}");
        }
        let mut cp = self.inner.control.lock();
        send_hearing(&mut cp, Some(&device));
    }

    /// Open the input Settings chose in `device`'s player, or keep the one open, off the control
    /// plane's lock (D14), and keep it for hearing: its faults are named from now (D3).
    fn open_input(&self, token: u64, device: &Device) -> Result<Listen> {
        let choice = self.inner.control.lock().record.arms.input.clone();
        let listen = device.player.open_input(&choice)?;
        self.inner.told.lock().opened(token, InputNamer::of(&listen));
        self.inner.control.lock().record.arms.listen = Some((device.epoch, listen.clone()));
        Ok(listen)
    }

    /// Start a take on the armed tracks of song `token`, or on lane `(module, lane)` with none armed
    /// (Q19 to Q21, MON-9): stopped, after a bar of clicks or at once, from where play starts;
    /// playing, from now; with Punch or the loop, from the loop's start at the earliest. Answers
    /// the take as the windows see it. Never waits for the session.
    pub fn record_start(&self, token: u64, module: &str, lane: &str, opts: Options) -> Result<Value> {
        if let Some(g) = opts.quantize
            && !(g.is_finite() && g > 0.0)
        {
            bail!("The grid is a number of beats above 0, such as 0.25 for sixteenths.");
        }
        let device = self.inner.device()?;
        // The lanes: the armed tracks', else the one named. Audio lanes record from the sound
        // input: it is opened, and each take's file made and its writer started, first, never under
        // the control plane's lock (D14, the review's io-r1). A take refused below lets them go,
        // and their part files with them.
        let (lanes, dir, labels) = {
            let mut cp = self.inner.control.lock();
            let a = cp.record.arms(token);
            (a.input, a.hear) = (opts.input.clone(), opts.hear);
            let armed: Lanes = a.armed.iter().map(|(m, (l, audio))| (m.clone(), l.clone(), *audio)).collect();
            let p = cp.open.as_ref().context("no project is open")?;
            let plays = p.engine.plays.as_ref().filter(|pl| pl.epoch == device.epoch);
            let dir = p.shown.dir.clone();
            let lanes: Lanes = match armed.is_empty() {
                false => armed,
                true => {
                    let audio = plays.is_some_and(|pl| audio_slot(pl, module, lane).is_some());
                    vec![(module.to_string(), lane.to_string(), audio)]
                }
            };
            let labels: Vec<String> = lanes.iter().map(|(m, _, _)| label_of(&cp, m)).collect();
            (lanes, dir, labels)
        };
        let audio_lanes: Vec<usize> = (0..lanes.len()).filter(|i| lanes[*i].2).collect();
        let mut parts = vec![];
        let mut listen = None;
        if !audio_lanes.is_empty() {
            let l = self.open_input(token, &device)?;
            for &i in &audio_lanes {
                let (module, lane, _) = &lanes[i];
                let ch = self.inner.control.lock().record.arms.channels.get(module).copied().unwrap_or_else(|| Channels::first(l.channels));
                let ring = l.ring(ch).with_context(|| {
                    format!(
                        "{} records from input {}, which {} hasn't: it has {}.",
                        labels[i],
                        ch.name().replace('-', "–"),
                        l.name,
                        inputs_said(l.channels)
                    )
                })?;
                let file = m2_editor::take::Growing::new(&dir, &labels[i], ch.width(), l.rate).map_err(|e| anyhow::anyhow!("{e}"))?;
                let writer = Writer::start(ring.clone(), ch, file, format!("{} take.wav", labels[i]))?;
                parts.push(AudioPart {
                    module: module.clone(),
                    lane: lane.clone(),
                    channels: ch,
                    writer: Some(writer),
                    ring: ring.clone(),
                    input: l.open,
                    lead: 0,
                });
            }
            listen = Some(l);
        }
        let looped = {
            let timing = self.inner.timing.lock();
            let t = timing.as_ref().context("no project is open")?;
            (t.range, t.on && !device.player.loop_refused())
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
        let mut notes = None;
        for (i, (m, l, audio)) in lanes.iter().enumerate() {
            let fits = plays.is_some_and(|pl| if *audio { audio_slot(pl, m, l).is_some() } else { plays_notes(pl, m, l) });
            if !fits && lanes.len() == 1 {
                bail!("{NO_LANE}");
            }
            if !fits {
                bail!("{} can't record now: its lane `{l}` isn't in the song. Arm it again.", labels[i]);
            }
            if !audio {
                notes = Some(NotesPart { module: m.clone(), lane: l.clone(), notes: vec![], held: BTreeMap::new() });
            }
        }
        let arrangement = p.engine.accepted.project.arrangement.clone();
        let tempo = plays.and_then(|pl| pl.tempo.as_deref().cloned()).unwrap_or_else(|| arrangement.tempo_map());
        let player = &device.player;
        let rate = player.rate() as f64;
        let st = player.status();
        if st.playing && st.pending.is_some() {
            // A jump the audio thread hasn't taken yet: where it is heard is about to change.
            bail!("The song is moving to another place. Press Record again.");
        }
        let now = if st.playing {
            let (_, pos) = player.heard_frame().context("the sound output hasn't started yet")?;
            tempo.beat_at(pos as f64 / rate)
        } else {
            st.pending.unwrap_or(st.beats).max(0.0)
        };
        // Where recording starts and ends: a punch or a loop starts it at the loop's start at the
        // earliest (MON-10, MON-11).
        let frame_of = |beat: f64| m2_runtime::transport::frame_of(&tempo, beat, rate);
        let play = frame_of(now);
        let (range, on) = looped;
        let (from, span) = match (opts.punch, range, on) {
            (true, None, _) => bail!("Punch records between the loop's edges: set a loop first."),
            (true, Some((_, end)), _) if now >= end => bail!("The playhead is past the loop's end, where Punch stops recording."),
            (_, Some((start, end)), true) if now < end => (now.max(start), Span::Loop { start, end, s: frame_of(start), e: frame_of(end) }),
            (true, Some((start, end)), _) => (now.max(start), Span::To { end, e: frame_of(end) }),
            _ => (now, Span::Open),
        };
        let frame = frame_of(from);
        let mut take = Take {
            token,
            epoch: device.epoch,
            from,
            frame,
            lead: 0.0,
            quantize: opts.quantize,
            tempo,
            arrangement,
            rate,
            phase: Phase::Take,
            anchor: None,
            skip: 0,
            span,
            punch: opts.punch,
            overdub: opts.overdub,
            last: (0, frame as f64),
            pass: 0,
            start: 0.0,
            notes,
            audio: parts,
            began: Instant::now(),
        };
        // Each file over a loop starts at the loop's start: silence up to where recording starts.
        if let Span::Loop { s, .. } = take.span {
            for a in &mut take.audio {
                a.lead = frame.saturating_sub(s);
            }
        }
        if st.playing {
            // From now, with no count; on a way in, from where it records.
            let (clock, pos) = player.heard_frame().context("the sound output hasn't started yet")?;
            take.skip = frame.saturating_sub(pos);
            take.anchor = Some(clock + take.skip);
        } else {
            take.skip = frame.saturating_sub(play);
            if opts.count_in {
                let meter = take.arrangement.meter_at(now);
                let beat = 4.0 / meter.unit as f64;
                let frames_per_beat = beat * 60.0 / take.tempo.bpm_at(now) * rate;
                let seq = player.count_in(meter.beats, frames_per_beat)?;
                let bar = (meter.beats as f64 * frames_per_beat).round().max(1.0) as u64;
                let frames = bar.div_ceil(m2_runtime::BLOCK as u64) * m2_runtime::BLOCK as u64;
                take.phase = Phase::Count { seq, frames, beats: meter.beats, ms: bar as f64 / rate * 1000.0 };
                // A note in the count's last beat lands on the first, unless the song plays first.
                take.lead = if take.skip == 0 { frames_per_beat } else { 0.0 };
            } else {
                take.phase = Phase::Play { seq: player.transport(m2_runtime::Cmd::Play)? };
            }
        }
        take.start = match (take.punch && !take.overdub, take.notes.is_some()) {
            (true, _) | (false, false) => from,
            (false, true) => bar_before(&take.arrangement, from),
        };
        take.resolve(player);
        let mut shown = take.shown(None);
        shown["hear"] = json!(opts.hear && !take.audio.is_empty());
        cp.record.take = Some(take);
        cp.record.changed = true;
        if let Some(l) = listen {
            cp.record.arms.listen = Some((device.epoch, l));
        }
        // Hearing yourself from now, the count included (MON-8).
        send_hearing(&mut cp, Some(&device));
        Ok(shown)
    }

    /// What the silent input hears next (D24): `samples`, then a sine `tone` (pitch, seconds, gain),
    /// at the song's rate, on every channel, or only on `channel` (from 1). Says so when no take has
    /// opened the input.
    pub fn inject_input(&self, samples: &[f32], tone: Option<(f32, f32, f32)>, channel: Option<u16>) -> Result<()> {
        const NONE: &str = "Nothing is recording from the sound input.";
        let device = self.inner.current_device().context(NONE)?;
        let player = &device.player;
        let Some(listen) = player.input() else { bail!(NONE) };
        let mut all = samples.to_vec();
        if let Some((hz, seconds, gain)) = tone {
            let rate = player.rate() as f32;
            let n = (seconds.clamp(0.0, 60.0) * rate) as usize;
            all.extend((0..n).map(|i| gain * (std::f32::consts::TAU * hz * i as f32 / rate).sin()));
        }
        match channel {
            None => player.inject_input(&all),
            Some(c) if c >= 1 && c <= listen.channels => {
                let mut each = vec![vec![0.0; all.len()]; listen.channels as usize];
                each[c as usize - 1] = all;
                player.inject_channels(&each)
            }
            Some(c) => bail!("{} has no channel {c}: it has {}.", listen.name, inputs_said(listen.channels)),
        }
    }

    /// The open input's device goes away, as a microphone unplugged (the inject API): a take from it
    /// ends with what it has (the review's io-r2).
    pub fn unplug_input(&self) -> Result<()> {
        let device = self.inner.current_device().context("Nothing is recording from the sound input.")?;
        device.player.input_error(m2_play::DeviceErrorKind::DeviceNotAvailable.into());
        Ok(())
    }

    /// The OS refuses the microphone from now, as a Mac's Don't Allow does (the inject API): a take
    /// from it is dropped, and the next R is refused.
    pub fn refuse_input(&self) -> Result<()> {
        let device = self.inner.current_device().context("Nothing is recording from the sound input.")?;
        device.player.refuse_input(true);
        Ok(())
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
        send_hearing(&mut cp, device.as_ref());
        Ok(json!({"ended": true}))
    }

    /// R or Esc while counting in: nothing plays, nothing is kept. While recording: the take is
    /// dropped, and play goes on.
    pub fn record_cancel(&self, token: u64) -> Result<Value> {
        let device = self.inner.current_device();
        let counting = {
            let mut cp = self.inner.control.lock();
            let Some(t) = cp.record.take.as_ref().filter(|t| t.token == token) else { return Ok(json!({"cancelled": false})) };
            // Counting as the windows were last told, though its bar may have just ended (the
            // review's R7).
            let counting = matches!(t.phase, Phase::Count { .. });
            cp.record.drop_take();
            send_hearing(&mut cp, device.as_ref());
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
            send_hearing(&mut cp, device.as_ref());
            return (action == "toggle").then_some("stop");
        }
        if matches!(action, "stop" | "toggle") {
            cp.record.end_now(device.as_ref());
            send_hearing(&mut cp, device.as_ref());
        }
        None
    }

    /// The loop changed (B's line in [`Graph::set_loop`], the review's R2): in the first pass the
    /// take now goes round the new loop, while the song is still before its end (a song past it
    /// plays on); with no loop, it goes on. After a pass, or with Punch, the take ends where it is
    /// heard (MON-11).
    pub(super) fn record_loop(&self, looped: Option<(f64, f64)>) {
        let device = self.inner.current_device();
        let mut cp = self.inner.control.lock();
        let Some(t) = cp.record.take.as_mut() else { return };
        if t.pass > 0 || t.punch {
            cp.record.end_now(device.as_ref());
            send_hearing(&mut cp, device.as_ref());
            return;
        }
        let now = device.filter(|d| d.epoch == t.epoch).map_or(t.from, |d| d.player.status().beats);
        let (tempo, rate) = (&t.tempo, t.rate);
        let span = match looped.filter(|&(_, end)| t.from < end && now < end) {
            Some((start, end)) => Span::Loop {
                start,
                end,
                s: m2_runtime::transport::frame_of(tempo, start, rate),
                e: m2_runtime::transport::frame_of(tempo, end, rate),
            },
            None => Span::Open,
        };
        if span != t.span {
            t.span = span;
            cp.record.changed = true;
        }
    }

    /// A jump (B's line in [`Graph::seek`]): it ends the take where it is heard (D5).
    pub(super) fn record_jump(&self) {
        let device = self.inner.current_device();
        let mut cp = self.inner.control.lock();
        if cp.record.take.is_some() {
            cp.record.end_now(device.as_ref());
            send_hearing(&mut cp, device.as_ref());
        }
    }

    /// The take thread's tick, every 100 ms (D7, D9): a count that turned into playing, or never
    /// did; a pass over, a punch past its end, or a take whose player or song went; then each take
    /// that ended, written; and what each lane hears, told again where it changed (MON-8). Answers
    /// what the windows hear, in order: the take as it is when it changed, then each outcome.
    pub fn record_tick(&self) -> Vec<Value> {
        let mut said = vec![];
        let mut unplugged = None;
        // A fault of the take's input, reported once no lock is held.
        let mut fault = None;
        let (ended, labels) = {
            let device = self.inner.current_device();
            let mut cp = self.inner.control.lock();
            let token = cp.open.as_ref().map(|p| p.engine.token);
            let r = &mut cp.record;
            if let Some(t) = r.take.as_mut() {
                let same = device.as_ref().filter(|d| d.epoch == t.epoch);
                let input = t.audio.first().map(|a| a.input);
                if Some(t.token) != token {
                    // The song closed or another opened: nothing is kept.
                    r.drop_take();
                } else if same.is_none() {
                    // A new player (a crash, another output): its clock starts again (D29). The take
                    // ends where it was last heard.
                    let at = t.last;
                    r.end_at(at);
                } else if let Some((d, input)) = same.zip(input).filter(|(d, _)| d.player.input_refused()) {
                    // The OS refuses the microphone (a Mac's Don't Allow, D11): the input gives
                    // silence, so the take is dropped, its part files with it, and said once. The
                    // song plays on.
                    fault = Some((Code::InputRefused, input));
                    unplugged = Some(d.player.clone());
                    r.drop_take();
                } else if let Some((d, input)) = same.zip(input).filter(|(d, _)| d.player.input_gone()) {
                    // The input went away (a microphone unplugged): the take ends where it is heard,
                    // with what it has, and the song plays on (the review's io-r2). Said once, as a
                    // problem.
                    t.resolve(&d.player);
                    let at = d.player.heard_frame().and_then(|h| t.place(h)).unwrap_or(t.last);
                    if !r.passed(at) {
                        r.end_at(at);
                    }
                    fault = Some((Code::InputLost, input));
                    unplugged = Some(d.player.clone());
                } else if t.audio.iter().any(|a| a.writer.as_ref().is_some_and(Writer::stopped)) {
                    // A writer stopped (a full disk, a file as long as a WAV can be): the take ends
                    // with the files as written, which say why (the review's io-r3).
                    let at = t.last;
                    r.end_at(at);
                } else if let Some(d) = same {
                    t.resolve(&d.player);
                    let heard = d.player.heard_frame();
                    let clock = heard.map(|(c, _)| c);
                    let at = heard.and_then(|h| t.place(h));
                    let counting = t.counting(clock);
                    let playing = d.player.is_playing();
                    let anchored = t.anchor.is_some();
                    match t.phase {
                        // The bar is over and the song plays: recording, or on its way in.
                        Phase::Count { .. } if !counting && playing => {
                            t.phase = Phase::Take;
                            r.changed = true;
                        }
                        // The bar ended a while ago and nothing plays, or the count never started:
                        // it was cancelled (D6).
                        Phase::Count { .. } => {
                            let late = match (t.anchor, clock) {
                                (Some(a), Some(c)) => (c + t.skip).saturating_sub(a) as f64 / t.rate > LATE_COUNT.as_secs_f64(),
                                _ => t.began.elapsed() > NEVER_COUNTED,
                            };
                            if late && !playing {
                                r.drop_take();
                            }
                        }
                        // Stopped some other way: the take ends there.
                        _ if !playing && anchored => r.end_now(Some(d)),
                        _ => {
                            if let Some(at) = at.filter(|_| !t.before(clock)) {
                                t.heard(at);
                                r.passed(at);
                            }
                        }
                    }
                }
            }
            send_hearing(&mut cp, device.as_ref());
            let r = &mut cp.record;
            if std::mem::take(&mut r.changed) {
                let now = device.as_ref().and_then(|d| {
                    let t = r.take.as_ref()?;
                    let (pass, frame) = d.player.heard_frame().and_then(|h| t.place(h))?;
                    (pass == t.pass).then(|| t.beat(frame))
                });
                said.push(match r.take.as_ref() {
                    Some(t) => {
                        let mut v = t.shown(now);
                        v["t"] = json!("take");
                        v["hear"] = json!(r.arms.hear && !t.audio.is_empty());
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
            let labels: Vec<String> =
                ended.iter().map(|e| e.parts.iter().map(|p| label_of(&cp, p.module())).collect::<Vec<_>>().join(", ")).collect();
            (ended, labels)
        };
        // Its stream stops; the next R opens it again.
        if let Some(p) = unplugged {
            p.close_input();
            self.inner.control.lock().record.arms.listen = None;
        }
        if let Some((code, input)) = fault {
            self.inner.faults.report(Fault { code, plan: 0, subject: Subject::Input(input) });
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

    /// Write a take that ended, or a pass of one: its audio files finished and durable first, a
    /// take over a loop with its note, then one Tx for every lane's clip, its undo step named
    /// "recording on Bass" (WRITE-1, MON-14). A file stays in `assets/` whatever becomes of the Tx
    /// (music2 never deletes an asset). Answers what the windows hear, or the take again when the
    /// code doesn't compile, to wait for it.
    fn write_take(&self, mut e: Ended, label: &str) -> Result<Value, Ended> {
        let lost = |why: &str| json!({"t": "take", "error": format!("The recording couldn't be kept: {why}")});
        let mut notes: Vec<String> = vec![];
        // Each file, written.
        for part in &mut e.parts {
            if let Part::Audio { file, .. } = part
                && let Audio::Writing(_) = file
            {
                let Audio::Writing(w) = std::mem::replace(file, Audio::Empty) else { unreachable!("writing") };
                match w.done() {
                    Ok(Some(w)) => *file = Audio::Written(w),
                    Ok(None) => {}
                    Err(err) => return Ok(lost(&format!("{err:#}"))),
                }
            }
        }
        // A file with no frame was nothing played.
        e.parts.retain(|p| !matches!(p, Part::Audio { file: Audio::Empty, .. }));
        if e.parts.is_empty() {
            return Ok(json!({"t": "take", "empty": true}));
        }
        let (open, dir) = {
            let cp = self.inner.control.lock();
            let p = cp.open.as_ref();
            (p.map(|p| p.engine.token), p.map(|p| p.shown.dir.clone()))
        };
        if open != Some(e.token) {
            return Ok(lost("its song was closed."));
        }
        let dir = dir.unwrap_or_default();
        let mut files = vec![];
        let mut takes = vec![];
        for part in &e.parts {
            let Ok(lane) = LaneAddr::new(part.module(), part.lane()) else { return Ok(lost(&format!("{label} is gone."))) };
            match part {
                Part::Notes { clip, how, .. } => takes.push(Taken { lane, clip: clip.clone(), how: *how }),
                Part::Audio { how, at, file: Audio::Written(w), .. } => {
                    let f = &w.0;
                    // A take over a loop: its note first, durable before the Tx names the file. One
                    // there already was written before a Tx that didn't land: it is this file's.
                    if !at.takes.is_empty() && m2_editor::take::takes_of(&dir, f.asset.as_str()).is_empty() {
                        if let Err(err) = m2_editor::take::note_takes(&dir, f.asset.as_str(), &at.takes) {
                            return Ok(lost(&format!("{err}")));
                        }
                    }
                    let length =
                        at.length.unwrap_or_else(|| m2_editor::import::length_at(&e.tempo, at.start, (f.seconds - at.offset).max(0.0)));
                    let mut clip = m2_editor::import::clip_json(f, at.start, length.max(m2_editor::clips::SHORTEST));
                    clip["offset"] = json!(at.offset);
                    takes.push(Taken { lane, clip, how: *how });
                    files.push(f.clone());
                    notes.extend(w.2.clone());
                    if w.1 > 0 {
                        notes.push(take_audio::TOO_SLOW.into());
                    }
                }
                Part::Audio { .. } => unreachable!("written or gone above"),
            }
        }
        match self.place_takes(e.token, files, takes) {
            (Ok(v), _) => {
                let revision = v["revision"].as_u64().unwrap_or(0);
                if let Err(err) = self.name_step(e.token, revision, format!("recording on {label}"), false) {
                    eprintln!("music2: couldn't name the recording's undo step: {err:#}");
                }
                let first = &e.parts[0];
                let mut w = json!({"revision": revision, "made": v["made"], "module": first.module(), "lane": first.lane()});
                if let Some(Part::Audio { file: Audio::Written(f), .. }) = e.parts.iter().find(|p| matches!(p, Part::Audio { .. })) {
                    w["asset"] = json!(f.0.asset.as_str());
                }
                notes.dedup();
                if !notes.is_empty() {
                    w["note"] = json!(notes.join(" "));
                }
                Ok(json!({"t": "take", "written": w}))
            }
            (Err(_), Some(Refused::Draft)) => Err(e),
            (Err(_), Some(Refused::Missing)) => Ok(lost(&format!("{label} is gone."))),
            (Err(_), Some(Refused::ReadOnly)) => Ok(lost("this song is open for writing somewhere else.")),
            (Err(err), _) => Ok(lost(&format!("{err:#}"))),
        }
    }

    /// Choose take `take` (from 0) of an audio clip whose file is a take over a loop (MON-11): its
    /// offset moves to the same place in that pass. One Tx, one undo step, "Vox take 2".
    pub fn choose_take(&self, token: u64, clip: m2_editor::clips::ClipRef, take: usize) -> Result<Value> {
        let (dir, label) = {
            let cp = self.inner.control.lock();
            let p = cp.open.as_ref().context("no project is open")?;
            (p.shown.dir.clone(), label_of(&cp, clip.lane.module.as_str()))
        };
        let found = {
            let cp = self.inner.control.lock();
            let p = cp.open.as_ref().context("no project is open")?;
            let rev = &p.engine.accepted;
            let Some(m2_song::LaneData::Audio(clips)) =
                rev.compiled.data.lane(clip.lane.module.as_str(), clip.lane.lane.as_str()).map(|d| (**d).clone())
            else {
                bail!("{label} has no audio lane `{}`.", clip.lane.lane);
            };
            let found = match (&clip.id, clip.at) {
                (Some(id), _) => clips.iter().find(|c| c.id.as_deref() == Some(id.as_str())),
                (None, Some((start, rank))) => clips.iter().filter(|c| c.start == start).nth(rank as usize),
                _ => None,
            };
            found.cloned().context("That clip isn't there any more.")?
        };
        let takes = m2_editor::take::takes_of(&dir, &found.asset);
        if takes.len() < 2 {
            bail!("This clip's file has no takes to choose from.");
        }
        let Some(to) = takes.get(take) else { bail!("Its file has {} takes: choose 1 to {}.", takes.len(), takes.len()) };
        let now = takes.iter().rposition(|t| *t <= found.offset + 1e-6).unwrap_or(0);
        let offset = (found.offset - takes[now] + to).max(0.0);
        let gesture = m2_editor::clips::Gesture::Audio {
            clip,
            gain_db: None,
            fade_in: None,
            fade_out: None,
            reverse: None,
            follow: None,
            offset: Some(offset),
        };
        let (done, _) = self.clips_why(token, gesture, None);
        let v = done?;
        let revision = v["revision"].as_u64().unwrap_or(0);
        if let Err(err) = self.name_step(token, revision, format!("{label} take {}", take + 1), false) {
            eprintln!("music2: couldn't name the take's undo step: {err:#}");
        }
        Ok(v)
    }

    /// An audio take's writers move nothing, or move again (tests).
    #[cfg(test)]
    pub(super) fn record_pause(&self, on: bool) {
        let cp = self.inner.control.lock();
        for w in cp.record.take.iter().flat_map(|t| &t.audio).filter_map(|a| a.writer.as_ref()) {
            w.pause(on);
        }
    }

    /// The disk has room for `bytes` more of each audio take's file (tests).
    #[cfg(test)]
    pub(super) fn record_leave_room(&self, bytes: u64) {
        let cp = self.inner.control.lock();
        for w in cp.record.take.iter().flat_map(|t| &t.audio).filter_map(|a| a.writer.as_ref()) {
            w.leave_room(bytes);
        }
    }

    /// Wait until each audio take's writer has moved everything its ring holds (tests).
    #[cfg(test)]
    pub(super) fn record_settle(&self) {
        let cp = self.inner.control.lock();
        for a in cp.record.take.iter().flat_map(|t| &t.audio) {
            if let Some(w) = &a.writer {
                w.settle(&a.ring);
            }
        }
    }
}

impl Part {
    fn module(&self) -> &str {
        match self {
            Part::Notes { module, .. } | Part::Audio { module, .. } => module,
        }
    }

    fn lane(&self) -> &str {
        match self {
            Part::Notes { lane, .. } | Part::Audio { lane, .. } => lane,
        }
    }
}

/// "1–2, 3–4" for an input of 4 channels; "1" for a mono one.
fn inputs_said(channels: u16) -> String {
    match channels {
        0 | 1 => "only 1".into(),
        2 => "1–2".into(),
        n => format!("1 to {n}"),
    }
}

/// What the windows hear when takes wait for the code to compile, once: "The recording waits for the
/// code. Fix the code, and it goes on Bass."
fn waits(cp: &mut ControlPlane) -> Option<Value> {
    let first = cp.record.ended.first()?;
    if cp.record.waiting {
        return None;
    }
    let label = label_of(cp, first.parts.first().map_or("", Part::module));
    cp.record.waiting = true;
    Some(json!({"t": "take", "waiting": format!("The recording waits for the code. Fix the code, and it goes on {label}.")}))
}

/// The state's `arm`: each armed track's lane, each track's channels chosen.
fn arm_state(r: &Recorder, token: u64) -> Value {
    let a = &r.arms;
    if a.token != token {
        return json!({"armed": {}, "channels": {}});
    }
    let armed: serde_json::Map<String, Value> = a.armed.iter().map(|(m, (l, _))| (m.clone(), json!(l))).collect();
    let channels: serde_json::Map<String, Value> = a.channels.iter().map(|(m, c)| (m.clone(), json!(c.name()))).collect();
    json!({"armed": armed, "channels": channels})
}

impl Inner {
    /// The state's `record`: the take as the windows see it, or null.
    pub(super) fn record_state(&self) -> Value {
        let device = self.current_device();
        let mut cp = self.control.lock();
        let token = cp.open.as_ref().map(|p| p.engine.token);
        let hear = cp.record.arms.hear;
        match cp.record.take.as_mut().filter(|t| Some(t.token) == token) {
            Some(t) => {
                let now = device.as_ref().filter(|d| d.epoch == t.epoch).and_then(|d| {
                    t.resolve(&d.player);
                    let (pass, frame) = d.player.heard_frame().and_then(|h| t.place(h))?;
                    (pass == t.pass).then(|| t.beat(frame))
                });
                let mut v = t.shown(now);
                v["hear"] = json!(hear && !t.audio.is_empty());
                v
            }
            None => Value::Null,
        }
    }

    /// The state's `arm` (MON-9).
    pub(super) fn arm_state(&self) -> Value {
        let cp = self.control.lock();
        let token = cp.open.as_ref().map_or(0, |p| p.engine.token);
        arm_state(&cp.record, token)
    }
}

#[cfg(test)]
#[path = "record_tests.rs"]
mod tests;
