//! R1 の設計決定用プローブ: オーバサンプリングで真ピーク超過がどこまで詰まるか。
//! **本実装ではない。** 到達可能な上限を測るための offline 実験。
//!
//! 比較する 3 方式:
//!   (a) 現行 — ベースレートで検出・ゲイン適用
//!   (b) 検出のみ 4x — ISP 包絡を検出器へ入れ、ゲインはベースレートで適用
//!   (c) 全段 4x — 4x へ上げて `Finalist` をそのまま 4x で回し、デシメート
//!
//! 実行: `cargo run --release -p fandango-finalist --example ovs_probe`

use fandango_finalist::dsp::{drive_shape, Finalist};

const SR: f32 = 48_000.0;
const OS: usize = 4;
/// 補間・間引き FIR の片側長 (ベースレートのサンプル数)。
const FIR_HALF: isize = 32;

fn db(x: f32) -> f32 {
    20.0 * x.max(1.0e-12).log10()
}

/// 窓付き sinc の位相 p 用タップ列 (長さ 2*FIR_HALF)。`frac = p/OS`。
fn taps(frac: f32) -> Vec<f32> {
    (0..2 * FIR_HALF as usize)
        .map(|k| {
            let t = (k as isize - FIR_HALF + 1) as f32 - frac;
            let s = if t.abs() < 1.0e-7 {
                1.0
            } else {
                (std::f32::consts::PI * t).sin() / (std::f32::consts::PI * t)
            };
            let u = (k as f32 + 1.0 - frac) / (2.0 * FIR_HALF as f32);
            let w = 0.42 - 0.5 * (2.0 * std::f32::consts::PI * u).cos()
                + 0.08 * (4.0 * std::f32::consts::PI * u).cos();
            s * w
        })
        .collect()
}

fn conv_at(x: &[f32], i: isize, t: &[f32]) -> f32 {
    let n = x.len() as isize;
    let mut acc = 0.0;
    for (k, tk) in t.iter().enumerate() {
        let idx = i + k as isize - FIR_HALF + 1;
        if idx >= 0 && idx < n {
            acc += x[idx as usize] * tk;
        }
    }
    acc
}

/// OS 倍補間ピーク (真ピーク)。
fn true_peak(x: &[f32]) -> f32 {
    let ts: Vec<Vec<f32>> = (0..OS).map(|p| taps(p as f32 / OS as f32)).collect();
    let mut peak = 0.0_f32;
    for i in 0..x.len() as isize {
        for t in &ts {
            peak = peak.max(conv_at(x, i, t).abs());
        }
    }
    peak
}

/// ゼロ詰め + FIR による OS 倍アップサンプル。
fn upsample(x: &[f32]) -> Vec<f32> {
    let ts: Vec<Vec<f32>> = (0..OS).map(|p| taps(p as f32 / OS as f32)).collect();
    let mut y = Vec::with_capacity(x.len() * OS);
    for i in 0..x.len() as isize {
        for t in &ts {
            y.push(conv_at(x, i, t));
        }
    }
    y
}

/// 4x 領域で帯域制限してから OS 分の 1 に間引く。
fn downsample(y: &[f32]) -> Vec<f32> {
    let half = FIR_HALF * OS as isize;
    let t: Vec<f32> = (0..2 * half as usize)
        .map(|k| {
            let tt = (k as isize - half + 1) as f32 / OS as f32;
            let s = if tt.abs() < 1.0e-7 {
                1.0
            } else {
                (std::f32::consts::PI * tt).sin() / (std::f32::consts::PI * tt)
            };
            let u = (k as f32 + 1.0) / (2.0 * half as f32);
            let w = 0.42 - 0.5 * (2.0 * std::f32::consts::PI * u).cos()
                + 0.08 * (4.0 * std::f32::consts::PI * u).cos();
            s * w / OS as f32
        })
        .collect();
    let n = y.len() as isize;
    let mut out = Vec::with_capacity(y.len() / OS);
    let mut i = 0isize;
    while i < n {
        let mut acc = 0.0;
        for (k, tk) in t.iter().enumerate() {
            let idx = i + k as isize - half + 1;
            if idx >= 0 && idx < n {
                acc += y[idx as usize] * tk;
            }
        }
        out.push(acc);
        i += OS as isize;
    }
    out
}

// ---- 現行アルゴリズムの offline 再実装 (方式 b 用。ロジックは dsp.rs と同一) ----

fn sliding(x: &[f32], w: usize, is_max: bool) -> Vec<f32> {
    (0..x.len())
        .map(|n| {
            let lo = n.saturating_sub(w - 1);
            let mut acc = if is_max { f32::MIN } else { f32::MAX };
            for v in &x[lo..=n] {
                acc = if is_max { acc.max(*v) } else { acc.min(*v) };
            }
            acc
        })
        .collect()
}

/// `Boxcar::new` が 1.0 初期化なので、負 index は 1.0 として扱う。
fn boxcar(x: &[f32], w: usize) -> Vec<f32> {
    (0..x.len())
        .map(|n| {
            let lo = n as isize - w as isize + 1;
            let mut s = 0.0_f64;
            for j in lo..=n as isize {
                s += if j < 0 { 1.0 } else { x[j as usize] as f64 };
            }
            (s / w as f64) as f32
        })
        .collect()
}

/// 方式 (b): 検出器へ渡すピーク列 `p` を差し替えた版。ゲインはベースレートで適用。
fn limit_with_env(x: &[f32], p: &[f32], ceiling: f32, release_ms: f32) -> Vec<f32> {
    let n = (2.5 / 1000.0 * SR).ceil().max(2.0) as usize;
    let d = 2 * n - 2;
    let c = ceiling * (1.0 - 1e-6);
    let rc = (-1.0 / (release_ms / 1000.0 * SR)).exp();
    let pk = sliding(p, n, true);
    let mut g_rel = Vec::with_capacity(x.len());
    let mut prev = 1.0_f32;
    for k in 0..x.len() {
        let g_req = c / pk[k].max(c);
        prev = g_req.min(1.0 - (1.0 - prev) * rc);
        g_rel.push(prev);
    }
    let g = boxcar(&boxcar(&sliding(&g_rel, n, false), n), n);
    (0..x.len())
        .map(|i| if i < d { 0.0 } else { x[i - d] * g[i] })
        .collect()
}

// ---- 素材 ----

fn sine(f: f32, n: usize, phase: f32) -> Vec<f32> {
    (0..n)
        .map(|i| (2.0 * std::f32::consts::PI * f * i as f32 / SR + phase).sin())
        .collect()
}

fn run_current(x: &[f32], ig: f32, dr: f32, c: f32) -> Vec<f32> {
    let mut f = Finalist::new(SR);
    let rc = f.release_coeff(80.0);
    let d = f.latency_samples() as usize;
    let mut out: Vec<f32> = x
        .iter()
        .map(|&s| f.process(s, s, ig, dr, c, rc, false).0)
        .collect();
    for _ in 0..d {
        out.push(f.process(0.0, 0.0, ig, dr, c, rc, false).0);
    }
    out.drain(0..d);
    out
}

/// 方式 (c): 4x へ上げて `Finalist` を 4x で回し、デシメート。
fn run_full_os(x: &[f32], ig: f32, dr: f32, c: f32) -> Vec<f32> {
    let up = upsample(x);
    let mut f = Finalist::new(SR * OS as f32);
    let rc = f.release_coeff(80.0);
    let d = f.latency_samples() as usize;
    let mut y: Vec<f32> = up
        .iter()
        .map(|&s| f.process(s, s, ig, dr, c, rc, false).0)
        .collect();
    for _ in 0..d {
        y.push(f.process(0.0, 0.0, ig, dr, c, rc, false).0);
    }
    y.drain(0..d);
    downsample(&y)
}

/// 方式 (b): drive 後の信号の ISP 包絡を検出器へ入れる。
/// drive 段は ADAA なしの静的整形で近似する (プローブなので可)。
fn run_det_os(x: &[f32], ig: f32, dr: f32, c: f32) -> Vec<f32> {
    let u: Vec<f32> = x.iter().map(|&s| drive_shape(s * ig, dr)).collect();
    let ts: Vec<Vec<f32>> = (0..OS).map(|p| taps(p as f32 / OS as f32)).collect();
    let env: Vec<f32> = (0..u.len() as isize)
        .map(|i| ts.iter().fold(0.0_f32, |a, t| a.max(conv_at(&u, i, t).abs())))
        .collect();
    limit_with_env(&u, &env, c, 80.0)
}

fn main() {
    let n = 24_000;
    let cdb = -0.2_f32;
    let c = 10.0_f32.powf(cdb / 20.0);
    let mats: Vec<(String, Vec<f32>)> = vec![
        (
            "sine fs/4 phi=45deg".into(),
            sine(SR / 4.0, n, std::f32::consts::PI / 4.0),
        ),
        ("sine 11 kHz".into(), sine(11_000.0, n, 0.0)),
        ("sine 1 kHz".into(), sine(1000.0, n, 0.0)),
        ("sine 997 Hz".into(), sine(997.0, n, 0.0)),
    ];
    println!("ceiling = {cdb:+.1} dBFS / OS = {OS} / 真ピーク [dBFS] と ceiling 比超過\n");
    println!(
        "{:<22} {:>4} {:>4} | {:>8} {:>7} | {:>8} {:>7} | {:>8} {:>7}",
        "素材", "in", "drv", "(a)現行", "超過", "(b)検出4x", "超過", "(c)全4x", "超過"
    );
    for (name, x) in &mats {
        for &(igd, dr) in &[(0.0_f32, 0.0_f32), (12.0, 0.0), (12.0, 1.0), (24.0, 1.0)] {
            let ig = 10.0_f32.powf(igd / 20.0);
            let a = db(true_peak(&run_current(x, ig, dr, c)));
            let b = db(true_peak(&run_det_os(x, ig, dr, c)));
            let cc = db(true_peak(&run_full_os(x, ig, dr, c)));
            println!(
                "{:<22} {:>4.0} {:>4.1} | {:>8.3} {:>+7.3} | {:>8.3} {:>+7.3} | {:>8.3} {:>+7.3}",
                name,
                igd,
                dr,
                a,
                a - cdb,
                b,
                b - cdb,
                cc,
                cc - cdb
            );
        }
    }
}
