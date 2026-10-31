//! What the kernels music2 ships are made of, shared by std's crate and the crate of each of its own
//! packs (ABI-10, PACK-21), so a kernel that moves from std into a pack takes nothing with it but its
//! own code, and plays the same bits:
//!
//! | Module | Holds |
//! |---|---|
//! | [`fx`] | the effects' helpers: a NaN-safe clamp, safe audio, a flush to zero, control smoothing, a TPT filter, a delay line, an LFO phase |
//! | [`voice`] | the instrument voices' blocks: a fast sine, band-limited steps, a decimator, filters, envelopes, noise |
//! | [`kit`] | the drum kit's map: which drum a note plays, and its choke group |
//! | `testing` | the native tests' harness, with the feature `testing` |
//!
//! It exports no kernel and writes no `m2_abi` section: each module's crate does (`m2_kernel::abi!`).

// A block's `next` is its next sample, not an iterator's, and its `new` is the state a voice starts
// from, not a default: as they were inside std's crate.
#![allow(clippy::should_implement_trait, clippy::new_without_default)]

pub mod fx;
pub mod kit;
pub mod voice;

#[cfg(any(test, feature = "testing"))]
pub mod testing;
