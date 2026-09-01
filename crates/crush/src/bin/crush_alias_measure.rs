//! crush の残留折返し実測。ADAA 2 次へ移行する価値があるかの go/no-go 判定用。
//!
//! `rate > 1` の S&H 折返しは仕様（商品）なので測定対象外、`rate = 1` 固定で
//! 量子化器単体の残留折返しだけを測る。
//!
//! # 結論（2026-08-24・決着済）
//!
//! **ADAA 2 次は不採用。現行 1 次で確定 (2026-08-24)。**
//!
//! 当初の判定閾値「非高調波の最大線が基音比 -70 dB」は**ビットクラッシャーには不適**だった。
//! 高調波そのものが商品なので、基音比では「意図した歪み」と「不要な折返し」を区別できない。
//! そこで `alias/harm`（エイリアス総電力 / 高調波総電力）を追加したところ、
//! **エイリアスが「大きい」点と「支配的な」点が一つも重ならない**ことが判明した。
//!
//! 本体の判定メッセージは当時の基準のまま残してあるので、
//! **「可聴リスクあり」と出ても行動は不要**。alias/harm 列と併読すること。
//!
//! 実行: cargo run -p fandango-crush --bin crush_alias_measure --release

use fandango_crush::dsp::CrushChannel;

const FS: f64 = 48_000.0;
const N: usize = 1 << 18;

/// Blackman-Harris 窓 (4 項)。
fn blackman_harris(n: usize, len: usize) -> f64 {
    let a = [0.35875, 0.48829, 0.14128, 0.01168];
    let t = 2.0 * std::f64::consts::PI * n as f64 / (len - 1) as f64;
    a[0] - a[1] * t.cos() + a[2] * (2.0 * t).cos() - a[3] * (3.0 * t).cos()
}

/// 素朴な DFT では 2^18 点が終わらないので基数 2 の FFT を自前で持つ。
fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    assert!(n.is_power_of_two());

    // ビット反転並べ替え。
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }

    let mut len = 2usize;
    while len <= n {
        let ang = -2.0 * std::f64::consts::PI / len as f64;
        let (wr, wi) = (ang.cos(), ang.sin());
        let mut i = 0usize;
        while i < n {
            let (mut cr, mut ci) = (1.0f64, 0.0f64);
            for k in 0..len / 2 {
                let (ur, ui) = (re[i + k], im[i + k]);
                let (vr, vi) = (
                    re[i + k + len / 2] * cr - im[i + k + len / 2] * ci,
                    re[i + k + len / 2] * ci + im[i + k + len / 2] * cr,
                );
                re[i + k] = ur + vr;
                im[i + k] = ui + vi;
                re[i + k + len / 2] = ur - vr;
                im[i + k + len / 2] = ui - vi;
                let ncr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = ncr;
            }
            i += len;
        }
        len <<= 1;
    }
}

/// 基音 `f0` の「真の高調波」ビン集合。`k·f0 < fs/2` を満たす k のみ。
///
/// 無記憶非線形に単一正弦を入れた場合、出力成分は全て `k·f0` にしか現れない。
/// このうち `k·f0 < fs/2` のものが正しい歪み（商品）、`k·f0 > fs/2` のものが
/// Nyquist で折り返って別位置に落ちたエイリアスである。折返し先を高調波と
/// 数えると全成分が高調波になり、測定床しか見えなくなる。
fn harmonic_bins(f0: f64, bin_hz: f64, n_bins: usize, guard: usize) -> Vec<bool> {
    let mut is_harm = vec![false; n_bins];
    // DC 近傍は窓の漏れが乗るので除外する。
    for k in 0..guard.min(n_bins) {
        is_harm[k] = true;
    }
    let mut h = 1usize;
    loop {
        let f = f0 * h as f64;
        if f >= FS / 2.0 {
            break;
        }
        let b = (f / bin_hz).round() as usize;
        if b < n_bins {
            for d in b.saturating_sub(guard)..(b + guard + 1).min(n_bins) {
                is_harm[d] = true;
            }
        }
        h += 1;
    }
    is_harm
}

struct Result {
    peak_nonharm_db: f64,
    band_nonharm_db: f64,
    /// エイリアス総電力 / 高調波総電力 [dB]。基音は含めない。
    ///
    /// ビットクラッシャーでは高調波そのものが商品なので、基音比では
    /// 「意図した歪み」と「不要な折返し」を区別できない。エイリアスが
    /// 意図した歪みに対してどれだけ持ち上がっているかが判断材料になる。
    alias_to_harm_db: f64,
}

fn measure(f0: f64, amp: f64, bits: f32) -> Result {
    let mut ch = CrushChannel::new();

    // 定常区間のみを使うため、窓の外で十分に走らせてから収録する。
    let warmup = 8192usize;
    for n in 0..warmup {
        let x = (amp * (2.0 * std::f64::consts::PI * f0 * n as f64 / FS).sin()) as f32;
        ch.process(x, bits, 1.0, 1.0);
    }

    let mut re = vec![0.0f64; N];
    let mut im = vec![0.0f64; N];
    let mut wsum = 0.0f64;
    for n in 0..N {
        let t = (warmup + n) as f64;
        let x = (amp * (2.0 * std::f64::consts::PI * f0 * t / FS).sin()) as f32;
        let y = ch.process(x, bits, 1.0, 1.0) as f64;
        let w = blackman_harris(n, N);
        wsum += w;
        re[n] = y * w;
    }

    fft(&mut re, &mut im);

    let n_bins = N / 2;
    let bin_hz = FS / N as f64;
    // Blackman-Harris の主ローブは片側 4 ビン。
    let guard = 6usize;
    let is_harm = harmonic_bins(f0, bin_hz, n_bins, guard);

    // 基音の振幅（窓のコヒーレントゲインで正規化）。
    let f0_bin = (f0 / bin_hz).round() as usize;
    let mut fund = 0.0f64;
    for b in f0_bin.saturating_sub(guard)..(f0_bin + guard + 1).min(n_bins) {
        let m = (re[b] * re[b] + im[b] * im[b]).sqrt();
        if m > fund {
            fund = m;
        }
    }
    let _ = wsum;

    let mut peak = 0.0f64;
    let mut band_pow = 0.0f64;
    let mut alias_pow = 0.0f64;
    let mut harm_pow = 0.0f64;
    let f0_lo = f0_bin.saturating_sub(guard);
    let f0_hi = (f0_bin + guard + 1).min(n_bins);
    for b in 0..n_bins {
        if is_harm[b] {
            // 基音そのものは高調波電力に数えない。
            if !(f0_lo..f0_hi).contains(&b) {
                harm_pow += re[b] * re[b] + im[b] * im[b];
            }
            continue;
        }
        alias_pow += re[b] * re[b] + im[b] * im[b];
        let p = re[b] * re[b] + im[b] * im[b];
        if p.sqrt() > peak {
            peak = p.sqrt();
        }
        let f = b as f64 * bin_hz;
        if (1_000.0..=16_000.0).contains(&f) {
            band_pow += p;
        }
    }

    if fund <= 0.0 {
        // 量子化セルより振幅が小さく出力が完全な無音。折返しも存在しない。
        return Result {
            peak_nonharm_db: f64::NEG_INFINITY,
            band_nonharm_db: f64::NEG_INFINITY,
            alias_to_harm_db: f64::NEG_INFINITY,
        };
    }

    Result {
        peak_nonharm_db: 20.0 * (peak / fund).log10(),
        band_nonharm_db: 10.0 * (band_pow / (fund * fund)).log10(),
        alias_to_harm_db: 10.0 * (alias_pow / harm_pow).log10(),
    }
}

fn main() {
    println!("crush 残留折返し実測 (fs={FS} Hz, N={N}, rate=1, mix=1)");
    println!("判定閾値: 非高調波の最大線が基音比 -70 dB 超で「可聴リスクあり」\n");
    println!(
        "{:>8} {:>6} {:>6} | {:>12} {:>14} {:>14}",
        "f0[Hz]", "amp", "bits", "peak nonh[dB]", "1-16k nonh[dB]", "alias/harm[dB]"
    );
    println!("{}", "-".repeat(76));

    let mut worst = f64::NEG_INFINITY;
    let mut worst_at = String::new();

    for &f0 in &[997.0f64, 3163.0] {
        for &amp in &[0.95f64, 0.25] {
            for &bits in &[2.0f32, 3.0, 4.0, 6.0, 8.0] {
                let r = measure(f0, amp, bits);
                println!(
                    "{f0:>8.0} {amp:>6.2} {bits:>6.1} | {:>12.1} {:>14.1} {:>14.1}",
                    r.peak_nonharm_db, r.band_nonharm_db, r.alias_to_harm_db
                );
                if r.peak_nonharm_db > worst {
                    worst = r.peak_nonharm_db;
                    worst_at = format!("f0={f0} amp={amp} bits={bits}");
                }
            }
        }
    }

    println!("\n最悪値: {worst:.1} dB ({worst_at})");
    if worst > -70.0 {
        println!("判定: 可聴リスクあり → ADAA 2 次プロトタイプへ進む");
    } else {
        println!("判定: 全点 -70 dB 以下 → 現行 ADAA 1 次で確定、本件クローズ");
    }
}
