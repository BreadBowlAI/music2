//! How a render's worker threads start (8a): wasmtime's per-thread state made at once, the one
//! floating-point mode (RENDER-2a), and, beside playback, the OS's background priority.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// A worker was refused the background priority it asked for.
static REFUSED: AtomicBool = AtomicBool::new(false);

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
