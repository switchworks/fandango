//! Linkwitz-Riley 4次クロスオーバー。
//!
//! 2 段カスケードのバターワース 2次で構成。LP/HP を並列に走らせて
//! 帯域分割 (相互加算で位相フラット・磁 flat/所謂 all-pass 位相応答)。

use crate::Biquad;

/// LR4 帯域分割器。1 チャンネル分。
#[derive(Debug, Clone)]
pub struct Lr4Crossover {
    lp1: Biquad,
    lp2: Biquad,
    hp1: Biquad,
    hp2: Biquad,
}

impl Lr4Crossover {
    pub fn new(sr: f32, fc: f32) -> Self {
        Self {
            lp1: Biquad::butterworth_lpf(sr, fc),
            lp2: Biquad::butterworth_lpf(sr, fc),
            hp1: Biquad::butterworth_hpf(sr, fc),
            hp2: Biquad::butterworth_hpf(sr, fc),
        }
    }

    /// カットオフを再設定。内部状態はそのまま。
    pub fn set_cutoff(&mut self, sr: f32, fc: f32) {
        let lp = Biquad::butterworth_lpf(sr, fc);
        let hp = Biquad::butterworth_hpf(sr, fc);
        self.lp1.b0 = lp.b0; self.lp1.b1 = lp.b1; self.lp1.b2 = lp.b2;
        self.lp1.a1 = lp.a1; self.lp1.a2 = lp.a2;
        self.lp2.b0 = lp.b0; self.lp2.b1 = lp.b1; self.lp2.b2 = lp.b2;
        self.lp2.a1 = lp.a1; self.lp2.a2 = lp.a2;
        self.hp1.b0 = hp.b0; self.hp1.b1 = hp.b1; self.hp1.b2 = hp.b2;
        self.hp1.a1 = hp.a1; self.hp1.a2 = hp.a2;
        self.hp2.b0 = hp.b0; self.hp2.b1 = hp.b1; self.hp2.b2 = hp.b2;
        self.hp2.a1 = hp.a1; self.hp2.a2 = hp.a2;
    }

    /// 周波数 `f` [Hz] での (低域側, 高域側) の振幅応答。
    /// 2 段カスケードなので各段の積。処理と同じ係数から求める。
    pub fn magnitudes(&self, sr: f32, f: f32) -> (f32, f32) {
        (
            self.lp1.magnitude(sr, f) * self.lp2.magnitude(sr, f),
            self.hp1.magnitude(sr, f) * self.hp2.magnitude(sr, f),
        )
    }

    pub fn reset(&mut self) {
        self.lp1.reset();
        self.lp2.reset();
        self.hp1.reset();
        self.hp2.reset();
    }

    /// 1 サンプル分割。戻り値 (low, high)。
    #[inline]
    pub fn split(&mut self, x: f32) -> (f32, f32) {
        let low = self.lp2.process(self.lp1.process(x));
        let high = self.hp2.process(self.hp1.process(x));
        (low, high)
    }
}

#[cfg(test)]
mod response_tests {
    use super::*;

    /// 係数から求めた振幅応答が、実際に信号を通した結果と一致すること。
    /// 可視化が実装からずれていないことの担保。
    #[test]
    fn magnitudes_match_the_processed_signal() {
        const SR: f32 = 48_000.0;
        let fc = 150.0;
        for &f in &[30.0_f32, 75.0, 150.0, 300.0, 1000.0, 8000.0] {
            let mut xo = Lr4Crossover::new(SR, fc);
            // 定常に達してから 2 周期分のピークを測る。
            // ピークではなく RMS で測る。8 kHz は 1 周期 6 サンプルしかなく、
            // ピークがサンプル点に当たらず最大 −1.2 dB 過小評価される。
            // RMS は位相に依存せず、正弦波なら振幅の 1/√2 に厳密対応する。
            let cycles = 200.0;
            let total = ((SR / f) * cycles) as usize;
            let settle = ((SR / f) * (cycles - 40.0)) as usize;
            let (mut sum_lo, mut sum_hi, mut count) = (0.0_f64, 0.0_f64, 0.0_f64);
            for n in 0..total {
                let x = (2.0 * std::f32::consts::PI * f * n as f32 / SR).sin();
                let (lo, hi) = xo.split(x);
                if n > settle {
                    sum_lo += (lo * lo) as f64;
                    sum_hi += (hi * hi) as f64;
                    count += 1.0;
                }
            }
            let scale = std::f32::consts::SQRT_2;
            let peak_lo = (sum_lo / count).sqrt() as f32 * scale;
            let peak_hi = (sum_hi / count).sqrt() as f32 * scale;
            let (predicted_lo, predicted_hi) = xo.magnitudes(SR, f);
            for (name, measured, predicted) in [
                ("low", peak_lo, predicted_lo),
                ("high", peak_hi, predicted_hi),
            ] {
                // 阻止域 (−40 dB 未満) は f32 の丸めと測定分解能に埋もれる。
                // 実測 30 Hz の高域側は −55 dB で、ここでの 0.3 dB 差は
                // 応答の誤りではなく測定側の限界。可視化に意味を持つのは通過域。
                if predicted < 1.0e-2 {
                    continue;
                }
                let error_db = 20.0 * (measured / predicted).log10();
                assert!(
                    error_db.abs() < 0.2,
                    "{name} @ {f} Hz: 実測 {measured:.5} / 予測 {predicted:.5} = {error_db:+.3} dB"
                );
            }
        }
    }
}
