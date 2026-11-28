//! Feeds (FEED-1 to FEED-8, the live note's Part 1): a track's notes written by an agent ahead of
//! the playhead, as a video player buffers ahead.
//!
//! - **What it holds:** each open feed's notes, in song beats, and the stretches of beats written
//!   ([`Feed::written`]), in memory only, on the control plane. Nothing of it is a file, a revision
//!   or an undo step (FEED-3).
//! - **The way in:** a put replaces the notes that start in its range and past the lock
//!   ([`Now::late`]); the feed's notes are built off the audio thread and sent to the lane's
//!   scheduler as its feed half (`LaneSwap::Feed`, FEED-2), under the control plane's lock, so a
//!   put queues behind the plan its slot names, and never waits for the session (RENDER-29). Each
//!   settle sends them again to a new plan, or when the tempo they were timed in changed.
//! - **What played:** each tick, the notes the playhead passed since the last are kept for Keep, a
//!   beat played again (a loop) keeping what played there last ([`Feed::passed`]), for 30 minutes.
//! - **What it says:** `feed` events with `why` `low`, `empty` and `jump` (and `open`, `closed`),
//!   and the state every quarter second while playing ([`Graph::feeds_tick`], every 50 ms on the
//!   server's `music2-feeds` thread).
//! - **Keep** writes what played as one Tx, through the session: a clip, or a version of the clip
//!   under it (FEED-7).

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use m2_editor::clips::{Act, ClipRef, Gesture, How, NewVersion, Taken};
use m2_runtime::{FEED, LaneSwap, NotesData};
use m2_song::{Arrangement, LaneAddr, LaneData, LaneKind, Note, TempoMap};
use serde::Deserialize;
use serde_json::{Value, json};

use super::lease::PlaysView;
use super::{ControlPlane, Device, Graph};

/// The low mark a feed opens with, in bars.
pub const LOW_BARS: f64 = 4.0;
/// How far past the playhead a put may write, in bars (FEED-4).
pub const AHEAD_BARS: f64 = 64.0;
/// Notes that end this many bars behind the playhead leave the feed (FEED-4).
pub const BEHIND_BARS: f64 = 16.0;
/// The lock's least margin past the playhead, in seconds; else two of the device's buffers.
pub const MARGIN: f64 = 0.050;
/// How long what played is kept for Keep (FEED-7).
pub const KEPT_FOR: Duration = Duration::from_secs(30 * 60);
/// How often the state is told while playing, with no `why` (FEED-5).
pub const EVERY: Duration = Duration::from_millis(250);
/// How often the server looks at the feeds.
pub const TICK: Duration = Duration::from_millis(50);
/// The most notes a feed holds (FEED-4): a put that would make it more is refused.
pub const MAX_NOTES: usize = 4096;
/// The most notes what played keeps, the newest (FEED-7).
pub const MAX_PLAYED: usize = 65_536;
/// Notes behind leave the feed in steps of this many bars, so it isn't sent again at every look.
const DROP_STEP_BARS: f64 = 4.0;
/// Beats closer than this are one.
const EPS: f64 = 1e-9;

/// `POST /api/feed/open`: a feed on `track`'s notes lane (`lane` when it has several), with its
/// low mark in bars.
#[derive(Clone, Debug, Deserialize)]
pub struct Open {
    pub track: String,
    #[serde(default)]
    pub lane: Option<String>,
    #[serde(default)]
    pub low_bars: Option<f64>,
}

/// `POST /api/feed/put`: the notes of `feed` from beat `from` to beat `to` are these, each a lane
/// file's note with `start` in song beats (FEED-4).
#[derive(Clone, Debug, Deserialize)]
pub struct Put {
    pub feed: String,
    pub from: f64,
    pub to: f64,
    #[serde(default)]
    pub notes: Vec<Note>,
}

/// How Keep writes what played (FEED-7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum As {
    Clip,
    Version,
}

/// The open song's feeds, on the control plane.
#[derive(Default)]
pub(super) struct Feeds {
    open: BTreeMap<String, Feed>,
    /// What played on each feed closed since the song opened, for Keep.
    closed: BTreeMap<String, History>,
}

/// One feed: a track's notes lane opened for live writing.
struct Feed {
    lane: String,
    /// The agent that opened it or wrote to it last; none while the musician opened it and no
    /// agent wrote yet.
    writer: Option<String>,
    low_bars: f64,
    /// Its notes, by start, then pitch.
    notes: Vec<Note>,
    /// The stretches of beats puts wrote, apart and in order.
    written: Vec<(f64, f64)>,
    played: History,
    empties: u64,
    late: u64,
    /// Said: under the low mark, empty.
    low: bool,
    empty: bool,
    /// Where what played is kept to: the playhead at the last look, while playing.
    cursor: Option<f64>,
    /// The transport jumped since the last look: said at the next.
    jumped: bool,
    /// When the state was last told.
    told: Option<Instant>,
    /// Where its notes were last sent: the player, the plan, the tempo they were timed in, and the
    /// message.
    sent: Sent,
    /// Why it plays nothing though it was sent: too dense for the tempo it was timed in (FEED-2).
    silent: Option<String>,
}

/// Where a feed's notes were sent: the player, the plan, the tempo they were timed in and the
/// message; None when its lane plays nowhere.
type Sent = Option<(u64, u64, TempoMap, u64)>;

/// A note that played, when, and its place in the order notes came (what goes first past the cap).
#[derive(Clone, Debug)]
struct Played {
    note: Note,
    at: Instant,
    n: u64,
}

/// What played on a feed (FEED-7): by start (bits of a beat 0 or more, which order as the beats
/// do), so a stretch the playhead passes again replaces what played there in a range, never a scan;
/// and in the order it came, so the oldest go first past [`MAX_PLAYED`] or [`KEPT_FOR`].
#[derive(Default)]
struct History {
    by_start: BTreeMap<u64, Vec<Played>>,
    /// Each note's `n` and start, in the order they came: an entry already replaced is passed over.
    order: VecDeque<(u64, u64)>,
    len: usize,
    next: u64,
}

impl History {
    /// What played from `[from, to)` before goes; `notes` (by start) that start there played now.
    fn passed(&mut self, notes: &[Note], from: f64, to: f64, at: Instant) {
        // A stretch of no beats, or one that isn't a number, passed nothing.
        if to.partial_cmp(&from) != Some(std::cmp::Ordering::Greater) {
            return;
        }
        let (a, b) = (from.max(0.0).to_bits(), to.max(0.0).to_bits());
        let mut mid = self.by_start.split_off(&a);
        let mut after = mid.split_off(&b);
        self.len -= mid.values().map(Vec::len).sum::<usize>();
        self.by_start.append(&mut after);
        let lo = notes.partition_point(|n| n.start < from);
        for note in notes[lo..].iter().take_while(|n| n.start < to) {
            self.next += 1;
            let key = note.start.max(0.0).to_bits();
            self.by_start.entry(key).or_default().push(Played { note: note.clone(), at, n: self.next });
            self.order.push_back((self.next, key));
            self.len += 1;
        }
        self.trim(at);
    }

    /// The oldest go while more than [`MAX_PLAYED`] are kept, or they played more than
    /// [`KEPT_FOR`] ago.
    fn trim(&mut self, now: Instant) {
        while let Some(&(n, key)) = self.order.front() {
            let found = self.by_start.get(&key).and_then(|v| v.iter().position(|p| p.n == n));
            let Some(i) = found else {
                self.order.pop_front();
                continue;
            };
            let old = now.saturating_duration_since(self.by_start[&key][i].at) > KEPT_FOR;
            if !old && self.len <= MAX_PLAYED {
                break;
            }
            self.remove_at(key, i);
            self.order.pop_front();
        }
        // Entries replaced since are passed over at the front; past twice what is kept, the order
        // is made again from what is.
        if self.order.len() > 2 * MAX_PLAYED.max(self.len) {
            let mut all: Vec<(u64, u64)> = self.by_start.iter().flat_map(|(k, v)| v.iter().map(move |p| (p.n, *k))).collect();
            all.sort_unstable();
            self.order = all.into();
        }
    }

    fn remove_at(&mut self, key: u64, i: usize) {
        if let Some(v) = self.by_start.get_mut(&key) {
            v.remove(i);
            self.len -= 1;
            if v.is_empty() {
                self.by_start.remove(&key);
            }
        }
    }

    /// What played, by start, each with its `n`.
    fn all(&self) -> Vec<(Note, u64)> {
        self.by_start.values().flatten().map(|p| (p.note.clone(), p.n)).collect()
    }

    /// Take out the notes `ns` names (Keep kept them).
    fn take_out(&mut self, ns: &[u64]) {
        let keys: Vec<(u64, usize)> = self
            .by_start
            .iter()
            .flat_map(|(k, v)| v.iter().enumerate().filter(|(_, p)| ns.contains(&p.n)).map(move |(i, _)| (*k, i)))
            .collect();
        for (k, i) in keys.into_iter().rev() {
            self.remove_at(k, i);
        }
    }

    /// Each note sounding at `beat` ends there: a close cut it.
    fn cut_at(&mut self, beat: f64) {
        for p in self.by_start.values_mut().flatten() {
            if p.note.start <= beat && p.note.start + p.note.length > beat {
                p.note.length = beat - p.note.start;
            }
        }
        let empty: Vec<(u64, usize)> = self
            .by_start
            .iter()
            .flat_map(|(k, v)| v.iter().enumerate().filter(|(_, p)| p.note.length <= 0.0).map(move |(i, _)| (*k, i)))
            .collect();
        for (k, i) in empty.into_iter().rev() {
            self.remove_at(k, i);
        }
    }

    /// `other`'s notes too (a feed closed, then opened again).
    fn join(&mut self, other: History) {
        for p in other.by_start.into_values().flatten() {
            self.next += 1;
            let key = p.note.start.max(0.0).to_bits();
            self.order.push_back((self.next, key));
            self.by_start.entry(key).or_default().push(Played { n: self.next, ..p });
            self.len += 1;
        }
    }
}

/// What a put makes of a feed, before anything changes (FEED-4): its notes, what it took and left
/// out, and what to send.
struct Planned {
    notes: Vec<Note>,
    taken: usize,
    late: usize,
    data: NotesData,
}

/// The transport as a feed reads it at one moment.
pub(super) struct Now {
    playing: bool,
    beat: f64,
    /// The loop's range, while it is on.
    looping: Option<(f64, f64)>,
    /// The lock's margin, in seconds.
    margin: f64,
    tempo: TempoMap,
    arrangement: Arc<Arrangement>,
}

impl Now {
    /// The beats of a bar at `beat`.
    fn bar(&self, beat: f64) -> f64 {
        self.arrangement.meter_at(beat.max(0.0)).bar()
    }

    /// The first beat a put can still change: the playhead plus the margin, while playing.
    fn lock(&self) -> f64 {
        match self.playing {
            true => self.tempo.beat_at(self.tempo.seconds_at(self.beat) + self.margin),
            false => self.beat,
        }
    }

    /// The loop's range, when the playhead is inside it.
    fn lap(&self) -> Option<(f64, f64)> {
        self.looping.filter(|&(s, e)| self.beat >= s && self.beat < e)
    }

    /// Whether a note starting at `x` is too late to take: before the lock while playing; inside a
    /// loop only what the lock covers, across its wrap, as the rest plays next time round.
    fn late(&self, x: f64) -> bool {
        if !self.playing {
            return false;
        }
        let lock = self.lock();
        match self.lap() {
            Some((s, e)) => (x >= self.beat - EPS && x < lock.min(e)) || (lock > e && x >= s && x < s + (lock - e)),
            None => x < lock,
        }
    }

    /// Whether note `n` sounds now.
    fn sounding(&self, n: &Note) -> bool {
        self.playing && n.start <= self.beat && n.start + n.length > self.beat
    }

    /// Whether note `n` ended more than [`BEHIND_BARS`] behind the playhead, and isn't inside the
    /// loop, which plays it again.
    fn behind(&self, n: &Note) -> bool {
        let inside = self.looping.is_some_and(|(s, e)| n.start >= s && n.start < e);
        !inside && n.start + n.length < self.beat - BEHIND_BARS * self.bar(self.beat)
    }
}

impl Feed {
    fn new(lane: String, writer: Option<String>, low_bars: f64) -> Feed {
        Feed {
            lane,
            writer,
            low_bars,
            notes: vec![],
            written: vec![],
            played: History::default(),
            empties: 0,
            late: 0,
            low: false,
            empty: false,
            cursor: None,
            jumped: false,
            told: None,
            sent: None,
            silent: None,
        }
    }

    /// The end of the written stretch `x` is in, or `x` when it is in none.
    fn stretch_end(&self, x: f64) -> f64 {
        self.written.iter().find(|&&(a, b)| a <= x + EPS && x < b - EPS).map_or(x, |&(_, b)| b)
    }

    /// Written to, the buffer in beats, and the low mark in beats, at `now`. Inside a loop the
    /// buffer goes round it: past its end, what is written from its start counts, up to a lap, and
    /// a loop shorter than the low mark is low only when it isn't all written.
    fn ahead(&self, now: &Now) -> (f64, f64, f64) {
        let low = self.low_bars * now.bar(now.beat);
        let w = self.stretch_end(now.beat);
        match now.lap() {
            Some((s, e)) if w >= e - EPS => (w, (e - now.beat) + (self.stretch_end(s).min(now.beat) - s).max(0.0), low.min(e - s)),
            Some((s, e)) => (w, w - now.beat, low.min(e - s)),
            None => (w, w - now.beat, low),
        }
    }

    /// What `p` would make of the feed at `now`, its notes timed by `timed` (FEED-4). Changes
    /// nothing: [`Feed::take`] makes it so once its notes reached the engine.
    fn plan(&self, p: &Put, now: &Now, timed: (&TempoMap, f64)) -> Result<Planned> {
        if !(p.from.is_finite() && p.to.is_finite()) || p.from < 0.0 || p.to <= p.from {
            bail!("A put's `from` and `to` are beats, `from` 0 or more and `to` after it: not {} to {}.", p.from, p.to);
        }
        let bar = now.bar(now.beat);
        let far = now.beat + AHEAD_BARS * bar;
        if p.to > far + EPS {
            bail!(
                "A feed holds at most {AHEAD_BARS} bars past the playhead (beat {:.2}): `to` is beat {}, {:.1} bars past it. A feed is a buffer, not a song: write the next few bars, and more as they play.",
                now.beat,
                p.to,
                (p.to - now.beat) / bar
            );
        }
        for n in &p.notes {
            if !(n.start.is_finite() && n.length.is_finite()) || n.start < p.from || n.start >= p.to {
                bail!("Each note starts inside the put's range, from beat {} to before {}: a note starts at {}.", p.from, p.to, n.start);
            }
            if n.length <= 0.0 {
                bail!("A note's length is above 0 beats: the note at {} is {} long.", n.start, n.length);
            }
            if !(0.0..=127.0).contains(&n.pitch) {
                bail!("A note's pitch is 0 to 127: the note at {} is {}.", n.start, n.pitch);
            }
            if !(0.0..=1.0).contains(&n.velocity) {
                bail!("A note's velocity is 0 to 1: the note at {} is {}.", n.start, n.velocity);
            }
        }
        // What stays: notes outside the range, those it can no longer change, and those sounding.
        let mut notes: Vec<Note> =
            self.notes.iter().filter(|n| n.start < p.from || n.start >= p.to || now.late(n.start) || now.sounding(n)).cloned().collect();
        let (mut taken, mut late) = (0, 0);
        for n in &p.notes {
            // A note sounding that the put would replace stays, and the put's note there with it.
            let held = notes.iter().any(|k| now.sounding(k) && k.pitch == n.pitch && k.start == n.start);
            if now.late(n.start) || held {
                late += 1;
            } else {
                notes.push(n.clone());
                taken += 1;
            }
        }
        notes.retain(|n| !now.behind(n));
        if notes.len() > MAX_NOTES {
            bail!(
                "A feed holds at most 4,096 notes: this put would make it {}. Write fewer, or let the bars behind play out first.",
                notes.len()
            );
        }
        notes.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.total_cmp(&b.pitch)));
        let data = NotesData::feed(&notes, timed.0, timed.1);
        if !FEED.fits(data.needs()) {
            bail!(
                "That's more notes at once than a feed plays: at most {} sounding together, and {} starting within 128 samples. Write fewer.",
                FEED.sounding,
                FEED.ons
            );
        }
        Ok(Planned { notes, taken, late, data })
    }

    /// Make `p`'s plan so: its notes reached the engine (or its lane plays nowhere).
    fn take(&mut self, p: &Put, planned: Planned) {
        self.notes = planned.notes;
        self.write(p.from, p.to);
        self.late += planned.late as u64;
    }

    /// Add the stretch `[from, to)` to what is written, joined with those it touches.
    fn write(&mut self, from: f64, to: f64) {
        let (mut a, mut b) = (from, to);
        self.written.retain(|&(x, y)| {
            let touches = x <= b + EPS && a <= y + EPS;
            if touches {
                (a, b) = (a.min(x), b.max(y));
            }
            !touches
        });
        self.written.push((a, b));
        self.written.sort_by(|x, y| x.0.total_cmp(&y.0));
    }

    /// The notes whose start the playhead passed in `[from, to)`, kept as what played: what played
    /// there before (an earlier time round a loop) goes.
    fn passed(&mut self, from: f64, to: f64, at: Instant) {
        self.played.passed(&self.notes, from, to, at);
    }

    /// What played, by start (tests).
    #[cfg(test)]
    fn played_notes(&self) -> Vec<Note> {
        self.played.all().into_iter().map(|(n, _)| n).collect()
    }

    /// Look at the feed at `now`: what played since the last look, and what to say (FEED-5).
    /// Returns each `why` to tell (None: the state, every quarter second while playing), and
    /// whether notes left it, so it must be sent again.
    fn look(&mut self, now: &Now, at: Instant) -> (Vec<Option<&'static str>>, bool) {
        match (now.playing, self.cursor) {
            (true, None) => self.cursor = Some(now.beat),
            (true, Some(c)) if now.beat >= c - EPS => {
                self.passed(c, now.beat, at);
                self.cursor = Some(now.beat);
            }
            (true, Some(c)) => {
                // Back: a loop's wrap (or a seek this didn't hear of).
                if let Some((s, e)) = now.looping.filter(|&(s, e)| c < e + EPS && now.beat >= s) {
                    self.passed(c, e, at);
                    self.passed(s, now.beat, at);
                }
                self.cursor = Some(now.beat);
                self.jumped = true;
            }
            (false, _) => self.cursor = None,
        }
        let mut whys = vec![];
        if std::mem::take(&mut self.jumped) {
            whys.push(Some("jump"));
        }
        if now.playing {
            let (_, buffer, low) = self.ahead(now);
            match buffer < low - EPS {
                true if !self.low => {
                    self.low = true;
                    whys.push(Some("low"));
                }
                true => {}
                false => self.low = false,
            }
            match buffer <= EPS {
                true if !self.empty => {
                    self.empty = true;
                    self.empties += 1;
                    whys.push(Some("empty"));
                }
                true => {}
                false => self.empty = false,
            }
            if whys.is_empty() && self.told.is_none_or(|t| at.saturating_duration_since(t) >= EVERY) {
                whys.push(None);
            }
            if !whys.is_empty() {
                self.told = Some(at);
            }
        }
        self.played.trim(at);
        // Behind in steps: once the oldest is DROP_STEP_BARS past the line, every note past it goes.
        let before = self.notes.len();
        let step = DROP_STEP_BARS * now.bar(now.beat);
        if self.notes.iter().any(|n| now.behind(&Note { length: n.length + step, ..n.clone() })) {
            self.notes.retain(|n| !now.behind(n));
        }
        // Stretches wholly behind go too.
        let gone = now.beat - BEHIND_BARS * now.bar(now.beat);
        self.written.retain(|&(_, b)| b >= gone || now.looping.is_some());
        (whys, self.notes.len() != before)
    }

    /// The feed's state as the event and `GET /api/feed` say it (FEED-5).
    fn shown(&self, key: &str, now: &Now, why: Option<&str>, token: u64) -> Value {
        let (written_to, beats, low) = self.ahead(now);
        let bar = now.bar(now.beat);
        let seconds = now.tempo.seconds_at(now.beat + beats) - now.tempo.seconds_at(now.beat);
        let mut v = json!({
            "t": "feed", "session": token, "feed": key, "track": key, "lane": self.lane, "writer": self.writer,
            "playing": now.playing, "playhead": now.beat, "lock": now.lock(), "written_to": written_to,
            "buffer": {"beats": beats, "bars": beats / bar, "seconds": seconds},
            "low_bars": self.low_bars, "low_mark": {"beats": low, "bars": low / bar},
            "notes": self.notes.len(), "empties": self.empties, "late": self.late,
            "heard": self.sent.is_some() && self.silent.is_none(),
        });
        if let Some(w) = &self.silent {
            v["silent"] = json!(w);
        }
        if let Some(w) = why {
            v["why"] = json!(w);
        }
        v
    }
}

/// What plays in `device`, if it is the player `plays` was sent to.
fn playing<'a>(plays: Option<&'a PlaysView>, device: Option<&'a Device>) -> Option<(&'a PlaysView, &'a Device)> {
    let (p, d) = (plays?, device?);
    (p.epoch == d.epoch).then_some((p, d))
}

/// Lane `(module, lane)`'s slot in what plays, if it is a notes lane there.
fn slot_of(plays: &PlaysView, module: &str, lane: &str) -> Option<u32> {
    let at = plays.info.lanes.iter().position(|l| l.lane.module == module && l.lane.lane == lane && l.caps.is_some())?;
    Some(at as u32)
}

/// Send `notes` (or `data`, built from them) as feed `key`'s, on lane `lane`, to what plays, timed
/// in the tempo of what plays (FEED-2): where it went, and why it plays nothing when the notes are
/// too dense for that tempo (they are sent empty then). Changes no feed: the caller does, once this
/// went (FEED-4).
fn deliver(
    engine: &super::EngineView,
    device: Option<&Device>,
    key: &str,
    lane: &str,
    notes: &[Note],
    data: Option<NotesData>,
) -> Result<(Sent, Option<String>)> {
    let Some((p, d)) = playing(engine.plays.as_ref(), device) else { return Ok((None, None)) };
    let Some(slot) = slot_of(p, key, lane) else { return Ok((None, None)) };
    let tempo = (*p.timing).clone();
    let mut data = data.unwrap_or_else(|| NotesData::feed(notes, &tempo, engine.accepted.project.sample_rate() as f64));
    let mut silent = None;
    if !FEED.fits(data.needs()) {
        data = NotesData::empty();
        silent = Some(format!(
            "too many notes at once at this tempo: a feed plays at most {} sounding together and {} starting within 128 samples. Write them again, fewer.",
            FEED.sounding, FEED.ons
        ));
    }
    let seq = d.player.swap_lane(p.generation, slot, LaneSwap::Feed(Arc::new(data)))?;
    Ok((Some((p.epoch, p.generation, tempo, seq)), silent))
}

/// Send feed `key`'s notes again, as it holds them.
fn resend(cp: &mut ControlPlane, key: &str, device: Option<&Device>) -> Result<()> {
    let Some(engine) = cp.open.as_ref().map(|p| p.engine.clone()) else { return Ok(()) };
    let Some(f) = cp.feeds.open.get_mut(key) else { return Ok(()) };
    let (sent, silent) = deliver(&engine, device, key, &f.lane, &f.notes, None)?;
    (f.sent, f.silent) = (sent, silent);
    Ok(())
}

/// Carry every feed to what a settle just published (its first phase, under the control plane's
/// lock, after its own sends): a new plan, or a new tempo, gets the feed's notes again, so they
/// play from its first block or the one after (the queue is FIFO). Until then the scheduler that
/// moved into it plays them as they were (FEED-2).
pub(super) fn settled(cp: &mut ControlPlane, token: u64, device: Option<&Device>) {
    let Some(engine) = cp.open.as_ref().filter(|p| p.engine.token == token).map(|p| p.engine.clone()) else { return };
    let keys: Vec<String> = cp.feeds.open.keys().cloned().collect();
    for key in keys {
        let fresh = match (playing(engine.plays.as_ref(), device), &cp.feeds.open[&key].sent) {
            (Some((p, _)), Some((epoch, generation, tempo, _))) => (p.epoch, p.generation) != (*epoch, *generation) || *p.timing != *tempo,
            (Some(_), None) => true,
            (None, _) => false,
        };
        if fresh && let Err(e) = resend(cp, &key, device) {
            eprintln!("music2: couldn't carry the {key} feed to the new plan: {e:#}");
        }
    }
}

/// The song closes, or another opens: its feeds go, and what played on them.
pub(super) fn end_all(cp: &mut ControlPlane) {
    cp.feeds = Feeds::default();
}

/// The feeds, for the graph state (the page's Live marks, FEED-1).
pub(super) fn listed(cp: &ControlPlane) -> Value {
    let feeds: Vec<Value> = cp
        .feeds
        .open
        .iter()
        .map(|(k, f)| json!({"feed": k, "track": k, "lane": f.lane, "writer": f.writer, "low_bars": f.low_bars}))
        .collect();
    json!(feeds)
}

impl Graph {
    /// The transport as feeds read it now, with the open song's token. Never waits for the session.
    fn feed_now(&self, cp: &ControlPlane) -> Option<(Now, u64)> {
        let open = cp.open.as_ref()?;
        let status = self.inner.status();
        let device = self.inner.current_device();
        let margin =
            device.as_ref().and_then(|d| Some(2.0 * d.player.frames()? as f64 / d.player.device_rate().max(1) as f64)).unwrap_or(0.0);
        let tempo = self.inner.timing.lock().as_ref().map(|t| t.tempo.clone()).unwrap_or_else(|| open.engine.accepted.tempo());
        let looping = status.looping.filter(|l| l.on).map(|l| (l.start, l.end));
        let now = Now {
            playing: status.playing,
            beat: status.beat,
            looping,
            margin: margin.max(MARGIN),
            tempo,
            arrangement: open.engine.accepted.compiled.data.arrangement.clone(),
        };
        Some((now, open.engine.token))
    }

    /// Who asks: an agent's name, none for the page.
    fn asker() -> Option<String> {
        match crate::caller::now() {
            crate::caller::Caller::Agent { name, .. } => Some(name),
            crate::caller::Caller::Page => None,
        }
    }

    /// Open a feed on `o.track` (FEED-1): its notes lane, the one `o.lane` names when it has more.
    /// Answers its state. Never waits for the session.
    pub fn feed_open(&self, o: Open) -> Result<Value> {
        let low_bars = o.low_bars.unwrap_or(LOW_BARS);
        if !(low_bars.is_finite() && low_bars > 0.0 && low_bars <= AHEAD_BARS) {
            bail!("The low mark is a number of bars above 0, at most {AHEAD_BARS}: not {low_bars}.");
        }
        let writer = Graph::asker();
        let mut cp = self.inner.control.lock();
        let open = cp.open.as_ref().context("no project is open")?;
        // A track with a header, where the page shows its Live mark (FEED-1).
        let headed =
            open.shown.timeline.tracks.iter().any(|t| t.module == o.track && t.head.get("track").and_then(Value::as_bool) == Some(true));
        if o.track == "song" || (!headed && open.shown.timeline.tracks.iter().any(|t| t.module == o.track)) {
            bail!("{} isn't a track: a feed plays a track's notes, which shows Live on its header.", o.track);
        }
        let rev = open.engine.accepted.clone();
        let module = rev
            .compiled
            .ir
            .modules
            .iter()
            .find(|m| m.key == o.track.as_str())
            .ok_or_else(|| anyhow!("The song has no track `{}`: `m2 check` lists its tracks and their lanes.", o.track))?;
        let notes: Vec<String> =
            module.lanes.iter().filter(|l| m2_runtime::lane_kind(l.kind) == LaneKind::Notes).map(|l| l.key.as_str().to_string()).collect();
        let lane = match (&o.lane, &notes[..]) {
            (_, []) => bail!("{} plays no notes lane: a feed plays a track's notes.", o.track),
            (Some(l), _) if notes.contains(l) => l.clone(),
            (Some(l), _) => bail!("{} has no notes lane `{l}`: it has {}.", o.track, notes.join(", ")),
            (None, [one]) => one.clone(),
            (None, _) => bail!("{} has more than one notes lane: say which, as `lane` ({}).", o.track, notes.join(", ")),
        };
        if cp.feeds.open.contains_key(&o.track) {
            bail!("{} already has a feed open: close it first, or write to it.", o.track);
        }
        let mut f = Feed::new(lane, writer, low_bars);
        // Sent first: a feed that can't be sent isn't opened (FEED-4).
        let device = self.inner.current_device();
        let engine = cp.open.as_ref().context("no project is open")?.engine.clone();
        (f.sent, f.silent) = deliver(&engine, device.as_ref(), &o.track, &f.lane, &f.notes, None)?;
        // What played before it closed is kept for Keep still.
        f.played = cp.feeds.closed.remove(&o.track).unwrap_or_default();
        cp.feeds.open.insert(o.track.clone(), f);
        let (now, token) = self.feed_now(&cp).context("no project is open")?;
        let shown = cp.feeds.open[&o.track].shown(&o.track, &now, Some("open"), token);
        drop(cp);
        self.inner.tell(shown.clone());
        Ok(shown)
    }

    /// Put notes on a feed (FEED-4): answers what it took, what was late, and where the feed
    /// stands. Never waits for the session.
    pub fn feed_put(&self, p: Put) -> Result<Value> {
        let writer = Graph::asker();
        let device = self.inner.current_device();
        let mut cp = self.inner.control.lock();
        let (now, _) = self.feed_now(&cp).context("no project is open")?;
        let engine = cp.open.as_ref().context("no project is open")?.engine.clone();
        // Timed as what plays plays (FEED-2).
        let tempo = playing(engine.plays.as_ref(), device.as_ref())
            .map(|(p, _)| (*p.timing).clone())
            .unwrap_or_else(|| engine.accepted.compiled.data.tempo().clone());
        let rate = engine.accepted.project.sample_rate() as f64;
        let f = cp.feeds.open.get_mut(&p.feed).ok_or_else(|| anyhow!("{} has no feed open: open one first (`m2 feed open`).", p.feed))?;
        let mut planned = f.plan(&p, &now, (&tempo, rate))?;
        let (taken, late) = (planned.taken, planned.late);
        // Sent first: a put that can't be sent changes nothing (FEED-4).
        let data = std::mem::replace(&mut planned.data, NotesData::empty());
        (f.sent, f.silent) = deliver(&engine, device.as_ref(), &p.feed, &f.lane, &planned.notes, Some(data))?;
        f.take(&p, planned);
        if writer.is_some() {
            f.writer = writer;
        }
        let (written_to, ..) = f.ahead(&now);
        let heard = f.sent.is_some() && f.silent.is_none();
        Ok(
            json!({"ok": true, "taken": taken, "late": late, "lock": now.lock(), "written_to": written_to, "playhead": now.beat, "heard": heard}),
        )
    }

    /// Close feed `key` (FEED-1): its notes end at once; what played stays for Keep until the song
    /// closes. Never waits for the session.
    pub fn feed_close(&self, key: &str) -> Result<Value> {
        let device = self.inner.current_device();
        let mut cp = self.inner.control.lock();
        let (now, token) = self.feed_now(&cp).context("no project is open")?;
        if !cp.feeds.open.contains_key(key) {
            bail!("{key} has no feed open.");
        }
        // An empty feed, sent first: its scheduler ends every note of it at the swap (FEED-2); a
        // close that can't be sent changes nothing (FEED-4).
        let engine = cp.open.as_ref().context("no project is open")?.engine.clone();
        let lane = cp.feeds.open[key].lane.clone();
        deliver(&engine, device.as_ref(), key, &lane, &[], None)?;
        let mut f = cp.feeds.open.remove(key).expect("open");
        // What played up to now, the notes sounding cut where they ended.
        if let Some(c) = f.cursor.filter(|_| now.playing) {
            f.passed(c, now.beat, Instant::now());
        }
        if now.playing {
            f.played.cut_at(now.beat);
        }
        f.notes.clear();
        let shown = f.shown(key, &now, Some("closed"), token);
        cp.feeds.closed.entry(key.to_string()).or_default().join(f.played);
        drop(cp);
        self.inner.tell(shown.clone());
        Ok(shown)
    }

    /// Every open feed's state, as the event says it (`GET /api/feed`).
    pub fn feeds(&self) -> Value {
        let cp = self.inner.control.lock();
        let Some((now, token)) = self.feed_now(&cp) else { return json!({"feeds": []}) };
        let feeds: Vec<Value> = cp.feeds.open.iter().map(|(k, f)| f.shown(k, &now, None, token)).collect();
        json!({"session": token, "feeds": feeds})
    }

    /// Look at every feed (the `music2-feeds` thread, every [`TICK`]): keep what played, tell what
    /// changed (FEED-5), and send again a feed whose old notes left it.
    pub fn feeds_tick(&self) {
        let device = self.inner.current_device();
        let mut told = vec![];
        {
            let mut cp = self.inner.control.lock();
            if cp.feeds.open.is_empty() {
                return;
            }
            let Some((now, token)) = self.feed_now(&cp) else { return };
            let at = Instant::now();
            let keys: Vec<String> = cp.feeds.open.keys().cloned().collect();
            for key in keys {
                let f = cp.feeds.open.get_mut(&key).expect("open");
                let (whys, dropped) = f.look(&now, at);
                told.extend(whys.into_iter().map(|w| f.shown(&key, &now, w, token)));
                if dropped && let Err(e) = resend(&mut cp, &key, device.as_ref()) {
                    eprintln!("music2: couldn't send the {key} feed: {e:#}");
                }
            }
        }
        for v in told {
            self.inner.tell(v);
        }
    }

    /// The transport jumps (FEED-5): to `to` (a seek), or stopping (None). What played up to here
    /// is kept first; the next look says `jump`.
    pub(super) fn feeds_jump(&self, to: Option<f64>) {
        let mut cp = self.inner.control.lock();
        if cp.feeds.open.is_empty() {
            return;
        }
        let Some((now, _)) = self.feed_now(&cp) else { return };
        if to.is_none() && !now.playing {
            return;
        }
        let at = Instant::now();
        for f in cp.feeds.open.values_mut() {
            if let Some(c) = f.cursor.filter(|_| now.playing) {
                f.passed(c, now.beat, at);
            }
            f.cursor = to.filter(|_| now.playing);
            f.jumped = true;
        }
    }

    /// Play was pressed: what plays from here on is kept.
    pub(super) fn feeds_play(&self) {
        let mut cp = self.inner.control.lock();
        if cp.feeds.open.is_empty() {
            return;
        }
        let Some((now, _)) = self.feed_now(&cp) else { return };
        for f in cp.feeds.open.values_mut() {
            f.cursor = Some(now.beat);
        }
    }

    /// Keep what played on feed `key` (FEED-7): one Tx, one undo step. Waits for the session, as
    /// every write does.
    pub fn feed_keep(&self, key: &str, how: Option<As>) -> Result<Value> {
        // What played, as it stands now.
        let (token, lane, writer, label, notes, rev) = {
            let cp = self.inner.control.lock();
            let open = cp.open.as_ref().context("no project is open")?;
            let (token, rev) = (open.engine.token, open.engine.accepted.clone());
            let label = super::record::label_of(&cp, key);
            // Each with where it is kept: (0, n) on the open feed, (1, n) on the closed one.
            let (lane, writer, mut played) = match cp.feeds.open.get(key) {
                Some(f) => {
                    (f.lane.clone(), f.writer.clone(), f.played.all().into_iter().map(|(note, n)| (note, (0u8, n))).collect::<Vec<_>>())
                }
                None => {
                    let lane = rev
                        .compiled
                        .ir
                        .modules
                        .iter()
                        .find(|m| m.key == key)
                        .and_then(|m| m.lanes.iter().find(|l| m2_runtime::lane_kind(l.kind) == LaneKind::Notes))
                        .map(|l| l.key.as_str().to_string());
                    (lane.unwrap_or_default(), None, vec![])
                }
            };
            played.extend(cp.feeds.closed.get(key).into_iter().flat_map(|h| h.all()).map(|(note, n)| (note, (1u8, n))));
            played.sort_by(|(a, _), (b, _)| a.start.total_cmp(&b.start).then(a.pitch.total_cmp(&b.pitch)));
            played.dedup_by(|(a, _), (b, _)| a.start == b.start && a.pitch == b.pitch);
            (token, lane, writer, label, played, rev)
        };
        let (notes, ids): (Vec<Note>, Vec<(u8, u64)>) = notes.into_iter().unzip();
        if notes.is_empty() {
            bail!("Nothing has played on the {key} feed yet: Keep keeps what played.");
        }
        let addr = LaneAddr::new(key, &lane).map_err(|e| anyhow!("{e}"))?;
        let arrangement = &rev.compiled.data.arrangement;
        // The notes clip under the first note, with its versions.
        let first = notes[0].start;
        let under = match rev.compiled.data.lane(key, &lane).map(|d| &**d) {
            Some(LaneData::Notes(clips)) => clips.iter().find(|c| c.start <= first + EPS && first < c.start + c.length - EPS).cloned(),
            _ => None,
        };
        let version = match (how, &under) {
            (Some(As::Version), None) => bail!("No notes clip lies under what played (beat {first}): keep it as a clip."),
            (Some(As::Version), Some(c)) if c.id.is_none() => bail!("The clip under what played has no id: keep it as a clip."),
            (Some(As::Version), Some(c)) => Some(c.clone()),
            (None, Some(c)) if !c.versions.is_empty() && c.id.is_some() => Some(c.clone()),
            _ => None,
        };
        // Which of what played it kept.
        let kept: Vec<usize>;
        let answer = match version {
            Some(clip) => {
                let within: Vec<usize> = (0..notes.len())
                    .filter(|&i| notes[i].start >= clip.start - EPS && notes[i].start < clip.start + clip.length - EPS)
                    .collect();
                let inside: Vec<Note> = within
                    .iter()
                    .map(|&i| &notes[i])
                    .map(|n| Note { start: n.start - clip.start, length: n.length.min(clip.start + clip.length - n.start), ..n.clone() })
                    .collect();
                let left_out = notes.len() - inside.len();
                let names: Vec<&str> = clip.versions.iter().map(|v| v.name.as_str()).collect();
                let name = (1..)
                    .map(|n| if n == 1 { "live".to_string() } else { format!("live {n}") })
                    .find(|n| !names.contains(&n.as_str()))
                    .expect("a name");
                let cref = ClipRef { lane: addr.clone(), id: clip.id.clone(), at: None, sha: None };
                let g = Gesture::Versions {
                    clip: cref,
                    act: Act::Add { versions: vec![NewVersion { name: name.clone(), notes: inside.clone() }], play: false },
                    by: None,
                };
                let v = match self.clips_since(token, g, None)? {
                    super::clips::Answer::Done(v) => v,
                    super::clips::Answer::Changed(why) => bail!("{why}"),
                };
                kept = within;
                {
                    // The change's own answer first, as every change's (AGENT-2): revision, step, moved.
                    let mut a = v.clone();
                    for (k, x) in [
                        ("kept", json!(inside.len())),
                        ("as", json!("version")),
                        ("version", json!(name)),
                        ("clip", json!(clip.id)),
                        ("left_out", json!(left_out)),
                    ] {
                        a[k] = x;
                    }
                    a
                }
            }
            None => {
                let start = super::record::bar_before(arrangement, first);
                let last = notes.iter().map(|n| n.start + n.length).fold(first, f64::max);
                let end = super::record::bar_after(arrangement, last).max(start + arrangement.meter_at(start).bar());
                let placed: Vec<Value> = notes
                    .iter()
                    .map(|n| json!({"pitch": n.pitch, "start": n.start - start, "length": n.length, "velocity": n.velocity}))
                    .collect();
                let clip = json!({"start": start, "length": end - start, "notes": placed});
                let v = match self.place_takes(token, vec![], vec![Taken { lane: addr.clone(), clip, how: How::New }]) {
                    (Ok(v), _) => v,
                    (Err(e), _) => return Err(e),
                };
                let revision = v["revision"].as_u64().unwrap_or(0);
                let who = writer.as_deref().unwrap_or("the feed");
                let named = self.name_step(token, revision, format!("what {who} played on {label}"), false);
                if let Err(e) = &named {
                    eprintln!("music2: couldn't name Keep's undo step: {e:#}");
                }
                kept = (0..notes.len()).collect();
                let made = v["made"].get(0).and_then(|m| m["id"].as_str()).map(String::from);
                {
                    let mut a = v.clone();
                    for (k, x) in [
                        ("kept", json!(notes.len())),
                        ("as", json!("clip")),
                        ("clip", json!(made)),
                        ("start", json!(start)),
                        ("length", json!(end - start)),
                    ] {
                        a[k] = x;
                    }
                    a
                }
            }
        };
        // What it kept leaves what played; as a clip, it leaves the feed too, as the clip plays it.
        let clip = answer["as"] == "clip";
        let device = self.inner.current_device();
        let mut cp = self.inner.control.lock();
        let of = |src: u8| -> Vec<u64> { kept.iter().filter(|&&i| ids[i].0 == src).map(|&i| ids[i].1).collect() };
        if let Some(c) = cp.feeds.closed.get_mut(key) {
            c.take_out(&of(1));
        }
        if let Some(f) = cp.feeds.open.get_mut(key) {
            f.played.take_out(&of(0));
            // Only each note as it was kept: one written since at its beat and pitch stays (FEED-7).
            let same = |a: &Note, b: &Note| a.start == b.start && a.pitch == b.pitch && a.length == b.length && a.velocity == b.velocity;
            if clip && f.notes.iter().any(|n| kept.iter().any(|&i| same(n, &notes[i]))) {
                f.notes.retain(|n| !kept.iter().any(|&i| same(n, &notes[i])));
                if let Err(e) = resend(&mut cp, key, device.as_ref()) {
                    eprintln!("music2: couldn't send the {key} feed: {e:#}");
                }
            }
        }
        Ok(answer)
    }

    /// The message that last sent feed `key`'s notes (tests).
    #[cfg(test)]
    pub(super) fn feed_seq(&self, key: &str) -> Option<u64> {
        self.inner.control.lock().feeds.open.get(key)?.sent.as_ref().map(|s| s.3)
    }

    /// The notes feed `key` holds (tests).
    #[cfg(test)]
    pub(super) fn feed_notes(&self, key: &str) -> Vec<Note> {
        self.inner.control.lock().feeds.open.get(key).map(|f| f.notes.clone()).unwrap_or_default()
    }

    /// The agent that wrote to feed `key` last, while it is open: what the page's Live mark names.
    pub fn feed_writer(&self, key: &str) -> Option<Option<String>> {
        Some(self.inner.control.lock().feeds.open.get(key)?.writer.clone())
    }
}

#[cfg(test)]
#[path = "feeds_tests.rs"]
mod tests;
