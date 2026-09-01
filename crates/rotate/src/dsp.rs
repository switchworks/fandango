//! 1 次オールパスセクションおよびその縦列 (`AllpassChain`)。
//!
//! `H(z) = (a + z⁻¹) / (1 + a·z⁻¹)`。振幅特性は全周波数で厳密に 1、
//! `0 < fc < fs/2` である限り `|a| < 1` で無条件安定。

/// 最大段数 (`Stages::Eight` に対応)。
pub const MAX_STAGES: usize = 8;

/// 1 次オールパスセクション係数を計算する。
///
/// `fc`: カットオフ周波数 (Hz)、`sr`: サンプルレート (Hz)。
/// `0 < fc < sr/2` を前提とする (呼び出し側でパラメータ範囲により保証)。
pub fn allpass_coeff(fc: f32, sr: f32) -> f32 {
    let t = (std::f32::consts::PI * fc / sr).tan();
    (t - 1.0) / (t + 1.0)
}

/// 1 次オールパスセクション (Direct Form II transposed、状態 1 個)。
#[derive(Debug, Clone, Copy, Default)]
pub struct AllpassSection {
    s: f32,
}

impl AllpassSection {
    pub fn new() -> Self {
        Self { s: 0.0 }
    }

    pub fn reset(&mut self) {
        self.s = 0.0;
    }

    /// `a`: `allpass_coeff` で得た係数。
    pub fn process(&mut self, x: f32, a: f32) -> f32 {
        let y = a * x + self.s;
        self.s = x - a * y;
        y
    }
}

/// 最大 [`MAX_STAGES`] 段の固定配列。実稼働段数は `process` 呼び出し時に指定する。
pub struct AllpassChain {
    sections: [AllpassSection; MAX_STAGES],
}

impl AllpassChain {
    pub fn new() -> Self {
        Self { sections: [AllpassSection::new(); MAX_STAGES] }
    }

    pub fn reset(&mut self) {
        for s in &mut self.sections {
            s.reset();
        }
    }

    /// `stages`: 稼働段数 (2/4/6/8 のいずれか)。
    pub fn process(&mut self, x: f32, fc: f32, sr: f32, stages: usize) -> f32 {
        let a = allpass_coeff(fc, sr);
        let mut y = x;
        for s in self.sections.iter_mut().take(stages) {
            y = s.process(y, a);
        }
        y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 信号 x に含まれる周波数 f (Hz) 成分の振幅を返す (正規化: 振幅 1.0 の
    /// 純音を入れたら 1.0 が返る)。
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

    fn sine(f: f64, fs: f64, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (2.0 * std::f64::consts::PI * f * i as f64 / fs).sin() as f32)
            .collect()
    }

    #[test]
    fn coeff_stable_for_valid_range() {
        let sr = 48_000.0;
        for fc in [50.0, 200.0, 1000.0, 2000.0, 23_999.0] {
            let a = allpass_coeff(fc, sr);
            assert!(a.abs() < 1.0, "fc={fc} a={a}");
        }
    }

    /// 基準1 (受け入れ基準1): 振幅平坦。全 stages × freq 端点で
    /// 50/100/500/1000/5000/15000 Hz の入出力振幅比が 1.0 ± 1e-4。
    ///
    /// オールパスの極半径は低域端点 (fc=50 Hz) で |a|≈0.9935、時定数約
    /// 153 サンプルの過渡応答を持つ。Goertzel は窓全体を積算するため過渡分が
    /// 誤差になる → 測定前に 4800 サンプル (時定数の 30 倍超、-270 dB 相当)
    /// のウォームアップで定常状態にしてから計測窓 (整数周期 1 秒) を取る。
    #[test]
    fn amplitude_flat_all_stages_and_freq_endpoints() {
        let sr = 48_000.0;
        let warmup = 4800usize;
        let measure = sr as usize; // 1 秒 = 整数周期を保証
        for stages in [2usize, 4, 6, 8] {
            for fc in [50.0f32, 200.0, 2000.0] {
                for &f in &[50.0, 100.0, 500.0, 1000.0, 5000.0, 15000.0] {
                    let x_full = sine(f, sr as f64, warmup + measure);
                    let mut chain = AllpassChain::new();
                    let y_full: Vec<f32> =
                        x_full.iter().map(|&v| chain.process(v, fc, sr, stages)).collect();
                    let x = &x_full[warmup..];
                    let y = &y_full[warmup..];
                    let amp_in = goertzel_amp(x, f, sr as f64);
                    let amp_out = goertzel_amp(y, f, sr as f64);
                    let ratio = amp_out / amp_in;
                    assert!(
                        (ratio - 1.0).abs() < 1e-4,
                        "stages={stages} fc={fc} f={f} ratio={ratio}"
                    );
                }
            }
        }
    }

    /// 基準3: 全パラメータ端点で |a| < 1。
    #[test]
    fn coeff_endpoints_stable() {
        let sr = 48_000.0;
        for fc in [50.0f32, 2000.0] {
            let a = allpass_coeff(fc, sr);
            assert!(a.abs() < 1.0);
        }
    }

    /// 受け入れ基準 2 のテスト信号
    /// `x[n] = 0.5*sin(wn) + 0.35*sin(2wn+0.9) + 0.25*sin(3wn+2.2)`
    /// (f=110 Hz, fs=48k, 1 秒)。
    fn peak_test_signal(sr: f64, n: usize) -> Vec<f32> {
        let f = 110.0;
        let w = 2.0 * std::f64::consts::PI * f / sr;
        (0..n)
            .map(|i| {
                let t = i as f64;
                (0.5 * (w * t).sin() + 0.35 * (2.0 * w * t + 0.9).sin() + 0.25 * (3.0 * w * t + 2.2).sin())
                    as f32
            })
            .collect()
    }

    /// 基準2: ピーク低減の実証。stages=8, freq=110 Hz 適用後のピーク絶対値が
    /// 入力ピーク比 0.97 以下に低下する。
    #[test]
    fn peak_reduction_stages8_freq110() {
        let sr = 48_000.0;
        let n = sr as usize;
        let x = peak_test_signal(sr as f64, n);
        let mut chain = AllpassChain::new();
        let y: Vec<f32> = x.iter().map(|&v| chain.process(v, 110.0, sr, 8)).collect();

        let peak_in = x.iter().fold(0.0f32, |m, &v| m.max(v.abs()));
        let peak_out = y.iter().fold(0.0f32, |m, &v| m.max(v.abs()));
        let ratio = peak_out / peak_in;
        assert!(ratio <= 0.97, "peak ratio={ratio} (peak_in={peak_in} peak_out={peak_out})");
    }

    /// 基準4: RMS 不変。同一テスト信号で入出力 RMS 比 1.0 ± 1e-4。
    ///
    /// f=110 Hz は 1 秒バッファに非整数周期 (436.36 周期) しか収まらず、
    /// 立ち上がり過渡がそのまま境界エネルギー差になる → 振幅平坦テスト同様
    /// ウォームアップ (4800 サンプル) で定常状態にしてから比較する。
    #[test]
    fn rms_invariant_stages8_freq110() {
        let sr = 48_000.0;
        let warmup = 4800usize;
        let measure = sr as usize;
        let x_full = peak_test_signal(sr as f64, warmup + measure);
        let mut chain = AllpassChain::new();
        let y_full: Vec<f32> = x_full.iter().map(|&v| chain.process(v, 110.0, sr, 8)).collect();
        let x = &x_full[warmup..];
        let y = &y_full[warmup..];

        let rms = |s: &[f32]| -> f64 {
            (s.iter().map(|&v| (v as f64) * (v as f64)).sum::<f64>() / s.len() as f64).sqrt()
        };
        let rms_in = rms(x);
        let rms_out = rms(y);
        let ratio = rms_out / rms_in;
        assert!((ratio - 1.0).abs() < 1e-4, "rms ratio={ratio}");
    }
}
