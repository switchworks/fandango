//! 母音間の出力レベル差を「系統成分（F1/F2 の位置 × 素材の傾き）」と
//! 「素材成分（倍音の当たり外れ）」に分解する。
//! 正規化済みチェーンを各素材 PSD に掛けた通過電力を母音別に出す。

use fandango_drawl::dsp::{chain_response, never_boost_scale, vowel_formants, HP_DIVISOR};

/// B = B0 * (f/1000)^alpha。alpha=0 は案K(B固定)、alpha=1 は定Q。
fn q_pair_alpha(f1: f32, f2: f32, alpha: f32) -> (f32, f32) {
    let b1 = 50.0 * (f1 / 1000.0).powf(alpha);
    let b2 = 70.0 * (f2 / 1000.0).powf(alpha);
    (f1 / b1, f2 / b2)
}

const NFFT: usize = 4096;

fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 { j ^= bit; bit >>= 1; }
        j |= bit;
        if i < j { re.swap(i, j); im.swap(i, j); }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * std::f64::consts::PI / len as f64;
        for i in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (c, s) = ((ang * k as f64).cos(), (ang * k as f64).sin());
                let (ur, ui) = (re[i + k], im[i + k]);
                let (vr, vi) = (re[i + k + len / 2] * c - im[i + k + len / 2] * s,
                                re[i + k + len / 2] * s + im[i + k + len / 2] * c);
                re[i + k] = ur + vr; im[i + k] = ui + vi;
                re[i + k + len / 2] = ur - vr; im[i + k + len / 2] = ui - vi;
            }
        }
        len <<= 1;
    }
}

/// Welch PSD (Hann, 50% overlap)
fn psd(x: &[f32]) -> Vec<f64> {
    let mut acc = vec![0.0_f64; NFFT / 2 + 1];
    let mut nseg = 0usize;
    let mut off = 0usize;
    while off + NFFT <= x.len() {
        let mut re: Vec<f64> = (0..NFFT).map(|i| {
            let w = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / NFFT as f64).cos();
            x[off + i] as f64 * w
        }).collect();
        let mut im = vec![0.0_f64; NFFT];
        fft(&mut re, &mut im);
        for k in 0..=NFFT / 2 { acc[k] += re[k] * re[k] + im[k] * im[k]; }
        nseg += 1;
        off += NFFT / 2;
    }
    for v in acc.iter_mut() { *v /= nseg.max(1) as f64; }
    acc
}

struct Rng(u32);
impl Rng {
    fn f(&mut self) -> f32 {
        self.0 ^= self.0 << 13; self.0 ^= self.0 >> 17; self.0 ^= self.0 << 5;
        (self.0 as i32) as f32 / 2_147_483_647.0
    }
}
fn saw(f0: f32, sr: f32, n: usize) -> Vec<f32> {
    (0..n).map(|i| 2.0 * ((i as f32 / sr) * f0).fract() - 1.0).collect()
}

fn main() {
    let sr = 44100.0_f32;
    let n = 44100 * 4;
    let path = std::env::args().nth(1);
    let mut mats: Vec<(String, Vec<f32>)> = Vec::new();
    if let Some(p) = path {
        let mut rd = hound::WavReader::open(&p).unwrap();
        let spec = rd.spec();
        let ch = spec.channels as usize;
        let s: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Float => rd.samples::<f32>().map(|v| v.unwrap()).collect(),
            hound::SampleFormat::Int => {
                let m = (1_i64 << (spec.bits_per_sample - 1)) as f32;
                rd.samples::<i32>().map(|v| v.unwrap() as f32 / m).collect()
            }
        };
        mats.push(("OPLL wav".into(), s.chunks(ch).map(|c| c[0]).collect()));
    }
    let mut rng = Rng(0x1234_5678);
    for f0 in [55.0_f32, 110.0, 165.0, 220.0, 330.0] {
        mats.push((format!("saw {f0:.0}Hz"), saw(f0, sr, n)));
    }
    mats.push(("white".into(), (0..n).map(|_| rng.f()).collect()));
    {
        let mut y = 0.0_f32;
        let mut v = Vec::with_capacity(n);
        for _ in 0..n { y += 0.02 * (rng.f() - y); v.push(y); }
        mats.push(("pink~-6dB/oct".into(), v));
    }

    let bin_hz = sr / NFFT as f32;
    let psds: Vec<(String, Vec<f64>)> =
        mats.iter().map(|(nm, x)| (nm.clone(), psd(x))).collect();
    println!("alpha: 0=案K(B固定) 1=定Q(旧Q_BASE相当)。値は母音間の出力レベル開き [dB]");
    print!("{:<14} |", "素材");
    for a in [0.0_f32, 0.25, 0.5, 0.75, 1.0] { print!(" a={a:<5.2}"); }
    println!();
    for (nm, p) in &psds {
        print!("{nm:<14} |");
        for alpha in [0.0_f32, 0.25, 0.5, 0.75, 1.0] {
            let mut g = Vec::new();
            for vp in 0..5 {
                let (f1, f2) = vowel_formants(vp as f32);
                let (q1, q2) = q_pair_alpha(f1, f2, alpha);
                let hp = f1 / HP_DIVISOR;
                let s = never_boost_scale(f1, f2, q1, q2, hp);
                let (mut pout, mut pin) = (0.0_f64, 0.0_f64);
                for k in 1..p.len() {
                    let f = k as f32 * bin_hz;
                    let (re, im) = chain_response(f, f1, f2, q1, q2, hp);
                    let h = (s * (re * re + im * im).sqrt()) as f64;
                    pout += p[k] * h * h;
                    pin += p[k];
                }
                g.push(10.0 * (pout / pin).log10());
            }
            let sp = g.iter().cloned().fold(f64::MIN, f64::max)
                - g.iter().cloned().fold(f64::MAX, f64::min);
            print!(" {sp:6.2} ");
        }
        println!();
    }
    println!();
    println!("F1 追随補正 corr(F1)=(F1/500)^(beta/6.02)。beta [dB/oct]。");
    println!("減衰のみ(max=1 に正規化 → never-boost 維持)。左=母音間の開き, 右=最大母音のレベル [dB]");
    print!("{:<14} |", "素材");
    for b in [0.0_f32, 3.0, 6.0, 9.0, 12.0] { print!("   b={b:<4.0}      "); }
    println!();
    for (nm, p) in &psds {
        print!("{nm:<14} |");
        for beta in [0.0_f32, 3.0, 6.0, 9.0, 12.0] {
            let mut g = Vec::new();
            let mut corr = Vec::new();
            for vp in 0..5 {
                let (f1, f2) = vowel_formants(vp as f32);
                let (q1, q2) = q_pair_alpha(f1, f2, 0.0);
                let hp = f1 / HP_DIVISOR;
                let s = never_boost_scale(f1, f2, q1, q2, hp);
                let c = (f1 / 500.0).powf(beta / 6.02);
                corr.push(c);
                let (mut pout, mut pin) = (0.0_f64, 0.0_f64);
                for k in 1..p.len() {
                    let f = k as f32 * bin_hz;
                    let (re, im) = chain_response(f, f1, f2, q1, q2, hp);
                    let h = (s * (re * re + im * im).sqrt()) as f64;
                    pout += p[k] * h * h;
                    pin += p[k];
                }
                g.push(10.0 * (pout / pin).log10());
            }
            let cmax = corr.iter().cloned().fold(f32::MIN, f32::max);
            for (i, c) in corr.iter().enumerate() {
                g[i] += 20.0 * (*c / cmax).log10() as f64;
            }
            let hi = g.iter().cloned().fold(f64::MIN, f64::max);
            let sp = hi - g.iter().cloned().fold(f64::MAX, f64::min);
            print!(" {sp:5.1}/{hi:6.1} ");
        }
        println!();
    }
    println!();
    println!("HP 設定別の母音間開き [dB]。div=F1/n 追従、fix=固定Hz、off=HPなし");
    let hpmodes: [(&str, Option<f32>, bool); 7] = [
        ("div3(現行)", None, false), ("div6", None, false), ("div1.5", None, false),
        ("fix 60Hz", Some(60.0), false), ("fix 120Hz", Some(120.0), false),
        ("fix 250Hz", Some(250.0), false), ("off", None, true),
    ];
    let divs = [3.0_f32, 6.0, 1.5, 0.0, 0.0, 0.0, 0.0];
    print!("{:<14} |", "素材");
    for (nm, _, _) in &hpmodes { print!(" {nm:>10}"); }
    println!();
    for (nm, p) in &psds {
        print!("{nm:<14} |");
        for (mi, (_, fixhz, off)) in hpmodes.iter().enumerate() {
            let mut g = Vec::new();
            for vp in 0..5 {
                let (f1, f2) = vowel_formants(vp as f32);
                let (q1, q2) = q_pair_alpha(f1, f2, 0.0);
                let hp = if *off { 1.0 } else if let Some(h) = fixhz { *h } else { f1 / divs[mi] };
                let s = never_boost_scale(f1, f2, q1, q2, hp);
                let (mut pout, mut pin) = (0.0_f64, 0.0_f64);
                for k in 1..p.len() {
                    let f = k as f32 * bin_hz;
                    let (re, im) = chain_response(f, f1, f2, q1, q2, hp);
                    let h = (s * (re * re + im * im).sqrt()) as f64;
                    pout += p[k] * h * h;
                    pin += p[k];
                }
                g.push(10.0 * (pout / pin).log10());
            }
            let sp = g.iter().cloned().fold(f64::MIN, f64::max)
                - g.iter().cloned().fold(f64::MAX, f64::min);
            print!(" {sp:10.2}");
        }
        println!();
    }
    println!();
    println!("素材           |    U      O      A      E      I  | 開き | 順位");
    for (nm, p) in &psds {
        let p = p.clone();
        let mut g = Vec::new();
        for vp in 0..5 {
            let (f1, f2) = vowel_formants(vp as f32);
            let (q1, q2) = q_pair_alpha(f1, f2, 0.0);
            let hp = f1 / HP_DIVISOR;
            let s = never_boost_scale(f1, f2, q1, q2, hp);
            let (mut pout, mut pin) = (0.0_f64, 0.0_f64);
            for k in 1..p.len() {
                let f = k as f32 * bin_hz;
                let (re, im) = chain_response(f, f1, f2, q1, q2, hp);
                let h = (s * (re * re + im * im).sqrt()) as f64;
                pout += p[k] * h * h;
                pin += p[k];
            }
            g.push(10.0 * (pout / pin).log10());
        }
        let sp = g.iter().cloned().fold(f64::MIN, f64::max)
            - g.iter().cloned().fold(f64::MAX, f64::min);
        // 大きい順の母音順位
        let mut idx: Vec<usize> = (0..5).collect();
        idx.sort_by(|a, b| g[*b].partial_cmp(&g[*a]).unwrap());
        let order: String = idx.iter().map(|i| ["U","O","A","E","I"][*i]).collect::<Vec<_>>().join(">");
        print!("{nm:<14} |");
        for v in &g { print!(" {v:6.2}"); }
        println!(" |{sp:5.1} | {order}");
    }
}
