//! drawl → finalist の直列で、母音間の出力レベル差がどこまで縮むかを実測する。
//! 新規 DSP は書かない。既存 2 crate をそのまま繋ぐ運用の測定。

use fandango_drawl::dsp::{DrawlDsp, VOWEL_NAMES};
use fandango_finalist::dsp::Finalist;

const BLOCK: usize = 128;

fn db(x: f32) -> f32 { 20.0 * x.max(1e-12).log10() }
fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| (*v as f64) * (*v as f64)).sum::<f64>() / x.len() as f64).sqrt() as f32
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = args.first().expect("usage: chain_finalist <wav> [gain_db..]");
    let gains: Vec<f32> = if args.len() > 1 {
        args[1..].iter().map(|s| s.parse().unwrap()).collect()
    } else { vec![0.0, 6.0, 12.0, 18.0, 24.0] };

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
    println!("入力 RMS {:.2} dBFS  ({} ch, {:.0} Hz)", db(rms(&il)), ch, sr);
    println!("finalist: ceiling -1.0 dBFS / drive 0 / release 100 ms");
    println!();
    println!("finalist入力ゲイン |     U      O      A      E      I  |  開き");

    for &gdb in &gains {
        let g = 10f32.powf(gdb / 20.0);
        let mut out_rms = Vec::new();
        for v in 0..5 {
            let mut d = DrawlDsp::new(sr);
            let mut f = Finalist::new(sr);
            let rc = f.release_coeff(100.0);
            let ceiling = 10f32.powf(-1.0 / 20.0);
            let mut acc: Vec<f32> = Vec::with_capacity(n);
            let mut i = 0;
            while i < n {
                let m = BLOCK.min(n - i);
                let (mut bl, mut br) = (il[i..i + m].to_vec(), ir[i..i + m].to_vec());
                d.process_block(&mut bl, &mut br, v as f32, 0.0, 1.0);
                for k in 0..m {
                    let (yl, _yr) = f.process(bl[k], br[k], g, 0.0, ceiling, rc, false);
                    acc.push(yl);
                }
                i += m;
            }
            out_rms.push(db(rms(&acc)));
        }
        let sp = out_rms.iter().cloned().fold(f32::MIN, f32::max)
            - out_rms.iter().cloned().fold(f32::MAX, f32::min);
        print!("        {gdb:+6.1} dB    |");
        for r in &out_rms { print!(" {r:6.2}"); }
        println!("  | {sp:5.2} dB");
    }
    let _ = VOWEL_NAMES;
}
