//! The kernel host (RENDER-32, `docs/designs/live/web-engine.md` §2): what runs kernel modules.
//! The runtime reaches its kernels through these two traits only: [`KernelHost`] compiles a module
//! and makes its sandboxes, and a [`Sandbox`] is one instance of it and its one memory, whose
//! exports the runtime calls and whose bytes it copies in and out.
//!
//! One host is built in, chosen by Cargo feature, never by target (PLAT-2): wasmtime, the default
//! feature `wasmtime` ([`wasmtime`]); without it, none, which refuses every module ([`none`]). The
//! runtime names it once, [`Host`], so every call is a plain call: no dynamic dispatch, on the
//! audio thread or anywhere.
//!
//! What a host doesn't decide is the runtime's, the same for every host: ABI-3's checks of a
//! module, read from its bytes ([`crate::abi`]); a state's block and settings
//! ([`crate::sandbox::Sandbox::state`]); what `m2.asset_read` reads ([`crate::sandbox::Assets`]);
//! and the words a fault is told in ([`crate::sandbox::Sandbox::ran_out`]).

use std::time::Duration;

use anyhow::Result;

use crate::abi::Call;
use crate::sandbox::Assets;

#[cfg(not(feature = "wasmtime"))]
mod none;
#[cfg(feature = "wasmtime")]
pub mod wasmtime;

/// The kernel host this build runs.
#[cfg(feature = "wasmtime")]
pub(crate) type Host = self::wasmtime::Wasmtime;
#[cfg(not(feature = "wasmtime"))]
pub(crate) type Host = none::NoHost;

/// A module, compiled by [`Host`].
pub(crate) type Module = <Host as KernelHost>::Module;

/// What runs kernel modules: wasmtime in the app; the browser's WebAssembly on the web, later.
pub(crate) trait KernelHost: 'static {
    /// A compiled module: cheap to clone, shared by every sandbox of it.
    type Module: Clone + Send + Sync + 'static;
    type Sandbox: Sandbox;

    /// Compile `bytes`, a module whose `m2_abi` section says this ABI (ABI-3). `checked`: with
    /// the time checks (ABI-6), for a pack that isn't music2's own (ABI-11).
    fn compile(bytes: &[u8], checked: bool) -> Result<Self::Module>;

    /// Whether `module` was compiled with the time checks.
    fn checked(module: &Self::Module) -> bool;

    /// A sandbox of `module`: one instance and its one memory, its `m2.asset_read` reading its
    /// [`Assets`]. Its start function, and every call until [`Sandbox::limit`] says otherwise,
    /// may run for `limit` when the module has the time checks. It may grow its memory up to
    /// [`crate::KERNEL_MEMORY`] until it is sealed.
    fn instantiate(module: &Self::Module, limit: Duration) -> Result<Self::Sandbox>;

    /// What a thread that calls kernels does first (wasmtime: makes its per-thread state).
    fn thread_start() {}
}

/// One instance of a module and its one memory (RENDER-32). Entered by one thread at a time.
pub(crate) trait Sandbox: Send {
    /// An export, found once, called many times.
    type Export: Clone + Send + Sync;

    /// Export `name`, a function of `call`'s shape.
    fn export(&mut self, name: &str, call: Call) -> Result<Self::Export>;

    /// Call `f` with its arguments, the first of `args` (the rest are 0): its `i32` result, or 0
    /// for none. A call that asked for memory it may not take fails with
    /// [`Fault::OutOfMemory`], and one stopped at its limit with [`Fault::TooSlow`], each as the
    /// error itself; any other trap, as the host says it. Never allocates while it succeeds.
    fn call(&mut self, f: &Self::Export, args: [u32; 4]) -> Result<u32>;

    /// Each call from now on may run for `limit`, when its module has the time checks (ABI-6).
    fn limit(&mut self, limit: Duration);

    /// What its `m2.asset_read` reads: its states add theirs as they are born.
    fn assets(&mut self) -> &mut Assets;

    /// From now on its memory and tables never grow, so it never moves. Every page is touched
    /// once, so none is first touched on the audio thread.
    fn seal(&mut self) -> Result<()>;

    /// Its memory's size in bytes, as it is now.
    fn size(&self) -> usize;

    /// Copy `bytes` into its memory at `at`. Never allocates. Past its memory's end panics.
    fn write(&mut self, at: usize, bytes: &[u8]);

    /// Copy its memory's bytes at `at` into `out`. Never allocates. Past its memory's end panics.
    fn read(&self, at: usize, out: &mut [u8]);
}

/// How a call into a kernel failed, when it is the host's to say: past its limit (ABI-6), or
/// memory it may not take (RENDER-32). Any other failure is a trap, in the host's own words. The
/// runtime tells each in words that name the node ([`crate::sandbox::Sandbox::ran_out`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fault {
    TooSlow,
    OutOfMemory,
}

impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Fault::TooSlow => "a call into a kernel ran past its time limit",
            Fault::OutOfMemory => "a kernel asked for memory it may not take",
        })
    }
}

impl std::error::Error for Fault {}
