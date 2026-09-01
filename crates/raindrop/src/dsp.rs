//! raindrop DSP — Dual IIR comb + Haas-shuffle + WIDTH ノブ。
//!
//! WIDTH=0 で Mid コア + Haas-shuffle MAX、
//! WIDTH=1 で L/R 完全独立コム + Haas 無し (純ステレオエコー)。

use fandango_shared::DelayLine;

/// 較正済み固定値: 第1反射メインゲイン。
pub const G_MAIN: f32 = 0.620;
/// 較正済み固定値: Haas 分数遅延の時間 [ms]。
///
/// 較正実測にもとづく。基準サンプルレート 44.1 kHz で、Haas タップは主タップから
/// `108 + 5/9` サンプル後方にある (= 2.4616 ms)。分数部 5/9 は補間タップ 2 点の
/// 振幅比として実測されたもの。
///
/// サンプル数ではなく時間で持つ理由: Haas 効果の知覚量は時間で決まるので、
/// サンプル数固定だと実効遅延が sr に反比例して再生環境ごとに効きが変わる。
/// 同じ crate の `time` パラメータは sr 換算済みで、そこと揃える。
///
/// f64 で持つ。f32 に丸めると ms → サンプル数へ戻したときに分数部が 5/9 から外れ、
/// 基準 sr での bit-exact が壊れる。
pub const HAAS_TAU_MS: f64 = HAAS_TAU_SAMPLES_AT_REF / HAAS_REF_SR * 1000.0;

/// 較正実測で得た主タップからのサンプル数 (基準 sr でのみ意味を持つ)。
const HAAS_TAU_SAMPLES_AT_REF: f64 = 108.0 + 5.0 / 9.0;
/// 較正時のサンプルレート [Hz]。`RaindropDsp::new` の既定値と一致させること。
const HAAS_REF_SR: f64 = 44_100.0;

/// サンプルレートから Haas タップの整数部と分数部を導く。
///
/// 分数遅延の線形補間は既存の機構をそのまま使うので、返すのは読み出し位置だけ。
pub fn haas_tap(sample_rate: f32) -> (usize, f32) {
    let samples = HAAS_TAU_MS as f64 * 1e-3 * sample_rate as f64;
    let int = samples.floor();
    // 整数部が 1 未満だと read_int(int - 1) が破綻する。可聴なサンプルレートでは
    // 起きないが、握り潰さずに下限を構造として持つ。
    let int = int.max(1.0);
    (int as usize, (samples - int) as f32)
}
/// WIDTH=0 時の Haas-shuffle 最大量。
pub const HAAS_MAX_AMOUNT: f32 = 0.90;

#[derive(Debug, Clone, Copy)]
pub struct RaindropParams {
    /// 遅延時間 (サンプル数、整数)。
    pub delay_samples: usize,
    /// フィードバック 0.0..0.90。
    pub feedback: f32,
    /// WIDTH 0.0..1.0 (0 = Mid コア、1 = 純ステレオ)。
    pub width: f32,
    /// MIX 0.0..1.0 (Dry/Wet 線形補間)。
    pub mix: f32,
}

pub struct RaindropDsp {
    dline_l: DelayLine,
    dline_r: DelayLine,
    dline_haas: DelayLine,
    /// `haas_tap(sr)` の整数部・分数部。`process` 内で再計算しない。
    haas_int: usize,
    haas_frac: f32,
}

impl RaindropDsp {
    /// 最大遅延サンプル数を確保。sr に応じて呼び出し側で計算。
    pub fn new(max_delay_samples: usize) -> Self {
        let (haas_int, haas_frac) = haas_tap(HAAS_REF_SR as f32);
        Self {
            dline_l: DelayLine::with_capacity(max_delay_samples + 1),
            dline_r: DelayLine::with_capacity(max_delay_samples + 1),
            dline_haas: DelayLine::with_capacity(haas_int + 2),
            haas_int,
            haas_frac,
        }
    }

    /// サンプルレートを設定する。Haas タップと遅延線の確保長を組み直す。
    /// `initialize` / sr 変更時に呼ぶ (`process` 内でアロケーションしないため)。
    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        let (int, frac) = haas_tap(sample_rate);
        self.haas_int = int;
        self.haas_frac = frac;
        self.dline_haas.resize(int + 2);
    }

    pub fn resize(&mut self, max_delay_samples: usize) {
        self.dline_l.resize(max_delay_samples + 1);
        self.dline_r.resize(max_delay_samples + 1);
        self.dline_haas.resize(self.haas_int + 2);
    }

    pub fn reset(&mut self) {
        self.dline_l.reset();
        self.dline_r.reset();
        self.dline_haas.reset();
    }

    /// 1 サンプル処理。
    #[inline]
    pub fn process_sample(&mut self, l: f32, r: f32, p: RaindropParams) -> (f32, f32) {
        let alpha = p.width.clamp(0.0, 1.0);

        // Pre-stage: WIDTH スケール付き入力
        let mid = 0.5 * (l + r);
        let side = 0.5 * (l - r);
        let l_pre = mid + alpha * side;
        let r_pre = mid - alpha * side;

        // Dual IIR comb (L/R 独立)
        // 内部状態 z[n] = M_in[n] + EFB * z[n-D]、wet[n] = G_MAIN * z[n-D]。
        // 反射比が feedback と一致するよう z を状態とする。
        let d = p.delay_samples.max(1);
        let delayed_l = self.dline_l.read_int(d - 1);
        let delayed_r = self.dline_r.read_int(d - 1);
        let wet_l = G_MAIN * delayed_l;
        let wet_r = G_MAIN * delayed_r;
        self.dline_l.write(l_pre + p.feedback * delayed_l);
        self.dline_r.write(r_pre + p.feedback * delayed_r);

        // Haas-shuffle (WIDTH に逆相関)
        let h_amount = (1.0 - alpha) * HAAS_MAX_AMOUNT;
        let m_a = self.dline_haas.read_int(self.haas_int - 1);
        let m_b = self.dline_haas.read_int(self.haas_int);
        let wet_s_haas = (1.0 - self.haas_frac) * m_a + self.haas_frac * m_b;
        self.dline_haas.write(0.5 * (wet_l + wet_r));

        let l_wet = wet_l + h_amount * wet_s_haas;
        let r_wet = wet_r - h_amount * wet_s_haas;

        // Mix (dry 常時、wet を MIX でスケール加算)
        let out_l = l + p.mix * l_wet;
        let out_r = r + p.mix * r_wet;
        (out_l, out_r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn width_zero_collapses_to_mid_core() {
        // WIDTH=0 で L 入力と R=0 入力を通した wet 出力は Mid 化 → wet_L = wet_R。
        let mut dsp = RaindropDsp::new(1000);
        let params = RaindropParams {
            delay_samples: 100,
            feedback: 0.0,
            width: 0.0,
            mix: 1.0,
        };
        // インパルスを L に入れる
        let (_out_l0, _out_r0) = dsp.process_sample(1.0, 0.0, params);
        for _ in 0..99 {
            dsp.process_sample(0.0, 0.0, params);
        }
        // 100 サンプル後の第1反射: dry=0 + wet = wet_L, wet_R
        let (out_l, out_r) = dsp.process_sample(0.0, 0.0, params);
        // wet_L = G_MAIN * (M-input scaled) = 0.620 * 0.5 = 0.310
        // wet_R も同じ (Mid化のため)
        assert!((out_l - out_r).abs() < 1e-6, "L/R diff = {}", out_l - out_r);
        assert!((out_l - 0.310).abs() < 1e-4, "wet_L = {}", out_l);
    }

    #[test]
    fn width_one_preserves_stereo() {
        // WIDTH=1 で L だけに入力 → L 側のみエコー、R 側はエコー無し。
        let mut dsp = RaindropDsp::new(1000);
        let params = RaindropParams {
            delay_samples: 100,
            feedback: 0.0,
            width: 1.0,
            mix: 1.0,
        };
        dsp.process_sample(1.0, 0.0, params);
        for _ in 0..99 {
            dsp.process_sample(0.0, 0.0, params);
        }
        let (out_l, out_r) = dsp.process_sample(0.0, 0.0, params);
        // WIDTH=1: L_pre=1, R_pre=0 → wet_L = 0.620, wet_R = 0
        // Haas 無し (h_amount = 0)
        assert!((out_l - 0.620).abs() < 1e-4, "out_l = {}", out_l);
        assert!(out_r.abs() < 1e-4, "out_r = {}", out_r);
    }

    #[test]
    fn mix_zero_bypasses_wet() {
        let mut dsp = RaindropDsp::new(1000);
        let params = RaindropParams {
            delay_samples: 100,
            feedback: 0.7,
            width: 0.0,
            mix: 0.0,
        };
        let (out_l, out_r) = dsp.process_sample(0.5, -0.3, params);
        assert!((out_l - 0.5).abs() < 1e-6);
        assert!((out_r - (-0.3)).abs() < 1e-6);
    }

    #[test]
    fn feedback_repeats_at_delay_interval() {
        let mut dsp = RaindropDsp::new(1000);
        let d = 50;
        let fb = 0.5;
        let params = RaindropParams {
            delay_samples: d,
            feedback: fb,
            width: 1.0, // L 側独立エコー
            mix: 1.0,
        };
        // インパルス L
        dsp.process_sample(1.0, 0.0, params);
        let mut peaks = Vec::new();
        for i in 1..=5 * d {
            let (l, _) = dsp.process_sample(0.0, 0.0, params);
            if i % d == 0 {
                peaks.push(l);
            }
        }
        // 反射比 = feedback (原信号は G_MAIN で 0.620)
        // peaks[0] = 0.620, peaks[1] = 0.620 * fb, ...
        assert!((peaks[0] - 0.620).abs() < 1e-4, "peak0 = {}", peaks[0]);
        let ratio = peaks[1] / peaks[0];
        assert!((ratio - fb).abs() < 1e-4, "ratio = {}", ratio);
    }

    #[test]
    fn haas_tap_matches_calibration_at_ref_sr() {
        // 較正実測値 (基準 sr で 108 + 5/9 サンプル)。bit-exact に戻ること。
        let (int, frac) = haas_tap(HAAS_REF_SR as f32);
        assert_eq!(int, 108);
        assert_eq!(frac, 5.0_f32 / 9.0);
    }
}
