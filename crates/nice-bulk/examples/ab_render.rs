//! nice-bulk の**盲検 A/B 用オフラインレンダ**。
//!
//! 機械判定が尽きた論点（音の良し悪し・効きの有無）をオーナーの耳へ渡すための装置。
//! 実測で決着した論点はここでは扱わない（→ `plans/nice_bulk_crossreview_v01.md`）。
//!
//! # 設計方針
//!
//! - **盲検**: 出力ファイル名はシードから決まる無作為な 2 文字。
//!   対応表は `<out_dir>/ANSWER_<seed>.txt` に書き、聴き終わるまで開かないこと
//! - **ラウドネス整合**: 各変種を基準（A 系列の先頭）の RMS に合わせる。
//!   適用したゲインは対応表に記録する。**「大きい方が良く聞こえる」を潰すため**
//! - **決定論**: 同じシード・同じ入力なら毎回同じ割り当てになる。再現できる
//! - **素材はオーナーが用意する**。合成信号では嗜好は決まらない
//!
//! # モード
//!
//! - `hi` — `hi_amount = 0` と指定値。**hi が可聴かを測る**
//! - `bass` — `bass_amount = 0` と指定値。対照として bass 側も同じ形で測る
//! - `hisplit` — hi 分割点 3000 / 4000 / 6000 Hz（`hi_amount` 固定）
//! - `basssplit` — bass 分割点 100 / 150 / 200 Hz（`bass_amount` 固定）
//!
//! # 実行
//!
//! ```text
//! cargo run -p fandango-nice-bulk --example ab_render --release -- \
//!     <in.wav> <out_dir> <hi|bass|hisplit|basssplit> <amount 0..1> [seed]
//! ```

use fandango_nice_bulk::dsp::{
    bass_env_typical, hi_env_typical, AdaaSaturator, NiceBulkChannel, PowerLawGain,
};
use fandango_shared::Lr4Crossover;

/// 分割点を可変にした `NiceBulkChannel` 相当。
///
/// **`crates/nice-bulk/src/dsp.rs` の `VarSplitChannel`（テスト内）と同じ構造。**
/// あちらは `mirror_matches_real_channel` で本物との bit 一致を担保している。
/// ここは `band_sat_ceiling` が private なため公開式を再掲する
/// （`band_sat_ceiling(alpha) = SAT_CEILING * static_makeup(alpha)` 相当の関係は
/// `sat_relative_drive_invariant_to_makeup` が担保。値は `static_transfer` 経由で照合する）。
struct Chain {
    stage_bass: Lr4Crossover,
    stage_hi: Lr4Crossover,
    bass_gain: PowerLawGain,
    hi_gain: PowerLawGain,
    sat_bass: AdaaSaturator,
    sat_hi: AdaaSaturator,
    sat_ceiling_bass: f32,
    sat_ceiling_hi: f32,
}

impl Chain {
    fn new(sr: f32, f_low: f32, f_high: f32, bass_amount: f32, hi_amount: f32) -> Self {
        Self {
            stage_bass: Lr4Crossover::new(sr, f_low),
            stage_hi: Lr4Crossover::new(sr, f_high),
            bass_gain: PowerLawGain::new(sr, 30.0, 150.0),
            hi_gain: PowerLawGain::new(sr, 1.0, 30.0),
            sat_bass: AdaaSaturator::new(),
            sat_hi: AdaaSaturator::new(),
            sat_ceiling_bass: sat_ceiling(bass_amount),
            sat_ceiling_hi: sat_ceiling(hi_amount),
        }
    }

    fn process(&mut self, x: f32, bass_amount: f32, hi_amount: f32) -> f32 {
        let x = if x.is_finite() { x.clamp(-4.0, 4.0) } else { 0.0 };
        let (low, rest) = self.stage_bass.split(x);
        let (mid, high) = self.stage_hi.split(rest);
        let low_out = self.bass_gain.process(low, bass_amount, bass_env_typical());
        let high_out = self.hi_gain.process(high, hi_amount, hi_env_typical());
        let low_sat = if bass_amount <= 1e-4 {
            self.sat_bass.prime(low_out);
            low_out
        } else {
            self.sat_bass.process(low_out, self.sat_ceiling_bass)
        };
        let high_sat = if hi_amount <= 1e-4 {
            self.sat_hi.prime(high_out);
            high_out
        } else {
            self.sat_hi.process(high_out, self.sat_ceiling_hi)
        };
        let out = low_sat + mid + high_sat;
        if out.is_finite() { out } else { 0.0 }
    }
}

/// `dsp.rs` の `band_sat_ceiling` と同じ式（`SAT_CEILING * static_makeup(alpha)`）。
/// 値は `fandango_nice_bulk::dsp::static_transfer` と突き合わせて自己検査する。
fn sat_ceiling(alpha: f32) -> f32 {
    const SAT_CEILING: f32 = 4.0;
    const G0_MAX: f32 = 2.0;
    SAT_CEILING * (1.0 + (G0_MAX - 1.0) * alpha.clamp(0.0, 1.0))
}

fn rms(x: &[f32]) -> f64 {
    if x.is_empty() {
        return 0.0;
    }
    (x.iter().map(|&v| (v as f64) * (v as f64)).sum::<f64>() / x.len() as f64).sqrt()
}

/// シードから 2 文字の識別子を作る（決定論）。
fn tag(seed: u64, i: usize) -> String {
    const A: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ"; // I/O を除く
    let mut s = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(i as u64 + 1);
    s ^= s >> 33;
    s = s.wrapping_mul(0xff51afd7ed558ccd);
    s ^= s >> 33;
    let a = A[(s % A.len() as u64) as usize] as char;
    let b = A[((s / 24) % A.len() as u64) as usize] as char;
    format!("{a}{b}")
}

struct Variant {
    label: String,
    f_low: f32,
    f_high: f32,
    bass: f32,
    hi: f32,
}

/// 写し（`Chain`）が本物（`NiceBulkChannel`）と 150/4000 で bit 一致することを起動時に確認する。
///
/// `band_sat_ceiling` が private のため式を再掲している。**ずれたら測定は全部無効**なので
/// 黙って進めず、ここで落とす。
fn self_check() {
    let sr = 48_000.0f32;
    let mut st = 0x2545F4914F6CDD1Du64;
    let mut rng = move || {
        st ^= st << 13;
        st ^= st >> 7;
        st ^= st << 17;
        ((st >> 40) as f32 / 8_388_608.0) - 1.0
    };
    let x: Vec<f32> = (0..4800)
        .map(|i| {
            let t = i as f32 / sr;
            let tp = std::f32::consts::TAU;
            0.3 * (tp * 45.0 * t).sin() + 0.2 * (tp * 220.0 * t).sin()
                + 0.1 * (tp * 6000.0 * t).sin()
                + 0.05 * rng()
        })
        .collect();
    for &(b, h) in &[(0.0f32, 0.0f32), (0.5, 0.0), (0.0, 0.5), (1.0, 1.0), (0.37, 0.73)] {
        let mut real = NiceBulkChannel::new(sr);
        let mut mirror = Chain::new(sr, 150.0, 4000.0, b, h);
        for &v in &x {
            let a = real.process(v, b, h);
            let c = mirror.process(v, b, h);
            assert_eq!(
                a, c,
                "写しが本物と一致しない (bass={b}, hi={h})。ab_render の Chain を修正すること"
            );
        }
    }
    println!("自己検査: 写し == 本物 (150/4000, 5 通りのノブ位置で bit 一致)");
}

fn main() {
    self_check();
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 5 {
        eprintln!(
            "usage: ab_render <in.wav> <out_dir> <hi|bass|hisplit|basssplit> <amount 0..1> [seed]"
        );
        std::process::exit(2);
    }
    let in_path = &args[1];
    let out_dir = &args[2];
    let mode = args[3].as_str();
    let amount: f32 = args[4].parse().expect("amount は 0..1 の実数");
    assert!(
        (0.0..=1.0).contains(&amount),
        "amount は 0..1。受け取った値: {amount}"
    );
    let seed: u64 = args
        .get(5)
        .map(|s| s.parse().expect("seed は整数"))
        .unwrap_or(20260830);

    // --- 入力 ---
    let mut reader = hound::WavReader::open(in_path)
        .unwrap_or_else(|e| panic!("入力 WAV を開けない: {in_path}: {e}"));
    let spec = reader.spec();
    let sr = spec.sample_rate as f32;
    assert!(
        spec.channels == 1 || spec.channels == 2,
        "対応するのは 1ch / 2ch。受け取った値: {}",
        spec.channels
    );
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().map(|s| s.unwrap()).collect(),
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.unwrap() as f32 * scale)
                .collect()
        }
    };
    let ch = spec.channels as usize;
    let frames = samples.len() / ch;
    println!(
        "入力: {in_path}  {} Hz / {} ch / {:.2} 秒",
        sr,
        ch,
        frames as f32 / sr
    );

    // --- 変種の定義 ---
    let variants: Vec<Variant> = match mode {
        "hi" => vec![
            Variant { label: format!("hi=0.00 (無処理)"), f_low: 150.0, f_high: 4000.0, bass: 0.0, hi: 0.0 },
            Variant { label: format!("hi={amount:.2}"), f_low: 150.0, f_high: 4000.0, bass: 0.0, hi: amount },
        ],
        "bass" => vec![
            Variant { label: format!("bass=0.00 (無処理)"), f_low: 150.0, f_high: 4000.0, bass: 0.0, hi: 0.0 },
            Variant { label: format!("bass={amount:.2}"), f_low: 150.0, f_high: 4000.0, bass: amount, hi: 0.0 },
        ],
        "hisplit" => [3000.0f32, 4000.0, 6000.0]
            .iter()
            .map(|&f| Variant {
                label: format!("hi 分割点 {f:.0} Hz (hi={amount:.2})"),
                f_low: 150.0,
                f_high: f,
                bass: 0.0,
                hi: amount,
            })
            .collect(),
        "basssplit" => [100.0f32, 150.0, 200.0]
            .iter()
            .map(|&f| Variant {
                label: format!("bass 分割点 {f:.0} Hz (bass={amount:.2})"),
                f_low: f,
                f_high: 4000.0,
                bass: amount,
                hi: 0.0,
            })
            .collect(),
        other => panic!("未知のモード: {other}"),
    };

    // --- レンダ ---
    let mut rendered: Vec<Vec<f32>> = Vec::new();
    for v in &variants {
        let mut chans: Vec<Chain> = (0..ch)
            .map(|_| Chain::new(sr, v.f_low, v.f_high, v.bass, v.hi))
            .collect();
        let mut out = Vec::with_capacity(samples.len());
        for f in 0..frames {
            for c in 0..ch {
                out.push(chans[c].process(samples[f * ch + c], v.bass, v.hi));
            }
        }
        rendered.push(out);
    }

    // --- ラウドネス整合（基準は変種 0）---
    let ref_rms = rms(&rendered[0]);
    let mut gains_db = Vec::new();
    for out in rendered.iter_mut() {
        let r = rms(out);
        let g = if r > 1e-12 { (ref_rms / r) as f32 } else { 1.0 };
        gains_db.push(20.0 * (g as f64).max(1e-12).log10());
        for s in out.iter_mut() {
            *s *= g;
        }
    }

    // --- 書き出し ---
    std::fs::create_dir_all(out_dir).expect("出力ディレクトリを作れない");
    let mut key = format!(
        "nice-bulk 盲検 A/B 対応表\n\
         入力      : {in_path}\n\
         モード    : {mode}   amount = {amount:.2}   seed = {seed}\n\
         sr / ch   : {sr} Hz / {ch} ch\n\
         ラウドネス: 変種 0 の RMS へ整合済み\n\n"
    );
    for (i, v) in variants.iter().enumerate() {
        let t = tag(seed, i);
        let path = format!("{out_dir}/{mode}_{t}.wav");
        let peak = rendered[i].iter().fold(0.0f32, |m, &x| m.max(x.abs()));
        let out_spec = hound::WavSpec {
            channels: spec.channels,
            sample_rate: spec.sample_rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut w = hound::WavWriter::create(&path, out_spec).expect("WAV を書けない");
        for &s in &rendered[i] {
            w.write_sample(s).unwrap();
        }
        w.finalize().unwrap();
        println!("  {t}  ->  {path}   (peak {peak:.3})");
        key.push_str(&format!(
            "  {t}  =  {}\n        整合ゲイン {:+.3} dB / peak {:.4}\n",
            v.label, gains_db[i], peak
        ));
        if peak > 1.0 {
            println!("      ※ peak > 1.0。再生系のクリップに注意");
            key.push_str("        ※ peak > 1.0\n");
        }
    }
    let key_path = format!("{out_dir}/ANSWER_{seed}.txt");
    std::fs::write(&key_path, key).expect("対応表を書けない");
    println!("\n対応表: {key_path}");
    println!("**聴き終わるまで開かないこと。**");
}
