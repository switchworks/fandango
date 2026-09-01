//! `drive` がラウドネス上げとして働いていないかの実測。**本実装ではない。**
//!
//! 整形関数 s(u) = (3u - u³)/2 は小信号で傾き 1.5 (= +3.52 dB) のため、
//! k を上げると低レベル成分が持ち上がる懸念がある。k を振って
//! 入出力 RMS / LUFS-I / THD を測る。
//!
//! 実行: `cargo run --release -p fandango-finalist --example drive_gain`

use fandango_finalist::dsp::{drive_shape, Finalist};

const SR: f32 = 48_000.0;

fn db(x: f32) -> f32 {
    20.0 * x.max(1.0e-12).log10()
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|&v| (v as f64) * (v as f64)).sum::<f64>() / x.len() as f64).sqrt() as f32
}

/// ITU-R BS.1770-4 の K 特性（48 kHz 係数）。
fn k_weight(x: &[f32]) -> Vec<f32> {
    let stages: [([f64; 3], [f64; 3]); 2] = [
        (
            [1.535_124_859_586_97, -2.691_696_189_406_38, 1.198_392_810_852_85],
            [1.0, -1.690_659_293_182_41, 0.732_480_774_215_85],
        ),
        ([1.0, -2.0, 1.0], [1.0, -1.990_047_454_833_98, 0.990_072_250_366_21]),
    ];
    let mut y: Vec<f64> = x.iter().map(|&v| v as f64).collect();
    for (b, a) in stages {
        let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
        for v in y.iter_mut() {
            let xn = *v;
            let yn = b[0] * xn + b[1] * x1 + b[2] * x2 - a[1] * y1 - a[2] * y2;
            x2 = x1;
            x1 = xn;
            y2 = y1;
            y1 = yn;
            *v = yn;
        }
    }
    y.iter().map(|&v| v as f32).collect()
}

/// LUFS-I（ステレオ同一信号 = モノを両ch に置いた前提。G_L = G_R = 1.0）。
fn lufs_i(x: &[f32]) -> f32 {
    let z = k_weight(x);
    let block = (0.400 * SR) as usize;
    let hop = block / 4;
    let mut ms: Vec<f64> = Vec::new();
    let mut i = 0;
    while i + block <= z.len() {
        let s: f64 = z[i..i + block].iter().map(|&v| (v as f64) * (v as f64)).sum();
        // 2 ch 分（同一信号）なので平均二乗を 2 倍する。
        ms.push(2.0 * s / block as f64);
        i += hop;
    }
    let l = |m: f64| -0.691 + 10.0 * m.max(1.0e-20).log10();
    let abs: Vec<f64> = ms.iter().copied().filter(|&m| l(m) > -70.0).collect();
    if abs.is_empty() {
        return f32::NEG_INFINITY;
    }
    let mean = abs.iter().sum::<f64>() / abs.len() as f64;
    let thr = l(mean) - 10.0;
    let gated: Vec<f64> = abs.into_iter().filter(|&m| l(m) > thr).collect();
    let g = gated.iter().sum::<f64>() / gated.len() as f64;
    l(g) as f32
}

fn thd_1k(y: &[f32]) -> f32 {
    let period = 48usize;
    let cycles = y.len() / period;
    let len = cycles * period;
    let y = &y[..len];
    let mag = |k: usize| -> f64 {
        let (mut re, mut im) = (0.0f64, 0.0f64);
        let w = 2.0 * std::f64::consts::PI * (k * cycles) as f64 / len as f64;
        for (i, &v) in y.iter().enumerate() {
            let p = w * i as f64;
            re += v as f64 * p.cos();
            im -= v as f64 * p.sin();
        }
        (re * re + im * im).sqrt() / len as f64
    };
    let f1 = mag(1);
    let h: f64 = (2..=20).map(|k| mag(k).powi(2)).sum();
    (h.sqrt() / f1.max(1.0e-12)) as f32
}

fn sine1k(len: usize) -> Vec<f32> {
    (0..len)
        .map(|i| 0.5 * (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / SR).sin())
        .collect()
}

fn noise(amp: f32, len: usize) -> Vec<f32> {
    let mut s = 0x1234_5678u32;
    (0..len)
        .map(|_| {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            amp * ((s >> 8) as f32 / 8_388_608.0 - 1.0)
        })
        .collect()
}

/// 低レベル成分（-40 dBFS 帯）を含む合成素材。drive の低レベル持ち上げを見るため。
fn mixed(len: usize) -> Vec<f32> {
    (0..len)
        .map(|i| {
            let t = i as f32 / SR;
            let env = 0.5 + 0.5 * (2.0 * std::f32::consts::PI * 1.7 * t).sin();
            0.45 * env * (2.0 * std::f32::consts::PI * 220.0 * t).sin()
                + 0.02 * (2.0 * std::f32::consts::PI * 1700.0 * t).sin()
        })
        .collect()
}

fn run(x: &[f32], ig: f32, k: f32, ceiling: f32) -> Vec<f32> {
    let mut f = Finalist::new(SR);
    let rc = (-1.0f32 / (0.080 * SR)).exp();
    x.iter().map(|&s| f.process(s, s, ig, k, ceiling, rc, false).0).collect()
}

fn main() {
    let ceiling = 10f32.powf(-1.0 / 20.0);
    let ks = [0.0f32, 0.25, 0.5, 0.75, 1.0];

    println!("[整形関数単体] s(u) の静的ゲイン（リミッタなし・ADAA なし）");
    println!("  入力レベル      k=0     k=0.25    k=0.5    k=0.75     k=1.0   (dB)");
    for lvl_db in [-60.0f32, -40.0, -20.0, -12.0, -6.0, -3.0, 0.0] {
        let u = 10f32.powf(lvl_db / 20.0);
        let mut cols = String::new();
        for &k in &ks {
            cols.push_str(&format!("  {:+8.3}", db(drive_shape(u, k) / u)));
        }
        println!("  {:+6.0} dBFS {}", lvl_db, cols);
    }
    println!();

    for (name, x, tonal) in [
        ("1 kHz 正弦 amp 0.5", sine1k(240_000), true),
        ("mixed(220Hz AM + 1.7kHz)", mixed(240_000), false),
        ("white noise -12 dBFS", noise(0.25, 240_000), false),
    ] {
        for ig_db in [0.0f32, 12.0] {
            let ig = 10f32.powf(ig_db / 20.0);
            println!("[{}] input {:+.0} dB, ceiling -1.0 dB, release 80 ms", name, ig_db);
            println!(
                "  in RMS {:+.3} dBFS / in LUFS-I {:+.3}",
                db(rms(&x)),
                lufs_i(&x)
            );
            println!("      k     out RMS(dB)   ΔRMS(dB)   out LUFS-I   ΔLUFS   THD(%)   補償後ΔRMS");
            let base_r = db(rms(&run(&x, ig, 0.0, ceiling)));
            let base_l = lufs_i(&run(&x, ig, 0.0, ceiling));
            for &k in &ks {
                let y = run(&x, ig, k, ceiling);
                let r = db(rms(&y));
                let lu = lufs_i(&y);
                let t = if tonal {
                    format!("{:6.3}", 100.0 * thd_1k(&y[10_000..]))
                } else {
                    "     -".to_string()
                };
                // 補償仮説: 整形段の出力を 1/(1+k/2) 倍する（小信号傾きを 1 に戻す）。
                // k = 0 では係数 1 なので厳密恒等は保たれる。
                let comp = r - db(1.0 + k * 0.5);
                println!(
                    "  {:5.2}   {:+10.3}   {:+8.3}   {:+10.3}   {:+6.3}   {}   {:+8.3}",
                    k, r, r - base_r, lu, lu - base_l, t, comp - base_r
                );
            }
            println!();
        }
    }

    // k = 0 の厳密恒等（ceiling 未満・input 0 dB）を確認する。
    let x = mixed(48_000);
    let y = run(&x, 1.0, 0.0, ceiling);
    let d = Finalist::new(SR).latency_samples() as usize;
    let maxdiff = (d..x.len())
        .map(|i| (y[i] - x[i - d]).abs())
        .fold(0.0f32, f32::max);
    println!("[k=0 恒等] ceiling 未満・input 0 dB での max|out[n] - in[n-D]| = {:.3e}", maxdiff);
}
