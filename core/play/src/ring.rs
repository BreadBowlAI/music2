//! The input's rings (RENDER-30a, D16): one writer, the input callback, and readers that each keep
//! their own place. Made whole when the input opens; the callback only stores atomics.
//!
//! A [`Ring`] holds the input's last frames at the engine's rate, numbered from 0 as they came.
//! The writer never waits for a reader: a reader that falls more than the ring behind finds those
//! frames gone, and plays or writes silence for them in their place, so time holds (D22). Each
//! frame is one `AtomicU64` (left and right as `f32` bits), so a torn read is never undefined;
//! the writer says how far it is about to write before it writes, and a reader checks that after
//! its copy, so a frame overwritten while it was copied is never taken for the old one.
//!
//! Each write may say which engine clock its first frame was heard at (D20): the ring keeps the
//! last [`STAMPS`] differences between clock and frame number, and a take places its frames by
//! their median.
//!
//! [`Queue`] carries an injected input's samples from a test's thread to the callback (D24).

use std::sync::atomic::{AtomicI64, AtomicU32, AtomicU64, Ordering, fence};

use m2_runtime::LiveInput;

/// Frames one store writes before it says it wrote them: a reader finds at most this many frames
/// near its oldest gone that a slower writer would have left.
const PIECE: usize = 1024;
/// Stamps kept.
pub const STAMPS: usize = 32;

/// The input's last frames, at the engine's rate, written by one callback.
pub struct Ring {
    frames: Box<[AtomicU64]>,
    /// Frames written, in all.
    head: AtomicU64,
    /// Frames written once the store under way is done: what may be overwritten now ends here.
    claim: AtomicU64,
    /// The last write's length: the input's buffer.
    last: AtomicU32,
    /// Engine clock less frame number, for the last [`STAMPS`] stamped writes.
    stamps: [AtomicI64; STAMPS],
    stamped: AtomicU64,
}

fn pack(f: [f32; 2]) -> u64 {
    f[0].to_bits() as u64 | (f[1].to_bits() as u64) << 32
}

fn unpack(x: u64) -> [f32; 2] {
    [f32::from_bits(x as u32), f32::from_bits((x >> 32) as u32)]
}

/// What a read found: frames there (copied), and of those, frames gone (silence in their place).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Read {
    pub frames: usize,
    pub gone: usize,
}

impl Ring {
    /// A ring of `len` frames, made here (never on the callback).
    pub fn new(len: usize) -> Ring {
        Ring {
            frames: (0..len.max(1)).map(|_| AtomicU64::new(0)).collect(),
            head: AtomicU64::new(0),
            claim: AtomicU64::new(0),
            last: AtomicU32::new(0),
            stamps: Default::default(),
            stamped: AtomicU64::new(0),
        }
    }

    fn cap(&self) -> u64 {
        self.frames.len() as u64
    }

    /// The callback's write: `frames`, after those before; `clock`, the engine clock its first
    /// frame was heard at, when known. Never allocates, locks or waits.
    pub fn write(&self, frames: &[[f32; 2]], clock: Option<u64>) {
        let first = self.head.load(Ordering::Relaxed);
        let mut h = first;
        for piece in frames.chunks(PIECE) {
            let to = h + piece.len() as u64;
            self.claim.store(to, Ordering::Relaxed);
            fence(Ordering::Release);
            for (i, f) in piece.iter().enumerate() {
                self.frames[((h + i as u64) % self.cap()) as usize].store(pack(*f), Ordering::Relaxed);
            }
            self.head.store(to, Ordering::Release);
            h = to;
        }
        if let Some(c) = clock {
            let n = self.stamped.load(Ordering::Relaxed);
            self.stamps[(n % STAMPS as u64) as usize].store(c as i64 - first as i64, Ordering::Relaxed);
            self.stamped.store(n + 1, Ordering::Release);
        }
        self.last.store(frames.len() as u32, Ordering::Relaxed);
    }

    /// Frames written so far.
    pub fn head(&self) -> u64 {
        self.head.load(Ordering::Acquire)
    }

    /// The last write's length, in frames.
    pub fn last(&self) -> u32 {
        self.last.load(Ordering::Relaxed)
    }

    /// Frames from number `at` into `out`, up to those written: how many, and how many of them were
    /// gone (overwritten before or while they were read: silence in `out`). Never allocates, locks
    /// or waits.
    pub fn read(&self, at: u64, out: &mut [[f32; 2]]) -> Read {
        let head = self.head();
        let n = (head.saturating_sub(at) as usize).min(out.len());
        let oldest = self.claim.load(Ordering::Relaxed).saturating_sub(self.cap());
        for (i, o) in out[..n].iter_mut().enumerate() {
            let k = at + i as u64;
            *o = if k < oldest { [0.0; 2] } else { unpack(self.frames[(k % self.cap()) as usize].load(Ordering::Relaxed)) };
        }
        // What the writer may have overwritten while this copied: a frame read then is gone.
        fence(Ordering::Acquire);
        let oldest = self.claim.load(Ordering::Relaxed).saturating_sub(self.cap());
        let gone = (oldest.saturating_sub(at) as usize).min(n);
        out[..gone].fill([0.0; 2]);
        Read { frames: n, gone }
    }

    /// The engine clock frame `0` was heard at, by the median of the last stamps: None before any.
    pub fn offset(&self) -> Option<i64> {
        let n = self.stamped.load(Ordering::Acquire).min(STAMPS as u64) as usize;
        if n == 0 {
            return None;
        }
        let mut seen: [i64; STAMPS] = [0; STAMPS];
        for (s, x) in seen.iter_mut().zip(&self.stamps[..n]) {
            *s = x.load(Ordering::Relaxed);
        }
        seen[..n].sort_unstable();
        Some(seen[n / 2])
    }
}

impl LiveInput for Ring {
    fn head(&self) -> (u64, u32) {
        (Ring::head(self), self.last())
    }

    fn read(&self, at: u64, out: &mut [[f32; 2]]) -> usize {
        Ring::read(self, at, out).frames
    }
}

/// Samples queued for an injected input: one writer (a test, a route), one reader (the callback).
/// The writer waits for room; the reader never waits.
pub struct Queue {
    samples: Box<[AtomicU32]>,
    written: AtomicU64,
    taken: AtomicU64,
}

impl Queue {
    pub fn new(len: usize) -> Queue {
        Queue { samples: (0..len.max(1)).map(|_| AtomicU32::new(0)).collect(), written: AtomicU64::new(0), taken: AtomicU64::new(0) }
    }

    fn cap(&self) -> u64 {
        self.samples.len() as u64
    }

    /// Queue `samples`, waiting for room while the callback takes them; false when it waited
    /// `up_to` for room and some weren't queued.
    pub fn put(&self, samples: &[f32], up_to: std::time::Duration) -> bool {
        let t = std::time::Instant::now();
        let mut rest = samples;
        while !rest.is_empty() {
            let w = self.written.load(Ordering::Relaxed);
            let room = (self.cap() - (w - self.taken.load(Ordering::Acquire))) as usize;
            if room == 0 {
                if t.elapsed() >= up_to {
                    return false;
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
                continue;
            }
            let n = room.min(rest.len());
            for (i, s) in rest[..n].iter().enumerate() {
                self.samples[((w + i as u64) % self.cap()) as usize].store(s.to_bits(), Ordering::Relaxed);
            }
            self.written.store(w + n as u64, Ordering::Release);
            rest = &rest[n..];
        }
        true
    }

    /// The callback's take: the next samples into `out`, silence for those not queued. Never
    /// allocates, locks or waits.
    pub fn take(&self, out: &mut [f32]) {
        let t = self.taken.load(Ordering::Relaxed);
        let n = ((self.written.load(Ordering::Acquire) - t) as usize).min(out.len());
        for (i, o) in out[..n].iter_mut().enumerate() {
            *o = f32::from_bits(self.samples[((t + i as u64) % self.cap()) as usize].load(Ordering::Relaxed));
        }
        out[n..].fill(0.0);
        self.taken.store(t + n as u64, Ordering::Release);
    }

    /// Samples queued and not yet taken.
    pub fn queued(&self) -> u64 {
        self.written.load(Ordering::Acquire) - self.taken.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reader_keeps_its_own_place_and_finds_what_was_overwritten_gone() {
        let r = Ring::new(8);
        let f = |k: u64| [k as f32, -(k as f32)];
        r.write(&(0..5).map(f).collect::<Vec<_>>(), Some(100));
        let mut out = [[9.0; 2]; 4];
        assert_eq!(r.read(1, &mut out), Read { frames: 4, gone: 0 });
        assert_eq!(out, [f(1), f(2), f(3), f(4)]);
        assert_eq!(r.read(5, &mut out).frames, 0, "nothing past the head");
        r.write(&(5..12).map(f).collect::<Vec<_>>(), Some(105));
        assert_eq!(r.read(2, &mut out), Read { frames: 4, gone: 2 }, "frames 2 and 3 were overwritten");
        assert_eq!(out, [[0.0; 2], [0.0; 2], f(4), f(5)]);
        assert_eq!(r.offset(), Some(100), "clock less frame number");
        assert_eq!(r.last(), 7);
    }

    #[test]
    fn a_torn_input_frame_is_never_heard() {
        // The writer writes frames whose two halves say their number; a reader near the oldest frame,
        // or behind it, races it.
        let r = std::sync::Arc::new(Ring::new(256));
        let w = r.clone();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let s = stop.clone();
        let writer = std::thread::spawn(move || {
            let mut k = 0u64;
            let mut buf = [[0.0f32; 2]; 100];
            while !s.load(Ordering::Relaxed) {
                for b in &mut buf {
                    k += 1;
                    *b = [k as f32, k as f32];
                }
                w.write(&buf, None);
            }
        });
        let mut out = [[0.0f32; 2]; 200];
        while r.head() < 10_000 {
            std::hint::spin_loop();
        }
        let (mut checked, mut gone) = (0, 0);
        for i in 0..20_000u64 {
            // From well inside the ring to past its oldest frame.
            let at = r.head().saturating_sub(100 + i % 200);
            let got = r.read(at, &mut out);
            for (i, o) in out[..got.frames].iter().enumerate().skip(got.gone) {
                assert_eq!(*o, [(at + i as u64 + 1) as f32; 2], "frame {} read whole and right", at + i as u64);
            }
            (checked, gone) = (checked + got.frames - got.gone, gone + got.gone);
        }
        assert!(checked > 1000 && gone > 0, "it raced: {checked} frames checked, {gone} found gone");
        stop.store(true, Ordering::Relaxed);
        writer.join().unwrap();
    }

    #[test]
    fn the_queue_hands_over_in_order_and_silence_when_empty() {
        let q = Queue::new(4);
        assert!(q.put(&[1.0, 2.0, 3.0], std::time::Duration::ZERO));
        assert!(!q.put(&[4.0, 5.0], std::time::Duration::ZERO), "no room for the fifth");
        let mut out = [9.0; 6];
        q.take(&mut out);
        assert_eq!(out, [1.0, 2.0, 3.0, 4.0, 0.0, 0.0]);
        assert_eq!(q.queued(), 0);
    }
}
