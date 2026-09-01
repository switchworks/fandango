use fandango_raindrop::dsp::{RaindropDsp, RaindropParams};

fn run(width: f32, stereo_in: bool) -> (f32, f32) {
    let sr = 48_000.0f32;
    let mut d = RaindropDsp::new(48_000);
    d.set_sample_rate(sr);
    let p = RaindropParams { delay_samples: 1200, feedback: 0.6, width, mix: 1.0 };
    let n = 48_000usize;
    let mut side = 0.0f32;
    let mut mid = 0.0f32;
    for i in 0..n {
        let t = i as f32 / sr;
        let l = (2.0 * std::f32::consts::PI * 220.0 * t).sin() * 0.5;
        let r = if stereo_in { (2.0 * std::f32::consts::PI * 330.0 * t).sin() * 0.5 } else { l };
        let (ol, or) = d.process_sample(l, r, p);
        side += (0.5 * (ol - or)).powi(2);
        mid += (0.5 * (ol + or)).powi(2);
    }
    ((side / n as f32).sqrt(), (mid / n as f32).sqrt())
}

fn main() {
    for &st in &[false, true] {
        for &w in &[0.0f32, 0.5, 1.0] {
            let (s, m) = run(w, st);
            println!("stereo_in={st} width={w:.1} side_rms={s:.6} mid_rms={m:.6}");
        }
    }
}
