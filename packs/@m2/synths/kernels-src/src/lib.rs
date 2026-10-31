//! The kernels of the pack `@m2/synths`, its own sound code (PACK-21): the polysynth's voice
//! (`synth`) and the FM synth's (`fm`), each called by the pack's id, `kernel("@m2/synths:synth", …)`.
//! Port order must match the pack's `m2pkg.toml`. Written with std's kit and blocks (`m2_kernel`,
//! `m2_dsp`), built with std's kernels by `std/build.sh` into `packs/@m2/synths/kernels/synths.wasm`
//! (ABI-10). This folder is no part of the pack (PACK-20): only the module built from it is.
//!
//! Each is one kernel per voice: a whole instrument in one place costs far less than the same sound
//! built from small kernels. Each smooths its own continuous controls, reads times and counts when a
//! note starts or on its own control clock (never the host's blocks), and ends in exact silence.
//! Both were std's until std 0.9, and play the same bits.

use m2_kernel::abi;

abi!();

mod fm;
mod synth;
#[cfg(test)]
mod testkit;

pub use fm::Fm;
pub use synth::Synth;
