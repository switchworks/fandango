//! velvet DSP コア — ヴェルベットノイズ (OVN) タップ列生成 + 疎 FIR 畳み込み。
//!
//! **PRNG バイト再現性について**: `Xorshift64` は fandango 独自の固定シード
//! 自前実装。`std` の乱数・`rand` crate 等への置き換え禁止 (受け入れ基準 2
//! の決定論が破綻する)。

/// 1 サンプルあたりのタップ数上限。50 ms × 1500 個/秒 = 75、余裕 5。
pub const MAX_TAPS_PER_CH: usize = 80;

/// パルス密度 (個/秒)。v0.1 では固定 (パラメータ化しない)。
pub const DENSITY: f32 = 1500.0;

pub use fandango_shared::velvet::{DecorrelatorState, VelvetSequence};

/// ヴェルベットノイズタップ列を生成する。
///
/// - `seed`: xorshift64* 初期シード (固定定数)
/// - `fs`: サンプルレート
/// - `length_sec`: シーケンス長 L (秒、-60 dB 減衰時間)
/// - `density`: パルス密度 ρ (個/秒)
pub fn build_sequence(seed: u64, fs: f32, length_sec: f32, density: f32) -> VelvetSequence {
    let decay_coef = -6.91 / (fs * length_sec); // exp( decay_coef · k )
    let seq = fandango_shared::velvet::build_sequence(seed, fs, length_sec, density, |k| {
        (decay_coef * k as f32).exp()
    });
    debug_assert!(seq.taps_k.len() <= MAX_TAPS_PER_CH);
    seq
}

/// wet 単体 RMS を入力等価にするエネルギー正規化係数 `1 / sqrt(Σ g(m)²)`。
pub fn energy_norm(seq: &VelvetSequence) -> f32 {
    let sum_sq: f32 = seq.taps_gs.iter().map(|&x| x * x).sum();
    1.0 / sum_sq.sqrt()
}

/// 1 次 IIR ローパス。`y[n] = y[n-1] + α·(x[n] - y[n-1])`。wet 信号 (color) 用。
pub struct OnePoleLp {
    y: f32,
}

impl OnePoleLp {
    pub fn new() -> Self {
        Self { y: 0.0 }
    }

    pub fn reset(&mut self) {
        self.y = 0.0;
    }

    #[inline]
    pub fn process(&mut self, x: f32, alpha: f32) -> f32 {
        self.y += alpha * (x - self.y);
        self.y
    }
}

/// `color` パラメータ (0.0–1.0) から LP カットオフ周波数 (Hz) を計算する。
/// 0.0 で 20 kHz (ほぼバイパス)、1.0 で 2 kHz (Dark Velvet 方向、10 分の 1)。
///
/// 計画書 (velvet_v01.md) は式を `20000 * exp(-4.6*color)` と記載しているが、
/// この係数だと 100% 時に 201 Hz になり同計画書の「100% で 2 kHz」という
/// 明記された数値目標と矛盾する (係数は本来 ln(10) ≈ 2.302585 であるべき
/// ところ 2*ln(10) ≈ 4.60517 になっていたと見られる)。本実装では明記された
/// 数値目標 (0%→20kHz, 100%→2kHz) を正とし、`10^(-color)` で計算する。
pub fn color_to_cutoff_hz(color: f32) -> f32 {
    20000.0 * 10f32.powf(-color)
}

/// カットオフ周波数とサンプルレートから 1 次 LP の平滑係数 α を計算する。
pub fn lp_alpha(cutoff_hz: f32, fs: f32) -> f32 {
    1.0 - (-2.0 * std::f32::consts::PI * cutoff_hz / fs).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    const L_SEED: u64 = 0x9E3779B97F4A7C15;

    #[test]
    fn test_tap_count() {
        let seq = build_sequence(L_SEED, 48000.0, 0.030, 1500.0);
        assert_eq!(seq.taps_k.len(), 45);
        assert_eq!(seq.taps_gs.len(), 45);
    }

    #[test]
    fn test_determinism() {
        let seq1 = build_sequence(L_SEED, 48000.0, 0.030, 1500.0);
        let seq2 = build_sequence(L_SEED, 48000.0, 0.030, 1500.0);
        assert_eq!(seq1.taps_k, seq2.taps_k);
        assert_eq!(seq1.taps_gs, seq2.taps_gs);
    }

    #[test]
    fn test_taps_within_grid() {
        let fs = 48000.0f32;
        let density = 1500.0f32;
        let td = fs / density;
        let seq = build_sequence(L_SEED, fs, 0.030, density);
        for (m, &k) in seq.taps_k.iter().enumerate() {
            let lo = (m as f32) * td;
            let hi = ((m + 1) as f32) * td;
            assert!(
                (k as f32) >= lo && (k as f32) < hi,
                "m={m} k={k} lo={lo} hi={hi}"
            );
        }
    }

    #[test]
    fn test_energy_norm_positive() {
        let seq = build_sequence(L_SEED, 48000.0, 0.030, 1500.0);
        let norm = energy_norm(&seq);
        assert!(norm > 0.0 && norm.is_finite());
    }

    #[test]
    fn test_g_monotonic_decay() {
        // g(m) 単体 (符号抜き) は k に対し単調減少。taps_gs は符号込みなので
        // ここでは g を taps_gs.abs() として再計算比較する (符号は乱数だが
        // |gs| = g のため abs で単調性を検証できる)。
        let fs = 48000.0f32;
        let length_sec = 0.030f32;
        let seq = build_sequence(L_SEED, fs, length_sec, 1500.0);
        let mut prev_k = 0u32;
        let mut prev_g = 1.0f32;
        for (i, (&k, &gs)) in seq.taps_k.iter().zip(seq.taps_gs.iter()).enumerate() {
            let g = gs.abs();
            if i > 0 {
                assert!(
                    k >= prev_k,
                    "k は m に対し単調増加のはず: i={i} k={k} prev_k={prev_k}"
                );
                assert!(
                    g <= prev_g,
                    "g は k に対し単調減少のはず: i={i} g={g} prev_g={prev_g}"
                );
            }
            prev_k = k;
            prev_g = g;
        }
    }

    #[test]
    fn test_impulse_response_length() {
        let fs = 48000.0f32;
        let length_sec = 0.030f32;
        let seq = build_sequence(L_SEED, fs, length_sec, DENSITY);
        let max_len_samples = (length_sec * fs).ceil() as u32 + 2400; // 50ms分余裕
        let mut state = DecorrelatorState::new(max_len_samples);

        let total_samples = (0.050 * fs as f64).ceil() as usize; // 50 ms 分流す
        let boundary = (length_sec * fs).ceil() as usize + 48; // 1 ms マージン
        for n in 0..total_samples {
            let x = if n == 0 { 1.0 } else { 0.0 };
            let y = state.process(x, &seq);
            if n >= boundary {
                assert_eq!(y, 0.0, "n={n} boundary={boundary} で出力が非ゼロ: {y}");
            }
        }
    }

    /// future_plugins.md §測定用ユーティリティ記載の Goertzel 振幅測定。
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

    #[test]
    fn test_linearity() {
        let fs = 48000.0f32;
        let length_sec = 0.030f32;
        let freq = 997.0f64;
        let n_samples = fs as usize; // 1 秒 = 997 周期 (整数周期)

        let run = |amp: f32| -> f64 {
            let seq = build_sequence(L_SEED, fs, length_sec, DENSITY);
            let mut state = DecorrelatorState::new((length_sec * fs).ceil() as u32 + 64);
            let mut out = Vec::with_capacity(n_samples);
            for n in 0..n_samples {
                let phase = 2.0 * std::f64::consts::PI * freq * n as f64 / fs as f64;
                let x = amp * phase.sin() as f32;
                out.push(state.process(x, &seq));
            }
            goertzel_amp(&out, freq, fs as f64)
        };

        let amp_full = run(0.5);
        let amp_half = run(0.25);
        let ratio = amp_full / amp_half;
        assert!(
            (ratio - 2.0).abs() / 2.0 < 1e-6,
            "ratio={ratio} (expected 2.0)"
        );
    }
}
