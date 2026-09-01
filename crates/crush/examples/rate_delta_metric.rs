//! 理想 ZOH からの差スペクトル Δ で測る。
//! **プラグイン本体には触らない。**
//!
//! これまでの指標 `fs/(2r) ± f_in` は次の 2 点で壊れていた (コミット f9256c1):
//!   - 高域入力では効果の線 `k·fs/r ± f_in` と衝突する
//!   - `rate` の小数部 0.5 に特化しており、他の小数部では別の場所に成分が出る
//!
//! 置き換え (スペクトルマスク方式):
//!   理想 = 平均周期 `T = rate/fs` の ZOH。純音入力に対する線は
//!     `f_in` と `fold(m·fs/rate ± f_in)`  (m = 1, 2, ...)、包絡は `sinc(f·T)`。
//!   **これらは効果そのものなので残す。** 実測スペクトルからこの線の近傍を除外し、
//!   残ったビンの最大値を Δ とする。段の時刻を整数格子へ丸めたことで生じる線だけが残る。
//!
//!   時間領域で「帯域制限した理想 ZOH」を引くのは誤り。それは効果のエイリアスごと
//!   除いてしまう (整数 rate でも -36 dB が出る形になり、床が立たない)。
//!
//! 量子化は外す。`bits` の効果と `rate` の効果を分離するため
//! (量子化器は本ハーネスの対象外)。
//!
//! 実行: cargo run -q -p fandango-crush --example rate_delta_metric --release

const SR: f64 = 48_000.0;
const F_IN: f64 = 220.0;
const AMP: f64 = 0.6;
/// FFT 長。解析区間はこの長さちょうどを使う。
const NFFT: usize = 32_768;
/// 過渡を避けて捨てる先頭サンプル数。
const SKIP: usize = 8_192;
/// 理想線の周りで除外するビン数 (片側)。窓の漏れを確実に外すため広めに取る。
const MASK_HALF: isize = 16;

fn fft(re: &mut [f64], im: &mut [f64], inverse: bool) {
    let n = re.len();
    assert!(n.is_power_of_two());
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
        let ang = 2.0 * std::f64::consts::PI / len as f64 * if inverse { 1.0 } else { -1.0 };
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
    if inverse {
        for v in re.iter_mut() {
            *v /= n as f64;
        }
        for v in im.iter_mut() {
            *v /= n as f64;
        }
    }
}

/// 入力。純音。非整数時刻でも解析的に評価できる。
fn x_at(t: f64) -> f64 {
    AMP * (2.0 * std::f64::consts::PI * F_IN * t / SR).sin()
}

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

/// ADAA 量子化 (本体 `crates/crush/src/dsp.rs` と同式)。
fn f_antideriv(x: f64, b: f64) -> f64 {
    let d = 2.0 / (2.0_f64.powf(b) - 1.0);
    let m = (x / d + 0.5).floor();
    d * (m * x - d * m * m * 0.5)
}
fn quant(x: f64, b: f64) -> f64 {
    let d = 2.0 / (2.0_f64.powf(b) - 1.0);
    d * (x / d + 0.5).floor()
}
fn adaa(x: f64, x_prev: f64, b: f64) -> f64 {
    let dx = x - x_prev;
    if dx.abs() > 1e-6 {
        (f_antideriv(x, b) - f_antideriv(x_prev, b)) / dx
    } else {
        quant((x + x_prev) * 0.5, b)
    }
}

fn render_sh(n: usize, rate: f64, os: usize, half_os: usize) -> Vec<f64> {
    render_sh_q(n, rate, os, half_os, None)
}

/// 現行および方式2 の S&H。`bits` を渡すと ADAA 量子化を掛ける
/// (現行は基底レート、方式2 は OS レート = デシメーションの前)。
/// `os = 1` で現行 (整数格子)、`os > 1` で OS 領域の S&H + デシメーション。
fn render_sh_q(n: usize, rate: f64, os: usize, half_os: usize, bits: Option<f64>) -> Vec<f64> {
    let k = sinc_kernel(0.5 / os as f64, half_os);
    let half = (k.len() - 1) / 2;

    // OS レートの入力列。os = 1 なら基底の入力そのもの。
    let n_os = n * os;
    let xs: Vec<f64> = (0..n_os).map(|i| x_at(i as f64 / os as f64)).collect();

    let rate_os = rate * os as f64;
    let mut acc = 0.0_f64;
    let mut hold = 0.0_f64;
    let mut ys = Vec::with_capacity(n_os);
    let mut x_prev = 0.0_f64;
    for &v in &xs {
        if acc <= 0.0 {
            hold = v;
            acc += rate_os;
        }
        acc -= 1.0;
        match bits {
            Some(b) => {
                ys.push(adaa(hold, x_prev, b));
                x_prev = hold;
            }
            None => ys.push(hold),
        }
    }

    if os == 1 {
        return ys;
    }

    // ローパス後に os 間引き。
    let mut y = Vec::with_capacity(n);
    let mut i = 0usize;
    while i < ys.len() && y.len() < n {
        let mut a = 0.0;
        for (t, kv) in k.iter().enumerate() {
            let j = i as isize + t as isize - half as isize;
            if j < 0 || j as usize >= ys.len() {
                continue;
            }
            a += ys[j as usize] * kv;
        }
        y.push(a);
        i += os;
    }
    while y.len() < n {
        y.push(0.0);
    }
    y
}

/// 理想 ZOH の線 (効果) が乗るビンの集合。`fold` して可聴帯へ落とす。
/// `m` は包絡 `sinc` の減衰で振幅が無視できる範囲まで取る。
fn ideal_bins(rate: f64, bin_hz: f64, nbin: usize) -> Vec<bool> {
    let mut mask = vec![false; nbin];
    let fold = |f: f64| -> f64 {
        let ny = SR / 2.0;
        let mut f = f.abs() % SR;
        if f > ny {
            f = SR - f;
        }
        f
    };
    let mut mark = |f: f64, mask: &mut Vec<bool>| {
        let b = (f / bin_hz).round() as isize;
        for d in -MASK_HALF..=MASK_HALF {
            let i = b + d;
            if i >= 0 && (i as usize) < nbin {
                mask[i as usize] = true;
            }
        }
    };
    mark(F_IN, &mut mask);
    mark(0.0, &mut mask);
    let f_sh = SR / rate;
    // 包絡 sinc(f*T) により振幅は 1/m で落ちる。m=200 で約 -46 dB。
    for m in 1..=200 {
        let c = m as f64 * f_sh;
        mark(fold(c - F_IN), &mut mask);
        mark(fold(c + F_IN), &mut mask);
        mark(fold(c), &mut mask);
    }
    mask
}

/// 実測スペクトルのうち**理想線に乗らないビン**の最大値 [dB]。基本波を 0 dB とする。
/// これが「段の時刻を整数格子へ丸めたことで生じた成分」= Δ。
fn delta_db(y: &[f64], rate: f64) -> (f64, f64, f64) {
    let mut re = vec![0.0; NFFT];
    let mut im = vec![0.0; NFFT];
    for i in 0..NFFT {
        // Blackman-Harris 4 項。サイドローブ -92 dB。
        // Hann (-31 dB) では基本波の漏れが Δ を覆い隠す (全条件が -40.7 dB / 224 Hz になる)。
        let u = 2.0 * std::f64::consts::PI * i as f64 / NFFT as f64;
        let w = 0.35875 - 0.48829 * u.cos() + 0.14128 * (2.0 * u).cos()
            - 0.01168 * (3.0 * u).cos();
        re[i] = y[SKIP + i] * w;
    }
    fft(&mut re, &mut im, false);

    let bin_hz = SR / NFFT as f64;
    let f0_bin = (F_IN / bin_hz).round() as usize;
    let fund = (re[f0_bin] * re[f0_bin] + im[f0_bin] * im[f0_bin]).sqrt().max(1e-30);

    let lo = (20.0 / bin_hz).ceil() as usize;
    let hi = (20_000.0 / bin_hz).floor() as usize;
    let mask = ideal_bins(rate, bin_hz, hi + 1);
    let mut worst = 0.0_f64;
    let mut worst_f = 0.0_f64;
    let mut masked = 0usize;
    for i in lo..=hi {
        if mask[i] {
            masked += 1;
            continue;
        }
        let m = (re[i] * re[i] + im[i] * im[i]).sqrt();
        if m > worst {
            worst = m;
            worst_f = i as f64 * bin_hz;
        }
    }
    let frac = masked as f64 / (hi - lo + 1) as f64 * 100.0;
    (20.0 * (worst / fund).max(1e-12).log10(), worst_f, frac)
}


/// 指定帯域だけを見た残差 [dB]。理想線マスクは同じ。
fn band_db(y: &[f64], rate: f64, f_lo: f64, f_hi: f64) -> (f64, f64) {
    let mut re = vec![0.0; NFFT];
    let mut im = vec![0.0; NFFT];
    for i in 0..NFFT {
        let u = 2.0 * std::f64::consts::PI * i as f64 / NFFT as f64;
        let w = 0.35875 - 0.48829 * u.cos() + 0.14128 * (2.0 * u).cos()
            - 0.01168 * (3.0 * u).cos();
        re[i] = y[SKIP + i] * w;
    }
    fft(&mut re, &mut im, false);
    let bin_hz = SR / NFFT as f64;
    let f0_bin = (F_IN / bin_hz).round() as usize;
    let fund = (re[f0_bin] * re[f0_bin] + im[f0_bin] * im[f0_bin]).sqrt().max(1e-30);
    let lo = (f_lo / bin_hz).ceil() as usize;
    let hi = (f_hi / bin_hz).floor() as usize;
    let mask = ideal_bins(rate, bin_hz, hi + 1);
    let (mut worst, mut wf) = (0.0_f64, 0.0_f64);
    for i in lo..=hi {
        if mask[i] {
            continue;
        }
        let m = (re[i] * re[i] + im[i] * im[i]).sqrt();
        if m > worst {
            worst = m;
            wf = i as f64 * bin_hz;
        }
    }
    (20.0 * (worst / fund).max(1e-12).log10(), wf)
}

/// RMS 包絡の変動 [dB p2p] と、包絡スペクトルのピーク周波数 [Hz]。
/// 窓 5 ms / ホップ 1 ms。当初の指標は
/// ホップ 5 ms で測って差なしとしたが、そこで測った rate は f_p が
/// 9600 Hz の *.5 系であり、包絡に出ないのは当然だった。
fn envelope(y: &[f64]) -> (f64, f64) {
    let win = (SR * 0.005) as usize;
    let hop = (SR * 0.001) as usize;
    let mut env = Vec::new();
    let mut i = SKIP;
    while i + win < y.len() {
        let s: f64 = y[i..i + win].iter().map(|v| v * v).sum();
        env.push((s / win as f64).sqrt());
        i += hop;
    }
    let mean = env.iter().sum::<f64>() / env.len() as f64;
    let (mut lo, mut hi) = (f64::MAX, f64::MIN);
    for &v in &env {
        lo = lo.min(v);
        hi = hi.max(v);
    }
    let p2p = 20.0 * (hi.max(1e-30) / lo.max(1e-30)).log10();

    // 包絡のスペクトル。DC を抜いてピーク周波数を出す。
    let m = env.len().next_power_of_two() / 2;
    let mut re = vec![0.0; m];
    let mut im = vec![0.0; m];
    for i in 0..m {
        let u = 2.0 * std::f64::consts::PI * i as f64 / m as f64;
        let w = 0.5 - 0.5 * u.cos();
        re[i] = (env[i] - mean) * w;
    }
    fft(&mut re, &mut im, false);
    let env_sr = SR / hop as f64;
    let bin = env_sr / m as f64;
    let (mut worst, mut wf) = (0.0_f64, 0.0_f64);
    for i in 1..m / 2 {
        let v = (re[i] * re[i] + im[i] * im[i]).sqrt();
        if v > worst {
            worst = v;
            wf = i as f64 * bin;
        }
    }
    (p2p, wf)
}


/// 指定周波数の振幅 [dB]。基本波を 0 dB とする。**マスクを掛けない。**
/// 近傍 +-3 ビンの最大値を取る (非整数ビンでの分散を拾うため)。
fn tone_db(y: &[f64], f_hz: f64) -> f64 {
    let mut re = vec![0.0; NFFT];
    let mut im = vec![0.0; NFFT];
    for i in 0..NFFT {
        let u = 2.0 * std::f64::consts::PI * i as f64 / NFFT as f64;
        let w = 0.35875 - 0.48829 * u.cos() + 0.14128 * (2.0 * u).cos()
            - 0.01168 * (3.0 * u).cos();
        re[i] = y[SKIP + i] * w;
    }
    fft(&mut re, &mut im, false);
    let bin_hz = SR / NFFT as f64;
    let f0 = (F_IN / bin_hz).round() as usize;
    let fund = (re[f0] * re[f0] + im[f0] * im[f0]).sqrt().max(1e-30);
    let b = (f_hz / bin_hz).round() as isize;
    let mut worst = 0.0_f64;
    for d in -3isize..=3 {
        let i = b + d;
        if i < 0 || i as usize >= NFFT / 2 {
            continue;
        }
        let i = i as usize;
        worst = worst.max((re[i] * re[i] + im[i] * im[i]).sqrt());
    }
    20.0 * (worst / fund).max(1e-12).log10()
}

fn main() {
    let n = SKIP + NFFT + 64;

    println!("理想 ZOH の線を除いた残差 Δ で測る (純音 {F_IN} Hz、量子化なし)");
    println!();
    println!("  理想線 = f_in と fold(m*fs/rate ± f_in) (m=1..200)。**効果なので残す**");
    println!("  値は理想線を除いたビンの最大値 [dB]、基本波を 0 dB とする");
    println!("  masked% = 除外したビンの割合。高すぎると Δ を見逃すので併記する");
    println!();

    println!("[0] 床の確認: 整数 rate (段の時刻誤差ゼロ)");
    println!("  ここに出る量は測定系の漏れであり、どの方式でも下回れない。");
    println!();
    println!("  rate    Δ最大[dB]   位置[Hz]  masked%");
    for r in [2.0_f64, 3.0, 4.0, 8.0, 16.0, 32.0] {
        let y = render_sh(n, r, 1, 16);
        let (d, f, m) = delta_db(&y, r);
        println!("  {r:<6.1} {d:>10.1} {f:>10.0} {m:>8.1}");
    }

    println!();
    println!("[1] 非整数 rate。rate*OS が整数になるかを併記 (整/非)");
    println!("  方式2 はいずれも lat 6 基底サンプル。");
    println!();
    println!("  rate    現行OS=1   OS=2 h=6      OS=4 h=12     OS=8 h=24    masked%");
    for r in [2.5_f64, 2.25, 3.2, 5.7, 7.125, 11.3, 16.5, 24.4] {
        let cur = render_sh(n, r, 1, 16);
        let (d0, _, mk) = delta_db(&cur, r);
        let mut cells = String::new();
        for (os, half_os) in [(2usize, 6usize), (4, 12), (8, 24)] {
            let y = render_sh(n, r, os, half_os);
            let (d, _, _) = delta_db(&y, r);
            let ros = r * os as f64;
            let mark = if (ros - ros.round()).abs() < 1e-9 { "整" } else { "非" };
            cells.push_str(&format!("{d:>9.1} ({mark})"));
        }
        println!("  {r:<6.3} {d0:>9.1}{cells} {mk:>8.1}");
    }

    println!();
    println!("[2] カーネル長を伸ばした場合 (OS=4 固定)");
    println!();
    println!("  rate    現行     h=6      h=12     h=24     h=48");
    for r in [2.25_f64, 7.125, 3.2, 5.7, 2.5, 16.5] {
        let cur = render_sh(n, r, 1, 16);
        let (d0, _, _) = delta_db(&cur, r);
        let mut cells = String::new();
        for half_os in [6usize, 12, 24, 48] {
            let y = render_sh(n, r, 4, half_os);
            let (d, _, _) = delta_db(&y, r);
            cells.push_str(&format!("{d:>9.1}"));
        }
        println!("  {r:<6.3} {d0:>7.1}{cells}");
    }

    println!();
    println!("[3] **量子化を入れた場合** (bits=8)。マスクは S&H の線のみ。");
    println!("  整数 rate = 段の時刻誤差ゼロ。ここを床として、非整数で増える分が");
    println!("  「時刻誤差 x 量子化」の相互作用による欠陥。");
    println!();
    println!("  rate    現行OS=1   OS=2 h=6      OS=4 h=12     OS=8 h=24");
    for r in [2.0_f64, 4.0, 8.0, 16.0, 2.5, 2.25, 3.2, 5.7, 7.125, 11.3, 16.5, 24.4] {
        let cur = render_sh_q(n, r, 1, 16, Some(8.0));
        let (d0, _, _) = delta_db(&cur, r);
        let mut cells = String::new();
        for (os, half_os) in [(2usize, 6usize), (4, 12), (8, 24)] {
            let y = render_sh_q(n, r, os, half_os, Some(8.0));
            let (d, _, _) = delta_db(&y, r);
            let ros = r * os as f64;
            let mark = if (ros - ros.round()).abs() < 1e-9 { "整" } else { "非" };
            cells.push_str(&format!("{d:>9.1} ({mark})"));
        }
        let tag = if (r - r.round()).abs() < 1e-9 { "  <- 整数 rate (床)" } else { "" };
        println!("  {r:<6.3} {d0:>9.1}{cells}{tag}");
    }

    // ---- [4] 整数近傍の rate。オーナーの原初の報告「LFO のような音」の検証 ----
    // 段は n か n+1 サンプル間隔のどちらかで、n+1 が現れる割合は delta = frac(rate)。
    // パターンの繰り返し周期は rate/delta サンプル、周波数は fs*delta/rate。
    // **rate が整数に近いほどこの周波数は下がり、LFO 域へ入る。**
    // FloatParam にすればユーザーは必ずこの領域を通る。
    println!();
    println!("[4] 整数近傍の rate (原初の報告「LFO のような音」の検証)");
    println!();
    println!("  f_p = fs*delta/rate [Hz] がパターンの予測周波数。");
    println!("  OS を掛けると delta_os = frac(rate*OS) となり f_p は OS 倍へ押し上がる。");
    println!();
    println!("  rate      f_p予測   現行OS=1 [dB @Hz]      OS=4 h=12 [dB @Hz]");
    for r in [2.001_f64, 2.005, 2.01, 2.05, 2.1, 2.25, 2.5, 8.001, 8.005, 8.05, 16.002] {
        let d = r - r.floor();
        let f_p = SR * d / r;
        let cur = render_sh_q(n, r, 1, 16, Some(8.0));
        let (d0, f0, _) = delta_db(&cur, r);
        let y = render_sh_q(n, r, 4, 12, Some(8.0));
        let (d1, f1, _) = delta_db(&y, r);
        println!("  {r:<9.4} {f_p:>8.1} {d0:>10.1} @{f0:<9.0} {d1:>10.1} @{f1:<9.0}");
    }

    println!();
    println!("[5] 同上、量子化なし (LFO 成分が S&H 単体で出るか)");
    println!();
    println!("  rate      f_p予測   現行OS=1 [dB @Hz]      OS=4 h=12 [dB @Hz]");
    for r in [2.001_f64, 2.005, 2.05, 2.5, 8.005] {
        let d = r - r.floor();
        let f_p = SR * d / r;
        let cur = render_sh(n, r, 1, 16);
        let (d0, f0, _) = delta_db(&cur, r);
        let y = render_sh(n, r, 4, 12);
        let (d1, f1, _) = delta_db(&y, r);
        println!("  {r:<9.4} {f_p:>8.1} {d0:>10.1} @{f0:<9.0} {d1:>10.1} @{f1:<9.0}");
    }

    println!();
    println!("[6] 低域限定 (20-500 Hz) の残差と RMS 包絡の変動");
    println!("  §4-1 は包絡ホップ 5 ms・rate が *.5 系 (f_p = 9600 Hz) で測っており、");
    println!("  包絡に出ないのは当然だった。整数近傍を窓 5 ms / ホップ 1 ms で測り直す。");
    println!();
    println!("  rate      f_p予測  低域残差[dB @Hz]      包絡p2p[dB]  包絡ピーク[Hz]");
    for r in [2.0_f64, 2.001, 2.005, 2.01, 2.05, 2.1, 2.5, 8.0, 8.001, 8.005, 8.05, 16.002] {
        let d = r - r.floor();
        let f_p = SR * d / r;
        let cur = render_sh_q(n, r, 1, 16, Some(8.0));
        let (db, f) = band_db(&cur, r, 20.0, 500.0);
        let (p2p, pf) = envelope(&cur);
        println!("  {r:<9.4} {f_p:>8.1} {db:>9.1} @{f:<8.0} {p2p:>11.2} {pf:>13.1}");
    }

    println!();
    println!("[7] 同上を方式2 (OS=4 h=12) で。f_p は OS 倍へ押し上がるはず");
    println!();
    println!("  rate      f_p予測  f_p(OS=4)  低域残差[dB @Hz]      包絡p2p[dB]  包絡ピーク[Hz]");
    for r in [2.001_f64, 2.005, 2.01, 2.05, 8.001, 8.005, 16.002] {
        let d = r - r.floor();
        let f_p = SR * d / r;
        let ros = r * 4.0;
        let dos = ros - ros.floor();
        let f_pos = SR * 4.0 * dos / ros;
        let y = render_sh_q(n, r, 4, 12, Some(8.0));
        let (db, f) = band_db(&y, r, 20.0, 500.0);
        let (p2p, pf) = envelope(&y);
        println!("  {r:<9.4} {f_p:>8.1} {f_pos:>10.1} {db:>9.1} @{f:<8.0} {p2p:>11.2} {pf:>13.1}");
    }

    println!();
    println!("[8] f_p の振幅を直接測る (マスクなし)。方式2 で消えるかを見る");
    println!();
    println!("  f_p = fold(round(r)*fs/r) = fs*delta/r。代数として恒等 (検算済み)。");
    println!("  OS を掛けたときの f_p(OS) = fold(round(r*OS)*fs*OS/(r*OS)) も併記。");
    println!("  量子化あり (bits=8)。");
    println!();
    println!("  rate      f_p[Hz]  現行[dB]  OS=2h6[dB]  OS=4h12[dB]  OS=8h24[dB]");
    for r in [2.001_f64, 2.005, 2.01, 2.05, 2.1, 2.25, 2.5, 8.001, 8.005, 8.05, 16.002, 16.5] {
        let n_r = r.round();
        let f_p = {
            let c = n_r * SR / r;
            let ny = SR / 2.0;
            let mut f = c.abs() % SR;
            if f > ny {
                f = SR - f;
            }
            f
        };
        let cur = render_sh_q(n, r, 1, 16, Some(8.0));
        let a0 = tone_db(&cur, f_p);
        let mut cells = String::new();
        for (os, half_os) in [(2usize, 6usize), (4, 12), (8, 24)] {
            let y = render_sh_q(n, r, os, half_os, Some(8.0));
            cells.push_str(&format!("{:>12.1}", tone_db(&y, f_p)));
        }
        println!("  {r:<9.4} {f_p:>8.1} {a0:>9.1}{cells}");
    }

    println!();
    println!("[9] 同上、量子化なし (S&H 単体で f_p が立つか)");
    println!();
    println!("  rate      f_p[Hz]  現行[dB]  OS=2h6[dB]  OS=4h12[dB]  OS=8h24[dB]");
    for r in [2.001_f64, 2.005, 2.05, 2.5, 8.005, 16.002] {
        let n_r = r.round();
        let f_p = {
            let c = n_r * SR / r;
            let ny = SR / 2.0;
            let mut f = c.abs() % SR;
            if f > ny {
                f = SR - f;
            }
            f
        };
        let cur = render_sh(n, r, 1, 16);
        let a0 = tone_db(&cur, f_p);
        let mut cells = String::new();
        for (os, half_os) in [(2usize, 6usize), (4, 12), (8, 24)] {
            let y = render_sh(n, r, os, half_os);
            cells.push_str(&format!("{:>12.1}", tone_db(&y, f_p)));
        }
        println!("  {r:<9.4} {f_p:>8.1} {a0:>9.1}{cells}");
    }
}
