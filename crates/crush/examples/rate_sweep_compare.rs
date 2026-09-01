//! 起点の掃引 wav を新方式 (経路 F) で作り直し、「降り注ぐ波」の形状を比較する
//! **プラグイン本体には触らない。**
//!
//! 経緯:
//!   オーナーが観測したのは「いくつもの波が上から下に降り注ぐ」形状。
//!   `rate_sweep_lfo.rs` で continuous はピークが連続下降し stepped は階段固定、と確認済み。
//!   その正体の同定は途中で覆った (半整数像ではなく整数像の折り返しだった) が、
//!   **形状が新方式で変わるかどうかは、同定と独立に判断材料になる。**
//!
//! 注意 (期待値):
//!   `rate` が連続変化すれば像 `m*fs/rate` は必ず連続移動する。これは効果そのもので、
//!   経路 F は像の**振幅**を変えるが**位置**は変えない。
//!   よって「連続下降が消える」ことは期待できない。見るべきは
//!   **軌跡の本数と強さ**が現行からどう変わるか。
//!
//! 3 経路:
//!   - stepped     : `rate.round()` (現行の挙動。オーナーが「全く出ない」と言った側)
//!   - continuous A: 実数 `rate` をそのまま現行の acc 方式へ
//!   - continuous F: 実数 `rate` を区間平均 + 線形補間で処理 (新方式)
//!
//! 実行: cargo run -q -p fandango-crush --example rate_sweep_compare --release [-- <out_dir>]

const SR: f64 = 48_000.0;
const BITS: f64 = 8.0;
const SECS: f64 = 6.0;
const FRAME: usize = 8_192;
const HOP: usize = 6_144;

fn f_antideriv(x: f64, b: f64) -> f64 {
    let d = 2.0 / (2.0_f64.powf(b) - 1.0);
    let m = (x / d + 0.5).floor();
    d * (m * x - d * m * m * 0.5)
}
fn quant(x: f64, b: f64) -> f64 {
    let d = 2.0 / (2.0_f64.powf(b) - 1.0);
    d * (x / d + 0.5).floor()
}
const ADAA_EPS: f64 = 1e-6;
fn adaa(x: f64, x_prev: f64) -> f64 {
    let dx = x - x_prev;
    if dx.abs() > ADAA_EPS {
        (f_antideriv(x, BITS) - f_antideriv(x_prev, BITS)) / dx
    } else {
        quant((x + x_prev) * 0.5, BITS)
    }
}

/// `rate_continuity.rs` の `test_input` と同一。
fn test_input(n: usize) -> Vec<f64> {
    (0..n)
        .map(|i| {
            let t = i as f64 / SR;
            let env = (-(t * 3.0)).exp();
            let tau = 2.0 * std::f64::consts::PI;
            0.6 * (tau * 220.0 * t).sin()
                + 0.25 * (tau * 1330.0 * t).sin()
                + 0.15 * env * (tau * 3700.0 * t).sin()
        })
        .collect()
}

/// 配列からの線形補間読み。非整数時刻 `t` の入力値。
/// **整数サンプル 2 点の加重和。LUT でも曲線近似でもない。**
fn read_lin(x: &[f64], t: f64) -> f64 {
    if t <= 0.0 {
        return x.first().copied().unwrap_or(0.0);
    }
    let i = t.floor() as usize;
    let f = t - i as f64;
    let a = x.get(i).copied().unwrap_or(0.0);
    let b = x.get(i + 1).copied().unwrap_or(a);
    a * (1.0 - f) + b * f
}

fn rate_at(i: usize, n: usize, start: f64, end: f64, stepped: bool) -> f64 {
    let t = i as f64 / (n - 1) as f64;
    let r = start + (end - start) * t;
    if stepped { r.round() } else { r }
}

/// 現行の acc 方式 (経路 A)。`stepped` で `rate.round()` を使う。
fn render_a(x: &[f64], start: f64, end: f64, stepped: bool) -> Vec<f64> {
    let n = x.len();
    let mut acc = 0.0_f64;
    let mut hold = 0.0_f64;
    let mut x_prev = 0.0_f64;
    let mut out = Vec::with_capacity(n);
    for (i, &v) in x.iter().enumerate() {
        let r = rate_at(i, n, start, end, stepped).max(1.0);
        if acc <= 0.0 {
            hold = v;
            acc += r;
        }
        acc -= 1.0;
        let q = adaa(hold, x_prev);
        x_prev = hold;
        out.push(q);
    }
    out
}

/// 経路 F。区間平均 + 線形補間。
///
/// 掃引では `rate` が時間変化するので、境界時刻を累積で持つ。
/// `t_next` が次の段の時刻。区間 `[i, i+1]` を `t_next` で分割し、
/// 各部分に対応する段の値を区間長で重み付けして平均する。
/// **`rate` が整数で境界が区間の端に来る場合は分割が起きず、経路 A と一致する。**
fn render_f(x: &[f64], start: f64, end: f64, stepped: bool) -> Vec<f64> {
    let n = x.len();
    let mut out = Vec::with_capacity(n);
    let mut x_prev = 0.0_f64;

    // 現在の段の時刻と値。
    let mut t_step = 0.0_f64;
    let mut v_step = read_lin(x, 0.0);
    // 次の段の時刻。
    let mut t_next = rate_at(0, n, start, end, stepped).max(1.0);

    for i in 0..n {
        let (lo, hi) = (i as f64, i as f64 + 1.0);
        let mut acc = 0.0_f64;
        let mut p = lo;
        while p < hi {
            let q = t_next.min(hi);
            acc += v_step * (q - p);
            if q >= hi {
                break;
            }
            // 段が切り替わる。次の段の値は「その時刻の入力」を線形補間で読む。
            t_step = t_next;
            v_step = read_lin(x, t_step);
            let r = rate_at(i, n, start, end, stepped).max(1.0);
            t_next = t_step + r;
            p = q;
        }
        let hold = acc / (hi - lo);
        let qv = adaa(hold, x_prev);
        x_prev = hold;
        out.push(qv);
    }
    let _ = t_step;
    out
}

fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2usize;
    while len <= n {
        let ang = -2.0 * std::f64::consts::PI / len as f64;
        let (wr, wi) = (ang.cos(), ang.sin());
        let mut i = 0usize;
        while i < n {
            let (mut cr, mut ci) = (1.0_f64, 0.0_f64);
            for k in 0..len / 2 {
                let (ur, ui) = (re[i + k], im[i + k]);
                let (vr, vi) = (
                    re[i + k + len / 2] * cr - im[i + k + len / 2] * ci,
                    re[i + k + len / 2] * ci + im[i + k + len / 2] * cr,
                );
                re[i + k] = ur + vr;
                im[i + k] = ui + vi;
                re[i + k + len / 2] = ur - vr;
                im[i + k + len / 2] = ui - vi;
                let nr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = nr;
            }
            i += len;
        }
        len <<= 1;
    }
}

fn db(x: f64) -> f64 {
    20.0 * x.max(1e-12).log10()
}

fn frame_spectrum(y: &[f64], off: usize) -> Vec<f64> {
    let mut re = vec![0.0; FRAME];
    let mut im = vec![0.0; FRAME];
    let mut cg = 0.0;
    for i in 0..FRAME {
        let u = 2.0 * std::f64::consts::PI * i as f64 / FRAME as f64;
        let w = 0.35875 - 0.48829 * u.cos() + 0.14128 * (2.0 * u).cos()
            - 0.01168 * (3.0 * u).cos();
        cg += w;
        re[i] = y.get(off + i).copied().unwrap_or(0.0) * w;
    }
    cg /= FRAME as f64;
    fft(&mut re, &mut im);
    (0..FRAME / 2)
        .map(|i| (re[i] * re[i] + im[i] * im[i]).sqrt() / FRAME as f64 * 2.0 / cg)
        .collect()
}

/// 2..22 kHz の局所最大のうち、入力の既知成分の近傍を除いた上位 `k` 本。
fn top_peaks(spec: &[f64], k: usize) -> Vec<(f64, f64)> {
    let bin_hz = SR / FRAME as f64;
    let known = [220.0_f64, 1330.0, 3700.0];
    let lo = (2_000.0 / bin_hz).ceil() as usize;
    let hi = ((22_000.0 / bin_hz).floor() as usize).min(spec.len() - 2);
    let mut cand: Vec<(f64, f64)> = Vec::new();
    for i in lo..=hi {
        if spec[i] >= spec[i - 1] && spec[i] >= spec[i + 1] {
            let f = i as f64 * bin_hz;
            if known.iter().any(|kk| (f - kk).abs() < 40.0) {
                continue;
            }
            cand.push((f, spec[i]));
        }
    }
    cand.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut out: Vec<(f64, f64)> = Vec::new();
    for c in cand {
        if out.iter().any(|o| (o.0 - c.0).abs() < 200.0) {
            continue;
        }
        out.push(c);
        if out.len() >= k {
            break;
        }
    }
    out
}

/// 2..22 kHz で、しきい値より上にある局所ピークの本数と合計エネルギー。
/// **「降り注ぐ波」の本数と強さ**を数える。
fn peak_census(spec: &[f64], floor_db: f64) -> (usize, f64) {
    let bin_hz = SR / FRAME as f64;
    let known = [220.0_f64, 1330.0, 3700.0];
    let lo = (2_000.0 / bin_hz).ceil() as usize;
    let hi = ((22_000.0 / bin_hz).floor() as usize).min(spec.len() - 2);
    let mut cnt = 0usize;
    let mut e = 0.0_f64;
    for i in lo..=hi {
        if spec[i] >= spec[i - 1] && spec[i] >= spec[i + 1] {
            let f = i as f64 * bin_hz;
            if known.iter().any(|kk| (f - kk).abs() < 40.0) {
                continue;
            }
            if db(spec[i]) > floor_db {
                cnt += 1;
                e += spec[i] * spec[i];
            }
        }
    }
    (cnt, db(e.sqrt()))
}

fn write_wav(path: &str, y: &[f64]) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SR as u32,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(path, spec).expect("create wav");
    for &v in y {
        w.write_sample(v as f32).expect("write");
    }
    w.finalize().expect("finalize");
    println!("  wrote {path}");
}

fn main() {
    let dir = std::env::args().nth(1).unwrap_or_else(|| "D:/output/crush_rate".to_string());
    std::fs::create_dir_all(&dir).expect("create out_dir");

    let n = (SR * SECS) as usize;
    let x = test_input(n);

    println!("掃引 wav の 3 経路比較");
    println!();
    println!("  stepped      = rate.round() (現行の挙動)");
    println!("  continuous A = 実数 rate をそのまま現行の acc 方式へ");
    println!("  continuous F = 実数 rate を区間平均 + 線形補間で処理 (新方式)");
    println!();
    println!("  注意: rate が連続変化すれば像 m*fs/rate は必ず連続移動する。");
    println!("  F は像の振幅を変えるが位置は変えない。**見るべきは本数と強さ。**");
    println!();

    for (name, start, end) in [("rate_1to8", 1.0_f64, 8.0_f64), ("rate_1to32", 1.0, 32.0)] {
        let st = render_a(&x, start, end, true);
        let ca = render_a(&x, start, end, false);
        let cf = render_f(&x, start, end, false);

        write_wav(&format!("{dir}/{name}_F_continuous.wav"), &cf);

        println!();
        println!("[{name}] 2..22 kHz のピーク本数と合計エネルギー (床 -70 dB)");
        println!();
        println!("  時刻  rate     stepped        continuous A     continuous F");
        println!("                  本数/合計[dB]  本数/合計[dB]    本数/合計[dB]");
        let mut off = 0usize;
        while off + FRAME <= n {
            let t_c = (off + FRAME / 2) as f64 / SR;
            let r = start + (end - start) * (t_c / SECS);
            let (c1, e1) = peak_census(&frame_spectrum(&st, off), -70.0);
            let (c2, e2) = peak_census(&frame_spectrum(&ca, off), -70.0);
            let (c3, e3) = peak_census(&frame_spectrum(&cf, off), -70.0);
            println!(
                "  {t_c:>4.2} {r:>6.2} {c1:>8} /{e1:>7.1} {c2:>10} /{e2:>7.1} {c3:>11} /{e3:>7.1}"
            );
            off += HOP * 2;
        }

        println!();
        println!("[{name}] 上位 3 ピークの軌跡 (Hz と dB)");
        println!();
        println!("  時刻  rate   stepped                      continuous A                 continuous F");
        let mut off = 0usize;
        while off + FRAME <= n {
            let t_c = (off + FRAME / 2) as f64 / SR;
            let r = start + (end - start) * (t_c / SECS);
            let fmt = |v: &Vec<(f64, f64)>| -> String {
                let mut s: Vec<String> = v
                    .iter()
                    .map(|(f, a)| format!("{f:>6.0}({:>5.1})", db(*a)))
                    .collect();
                while s.len() < 3 {
                    s.push("      (     )".to_string());
                }
                s.join(" ")
            };
            println!(
                "  {t_c:>4.2} {r:>6.2} {}  {}  {}",
                fmt(&top_peaks(&frame_spectrum(&st, off), 3)),
                fmt(&top_peaks(&frame_spectrum(&ca, off), 3)),
                fmt(&top_peaks(&frame_spectrum(&cf, off), 3))
            );
            off += HOP * 3;
        }
    }
}
