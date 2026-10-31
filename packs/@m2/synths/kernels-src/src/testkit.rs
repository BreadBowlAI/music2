//! The kit's test helpers (`m2_dsp::testkit`), with the pack's manifest.

pub(crate) use m2_dsp::testkit::*;

/// A kernel's defaults in the pack's `m2pkg.toml` ([`defaults_in`]).
pub(crate) fn defaults(kernel: &str, names: &[&str]) -> Vec<f32> {
    defaults_in(include_str!("../../m2pkg.toml"), kernel, names)
}
