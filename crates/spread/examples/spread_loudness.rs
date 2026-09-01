//! t=0/0.5/1 における相関 -1..+1 の入出力 RMS 比を実測し、
//! 無相関点でラウドネスを保つ Mid ゲイン係数を数値的に求める。
//!
//! 実行: cargo run -p fandango-spread --example spread_loudness --release

use fandango_spread::dsp::Spread;

/// xorshift ベースの決定論 PRNG（外部依存を持たないため）。
struct Rng(u64);
impl Rng {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    /// Box-Muller で標準正規を 1 個返す。
    fn normal(&mut self) -> f64 {
        let u1 = ((self.next_u64() >> 11) as f64 + 1.0) / (1u64 << 53) as f64;
        let u2 = (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
        (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
    }
}

const N: usize = 1 << 20;

/// 相関係数 rho の白色ステレオを生成（各チャネル分散 1）。
fn make_pair(rho: f64, seed: u64) -> (Vec<f32>, Vec<f32>) {
    let mut rng = Rng(seed);
    let k = (1.0 - rho * rho).max(0.0).sqrt();
    let mut l = Vec::with_capacity(N);
    let mut r = Vec::with_capacity(N);
    for _ in 0..N {
        let x = rng.normal();
        let y = rng.normal();
        l.push(x as f32);
        r.push((rho * x + k * y) as f32);
    }
    (l, r)
}

fn rms(v: &[f32]) -> f64 {
    (v.iter().map(|&s| (s as f64) * (s as f64)).sum::<f64>() / v.len() as f64).sqrt()
}

/// L/R をまとめた全体 RMS。
fn rms2(l: &[f32], r: &[f32]) -> f64 {
    let s: f64 = l.iter().chain(r.iter()).map(|&s| (s as f64) * (s as f64)).sum();
    (s / (l.len() + r.len()) as f64).sqrt()
}

fn db(x: f64) -> f64 {
    20.0 * x.log10()
}

fn main() {
    println!("N = {N} samples/ch, 白色ガウス, 決定論 seed");
    println!();
    println!("rho\tmeas_rho\tt=0 ratio(dB)\tt=0.5 ratio(dB)\tt=1 ratio(dB)");

    let rhos: [f64; 9] = [-1.0, -0.75, -0.5, -0.25, 0.0, 0.25, 0.5, 0.75, 1.0];
    for (i, &rho) in rhos.iter().enumerate() {
        let (l, r) = make_pair(rho, 0x9E3779B97F4A7C15 ^ (i as u64 + 1));
        // 実測相関
        let n = l.len() as f64;
        let cov: f64 = l.iter().zip(&r).map(|(&a, &b)| a as f64 * b as f64).sum::<f64>() / n;
        let meas_rho = cov / (rms(&l) * rms(&r));
        let rin = rms2(&l, &r);

        let mut cols = String::new();
        for &t in &[0.0_f32, 0.5, 1.0] {
            let p = Spread { expander: t, inverse: false, center: false };
            let (mut lo, mut ro) = (l.clone(), r.clone());
            p.process_buffer(&mut lo, &mut ro);
            let ratio = rms2(&lo, &ro) / rin;
            cols += &format!("\t{ratio:.6} ({:+.3})", db(ratio));
        }
        println!("{rho:+.2}\t{meas_rho:+.6}{cols}");
    }

    // 無相関点で入出力 RMS 比 = 1 になる Mid ゲイン α を数値的に求める。
    // t=0 の出力は out_L = out_R = alpha * (L+R)/2 なので
    // alpha_opt = rms_in / rms(mid) を直接測る。
    println!();
    let (l, r) = make_pair(0.0, 0xDEADBEEF);
    let rin = rms2(&l, &r);
    let mid: Vec<f32> = l.iter().zip(&r).map(|(&a, &b)| 0.5 * (a + b)).collect();
    let rmid = rms2(&mid, &mid);
    let alpha_opt = rin / rmid;
    let alpha = 4.0 / 3.0;
    println!("無相関点の最適 Mid ゲイン alpha_opt = {alpha_opt:.6}  ({:+.4} dB)", db(alpha_opt));
    println!("理論値 sqrt(2)                      = {:.6}", 2.0_f64.sqrt());
    println!("実装値 4/3                          = {alpha:.6}  ({:+.4} dB)", db(alpha));
    println!(
        "差: 4/3 / alpha_opt = {:.6}  ({:+.4} dB)",
        alpha / alpha_opt,
        db(alpha / alpha_opt)
    );

    // 相関軸全体で、4/3 と sqrt(2) のどちらが 0 dB に近いかを実測比較する。
    // t=0 の出力比は alpha * rms(M)/rms_in なので、rms(M)/rms_in を測って両者を掛ける。
    println!();
    println!("rho	4/3 (dB)	sqrt2 (dB)	0dB に近い方");
    let sq2 = 2.0_f64.sqrt();
    let mut acc43 = 0.0;
    let mut acc_sq2 = 0.0;
    let mut acc43_pos = 0.0;
    let mut acc_sq2_pos = 0.0;
    let mut n_pos = 0;
    for (i, &rho) in rhos.iter().enumerate() {
        let (l, r) = make_pair(rho, 0x5851F42D4C957F2D ^ (i as u64 + 1));
        let rin = rms2(&l, &r);
        let mid: Vec<f32> = l.iter().zip(&r).map(|(&a, &b)| 0.5 * (a + b)).collect();
        let g = rms2(&mid, &mid) / rin;
        let (d43, dsq) = (db(alpha * g), db(sq2 * g));
        acc43 += d43 * d43;
        acc_sq2 += dsq * dsq;
        if rho >= 0.0 {
            acc43_pos += d43 * d43;
            acc_sq2_pos += dsq * dsq;
            n_pos += 1;
        }
        let win = if d43.abs() < dsq.abs() { "4/3" } else { "sqrt2" };
        println!("{rho:+.2}	{d43:+.3}		{dsq:+.3}		{win}");
    }
    let n = rhos.len() as f64;
    println!();
    println!(
        "全 rho の RMS 偏差:      4/3 = {:.3} dB, sqrt2 = {:.3} dB",
        (acc43 / n).sqrt(),
        (acc_sq2 / n).sqrt()
    );
    println!(
        "rho >= 0 の RMS 偏差:    4/3 = {:.3} dB, sqrt2 = {:.3} dB",
        (acc43_pos / n_pos as f64).sqrt(),
        (acc_sq2_pos / n_pos as f64).sqrt()
    );
    // 出力比 = 1 になる alpha は alpha(rho) = sqrt(2/(1+rho))。
    // 各定数がどの相関で等ラウドになるかを逆算する。
    println!();
    for (name, a) in [("4/3", alpha), ("sqrt2", sq2)] {
        let rho0 = 2.0 / (a * a) - 1.0;
        println!("{name} が等ラウドになる相関: rho = {rho0:+.4}");
    }
}
