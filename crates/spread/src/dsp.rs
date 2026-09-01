//! 完全線形ステレオワイドナー。フィルタを持たず、M/S ゲインのみで幅を決める。
//!
//! 幅カーブは有理関数の閉形式で与える。Center Mix は Wide 側の振る舞いを
//! 連続量で切り替える拡張:
//!
//! ```text
//! t <= 0.5 領域（Center Mix に依らず共通）:
//!     alpha = 4*(1-t) / (3-2*t)
//!     beta  = 4*t     / (3-2*t)
//!
//! t > 0.5 領域: Normal と Center の線形補間（c = center_amount, 0.0..=1.0）
//!     alpha_normal = 4*(1-t) / (3-2*t)
//!     beta_normal  = 4*t     / (3-2*t)
//!     alpha_center = 1.0
//!     beta_center  = 2*t
//!     alpha = (1-c)*alpha_normal + c*alpha_center
//!     beta  = (1-c)*beta_normal  + c*beta_center
//!
//! 出力:
//!     a = (alpha + beta) / 2
//!     c_mtx = (alpha - beta) / 2
//!     L_out = a*L + c_mtx*R
//!     R_out = c_mtx*L + a*R
//! ```
//!
//! `center_amount = 0.0` で旧 Normal (`center: false`) と、`1.0` で旧
//! Center (`center: true`) と bit-exact 一致する。
//!
//! # `t = 0`（Mono 端）の Mid ゲイン `alpha = 4/3` について
//!
//! `t = 0` では `alpha = 4/3`（+2.50 dB）、`beta = 0` となり、出力は Mid のみ。
//! 「モノ化で音量が上がる」ように見えるため、等ラウド補正への差し替えを検討したが、
//! **原理的に不可能**であることが分かったので `4/3` を維持している。
//!
//! 相関係数 `rho` の白色ステレオ（各ch RMS = r）では Mid の RMS が
//! `r * sqrt((1+rho)/2)` なので、`t = 0` の入出力 RMS 比は
//!
//! ```text
//!     ratio(rho) = alpha * sqrt((1 + rho) / 2)
//! ```
//!
//! 等ラウド（`ratio = 1`）にする係数は `alpha(rho) = sqrt(2 / (1 + rho))` であり
//! **`rho` の関数**。したがって定数 `alpha` は相関軸上の 1 点でしか等ラウドにならない。
//! 逆算すると `sqrt(2)` は `rho = 0`、`4/3` は `rho = +0.125` で等ラウドになる。
//!
//! どちらが実用的かは実測で決まる（白色ガウス、各ch 2^20 サンプル、L/R 合算 RMS）:
//!
//! ```text
//!     rho     alpha=4/3    alpha=sqrt(2)
//!    -0.75    -6.534 dB     -6.022 dB
//!    -0.50    -3.514        -3.003
//!    -0.25    -1.763        -1.251
//!     0.00    -0.506        +0.006
//!    +0.25    +0.455        +0.967
//!    +0.50    +1.248        +1.760
//!    +0.75    +1.919        +2.430
//!    +1.00    +2.499        +3.010
//!
//!    rho >= 0 の RMS 偏差:  4/3 = 1.546 dB,  sqrt(2) = 1.949 dB
//! ```
//!
//! `sqrt(2)` が優るのは `rho <= 0` の領域のみ。**以下 2 点は実測ではなく想定**:
//! (a) モノ端の用途はミックスのモノ互換確認であり、素材は強相関側
//! （`rho` おおむね 0.5..0.9）に集中する。(b) 上の RMS 偏差という指標は、
//! `rho >= 0` で偏差がほぼ正側に出るため構造的に小さい係数を有利にする
//! （`sqrt(2)` は `4/3` より一律 +0.51 dB 高い）。
//! よって「等ラウドで `4/3` が優る」は決定打ではなく、**`sqrt(2)` へ変える積極的な理由が
//! 出てこない**という消極的な結論に留める。
//!
//! **主論拠は次の 1 点**: `4/3` は独立に選んだ定数ではなく、
//! `alpha + beta = K(t) = 4/(3-2t)` の端点 `K(0)` である（`K(0.5) = 2` が恒等条件に対応）。
//! 曲線が同定済みである以上、端点だけ差し替えると曲線全体の根拠が失われる。
//!
//! Inverse=true のとき L と R を入れ替えてから上記処理。

/// 1 ステレオサンプル分の処理用係数（行列 [[a, c], [c, a]]）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coefs {
    pub a: f32,
    pub c: f32,
}

impl Coefs {
    /// `t` (Expander, 0.0..=1.0) と `center_amount` (Center Mix, 0.0..=1.0) から (a, c) を導出。
    #[inline]
    pub fn from_params(t: f32, center_amount: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let (alpha, beta) = if t <= 0.5 {
            let denom = 3.0 - 2.0 * t;
            (4.0 * (1.0 - t) / denom, 4.0 * t / denom)
        } else {
            let c = center_amount.clamp(0.0, 1.0);
            let denom = 3.0 - 2.0 * t;
            let alpha_normal = 4.0 * (1.0 - t) / denom;
            let beta_normal = 4.0 * t / denom;
            let alpha_center = 1.0;
            let beta_center = 2.0 * t;
            (
                (1.0 - c) * alpha_normal + c * alpha_center,
                (1.0 - c) * beta_normal + c * beta_center,
            )
        };
        Self {
            a: 0.5 * (alpha + beta),
            c: 0.5 * (alpha - beta),
        }
    }
}

/// ワイドナーの状態。内部状態を持たない（フィルタなし）。
#[derive(Debug, Clone, Copy)]
pub struct Spread {
    /// Expander スライダー（0.0 = Mono, 0.5 = Normal, 1.0 = Wide）。
    pub expander: f32,
    /// Inverse トグル（ステレオ L/R 入れ替え）。
    pub inverse: bool,
    /// Center トグル（Wide 側の Mid クリップ）。
    pub center: bool,
}

impl Default for Spread {
    fn default() -> Self {
        Self { expander: 0.5, inverse: false, center: false }
    }
}

impl Spread {
    /// 1 サンプルを処理して `(out_L, out_R)` を返す。
    #[inline]
    pub fn process_sample(&self, l: f32, r: f32) -> (f32, f32) {
        let (l, r) = if self.inverse { (r, l) } else { (l, r) };
        let k = Coefs::from_params(self.expander, if self.center { 1.0 } else { 0.0 });
        (k.a * l + k.c * r, k.c * l + k.a * r)
    }

    /// ステレオバッファをインプレースで処理。長さの揃った 2 スライス想定。
    pub fn process_buffer(&self, left: &mut [f32], right: &mut [f32]) {
        assert_eq!(left.len(), right.len(), "L/R buffer length mismatch");
        let k = Coefs::from_params(self.expander, if self.center { 1.0 } else { 0.0 });
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let (lin, rin) = if self.inverse { (*r, *l) } else { (*l, *r) };
            *l = k.a * lin + k.c * rin;
            *r = k.c * lin + k.a * rin;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn normal_at_05_is_identity() {
        let p = Spread { expander: 0.5, inverse: false, center: false };
        let (l, r) = p.process_sample(0.7, -0.3);
        assert!(approx(l, 0.7, 1e-7));
        assert!(approx(r, -0.3, 1e-7));
    }

    #[test]
    fn mono_at_0_collapses_to_mid() {
        let p = Spread { expander: 0.0, inverse: false, center: false };
        // a = c = 2/3 -> (1, 0) -> (2/3, 2/3)
        let (l, r) = p.process_sample(1.0, 0.0);
        assert!(approx(l, 2.0 / 3.0, 1e-7));
        assert!(approx(r, 2.0 / 3.0, 1e-7));
    }

    #[test]
    fn wide_at_1_eliminates_mid() {
        let p = Spread { expander: 1.0, inverse: false, center: false };
        let (l, r) = p.process_sample(1.0, 1.0); // 純Mid -> 出力ゼロ
        assert!(approx(l, 0.0, 1e-7));
        assert!(approx(r, 0.0, 1e-7));
    }

    #[test]
    fn inverse_swaps_lr() {
        let p = Spread { expander: 0.5, inverse: true, center: false };
        let (l, r) = p.process_sample(0.7, -0.3);
        assert!(approx(l, -0.3, 1e-7));
        assert!(approx(r, 0.7, 1e-7));
    }

    #[test]
    fn center_clips_mid_above_05() {
        let p = Spread { expander: 1.0, inverse: false, center: true };
        // alpha = 1.0, beta = 2.0 -> a = 1.5, c = -0.5
        let (l, r) = p.process_sample(1.0, 0.0);
        assert!(approx(l, 1.5, 1e-7));
        assert!(approx(r, -0.5, 1e-7));
    }

    #[test]
    fn center_below_05_matches_normal() {
        for &t in &[0.0_f32, 0.1, 0.25, 0.4, 0.5] {
            let n = Coefs::from_params(t, 0.0);
            let c = Coefs::from_params(t, 1.0);
            assert!(approx(n.a, c.a, 1e-7), "a mismatch at t={t}");
            assert!(approx(n.c, c.c, 1e-7), "c mismatch at t={t}");
        }
    }

    #[test]
    fn center_amount_0_matches_normal() {
        for &t in &[0.0_f32, 0.25, 0.5, 0.6, 0.75, 0.9, 1.0] {
            let n = Coefs::from_params(t, 0.0);
            // 旧 Normal 式（center=false 相当）を直接検算
            let denom = 3.0 - 2.0 * t;
            let expected = Coefs {
                a: 0.5 * (4.0 * (1.0 - t) / denom + 4.0 * t / denom),
                c: 0.5 * (4.0 * (1.0 - t) / denom - 4.0 * t / denom),
            };
            assert!(approx(n.a, expected.a, 1e-7), "a mismatch at t={t}");
            assert!(approx(n.c, expected.c, 1e-7), "c mismatch at t={t}");
        }
    }

    #[test]
    fn center_amount_1_matches_old_center() {
        for &t in &[0.0_f32, 0.25, 0.5, 0.6, 0.75, 0.9, 1.0] {
            let n = Coefs::from_params(t, 1.0);
            let (alpha, beta) = if t <= 0.5 {
                let denom = 3.0 - 2.0 * t;
                (4.0 * (1.0 - t) / denom, 4.0 * t / denom)
            } else {
                (1.0, 2.0 * t)
            };
            let expected = Coefs { a: 0.5 * (alpha + beta), c: 0.5 * (alpha - beta) };
            assert!(approx(n.a, expected.a, 1e-7), "a mismatch at t={t}");
            assert!(approx(n.c, expected.c, 1e-7), "c mismatch at t={t}");
        }
    }

    #[test]
    fn center_amount_below_half_t_is_c_invariant() {
        for &t in &[0.0_f32, 0.1, 0.25, 0.4, 0.5] {
            let base = Coefs::from_params(t, 0.0);
            for &c in &[0.25_f32, 0.5, 0.75, 1.0] {
                let k = Coefs::from_params(t, c);
                assert!(approx(base.a, k.a, 1e-7), "a mismatch at t={t}, c={c}");
                assert!(approx(base.c, k.c, 1e-7), "c mismatch at t={t}, c={c}");
            }
        }
    }

    #[test]
    fn center_amount_midpoint_continuity() {
        for &c in &[0.0_f32, 0.25, 0.5, 0.75, 1.0] {
            let below = Coefs::from_params(0.5 - 1e-6, c);
            let above = Coefs::from_params(0.5 + 1e-6, c);
            assert!(approx(below.a, above.a, 1e-4), "a discontinuity at c={c}");
            assert!(approx(below.c, above.c, 1e-4), "c discontinuity at c={c}");
        }
    }
}
