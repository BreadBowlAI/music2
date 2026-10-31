//! What std's native tests share: m2-dsp's kit (`m2_dsp::testing`), read against std's manifest.

pub(crate) use m2_dsp::testing::*;

/// The manifest's file: std's.
const MANIFEST: &str = include_str!("../../m2pkg.toml");

/// The defaults of std's `kernel`, by port, with `names` checked against the manifest's order (an
/// input without a default reads 0). The tests play every kernel with the manifest's own defaults,
/// so a port order or default that drifts from the code fails here.
pub(crate) fn defaults(kernel: &str, names: &[&str]) -> Vec<f32> {
    defaults_in(MANIFEST, kernel, names)
}
