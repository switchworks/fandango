//! trip-delay DSP — 2/8拍と3/8拍の非対称2タップシンコペーションディレイ。
//!
//! 別プロジェクトのディレイ実装からの移植。L/R が異なる拍位置で発音するため
//! ping-pong ではなくシンコペーションと三連的リズム感を持つ。

use fandango_shared::DelayLine;

pub struct TripDelayDsp {
    dline_l: DelayLine,
    dline_r: DelayLine,
    delay_l_samp: usize, // 2/8 拍
    delay_r_samp: usize, // 3/8 拍
    sample_rate: f32,
}

/// 対応する最低テンポ [BPM]。これ以下では遅延線が足りないため `set_tempo` が clamp する。
const BPM_MIN: f32 = 60.0;
/// 遅延線に積む余裕 [サンプル]。`set_tempo` の丸め（`round`）で `ceil` を 1 超えることは
/// 無いが、resize を伴わずにテンポが動く経路を安全側に倒すための定数。
const CAPACITY_SLACK: usize = 64;

/// 必要な遅延線長を閉形式で返す。最長タップは `BPM_MIN` での 3/8 拍。
///
/// 旧実装はこの値に `.max(17000)` の床を持っていたが、**全サポートレートで不活性**だった。
/// `DelayLine` は容量を 2 の冪乗へ切り上げる（`fandango_shared::DelayLine::resize`）ため、
/// 44.1 kHz では必要容量 16602 も床 17000 もともに 32768 へ丸まる。
/// 48 kHz 以上では必要容量が床を上回るので床は最初から効かない。
/// 値の出所が追えず、かつ結果を変えないので削除した（`capacity_floor_was_inert`）。
fn required_capacity(sample_rate: f32) -> usize {
    let max_beat = sample_rate * 60.0 / BPM_MIN;
    (max_beat * 3.0 / 8.0).ceil() as usize + CAPACITY_SLACK
}

impl TripDelayDsp {
    /// `prepare` 前の暫定状態。`initialize` で必ず `prepare` が呼ばれ上書きされる。
    pub fn new() -> Self {
        let sample_rate = 44_100.0;
        let mut dsp = Self {
            dline_l: DelayLine::with_capacity(required_capacity(sample_rate)),
            dline_r: DelayLine::with_capacity(required_capacity(sample_rate)),
            delay_l_samp: 1,
            delay_r_samp: 1,
            sample_rate,
        };
        dsp.set_tempo(120.0);
        dsp
    }

    /// 最大バッファサイズ = `BPM_MIN` での 3/8 拍 + 余裕（→ [`required_capacity`]）。
    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        let cap = required_capacity(sample_rate);
        self.dline_l.resize(cap);
        self.dline_r.resize(cap);
        self.set_tempo(120.0);
    }

    pub fn set_tempo(&mut self, bpm: f32) {
        let beat = self.sample_rate * 60.0 / bpm.max(1.0);
        let cap = self.dline_l.capacity() - 1;
        self.delay_l_samp = (((beat * 2.0 / 8.0).round()) as usize).clamp(1, cap);
        self.delay_r_samp = (((beat * 3.0 / 8.0).round()) as usize).clamp(1, cap);
    }

    pub fn reset(&mut self) {
        self.dline_l.reset();
        self.dline_r.reset();
    }

    /// 1 サンプル処理。dry は入力に含まれた状態で `out_l`, `out_r` を返す
    /// (原実装踏襲: dry passthrough + wet 加算)。
    #[inline]
    pub fn process_sample(&mut self, l: f32, r: f32, level: f32, feedback: f32) -> (f32, f32) {
        let in_s = 0.5 * (l + r);

        // 3/8 拍位置で self-feedback (両チャンネル共通周期)。
        //
        // L のタップは 2/8 拍だが、帰還は L/R とも 3/8 拍 (`delay_r_samp`) で回す。
        // したがって L の反射列は 2/8 起点にならず、3/8 周期で並ぶ。
        // **これは意図した設計** (2026-08-30 オーナー裁定で確定)。非対称なのは
        // タップ位置だけで、帰還の周期は両チャンネルで共通に保つ。
        // 事故ではないことの記録であり、値の変更対象ではない。
        let fbk_l = self.dline_l.read_int(self.delay_r_samp - 1);
        let fbk_r = self.dline_r.read_int(self.delay_r_samp - 1);
        self.dline_l.write(in_s + feedback * fbk_l);
        self.dline_r.write(in_s + feedback * fbk_r);

        // L: 2/8 拍, R: 3/8 拍
        let out_l = l + level * self.dline_l.read_int(self.delay_l_samp - 1);
        let out_r = r + level * self.dline_r.read_int(self.delay_r_samp - 1);
        (out_l, out_r)
    }
}

impl Default for TripDelayDsp {
    fn default() -> Self {
        Self::new()
    }
}

/// Level 写像の下端 dB。UI 値 `v = LEVEL_KNEE` 以上で `LEVEL_DB_MIN + LEVEL_DB_SPAN·v` dB。
const LEVEL_DB_MIN: f32 = -24.0;
/// Level 写像の dB 幅（`v = 1` で `LEVEL_DB_MIN + LEVEL_DB_SPAN` dB）。
const LEVEL_DB_SPAN: f32 = 15.0;

/// 無音への膝の幅（UI 値）。**閉形式で決まる。較正値ではない。**
///
/// 旧実装は `v = 0` で 0、`v → 0+` で `LEVEL_DB_MIN`（-24 dB）を返しており、
/// ゲインが一段で無音へ落ちていた。`SmoothingStyle::Linear` はターゲット `0.0` へ
/// 厳密に到達するので、Level を最小まで下げるだけでこの段差を踏む。
/// 実測（220 Hz 正弦・振幅 0.5・120 BPM・fb 0.30）で出力段差 -30.4 dBFS、
/// フルスケール素材では -24.4 dBFS。可聴なクリック
/// （再現: `cargo run -p fandango-trip-delay --example level_step --release`）。
///
/// 膝を入れて連続化する。幅を決める条件は
/// **「膝が、この写像が既に出しているゲイン変化率を超えないこと」**:
///
/// ```text
///     amp(0+) / K = amp(1) - amp(0+)
///     K = 1 / (10^(SPAN/20) - 1)
/// ```
///
/// 左辺が膝の単位移動量あたりのゲイン変化率、右辺が可動域全体のそれ。
/// `K` は `LEVEL_DB_SPAN` のみで決まり `LEVEL_DB_MIN` に依らない。
///
/// より強い条件「膝の**瞬間**傾きが可動域のどこよりも速くない」も満たす
/// （膝端 0.5822 ≤ 可動域最大 0.6127。`knee_slope_never_exceeds_range_slope`）。
/// 指数の凸性から傾き最大は `v = 1` に来るので、膝側が下回る。
///
/// `v >= K` では旧実装と bit-exact 一致する（`above_knee_matches_legacy`）。
const LEVEL_KNEE: f32 = 0.216_289_6;

/// Level UI 値 (0..1) を線形振幅に変換。
///
/// `v = 0` で厳密に 0。`v >= LEVEL_KNEE` で `-24..-9 dB` の指数写像そのもの。
/// その間は線形フェードで繋ぐ（→ [`LEVEL_KNEE`]）。
pub fn level_ui_to_amp(v: f32) -> f32 {
    if v <= 0.0 {
        return 0.0;
    }
    let v = v.min(1.0);
    let db = LEVEL_DB_MIN + v * LEVEL_DB_SPAN;
    let base = 10.0_f32.powf(db / 20.0);
    if v >= LEVEL_KNEE {
        base
    } else {
        (v / LEVEL_KNEE) * base
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 膝幅は閉形式 `K = 1/(10^(SPAN/20) - 1)` で決まる。定数がそこからずれていないこと。
    #[test]
    fn knee_matches_closed_form() {
        let k = 1.0 / (10.0_f32.powf(LEVEL_DB_SPAN / 20.0) - 1.0);
        assert!(
            (LEVEL_KNEE - k).abs() < 1e-6,
            "LEVEL_KNEE = {LEVEL_KNEE}, closed form = {k}"
        );
    }

    /// `v = 0` で厳密に 0、かつ 0 近傍で段差が無いこと（本修正の目的）。
    #[test]
    fn level_is_continuous_at_zero() {
        assert_eq!(level_ui_to_amp(0.0), 0.0);
        for &v in &[1e-6_f32, 1e-5, 1e-4, 1e-3] {
            let a = level_ui_to_amp(v);
            // 線形フェード領域なので amp は v に比例して 0 へ向かう
            assert!(a < 1e-3, "v = {v} で amp = {a} (段差が残っている)");
        }
    }

    /// 膝の上では旧実装と bit-exact 一致すること。
    #[test]
    fn above_knee_matches_legacy() {
        for i in 0..=100 {
            let v = LEVEL_KNEE + (1.0 - LEVEL_KNEE) * (i as f32 / 100.0);
            let legacy = 10.0_f32.powf((-24.0 + v.min(1.0) * 15.0) / 20.0);
            assert_eq!(level_ui_to_amp(v), legacy, "v = {v}");
        }
    }

    /// 膝の中でのゲイン傾きが、可動域のどこよりも速くならないこと。
    /// これが本修正の受け入れ基準（段差を消しても新しい急変を作らない）。
    #[test]
    fn knee_slope_never_exceeds_range_slope() {
        let n = 200_000;
        let mut max_knee = 0.0_f32;
        let mut max_above = 0.0_f32;
        let h = 1.0 / n as f32;
        for i in 0..n {
            let v = i as f32 * h;
            let slope = (level_ui_to_amp(v + h) - level_ui_to_amp(v)) / h;
            if v + h <= LEVEL_KNEE {
                max_knee = max_knee.max(slope);
            } else if v >= LEVEL_KNEE {
                max_above = max_above.max(slope);
            }
        }
        assert!(
            max_knee <= max_above,
            "膝の傾き {max_knee} が可動域最大 {max_above} を超えている"
        );
    }

    #[test]
    fn level_mapping_endpoints() {
        assert_eq!(level_ui_to_amp(0.0), 0.0);
        let v_min = level_ui_to_amp(LEVEL_KNEE);
        assert!((v_min - 10.0_f32.powf((-24.0 + LEVEL_KNEE * 15.0) / 20.0)).abs() < 1e-6);
        let v_max = level_ui_to_amp(1.0);
        assert!((v_max - 10.0_f32.powf(-9.0 / 20.0)).abs() < 1e-4);
    }

    /// 旧実装の `.max(17000)` は全サポートレートで確保長を変えなかったこと。
    /// `DelayLine` は 2 の冪乗へ切り上げるので、床の有無で結果が一致する。
    #[test]
    fn capacity_floor_was_inert() {
        for &sr in &[44_100.0_f32, 48_000.0, 88_200.0, 96_000.0, 176_400.0, 192_000.0] {
            let req = required_capacity(sr);
            let legacy = req.max(17_000);
            let a = DelayLine::with_capacity(req).capacity();
            let b = DelayLine::with_capacity(legacy).capacity();
            assert_eq!(a, b, "sr = {sr}: req = {req} -> {a}, legacy = {legacy} -> {b}");
        }
    }

    /// 確保長が最長タップ（BPM_MIN の 3/8 拍）を実際に収容できること。
    #[test]
    fn capacity_covers_longest_tap() {
        for &sr in &[44_100.0_f32, 48_000.0, 96_000.0, 192_000.0] {
            let mut dsp = TripDelayDsp::new();
            dsp.prepare(sr);
            dsp.set_tempo(BPM_MIN);
            let longest = ((sr * 60.0 / BPM_MIN) * 3.0 / 8.0).round() as usize;
            assert_eq!(dsp.delay_r_samp, longest, "sr = {sr}: clamp されている");
            assert!(
                dsp.dline_r.capacity() > longest,
                "sr = {sr}: cap = {} <= longest = {longest}",
                dsp.dline_r.capacity()
            );
        }
    }

    #[test]
    fn tap_positions_reflect_2_8_and_3_8() {
        let mut dsp = TripDelayDsp::new();
        dsp.prepare(48000.0);
        dsp.set_tempo(120.0);
        // 120 BPM at 48kHz: 1 拍 = 24000 サンプル
        // 2/8 = 6000, 3/8 = 9000
        assert_eq!(dsp.delay_l_samp, 6000);
        assert_eq!(dsp.delay_r_samp, 9000);
    }
}
