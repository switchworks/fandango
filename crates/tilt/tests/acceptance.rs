//! tilt v0.1 受け入れテスト (受け入れ基準 1-6 対応)。

use fandango_tilt::dsp::{biquad_amp, high_shelf_coeffs, low_shelf_coeffs, TiltChannel};

const FS: f32 = 48000.0;

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

fn sine(freq: f64, fs: f32, n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| (2.0 * std::f64::consts::PI * freq * i as f64 / fs as f64).sin() as f32)
        .collect()
}

#[test]
fn test_passthrough_zero_gain() {
    let ls = low_shelf_coeffs(FS, 700.0, 0.0, 0.7);
    let hs = high_shelf_coeffs(FS, 700.0, 0.0, 0.7);
    let mut ch = TiltChannel::new();
    let n = FS as usize;
    let input = sine(997.0, FS, n);
    let output: Vec<f32> = input.iter().map(|&x| ch.process(x, &ls, &hs)).collect();
    let amp_in = goertzel_amp(&input, 997.0, FS as f64);
    let amp_out = goertzel_amp(&output, 997.0, FS as f64);
    let err = (amp_out - amp_in).abs();
    assert!(err < 1e-4, "err={err}");
}

#[test]
fn test_pivot_unity_gain() {
    for &g in &[-12.0f32, -6.0, 6.0, 12.0] {
        let ls = low_shelf_coeffs(FS, 700.0, -g, 0.7);
        let hs = high_shelf_coeffs(FS, 700.0, g, 0.7);
        let amp_ls = biquad_amp(&ls, 700.0, FS);
        let amp_hs = biquad_amp(&hs, 700.0, FS);
        let total_db = 20.0 * (amp_ls * amp_hs).log10();
        assert!(total_db.abs() < 0.5, "gain={g} pivot dev {total_db:.2} dB > 0.5");
    }
}

#[test]
fn test_tilt_symmetry() {
    let ls_pos = low_shelf_coeffs(FS, 700.0, -6.0, 0.7);
    let hs_pos = high_shelf_coeffs(FS, 700.0, 6.0, 0.7);
    let amp_a = 20.0
        * (biquad_amp(&ls_pos, 100.0, FS) as f64 * biquad_amp(&hs_pos, 100.0, FS) as f64).log10();

    let ls_neg = low_shelf_coeffs(FS, 700.0, 6.0, 0.7);
    let hs_neg = high_shelf_coeffs(FS, 700.0, -6.0, 0.7);
    let amp_b = 20.0
        * (biquad_amp(&ls_neg, 4900.0, FS) as f64 * biquad_amp(&hs_neg, 4900.0, FS) as f64)
            .log10();

    let diff = (amp_a - amp_b).abs();
    assert!(diff < 0.5, "A={amp_a:.2} B={amp_b:.2} diff={diff:.2}");
}

#[test]
fn test_stability_all_endpoints() {
    let n = FS as usize;
    for &gain in &[-12.0f32, 12.0] {
        for &pivot in &[100.0f32, 5000.0] {
            for &slope in &[0.3f32, 1.0] {
                let ls = low_shelf_coeffs(FS, pivot, -gain, slope);
                let hs = high_shelf_coeffs(FS, pivot, gain, slope);
                let mut ch = TiltChannel::new();
                let mut last = 0.0f32;
                for i in 0..n {
                    let x = if i == 0 { 1.0 } else { 0.0 };
                    last = ch.process(x, &ls, &hs);
                }
                assert!(
                    last.abs() < 1e-6,
                    "gain={gain} pivot={pivot} slope={slope} last={last}"
                );
            }
        }
    }
}

#[test]
fn test_hostile_input_stays_finite() {
    let ls = low_shelf_coeffs(FS, 700.0, 12.0, 1.0);
    let hs = high_shelf_coeffs(FS, 700.0, 12.0, 1.0);
    let mut ch = TiltChannel::new();
    for &x in &[1e9f32, -1e9, f32::INFINITY, f32::NAN, 100.0, -100.0, 0.0] {
        let y = ch.process(x, &ls, &hs);
        assert!(y.is_finite(), "x={x} y={y}");
    }
}
