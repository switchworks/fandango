//! drawl 検証ハーネス (plans/drawl_formant_v01.md §6-2 / 6-3 / 6-4)。
//!
//! 実行: `cargo run --release -p fandango-drawl --example measure`

use fandango_drawl::dsp::DrawlDsp;

const SR: f32 = 48_000.0;
const BLOCK: usize = 128;

struct Rng(u32);
impl Rng {
    fn next_f32(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 as i32) as f32 / 2_147_483_647.0
    }
}

/// 白色雑音 (振幅 amp)。
fn white(n: usize, amp: f32, seed: u32) -> Vec<f32> {
    let mut rng = Rng(seed);
    (0..n).map(|_| amp * rng.next_f32()).collect()
}

/// 低域主体素材: 40〜120 Hz の正弦 5 本 (等パワー)。
fn low_heavy(n: usize, amp: f32) -> Vec<f32> {
    let freqs = [40.0_f32, 60.0, 80.0, 100.0, 120.0];
    let g = amp / (freqs.len() as f32).sqrt();
    (0..n)
        .map(|i| {
            let t = i as f32 / SR;
            freqs
                .iter()
                .map(|f| g * (2.0 * std::f32::consts::PI * f * t).sin())
                .sum::<f32>()
        })
        .collect()
}

/// 現実的なベース素材: 55 Hz 鋸波 (倍音が 5 kHz まで伸びる)。
/// (b) の純正弦のみの信号は F1/K のハイパス遷移域に全エネルギーが載る病的入力なので、
/// 倍音を持つ実素材との差を見るために併置する。
fn bass_saw(n: usize, amp: f32) -> Vec<f32> {
    (0..n)
        .map(|i| {
            let t = i as f32 / SR;
            amp * (2.0 * (t * 55.0).fract() - 1.0)
        })
        .collect()
}

/// 広帯域素材: 100 Hz 鋸波 + 4 Hz 周期のノイズバースト (ドラム的アタック)。
fn broadband(n: usize, amp: f32, seed: u32) -> Vec<f32> {
    let mut rng = Rng(seed);
    (0..n)
        .map(|i| {
            let t = i as f32 / SR;
            let saw = 2.0 * (t * 100.0).fract() - 1.0;
            let phase = (t * 4.0).fract();
            let burst = (-phase * 30.0).exp() * rng.next_f32();
            amp * (0.6 * saw + 0.8 * burst)
        })
        .collect()
}

/// 100 ms 窓の RMS 列を dB で返す。無音窓 (< -80 dBFS) は除外。
fn rms_windows_db(x: &[f32]) -> Vec<f32> {
    let w = (SR * 0.1) as usize;
    x.chunks(w)
        .filter(|c| c.len() == w)
        .map(|c| {
            let p: f64 = c.iter().map(|v| (*v as f64) * (*v as f64)).sum::<f64>() / w as f64;
            20.0 * (p.sqrt().max(1e-12)).log10() as f32
        })
        .filter(|d| *d > -80.0)
        .collect()
}

fn min_max(v: &[f32]) -> (f32, f32) {
    v.iter().fold((f32::MAX, f32::MIN), |(lo, hi), x| (lo.min(*x), hi.max(*x)))
}

fn mean(v: &[f32]) -> f32 {
    v.iter().sum::<f32>() / v.len() as f32
}

/// vowel を 0→4 に定速スイープしながら処理し、100 ms 窓 RMS(dB) 列を返す。
fn sweep_rms_db(input: &[f32], jitter: f32) -> Vec<f32> {
    let n = input.len();
    let mut dsp = DrawlDsp::new(SR);
    let mut out = Vec::with_capacity(n);
    let mut pos = 0;
    while pos + BLOCK <= n {
        let vowel = 4.0 * (pos as f32 / n as f32);
        let mut l: Vec<f32> = input[pos..pos + BLOCK].to_vec();
        let mut r = l.clone();
        dsp.process_block(&mut l, &mut r, vowel, jitter, 1.0);
        out.extend_from_slice(&l);
        pos += BLOCK;
    }
    rms_windows_db(&out)
}

fn report(label: &str, db: &[f32]) {
    let (lo, hi) = min_max(db);
    println!(
        "{label:<28} mean {:7.2} dB   min {:7.2}   max {:7.2}   変動 ±{:.2} dB",
        mean(db),
        lo,
        hi,
        (hi - lo) / 2.0
    );
}

/// 出力包絡の 50 Hz 超変調成分を、平均包絡に対する相対値 (dB) で返す。
///
/// 整流 → 1 次 LP(2 kHz) で包絡を取り、1 次 HP(50 Hz) で低速成分を除去し、
/// 残った変調の RMS を平均包絡で正規化する。
fn modulation_above_50hz_db(x: &[f32]) -> f32 {
    let a_lp = 1.0 - (-2.0 * std::f32::consts::PI * 2000.0 / SR).exp();
    let a_hp = (-2.0 * std::f32::consts::PI * 50.0 / SR).exp();
    let (mut lp, mut hp_z, mut hp_y) = (0.0_f32, 0.0_f32, 0.0_f32);
    let (mut acc, mut mean) = (0.0_f64, 0.0_f64);
    // 先頭 0.5 s は過渡なので除外。
    let skip = (SR * 0.5) as usize;
    for (i, v) in x.iter().enumerate() {
        lp += a_lp * (v.abs() - lp);
        hp_y = a_hp * (hp_y + lp - hp_z);
        hp_z = lp;
        if i >= skip {
            acc += (hp_y as f64) * (hp_y as f64);
            mean += lp as f64;
        }
    }
    let n = (x.len() - skip) as f64;
    20.0 * ((acc / n).sqrt() / (mean / n).max(1e-12)).log10() as f32
}

/// 定常入力を固定母音で処理した出力を返す。
fn steady(input: &[f32], jitter: f32) -> Vec<f32> {
    let mut dsp = DrawlDsp::new(SR);
    let mut out = Vec::with_capacity(input.len());
    let mut pos = 0;
    while pos + BLOCK <= input.len() {
        let mut l: Vec<f32> = input[pos..pos + BLOCK].to_vec();
        let mut r = l.clone();
        dsp.process_block(&mut l, &mut r, 2.0, jitter, 1.0);
        out.extend_from_slice(&l);
        pos += BLOCK;
    }
    out
}

/// 定常正弦を固定 vowel で処理した出力を返す (mix=1)。
fn steady_at(input: &[f32], vowel: f32, jitter: f32) -> Vec<f32> {
    let mut dsp = DrawlDsp::new(SR);
    let mut out = Vec::with_capacity(input.len());
    let mut pos = 0;
    while pos + BLOCK <= input.len() {
        let mut l: Vec<f32> = input[pos..pos + BLOCK].to_vec();
        let mut r = l.clone();
        dsp.process_block(&mut l, &mut r, vowel, jitter, 1.0);
        out.extend_from_slice(&l);
        pos += BLOCK;
    }
    out
}

/// 正弦 (振幅 1.0)。
fn sine(n: usize, f: f32) -> Vec<f32> {
    (0..n)
        .map(|i| (2.0 * std::f32::consts::PI * f * i as f32 / SR).sin())
        .collect()
}

/// 実装 (離散フィルタ + 電力正規化) の |H(f)| を実測する。
///
/// 正弦を通し、末尾 0.1 s の RMS / 入力 RMS を取る。入力振幅 1.0 の正弦の RMS は
/// `1/sqrt(2)` なので、これで割って線形ゲインにする。
fn gain_at(vowel: f32, f: f32) -> f32 {
    let n = (SR * 0.25) as usize;
    let out = steady_at(&sine(n, f), vowel, 0.0);
    let tail = (SR * 0.1) as usize;
    let seg = &out[out.len() - tail..];
    let p: f64 = seg.iter().map(|v| (*v as f64) * (*v as f64)).sum::<f64>() / tail as f64;
    (p.sqrt() as f32) * std::f32::consts::SQRT_2
}

/// `lo..hi` を対数グリッドで走査して最大ゲインとその周波数を返す。
fn peak_scan(vowel: f32, lo: f32, hi: f32, steps: usize) -> (f32, f32) {
    let mut best = (0.0_f32, lo);
    for i in 0..=steps {
        let f = lo * (hi / lo).powf(i as f32 / steps as f32);
        let g = gain_at(vowel, f);
        if g > best.0 {
            best = (g, f);
        }
    }
    best
}

/// S&H 境界での 2 階差分。
///
/// 平滑な正弦を入力にすると 2 階差分は本来ごく小さい。ジッタによる係数更新
/// (`hold_len = SR / JITTER_RATE_HZ` サンプルごと) で段差が出ると、
/// 境界近傍だけ 2 階差分が跳ねる。境界近傍の最大値と、境界から離れた区間の
/// 最大値の比を返す (境界比 = 1.0 なら段差が信号本来の曲率に埋もれている)。
///
/// `MEMORY: DSP verification measurement` の流儀。RMS/SNR の平均では見えない。
fn sh_boundary_d2(vowel: f32, jitter: f32) -> (f32, f32, f32) {
    let n = (SR * 4.0) as usize;
    let out = steady_at(&sine(n, 220.0), vowel, jitter);
    let hold = (SR / fandango_drawl::dsp::JITTER_RATE_HZ).round() as usize;
    // 先頭 0.5 s は過渡なので除外。
    let skip = (SR * 0.5) as usize;
    let (mut near, mut far) = (0.0_f32, 0.0_f32);
    for i in (skip + 1)..(out.len() - 1) {
        let d2 = (out[i + 1] - 2.0 * out[i] + out[i - 1]).abs();
        // 境界からの距離。±2 サンプルを「境界近傍」とする。
        let phase = i % hold;
        let dist = phase.min(hold - phase);
        if dist <= 2 {
            near = near.max(d2);
        } else {
            far = far.max(d2);
        }
    }
    (near, far, near / far.max(1e-12))
}

fn main() {
    let n = (SR * 10.0) as usize;

    println!("== §6-2 母音スイープ時のレベル平坦性 (jitter=0, mix=1) ==");
    // 白色雑音は 100 ms 窓 RMS 自体が推定ノイズを持つ (狭帯域通過後の等価自由度が
    // 小さいため ±2 dB 程度)。独立 4 seed の電力平均を取って推定ノイズを落とす。
    // 帯域幅を狭めると通過後の等価自由度が下がり、同じ seed 数では推定ノイズが増える。
    // 案 K (B1=50/B2=70) は旧 Q 固定 (B = f/4) より B が狭いので seed を 16 に増やす。
    let seeds = [
        0x1234_5678_u32, 0x9e37_79b9, 0x8536_2f1d, 0xc2b2_ae35,
        0x27d4_eb2f, 0x1656_67b1, 0x85eb_ca6b, 0xc2b2_ae3d,
        0x9e37_79b1, 0x6c07_8965, 0xd3a2_646c, 0xff51_afd7,
        0x4f27_43f6, 0x9e37_79b9 ^ 0x5bf0_3635, 0xb55a_4f09, 0x1656_67b5,
    ];
    let mut acc: Vec<f64> = Vec::new();
    for &sd in &seeds {
        let db = sweep_rms_db(&white(n, 0.25, sd), 0.0);
        if acc.is_empty() {
            acc = vec![0.0; db.len()];
        }
        for (a, d) in acc.iter_mut().zip(&db) {
            *a += 10f64.powf(*d as f64 / 10.0) / seeds.len() as f64;
        }
    }
    let w_db: Vec<f32> = acc.iter().map(|p| 10.0 * p.log10() as f32).collect();
    report("(a) 白色雑音 (16seed平均)", &w_db);
    let w = white(n, 0.25, seeds[0]);
    let lo = low_heavy(n, 0.25);
    report("(b) 低域主体", &sweep_rms_db(&lo, 0.0));
    let bb = broadband(n, 0.25, 0x9e37_79b9);
    report("(c) 広帯域", &sweep_rms_db(&bb, 0.0));
    let bs = bass_saw(n, 0.25);
    report("(d) ベース鋸波55Hz", &sweep_rms_db(&bs, 0.0));

    println!("");
    println!("== §6-4 Q ジッタ (vowel=A 固定) ==");
    println!("   JITTER_RATE_HZ = {} Hz", fandango_drawl::dsp::JITTER_RATE_HZ);
    let steady_in: Vec<f32> = (0..n)
        .map(|i| {
            let t = i as f32 / SR;
            0.2 * (2.0 * std::f32::consts::PI * 440.0 * t).sin() + 0.5 * w[i]
        })
        .collect();
    let m0 = modulation_above_50hz_db(&steady(&steady_in, 0.0));
    let m1 = modulation_above_50hz_db(&steady(&steady_in, 1.0));
    println!(
        "   受入1 50Hz超変調: jitter=0 {:6.2} dB -> jitter=1 {:6.2} dB  (差 {:+.2} dB / 許容 +3.00)",
        m0, m1, m1 - m0
    );

    // 受入3 も推定ノイズを落とすため 4 seed の電力平均で測る。
    let mut acc3: Vec<f64> = Vec::new();
    for &sd in &seeds {
        let db = rms_windows_db(&steady(&white(n, 0.25, sd), 1.0));
        if acc3.is_empty() {
            acc3 = vec![0.0; db.len()];
        }
        for (a, d) in acc3.iter_mut().zip(&db) {
            *a += 10f64.powf(*d as f64 / 10.0) / seeds.len() as f64;
        }
    }
    let j_db: Vec<f32> = acc3.iter().map(|p| 10.0 * p.log10() as f32).collect();
    report("   受入3 jitter=1 白色", &j_db);

    println!("");
    println!("== Step 1 基準値 (plans/drawl_q_separation_v01.md §10) ==");
    println!("   VOWELS 各点の実測ゲイン (mix=1, jitter=0, 電力正規化込み)");
    println!("   {:<6} {:>9} {:>9} {:>9} {:>9}", "vowel", "F1peak", "F2peak", "F2-F1", "谷");
    for (vi, name) in fandango_drawl::dsp::VOWEL_NAMES.iter().enumerate() {
        let (f1, f2) = fandango_drawl::dsp::vowel_formants(vi as f32);
        let (p1, _) = peak_scan(vi as f32, f1 * 0.7, f1 * 1.4, 40);
        let (p2, _) = peak_scan(vi as f32, f2 * 0.75, f2 * 1.35, 40);
        // 谷: F1 と F2 の間の最小ゲイン。F1/F2 が近い母音では意味が薄い。
        let mut valley = f32::MAX;
        if f2 > f1 * 1.2 {
            for i in 0..=30 {
                let f = (f1 * 1.05) * ((f2 * 0.95) / (f1 * 1.05)).powf(i as f32 / 30.0);
                valley = valley.min(gain_at(vi as f32, f));
            }
        }
        let db = |g: f32| 20.0 * g.max(1e-12).log10();
        println!(
            "   {:<6} {:>9.2} {:>9.2} {:>9.2} {:>9.2}",
            name,
            db(p1),
            db(p2),
            db(p2) - db(p1),
            if valley == f32::MAX { f32::NAN } else { db(valley) - db(p1) }
        );
    }

    println!("");
    println!("   S&H 境界の 2 階差分 (220 Hz 正弦, jitter=1.0)");
    println!("   {:<6} {:>12} {:>12} {:>10}", "vowel", "境界近傍max", "非境界max", "境界比");
    for (vi, name) in fandango_drawl::dsp::VOWEL_NAMES.iter().enumerate() {
        let (near, far, ratio) = sh_boundary_d2(vi as f32, 1.0);
        println!("   {:<6} {:>12.3e} {:>12.3e} {:>10.3}", name, near, far, ratio);
    }
}
