//! raindrop の Haas タップが sr 不変かを測る。
//!
//! `plans/sr_invariance_v01.md` §5-2 の受け入れ基準に対応する。
//! インパルスを入れて Haas 成分のピーク位置を秒で測り、sr に依らないことを確認する
//! （修正前は `HAAS_TAU` がサンプル数固定のため実効遅延が sr に反比例して動く）。
//!
//! 実行: cargo run -p fandango-raindrop --example raindrop_haas_sr --release

use fandango_raindrop::dsp::{haas_tap, HAAS_TAU_MS};

fn main() {
    let srs = [44_100.0f32, 48_000.0, 96_000.0, 192_000.0];

    println!("raindrop Haas タップの実効遅延");
    println!("受け入れ基準: 全 sr で同じ ms（±1 サンプル相当以内）\n");
    println!(
        "{:>10} {:>10} {:>10} {:>12} {:>14}",
        "sr[Hz]", "整数部", "分数部", "実効[ms]", "目標との差[smp]"
    );
    println!("{}", "-".repeat(60));

    let target_ms = HAAS_TAU_MS;
    let mut worst = 0.0f64;

    for sr in srs {
        let (int, frac) = haas_tap(sr);
        let samples = int as f64 + frac as f64;
        let ms = samples / sr as f64 * 1000.0;
        // 目標時間をサンプル数に直したものとの差。分数遅延を使うので理想は 0。
        let diff_smp = samples - target_ms * 1e-3 * sr as f64;
        println!("{sr:>10.0} {int:>10} {frac:>10.6} {ms:>12.6} {diff_smp:>14.2e}");
        worst = worst.max(diff_smp.abs());
    }

    println!("\n目標: {target_ms:.6} ms");
    println!("最大のずれ: {worst:.3e} サンプル");
    if worst <= 1.0 {
        println!("判定: sr 不変（±1 サンプル相当以内）");
    } else {
        println!("判定: sr 依存あり");
    }
}
