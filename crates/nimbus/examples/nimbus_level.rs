//! nimbus の設計レベルを数値で確認する。
//!
//! # 前提（これを外すと数字の意味が変わる）
//!
//! nimbus は **センドバス用**であり、インサートで直挿しする装置ではない。
//! `mix = 100%` が既定なのは発注仕様であって惰性ではなく、参照機は Lexicon Pantheon。
//! **センドの送り量は -18 dB 程度**を前提に設計・較正されている。
//!
//! `TARGET_ENERGY_DB = 13.0` は「wet-only (r=1, m=1) の総エネルギーを
//! dry インパルス基準で +13 dB にする」という室の物理に基づく目標値である。
//! よって 0 dBFS の密な素材をインサートで直挿しすれば当然クリップするが、
//! **それは設計の欠陥ではなく前提の取り違えである**（2026-08-24 に一度誤診した）。
//!
//! 本 example はその前提込みで挙動を数値化し、次に誰かが「割れている」と思ったときに
//! 前提を確認できるようにするために置く。
//!
//! # この数字の読み方（重要）
//!
//! 刺激はホワイトノイズで、**実素材より遥かに厳しい最悪ケース**である。
//! 連続・広帯域・クレストが低いので、インパルス応答の総エネルギーがそのまま
//! 定常ゲインとして出る。実素材（ピアノ等）はスパースなのでここまで積み上がらない。
//! **本 example の数値だけで可否を判定してはならない。** 前提の把握と回帰の検出に使う。
//!
//! 実行: cargo run -p fandango-nimbus --example nimbus_level --release

use fandango_nimbus::dsp::{hf_shelf_coeffs, hpf_coeffs, ErFir};

const SR: f32 = 48_000.0;

/// 設計が前提とするセンド送り量 [dB]。
const SEND_LEVEL_DB: f32 = -18.0;

/// xorshift による決定論的ホワイトノイズ。
fn noise(n: usize, amp: f32) -> Vec<f32> {
    let mut s = 0x2545_F491_4F6C_DD1Du64;
    (0..n)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            let u = (s >> 40) as f32 / 8_388_608.0 - 1.0; // [-1, 1)
            u * amp
        })
        .collect()
}

struct Stat {
    peak: f32,
    rms: f32,
}

fn run(input: &[f32], distance: f32, mix: f32) -> Stat {
    let mut er = ErFir::new(SR);
    let shelf = hf_shelf_coeffs(SR, distance);
    let hpf = hpf_coeffs(SR);

    let mut peak = 0.0f32;
    let mut sum_sq = 0.0f64;
    for &x in input {
        let (out_l, out_r) = er.process(x, x, distance, mix, &shelf, &hpf);
        let outs: [f32; 2] = [out_l, out_r];
        for v in outs {
            assert!(v.is_finite(), "出力が有限でない: distance={distance} mix={mix}");
            peak = peak.max(v.abs());
            sum_sq += (v as f64) * (v as f64);
        }
    }
    Stat {
        peak,
        rms: (sum_sq / (input.len() * 2) as f64).sqrt() as f32,
    }
}

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-12).log10()
}

fn from_db(x: f32) -> f32 {
    10f32.powf(x / 20.0)
}

fn table(label: &str, input: &[f32]) {
    let in_peak = input.iter().fold(0.0f32, |a, &b| a.max(b.abs()));
    println!("=== {label}（入力ピーク {:.1} dBFS）===", db(in_peak));
    println!(
        "{:>10} {:>8} {:>12} {:>12}",
        "distance", "mix", "出力[dBFS]", "RMS[dB]"
    );
    println!("{}", "-".repeat(46));
    for &d in &[0.0f32, 0.5, 1.0] {
        for &m in &[0.5f32, 1.0] {
            let s = run(input, d, m);
            // 「超えた = 欠陥」ではない。刺激が最悪ケースであることを踏まえて読む。
            let flag = if s.peak > 1.0 { "  *" } else { "" };
            println!(
                "{d:>10.2} {m:>8.2} {:>12.2} {:>12.2}{flag}",
                db(s.peak),
                db(s.rms)
            );
        }
    }
    println!();
}

fn main() {
    let n = (SR * 2.0) as usize;
    println!("nimbus 設計レベル確認 (sr = {SR} Hz)");
    println!("前提: センドバス用・送り {SEND_LEVEL_DB:.0} dB・mix 100% が既定（Lexicon Pantheon 参照）\n");

    // 設計どおりの使い方。ここでクリップしないことが本 example の主眼。
    let send = from_db(SEND_LEVEL_DB);
    table("設計どおり: センド送り -18 dB", &noise(n, send));

    // 前提を外した場合。設計の欠陥ではなく、使い方が仕様外であることを示すための対照。
    table("仕様外（対照）: 0 dBFS 直挿し", &noise(n, 1.0));

    println!("* は 0 dBFS 超。ホワイトノイズは最悪ケースの刺激なので、これ自体は欠陥を意味しない。");
    println!("実素材での可否は耳で確認すること（2026-08-24: 綺麗なピアノ・distance 最大で問題なしを確認済み）。");
}
