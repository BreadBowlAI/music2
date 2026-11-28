//! The wasmtime kernel host, the app's (the default feature `wasmtime`): the only place the
//! runtime names wasmtime.
//!
//! A sandbox is one wasmtime `Store` holding one instance of a package's module and its one
//! memory (RENDER-32). While it is born its memory may grow up to [`KERNEL_MEMORY`]; once sealed
//! nothing grows, so the memory never moves, and the address kept at the seal stays its memory's:
//! a copy in or out of it needn't ask the store (one of hundreds a block, and cold) where it is.
//!
//! The time limit (ABI-6) is wasmtime's epoch interruption: a pack's module is compiled by an
//! engine that checks, at each function's start and each loop's turn, whether its epoch has
//! reached the deadline its store was given, and traps once it has. The ticker moves the epoch on
//! ([`limit::start`]); each call into a pack's sandbox is given its deadline just before it starts.

use std::ptr::NonNull;
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use wasmtime::{AsContextMut, Caller, Engine, Extern, Func, Instance, Memory, ResourceLimiter, Store, Trap, TypedFunc};

use crate::KERNEL_MEMORY;
use crate::abi::Call;
use crate::host::{Fault, KernelHost};
use crate::limit;
use crate::sandbox::Assets;

/// The wasmtime host.
pub(crate) struct Wasmtime;

/// The engine every kernel runs in.
///
/// WebAssembly floating point is bit-exact on every machine except for the bits
/// inside NaNs. Canonicalising those costs about 4.6× in the M1 benchmark, so instead
/// the renderer silences non-finite output (a NaN means a kernel is broken anyway).
/// Relaxed SIMD is off because its results may differ between CPUs.
///
/// Memories take wasmtime's default reservation, 4 GiB with guards, so the hardware catches a
/// stray access: no access is bounds-checked, and Cranelift's Spectre guard stays on, at no cost
/// (RENDER-32). A sandbox has one memory, and a box one sandbox (one a package for a voice pool),
/// so two renderers of p50 hold about 1.2% of the address space. A sealed memory never grows, so it
/// never moves, and a sandbox keeps its address.
///
/// Two engines, the same but for one switch (ABI-6): a `checked` one compiles the time checks into
/// a pack's kernels, at each function's start and each loop's turn, and has a ticker
/// ([`crate::limit::start`]) that starts with it, on the first pack loaded, and lives as long as it
/// does; std's has none, so std's kernels cost what they did before the limit.
/// [`crate::packages::checked`] says which a package's module is compiled by, and a sandbox runs on
/// its module's own engine.
///
/// One of each for the process, as wasmtime intends: a module one package set loaded can be played
/// through any other (a render of a revision plays its view through a copy of the session's set,
/// and the gates' lab compiles with one set and plays with another).
pub fn engine(checked: bool) -> Engine {
    ENGINES[checked as usize]
        .get_or_init(|| {
            let mut config = wasmtime::Config::new();
            config.cranelift_nan_canonicalization(false);
            config.wasm_relaxed_simd(false);
            config.memory_may_move(false);
            config.epoch_interruption(checked);
            let engine = Engine::new(&config).expect("wasmtime engine");
            if checked {
                let weak = engine.weak();
                limit::start(move || weak.upgrade(), Engine::increment_epoch);
            }
            engine
        })
        .clone()
}

/// The two [`engine`]s, std's first, each made when first asked for.
static ENGINES: [OnceLock<Engine>; 2] = [OnceLock::new(), OnceLock::new()];

impl KernelHost for Wasmtime {
    type Module = wasmtime::Module;
    type Sandbox = Sandbox;

    fn compile(bytes: &[u8], checked: bool) -> Result<wasmtime::Module> {
        Ok(wasmtime::Module::new(&engine(checked), bytes)?)
    }

    /// Compiled by the [`engine`] of a pack's modules (none was, while no pack has been loaded).
    fn checked(module: &wasmtime::Module) -> bool {
        ENGINES[1].get().is_some_and(|e| Engine::same(e, module.engine()))
    }

    /// A store and an instance of `module` in it, on its module's engine, with its limits, its
    /// imports (each `m2.asset_read`: the module passed ABI-3's checks) and its memory.
    fn instantiate(module: &wasmtime::Module, limit: Duration) -> Result<Sandbox> {
        let host = Host { sealed: false, refused: false, memory: None, assets: Assets::default(), deadline: 0 };
        let mut store = Store::new(module.engine(), host);
        store.limiter(|l| l);
        debug_assert!(module.imports().all(|i| (i.module(), i.name()) == ("m2", "asset_read")), "ABI-3's checks ran first");
        let ext: Vec<Extern> = module.imports().map(|_| Func::wrap(&mut store, asset_read).into()).collect();
        let (checked, ticks) = (Wasmtime::checked(module), limit::ticks(limit));
        if checked {
            arm(&mut store, ticks);
        }
        let instance = match Instance::new(&mut store, module, &ext).map_err(anyhow::Error::from).context("instantiating") {
            Ok(i) => i,
            Err(e) if interrupted(&e) => {
                bail!("its module took too long to start and was stopped: its start ran past {}", limit::said(limit))
            }
            Err(e) => return Err(e),
        };
        let memory = instance.get_memory(&mut store, "memory").context("missing export `memory`")?;
        store.data_mut().memory = Some(memory);
        let base = NonNull::new(memory.data_ptr(&store)).context("the kernel memory has no address")?;
        Ok(Sandbox { store, instance, memory, base, len: 0, checked, ticks })
    }

    /// Each render thread's wasmtime state, made at once.
    fn thread_start() {
        Engine::tls_eager_initialize();
    }
}

/// What the host keeps in a sandbox's store: its limits, and what `m2.asset_read` reads.
///
/// While it is born its memory may grow up to its cap; once sealed nothing grows. A refused grow
/// traps at once, so a kernel never runs on without what it asked for, and `refused` says why it
/// stopped: [`Fault::OutOfMemory`], until the call's error is made.
struct Host {
    sealed: bool,
    refused: bool,
    /// Its memory, as a handle: `m2.asset_read` writes there without looking it up by name.
    memory: Option<Memory>,
    /// The assets its states read, by handle: never copied in.
    assets: Assets,
    /// The tick ([`limit::now`]) the call that runs now is stopped at: what a read from a file
    /// moves on by the time it took.
    deadline: u64,
}

impl ResourceLimiter for Host {
    fn memory_growing(&mut self, _current: usize, desired: usize, _maximum: Option<usize>) -> wasmtime::Result<bool> {
        if self.sealed || desired > KERNEL_MEMORY {
            self.refused = true;
            match self.sealed {
                true => wasmtime::bail!("a kernel asked for memory after its sandbox was sealed"),
                false => wasmtime::bail!("a kernel asked for {desired} bytes of memory, past its {KERNEL_MEMORY}"),
            }
        }
        // A module's own maximum is wasmtime's to keep.
        Ok(true)
    }

    fn table_growing(&mut self, _current: usize, _desired: usize, _maximum: Option<usize>) -> wasmtime::Result<bool> {
        if self.sealed {
            wasmtime::bail!("a kernel grew a table after its sandbox was sealed");
        }
        Ok(true)
    }

    fn instances(&self) -> usize {
        1
    }

    fn tables(&self) -> usize {
        1
    }

    fn memories(&self) -> usize {
        1
    }
}

/// `m2.asset_read(handle, channel, frame, dst, frames) -> frames read`, as [`Assets::read`] says,
/// into the kernel's memory. The ticks a read from a file took aren't the kernel's (ABI-6): its
/// deadline moves on by them.
fn asset_read(mut caller: Caller<'_, Host>, handle: u32, channel: u32, frame: u32, dst: u32, frames: u32) -> wasmtime::Result<u32> {
    let Some(memory) = caller.data().memory else { wasmtime::bail!("m2.asset_read before its sandbox has a memory") };
    let started = limit::now();
    let (bytes, host) = memory.data_and_store_mut(&mut caller);
    let got = host.assets.read((handle, channel, frame, dst, frames), bytes).map_err(wasmtime::Error::from_anyhow)?;
    let spent = limit::now() - started;
    if spent > 0 && host.assets.on_disk(handle) {
        host.deadline += spent;
        let left = limit::left(host.deadline);
        caller.as_context_mut().set_epoch_deadline(left);
    }
    Ok(got)
}

/// Give the next call into `store` `ticks` before it is stopped (ABI-6).
fn arm(store: &mut Store<Host>, ticks: u64) {
    let deadline = limit::now() + ticks;
    store.data_mut().deadline = deadline;
    store.set_epoch_deadline(limit::left(deadline));
}

/// Whether `e` says a call ran past its deadline (ABI-6).
fn interrupted(e: &anyhow::Error) -> bool {
    e.chain().any(|c| c.downcast_ref::<Trap>() == Some(&Trap::Interrupt))
}

/// One sandbox: a store, its instance of one package's module, and its memory.
pub(crate) struct Sandbox {
    store: Store<Host>,
    instance: Instance,
    memory: Memory,
    /// The memory's address, and its size once sealed (0 before), so a copy needn't ask the store
    /// where its memory is. A sealed memory never grows or moves.
    base: NonNull<u8>,
    len: usize,
    /// Whether its module has the time checks, and the ticks each call has now (ABI-6).
    checked: bool,
    ticks: u64,
}

// SAFETY: `base` is dereferenced only through `&mut Sandbox` (or `&Sandbox` for reads), and so with
// the store that owns the memory; the memory goes to another thread only with that store.
unsafe impl Send for Sandbox {}

/// An export, by its shape.
#[derive(Clone)]
pub(crate) enum Export {
    Alloc(TypedFunc<(u32, u32), u32>),
    Ports(TypedFunc<(), u32>),
    New(TypedFunc<u32, u32>),
    Reset(TypedFunc<u32, ()>),
    Process(TypedFunc<(u32, u32, u32, u32), ()>),
    Active(TypedFunc<u32, u32>),
}

impl Sandbox {
    /// Its memory's bytes: through the store while it is born, from the address kept at the seal
    /// once sealed. Debug builds check a sealed memory is still there.
    #[inline]
    fn bytes(&self) -> &[u8] {
        if self.len == 0 {
            return self.memory.data(&self.store);
        }
        debug_assert_eq!(self.memory.data_ptr(&self.store), self.base.as_ptr(), "a kernel memory moved (RENDER-32)");
        // SAFETY: `base..base + len` is this sandbox's memory, which its store owns and which never
        // moves or shrinks once sealed. `&self` holds that store for as long as the slice lives, so
        // no call into the kernel can happen meanwhile.
        unsafe { std::slice::from_raw_parts(self.base.as_ptr(), self.len) }
    }

    /// [`Sandbox::bytes`], to write.
    #[inline]
    fn bytes_mut(&mut self) -> &mut [u8] {
        if self.len == 0 {
            return self.memory.data_mut(&mut self.store);
        }
        debug_assert_eq!(self.memory.data_ptr(&self.store), self.base.as_ptr(), "a kernel memory moved (RENDER-32)");
        // SAFETY: as in `bytes`; `&mut self` holds the store, so no other view of the memory can
        // happen meanwhile.
        unsafe { std::slice::from_raw_parts_mut(self.base.as_ptr(), self.len) }
    }

    /// A call's error: [`Fault::OutOfMemory`] when it asked for memory it may not take (the
    /// refusal is taken: a later error is its own), [`Fault::TooSlow`] when it ran past its
    /// deadline, else wasmtime's own.
    #[cold]
    fn fault(&mut self, e: wasmtime::Error) -> anyhow::Error {
        let e = anyhow::Error::from(e);
        if std::mem::take(&mut self.store.data_mut().refused) {
            return Fault::OutOfMemory.into();
        }
        if interrupted(&e) {
            return Fault::TooSlow.into();
        }
        e
    }

    /// Where its memory is now, as its store says, and where it kept it (tests).
    #[cfg(test)]
    pub(crate) fn memory_at(&self) -> (*mut u8, *mut u8) {
        (self.memory.data_ptr(&self.store), self.base.as_ptr())
    }

    /// Its memory's bytes, to read and write in place (tests).
    #[cfg(test)]
    pub(crate) fn data(&mut self) -> &mut [u8] {
        self.bytes_mut()
    }
}

impl crate::host::Sandbox for Sandbox {
    type Export = Export;

    fn export(&mut self, name: &str, call: Call) -> Result<Export> {
        let (i, s) = (&self.instance, &mut self.store);
        Ok(match call {
            Call::Alloc => Export::Alloc(i.get_typed_func(s, name)?),
            Call::Ports => Export::Ports(i.get_typed_func(s, name)?),
            Call::New => Export::New(i.get_typed_func(s, name)?),
            Call::Reset => Export::Reset(i.get_typed_func(s, name)?),
            Call::Process => Export::Process(i.get_typed_func(s, name)?),
            Call::Active => Export::Active(i.get_typed_func(s, name)?),
        })
    }

    #[inline]
    fn call(&mut self, f: &Export, [a, b, c, d]: [u32; 4]) -> Result<u32> {
        if self.checked {
            arm(&mut self.store, self.ticks);
        }
        let s = &mut self.store;
        let done = match f {
            Export::Alloc(f) => f.call(s, (a, b)),
            Export::Ports(f) => f.call(s, ()),
            Export::New(f) => f.call(s, a),
            Export::Reset(f) => f.call(s, a).map(|()| 0),
            Export::Process(f) => f.call(s, (a, b, c, d)).map(|()| 0),
            Export::Active(f) => f.call(s, a),
        };
        done.map_err(|e| self.fault(e))
    }

    fn limit(&mut self, limit: Duration) {
        self.ticks = limit::ticks(limit);
    }

    fn assets(&mut self) -> &mut Assets {
        &mut self.store.data_mut().assets
    }

    fn seal(&mut self) -> Result<()> {
        self.store.data_mut().sealed = true;
        let base = NonNull::new(self.memory.data_ptr(&self.store)).context("the kernel memory has no address")?;
        if base != self.base {
            bail!("its kernel memory moved while it was born (RENDER-32: a kernel memory never moves)");
        }
        self.len = self.memory.data_size(&self.store);
        for at in (0..self.len).step_by(4096) {
            // SAFETY: `at` is inside the memory, which the store owns and `&mut self` holds.
            unsafe {
                let p = base.as_ptr().add(at);
                p.write_volatile(p.read_volatile());
            }
        }
        Ok(())
    }

    fn size(&self) -> usize {
        self.memory.data_size(&self.store)
    }

    #[inline]
    fn write(&mut self, at: usize, bytes: &[u8]) {
        self.bytes_mut()[at..at + bytes.len()].copy_from_slice(bytes);
    }

    #[inline]
    fn read(&self, at: usize, out: &mut [u8]) {
        out.copy_from_slice(&self.bytes()[at..at + out.len()]);
    }
}
