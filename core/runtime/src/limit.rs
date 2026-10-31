//! The time limit on every call into a pack's kernel (ABI-6, ABI-7, `docs/kernel-abi.md`):
//! wasmtime's epoch interruption. The compiled code checks, at each function's start and each
//! loop's turn, whether the engine's epoch has reached the deadline its store was given, and traps
//! once it has. One thread, the ticker, moves the epoch on every [`TICK`]. Each call into a pack's
//! sandbox is given its deadline just before it starts ([`crate::sandbox`]), so a call still running
//! at it stops, wherever it is. The checks cost a call about a fifth of its time, so std's modules,
//! music2's own, are compiled without them, by an engine of their own
//! ([`crate::packages::engine`]).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use wasmtime::Engine;

/// How often the ticker moves the epoch on.
pub(crate) const TICK: Duration = Duration::from_millis(10);

/// How long one call into a kernel may run (ABI-6). A call is never stopped before its limit,
/// and is stopped within a [`TICK`] after it while the ticker keeps time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Limits {
    /// A call the live engine makes into a sealed sandbox of a plan prepared for it: `k_process`,
    /// `k_reset`, `k_active`.
    pub(crate) live: Duration,
    /// Every other call: a sandbox's birth and prewarm (live too), an offline render's, an offline
    /// kernel's.
    pub(crate) offline: Duration,
}

impl Default for Limits {
    /// 100 ms live: about 37 blocks at 48 kHz, which no working kernel's call comes near, and short
    /// enough that the song is heard again at once. 5 s for the rest, which wait for no device and
    /// may run at background priority, where a busy computer gives them the CPU late.
    fn default() -> Limits {
        Limits { live: Duration::from_millis(100), offline: Duration::from_secs(5) }
    }
}

impl Limits {
    /// The limits of a sandbox: of a plan the live engine plays (`live`), or of any other.
    pub(crate) fn of(self, live: bool) -> Deadline {
        Deadline { born: self.offline, sealed: if live { self.live } else { self.offline } }
    }
}

/// A sandbox's limits: for each call while it is born, and once it is sealed.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Deadline {
    pub(crate) born: Duration,
    pub(crate) sealed: Duration,
}

/// The ticks a call is given so that it is never stopped before `limit`: one more than the
/// limit's, as the epoch may move on just after the call starts.
pub(crate) fn ticks(limit: Duration) -> u64 {
    (limit.as_nanos().div_ceil(TICK.as_nanos()) as u64).saturating_add(1)
}

/// Ticks so far: the engine's epoch, as the ticker counts it. Wasmtime keeps its own, which it
/// doesn't show; the two move on together.
static EPOCH: AtomicU64 = AtomicU64::new(0);

/// The epoch now, by [`EPOCH`].
pub(crate) fn epoch() -> u64 {
    EPOCH.load(Ordering::Relaxed)
}

/// Start `engine`'s ticker: a thread that moves its epoch on every [`TICK`], for as long as the
/// engine lives.
pub(crate) fn start(engine: &Engine) {
    let engine = engine.weak();
    let ticker = std::thread::Builder::new().name("m2-kernel-ticker".into()).spawn(move || {
        loop {
            std::thread::sleep(TICK);
            let Some(engine) = engine.upgrade() else { return };
            engine.increment_epoch();
            EPOCH.fetch_add(1, Ordering::Relaxed);
        }
    });
    ticker.expect("a thread for the kernels' ticker");
}

/// The boxes whose birth ran past its limit, by their shape's hash (RENDER-25: the kernel, its
/// module's hash and its settings; an instrument's voices and modules): a kernel is deterministic,
/// so one would get stuck again. The next plan that would make one again is refused at once, in the
/// same words, rather than waiting the whole limit (ABI-7). Shared by every copy of a package set,
/// so for as long as a session's set lives; a rebuilt pack's module is another shape.
#[derive(Clone, Default)]
pub(crate) struct Stuck(Arc<Mutex<HashMap<u64, TooSlow>>>);

impl Stuck {
    /// How a box of shape `shape` got stuck being born, if one did.
    pub(crate) fn get(&self, shape: u64) -> Option<TooSlow> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).get(&shape).cloned()
    }

    /// A box of shape `shape` got stuck being born, as `slow` says. Shape 0 is no box's.
    pub(crate) fn insert(&self, shape: u64, slow: &TooSlow) {
        if shape != 0 {
            self.0.lock().unwrap_or_else(|e| e.into_inner()).insert(shape, slow.clone());
        }
    }
}

/// A call into a kernel ran past its limit and was stopped (ABI-6): told as `sound.too-slow`
/// (ABI-7).
#[derive(Clone, Debug)]
pub struct TooSlow {
    /// The node, `<module>/<id>`.
    pub node: String,
    /// The limit it ran past.
    pub limit: Duration,
}

impl std::fmt::Display for TooSlow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} took too long and was stopped: one call into it ran past {}", self.node, said(self.limit))
    }
}

impl std::error::Error for TooSlow {}

/// `d` in words: `100 ms`, `5 s`.
pub(crate) fn said(d: Duration) -> String {
    match d.as_millis() {
        ms if ms < 1000 || !ms.is_multiple_of(1000) => format!("{ms} ms"),
        ms => format!("{} s", ms / 1000),
    }
}
