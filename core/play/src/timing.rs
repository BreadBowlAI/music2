//! A record of every callback's time (8a's live half, G12).
//!
//! In short:
//! - Each callback adds its time to a histogram, counts a gap when it starts over 1.5 periods after
//!   the one before, and counts the device frames it filled. Relaxed atomics only: no lock, no
//!   allocation (RENDER-28).
//! - [`Player::timing`] copies the record, off the audio thread, with what the player counts
//!   elsewhere: late callbacks (8b.8's count), the device's dropouts, crashes, non-finite samples,
//!   the engine clock, and the engine threads' counters.
//! - [`Timing::since`] is what happened between two copies; [`Timing::quantile`] reads its
//!   callback times.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering::Relaxed};
use std::time::Duration;

use m2_runtime::Counters;

use crate::Player;

/// The histogram's buckets: 4 µs each, to 8.192 ms. A longer callback counts in [`Timing::over`].
pub const BUCKET_NS: u64 = 4_000;
const BUCKETS: usize = 2048;

/// What the callback writes: its own, so every field has one writer.
pub(crate) struct Record {
    hist: [AtomicU32; BUCKETS],
    over: AtomicU64,
    max_ns: AtomicU64,
    callbacks: AtomicU64,
    gaps: AtomicU64,
    frames: AtomicU64,
    /// The last callback's start, in [`crate::now_ns`]'s clock: 0 before the first.
    last: AtomicU64,
    /// The engine playing: its crashes, and its renderer's non-finite samples.
    crashes: AtomicU64,
    non_finite: AtomicU64,
}

impl Default for Record {
    fn default() -> Record {
        Record {
            hist: std::array::from_fn(|_| AtomicU32::new(0)),
            over: AtomicU64::new(0),
            max_ns: AtomicU64::new(0),
            callbacks: AtomicU64::new(0),
            gaps: AtomicU64::new(0),
            frames: AtomicU64::new(0),
            last: AtomicU64::new(0),
            crashes: AtomicU64::new(0),
            non_finite: AtomicU64::new(0),
        }
    }
}

impl Record {
    /// One callback: it started at `start`, took `took` ns and filled `frames` frames, whose time
    /// at the device's rate is `period` ns.
    pub(crate) fn callback(&self, start: u64, took: u64, frames: usize, period: u64) {
        let last = self.last.load(Relaxed);
        self.last.store(start, Relaxed);
        if last > 0 && start.saturating_sub(last).saturating_mul(2) > period.saturating_mul(3) {
            self.gaps.fetch_add(1, Relaxed);
        }
        match self.hist.get((took / BUCKET_NS) as usize) {
            Some(n) => {
                n.fetch_add(1, Relaxed);
            }
            None => {
                self.over.fetch_add(1, Relaxed);
            }
        }
        self.max_ns.fetch_max(took, Relaxed);
        self.frames.fetch_add(frames as u64, Relaxed);
        self.callbacks.fetch_add(1, Relaxed);
    }

    /// The engine playing, after a callback: its crashes and its renderer's stopped sounds, and the
    /// renderer's non-finite samples.
    pub(crate) fn engine(&self, crashes: u64, non_finite: u64) {
        self.crashes.store(crashes, Relaxed);
        self.non_finite.store(non_finite, Relaxed);
    }
}

/// What a player's callbacks did, from its start or between two copies ([`Timing::since`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Timing {
    /// Callbacks, and their times by [`BUCKET_NS`] buckets; `over` took longer than the last.
    pub callbacks: u64,
    pub hist: Vec<u64>,
    pub over: u64,
    /// The longest callback, in ns: since the player started, or in a window to within a bucket.
    pub max_ns: u64,
    /// Callbacks longer than their period (8b.8's count): each a dropout.
    pub late: u64,
    /// Callbacks that started over 1.5 periods after the one before: the device came late.
    pub gaps: u64,
    /// Dropouts the device reported (CoreAudio's overloads, ALSA's xruns).
    pub xruns: u64,
    /// Device frames filled; engine frames rendered (the engine clock).
    pub frames: u64,
    pub clock: u64,
    /// The player crashed (a panic: for good); the engine's crashes (a sound that stopped, as a
    /// kernel failed (RENDER-25), or an error that reached the engine); the playing renderer's
    /// non-finite samples, silenced.
    pub crashed: bool,
    pub crashes: u64,
    pub non_finite: u64,
    /// The pool's counters, the device's thread first: empty on one thread.
    pub threads: Vec<Counters>,
}

impl Timing {
    /// What happened since `before`, an earlier copy of the same player's.
    pub fn since(&self, before: &Timing) -> Timing {
        let hist: Vec<u64> = self.hist.iter().zip(&before.hist).map(|(a, b)| a.saturating_sub(*b)).collect();
        let over = self.over.saturating_sub(before.over);
        // The longest since the player started, when it came in this window; else the top bucket.
        let max_ns = match self.max_ns > before.max_ns {
            true => self.max_ns,
            false if over > 0 => before.max_ns,
            false => hist.iter().rposition(|&n| n > 0).map_or(0, |k| (k as u64 + 1) * BUCKET_NS),
        };
        let threads = match self.threads.len() == before.threads.len() {
            true => self.threads.iter().zip(&before.threads).map(|(a, b)| minus(a, b)).collect(),
            false => self.threads.clone(),
        };
        let d = |a: u64, b: u64| a.saturating_sub(b);
        Timing {
            callbacks: d(self.callbacks, before.callbacks),
            hist,
            over,
            max_ns,
            late: d(self.late, before.late),
            gaps: d(self.gaps, before.gaps),
            xruns: d(self.xruns, before.xruns),
            frames: d(self.frames, before.frames),
            clock: d(self.clock, before.clock),
            crashed: self.crashed,
            crashes: d(self.crashes, before.crashes),
            non_finite: d(self.non_finite, before.non_finite),
            threads,
        }
    }

    /// The callback time below which a share `q` of callbacks took (the top of its bucket; the
    /// longest when it is past the histogram). None with no callback.
    pub fn quantile(&self, q: f64) -> Option<Duration> {
        let n = self.hist.iter().sum::<u64>() + self.over;
        if n == 0 {
            return None;
        }
        let want = ((q.clamp(0.0, 1.0) * n as f64).ceil() as u64).max(1);
        let mut seen = 0;
        for (k, &c) in self.hist.iter().enumerate() {
            seen += c;
            if seen >= want {
                return Some(Duration::from_nanos((k as u64 + 1) * BUCKET_NS));
            }
        }
        Some(Duration::from_nanos(self.max_ns))
    }

    /// Dropouts: the device's, late callbacks, and gaps.
    pub fn dropouts(&self) -> u64 {
        self.xruns + self.late + self.gaps
    }
}

/// One thread's counters since `b`.
fn minus(a: &Counters, b: &Counters) -> Counters {
    Counters {
        tasks: a.tasks.saturating_sub(b.tasks),
        parks: a.parks.saturating_sub(b.parks),
        signals: a.signals.saturating_sub(b.signals),
        dozes: a.dozes.saturating_sub(b.dozes),
        rung: a.rung.saturating_sub(b.rung),
        timeouts: a.timeouts.saturating_sub(b.timeouts),
        entry: std::array::from_fn(|k| a.entry[k].saturating_sub(b.entry[k])),
    }
}

impl Player {
    /// What its callbacks did so far (G12). Copies the record: off the audio thread.
    pub fn timing(&self) -> Timing {
        let (r, s) = (&self.shared.timing, &self.shared);
        Timing {
            callbacks: r.callbacks.load(Relaxed),
            hist: r.hist.iter().map(|n| u64::from(n.load(Relaxed))).collect(),
            over: r.over.load(Relaxed),
            max_ns: r.max_ns.load(Relaxed),
            late: s.late.load(Relaxed),
            gaps: r.gaps.load(Relaxed),
            xruns: s.device_errors().xruns,
            frames: r.frames.load(Relaxed),
            clock: s.clock.load(Relaxed),
            crashed: s.crashed.load(Relaxed),
            crashes: r.crashes.load(Relaxed),
            non_finite: r.non_finite.load(Relaxed),
            threads: s.engines.counters(),
        }
    }

    /// Whether the device hears zeros: the callback renders as ever, then writes silence (G12
    /// plays muted, as `ci/check.sh` never plays sound).
    pub fn mute_output(&self, on: bool) {
        self.shared.muted.store(on, Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering::SeqCst};

    use m2_runtime::Trip;

    use super::*;
    use crate::Driver;

    /// One callback of `n` frames, stereo.
    fn fill(d: &mut Driver, n: usize) {
        d.fill(&mut vec![0.0f32; 2 * n], 2);
    }

    // G12 (8a's live half): a callback longer than its period is late, and one that starts over
    // 1.5 periods after the one before is a gap. At 128 frames (2.67 ms): a block held 50 ms, then
    // a pause of 10 ms before the next callback.
    #[test]
    fn a_late_callback_and_a_gap_are_counted() {
        let (player, mut d) = Player::driven(48_000);
        player.open(crate::tests::sine(48_000)).unwrap();
        player.play();
        fill(&mut d, 128);
        let before = player.timing();
        let open = Arc::new(AtomicBool::new(false));
        let at = d.runner().runs() + 1;
        d.runner().trip(at, Trip::Hold(open.clone()));
        let opener = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            open.store(true, SeqCst);
        });
        fill(&mut d, 128);
        opener.join().unwrap();
        std::thread::sleep(Duration::from_millis(10));
        fill(&mut d, 128);
        let t = player.timing().since(&before);
        assert_eq!(d.runner().tripped_on(), Some(0), "the held block ran here");
        assert!(t.late >= 1 && t.gaps >= 1 && t.dropouts() >= 2, "{t:?}");
        assert!(t.max_ns >= 40_000_000 && t.quantile(1.0) >= Some(Duration::from_millis(40)), "{t:?}");
    }

    // G12 (8a's live half): each callback counts once in the histogram, with its frames; a copy
    // since itself is empty; the quantiles read the buckets.
    #[test]
    fn the_histogram_counts_each_callback_once() {
        let (player, mut d) = Player::driven(48_000);
        player.open(crate::tests::sine(48_000)).unwrap();
        player.play();
        let sizes = [64usize, 128, 1, 480, 4096, 37, 128];
        for n in sizes {
            fill(&mut d, n);
        }
        let t = player.timing();
        assert_eq!((t.callbacks, t.hist.iter().sum::<u64>() + t.over), (7, 7), "{t:?}");
        assert_eq!(t.frames, sizes.iter().sum::<usize>() as u64);
        assert!(t.clock >= t.frames && t.clock < t.frames + m2_runtime::BLOCK as u64, "the engine renders ahead by a block at most");
        assert_eq!(t.since(&t), Timing { hist: vec![0; BUCKETS], max_ns: 0, crashed: t.crashed, ..Timing::default() });
        let (p50, top) = (t.quantile(0.5).unwrap(), t.quantile(1.0).unwrap());
        assert!(p50 <= top && top.as_nanos() as u64 >= t.max_ns.min(BUCKETS as u64 * BUCKET_NS), "{p50:?} {top:?} {}", t.max_ns);
        assert_eq!(Timing::default().quantile(0.99), None);
        assert!(!t.crashed && t.crashes == 0 && t.non_finite == 0 && t.threads.is_empty());
    }

    // G12 (8a's live half), RENDER-25: live, a node that fails stops alone and the song plays on;
    // the record counts it as a crash, so G12 fails on it.
    #[test]
    fn a_sound_that_stops_counts_as_a_crash() {
        let (player, mut d) = Player::driven(48_000);
        let r = crate::tests::sine(48_000);
        let info = r.info();
        player.open(r).unwrap();
        player.play();
        fill(&mut d, 128);
        let before = player.timing();
        // The node fails as the audio thread would mark it.
        info.stopped.stop(1, m2_runtime::Why::Crashed);
        fill(&mut d, 128);
        let t = player.timing().since(&before);
        assert_eq!((t.crashed, t.crashes), (false, 1), "the sound's stop is counted");
    }

    // G12 (8a's live half): a muted player renders as ever, and the device hears zeros.
    #[test]
    fn a_muted_player_renders_and_the_device_hears_zeros() {
        let (player, mut d) = Player::driven(48_000);
        player.open(crate::tests::sine(48_000)).unwrap();
        player.play();
        player.mute_output(true);
        let mut data = vec![0.0f32; 2 * 4096];
        d.fill(&mut data, 2);
        assert!(data.iter().all(|&x| x == 0.0) && player.timing().clock >= 4096, "rendered, not heard");
        player.mute_output(false);
        d.fill(&mut data, 2);
        assert!(data.iter().any(|&x| x.abs() > 0.1), "heard again");
    }
}
