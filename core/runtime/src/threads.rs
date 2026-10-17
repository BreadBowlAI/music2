//! How a render's worker threads start (8a): wasmtime's per-thread state made at once, the one
//! floating-point mode (RENDER-2a), and, beside playback, the OS's background priority. A live
//! player's workers are engine threads too: real time at the device's period, in its workgroup
//! (RENDER-28a, [`live`]).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use m2_os::rt::Group;

/// A worker was refused the background priority it asked for.
static REFUSED: AtomicBool = AtomicBool::new(false);

/// Live workers alive now, in every pool of this process.
static LIVE: AtomicUsize = AtomicUsize::new(0);

/// What a pool's workers do first, for [`m2_pool::Pool::new`]: with `background`, each lowers its
/// priority below the app's (`m2_os::thread::background`: an export beside playback). Workers spin
/// 50 µs for the next block before they sleep.
pub fn setup(background: bool) -> m2_pool::Setup {
    let start = move |_worker: usize| {
        wasmtime::Engine::tls_eager_initialize();
        crate::fpenv::canon();
        if background && m2_os::thread::background().is_err() {
            REFUSED.store(true, Ordering::Relaxed);
        }
    };
    m2_pool::Setup { start: Some(Arc::new(start)), ..m2_pool::Setup::plain() }
}

/// Whether any worker was refused the background priority (the gates say so).
pub fn background_refused() -> bool {
    REFUSED.load(Ordering::Relaxed)
}

/// What a live pool's workers got as they started: counted once each, before its first block.
#[derive(Debug, Default)]
pub struct Joins {
    pub started: AtomicUsize,
    pub real_time: AtomicUsize,
    pub joined: AtomicUsize,
    /// Refused by a cancelled workgroup: the output changed under it.
    pub cancelled: AtomicUsize,
}

/// A live player's workers (8a's live half, RENDER-28a): each makes wasmtime's per-thread state,
/// enters the one floating-point mode and becomes an engine thread of `group` (real time at its
/// period, in its workgroup), counted in the [`Joins`]; then `mark(true)` (tests: the allocation
/// trap) until `mark(false)` before it leaves the workgroup and exits. With nothing to do a worker
/// spins half a period (1.25 periods with `spin`, so the next callback finds it awake) before it
/// parks; the device's thread spins a period with nothing finishing before it dozes, a period at
/// most each time.
pub fn live(group: Arc<Group>, spin: bool, mark: Option<fn(bool)>) -> (m2_pool::Setup, Arc<Joins>) {
    let period = group.period().unwrap_or(Duration::from_millis(10));
    let joins = Arc::new(Joins::default());
    let start = {
        let joins = joins.clone();
        move |_worker: usize| {
            wasmtime::Engine::tls_eager_initialize();
            crate::fpenv::canon();
            let j = m2_os::rt::engine_thread(&group);
            for (n, yes) in [(&joins.real_time, j.real_time), (&joins.joined, j.workgroup), (&joins.cancelled, j.cancelled)] {
                n.fetch_add(usize::from(yes), Ordering::Relaxed);
            }
            LIVE.fetch_add(1, Ordering::SeqCst);
            joins.started.fetch_add(1, Ordering::SeqCst);
            if let Some(mark) = mark {
                mark(true);
            }
        }
    };
    let stop = move |_worker: usize| {
        if let Some(mark) = mark {
            mark(false);
        }
        m2_os::rt::end_engine_thread();
        LIVE.fetch_sub(1, Ordering::SeqCst);
    };
    let setup = m2_pool::Setup {
        start: Some(Arc::new(start)),
        stop: Some(Arc::new(stop)),
        patience: if spin { period * 5 / 4 } else { period / 2 },
        stuck: period,
        doze: period,
    };
    (setup, joins)
}

/// Live workers alive now in this process: started and not yet stopped.
pub fn live_workers() -> usize {
    LIVE.load(Ordering::SeqCst)
}

/// A count of threads, 1 to 64, as `--threads` and `M2_THREADS` give it.
pub fn parse(text: &str) -> Result<usize> {
    match text.trim().parse::<usize>() {
        Ok(n @ 1..=m2_pool::Pool::MAX_THREADS) => Ok(n),
        _ => bail!("say how many threads music2 may render on: a number from 1 to 64, not `{}`", text.trim()),
    }
}

/// What `M2_THREADS` says, if set: how many threads music2 renders on, live and offline.
pub fn from_env() -> Result<Option<usize>> {
    match std::env::var("M2_THREADS") {
        Ok(v) => parse(&v).map(Some).with_context(|| format!("M2_THREADS={v}")),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(e) => Err(anyhow!("M2_THREADS: {e}")),
    }
}
