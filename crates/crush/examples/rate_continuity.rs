//! `rate` 非整数化の最小検証ハーネス。
//!
//! **使い捨て。プラグイン本体には触らない。** 位相アキュムレータ版の S&H を
//! 実験用の型として複製し、
//!   1. 整数 `rate` で現行 `CrushChannel` と bit-exact 一致するか
//!   2. 非整数 `rate` の音を wav へ書き出す
//! を行う。1 が通らないうちは 2 を聴かない (境界条件の誤りを音で判断できないため)。
//!
//! 実行: cargo run -p fandango-crush --example rate_continuity --release [-- <out_dir>]

use fandango_crush::dsp::{CrushChannel, DcBlocker};

// ---- 以下 3 関数は `crates/crush/src/dsp.rs` の非公開関数の複製。 ----
// 本体は private なので example から呼べない。**複製がずれていないことは
// `bit_exact_at_integer_rates` が機械判定する** (ずれれば整数 rate で一致しない)。

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

// ---- 複製ここまで ----

/// 位相アキュムレータ版 S&H を持つ実験用チャンネル。
///
/// 現行は `hold_counter = (hold_counter + 1) % rate` の整数分周。
/// ここでは残りサンプル数を f64 で持ち、`acc <= 0` でホールドを更新して `acc += rate`。
/// **整数 `rate` では acc が f64 の整数値のまま推移するので現行と厳密に一致する。**
struct ExpChannel {
    x_prev: f64,
    hold: f32,
    /// 次のホールド更新までの残り (サンプル)。0 以下で更新。
    acc: f64,
    dc_blocker: DcBlocker,
}

impl ExpChannel {
    fn new() -> Self {
        Self { x_prev: 0.0, hold: 0.0, acc: 0.0, dc_blocker: DcBlocker::new() }
    }

    fn set_sample_rate(&mut self, sr: f32) {
        self.dc_blocker.set_sample_rate(sr);
    }

    #[inline]
    fn process(&mut self, x: f32, bits: f32, rate: f64, mix: f32) -> f32 {
        let x_dry = if x.is_finite() { x.clamp(-4.0, 4.0) } else { 0.0 };

        let rate = rate.max(1.0);
        if self.acc <= 0.0 {
            self.hold = x_dry;
            self.acc += rate;
        }
        self.acc -= 1.0;
        let x_held = self.hold;

        let y_raw = shape_adaa_quant(x_held, self.x_prev, bits);
        self.x_prev = x_held as f64;
        let y_gated = denormal_gate(y_raw);
        let y_wet = self.dc_blocker.process(y_gated);

        let out = (1.0 - mix) * x_dry + mix * y_wet;
        if out.is_finite() { out.clamp(-6.0, 6.0) } else { 0.0 }
    }
}

const SR: f32 = 48_000.0;

/// 検証用入力。単一正弦だと S&H の境界差が出にくいので、倍音と過渡を混ぜる。
fn test_input(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| {
            let t = i as f32 / SR;
            let env = (-(t * 3.0)).exp();
            0.6 * (2.0 * std::f32::consts::PI * 220.0 * t).sin()
                + 0.25 * (2.0 * std::f32::consts::PI * 1330.0 * t).sin()
                + 0.15 * env * (2.0 * std::f32::consts::PI * 3700.0 * t).sin()
        })
        .collect()
}

/// 判定 1: 整数 `rate` で現行と bit-exact 一致するか。
fn bit_exact_at_integer_rates() -> bool {
    let x = test_input(48_000);
    let mut ok = true;
    for rate in 1..=32u32 {
        for &bits in &[1.0_f32, 4.0, 8.0, 16.0] {
            let mut a = CrushChannel::new();
            let mut b = ExpChannel::new();
            a.set_sample_rate(SR);
            b.set_sample_rate(SR);
            let mut worst = 0.0_f32;
            let mut diff_at = usize::MAX;
            // F は 1 サンプル遅れるので、A の 1 つ前の出力と突き合わせる
            let mut a_prev: Option<f32> = None;
            for (i, &v) in x.iter().enumerate() {
                let yf = a.process(v, bits, rate as f32, 1.0);
                let ya = b.process(v, bits, rate as f64, 1.0);
                if let Some(prev) = a_prev {
                    if yf.to_bits() != prev.to_bits() {
                        worst = worst.max((yf - prev).abs());
                        if diff_at == usize::MAX {
                            diff_at = i;
                        }
                    }
                }
                a_prev = Some(ya);
            }
            if diff_at != usize::MAX {
                ok = false;
                println!(
                    "  NG rate={rate} bits={bits}: 初回不一致 sample {diff_at} / 最大差 {worst:e}"
                );
            }
        }
    }
    ok
}

/// 実効レートの刻み。対数軸でどれだけ不均等かを数字で出す。
fn print_step_table() {
    println!("  rate   実効レート[Hz]   直前からの段差[oct]");
    let mut prev = 0.0_f64;
    for r in [1u32, 2, 3, 4, 6, 8, 12, 16, 24, 32] {
        let hz = SR as f64 / r as f64;
        let oct = if prev > 0.0 { (prev / hz).log2() } else { f64::NAN };
        if oct.is_nan() {
            println!("  {r:>4}   {hz:>12.1}              -");
        } else {
            println!("  {r:>4}   {hz:>12.1}          {oct:>6.3}");
        }
        prev = hz;
    }
}

/// `rate` を start → end へ線形に掃引しながら書き出す。
/// `quantize_rate` が true なら整数へ丸める (＝現行の挙動の再現)。
#[derive(Clone, Copy, PartialEq)]
enum Path {
    /// 実装済みの本体 (経路 F、区間平均)。
    F,
    /// 旧実装 (経路 A、整数格子)。A/F を耳で比べるための対照。
    A,
}

fn sweep(path: &str, start: f64, end: f64, quantize_rate: bool, bits: f32, which: Path) {
    let secs = 6.0_f32;
    let n = (SR * secs) as usize;
    let x = test_input(n);
    let mut chf = CrushChannel::new();
    let mut cha = ExpChannel::new();
    chf.set_sample_rate(SR);
    cha.set_sample_rate(SR);

    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SR as u32,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(path, spec).expect("create wav");
    for (i, &v) in x.iter().enumerate() {
        let t = i as f64 / (n - 1) as f64;
        let r = start + (end - start) * t;
        let r = if quantize_rate { r.round() } else { r };
        let y = match which {
            Path::F => chf.process(v, bits, r as f32, 1.0),
            Path::A => cha.process(v, bits, r, 1.0),
        };
        w.write_sample(y).expect("write");
    }
    w.finalize().expect("finalize");
    println!("  wrote {path}");
}

fn main() {
    let out_dir = std::env::args().nth(1).unwrap_or_else(|| "D:/output/crush_rate".to_string());
    std::fs::create_dir_all(&out_dir).expect("create out_dir");

    println!("1) 整数 rate での bit-exact 一致 (現行 CrushChannel との比較)");
    let ok = bit_exact_at_integer_rates();
    println!("   => {}", if ok { "一致 (rate 1..32 × bits 1/4/8/16)" } else { "不一致あり" });

    println!("\n2) 実効レートの刻み (sr = {SR} Hz)");
    print_step_table();

    if !ok {
        eprintln!("\nbit-exact が通っていない。音は書き出さない (境界条件の誤りを音で判断できないため)。");
        std::process::exit(1);
    }

    println!("\n3) 掃引の書き出し (bits = 8, mix = 1.0, 6 秒)");
    println!("   stepped      — rate を整数へ丸める。オーナーが「決定的に出ない」と確認した基準");
    println!("   continuous_A — 旧実装で rate を連続変化。**消したい音**が乗っている側");
    println!("   continuous_F — 実装済みの経路 F で rate を連続変化。**判定対象**");
    for (tag, hi) in [("1to8", 8.0_f64), ("1to32", 32.0)] {
        sweep(&format!("{out_dir}/rate_{tag}_stepped.wav"), 1.0, hi, true, 8.0, Path::F);
        sweep(&format!("{out_dir}/rate_{tag}_continuous_A.wav"), 1.0, hi, false, 8.0, Path::A);
        sweep(&format!("{out_dir}/rate_{tag}_continuous_F.wav"), 1.0, hi, false, 8.0, Path::F);
    }
}
