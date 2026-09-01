//! 非整数 `rate` で出る振幅うねりの診断。
//!
//! 仮説: ホールド長が floor(r)/ceil(r) の 2 値で交互になり、その並びの周期
//! `1/frac(r)` ホールド = `fs*frac(r)/r` Hz が振幅変調として現れる。
//! 整数 r では frac(r)=0 なので消える。
//!
//! 実行: cargo run -p fandango-crush --example rate_artifact --release

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

struct ExpChannel {
    x_prev: f64,
    hold: f32,
    acc: f64,
    dc: DcBlocker,
    /// 何番目のホールドか。理想サンプル時刻 `k * rate` を出すのに使う。
    k: u64,
}
impl ExpChannel {
    fn new(sr: f32) -> Self {
        let mut dc = DcBlocker::new();
        dc.set_sample_rate(sr);
        Self { x_prev: 0.0, hold: 0.0, acc: 0.0, dc, k: 0 }
    }
}

/// 全体を一括処理する。分数遅延補間が過去の任意時刻を読むため、入力列全体を要する。
fn render(x: &[f32], bits: f32, rate: f64, mix: f32, interp: Interp, f_in: f64) -> Vec<f32> {
    let mut ch = ExpChannel::new(SR as f32);
    let rate = rate.max(1.0);
    let mut out = Vec::with_capacity(x.len());

    let at = |t: f64| -> f32 {
        // 理想時刻 t での入力値。t は必ず現在サンプル以下 (k*rate <= エッジ位置)。
        let i = t.floor() as isize;
        let f = t - i as f64;
        let g = |j: isize| -> f64 {
            if j < 0 || j as usize >= x.len() { 0.0 } else { x[j as usize] as f64 }
        };
        match interp {
            Interp::None => unreachable!(),
            Interp::Linear => (g(i) * (1.0 - f) + g(i + 1) * f) as f32,
            Interp::Cubic => {
                let (m1, p0, p1, p2) = (g(i - 1), g(i), g(i + 1), g(i + 2));
                let a0 = p0;
                let a1 = 0.5 * (p1 - m1);
                let a2 = m1 - 2.5 * p0 + 2.0 * p1 - 0.5 * p2;
                let a3 = 0.5 * (p2 - m1) + 1.5 * (p0 - p1);
                (a0 + f * (a1 + f * (a2 + f * a3))) as f32
            }
            Interp::Exact => (0.6 * (2.0 * std::f64::consts::PI * f_in * t / SR).sin()) as f32,
        }
    };

    for (n, &v) in x.iter().enumerate() {
        let x_dry = if v.is_finite() { v.clamp(-4.0, 4.0) } else { 0.0 };
        if ch.acc <= 0.0 {
            ch.hold = if interp == Interp::None {
                x_dry
            } else {
                // 理想サンプル時刻。エッジの立つサンプル n 以下に必ず収まる。
                let t = ch.k as f64 * rate;
                debug_assert!(t <= n as f64 + 1e-9, "理想時刻が未来にある: t={t} n={n}");
                at(t)
            };
            ch.acc += rate;
            ch.k += 1;
        }
        ch.acc -= 1.0;

        let y = shape_adaa_quant(ch.hold, ch.x_prev, bits);
        ch.x_prev = ch.hold as f64;
        let y = ch.dc.process(denormal_gate(y));
        let o = (1.0 - mix) * x_dry + mix * y;
        out.push(if o.is_finite() { o.clamp(-6.0, 6.0) } else { 0.0 });
    }
    out
}

/// 分数遅延補間の方式。**発生源 1（入力を掴む時刻の量子化）だけ**を潰すための比較軸。
/// 出力の段の切り替わり時刻（発生源 2）はどれも整数グリッドのまま。
#[derive(Clone, Copy, PartialEq)]
enum Interp {
    /// 現行。エッジの立つサンプルの値をそのまま掴む（時刻ジッタあり）。
    None,
    /// 理想時刻 `k*r` を線形補間で読む。
    Linear,
    /// 同上、Catmull-Rom 4 点。
    Cubic,
    /// 同上、解析的に厳密。**入力が既知の正弦だから可能な上限値**。実装不可、比較の天井。
    Exact,
}

const SR: f64 = 48_000.0;
const BITS: f32 = 8.0;
/// 診断入力は純音 1 本。合成入力だと素材側の過渡と混ざる。
const F_IN: f64 = 220.0;

fn sine(n: usize) -> Vec<f32> {
    (0..n).map(|i| (0.6 * (2.0 * std::f64::consts::PI * F_IN * i as f64 / SR).sin()) as f32).collect()
}

/// 5 ms ホップの RMS 包絡。
fn envelope(y: &[f32], hop: usize) -> Vec<f64> {
    y.chunks(hop)
        .map(|c| (c.iter().map(|v| (*v as f64) * (*v as f64)).sum::<f64>() / c.len() as f64).sqrt())
        .collect()
}

/// 包絡の変調深さ [dB] と、自己相関の第 1 ピークから求めた変調周波数 [Hz]。
fn modulation(env: &[f64], hop_hz: f64) -> (f64, f64) {
    let mean = env.iter().sum::<f64>() / env.len() as f64;
    let lo = env.iter().cloned().fold(f64::INFINITY, f64::min).max(1e-12);
    let hi = env.iter().cloned().fold(0.0, f64::max);
    let depth = 20.0 * (hi / lo).log10();

    let d: Vec<f64> = env.iter().map(|v| v - mean).collect();
    let denom: f64 = d.iter().map(|v| v * v).sum();
    let mut best = (0usize, 0.0_f64);
    // lag 2..len/2 で自己相関の最大。lag 1 は隣接相関なので除く。
    for lag in 2..d.len() / 2 {
        let c: f64 = (0..d.len() - lag).map(|i| d[i] * d[i + lag]).sum::<f64>() / denom;
        if c > best.1 {
            best = (lag, c);
        }
    }
    let f = if best.0 > 0 { hop_hz / best.0 as f64 } else { f64::NAN };
    (depth, f)
}

/// Goertzel。`f_hz` の複素振幅の絶対値。
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

/// `f0 ± F_IN` の 2 本のうち大きい方 [dB]。
fn sideband_db(y: &[f32], f0: f64) -> f64 {
    let a = goertzel(y, f0 - F_IN);
    let b = goertzel(y, f0 + F_IN);
    db(a.max(b))
}

fn main() {
    let n = (SR * 1.0) as usize;
    let x = sine(n);
    let skip = n / 10; // 過渡を捨てる

    println!("パターン成分の発生源の切り分け (純音 {F_IN} Hz, bits={BITS}, mix=1)");
    println!();
    println!("  「本来」 = fs/r ± {F_IN} Hz  (S&H 本来のエイリアス。**残すべき音**)");
    println!("  「パターン」 = fs/(2r) ± {F_IN} Hz  (非整数ホールドの交互パターンが作る副生成物)");
    println!();
    println!("  rate  補間        本来[dB]  パターン[dB]   パターン低減[dB]");

    for r in [2.0_f64, 2.5, 3.0, 3.5, 4.0, 4.5, 8.5, 16.5] {
        let mut base = f64::NAN;
        for (label, interp) in [
            ("なし(現行)", Interp::None),
            ("線形", Interp::Linear),
            ("Catmull-Rom", Interp::Cubic),
            ("解析的(上限)", Interp::Exact),
        ] {
            let y = render(&x, BITS, r, 1.0, interp, F_IN);
            let y = &y[skip..];
            let want = sideband_db(y, SR / r);
            let patt = sideband_db(y, SR / (2.0 * r));
            if interp == Interp::None {
                base = patt;
                println!("  {r:<5.1} {label:<12} {want:>8.1}  {patt:>11.1}              -");
            } else {
                println!("  {r:<5.1} {label:<12} {want:>8.1}  {patt:>11.1}   {:>12.1}", base - patt);
            }
        }
        println!();
    }
}
