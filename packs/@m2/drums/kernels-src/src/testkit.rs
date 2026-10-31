//! What the pack's native tests share: m2-dsp's harness (`m2_dsp::testing`), read against the
//! pack's manifest.

pub(crate) use m2_dsp::testing::*;

/// The manifest's file: the pack's.
const MANIFEST: &str = include_str!("../../m2pkg.toml");

/// The defaults of the pack's `kernel`, by port, with `names` checked against the manifest's order
/// (an input without a default reads 0).
pub(crate) fn defaults(kernel: &str, names: &[&str]) -> Vec<f32> {
    defaults_in(MANIFEST, kernel, names)
}
