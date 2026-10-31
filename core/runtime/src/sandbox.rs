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
//! The one import a kernel may have is `m2.asset_read` ([`asset_read`]): it copies an asset's
//! audio into the kernel's memory, from where its sandbox holds it ([`Held`]): decoded whole,
//! shared and outside every sandbox, which allocates nothing and never waits; or, for a sandbox off
//! the audio thread, from its file on disk.
//!
//! Every call into it runs with a deadline ([`crate::limit`], ABI-6): its birth's limit while it is
//! born, its plan's once sealed. A call past it stops, and the sandbox is stuck: a panic never
//! calls it again (ABI-7).
//!
//! The rest of the runtime sees states and their port buffers, and never a `Store`.

use std::ptr::NonNull;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use wasmtime::{
    AsContextMut, Caller, Engine, Extern, ExternType, Func, Instance, Memory, Module, ResourceLimiter, Store, TypedFunc, ValType,
};

use crate::audio::Asset;
use crate::limit::{self, Deadline, TooSlow};
use crate::stream::Wav;
use crate::{BLOCK, KERNEL_MEMORY, PORT_BYTES, WasmContext};

/// Bytes of a v1 settings block (`m2_kernel::Settings` on wasm32).
const SETTINGS_BYTES: usize = 60;

/// Bytes of one entry of a settings block's asset list: its handle, frames, channels and rate.
const ASSET_BYTES: usize = 16;

/// What a state is made from, besides its kernel: its ports, the channel count of each (inputs
/// first) and the count resolved for its `audio<*>` ports, the song's rate, and its settings.
pub(crate) struct Spec<'a> {
    pub(crate) inputs: usize,
    pub(crate) outputs: usize,
    pub(crate) channels: &'a [u32],
    pub(crate) audio: u8,
    pub(crate) sample_rate: f64,
    pub(crate) values: &'a [f64],
    /// Each asset it reads; `None` for one that is missing.
    pub(crate) assets: &'a [Option<Held>],
    /// An offline kernel's time map (`flags` bit 0): pairs of (source seconds, output seconds).
    /// `None` for a live kernel.
    pub(crate) offline: Option<&'a [[f64; 2]]>,
}

/// Where a sandbox reads an asset from (`docs/kernel-abi.md`, The asset import): the host's choice,
/// per sandbox. Either way a kernel reads the same samples, bit for bit.
#[derive(Clone)]
pub(crate) enum Held {
    /// Decoded whole, shared with the [`crate::audio::Cache`] that decoded it: a read copies and
    /// never waits, so a sandbox on the audio thread reads this kind only.
    Whole(Arc<Asset>),
    /// Its file, read at each call through [`Wav::read`] into the sandbox's own buffer, in pieces,
    /// never decoded whole. A read may wait for the disk, so only a sandbox off the audio thread (an
    /// offline kernel's) holds this kind.
    Disk(Arc<Wav>),
}

impl Held {
    fn same(&self, other: &Held) -> bool {
        match (self, other) {
            (Held::Whole(a), Held::Whole(b)) => Arc::ptr_eq(a, b),
            (Held::Disk(a), Held::Disk(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }

    /// Its frames, channels and rate, as its settings entry gives them.
    pub(crate) fn shape(&self) -> (u32, u32, f32) {
        match self {
            Held::Whole(a) => (a.len() as u32, a.channels.len() as u32, a.sample_rate as f32),
            Held::Disk(w) => (w.frames() as u32, w.channels() as u32, w.rate as f32),
        }
    }
}

/// Frames a [`Held::Disk`] read decodes at a time, so its buffer stays small (128 KiB of stereo).
const DISK_PIECE: usize = 16 * 1024;

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
    /// The assets its states read, by handle: never copied in.
    assets: Vec<Held>,
    /// What a [`Held::Disk`] read uses: the file's bytes and their samples, per channel, and which
    /// asset's frames those are: a read of the other channel of the same frames needs no new one.
    buf: Vec<u8>,
    pcm: [Vec<f32>; 2],
    piece: Option<(u32, u64, u64)>,
    /// The epoch ([`limit::epoch`]) the call that runs now is stopped at: what a read from a file
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

/// The one import kernel code may have (ABI-2).
pub(crate) const ASSET_READ: (&str, &str) = ("m2", "asset_read");

/// `m2.asset_read(handle, channel, frame, dst, frames) -> frames read`: `frames` floats at `dst`,
/// from `frame` on, of channel `channel` of the asset `handle` names (a mono asset's channel 1 is
/// its channel 0). Past the asset's end, and for a handle or channel it hasn't, it writes silence.
/// It returns how many frames came from the asset. A `dst` whose floats aren't all in the memory
/// traps. From a [`Held::Whole`] asset it allocates nothing: the asset and the memory are found by
/// index (RENDER-28). From a [`Held::Disk`] one it reads the file, and a file it can't read traps.
fn asset_read(mut caller: Caller<'_, Host>, handle: u32, channel: u32, frame: u32, dst: u32, frames: u32) -> wasmtime::Result<u32> {
    let Some(memory) = caller.data().memory else { wasmtime::bail!("m2.asset_read before its sandbox has a memory") };
    let started = limit::epoch();
    let (bytes, host) = memory.data_and_store_mut(&mut caller);
    let (at, n) = (dst as usize, frames as usize);
    let Some(out) = at.checked_add(4 * n).and_then(|end| bytes.get_mut(at..end)) else {
        wasmtime::bail!("m2.asset_read: {frames} frames at {dst} lie past the kernel's memory")
    };
    let got = match host.assets.get(handle as usize) {
        Some(Held::Whole(a)) => {
            let src: &[f32] = match a.channels.len() {
                1 => &a.channels[0],
                _ => a.channels.get(channel as usize).map_or(&[], |c| &c[..]),
            };
            let from = (frame as usize).min(src.len());
            let got = (src.len() - from).min(n);
            out[..4 * got].copy_from_slice(bytemuck::cast_slice(&src[from..from + got]));
            got
        }
        Some(Held::Disk(w)) => read_disk(w, (handle, channel, frame), out, &mut host.buf, &mut host.pcm, &mut host.piece)?,
        None => 0,
    };
    out[4 * got..].fill(0);
    // The ticks a read from a file took aren't the kernel's (ABI-6): its deadline moves on by them.
    let spent = limit::epoch() - started;
    if spent > 0 && matches!(host.assets.get(handle as usize), Some(Held::Disk(_))) {
        host.deadline += spent;
        let left = host.deadline.saturating_sub(limit::epoch());
        caller.as_context_mut().set_epoch_deadline(left);
    }
    Ok(got as u32)
}

/// [`asset_read`] from a file: the frames it has of `out`'s, from `frame` on, of `channel` of asset
/// `handle`, decoded as a whole decode decodes them, a piece at a time; how many. `piece` says which
/// frames `pcm` holds, both channels: a kernel reads the channels of the same frames in turn.
fn read_disk(
    w: &Wav,
    (handle, channel, frame): (u32, u32, u32),
    out: &mut [u8],
    buf: &mut Vec<u8>,
    pcm: &mut [Vec<f32>; 2],
    piece: &mut Option<(u32, u64, u64)>,
) -> wasmtime::Result<usize> {
    let ch = if w.channels() == 1 { 0 } else { channel as usize };
    if ch >= w.channels() {
        return Ok(0);
    }
    let from = (frame as u64).min(w.frames());
    let got = ((w.frames() - from) as usize).min(out.len() / 4);
    let mut done = 0;
    while done < got {
        let n = (got - done).min(DISK_PIECE);
        let want = (handle, from + done as u64, from + (done + n) as u64);
        if *piece != Some(want) {
            *piece = None;
            #[cfg(test)]
            std::thread::sleep(SLOW.with(std::cell::Cell::take));
            if let Err(e) = w.read(want.1, want.2, buf, pcm) {
                wasmtime::bail!("m2.asset_read: its file can't be read: {e:#}")
            }
            *piece = Some(want);
        }
        out[4 * done..4 * (done + n)].copy_from_slice(bytemuck::cast_slice(&pcm[ch]));
        done += n;
    }
    Ok(got)
}

#[cfg(test)]
thread_local! {
    /// The next read from a file on this thread waits this long first ([`slow_disk`]).
    static SLOW: std::cell::Cell<std::time::Duration> = const { std::cell::Cell::new(std::time::Duration::ZERO) };
}

/// `f`, with the first read from a file it makes on this thread waiting `d` first: a drive
/// waking up (tests).
#[cfg(test)]
pub(crate) fn slow_disk<R>(d: std::time::Duration, f: impl FnOnce() -> R) -> R {
    SLOW.with(|s| s.set(d));
    let r = f();
    SLOW.with(|s| s.set(std::time::Duration::ZERO));
    r
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
        let same = f.params().len() == 5
            && f.params().all(|p| matches!(p, ValType::I32))
            && matches!(f.results().collect::<Vec<_>>()[..], [ValType::I32]);
        if !same {
            bail!("`m2.asset_read` has the wrong signature: it is (i32, i32, i32, i32, i32) -> i32 (docs/kernel-abi.md)");
        }
        out.push(Func::wrap(&mut *store, asset_read).into());
    }
    Ok(out)
}

/// Give the next call into `store` `ticks` before it is stopped (ABI-6).
fn arm(store: &mut Store<Host>, ticks: u64) {
    store.data_mut().deadline = limit::epoch() + ticks;
    store.set_epoch_deadline(ticks);
}

/// Whether `e` says a call ran past its deadline (ABI-6).
fn interrupted(e: &anyhow::Error) -> bool {
    e.chain().any(|c| c.downcast_ref::<wasmtime::Trap>() == Some(&wasmtime::Trap::Interrupt))
}

/// A store and an instance of `module` in it, with its limits and its imports, and its memory.
/// Its start function, if it has one, may run for `limit` (ABI-6).
pub(crate) fn instantiate(engine: &Engine, module: &Module, limit: std::time::Duration) -> Result<(Store<Host>, Instance, Memory)> {
    let host =
        Host { sealed: false, refused: false, memory: None, assets: vec![], buf: vec![], pcm: [vec![], vec![]], piece: None, deadline: 0 };
    let mut store = Store::new(engine, host);
    store.limiter(|l| l);
    let ext = imports(&mut store, module)?;
    arm(&mut store, limit::ticks(limit));
    let instance = match Instance::new(&mut store, module, &ext).wctx(|| "instantiating") {
        Ok(i) => i,
        Err(e) if interrupted(&e) => {
            bail!("its module took too long to start and was stopped (ABI-6): its start ran past {}", limit::said(limit))
        }
        Err(e) => return Err(e),
    };
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
    /// Its limits, and the one each call has now, in ticks too: its birth's until it is sealed
    /// (ABI-6).
    deadline: Deadline,
    limit: std::time::Duration,
    ticks: u64,
    /// A call ran past its deadline: a panic never calls it again in this plan (ABI-7).
    stuck: bool,
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
    /// A sandbox of `module`, to be born: states may be made in it until it is sealed. Each call
    /// into it may run as `deadline` says (ABI-6).
    pub(crate) fn new(engine: &Engine, module: &Module, deadline: Deadline) -> Result<Sandbox> {
        let (mut store, instance, memory) = instantiate(engine, module, deadline.born)?;
        let alloc = instance.get_typed_func::<(u32, u32), u32>(&mut store, "m2_alloc")?;
        let base = NonNull::new(memory.data_ptr(&store)).context("the kernel memory has no address")?;
        let (limit, ticks) = (deadline.born, limit::ticks(deadline.born));
        Ok(Sandbox { store, instance, memory, alloc, base, len: 0, deadline, limit, ticks, stuck: false })
    }

    /// Give the next call its deadline (ABI-6). Never allocates.
    fn arm(&mut self) {
        arm(&mut self.store, self.ticks);
    }

    /// Whether a call into it ran past its deadline, so it is called no more in this plan (ABI-7).
    pub(crate) fn stuck(&self) -> bool {
        self.stuck
    }

    /// It may be called again: its box was reset, as the next plan moved it (RENDER-25).
    pub(crate) fn unstick(&mut self) {
        self.stuck = false;
    }

    /// A state of kernel `prefix` (the export names' prefix), as `spec` says. One block per state
    /// holds its port buffers, its settings block and its lists, and its port table
    /// (`docs/kernel-abi.md`).
    pub(crate) fn state(&mut self, prefix: &str, spec: &Spec) -> Result<State> {
        debug_assert!(self.len == 0, "a state made in a sealed sandbox");
        let (inputs, outputs, channels) = (spec.inputs, spec.outputs, spec.channels);
        let f = |name: &str| format!("{prefix}_{name}");
        let ports = self.instance.get_typed_func::<(), u32>(&mut self.store, &f("ports"))?;
        let new = self.instance.get_typed_func::<u32, u32>(&mut self.store, &f("new"))?;
        let process = self.instance.get_typed_func::<(u32, u32, u32, u32), ()>(&mut self.store, &f("process"))?;
        let reset = self.instance.get_typed_func::<u32, ()>(&mut self.store, &f("reset"))?;
        let active = self.instance.get_typed_func::<u32, u32>(&mut self.store, &f("active"))?;
        self.arm();
        let declared = ports.call(&mut self.store, ())?;
        if declared != ((inputs as u32) << 8 | outputs as u32) {
            bail!(
                "`{prefix}` has {} inputs and {} outputs, but its manifest declares {inputs} and {outputs}",
                declared >> 8,
                declared & 0xff
            );
        }
        let n = inputs + outputs;
        debug_assert_eq!(channels.len(), n);
        // Each asset's handle: its place in the sandbox's table, shared by every state that reads
        // the same audio. A missing one has none, and reads silence.
        let table = &mut self.store.data_mut().assets;
        let handles: Vec<u32> = spec
            .assets
            .iter()
            .map(|a| match a {
                Some(a) => match table.iter().position(|t| t.same(a)) {
                    Some(at) => at as u32,
                    None => {
                        table.push(a.clone());
                        table.len() as u32 - 1
                    }
                },
                None => u32::MAX,
            })
            .collect();
        // The block: the port buffers, then the settings, the channel list, the values, the
        // assets, the time map and the port table.
        let map = spec.offline.unwrap_or_default();
        let (nv, na, nm) = (spec.values.len(), spec.assets.len(), map.len());
        let table_len: usize = channels.iter().map(|&c| c as usize).sum();
        let settings_at = n * PORT_BYTES;
        let list_at = settings_at + SETTINGS_BYTES;
        let values_at = (list_at + 4 * n).next_multiple_of(8);
        let assets_at = values_at + 8 * nv;
        let map_at = (assets_at + ASSET_BYTES * na).next_multiple_of(8);
        let table_at = map_at + 16 * nm;
        let bytes = table_at + 4 * table_len;
        self.arm();
        let block = self.alloc.call(&mut self.store, (bytes as u32, 16))? as usize;
        if block == 0 || !block.is_multiple_of(16) || block + bytes > self.memory.data_size(&self.store) {
            bail!("`{prefix}`'s module gave no room for a state's ports ({bytes} bytes)");
        }
        let bufs: Vec<usize> = (0..n).map(|p| block + p * PORT_BYTES).collect();
        let mut io = Vec::with_capacity(table_len);
        for (p, &c) in channels.iter().enumerate() {
            io.extend((0..c as usize).map(|ch| (bufs[p] + ch * BLOCK * 4) as u32));
        }
        let at = |off: usize, len: usize| if len == 0 { 0 } else { (block + off) as u32 };
        let mut words: Vec<u32> = vec![
            SETTINGS_BYTES as u32,
            (spec.sample_rate as f32).to_bits(),
            BLOCK as u32,
            spec.offline.is_some() as u32,
            spec.audio.clamp(1, 2) as u32,
            at(list_at, n),
            n as u32,
            at(values_at, nv),
            nv as u32,
            at(assets_at, na),
            na as u32,
            0,
            0,
            at(map_at, nm),
            nm as u32,
        ];
        debug_assert_eq!(words.len(), SETTINGS_BYTES / 4);
        words.extend(channels);
        let data = self.memory.data_mut(&mut self.store);
        data[block + settings_at..block + values_at].fill(0);
        data[block + settings_at..block + list_at + 4 * n].copy_from_slice(bytemuck::cast_slice(&words));
        for (i, v) in spec.values.iter().enumerate() {
            data[block + values_at + 8 * i..][..8].copy_from_slice(&v.to_le_bytes());
        }
        for (i, (a, h)) in spec.assets.iter().zip(&handles).enumerate() {
            let (frames, chans, rate) = a.as_ref().map_or((0, 0, 0.0), Held::shape);
            let entry = [*h, frames, chans, rate.to_bits()];
            data[block + assets_at + ASSET_BYTES * i..][..ASSET_BYTES].copy_from_slice(bytemuck::cast_slice(&entry));
        }
        for (i, pair) in map.iter().enumerate() {
            for (j, v) in pair.iter().enumerate() {
                data[block + map_at + 16 * i + 8 * j..][..8].copy_from_slice(&v.to_le_bytes());
            }
        }
        data[block + table_at..block + bytes].copy_from_slice(bytemuck::cast_slice(&io));
        self.arm();
        let handle = new.call(&mut self.store, (block + settings_at) as u32)?;
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
        (self.limit, self.ticks) = (self.deadline.sealed, limit::ticks(self.deadline.sealed));
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
    /// take, [`TooSlow`] when it ran past its deadline (and the sandbox is stuck, ABI-7), else `e`
    /// itself. The refusal is taken: a later error, once a panic or an activation made it play
    /// again, is its own. It ran out of memory, or of time.
    pub(crate) fn ran_out(&mut self, name: &str, e: impl Into<anyhow::Error>) -> anyhow::Error {
        let e = e.into();
        if std::mem::take(&mut self.store.data_mut().refused) {
            return OutOfMemory { node: name.to_string() }.into();
        }
        if interrupted(&e) {
            self.stuck = true;
            return TooSlow { node: name.to_string(), limit: self.limit }.into();
        }
        e
    }
}

impl State {
    /// `k_process` for frames `0..frames` of the block.
    pub(crate) fn process(&self, sb: &mut Sandbox, frames: usize) -> wasmtime::Result<()> {
        sb.arm();
        self.process.call(&mut sb.store, (self.handle, self.io, 0, frames as u32))
    }

    /// `k_reset`.
    pub(crate) fn reset(&self, sb: &mut Sandbox) -> wasmtime::Result<()> {
        sb.arm();
        self.reset.call(&mut sb.store, self.handle)
    }

    /// `k_active`.
    pub(crate) fn active(&self, sb: &mut Sandbox) -> wasmtime::Result<bool> {
        sb.arm();
        Ok(self.active.call(&mut sb.store, self.handle)? != 0)
    }
}

/// A kernel asked for more memory than a sound may use: [`crate::KERNEL_MEMORY`], 256 MiB, while
/// its sandbox was born, or any at all after it was sealed (RENDER-32).
#[derive(Debug)]
pub struct OutOfMemory {
    /// The node, `<module>/<id>`.
    pub node: String,
}

impl std::fmt::Display for OutOfMemory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ran out of memory: a sound may use up to {} MB", self.node, crate::KERNEL_MEMORY >> 20)
    }
}

impl std::error::Error for OutOfMemory {}

/// A song needs more sandboxes than a renderer may hold, [`crate::SANDBOX_LIMIT`] (RENDER-32): told
/// at its build, before any is made, in words that say what to do.
#[derive(Debug)]
pub struct TooManySounds {
    pub needed: usize,
    pub limit: usize,
}

impl std::fmt::Display for TooManySounds {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "This song needs {} sound parts that each run on their own (each effect or sound outside an instrument, and each \
             instrument), and music2 can play {} at once. Remove some effects or tracks",
            thousands(self.needed),
            thousands(self.limit)
        )
    }
}

impl std::error::Error for TooManySounds {}

/// `n` with a comma between thousands: 4,000.
fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}
