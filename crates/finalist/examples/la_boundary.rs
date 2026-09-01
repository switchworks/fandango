//! `lookahead_sweep` で見えた「THD が厳密に 0 になる N の下限」が
//! 閉形式 N* = ceil(SR/(4f)) に一致するかの確認用プローブ。**本実装ではない。**

use fandango_finalist::dsp::{Finalist, LOOKAHEAD_MS};

const SR: f32 = 48_000.0;

fn thd(y: &[f32], period: usize) -> f32 {
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

fn main() {
    let ceiling = 10f32.powf(-1.0 / 20.0);
    let ig = 10f32.powf(12.0 / 20.0);
    println!("f(Hz)  period  予測 N*=ceil(SR/4f)   実測の最小 N (THD < 1e-6)");
    for (f, p) in [(50.0f32, 960usize), (100.0, 480), (150.0, 320), (200.0, 240), (400.0, 120)] {
        let pred = (SR / (4.0 * f)).ceil() as usize;
        let x: Vec<f32> = (0..192_000)
            .map(|i| 0.5 * (2.0 * std::f32::consts::PI * f * i as f32 / SR).sin())
            .collect();
        let mut found = 0;
        for n in (pred.saturating_sub(6).max(2))..=(pred + 6) {
            let mut fin = Finalist::new(n as f32 * 1000.0 / LOOKAHEAD_MS);
            let d = fin.latency_samples() as usize;
            let rc = (-1.0f32 / (0.080 * SR)).exp();
            let out: Vec<f32> = x.iter().map(|&s| fin.process(s, s, ig, 0.0, ceiling, rc, false).0).collect();
            let skip = ((d + n) / p + 40) * p;
            if thd(&out[skip..], p) < 1.0e-6 {
                found = n;
                break;
            }
        }
        println!("{:6.0}  {:6}  {:18}  {:10}", f, p, pred, found);
    }
}
