//! 双二次フィルタ (Direct Form II Transposed)。

use std::f32::consts::PI;

/// 標準形 y[n] = b0·x[n] + b1·x[n-1] + b2·x[n-2] − a1·y[n-1] − a2·y[n-2]。
/// 係数は a0 で正規化済み前提。
#[derive(Debug, Clone, Copy, Default)]
pub struct Biquad {
    pub b0: f32,
    pub b1: f32,
    pub b2: f32,
    pub a1: f32,
    pub a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    /// Direct Form II Transposed 1 サンプル処理。
    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }

    /// 周波数 `f` [Hz] における振幅応答 `|H(e^jw)|`。
    ///
    /// 設計式を書き直さず、**実際に処理へ使っている係数から**求める。
    /// GUI の可視化が実装とずれないための経路（`plans/gui_overhaul_v01.md` §Step 5）。
    pub fn magnitude(&self, sr: f32, f: f32) -> f32 {
        let w = 2.0 * PI * f / sr;
        let (c1, s1) = ((-w).cos(), (-w).sin());
        let (c2, s2) = ((-2.0 * w).cos(), (-2.0 * w).sin());
        let num_re = self.b0 + self.b1 * c1 + self.b2 * c2;
        let num_im = self.b1 * s1 + self.b2 * s2;
        let den_re = 1.0 + self.a1 * c1 + self.a2 * c2;
        let den_im = self.a1 * s1 + self.a2 * s2;
        ((num_re * num_re + num_im * num_im) / (den_re * den_re + den_im * den_im)).sqrt()
    }

    /// 内部状態リセット。
    #[inline]
    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }

    /// 2次バターワース ローパス。Q = 1/sqrt(2) → alpha = sin(w0)/sqrt(2)。
    pub fn butterworth_lpf(sr: f32, fc: f32) -> Self {
        let w0 = 2.0 * PI * fc / sr;
        let (sin_w0, cos_w0) = w0.sin_cos();
        let alpha = sin_w0 / std::f32::consts::SQRT_2;
        let a0 = 1.0 + alpha;
        let inv_a0 = 1.0 / a0;
        let b0 = (1.0 - cos_w0) * 0.5 * inv_a0;
        let b1 = (1.0 - cos_w0) * inv_a0;
        let b2 = b0;
        let a1 = -2.0 * cos_w0 * inv_a0;
        let a2 = (1.0 - alpha) * inv_a0;
        Self { b0, b1, b2, a1, a2, z1: 0.0, z2: 0.0 }
    }

    /// 2次バターワース ハイパス。Q = 1/sqrt(2)。
    pub fn butterworth_hpf(sr: f32, fc: f32) -> Self {
        let w0 = 2.0 * PI * fc / sr;
        let (sin_w0, cos_w0) = w0.sin_cos();
        let alpha = sin_w0 / std::f32::consts::SQRT_2;
        let a0 = 1.0 + alpha;
        let inv_a0 = 1.0 / a0;
        let b0 = (1.0 + cos_w0) * 0.5 * inv_a0;
        let b1 = -(1.0 + cos_w0) * inv_a0;
        let b2 = b0;
        let a1 = -2.0 * cos_w0 * inv_a0;
        let a2 = (1.0 - alpha) * inv_a0;
        Self { b0, b1, b2, a1, a2, z1: 0.0, z2: 0.0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lpf_dc_gain_is_unity() {
        let mut f = Biquad::butterworth_lpf(48000.0, 1000.0);
        // DC 入力で定常応答が 1.0 に収束するか。
        let mut y = 0.0;
        for _ in 0..2000 {
            y = f.process(1.0);
        }
        assert!((y - 1.0).abs() < 1e-4, "DC gain = {y}");
    }

    #[test]
    fn hpf_dc_gain_is_zero() {
        let mut f = Biquad::butterworth_hpf(48000.0, 1000.0);
        let mut y = 0.0;
        for _ in 0..2000 {
            y = f.process(1.0);
        }
        assert!(y.abs() < 1e-4, "DC gain = {y}");
    }
}
