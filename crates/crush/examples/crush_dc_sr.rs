//! crush の DC ブロッカーが sr 不変かを測る。
//!
//! 受け入れ基準: 1 / 5 / 20 Hz の正弦を入れたときの減衰量が sr に依らないことを確認する
//! （修正前は `DC_BLOCK_R` 固定のためカットオフが fs に比例し、値が動く）。
//!
//! 実行: cargo run -p fandango-crush --example crush_dc_sr --release

use fandango_crush::dsp::DcBlocker;

/// 定常状態での振幅比 [dB]。過渡を捨ててから 1 周期ぶんの最大振幅で測る。
fn attenuation_db(f_hz: f64, sr: f64) -> f64 {
    let mut b = DcBlocker::new();
    // ホストの initialize 相当。これを呼ばないと基準 sr の係数のまま測ってしまう。
    b.set_sample_rate(sr as f32);
    // 1 極 HP の時定数は fc が最小 (数 Hz) でも 1 秒あれば十分に落ち着く。
    let settle = (sr * 3.0) as usize;
    for n in 0..settle {
        let x = (2.0 * std::f64::consts::PI * f_hz * n as f64 / sr).sin() as f32;
        b.process(x);
    }

    // 収録は 2 周期ぶん。最大振幅の比を取る。
    let span = ((sr / f_hz) * 2.0) as usize;
    let mut peak_in = 0.0f64;
    let mut peak_out = 0.0f64;
    for n in 0..span {
        let t = (settle + n) as f64;
        let x = (2.0 * std::f64::consts::PI * f_hz * t / sr).sin();
        let y = b.process(x as f32) as f64;
        peak_in = peak_in.max(x.abs());
        peak_out = peak_out.max(y.abs());
    }
    20.0 * (peak_out / peak_in).log10()
}

fn main() {
    let srs = [44_100.0f64, 48_000.0, 96_000.0, 192_000.0];
    let freqs = [1.0f64, 5.0, 20.0];

    println!("crush DC ブロッカーの減衰量 [dB]（正弦入力、定常状態）");
    println!("受け入れ基準: 各周波数で sr 間の差が ±0.5 dB 以内\n");

    print!("{:>8}", "f[Hz]");
    for sr in srs {
        print!("{:>12.0}", sr);
    }
    println!("{:>10}", "幅[dB]");
    println!("{}", "-".repeat(8 + 12 * srs.len() + 10));

    let mut worst = 0.0f64;
    for f in freqs {
        print!("{f:>8.0}");
        let vals: Vec<f64> = srs.iter().map(|&sr| attenuation_db(f, sr)).collect();
        for v in &vals {
            print!("{v:>12.2}");
        }
        let spread = vals.iter().cloned().fold(f64::MIN, f64::max)
            - vals.iter().cloned().fold(f64::MAX, f64::min);
        println!("{spread:>10.2}");
        worst = worst.max(spread);
    }

    println!("\n最大の幅: {worst:.2} dB");
    if worst <= 0.5 {
        println!("判定: sr 不変 (±0.5 dB 以内)");
    } else {
        println!("判定: sr 依存あり");
    }
}
