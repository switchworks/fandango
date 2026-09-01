//! `level_ui_to_amp` の v=0 近傍を実測する。
//!
//! 膝導入前は `v -> 0+` で -24 dB、`v = 0` で無音という段差があり、
//! 出力に -30.4 dBFS (フルスケール素材で -24.4 dBFS) のクリックが出ていた。
//! 本例はその段差が消えていることの再現用。
//!
//! UI 値 `v` を 1 サンプルずつ線形に 0 へ動かしたとき、wet ゲインに
//! 不連続が出るか、出るならその大きさはいくらか。
//!
//! 実行: `cargo run -p fandango-trip-delay --example level_step --release`

use fandango_trip_delay::dsp::{level_ui_to_amp, TripDelayDsp};

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-12).log10()
}

fn main() {
    // --- 1. 写像そのものの段差 ---
    println!("== level_ui_to_amp の v -> 0 近傍 (膝導入後) ==");
    for &v in &[1e-2_f32, 1e-3, 1e-4, 1e-5, 1e-6, 1e-7, 0.0] {
        let a = level_ui_to_amp(v);
        println!("  v = {v:<10} amp = {a:.9}  ({:.3} dB)", db(a));
    }
    let limit = level_ui_to_amp(1e-9);
    println!("  v -> 0+ の極限 amp = {limit:.9} ({:.3} dB)", db(limit));
    println!("  v = 0     の値   amp = {:.9}", level_ui_to_amp(0.0));
    println!("  => 写像の段差 = {:.9} ({:.3} dB 相当)", limit, db(limit));

    // --- 2. 実信号での出力段差 ---
    // 遅延線の状態は level に依存しない（帰還路 `in_s + feedback*fbk` に level が入らない）。
    // よって level が eps -> 0 へ落ちた瞬間の出力段差は厳密に `amp_limit * |tap|`。
    // tap は level=1.0 で回して dry を引けば取り出せる。
    let sr = 48_000.0_f32;
    let mut dsp = TripDelayDsp::new();
    dsp.prepare(sr);
    dsp.set_tempo(120.0);

    let feedback = 0.30_f32;
    let sig = |n: usize| 0.5 * (2.0 * std::f32::consts::PI * 220.0 * n as f32 / sr).sin();

    let mut max_tap = 0.0_f32;
    for n in 0..(sr as usize * 4) {
        let x = sig(n);
        let (out_l, _) = dsp.process_sample(x, x, 1.0, feedback);
        let tap = out_l - x;
        if n > sr as usize {
            max_tap = max_tap.max(tap.abs());
        }
    }

    let step = limit * max_tap;
    println!();
    println!("== 220 Hz 定常正弦 (振幅 0.5), 120 BPM, fb=0.30 ==");
    println!("  |tap| の最大                 : {max_tap:.9}");
    println!("  v: eps -> 0 の瞬間の出力段差  : {step:.9} ({:.2} dBFS)", db(step));
    println!("  参考: フルスケール素材なら     : {:.9} ({:.2} dBFS)",
             limit * (max_tap / 0.5), db(limit * (max_tap / 0.5)));
}
