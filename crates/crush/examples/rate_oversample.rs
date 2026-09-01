//! オーバーサンプリング + デシメーションが crush の音に何をするかの検証
//! **プラグイン本体には触らない。**
//!
//! 問い 2 つ:
//!   Q1. 非整数 `rate` のパターン成分は OS で落ちるか (落ちるなら §5-2 の機構と整合)。
//!   Q2. **整数 `rate` の「欲しい方」のエイリアスまで消えないか。**
//!       S&H の像は `k*fs/r` に立ち、現行は `fs/2` 超が折り返して可聴帯へ降りてくる。
//!       OS 領域では折り返さず、デシメーションフィルタが捨てるので**降りてこない**。
//!       これが起きるなら「修正の顔をした別物」であり §2 裁定 2 へ戻す。
//!
//! 実行: cargo run -p fandango-crush --example rate_oversample --release

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

/// 広帯域の検証入力。純音 1 本だけで結論を出さないため。
/// 非整数比の倍音 + 白色雑音（`rate_baseline.rs` の基準入力と同式）。
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

fn sine(n: usize) -> Vec<f32> {
    (0..n).map(|i| (0.6 * (2.0 * std::f64::consts::PI * F_IN * i as f64 / SR).sin()) as f32).collect()
}

/// 窓付き sinc のローパス核。`cutoff` は**出力レートに対する正規化周波数** (0..0.5)。
/// `half` は片側タップ数。Blackman 窓。
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

/// ゼロ詰め + ローパスで `os` 倍アップサンプル。
fn upsample(x: &[f32], os: usize, k: &[f64]) -> Vec<f32> {
    let n = x.len() * os;
    let half = (k.len() - 1) / 2;
    let mut y = vec![0.0_f32; n];
    for (i, o) in y.iter_mut().enumerate() {
        let mut acc = 0.0;
        // ゼロ詰め列 z[j] = x[j/os] (j%os==0)。畳み込みは非ゼロ位置だけ回す。
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

/// ローパス後に `os` 間引き。
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

/// `os = 1` で現行と同じ経路 (整数グリッド S&H)、`os > 1` で OS 領域の S&H。
/// **`mix` は 1.0 固定。** dry を混ぜると遅延補正の話が別途要り、本検証の問いから外れる。
fn render(x: &[f32], rate: f64, os: usize, half_os: usize) -> Vec<f32> {
    render2(x, rate, os, half_os, half_os)
}

/// up / down のカーネル長を別々に指定する版。抑圧が down 側の仕事か切り分けるため。
fn render2(x: &[f32], rate: f64, os: usize, half_up: usize, half_dn: usize) -> Vec<f32> {
    let ku = sinc_kernel(0.5 / os as f64, half_up);
    let k = sinc_kernel(0.5 / os as f64, half_dn);
    let xs = if os == 1 { x.to_vec() } else { upsample(x, os, &ku) };

    // S&H とクラッシュは OS レートで回す。ホールド長は rate*os OS-サンプル。
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

    // DC ブロッカーは基底レートで掛ける (現行と同じ位置)。
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

/// `f0 ± F_IN` のうち大きい方 [dB]。`f0` が Nyquist を超える場合は折り返し先で測る。
fn sideband_db(y: &[f32], f0: f64) -> f64 {
    let fold = |f: f64| -> f64 {
        let ny = SR / 2.0;
        let mut f = f.abs() % SR;
        if f > ny {
            f = SR - f;
        }
        f
    };
    db(goertzel(y, fold(f0 - F_IN)).max(goertzel(y, fold(f0 + F_IN))))
}

/// 20 Hz 刻みのスペクトル (20 Hz .. 20 kHz)。
fn spectrum(y: &[f32]) -> Vec<(f64, f64)> {
    (1..=1000).map(|k| {
        let f = k as f64 * 20.0;
        (f, db(goertzel(y, f)))
    }).collect()
}

fn main() {
    let x = sine(N);
    let skip = N / 10;

    println!("OS + デシメーションの影響 (純音 {F_IN} Hz, bits={BITS}, mix=1)");
    println!();
    println!("  基本波 = {F_IN} Hz / 本来 = fs/r ± {F_IN} (S&H 本来のエイリアス)");
    println!("  パターン = fs/(2r) ± {F_IN} (非整数ホールドの副生成物)");
    println!("  像2 = 2*fs/r ± {F_IN} (現行では折り返して可聴帯へ降りてくる成分)");
    println!();
    println!("  rate  OS   基本波[dB]  本来[dB]  パターン[dB]   像2[dB]");

    for r in [4.0_f64, 4.5, 8.0, 8.5] {
        for os in [1usize, 4, 16] {
            let y = render(&x, r, os, 16 * os);
            let y = &y[skip..];
            println!(
                "  {r:<5.1} {os:<4} {:>10.1} {:>9.1} {:>12.1} {:>9.1}",
                db(goertzel(y, F_IN)),
                sideband_db(y, SR / r),
                sideband_db(y, SR / (2.0 * r)),
                sideband_db(y, 2.0 * SR / r),
            );
        }
        println!();
    }

    println!("整数 rate での広帯域比較 (OS=1 現行 vs OS=16、20 Hz 刻み 20 Hz..20 kHz)");
    println!("  しきい値ごとに集計する。低い床のビンは差が大きく出ても可聴に効かないため。");
    println!("  位相・遅延は比較しない (振幅のみ)。");
    println!();
    for (name, src) in [("純音 220 Hz", sine(N)), ("広帯域 (倍音+雑音)", composite(N))] {
    println!();
    println!("  [{name}]");
    println!("  rate  床[dB]  ビン数  最大差[dB]  位置[Hz]  中央値|差|  |差|>3dB");
    for r in [2.0_f64, 3.0, 4.0, 6.0, 7.0, 8.0, 16.0, 32.0] {
        let a = render(&src, r, 1, 16);
        let b = render(&src, r, 16, 16 * 16);
        let (sa, sb) = (spectrum(&a[skip..]), spectrum(&b[skip..]));
        for floor in [-100.0_f64, -80.0, -60.0, -40.0] {
            let diffs: Vec<(f64, f64)> = sa
                .iter()
                .zip(sb.iter())
                .filter(|((_, va), (_, vb))| va.max(*vb) > floor)
                .map(|((f, va), (_, vb))| (*f, vb - va))
                .collect();
            if diffs.is_empty() {
                println!("  {r:<5.0} {floor:>6.0}       0           -         -           -         -");
                continue;
            }
            let mut sorted: Vec<f64> = diffs.iter().map(|(_, d)| d.abs()).collect();
            sorted.sort_by(f64::total_cmp);
            let med = sorted[sorted.len() / 2];
            let (fmax, dmax) = diffs
                .iter()
                .cloned()
                .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
                .unwrap();
            let over = diffs.iter().filter(|(_, d)| d.abs() > 3.0).count();
            println!(
                "  {r:<5.0} {floor:>6.0} {:>7} {dmax:>11.1} {fmax:>9.0} {med:>11.2} {over:>9}",
                diffs.len()
            );
        }
        println!();
    }
    }

    // ---- §7-1: カーネル長 <-> 抑圧量 <-> レイテンシ の曲線 ----
    // レイテンシは up 側 half_os/os + down 側 half_os/os = 2*half_os/os 基底サンプル。
    println!();
    println!("§7-1 カーネル長スイープ (OS=4 固定, 純音 220 Hz, bits={BITS}, mix=1)");
    println!("  lat = 2*half_os/os [基底サンプル] (up + down)。44.1k 換算 ms も併記。");
    println!("  受け入れ暫定基準: パターン <= -70 dB");
    println!();
    println!("  half_os  lat[smp]  lat[ms]   r=2.5    r=3.5    r=4.5    r=8.5   r=16.5");
    let os = 4usize;
    for half_os in [1usize, 2, 3, 4, 6, 8, 12, 16, 24, 32, 64] {
        let lat = 2.0 * half_os as f64 / os as f64;
        let mut cells = String::new();
        for r in [2.5_f64, 3.5, 4.5, 8.5, 16.5] {
            let y = render(&x, r, os, half_os);
            let p = sideband_db(&y[skip..], SR / (2.0 * r));
            cells.push_str(&format!("{p:>9.1}"));
        }
        println!("  {half_os:<8} {lat:>8.1} {:>8.3}{cells}", lat / 44_100.0 * 1000.0);
    }

    println!();
    println!("§7-1 整数 rate のキャラクタ保存 (OS=1 現行 vs OS=4/half_os、床 -40 dB)");
    println!("  受け入れ暫定基準: |差| <= 3 dB のビンがゼロ");
    println!();
    for (name, src) in [("純音 220 Hz", sine(N)), ("広帯域 (倍音+雑音)", composite(N))] {
        println!("  [{name}]");
        println!("  half_os  lat[smp]  最大差[dB]  位置[Hz]  中央値|差|  |差|>3dB");
        for half_os in [2usize, 4, 8, 16, 32, 64] {
            let lat = 2.0 * half_os as f64 / os as f64;
            let mut worst = (0.0_f64, 0.0_f64);
            let mut meds: Vec<f64> = Vec::new();
            let mut over = 0usize;
            for r in [2.0_f64, 3.0, 4.0, 6.0, 7.0, 8.0, 16.0, 32.0] {
                let a = render(&src, r, 1, 16);
                let b = render(&src, r, os, half_os);
                let (sa, sb) = (spectrum(&a[skip..]), spectrum(&b[skip..]));
                let diffs: Vec<(f64, f64)> = sa
                    .iter()
                    .zip(sb.iter())
                    .filter(|((_, va), (_, vb))| va.max(*vb) > -40.0)
                    .map(|((f, va), (_, vb))| (*f, vb - va))
                    .collect();
                if diffs.is_empty() {
                    continue;
                }
                let mut sorted: Vec<f64> = diffs.iter().map(|(_, d)| d.abs()).collect();
                sorted.sort_by(f64::total_cmp);
                meds.push(sorted[sorted.len() / 2]);
                over += diffs.iter().filter(|(_, d)| d.abs() > 3.0).count();
                let (fm, dm) = diffs
                    .iter()
                    .cloned()
                    .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
                    .unwrap();
                if dm.abs() > worst.1.abs() {
                    worst = (fm, dm);
                }
            }
            meds.sort_by(f64::total_cmp);
            let med = meds[meds.len() / 2];
            println!(
                "  {half_os:<8} {lat:>8.1} {:>11.1} {:>9.0} {med:>11.2} {over:>9}",
                worst.1, worst.0
            );
        }
        println!();
    }

    println!();
    println!("§7-1b up/down 非対称スイープ (OS=4, 純音 220 Hz)。lat = (half_up+half_dn)/os");
    println!();
    println!("  h_up  h_dn  lat[smp]   r=2.5    r=3.5    r=4.5    r=8.5   r=16.5");
    for (hu, hd) in [(2usize, 12usize), (4, 12), (8, 12), (12, 12), (2, 16), (4, 16), (12, 4), (12, 8), (16, 2)] {
        let lat = (hu + hd) as f64 / os as f64;
        let mut cells = String::new();
        for r in [2.5_f64, 3.5, 4.5, 8.5, 16.5] {
            let y = render2(&x, r, os, hu, hd);
            cells.push_str(&format!("{:>9.1}", sideband_db(&y[skip..], SR / (2.0 * r))));
        }
        println!("  {hu:<5} {hd:<5} {lat:>8.1}{cells}");
    }
}
