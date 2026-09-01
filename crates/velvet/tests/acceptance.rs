//! velvet v0.1 受け入れテスト一式。
//!
//! Goertzel は future_plugins.md §測定用ユーティリティより転記。

use fandango_velvet::dsp::{
    build_sequence, color_to_cutoff_hz, energy_norm, lp_alpha, DecorrelatorState, OnePoleLp,
    DENSITY, MAX_TAPS_PER_CH,
};

/// L チャンネル用シード。lib.rs の定数と一致させ、決定論・モノ互換の
/// 焼込済みシードを外部からも検証する。
const L_SEED: u64 = 0x98C94E6B3AA7E83D;
/// R チャンネル用シード。
const R_SEED: u64 = 0xB86598FE74D7396A;

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
fn test_linearity_997hz() {
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

#[test]
fn test_determinism_across_initialize() {
    let fs = 48000.0f32;
    let lengths = [0.010f32, 0.020, 0.030, 0.040, 0.050];
    for &l in &lengths {
        for &seed in &[L_SEED, R_SEED] {
            let seq1 = build_sequence(seed, fs, l, DENSITY);
            let seq2 = build_sequence(seed, fs, l, DENSITY);
            assert_eq!(seq1.taps_k, seq2.taps_k, "seed={seed:#x} l={l}");
            assert_eq!(seq1.taps_gs, seq2.taps_gs, "seed={seed:#x} l={l}");
        }
    }
}

#[test]
fn test_impulse_zero_after_length() {
    let fs = 48000.0f32;
    let length_sec = 0.050f32; // length = 50 ms 時
    let seq = build_sequence(L_SEED, fs, length_sec, DENSITY);
    let mut state = DecorrelatorState::new((length_sec * fs).ceil() as u32 + 64);

    let total_samples = (0.060 * fs as f64).ceil() as usize; // 60 ms 分流す
    let boundary = (0.055 * fs as f64).ceil() as usize; // 55 ms 以降ゼロ
    for n in 0..total_samples {
        let x = if n == 0 { 1.0 } else { 0.0 };
        let y = state.process(x, &seq);
        if n >= boundary {
            assert_eq!(y, 0.0, "n={n} boundary={boundary} で出力が非ゼロ: {y}");
        }
    }
}

/// 受け入れ基準 3「全パラメータ端点でインパルス応答が length + 1 ms 以降ゼロ」
/// を length の全 5 段 (10/20/30/40/50 ms) で検証する。
#[test]
fn test_impulse_zero_all_length_endpoints() {
    let fs = 48000.0f32;
    for &length_sec in &[0.010f32, 0.020, 0.030, 0.040, 0.050] {
        let seq = build_sequence(L_SEED, fs, length_sec, DENSITY);
        let mut state = DecorrelatorState::new((length_sec * fs).ceil() as u32 + 64);

        let boundary = (length_sec * fs).ceil() as usize + 48; // length + 1ms マージン
        let total_samples = boundary + 480; // 境界後 10ms 分も確認
        for n in 0..total_samples {
            let x = if n == 0 { 1.0 } else { 0.0 };
            let y = state.process(x, &seq);
            if n >= boundary {
                assert_eq!(
                    y, 0.0,
                    "length={length_sec} n={n} boundary={boundary} で出力が非ゼロ: {y}"
                );
            }
        }
    }
}

/// `(outL+outR)/2` のモノ和インパルス応答から DTFT で周波数応答を計算し、
/// 1/3 oct バンド平均で 20 Hz–20 kHz 内に ±3 dB 超の谷が無いことを検証する。
///
/// 計画書は「ホワイトノイズを入れて 1/3 oct 平均」と記載するが、本システムは
/// 完全 LTI (フィードバック無し疎 FIR + dry 経路の和) であるため、白色雑音を
/// 走らせてスペクトルを推定するより、インパルス応答から直接 DTFT で厳密な
/// 伝達関数を求める方が数学的に等価かつ乱数分散が無く決定論的・再現性が高い。
///本テストではこの等価な決定論的手法を採用する (谷が出た場合は L_SEED/R_SEED
/// を変えて再測定し、満たすシードをソース定数に焼き込む — 受け入れ基準 4)。
fn mono_sum_max_deviation_db(l_seed: u64, r_seed: u64, width: f32) -> f64 {
    let fs = 48000.0f32;
    let length_sec = 0.030f32; // デフォルト length

    let seq_l = build_sequence(l_seed, fs, length_sec, DENSITY);
    let seq_r = build_sequence(r_seed, fs, length_sec, DENSITY);
    let norm_l = energy_norm(&seq_l);
    let norm_r = energy_norm(&seq_r);
    let ir_len = (length_sec * fs).ceil() as u32 + 64;
    let mut decorr_l = DecorrelatorState::new(ir_len);
    let mut decorr_r = DecorrelatorState::new(ir_len);

    // モノ入力インパルス (L=R=M) → outL/outR のインパルス応答を直接計算。
    // h_out[n] = dry (delta[n]) + width · norm · wet_ir[n]
    let n = ir_len as usize;
    let mut h_mono = vec![0.0f32; n];
    for i in 0..n {
        let m = if i == 0 { 1.0 } else { 0.0 };
        let wet_l = decorr_l.process(m, &seq_l) * norm_l;
        let wet_r = decorr_r.process(m, &seq_r) * norm_r;
        let dry = if i == 0 { 1.0 } else { 0.0 };
        let out_l = dry + width * wet_l;
        let out_r = dry + width * wet_r;
        h_mono[i] = 0.5 * (out_l + out_r);
    }

    // 1/3 oct バンド中心周波数 (20 Hz - 20 kHz、ISO 準拠近似)
    let band_centers: Vec<f64> = {
        let mut v = Vec::new();
        let mut f = 20.0f64;
        while f <= 20000.0 {
            v.push(f);
            f *= 2f64.powf(1.0 / 3.0);
        }
        v
    };

    let band_edge_ratio = 2f64.powf(1.0 / 6.0); // 1/3 oct の半バンド幅
    let points_per_band = 9;

    let mut amps_db: Vec<f64> = Vec::with_capacity(band_centers.len());
    for &center in &band_centers {
        let f_lo = center / band_edge_ratio;
        let f_hi = center * band_edge_ratio;
        // バンド内を対数等間隔で points_per_band 点サンプルし、
        // |H(f)|^2 (パワー) を平均してから dB 化する。
        let mut power_sum = 0.0f64;
        for p in 0..points_per_band {
            let t = p as f64 / (points_per_band - 1) as f64;
            let f = f_lo * (f_hi / f_lo).powf(t);
            let amp = goertzel_amp(&h_mono, f, fs as f64);
            power_sum += amp * amp;
        }
        let power_avg = (power_sum / points_per_band as f64).max(1e-24);
        amps_db.push(10.0 * power_avg.log10());
    }

    let mean_db = amps_db.iter().sum::<f64>() / amps_db.len() as f64;
    amps_db
        .iter()
        .map(|&db| (db - mean_db).abs())
        .fold(0.0f64, f64::max)
}

#[test]
fn test_mono_compat_flat() {
    let default_width = 0.35f32; // width パラメータのデフォルト値
    let max_deviation = mono_sum_max_deviation_db(L_SEED, R_SEED, default_width);

    eprintln!(
        "test_mono_compat_flat: L_SEED={L_SEED:#x} R_SEED={R_SEED:#x} width={default_width} max_deviation={max_deviation:.3} dB"
    );

    assert!(
        max_deviation <= 3.0,
        "1/3 oct 平均からの最大偏差 {max_deviation:.3} dB が ±3 dB を超過 (L_SEED={L_SEED:#x} R_SEED={R_SEED:#x})"
    );
}

#[test]
fn test_cpu_tap_bound() {
    let fs = 48000.0f32;
    // 全 length 端点 (10/20/30/40/50 ms) でタップ数上限を検証
    for &l in &[0.010f32, 0.020, 0.030, 0.040, 0.050] {
        let seq = build_sequence(L_SEED, fs, l, DENSITY);
        assert!(
            seq.taps_k.len() <= MAX_TAPS_PER_CH,
            "l={l} taps={} > MAX_TAPS_PER_CH={MAX_TAPS_PER_CH}",
            seq.taps_k.len()
        );
    }
}

#[test]
fn test_color_lp_cutoff_mapping() {
    // color=0 -> 20kHz, color=1 -> 2kHz (仕様通りのマッピング検証)
    let fc_0 = color_to_cutoff_hz(0.0);
    let fc_1 = color_to_cutoff_hz(1.0);
    assert!((fc_0 - 20000.0).abs() < 1e-3, "fc_0={fc_0}");
    assert!((fc_1 - 2000.0).abs() < 1.0, "fc_1={fc_1}");

    // alpha は 0 < alpha <= 1 の範囲に収まる (安定性)
    let fs = 48000.0f32;
    for &color in &[0.0f32, 0.25, 0.5, 0.75, 1.0] {
        let alpha = lp_alpha(color_to_cutoff_hz(color), fs);
        assert!(alpha > 0.0 && alpha <= 1.0, "color={color} alpha={alpha}");
    }
}

#[test]
fn test_one_pole_lp_step_response_stable() {
    let mut lp = OnePoleLp::new();
    let alpha = lp_alpha(2000.0, 48000.0);
    let mut y = 0.0f32;
    for _ in 0..48000 {
        y = lp.process(1.0, alpha);
    }
    assert!((y - 1.0).abs() < 1e-3, "収束先が 1.0 でない: y={y}");
}
