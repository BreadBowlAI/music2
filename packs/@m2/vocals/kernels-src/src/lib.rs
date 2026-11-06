//! The kernels of the pack `@m2/vocals`, its own sound code (PACK-21): each is called by the pack's
//! id, `kernel("@m2/vocals:shift", …)`. Port order must match the pack's `m2pkg.toml`. Written with
//! std's kit and blocks (`m2_kernel`, `m2_dsp`), built with std's kernels by `std/build.sh` into
//! `packs/@m2/vocals/kernels/vocals.wasm` (ABI-10); its crate's manifest is
//! `std/kernels-src/packs/@m2/vocals/Cargo.toml`. This folder is no part of the pack (PACK-20).
//!
//! Tune (VOX-20) is the three in a row, joined in the pack's TypeScript: [`pitch`] finds the voice's
//! pitch ([`yin`]), [`snap`] works out how far to move it to the song's key ([`aim`]), [`shift`] moves it ([`psola`]) and keeps its
//! colour. Classic sound maths only, each written from its paper, nothing learned from data.
//!
//! The house rules are m2-dsp's: audio that isn't finite counts as silence and absurd levels are
//! capped before they reach any state; controls are clamped (NaN to their rest); every buffer is
//! sized at birth from the rate, so nothing is taken while they play; and each counts its own
//! frames from its birth, so the output is the same however the host splits its blocks.

// A `new` here is the state its kind starts from (a birth's), not a default, as in m2-dsp.
#![allow(clippy::new_without_default)]

use m2_kernel::{abi, export};

pub mod aim;
pub mod fft;
pub mod psola;
pub mod yin;

pub use aim::Snap;
pub use psola::{LATENCY_S, Shift};
pub use yin::{HOP, Pitch};

abi!();

export!(pitch, Pitch);
export!(snap, Snap);
export!(shift, Shift);

#[cfg(test)]
mod tests;
