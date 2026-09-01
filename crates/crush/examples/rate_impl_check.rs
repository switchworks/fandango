//! 実装済み `CrushChannel`（経路 F）の検定。
//! 未検証項目を実測する:
//! (1) 非整数 rate で副生成物が下がるか（ハーネスの経路 F 実測と整合するか）
//! (2) 床の持ち上がりが全動作点で -80 dB 以下か
//! (3) rate 時変（高速掃引）で「1 区間に境界は高々 1 つ」が破れないか
//! (4) NaN / denormal 掃引

use fandango_crush::dsp::CrushChannel;

const SR: f64 = 48_000.0;
const N: usize = 48_000;
const SKIP: usize = 4_800;
const F_IN: f64 = 220.0;
const AMP: f64 = 0.6;

fn x_at(t: f64) -> f32 {
    (AMP * (2.0 * std::f64::consts::PI * F_IN * t / SR).sin()) as f32
}

fn render(rate: f32, bits: f32) -> Vec<f64> {
    let mut ch = CrushChannel::new();
    ch.set_sample_rate(SR as f32);
    (0..N).map(|i| ch.process(x_at(i as f64), bits, rate, 1.0) as f64).collect()
}

/// 現行実装（経路 A）の参照。変更前の `CrushChannel::process` の S&H 部分を再現する。
/// 量子化・DC ブロッカー以降は同一なので、S&H の違いだけが出る。
/// **床の比較は A と F の差で行う**: 絶対値の床は量子化歪みそのものであり、
/// `rate = 1`（F が何もしない点）でも出る。F 由来の持ち上がりはこの差にしか現れない。
fn render_a(rate: u32, bits: f32) -> Vec<f64> {
    let mut ch = CrushChannel::new();
    ch.set_sample_rate(SR as f32);
    let mut hold = 0.0_f32;
    let mut counter = 0u32;
    let rate = rate.max(1);
    (0..N)
        .map(|i| {
            let x = x_at(i as f64);
            if counter == 0 {
                hold = x;
            }
            counter = (counter + 1) % rate;
            // S&H 済みの値を rate=1 で通せば以降の段（ADAA→gate→DC→mix）は現行と同じ。
            ch.process(hold, bits, 1.0, 1.0) as f64
        })
        .collect()
}

fn goertzel(y: &[f64], f_hz: f64) -> f64 {
    let w = 2.0 * std::f64::consts::PI * f_hz / SR;
    let c = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0_f64, 0.0_f64);
    for &v in y {
        let s0 = v + c * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    ((s1 * s1 + s2 * s2 - c * s1 * s2).max(0.0)).sqrt() / y.len() as f64 * 2.0
}
fn db(x: f64) -> f64 {
    20.0 * x.max(1e-15).log10()
}

/// `fs/(2r) ± f_in` の大きい方 [絶対 dB]。v02 §3 と同じ測定点。
fn image_db(y: &[f64], rate: f64) -> f64 {
    let c = SR / (2.0 * rate);
    let a = goertzel(y, (c - F_IN).abs());
    let b = goertzel(y, c + F_IN);
    db(a.max(b))
}

/// 2〜22 kHz の局所ピーク数と最大値。入力の既知成分近傍は除外。
fn floor_scan(y: &[f64]) -> (usize, f64) {
    let mut n = 0usize;
    let mut mx = -240.0_f64;
    let mut f = 2000.0_f64;
    while f < 22_000.0 {
        if (f - F_IN).abs() > 50.0 {
            let d = db(goertzel(y, f));
            if d > -80.0 {
                n += 1;
                if d > mx {
                    mx = d;
                }
            }
        }
        f += 25.0;
    }
    (n, mx)
}

fn main() {
    let y = &render(1.0, 8.0)[SKIP..];
    println!("実装済み CrushChannel（経路 F）の検定  sr={SR} n={N} f_in={F_IN} amp={AMP} mix=1");
    println!();

    println!("[1] 非整数 rate の副生成物（bits=8）。v02 §3 の経路 F 実測と突き合わせる");
    println!();
    println!("  rate       実装値[dB]   v02 の F:Linear[dB]");
    for (r, want) in [
        (2.5_f32, -58.7_f64),
        (3.5, -60.9),
        (4.5, -62.8),
        (8.5, -68.2),
        (16.5, -73.5),
        (2.25, -70.7),
        (7.125, -85.6),
    ] {
        let y = render(r, 8.0);
        let d = image_db(&y[SKIP..], r as f64);
        println!("  {r:<10.5} {d:>10.1}   {want:>10.1}");
    }
    println!();

    println!("[2] 床の持ち上がり。**整数 rate で A と F を比較する**");
    println!("  絶対値の床は量子化歪みそのもので、rate=1 でも 147 本出る（F 由来ではない）。");
    println!("  F 固有の持ち上がりは A との差にしか現れないので、整数 rate で突き合わせる。");
    println!();
    println!("  bits  rate    A: 数/最大[dB]      F: 数/最大[dB]      差[dB]");
    for bits in [2.0_f32, 8.0, 16.0] {
        for r in [1u32, 2, 4, 8, 16, 32] {
            let ya = render_a(r, bits);
            let yf = render(r as f32, bits);
            let (na, ma) = floor_scan(&ya[SKIP..]);
            let (nf, mf) = floor_scan(&yf[SKIP..]);
            println!("  {bits:<5.0} {r:<7} {na:>4} / {ma:>8.1}      {nf:>4} / {mf:>8.1}      {:>6.1}", mf - ma);
        }
    }
    println!();
    println!("  非整数 rate は A に対応点が無いので、両隣の整数 rate の A と挟んで見る。");
    println!();
    println!("  bits  rate       F: 数/最大[dB]   下隣 A の最大   上隣 A の最大");
    for bits in [2.0_f32, 8.0, 16.0] {
        for r in [2.6_f32, 3.14159, 8.5, 16.5, 31.7] {
            let yf = render(r, bits);
            let (nf, mf) = floor_scan(&yf[SKIP..]);
            let lo = render_a(r.floor() as u32, bits);
            let hi = render_a(r.ceil() as u32, bits);
            let (_, ml) = floor_scan(&lo[SKIP..]);
            let (_, mh) = floor_scan(&hi[SKIP..]);
            println!("  {bits:<5.0} {r:<10.5} {nf:>4} / {mf:>8.1}   {ml:>13.1}   {mh:>13.1}");
        }
    }
    println!();

    println!("[3] rate 時変。1 サンプルごとに rate を掃引しても出力が有限か");
    println!("  「1 区間に境界は高々 1 つ」が破れると段が飛ぶ。edge_dist < 0 になれば破綻。");
    println!();
    for (name, lo, hi) in [("1→32 を 1 秒で", 1.0_f32, 32.0_f32), ("32→1 を 1 秒で", 32.0, 1.0), ("1→32 を 100 サンプルで", 1.0, 32.0)] {
        let span = if name.contains("100") { 100 } else { N };
        let mut ch = CrushChannel::new();
        ch.set_sample_rate(SR as f32);
        let mut worst = 0.0_f64;
        let mut nonfinite = 0usize;
        for i in 0..N {
            let t = (i % span) as f32 / span as f32;
            let r = lo + (hi - lo) * t;
            let y = ch.process(x_at(i as f64), 8.0, r, 1.0);
            if !y.is_finite() {
                nonfinite += 1;
            }
            let a = y.abs() as f64;
            if a > worst {
                worst = a;
            }
        }
        println!("  {name:<24} 非有限={nonfinite}  最大絶対値={worst:.4}");
    }
    println!();

    println!("[4] 敵対入力（NaN / inf / denormal / 巨大値）× 非整数 rate");
    let mut bad = 0usize;
    for r in [1.0_f32, 2.5, 3.14159, 31.7] {
        let mut ch = CrushChannel::new();
        ch.set_sample_rate(SR as f32);
        for i in 0..4000 {
            let x = match i % 7 {
                0 => f32::NAN,
                1 => f32::INFINITY,
                2 => -f32::INFINITY,
                3 => 1e-40,
                4 => -1e-40,
                5 => 1e9,
                _ => 0.3,
            };
            let y = ch.process(x, 8.0, r, 1.0);
            if !y.is_finite() || y.abs() < 1e-30 && y != 0.0 {
                bad += 1;
            }
        }
    }
    println!("  非有限 or denormal の出力: {bad} 個");
    println!();
    let _ = y;
}
