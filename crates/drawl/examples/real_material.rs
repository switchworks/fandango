//! 実素材を実 DSP に通してピーク超過を測る (plans/drawl_q_separation_v01.md §12-4)。
//!
//! 実行: `cargo run --release -p fandango-drawl --example real_material -- <wav> [trim_db..]`

use fandango_drawl::dsp::{DrawlDsp, VOWEL_NAMES};

const BLOCK: usize = 128;
/// 真ピーク用オーバサンプル比と窓付き sinc の半幅 (入力サンプル単位)。
const OS: usize = 4;
const HALF: isize = 16;

fn db(x: f32) -> f32 {
    20.0 * x.max(1.0e-12).log10()
}

/// 窓付き sinc による OS 倍補間での真ピーク。
fn true_peak(x: &[f32]) -> f32 {
    let mut taps = vec![0.0_f32; OS * (2 * HALF as usize)];
    for p in 0..OS {
        let frac = p as f32 / OS as f32;
        for k in 0..2 * HALF as usize {
            let t = (k as isize - HALF + 1) as f32 - frac;
            let s = if t.abs() < 1.0e-7 {
                1.0
            } else {
                (std::f32::consts::PI * t).sin() / (std::f32::consts::PI * t)
            };
            // Blackman 窓
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
                if idx < 0 || idx >= n {
                    continue;
                }
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

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = args.first().expect("usage: real_material <wav> [trim_db..]");
    let trims: Vec<f32> = if args.len() > 1 {
        args[1..].iter().map(|s| s.parse().expect("trim_db")).collect()
    } else {
        vec![0.0, -6.0, -12.0]
    };

    let mut rd = hound::WavReader::open(path).expect("open wav");
    let spec = rd.spec();
    let ch = spec.channels as usize;
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => rd.samples::<f32>().map(|s| s.unwrap()).collect(),
        hound::SampleFormat::Int => {
            let full = (1_i64 << (spec.bits_per_sample - 1)) as f32;
            rd.samples::<i32>().map(|s| s.unwrap() as f32 / full).collect()
        }
    };
    // L / R へ分解 (モノラルなら複製)。
    let n = samples.len() / ch;
    let mut l = Vec::with_capacity(n);
    let mut r = Vec::with_capacity(n);
    for i in 0..n {
        l.push(samples[i * ch]);
        r.push(samples[i * ch + if ch > 1 { 1 } else { 0 }]);
    }
    let sr = spec.sample_rate as f32;

    let in_peak = true_peak(&l).max(true_peak(&r));
    let in_rms = rms(&l).max(rms(&r));
    println!(
        "input: {} ch={} sr={} bits={} {:?} n={} ({:.2}s)",
        path, ch, spec.sample_rate, spec.bits_per_sample, spec.sample_format, n, n as f32 / sr
    );
    println!(
        "input truepeak {:+.2} dBFS / rms {:+.2} dBFS / crest {:.2} dB\n",
        db(in_peak), db(in_rms), db(in_peak) - db(in_rms)
    );

    println!("trim  vowel jit  outTP    outRMS   crest   TPgain  RMSgain  over0dBFS");
    for &trim in &trims {
        let g = 10.0_f32.powf(trim / 20.0);
        for v in 0..5 {
            for &jit in &[0.0_f32, 1.0] {
                let mut d = DrawlDsp::new(sr);
                d.reset();
                let mut ol: Vec<f32> = l.iter().map(|x| x * g).collect();
                let mut or: Vec<f32> = r.iter().map(|x| x * g).collect();
                let mut i = 0;
                while i < n {
                    let e = (i + BLOCK).min(n);
                    d.process_block(&mut ol[i..e], &mut or[i..e], v as f32, jit, 1.0);
                    i = e;
                }
                let tp = true_peak(&ol).max(true_peak(&or));
                let rm = rms(&ol).max(rms(&or));
                let over = ol.iter().chain(or.iter()).filter(|x| x.abs() > 1.0).count();
                println!(
                    "{:+5.0} {:>5} {:.0}   {:+7.2} {:+8.2} {:7.2} {:+7.2} {:+8.2}  {}",
                    trim, VOWEL_NAMES[v], jit,
                    db(tp), db(rm), db(tp) - db(rm),
                    db(tp) - db(in_peak * g), db(rm) - db(in_rms * g),
                    over
                );
            }
        }
        println!();
    }
}
