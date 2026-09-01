//! §3-1 の実測 (`rate_artifact.rs`、Goertzel) と Δ 指標 (`rate_delta_metric.rs`、FFT) の
//! 18 dB 乖離を潰す。
//! **プラグイン本体には触らない。**
//!
//! 乖離の候補は 4 つ。1 つずつ振って、どれが効いているかを特定する。
//!   (a) 処理の型: f32 チェーン (本体と同じ) vs f64
//!   (b) DC ブロッカー・出力ガードの有無
//!   (c) 測定器: Goertzel vs FFT
//!   (d) 窓: 矩形 vs Blackman-Harris、および解析長 48000 vs 32768
//!
//! すべて `r = 2.5`、`bits = 8`、純音 220 Hz、振幅 0.6、sr 48 kHz。
//! 測る周波数は `fs/(2r) - F_IN = 9380` と `fs/(2r) + F_IN = 9820`、および基本波 220。
//!
//! 実行: cargo run -q -p fandango-crush --example rate_metric_crosscheck --release

use fandango_crush::dsp::DcBlocker;

const SR: f64 = 48_000.0;
const BITS: f64 = 8.0;
const F_IN: f64 = 220.0;
const AMP: f64 = 0.6;
const RATE: f64 = 2.5;

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

/// f32 チェーン。`rate_artifact.rs` / 本体と同じ型と順序。
fn render_f32(n: usize, rate: f64, dc_on: bool) -> Vec<f64> {
    let mut acc = 0.0_f64;
    let mut hold = 0.0_f32;
    let mut x_prev = 0.0_f64;
    let mut dc = DcBlocker::new();
    dc.set_sample_rate(SR as f32);
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let x = (AMP * (2.0 * std::f64::consts::PI * F_IN * i as f64 / SR).sin()) as f32;
        let xd = if x.is_finite() { x.clamp(-4.0, 4.0) } else { 0.0 };
        if acc <= 0.0 {
            hold = xd;
            acc += rate;
        }
        acc -= 1.0;
        let xf = hold as f64;
        let dx = xf - x_prev;
        let q = if dx.abs() > ADAA_EPS {
            ((f_antideriv(xf, BITS) - f_antideriv(x_prev, BITS)) / dx) as f32
        } else {
            quant((xf + x_prev) * 0.5, BITS) as f32
        };
        x_prev = xf;
        let mut y = if q.abs() < 1e-30 { 0.0 } else { q };
        if dc_on {
            y = dc.process(y);
            y = if y.is_finite() { y.clamp(-6.0, 6.0) } else { 0.0 };
        }
        out.push(y as f64);
    }
    out
}

/// f64 チェーン。`rate_delta_metric.rs` と同じ。
fn render_f64(n: usize, rate: f64) -> Vec<f64> {
    let mut acc = 0.0_f64;
    let mut hold = 0.0_f64;
    let mut x_prev = 0.0_f64;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let x = AMP * (2.0 * std::f64::consts::PI * F_IN * i as f64 / SR).sin();
        if acc <= 0.0 {
            hold = x;
            acc += rate;
        }
        acc -= 1.0;
        let dx = hold - x_prev;
        let q = if dx.abs() > ADAA_EPS {
            (f_antideriv(hold, BITS) - f_antideriv(x_prev, BITS)) / dx
        } else {
            quant((hold + x_prev) * 0.5, BITS)
        };
        x_prev = hold;
        out.push(q);
    }
    out
}

/// `rate_artifact.rs` と同一式の Goertzel。振幅を返す。
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

fn fft(re: &mut [f64], im: &mut [f64]) {
    fft_dir(re, im, false)
}

fn fft_dir(re: &mut [f64], im: &mut [f64], inverse: bool) {
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

/// FFT で `f_hz` の振幅を測る。`bh = true` で Blackman-Harris、false で矩形窓。
/// 近傍 ±3 ビンの最大値。窓のコヒーレントゲインで正規化して振幅を返す。
fn fft_amp(y: &[f64], off: usize, nfft: usize, f_hz: f64, bh: bool) -> f64 {
    let mut re = vec![0.0; nfft];
    let mut im = vec![0.0; nfft];
    let mut cg = 0.0;
    for i in 0..nfft {
        let w = if bh {
            let u = 2.0 * std::f64::consts::PI * i as f64 / nfft as f64;
            0.35875 - 0.48829 * u.cos() + 0.14128 * (2.0 * u).cos() - 0.01168 * (3.0 * u).cos()
        } else {
            1.0
        };
        cg += w;
        re[i] = y[off + i] * w;
    }
    cg /= nfft as f64;
    fft(&mut re, &mut im);
    let bin_hz = SR / nfft as f64;
    let b = (f_hz / bin_hz).round() as isize;
    let mut worst = 0.0_f64;
    for d in -3isize..=3 {
        let i = b + d;
        if i < 0 || i as usize >= nfft / 2 {
            continue;
        }
        let i = i as usize;
        worst = worst.max((re[i] * re[i] + im[i] * im[i]).sqrt());
    }
    worst / nfft as f64 * 2.0 / cg
}

fn db(x: f64) -> f64 {
    20.0 * x.max(1e-15).log10()
}


/// 理想 ZOH のサンプル列。段を `k*rate` へ OS 分解能で置き、
/// **ローパスを掛けずに単純間引き**する (エイリアスを保存 = 効果を残す)。
/// `rate*os` が整数なら段の時刻誤差はゼロ。
fn render_ideal(n: usize, rate: f64, os: usize, quantize: bool) -> Vec<f64> {
    let n_os = n * os;
    let rate_os = rate * os as f64;
    let mut acc = 0.0_f64;
    let mut hold = 0.0_f64;
    let mut x_prev = 0.0_f64;
    let mut out = Vec::with_capacity(n);
    for i in 0..n_os {
        let x = AMP * (2.0 * std::f64::consts::PI * F_IN * (i as f64 / os as f64) / SR).sin();
        if acc <= 0.0 {
            hold = x;
            acc += rate_os;
        }
        acc -= 1.0;
        let v = if quantize {
            let dx = hold - x_prev;
            let q = if dx.abs() > ADAA_EPS {
                (f_antideriv(hold, BITS) - f_antideriv(x_prev, BITS)) / dx
            } else {
                quant((hold + x_prev) * 0.5, BITS)
            };
            x_prev = hold;
            q
        } else {
            hold
        };
        // ローパスなしの単純間引き。
        if i % os == 0 {
            out.push(v);
        }
    }
    out
}


// ---- 方式2: OS + 窓付き sinc デシメーション ----

fn sinc_kernel(cutoff: f64, half: usize) -> Vec<f64> {
    let n = 2 * half + 1;
    let mut k = vec![0.0; n];
    let mut sum = 0.0;
    for (i, v) in k.iter_mut().enumerate() {
        let t = i as f64 - half as f64;
        let sc = if t.abs() < 1e-12 {
            2.0 * cutoff
        } else {
            (2.0 * std::f64::consts::PI * cutoff * t).sin() / (std::f64::consts::PI * t)
        };
        let u = i as f64 / (n - 1) as f64;
        let w = 0.42 - 0.5 * (2.0 * std::f64::consts::PI * u).cos()
            + 0.08 * (4.0 * std::f64::consts::PI * u).cos();
        *v = sc * w;
        sum += *v;
    }
    for v in k.iter_mut() {
        *v /= sum;
    }
    k
}

fn render_os(n: usize, rate: f64, os: usize, half_os: usize) -> Vec<f64> {
    let k = sinc_kernel(0.5 / os as f64, half_os);
    let half = (k.len() - 1) / 2;
    let n_os = n * os;
    let rate_os = rate * os as f64;
    let mut acc = 0.0_f64;
    let mut hold = 0.0_f64;
    let mut x_prev = 0.0_f64;
    let mut ys = Vec::with_capacity(n_os);
    for i in 0..n_os {
        let x = AMP * (2.0 * std::f64::consts::PI * F_IN * (i as f64 / os as f64) / SR).sin();
        if acc <= 0.0 {
            hold = x;
            acc += rate_os;
        }
        acc -= 1.0;
        let dx = hold - x_prev;
        let q = if dx.abs() > ADAA_EPS {
            (f_antideriv(hold, BITS) - f_antideriv(x_prev, BITS)) / dx
        } else {
            quant((hold + x_prev) * 0.5, BITS)
        };
        x_prev = hold;
        ys.push(q);
    }
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

// ---- 方式1: minBLEP 差分形 (rate_minblep.rs から移植) ----

fn blit(half: usize, l: usize) -> Vec<f64> {
    let n = 2 * half * l + 1;
    let mut k = vec![0.0; n];
    let mut sum = 0.0;
    for (i, v) in k.iter_mut().enumerate() {
        let t = (i as f64 - (half * l) as f64) / l as f64;
        let sc = if t.abs() < 1e-12 {
            1.0
        } else {
            (std::f64::consts::PI * t).sin() / (std::f64::consts::PI * t)
        };
        let u = i as f64 / (n - 1) as f64;
        let w = 0.42 - 0.5 * (2.0 * std::f64::consts::PI * u).cos()
            + 0.08 * (4.0 * std::f64::consts::PI * u).cos();
        *v = sc * w;
        sum += *v;
    }
    for v in k.iter_mut() {
        *v /= sum;
    }
    k
}

fn min_phase(h: &[f64], m: usize) -> Vec<f64> {
    let mut re = vec![0.0; m];
    let mut im = vec![0.0; m];
    re[..h.len()].copy_from_slice(h);
    fft_dir(&mut re, &mut im, false);
    for i in 0..m {
        let mag = (re[i] * re[i] + im[i] * im[i]).sqrt();
        re[i] = mag.max(1e-14).ln();
        im[i] = 0.0;
    }
    fft_dir(&mut re, &mut im, true);
    for i in 1..m / 2 {
        re[i] *= 2.0;
        im[i] *= 2.0;
    }
    for i in m / 2 + 1..m {
        re[i] = 0.0;
        im[i] = 0.0;
    }
    fft_dir(&mut re, &mut im, false);
    for i in 0..m {
        let e = re[i].exp();
        let (c, sn) = (im[i].cos(), im[i].sin());
        re[i] = e * c;
        im[i] = e * sn;
    }
    fft_dir(&mut re, &mut im, true);
    re[..h.len()].to_vec()
}

struct StepTable {
    s: Vec<f64>,
    l: usize,
}
impl StepTable {
    fn new(half: usize, l: usize) -> Self {
        let h = min_phase(&blit(half, l), (2 * half * l + 1).next_power_of_two() * 8);
        let mut s = Vec::with_capacity(h.len());
        let mut acc = 0.0;
        for v in &h {
            acc += v;
            s.push(acc);
        }
        let tail = *s.last().unwrap();
        for v in s.iter_mut() {
            *v /= tail;
        }
        Self { s, l }
    }
    fn group_delay(&self) -> f64 {
        for j in 1..self.s.len() {
            if self.s[j - 1] < 0.5 && self.s[j] >= 0.5 {
                return j as f64 / self.l as f64;
            }
        }
        panic!("step table が 0.5 を横切らない");
    }
    fn at(&self, tau: f64) -> f64 {
        if tau <= 0.0 {
            return 0.0;
        }
        let x = tau * self.l as f64;
        let i = x.floor() as usize;
        if i + 1 >= self.s.len() {
            return 1.0;
        }
        let f = x - i as f64;
        self.s[i] * (1.0 - f) + self.s[i + 1] * f
    }
}

/// 方式1。補正は ADAA 量子化の**後**、naive は群遅延 D だけ遅らせる
/// (rate_minblep.rs で最良だった構成)。
fn render_minblep(n: usize, rate: f64, half: usize) -> Vec<f64> {
    let tab = StepTable::new(half, 64);
    let d = tab.group_delay().round() as usize;
    let tail = 4 * half;

    let mut acc = 0.0_f64;
    let mut hold = 0.0_f64;
    let mut x_prev = 0.0_f64;
    let mut sh: Vec<f64> = Vec::with_capacity(n);
    let mut steps: Vec<(usize, f64)> = Vec::new();
    let mut t_true = 0.0_f64;
    for i in 0..n {
        let x = AMP * (2.0 * std::f64::consts::PI * F_IN * i as f64 / SR).sin();
        let k = sh.len();
        if acc <= 0.0 {
            steps.push((k, k as f64 - t_true));
            hold = x;
            acc += rate;
            t_true += rate;
        }
        acc -= 1.0;
        sh.push(hold);
    }

    let mut y: Vec<f64> = Vec::with_capacity(n);
    for k in 0..sh.len() {
        let base = if k >= d { sh[k - d] } else { 0.0 };
        let dx = base - x_prev;
        let q = if dx.abs() > ADAA_EPS {
            (f_antideriv(base, BITS) - f_antideriv(x_prev, BITS)) / dx
        } else {
            quant((base + x_prev) * 0.5, BITS)
        };
        x_prev = base;
        y.push(q);
    }

    let mut pc = vec![0.0_f64; y.len() + tail + 2];
    for &(nn, delta) in &steps {
        let p = nn + d;
        if p == 0 || p + 1 >= y.len() {
            continue;
        }
        let h = y[p + 1] - y[p - 1];
        let end = (nn + tail).min(pc.len() - 1);
        for k in nn..end {
            let tau = (k - nn) as f64;
            pc[k] += h * (tab.at(tau + delta) - tab.at(tau));
        }
    }
    for (k, v) in y.iter_mut().enumerate() {
        *v += pc[k];
    }
    y
}

/// `rate * os` が整数になる最小の os (>= 64)。理想 ZOH の段の時刻誤差をゼロにする。
fn ideal_os(rate: f64) -> usize {
    for os in 64..=2048 {
        let v = rate * os as f64;
        if (v - v.round()).abs() < 1e-9 {
            return os;
        }
    }
    2048
}

fn main() {
    let f_lo = SR / (2.0 * RATE) - F_IN; // 9380
    let f_hi = SR / (2.0 * RATE) + F_IN; // 9820
    println!("§3-1 (Goertzel) と Δ 指標 (FFT) の乖離の切り分け");
    println!();
    println!("  rate = {RATE}, bits = {BITS}, 純音 {F_IN} Hz 振幅 {AMP}, sr = {SR}");
    println!("  測る周波数: {f_lo} / {f_hi} Hz (= fs/(2r) ± f_in) と基本波 {F_IN} Hz");
    println!("  値はすべて**絶対振幅の dB**。§3-1 の表記に合わせる。");
    println!();

    // 解析長は §3-1 と同じ 48000。FFT 用に 32768 も用意する。
    let n48 = 48_000usize;
    let n_long = 4_800 + 48_000 + 64;

    println!("[A] 処理チェーンの差 (測定器は Goertzel、解析長 48000、先頭 4800 捨て)");
    println!();
    println!("  チェーン                    220Hz      9380Hz     9820Hz   大きい方");
    for (label, y) in [
        ("f32 + DCブロッカー (本体相当)", render_f32(n48, RATE, true)),
        ("f32 + DCなし                 ", render_f32(n48, RATE, false)),
        ("f64 + DCなし (Δ指標相当)     ", render_f64(n48, RATE)),
    ] {
        let s = &y[4_800..];
        let (a, b, c) = (goertzel(s, F_IN), goertzel(s, f_lo), goertzel(s, f_hi));
        println!(
            "  {label} {:>9.1} {:>10.1} {:>10.1} {:>10.1}",
            db(a),
            db(b),
            db(c),
            db(b.max(c))
        );
    }

    println!();
    println!("[B] 測定器の差 (チェーンは f32 + DCブロッカーで固定)");
    println!();
    let y = render_f32(n_long, RATE, true);
    println!("  測定器                          220Hz      9380Hz     9820Hz   大きい方");
    let s = &y[4_800..4_800 + n48];
    let (a, b, c) = (goertzel(s, F_IN), goertzel(s, f_lo), goertzel(s, f_hi));
    println!(
        "  Goertzel (len 48000)         {:>9.1} {:>10.1} {:>10.1} {:>10.1}",
        db(a),
        db(b),
        db(c),
        db(b.max(c))
    );
    for (label, bh) in [("FFT 矩形窓 (32768)          ", false), ("FFT Blackman-Harris (32768) ", true)] {
        let a = fft_amp(&y, 8_192, 32_768, F_IN, bh);
        let b = fft_amp(&y, 8_192, 32_768, f_lo, bh);
        let c = fft_amp(&y, 8_192, 32_768, f_hi, bh);
        println!(
            "  {label} {:>9.1} {:>10.1} {:>10.1} {:>10.1}",
            db(a),
            db(b),
            db(c),
            db(b.max(c))
        );
    }

    println!();
    println!("[C] 同じチェーン・同じ測定器で f64 も見る (型の影響の再確認)");
    println!();
    let y64 = render_f64(n_long, RATE);
    println!("  型 / 測定器                     220Hz      9380Hz     9820Hz   大きい方");
    for (label, src) in [("f32 + DC", &y), ("f64 DCなし", &y64)] {
        let a = fft_amp(src, 8_192, 32_768, F_IN, true);
        let b = fft_amp(src, 8_192, 32_768, f_lo, true);
        let c = fft_amp(src, 8_192, 32_768, f_hi, true);
        println!(
            "  {label:<12} (BH FFT)      {:>9.1} {:>10.1} {:>10.1} {:>10.1}",
            db(a),
            db(b),
            db(c),
            db(b.max(c))
        );
    }

    println!();
    println!("  §3-1 の記録値: 基本波 -4.4 dB / パターン -46.9 dB (r=2.5)");
    println!("  上表のどれと一致するかで、どちらの測定が正しいかが決まる。");

    println!();
    println!("[D] **決着**: 9600 Hz は効果か欠陥か");
    println!();
    println!("  理想 ZOH = 段を k*rate へ OS 分解能で置き、ローパスなしで単純間引き。");
    println!("  rate=2.5 は OS=64 で rate*OS=160 (整数) となり段の時刻誤差ゼロ。");
    println!("  ここで 9380/9820 Hz が立つなら**効果**、立たないなら**欠陥**。");
    println!();
    println!("  経路                              220Hz      9380Hz     9820Hz");
    for (label, y) in [
        ("現行 (整数格子 S&H)         ", render_f64(n48, RATE)),
        ("理想 ZOH OS=16 (rate*OS=40) ", render_ideal(n48, RATE, 16, true)),
        ("理想 ZOH OS=64 (rate*OS=160)", render_ideal(n48, RATE, 64, true)),
        ("理想 ZOH OS=64 量子化なし   ", render_ideal(n48, RATE, 64, false)),
        ("現行 量子化なし             ", render_ideal(n48, RATE, 1, false)),
    ] {
        let s = &y[4_800..];
        println!(
            "  {label} {:>9.1} {:>10.1} {:>10.1}",
            db(goertzel(s, F_IN)),
            db(goertzel(s, f_lo)),
            db(goertzel(s, f_hi))
        );
    }

    println!();
    println!("  参考: 整数 rate では 9600 Hz 系に線が立たない (§3-1 の観測)");
    println!();
    println!("  rate  経路                       220Hz    fs/(2r)-f_in  fs/(2r)+f_in");
    for r in [2.0_f64, 3.0, 2.5] {
        let lo = SR / (2.0 * r) - F_IN;
        let hi = SR / (2.0 * r) + F_IN;
        for (tag, y) in [
            ("現行      ", render_f64(n48, r)),
            ("理想 OS=64", render_ideal(n48, r, 64, true)),
        ] {
            let s = &y[4_800..];
            println!(
                "  {r:<5.1} {tag} {:>12.1} {:>13.1} {:>13.1}",
                db(goertzel(s, F_IN)),
                db(goertzel(s, lo)),
                db(goertzel(s, hi))
            );
        }
    }

    println!();
    println!("[E] **3 方式を理想 ZOH との一致度で比較**");
    println!();
    println!("  基準は理想 ZOH (段を k*rate へ置き、ローパスなしで単純間引き)。");
    println!("  値は fs/(2r) ± f_in の大きい方 [絶対 dB] と、理想からの差 [dB]。");
    println!("  **差が 0 に近いほど良い。負に大きいのは効果を削っている。**");
    println!();
    println!("  rate   理想    現行(差)     方式2 OS4h12(差)  方式1 h=32(差)   方式1 h=8(差)");
    for r in [2.5_f64, 3.5, 4.5, 8.5, 16.5, 2.25, 7.125] {
        let lo = SR / (2.0 * r) - F_IN;
        let hi = SR / (2.0 * r) + F_IN;
        let meas = |y: &[f64]| -> f64 {
            let s = &y[4_800..];
            db(goertzel(s, lo).max(goertzel(s, hi)))
        };
        let os_i = ideal_os(r);
        let id = meas(&render_ideal(n48, r, os_i, true));
        let cur = meas(&render_f64(n48, r));
        let m2 = meas(&render_os(n48, r, 4, 12));
        let m1a = meas(&render_minblep(n48, r, 32));
        let m1b = meas(&render_minblep(n48, r, 8));
        println!(
            "  {r:<6.3} {id:>6.1} {cur:>7.1}({:>+5.1}) {m2:>13.1}({:>+6.1}) {m1a:>10.1}({:>+6.1}) {m1b:>10.1}({:>+6.1})",
            cur - id, m2 - id, m1a - id, m1b - id
        );
    }
    println!();
    println!("  (理想の OS は rate*OS が整数になる最小値を自動選択)");
}
