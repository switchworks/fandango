//! `fs/(2r)` に立つ成分が「半整数像」なのか「整数像の折り返し」なのかを分離する
//! (クロスレビューで提起された論点)。
//! **プラグイン本体には触らない。**
//!
//! 争点:
//!   Fable は「半整数像 `(m+1/2)*fs/rate` は理想の連続時間 S&H には存在しない
//!   (像は `m/T` のみ)。よって実装副産物である」と主張した。
//!   一方こちらの実測では、段を正確な時刻に置いた経路 D にも `fs/(2r)` が -57.0 dB で立つ。
//!
//! 食い違いの原因 (代数):
//!   `fs/(2r) = fold(m*fs/r)` は `r = (m -+ 1/2)/k` で成立する。
//!   例 `r=2.5`: `m=2` の `38400` が fold して `9600 = fs/(2*2.5)`。
//!      `r=4.5`: `m=5` の `53333` が fold して `5333 = fs/(2*4.5)`。
//!   **これまで測った rate は全て一致点に当たっており、両者を分離できていない。**
//!
//! 分離の方法:
//!   `fs/(2r)` と最近接の整数像 fold が十分離れる `rate` を選び、そこで測る。
//!   経路 D に立たなければ Fable が正しい (実装副産物)。立てば理想にも存在する。
//!
//! 実行: cargo run -q -p fandango-crush --example rate_image_separation --release

const SR: f64 = 48_000.0;
const BITS: f64 = 8.0;
const F_IN: f64 = 220.0;
const AMP: f64 = 0.6;
const N: usize = 48_000;
const SKIP: usize = 4_800;

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
    adaa_b(x, x_prev, BITS)
}

fn adaa_b(x: f64, x_prev: f64, bits: f64) -> f64 {
    let dx = x - x_prev;
    if dx.abs() > ADAA_EPS {
        (f_antideriv(x, bits) - f_antideriv(x_prev, bits)) / dx
    } else {
        quant((x + x_prev) * 0.5, bits)
    }
}

/// 入力。純音なので非整数時刻でも解析的に評価できる。
fn x_at(t: f64) -> f64 {
    AMP * (2.0 * std::f64::consts::PI * F_IN * t / SR).sin()
}

fn fold(f: f64) -> f64 {
    let ny = SR / 2.0;
    let mut f = f.abs() % SR;
    if f > ny {
        f = SR - f;
    }
    f
}

/// `fs/(2r)` と、整数像 `m*fs/r` の fold との最小距離 [Hz]。
/// これが大きい `rate` でのみ、半整数像と整数像の折り返しを分離できる。
fn separation_hz(rate: f64, m_max: usize) -> (f64, usize) {
    let target = SR / (2.0 * rate);
    let mut best = f64::MAX;
    let mut bm = 0usize;
    for m in 1..=m_max {
        let d = (fold(m as f64 * SR / rate) - target).abs();
        if d < best {
            best = d;
            bm = m;
        }
    }
    (best, bm)
}

/// 経路 A: 現行。整数格子でしかホールドを切り替えない。
fn render_a(n: usize, rate: f64) -> Vec<f64> {
    let mut acc = 0.0_f64;
    let mut hold = 0.0_f64;
    let mut x_prev = 0.0_f64;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let x = x_at(i as f64);
        if acc <= 0.0 {
            hold = x;
            acc += rate;
        }
        acc -= 1.0;
        let q = adaa(hold, x_prev);
        x_prev = hold;
        out.push(q);
    }
    out
}

/// 経路 D: 段を `k*rate` へ OS 分解能で置き、ローパスなしで単純間引き。
/// `rate*os` が整数でないと段の時刻誤差が `1/os` 残る。
fn render_d_os(n: usize, rate: f64, os: usize) -> Vec<f64> {
    let n_os = n * os;
    let rate_os = rate * os as f64;
    let mut acc = 0.0_f64;
    let mut hold = 0.0_f64;
    let mut x_prev = 0.0_f64;
    let mut out = Vec::with_capacity(n);
    for i in 0..n_os {
        let x = x_at(i as f64 / os as f64);
        if acc <= 0.0 {
            hold = x;
            acc += rate_os;
        }
        acc -= 1.0;
        let q = adaa(hold, x_prev);
        x_prev = hold;
        if i % os == 0 {
            out.push(q);
        }
    }
    out
}

/// 経路 E: 境界時刻を f64 で直接持つ。OS 格子を使わない。
/// サンプル `n` が属する区間は `k = floor(n / rate)`、その区間のホールド値は `x(k*rate)`。
/// **段の切り替わりは依然として整数サンプルで起きるが、ホールド値は正確な時刻の入力。**
/// Fable が「境界時刻を丸めない S&H」と呼んだ形の、OS 近似を使わない実装。
fn render_e(n: usize, rate: f64) -> Vec<f64> {
    let mut x_prev = 0.0_f64;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let k = (i as f64 / rate).floor();
        let hold = x_at(k * rate);
        let q = adaa(hold, x_prev);
        x_prev = hold;
        out.push(q);
    }
    out
}


/// 経路 F: **区間平均 (area sampling)**。オーナー提案。
/// サンプル `n` の値を「区間 `[n, n+1]` の間に段がとった値の時間平均」とする。
/// 段の境界が区間の途中 `n+delta` にあれば、前の段が `delta`、次の段が `1-delta` の
/// 重みで混ざる。**はみ出した端数を次のサンプルへ正しい比率で配分する**形。
///
/// `delta = 0` (整数 rate) では境界が区間の端に来るので混合が起きず、現行と一致する。
/// よって §4-3 で棄却した 1 次ランプ補正 (`h*(-0.5-delta)` が delta=0 でも残る) とは別物。
///
/// フィルタ・LUT・場合分けを持ち込まない。加重平均だけ。
/// 区間 `[n, n+1]` を使うので実装上は 1 サンプルの先読みが要る。
fn render_f(n: usize, rate: f64) -> Vec<f64> {
    let mut x_prev = 0.0_f64;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let (lo, hi) = (i as f64, i as f64 + 1.0);
        let mut acc = 0.0_f64;
        let mut p = lo;
        while p < hi {
            let k = (p / rate).floor();
            let t_next = (k + 1.0) * rate;
            let q = t_next.min(hi);
            acc += x_at(k * rate) * (q - p);
            // 数値的に q == p で止まらないよう保険をかける。
            if q <= p {
                break;
            }
            p = q;
        }
        let hold = acc / (hi - lo);
        let qv = adaa(hold, x_prev);
        x_prev = hold;
        out.push(qv);
    }
    out
}

/// 経路 F の変種: 段の値は正確な時刻の入力ではなく、**区間平均した入力**を使う。
/// F は `x(k*rate)` を点で読むが、こちらは段の区間 `[k*rate, (k+1)*rate]` にわたる
/// 入力の平均を段の値とする。S&H の「掴む」動作自体を区間平均にした形。
fn render_g(n: usize, rate: f64) -> Vec<f64> {
    // 段ごとの値を先に作る。
    let n_steps = (n as f64 / rate).ceil() as usize + 2;
    let mut step_val = Vec::with_capacity(n_steps);
    for k in 0..n_steps {
        let t0 = k as f64 * rate;
        let t1 = (k + 1) as f64 * rate;
        // 区間平均を解析的に出す。x(t) = AMP*sin(w*t), w = 2*pi*F_IN/SR。
        let w = 2.0 * std::f64::consts::PI * F_IN / SR;
        let v = if w.abs() < 1e-15 {
            0.0
        } else {
            AMP * ((w * t0).cos() - (w * t1).cos()) / (w * (t1 - t0))
        };
        step_val.push(v);
    }
    let mut x_prev = 0.0_f64;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let (lo, hi) = (i as f64, i as f64 + 1.0);
        let mut acc = 0.0_f64;
        let mut p = lo;
        while p < hi {
            let k = (p / rate).floor() as usize;
            let t_next = (k + 1) as f64 * rate;
            let q = t_next.min(hi);
            acc += step_val.get(k).copied().unwrap_or(0.0) * (q - p);
            if q <= p {
                break;
            }
            p = q;
        }
        let hold = acc / (hi - lo);
        let qv = adaa(hold, x_prev);
        x_prev = hold;
        out.push(qv);
    }
    out
}


/// 段の値をどの時刻から読むかの選択。**補間の要否を分ける唯一の争点**。
#[derive(Clone, Copy, PartialEq)]
enum Grab {
    /// `x(k*rate)` を非整数時刻で読む。**実信号では補間が要る**
    Exact,
    /// `x[round(k*rate)]`。整数サンプルからの読み。補間不要
    Round,
    /// `x[floor(k*rate)]`。整数サンプルからの読み。補間不要
    Floor,
    /// `x[ceil(k*rate)]`。整数サンプルからの読み。補間不要。現行 A に最も近い
    Ceil,
    /// 隣接 2 サンプルの線形補間。`§4-2` では線形 / Catmull-Rom / 厳密が同値だった
    Linear,
    /// 4 点 Catmull-Rom。線形で足りるかの確認用
    Catmull,
}

fn grab_at(k: f64, rate: f64, g: Grab) -> f64 {
    let t = k * rate;
    match g {
        Grab::Exact => x_at(t),
        Grab::Round => x_at(t.round()),
        Grab::Floor => x_at(t.floor()),
        Grab::Ceil => x_at(t.ceil()),
        Grab::Linear => {
            // 整数サンプル 2 点からの線形補間。実信号でも配列アクセス 2 回で済む。
            let i = t.floor();
            let f = t - i;
            x_at(i) * (1.0 - f) + x_at(i + 1.0) * f
        }
        Grab::Catmull => {
            // 4 点 Catmull-Rom。線形との差を見るための対照。
            let i = t.floor();
            let f = t - i;
            let (p0, p1, p2, p3) = (x_at(i - 1.0), x_at(i), x_at(i + 1.0), x_at(i + 2.0));
            let a = -0.5 * p0 + 1.5 * p1 - 1.5 * p2 + 0.5 * p3;
            let b = p0 - 2.5 * p1 + 2.0 * p2 - 0.5 * p3;
            let c = -0.5 * p0 + 0.5 * p2;
            ((a * f + b) * f + c) * f + p1
        }
    }
}

/// 経路 F の一般形。`g` で段の値を読む時刻を選ぶ。
/// 区間平均そのもの (加重平均) は `g` によらず同じ。
fn render_f_g(n: usize, rate: f64, g: Grab) -> Vec<f64> {
    let mut x_prev = 0.0_f64;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let (lo, hi) = (i as f64, i as f64 + 1.0);
        let mut acc = 0.0_f64;
        let mut p = lo;
        while p < hi {
            let k = (p / rate).floor();
            let t_next = (k + 1.0) * rate;
            let q = t_next.min(hi);
            acc += grab_at(k, rate, g) * (q - p);
            if q <= p {
                break;
            }
            p = q;
        }
        let hold = acc / (hi - lo);
        let qv = adaa(hold, x_prev);
        x_prev = hold;
        out.push(qv);
    }
    out
}


/// 経路 F の**因果版**。区間を `[n, n+1]` から `[n-1, n]` へずらす。
/// Fable の主張: 出力は `[n, n+1]` 版をちょうど 1 サンプル遅らせたものと同一であり、
/// よって因果版のレイテンシは 0、残るのは群遅延 0.5 サンプルのみ。
/// **この主張を実測で照合する。**
fn render_f_causal(n: usize, rate: f64, g: Grab) -> Vec<f64> {
    let mut x_prev = 0.0_f64;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let (lo, hi) = (i as f64 - 1.0, i as f64);
        if hi <= 0.0 {
            out.push(0.0);
            continue;
        }
        let lo = lo.max(0.0);
        let mut acc = 0.0_f64;
        let mut p = lo;
        while p < hi {
            let k = (p / rate).floor();
            let t_next = (k + 1.0) * rate;
            let q = t_next.min(hi);
            acc += grab_at(k, rate, g) * (q - p);
            if q <= p {
                break;
            }
            p = q;
        }
        let hold = acc / (hi - lo);
        let qv = adaa(hold, x_prev);
        x_prev = hold;
        out.push(qv);
    }
    out
}


/// 経路 A の bits 可変版。
fn render_a_b(n: usize, rate: f64, bits: f64) -> Vec<f64> {
    let mut acc = 0.0_f64;
    let mut hold = 0.0_f64;
    let mut x_prev = 0.0_f64;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let x = x_at(i as f64);
        if acc <= 0.0 {
            hold = x;
            acc += rate;
        }
        acc -= 1.0;
        let q = adaa_b(hold, x_prev, bits);
        x_prev = hold;
        out.push(q);
    }
    out
}

/// 経路 F (因果版・線形補間) の bits 可変版。
/// Fable の指摘: 混合サンプルは 2 つの段値の中間値なので、後段の ADAA 量子化器は
/// 「どの量子化レベルにも属さない過渡値」を毎境界で 1 個受け取る。
/// **bits が低い (2〜4) とき、この過渡値が量子化器の段差を余分に踏む可能性がある。**
fn render_f_b(n: usize, rate: f64, bits: f64) -> Vec<f64> {
    let mut x_prev = 0.0_f64;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let hi = i as f64;
        if hi <= 0.0 {
            out.push(0.0);
            continue;
        }
        let lo = hi - 1.0;
        let mut acc = 0.0_f64;
        let mut p = lo;
        while p < hi {
            let k = (p / rate).floor();
            let t_next = (k + 1.0) * rate;
            let q = t_next.min(hi);
            acc += grab_at(k, rate, Grab::Linear) * (q - p);
            if q <= p {
                break;
            }
            p = q;
        }
        let hold = acc / (hi - lo);
        let qv = adaa_b(hold, x_prev, bits);
        x_prev = hold;
        out.push(qv);
    }
    out
}

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
fn db(x: f64) -> f64 {
    20.0 * x.max(1e-15).log10()
}

/// `fs/(2r) ± f_in` の大きい方 [絶対 dB]。
fn half_image_db(y: &[f64], rate: f64) -> f64 {
    let c = SR / (2.0 * rate);
    let a = goertzel(&y[SKIP..], fold(c - F_IN));
    let b = goertzel(&y[SKIP..], fold(c + F_IN));
    db(a.max(b))
}

/// `rate * os` が整数になる最小の os (>= 64)。無ければ 0 (= 分解能不足)。
fn exact_os(rate: f64) -> usize {
    for os in 64..=4096 {
        let v = rate * os as f64;
        if (v - v.round()).abs() < 1e-9 {
            return os;
        }
    }
    0
}

fn main() {
    println!("`fs/(2r)` の成分は半整数像か、整数像の折り返しか");
    println!();
    println!("  争点: Fable は「半整数像は理想の連続時間 S&H に存在しない = 実装副産物」と主張。");
    println!("  こちらの実測では経路 D にも立つ。**両者を分離できていないのが原因。**");
    println!();

    println!("[1] 分離可能性の確認: fs/(2r) と最近接の整数像 fold との距離");
    println!();
    println!("  距離が小さい rate では両者が同じビンに乗り、分離できない。");
    println!("  Goertzel の分解能は約 {:.1} Hz (解析長 {} サンプル)。", SR / (N - SKIP) as f64, N - SKIP);
    println!();
    println!("  rate      fs/(2r)[Hz]   最近接整数像 fold との距離[Hz]  (m)   分離");
    let rates = [
        2.5_f64, 3.5, 4.5, 8.5, 16.5, 2.25, 7.125, // これまで測ってきた点
        2.501, 2.51, 2.53, 2.6, 3.14159, 5.3, 11.7, 2.0001, 2.001,
    ];
    for &r in &rates {
        let (d, m) = separation_hz(r, 400);
        let sep = if d > 200.0 { "可" } else { "不可" };
        println!(
            "  {r:<9.5} {:>11.1} {d:>26.1} {m:>7} {sep:>7}",
            SR / (2.0 * r)
        );
    }

    println!();
    println!("[2] 分離可能な rate での実測");
    println!();
    println!("  経路 A = 現行 (整数格子)");
    println!("  経路 D = 段を k*rate へ OS 分解能で配置 + ローパスなし単純間引き");
    println!("  経路 E = 境界時刻を f64 で直接保持 (OS 近似なし)");
    println!("  値は fs/(2r) ± f_in の大きい方 [絶対 dB]。基本波は約 -4.4 dB。");
    println!();
    println!("  rate       距離[Hz]   経路A      経路D(os)      経路E     経路F     経路G");
    for &r in &rates {
        let (d, _) = separation_hz(r, 400);
        let a = half_image_db(&render_a(N, r), r);
        let os = exact_os(r);
        let dcell = if os > 0 && os <= 512 {
            format!("{:>8.1}({os:>4})", half_image_db(&render_d_os(N, r, os), r))
        } else {
            "     --(  --)".to_string()
        };
        let e = half_image_db(&render_e(N, r), r);
        let f = half_image_db(&render_f(N, r), r);
        let g = half_image_db(&render_g(N, r), r);
        println!("  {r:<10.5} {d:>9.1} {a:>8.1} {dcell} {e:>10.1} {f:>9.1} {g:>9.1}");
    }

    println!();
    println!("判定:");
    println!("  分離「可」の rate で経路 D / E に fs/(2r) が立たない (A より大きく下回る)");
    println!("  → Fable が正しい。半整数像は整数格子への丸めが生む実装副産物。");
    println!("  分離「可」でも立つ → 理想にも存在する成分。");

    println!();
    println!("[3] 整数 rate での恒等性 (経路 F / G が現行と一致するか)");
    println!();
    println!("  区間平均は delta=0 で混合が起きないので、現行と一致するはず。");
    println!("  値は最大絶対差。0 なら完全一致。");
    println!();
    println!("  rate    F との最大差      G との最大差");
    for r in [1.0_f64, 2.0, 3.0, 4.0, 8.0, 16.0, 32.0] {
        let a = render_a(N, r);
        let f = render_f(N, r);
        let g = render_g(N, r);
        let df = a.iter().zip(f.iter()).map(|(p, q)| (p - q).abs()).fold(0.0_f64, f64::max);
        let dg = a.iter().zip(g.iter()).map(|(p, q)| (p - q).abs()).fold(0.0_f64, f64::max);
        println!("  {r:<6.1} {df:>14.3e} {dg:>17.3e}");
    }

    println!();
    println!("[4] 経路 F / G の全体像 (分離不可の点も含む)");
    println!();
    println!("  A を基準にした改善量 [dB]。負が改善。");
    println!();
    println!("  rate       経路A     F(差)        G(差)        D(差)");
    for &r in &rates {
        let a = half_image_db(&render_a(N, r), r);
        let f = half_image_db(&render_f(N, r), r);
        let g = half_image_db(&render_g(N, r), r);
        let os = exact_os(r);
        let dcell = if os > 0 && os <= 512 {
            let d = half_image_db(&render_d_os(N, r, os), r);
            format!("{d:>8.1}({:>+6.1})", d - a)
        } else {
            "      --(    --)".to_string()
        };
        println!(
            "  {r:<10.5} {a:>8.1} {f:>8.1}({:>+6.1}) {g:>8.1}({:>+6.1}) {dcell}",
            f - a,
            g - a
        );
    }

    println!();
    println!("[5] **争点 (a): 段の値の読み時刻。補間が要るか**");
    println!();
    println!("  F は `x(k*rate)` を非整数時刻で読むため、実信号では補間が要る。");
    println!("  整数サンプルからしか読まない版 (round / floor / ceil) で改善量が保たれるなら、");
    println!("  **補間そのものが不要になり、優先順位5 の literal 読みでも抵触しなくなる。**");
    println!();
    println!("  §4-2 の既存実測では「入力を掴む時刻の精度」の影響は天井 1.9 dB。");
    println!();
    println!("  rate       経路A    F:Exact(差)   F:Round(差)   F:Floor(差)   F:Ceil(差)");
    for &r in &rates {
        let a = half_image_db(&render_a(N, r), r);
        let cells: Vec<String> = [Grab::Exact, Grab::Round, Grab::Floor, Grab::Ceil]
            .iter()
            .map(|&g| {
                let v = half_image_db(&render_f_g(N, r, g), r);
                format!("{v:>8.1}({:>+6.1})", v - a)
            })
            .collect();
        println!("  {r:<10.5} {a:>8.1} {}", cells.join(" "));
    }

    println!();
    println!("[6] 整数 rate の恒等性 (読み時刻を変えても保たれるか)");
    println!();
    println!("  現行 A との最大絶対差。0 なら完全一致。");
    println!();
    println!("  rate      Exact        Round        Floor        Ceil");
    for r in [1.0_f64, 2.0, 3.0, 4.0, 8.0, 16.0, 32.0] {
        let a = render_a(N, r);
        let cells: Vec<String> = [Grab::Exact, Grab::Round, Grab::Floor, Grab::Ceil]
            .iter()
            .map(|&g| {
                let y = render_f_g(N, r, g);
                let d = a.iter().zip(y.iter()).map(|(p, q)| (p - q).abs()).fold(0.0_f64, f64::max);
                format!("{d:>11.3e}")
            })
            .collect();
        println!("  {r:<8.1} {}", cells.join(" "));
    }

    println!();
    println!("[7] **補間の質は効くか** (§4-2 では線形 / Catmull-Rom / 厳密が同値だった)");
    println!();
    println!("  Linear は整数サンプル 2 点の加重和。実信号でも配列アクセス 2 回で済む。");
    println!("  Exact に届くなら、閉形式のまま Exact 相当が得られる。");
    println!();
    println!("  rate       経路A   F:Exact(差)   F:Linear(差)  F:Catmull(差)  F:Round(差)");
    for &r in &rates {
        let a = half_image_db(&render_a(N, r), r);
        let cells: Vec<String> = [Grab::Exact, Grab::Linear, Grab::Catmull, Grab::Round]
            .iter()
            .map(|&g| {
                let v = half_image_db(&render_f_g(N, r, g), r);
                format!("{v:>8.1}({:>+6.1})", v - a)
            })
            .collect();
        println!("  {r:<10.5} {a:>8.1} {}", cells.join(" "));
    }

    println!();
    println!("[8] 整数 rate の恒等性 (補間を入れても保たれるか)");
    println!();
    println!("  rate      Exact        Linear       Catmull");
    for r in [1.0_f64, 2.0, 3.0, 4.0, 8.0, 16.0, 32.0] {
        let a = render_a(N, r);
        let cells: Vec<String> = [Grab::Exact, Grab::Linear, Grab::Catmull]
            .iter()
            .map(|&g| {
                let y = render_f_g(N, r, g);
                let d = a.iter().zip(y.iter()).map(|(p, q)| (p - q).abs()).fold(0.0_f64, f64::max);
                format!("{d:>11.3e}")
            })
            .collect();
        println!("  {r:<8.1} {}", cells.join(" "));
    }

    println!();
    println!("[9] **Fable の主張の照合: 因果版 [n-1,n] は 1 サンプル遅延と同一か**");
    println!();
    println!("  主張が正しければ causal[n+1] == f[n] が全 n で成立し、");
    println!("  因果版のレイテンシは 0 (群遅延 0.5 サンプルのみ) になる。");
    println!("  先頭は初期化の違いが出るので 100 サンプル捨てて比較する。");
    println!();
    println!("  rate       最大絶対差 (causal[n+1] vs f[n])   スペクトル値の一致");
    for &r in &rates {
        let f = render_f_g(N, r, Grab::Linear);
        let c = render_f_causal(N, r, Grab::Linear);
        let d = (100..N - 1)
            .map(|i| (c[i + 1] - f[i]).abs())
            .fold(0.0_f64, f64::max);
        let sf = half_image_db(&f, r);
        let sc = half_image_db(&c, r);
        println!("  {r:<10.5} {d:>28.3e}   F {sf:>7.1} / causal {sc:>7.1} dB");
    }

    println!();
    println!("[10] **Fable の指摘の照合: bits = 2/4 での F**");
    println!();
    println!("  混合サンプルは 2 つの段値の中間値なので、ADAA 量子化器が");
    println!("  「どの量子化レベルにも属さない過渡値」を毎境界で受け取る。");
    println!("  bits が低いときにこれが段差を余分に踏むか。");
    println!();
    println!("  bits  rate       経路A      経路F(因果)     改善[dB]");
    for bits in [2.0_f64, 4.0, 8.0, 16.0] {
        for &r in &[2.5_f64, 4.5, 8.5, 16.5, 2.25, 7.125] {
            let a = half_image_db(&render_a_b(N, r, bits), r);
            let f = half_image_db(&render_f_b(N, r, bits), r);
            println!("  {bits:<5.0} {r:<10.5} {a:>8.1} {f:>13.1} {:>12.1}", f - a);
        }
        println!();
    }

    println!("[11] bits を変えても整数 rate の恒等性が保たれるか");
    println!();
    println!("  現行 A との最大絶対差 (F 因果版は 1 サンプル遅延して比較)。");
    println!();
    println!("  bits   rate=1      rate=2      rate=4      rate=8      rate=16     rate=32");
    for bits in [2.0_f64, 4.0, 8.0, 16.0] {
        let cells: Vec<String> = [1.0_f64, 2.0, 4.0, 8.0, 16.0, 32.0]
            .iter()
            .map(|&r| {
                let a = render_a_b(N, r, bits);
                let f = render_f_b(N, r, bits);
                let d = (100..N - 1)
                    .map(|i| (f[i + 1] - a[i]).abs())
                    .fold(0.0_f64, f64::max);
                format!("{d:>11.3e}")
            })
            .collect();
        println!("  {bits:<5.0} {}", cells.join(" "));
    }
}
