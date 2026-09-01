//! Mid/Side 変換。

/// (L, R) → (M, S) 変換。M = (L+R)/2, S = (L-R)/2。
#[inline]
pub fn lr_to_ms(l: f32, r: f32) -> (f32, f32) {
    (0.5 * (l + r), 0.5 * (l - r))
}

/// (M, S) → (L, R) 変換。L = M+S, R = M-S。
#[inline]
pub fn ms_to_lr(m: f32, s: f32) -> (f32, f32) {
    (m + s, m - s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        for &(l, r) in &[(1.0_f32, -0.5), (0.3, 0.7), (-0.8, 0.2)] {
            let (m, s) = lr_to_ms(l, r);
            let (l2, r2) = ms_to_lr(m, s);
            assert!((l - l2).abs() < 1e-7);
            assert!((r - r2).abs() < 1e-7);
        }
    }
}
