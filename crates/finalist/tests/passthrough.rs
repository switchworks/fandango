//! 受け入れ基準 1・2・4・6。基準 3・5（drive stage）は tests/drive.rs。

use fandango_finalist::dsp::Finalist;

const SAMPLE_RATES: [f32; 3] = [44_100.0, 48_000.0, 96_000.0];
/// 既定 ceiling -1.0 dB の線形値。
const CEILING: f32 = 0.891_250_9;
const RELEASE_MS: f32 = 80.0;

fn run(sr: f32, input: &[(f32, f32)], input_gain: f32, drive: f32, ceiling: f32) -> Vec<(f32, f32)> {
    let mut dsp = Finalist::new(sr);
    let rc = dsp.release_coeff(RELEASE_MS);
    input
        .iter()
        .map(|&(l, r)| dsp.process(l, r, input_gain, drive, ceiling, rc, false))
        .collect()
}

/// ceiling 未満の穏やかな信号。
fn quiet(len: usize) -> Vec<(f32, f32)> {
    (0..len)
        .map(|n| {
            let t = n as f32;
            (
                0.3 * (t * 0.031).sin(),
                0.25 * (t * 0.017).sin() + 0.1 * (t * 0.11).sin(),
            )
        })
        .collect()
}

// --- 受け入れ基準 1: 天井不変条件 -------------------------------------------

fn sine_burst(len: usize, sr: f32) -> Vec<(f32, f32)> {
    let w = 2.0 * std::f32::consts::PI * 997.0 / sr;
    (0..len)
        .map(|n| {
            // 1024 サンプルごとに無音とフルスケールを切り替える（アタック/リリース両方を叩く）
            let on = (n / 1024) % 2 == 0;
            let v = if on { (n as f32 * w).sin() } else { 0.0 };
            (v, -v)
        })
        .collect()
}

fn impulses(len: usize) -> Vec<(f32, f32)> {
    (0..len)
        .map(|n| {
            if n % 373 == 0 {
                (1.0, 1.0)
            } else {
                (0.0, 0.0)
            }
        })
        .collect()
}

fn white_noise(len: usize) -> Vec<(f32, f32)> {
    // 決定論的な LCG（テスト内で完結、DSP は rng 不使用）
    let mut s: u32 = 0x1234_5678;
    (0..len)
        .map(|_| {
            let mut next = || {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (s >> 8) as f32 / 8_388_608.0 - 1.0
            };
            (next(), next())
        })
        .collect()
}

fn square(len: usize, sr: f32) -> Vec<(f32, f32)> {
    let period = (sr / 110.0) as usize;
    (0..len)
        .map(|n| {
            let v = if (n / (period / 2)) % 2 == 0 { 1.0 } else { -1.0 };
            (v, v)
        })
        .collect()
}

#[test]
fn ceiling_is_never_exceeded() {
    for sr in SAMPLE_RATES {
        let len = 40_000;
        let cases: [(&str, Vec<(f32, f32)>); 4] = [
            ("sine burst", sine_burst(len, sr)),
            ("impulses", impulses(len)),
            ("white noise", white_noise(len)),
            ("square", square(len, sr)),
        ];
        for ceiling_db in [-1.0f32, -12.0, 0.0] {
            let ceiling = 10f32.powf(ceiling_db / 20.0);
            for (name, input) in &cases {
                // input gain +12 dB の過大入力も含める
                for (input_gain, drive) in
                    [(1.0f32, 0.0f32), (3.981_071_7, 0.0), (1.0, 1.0), (3.981_071_7, 1.0)]
                {
                    let out = run(sr, input, input_gain, drive, ceiling);
                    let peak = out
                        .iter()
                        .map(|&(l, r)| l.abs().max(r.abs()))
                        .fold(0.0f32, f32::max);
                    assert!(
                        peak <= ceiling + 1e-6,
                        "sr={sr} case={name} ceiling_db={ceiling_db} in_gain={input_gain} drive={drive}: \
                         peak={peak} > ceiling={ceiling}"
                    );
                }
            }
        }
    }
}

// --- 受け入れ基準 2: 恒等性 --------------------------------------------------

#[test]
fn identity_at_defaults_is_bit_exact() {
    let input = quiet(8192);
    for sr in SAMPLE_RATES {
        let d = Finalist::new(sr).latency_samples() as usize;
        let out = run(sr, &input, 1.0, 0.0, CEILING);
        for n in d..input.len() {
            assert_eq!(
                out[n],
                input[n - d],
                "sr={sr} n={n}: 既定値でのパススルーが bit-exact でない"
            );
        }
    }
}

// --- 受け入れ基準 4: 決定論 --------------------------------------------------

#[test]
fn deterministic_across_runs() {
    let input = white_noise(8192);
    for sr in SAMPLE_RATES {
        assert_eq!(
            run(sr, &input, 1.0, 0.0, CEILING),
            run(sr, &input, 1.0, 0.0, CEILING),
            "sr={sr}: 決定論違反"
        );
    }
}

// --- 受け入れ基準 6: レイテンシ ----------------------------------------------

#[test]
fn reported_latency_matches_measured_delay() {
    for sr in SAMPLE_RATES {
        let d = Finalist::new(sr).latency_samples() as usize;
        let mut input = vec![(0.0f32, 0.0f32); 4 * d + 64];
        // ceiling 未満のインパルスなのでゲインは 1 のまま
        input[d] = (0.5, 0.5);
        let out = run(sr, &input, 1.0, 0.0, CEILING);
        let measured = out
            .iter()
            .position(|&(l, _)| l != 0.0)
            .expect("インパルスが出力に現れない");
        assert_eq!(measured, 2 * d, "sr={sr}: 報告レイテンシと実測遅延が不一致");
    }
}
