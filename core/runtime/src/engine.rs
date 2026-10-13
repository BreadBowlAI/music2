//! The live engine (RENDER-6, RENDER-6a, RENDER-8): a persistent engine with its own clock,
//! playing one renderer, which owns the transport.
//!
//! It never replays. It keeps processing while stopped, so tails decay (RENDER-6a), and may
//! idle once nothing can sound. A new renderer is activated at the engine's own position: it
//! takes over the transport, the old one ends its notes and both play for 5 ms while the output
//! crossfades (RENDER-25a); the old one is then retired. Lane data is swapped in place
//! (RENDER-26). Transport commands are beats, applied to the playing renderers at the block
//! line (RENDER-9).
//!
//! Messages come from an inbox and leave through a retire callback, both supplied by the
//! caller (the player's channels). The audio thread never allocates, locks, waits or drops
//! anything that owns memory here (RENDER-28): what can't be retired yet waits in a fixed stash.
//!
//! Order: transport commands and panic apply as soon as they are read. Structural messages
//! (activate, swap, set const) apply in strict order: one that can't apply yet waits at the
//! head, and the one after it waits in the inbox; only consecutive activations coalesce.

use m2_ir::Origin;

use crate::{BLOCK, Cmd, LaneSwap, Pending, Renderer};

/// A message and its sequence number, which [`Applied`] stamps report.
pub struct Envelope {
    pub seq: u64,
    pub msg: Msg,
}

pub enum Msg {
    /// Play this renderer from the engine's position (built without seeking).
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
    SetConst(Box<Origin>, f64),
    /// Every note off and every instance reset (RENDER-10).
    Panic,
}

impl Msg {
    fn structural(&self) -> bool {
        !matches!(self, Msg::Transport(_) | Msg::Panic)
    }
}

/// What leaves the audio thread to be dropped elsewhere.
pub enum Retired {
    Renderer(Box<Renderer>),
    Lane(LaneSwap),
    Origin(Box<Origin>),
}

/// When a message took effect: the engine clock of the block it applied at, the callback that
/// rendered that block, and the frame of the callback the block starts at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Applied {
    pub seq: u64,
    pub clock: u64,
    pub callback_ns: u64,
    pub frame: u32,
}

/// The audio callback that is running: when it started, in nanoseconds of the caller's clock.
#[derive(Clone, Copy, Debug, Default)]
pub struct CallbackStamp {
    pub ns: u64,
}

/// Retired objects that can wait on the audio thread for the retire channel.
pub const STASH: usize = 16;
/// Applied stamps kept.
pub const APPLIED: usize = 16;
/// Blocks of silence before the engine may idle (RENDER-6a).
const IDLE_AFTER: u32 = 8;
/// Silence: below -100 dBFS.
const SILENT: f32 = 1e-5;

struct Fade {
    old: Box<Renderer>,
    /// Frames of the fade done.
    done: u32,
    first: bool,
    /// Done, but the stash was full: kept, silent, until it can be retired.
    finished: bool,
}

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
}

pub struct Engine {
    current: Option<Box<Renderer>>,
    fading: Option<Fade>,
    head: Option<Envelope>,
    blocked: Option<Envelope>,
    clock: u64,
    rate: f64,
    fade_len: u32,
    out: Pending,
    /// The transport position after the last frame handed out.
    pos: u64,
    stash: [Option<Retired>; STASH],
    idle: bool,
    silent_blocks: u32,
    applied: [Applied; APPLIED],
    applied_n: u64,
    /// Kernel resets of the renderers that have left.
    resets_gone: u64,
    activations: u64,
    seeks: u64,
    crashes: u64,
    refused_swaps: u64,
}

impl Engine {
    /// An engine at `rate` with nothing to play: it outputs silence.
    pub fn new(rate: u32) -> Engine {
        Engine {
            current: None,
            fading: None,
            head: None,
            blocked: None,
            clock: 0,
            rate: rate as f64,
            fade_len: (0.005 * rate as f64).round().max(1.0) as u32,
            out: Pending::EMPTY,
            pos: 0,
            stash: Default::default(),
            idle: false,
            silent_blocks: 0,
            applied: [Applied::default(); APPLIED],
            applied_n: 0,
            resets_gone: 0,
            activations: 0,
            seeks: 0,
            crashes: 0,
            refused_swaps: 0,
        }
    }

    /// An engine playing `renderer`, at its rate, from its transport. The engine clock goes on
    /// from the renderer's, so a renderer that already ran keeps its voices' timing.
    pub fn with(renderer: Box<Renderer>) -> Engine {
        let mut e = Engine::new(renderer.sample_rate());
        e.clock = renderer.clock();
        e.pos = renderer.transport().pos();
        e.current = Some(renderer);
        e
    }

    pub fn rate(&self) -> u32 {
        self.rate as u32
    }

    pub fn clock(&self) -> u64 {
        self.clock
    }

    pub fn current(&self) -> Option<&Renderer> {
        self.current.as_deref()
    }

    pub fn current_mut(&mut self) -> Option<&mut Renderer> {
        self.current.as_deref_mut()
    }

    /// The renderer fading out after an activation, while it plays.
    pub fn fading(&self) -> Option<&Renderer> {
        self.fading.as_ref().map(|f| &*f.old)
    }

    /// Kernel resets of every renderer it played, counted where the graph resets
    /// ([`Renderer::resets`]): a panic resets the playing renderer, and the fading one during a
    /// fade. Nothing else may, live (RENDER-10).
    pub fn resets(&self) -> u64 {
        self.resets_gone + self.current.as_ref().map_or(0, |r| r.resets()) + self.fading.as_ref().map_or(0, |f| f.old.resets())
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

    pub fn is_idle(&self) -> bool {
        self.idle
    }

    /// The last [`APPLIED`] stamps, and how many messages were applied in all.
    pub fn applied(&self) -> (&[Applied; APPLIED], u64) {
        (&self.applied, self.applied_n)
    }

    /// Where the transport is after the last frame handed out, and what plays.
    pub fn status(&self) -> Status {
        let seconds = self.pos as f64 / self.rate;
        match self.current.as_deref() {
            Some(r) => Status {
                pos: self.pos,
                beats: r.tempo().beat_at(seconds),
                seconds,
                playing: r.transport().playing(),
                generation: r.generation(),
                loop_refused: r.transport().loop_refused(),
                dropped_ons: r.dropped_ons(),
            },
            None => Status { pos: self.pos, seconds, ..Status::default() },
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
        for (k, frame) in out.iter_mut().enumerate() {
            if self.out.is_empty() {
                self.block(inbox, retire, stamp, k as u32);
            }
            let (f, next) = self.out.pop();
            *frame = f;
            self.pos = next;
        }
    }

    fn stash_room(&self) -> bool {
        self.stash.iter().any(Option::is_none)
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
            std::mem::forget(r);
        }
    }

    /// Retire a renderer, which must fit, keeping its resets in [`Self::resets`].
    fn retire_renderer(&mut self, r: Box<Renderer>, retire: &mut dyn FnMut(Retired) -> Result<(), Retired>) {
        self.resets_gone += r.resets();
        self.retire_now(Retired::Renderer(r), retire);
    }

    fn stamp(&mut self, seq: u64, stamp: CallbackStamp, frame: u32) {
        self.applied[(self.applied_n % APPLIED as u64) as usize] = Applied { seq, clock: self.clock, callback_ns: stamp.ns, frame };
        self.applied_n += 1;
    }

    /// Apply a transport command or a panic: always possible, as seeks and loops are beats.
    fn apply_now(&mut self, msg: &Msg) {
        match *msg {
            Msg::Transport(cmd) => {
                if matches!(cmd, Cmd::Seek(_)) {
                    self.seeks += 1;
                }
                if let Some(r) = &mut self.current {
                    r.transport_cmd(cmd);
                }
                if let Some(f) = &mut self.fading {
                    f.old.transport_cmd(cmd);
                }
            }
            Msg::Panic => {
                for r in self.current.iter_mut().map(|r| &mut **r).chain(self.fading.iter_mut().map(|f| &mut *f.old)) {
                    if r.panic().is_err() {
                        r.transport_cmd(Cmd::Stop);
                    }
                }
            }
            _ => unreachable!("structural messages wait their turn"),
        }
    }

    /// Apply a structural message, or give it back when it can't apply yet.
    fn try_apply(&mut self, env: Envelope, retire: &mut dyn FnMut(Retired) -> Result<(), Retired>) -> Result<(), Envelope> {
        let ready = match &env.msg {
            Msg::Activate { .. } => self.current.is_none() || self.fading.is_none(),
            _ => self.stash_room(),
        };
        if !ready {
            return Err(env);
        }
        match env.msg {
            Msg::Activate { mut renderer } => {
                if renderer.sample_rate() != self.rate as u32 {
                    // A rate change replaces the whole engine (the player's job): never played.
                    debug_assert!(false, "activated a renderer at another rate");
                    self.retire_renderer(renderer, retire);
                    return Ok(());
                }
                match self.current.take() {
                    None => self.current = Some(renderer),
                    Some(old) => {
                        renderer.enter_from(old.transport(), old.tempo(), self.rate);
                        self.fading = Some(Fade { old, done: 0, first: true, finished: false });
                        self.current = Some(renderer);
                    }
                }
                self.activations += 1;
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
            Msg::SetConst(origin, value) => {
                if let Some(r) = &mut self.current {
                    r.set_const(&origin, value);
                }
                self.retire_now(Retired::Origin(origin), retire);
            }
            Msg::Transport(_) | Msg::Panic => unreachable!("applied at once"),
        }
        Ok(())
    }

    /// One block: messages at its line, then the render.
    fn block(
        &mut self,
        inbox: &mut dyn FnMut() -> Option<Envelope>,
        retire: &mut dyn FnMut(Retired) -> Result<(), Retired>,
        stamp: CallbackStamp,
        frame: u32,
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
        // A fade that is over retires its renderer: an activation waits for that.
        if let Some(f) = &mut self.fading
            && f.done >= self.fade_len
        {
            f.finished = true;
            if self.stash_room() {
                let f = self.fading.take().expect("a fade");
                self.retire_renderer(f.old, retire);
            }
        }
        // Structural messages that waited, in order.
        if let Some(h) = self.head.take() {
            let seq = h.seq;
            match self.try_apply(h, retire) {
                Ok(()) => self.stamp(seq, stamp, frame),
                Err(h) => self.head = Some(h),
            }
        }
        if self.head.is_none()
            && let Some(b) = self.blocked.take()
        {
            let seq = b.seq;
            match self.try_apply(b, retire) {
                Ok(()) => self.stamp(seq, stamp, frame),
                Err(b) => self.head = Some(b),
            }
        }
        while self.blocked.is_none() {
            let Some(env) = inbox() else { break };
            (self.idle, self.silent_blocks) = (false, 0);
            let seq = env.seq;
            if !env.msg.structural() {
                self.apply_now(&env.msg);
                self.stamp(seq, stamp, frame);
                continue;
            }
            match &self.head {
                None => match self.try_apply(env, retire) {
                    Ok(()) => self.stamp(seq, stamp, frame),
                    Err(env) => self.head = Some(env),
                },
                // RENDER-28: a newer plan replaces one that never played.
                Some(Envelope { msg: Msg::Activate { .. }, .. }) if matches!(env.msg, Msg::Activate { .. }) && self.stash_room() => {
                    let Some(Envelope { msg: Msg::Activate { renderer }, .. }) = self.head.replace(env) else { unreachable!() };
                    self.retire_renderer(renderer, retire);
                }
                Some(_) => self.blocked = Some(env),
            }
        }
        self.render();
    }

    fn render(&mut self) {
        let clock = self.clock;
        self.clock += BLOCK as u64;
        let Some(cur) = self.current.as_deref_mut() else {
            self.out.silence(self.pos);
            return;
        };
        if self.idle {
            self.out.silence(cur.transport().pos());
            return;
        }
        let mut frames = match cur.render_block(clock, false) {
            Ok(f) => *f,
            Err(e) => {
                // A kernel trapped: silence it and stop, as an error can't play (the error
                // allocated; this path only).
                drop(e);
                self.crashes += 1;
                cur.transport_cmd(Cmd::Stop);
                [[0.0; 2]; BLOCK]
            }
        };
        let runs = *cur.last_runs();
        if let Some(f) = &mut self.fading
            && !f.finished
        {
            let old = match f.old.render_block(clock, f.first) {
                Ok(o) => *o,
                Err(e) => {
                    drop(e);
                    [[0.0; 2]; BLOCK]
                }
            };
            f.first = false;
            // Linear: the two are the same music, correlated (RENDER-25a).
            for (k, (x, o)) in frames.iter_mut().zip(old).enumerate() {
                let i = f.done + k as u32;
                if i < self.fade_len {
                    let g = (i + 1) as f32 / self.fade_len as f32;
                    *x = [o[0] * (1.0 - g) + x[0] * g, o[1] * (1.0 - g) + x[1] * g];
                }
            }
            f.done = (f.done + BLOCK as u32).min(self.fade_len);
        }
        self.out.load(&frames, &runs, cur.transport().pos());
        // RENDER-6a: idle only stopped, after 8 silent blocks, when nothing can sound later.
        let silent = frames.iter().all(|f| f[0].abs() < SILENT && f[1].abs() < SILENT);
        self.silent_blocks = if silent && !cur.transport().playing() { self.silent_blocks + 1 } else { 0 };
        if self.silent_blocks >= IDLE_AFTER && self.fading.is_none() && cur.can_idle() {
            self.idle = true;
        }
    }
}
