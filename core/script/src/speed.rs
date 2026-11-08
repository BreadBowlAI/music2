//! How fast this machine works, for the speed tests (CARD-75): a bound such a test checks is the
//! share of the work it may take on the machine the bound was set on, so here it is scaled by how
//! much slower this machine does a fixed piece of work.
//!
//! The work is plain native Rust on the test's own thread, sharing no code with what the speed
//! tests judge (no QuickJS, no picture runtime, no editor code), so a slowdown of that code can't
//! hide itself by slowing the reference too (review Y1): a filter run over a buffer it fills, and a
//! sort of keys it makes, each sample in fresh memory, timed in the thread's CPU time. Its median
//! of nine samples here, against [`REFERENCE`] (its median on the machine the bounds were set on),
//! is the ratio, clamped to 1 to 3: a bound is never tighter than it was set, nor more than 3 times
//! looser. Measured once a process and said once on stderr, past the test harness's capture, so a
//! log of passing tests shows it too.

use std::hint::black_box;
use std::io::Write;
use std::sync::OnceLock;
use std::time::Duration;

/// The work's median on the machine the bounds were set on: the shared Linux server (AMD Ryzen 9
/// 7950X3D, 32 threads, Ubuntu 24.04), 2026-11-08: nine processes at a load of 9.6 to 10.3 gave
/// 4.51 to 4.83 ms, their median 4.59 ms.
pub const REFERENCE: Duration = Duration::from_micros(REFERENCE_US);
const REFERENCE_US: u64 = 4_600;

/// The ratio's range: never tighter than the bound as set, never more than 3 times looser.
const CLAMP: (f64, f64) = (1.0, 3.0);

/// Samples in the median.
const SAMPLES: usize = 9;

/// The ratio of this machine's time for the work to [`REFERENCE`], clamped to 1 to 3.
pub fn ratio() -> f64 {
    static RATIO: OnceLock<f64> = OnceLock::new();
    *RATIO.get_or_init(|| {
        let here = median();
        let measured = here.as_secs_f64() / REFERENCE.as_secs_f64();
        let ratio = measured.clamp(CLAMP.0, CLAMP.1);
        let said = format!(
            "speed: a fixed native work (a filter and a sort) took {here:?} here (median of {SAMPLES}), {REFERENCE:?} on the Linux server: {measured:.2}, the speed tests' bounds × {ratio:.2} (clamped to 1 to 3)\n"
        );
        let _ = std::io::stderr().lock().write_all(said.as_bytes());
        ratio
    })
}

/// `bound` on this machine: `bound` times [`ratio`].
pub fn scaled(bound: Duration) -> Duration {
    bound.mul_f64(ratio())
}

/// The work's median here: [`SAMPLES`] samples, in the CPU time of this thread, the first left out
/// (its pages and caches cold).
pub fn median() -> Duration {
    work();
    let mut samples: Vec<Duration> = (0..SAMPLES)
        .map(|_| {
            let t = m2_os::thread::cpu_time();
            work();
            m2_os::thread::cpu_time().saturating_sub(t)
        })
        .collect();
    samples.sort();
    samples[SAMPLES / 2]
}

/// The fixed work: fill a buffer from a seeded generator, run a two-pole filter over it, then sort
/// keys made from what came out. The same every time; `black_box` keeps the optimiser from
/// dropping it.
fn work() {
    const N: usize = 3 << 16;
    let mut seed = black_box(0x2545_f491_4f6c_dd1d_u64);
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let input: Vec<f32> = (0..N).map(|_| (next() >> 40) as f32 / (1u64 << 24) as f32 - 0.5).collect();
    let (b0, b1, b2, a1, a2) = black_box((0.067_455_2_f32, 0.134_910_4, 0.067_455_2, -1.142_980_5, 0.412_801_4));
    let (mut x1, mut x2, mut y1, mut y2) = (0.0f32, 0.0, 0.0, 0.0);
    let mut out = Vec::with_capacity(N);
    for x in input {
        let y = b0 * x + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
        (x2, x1, y2, y1) = (x1, x, y1, y);
        out.push(y);
    }
    let mut keys: Vec<u32> = out.iter().map(|y| y.to_bits() ^ (next() as u32)).collect();
    keys.sort_unstable();
    black_box((out, keys));
}
