//! 電力正規化の参照スペクトル候補の比較 (plans/parked-decisions.md
//! 「drawl — 電力正規化の参照スペクトルを白色から傾斜つきへ変えるか」)。
//!
//! 実材料の PSD を測り、参照スペクトル `S` を変えたときの母音別 RMS ゲインを予測する。
//! 実行: `cargo run --release -p fandango-drawl --example ref_spectrum -- <wav>`

use fandango_drawl::dsp::{
    bandwidths, chain_response, q_pair, HP_DIVISOR, VOWELS,
};

const NFFT: usize = 4096;

fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let mut j = 0;
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
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * std::f64::consts::PI / len as f64;
        for i in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (c, s) = (ang * k as f64).cos_sin();
                let (ur, ui) = (re[i + k], im[i + k]);
                let (vr, vi) = (
                    re[i + k + len / 2] * c - im[i + k + len / 2] * s,
                    re[i + k + len / 2] * s + im[i + k + len / 2] * c,
                );
                re[i + k] = ur + vr;
                im[i + k] = ui + vi;
                re[i + k + len / 2] = ur - vr;
                im[i + k + len / 2] = ui - vi;
            }
        }
        len <<= 1;
    }
}

trait CosSin {
    fn cos_sin(self) -> (f64, f64);
}
impl CosSin for f64 {
    fn cos_sin(self) -> (f64, f64) {
        (self.cos(), self.sin())
    }
}

/// Welch 法 (Hann, 50% オーバラップ) による片側 PSD。返り値の添字はビン番号。
fn psd(x: &[f32]) -> Vec<f64> {
    let win: Vec<f64> = (0..NFFT)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / NFFT as f64).cos())
        .collect();
    let mut acc = vec![0.0_f64; NFFT / 2 + 1];
    let mut cnt = 0usize;
    let mut off = 0usize;
    while off + NFFT <= x.len() {
        let mut re: Vec<f64> = (0..NFFT).map(|i| x[off + i] as f64 * win[i]).collect();
        let mut im = vec![0.0_f64; NFFT];
        fft(&mut re, &mut im);
        for k in 0..=NFFT / 2 {
            acc[k] += re[k] * re[k] + im[k] * im[k];
        }
        cnt += 1;
        off += NFFT / 2;
    }
    assert!(cnt > 0, "素材が短すぎる");
    acc.iter().map(|v| v / cnt as f64).collect()
}

fn db(x: f64) -> f64 {
    10.0 * x.max(1.0e-300).log10()
}

/// 参照スペクトル候補。`None` = 白色、`Some(wr_hz)` = 1 次 LP `|wr/(s+wr)|^2`。
#[derive(Clone, Copy)]
enum Ref {
    White,
    /// `|wr/(s+wr)|^(2n)`。n=1 で -6 dB/oct、n=2 で -12 dB/oct。
    Lp(f32, u32),
}

fn spread_for(p: &[f64], bin_hz: f32, r: &dyn Fn(f32, f32) -> Ref) -> (Vec<f64>, f64) {
    let mag2 = |v: (f32, f32)| (v.0 as f64) * (v.0 as f64) + (v.1 as f64) * (v.1 as f64);
    let refw = |r: Ref, f: f32| -> f64 {
        match r {
            Ref::White => 1.0,
            Ref::Lp(wr, n) => {
                let x = (f / wr) as f64;
                (1.0 / (1.0 + x * x)).powi(n as i32)
            }
        }
    };
    let pin: f64 = p[1..].iter().sum();
    let mut gains = Vec::new();
    for v in 0..5 {
        let (f1, f2) = VOWELS[v];
        let (q1, q2) = q_pair(f1, f2, 1.0);
        let hp = f1 / HP_DIVISOR;
        let rr = r(f1, f2);
        let (mut num, mut den, mut pout) = (0.0_f64, 0.0_f64, 0.0_f64);
        for k in 1..p.len() {
            let fr = k as f32 * bin_hz;
            let h2 = mag2(chain_response(fr, f1, f2, q1, q2, hp));
            let s2 = refw(rr, fr);
            num += s2;
            den += s2 * h2;
            pout += p[k] * h2;
        }
        gains.push(10.0 * (pout / pin * (num / den)).log10());
    }
    let sp = gains.iter().cloned().fold(f64::MIN, f64::max)
        - gains.iter().cloned().fold(f64::MAX, f64::min);
    (gains, sp)
}

struct Rng(u32);
impl Rng {
    fn f(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 as i32) as f32 / 2_147_483_647.0
    }
}

fn synth_saw(f0: f32, sr: f32, n: usize) -> Vec<f32> {
    (0..n).map(|i| 2.0 * ((i as f32 / sr) * f0).fract() - 1.0).collect()
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: ref_spectrum <wav>");
    let mut rd = hound::WavReader::open(&path).expect("open wav");
    let spec = rd.spec();
    let ch = spec.channels as usize;
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => rd.samples::<f32>().map(|s| s.unwrap()).collect(),
        hound::SampleFormat::Int => {
            let full = (1_i64 << (spec.bits_per_sample - 1)) as f32;
            rd.samples::<i32>().map(|s| s.unwrap() as f32 / full).collect()
        }
    };
    let mono: Vec<f32> = samples.chunks(ch).map(|c| c[0]).collect();
    let sr = spec.sample_rate as f32;
    let bin_hz = sr / NFFT as f32;
    let n = 48_000 * 4;

    let mut rng = Rng(0x1234_5678);
    let mut pink = {
        // 1 次 LP をかけた白色 (-6 dB/oct 近似)。
        let mut y = 0.0_f32;
        (0..n).map(|_| { y += 0.02 * (rng.f() - y); y }).collect::<Vec<f32>>()
    };
    pink.truncate(n);
    let mats: Vec<(&str, Vec<f32>)> = vec![
        ("OPLL wav", mono),
        ("saw 55Hz", synth_saw(55.0, sr, n)),
        ("saw 110Hz", synth_saw(110.0, sr, n)),
        ("white", (0..n).map(|_| rng.f()).collect()),
        ("pinkish", pink),
    ];
    let psds: Vec<(&str, Vec<f64>)> =
        mats.iter().map(|(nm, x)| (*nm, psd(x))).collect();

    for (nm, p) in &psds {
        let (mut sx, mut sy, mut sxx, mut sxy, mut m) = (0.0, 0.0, 0.0, 0.0, 0.0);
        for k in 1..p.len() {
            let f = k as f32 * bin_hz;
            if !(100.0..=5000.0).contains(&f) { continue; }
            let (x, y) = ((f as f64).log2(), db(p[k]));
            sx += x; sy += y; sxx += x * x; sxy += x * y; m += 1.0;
        }
        println!("{:<10} PSD 傾き {:+.2} dB/oct", nm, (m * sxy - sx * sy) / (m * sxx - sx * sx));
    }

    let cands: Vec<(String, Box<dyn Fn(f32, f32) -> Ref>)> = {
        let mut c: Vec<(String, Box<dyn Fn(f32, f32) -> Ref>)> =
            vec![("白色(現行)".into(), Box::new(|_f1, _f2| Ref::White))];
        for wr in [100.0_f32, 150.0, 200.0, 300.0, 500.0] {
            c.push((format!("1次 wr={wr:.0}"), Box::new(move |_f1, _f2| Ref::Lp(wr, 1))));
        }
        for wr in [100.0_f32, 150.0, 200.0, 300.0, 500.0] {
            c.push((format!("2次 wr={wr:.0}"), Box::new(move |_f1, _f2| Ref::Lp(wr, 2))));
        }
        c.push(("2次 wr=F1/3".into(), Box::new(|f1: f32, _f2| Ref::Lp(f1 / HP_DIVISOR, 2))));
        c.push(("2次 wr=B1".into(), Box::new(|f1: f32, _f2| Ref::Lp(bandwidths(f1, 1000.0).0, 2))));
        c
    };

    println!("
母音間の開き [dB] (小さいほど良い)
");
    print!("{:<14}", "参照 S");
    for (nm, _) in &psds { print!("{:>11}", nm); }
    println!("{:>9}", "最悪");
    for (nm, f) in &cands {
        print!("{:<14}", nm);
        let mut worst = 0.0_f64;
        for (_, p) in &psds {
            let (_, sp) = spread_for(p, bin_hz, f.as_ref());
            print!("{:>11.2}", sp);
            worst = worst.max(sp);
        }
        println!("{:>9.2}", worst);
    }
}
