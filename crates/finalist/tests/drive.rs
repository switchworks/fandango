//! 受け入れ基準 3（多項式恒等）・5（ADAA 妥当性）。

use fandango_finalist::dsp::{drive_shape, Finalist};

/// 基準 3: k = 0 で drive stage が bit-exact パススルー。
/// stage 単体は private なので、limiter を素通りさせる小振幅信号でフル経路を見る。
#[test]
fn drive_zero_is_bit_exact() {
    let sr = 48_000.0;
    let mut dsp = Finalist::new(sr);
    let d = dsp.latency_samples() as usize;
    let rc = dsp.release_coeff(80.0);
    let ceiling = 0.891_250_9;
    let input: Vec<f32> = (0..8192)
        .map(|n| 0.4 * ((n as f32 * 0.0271).sin() + 0.5 * (n as f32 * 0.313).sin()))
        .collect();
    let out: Vec<f32> = input
        .iter()
        .map(|&x| dsp.process(x, -x, 1.0, 0.0, ceiling, rc, false).0)
        .collect();
    for n in d..input.len() {
        assert_eq!(out[n], input[n - d], "n={n}: drive=0 が bit-exact でない");
    }
}

/// `drive_shape` 自体の恒等性（k = 0 で全域）。
#[test]
fn drive_shape_zero_is_identity() {
    for i in -2000..=2000 {
        let u = i as f32 / 1000.0;
        assert_eq!(drive_shape(u, 0.0), u, "u={u}");
    }
}

/// Goertzel で単一周波数の振幅を測る。
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

/// 基準 5: k = 1 で ADAA が naive `f(x)` 適用よりエイリアス成分を減らす。
///
/// テスト条件は計画書の「フルスケール 1 kHz」から変更した。理由:
/// - 振幅 1.0 の 1 kHz は `|u| ≤ 1` で 3 次多項式のみが効き、生成される高調波は
///   3 kHz だけ。折り返しが起きずエイリアス差が測れない
/// - 低次高調波しか出ない条件（例 3 kHz / 振幅 4.0）では naive のエイリアスが
///   -100 dB 台まで小さく、1 次 ADAA 自身の近似誤差床（実測 ≈ -75 dB）が上回る。
///   これは 1 次 ADAA の既知の性質で、実装バグではない
/// そこで折り返しが実際に問題になる領域 = 7 kHz・振幅 4.0（深いクリップ）で測る。
///
/// 8820 サンプル = 0.2 s。bin 間隔 5 Hz で 7 kHz と各折り返し周波数が整合するため
/// スペクトルリークが乗らない（非整合長だと基本波の裾がエイリアス測定を埋める）。
/// 折り返し先: 5 次 35 kHz → 9100 Hz、7 次 49 kHz → 4900 Hz、11 次 77 kHz → 11200 Hz。
/// いずれも 7 kHz の整数倍でないので折り返し由来と特定できる。
#[test]
fn adaa_reduces_aliasing_vs_naive() {
    let sr = 44_100.0f64;
    let f = 7000.0f64;
    let len = 8820usize;
    let amp = 4.0f32;

    let mut dsp = Finalist::new(sr as f32);
    let d = dsp.latency_samples() as usize;
    let rc = dsp.release_coeff(500.0);
    let input: Vec<f32> = (0..len + d)
        .map(|n| amp * ((2.0 * std::f64::consts::PI * f * n as f64 / sr).sin() as f32))
        .collect();

    let naive: Vec<f32> = input[..len].iter().map(|&u| drive_shape(u, 1.0)).collect();

    // ADAA 段は private なので Finalist 経由で取り出す。ceiling を十分高くして
    // limiter ゲインを 1 に固定し、drive stage の差だけを見る。
    let adaa_full: Vec<f32> = input
        .iter()
        .map(|&u| dsp.process(u, u, 1.0, 1.0, 8.0, rc, false).0)
        .collect();
    let adaa = &adaa_full[d..d + len];

    for alias_f in [9100.0f64, 4900.0, 11200.0] {
        let a_naive = goertzel_amp(&naive, alias_f, sr);
        let a_adaa = goertzel_amp(adaa, alias_f, sr);
        assert!(
            a_adaa < a_naive,
            "{alias_f} Hz: ADAA={a_adaa:e} が naive={a_naive:e} を下回っていない"
        );
    }

    // 基本波は保たれていること。1 次 ADAA は実効 0.5 サンプル遅れを伴い
    // cos(ω/2) = cos(π·7000/44100) ≈ 0.878 の減衰が理論値なので、その分は許容する。
    let fund_naive = goertzel_amp(&naive, f, sr);
    let fund_adaa = goertzel_amp(adaa, f, sr);
    assert!(
        fund_adaa > 0.85 * fund_naive,
        "基本波が過剰に失われた: adaa={fund_adaa} naive={fund_naive}"
    );
}
