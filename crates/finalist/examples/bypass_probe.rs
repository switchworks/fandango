//! bypass トグル前後で天井保証が保たれるかの実測。
//!
//! 疑い: `bypass = true` のとき `out_gain = 1.0` になるが、遅延線から読み出される
//! `out_l` は D サンプル前に書かれた値であり、**バイパス直前の入力ゲイン適用済み**。
//! つまりバイパスへ入った直後の D サンプルは、ゲインが乗ったままリミッタを通らずに出る。
//!
//! 実行: `cargo run --release -p fandango-finalist --example bypass_probe`

use fandango_finalist::dsp::Finalist;

const SR: f32 = 48_000.0;

fn db(x: f32) -> f32 {
    20.0 * x.max(1.0e-12).log10()
}

fn main() {
    let cdb = -0.2_f32;
    let c = 10.0_f32.powf(cdb / 20.0);
    let n = 4800_usize;

    for &igd in &[0.0_f32, 6.0, 12.0, 24.0] {
        let ig = 10.0_f32.powf(igd / 20.0);
        let mut f = Finalist::new(SR);
        let rc = f.release_coeff(80.0);
        let d = f.latency_samples() as usize;

        // 1 kHz 正弦をフルスケールで流し、途中で bypass を ON にする。
        let toggle = 2400_usize;
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let s = (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / SR).sin();
            let byp = i >= toggle;
            out.push(f.process(s, s, ig, 0.0, c, rc, byp).0);
        }

        // 区間ごとのピーク。
        let seg = |a: usize, b: usize| -> f32 {
            out[a..b].iter().fold(0.0_f32, |m, v| m.max(v.abs()))
        };
        // bypass 前の定常 (十分に落ち着いた区間)
        let before = seg(1200, toggle);
        // bypass ON 直後の D サンプル
        let during = seg(toggle, toggle + d);
        // その後の定常 (素通し = 入力そのもの、ピーク 1.0 のはず)
        let after = seg(toggle + d + 100, n);

        println!(
            "input {:+5.0} dB | bypass前 {:+7.3} dB | 切替直後D={:4} {:+7.3} dB | bypass後 {:+7.3} dB | 天井超過 {:+7.3} dB",
            igd,
            db(before),
            d,
            db(during),
            db(after),
            db(during) - cdb
        );
    }
}
