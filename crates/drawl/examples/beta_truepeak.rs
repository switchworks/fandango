//! F1 追随補正 corr(F1) = (F1/500)^(beta/6.02) を実 DSP 出力に掛けたときの
//! 真ピーク / RMS / 0dBFS 超過を測る。
//!
//! 補正は母音ごとに定数なので、DSP 内スカラに掛けるのと出力に掛けるのは厳密に等価
//! (線形フィルタ × スカラ)。よって drawl 本体は改変しない。
//!
//! 正規化側: "atten" = max(corr)=1 (never-boost 維持) / "boost" = min(corr)=1 (定理を失う)

use fandango_drawl::dsp::{vowel_formants, DrawlDsp};

const BLOCK: usize = 128;
const OS: usize = 4;
const HALF: isize = 16;

fn db(x: f32) -> f32 { 20.0 * x.max(1e-12).log10() }

fn true_peak(x: &[f32]) -> f32 {
    let mut taps = vec![0.0_f32; OS * (2 * HALF as usize)];
    for p in 0..OS {
        let frac = p as f32 / OS as f32;
        for k in 0..2 * HALF as usize {
            let t = (k as isize - HALF + 1) as f32 - frac;
            let s = if t.abs() < 1e-7 { 1.0 }
                    else { (std::f32::consts::PI * t).sin() / (std::f32::consts::PI * t) };
            let u = (k as f32 + 1.0 - frac) / (2.0 * HALF as f32);
            let w = 0.42 - 0.5 * (2.0 * std::f32::consts::PI * u).cos()
                + 0.08 * (4.0 * std::f32::consts::PI * u).cos();
            taps[p * 2 * HALF as usize + k] = s * w;
        }
    }
    let n = x.len() as isize;
    let mut peak = 0.0_f32;
    for i in 0..n {
        for p in 0..OS {
            let mut acc = 0.0_f32;
            for k in 0..2 * HALF {
                let idx = i + k - HALF + 1;
                if idx < 0 || idx >= n { continue; }
                acc += x[idx as usize] * taps[p * 2 * HALF as usize + k as usize];
            }
            peak = peak.max(acc.abs());
        }
    }
    peak
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| (*v as f64) * (*v as f64)).sum::<f64>() / x.len() as f64).sqrt() as f32
}

fn corr(vp: f32, beta: f32) -> f32 {
    let (f1, _) = vowel_formants(vp);
    (f1 / 500.0).powf(beta / 6.02)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = args.first().expect("usage: beta_truepeak <wav>");
    let mut rd = hound::WavReader::open(path).unwrap();
    let spec = rd.spec();
    let ch = spec.channels as usize;
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => rd.samples::<f32>().map(|s| s.unwrap()).collect(),
        hound::SampleFormat::Int => {
            let full = (1_i64 << (spec.bits_per_sample - 1)) as f32;
            rd.samples::<i32>().map(|s| s.unwrap() as f32 / full).collect()
        }
    };
    let n = samples.len() / ch;
    let (mut il, mut ir) = (Vec::with_capacity(n), Vec::with_capacity(n));
    for i in 0..n {
        il.push(samples[i * ch]);
        ir.push(samples[i * ch + if ch > 1 { 1 } else { 0 }]);
    }
    let sr = spec.sample_rate as f32;
    let in_tp = true_peak(&il);
    println!("{}  in TP {:.2} / RMS {:.2} dBFS", path, db(in_tp), db(rms(&il)));

    // vowel 位置は 5 頂点 + 中間 (0.5 刻み)
    let vps: Vec<f32> = (0..=8).map(|i| i as f32 * 0.5).collect();

    for &beta in &[0.0_f32, 3.0, 6.0] {
        let cs: Vec<f32> = vps.iter().map(|v| corr(*v, beta)).collect();
        let cmax = cs.iter().cloned().fold(f32::MIN, f32::max);
        let cmin = cs.iter().cloned().fold(f32::MAX, f32::min);
        for (side, norm) in [("atten", cmax), ("boost", cmin)] {
            if beta == 0.0 && side == "boost" { continue; }
            let mut tps = Vec::new();
            let mut rs = Vec::new();
            let mut over = 0usize;
            for (vi, &vp) in vps.iter().enumerate() {
                for &jit in &[0.0_f32, 1.0] {
                    let g = cs[vi] / norm;
                    let mut d = DrawlDsp::new(sr);
                    let mut ol: Vec<f32> = Vec::with_capacity(n);
                    let mut i = 0;
                    while i < n {
                        let m = BLOCK.min(n - i);
                        let (mut bl, mut br) = (il[i..i + m].to_vec(), ir[i..i + m].to_vec());
                        d.process_block(&mut bl, &mut br, vp, jit, 1.0);
                        for k in 0..m { ol.push(bl[k] * g); }
                        i += m;
                    }
                    over += ol.iter().filter(|v| v.abs() > 1.0).count();
                    tps.push(db(true_peak(&ol)) - db(in_tp));
                    if jit == 0.0 { rs.push(db(rms(&ol))); }
                }
            }
            let tmax = tps.iter().cloned().fold(f32::MIN, f32::max);
            let sp = rs.iter().cloned().fold(f32::MIN, f32::max)
                - rs.iter().cloned().fold(f32::MAX, f32::min);
            let lmax = rs.iter().cloned().fold(f32::MIN, f32::max);
            println!("  beta={beta:4.1} {side:<5} | TPゲイン最大 {tmax:+6.2} dB | RMS開き {sp:5.2} dB \
| 最大母音RMS {lmax:6.2} dBFS | 0dBFS超過 {over}");
        }
    }
}
