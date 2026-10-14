//! The one crossfade a plan activation makes (RENDER-25a, RENDER-27): on every changed edge into a
//! kept node, on the song's output, and on an audio lane whose renders change.

/// Frame `k` (counted from the activation's first frame) of a linear crossfade over `len` frames
/// from `o` to `n`. Linear, because the two sides are mostly the same music (dry and wet, one clip
/// at two gains); an exact copy of `n` from the window's last frame on; and bit-exact wherever the
/// two sides are equal, so a silent edit changes no bit.
#[inline]
pub(crate) fn xfade(o: f32, n: f32, k: u32, len: u32) -> f32 {
    if k + 1 >= len {
        n
    } else if o.to_bits() == n.to_bits() {
        o
    } else {
        o + (n - o) * ((k + 1) as f32 / len as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::xfade;

    // RENDER-25a: equal sides stay bit-identical through the window, -0.0 and NaN included; unequal
    // ones move linearly, and the window's last frame is the new side exactly.
    #[test]
    fn equal_sides_stay_exact() {
        for x in [0.3f32, -0.0, f32::NAN, 1e-30] {
            assert!((0..240).all(|k| xfade(x, x, k, 240).to_bits() == x.to_bits()));
        }
        assert_eq!(xfade(0.0, 1.0, 0, 4), 0.25);
        assert_eq!(xfade(0.0, 1.0, 2, 4), 0.75);
        assert_eq!(xfade(0.1, 0.7, 3, 4), 0.7);
        assert_eq!(xfade(0.1, 0.7, 1000, 4), 0.7);
    }
}
