//! finalist の真ピーク (ISP / dBTP) 実測。
//!
//! コードが保証しているのは**サンプルピーク** `|out| <= ceiling*(1-1e-6)` のみ。
//! 「業界標準」(ITU-R BS.1770-4 / EBU R128) は真ピークなので、その差を測る。
//!
//! 実行: `cargo run --release -p fandango-finalist --example isp_measure`

use fandango_finalist::dsp::Finalist;

const SR: f32 = 48_000.0;

fn db(x: f32) -> f32 {
    20.0 * x.max(1.0e-12).log10()
}

/// 窓付き sinc による OS 倍補間ピーク。BS.1770-4 は最低 4x を要求する。
fn true_peak(x: &[f32], os: usize) -> f32 {
    const HALF: isize = 24;
    let taps: Vec<f32> = (0..os)
        .flat_map(|p| {
            let frac = p as f32 / os as f32;
            (0..2 * HALF as usize).map(move |k| {
                let t = (k as isize - HALF + 1) as f32 - frac;
                let s = if t.abs() < 1.0e-7 {
                    1.0
                } else {
                    (std::f32::consts::PI * t).sin() / (std::f32::consts::PI * t)
                };
                let u = (k as f32 + 1.0 - frac) / (2.0 * HALF as f32);
                let w = 0.42 - 0.5 * (2.0 * std::f32::consts::PI * u).cos()
                    + 0.08 * (4.0 * std::f32::consts::PI * u).cos();
                s * w
            })
        })
        .collect();
    let n = x.len() as isize;
    let mut peak = 0.0_f32;
    for i in 0..n {
        for p in 0..os {
            let mut acc = 0.0_f32;
            for k in 0..2 * HALF {
                let idx = i + k - HALF + 1;
                if idx >= 0 && idx < n {
                    acc += x[idx as usize] * taps[p * 2 * HALF as usize + k as usize];
                }
            }
            peak = peak.max(acc.abs());
        }
    }
    peak
}

struct Rng(u32);
impl Rng {
    fn f(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 as i32) as f32 / 2_147_483_647.0
    }
}

fn sine(f: f32, n: usize, phase: f32) -> Vec<f32> {
    (0..n)
        .map(|i| (2.0 * std::f32::consts::PI * f * i as f32 / SR + phase).sin())
        .collect()
}

/// BS.1770-4 の ISP 最悪ケース族: fs/4 近傍の正弦を位相オフセットさせたもの。
fn worst_isp(n: usize) -> Vec<f32> {
    sine(SR / 4.0, n, std::f32::consts::PI / 4.0)
}

fn square(f: f32, n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| if ((i as f32 / SR) * f).fract() < 0.5 { 1.0 } else { -1.0 })
        .collect()
}

fn saw(f: f32, n: usize) -> Vec<f32> {
    (0..n).map(|i| 2.0 * ((i as f32 / SR) * f).fract() - 1.0).collect()
}

fn white(n: usize, seed: u32) -> Vec<f32> {
    let mut r = Rng(seed);
    (0..n).map(|_| r.f()).collect()
}

/// finalist を通す。`input_db` は入力ゲイン、`drive` ∈ [0,1]、`ceiling_db` は天井。
fn run(x: &[f32], input_db: f32, drive: f32, ceiling_db: f32, release_ms: f32) -> Vec<f32> {
    let mut f = Finalist::new(SR);
    let ig = 10.0_f32.powf(input_db / 20.0);
    let c = 10.0_f32.powf(ceiling_db / 20.0);
    let rc = f.release_coeff(release_ms);
    let d = f.latency_samples() as usize;
    let mut out = Vec::with_capacity(x.len() + d);
    for &s in x {
        let (l, _r) = f.process(s, s, ig, drive, c, rc, false);
        out.push(l);
    }
    // 遅延分を吐き出してから、先頭 D サンプル (遅延線の初期ゼロ) を捨てる。
    for _ in 0..d {
        let (l, _r) = f.process(0.0, 0.0, ig, drive, c, rc, false);
        out.push(l);
    }
    out.drain(0..d);
    out
}

fn main() {
    let n = 48_000;
    let ceiling_db: f32 = std::env::args()
        .nth(1)
        .map(|v| v.parse().expect("ceiling_db"))
        .unwrap_or(-0.2);
    let mats: Vec<(String, Vec<f32>)> = vec![
        ("sine 1 kHz".into(), sine(1000.0, n, 0.0)),
        ("sine 997 Hz".into(), sine(997.0, n, 0.0)),
        ("sine fs/4 phi=45deg (ISP最悪)".into(), worst_isp(n)),
        ("sine 11 kHz".into(), sine(11_000.0, n, 0.0)),
        ("square 100 Hz".into(), square(100.0, n)),
        ("saw 55 Hz".into(), saw(55.0, n)),
        ("white".into(), white(n, 0x1234_5678)),
    ];

    println!("ceiling = {ceiling_db:+.1} dBFS / release = 80 ms / sr = {SR} Hz");
    println!("保証: サンプルピーク <= ceiling*(1-1e-6)。真ピークは無保証。\n");
    println!("{:<30} {:>6} {:>6} {:>9} {:>9} {:>9} {:>9}",
        "素材", "in dB", "drive", "smpPeak", "TP 4x", "TP 8x", "ISP超過");
    for (name, x) in &mats {
        for &(ig, dr) in &[(0.0_f32, 0.0_f32), (12.0, 0.0), (12.0, 1.0), (24.0, 1.0)] {
            let y = run(x, ig, dr, ceiling_db, 80.0);
            let sp = y.iter().fold(0.0_f32, |a, v| a.max(v.abs()));
            let tp4 = true_peak(&y, 4);
            let tp8 = true_peak(&y, 8);
            println!("{:<30} {:>6.0} {:>6.1} {:>9.3} {:>9.3} {:>9.3} {:>+9.3}",
                name, ig, dr, db(sp), db(tp4), db(tp8), db(tp8) - ceiling_db);
        }
    }
}
