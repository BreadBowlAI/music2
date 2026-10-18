//! One floating-point mode on every thread that renders (RENDER-2a): `m2_os::fp`, where the plugin
//! host process uses it too (HOST-3). Read its module documentation.

pub use m2_os::fp::{Canon, canon, flush_for_tests, is_canon, mode};

/// For tests: the calling thread flushes subnormals to zero, as a plugin or a host might leave it.
#[cfg(test)]
pub(crate) fn flush_to_zero() {
    m2_os::fp::flush_to_zero()
}

/// For tests: whether the calling thread flushes subnormals to zero.
#[cfg(test)]
pub(crate) fn flushes() -> bool {
    m2_os::fp::flushes()
}
