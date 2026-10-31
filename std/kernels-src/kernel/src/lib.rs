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
//! **An offline kernel** ([`Cfg::offline`]) is run by the host off the audio thread, never in a
//! song's graph: to make a clip's render, say. Its source is its asset 0 and its time map
//! [`Cfg::time_map`]; it has no inputs, and the host calls `process` again and again, each call
//! the next `max_block` frames of its output (or fewer, at its end).
//!
//! **The port table** `io` holds one buffer address per port *channel*: the inputs first, then
//! the outputs, each port's channels in order, each port with the channel count the settings give
//! it. Each buffer holds `max_block` floats, indexed by the frame in the block. The kit maps a mono
//! port's channel 1 to its channel 0 for reads, and to a scratch buffer of its own for writes, so a
//! kernel may treat every port as stereo.
//!
//! **Notes** (ABI-8): an input of kind `events<note>` takes one entry in the port table, the address
//! of a header `{count, cap, events}` the host writes each block, with the lane's events after it,
//! 24 bytes each ([`RawEvent`]). A kernel names those inputs in [`Kernel::NOTES`] and reads them with
//! [`Io::events`]: each [`Event`] with its frame counted from the call's first frame.
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

/// [`Cfg::flags`] bit 0: the state is an offline kernel's ([`Cfg::offline`]).
pub const OFFLINE: u32 = 1;

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
    /// Bit 0 ([`OFFLINE`]): an offline kernel's state. The rest are 0 in v1.
    pub flags: u32,
    /// The kernel's numbers, as the song's code gives them (`values`).
    pub values: &'static [f64],
    /// The audio files the kernel reads, as the song's code names them (`assets`), in order.
    pub assets: &'static [Asset],
    /// An offline kernel's time map: pairs of (source seconds, output seconds), in order. Empty
    /// for a live kernel. See [`Cfg::time_map`].
    pub time_map: &'static [[f64; 2]],
}

impl Cfg {
    pub fn new(sample_rate: f32, max_block: usize, channels: usize) -> Cfg {
        Cfg { sample_rate, max_block, channels: channels.clamp(1, PORT_CHANNELS), flags: 0, values: &[], assets: &[], time_map: &[] }
    }

    /// Whether the host runs this state offline: off the audio thread, never in a song's graph, its
    /// source its asset 0 and its time map [`Cfg::time_map`].
    pub fn offline(&self) -> bool {
        self.flags & OFFLINE != 0
    }

    /// An offline kernel's time map: pairs of (source seconds, output seconds from the output's
    /// start), in order of output time. Between two pairs time runs in a straight line; before the
    /// first and after the last, the nearest line goes on. One pair means speed 1 through it; none,
    /// the source as it is.
    pub fn time_map(&self) -> &'static [[f64; 2]] {
        self.time_map
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
    /// from the file. For a live kernel it allocates nothing and never waits, so `process` may call
    /// it; an offline kernel's host may read the file from the disk then, which waits.
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
    /// An offline kernel's time map: `time_map_len` pairs of f64 (source seconds, output seconds),
    /// 8-byte aligned. 0 and 0 for a live kernel.
    pub time_map: usize,
    pub time_map_len: u32,
}

/// Bytes in a v1 settings block on wasm32.
pub const SETTINGS_BYTES: u32 = 60;

pub trait Kernel: Sized {
    const INPUTS: usize;
    const OUTPUTS: usize;
    /// Its inputs of kind `events<note>`, by place: the ones [`Io::events`] reads (ABI-8).
    const NOTES: &'static [usize] = &[];
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

/// A note event as a kernel reads it (ABI-8): the host's `Event` (architecture.md, RENDER-14), its
/// frame counted from the first frame of the call that reads it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// A note starts: its pitch in semitones (60 is C4), its velocity 0 to 1, and the pitch it
    /// slides from, if any.
    On { frame: usize, id: u32, pitch: f32, velocity: f32, from_pitch: Option<f32> },
    /// A note ends: its release velocity.
    Off { frame: usize, id: u32, velocity: f32 },
    /// A note's pitch, pressure or timbre changes (reserved: not sent yet).
    Expression { frame: usize, id: u32, kind: Expression, value: f32 },
    /// A parameter changes at a frame (reserved: not sent yet).
    Param { frame: usize, id: u32, value: f32 },
    /// A controller from MIDI input: the pitch wheel, the mod wheel, the sustain pedal (CC 64).
    Midi { frame: usize, bytes: [u8; 3] },
}

/// What a note expression changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Expression {
    Pitch,
    Pressure,
    Timbre,
}

impl Event {
    pub fn frame(&self) -> usize {
        match *self {
            Event::On { frame, .. }
            | Event::Off { frame, .. }
            | Event::Expression { frame, .. }
            | Event::Param { frame, .. }
            | Event::Midi { frame, .. } => frame,
        }
    }
}

/// One event as the host lays it out (24 bytes, `docs/kernel-abi.md`): its kind (0 on, 1 off, 2
/// note expression, 3 param, 4 MIDI), its frame in the block, its note's id, and three values: an
/// on's pitch, velocity and `from_pitch` (NaN for none); an off's release velocity; a note
/// expression's kind (0 pitch, 1 pressure, 2 timbre) and value; a param's value; a MIDI message's
/// three bytes in the low bytes of the first.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RawEvent {
    pub kind: u32,
    pub frame: u32,
    pub id: u32,
    pub values: [u32; 3],
}

impl RawEvent {
    /// `e` as the host writes it, its frame in the block.
    pub fn of(e: &Event) -> RawEvent {
        let (kind, frame, id, values) = match *e {
            Event::On { frame, id, pitch, velocity, from_pitch } => {
                (0, frame, id, [pitch.to_bits(), velocity.to_bits(), from_pitch.unwrap_or(f32::NAN).to_bits()])
            }
            Event::Off { frame, id, velocity } => (1, frame, id, [velocity.to_bits(), 0, 0]),
            Event::Expression { frame, id, kind, value } => (2, frame, id, [kind as u32, value.to_bits(), 0]),
            Event::Param { frame, id, value } => (3, frame, id, [value.to_bits(), 0, 0]),
            Event::Midi { frame, bytes: [a, b, c] } => (4, frame, 0, [u32::from_le_bytes([a, b, c, 0]), 0, 0]),
        };
        RawEvent { kind, frame: frame as u32, id, values }
    }

    /// The event, if its frame is in `from..to` and its kind is one the kit knows: its frame
    /// counted from `from`.
    pub fn event(&self, from: usize, to: usize) -> Option<Event> {
        let at = self.frame as usize;
        if !(from..to).contains(&at) {
            return None;
        }
        let (frame, id, [a, b, c]) = (at - from, self.id, self.values);
        let f = f32::from_bits;
        Some(match self.kind {
            0 => Event::On { frame, id, pitch: f(a), velocity: f(b), from_pitch: Some(f(c)).filter(|x| !x.is_nan()) },
            1 => Event::Off { frame, id, velocity: f(a) },
            2 => {
                let kind = match a {
                    0 => Expression::Pitch,
                    1 => Expression::Pressure,
                    2 => Expression::Timbre,
                    _ => return None,
                };
                Event::Expression { frame, id, kind, value: f(b) }
            }
            3 => Event::Param { frame, id, value: f(a) },
            4 => {
                let [x, y, z, _] = a.to_le_bytes();
                Event::Midi { frame, bytes: [x, y, z] }
            }
            _ => return None,
        })
    }
}

/// An events input's header, as the host writes it each block (ABI-8).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Notes {
    /// Events this block: never more than `cap`.
    pub count: u32,
    pub cap: u32,
    /// The first of them.
    pub events: *const RawEvent,
}

/// One events input's events in one call, in order: an iterator of [`Event`]s.
pub struct Events<'a> {
    raw: std::slice::Iter<'a, RawEvent>,
    from: usize,
    to: usize,
}

impl Iterator for Events<'_> {
    type Item = Event;

    fn next(&mut self) -> Option<Event> {
        let (from, to) = (self.from, self.to);
        self.raw.by_ref().find_map(|r| r.event(from, to))
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
    /// The kernel's `events<note>` inputs ([`Kernel::NOTES`]).
    notes: &'static [usize],
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

    /// Input `i`'s events in this call's frames, in order, each frame counted from the call's first
    /// (ABI-8). Input `i` must be one [`Kernel::NOTES`] names.
    pub fn events(&self, i: usize) -> Events<'_> {
        assert!(self.notes.contains(&i) && i < self.inputs, "input {i} isn't one of the kernel's `events<note>` inputs");
        let head = self.table[self.ports[i].first as usize] as *const Notes;
        // SAFETY: the host gives an events input's one table entry as the address of its header,
        // whose `events` hold `count` (at most `cap`) events, in input memory nothing writes during
        // the call (ABI-1); the slice is lent with `&self`.
        let raw = unsafe {
            let h = &*head;
            match h.events.is_null() {
                true => &[][..],
                false => std::slice::from_raw_parts(h.events, h.count.min(h.cap) as usize),
            }
        };
        Events { raw: raw.iter(), from: self.from, to: self.from + self.frames }
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
            notes: K::NOTES,
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
    /// Each events input's header and events, as [`Host::events`] gave them.
    notes: Vec<Option<(Box<Notes>, Vec<RawEvent>)>>,
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
            notes: (0..n).map(|_| None).collect(),
        }
    }

    /// Give events input `i` these events for the next calls, their frames in the block, as a host
    /// lays them out (ABI-8).
    pub fn events(&mut self, i: usize, events: &[Event]) {
        let raw: Vec<RawEvent> = events.iter().map(RawEvent::of).collect();
        let head = Box::new(Notes { count: raw.len() as u32, cap: raw.len() as u32, events: raw.as_ptr() });
        self.notes[i] = Some((head, raw));
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
        self.process_range(0, frames);
    }

    /// Frames `from..to` of a block, as a host calls a voice's kernels where its note changes.
    pub fn process_range(&mut self, from: u32, to: u32) {
        let mb = self.max_block;
        self.table.clear();
        for (p, notes) in self.ports.iter_mut().zip(&self.notes) {
            let at = p.as_mut_ptr();
            match notes {
                // An events input: its header, in its one entry (and the one after, unread).
                Some((head, _)) => self.table.extend([&**head as *const Notes as *mut f32; PORT_CHANNELS]),
                None => self.table.extend((0..PORT_CHANNELS).map(|c| at.wrapping_add(c * mb))),
            }
        }
        // SAFETY: each address is of a channel of `max_block` floats, or an events input's header,
        // and the ports are distinct.
        unsafe { self.state.process(self.table.as_ptr(), from, to) }
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
        time_map: cfg.time_map.as_ptr() as usize,
        time_map_len: cfg.time_map.len() as u32,
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
        let time_map = unsafe { list::<[f64; 2]>(s.time_map, s.time_map_len) };
        let cfg = Cfg {
            sample_rate: s.sample_rate,
            max_block: s.max_block as usize,
            channels: (s.channels as usize).clamp(1, PORT_CHANNELS),
            flags: s.flags,
            values,
            assets,
            time_map,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A kernel whose input 0 is `events<note>`: it keeps every event it reads, by call.
    struct Heard {
        calls: Vec<Vec<Event>>,
    }

    impl Kernel for Heard {
        const INPUTS: usize = 2;
        const OUTPUTS: usize = 1;
        const NOTES: &'static [usize] = &[0];

        fn new(_: &Cfg) -> Self {
            Heard { calls: vec![] }
        }

        fn process(&mut self, io: &mut Io) {
            self.calls.push(io.events(0).collect());
            io.output(0).fill(0.0);
        }
    }

    // ABI-9: a kernel reads its events through the kit: as many as the host counted, in order,
    // each with its values, and its frame counted from the call's first frame. A call reads only its
    // own frames' events.
    #[test]
    fn a_kernel_reads_its_events_through_the_kit() {
        let mut host = Host::<Heard>::new(48_000.0, 128, 1);
        let given = [
            Event::Midi { frame: 0, bytes: [0xB0, 64, 127] },
            Event::Off { frame: 5, id: 1, velocity: 0.0 },
            Event::On { frame: 5, id: 2, pitch: 61.5, velocity: 0.25, from_pitch: None },
            Event::Expression { frame: 9, id: 2, kind: Expression::Timbre, value: 0.5 },
            Event::Param { frame: 9, id: 7, value: -3.0 },
            Event::On { frame: 100, id: 3, pitch: 40.0, velocity: 1.0, from_pitch: Some(38.0) },
        ];
        host.events(0, &given);
        host.process(128);
        host.process_range(5, 64);
        host.process_range(64, 128);
        let calls = &host.state.kernel.calls;
        assert_eq!(calls[0], given, "the whole block");
        assert_eq!(
            calls[1],
            [
                Event::Off { frame: 0, id: 1, velocity: 0.0 },
                Event::On { frame: 0, id: 2, pitch: 61.5, velocity: 0.25, from_pitch: None },
                Event::Expression { frame: 4, id: 2, kind: Expression::Timbre, value: 0.5 },
                Event::Param { frame: 4, id: 7, value: -3.0 },
            ],
            "frames 5 to 64, counted from 5"
        );
        assert_eq!(calls[2], [Event::On { frame: 36, id: 3, pitch: 40.0, velocity: 1.0, from_pitch: Some(38.0) }]);
    }

    // ABI-8: the bytes are the host's: each event 24 bytes, as `docs/kernel-abi.md` lays them out; a
    // kind the kit doesn't know is skipped.
    #[test]
    fn an_event_is_24_bytes_as_the_host_writes_it() {
        assert_eq!(std::mem::size_of::<RawEvent>(), 24);
        let on = RawEvent::of(&Event::On { frame: 3, id: 9, pitch: 60.0, velocity: 0.5, from_pitch: None });
        assert_eq!((on.kind, on.frame, on.id, f32::from_bits(on.values[0]), f32::from_bits(on.values[1])), (0, 3, 9, 60.0, 0.5));
        assert!(f32::from_bits(on.values[2]).is_nan(), "no `from_pitch` is NaN");
        let midi = RawEvent::of(&Event::Midi { frame: 0, bytes: [0xE0, 0x12, 0x40] });
        assert_eq!((midi.kind, midi.values), (4, [0x0040_12E0, 0, 0]));
        let unknown = RawEvent { kind: 9, ..on };
        assert_eq!(unknown.event(0, 128), None);
    }

    // ABI-9: a kernel asks for events only of an input it says is `events<note>`.
    #[test]
    #[should_panic(expected = "input 1 isn't one of the kernel's `events<note>` inputs")]
    fn events_of_another_input_are_refused() {
        struct Wrong;
        impl Kernel for Wrong {
            const INPUTS: usize = 2;
            const OUTPUTS: usize = 1;
            const NOTES: &'static [usize] = &[0];
            fn new(_: &Cfg) -> Self {
                Wrong
            }
            fn process(&mut self, io: &mut Io) {
                let _ = io.events(1).count();
            }
        }
        Host::<Wrong>::new(48_000.0, 128, 1).process(128);
    }
}
