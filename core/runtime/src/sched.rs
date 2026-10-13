//! Notes-lane schedulers (RENDER-12, RENDER-26a): each block, a scheduler fills its lane's
//! event buffer from the transport, and it owns the notes it has turned on and not yet off.
//!
//! A lane's notes are prepared off the audio thread ([`NotesData`]): in samples, in the order
//! today's renders play them, each named by its `(clip, pitch, start beat)`. A jump ends every
//! sounding note and seeks the cursor; notes already sounding at the target are not chased
//! (RENDER-11). A swap ([`NoteScheduler::swap`]) keeps the sounding notes the new data still
//! has, reschedules their offs, and ends the rest.

use std::sync::Arc;

use m2_song::{ClipId, ControlLane, NoteClip, TempoMap};

use crate::event::{Event, Events};
use crate::transport::{Jump, Run, Runs};

/// The clip a note comes from: its id, or for a clip without a valid one, its start beat.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ClipKey {
    Id([u8; 16], u8),
    Start(u64),
}

impl ClipKey {
    fn of(clip: &NoteClip) -> ClipKey {
        match clip.id.as_deref().filter(|id| ClipId::valid(id)) {
            Some(id) => {
                let mut b = [0; 16];
                b[..id.len()].copy_from_slice(id.as_bytes());
                ClipKey::Id(b, id.len() as u8)
            }
            None => ClipKey::Start(clip.start.to_bits()),
        }
    }
}

/// A note's identity across edits: its clip, pitch and absolute start beat (as bits). Moving a
/// clip moves its notes' start beats, so they end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NoteKey {
    pub clip: ClipKey,
    pub pitch: u64,
    pub start: u64,
}

#[derive(Clone, Copy, Debug)]
struct SchedNote {
    start: u64,
    end: u64,
    end_beat: f64,
    pitch: f32,
    velocity: f32,
    key: NoteKey,
    /// Its place among the notes with the same key (usually 0).
    ord: u16,
}

/// What a scheduler must hold: the most notes sounding at once, and the most that start in any
/// 128 frames.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Caps {
    pub sounding: usize,
    pub ons: usize,
}

impl Caps {
    /// Room every scheduler has at least, so most edits swap in place.
    pub const FLOOR: Caps = Caps { sounding: 256, ons: 128 };

    pub fn max(self, o: Caps) -> Caps {
        Caps { sounding: self.sounding.max(o.sounding), ons: self.ons.max(o.ons) }
    }

    /// Whether data that needs `needs` fits these.
    pub fn fits(&self, needs: Caps) -> bool {
        needs.sounding <= self.sounding && needs.ons <= self.ons
    }

    /// Events one block can hold: an Off for every sounding note, and two runs of Ons, each
    /// with its Off.
    pub fn events(&self) -> usize {
        self.sounding + 4 * self.ons + 8
    }
}

/// A notes lane, ready for the audio thread.
#[derive(Debug, Default)]
pub struct NotesData {
    /// By start, as today's renders order them.
    notes: Vec<SchedNote>,
    /// Indexes of `notes` by key, then end, velocity and index: a total order.
    by_key: Vec<u32>,
    needs: Caps,
}

impl NotesData {
    /// The notes of `clips` at `rate` in `tempo`: the same notes, in the same order and with the
    /// same samples, as today's offline renders (`m2_song::lane::notes`).
    pub fn build(clips: &[NoteClip], tempo: &TempoMap, rate: f64) -> NotesData {
        let at = |beat: f64| (tempo.seconds_at(beat) * rate).round().max(0.0) as u64;
        let notes: Vec<SchedNote> = m2_song::lane::notes_keyed(clips)
            .into_iter()
            .map(|(n, c)| SchedNote {
                start: at(n.start),
                end: at(n.end),
                end_beat: n.end,
                pitch: n.pitch as f32,
                velocity: n.velocity as f32,
                key: NoteKey { clip: ClipKey::of(&clips[c]), pitch: n.pitch.to_bits(), start: n.start.to_bits() },
                ord: 0,
            })
            .filter(|n| n.end > n.start)
            .collect();
        NotesData::from_notes(notes)
    }

    fn from_notes(mut notes: Vec<SchedNote>) -> NotesData {
        let mut by_key: Vec<u32> = (0..notes.len() as u32).collect();
        by_key.sort_unstable_by(|&a, &b| {
            let (x, y) = (&notes[a as usize], &notes[b as usize]);
            x.key.cmp(&y.key).then(x.end.cmp(&y.end)).then(x.velocity.to_bits().cmp(&y.velocity.to_bits())).then(a.cmp(&b))
        });
        for k in 1..by_key.len() {
            let (prev, i) = (by_key[k - 1] as usize, by_key[k] as usize);
            if notes[prev].key == notes[i].key {
                notes[i].ord = notes[prev].ord.saturating_add(1);
            }
        }
        // The most notes sounding at once: an end and a start at one frame don't overlap.
        let mut edges: Vec<(u64, i8)> = notes.iter().flat_map(|n| [(n.start, 1), (n.end, -1)]).collect();
        edges.sort_unstable();
        let (mut now, mut sounding) = (0i64, 0i64);
        for (_, d) in edges {
            now += d as i64;
            sounding = sounding.max(now);
        }
        // The most starts in any 128 frames.
        let (mut ons, mut j) = (0, 0);
        for i in 0..notes.len() {
            while notes[j].start + crate::BLOCK as u64 <= notes[i].start {
                j += 1;
            }
            ons = ons.max(i + 1 - j);
        }
        NotesData { notes, by_key, needs: Caps { sounding: sounding as usize, ons } }
    }

    pub fn empty() -> NotesData {
        NotesData::default()
    }

    pub fn needs(&self) -> Caps {
        self.needs
    }

    pub fn len(&self) -> usize {
        self.notes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }

    /// The notes whose key is `key`, in `by_key` order. Never allocates.
    fn group(&self, key: &NoteKey) -> &[u32] {
        let lo = self.by_key.partition_point(|&i| self.notes[i as usize].key < *key);
        let hi = lo + self.by_key[lo..].partition_point(|&i| self.notes[i as usize].key == *key);
        &self.by_key[lo..hi]
    }

    /// The first note that starts at or after transport frame `at`.
    fn seek(&self, at: u64) -> usize {
        self.notes.partition_point(|n| n.start < at)
    }
}

/// New data for one lane, built off the audio thread (RENDER-26).
#[derive(Clone, Debug)]
pub enum LaneSwap {
    Notes(Arc<NotesData>),
    Control(Arc<ControlLane>),
}

/// A note the scheduler turned on and hasn't turned off.
#[derive(Clone, Copy, Debug)]
struct Sounding {
    id: u32,
    end: u64,
    end_beat: f64,
    key: NoteKey,
    ord: u16,
}

pub struct NoteScheduler {
    data: Arc<NotesData>,
    /// The next note to turn on.
    cursor: usize,
    sounding: Vec<Sounding>,
    next_id: u32,
    /// New data arrived: match the sounding notes to it in the next block.
    swapped: bool,
    /// The renderer is being replaced: everything was turned off, and nothing starts.
    released: bool,
    /// It took over another scheduler's sounding notes (a carried plugin instance): once only.
    adopted: bool,
    events: Events,
    caps: Caps,
}

impl NoteScheduler {
    /// A scheduler with room for `data` and at least [`Caps::FLOOR`].
    pub fn new(data: Arc<NotesData>) -> NoteScheduler {
        let caps = data.needs.max(Caps::FLOOR);
        NoteScheduler {
            data,
            cursor: 0,
            sounding: Vec::with_capacity(caps.sounding),
            next_id: 1,
            swapped: false,
            released: false,
            adopted: false,
            events: Events::with_capacity(caps.events()),
            caps,
        }
    }

    pub fn caps(&self) -> Caps {
        self.caps
    }

    pub fn events(&self) -> &Events {
        &self.events
    }

    /// Notes turned on and not yet off.
    pub fn held(&self) -> usize {
        self.sounding.len()
    }

    /// Whether note `id` is sounding.
    pub fn is_sounding(&self, id: u32) -> bool {
        self.sounding.iter().any(|s| s.id == id)
    }

    /// The id its next note gets.
    pub fn next_id(&self) -> u32 {
        self.next_id
    }

    /// Never give a note an id below `id`: a plugin instance carried from another scheduler had
    /// notes up to it, and CLAP matches voices by id (§5.3).
    pub fn raise_next_id(&mut self, id: u32) {
        self.next_id = self.next_id.max(id);
    }

    /// Take over the notes `old` turned on and hasn't turned off, keeping their ids, as far as this
    /// scheduler has room: an activation that carries a plugin instance whose notes go on (C7,
    /// RENDER-25). At its first block they are matched against this scheduler's data as a swap's
    /// are (RENDER-26a): one the data dropped, or that ends at or before the transport, gets an
    /// Off there, and the rest keep sounding until their ends in the new data. Once per scheduler;
    /// returns how many it took. Never allocates.
    pub fn adopt(&mut self, old: &NoteScheduler) -> usize {
        if self.adopted {
            return 0;
        }
        self.adopted = true;
        self.next_id = self.next_id.max(old.next_id);
        let mut n = 0;
        for s in &old.sounding {
            if self.sounding.len() == self.sounding.capacity() {
                break;
            }
            self.sounding.push(*s);
            n += 1;
        }
        if n > 0 {
            self.swapped = true;
        }
        n
    }

    /// Play `data` from the next block on. Returns the data replaced, to retire off the audio
    /// thread, or `data` itself when it needs more room than this scheduler has.
    pub fn swap(&mut self, data: Arc<NotesData>) -> Result<Arc<NotesData>, Arc<NotesData>> {
        if !self.caps.fits(data.needs) {
            return Err(data);
        }
        self.swapped = true;
        Ok(std::mem::replace(&mut self.data, data))
    }

    /// Back to the state after building, without Offs: the voices they would reach were reset.
    /// Ids go on counting, so an id is never reused.
    pub fn reset(&mut self) {
        self.cursor = 0;
        self.sounding.clear();
        self.swapped = false;
        self.released = false;
        self.events.clear();
    }

    /// Fill this block's events for `runs`. `release_all`: this renderer is being replaced, so
    /// every sounding note ends at frame 0 and no note starts from now on (RENDER-26a).
    /// Never allocates.
    pub fn run(&mut self, runs: &Runs, release_all: bool) {
        self.events.clear();
        if release_all && !self.released {
            self.released = true;
            for s in self.sounding.drain(..) {
                self.events.push(Event::Off { frame: 0, id: s.id, velocity: 0.0 });
            }
        }
        for (i, run) in runs.as_slice().iter().enumerate() {
            if run.jump != Jump::None {
                // RENDER-9: a jump ends every sounding note, and the cursor seeks. A renderer's
                // first block (Enter) has nothing of its own sounding: only notes it adopted from
                // the renderer it replaces, which go on (C7).
                if run.jump != Jump::Enter {
                    for s in self.sounding.drain(..) {
                        self.events.push(Event::Off { frame: run.off, id: s.id, velocity: 0.0 });
                    }
                }
                self.cursor = self.data.seek(run.at);
            }
            if self.swapped && i == 0 {
                self.reconcile(run);
                self.cursor = self.data.seek(run.at);
                self.swapped = false;
            }
            if run.moving {
                self.play(run);
            }
        }
        self.events.sort();
    }

    /// RENDER-26a: a sounding note the new data no longer has, or whose new end is at or before
    /// the transport, ends now; one whose end moved is rescheduled. Equal keys are matched by
    /// their place among equals.
    fn reconcile(&mut self, run: &Run) {
        let Self { data, sounding, events, .. } = self;
        sounding.retain_mut(|s| match data.group(&s.key).get(s.ord as usize) {
            Some(&i) if data.notes[i as usize].end > run.at => {
                let n = &data.notes[i as usize];
                (s.end, s.end_beat) = (n.end, n.end_beat);
                true
            }
            _ => {
                events.push(Event::Off { frame: run.off, id: s.id, velocity: 0.0 });
                false
            }
        });
    }

    fn play(&mut self, run: &Run) {
        let end = run.at + run.n as u64;
        let Self { data, cursor, sounding, next_id, released, events, .. } = self;
        sounding.retain(|s| {
            if s.end >= end {
                return true;
            }
            debug_assert!(s.end >= run.at, "a sounding note ended before the run");
            events.push(Event::Off { frame: run.off + s.end.saturating_sub(run.at) as u32, id: s.id, velocity: 0.0 });
            false
        });
        while *cursor < data.notes.len() && data.notes[*cursor].start < end {
            let n = data.notes[*cursor];
            *cursor += 1;
            if n.start < run.at {
                // Only after a jump this scheduler missed: never chased, never an On before the run.
                debug_assert!(false, "a note before the run");
                continue;
            }
            if *released {
                continue;
            }
            // An On needs room for itself and its Off, and a place among the sounding.
            if !events.room(2) || (n.end >= end && sounding.len() == sounding.capacity()) {
                debug_assert!(false, "the scheduler is out of room");
                events.drop_on();
                continue;
            }
            let id = *next_id;
            *next_id = next_id.checked_add(1).unwrap_or(1);
            let frame = run.off + (n.start - run.at) as u32;
            events.push(Event::On { frame, id, pitch: n.pitch, velocity: n.velocity, from_pitch: None });
            if n.end < end {
                events.push(Event::Off { frame: run.off + (n.end - run.at) as u32, id, velocity: 0.0 });
            } else {
                sounding.push(Sounding { id, end: n.end, end_beat: n.end_beat, key: n.key, ord: n.ord });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use m2_song::{LaneData, LaneKind};
    use serde_json::{Value, json};

    use super::*;
    use crate::transport::{Cmd, Transport};

    const RATE: f64 = 48_000.0;

    fn clips(v: Value) -> Vec<NoteClip> {
        match LaneData::read(&v.to_string(), &LaneKind::Notes).unwrap() {
            LaneData::Notes(c) => c,
            _ => unreachable!(),
        }
    }

    fn data(v: Value) -> Arc<NotesData> {
        Arc::new(NotesData::build(&clips(v), &TempoMap::constant(120.0), RATE))
    }

    /// Every event of `blocks` blocks, with its transport frame.
    fn drive(s: &mut NoteScheduler, t: &mut Transport, blocks: usize) -> Vec<(u64, Event)> {
        let mut out = vec![];
        for _ in 0..blocks {
            let runs = t.advance();
            s.run(&runs, false);
            out.extend(s.events().as_slice().iter().map(|e| (runs.frame(e.frame()), *e)));
        }
        out
    }

    fn ids(events: &[(u64, Event)], on: bool) -> Vec<u32> {
        events
            .iter()
            .filter_map(|(_, e)| match (e, on) {
                (Event::On { id, .. }, true) | (Event::Off { id, .. }, false) => Some(*id),
                _ => None,
            })
            .collect()
    }

    // RENDER-12: the scheduler plays today's notes, in today's order, at today's samples.
    #[test]
    fn notes_come_in_todays_order_at_todays_samples() {
        let c = clips(json!({"clips": [
            {"id": "b", "start": 2, "length": 2, "notes": [{"pitch": 55, "start": -0.5, "length": 1}]},
            {"id": "a", "start": 0, "length": 8, "notes": [
                {"pitch": 64, "start": 0.25, "length": 1}, {"pitch": 60, "start": 0, "length": 1}, {"pitch": 60, "start": 0, "length": 1}]}]}));
        let tempo = TempoMap::constant(120.0);
        let d = NotesData::build(&c, &tempo, RATE);
        let today: Vec<(u64, u64, f32)> = m2_song::lane::notes(&c)
            .iter()
            .map(|n| ((tempo.seconds_at(n.start) * RATE).round() as u64, (tempo.seconds_at(n.end) * RATE).round() as u64, n.pitch as f32))
            .collect();
        let ours: Vec<(u64, u64, f32)> = d.notes.iter().map(|n| (n.start, n.end, n.pitch)).collect();
        assert_eq!(ours, today);
        assert_eq!(d.needs(), Caps { sounding: 3, ons: 2 });
        assert_eq!(d.notes.iter().map(|n| n.ord).collect::<Vec<_>>(), [0, 1, 0, 0], "the duplicate is second among equals");
    }

    // RENDER-12, RENDER-13: Ons at their frames, Offs at their ends; a note ending on a block
    // line gets its Off at frame 0 of the next block.
    #[test]
    fn ons_and_offs_land_on_their_frames() {
        // 120 bpm: beat 0.01 is 240 frames; 128 frames is 0.0053 beats.
        let mut s = NoteScheduler::new(data(json!({"clips": [{"start": 0, "length": 4, "notes": [
            {"pitch": 60, "start": 0.01, "length": 0.01}, {"pitch": 62, "start": 0, "length": 0.016}]}]})));
        let mut t = Transport::new(0, true);
        let ev = drive(&mut s, &mut t, 6);
        let at: Vec<(u64, bool)> = ev.iter().map(|(f, e)| (*f, matches!(e, Event::On { .. }))).collect();
        assert_eq!(at, [(0, true), (240, true), (384, false), (480, false)]);
        assert_eq!(ev[2].1.frame(), 0, "an end on the block line is the next block's frame 0");
        assert_eq!(s.held(), 0);
    }

    // RENDER-9, RENDER-11: a jump ends every sounding note at its frame and seeks; a note
    // sounding at the target is not chased.
    #[test]
    fn a_jump_ends_notes_and_never_chases() {
        let tempo = TempoMap::constant(120.0);
        let mut s = NoteScheduler::new(data(json!({"clips": [{"start": 0, "length": 8, "notes": [
            {"pitch": 60, "start": 0, "length": 4}, {"pitch": 64, "start": 2, "length": 1}]}]})));
        let mut t = Transport::new(0, true);
        drive(&mut s, &mut t, 2);
        assert_eq!(s.held(), 1);
        t.apply(Cmd::Seek(1.0), &tempo, RATE);
        let ev = drive(&mut s, &mut t, 1);
        assert_eq!(ev.iter().map(|e| e.1).collect::<Vec<_>>(), [Event::Off { frame: 0, id: 1, velocity: 0.0 }]);
        assert_eq!(s.held(), 0, "the long note isn't chased");
        let ev = drive(&mut s, &mut t, 400);
        assert_eq!(ids(&ev, true), [2], "the next note plays");
        t.apply(Cmd::Seek(2.5), &tempo, RATE);
        assert!(drive(&mut s, &mut t, 1).is_empty());
        assert_eq!(s.held(), 0, "2.5 is inside the second note: not chased");
    }

    // RENDER-26a, step 3 gate (G4): a swap ends a deleted note, keeps a lengthened one,
    // reschedules a shortened one and ends one shortened behind the transport. Nothing hangs.
    #[test]
    fn swaps_reconcile_the_sounding_notes() {
        let base = |len: f64, extra: bool| {
            let mut notes = vec![json!({"pitch": 60, "start": 0, "length": len})];
            if extra {
                notes.push(json!({"pitch": 67, "start": 0, "length": 4}));
            }
            data(json!({"clips": [{"id": "a", "start": 0, "length": 8, "notes": notes}]}))
        };
        let tempo = TempoMap::constant(120.0);
        let start = || {
            let mut s = NoteScheduler::new(base(4.0, true));
            let mut t = Transport::new(0, false);
            t.apply(Cmd::Play, &tempo, RATE);
            drive(&mut s, &mut t, 100); // 12 800 frames: beat 0.53
            assert_eq!(s.held(), 2);
            (s, t)
        };
        // Deleted.
        let (mut s, mut t) = start();
        s.swap(base(4.0, false)).unwrap();
        let ev = drive(&mut s, &mut t, 1);
        assert_eq!(ev.iter().map(|e| e.1).collect::<Vec<_>>(), [Event::Off { frame: 0, id: 2, velocity: 0.0 }]);
        assert_eq!(s.held(), 1);
        // Shortened ahead of the transport: its Off moves there.
        let (mut s, mut t) = start();
        s.swap(base(1.0, true)).unwrap();
        let ev = drive(&mut s, &mut t, 200);
        let off: Vec<u64> = ev.iter().filter(|e| matches!(e.1, Event::Off { id: 1, .. })).map(|(f, _)| *f).collect();
        assert_eq!(off, [24_000]);
        // Shortened behind it: Off now. Lengthened: still sounding at the old end.
        let (mut s, mut t) = start();
        s.swap(base(0.25, true)).unwrap();
        assert_eq!(ids(&drive(&mut s, &mut t, 1), false), [1]);
        let (mut s, mut t) = start();
        s.swap(base(6.0, true)).unwrap();
        drive(&mut s, &mut t, 900); // past beat 4
        assert!(s.is_sounding(1) && !s.is_sounding(2));
        // A moved clip: the start beat changed, so its notes end.
        let (mut s, mut t) = start();
        s.swap(data(json!({"clips": [{"id": "a", "start": 0.25, "length": 8, "notes": [
            {"pitch": 60, "start": 0, "length": 4}, {"pitch": 67, "start": 0, "length": 4}]}]})))
            .unwrap();
        let ev = drive(&mut s, &mut t, 1);
        assert_eq!(ids(&ev, false), [1, 2]);
        // Stopped, then swapped: nothing sounds, nothing breaks.
        let (mut s, mut t) = start();
        t.apply(Cmd::Stop, &tempo, RATE);
        drive(&mut s, &mut t, 1);
        s.swap(base(4.0, false)).unwrap();
        assert!(drive(&mut s, &mut t, 1).is_empty());
        assert_eq!(s.held(), 0);
    }

    // RENDER-26a: notes with equal keys (duplicates, layered clips with one id, anonymous clips
    // at one start) are matched by their place among equals: a no-op swap keeps every note,
    // and deleting one of two duplicates ends exactly one.
    #[test]
    fn equal_keys_are_matched_by_ordinal() {
        let lane = |n: usize| {
            let dup: Vec<Value> = (0..n).map(|k| json!({"pitch": 60, "start": 0, "length": 2 + k})).collect();
            json!({"clips": [
                {"id": "a", "start": 0, "length": 8, "notes": dup},
                {"id": "l", "start": 0, "length": 8, "notes": [{"pitch": 64, "start": 0, "length": 3}]},
                {"id": "l", "start": 0, "length": 8, "notes": [{"pitch": 64, "start": 0, "length": 3}]},
                {"start": 1, "length": 8, "notes": [{"pitch": 67, "start": 0, "length": 3}]},
                {"start": 1, "length": 8, "notes": [{"pitch": 67, "start": 0, "length": 3}]}]})
        };
        let mut s = NoteScheduler::new(data(lane(2)));
        let mut t = Transport::new(0, true);
        drive(&mut s, &mut t, 200); // beat 1.07
        assert_eq!(s.held(), 6);
        s.swap(data(lane(2))).unwrap();
        assert!(drive(&mut s, &mut t, 1).is_empty(), "a no-op swap changes nothing");
        assert_eq!(s.held(), 6);
        s.swap(data(lane(1))).unwrap();
        assert_eq!(ids(&drive(&mut s, &mut t, 1), false).len(), 1, "one of the duplicates ends");
        let ev = drive(&mut s, &mut t, 2000);
        assert_eq!(ids(&ev, false).len(), 5);
        assert_eq!(s.held(), 0);
    }

    // RENDER-26a: an activation's old renderer ends every note at frame 0 and starts none;
    // a swap that needs more room than the scheduler has is refused.
    #[test]
    fn release_all_and_capacity() {
        let mut s = NoteScheduler::new(data(json!({"clips": [{"start": 0, "length": 8, "notes": [
            {"pitch": 60, "start": 0, "length": 4}, {"pitch": 62, "start": 0.01, "length": 4}]}]})));
        let mut t = Transport::new(0, true);
        drive(&mut s, &mut t, 1);
        s.run(&t.advance(), true);
        assert_eq!(s.events().as_slice(), [Event::Off { frame: 0, id: 1, velocity: 0.0 }]);
        for _ in 0..10 {
            s.run(&t.advance(), true);
            assert!(s.events().as_slice().is_empty(), "nothing starts once released");
        }
        let many: Vec<Value> = (0..300).map(|k| json!({"pitch": k % 128, "start": 0, "length": 1})).collect();
        let big = data(json!({"clips": [{"start": 0, "length": 8, "notes": many}]}));
        assert_eq!(big.needs(), Caps { sounding: 300, ons: 300 });
        assert!(s.swap(big).is_err());
        assert_eq!(Caps::FLOOR.events(), 256 + 4 * 128 + 8);
    }

    // C7, RENDER-26a: a scheduler adopts the notes another turned on, with their ids. At its first
    // block (Enter) they go on: one its data dropped ends at frame 0, the rest end where its data
    // says, and its own notes take ids above every adopted one. Adopting is once, within its room.
    #[test]
    fn adopted_notes_go_on_through_enter() {
        let lane = |top: bool| {
            let mut ns = vec![json!({"pitch": 60, "start": 0, "length": 1}), json!({"pitch": 67, "start": 2, "length": 1})];
            if top {
                ns.push(json!({"pitch": 64, "start": 0, "length": 1}));
            }
            data(json!({"clips": [{"id": "c", "start": 0, "length": 8, "notes": ns}]}))
        };
        let mut old = NoteScheduler::new(lane(true));
        let mut t = Transport::new(0, true);
        drive(&mut old, &mut t, 100); // beat 0.53
        assert_eq!(old.held(), 2);
        let mut new = NoteScheduler::new(lane(false));
        assert_eq!(new.adopt(&old), 2);
        assert_eq!(new.adopt(&old), 0, "once");
        assert_eq!(old.held(), 2, "the old scheduler keeps its copy, for its own consumers' release");
        t.enter();
        let runs = t.advance();
        assert_eq!(runs.first().jump, Jump::Enter);
        new.run(&runs, false);
        let top = new.events().as_slice().to_vec();
        assert_eq!(top, [Event::Off { frame: 0, id: 2, velocity: 0.0 }], "the note the data dropped");
        assert!(new.is_sounding(1) && new.held() == 1);
        let ev = drive(&mut new, &mut t, 900); // past beat 2
        assert_eq!(ev.iter().find(|e| matches!(e.1, Event::Off { id: 1, .. })).map(|e| e.0), Some(24_000), "at its own end");
        assert_eq!(ids(&ev, true), [3], "a new note never takes an adopted id");
        // Without adopting, Enter is what it always was: nothing sounds, nothing ends.
        let mut fresh = NoteScheduler::new(lane(false));
        let mut t = Transport::new(12_800, true);
        t.enter();
        fresh.run(&t.advance(), false);
        assert!(fresh.events().as_slice().is_empty() && fresh.held() == 0);
        // Past its room: the rest isn't taken (whoever held it lets it go).
        let many: Vec<Value> = (0..300).map(|k| json!({"pitch": k % 128, "start": 0, "length": 4})).collect();
        let mut big = NoteScheduler::new(data(json!({"clips": [{"start": 0, "length": 8, "notes": many}]})));
        drive(&mut big, &mut Transport::new(0, true), 2);
        assert_eq!(big.held(), 300);
        assert_eq!(NoteScheduler::new(lane(false)).adopt(&big), Caps::FLOOR.sounding);
    }

    // RENDER-9a: a wrap ends the sounding notes at its frame and plays the loop's start again.
    #[test]
    fn a_wrap_ends_notes_at_its_frame() {
        let tempo = TempoMap::constant(120.0);
        let mut s = NoteScheduler::new(data(json!({"clips": [{"start": 0, "length": 8, "notes": [
            {"pitch": 60, "start": 0.5, "length": 4}]}]})));
        let mut t = Transport::new(0, true);
        t.apply(Cmd::Loop(Some((0.0, 1.0))), &tempo, RATE);
        // One On per lap, mid-loop, and one Off at each wrap, by engine frame.
        let (mut engine, mut frames) = (0u64, vec![]);
        for _ in 0..400 {
            s.run(&t.advance(), false);
            frames.extend(s.events().as_slice().iter().map(|e| (engine + e.frame() as u64, matches!(e, Event::On { .. }))));
            engine += 128;
        }
        assert_eq!(&frames[..4], [(12_000, true), (24_000, false), (36_000, true), (48_000, false)].as_slice());
    }
}
