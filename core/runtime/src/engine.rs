//! The live engine (RENDER-6, RENDER-6a, RENDER-8): a persistent engine with its own clock,
//! playing one renderer, which owns the transport.
//!
//! It never replays. It keeps processing while stopped, so tails decay (RENDER-6a), and may
//! idle once nothing can sound. A new plan is activated at the engine's own position, at a block
//! line: it takes over the transport, and the instances it was prepared to keep move into it
//! (`Renderer::take_over`); its changed edges crossfade over 5 ms while what it replaced plays out
//! as ghosts (RENDER-25a). The old renderer leaves at once, with every instance nothing moved.
//! Lane data is swapped in place (RENDER-26). Transport commands are beats, applied to the playing
//! renderer at the block line (RENDER-9).
//!
//! Messages come from an inbox and leave through a retire callback, both supplied by the
//! caller (the player's channels). The audio thread never allocates, locks, waits or drops
//! anything that owns memory here (RENDER-28): what can't be retired yet waits in a fixed stash.
//!
//! Order: transport commands and panic apply as soon as they are read. Structural messages
//! (activate, swap, hold, live notes) apply in strict order: one that can't apply yet waits at the
//! head, and the one after it waits in the inbox. So a live note (MON-7) reaches the plan it names. Activations never coalesce: each is prepared against the
//! plan before it (RENDER-28), and one that doesn't fit what plays is refused whole. A resume
//! applies as soon as it is read too, as resuming never conflicts with an activation or a swap: it
//! waits only behind the hold it ends, while that hold waits for a window to end.
//!
//! A hold, for an export that borrows the live plugin instances (RENDER-1), is a tape pause and
//! RENDER-6a's one exception: the output fades out over 5 ms, then the engine processes nothing,
//! its clock stands still, and it says so ([`Status::held`]); a resume fades back in where it held.
//! While held, a swap applies, transport commands apply to the frozen transport (where it is is
//! published at once), and an activation and a panic wait for the resume. At the resume, each
//! plugin the export borrowed starts again the notes the song still holds, as the export ended them.
//!
//! Cell values are never messages (RENDER-28): each renderer reads its own cells' slots as it
//! renders a block, after the block line's messages, and the engine stamps when it heard them.

use std::sync::Arc;

use crate::click::Click;
use crate::{BLOCK, Cmd, LaneSwap, Leftovers, LiveEv, LiveInput, Pending, Renderer, Runner};

/// A message and its sequence number, which [`Applied`] stamps report.
pub struct Envelope {
    pub seq: u64,
    pub msg: Msg,
}

pub enum Msg {
    /// Play this renderer from the engine's position (built without seeking), taking over the
    /// plan it was prepared against.
    Activate {
        renderer: Box<Renderer>,
    },
    /// New data for lane `slot` of the renderer with this generation.
    SwapLane {
        generation: u64,
        slot: u32,
        data: LaneSwap,
    },
    Transport(Cmd),
    /// Every note off and every instance reset (RENDER-10).
    Panic,
    /// Pause (true) for an export that borrows the live plugin instances, or resume (RENDER-1).
    Hold(bool),
    /// A live note for lane `slot` of the renderer with this generation (MON-7). In order behind
    /// activations, so the plan it names is the one playing when it applies.
    Live {
        generation: u64,
        slot: u32,
        ev: LiveEv,
    },
    /// Count in one bar, then play (lane B): `beats` clicks `frames_per_beat` apart, the song
    /// silent meanwhile; the transport plays from the block after the bar. Applied at once, as a
    /// transport command is; a Play, a Stop, a seek or a hold cancels it.
    CountIn {
        beats: u32,
        frames_per_beat: f64,
    },
    /// Hear a live input on audio lane `slot` of the renderer with this generation while a take
    /// records (MON-8), or with None stop hearing it. In order behind activations, as `Live` is.
    Hear {
        generation: u64,
        slot: u32,
        input: Option<Arc<dyn LiveInput>>,
    },
}

impl Msg {
    fn structural(&self) -> bool {
        !matches!(self, Msg::Transport(_) | Msg::Panic | Msg::CountIn { .. })
    }
}

/// What leaves the audio thread to be dropped elsewhere.
pub enum Retired {
    Renderer(Box<Renderer>),
    Lane(LaneSwap),
    /// A plan's ghosts, ghost lanes and replaced renders, once its fade window is over.
    Nodes(Box<Leftovers>),
    /// A live input no longer heard (MON-8), or one refused.
    Input(Arc<dyn LiveInput>),
}

/// When a message took effect: the engine clock of the block it applied at, the callback that
/// rendered that block, and the frame of the callback the block starts at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Applied {
    pub seq: u64,
    pub clock: u64,
    pub callback_ns: u64,
    pub frame: u32,
    /// An activation that didn't fit what plays: retired unplayed, what played plays on.
    pub refused: bool,
}

/// When cell writes were heard: a renderer's block applied every cell write after seq `from` up
/// to seq `to`. The block is placed as an [`Applied`] stamp places it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Heard {
    pub from: u64,
    pub to: u64,
    pub clock: u64,
    pub callback_ns: u64,
    pub frame: u32,
}

/// The audio callback that is running: when it started, in nanoseconds of the caller's clock.
#[derive(Clone, Copy, Debug, Default)]
pub struct CallbackStamp {
    pub ns: u64,
}

/// A hold's fades: 5 ms at `rate`.
fn fade_len(rate: u32) -> u32 {
    (0.005 * rate as f64).round().max(1.0) as u32
}

/// Retired objects that can wait on the audio thread for the retire channel.
pub const STASH: usize = 16;
/// Applied stamps kept.
pub const APPLIED: usize = 16;
/// Blocks of silence before the engine may idle (RENDER-6a).
const IDLE_AFTER: u32 = 8;
/// Silence: below -100 dBFS.
const SILENT: f32 = 1e-5;

/// What the engine reports after each callback.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Status {
    /// The transport frame of the next frame handed out.
    pub pos: u64,
    pub beats: f64,
    pub seconds: f64,
    pub playing: bool,
    pub generation: u64,
    pub loop_refused: bool,
    pub dropped_ons: u32,
    /// Live notes held (MON-7).
    pub live: u32,
    /// Lanes whose pitch wheel or mod wheel live input left away from rest.
    pub wheels: u32,
    /// Held for an export: nothing was processed since its fade out ended, and nothing is until a
    /// resume (RENDER-1).
    pub held: bool,
}

pub struct Engine {
    current: Option<Box<Renderer>>,
    head: Option<Envelope>,
    blocked: Option<Envelope>,
    clock: u64,
    rate: f64,
    out: Pending,
    /// The transport position after the last frame handed out.
    pos: u64,
    stash: [Option<Retired>; STASH],
    idle: bool,
    silent_blocks: u32,
    applied: [Applied; APPLIED],
    applied_n: u64,
    heard: [Heard; APPLIED],
    heard_n: u64,
    /// Kernel resets of the renderers that have left.
    resets_gone: u64,
    activations: u64,
    seeks: u64,
    crashes: u64,
    refused_swaps: u64,
    refused_activations: u64,
    refused_live: u64,
    /// Retired objects forgotten for want of room: unreachable, as room is checked first.
    leaked: u64,
    /// Held or holding for an export: the output fades to silence and stays there (RENDER-1).
    hold: bool,
    /// The output's level through a hold's fades, in frames of `fade`: `fade` is whole, 0 silent.
    level: u32,
    fade: u32,
    /// A panic read while held or holding: applied at the resume, so no plugin is touched.
    panic_held: bool,
    /// The metronome, mixed after the plan renders (gaps #19).
    click: Click,
}

impl Engine {
    /// An engine at `rate` with nothing to play: it outputs silence.
    pub fn new(rate: u32) -> Engine {
        Engine {
            current: None,
            head: None,
            blocked: None,
            clock: 0,
            rate: rate as f64,
            out: Pending::EMPTY,
            pos: 0,
            stash: Default::default(),
            idle: false,
            silent_blocks: 0,
            applied: [Applied::default(); APPLIED],
            applied_n: 0,
            heard: [Heard::default(); APPLIED],
            heard_n: 0,
            resets_gone: 0,
            activations: 0,
            seeks: 0,
            crashes: 0,
            refused_swaps: 0,
            refused_activations: 0,
            refused_live: 0,
            leaked: 0,
            hold: false,
            level: fade_len(rate),
            fade: fade_len(rate),
            panic_held: false,
            click: Click::default(),
        }
    }

    /// The metronome on or off, from the next block: a click on each beat while the transport
    /// moves. Only the live engine has one: exports never make an engine (RENDER-1).
    pub fn set_click(&mut self, on: bool) {
        self.click.set(on);
    }

    /// An engine playing `renderer`, at its rate, from its transport. The engine clock goes on
    /// from the renderer's, so a renderer that already ran keeps its voices' timing.
    pub fn with(mut renderer: Box<Renderer>) -> Engine {
        let mut e = Engine::new(renderer.sample_rate());
        e.clock = renderer.clock();
        e.pos = renderer.transport().pos();
        // What an engine plays is live: a node that fails stops alone (RENDER-25). Its rings are
        // a disk thread's to fill, never the sound thread's (RENDER-30, the review's io-r6).
        renderer.live = true;
        renderer.disk = true;
        e.current = Some(renderer);
        e
    }

    pub fn rate(&self) -> u32 {
        self.rate as u32
    }

    pub fn clock(&self) -> u64 {
        self.clock
    }

    /// The engine clock of the next frame it hands out: [`Self::clock`] less what its last block
    /// still holds. What an injected input stamps its frames by (RENDER-30a).
    pub fn out_clock(&self) -> u64 {
        self.clock.saturating_sub(self.out.left() as u64)
    }

    pub fn current(&self) -> Option<&Renderer> {
        self.current.as_deref()
    }

    pub fn current_mut(&mut self) -> Option<&mut Renderer> {
        self.current.as_deref_mut()
    }

    /// Whether the playing plan's activation window runs: its ghosts and fades.
    pub fn in_window(&self) -> bool {
        self.current.as_ref().is_some_and(|r| r.in_window())
    }

    /// Kernel resets of every renderer it played, counted where the graph resets
    /// ([`Renderer::resets`]): a panic resets the playing renderer. Nothing else may, live, but an
    /// activation's reset of a box that went non-finite (RENDER-10, counted in its stats).
    pub fn resets(&self) -> u64 {
        self.resets_gone + self.current.as_ref().map_or(0, |r| r.resets())
    }

    pub fn activations(&self) -> u64 {
        self.activations
    }

    /// Seek commands applied so far.
    pub fn seeks(&self) -> u64 {
        self.seeks
    }

    /// Renderers silenced because a kernel crashed.
    pub fn crashes(&self) -> u64 {
        self.crashes
    }

    /// Lane swaps refused for not fitting their slot.
    pub fn refused_swaps(&self) -> u64 {
        self.refused_swaps
    }

    /// Activations refused for not fitting what plays (RENDER-25).
    pub fn refused_activations(&self) -> u64 {
        self.refused_activations
    }

    /// Live notes refused (MON-7): for another plan than the one playing, for a slot or a note that
    /// doesn't fit it, or an On while held for an export.
    pub fn refused_live(&self) -> u64 {
        self.refused_live
    }

    /// Retired objects forgotten for want of room: 0, or memory leaked (RENDER-28).
    pub fn leaked(&self) -> u64 {
        self.leaked
    }

    pub fn is_idle(&self) -> bool {
        self.idle
    }

    /// Held for an export: processing nothing until a resume (RENDER-1).
    pub fn is_held(&self) -> bool {
        self.hold && self.level == 0
    }

    /// The last [`APPLIED`] stamps, and how many messages were applied in all.
    pub fn applied(&self) -> (&[Applied; APPLIED], u64) {
        (&self.applied, self.applied_n)
    }

    /// The last [`APPLIED`] stamps of cell writes heard, and how many there were in all.
    pub fn heard(&self) -> (&[Heard; APPLIED], u64) {
        (&self.heard, self.heard_n)
    }

    /// Where the transport is after the last frame handed out, and what plays.
    pub fn status(&self) -> Status {
        // The position heard: the song's output is `L` frames late (RENDER-19), so it is where the
        // transport was `L` frames before, while that is remembered.
        let pos = match self.current.as_deref() {
            Some(r) if r.latency() > 0 => {
                let next = self.clock.saturating_sub(self.out.left() as u64);
                next.checked_sub(r.latency() as u64).and_then(|t| r.position_at(t)).unwrap_or(self.pos)
            }
            _ => self.pos,
        };
        let seconds = pos as f64 / self.rate;
        match self.current.as_deref() {
            Some(r) => Status {
                pos,
                beats: r.tempo().beat_at(seconds),
                seconds,
                playing: r.transport().playing(),
                generation: r.generation(),
                loop_refused: r.transport().loop_refused(),
                dropped_ons: r.dropped_ons(),
                live: r.live_notes() as u32,
                wheels: r.live_wheels() as u32,
                held: self.is_held(),
            },
            None => Status { pos: self.pos, seconds, held: self.is_held(), ..Status::default() },
        }
    }

    /// Fill `out` at the engine's rate. Messages are read from `inbox` at each block line;
    /// what is replaced leaves through `retire`, which gives an object back when it has no room.
    /// Never allocates, locks or waits (RENDER-28).
    pub fn process(
        &mut self,
        out: &mut [[f32; 2]],
        inbox: &mut dyn FnMut() -> Option<Envelope>,
        retire: &mut dyn FnMut(Retired) -> Result<(), Retired>,
        stamp: CallbackStamp,
    ) {
        self.process_on(out, inbox, retire, stamp, &mut Runner::one());
    }

    /// [`Engine::process`], each block rendered on `runner`'s threads (8a): the same bits.
    pub fn process_on(
        &mut self,
        out: &mut [[f32; 2]],
        inbox: &mut dyn FnMut() -> Option<Envelope>,
        retire: &mut dyn FnMut(Retired) -> Result<(), Retired>,
        stamp: CallbackStamp,
        runner: &mut Runner,
    ) {
        for (k, frame) in out.iter_mut().enumerate() {
            if self.out.is_empty() {
                self.block(inbox, retire, stamp, k as u32, runner);
            }
            let (f, next) = self.out.pop();
            *frame = f;
            self.pos = next;
        }
    }

    fn stash_room(&self) -> bool {
        self.stash_free() > 0
    }

    fn stash_free(&self) -> usize {
        self.stash.iter().filter(|s| s.is_none()).count()
    }

    /// Retire `r` now, or keep it in the stash; give it back when both are full.
    fn retire(&mut self, r: Retired, retire: &mut dyn FnMut(Retired) -> Result<(), Retired>) -> Result<(), Retired> {
        match retire(r) {
            Ok(()) => Ok(()),
            Err(r) => match self.stash.iter_mut().find(|s| s.is_none()) {
                Some(slot) => {
                    *slot = Some(r);
                    Ok(())
                }
                None => Err(r),
            },
        }
    }

    /// Retire `r`, which must fit: callers check [`Self::stash_room`] first.
    fn retire_now(&mut self, r: Retired, retire: &mut dyn FnMut(Retired) -> Result<(), Retired>) {
        if let Err(r) = self.retire(r, retire) {
            // Unreachable when the room was checked; forgetting leaks rather than freeing here.
            debug_assert!(false, "retired with no room");
            self.leaked += 1;
            std::mem::forget(r);
        }
    }

    /// Retire a renderer, which must fit, keeping its resets in [`Self::resets`].
    fn retire_renderer(&mut self, r: Box<Renderer>, retire: &mut dyn FnMut(Retired) -> Result<(), Retired>) {
        self.resets_gone += r.resets();
        self.retire_now(Retired::Renderer(r), retire);
    }

    fn stamp(&mut self, seq: u64, stamp: CallbackStamp, frame: u32, refused: bool) {
        self.applied[(self.applied_n % APPLIED as u64) as usize] =
            Applied { seq, clock: self.clock, callback_ns: stamp.ns, frame, refused };
        self.applied_n += 1;
    }

    /// Apply a transport command, a panic or a resume: always possible, as seeks and loops are
    /// beats.
    fn apply_now(&mut self, msg: &Msg) {
        match *msg {
            Msg::Transport(cmd) => {
                if matches!(cmd, Cmd::Seek(_)) {
                    self.seeks += 1;
                }
                // A count-in waits for nothing else: a Play, a Stop or a seek ends it (lane B).
                if !matches!(cmd, Cmd::Loop(_)) {
                    self.click.cancel_count();
                }
                if let Some(r) = &mut self.current {
                    r.transport_cmd(cmd);
                }
            }
            // Held or holding, no plugin may be called: at the resume.
            Msg::Panic if self.hold => self.panic_held = true,
            Msg::Panic => self.panic(),
            Msg::Hold(false) => self.resume(),
            // Held for an export, nothing would click: no count (lane B).
            Msg::CountIn { beats, frames_per_beat } if !self.hold => self.click.count_in(beats, frames_per_beat),
            Msg::CountIn { .. } => {}
            _ => unreachable!("structural messages wait their turn"),
        }
    }

    /// The end of a hold: the output fades back in from the next block, a panic read meanwhile
    /// applies, and each plugin an export borrowed starts again the notes the song still holds
    /// (RENDER-1).
    fn resume(&mut self) {
        self.hold = false;
        if let Some(r) = &mut self.current {
            r.resumed();
        }
        if std::mem::take(&mut self.panic_held) {
            self.panic();
        }
    }

    fn panic(&mut self) {
        // Ghosts too: the window goes on, and ends on time.
        if let Some(r) = &mut self.current {
            r.panic();
        }
    }

    /// Apply a structural message, or give it back when it can't apply yet. Ok(true): an activation
    /// that was refused.
    fn try_apply(&mut self, env: Envelope, retire: &mut dyn FnMut(Retired) -> Result<(), Retired>) -> Result<bool, Envelope> {
        // An activation waits for the window before it to end, and for room to retire the plan it
        // replaces, its window's leftovers and one to spare.
        let ready = match &env.msg {
            Msg::Activate { .. } => !self.in_window() && self.stash_free() >= 3 && !self.hold,
            Msg::Hold(true) => !self.in_window(),
            // Retires nothing, so it needs no room (M4).
            Msg::Hold(false) | Msg::Live { .. } => true,
            _ => self.stash_room(),
        };
        if !ready {
            return Err(env);
        }
        let mut refused = false;
        match env.msg {
            Msg::Activate { mut renderer } => {
                if renderer.sample_rate() != self.rate as u32 {
                    // A rate change replaces the whole engine (the player's job): never played.
                    debug_assert!(false, "activated a renderer at another rate");
                    self.retire_renderer(renderer, retire);
                    return Ok(false);
                }
                renderer.live = true;
                renderer.disk = true;
                let took = match self.current.as_deref_mut() {
                    None => !renderer.has_entry(),
                    Some(old) => renderer.take_over(old, self.clock),
                };
                if took {
                    // Every box nothing moved leaves with what played, off the audio thread.
                    if let Some(old) = self.current.replace(renderer) {
                        self.retire_renderer(old, retire);
                    }
                    self.activations += 1;
                } else {
                    // Prepared against another plan, or a box isn't what it was prepared for: the
                    // whole activation is refused, and what plays plays on (RENDER-25).
                    self.refused_activations += 1;
                    refused = true;
                    self.retire_renderer(renderer, retire);
                }
            }
            Msg::SwapLane { generation, slot, data } => {
                let back = match &mut self.current {
                    Some(r) if r.generation() == generation => match r.swap_lane(slot as usize, data) {
                        Ok(old) => old,
                        Err(refused) => {
                            self.refused_swaps += 1;
                            refused
                        }
                    },
                    // Meant for a renderer that is gone, or never came: never applied.
                    _ => data,
                };
                self.retire_now(Retired::Lane(back), retire);
            }
            Msg::Live { generation, slot, ev } => {
                // Held for an export, nothing renders: an On would sound only once it ends, so it is
                // refused. An Off or a release waits in its table for the resume (M4), and a wheel in
                // its slot: refused, one let go would stay bent.
                let late = self.hold && matches!(ev, LiveEv::On { .. });
                let took = match &mut self.current {
                    Some(r) if r.generation() == generation && !late => r.live(slot as usize, ev),
                    _ => false,
                };
                self.refused_live += u64::from(!took);
            }
            Msg::Hear { generation, slot, input } => {
                let back = match &mut self.current {
                    // None stops hearing whatever plan plays.
                    Some(r) if r.generation() == generation || input.is_none() => r.hear(slot as usize, input),
                    _ => Err(input),
                };
                let (gone, refused) = match back {
                    Ok(old) => (old, false),
                    Err(input) => (input, true),
                };
                self.refused_live += u64::from(refused);
                if let Some(i) = gone {
                    self.retire_now(Retired::Input(i), retire);
                }
            }
            Msg::Hold(true) => {
                self.hold = true;
                // An export's hold ends a count-in: nothing clicks while held (lane B).
                self.click.cancel_count();
            }
            Msg::Hold(false) => self.resume(),
            Msg::Transport(_) | Msg::Panic | Msg::CountIn { .. } => unreachable!("applied at once"),
        }
        Ok(refused)
    }

    /// One block: messages at its line, then the render.
    fn block(
        &mut self,
        inbox: &mut dyn FnMut() -> Option<Envelope>,
        retire: &mut dyn FnMut(Retired) -> Result<(), Retired>,
        stamp: CallbackStamp,
        frame: u32,
        runner: &mut Runner,
    ) {
        // What waited for room leaves first.
        for i in 0..STASH {
            if let Some(r) = self.stash[i].take()
                && let Err(r) = retire(r)
            {
                self.stash[i] = Some(r);
                break;
            }
        }
        // A window that is over retires its ghosts: an activation waits for that. Without room, the
        // ghosts play on a block, faded to nothing.
        if self.current.as_ref().is_some_and(|r| r.window_over(self.clock))
            && self.stash_room()
            && let Some(l) = self.current.as_mut().and_then(|r| r.end_fade())
        {
            self.retire_now(Retired::Nodes(l), retire);
        }
        // Structural messages that waited, in order.
        if let Some(h) = self.head.take() {
            let seq = h.seq;
            match self.try_apply(h, retire) {
                Ok(refused) => self.stamp(seq, stamp, frame, refused),
                Err(h) => self.head = Some(h),
            }
        }
        if self.head.is_none()
            && let Some(b) = self.blocked.take()
        {
            let seq = b.seq;
            match self.try_apply(b, retire) {
                Ok(refused) => self.stamp(seq, stamp, frame, refused),
                Err(b) => self.head = Some(b),
            }
        }
        while self.blocked.is_none() {
            let Some(env) = inbox() else { break };
            (self.idle, self.silent_blocks) = (false, 0);
            let seq = env.seq;
            // A resume never waits behind an activation that waits for it, which would hold the
            // engine for good; only behind the hold it ends.
            let resume = matches!(env.msg, Msg::Hold(false)) && !self.head.as_ref().is_some_and(|h| matches!(h.msg, Msg::Hold(true)));
            if !env.msg.structural() || resume {
                self.apply_now(&env.msg);
                self.stamp(seq, stamp, frame, false);
                continue;
            }
            match &self.head {
                None => match self.try_apply(env, retire) {
                    Ok(refused) => self.stamp(seq, stamp, frame, refused),
                    Err(env) => self.head = Some(env),
                },
                Some(_) => self.blocked = Some(env),
            }
        }
        self.render(stamp, frame, runner);
    }

    /// Stamp cell writes a renderer's last block heard.
    fn stamp_heard(&mut self, (from, to): (u64, u64), clock: u64, stamp: CallbackStamp, frame: u32) {
        self.heard[(self.heard_n % APPLIED as u64) as usize] = Heard { from, to, clock, callback_ns: stamp.ns, frame };
        self.heard_n += 1;
    }

    fn render(&mut self, stamp: CallbackStamp, frame: u32, runner: &mut Runner) {
        // Held: nothing runs and the clock stands still (RENDER-6a's exception, RENDER-1). The
        // transport's position, which a seek moves at once, is what the status says.
        if self.is_held() {
            self.out.silence(self.current.as_deref().map_or(self.pos, |r| r.transport().pos()));
            return;
        }
        let clock = self.clock;
        self.clock += BLOCK as u64;
        let Some(cur) = self.current.as_deref_mut() else {
            // Silent already: nothing to fade, so a hold holds at once.
            self.level = if self.hold { 0 } else { self.fade };
            self.out.silence(self.pos);
            return;
        };
        if self.idle && !self.click.counting() {
            // A plugin that slept may ask to run again (CLAP's request_process): it wakes the
            // engine here, at the block line, as a message does.
            if cur.woken() {
                (self.idle, self.silent_blocks) = (false, 0);
            } else {
                if self.hold {
                    self.level = 0;
                }
                self.out.silence(cur.transport().pos());
                return;
            }
        }
        let mut frames = match cur.render_block_on(clock, runner) {
            Ok(f) => *f,
            Err(e) => {
                // Live, a node that fails stops alone (RENDER-25), so no error comes here: one that
                // does is counted, and its block is silence (it allocated; this path only).
                drop(e);
                self.crashes += 1;
                [[0.0; 2]; BLOCK]
            }
        };
        let runs = *cur.last_runs();
        self.click.mix(&mut frames, &runs, cur);
        // A count-in's bar ended with this block: the song plays from the next (lane B).
        if self.click.count_ended() {
            cur.transport_cmd(Cmd::Play);
        }
        if let Some(h) = cur.take_heard() {
            self.stamp_heard(h, clock, stamp, frame);
        }
        let Some(cur) = self.current.as_deref_mut() else { unreachable!("rendered above") };
        // A hold's fade out, or a resume's fade in: linear, sample by sample.
        if self.hold || self.level < self.fade {
            for f in &mut frames {
                self.level = if self.hold { self.level.saturating_sub(1) } else { (self.level + 1).min(self.fade) };
                let g = self.level as f32 / self.fade as f32;
                *f = [f[0] * g, f[1] * g];
            }
        }
        self.out.load(&frames, &runs, cur.transport().pos());
        // RENDER-6a: idle only stopped, after 8 silent blocks, when nothing can sound later.
        let silent = frames.iter().all(|f| f[0].abs() < SILENT && f[1].abs() < SILENT);
        self.silent_blocks = if silent && !cur.transport().playing() { self.silent_blocks + 1 } else { 0 };
        if self.silent_blocks >= IDLE_AFTER && cur.can_idle() && !self.click.counting() {
            self.idle = true;
        }
    }
}
