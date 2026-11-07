//! Note effects (RENDER-41, RENDER-42): a kernel that gives notes (ABI-15), run in the lane pass
//! right after the lane or note effect it reads, and the guard every note it gives goes through.
//!
//! A note effect's slot is a lane slot: its consumers read its events as a lane's. What moves
//! between plans is its kernel and its guard ([`Held`]), by the slot's address (RENDER-26a); what
//! it reads is the plan's own ([`NoteEffect`]). Nothing here allocates once it is built.

use anyhow::Result;
use m2_ir::TimeOf;

use crate::event::{Event, Events};
use crate::fault::{Code, Failed, Reporter, Subject};
use crate::plan::LaneSlot;
use crate::sched::{Caps, LIVE_ROOM, WHEEL_EVENTS, WHEELS, is_wheel};
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
    from: usize,
    /// Its kernel's inputs, by place; its notes input's place among them.
    inputs: Vec<Fed>,
    notes: usize,
    /// The events its notes input has room for: its source's.
    room_in: usize,
    /// Its meters, after its notes output: copied into its node's buffers (ABI-13).
    meters: usize,
    /// Empty only while it moves between plans: it gives nothing then.
    pub(crate) held: Option<Box<Held>>,
    /// Its constants wait to be written, at its first block in this plan.
    fresh: bool,
    /// A drop was said, once in this plan (RENDER-42).
    told: bool,
}

/// What a note effect keeps across plans: its kernel's state and its guard's notes.
pub(crate) struct Held {
    pub(crate) inst: Box<KernelInst>,
    pub(crate) guard: Guard,
}

// SAFETY: the graph's tasks share a plan's lane slots while a block runs (8a), and reach a note
// effect only through `&`: its guard's events and sounding notes, plain data. Its kernel's sandbox is
// touched only through `&mut`, by the lane pass and the renderer's own calls on its one thread, never
// while the tasks run.
unsafe impl Sync for Held {}

impl NoteEffect {
    pub(crate) fn new(
        node: usize,
        from: usize,
        inputs: Vec<Fed>,
        notes: usize,
        room_in: usize,
        meters: usize,
        held: Option<Box<Held>>,
    ) -> Self {
        NoteEffect { node, from, inputs, notes, room_in, meters, held, fresh: true, told: false }
    }

    /// Whether it holds its kernel: what an activation moves.
    pub(crate) fn filled(&self) -> bool {
        self.held.is_some()
    }

    /// This block's events, for its consumers.
    pub(crate) fn events(&self) -> &[Event] {
        self.held.as_ref().map_or(&[], |h| h.guard.out.as_slice())
    }

    /// Whether note `id` is one it has sounding.
    pub(crate) fn is_sounding(&self, id: u32) -> bool {
        self.held.as_ref().is_some_and(|h| h.guard.sounding.iter().any(|s| s.1 == id))
    }

    /// Notes it has sounding.
    pub(crate) fn held_notes(&self) -> usize {
        self.held.as_ref().map_or(0, |h| h.guard.sounding.len())
    }

    /// Whether it may still give notes with no new ones coming in: notes sounding, or a kernel
    /// that says it is active (an arpeggiator still playing a held chord). Runs the kernel's
    /// `k_active`.
    pub(crate) fn active(&mut self) -> bool {
        let Some(h) = self.held.as_deref_mut() else { return false };
        !h.guard.sounding.is_empty() || (!h.inst.stopped && h.inst.active(&mut []).unwrap_or(false))
    }

    /// Its wheels away from rest are sent again at the next block: a new plan's consumers, or a
    /// plugin an export lent, hear them.
    pub(crate) fn resend_wheels(&mut self) {
        if let Some(h) = &mut self.held {
            h.guard.resend();
        }
    }

    /// Moved into a new plan (RENDER-26a): its wheels are sent again, and a kernel that stopped
    /// starts again, reset, as an activation resets a stopped box ([RENDER-25]). Its error, when
    /// that reset fails: it stays stopped.
    pub(crate) fn entered(&mut self) -> Result<()> {
        let Some(h) = self.held.as_deref_mut() else { return Ok(()) };
        h.guard.resend();
        if h.inst.stopped {
            h.inst.own.iter_mut().for_each(crate::sandbox::Sandbox::unstick);
            h.inst.reset(&mut [])?;
            h.inst.stopped = false;
        }
        Ok(())
    }

    /// A panic (RENDER-10): its kernel reset, its guard's notes forgotten with no Off (the voices
    /// they reach are reset too), its wheels at rest from the next block. One that ran past its
    /// deadline isn't called (ABI-7). Its error, when the reset fails: it stops.
    pub(crate) fn reset(&mut self) -> Result<()> {
        let Some(h) = self.held.as_deref_mut() else { return Ok(()) };
        h.guard.reset();
        if h.inst.own.as_ref().is_some_and(crate::sandbox::Sandbox::stuck) {
            return Ok(());
        }
        let done = h.inst.reset(&mut []);
        h.inst.stopped = done.is_err();
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
        let Some(h) = self.held.as_deref_mut() else { return Ok(()) };
        h.guard.begin();
        if h.inst.stopped {
            return Ok(());
        }
        if hush {
            h.guard.end(0, true);
        }
        let fresh = std::mem::take(&mut self.fresh);
        let p = h.inst.ports(&mut []);
        // Its notes: its source's events of the block, as a kernel's notes input holds them (ABI-8).
        let events = before.get(self.from).map_or(&[][..], LaneSlot::events);
        debug_assert!(events.len() <= self.room_in, "a note effect's source gave more than its room");
        let events = &events[..events.len().min(self.room_in)];
        let head = p.ins[self.notes];
        for (k, e) in events.iter().enumerate() {
            let at = head + NOTES_HEAD + k * EVENT_BYTES;
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
        if let Err(e) = h.inst.process(&mut [], BLOCK) {
            if !b.live {
                let f = report.fault(Code::of(&e), Subject::Node(self.node as u32));
                return Err(Failed::new(f, Some(e)).into());
            }
            // Its error allocated, and is dropped here: this path only.
            report.node(Code::of(&e), self.node);
            h.inst.stopped = true;
            h.guard.stop();
            return Ok(());
        }
        let p = h.inst.ports(&mut []);
        // ABI-15: the count it says, never more than the room the host gave it, read from where the
        // host put its events, never from the header's own address.
        let count = u32::from_le_bytes(p.data[out..out + 4].try_into().expect("4 bytes")) as usize;
        let given = &p.data[out + NOTES_HEAD..out + NOTES_HEAD + count.min(h.guard.room) * EVENT_BYTES];
        let dropped = h.guard.take(given, &b.runs, next);
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

/// The pitch wheel's, the mod wheel's and the pedal's places among a guard's wheels, as a lane's
/// (`sched::WHEELS`).
const PEDAL: usize = 2;

/// A wheel's value at rest: the pitch wheel centred, the mod wheel and the pedal at 0.
const REST: [u16; WHEELS] = [8192, 0, 0];

/// The guard (RENDER-42): what a note effect's kernel gives, put in order, checked and numbered,
/// with every note it has sounding and the wheels it passed, so it can end them.
pub(crate) struct Guard {
    /// What it gives this block: its consumers read it as a lane's.
    out: Events,
    /// The events a kernel may give in a block: its output header's `cap`.
    room: usize,
    /// The notes it has sounding, oldest first: the kernel's id, and the one it gave.
    sounding: Vec<(u32, u32)>,
    /// Ons it may start in one block.
    ons: usize,
    /// The latest value of each wheel it passed, and whether to send it at the next block.
    wheels: [(u16, bool); WHEELS],
    /// The counting sort's buckets, a frame's three kinds each (controllers, Offs, Ons), and the
    /// order it puts what the kernel gave in.
    buckets: Box<[u32; 3 * BLOCK + 1]>,
    order: Vec<u32>,
}

/// One event a kernel gave, as the guard reads it: its kind's rank at its frame (controllers 0,
/// Offs 1, Ons 2), and the event with the kernel's own id. None for one no one can play.
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
            Some((2, Event::On { frame, id, pitch: f(3), velocity: f(4), from_pitch: Some(f(5)).filter(|x| !x.is_nan()) }))
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
        let room = crate::note_delay_room(caps);
        Guard {
            out: Events::with_capacity(room),
            room,
            sounding: Vec::with_capacity(caps.sounding),
            ons: 2 * caps.ons + LIVE_ROOM,
            wheels: REST.map(|r| (r, false)),
            buckets: Box::new([0; 3 * BLOCK + 1]),
            order: Vec::with_capacity(room),
        }
    }

    /// The events its kernel may give in a block: the `cap` its output header says.
    pub(crate) fn room(&self) -> usize {
        self.room
    }

    /// A new block: nothing given yet but the wheels it was asked to send again, at its first
    /// frame.
    fn begin(&mut self) {
        self.out.clear();
        for (k, w) in self.wheels.iter_mut().enumerate() {
            if std::mem::take(&mut w.1) {
                self.out.push(Event::Midi { frame: 0, bytes: wheel(k, w.0) });
            }
        }
    }

    /// Send the wheels away from rest again at the next block.
    fn resend(&mut self) {
        for (w, rest) in self.wheels.iter_mut().zip(REST) {
            w.1 |= w.0 != rest;
        }
    }

    /// Forget every note with no Off, and put the wheels at rest from the next block (a panic).
    fn reset(&mut self) {
        self.sounding.clear();
        for (w, rest) in self.wheels.iter_mut().zip(REST) {
            *w = (rest, w.1 || w.0 != rest);
        }
        self.out.clear();
    }

    /// End every note it has sounding at `frame`, the pedal let up first when `pedal` and it is
    /// down.
    fn end(&mut self, frame: u32, pedal: bool) {
        if pedal && self.wheels[PEDAL].0 != REST[PEDAL] {
            self.wheels[PEDAL] = (REST[PEDAL], false);
            self.out.push(Event::Midi { frame, bytes: wheel(PEDAL, REST[PEDAL]) });
        }
        for (_, id) in self.sounding.drain(..) {
            self.out.push(Event::Off { frame, id, velocity: 0.0 });
        }
    }

    /// Its kernel stopped: every note ends, and every wheel goes back to rest, at the block's
    /// first frame. It gives nothing more.
    fn stop(&mut self) {
        self.out.clear();
        for (k, (w, rest)) in self.wheels.iter_mut().zip(REST).enumerate() {
            if w.0 != rest {
                *w = (rest, false);
                self.out.push(Event::Midi { frame: 0, bytes: wheel(k, rest) });
            }
        }
        self.end(0, false);
    }

    /// Whether `n` more events fit in its room beside an Off for every note sounding and the wheels
    /// at rest: what its consumers have room for, as for a lane's.
    fn fits(&self, n: usize) -> bool {
        self.out.as_slice().len() + self.sounding.len() + n + WHEEL_EVENTS <= self.out.capacity()
    }

    /// End the note it gave as `id`, at `frame`.
    fn end_one(&mut self, at: usize, frame: u32) {
        let (_, id) = self.sounding.remove(at);
        self.out.push(Event::Off { frame, id, velocity: 0.0 });
    }

    /// Read what the kernel gave this block, `given` (24 bytes an event), into its events: in
    /// RENDER-14's order, checked, numbered from `next`; ending every note at a seek, a Stop or a
    /// wrap in `runs`. Whether it left any out. Never allocates.
    fn take(&mut self, given: &[u8], runs: &Runs, next: &mut u32) -> bool {
        let n = given.len() / EVENT_BYTES;
        let raw = |i: usize| &given[i * EVENT_BYTES..(i + 1) * EVENT_BYTES];
        // A counting sort by frame and kind: one pass to count, one to place. Stable.
        let mut dropped = false;
        self.buckets.fill(0);
        for i in 0..n {
            match read(raw(i)) {
                Some((rank, e)) => self.buckets[1 + 3 * e.frame() as usize + rank as usize] += 1,
                None => dropped = true,
            }
        }
        for k in 1..self.buckets.len() {
            self.buckets[k] += self.buckets[k - 1];
        }
        let placed = self.buckets[3 * BLOCK] as usize;
        self.order.clear();
        self.order.resize(placed, 0);
        for i in 0..n {
            if let Some((rank, e)) = read(raw(i)) {
                let b = &mut self.buckets[3 * e.frame() as usize + rank as usize];
                self.order[*b as usize] = i as u32;
                *b += 1;
            }
        }
        // Where the song stops or jumps, every note ends: before what the kernel gave there that
        // isn't a controller (RENDER-9, RENDER-36).
        let mut ends = runs.as_slice().iter().filter(|r| !matches!(r.jump, Jump::None | Jump::Enter | Jump::Start)).peekable();
        let mut ons = 0;
        for k in 0..self.order.len() {
            let Some((rank, e)) = read(raw(self.order[k] as usize)) else { continue };
            while let Some(r) = ends.next_if(|r| (r.off, 1) <= (e.frame(), rank)) {
                self.end(r.off, matches!(r.jump, Jump::Seek | Jump::Stop));
            }
            match e {
                Event::On { frame, id, pitch, velocity, from_pitch } => {
                    // Ending a note keeps room as it was: its Off takes the place it kept.
                    if ons >= self.ons || !self.fits(2) {
                        dropped = true;
                        continue;
                    }
                    if let Some(at) = self.sounding.iter().position(|s| s.0 == id) {
                        self.end_one(at, frame);
                    }
                    if self.sounding.len() == self.sounding.capacity() {
                        self.end_one(0, frame);
                    }
                    ons += 1;
                    let mine = *next;
                    *next = next.checked_add(1).unwrap_or(1);
                    self.sounding.push((id, mine));
                    self.out.push(Event::On { frame, id: mine, pitch, velocity, from_pitch });
                }
                Event::Off { frame, id, velocity } => {
                    // An Off of a note it doesn't have sounding: one it ended itself, or never had.
                    if let Some(at) = self.sounding.iter().position(|s| s.0 == id) {
                        let (_, mine) = self.sounding.remove(at);
                        self.out.push(Event::Off { frame, id: mine, velocity });
                    }
                }
                Event::Midi { frame, bytes } => {
                    if !self.fits(1) {
                        dropped = true;
                        continue;
                    }
                    self.wheel(bytes);
                    self.out.push(Event::Midi { frame, bytes });
                }
                _ => unreachable!("`read` gives Ons, Offs and controllers"),
            }
        }
        for r in ends {
            self.end(r.off, matches!(r.jump, Jump::Seek | Jump::Stop));
        }
        dropped
    }

    /// Keep the wheel `bytes` moves, as a lane does (RENDER-14, RENDER-35).
    fn wheel(&mut self, [status, d1, d2]: [u8; 3]) {
        let set = |w: &mut (u16, bool), v: u16| *w = (v, false);
        match (status & 0xF0, d1) {
            (0xE0, _) => set(&mut self.wheels[0], u16::from(d1) | u16::from(d2) << 7),
            (0xB0, 1) => set(&mut self.wheels[1], u16::from(d2)),
            (0xB0, 64) => set(&mut self.wheels[PEDAL], u16::from(d2)),
            _ => self.wheels.iter_mut().zip(REST).for_each(|(w, r)| set(w, r)),
        }
    }
}

/// The bytes wheel `k` sends at `value`, on channel 1, as a lane's (`sched`).
fn wheel(k: usize, value: u16) -> [u8; 3] {
    match k {
        0 => [0xE0, (value & 0x7F) as u8, (value >> 7) as u8],
        PEDAL => [0xB0, 64, value as u8],
        _ => [0xB0, 1, value as u8],
    }
}
