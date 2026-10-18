//! Authoring kit for music2 kernels written in Rust.
//!
//! A kernel is a small piece of DSP compiled to WebAssembly. A package's module runs in
//! *sandboxes*: one wasmtime store with one instance of the module and its one memory. A sandbox
//! may hold many *states* (one per graph node, one per voice inside `poly`), of one kernel type or
//! several, so every call names its state, and no state lives in a static. Their one import is
//! `m2.asset_read`: all else they know arrives through their settings and ports, time included.
//!
//! ABI v1, `docs/kernel-abi.md`. The version is the custom section `m2_abi`: 4 bytes, 1 as a
//! little-endian u32, which [`abi!`] writes. Exports: `memory`, `m2_alloc(size, align) -> address`
//! (0: no room), and for a kernel named `k`:
//!
//! | export | meaning |
//! |---|---|
//! | `k_ports() -> u32` | `inputs << 8 \| outputs`, so the host can check the manifest |
//! | `k_new(cfg) -> state` | build one state from the settings block at `cfg`; it may take memory |
//! | `k_reset(state)` | back to the state `k_new` made; takes no memory |
//! | `k_process(state, io, from, to)` | read inputs, write outputs, for frames `from..to` of the block |
//! | `k_active(state) -> u32` | 1 while the state holds sound that will still come out (a delay line, a release) |
//!
//! **Memory is taken only at birth.** A sandbox is born off the audio thread: the host calls
//! `m2_alloc` for its own blocks and `k_new` for each state, then seals it, so its memory never
//! grows again. On wasm32 the kit's allocator moves a pointer forward and frees nothing; it works
//! only inside `k_new` and `m2_alloc`, and anywhere else it refuses, which traps. So a kernel whose
//! state holds a `Vec` writes its own [`Kernel::reset`], which clears it in place.
//!
//! **The settings block** ([`Settings`]) is the host's, written before `k_new` and kept for as long
//! as the sandbox lives. It starts with its own length, and fields are only ever added at the end.
//! Each list in it is an address and a count. A kernel reads its numbers in [`Cfg::values`] and its
//! audio files in [`Cfg::assets`]; [`Asset::read`] reads a file through `m2.asset_read` (off wasm32,
//! from the files [`native`] keeps, for tests).
//!
//! **The port table** `io` holds one buffer address per port *channel*: the inputs first, then
//! the outputs, each port's channels in order, each port with the channel count the settings give
//! it. Each buffer holds `max_block` floats, indexed by the frame in the block. The kit maps a mono
//! port's channel 1 to its channel 0 for reads, and to a scratch buffer of its own for writes, so a
//! kernel may treat every port as stereo.
//!
//! Input memory is the host's (ABI-1). `k_process` reads its inputs and never writes them, and
//! `k_reset` never changes them: the host may write an input once and leave it there for as long
//! as its value holds (a knob's cell). [`Io`] gives inputs as shared slices, so a kernel written
//! with this kit keeps the rule by construction. Debug builds of the host check it.
//!
//! ```ignore
//! struct Gain { sr: f32 }
//! impl m2_kernel::Kernel for Gain {
//!     const INPUTS: usize = 2;   // in, db
//!     const OUTPUTS: usize = 1;  // out
//!     fn new(cfg: &m2_kernel::Cfg) -> Self { Gain { sr: cfg.sample_rate } }
//!     fn process(&mut self, io: &mut m2_kernel::Io) { ... }
//! }
//! m2_kernel::export!(gain, Gain);
//! ```

use std::marker::PhantomData;

pub const ABI: u32 = 1;

/// The most channels a port has.
pub const PORT_CHANNELS: usize = 2;

/// What a state is built from: the settings block's values, as a kernel reads them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cfg {
    pub sample_rate: f32,
    /// The most frames one `process` call is given.
    pub max_block: usize,
    /// The channel count the host resolved for the kernel's `audio<*>` ports (1 or 2).
    pub channels: usize,
    /// Reserved: bit 0 will say "offline".
    pub flags: u32,
    /// The kernel's numbers, as the song's code gives them (`values`).
    pub values: &'static [f64],
    /// The audio files the kernel reads, as the song's code names them (`assets`), in order.
    pub assets: &'static [Asset],
}

impl Cfg {
    pub fn new(sample_rate: f32, max_block: usize, channels: usize) -> Cfg {
        Cfg { sample_rate, max_block, channels: channels.clamp(1, PORT_CHANNELS), flags: 0, values: &[], assets: &[] }
    }
}

/// One audio file a kernel reads: an entry of the settings block's asset list (16 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Asset {
    /// What `m2.asset_read` takes.
    pub handle: u32,
    /// Its length, in frames at its own rate: 0 for a file that is missing.
    pub frames: u32,
    /// 1 or 2; 0 for a file that is missing.
    pub channels: u32,
    /// Its own rate, in Hz; 0 for a file that is missing.
    pub sample_rate: f32,
}

impl Asset {
    /// Whether there is nothing to play: the file is missing, or empty.
    pub fn missing(&self) -> bool {
        self.channels == 0 || self.frames == 0
    }

    /// Channel `channel` from frame `frame` on, into all of `dst`, at the file's own rate: a mono
    /// file's channel 1 is its channel 0, and past its end is silence. Returns how many frames came
    /// from the file. It allocates nothing and never waits, so `process` may call it.
    pub fn read(&self, channel: usize, frame: u32, dst: &mut [f32]) -> usize {
        read(self.handle, channel as u32, frame, dst)
    }
}

#[cfg(target_arch = "wasm32")]
fn read(handle: u32, channel: u32, frame: u32, dst: &mut [f32]) -> usize {
    #[link(wasm_import_module = "m2")]
    unsafe extern "C" {
        #[link_name = "asset_read"]
        fn asset_read(handle: u32, channel: u32, frame: u32, dst: *mut f32, frames: u32) -> u32;
    }
    // SAFETY: `dst` is `dst.len()` floats of this module's memory, which is all the host writes.
    unsafe { asset_read(handle, channel, frame, dst.as_mut_ptr(), dst.len() as u32) as usize }
}

#[cfg(not(target_arch = "wasm32"))]
fn read(handle: u32, channel: u32, frame: u32, dst: &mut [f32]) -> usize {
    native::read(handle, channel, frame, dst)
}

/// Off wasm32, a stand-in for the host's audio files, for tests: files kept on the calling thread,
/// read as `m2.asset_read` reads them.
#[cfg(not(target_arch = "wasm32"))]
pub mod native {
    use std::cell::RefCell;

    use crate::Asset;

    thread_local! {
        static FILES: RefCell<Vec<Vec<Vec<f32>>>> = const { RefCell::new(Vec::new()) };
    }

    /// A file of `channels` (1 or 2, each as long) at `sample_rate`, kept on this thread: its entry,
    /// as a host writes it.
    pub fn asset(sample_rate: f32, channels: Vec<Vec<f32>>) -> Asset {
        assert!(matches!(channels.len(), 1 | 2) && channels.iter().all(|c| c.len() == channels[0].len()), "1 or 2 channels, as long");
        let frames = channels[0].len() as u32;
        let n = channels.len() as u32;
        let handle = FILES.with_borrow_mut(|f| {
            f.push(channels);
            f.len() as u32 - 1
        });
        Asset { handle, frames, channels: n, sample_rate }
    }

    /// The entry of a file that is missing.
    pub fn missing() -> Asset {
        Asset { handle: u32::MAX, frames: 0, channels: 0, sample_rate: 0.0 }
    }

    pub(crate) fn read(handle: u32, channel: u32, frame: u32, dst: &mut [f32]) -> usize {
        FILES.with_borrow(|f| {
            let src: &[f32] = match f.get(handle as usize) {
                Some(c) if c.len() == 1 => &c[0],
                Some(c) => c.get(channel as usize).map_or(&[], |c| &c[..]),
                None => &[],
            };
            let from = (frame as usize).min(src.len());
            let got = (src.len() - from).min(dst.len());
            dst[..got].copy_from_slice(&src[from..from + got]);
            dst[got..].fill(0.0);
            got
        })
    }
}

/// The settings block the host writes before `k_new` (ABI v1). Every field is 4 bytes on wasm32,
/// so its byte offsets are fixed: `len` at 0, `sample_rate` at 4, and so on, `time_map_len` at 56.
/// Addresses are `usize`, 4 bytes on wasm32, so a native host can run the same code.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Settings {
    /// Bytes in the block, so later fields can be added at the end.
    pub len: u32,
    pub sample_rate: f32,
    pub max_block: u32,
    pub flags: u32,
    /// The channel count resolved for `audio<*>` ports.
    pub channels: u32,
    /// The channel count of each port, inputs first: `n_ports` u32s at `ports`.
    pub ports: usize,
    pub n_ports: u32,
    /// The kernel's numbers (`values`): `n_values` f64s at `values`.
    pub values: usize,
    pub n_values: u32,
    /// The kernel's audio files: `n_assets` entries of 16 bytes (handle, frames, channels, rate).
    pub assets: usize,
    pub n_assets: u32,
    /// Reserved: a saved state for `k_load`.
    pub saved: usize,
    pub saved_len: u32,
    /// Reserved: the offline time map.
    pub time_map: usize,
    pub time_map_len: u32,
}

/// Bytes in a v1 settings block on wasm32.
pub const SETTINGS_BYTES: u32 = 60;

pub trait Kernel: Sized {
    const INPUTS: usize;
    const OUTPUTS: usize;
    /// Build a state. It may take memory: it runs only while its sandbox is born.
    fn new(cfg: &Cfg) -> Self;
    /// Back to the state `new` made, from the settings kept. It must take no memory, so a kernel
    /// whose state holds a `Vec` clears it in place here instead of building it again.
    fn reset(&mut self, cfg: &Cfg) {
        *self = Self::new(cfg);
    }
    fn process(&mut self, io: &mut Io);
    /// Whether sound this kernel has already taken in can still come out later: echoes
    /// in a delay line, a reverb tail, an envelope's release. Brief ringing (a filter)
    /// doesn't count, and neither does sound a kernel makes from nothing (an
    /// oscillator): a voice is gated by its envelopes, not by its sources.
    /// The host ends a `poly` voice only when its note is over, its output is quiet
    /// and no kernel in it is active.
    fn active(&self) -> bool {
        false
    }
}

/// Where one port's channels are: its first channel's place in the port table, its channel count,
/// and for a mono output, which scratch buffer its channel 1 writes.
#[derive(Clone, Copy)]
struct Port {
    first: u16,
    channels: u8,
    scratch: u8,
}

/// The port buffers for one `process` call.
pub struct Io<'a> {
    table: &'a [*mut f32],
    ports: &'a [Port],
    inputs: usize,
    scratch: *mut f32,
    max_block: usize,
    from: usize,
    /// Samples to process this call.
    pub frames: usize,
    /// Resolved channel count of the kernel's `audio<*>` ports.
    pub channels: usize,
    _bufs: PhantomData<&'a mut [f32]>,
}

impl Io<'_> {
    /// Where channel `ch` of input `i` starts this call: channel 1 of a mono port is its channel 0.
    fn input_at(&self, i: usize, ch: usize) -> *const f32 {
        assert!(i < self.inputs && ch < PORT_CHANNELS, "no input ({i}, {ch})");
        let p = self.ports[i];
        let c = if ch < p.channels as usize { ch } else { 0 };
        self.table[p.first as usize + c].wrapping_add(self.from)
    }

    /// Where channel `ch` of output `o` starts this call: channel 1 of a mono port is a scratch
    /// buffer of its own.
    fn output_at(&self, o: usize, ch: usize) -> *mut f32 {
        assert!(self.inputs + o < self.ports.len() && ch < PORT_CHANNELS, "no output ({o}, {ch})");
        let p = self.ports[self.inputs + o];
        match ch < p.channels as usize {
            true => self.table[p.first as usize + ch].wrapping_add(self.from),
            false => self.scratch.wrapping_add(p.scratch as usize * self.max_block + self.from),
        }
    }

    /// Input `i`, channel 0.
    pub fn input(&self, i: usize) -> &[f32] {
        self.input_ch(i, 0)
    }

    pub fn input_ch(&self, i: usize, ch: usize) -> &[f32] {
        // SAFETY: the host gives each channel `max_block` floats, `from + frames <= max_block`, and
        // no output overlaps an input; outputs are lent out only through `&mut self`.
        unsafe { std::slice::from_raw_parts(self.input_at(i, ch), self.frames) }
    }

    /// First sample of input `i` this call (for values read once per block).
    pub fn first(&self, i: usize) -> f32 {
        self.input(i)[0]
    }

    /// Output `i`, channel 0.
    pub fn output(&mut self, i: usize) -> &mut [f32] {
        self.output_ch(i, 0)
    }

    pub fn output_ch(&mut self, i: usize, ch: usize) -> &mut [f32] {
        // SAFETY: as `input_ch`; `&mut self` lends one output at a time.
        unsafe { std::slice::from_raw_parts_mut(self.output_at(i, ch), self.frames) }
    }

    /// Several `(port, channel)` inputs and outputs at once. Outputs must be distinct.
    pub fn ports<const M: usize, const N: usize>(
        &mut self,
        ins: [(usize, usize); M],
        outs: [(usize, usize); N],
    ) -> ([&[f32]; M], [&mut [f32]; N]) {
        for (a, x) in outs.iter().enumerate() {
            assert!(outs[..a].iter().all(|y| y != x), "output {x:?} requested twice");
        }
        let frames = self.frames;
        // SAFETY: every buffer holds `from + frames` floats or more; distinct (port, channel) outputs
        // are distinct buffers (a mono port's channel 1 is its own scratch), no output overlaps an
        // input, and all borrow `self` mutably for their lifetime.
        let ins = ins.map(|(i, ch)| unsafe { std::slice::from_raw_parts(self.input_at(i, ch), frames) });
        let outs = outs.map(|(o, ch)| unsafe { std::slice::from_raw_parts_mut(self.output_at(o, ch), frames) });
        (ins, outs)
    }
}

/// One state as the kit keeps it: the kernel's own, the settings it was built from, and where its
/// ports are in a port table. What `k_new` returns the address of. Not for direct use.
#[doc(hidden)]
pub struct State<K: Kernel> {
    kernel: K,
    cfg: Cfg,
    /// Each port, inputs first.
    ports: Box<[Port]>,
    /// `max_block` floats for each mono output: its channel 1.
    scratch: Box<[f32]>,
}

#[doc(hidden)]
impl<K: Kernel> State<K> {
    /// A state for ports with `channels` channels each, inputs first.
    pub fn new(cfg: Cfg, channels: &[u32]) -> Self {
        assert_eq!(channels.len(), K::INPUTS + K::OUTPUTS, "a channel count for each port");
        let (mut first, mut mono) = (0u16, 0u8);
        let ports: Box<[Port]> = channels
            .iter()
            .enumerate()
            .map(|(i, &n)| {
                let n = n.clamp(1, PORT_CHANNELS as u32) as u8;
                let p = Port { first, channels: n, scratch: mono };
                first += n as u16;
                mono += (i >= K::INPUTS && n == 1) as u8;
                p
            })
            .collect();
        let scratch = vec![0.0; mono as usize * cfg.max_block].into_boxed_slice();
        State { kernel: K::new(&cfg), cfg, ports, scratch }
    }

    /// Entries in the port table this state reads.
    pub fn table_len(&self) -> usize {
        self.ports.last().map_or(0, |p| p.first as usize + p.channels as usize)
    }

    pub fn reset(&mut self) {
        self.kernel.reset(&self.cfg);
    }

    pub fn active(&self) -> bool {
        self.kernel.active()
    }

    /// Frames `from..to` of a block, with the port table at `table`.
    ///
    /// # Safety
    /// `table` holds [`State::table_len`] addresses, each of a buffer of `max_block` floats, and no
    /// output's buffer overlaps another buffer.
    pub unsafe fn process(&mut self, table: *const *mut f32, from: u32, to: u32) {
        let mb = self.cfg.max_block;
        let (from, to) = ((from as usize).min(mb), (to as usize).min(mb));
        // SAFETY: as the caller promises.
        let table = unsafe { std::slice::from_raw_parts(table, self.table_len()) };
        let mut io = Io {
            table,
            ports: &self.ports,
            inputs: K::INPUTS,
            scratch: self.scratch.as_mut_ptr(),
            max_block: mb,
            from,
            frames: to.saturating_sub(from),
            channels: self.cfg.channels,
            _bufs: PhantomData,
        };
        self.kernel.process(&mut io);
    }
}

/// A native host for one state, for tests and benchmarks: its own port buffers, each
/// `PORT_CHANNELS × max_block` floats, channel-major, and every port stereo.
pub struct Host<K: Kernel> {
    state: State<K>,
    ports: Vec<Vec<f32>>,
    table: Vec<*mut f32>,
    max_block: usize,
}

impl<K: Kernel> Host<K> {
    pub fn new(sample_rate: f32, max_block: u32, channels: u32) -> Self {
        Self::of(Cfg::new(sample_rate, max_block as usize, channels as usize))
    }

    /// A host for a state built from `cfg`, its values and assets included.
    pub fn of(cfg: Cfg) -> Self {
        let n = K::INPUTS + K::OUTPUTS;
        Host {
            state: State::new(cfg, &vec![PORT_CHANNELS as u32; n]),
            ports: (0..n).map(|_| vec![0.0; PORT_CHANNELS * cfg.max_block]).collect(),
            table: Vec::with_capacity(n * PORT_CHANNELS),
            max_block: cfg.max_block,
        }
    }

    /// A port buffer: inputs first, then outputs; channel `c` starts at `c × max_block`.
    pub fn port_mut(&mut self, i: usize) -> &mut [f32] {
        &mut self.ports[i]
    }

    pub fn reset(&mut self) {
        self.state.reset();
        // Outputs only: input memory is the host's (ABI-1).
        for p in &mut self.ports[K::INPUTS..] {
            p.fill(0.0);
        }
    }

    pub fn active(&self) -> bool {
        self.state.active()
    }

    pub fn process(&mut self, frames: u32) {
        let mb = self.max_block;
        self.table.clear();
        for p in &mut self.ports {
            let at = p.as_mut_ptr();
            self.table.extend((0..PORT_CHANNELS).map(|c| at.wrapping_add(c * mb)));
        }
        // SAFETY: each address is of a channel of `max_block` floats, and the ports are distinct.
        unsafe { self.state.process(self.table.as_ptr(), 0, frames) }
    }
}

/// The settings block a native caller of the exports passes, pointing at `channels`.
pub fn settings(cfg: &Cfg, channels: &[u32]) -> Settings {
    Settings {
        len: SETTINGS_BYTES,
        sample_rate: cfg.sample_rate,
        max_block: cfg.max_block as u32,
        flags: cfg.flags,
        channels: cfg.channels as u32,
        ports: channels.as_ptr() as usize,
        n_ports: channels.len() as u32,
        values: cfg.values.as_ptr() as usize,
        n_values: cfg.values.len() as u32,
        assets: cfg.assets.as_ptr() as usize,
        n_assets: cfg.assets.len() as u32,
        saved: 0,
        saved_len: 0,
        time_map: 0,
        time_map_len: 0,
    }
}

/// A state built from the settings block at `cfg`, inside its birth. Not for direct use.
///
/// # Safety
/// `cfg` points at a [`Settings`] whose `ports` list is readable.
#[doc(hidden)]
pub unsafe fn born<K: Kernel>(cfg: *const Settings) -> *mut State<K> {
    birth(|| {
        // SAFETY: as the caller promises; the host writes the block before it calls `k_new`.
        let s = unsafe { &*cfg };
        let channels = unsafe { std::slice::from_raw_parts(s.ports as *const u32, s.n_ports as usize) };
        // The block lives as long as the sandbox, so its lists outlive every state in it.
        let values = unsafe { list::<f64>(s.values, s.n_values) };
        let assets = unsafe { list::<Asset>(s.assets, s.n_assets) };
        let cfg = Cfg {
            sample_rate: s.sample_rate,
            max_block: s.max_block as usize,
            channels: (s.channels as usize).clamp(1, PORT_CHANNELS),
            flags: s.flags,
            values,
            assets,
        };
        Box::into_raw(Box::new(State::<K>::new(cfg, channels)))
    })
}

/// The list of `n` `T`s at `at` in a settings block: empty when there are none, or when the address
/// isn't aligned for `T` (never from the host).
///
/// # Safety
/// `n` `T`s at `at` are readable for as long as the sandbox lives.
unsafe fn list<T>(at: usize, n: u32) -> &'static [T] {
    if n == 0 || at == 0 || !at.is_multiple_of(std::mem::align_of::<T>()) {
        return &[];
    }
    // SAFETY: as the caller promises; the address isn't null and is aligned.
    unsafe { std::slice::from_raw_parts(at as *const T, n as usize) }
}

/// Export the ABI functions for kernel type `$ty` under the name `$name`.
#[macro_export]
macro_rules! export {
    ($name:ident, $ty:ty) => {
        pub mod $name {
            use super::*;
            type S = $crate::State<$ty>;

            #[unsafe(export_name = concat!(stringify!($name), "_ports"))]
            pub extern "C" fn ports() -> u32 {
                ((<$ty as $crate::Kernel>::INPUTS as u32) << 8) | <$ty as $crate::Kernel>::OUTPUTS as u32
            }

            /// # Safety
            /// `cfg` is a settings block the host wrote.
            #[unsafe(export_name = concat!(stringify!($name), "_new"))]
            pub unsafe extern "C" fn new(cfg: *const $crate::Settings) -> *mut S {
                unsafe { $crate::born::<$ty>(cfg) }
            }

            /// # Safety
            /// `state` is one `new` returned.
            #[unsafe(export_name = concat!(stringify!($name), "_reset"))]
            pub unsafe extern "C" fn reset(state: *mut S) {
                unsafe { (*state).reset() }
            }

            /// # Safety
            /// `state` is one `new` returned, and `io` a port table for it.
            #[unsafe(export_name = concat!(stringify!($name), "_process"))]
            pub unsafe extern "C" fn process(state: *mut S, io: *const *mut f32, from: u32, to: u32) {
                unsafe { (*state).process(io, from, to) }
            }

            /// # Safety
            /// `state` is one `new` returned.
            #[unsafe(export_name = concat!(stringify!($name), "_active"))]
            pub unsafe extern "C" fn active(state: *const S) -> u32 {
                unsafe { (*state).active() as u32 }
            }
        }
    };
}

/// Write the module's `m2_abi` section (the ABI version, 4 bytes, little-endian) and export
/// `m2_alloc`. Call once per WebAssembly module: a second call would make the section 8 bytes.
#[macro_export]
macro_rules! abi {
    () => {
        #[cfg(target_arch = "wasm32")]
        #[used]
        #[unsafe(link_section = "m2_abi")]
        static M2_ABI: [u8; 4] = $crate::ABI.to_le_bytes();

        /// Room for the host's own blocks while the sandbox is born: 0 when there is none.
        #[unsafe(no_mangle)]
        pub extern "C" fn m2_alloc(size: usize, align: usize) -> *mut u8 {
            $crate::alloc(size, align)
        }
    };
}

/// `size` bytes aligned to `align`, inside a birth: null when there is no room.
#[doc(hidden)]
pub fn alloc(size: usize, align: usize) -> *mut u8 {
    match std::alloc::Layout::from_size_align(size.max(1), align.max(1)) {
        // SAFETY: the layout's size isn't 0.
        Ok(layout) => birth(|| unsafe { std::alloc::alloc(layout) }),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Runs `f` with the allocator open, as `k_new` and `m2_alloc` do.
#[doc(hidden)]
pub fn birth<R>(f: impl FnOnce() -> R) -> R {
    #[cfg(target_arch = "wasm32")]
    heap::open(true);
    let r = f();
    #[cfg(target_arch = "wasm32")]
    heap::open(false);
    r
}

/// The allocator on wasm32: a pointer moved forward, and only while a state is born. It frees
/// nothing, so all one `k_new` takes lies in one run of bytes. Anywhere else it refuses, which
/// traps (Rust aborts when it can't allocate).
#[cfg(target_arch = "wasm32")]
mod heap {
    use std::alloc::{GlobalAlloc, Layout};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed};

    // A sandbox runs one call at a time, so relaxed atomics are plain loads and stores here.
    static OPEN: AtomicBool = AtomicBool::new(false);
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    static END: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" {
        static __heap_base: u8;
    }

    pub(super) fn open(open: bool) {
        OPEN.store(open, Relaxed);
    }

    struct Birth;

    unsafe impl GlobalAlloc for Birth {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            if !OPEN.load(Relaxed) {
                return std::ptr::null_mut();
            }
            if END.load(Relaxed) == 0 {
                NEXT.store(&raw const __heap_base as usize, Relaxed);
                END.store(core::arch::wasm32::memory_size(0) << 16, Relaxed);
            }
            let start = (NEXT.load(Relaxed) + layout.align() - 1) & !(layout.align() - 1);
            let Some(end) = start.checked_add(layout.size()) else { return std::ptr::null_mut() };
            let have = END.load(Relaxed);
            if end > have {
                let pages = (end - have).div_ceil(1 << 16);
                if core::arch::wasm32::memory_grow(0, pages) == usize::MAX {
                    return std::ptr::null_mut();
                }
                END.store(have + (pages << 16), Relaxed);
            }
            NEXT.store(end, Relaxed);
            start as *mut u8
        }

        unsafe fn dealloc(&self, _: *mut u8, _: Layout) {}
    }

    #[global_allocator]
    static HEAP: Birth = Birth;
}

pub fn db_to_gain(db: f32) -> f32 {
    if db <= -96.0 { 0.0 } else { 10f32.powf(db / 20.0) }
}
