//! Note effects (RENDER-41, RENDER-42): a kernel that gives notes (ABI-15), run in the lane pass
//! right after the lane or note effect it reads, and the guard every note it gives goes through.
//!
//! A note effect's slot is a lane slot: its consumers read its events as a lane's. What moves
//! between plans is its kernel and its guard ([`Held`]), by the slot's address (RENDER-26a); what
//! it reads is the plan's own ([`NoteEffect`]). Nothing here allocates once it is built.

use std::sync::{Mutex, PoisonError};

use anyhow::Result;
use m2_ir::TimeOf;

use crate::event::{Event, Events};
use crate::fault::{Code, Failed, Reporter, Subject};
use crate::plan::LaneSlot;
use crate::sched::{Caps, LIVE_ROOM, PEDAL, WHEEL_EVENTS, WHEELS, Wheel, is_wheel, turn};
use crate::transport::{Jump, Runs};
use crate::{BLOCK, Block, COPY, EVENT_BYTES, Graph, KernelInst, NOTES_HEAD, control_of, floats, time_of};

/// What one input of a note effect's kernel reads in the lane pass, other than its notes.
pub(crate) enum Fed {
    /// Its notes: its source's events, written before each call.
    Notes,
    /// A constant: written at birth, and again at its first block in each plan, as a kept kernel's
    /// memory may hold what the plan before wrote there.
    Fixed(f32),
    /// A knob's cell: top-level node `node`'s output, copied when its version changes (RENDER-22).
    Cell {
        node: usize,
        seen: u64,
    },
    Time(TimeOf),
    /// A control lane, by its slot, with its value where it has no data.
    Lane {
        slot: usize,
        default: f32,
    },
}

/// A note effect in one plan: what it reads, and the kernel and guard it holds.
pub(crate) struct NoteEffect {
    /// Its node in the plan's graph: what its faults name.
    pub(crate) node: usize,
    /// The slot its notes come from: a lane's or a note effect's, always before it.
    pub(crate) from: usize,
    /// Its kernel's inputs, by place; its notes input's place among them.
    pub(crate) inputs: Vec<Fed>,
    pub(crate) notes: usize,
    /// The events its notes input has room for: its source's.
    pub(crate) room_in: usize,
    /// Its meters, after its notes output: copied into its node's buffers (ABI-13).
    pub(crate) meters: usize,
    /// Empty only while it moves between plans: it gives nothing then.
    pub(crate) held: Option<Box<Held>>,
    /// Its constants wait to be written, at its first block in this plan.
    pub(crate) fresh: bool,
    /// A drop was said, once in this plan (RENDER-42).
    pub(crate) told: bool,
}

/// What a note effect keeps across plans: its kernel's state and its guard's notes. The kernel is
/// behind a lock that is never taken: only `get_mut` reaches it, which needs `&mut`, so the graph's
/// tasks, which share the lane slots, read the guard's events and can never reach the kernel.
pub(crate) struct Held {
    inst: Mutex<KernelInst>,
    pub(crate) guard: Guard,
}

impl Held {
    pub(crate) fn new(inst: KernelInst, guard: Guard) -> Box<Held> {
        Box::new(Held { inst: Mutex::new(inst), guard })
    }

    fn parts(&mut self) -> (&mut KernelInst, &mut Guard) {
        (self.inst.get_mut().unwrap_or_else(PoisonError::into_inner), &mut self.guard)
    }
}

impl NoteEffect {
    /// Whether it holds its kernel: what an activation moves.
    pub(crate) fn filled(&self) -> bool {
        self.held.is_some()
    }

    /// This block's events, for its consumers.
    pub(crate) fn events(&self) -> &[Event] {
        self.held.as_ref().map_or(&[], |h| h.guard.out.as_slice())
    }

    /// The pitch and velocity of note `id` while it sounds: what starts it again in a plugin an
    /// export lent (RENDER-1).
    pub(crate) fn sounding_note(&self, id: u32) -> Option<(f32, f32)> {
        self.held.as_ref()?.guard.sounding().find(|n| n.mine == id).map(|n| (n.pitch, n.velocity))
    }

    /// Notes it has sounding.
    pub(crate) fn held_notes(&self) -> usize {
        self.held.as_ref().map_or(0, |h| h.guard.len)
    }

    /// Whether it may still give notes with no new ones coming in: notes sounding, or a kernel
    /// that says it is active (an arpeggiator still playing a held chord). Runs the kernel's
    /// `k_active`.
    pub(crate) fn active(&mut self) -> bool {
        let Some((k, guard)) = self.held.as_deref_mut().map(Held::parts) else { return false };
        guard.len > 0 || (!k.stopped && k.active(&mut []).unwrap_or(false))
    }

    /// Its wheels away from rest are sent again at the next block: a new plan's consumers, or a
    /// plugin an export lent, hear them.
    pub(crate) fn resend_wheels(&mut self) {
        if let Some(h) = &mut self.held {
            h.guard.wheels.iter_mut().for_each(Wheel::again);
        }
    }

    /// Moved into a new plan (RENDER-26a): its wheels are sent again, and a kernel that stopped
    /// starts again, reset, as an activation resets a stopped box (RENDER-25). Its error, when that
    /// reset fails: it stays stopped.
    pub(crate) fn entered(&mut self) -> Result<()> {
        self.resend_wheels();
        let Some((k, _)) = self.held.as_deref_mut().map(Held::parts) else { return Ok(()) };
        if k.stopped {
            k.own.iter_mut().for_each(crate::sandbox::Sandbox::unstick);
            k.reset(&mut [])?;
            k.stopped = false;
        }
        Ok(())
    }

    /// A panic (RENDER-10): its kernel reset, its guard's notes forgotten with no Off (the voices
    /// they reach are reset too), its wheels at rest from the next block. One that ran past its
    /// deadline isn't called (ABI-7). Its error, when the reset fails: it stops.
    pub(crate) fn reset(&mut self) -> Result<()> {
        let Some((k, guard)) = self.held.as_deref_mut().map(Held::parts) else { return Ok(()) };
        guard.forget();
        guard.wheels.iter_mut().for_each(Wheel::rest_later);
        if k.own.as_ref().is_some_and(crate::sandbox::Sandbox::stuck) {
            return Ok(());
        }
        let done = k.reset(&mut []);
        k.stopped = done.is_err();
        done
    }

    /// Its block in the lane pass (RENDER-41): its inputs written, its kernel called, and what it
    /// gave read through its guard (RENDER-42), its notes numbered from `next`. `before` are the
    /// slots before it, its source among them. `hush`: Stop was pressed while stopped. Live, a
    /// kernel that fails stops alone and its notes end; offline, its fault is the error. Never
    /// allocates while it succeeds.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn run(
        &mut self,
        before: &[LaneSlot],
        graph: &mut Graph,
        b: &Block,
        hush: bool,
        next: &mut u32,
        report: &Reporter,
    ) -> Result<()> {
        let Some((k, guard)) = self.held.as_deref_mut().map(Held::parts) else { return Ok(()) };
        guard.begin();
        if k.stopped {
            return Ok(());
        }
        // Stop pressed while stopped: the kernel starts again as a panic starts it, so a note it
        // holds itself (a Hold) goes too; the guard ends what sounds, at frame 0 (RENDER-42).
        let mut done = if hush { k.reset(&mut []) } else { Ok(()) };
        let fresh = std::mem::take(&mut self.fresh);
        let p = k.ports(&mut []);
        // Its notes: its source's events of the block, as a kernel's notes input holds them (ABI-8).
        let events = before.get(self.from).map_or(&[][..], LaneSlot::events);
        debug_assert!(events.len() <= self.room_in, "a note effect's source gave more than its room");
        let events = &events[..events.len().min(self.room_in)];
        let head = p.ins[self.notes];
        for (i, e) in events.iter().enumerate() {
            let at = head + NOTES_HEAD + i * EVENT_BYTES;
            p.data[at..at + EVENT_BYTES].copy_from_slice(bytemuck::cast_slice(&e.words()));
        }
        p.data[head..head + 4].copy_from_slice(&(events.len() as u32).to_le_bytes());
        // Its other inputs, as their own nodes would read them.
        for (fed, &at) in self.inputs.iter_mut().zip(p.ins) {
            match fed {
                Fed::Fixed(v) if fresh => floats(p.data, at, BLOCK).fill(*v),
                Fed::Fixed(_) | Fed::Notes => {}
                Fed::Cell { node, seen } => {
                    let version = graph.versions[*node];
                    if version == COPY || *seen != version {
                        floats(p.data, at, BLOCK).copy_from_slice(&graph.bufs[*node][0][..BLOCK]);
                        *seen = version;
                    }
                }
                Fed::Time(t) => time_of(*t, b, 0, floats(p.data, at, BLOCK)),
                Fed::Lane { slot, default } => {
                    let lane = before.get(*slot).unwrap_or(&LaneSlot::Control(None));
                    control_of(lane, *default, &b.beats64, floats(p.data, at, BLOCK));
                }
            }
        }
        // ABI-15: the host sets `count` to 0 before each call.
        let out = p.outs[0];
        p.data[out..out + 4].fill(0);
        if done.is_ok() {
            done = k.process(&mut [], BLOCK);
        }
        if let Err(e) = done {
            if !b.live {
                let f = report.fault(Code::of(&e), Subject::Node(self.node as u32));
                return Err(Failed::new(f, Some(e)).into());
            }
            // Its error allocated, and is dropped here: this path only.
            report.node(Code::of(&e), self.node);
            k.stopped = true;
            guard.stop();
            return Ok(());
        }
        let p = k.ports(&mut []);
        // ABI-15: the count it says, read from where the host put its events, never from the
        // header's own address; past the room, a drop (RENDER-42).
        let count = u32::from_le_bytes(p.data[out..out + 4].try_into().expect("4 bytes")) as usize;
        let given = &p.data[out + NOTES_HEAD..out + NOTES_HEAD + count.min(guard.room) * EVENT_BYTES];
        let dropped = guard.take(given, &b.runs, hush, next, b.live) | (count > guard.room);
        // Its meters, for the window (ABI-13).
        for m in 0..self.meters {
            if let (Some(&at), Some(buf)) = (p.outs.get(1 + m), graph.bufs[self.node].get_mut(1 + m)) {
                buf[..BLOCK].copy_from_slice(floats(p.data, at, BLOCK));
            }
        }
        if dropped {
            if !b.live {
                let f = report.fault(Code::NotesLeftOut, Subject::Node(self.node as u32));
                return Err(Failed::new(f, None).into());
            }
            if !std::mem::replace(&mut self.told, true) {
                report.node(Code::NotesLeftOut, self.node);
            }
        }
        Ok(())
    }
}

/// No note, no place.
const NONE: u32 = u32::MAX;

/// How far past where a kernel's id hashes the guard's table looks for it (RENDER-42).
const PROBES: usize = 16;

/// A note the guard has sounding, or a free one.
#[derive(Clone, Copy)]
struct Note {
    /// The kernel's id, and the one the guard gave it.
    kernel: u32,
    mine: u32,
    pitch: f32,
    velocity: f32,
    from_pitch: Option<f32>,
    /// Its place in the table; [`NONE`] while it is free.
    at: u32,
    /// The notes started before and after it (the next free one, while it is free).
    prev: u32,
    next: u32,
    /// Started at the frame being read: its On waits until the frame is read.
    waits: bool,
}

/// The guard (RENDER-42): what a note effect's kernel gives, put in order, checked and numbered,
/// with every note it has sounding and the wheels it passed, so it can end them. Each event costs
/// it a few steps, however many notes sound: a note is found by the kernel's id in a fixed table,
/// at most [`PROBES`] steps from where the id hashes, and the oldest is the head of a list of them
/// in the order they started.
pub(crate) struct Guard {
    /// What it gives this block: its consumers read it as a lane's.
    out: Events,
    /// The events a kernel may give in a block: its output header's `cap`.
    room: usize,
    /// The Ons it may start in a block, and those it started in this one.
    ons: usize,
    started: usize,
    /// Its notes, sounding or free; how many sound, and of them how many wait to be sent.
    notes: Box<[Note]>,
    len: usize,
    waiting: usize,
    /// The first free note, and the oldest and newest sounding.
    free: u32,
    first: u32,
    last: u32,
    /// Each place: the note filed there, or [`NONE`]. A note free since, or filed elsewhere since,
    /// no longer is: the place is free again. Four places or more a note, a power of two.
    table: Box<[u32]>,
    /// The latest value of each wheel it passed, and whether to send it again.
    wheels: [Wheel; WHEELS],
    /// The counting sort's buckets, a frame's two kinds each (controllers, notes), and the order it
    /// puts what the kernel gave in.
    buckets: Box<[u32; 2 * BLOCK + 1]>,
    order: Vec<u32>,
    /// The steps its table took, and the events it read: what bounds its time (tests).
    pub(crate) steps: (u64, u64),
}

/// One event a kernel gave, as the guard reads it: its kind, controllers 0 and notes 1, and the
/// event with the kernel's own id. None for one no one can play.
fn read(raw: &[u8]) -> Option<(u32, Event)> {
    let w = |i: usize| u32::from_le_bytes(raw[4 * i..4 * i + 4].try_into().expect("4 bytes"));
    let (kind, frame, id) = (w(0), w(1), w(2));
    let f = |i: usize| f32::from_bits(w(i));
    let unit = |x: f32| (0.0..=1.0).contains(&x);
    let pitch = |x: f32| (0.0..=127.0).contains(&x);
    if frame as usize >= BLOCK {
        return None;
    }
    match kind {
        0 if pitch(f(3)) && unit(f(4)) && (f(5).is_nan() || pitch(f(5))) => {
            Some((1, Event::On { frame, id, pitch: f(3), velocity: f(4), from_pitch: Some(f(5)).filter(|x| !x.is_nan()) }))
        }
        1 if unit(f(3)) => Some((1, Event::Off { frame, id, velocity: f(3) })),
        4 => {
            let [a, b, c, _] = w(3).to_le_bytes();
            is_wheel([a, b, c]).then_some((0, Event::Midi { frame, bytes: [a, b, c] }))
        }
        _ => None,
    }
}

impl Guard {
    /// A guard for a note effect whose room is `caps`: as many notes sounding, and starting in a
    /// block, as a lane of that room has (RENDER-42). Its kernel may give `room` events a block.
    pub(crate) fn new(caps: Caps) -> Guard {
        let (room, size) = (crate::note_delay_room(caps), caps.sounding.max(1));
        let free = Note { kernel: 0, mine: 0, pitch: 0.0, velocity: 0.0, from_pitch: None, at: NONE, prev: NONE, next: NONE, waits: false };
        let mut g = Guard {
            out: Events::with_capacity(room),
            room,
            ons: 2 * caps.ons + LIVE_ROOM,
            started: 0,
            notes: vec![free; size].into_boxed_slice(),
            len: 0,
            waiting: 0,
            free: NONE,
            first: NONE,
            last: NONE,
            table: vec![NONE; (4 * size).next_power_of_two()].into_boxed_slice(),
            wheels: Wheel::AT_REST,
            buckets: Box::new([0; 2 * BLOCK + 1]),
            order: Vec::with_capacity(room),
            steps: (0, 0),
        };
        g.forget();
        g
    }

    /// The events its kernel may give in a block: the `cap` its output header says.
    pub(crate) fn room(&self) -> usize {
        self.room
    }

    /// Its notes sounding, oldest first.
    fn sounding(&self) -> impl Iterator<Item = &Note> {
        std::iter::successors(self.notes.get(self.first as usize), |n| self.notes.get(n.next as usize))
    }

    /// A new block: nothing given yet but the wheels it was asked to send again, at its first
    /// frame.
    fn begin(&mut self) {
        self.out.clear();
        self.started = 0;
        for (k, w) in self.wheels.iter_mut().enumerate() {
            if let Some(bytes) = w.take_send(k) {
                self.out.push(Event::Midi { frame: 0, bytes });
            }
        }
    }

    /// Every note forgotten with no Off, each free again.
    fn forget(&mut self) {
        let last = self.notes.len() - 1;
        for (i, n) in self.notes.iter_mut().enumerate() {
            (n.at, n.next) = (NONE, if i < last { i as u32 + 1 } else { NONE });
        }
        (self.free, self.first, self.last, self.len, self.waiting) = (0, NONE, NONE, 0, 0);
    }

    /// End every note it has sounding at `frame`, the pedal let up first when `pedal` and it is
    /// down.
    fn end(&mut self, frame: u32, pedal: bool) {
        if let Some(bytes) = pedal.then(|| self.wheels[PEDAL].let_go(PEDAL)).flatten() {
            self.out.push(Event::Midi { frame, bytes });
        }
        while self.first != NONE {
            self.end_one(self.first, frame, 0.0);
        }
    }

    /// Its kernel stopped: every wheel goes back to rest, and every note ends, at the block's first
    /// frame. It gives nothing more.
    fn stop(&mut self) {
        self.out.clear();
        for (k, w) in self.wheels.iter_mut().enumerate() {
            if let Some(bytes) = w.let_go(k) {
                self.out.push(Event::Midi { frame: 0, bytes });
            }
        }
        self.end(0, false);
    }

    /// Whether `n` more events fit in its room beside an On for each note waiting to be sent, an
    /// Off for every note sounding and the wheels at rest: what its consumers have room for, as for
    /// a lane's.
    fn fits(&self, n: usize) -> bool {
        self.out.as_slice().len() + self.waiting + self.len + n + WHEEL_EVENTS <= self.room
    }

    /// The places kernel id `id` may be filed at: [`PROBES`] of them from where it hashes.
    fn places(&self, id: u32) -> impl Iterator<Item = usize> + use<> {
        // A finalizer that spreads every bit of the id over the place (MurmurHash3's).
        let mut h = id ^ (id >> 16);
        h = h.wrapping_mul(0x85EB_CA6B);
        h ^= h >> 13;
        h = h.wrapping_mul(0xC2B2_AE35);
        h ^= h >> 16;
        let mask = self.table.len() - 1;
        (0..PROBES).map(move |k| (h as usize).wrapping_add(k) & mask)
    }

    /// The note sounding under the kernel's id `id`, if one is.
    fn find(&mut self, id: u32) -> Option<u32> {
        for at in self.places(id) {
            self.steps.0 += 1;
            match self.table[at] {
                NONE => return None,
                n if self.notes[n as usize].at == at as u32 && self.notes[n as usize].kernel == id => return Some(n),
                _ => {}
            }
        }
        None
    }

    /// Start the note of `on` as `mine`, the newest, its On sent once the frame is read: filed under
    /// the kernel's id. False when no place is free within [`PROBES`] steps of where the id hashes
    /// (ids chosen to collide). It has a free note: the caller made room.
    fn start(&mut self, on: Event, mine: u32) -> bool {
        let Event::On { id, pitch, velocity, from_pitch, .. } = on else { return false };
        for at in self.places(id) {
            self.steps.0 += 1;
            if self.table[at] != NONE && self.notes[self.table[at] as usize].at == at as u32 {
                continue;
            }
            let (n, prev) = (self.free, self.last);
            self.free = self.notes[n as usize].next;
            self.notes[n as usize] = Note { kernel: id, mine, pitch, velocity, from_pitch, at: at as u32, prev, next: NONE, waits: true };
            match prev {
                NONE => self.first = n,
                p => self.notes[p as usize].next = n,
            }
            (self.last, self.table[at]) = (n, n);
            (self.len, self.waiting, self.started) = (self.len + 1, self.waiting + 1, self.started + 1);
            return true;
        }
        false
    }

    /// End note `n` at `frame`: its Off, or nothing for one whose On waits, a note of no length.
    fn end_one(&mut self, n: u32, frame: u32, velocity: f32) {
        let Note { mine, prev, next, waits, .. } = self.notes[n as usize];
        if waits {
            (self.waiting, self.started) = (self.waiting - 1, self.started - 1);
        } else {
            self.out.push(Event::Off { frame, id: mine, velocity });
        }
        match prev {
            NONE => self.first = next,
            p => self.notes[p as usize].next = next,
        }
        match next {
            NONE => self.last = prev,
            x => self.notes[x as usize].prev = prev,
        }
        let note = &mut self.notes[n as usize];
        (note.at, note.next, self.free, self.len) = (NONE, self.free, n, self.len - 1);
    }

    /// Send the Ons of the notes that wait, all started at `frame`: the newest, in the order they
    /// started.
    fn send(&mut self, frame: u32) {
        let mut n = self.last;
        for _ in 1..self.waiting {
            n = self.notes[n as usize].prev;
        }
        for _ in 0..std::mem::take(&mut self.waiting) {
            let note = &mut self.notes[n as usize];
            note.waits = false;
            let (id, pitch, velocity, from_pitch) = (note.mine, note.pitch, note.velocity, note.from_pitch);
            self.out.push(Event::On { frame, id, pitch, velocity, from_pitch });
            n = note.next;
        }
    }

    /// Read what the kernel gave this block, `given` (24 bytes an event), into its events: in
    /// RENDER-14's order, checked, numbered from `next`, with the live bit of the kernel's id only
    /// while it plays `live`; ending every note at a seek, a Stop or a wrap in `runs`, and at frame 0
    /// when `hush`. Whether it left any out. Never allocates.
    fn take(&mut self, given: &[u8], runs: &Runs, hush: bool, next: &mut u32, live: bool) -> bool {
        let n = given.len() / EVENT_BYTES;
        let raw = |i: usize| &given[i * EVENT_BYTES..(i + 1) * EVENT_BYTES];
        // A counting sort by frame and kind: one pass to count, one to place. Stable, so a frame's
        // notes keep the kernel's order: an On, then its Off, is a note of no length.
        let mut dropped = false;
        self.buckets.fill(0);
        for i in 0..n {
            match read(raw(i)) {
                Some((kind, e)) => self.buckets[1 + 2 * e.frame() as usize + kind as usize] += 1,
                None => dropped = true,
            }
        }
        for k in 1..self.buckets.len() {
            self.buckets[k] += self.buckets[k - 1];
        }
        self.order.clear();
        self.order.resize(self.buckets[2 * BLOCK] as usize, 0);
        for i in 0..n {
            if let Some((kind, e)) = read(raw(i)) {
                let b = &mut self.buckets[2 * e.frame() as usize + kind as usize];
                self.order[*b as usize] = i as u32;
                *b += 1;
            }
        }
        // Where every note ends, after the controllers there: at frame 0 when Stop was pressed while
        // stopped, and where the song stops or jumps (RENDER-9, RENDER-36).
        let jumps = runs.as_slice().iter().filter(|r| !matches!(r.jump, Jump::None | Jump::Enter | Jump::Start));
        let jumps = jumps.map(|r| (r.off, matches!(r.jump, Jump::Seek | Jump::Stop)));
        let mut ends = hush.then_some((0, true)).into_iter().chain(jumps).peekable();
        let mut frame = 0;
        for k in 0..self.order.len() {
            let Some((kind, e)) = read(raw(self.order[k] as usize)) else { continue };
            self.steps.1 += 1;
            if e.frame() != frame {
                self.send(frame);
                frame = e.frame();
            }
            while let Some((at, pedal)) = ends.next_if(|&(at, _)| (at, 1) <= (frame, kind)) {
                self.end(at, pedal);
            }
            match e {
                Event::On { id, .. } if self.started < self.ons && self.fits(2) => {
                    // Its id sounding: that note ends first. Its room full: the oldest does, unless
                    // none sounds yet, all started at this frame: then this one is left out.
                    if let Some(old) = self.find(id) {
                        self.end_one(old, frame, 0.0);
                    }
                    if self.len == self.notes.len() && self.notes[self.first as usize].waits {
                        dropped = true;
                        continue;
                    }
                    if self.len == self.notes.len() {
                        self.end_one(self.first, frame, 0.0);
                    }
                    // The engine's next id, with the live bit of the kernel's while it plays live, and
                    // none offline: an export has no live keys (RENDER-14).
                    if self.start(e, *next | if live { id & crate::event::LIVE_BIT } else { 0 }) {
                        crate::event::take_id(next);
                    } else {
                        dropped = true;
                    }
                }
                // An Off of a note it doesn't have sounding (one it ended itself, or never had) is
                // left out, and no drop.
                Event::Off { id, velocity, .. } => {
                    if let Some(old) = self.find(id) {
                        self.end_one(old, frame, velocity);
                    }
                }
                Event::Midi { bytes, .. } if self.fits(1) => {
                    turn(&mut self.wheels, 0, bytes, false);
                    self.out.push(e);
                }
                _ => dropped = true,
            }
        }
        self.send(frame);
        for (at, pedal) in ends {
            self.end(at, pedal);
        }
        dropped
    }
}
