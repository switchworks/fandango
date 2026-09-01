//! bypass の切替をまたいでも天井保証が成立すること。
//!
//! 回帰対象 (2026-08-27): `bypass = true` で出力ゲインだけ 1.0 にし、遅延線には
//! 入力ゲイン適用済みの値が残っていたため、bypass へ切り替えた直後の D サンプルが
//! リミッタを通らずに出ていた（`input = +24 dB` で出力 +24 dBFS を実測）。

use fandango_finalist::dsp::Finalist;

const SR: f32 = 48_000.0;
/// 既定 ceiling -1.0 dB の線形値。
const CEILING: f32 = 0.891_250_9;
const RELEASE_MS: f32 = 80.0;

fn signal(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / SR).sin())
        .collect()
}

/// `bypass` を何度切り替えても、非 bypass 区間の出力が ceiling を超えないこと。
#[test]
fn ceiling_holds_across_bypass_toggles() {
    let n = 12_000;
    let x = signal(n);
    for &input_db in &[0.0_f32, 6.0, 12.0, 24.0] {
        let ig = 10.0_f32.powf(input_db / 20.0);
        let mut dsp = Finalist::new(SR);
        let rc = dsp.release_coeff(RELEASE_MS);
        let d = dsp.latency_samples() as usize;
        for (i, &s) in x.iter().enumerate() {
            // 1200 サンプルごとに ON/OFF を往復させる。
            let byp = (i / 1200) % 2 == 1;
            let (l, r) = dsp.process(s, s, ig, 0.0, CEILING, rc, byp);
            if byp {
                // bypass 中は D サンプル前の dry がそのまま出る。
                let want = if i >= d { x[i - d] } else { 0.0 };
                assert_eq!(l, want, "bypass 中の出力が dry と一致しない: i={i}");
                assert_eq!(r, want);
            } else {
                let m = l.abs().max(r.abs());
                assert!(
                    m <= CEILING * (1.0 + 1e-5),
                    "天井超過: input={input_db} dB i={i} out={m} ceiling={CEILING}"
                );
            }
        }
    }
}
