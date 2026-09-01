//! rotate の crest factor 低減量を実素材で実測する。存続 / 作り直しの判定用。
//!
//! `plans/bundle_review_v01_fable.md` §第2便 D-3 の判定条件に対応する。
//! 全グリッドで 0.5 dB 未満なら「規範は満たすが仕事をしない」、
//! 1–2 dB 出る領域があるなら現行のまま用途を絞る。
//!
//! 実行: cargo run -p fandango-rotate --example rotate_crest --release -- <wav パス>

use fandango_rotate::dsp::AllpassChain;

/// crest factor [dB] = peak / RMS。
fn crest_db(x: &[f32]) -> f64 {
    let mut peak = 0.0f64;
    let mut sum_sq = 0.0f64;
    for &s in x {
        let a = s as f64;
        if a.abs() > peak {
            peak = a.abs();
        }
        sum_sq += a * a;
    }
    let rms = (sum_sq / x.len() as f64).sqrt();
    20.0 * (peak / rms).log10()
}

/// 陽性対照。非対称な波形（全高調波を余弦位相で揃えたパルス列）を作る。
///
/// 位相ローテータは「振幅スペクトルを変えずに位相だけ回してピークを下げる」装置なので、
/// 位相が揃って尖った波形に対しては必ず crest factor が下がるはずである。
/// これが下がらないなら実装が壊れている。素材の性質と実装の健全性を分離するために置く。
fn synth_asymmetric(sr: f32, secs: f32, f0: f32, n_harm: usize) -> Vec<f32> {
    let n = (sr * secs) as usize;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f64 / sr as f64;
        let mut y = 0.0f64;
        for h in 1..=n_harm {
            // 全高調波が同位相 = 時間軸で一点に集中する尖ったパルス。
            y += (2.0 * std::f64::consts::PI * f0 as f64 * h as f64 * t).cos() / h as f64;
        }
        out.push((y * 0.15) as f32);
    }
    out
}

/// 1 素材ぶんのグリッド測定を表として出す。最大低減量を返す。
fn run_grid(label: &str, chans: &[Vec<f32>], sr: f32) -> (f64, String) {
    let dry: Vec<f64> = chans.iter().map(|c| crest_db(c)).collect();
    for (i, d) in dry.iter().enumerate() {
        println!("{label} 原音 crest factor [ch{i}]: {d:.2} dB");
    }
    println!();

    let freqs = [50.0f32, 100.0, 200.0, 500.0, 1000.0, 2000.0];
    let stages_list = [2usize, 4, 6, 8];

    print!("{:>10}", "stages/freq");
    for f in freqs {
        print!("{:>9.0}", f);
    }
    println!();
    println!("{}", "-".repeat(10 + 9 * freqs.len()));

    let mut best = f64::NEG_INFINITY;
    let mut best_at = String::new();

    for &stages in &stages_list {
        print!("{stages:>10}");
        for &fc in &freqs {
            // 全チャンネルで最も低減量が小さいものを採る（保守側）。
            let mut worst_gain = f64::INFINITY;
            for (i, c) in chans.iter().enumerate() {
                let mut chain = AllpassChain::new();
                let out: Vec<f32> = c.iter().map(|&x| chain.process(x, fc, sr, stages)).collect();
                let gain = dry[i] - crest_db(&out);
                if gain < worst_gain {
                    worst_gain = gain;
                }
            }
            print!("{worst_gain:>9.2}");
            if worst_gain > best {
                best = worst_gain;
                best_at = format!("stages={stages} freq={fc:.0}Hz");
            }
        }
        println!();
    }
    println!();
    (best, best_at)
}

/// 既定値探索。基音を振って、stages ごとに最も安定して効く fc を探す。
///
/// 単一の合成波で最適点を決めると、その信号の基音に過適合する。
/// 実素材の基音は帯域を持つので、複数 f0 での平均低減量が大きい点を採る。
fn find_default(sr: f32) {
    let f0s = [80.0f32, 110.0, 160.0, 220.0, 300.0];
    let freqs = [100.0f32, 200.0, 300.0, 500.0, 700.0, 1000.0, 1500.0, 2000.0];
    let stages_list = [2usize, 4, 6, 8];

    println!("=== 既定値探索: 基音 {f0s:?} Hz での平均 crest 低減量 [dB] ===");
    println!();
    print!("{:>10}", "stages/freq");
    for f in freqs {
        print!("{:>8.0}", f);
    }
    println!();
    println!("{}", "-".repeat(10 + 8 * freqs.len()));

    for &stages in &stages_list {
        print!("{stages:>10}");
        for &fc in &freqs {
            let mut sum = 0.0f64;
            for &f0 in &f0s {
                let x = synth_asymmetric(sr, 2.0, f0, 40);
                let dry = crest_db(&x);
                let mut chain = AllpassChain::new();
                let out: Vec<f32> = x.iter().map(|&v| chain.process(v, fc, sr, stages)).collect();
                sum += dry - crest_db(&out);
            }
            print!("{:>8.2}", sum / f0s.len() as f64);
        }
        println!();
    }
    println!();
}

fn main() {
    find_default(44_100.0);

    // 陽性対照を先に走らせる。実装が健全かを素材と切り離して確認するため。
    println!("=== 陽性対照: 合成非対称波 (44.1 kHz, 110 Hz, 40 倍音を同位相) ===");
    println!();
    let ctrl = synth_asymmetric(44_100.0, 5.0, 110.0, 40);
    let (ctrl_best, ctrl_at) = run_grid("対照", &[ctrl], 44_100.0);
    println!("対照の最大低減: {ctrl_best:.2} dB ({ctrl_at})");
    if ctrl_best < 1.0 {
        println!("警告: 陽性対照でも下がらない → 実装を疑う");
    } else {
        println!("実装は健全（尖った波形では確かにピークが下がる）");
    }
    println!();
    println!("{}", "=".repeat(70));
    println!();

    let path = std::env::args()
        .nth(1)
        .expect("usage: rotate_crest <wav パス>");

    let mut reader = hound::WavReader::open(&path).expect("wav を開けない");
    let spec = reader.spec();
    let sr = spec.sample_rate as f32;
    let ch = spec.channels as usize;

    // 全チャンネルを f32 に正規化して読む。
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

    // チャンネルごとに分離する。crest factor はチャンネル単位で意味を持つ。
    let n = samples.len() / ch;
    let mut chans: Vec<Vec<f32>> = vec![Vec::with_capacity(n); ch];
    for (i, &s) in samples.iter().enumerate() {
        chans[i % ch].push(s);
    }

    println!("素材: {path}");
    println!("{} Hz / {} ch / {} bit / {:.1} 秒\n", sr, ch, spec.bits_per_sample, n as f32 / sr);

    let (best, best_at) = run_grid("素材", &chans, sr);

    println!("値は crest factor の低減量 [dB]（正 = ピークが下がった）");
    println!("最大低減: {best:.2} dB ({best_at})");
    if best < 0.5 {
        println!("判定: 全グリッドで 0.5 dB 未満 → この素材では仕事をしていない");
    } else {
        println!("判定: 0.5 dB 以上の低減あり → この素材では仕事をしている");
    }
}
