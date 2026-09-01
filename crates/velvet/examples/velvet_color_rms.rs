//! velvet の wet 単体 RMS が `color` に依存しないかを測る。
//!
//! `plans/invariant_path_v01.md` §5-1 の受け入れ基準に対応する。
//!
//! velvet は「wet 単体 RMS を入力等価に正規化する」と宣言している
//! (`energy_norm(seq) = 1 / sqrt(Σ g(m)²)`)。しかしこの正規化は疎 FIR の
//! タップ包絡しか見ておらず、後段の 1 極 LP (`color`) のエネルギー損失を
//! 計上していなかった。宣言した不変条件のスコープが信号経路の全段を
//! 覆っていない、という穴である。
//!
//! 目標: 全 `color` で 0 dB（入力等価）。0 dB を超える方向の変更ではない。
//!
//! 実行: cargo run -p fandango-velvet --example velvet_color_rms --release

use fandango_velvet::dsp::{
    build_sequence, color_to_cutoff_hz, energy_norm, lp_alpha, DecorrelatorState, OnePoleLp,
    DENSITY,
};

/// lib.rs と同じ L チャンネル用シード。
const L_SEED: u64 = 0x98C94E6B3AA7E83D;
const SR: f32 = 48_000.0;

/// xorshift による決定論的ホワイトノイズ。
fn noise(n: usize) -> Vec<f32> {
    let mut s = 0x2545_F491_4F6C_DD1Du64;
    (0..n)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s >> 40) as f32 / 8_388_608.0 - 1.0 // [-1, 1)
        })
        .collect()
}

fn rms(x: &[f32]) -> f64 {
    let sum: f64 = x.iter().map(|&v| (v as f64) * (v as f64)).sum();
    (sum / x.len() as f64).sqrt()
}

/// wet 単体の RMS を入力 RMS 比 [dB] で返す。
///
/// `lib.rs` の処理順をそのまま再現する:
/// `decorr.process(mid, seq) * norm` → `lp.process(.., alpha)`。
fn wet_rms_db(input: &[f32], length_sec: f32, color: f32) -> f64 {
    let seq = build_sequence(L_SEED, SR, length_sec, DENSITY);
    let norm = energy_norm(&seq);
    let mut decorr = DecorrelatorState::new((SR * length_sec) as u32 + 2);
    let mut lp = OnePoleLp::new();
    let alpha = lp_alpha(color_to_cutoff_hz(color), SR);

    let wet: Vec<f32> = input
        .iter()
        .map(|&x| {
            let w = decorr.process(x, &seq) * norm;
            // lib.rs と同じく LP の後で補償を掛ける。
            lp.process(w, alpha) * ((2.0 - alpha) / alpha).sqrt()
        })
        .collect();

    // 過渡（タップ列が満たされるまで）を捨てる。
    let skip = (SR * length_sec) as usize * 2;
    20.0 * (rms(&wet[skip..]) / rms(&input[skip..])).log10()
}

fn main() {
    let input = noise((SR * 4.0) as usize);
    let lengths = [0.010f32, 0.020, 0.030, 0.040, 0.050];
    let colors = [0.0f32, 0.25, 0.5, 0.75, 1.0];

    println!("velvet wet 単体 RMS（入力比 [dB]、sr = {SR} Hz）");
    println!("目標: 全 color で 0 dB。受け入れ基準は幅 0.2 dB 以内\n");

    print!("{:>10}", "length[ms]");
    for c in colors {
        print!("{:>10.2}", c);
    }
    println!("{:>10}", "幅[dB]");
    println!("{}", "-".repeat(10 + 10 * colors.len() + 10));

    let mut worst = 0.0f64;
    for len in lengths {
        print!("{:>10.0}", len * 1000.0);
        let vals: Vec<f64> = colors
            .iter()
            .map(|&c| wet_rms_db(&input, len, c))
            .collect();
        for v in &vals {
            print!("{v:>10.2}");
        }
        let spread = vals.iter().cloned().fold(f64::MIN, f64::max)
            - vals.iter().cloned().fold(f64::MAX, f64::min);
        println!("{spread:>10.2}");
        worst = worst.max(spread);
    }

    println!("\n最大の幅: {worst:.2} dB");
    if worst <= 0.2 {
        println!("判定: color 不変（幅 0.2 dB 以内）");
    } else {
        println!("判定: color 依存あり");
    }
}
