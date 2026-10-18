//! Kernel sandboxes (RENDER-32, kernel ABI v1, `docs/kernel-abi.md`): one wasmtime `Store`
//! holding one instance of a package's module and its one memory, the only bytes that code can
//! touch.
//!
//! A sandbox is *born* off the audio thread: the host asks the module's allocator for each state's
//! block (`m2_alloc`), writes the state's settings and port table there, and builds the state
//! (`k_new`). Only then may its memory grow, up to [`KERNEL_MEMORY`]. Then it is *sealed*: its
//! memory and tables never grow again, so it never moves, and the address kept at the seal stays
//! its memory's. Every page of it is touched once then, so no first touch reaches the audio thread.
//! Then the plan resets each state once (its prewarm): a reset that would take memory fails there,
//! at the build, and names its node.
//!
//! The one import a kernel may have is `m2.asset_read` ([`asset_read`]): it copies decoded audio
//! the host holds, shared and outside every sandbox, into the kernel's memory, and allocates
//! nothing.
//!
//! The rest of the runtime sees states and their port buffers, and never a `Store`.

use std::ptr::NonNull;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use wasmtime::{Caller, Engine, Extern, ExternType, Func, Instance, Memory, Module, ResourceLimiter, Store, TypedFunc, ValType};

use crate::audio::Asset;
use crate::{BLOCK, KERNEL_MEMORY, OutOfMemory, PORT_BYTES, WasmContext};

/// Bytes of a v1 settings block (`m2_kernel::Settings` on wasm32).
const SETTINGS_BYTES: usize = 60;

/// What the host keeps in a sandbox's store: its limits, and what `m2.asset_read` reads.
///
/// While it is born its memory may grow up to its cap; once sealed nothing grows. A refused grow
/// traps at once, so a kernel never runs on without what it asked for, and `refused` says why it
/// stopped: [`OutOfMemory`], until that error is made.
pub(crate) struct Host {
    sealed: bool,
    refused: bool,
    /// Its memory, as a handle: `m2.asset_read` writes there without looking it up by name.
    memory: Option<Memory>,
    /// The assets its states read, by handle: shared with the [`crate::audio::Cache`] that decoded
    /// them, never copied in.
    assets: Vec<Arc<Asset>>,
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

/// The one import kernel code may have (ABI-2).
pub(crate) const ASSET_READ: (&str, &str) = ("m2", "asset_read");

/// `m2.asset_read(handle, channel, frame, dst, frames) -> frames read`: `frames` floats at `dst`,
/// from `frame` on, of channel `channel` of the asset `handle` names (a mono asset's channel 1 is
/// its channel 0). Past the asset's end, and for a handle or channel it hasn't, it writes silence.
/// It returns how many frames came from the asset. A `dst` whose floats aren't all in the memory
/// traps. It allocates nothing: the asset and the memory are found by index (RENDER-28).
fn asset_read(mut caller: Caller<'_, Host>, handle: u32, channel: u32, frame: u32, dst: u32, frames: u32) -> wasmtime::Result<u32> {
    let Some(memory) = caller.data().memory else { wasmtime::bail!("m2.asset_read before its sandbox has a memory") };
    let (bytes, host) = memory.data_and_store_mut(&mut caller);
    let (at, n) = (dst as usize, frames as usize);
    let Some(out) = at.checked_add(4 * n).and_then(|end| bytes.get_mut(at..end)) else {
        wasmtime::bail!("m2.asset_read: {frames} frames at {dst} lie past the kernel's memory")
    };
    let src: &[f32] = match host.assets.get(handle as usize) {
        Some(a) if a.channels.len() == 1 => &a.channels[0],
        Some(a) => a.channels.get(channel as usize).map_or(&[], |c| &c[..]),
        None => &[],
    };
    let from = (frame as usize).min(src.len());
    let got = (src.len() - from).min(n);
    out[..4 * got].copy_from_slice(bytemuck::cast_slice(&src[from..from + got]));
    out[4 * got..].fill(0);
    Ok(got as u32)
}

/// What `module` imports, made in `store`: only [`ASSET_READ`], with its signature. Anything else
/// is refused, naming it.
fn imports(store: &mut Store<Host>, module: &Module) -> Result<Vec<Extern>> {
    let mut out = vec![];
    for i in module.imports() {
        if (i.module(), i.name()) != ASSET_READ {
            bail!("a kernel may import only `m2.asset_read`, but this module imports `{}.{}`", i.module(), i.name());
        }
        let ExternType::Func(f) = i.ty() else { bail!("`m2.asset_read` must be imported as a function") };
        let same = f.params().len() == 5 && f.params().all(|p| matches!(p, ValType::I32)) && matches!(f.results().collect::<Vec<_>>()[..], [ValType::I32]);
        if !same {
            bail!("`m2.asset_read` has the wrong signature: it is (i32, i32, i32, i32, i32) -> i32 (docs/kernel-abi.md)");
        }
        out.push(Func::wrap(&mut *store, asset_read).into());
    }
    Ok(out)
}

/// A store and an instance of `module` in it, with its limits and its imports, and its memory.
pub(crate) fn instantiate(engine: &Engine, module: &Module) -> Result<(Store<Host>, Instance, Memory)> {
    let mut store = Store::new(engine, Host { sealed: false, refused: false, memory: None, assets: vec![] });
    store.limiter(|l| l);
    let ext = imports(&mut store, module)?;
    let instance = Instance::new(&mut store, module, &ext).wctx(|| "instantiating")?;
    let memory = instance.get_memory(&mut store, "memory").context("missing export `memory`")?;
    store.data_mut().memory = Some(memory);
    Ok((store, instance, memory))
}

/// One sandbox: a store, its instance of one package's module, and its memory.
pub(crate) struct Sandbox {
    store: Store<Host>,
    instance: Instance,
    memory: Memory,
    alloc: TypedFunc<(u32, u32), u32>,
    /// The memory's address and size, kept at the seal, so a call needn't ask the store (one of
    /// hundreds, and cold) where its memory is. A sealed memory never grows or moves.
    base: NonNull<u8>,
    len: usize,
}

// SAFETY: `base` is dereferenced only through `&mut Sandbox` (or `&Sandbox` for reads), and so with
// the store that owns the memory; the memory goes to another thread only with that store.
unsafe impl Send for Sandbox {}

/// One kernel state in a sandbox: its handle, its port table, its port buffers and the calls of
/// its kernel type. Its calls are made with the sandbox it was born in.
pub(crate) struct State {
    handle: u32,
    io: u32,
    process: TypedFunc<(u32, u32, u32, u32), ()>,
    reset: TypedFunc<u32, ()>,
    active: TypedFunc<u32, u32>,
    /// Each input's buffer and each output's: `PORT_BYTES`, channel 1 at `BLOCK` floats in.
    pub(crate) ins: Vec<usize>,
    pub(crate) outs: Vec<usize>,
}

impl Sandbox {
    /// A sandbox of `module`, to be born: states may be made in it until it is sealed.
    pub(crate) fn new(engine: &Engine, module: &Module) -> Result<Sandbox> {
        let (mut store, instance, memory) = instantiate(engine, module)?;
        let alloc = instance.get_typed_func::<(u32, u32), u32>(&mut store, "m2_alloc")?;
        let base = NonNull::new(memory.data_ptr(&store)).context("the kernel memory has no address")?;
        Ok(Sandbox { store, instance, memory, alloc, base, len: 0 })
    }

    /// A state of kernel `prefix` (the export names' prefix) with `inputs` and `outputs` ports, each
    /// with the channel count `channels` gives, inputs first; `audio` is the count resolved for its
    /// `audio<*>` ports. One block per state holds its port buffers, its settings and its port table.
    pub(crate) fn state(&mut self, prefix: &str, inputs: usize, outputs: usize, channels: &[u32], audio: u8, sample_rate: f64) -> Result<State> {
        debug_assert!(self.len == 0, "a state made in a sealed sandbox");
        let f = |name: &str| format!("{prefix}_{name}");
        let ports = self.instance.get_typed_func::<(), u32>(&mut self.store, &f("ports"))?;
        let new = self.instance.get_typed_func::<u32, u32>(&mut self.store, &f("new"))?;
        let process = self.instance.get_typed_func::<(u32, u32, u32, u32), ()>(&mut self.store, &f("process"))?;
        let reset = self.instance.get_typed_func::<u32, ()>(&mut self.store, &f("reset"))?;
        let active = self.instance.get_typed_func::<u32, u32>(&mut self.store, &f("active"))?;
        let declared = ports.call(&mut self.store, ())?;
        if declared != ((inputs as u32) << 8 | outputs as u32) {
            bail!("`{prefix}` has {} inputs and {} outputs, but its manifest declares {inputs} and {outputs}", declared >> 8, declared & 0xff);
        }
        let n = inputs + outputs;
        debug_assert_eq!(channels.len(), n);
        // The block: the port buffers, then the settings, the channel list and the port table.
        let table_len: usize = channels.iter().map(|&c| c as usize).sum();
        let settings_at = n * PORT_BYTES;
        let list_at = settings_at + SETTINGS_BYTES;
        let table_at = list_at + 4 * n;
        let bytes = table_at + 4 * table_len;
        let block = self.alloc.call(&mut self.store, (bytes as u32, 16))? as usize;
        if block == 0 || !block.is_multiple_of(16) || block + bytes > self.memory.data_size(&self.store) {
            bail!("`{prefix}`'s module gave no room for a state's ports ({bytes} bytes)");
        }
        let bufs: Vec<usize> = (0..n).map(|p| block + p * PORT_BYTES).collect();
        let mut table = Vec::with_capacity(table_len);
        for (p, &c) in channels.iter().enumerate() {
            table.extend((0..c as usize).map(|ch| (bufs[p] + ch * BLOCK * 4) as u32));
        }
        let (settings, list) = ((block + settings_at) as u32, (block + list_at) as u32);
        let mut words: Vec<u32> = vec![
            SETTINGS_BYTES as u32,
            (sample_rate as f32).to_bits(),
            BLOCK as u32,
            0,
            audio.clamp(1, 2) as u32,
            list,
            n as u32,
        ];
        words.resize(SETTINGS_BYTES / 4, 0);
        words.extend(channels);
        words.extend(&table);
        let data = self.memory.data_mut(&mut self.store);
        data[block + settings_at..block + bytes].copy_from_slice(bytemuck::cast_slice(&words));
        let handle = new.call(&mut self.store, settings)?;
        if handle == 0 || handle as usize >= self.memory.data_size(&self.store) {
            bail!("`{prefix}_new` made no state");
        }
        let outs = bufs[inputs..].to_vec();
        let mut ins = bufs;
        ins.truncate(inputs);
        Ok(State { handle, io: (block + table_at) as u32, process, reset, active, ins, outs })
    }

    /// Seal it: from now on its memory and tables never grow, so its memory never moves. Every page
    /// is touched once, read and written back, so the system has each one ready before it plays.
    pub(crate) fn seal(&mut self) -> Result<()> {
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

    /// Its memory's bytes, from the address kept at the seal, without asking the store. Debug
    /// builds check the memory is still there.
    pub(crate) fn data(&mut self) -> &mut [u8] {
        debug_assert!(self.len > 0, "a sandbox's memory used before its seal");
        debug_assert_eq!(self.memory.data_ptr(&self.store), self.base.as_ptr(), "a kernel memory moved (RENDER-32)");
        // SAFETY: `base..base + len` is this sandbox's memory, which its store owns and which never
        // moves or shrinks once sealed. `&mut self` holds that store for as long as the slice lives,
        // so no call into the kernel and no other view of the memory can happen meanwhile.
        unsafe { std::slice::from_raw_parts_mut(self.base.as_ptr(), self.len) }
    }

    /// Its memory's bytes, through the store (debug checks only).
    #[cfg(debug_assertions)]
    pub(crate) fn bytes(&self) -> &[u8] {
        self.memory.data(&self.store)
    }

    /// Its memory's bytes, through the store, while it is born.
    pub(crate) fn data_born(&mut self) -> &mut [u8] {
        self.memory.data_mut(&mut self.store)
    }

    /// The memory it holds, in bytes.
    pub(crate) fn size(&self) -> usize {
        self.memory.data_size(&self.store)
    }

    /// Where its memory is now, as its store says.
    #[cfg(test)]
    pub(crate) fn memory_at(&self) -> (*mut u8, *mut u8) {
        (self.memory.data_ptr(&self.store), self.base.as_ptr())
    }

    /// `e`, from a call into node `name`: [`OutOfMemory`] when the call asked for memory it may not
    /// take, else `e` itself. The refusal is taken: a later error, once a panic or an activation
    /// made it play again, is its own.
    pub(crate) fn ran_out(&mut self, name: &str, e: impl Into<anyhow::Error>) -> anyhow::Error {
        match std::mem::take(&mut self.store.data_mut().refused) {
            true => OutOfMemory { node: name.to_string() }.into(),
            false => e.into(),
        }
    }
}

impl State {
    /// `k_process` for frames `0..frames` of the block.
    pub(crate) fn process(&self, sb: &mut Sandbox, frames: usize) -> wasmtime::Result<()> {
        self.process.call(&mut sb.store, (self.handle, self.io, 0, frames as u32))
    }

    /// `k_reset`.
    pub(crate) fn reset(&self, sb: &mut Sandbox) -> wasmtime::Result<()> {
        self.reset.call(&mut sb.store, self.handle)
    }

    /// `k_active`.
    pub(crate) fn active(&self, sb: &mut Sandbox) -> wasmtime::Result<bool> {
        Ok(self.active.call(&mut sb.store, self.handle)? != 0)
    }
}
