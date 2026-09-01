//! 阻止域仕様から `OS` と `N` を導出する
//! **プラグイン本体には触らない。**
//!
//! 問題意識:
//!   `half_os = 12` を「8 で -42 dB、12 で -72 dB だったから 12」と選ぶのは
//!   至上命題 優先順位 3 のマジックナンバーであり、優先順位 2 が禁じる
//!   「テストしたら満たされていた」そのもの。
//!   よって**不変条件を阻止域仕様に置き、`OS` と `N` はそこから導出**する。
//!
//! 仕様の定義:
//!   可聴上限 `F_AUDIBLE` の信号がアップサンプルで作る最初のイメージ帯の下端を
//!     `f_s = (1 - F_AUDIBLE/SR) / OS`   [OS レート正規化周波数]
//!   とする。イメージは基底サンプリング周波数 `1/OS` (OS レート正規化) の周りに
//!   `1/OS - f_a/OS` から立つ (`f_a = F_AUDIBLE/SR` は基底正規化した可聴上限)。
//!   とし、`f_s` から Nyquist (0.5) までの `|H(f)|` の最大値を阻止域減衰 `A` [dB] とする。
//!   `A` はカーネルの式だけで決まり、素材にも sr にも依存しない。
//!
//! 実行: cargo run -q -p fandango-crush --example rate_kernel_spec --release

use fandango_crush::dsp::DcBlocker;

fn f_antideriv_f64(x: f64, b: f64) -> f64 {
    let d = 2.0 / (2.0_f64.powf(b) - 1.0);
    let m = (x / d + 0.5).floor();
    d * (m * x - d * m * m * 0.5)
}
fn quantize_f64(x: f64, b: f64) -> f64 {
    let d = 2.0 / (2.0_f64.powf(b) - 1.0);
    d * (x / d + 0.5).floor()
}
const ADAA_EPS: f64 = 1e-6;
fn shape_adaa_quant(x: f32, x_prev: f64, b: f32) -> f32 {
    let xf = x as f64;
    let dx = xf - x_prev;
    if dx.abs() > ADAA_EPS {
        ((f_antideriv_f64(xf, b as f64) - f_antideriv_f64(x_prev, b as f64)) / dx) as f32
    } else {
        quantize_f64((xf + x_prev) * 0.5, b as f64) as f32
    }
}
const DENORMAL_GATE: f32 = 1e-30;
fn denormal_gate(x: f32) -> f32 {
    if x.abs() < DENORMAL_GATE { 0.0 } else { x }
}

const SR: f64 = 48_000.0;
const BITS: f32 = 8.0;
const F_IN: f64 = 220.0;
const N: usize = 48_000;
/// 可聴上限。阻止域下端の定義に使う。
const F_AUDIBLE: f64 = 20_000.0;
/// 裁定 C で確定した受け入れ床。
const ACCEPT_FLOOR_DB: f64 = -60.0;

fn sine(n: usize) -> Vec<f32> {
    sine_at(n, F_IN)
}

fn sine_at(n: usize, f: f64) -> Vec<f32> {
    (0..n).map(|i| (0.6 * (2.0 * std::f64::consts::PI * f * i as f64 / SR).sin()) as f32).collect()
}

fn composite(n: usize) -> Vec<f32> {
    let mut rng = 0x2545_F491_u32;
    (0..n)
        .map(|i| {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            let noise = (rng as i32) as f32 / 2_147_483_647.0;
            let t = i as f64 / SR;
            let tau = 2.0 * std::f64::consts::PI;
            (0.45 * (tau * 220.0 * t).sin()
                + 0.20 * (tau * 1237.0 * t).sin()
                + 0.10 * (tau * 4519.0 * t).sin()) as f32
                + 0.05 * noise
        })
        .collect()
}

/// 窓付き sinc のローパス核。`rate_oversample.rs` と同一式。
fn sinc_kernel(cutoff: f64, half: usize) -> Vec<f64> {
    let n = 2 * half + 1;
    let mut k = vec![0.0; n];
    let mut sum = 0.0;
    for (i, v) in k.iter_mut().enumerate() {
        let t = i as f64 - half as f64;
        let s = if t.abs() < 1e-12 {
            2.0 * cutoff
        } else {
            (2.0 * std::f64::consts::PI * cutoff * t).sin() / (std::f64::consts::PI * t)
        };
        let u = i as f64 / (n - 1) as f64;
        let w = 0.42 - 0.5 * (2.0 * std::f64::consts::PI * u).cos()
            + 0.08 * (4.0 * std::f64::consts::PI * u).cos();
        *v = s * w;
        sum += *v;
    }
    for v in k.iter_mut() {
        *v /= sum;
    }
    k
}

/// カーネルの阻止域減衰 `A` [dB]。`f_s` から Nyquist までの `|H(f)|` の最大値。
/// **これが仕様。`half` はこの値を満たす最小値として導出される。**
fn stopband_db(k: &[f64], os: usize) -> f64 {
    let half = (k.len() - 1) / 2;
    let f_s = (1.0 - F_AUDIBLE / SR) / os as f64;
    let steps = 4096;
    let mut worst: f64 = 0.0;
    for i in 0..=steps {
        let f = f_s + (0.5 - f_s) * i as f64 / steps as f64;
        let w = 2.0 * std::f64::consts::PI * f;
        let (mut re, mut im) = (0.0_f64, 0.0_f64);
        for (j, v) in k.iter().enumerate() {
            let t = j as f64 - half as f64;
            re += v * (w * t).cos();
            im -= v * (w * t).sin();
        }
        worst = worst.max((re * re + im * im).sqrt());
    }
    20.0 * worst.max(1e-15).log10()
}

fn upsample(x: &[f32], os: usize, k: &[f64]) -> Vec<f32> {
    let n = x.len() * os;
    let half = (k.len() - 1) / 2;
    let mut y = vec![0.0_f32; n];
    for (i, o) in y.iter_mut().enumerate() {
        let mut acc = 0.0;
        let lo = (i as isize - half as isize).div_euclid(os as isize);
        let hi = (i as isize + half as isize).div_euclid(os as isize);
        for j in lo..=hi {
            if j < 0 || j as usize >= x.len() {
                continue;
            }
            let tap = i as isize - j * os as isize + half as isize;
            if tap < 0 || tap as usize >= k.len() {
                continue;
            }
            acc += x[j as usize] as f64 * k[tap as usize];
        }
        *o = (acc * os as f64) as f32;
    }
    y
}

fn decimate(x: &[f32], os: usize, k: &[f64]) -> Vec<f32> {
    let half = (k.len() - 1) / 2;
    let mut y = Vec::with_capacity(x.len() / os);
    let mut i = 0;
    while i < x.len() {
        let mut acc = 0.0;
        for (t, kv) in k.iter().enumerate() {
            let j = i as isize + t as isize - half as isize;
            if j < 0 || j as usize >= x.len() {
                continue;
            }
            acc += x[j as usize] as f64 * kv;
        }
        y.push(acc as f32);
        i += os;
    }
    y
}

/// `rate_oversample.rs` の `render` と同一構成。`os = 1` で現行経路。
fn render(x: &[f32], rate: f64, os: usize, half_os: usize) -> Vec<f32> {
    let k = sinc_kernel(0.5 / os as f64, half_os);
    let xs = if os == 1 { x.to_vec() } else { upsample(x, os, &k) };

    let rate_os = rate * os as f64;
    let mut acc = 0.0_f64;
    let mut hold = 0.0_f32;
    let mut x_prev = 0.0_f64;
    let mut ys = Vec::with_capacity(xs.len());
    for &v in &xs {
        let xd = if v.is_finite() { v.clamp(-4.0, 4.0) } else { 0.0 };
        if acc <= 0.0 {
            hold = xd;
            acc += rate_os;
        }
        acc -= 1.0;
        let y = shape_adaa_quant(hold, x_prev, BITS);
        x_prev = hold as f64;
        ys.push(denormal_gate(y));
    }

    let mut y = if os == 1 { ys } else { decimate(&ys, os, &k) };

    let mut dc = DcBlocker::new();
    dc.set_sample_rate(SR as f32);
    for v in y.iter_mut() {
        *v = dc.process(*v);
        *v = if v.is_finite() { v.clamp(-6.0, 6.0) } else { 0.0 };
    }
    y
}

fn goertzel(y: &[f32], f_hz: f64) -> f64 {
    let w = 2.0 * std::f64::consts::PI * f_hz / SR;
    let c = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0_f64, 0.0_f64);
    for &v in y {
        let s0 = v as f64 + c * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    ((s1 * s1 + s2 * s2 - c * s1 * s2).max(0.0)).sqrt() / y.len() as f64 * 2.0
}
fn db(x: f64) -> f64 {
    20.0 * x.max(1e-12).log10()
}
fn sideband_db(y: &[f32], f0: f64) -> f64 {
    sideband_db_at(y, f0, F_IN)
}
fn sideband_db_at(y: &[f32], f0: f64, f_in: f64) -> f64 {
    let fold = |f: f64| -> f64 {
        let ny = SR / 2.0;
        let mut f = f.abs() % SR;
        if f > ny {
            f = SR - f;
        }
        f
    };
    db(goertzel(y, fold(f0 - f_in)).max(goertzel(y, fold(f0 + f_in))))
}
fn spectrum(y: &[f32]) -> Vec<(f64, f64)> {
    (1..=1000).map(|k| {
        let f = k as f64 * 20.0;
        (f, db(goertzel(y, f)))
    }).collect()
}

/// 非整数 rate 群での副生成物の最悪値 [dB]。
fn worst_pattern(x: &[f32], os: usize, half_os: usize, skip: usize) -> f64 {
    [2.5_f64, 3.5, 4.5, 8.5, 16.5]
        .iter()
        .map(|&r| sideband_db(&render(x, r, os, half_os)[skip..], SR / (2.0 * r)))
        .fold(f64::NEG_INFINITY, f64::max)
}

fn main() {
    let x = sine(N);
    let skip = N / 10;

    println!("阻止域仕様からの OS / N 導出");
    println!();
    println!("  仕様: f_s = (1 - {F_AUDIBLE}/{SR}) / OS 以上での |H(f)| 最大値を A [dB] とする。");
    println!("  受け入れ床 (裁定 C): 副生成物 <= {ACCEPT_FLOOR_DB} dB");
    println!("  レイテンシ = (half_up + half_dn) / OS = 2*half_os/OS [基底サンプル]");
    println!();
    println!("  A はカーネルの式だけで決まり、素材にも sr にも依存しない。");
    println!("  half_os は「A を満たす最小値」として従属的に決まる。");
    println!();

    // ---- 1. 阻止域減衰 A と副生成物の相関 ----
    println!("[1] 阻止域減衰 A と実測副生成物の相関 (A が仕様として機能するか)");
    println!();
    println!("  OS  half_os  lat[smp]   A[dB]   副生成物の最悪[dB]");
    let mut rows: Vec<(usize, usize, f64, f64, f64)> = Vec::new();
    for os in [2usize, 4, 8, 16] {
        for half_os in [2usize, 3, 4, 6, 8, 12, 16, 24, 32] {
            // カーネル片側が OS より短いと遷移帯域が成立しない。
            if half_os < os {
                continue;
            }
            let k = sinc_kernel(0.5 / os as f64, half_os);
            let a = stopband_db(&k, os);
            let lat = 2.0 * half_os as f64 / os as f64;
            let p = worst_pattern(&x, os, half_os, skip);
            println!("  {os:<3} {half_os:<8} {lat:>8.2} {a:>8.1} {p:>18.1}");
            rows.push((os, half_os, lat, a, p));
        }
        println!();
    }

    // ---- 2. 受け入れ床を満たす最小レイテンシ点 ----
    println!("[2] 受け入れ床 {ACCEPT_FLOOR_DB} dB を満たす (OS, half_os) のうち最小レイテンシ");
    println!();
    let mut ok: Vec<&(usize, usize, f64, f64, f64)> =
        rows.iter().filter(|r| r.4 <= ACCEPT_FLOOR_DB).collect();
    ok.sort_by(|a, b| a.2.total_cmp(&b.2));
    if ok.is_empty() {
        println!("  **該当なし。** 探索範囲を広げるか床を見直す必要がある。");
    } else {
        println!("  OS  half_os  lat[smp]  lat[ms@44.1k]   A[dB]   副生成物[dB]");
        for r in ok.iter().take(8) {
            println!(
                "  {:<3} {:<8} {:>8.2} {:>13.3} {:>8.1} {:>13.1}",
                r.0,
                r.1,
                r.2,
                r.2 / 44_100.0 * 1000.0,
                r.3,
                r.4
            );
        }
        let best = ok[0];
        println!();
        println!(
            "  → 最小レイテンシ: OS={} half_os={} で {:.2} 基底サンプル ({:.3} ms @44.1k)",
            best.0,
            best.1,
            best.2,
            best.2 / 44_100.0 * 1000.0
        );
        println!("    このとき阻止域減衰 A = {:.1} dB、副生成物 {:.1} dB", best.3, best.4);

        // ---- 3. 最小点での整数 rate のキャラクタ保存 ----
        println!();
        println!("[3] 最小レイテンシ点での整数 rate のキャラクタ保存 (床 -40 dB)");
        println!();
        for (name, src) in [("純音 220 Hz", sine(N)), ("広帯域 (倍音+雑音)", composite(N))] {
            let mut worst = (0.0_f64, 0.0_f64);
            let mut over = 0usize;
            for r in [2.0_f64, 3.0, 4.0, 6.0, 7.0, 8.0, 16.0, 32.0] {
                let a = render(&src, r, 1, 16);
                let b = render(&src, r, best.0, best.1);
                let (sa, sb) = (spectrum(&a[skip..]), spectrum(&b[skip..]));
                for ((f, va), (_, vb)) in sa.iter().zip(sb.iter()) {
                    if va.max(*vb) <= -40.0 {
                        continue;
                    }
                    let d = vb - va;
                    if d.abs() > worst.1.abs() {
                        worst = (*f, d);
                    }
                    if d.abs() > 3.0 {
                        over += 1;
                    }
                }
            }
            println!(
                "  [{name}] 最大差 {:.2} dB @ {:.0} Hz、|差|>3dB のビン数 = {over}",
                worst.1, worst.0
            );
        }
    }

    // ---- 4. A が仕様として機能するかの検証: 最悪ケース (高域信号) で測り直す ----
    // A は可聴上限の信号を想定した値なので、測定側も高域でないと相関を見られない。
    println!();
    println!("[4] A の妥当性検証: 高域信号での副生成物 (A は最悪ケースを想定した量)");
    println!("  f_in を変えて、A と副生成物が相関するかを見る。相関しなければ");
    println!("  A は仕様として使えず、仕様は副生成物の床そのものに置くしかない。");
    println!();
    println!("  OS  half_os  lat[smp]   A[dB]   f_in=220   f_in=8k   f_in=15k   f_in=19k");
    for (os, half_os) in [(2usize, 4usize), (2, 6), (2, 8), (2, 12), (2, 16), (2, 24), (2, 32)] {
        let k = sinc_kernel(0.5 / os as f64, half_os);
        let a = stopband_db(&k, os);
        let lat = 2.0 * half_os as f64 / os as f64;
        let mut cells = String::new();
        for f_in in [220.0_f64, 8_000.0, 15_000.0, 19_000.0] {
            let src = sine_at(N, f_in);
            let w = [2.5_f64, 3.5, 4.5, 8.5, 16.5]
                .iter()
                .map(|&r| {
                    sideband_db_at(&render(&src, r, os, half_os)[skip..], SR / (2.0 * r), f_in)
                })
                .fold(f64::NEG_INFINITY, f64::max);
            cells.push_str(&format!("{w:>10.1}"));
        }
        println!("  {os:<3} {half_os:<8} {lat:>8.2} {a:>8.1}{cells}");
    }

    // ---- 5. [4] の切り分け: 測定位置に「本来の成分」が重なっていないか ----
    // 整数 rate では非整数由来の副生成物は原理的に存在しない。
    // それでも fs/(2r) +- f_in に大きな値が立つなら、それは本来のエイリアスの裾であり
    // 「副生成物」として測ってはいけない。= 高域での測定手法が成立していない証拠。
    println!();
    println!("[5] 測定位置の妥当性: 整数 rate (副生成物ゼロのはず) で同じ周波数を測る");
    println!();
    println!("  f_in    整数 rate での fs/(2r)+-f_in [dB]   (r=2/4/8/16、OS=1 現行経路)");
    for f_in in [220.0_f64, 8_000.0, 15_000.0, 19_000.0] {
        let src = sine_at(N, f_in);
        let mut cells = String::new();
        for r in [2.0_f64, 4.0, 8.0, 16.0] {
            let y = render(&src, r, 1, 16);
            cells.push_str(&format!(
                "{:>10.1}",
                sideband_db_at(&y[skip..], SR / (2.0 * r), f_in)
            ));
        }
        println!("  {f_in:<7.0}{cells}");
    }
    println!();
    println!("  上が -60 dB より大きい f_in では、その周波数に本来の成分が乗っており");
    println!("  副生成物の測定に使えない。[4] の高域列はこの理由で無効。");

    // ---- 6. [4] に現行 (OS=1) の基準線を足す ----
    // 方式2 が悪化させたのか、現行から既にそうなのかを判定する。
    println!();
    println!("[6] 現行 (OS=1) を基準にした f_in 別の副生成物");
    println!();
    println!("  経路              f_in=220   f_in=8k   f_in=19k");
    for (label, os, half_os) in [
        ("現行 OS=1        ", 1usize, 16usize),
        ("方式2 OS=2 half=6", 2, 6),
        ("方式2 OS=2 half=32", 2, 32),
        ("方式2 OS=4 half=12", 4, 12),
    ] {
        let mut cells = String::new();
        for f_in in [220.0_f64, 8_000.0, 19_000.0] {
            let src = sine_at(N, f_in);
            let w = [2.5_f64, 3.5, 4.5, 8.5, 16.5]
                .iter()
                .map(|&r| {
                    sideband_db_at(&render(&src, r, os, half_os)[skip..], SR / (2.0 * r), f_in)
                })
                .fold(f64::NEG_INFINITY, f64::max);
            cells.push_str(&format!("{w:>10.1}"));
        }
        println!("  {label}{cells}");
    }

    // ---- 7. rate 集合のバイアス除去 (Grok 2026-08-27 の指摘) ----
    // これまでの測定は r が全て *.5 で、OS=2 では rate*OS が整数 = 丸め誤差ゼロ。
    // 最も都合の良い点だけを測っていた。OS 格子上でも非整数になる r を入れる。
    // f_in=220 は測定ビンが汚染されていない ([5] で確認済み) のでこの指標が有効。
    println!();
    println!("[7] rate 集合のバイアス除去 (f_in=220 Hz、副生成物 [dB])");
    println!("  rate*OS が整数になるかを併記する。整数なら丸め誤差ゼロの好条件。");
    println!();
    println!("  rate   現行OS=1   OS=2 h=6 (r*OS)   OS=4 h=12 (r*OS)   OS=8 h=24 (r*OS)");
    for r in [2.5_f64, 3.2, 2.25, 5.7, 11.3, 3.5, 7.125, 16.5, 24.4] {
        let mut cells = String::new();
        let base = sideband_db(&render(&x, r, 1, 16)[skip..], SR / (2.0 * r));
        for (os, half_os) in [(2usize, 6usize), (4, 12), (8, 24)] {
            let v = sideband_db(&render(&x, r, os, half_os)[skip..], SR / (2.0 * r));
            let ros = r * os as f64;
            let mark = if (ros - ros.round()).abs() < 1e-9 { "整数" } else { "非整数" };
            cells.push_str(&format!("{v:>10.1} ({mark:<6})"));
        }
        println!("  {r:<6.3} {base:>8.1}{cells}");
    }
}
