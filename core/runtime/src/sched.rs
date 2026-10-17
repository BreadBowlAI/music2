//! Notes-lane schedulers (RENDER-12, RENDER-26a): each block, a scheduler fills its lane's
//! event buffer from the transport, and it owns the notes it has turned on and not yet off.
//!
//! A lane's notes are prepared off the audio thread ([`NotesData`]): in samples, in the order
//! today's renders play them, each named by its `(clip, pitch, start beat)`. A jump ends every
//! sounding note and seeks the cursor; notes already sounding at the target are not chased
//! (RENDER-11). A swap ([`NoteScheduler::swap`]) keeps the sounding notes the new data still
//! has, reschedules their offs, and ends the rest.
//!
//! A scheduler also holds the notes played on its lane live (MON-7): audition today. They aren't
//! in the data. Each is sent at a block's first frame, and ends only by its Off, its source's
//! release or a panic: a jump or a swap never touches them.

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

/// New data for one lane, built off the audio thread (RENDER-26). `SwapLane` carries notes and
/// control data; an audio lane's new renders come only in an activation, which crossfades them.
#[derive(Clone, Debug)]
pub enum LaneSwap {
    Notes(Arc<NotesData>),
    Control(Arc<ControlLane>),
    Audio(Arc<crate::audio::AudioLane>),
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

/// Live notes one lane holds at once (MON-7): more than two hands, few enough to scan each block.
pub const LIVE: usize = 16;
/// Its table's room: as many again let go whose Offs wait for the next block, so a key let go and
/// another struck at one block line never refuse each other. Each sends at most one event a block,
/// and a consumer holds no more of them.
pub const LIVE_ROOM: usize = 2 * LIVE;

/// A note played on a lane live, not from its data (MON-7): audition today, a MIDI keyboard
/// later. Each belongs to a source (a window), which names its own notes by handle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LiveEv {
    On { source: u32, handle: u32, pitch: f32, velocity: f32 },
    Off { source: u32, handle: u32 },
    /// Every note of the source, on every lane.
    Release { source: u32 },
}

/// A live note the scheduler holds, from its On until its Off is sent.
#[derive(Clone, Copy, Debug)]
struct LiveNote {
    source: u32,
    handle: u32,
    id: u32,
    pitch: f32,
    velocity: f32,
    /// Its On waits for the next block.
    on: bool,
    /// It was let go: its Off waits for the next block.
    off: bool,
}

pub struct NoteScheduler {
    data: Arc<NotesData>,
    /// The next note to turn on.
    cursor: usize,
    sounding: Vec<Sounding>,
    /// New data arrived: match the sounding notes to it in the next block.
    swapped: bool,
    events: Events,
    caps: Caps,
    /// Notes played live (MON-7). Boxed: a lane slot stays small.
    live: Box<[Option<LiveNote>; LIVE_ROOM]>,
}

impl NoteScheduler {
    /// A scheduler with room for `data` and at least [`Caps::FLOOR`], and for live notes.
    pub fn new(data: Arc<NotesData>) -> NoteScheduler {
        let caps = data.needs.max(Caps::FLOOR);
        NoteScheduler {
            data,
            cursor: 0,
            sounding: Vec::with_capacity(caps.sounding),
            swapped: false,
            events: Events::with_capacity(caps.events() + LIVE_ROOM),
            caps,
            live: Box::new([None; LIVE_ROOM]),
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

    /// Whether note `id` is sounding: one of its data's, or one played live.
    pub fn is_sounding(&self, id: u32) -> bool {
        self.sounding.iter().any(|s| s.id == id) || self.live.iter().flatten().any(|n| n.id == id)
    }

    /// Live notes held: struck, and their Off not yet sent (MON-7).
    pub fn live_held(&self) -> usize {
        self.live.iter().flatten().count()
    }

    /// Take a live note's On or Off, or its source's release (MON-7), sent at the next block's
    /// first frame. A struck note takes the id `next` holds: the engine's, so a live note's id never
    /// meets a scheduled one's. A key struck again before its Off ends first. With [`LIVE`] notes
    /// held, an On is refused and counted with the dropped Ons. Never allocates.
    pub(crate) fn live(&mut self, ev: LiveEv, next: &mut u32) {
        match ev {
            LiveEv::On { source, handle, pitch, velocity } => {
                self.end_live(|n| n.source == source && n.handle == handle);
                let held = self.live.iter().flatten().filter(|n| !n.off).count();
                match self.live.iter_mut().find(|n| n.is_none()).filter(|_| held < LIVE) {
                    Some(free) => {
                        let id = *next;
                        *next = next.checked_add(1).unwrap_or(1);
                        *free = Some(LiveNote { source, handle, id, pitch, velocity, on: true, off: false });
                    }
                    None => self.events.drop_on(),
                }
            }
            LiveEv::Off { source, handle } => self.end_live(|n| n.source == source && n.handle == handle),
            LiveEv::Release { source } => self.end_live(|n| n.source == source),
        }
    }

    /// Let go of the live notes `which` picks.
    fn end_live(&mut self, which: impl Fn(&LiveNote) -> bool) {
        for n in self.live.iter_mut().flatten().filter(|n| !n.off && which(n)) {
            n.off = true;
        }
    }

    /// This block's live events, at its first frame: the On of each note struck since the last
    /// block, and the Off of each let go. One struck and let go since then sends its On now and its
    /// Off at the next block: at one frame an Off sorts first, and would leave the note on.
    fn send_live(&mut self) {
        let Self { live, events, .. } = self;
        for slot in live.iter_mut() {
            let Some(n) = slot else { continue };
            if n.on {
                n.on = false;
                events.push(Event::On { frame: 0, id: n.id, pitch: n.pitch, velocity: n.velocity, from_pitch: None });
            } else if n.off {
                events.push(Event::Off { frame: 0, id: n.id, velocity: 0.0 });
                *slot = None;
            }
        }
    }

    /// The pitch and velocity of note `id` while it sounds, as its data has them now: what starts
    /// it again in a plugin an export borrowed (RENDER-1). Never allocates.
    pub(crate) fn sounding_note(&self, id: u32) -> Option<(f32, f32)> {
        if let Some(n) = self.live.iter().flatten().find(|n| n.id == id) {
            return Some((n.pitch, n.velocity));
        }
        let s = self.sounding.iter().find(|s| s.id == id)?;
        let i = *self.data.group(&s.key).get(s.ord as usize)?;
        let n = &self.data.notes[i as usize];
        Some((n.pitch, n.velocity))
    }

    /// Take over the notes `small` turned on and hasn't turned off, with their ids: this scheduler
    /// replaces it on its lane at an activation, sized for data that outgrew it (RENDER-26a). At its
    /// first block they are matched against this scheduler's data as a swap's are. Its live notes
    /// come as they are (MON-7). Never allocates: it is the bigger of the two.
    pub(crate) fn take_state(&mut self, small: &NoteScheduler) {
        *self.live = *small.live;
        for s in &small.sounding {
            if self.sounding.len() == self.sounding.capacity() {
                debug_assert!(false, "a grown scheduler has less room than the one it replaces");
                break;
            }
            self.sounding.push(*s);
        }
        self.swapped = true;
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
    /// Live notes end too (a panic, A10). Ids are the engine's, so none is reused.
    pub fn reset(&mut self) {
        self.cursor = 0;
        self.sounding.clear();
        *self.live = [None; LIVE_ROOM];
        self.swapped = false;
        self.events.clear();
    }

    /// Fill this block's events for `runs`, giving each note it starts the id `next` holds and
    /// counting it on: the engine's, so ids never repeat across lanes or plans (RENDER-26a). Live
    /// notes come first, at frame 0, whether the transport moves or not; a jump never ends them
    /// (MON-7). Never allocates.
    pub fn run(&mut self, runs: &Runs, next: &mut u32) {
        self.events.clear();
        self.send_live();
        for (i, run) in runs.as_slice().iter().enumerate() {
            if run.jump != Jump::None {
                // RENDER-9: a jump ends every sounding note, and the cursor seeks. A plan's first
                // block with no other jump (Enter) ends nothing: the scheduler moved into it with
                // the notes its consumers hold, which go on.
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
                self.play(run, next);
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

    fn play(&mut self, run: &Run, next_id: &mut u32) {
        let end = run.at + run.n as u64;
        let Self { data, cursor, sounding, events, .. } = self;
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

    /// A scheduler and the note ids it gives, which are the engine's (RENDER-26a).
    struct Sched(NoteScheduler, u32);

    impl Sched {
        fn new(data: Arc<NotesData>) -> Sched {
            Sched(NoteScheduler::new(data), 1)
        }

        fn run(&mut self, runs: &Runs) {
            self.0.run(runs, &mut self.1);
        }
    }

    impl std::ops::Deref for Sched {
        type Target = NoteScheduler;
        fn deref(&self) -> &NoteScheduler {
            &self.0
        }
    }

    impl std::ops::DerefMut for Sched {
        fn deref_mut(&mut self) -> &mut NoteScheduler {
            &mut self.0
        }
    }

    /// Every event of `blocks` blocks, with its transport frame.
    fn drive(s: &mut Sched, t: &mut Transport, blocks: usize) -> Vec<(u64, Event)> {
        let mut out = vec![];
        for _ in 0..blocks {
            let runs = t.advance();
            s.run(&runs);
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
        let mut s = Sched::new(data(json!({"clips": [{"start": 0, "length": 4, "notes": [
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
        let mut s = Sched::new(data(json!({"clips": [{"start": 0, "length": 8, "notes": [
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
            let mut s = Sched::new(base(4.0, true));
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
        let mut s = Sched::new(data(lane(2)));
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

    // RENDER-26a: a swap that needs more room than the scheduler has is refused (an activation
    // grows the lane instead).
    #[test]
    fn a_swap_past_the_room_is_refused() {
        let mut s = Sched::new(data(json!({"clips": [{"start": 0, "length": 8, "notes": [{"pitch": 60, "start": 0, "length": 4}]}]})));
        let many: Vec<Value> = (0..300).map(|k| json!({"pitch": k % 128, "start": 0, "length": 1})).collect();
        let big = data(json!({"clips": [{"start": 0, "length": 8, "notes": many}]}));
        assert_eq!(big.needs(), Caps { sounding: 300, ons: 300 });
        assert!(s.swap(big).is_err());
        assert_eq!(Caps::FLOOR.events(), 256 + 4 * 128 + 8);
    }

    // RENDER-26a: a lane that outgrows its scheduler gets a bigger one at an activation, which
    // takes over the sounding notes with their ids. At its first block (Enter) they go on: one its
    // data dropped ends at frame 0, the rest end where its data says, and new notes take the
    // engine's next ids.
    #[test]
    fn take_state_keeps_sounding_notes() {
        let lane = |top: bool, extra: usize| {
            let mut ns = vec![json!({"pitch": 60, "start": 0, "length": 1}), json!({"pitch": 67, "start": 2, "length": 1})];
            if top {
                ns.push(json!({"pitch": 64, "start": 0, "length": 1}));
            }
            ns.extend((0..extra).map(|k| json!({"pitch": k % 100, "start": 4, "length": 1})));
            data(json!({"clips": [{"id": "c", "start": 0, "length": 8, "notes": ns}]}))
        };
        let mut old = Sched::new(lane(true, 0));
        let mut t = Transport::new(0, true);
        drive(&mut old, &mut t, 100); // beat 0.53
        assert_eq!(old.held(), 2);
        let mut big = Sched::new(lane(false, 300));
        assert!(big.caps().sounding > old.caps().sounding);
        big.1 = old.1; // the engine's next id, as the activation takes it over
        big.take_state(&old);
        t.enter();
        let runs = t.advance();
        assert_eq!(runs.first().jump, Jump::Enter);
        big.run(&runs);
        assert_eq!(big.events().as_slice(), [Event::Off { frame: 0, id: 2, velocity: 0.0 }], "the note the data dropped");
        assert!(big.is_sounding(1) && big.held() == 1);
        let ev = drive(&mut big, &mut t, 900); // past beat 2
        assert_eq!(ev.iter().find(|e| matches!(e.1, Event::Off { id: 1, .. })).map(|e| e.0), Some(24_000), "at its own end");
        assert_eq!(ids(&ev, true).first(), Some(&3), "the next note takes the next id");
        // Enter alone, on a scheduler that took nothing: nothing sounds, nothing ends.
        let mut fresh = Sched::new(lane(false, 0));
        let mut t = Transport::new(12_800, true);
        t.enter();
        fresh.run(&t.advance());
        assert!(fresh.events().as_slice().is_empty() && fresh.held() == 0);
    }

    // RENDER-9a: a wrap ends the sounding notes at its frame and plays the loop's start again.
    #[test]
    fn a_wrap_ends_notes_at_its_frame() {
        let tempo = TempoMap::constant(120.0);
        let mut s = Sched::new(data(json!({"clips": [{"start": 0, "length": 8, "notes": [
            {"pitch": 60, "start": 0.5, "length": 4}]}]})));
        let mut t = Transport::new(0, true);
        t.apply(Cmd::Loop(Some((0.0, 1.0))), &tempo, RATE);
        // One On per lap, mid-loop, and one Off at each wrap, by engine frame.
        let (mut engine, mut frames) = (0u64, vec![]);
        for _ in 0..400 {
            s.run(&t.advance());
            frames.extend(s.events().as_slice().iter().map(|e| (engine + e.frame() as u64, matches!(e, Event::On { .. }))));
            engine += 128;
        }
        assert_eq!(&frames[..4], [(12_000, true), (24_000, false), (36_000, true), (48_000, false)].as_slice());
    }
}
