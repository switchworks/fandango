//! lookahead 窓長 N のスイープ実測。`LOOKAHEAD_MS` の根拠を作るための offline 測定。
//! **本実装ではない。**
//!
//! N の与え方: `Finalist::process` が sample_rate に依存するのは窓長 N の決定のみで、
//! release 係数は引数として外から渡す。よって `Finalist::new(N*1000/LOOKAHEAD_MS)` で
//! N を直に指定し、release 係数は実 sr = 48 kHz の値を渡せば、
//! 48 kHz の信号に対して N だけを振った測定になる。
//!
//! 実行: `cargo run --release -p fandango-finalist --example lookahead_sweep`

use fandango_finalist::dsp::{Finalist, LOOKAHEAD_MS};

const SR: f32 = 48_000.0;

fn db(x: f32) -> f32 {
    20.0 * x.max(1.0e-12).log10()
}

fn make(n: usize) -> Finalist {
    Finalist::new(n as f32 * 1000.0 / LOOKAHEAD_MS)
}

fn release_coeff(release_ms: f32) -> f32 {
    (-1.0 / (release_ms / 1000.0 * SR)).exp()
}

/// 単一の設定でモノ信号を通し、(出力, ゲイン列 g[n], D) を返す。
/// g[n] = out[n] / (x[n-D]·input_gain)（drive = 0 なら wet = x·input_gain で厳密）。
fn run(n: usize, x: &[f32], input_gain: f32, ceiling: f32, rel_ms: f32) -> (Vec<f32>, Vec<f32>, usize) {
    let mut f = make(n);
    let d = f.latency_samples() as usize;
    let rc = release_coeff(rel_ms);
    let mut out = Vec::with_capacity(x.len());
    for &s in x {
        let (l, _) = f.process(s, s, input_gain, 0.0, ceiling, rc, false);
        out.push(l);
    }
    let mut g = vec![f32::NAN; out.len()];
    for i in d..out.len() {
        let w = x[i - d] * input_gain;
        if w.abs() > 1.0e-3 {
            g[i] = out[i] / w;
        }
    }
    (out, g, d)
}

/// 整数周期区間の DFT による THD（2〜20 次）。
fn thd(y: &[f32], period: usize) -> f32 {
    let cycles = y.len() / period;
    let len = cycles * period;
    let y = &y[..len];
    let mag = |k: usize| -> f32 {
        let (mut re, mut im) = (0.0f64, 0.0f64);
        let w = 2.0 * std::f64::consts::PI * (k * cycles) as f64 / len as f64;
        for (i, &v) in y.iter().enumerate() {
            let p = w * i as f64;
            re += v as f64 * p.cos();
            im -= v as f64 * p.sin();
        }
        ((re * re + im * im).sqrt() / len as f64) as f32
    };
    let f1 = mag(1);
    let h: f32 = (2..=20).map(|k| mag(k).powi(2)).sum();
    h.sqrt() / f1.max(1.0e-12)
}

fn sine(freq: f32, amp: f32, len: usize) -> Vec<f32> {
    (0..len)
        .map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / SR).sin())
        .collect()
}

/// 決定論的な擬似ホワイトノイズ（LCG）。
fn noise(amp: f32, len: usize) -> Vec<f32> {
    let mut s = 0x1234_5678u32;
    (0..len)
        .map(|_| {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            amp * ((s >> 8) as f32 / 8_388_608.0 - 1.0)
        })
        .collect()
}

/// 低レベル 1 kHz 正弦の上に 5 ms の大音量バーストを載せた過渡素材。
fn burst(len: usize) -> Vec<f32> {
    let mut v = sine(1000.0, 0.05, len);
    let start = len / 3;
    let n = (0.005 * SR) as usize;
    for i in start..(start + n).min(len) {
        v[i] += 0.9 * (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / SR).sin();
    }
    v
}

fn main() {
    let ceiling = 10f32.powf(-1.0 / 20.0); // -1.0 dB
    let ns: [usize; 7] = [12, 24, 48, 72, 120, 192, 288];
    let rel_ms = 80.0;
    let ig = 10f32.powf(12.0 / 20.0);

    println!("SR = {} Hz, ceiling = -1.0 dB, drive = 0, release = {} ms", SR, rel_ms);
    println!();

    println!("[天井保証] max|out| / ceiling  (dB, 正なら超過)");
    println!("  N(smp)  N(ms)   D(smp)  D(ms)   50Hz       1kHz       noise      burst");
    for &n in &ns {
        let d = 2 * n - 2;
        let mats: Vec<Vec<f32>> = vec![
            sine(50.0, 0.5, 48_000),
            sine(1000.0, 0.5, 48_000),
            noise(0.5, 48_000),
            burst(48_000),
        ];
        let mut cols = String::new();
        for m in &mats {
            let (out, _, _) = run(n, m, ig, ceiling, rel_ms);
            let pk = out.iter().fold(0.0f32, |a, &b| a.max(b.abs()));
            cols.push_str(&format!("  {:+9.6}", db(pk / ceiling)));
        }
        println!(
            "  {:5}  {:5.2}  {:6}  {:5.2}{}",
            n, n as f32 * 1000.0 / SR, d, d as f32 * 1000.0 / SR, cols
        );
    }
    println!();

    println!("[THD 2〜20次] 正弦 amp 0.5、input +12 dB（単位 %）");
    println!("  N(smp)  N(ms)     50Hz     100Hz      200Hz      1kHz");
    for &n in &ns {
        let mut cols = String::new();
        for (f, p) in [(50.0f32, 960usize), (100.0, 480), (200.0, 240), (1000.0, 48)] {
            let x = sine(f, 0.5, 96_000);
            let (out, _, d) = run(n, &x, ig, ceiling, rel_ms);
            let skip = ((d + n) / p + 40) * p;
            cols.push_str(&format!("  {:9.4}", 100.0 * thd(&out[skip..], p)));
        }
        println!("  {:5}  {:5.2}{}", n, n as f32 * 1000.0 / SR, cols);
    }
    println!();

    println!("[ゲイン変動] burst 素材、input +12 dB");
    println!("  N(smp)  N(ms)   max|dg|/smp   min g(dB)   復帰 g>0.99 (ms)");
    for &n in &ns {
        let x = burst(48_000);
        let (_, g, _) = run(n, &x, ig, ceiling, rel_ms);
        let mut maxd = 0.0f32;
        let mut prev: Option<(usize, f32)> = None;
        for (i, &v) in g.iter().enumerate() {
            if v.is_nan() {
                continue;
            }
            if let Some((pi, pv)) = prev {
                maxd = maxd.max((v - pv).abs() / (i - pi) as f32);
            }
            prev = Some((i, v));
        }
        let ming = g.iter().filter(|v| !v.is_nan()).fold(1.0f32, |a, &b| a.min(b));
        let dip = g.iter().position(|v| !v.is_nan() && *v < 0.999).unwrap_or(0);
        let rec = g
            .iter()
            .enumerate()
            .skip(dip)
            .find(|(_, v)| !v.is_nan() && **v > 0.99)
            .map(|(i, _)| (i - dip) as f32 * 1000.0 / SR)
            .unwrap_or(f32::NAN);
        println!(
            "  {:5}  {:5.2}   {:10.3e}   {:8.3}   {:8.2}",
            n, n as f32 * 1000.0 / SR, maxd, db(ming), rec
        );
    }
}
