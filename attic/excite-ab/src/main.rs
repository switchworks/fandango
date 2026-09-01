//! excite 統合版 (`plans/excite_chebyshev.md` Step 5) の
//! bass_legacy_mix/hi_legacy_mix 挙動確認ハーネス。
//! Chebyshev 相当 (legacy_mix=0.0, デフォルト) と Legacy 相当 (legacy_mix=1.0)
//! を同一 `ExciteChannel` で比較する。
//!
//! 実行: `cargo run -p fandango-excite-ab --release`
//!
//! 1. 倍音スペクトル純度: sin 入力 (bass/hi 各帯域) で wet 成分を DFT で解析し、
//!    基音〜5次倍音のエネルギー比率を legacy_mix=0/1 で比較。
//! 2. 過渡応答再現度: kick 型入力 (減衰 sine) を通し、波形相関・ピーク差を比較。
//! 3. 実素材 A/B レンダリング: 指定 wav を複数パラメータセットで処理し、
//!    legacy_mix=0/1 それぞれの wav を書き出す (官能評価用)。

use std::f32::consts::PI as PI32;
use std::f64::consts::PI as PI64;
use std::path::Path;

use fandango_excite::dsp::{Character, ExciteChannel};

const SR: f32 = 48000.0;
const SOURCE_WAV: &str = "D:/download/swingtechno01_c1f7b9e9-04c6-45cc-984e-1c184c2a2aa4.wav";
const OUT_DIR: &str = "tmp/excite_ab";

fn main() {
    std::fs::create_dir_all(OUT_DIR).expect("create OUT_DIR failed");

    println!("=== 1. 倍音スペクトル純度 (bass 帯域, 50Hz sin) ===");
    harmonic_purity_report(5, true);
    println!("\n=== 1. 倍音スペクトル純度 (hi 帯域, 4500Hz sin) ===");
    // fund=4500Hz: 5 次倍音 22500Hz が Nyquist (24kHz) 内に収まる上限付近。
    // fund=8000Hz だと 5 次 (40kHz) が Nyquist を超えて基音 bin にエイリアス
    // する不具合があったため変更。
    harmonic_purity_report(450, false);

    println!("\n=== 2. 過渡応答再現度 (kick 型入力, bass 帯域) ===");
    transient_response_report();

    println!("\n=== 3. 実素材 A/B レンダリング ===");
    render_ab_sets();
}

/// 実数信号の DFT 振幅 (bin 指定、O(n) per call)。
fn dft_magnitude(signal: &[f32], bin: usize) -> f64 {
    let n = signal.len();
    let mut re = 0.0f64;
    let mut im = 0.0f64;
    for (i, &s) in signal.iter().enumerate() {
        let theta = -2.0 * PI64 * (bin as f64) * (i as f64) / (n as f64);
        re += s as f64 * theta.cos();
        im += s as f64 * theta.sin();
    }
    (re * re + im * im).sqrt()
}

/// bass_amt/hi_amt=1.0, Character::Neutral で sin 入力を通し、dry 減算で wet を
/// 抽出、基音 (1x) 〜 5 次倍音のエネルギー比率を
/// legacy_mix=0.0(chebyshev)/1.0(legacy) で比較する。
///
/// `n=4800` (SR=48000 で厳密に 10Hz/bin) を採用し `fund_bin` から
/// `freq = fund_bin * 10Hz` を逆算することで spectral leakage を排除。
/// フィルタ/エンベロープの settling 用に前段 `n` サンプルを warm-up として捨てる。
fn harmonic_purity_report(fund_bin: usize, is_bass: bool) {
    let n = 4800usize;
    let bin_hz = SR / n as f32;
    let freq = fund_bin as f32 * bin_hz;
    let (bass_a, hi_a) = if is_bass { (1.0, 0.0) } else { (0.0, 1.0) };

    let mut cheby = ExciteChannel::new(SR);
    let mut legacy = ExciteChannel::new(SR);

    // warm-up: フィルタ位相・envelope settling を解析区間から除外
    for i in 0..n {
        let x = (2.0 * PI32 * freq * i as f32 / SR).sin();
        cheby.process(x, bass_a, hi_a, Character::Neutral, 0.0, 0.0);
        legacy.process(x, bass_a, hi_a, Character::Neutral, 1.0, 1.0);
    }

    let mut wet_cheby = vec![0.0f32; n];
    let mut wet_legacy = vec![0.0f32; n];

    for i in 0..n {
        let t = (n + i) as f32;
        let x = (2.0 * PI32 * freq * t / SR).sin();
        let out_c = cheby.process(x, bass_a, hi_a, Character::Neutral, 0.0, 0.0);
        let out_l = legacy.process(x, bass_a, hi_a, Character::Neutral, 1.0, 1.0);
        wet_cheby[i] = out_c - x;
        wet_legacy[i] = out_l - x;
    }

    print_harmonic_table("chebyshev (mix=0)", &wet_cheby, fund_bin);
    print_harmonic_table("legacy (mix=1)", &wet_legacy, fund_bin);
}

fn print_harmonic_table(label: &str, signal: &[f32], fund_bin: usize) {
    let n = signal.len();
    let energies: Vec<(usize, f64)> = (1..=5)
        .map(|k| (k, dft_magnitude(signal, fund_bin * k).powi(2)))
        .collect();
    let total: f64 = (1..n / 2).map(|b| dft_magnitude(signal, b).powi(2)).sum();
    print!("  {label:18}: ");
    for (k, e) in &energies {
        print!("H{k}={:.1}% ", 100.0 * e / total);
    }
    println!();
}

/// kick 型入力 (40Hz, exp(-15t) envelope, amp 0.9) を bass_amt=0.9, Neutral で
/// 通し、legacy_mix=0(chebyshev)/1(legacy) 出力波形の相関係数・ピーク値・
/// ピーク到達時刻差を比較。
fn transient_response_report() {
    let dur_s = 1.0f32;
    let n = (SR * dur_s) as usize;

    let mut cheby = ExciteChannel::new(SR);
    let mut legacy = ExciteChannel::new(SR);

    let mut out_cheby = vec![0.0f32; n];
    let mut out_legacy = vec![0.0f32; n];

    for i in 0..n {
        let t = i as f32 / SR;
        let env = (-t * 15.0).exp();
        let x = 0.9 * env * (2.0 * PI32 * 40.0 * t).sin();
        out_cheby[i] = cheby.process(x, 0.9, 0.0, Character::Neutral, 0.0, 0.0);
        out_legacy[i] = legacy.process(x, 0.9, 0.0, Character::Neutral, 1.0, 1.0);
    }

    let peak_cheby = out_cheby.iter().cloned().fold(0.0f32, |a, b| a.max(b.abs()));
    let peak_legacy = out_legacy.iter().cloned().fold(0.0f32, |a, b| a.max(b.abs()));
    let peak_idx_cheby = out_cheby
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.abs().partial_cmp(&b.1.abs()).unwrap())
        .map(|(i, _)| i)
        .unwrap();
    let peak_idx_legacy = out_legacy
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.abs().partial_cmp(&b.1.abs()).unwrap())
        .map(|(i, _)| i)
        .unwrap();

    let mean_c: f64 = out_cheby.iter().map(|&v| v as f64).sum::<f64>() / n as f64;
    let mean_l: f64 = out_legacy.iter().map(|&v| v as f64).sum::<f64>() / n as f64;
    let mut cov = 0.0f64;
    let mut var_c = 0.0f64;
    let mut var_l = 0.0f64;
    for i in 0..n {
        let dc = out_cheby[i] as f64 - mean_c;
        let dl = out_legacy[i] as f64 - mean_l;
        cov += dc * dl;
        var_c += dc * dc;
        var_l += dl * dl;
    }
    let corr = cov / (var_c.sqrt() * var_l.sqrt());

    println!("  chebyshev (mix=0) : peak={peak_cheby:.4} peak_t={:.4}ms", peak_idx_cheby as f32 / SR * 1000.0);
    println!("  legacy (mix=1)    : peak={peak_legacy:.4} peak_t={:.4}ms", peak_idx_legacy as f32 / SR * 1000.0);
    println!("  波形相関係数 (全区間): {corr:.4}");
}

struct ParamSet {
    name: &'static str,
    bass: f32,
    hi: f32,
    character: Character,
}

fn render_ab_sets() {
    let src = Path::new(SOURCE_WAV);
    if !src.exists() {
        println!("  素材 wav が見つからない: {SOURCE_WAV} — スキップ");
        return;
    }
    let mut reader = hound::WavReader::open(src).expect("wav open failed");
    let spec = reader.spec();
    println!(
        "  素材: {SOURCE_WAV}\n  spec: sr={} ch={} bits={} fmt={:?}",
        spec.sample_rate, spec.channels, spec.bits_per_sample, spec.sample_format
    );
    let sr = spec.sample_rate as f32;
    let channels = spec.channels as usize;

    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().map(|s| s.unwrap()).collect(),
        hound::SampleFormat::Int => {
            let max_val = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.unwrap() as f32 / max_val)
                .collect()
        }
    };
    let n_frames = samples.len() / channels;

    let sets = [
        ParamSet { name: "bass_neutral", bass: 0.7, hi: 0.0, character: Character::Neutral },
        ParamSet { name: "hi_neutral", bass: 0.0, hi: 0.7, character: Character::Neutral },
        ParamSet { name: "mix_bright", bass: 0.3, hi: 0.5, character: Character::Bright },
    ];

    for set in &sets {
        // 各セットを legacy_mix=0 (Chebyshev) と legacy_mix=1 (Legacy) の両方で書き出す。
        render_one(&samples, n_frames, channels, sr, set, 0.0, 0.0, "chebyshev");
        render_one(&samples, n_frames, channels, sr, set, 1.0, 1.0, "legacy");
    }
}

#[allow(clippy::too_many_arguments)]
fn render_one(
    samples: &[f32],
    n_frames: usize,
    channels: usize,
    sr: f32,
    set: &ParamSet,
    bass_legacy_mix: f32,
    hi_legacy_mix: f32,
    suffix: &str,
) {
    let mut ch_l = ExciteChannel::new(sr);
    let mut ch_r = ExciteChannel::new(sr);

    let mut out = vec![0.0f32; samples.len()];

    for f in 0..n_frames {
        let l_idx = f * channels;
        let r_idx = if channels > 1 { f * channels + 1 } else { l_idx };
        let xl = samples[l_idx];
        let xr = samples[r_idx];

        out[l_idx] = ch_l.process(xl, set.bass, set.hi, set.character, bass_legacy_mix, hi_legacy_mix);
        if channels > 1 {
            out[r_idx] = ch_r.process(xr, set.bass, set.hi, set.character, bass_legacy_mix, hi_legacy_mix);
        }
    }

    let spec = hound::WavSpec {
        channels: channels as u16,
        sample_rate: sr as u32,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };

    let path = format!("{OUT_DIR}/{}_{}.wav", set.name, suffix);
    write_wav(&path, &spec, &out);
    println!("  書き出し: {path}");
}

fn write_wav(path: &str, spec: &hound::WavSpec, samples: &[f32]) {
    let mut writer = hound::WavWriter::create(path, *spec).expect("wav create failed");
    for &s in samples {
        writer.write_sample(s).expect("wav write failed");
    }
    writer.finalize().expect("wav finalize failed");
}
