//! The kernels of the pack `@m2/drums`, its own sound code (PACK-21): the drum machine's voice,
//! `drum`, called by the pack's id, `kernel("@m2/drums:drum", …)`. Port order must match the pack's
//! `m2pkg.toml`. Written with std's kit and blocks (`m2_kernel`, `m2_dsp`), built with std's kernels
//! by `std/build.sh` into `packs/@m2/drums/kernels/drums.wasm` (ABI-10). This folder is no part of
//! the pack (PACK-20): only the module built from it is.
//!
//! The drums read the marks std's `drumhit` makes for each hit (its choke group, by `m2_dsp::kit`) and
//! the count std's `hitcount` keeps of a lane's hits: those two are std's, since std's Sampler counts
//! its round robin with them too. The drum voice was std's until std 0.9, and plays the same bits.

use m2_kernel::abi;

abi!();

mod drum;
#[cfg(test)]
mod testkit;

pub use drum::Drum;
