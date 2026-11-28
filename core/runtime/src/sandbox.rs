//! Kernel sandboxes (RENDER-32, kernel ABI v1, `docs/kernel-abi.md`): one instance of a
//! package's module and its one memory, the only bytes that code can touch, made by the kernel
//! host ([`crate::host`]).
//!
//! A sandbox is *born* off the audio thread: the host asks the module's allocator for each state's
//! block (`m2_alloc`), writes the state's settings and port table there, and builds the state
//! (`k_new`). Only then may its memory grow, up to [`crate::KERNEL_MEMORY`]. Then it is *sealed*:
//! its memory and tables never grow again, so it never moves. Every page of it is touched once
//! then, so no first touch reaches the audio thread. Then the plan resets each state once (its
//! prewarm): a reset that would take memory fails there, at the build, and names its node.
//!
//! The one import a kernel may have is `m2.asset_read` ([`Assets::read`]): it copies an asset's
//! audio into the kernel's memory, from where its sandbox holds it ([`Held`]): decoded whole,
//! shared and outside every sandbox, which allocates nothing and never waits; or, for a sandbox off
//! the audio thread, from its file on disk.
//!
//! Every call into a pack's sandbox runs with a deadline ([`crate::limit`], ABI-6): its birth's
//! limit while it is born and prewarmed, its plan's once it plays. A call past it stops, and the
//! sandbox is stuck: a panic never calls it again (ABI-7). std's, music2's own, run with no checks
//! ([`crate::packages::checked`]).
//!
//! The rest of the runtime sees states and their port buffers, and copies in and out of them, and
//! never the host's own types.

use std::sync::Arc;

use anyhow::{Result, bail};

use crate::abi::Call;
use crate::audio::Asset;
use crate::host::{self, Fault, Host, KernelHost, Module, Sandbox as _};
use crate::limit::{Deadline, TooSlow};
use crate::stream::Wav;
use crate::{BLOCK, EVENT_BYTES, NOTES_HEAD, PORT_BYTES};

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
    /// Its `events<note>` inputs, by place among its inputs, and the events each holds (ABI-8):
    /// each one entry in the port table, a header and that many events.
    pub(crate) notes: &'a [(usize, usize)],
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

/// What a sandbox's `m2.asset_read` reads (ABI-2): the assets its states read, by handle, never
/// copied in; and what a [`Held::Disk`] read uses: the file's bytes and their samples, per channel,
/// and which asset's frames those are: a read of the other channel of the same frames needs no new
/// one. The same for every kernel host: a host gives it the kernel's memory.
#[derive(Default)]
pub(crate) struct Assets {
    held: Vec<Held>,
    buf: Vec<u8>,
    pcm: [Vec<f32>; 2],
    piece: Option<(u32, u64, u64)>,
}

impl Assets {
    /// The handle of asset `a`: its place in the table, shared by every state that reads the same
    /// audio.
    fn handle(&mut self, a: &Held) -> u32 {
        match self.held.iter().position(|t| t.same(a)) {
            Some(at) => at as u32,
            None => {
                self.held.push(a.clone());
                self.held.len() as u32 - 1
            }
        }
    }

    /// Whether asset `handle` is read from its file: the time a read takes isn't the kernel's
    /// (ABI-6).
    pub(crate) fn on_disk(&self, handle: u32) -> bool {
        matches!(self.held.get(handle as usize), Some(Held::Disk(_)))
    }

    /// `m2.asset_read(handle, channel, frame, dst, frames) -> frames read`, into `memory`, the
    /// kernel's: `frames` floats at `dst`, from `frame` on, of channel `channel` of the asset
    /// `handle` names (a mono asset's channel 1 is its channel 0). Past the asset's end, and for a
    /// handle or channel it hasn't, it writes silence. It returns how many frames came from the
    /// asset. A `dst` whose floats aren't all in the memory fails (the kernel traps). From a
    /// [`Held::Whole`] asset it allocates nothing: the asset is found by index (RENDER-28). From a
    /// [`Held::Disk`] one it reads the file, and a file it can't read fails.
    pub(crate) fn read(&mut self, (handle, channel, frame, dst, frames): (u32, u32, u32, u32, u32), memory: &mut [u8]) -> Result<u32> {
        let (at, n) = (dst as usize, frames as usize);
        let Some(out) = at.checked_add(4 * n).and_then(|end| memory.get_mut(at..end)) else {
            bail!("m2.asset_read: {frames} frames at {dst} lie past the kernel's memory")
        };
        let got = match self.held.get(handle as usize) {
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
            Some(Held::Disk(w)) => read_disk(w, (handle, channel, frame), out, &mut self.buf, &mut self.pcm, &mut self.piece)?,
            None => 0,
        };
        out[4 * got..].fill(0);
        Ok(got as u32)
    }
}

/// The one import kernel code may have (ABI-2).
pub(crate) const ASSET_READ: (&str, &str) = ("m2", "asset_read");

/// [`Assets::read`] from a file: the frames it has of `out`'s, from `frame` on, of `channel` of
/// asset `handle`, decoded as a whole decode decodes them, a piece at a time; how many. `piece`
/// says which frames `pcm` holds, both channels: a kernel reads the channels of the same frames in
/// turn.
fn read_disk(
    w: &Wav,
    (handle, channel, frame): (u32, u32, u32),
    out: &mut [u8],
    buf: &mut Vec<u8>,
    pcm: &mut [Vec<f32>; 2],
    piece: &mut Option<(u32, u64, u64)>,
) -> Result<usize> {
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
                bail!("m2.asset_read: its file can't be read: {e:#}")
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

/// One sandbox: the kernel host's ([`host::Sandbox`]), its limits, and whether it is stuck.
pub(crate) struct Sandbox {
    sb: <Host as KernelHost>::Sandbox,
    alloc: Export,
    /// Its limits, and the one each call has now: its birth's until it plays (ABI-6). None for a
    /// module with no time checks: std's.
    deadline: Option<Deadline>,
    limit: std::time::Duration,
    /// It was sealed: no state is made in it from then on.
    sealed: bool,
    /// A call ran past its deadline: a panic never calls it again in this plan (ABI-7).
    stuck: bool,
}

/// An export of a sandbox of this build's host.
type Export = <<Host as KernelHost>::Sandbox as host::Sandbox>::Export;

/// One kernel state in a sandbox: its handle, its port table, its port buffers and the calls of
/// its kernel type. Its calls are made with the sandbox it was born in.
pub(crate) struct State {
    handle: u32,
    io: u32,
    process: Export,
    reset: Export,
    active: Export,
    /// Each input's buffer and each output's: `PORT_BYTES`, channel 1 at `BLOCK` floats in; an
    /// events input's header and its events (ABI-8).
    pub(crate) ins: Vec<usize>,
    pub(crate) outs: Vec<usize>,
    /// Each input's bytes: what debug builds compare (ABI-1). A release build doesn't compare them.
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub(crate) in_len: Vec<usize>,
}

impl Sandbox {
    /// A sandbox of `module`, to be born: states may be made in it until it is sealed. Each call
    /// into it may run as `deadline` says when the module has the time checks (ABI-6).
    pub(crate) fn new(module: &Module, deadline: Deadline) -> Result<Sandbox> {
        let mut sb = Host::instantiate(module, deadline.born)?;
        let alloc = sb.export("m2_alloc", Call::Alloc)?;
        let limit = deadline.born;
        let deadline = Host::checked(module).then_some(deadline);
        Ok(Sandbox { sb, alloc, deadline, limit, sealed: false, stuck: false })
    }

    /// From now on each call has its plan's limit, not its birth's: it plays, once born, sealed and
    /// prewarmed (ABI-6).
    pub(crate) fn play(&mut self) {
        if let Some(d) = self.deadline {
            self.limit = d.sealed;
            self.sb.limit(d.sealed);
        }
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
        debug_assert!(!self.sealed, "a state made in a sealed sandbox");
        let (inputs, outputs, channels) = (spec.inputs, spec.outputs, spec.channels);
        let f = |name: &str| format!("{prefix}_{name}");
        let sb = &mut self.sb;
        let ports = sb.export(&f("ports"), Call::Ports)?;
        let new = sb.export(&f("new"), Call::New)?;
        let process = sb.export(&f("process"), Call::Process)?;
        let reset = sb.export(&f("reset"), Call::Reset)?;
        let active = sb.export(&f("active"), Call::Active)?;
        let declared = sb.call(&ports, [0; 4])?;
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
        let table = sb.assets();
        let handles: Vec<u32> = spec.assets.iter().map(|a| a.as_ref().map_or(u32::MAX, |a| table.handle(a))).collect();
        // The block: the port buffers, then the settings, the channel list, the values, the
        // assets, the time map and the port table.
        let map = spec.offline.unwrap_or_default();
        let (nv, na, nm) = (spec.values.len(), spec.assets.len(), map.len());
        let table_len: usize = channels.iter().map(|&c| c as usize).sum();
        let room = |p: usize| spec.notes.iter().find(|(i, _)| *i == p).map(|(_, cap)| *cap);
        let lens: Vec<usize> =
            (0..n).map(|p| room(p).map_or(PORT_BYTES, |cap| (NOTES_HEAD + cap * EVENT_BYTES).next_multiple_of(16))).collect();
        let settings_at: usize = lens.iter().sum();
        let list_at = settings_at + SETTINGS_BYTES;
        let values_at = (list_at + 4 * n).next_multiple_of(8);
        let assets_at = values_at + 8 * nv;
        let map_at = (assets_at + ASSET_BYTES * na).next_multiple_of(8);
        let table_at = map_at + 16 * nm;
        let bytes = table_at + 4 * table_len;
        let block = sb.call(&self.alloc, [bytes as u32, 16, 0, 0])? as usize;
        if block == 0 || !block.is_multiple_of(16) || block + bytes > sb.size() {
            bail!("`{prefix}`'s module gave no room for a state's ports ({bytes} bytes)");
        }
        let bufs: Vec<usize> = lens.iter().scan(block, |at, len| Some(std::mem::replace(at, *at + len))).collect();
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
        sb.write(block + settings_at, &vec![0; values_at - settings_at]);
        sb.write(block + settings_at, bytemuck::cast_slice(&words));
        for (i, v) in spec.values.iter().enumerate() {
            sb.write(block + values_at + 8 * i, &v.to_le_bytes());
        }
        for (i, (a, h)) in spec.assets.iter().zip(&handles).enumerate() {
            let (frames, chans, rate) = a.as_ref().map_or((0, 0, 0.0), Held::shape);
            let entry = [*h, frames, chans, rate.to_bits()];
            sb.write(block + assets_at + ASSET_BYTES * i, bytemuck::cast_slice(&entry));
        }
        for (i, pair) in map.iter().enumerate() {
            for (j, v) in pair.iter().enumerate() {
                sb.write(block + map_at + 16 * i + 8 * j, &v.to_le_bytes());
            }
        }
        sb.write(block + table_at, bytemuck::cast_slice(&io));
        // An events input's header (ABI-8): none yet, its room, and where its events are.
        for &(p, cap) in spec.notes {
            let head = [0, cap as u32, (bufs[p] + NOTES_HEAD) as u32, 0];
            sb.write(bufs[p], bytemuck::cast_slice(&head));
        }
        let handle = sb.call(&new, [(block + settings_at) as u32, 0, 0, 0])?;
        if handle == 0 || handle as usize >= sb.size() {
            bail!("`{prefix}_new` made no state");
        }
        let outs = bufs[inputs..].to_vec();
        let mut ins = bufs;
        ins.truncate(inputs);
        let in_len = lens[..inputs].to_vec();
        Ok(State { handle, io: (block + table_at) as u32, process, reset, active, ins, outs, in_len })
    }

    /// Seal it: from now on its memory and tables never grow, so its memory never moves. Every page
    /// is touched once, so the system has each one ready before it plays.
    pub(crate) fn seal(&mut self) -> Result<()> {
        self.sealed = true;
        self.sb.seal()
    }

    /// Copy `bytes` into its memory at `at`. Never allocates.
    #[inline]
    pub(crate) fn write(&mut self, at: usize, bytes: &[u8]) {
        self.sb.write(at, bytes);
    }

    /// Copy its memory's bytes at `at` into `out`. Never allocates.
    #[inline]
    pub(crate) fn read(&self, at: usize, out: &mut [u8]) {
        self.sb.read(at, out);
    }

    /// Copy `floats` into its memory at `at`. Never allocates.
    #[inline]
    pub(crate) fn write_floats(&mut self, at: usize, floats: &[f32]) {
        self.sb.write(at, bytemuck::cast_slice(floats));
    }

    /// Copy its memory's floats at `at` into `out`. Never allocates.
    #[inline]
    pub(crate) fn read_floats(&self, at: usize, out: &mut [f32]) {
        self.sb.read(at, bytemuck::cast_slice_mut(out));
    }

    /// The memory it holds, in bytes.
    pub(crate) fn size(&self) -> usize {
        self.sb.size()
    }

    /// Where its memory is now, as its store says, and where it kept it (tests).
    #[cfg(test)]
    pub(crate) fn memory_at(&self) -> (*mut u8, *mut u8) {
        self.sb.memory_at()
    }

    /// Its memory's bytes, to read and write in place (tests).
    #[cfg(test)]
    pub(crate) fn data(&mut self) -> &mut [u8] {
        self.sb.data()
    }

    /// `e`, from a call into node `name`: [`OutOfMemory`] when the call asked for memory it may not
    /// take, [`TooSlow`] when it ran past its deadline (and the sandbox is stuck, ABI-7), as the
    /// host said ([`Fault`]), else `e` itself.
    pub(crate) fn ran_out(&mut self, name: &str, e: impl Into<anyhow::Error>) -> anyhow::Error {
        let e = e.into();
        match e.downcast_ref::<Fault>() {
            Some(Fault::OutOfMemory) => OutOfMemory { node: name.to_string() }.into(),
            Some(Fault::TooSlow) => {
                self.stuck = true;
                TooSlow { node: name.to_string(), limit: self.limit }.into()
            }
            None => e,
        }
    }
}

impl State {
    /// `k_process` for frames `0..frames` of the block.
    #[inline]
    pub(crate) fn process(&self, sb: &mut Sandbox, frames: usize) -> Result<()> {
        sb.sb.call(&self.process, [self.handle, self.io, 0, frames as u32]).map(drop)
    }

    /// `k_reset`.
    pub(crate) fn reset(&self, sb: &mut Sandbox) -> Result<()> {
        sb.sb.call(&self.reset, [self.handle, 0, 0, 0]).map(drop)
    }

    /// `k_active`.
    pub(crate) fn active(&self, sb: &mut Sandbox) -> Result<bool> {
        Ok(sb.sb.call(&self.active, [self.handle, 0, 0, 0])? != 0)
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
