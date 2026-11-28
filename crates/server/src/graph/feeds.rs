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

use std::collections::BTreeMap;
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
    closed: BTreeMap<String, Vec<Played>>,
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
    played: Vec<Played>,
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
    sent: Option<(u64, u64, TempoMap, u64)>,
}

/// A note that played, and when.
#[derive(Clone, Debug)]
struct Played {
    note: Note,
    at: Instant,
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
            played: vec![],
            empties: 0,
            late: 0,
            low: false,
            empty: false,
            cursor: None,
            jumped: false,
            told: None,
            sent: None,
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

    /// Take `p` at `now`, its notes timed by `timed` (FEED-4): what it took and left out, and the
    /// feed's notes to send. Nothing changes when it is refused.
    fn put(&mut self, p: &Put, now: &Now, timed: (&TempoMap, f64)) -> Result<(usize, usize, NotesData)> {
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
        notes.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.total_cmp(&b.pitch)));
        let data = NotesData::feed(&notes, timed.0, timed.1);
        if !FEED.fits(data.needs()) {
            bail!(
                "That's more notes at once than a feed plays: at most {} sounding together, and {} starting within 128 samples. Write fewer.",
                FEED.sounding,
                FEED.ons
            );
        }
        self.notes = notes;
        self.write(p.from, p.to);
        self.late += late as u64;
        Ok((taken, late, data))
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
        if to <= from {
            return;
        }
        self.played.retain(|p| p.note.start < from || p.note.start >= to);
        self.played.extend(self.notes.iter().filter(|n| n.start >= from && n.start < to).map(|n| Played { note: n.clone(), at }));
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
        self.played.retain(|p| at.saturating_duration_since(p.at) <= KEPT_FOR);
        let before = self.notes.len();
        self.notes.retain(|n| !now.behind(n));
        // Stretches wholly behind go too.
        let gone = now.beat - BEHIND_BARS * now.bar(now.beat);
        self.written.retain(|&(_, b)| b >= gone || now.looping.is_some());
        (whys, self.notes.len() != before)
    }

    /// The feed's state as the event and `GET /api/feed` say it (FEED-5).
    fn shown(&self, key: &str, now: &Now, why: Option<&str>, token: u64) -> Value {
        let (written_to, beats, _) = self.ahead(now);
        let bar = now.bar(now.beat);
        let seconds = now.tempo.seconds_at(now.beat + beats) - now.tempo.seconds_at(now.beat);
        let mut v = json!({
            "t": "feed", "session": token, "feed": key, "track": key, "lane": self.lane, "writer": self.writer,
            "playing": now.playing, "playhead": now.beat, "lock": now.lock(), "written_to": written_to,
            "buffer": {"beats": beats, "bars": beats / bar, "seconds": seconds},
            "low_bars": self.low_bars, "notes": self.notes.len(), "empties": self.empties, "late": self.late,
            "heard": self.sent.is_some(),
        });
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

/// Send feed `key`'s notes to what plays, timed as it plays them: its message, or None when its
/// lane plays nowhere.
fn send(cp: &mut ControlPlane, key: &str, device: Option<&Device>, data: Option<NotesData>) -> Result<Option<u64>> {
    let Some(engine) = cp.open.as_ref().map(|p| p.engine.clone()) else { return Ok(None) };
    let Some(f) = cp.feeds.open.get_mut(key) else { return Ok(None) };
    let Some((p, d)) = playing(engine.plays.as_ref(), device) else {
        f.sent = None;
        return Ok(None);
    };
    let tempo = p.tempo.as_deref().unwrap_or(engine.accepted.compiled.data.tempo()).clone();
    let Some(slot) = slot_of(p, key, &f.lane) else {
        f.sent = None;
        return Ok(None);
    };
    let data = data.unwrap_or_else(|| NotesData::feed(&f.notes, &tempo, engine.accepted.project.sample_rate() as f64));
    let seq = d.player.swap_lane(p.generation, slot, LaneSwap::Feed(Arc::new(data)))?;
    f.sent = Some((p.epoch, p.generation, tempo, seq));
    Ok(Some(seq))
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
            (Some((p, _)), Some((epoch, generation, tempo, _))) => {
                (p.epoch, p.generation) != (*epoch, *generation)
                    || p.tempo.as_deref().unwrap_or(engine.accepted.compiled.data.tempo()) != tempo
            }
            (Some(_), None) => true,
            (None, _) => false,
        };
        if fresh && let Err(e) = send(cp, &key, device, None) {
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
        let rev = open.engine.accepted.clone();
        let module = rev
            .compiled
            .ir
            .modules
            .iter()
            .find(|m| m.key == o.track.as_str())
            .ok_or_else(|| anyhow!("The song has no track `{}`: `m2 timeline` lists its tracks.", o.track))?;
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
        // What played before it closed is kept for Keep still.
        f.played = cp.feeds.closed.remove(&o.track).unwrap_or_default();
        cp.feeds.open.insert(o.track.clone(), f);
        let device = self.inner.current_device();
        send(&mut cp, &o.track, device.as_ref(), None)?;
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
        let tempo = playing(engine.plays.as_ref(), device.as_ref())
            .and_then(|(p, _)| p.tempo.as_deref().cloned())
            .unwrap_or_else(|| engine.accepted.compiled.data.tempo().clone());
        let rate = engine.accepted.project.sample_rate() as f64;
        let f = cp.feeds.open.get_mut(&p.feed).ok_or_else(|| anyhow!("{} has no feed open: open one first (`m2 feed open`).", p.feed))?;
        let (taken, late, data) = f.put(&p, &now, (&tempo, rate))?;
        if writer.is_some() {
            f.writer = writer;
        }
        send(&mut cp, &p.feed, device.as_ref(), Some(data))?;
        let f = &cp.feeds.open[&p.feed];
        let (written_to, ..) = f.ahead(&now);
        Ok(
            json!({"ok": true, "taken": taken, "late": late, "lock": now.lock(), "written_to": written_to, "playhead": now.beat, "heard": f.sent.is_some()}),
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
        // What played up to now first.
        let f = cp.feeds.open.get_mut(key).expect("open");
        if let Some(c) = f.cursor.filter(|_| now.playing) {
            f.passed(c, now.beat, Instant::now());
        }
        // An empty feed: its scheduler ends every note of it at the swap (FEED-2).
        f.notes.clear();
        send(&mut cp, key, device.as_ref(), None)?;
        let mut f = cp.feeds.open.remove(key).expect("open");
        // The notes sounding, cut where they ended.
        for p in &mut f.played {
            if now.sounding(&p.note) {
                p.note.length = now.beat - p.note.start;
            }
        }
        f.played.retain(|p| p.note.length > 0.0);
        let shown = f.shown(key, &now, Some("closed"), token);
        cp.feeds.closed.entry(key.to_string()).or_default().extend(f.played);
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
                if dropped && let Err(e) = send(&mut cp, &key, device.as_ref(), None) {
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
            let at = Instant::now();
            let (lane, writer, mut played) = match cp.feeds.open.get(key) {
                Some(f) => (f.lane.clone(), f.writer.clone(), f.played.clone()),
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
            played.extend(cp.feeds.closed.get(key).into_iter().flatten().cloned());
            played.retain(|p| at.saturating_duration_since(p.at) <= KEPT_FOR);
            let mut notes: Vec<Note> = played.into_iter().map(|p| p.note).collect();
            notes.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.total_cmp(&b.pitch)));
            notes.dedup_by(|a, b| a.start == b.start && a.pitch == b.pitch);
            (token, lane, writer, label, notes, rev)
        };
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
        let kept: Vec<(u64, u64)>;
        let answer = match version {
            Some(clip) => {
                let inside: Vec<Note> = notes
                    .iter()
                    .filter(|n| n.start >= clip.start - EPS && n.start < clip.start + clip.length - EPS)
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
                kept = inside.iter().map(|n| ((n.start + clip.start).to_bits(), n.pitch.to_bits())).collect();
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
                kept = notes.iter().map(|n| (n.start.to_bits(), n.pitch.to_bits())).collect();
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
        let is = |n: &Note| kept.contains(&(n.start.to_bits(), n.pitch.to_bits()));
        if let Some(c) = cp.feeds.closed.get_mut(key) {
            c.retain(|p| !is(&p.note));
        }
        if let Some(f) = cp.feeds.open.get_mut(key) {
            f.played.retain(|p| !is(&p.note));
            if clip {
                f.notes.retain(|n| !is(n));
                if let Err(e) = send(&mut cp, key, device.as_ref(), None) {
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
}

#[cfg(test)]
#[path = "feeds_tests.rs"]
mod tests;
