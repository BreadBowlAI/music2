//! Authoring kit for music2 kernels written in Rust.
//!
//! A kernel is a small piece of DSP compiled to WebAssembly. The host creates one
//! instance per graph node (one per voice inside `poly`), so a kernel's state is
//! simply the instance's memory. Kernels have no imports: everything they know
//! arrives through their ports, including time.
//!
//! ABI v0. For a kernel named `k` the module exports:
//!
//! | export | meaning |
//! |---|---|
//! | `m2_abi() -> u32` | ABI version (0), once per module |
//! | `k_init(sample_rate: f32, max_block: u32, channels: u32)` | create state and port buffers |
//! | `k_ports() -> u32` | `inputs << 8 \| outputs`, so the host can check the manifest |
//! | `k_port(index: u32) -> u32` | address of a port buffer: inputs first, then outputs, in manifest order |
//! | `k_reset()` | back to the state right after `init` |
//! | `k_process(frames: u32)` | read inputs, write outputs, for `frames` samples |
//!
//! Every port buffer holds `PORT_CHANNELS × max_block` floats, channel-major: channel
//! `c` starts at `c × max_block`. Control ports use channel 0 only. `channels` is the
//! channel count the host resolved for the kernel's `audio<*>` ports (1 or 2).
//!
//! ```ignore
//! struct Gain { sr: f32 }
//! impl m2_kernel::Kernel for Gain {
//!     const INPUTS: usize = 2;   // in, db
//!     const OUTPUTS: usize = 1;  // out
//!     fn new(sample_rate: f32, _channels: usize) -> Self { Gain { sr: sample_rate } }
//!     fn process(&mut self, io: &mut m2_kernel::Io) { ... }
//! }
//! m2_kernel::export!(gain, Gain);
//! ```

pub const ABI: u32 = 0;

/// Channels allocated per port buffer.
pub const PORT_CHANNELS: usize = 2;

pub trait Kernel {
    const INPUTS: usize;
    const OUTPUTS: usize;
    /// Build fresh state. `reset` calls this again, so it must not depend on history.
    fn new(sample_rate: f32, channels: usize) -> Self;
    fn process(&mut self, io: &mut Io);
}

/// The port buffers for one `process` call.
pub struct Io<'a> {
    ins: &'a [Vec<f32>],
    outs: &'a mut [Vec<f32>],
    max_block: usize,
    /// Samples to process this call.
    pub frames: usize,
    /// Resolved channel count of the kernel's `audio<*>` ports.
    pub channels: usize,
}

impl Io<'_> {
    /// Input `i`, channel 0.
    pub fn input(&self, i: usize) -> &[f32] {
        self.input_ch(i, 0)
    }

    pub fn input_ch(&self, i: usize, ch: usize) -> &[f32] {
        let start = ch * self.max_block;
        &self.ins[i][start..start + self.frames]
    }

    /// First sample of input `i` (for values read once per block).
    pub fn first(&self, i: usize) -> f32 {
        self.ins[i][0]
    }

    /// Output `i`, channel 0.
    pub fn output(&mut self, i: usize) -> &mut [f32] {
        self.output_ch(i, 0)
    }

    pub fn output_ch(&mut self, i: usize, ch: usize) -> &mut [f32] {
        let start = ch * self.max_block;
        &mut self.outs[i][start..start + self.frames]
    }

    /// Several `(port, channel)` inputs and outputs at once. Outputs must be distinct.
    pub fn ports<const M: usize, const N: usize>(
        &mut self,
        ins: [(usize, usize); M],
        outs: [(usize, usize); N],
    ) -> ([&[f32]; M], [&mut [f32]; N]) {
        let (frames, mb) = (self.frames, self.max_block);
        for (a, x) in outs.iter().enumerate() {
            assert!(outs[..a].iter().all(|y| y != x), "output {x:?} requested twice");
            assert!(x.0 < self.outs.len() && (x.1 + 1) * mb <= self.outs[x.0].len(), "no output {x:?}");
        }
        let ins = ins.map(|(i, ch)| &self.ins[i][ch * mb..ch * mb + frames]);
        let base: *mut Vec<f32> = self.outs.as_mut_ptr();
        // SAFETY: every (port, channel) pair is in bounds and distinct (checked above), so
        // the slices never overlap, and they borrow `self` mutably for their lifetime.
        let outs = outs.map(|(o, ch)| unsafe { std::slice::from_raw_parts_mut((*base.add(o)).as_mut_ptr().add(ch * mb), frames) });
        (ins, outs)
    }
}

/// What `export!` keeps per instance. Not for direct use.
#[doc(hidden)]
pub struct Host<K: Kernel> {
    kernel: K,
    sample_rate: f32,
    channels: usize,
    max_block: usize,
    ports: Vec<Vec<f32>>,
}

#[doc(hidden)]
impl<K: Kernel> Host<K> {
    pub fn new(sample_rate: f32, max_block: u32, channels: u32) -> Self {
        let channels = channels.clamp(1, PORT_CHANNELS as u32) as usize;
        let max_block = max_block as usize;
        Host {
            kernel: K::new(sample_rate, channels),
            sample_rate,
            channels,
            max_block,
            ports: (0..K::INPUTS + K::OUTPUTS).map(|_| vec![0.0; PORT_CHANNELS * max_block]).collect(),
        }
    }

    pub fn port(&mut self, i: u32) -> u32 {
        self.ports.get_mut(i as usize).map_or(0, |p| p.as_mut_ptr() as usize as u32)
    }

    /// A port buffer, for hosts running kernels natively (benchmarks, tests).
    pub fn port_mut(&mut self, i: usize) -> &mut [f32] {
        &mut self.ports[i]
    }

    pub fn reset(&mut self) {
        self.kernel = K::new(self.sample_rate, self.channels);
        for p in &mut self.ports[K::INPUTS..] {
            p.fill(0.0);
        }
    }

    pub fn process(&mut self, frames: u32) {
        let (ins, outs) = self.ports.split_at_mut(K::INPUTS);
        let mut io = Io { ins, outs, max_block: self.max_block, frames: (frames as usize).min(self.max_block), channels: self.channels };
        self.kernel.process(&mut io);
    }
}

/// Export the ABI functions for kernel type `$ty` under the name `$name`.
#[macro_export]
macro_rules! export {
    ($name:ident, $ty:ty) => {
        pub mod $name {
            use super::*;
            static mut HOST: Option<$crate::Host<$ty>> = None;

            #[allow(clippy::mut_from_ref)]
            fn host() -> &'static mut $crate::Host<$ty> {
                // SAFETY: WebAssembly instances are single-threaded and the host calls
                // `init` before anything else.
                unsafe { (*(&raw mut HOST)).as_mut().expect(concat!(stringify!($name), "_init not called")) }
            }

            #[unsafe(export_name = concat!(stringify!($name), "_init"))]
            pub extern "C" fn init(sample_rate: f32, max_block: u32, channels: u32) {
                unsafe { *(&raw mut HOST) = Some($crate::Host::new(sample_rate, max_block, channels)) };
            }

            #[unsafe(export_name = concat!(stringify!($name), "_ports"))]
            pub extern "C" fn ports() -> u32 {
                ((<$ty as $crate::Kernel>::INPUTS as u32) << 8) | <$ty as $crate::Kernel>::OUTPUTS as u32
            }

            #[unsafe(export_name = concat!(stringify!($name), "_port"))]
            pub extern "C" fn port(i: u32) -> u32 {
                host().port(i)
            }

            #[unsafe(export_name = concat!(stringify!($name), "_reset"))]
            pub extern "C" fn reset() {
                host().reset()
            }

            #[unsafe(export_name = concat!(stringify!($name), "_process"))]
            pub extern "C" fn process(frames: u32) {
                host().process(frames)
            }
        }
    };
}

/// Export `m2_abi`. Call once per WebAssembly module.
#[macro_export]
macro_rules! abi {
    () => {
        #[unsafe(no_mangle)]
        pub extern "C" fn m2_abi() -> u32 {
            $crate::ABI
        }
    };
}

pub fn db_to_gain(db: f32) -> f32 {
    if db <= -96.0 { 0.0 } else { 10f32.powf(db / 20.0) }
}
