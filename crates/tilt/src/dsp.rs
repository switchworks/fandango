//! tilt DSP コア — Baxandall 型シェルフ biquad 係数計算 + Direct Form I 処理。

/// 正規化済み biquad 係数 (a0 = 1)。
#[derive(Clone, Copy, Default)]
pub struct BiquadCoeffs {
    pub b0: f32,
    pub b1: f32,
    pub b2: f32,
    pub a1: f32,
    pub a2: f32,
}

// 係数計算は f64 内部で行い最後に f32 キャスト。
// 理由: DC 応答 = (b0+b1+b2)/(1+a1+a2) 等が微小和の商になり、pivot 下端
// (100 Hz, ω0 ≈ 0.013) では f32 丸めのキャンセレーションで相対誤差 1e-3 級
// に劣化するため。

pub fn low_shelf_coeffs(fs: f32, f0: f32, db_gain: f32, s: f32) -> BiquadCoeffs {
    let a = 10f64.powf(db_gain as f64 / 40.0);
    let w0 = 2.0 * std::f64::consts::PI * f0 as f64 / fs as f64;
    let cos_w = w0.cos();
    let sin_w = w0.sin();
    let alpha = sin_w * 0.5 * ((a + 1.0 / a) * (1.0 / s as f64 - 1.0) + 2.0).sqrt();
    let beta = 2.0 * a.sqrt() * alpha;

    let b0 = a * ((a + 1.0) - (a - 1.0) * cos_w + beta);
    let b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w);
    let b2 = a * ((a + 1.0) - (a - 1.0) * cos_w - beta);
    let a0 = (a + 1.0) + (a - 1.0) * cos_w + beta;
    let a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cos_w);
    let a2 = (a + 1.0) + (a - 1.0) * cos_w - beta;

    let inv_a0 = 1.0 / a0;
    BiquadCoeffs {
        b0: (b0 * inv_a0) as f32,
        b1: (b1 * inv_a0) as f32,
        b2: (b2 * inv_a0) as f32,
        a1: (a1 * inv_a0) as f32,
        a2: (a2 * inv_a0) as f32,
    }
}

pub fn high_shelf_coeffs(fs: f32, f0: f32, db_gain: f32, s: f32) -> BiquadCoeffs {
    let a = 10f64.powf(db_gain as f64 / 40.0);
    let w0 = 2.0 * std::f64::consts::PI * f0 as f64 / fs as f64;
    let cos_w = w0.cos();
    let sin_w = w0.sin();
    let alpha = sin_w * 0.5 * ((a + 1.0 / a) * (1.0 / s as f64 - 1.0) + 2.0).sqrt();
    let beta = 2.0 * a.sqrt() * alpha;

    let b0 = a * ((a + 1.0) + (a - 1.0) * cos_w + beta);
    let b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w);
    let b2 = a * ((a + 1.0) + (a - 1.0) * cos_w - beta);
    let a0 = (a + 1.0) - (a - 1.0) * cos_w + beta;
    let a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cos_w);
    let a2 = (a + 1.0) - (a - 1.0) * cos_w - beta;

    let inv_a0 = 1.0 / a0;
    BiquadCoeffs {
        b0: (b0 * inv_a0) as f32,
        b1: (b1 * inv_a0) as f32,
        b2: (b2 * inv_a0) as f32,
        a1: (a1 * inv_a0) as f32,
        a2: (a2 * inv_a0) as f32,
    }
}

/// 複素周波数応答の振幅 |H(e^{jω})| (測定用、f64 内部で精度確保)。
pub fn biquad_amp(c: &BiquadCoeffs, f: f32, fs: f32) -> f32 {
    let w = 2.0 * std::f64::consts::PI * f as f64 / fs as f64;
    let (cw1, sw1) = ((-w).cos(), (-w).sin());
    let (cw2, sw2) = ((-2.0 * w).cos(), (-2.0 * w).sin());
    let (b0, b1, b2) = (c.b0 as f64, c.b1 as f64, c.b2 as f64);
    let (a1, a2) = (c.a1 as f64, c.a2 as f64);
    let num_re = b0 + b1 * cw1 + b2 * cw2;
    let num_im = b1 * sw1 + b2 * sw2;
    let den_re = 1.0 + a1 * cw1 + a2 * cw2;
    let den_im = a1 * sw1 + a2 * sw2;
    let num_mag = (num_re * num_re + num_im * num_im).sqrt();
    let den_mag = (den_re * den_re + den_im * den_im).sqrt();
    (num_mag / den_mag) as f32
}

/// ホスト側「暴走プラグイン」誤判定 → kill を防ぐ最重要対策。
const DENORMAL_GATE: f32 = 1e-30;

/// Denormal ゲート適用。|x| < DENORMAL_GATE → 0.0。
#[inline]
fn denormal_gate(x: f32) -> f32 {
    if x.abs() < DENORMAL_GATE { 0.0 } else { x }
}

/// 低/高シェルフ縦列 1 チャンネル分の状態 (Direct Form I)。
pub struct TiltChannel {
    ls_x1: f32,
    ls_x2: f32,
    ls_y1: f32,
    ls_y2: f32,
    hs_x1: f32,
    hs_x2: f32,
    hs_y1: f32,
    hs_y2: f32,
}

impl TiltChannel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    #[inline]
    pub fn process(&mut self, x: f32, ls: &BiquadCoeffs, hs: &BiquadCoeffs) -> f32 {
        // 非有限のみ弾く。振幅の clamp は置かない。
        //
        // 旧実装は `x.clamp(-4.0, 4.0)` / 出力 `y.clamp(-6.0, 6.0)` を持っていたが、
        // どちらも `attic/excite` 由来のコピーで Baxandall の式とは無関係だった
        // (`nice-bulk` は同じ出力 clamp を既にハードクリップと認定して置換済み)。
        // 撤去の根拠は 3 つ:
        //
        // 1. **仕様内で発火していた。** gain=+12 / pivot=700 (既定) で
        //    フルスケール 1 kHz 矩形波の peak は 7.64。出力上限 6.0 を 2.10 dB 超過する。
        //    slope 下端 0.3 でも 6.47 で超過。安全網ではなく隠れたハードクリップ
        // 2. **有意な値が存在しない。** LTI フィルタに入力と独立した有限の出力上界は
        //    原理的に無い。有限 `C` は `C < ‖h‖₁·X_max` なら仕様内で歪み、
        //    `C ≥ ‖h‖₁·X_max` ならホストの `X_max` が未定義で一意に決まらない
        //    (実測: `‖h‖₁ ≤ 9.11` はパラメタ空間の最大値であって上限の閉形式ではない)
        // 3. **入力 clamp が `gain = 0` の厳密恒等を壊していた。** A=1 では
        //    `b0 = 1, b1 = a1, b2 = a2` で恒等なのに、`|x| > 4` だと音が変わっていた
        //
        // ピーク管理は `finalist` (true-peak リミッタ) の管轄であり、tilt は代行しない。
        let x = if x.is_finite() { x } else { 0.0 };

        let y_ls = ls.b0 * x + ls.b1 * self.ls_x1 + ls.b2 * self.ls_x2
            - ls.a1 * self.ls_y1
            - ls.a2 * self.ls_y2;
        self.ls_x2 = self.ls_x1;
        self.ls_x1 = x;
        self.ls_y2 = self.ls_y1;
        self.ls_y1 = y_ls;

        let y_hs = hs.b0 * y_ls + hs.b1 * self.hs_x1 + hs.b2 * self.hs_x2
            - hs.a1 * self.hs_y1
            - hs.a2 * self.hs_y2;
        self.hs_x2 = self.hs_x1;
        self.hs_x1 = y_ls;
        self.hs_y2 = self.hs_y1;
        self.hs_y1 = y_hs;

        let y_hs = denormal_gate(y_hs);
        // 非有限を検出したら**状態も掃除する**。出力を 0 にするだけでは
        // DF1 の遅延段に残った Inf/NaN が次サンプル以降を汚染し続け、
        // このガードがそれを黙って隠して**永久に無音**になる
        // (実測: `1e38` を 1 サンプル注入すると reset 無しでは以降ずっと 0.0、
        // reset 有りなら次サンプルから正常復帰。`recovers_after_nonfinite_sample`)。
        if y_hs.is_finite() {
            y_hs
        } else {
            self.reset();
            0.0
        }
    }
}

impl Default for TiltChannel {
    fn default() -> Self {
        Self {
            ls_x1: 0.0,
            ls_x2: 0.0,
            ls_y1: 0.0,
            ls_y2: 0.0,
            hs_x1: 0.0,
            hs_x2: 0.0,
            hs_y1: 0.0,
            hs_y2: 0.0,
        }
    }
}

/// Goertzel アルゴリズムによる単一周波数振幅測定 (テスト用)。
/// テスト信号は整数周期 (fs=48000, 1 秒バッファなら f は整数 Hz) 前提。
#[cfg(test)]
fn goertzel_amp(x: &[f32], f: f64, fs: f64) -> f64 {
    let w = 2.0 * std::f64::consts::PI * f / fs;
    let coeff = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0f64, 0.0f64);
    for &v in x {
        let s0 = v as f64 + coeff * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    let power = s1 * s1 + s2 * s2 - coeff * s1 * s2;
    2.0 * power.max(0.0).sqrt() / x.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_low_shelf_dc_gain() {
        let c = low_shelf_coeffs(48000.0, 700.0, 6.0, 0.7);
        let amp = biquad_amp(&c, 1.0, 48000.0);
        let expect = 10f32.powf(6.0 / 20.0);
        assert!(
            ((amp - expect) / expect).abs() < 1e-3,
            "amp={amp} expect={expect}"
        );
    }

    #[test]
    fn test_low_shelf_nyquist_gain() {
        let c = low_shelf_coeffs(48000.0, 700.0, 6.0, 0.7);
        let amp = biquad_amp(&c, 23999.0, 48000.0);
        assert!((amp - 1.0).abs() / 1.0 < 1e-2, "amp={amp}");
    }

    #[test]
    fn test_high_shelf_dc_gain() {
        let c = high_shelf_coeffs(48000.0, 700.0, 6.0, 0.7);
        let amp = biquad_amp(&c, 1.0, 48000.0);
        assert!((amp - 1.0).abs() < 1e-2, "amp={amp}");
    }

    #[test]
    fn test_high_shelf_nyquist_gain() {
        let c = high_shelf_coeffs(48000.0, 700.0, 6.0, 0.7);
        let amp = biquad_amp(&c, 23999.0, 48000.0);
        let expect = 10f32.powf(6.0 / 20.0);
        assert!(
            ((amp - expect) / expect).abs() < 1e-2,
            "amp={amp} expect={expect}"
        );
    }

    #[test]
    fn test_zero_gain_passthrough() {
        for &f in &[100.0f32, 700.0, 5000.0, 15000.0] {
            let ls = low_shelf_coeffs(48000.0, 700.0, 0.0, 0.7);
            let hs = high_shelf_coeffs(48000.0, 700.0, 0.0, 0.7);
            let amp_ls = biquad_amp(&ls, f, 48000.0);
            let amp_hs = biquad_amp(&hs, f, 48000.0);
            assert!((amp_ls - 1.0).abs() < 1e-4, "low f={f} amp={amp_ls}");
            assert!((amp_hs - 1.0).abs() < 1e-4, "high f={f} amp={amp_hs}");
        }
    }

    #[test]
    fn test_channel_zero_gain_passthrough() {
        let fs = 48000.0f32;
        let ls = low_shelf_coeffs(fs, 700.0, 0.0, 0.7);
        let hs = high_shelf_coeffs(fs, 700.0, 0.0, 0.7);
        let mut ch = TiltChannel::new();
        let n = fs as usize;
        let f = 997.0f64;
        let input: Vec<f32> = (0..n)
            .map(|i| (2.0 * std::f64::consts::PI * f * i as f64 / fs as f64).sin() as f32)
            .collect();
        let output: Vec<f32> = input.iter().map(|&x| ch.process(x, &ls, &hs)).collect();
        let amp_in = goertzel_amp(&input, f, fs as f64);
        let amp_out = goertzel_amp(&output, f, fs as f64);
        let ratio = amp_out / amp_in;
        assert!((ratio - 1.0).abs() < 1e-4, "ratio={ratio}");
    }

    #[test]
    fn test_channel_positive_tilt_high_boost() {
        let fs = 48000.0f32;
        let ls = low_shelf_coeffs(fs, 700.0, -6.0, 0.7);
        let hs = high_shelf_coeffs(fs, 700.0, 6.0, 0.7);

        let n = fs as usize;
        let measure = |freq: f64| -> f64 {
            let mut ch = TiltChannel::new();
            let input: Vec<f32> = (0..n)
                .map(|i| (2.0 * std::f64::consts::PI * freq * i as f64 / fs as f64).sin() as f32)
                .collect();
            let output: Vec<f32> = input.iter().map(|&x| ch.process(x, &ls, &hs)).collect();
            let amp_in = goertzel_amp(&input, freq, fs as f64);
            let amp_out = goertzel_amp(&output, freq, fs as f64);
            20.0 * (amp_out / amp_in).log10()
        };

        let db_5000 = measure(5000.0);
        let db_100 = measure(100.0);
        assert!(db_5000 > 3.0, "5000 Hz db={db_5000}");
        assert!(db_100 < -3.0, "100 Hz db={db_100}");
    }

    /// 非有限が **1 サンプルだけ**混入した後に正常復帰すること。
    ///
    /// 振幅 clamp を撤去したので、巨大な有限入力 (`1e38`) は縦列の内部で
    /// f32 を溢れさせて Inf を作れる。`is_finite` ガードが出力をゼロ化するだけだと
    /// DF1 の遅延段に Inf/NaN が残り、以降**永久に無音**になる
    /// (ガードがそれを隠すので誰も気づけない)。`reset()` がその唯一の復帰手段。
    ///
    /// 既存の `no_nan_no_denormal` (lib.rs) は敵対入力を**先頭から通す**ため、
    /// この「途中注入 → 復帰」を検出できない。本テストがその穴を埋める。
    #[test]
    fn recovers_after_nonfinite_sample() {
        let fs = 48000.0f32;
        // ゲイン最大側。内部で最も大きくなる設定を選ぶ。
        let ls = low_shelf_coeffs(fs, 700.0, -12.0, 1.0);
        let hs = high_shelf_coeffs(fs, 700.0, 12.0, 1.0);

        for &spike in &[1e38f32, f32::INFINITY, f32::NAN, -1e38] {
            let mut ch = TiltChannel::new();
            let mut out = Vec::with_capacity(2000);
            for i in 0..2000 {
                let x = if i == 0 {
                    spike
                } else {
                    0.5 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / fs).sin()
                };
                out.push(ch.process(x, &ls, &hs));
            }

            assert!(
                out.iter().all(|v| v.is_finite()),
                "spike={spike}: 非有限が出力に漏れた"
            );
            // 復帰の判定: スパイク直後を避けた区間が無音でないこと。
            // reset を忘れると全区間が 0.0 に張り付く。
            let tail_peak = out[1000..].iter().fold(0.0f32, |a, &v| a.max(v.abs()));
            assert!(
                tail_peak > 0.1,
                "spike={spike}: 復帰していない (tail_peak={tail_peak})。                 状態に非有限が残り永久ミュートになっている"
            );
        }
    }
}
