//! 方式 1 (最小位相カーネル / minBLEP 系、レイテンシ 0) の測定
//! **プラグイン本体には触らない。**
//!
//! 定式化:
//!   段の真の時刻 `t*` に対し naive S&H はサンプル `p` で段を立てる。
//!   素の minBLEP は naive 段を帯域制限段へ**置き換える**ため、`t*` が整数 (= 整数 `rate`) でも
//!   段が鈍る。これは §4-3 で棄却した 1 次ランプ補正と同じ罠。
//!   よって補正を**差分形**にする:
//!       corr(n) = h * ( s(n - t*) - s(n - p + 1) )
//!   `t*` が整数のとき `t* = p - 1` で第 2 項と一致し、補正は厳密に 0。
//!   → **整数 `rate` は無変更**（§6 の bit-exact 判定に載る形）。
//!
//! 実行: cargo run -q -p fandango-crush --example rate_minblep --release

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

fn sine(n: usize) -> Vec<f32> {
    (0..n).map(|i| (0.6 * (2.0 * std::f64::consts::PI * F_IN * i as f64 / SR).sin()) as f32).collect()
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

// ---- 最小限の複素 FFT (radix-2)。cepstrum 法の最小位相化に要る。 ----
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

/// 線形位相の帯域制限インパルス (窓付き sinc)。`l` 倍オーバーサンプルのテーブル用。
fn blit(half: usize, l: usize) -> Vec<f64> {
    let n = 2 * half * l + 1;
    let mut k = vec![0.0; n];
    let mut sum = 0.0;
    for (i, v) in k.iter_mut().enumerate() {
        let t = (i as f64 - (half * l) as f64) / l as f64;
        let s = if t.abs() < 1e-12 {
            1.0
        } else {
            (std::f64::consts::PI * t).sin() / (std::f64::consts::PI * t)
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

/// 実 cepstrum による最小位相化。返り値は先頭 `h.len()` サンプル。
fn min_phase(h: &[f64], m: usize) -> Vec<f64> {
    assert!(m >= h.len() * 4, "cepstrum のエイリアスを避けるため FFT 長を十分取る");
    let mut re = vec![0.0; m];
    let mut im = vec![0.0; m];
    re[..h.len()].copy_from_slice(h);
    fft(&mut re, &mut im, false);
    for i in 0..m {
        let mag = (re[i] * re[i] + im[i] * im[i]).sqrt();
        re[i] = mag.max(1e-14).ln();
        im[i] = 0.0;
    }
    fft(&mut re, &mut im, true);
    // 因果側へ折り畳む
    for i in 1..m / 2 {
        re[i] *= 2.0;
        im[i] *= 2.0;
    }
    for i in m / 2 + 1..m {
        re[i] = 0.0;
        im[i] = 0.0;
    }
    fft(&mut re, &mut im, false);
    for i in 0..m {
        let e = re[i].exp();
        let (c, s) = (im[i].cos(), im[i].sin());
        re[i] = e * c;
        im[i] = e * s;
    }
    fft(&mut re, &mut im, true);
    re[..h.len()].to_vec()
}

/// 最小位相の帯域制限ステップ `s(tau)` を `l` 倍解像度で持つテーブル。
/// `tau <= 0` で 0、テーブル末尾以降は 1。
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
    /// `s` が 0.5 を横切る `tau` [基底サンプル] = 実効群遅延。
    /// **最小位相化は群遅延を最小にするが 0 にはしない。** 補正はこの分だけ遅れて効く。
    fn group_delay(&self) -> f64 {
        for j in 1..self.s.len() {
            if self.s[j - 1] < 0.5 && self.s[j] >= 0.5 {
                return j as f64 / self.l as f64;
            }
        }
        panic!("step table が 0.5 を横切らない");
    }
    /// `tau` [基底サンプル] における `s(tau)`。線形補間。
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

/// 現行と同じ整数グリッド S&H。`tab` を渡すと段の時刻誤差だけを差分形で補正する。
/// **`tab = None` が現行経路。**
fn render(x: &[f32], rate: f64, tab: Option<&StepTable>, tail: usize) -> Vec<f32> {
    render_at(x, rate, tab, tail, false)
}

/// `post = true` で補正を **ADAA 量子化の後**に適用する。
/// 仮説: 量子化は非線形なので、量子化前に足した delta 情報は再量子化で潰れる。
/// 遅延整合は両経路とも同じ (naive を D 遅延させてから補正を足す)。
fn render_at(x: &[f32], rate: f64, tab: Option<&StepTable>, tail: usize, post: bool) -> Vec<f32> {
    // BL step は S(tau) ~ u(tau - D) と群遅延 D を持つ。よって補正
    //   corr(k) = h * ( S(k - t*) - S(k - n) )
    // の第 2 項は「D 遅延した naive」に対応する。**naive 側を D 遅らせないと不整合**。
    // delta = 0 のとき両項が一致し補正は厳密に 0。
    let d = tab.map(|t| t.group_delay().round() as usize).unwrap_or(0);

    let mut acc = 0.0_f64;
    let mut hold = 0.0_f32;
    let mut x_prev = 0.0_f64;
    let mut corr = vec![0.0_f64; x.len() + tail + 2];
    let mut sh: Vec<f32> = Vec::with_capacity(x.len());
    let mut steps: Vec<(usize, f64)> = Vec::new(); // (naive で段が立つサンプル, delta)

    let mut t_true = 0.0_f64;
    for &v in x.iter() {
        let n = sh.len();
        let xd = if v.is_finite() { v.clamp(-4.0, 4.0) } else { 0.0 };
        if acc <= 0.0 {
            let delta = n as f64 - t_true;
            steps.push((n, delta));
            if !post {
                if let Some(t) = tab {
                    let h = (xd - hold) as f64;
                    let end = (n + tail).min(corr.len() - 1);
                    for k in n..end {
                        let tau = (k - n) as f64;
                        corr[k] += h * (t.at(tau + delta) - t.at(tau));
                    }
                }
            }
            hold = xd;
            acc += rate;
            t_true += rate;
        }
        acc -= 1.0;
        sh.push(hold);
    }

    let mut y: Vec<f32> = Vec::with_capacity(x.len());
    for k in 0..sh.len() {
        // naive を D 遅延させたものへ補正を足す (post のときは足さず後段へ回す)。
        let base = if k >= d { sh[k - d] as f64 } else { 0.0 };
        let v = (base + corr[k]) as f32;
        let q = shape_adaa_quant(v, x_prev, BITS);
        x_prev = v as f64;
        y.push(q);
    }

    if post {
        if let Some(t) = tab {
            let mut pc = vec![0.0_f64; y.len() + tail + 2];
            for &(n, delta) in &steps {
                // 遅延後、段は k = n + d に立つ。高さは量子化後の実測差。
                let p = n + d;
                if p == 0 || p + 1 >= y.len() {
                    continue;
                }
                let h = (y[p + 1] - y[p - 1]) as f64;
                let end = (n + tail).min(pc.len() - 1);
                for k in n..end {
                    let tau = (k - n) as f64;
                    pc[k] += h * (t.at(tau + delta) - t.at(tau));
                }
            }
            for (k, v) in y.iter_mut().enumerate() {
                *v = (*v as f64 + pc[k]) as f32;
            }
        }
    }

    for v in y.iter_mut() {
        *v = denormal_gate(*v);
    }

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
fn spectrum(y: &[f32]) -> Vec<(f64, f64)> {
    (1..=1000).map(|k| {
        let f = k as f64 * 20.0;
        (f, db(goertzel(y, f)))
    }).collect()
}

fn main() {
    let x = sine(N);
    let skip = N / 10;

    println!("方式 1 (minBLEP 差分形、レイテンシ 0) — 純音 {F_IN} Hz, bits={BITS}, mix=1");
    println!("  補正 corr(n) = h * ( s(n - t*) - s(n - p) )。整数 rate で厳密に 0。");
    println!("  half = 補正の片側タップ数 [基底サンプル]、テーブル解像度 L=64。");
    println!("  受け入れ暫定基準: パターン成分 <= -70 dB");
    println!();
    println!("  half   D[smp]   r=2.5    r=3.5    r=4.5    r=8.5   r=16.5");
    let mut cells = String::new();
    for r in [2.5_f64, 3.5, 4.5, 8.5, 16.5] {
        cells.push_str(&format!(
            "{:>9.1}",
            sideband_db(&render(&x, r, None, 0)[skip..], SR / (2.0 * r))
        ));
    }
    println!("  現行      -   {cells}");
    for half in [4usize, 8, 16, 32, 64] {
        let tab = StepTable::new(half, 64);
        let mut cells = String::new();
        for r in [2.5_f64, 3.5, 4.5, 8.5, 16.5] {
            let y = render(&x, r, Some(&tab), 4 * half);
            cells.push_str(&format!("{:>9.1}", sideband_db(&y[skip..], SR / (2.0 * r))));
        }
        println!("  {half:<5} {:>5} {cells}", tab.group_delay().round() as usize);
    }

    println!();
    println!("整数 rate の不変性 (現行 vs minBLEP、サンプル単位の最大絶対差)");
    println!("  差分形が意図どおりなら**完全一致**するはず。");
    println!();
    for half in [16usize, 64] {
        let tab = StepTable::new(half, 64);
        let mut worst = 0.0_f32;
        let mut moved = 0usize;
        for r in [1.0_f64, 2.0, 3.0, 4.0, 6.0, 7.0, 8.0, 16.0, 32.0] {
            let a = render(&x, r, None, 0);
            let b = render(&x, r, Some(&tab), 4 * half);
            let d = a.iter().zip(b.iter()).map(|(p, q)| (p - q).abs()).fold(0.0_f32, f32::max);
            if d > 0.0 {
                moved += 1;
            }
            worst = worst.max(d);
        }
        println!("  half={half:<4} 動いた rate 数 = {moved} / 9、最大絶対差 = {worst:.3e}");
    }

    println!();
    println!("整数 rate のスペクトル比較 (床 -40 dB、現行 vs minBLEP half=32)");
    println!();
    let tab = StepTable::new(32, 64);
    for (name, src) in [("純音 220 Hz", sine(N)), ("広帯域 (倍音+雑音)", composite(N))] {
        let mut worst = (0.0_f64, 0.0_f64);
        let mut over = 0usize;
        for r in [2.0_f64, 3.0, 4.0, 6.0, 7.0, 8.0, 16.0, 32.0] {
            let a = render(&src, r, None, 0);
            let b = render(&src, r, Some(&tab), 128);
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

    // ---- 疑い 2 の単体検証: StepTable の tau=0 原点はどこか ----
    // 段が tau=0 に立つなら s(tau) は tau=0 から立ち上がり、tau≈0.5 で 0.5 を横切るはず。
    // ずれていれば補正が誤った時刻に当たる。
    println!();
    println!("[疑い 2] StepTable の原点検証 (L=64)");
    println!("  sum(h) = 1 か / s(tau) の立ち上がり / 0.5 を横切る tau (= 実効群遅延)");
    println!();
    for half in [4usize, 16, 64] {
        let l = 64usize;
        let lin = blit(half, l);
        let sum: f64 = lin.iter().sum();
        let mp = min_phase(&lin, (2 * half * l + 1).next_power_of_two() * 8);
        let sum_mp: f64 = mp.iter().sum();
        // 最小位相インパルスのピーク位置とエネルギー重心 [基底サンプル]
        let (pk, _) = mp
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
            .unwrap();
        let e: f64 = mp.iter().map(|v| v * v).sum();
        let cen: f64 = mp.iter().enumerate().map(|(i, v)| i as f64 * v * v).sum::<f64>() / e;
        let tab = StepTable::new(half, l);
        // s が 0.5 を横切る tau
        let mut cross = f64::NAN;
        for j in 1..tab.s.len() {
            if tab.s[j - 1] < 0.5 && tab.s[j] >= 0.5 {
                cross = j as f64 / l as f64;
                break;
            }
        }
        println!(
            "  half={half:<3} sum(lin)={sum:.6} sum(minphase)={sum_mp:.6} \
peak@{:.3} 重心@{:.3} s=0.5@{cross:.3} [基底smp]",
            pk as f64 / l as f64,
            cen / l as f64
        );
        let probe = [0.0_f64, 0.125, 0.25, 0.5, 0.75, 1.0, 2.0, 4.0, 8.0];
        let mut line = String::new();
        for t in probe {
            line.push_str(&format!(" s({t})={:.4}", tab.at(t)));
        }
        println!("   {line}");
    }

    println!();
    println!("[仮説] 補正を ADAA 量子化の後に適用した場合 (遅延整合は同じ)");
    println!("  量子化が非線形なので、量子化前に足した delta 情報が潰れているか。");
    println!();
    println!("  half   D[smp]   r=2.5    r=3.5    r=4.5    r=8.5   r=16.5");
    for half in [4usize, 16, 64] {
        let tab = StepTable::new(half, 64);
        let mut cells = String::new();
        for r in [2.5_f64, 3.5, 4.5, 8.5, 16.5] {
            let y = render_at(&x, r, Some(&tab), 4 * half, true);
            cells.push_str(&format!("{:>9.1}", sideband_db(&y[skip..], SR / (2.0 * r))));
        }
        println!("  {half:<5} {:>5} {cells}", tab.group_delay().round() as usize);
    }
}
