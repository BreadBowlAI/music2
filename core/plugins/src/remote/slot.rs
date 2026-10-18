//! One active instance's slot in shared memory (HOST-3): where a `process` or `reset` call crosses
//! to the host process and back. A call is synchronous, a ring of one: the app copies the call in,
//! bumps the call word and wakes it; the instance's audio thread in the host process makes the
//! call and bumps the done word; the app spins a little, then sleeps on it. Nothing here allocates,
//! locks or does I/O (RENDER-28; `tests/no_blocking.rs` reads this file).
//!
//! | Offset | What |
//! |---|---|
//! | 0 | the [`Header`]: the two words, the call's op and sizes, its transport, what came back |
//! | 512 | audio in: 2 channels × `max_frames` f32 |
//! | then | audio out: 2 channels × `max_frames` f32 |
//! | then | events in: [`MAX_CALL_EVENTS`] in CLAP's own layout |
//! | then | events out: [`OUT_EVENTS`] |

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use m2_os::ipc;

use crate::Transport;
use crate::host::{Events, MAX_CALL_EVENTS, Raw, SharedBuffers, Status};

/// Events one call may output; more are counted.
pub(crate) const OUT_EVENTS: usize = 256;
const HEADER: usize = 512;
/// The spin before the app sleeps on a call: most calls are done by then.
const SPIN: Duration = Duration::from_micros(20);
/// How long one sleep on the done word lasts before the app looks at the process again.
const NAP: Duration = Duration::from_millis(20);

pub(crate) const PROCESS: u32 = 1;
pub(crate) const RESET: u32 = 2;
pub(crate) const QUIT: u32 = 3;

/// The transport, packed into words: [`pack`] and [`unpack`].
const T_WORDS: usize = 11;

#[repr(C)]
pub(crate) struct Header {
    /// Bumped by the app for each call; the host process's audio thread sleeps on it.
    pub call: AtomicU32,
    /// Set to the call's number once it is done; the app sleeps on it.
    pub done: AtomicU32,
    pub op: AtomicU32,
    pub frames: AtomicU32,
    pub n_in: AtomicU32,
    pub n_out: AtomicU32,
    pub ev_in: AtomicU32,
    pub ev_out: AtomicU32,
    pub ev_out_dropped: AtomicU32,
    pub status: AtomicU32,
    /// The plugin asked to be processed (CLAP's `request_process`): set by the host process, taken
    /// by the app.
    pub requested: AtomicU32,
    pub max_frames: AtomicU32,
    transport: [AtomicU64; T_WORDS],
}

const _: () = assert!(size_of::<Header>() <= HEADER);

/// The bytes a slot of `max_frames` takes.
pub(crate) fn size(max_frames: u32) -> usize {
    let audio = 4 * max_frames as usize * size_of::<f32>();
    HEADER + audio.next_multiple_of(64) + (MAX_CALL_EVENTS + OUT_EVENTS) * size_of::<Raw>()
}

/// A mapped slot.
#[derive(Clone, Copy)]
pub(crate) struct View {
    base: *mut u8,
    max_frames: usize,
}

// SAFETY: a view of shared memory; who touches what when is the protocol's rule (the module doc).
unsafe impl Send for View {}
unsafe impl Sync for View {}

impl View {
    /// # Safety
    /// `base` is a mapped slot of [`size`]`(max_frames)` bytes that outlives the view.
    pub(crate) unsafe fn new(base: *mut u8, max_frames: u32) -> View {
        View { base, max_frames: max_frames as usize }
    }

    pub(crate) fn header(&self) -> &Header {
        // SAFETY: the slot starts with its header, page-aligned.
        unsafe { &*(self.base as *const Header) }
    }

    fn audio(&self, out: bool, ch: usize) -> *mut f32 {
        let i = (out as usize) * 2 + ch.min(1);
        // SAFETY: within the slot's audio part.
        unsafe { (self.base.add(HEADER) as *mut f32).add(i * self.max_frames) }
    }

    fn events(&self, out: bool) -> *mut Raw {
        let audio = (4 * self.max_frames * size_of::<f32>()).next_multiple_of(64);
        // SAFETY: within the slot's event part, aligned to 64.
        unsafe { (self.base.add(HEADER + audio) as *mut Raw).add(if out { MAX_CALL_EVENTS } else { 0 }) }
    }
}

fn pack(t: &Transport) -> [u64; T_WORDS] {
    let ((ls, le), (lss, les)) = t.looping.unwrap_or_default();
    let flags = t.playing as u64 | (t.looping.is_some() as u64) << 1 | (t.tsig.0 as u64) << 16 | (t.tsig.1 as u64) << 32;
    let f = f64::to_bits;
    [f(t.beats), f(t.seconds), f(t.bpm), flags, f(ls), f(le), f(lss), f(les), f(t.bar_start), t.bar as i64 as u64, t.steady as u64]
}

fn unpack(w: &[u64; T_WORDS]) -> Transport {
    let f = |i: usize| f64::from_bits(w[i]);
    Transport {
        beats: f(0),
        seconds: f(1),
        bpm: f(2),
        playing: w[3] & 1 != 0,
        looping: (w[3] & 2 != 0).then(|| ((f(4), f(5)), (f(6), f(7)))),
        tsig: ((w[3] >> 16) as u16, (w[3] >> 32) as u16),
        bar_start: f(8),
        bar: w[9] as i64 as i32,
        steady: w[10] as i64,
    }
}

/// What a waiting call watches besides its slot: its instance's process dying, and the watchdog's
/// clock.
pub(crate) struct Waiter<'a> {
    pub dead: &'a AtomicBool,
    /// When this live call began (ns since `epoch`, never 0); 0 when none is in flight or the
    /// call is offline, which waits with no limit.
    pub since: &'a AtomicU64,
    pub live: bool,
    pub epoch: Instant,
}

impl Waiter<'_> {
    /// Ring the call in `v` and wait for it: false when the process died first.
    fn ring(&self, v: &View) -> bool {
        let h = v.header();
        let c = h.call.load(Ordering::Relaxed).wrapping_add(1);
        if self.live {
            self.since.store((self.epoch.elapsed().as_nanos() as u64).max(1), Ordering::Relaxed);
        }
        h.call.store(c, Ordering::Release);
        ipc::wake(&h.call);
        let start = Instant::now();
        let done = loop {
            if h.done.load(Ordering::Acquire) == c {
                break true;
            }
            if self.dead.load(Ordering::Acquire) {
                break false;
            }
            if start.elapsed() < SPIN {
                std::hint::spin_loop();
            } else {
                ipc::wait(&h.done, c.wrapping_sub(1), NAP);
            }
        };
        self.since.store(0, Ordering::Relaxed);
        done
    }
}

/// Silence every output of `io`.
pub(crate) fn silence(io: &mut SharedBuffers) {
    let n = io.frames as usize;
    for o in io.outputs.iter_mut().take(io.n_out) {
        let len = n.min(o.len());
        o[..len].fill(0.0);
    }
}

/// The app's side of a `process` call: copied in, made in the host process, copied out. The
/// caller checked that `io.frames` and `ev_in` fit the slot.
pub(crate) fn process(v: &View, w: &Waiter, io: &mut SharedBuffers, ev_in: &Events, ev_out: &mut Events, t: &Transport) -> Status {
    let h = v.header();
    let n = io.frames as usize;
    for ch in 0..io.n_in.min(2) {
        // SAFETY: the slot's input channel holds `max_frames` ≥ `n` samples; only this side
        // writes it, and only while no call is in flight.
        unsafe { std::ptr::copy_nonoverlapping(io.inputs[ch].as_ptr(), v.audio(false, ch), n.min(io.inputs[ch].len())) };
    }
    let raw = ev_in.raw();
    // SAFETY: the slot holds `MAX_CALL_EVENTS` ≥ `raw.len()` events in.
    unsafe { std::ptr::copy_nonoverlapping(raw.as_ptr(), v.events(false), raw.len()) };
    for (word, x) in h.transport.iter().zip(pack(t)) {
        word.store(x, Ordering::Relaxed);
    }
    h.frames.store(io.frames, Ordering::Relaxed);
    h.n_in.store(io.n_in.min(2) as u32, Ordering::Relaxed);
    h.n_out.store(io.n_out.min(2) as u32, Ordering::Relaxed);
    h.ev_in.store(raw.len() as u32, Ordering::Relaxed);
    h.op.store(PROCESS, Ordering::Relaxed);
    if !w.ring(v) {
        silence(io);
        return Status::Error;
    }
    for ch in 0..io.n_out.min(2) {
        let o = &mut io.outputs[ch];
        let len = n.min(o.len());
        // SAFETY: the slot's output channel holds `max_frames` ≥ `len` samples, written by the
        // call that is done.
        unsafe { std::ptr::copy_nonoverlapping(v.audio(true, ch), o.as_mut_ptr(), len) };
    }
    let back = (h.ev_out.load(Ordering::Relaxed) as usize).min(OUT_EVENTS);
    // SAFETY: the call wrote `back` whole events out.
    ev_out.append_raw(unsafe { std::slice::from_raw_parts(v.events(true), back) }, h.ev_out_dropped.load(Ordering::Relaxed));
    status(h.status.load(Ordering::Relaxed))
}

/// The app's side of a `reset` call.
pub(crate) fn reset(v: &View, w: &Waiter) {
    v.header().op.store(RESET, Ordering::Relaxed);
    w.ring(v);
}

fn status(s: u32) -> Status {
    match s {
        0 => Status::Continue,
        1 => Status::ContinueIfNotQuiet,
        2 => Status::Tail,
        3 => Status::Sleep,
        _ => Status::Error,
    }
}

fn status_word(s: Status) -> u32 {
    match s {
        Status::Continue => 0,
        Status::ContinueIfNotQuiet => 1,
        Status::Tail => 2,
        Status::Sleep => 3,
        Status::Error => 4,
    }
}

/// The host process's side: the instance's audio thread sleeps on its slot and makes each call
/// with `call` (process) and `reset`, then puts the one float mode back (RENDER-2a). Returns at
/// [`QUIT`]. `ev_in` and `ev_out` were made with room for [`MAX_CALL_EVENTS`] and [`OUT_EVENTS`].
pub(crate) fn serve(
    v: &View,
    ev_in: &mut Events,
    ev_out: &mut Events,
    mut call: impl FnMut(&mut SharedBuffers, &Events, &mut Events, &Transport) -> Status,
    mut reset: impl FnMut(),
) {
    let h = v.header();
    // Calls up to the last one done are answered: one rung before this thread started is not.
    let mut seen = h.done.load(Ordering::Acquire);
    loop {
        let c = h.call.load(Ordering::Acquire);
        if c == seen {
            ipc::wait(&h.call, seen, Duration::from_secs(1));
            continue;
        }
        seen = c;
        match h.op.load(Ordering::Relaxed) {
            PROCESS => {
                let n = (h.frames.load(Ordering::Relaxed) as usize).min(v.max_frames);
                let mut words = [0u64; T_WORDS];
                for (x, word) in words.iter_mut().zip(&h.transport) {
                    *x = word.load(Ordering::Relaxed);
                }
                let t = unpack(&words);
                let count = (h.ev_in.load(Ordering::Relaxed) as usize).min(MAX_CALL_EVENTS);
                // SAFETY: the app wrote `count` whole events in before ringing.
                ev_in.clear();
                ev_in.append_raw(unsafe { std::slice::from_raw_parts(v.events(false), count) }, 0);
                ev_out.clear();
                // SAFETY: the slot's channels, `max_frames` ≥ `n` long; the app touches none of
                // them while the call is in flight.
                let (i0, i1, o0, o1) = unsafe {
                    (
                        std::slice::from_raw_parts(v.audio(false, 0), n),
                        std::slice::from_raw_parts(v.audio(false, 1), n),
                        std::slice::from_raw_parts_mut(v.audio(true, 0), n),
                        std::slice::from_raw_parts_mut(v.audio(true, 1), n),
                    )
                };
                let mut io = SharedBuffers {
                    inputs: [i0, i1],
                    n_in: h.n_in.load(Ordering::Relaxed) as usize,
                    outputs: [o0, o1],
                    n_out: h.n_out.load(Ordering::Relaxed) as usize,
                    frames: n as u32,
                };
                let s = call(&mut io, ev_in, ev_out, &t);
                let out = ev_out.raw();
                // SAFETY: the slot holds `OUT_EVENTS` events out, and `ev_out` holds no more.
                unsafe { std::ptr::copy_nonoverlapping(out.as_ptr(), v.events(true), out.len().min(OUT_EVENTS)) };
                h.ev_out.store(out.len().min(OUT_EVENTS) as u32, Ordering::Relaxed);
                h.ev_out_dropped.store(ev_out.take_dropped(), Ordering::Relaxed);
                h.status.store(status_word(s), Ordering::Relaxed);
            }
            RESET => reset(),
            _ => {
                h.done.store(c, Ordering::Release);
                ipc::wake(&h.done);
                return;
            }
        }
        // A plugin may leave its thread flushing subnormals: its next call computes as the
        // engine's would (RENDER-2a).
        m2_os::fp::canon();
        h.done.store(c, Ordering::Release);
        ipc::wake(&h.done);
    }
}

/// Ask the audio thread serving `v` to return, and wait for it (off the audio thread).
pub(crate) fn quit(v: &View) {
    let h = v.header();
    h.op.store(QUIT, Ordering::Relaxed);
    let c = h.call.load(Ordering::Relaxed).wrapping_add(1);
    h.call.store(c, Ordering::Release);
    ipc::wake(&h.call);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_transport_packs_and_unpacks_whole() {
        let t = Transport {
            beats: 12.5,
            seconds: 6.25,
            bpm: 120.0,
            playing: true,
            looping: Some(((4.0, 8.0), (2.0, 4.0))),
            tsig: (7, 8),
            bar_start: 10.5,
            bar: -1,
            steady: 1 << 40,
        };
        assert_eq!(format!("{:?}", unpack(&pack(&t))), format!("{t:?}"));
        let t = Transport::default();
        assert_eq!(format!("{:?}", unpack(&pack(&t))), format!("{t:?}"));
    }
}
