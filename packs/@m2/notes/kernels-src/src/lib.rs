//! The kernels of the pack `@m2/notes`, its own sound code (PACK-21): four note effects (PACK-29),
//! each called by the pack's id, `kernel("@m2/notes:arp", …)`. Port order must match the pack's
//! `m2pkg.toml`. Written with std's kit (`m2_kernel`), built with std's kernels by `std/build.sh`
//! into `packs/@m2/notes/kernels/notes.wasm` (ABI-10); its crate's manifest is
//! `std/kernels-src/packs/@m2/notes/Cargo.toml`. This folder is no part of the pack (PACK-20).
//!
//! A note effect takes notes and gives notes (ABI-15): the host runs it in the lane pass, on whole
//! blocks, and reads what it gives through a guard (RENDER-41, RENDER-42). [`Arp`] plays the notes
//! held one at a time ([`arpeggio`]), [`Chord`] a chord from each note ([`chords`]), [`Echo`] each
//! note again, later and quieter ([`echoes`]), and [`LockToKey`] moves each note onto the song's key
//! ([`scales`]). They share the song's key ([`key`]) and where the song is ([`time`]).
//!
//! The house rules: each gives an On only with a pitch from 0 to 127 and a velocity from 0 to 1, so
//! the guard never drops one; each ends every note it starts; the wheels and the pedal pass as they
//! come; a control past its range plays as its nearest end, one that isn't a number as its
//! default; and each keeps its state in fixed room taken at its birth, so nothing is taken while it
//! plays.

// A `new` here is the state its kind starts from (a birth's), not a default, as in m2-dsp.
#![allow(clippy::new_without_default)]

use m2_kernel::{abi, export};

pub mod arpeggio;
pub mod chords;
pub mod echoes;
pub mod key;
pub mod scales;
pub mod time;

pub use arpeggio::Arp;
pub use chords::Chord;
pub use echoes::Echo;
pub use scales::LockToKey;

abi!();

export!(arp, Arp);
export!(chord, Chord);
export!(echo, Echo);
export!(lock, LockToKey);

#[cfg(test)]
mod tests;
