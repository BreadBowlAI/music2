//! How fast this machine works, for the speed tests (CARD-75): a bound such a test checks is the
//! share of the work it may take on the machine the bound was set on, so here it is scaled by how
//! much slower or faster this machine does the same fixed work.
//!
//! The work is one std picture's draw, the same each time: std's compress drawing its transfer
//! curve, twenty draws a sample, its two values in turn so nothing kept is reused, timed in the CPU
//! time of the runtime's thread. Its median of nine samples here, against [`REFERENCE`] (its
//! median on the machine the bounds were set on), is the ratio: a bound times the ratio is that
//! bound on this machine. Measured once a process, and said once on stderr, past the test
//! harness's capture, so a log of passing tests shows it too.

use std::io::Write;
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

use crate::pictures::{Ask, Drawn, Runner};

/// The work's median on the machine the bounds were set on: the shared Linux server (AMD Ryzen 9
/// 7950X3D, 32 threads, Ubuntu 24.04), 2026-11-08: nine runs at a load of 6 to 11 gave 4.41 to
/// 4.68 ms, their median 4.49 ms.
pub const REFERENCE: Duration = Duration::from_micros(4_500);

/// Draws in a sample, and samples in the median.
const DRAWS: usize = 20;
const SAMPLES: usize = 9;

/// The ratio of this machine's time for the work to [`REFERENCE`]: over 1 on a slower machine.
pub fn ratio() -> f64 {
    static RATIO: OnceLock<f64> = OnceLock::new();
    *RATIO.get_or_init(|| {
        let here = median();
        let ratio = here.as_secs_f64() / REFERENCE.as_secs_f64();
        let said = format!(
            "speed: std's compress draws its transfer curve {DRAWS} times in {here:?} here (median of {SAMPLES}), {REFERENCE:?} on the Linux server: the speed tests' bounds × {ratio:.2}\n"
        );
        let _ = std::io::stderr().lock().write_all(said.as_bytes());
        ratio
    })
}

/// `bound` on this machine: `bound` times [`ratio`].
pub fn scaled(bound: Duration) -> Duration {
    bound.mul_f64(ratio())
}

/// The work's median here: [`SAMPLES`] samples of [`DRAWS`] draws each, in the CPU time of the
/// runtime's thread.
pub fn median() -> Duration {
    let std = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std");
    let packages = vec![("std".to_string(), std)];
    let catalog = crate::describe(&packages);
    let descriptor = catalog.components.get("std:compress").cloned().unwrap_or_else(|| panic!("no std:compress: {:?}", catalog.problems));
    let ask = |threshold: f64| Ask {
        key: "speed".into(),
        packages: packages.clone(),
        song: None,
        module: descriptor.module.clone(),
        descriptor: descriptor.clone(),
        view: "transfer".into(),
        values: serde_json::json!({"threshold": threshold, "ratio": 4, "makeup": 0}).to_string(),
        memo: "std:compress\u{0}transfer".into(),
        rate: 48_000,
    };
    let runner = Runner::new();
    let draw = |threshold: f64| {
        let (drawn, took) = runner.draw_timed(ask(threshold));
        assert!(matches!(drawn, Drawn::Shapes(_)), "std's compress draws its transfer curve: {drawn:?}");
        took
    };
    // Its first draw loads the module and runs twice (CARD-64): left out.
    draw(-20.0);
    let mut samples: Vec<Duration> = (0..SAMPLES).map(|_| (0..DRAWS).map(|i| draw(if i % 2 == 0 { -24.0 } else { -20.0 })).sum()).collect();
    samples.sort();
    samples[SAMPLES / 2]
}
