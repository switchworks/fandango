//! 掃引 wav に乗る「LFO めいた音」の正体を特定する
//! **段配置の検証はこれが起点。**
//! **プラグイン本体には触らない。**
//!
//! 経緯: これまでの検証は固定 `rate` の定常スペクトルばかり測っていた。
//! しかしオーナーが聴いたのは `rate_continuity.rs` が書き出す**掃引 wav**であり、
//! `rate` が時間変化する非定常な信号。定常解析では捉えられない。
//!
//! 仮説: 掃引中は `delta = frac(rate)` が連続変化するので
//!   `f_p(t) = fs * delta(t) / rate(t)`
//! が時間とともに動く。`rate` が整数を通過するたびに `f_p` は 0 Hz へ落ちて再び上がる。
//! 1 -> 8 の掃引なら 7 回。これがうねりとして聴こえているのではないか。
//!
//! `stepped` は `rate.round()` なので `delta = 0` が常に成立し `f_p` が立たない。
//! **両者の差がそのまま答えになる。**
//!
//! 実行: cargo run -q -p fandango-crush --example rate_sweep_lfo --release [-- <wav_dir>]

const SR: f64 = 48_000.0;
/// STFT のフレーム長。低域 20 Hz を分離するため長めに取る。
const FRAME: usize = 8_192;
/// ホップ。時間分解能 = HOP/SR 秒。
const HOP: usize = 2_048;
/// 掃引の長さ [秒]。`rate_continuity.rs` の `sweep` と同じ。
const SECS: f64 = 6.0;

fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
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
            let (mut cr, mut ci) = (1.0_f64, 0.0_f64);
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
                let nr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = nr;
            }
            i += len;
        }
        len <<= 1;
    }
}

fn db(x: f64) -> f64 {
    20.0 * x.max(1e-12).log10()
}

/// 1 フレームのスペクトル。Blackman-Harris 窓。振幅を返す。
fn frame_spectrum(y: &[f64], off: usize) -> Vec<f64> {
    let mut re = vec![0.0; FRAME];
    let mut im = vec![0.0; FRAME];
    let mut cg = 0.0;
    for i in 0..FRAME {
        let u = 2.0 * std::f64::consts::PI * i as f64 / FRAME as f64;
        let w = 0.35875 - 0.48829 * u.cos() + 0.14128 * (2.0 * u).cos()
            - 0.01168 * (3.0 * u).cos();
        cg += w;
        re[i] = y.get(off + i).copied().unwrap_or(0.0) * w;
    }
    cg /= FRAME as f64;
    fft(&mut re, &mut im);
    (0..FRAME / 2)
        .map(|i| (re[i] * re[i] + im[i] * im[i]).sqrt() / FRAME as f64 * 2.0 / cg)
        .collect()
}

/// 指定帯域のピーク (周波数, 振幅)。入力の既知成分の近傍は除く。
fn band_peak(spec: &[f64], f_lo: f64, f_hi: f64) -> (f64, f64) {
    let bin_hz = SR / FRAME as f64;
    // 掃引 wav の入力は 220 / 1330 / 3700 Hz の合成 (`rate_continuity.rs` の `test_input`)。
    let known = [220.0_f64, 1330.0, 3700.0];
    let lo = (f_lo / bin_hz).ceil() as usize;
    let hi = ((f_hi / bin_hz).floor() as usize).min(spec.len() - 1);
    let (mut best, mut bf) = (0.0_f64, 0.0_f64);
    for i in lo..=hi {
        let f = i as f64 * bin_hz;
        if known.iter().any(|k| (f - k).abs() < 30.0) {
            continue;
        }
        if spec[i] > best {
            best = spec[i];
            bf = f;
        }
    }
    (bf, best)
}


/// RMS 包絡。窓 `win` サンプル / ホップ `hop` サンプル。
/// 窓は入力最低音 220 Hz の周期 (218 サンプル) より十分長く取る。
/// 短い窓 (5 ms = 240 サンプル) だと 220 Hz の周期内変動を拾い、
/// LFO 域の変動が埋もれる (先の測定が失敗した理由)。
fn envelope(y: &[f64], win: usize, hop: usize) -> Vec<f64> {
    let mut env = Vec::new();
    let mut i = 0usize;
    while i + win <= y.len() {
        let s: f64 = y[i..i + win].iter().map(|v| v * v).sum();
        env.push((s / win as f64).sqrt());
        i += hop;
    }
    env
}

/// 包絡の LFO 域 (f_lo..f_hi Hz) のスペクトル。
/// 掃引による緩やかなトレンドは 1 次差分で落とす。
/// 返り値は (ピーク周波数 [Hz], ピーク振幅 [包絡平均に対する比])。
fn env_lfo(env: &[f64], env_sr: f64, f_lo: f64, f_hi: f64) -> (f64, f64) {
    let mean = env.iter().sum::<f64>() / env.len() as f64;
    let m = env.len().next_power_of_two() / 2;
    if m < 16 {
        return (0.0, 0.0);
    }
    let mut re = vec![0.0; m];
    let mut im = vec![0.0; m];
    let mut cg = 0.0;
    for i in 0..m {
        let u = 2.0 * std::f64::consts::PI * i as f64 / m as f64;
        let w = 0.35875 - 0.48829 * u.cos() + 0.14128 * (2.0 * u).cos()
            - 0.01168 * (3.0 * u).cos();
        cg += w;
        re[i] = (env[i] - mean) * w;
    }
    cg /= m as f64;
    fft(&mut re, &mut im);
    let bin = env_sr / m as f64;
    let lo = ((f_lo / bin).ceil() as usize).max(1);
    let hi = ((f_hi / bin).floor() as usize).min(m / 2 - 1);
    let (mut best, mut bf) = (0.0_f64, 0.0_f64);
    for i in lo..=hi {
        let v = (re[i] * re[i] + im[i] * im[i]).sqrt() / m as f64 * 2.0 / cg;
        if v > best {
            best = v;
            bf = i as f64 * bin;
        }
    }
    (bf, best / mean.max(1e-30))
}


/// 指定帯域の上位 `k` ピーク (周波数, 振幅)。局所最大のみ拾い、近接ピークは間引く。
fn top_peaks(spec: &[f64], f_lo: f64, f_hi: f64, k: usize) -> Vec<(f64, f64)> {
    let bin_hz = SR / FRAME as f64;
    let lo = ((f_lo / bin_hz).ceil() as usize).max(1);
    let hi = ((f_hi / bin_hz).floor() as usize).min(spec.len() - 2);
    let mut cand: Vec<(f64, f64)> = Vec::new();
    for i in lo..=hi {
        if spec[i] >= spec[i - 1] && spec[i] >= spec[i + 1] {
            cand.push((i as f64 * bin_hz, spec[i]));
        }
    }
    cand.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut out: Vec<(f64, f64)> = Vec::new();
    for c in cand {
        if out.iter().any(|o| (o.0 - c.0).abs() < 150.0) {
            continue;
        }
        out.push(c);
        if out.len() >= k {
            break;
        }
    }
    out
}

/// Nyquist で折り返す。
fn fold(f: f64) -> f64 {
    let ny = SR / 2.0;
    let mut f = f.abs() % SR;
    if f > ny {
        f = SR - f;
    }
    f
}

fn read_wav(path: &str) -> Option<Vec<f64>> {
    let mut r = hound::WavReader::open(path).ok()?;
    Some(r.samples::<f32>().map(|s| s.unwrap_or(0.0) as f64).collect())
}

fn main() {
    let dir = std::env::args().nth(1).unwrap_or_else(|| "D:/output/crush_rate".to_string());

    println!("掃引 wav に乗る「LFO めいた音」の特定");
    println!();
    println!("  入力は 220 + 1330 + 3700(減衰) Hz の合成 (test_input)。");
    println!("  掃引は 6 秒。STFT フレーム {FRAME} / ホップ {HOP}。");
    println!("  予測 f_p(t) = fs * frac(rate(t)) / rate(t)。");
    println!("  stepped は rate.round() なので frac = 0、f_p は立たないはず。");
    println!();

    for (name, start, end) in [("rate_1to8", 1.0_f64, 8.0_f64), ("rate_1to32", 1.0, 32.0)] {
        let cont = match read_wav(&format!("{dir}/{name}_continuous.wav")) {
            Some(v) => v,
            None => {
                println!("  !! {dir}/{name}_continuous.wav が無い。");
                println!("     先に cargo run -q -p fandango-crush --example rate_continuity --release");
                continue;
            }
        };
        let step = read_wav(&format!("{dir}/{name}_stepped.wav")).expect("stepped wav");

        println!("[{name}]");
        println!();
        println!("  時刻[s]  rate    frac   f_p予測[Hz]  continuous 低域ピーク    stepped 低域ピーク");
        let n = cont.len();
        let mut off = 0usize;
        while off + FRAME <= n {
            let t_c = (off + FRAME / 2) as f64 / SR;
            let u = t_c / SECS;
            let r = start + (end - start) * u;
            let frac = r - r.floor();
            let f_p = SR * frac / r;

            let sc = frame_spectrum(&cont, off);
            let ss = frame_spectrum(&step, off);
            let (fc, ac) = band_peak(&sc, 20.0, 600.0);
            let (fs_, as_) = band_peak(&ss, 20.0, 600.0);

            println!(
                "  {t_c:>6.2} {r:>7.3} {frac:>7.3} {f_p:>11.1} {:>10.1} Hz {:>7.1} dB {:>8.1} Hz {:>7.1} dB",
                fc,
                db(ac),
                fs_,
                db(as_)
            );
            off += HOP * 2;
        }
        println!();
    }

    println!("判定:");
    println!("  continuous の低域ピークが f_p 予測を追い、stepped では出ない");
    println!("  → LFO 音の正体は f_p(t) の掃引で確定。");
    println!("  両者に差が無い → f_p は正体ではない。別を探す。");

    println!();
    println!("=== 振幅包絡の解析 (LFO 音はここに出るはず) ===");
    println!();
    println!("  包絡は窓 50 ms / ホップ 2 ms。入力最低音 220 Hz の周期 4.5 ms より十分長い。");
    println!("  包絡の LFO 域 (0.5-40 Hz) のピークを見る。値は包絡平均に対する比 [%]。");
    println!();

    for (name, start, end) in [("rate_1to8", 1.0_f64, 8.0_f64), ("rate_1to32", 1.0, 32.0)] {
        let cont = match read_wav(&format!("{dir}/{name}_continuous.wav")) {
            Some(v) => v,
            None => continue,
        };
        let step = read_wav(&format!("{dir}/{name}_stepped.wav")).expect("stepped wav");

        let win = (SR * 0.050) as usize;
        let hop = (SR * 0.002) as usize;
        let env_sr = SR / hop as f64;

        println!("[{name}] 全区間");
        for (tag, y) in [("continuous", &cont), ("stepped   ", &step)] {
            let e = envelope(y, win, hop);
            let (f, a) = env_lfo(&e, env_sr, 0.5, 40.0);
            println!("  {tag}  LFO ピーク {f:>6.2} Hz  変動 {:>6.2} %", a * 100.0);
        }
        println!();

        // 区間ごと。rate のどこで強くなるかを見る。
        println!("  区間別 (0.5 秒ごと)");
        println!("  時刻[s]   rate範囲      continuous [Hz / %]      stepped [Hz / %]");
        let seg = (SR * 0.5) as usize;
        let mut off = 0usize;
        while off + seg <= cont.len() {
            let t0 = off as f64 / SR;
            let t1 = (off + seg) as f64 / SR;
            let r0 = start + (end - start) * (t0 / SECS);
            let r1 = start + (end - start) * (t1 / SECS);
            let ec = envelope(&cont[off..off + seg], win, hop);
            let es = envelope(&step[off..off + seg], win, hop);
            let (fc, ac) = env_lfo(&ec, env_sr, 0.5, 40.0);
            let (fs_, as_) = env_lfo(&es, env_sr, 0.5, 40.0);
            println!(
                "  {t0:>4.1}-{t1:<4.1} {r0:>5.2}-{r1:<5.2} {fc:>10.2} Hz {:>7.2} % {fs_:>10.2} Hz {:>7.2} %",
                ac * 100.0,
                as_ * 100.0
            );
            off += seg;
        }
        println!();
    }

    println!();
    println!("=== 高域の軌跡 (「上から下に降り注ぐ波」の正体) ===");
    println!();
    println!("  整数 rate に無く非整数にだけ立つ像は半整数倍 (m + 1/2) * fs/rate。");
    println!("  m = 0 が fs/(2*rate) で、§3-1 以来測ってきた成分。");
    println!("  rate が増えると fs/rate が減るので、これらは連続的に下降する。");
    println!("  Nyquist で折り返すと上昇に転じる。");
    println!();

    for (name, start, end) in [("rate_1to8", 1.0_f64, 8.0_f64), ("rate_1to32", 1.0, 32.0)] {
        let cont = match read_wav(&format!("{dir}/{name}_continuous.wav")) {
            Some(v) => v,
            None => continue,
        };
        let step = read_wav(&format!("{dir}/{name}_stepped.wav")).expect("stepped wav");

        println!("[{name}]");
        println!();
        println!("  半整数像の予測 (fold 済み) と、実測ピーク上位 3 本を並べる。");
        println!("  cont = continuous、step = stepped。**step に無く cont にある線が答え。**");
        println!();
        println!("  時刻  rate    予測 m=0/1/2 [Hz]           cont 実測 [Hz(dB)]                    step 実測 [Hz(dB)]");
        let n = cont.len();
        let mut off = 0usize;
        while off + FRAME <= n {
            let t_c = (off + FRAME / 2) as f64 / SR;
            let r = start + (end - start) * (t_c / SECS);
            let f_sh = SR / r;
            let p0 = fold(0.5 * f_sh);
            let p1 = fold(1.5 * f_sh);
            let p2 = fold(2.5 * f_sh);

            let sc = frame_spectrum(&cont, off);
            let ss = frame_spectrum(&step, off);
            let pc = top_peaks(&sc, 2_000.0, 22_000.0, 3);
            let ps = top_peaks(&ss, 2_000.0, 22_000.0, 3);

            let fmt = |v: &Vec<(f64, f64)>| -> String {
                v.iter()
                    .map(|(f, a)| format!("{f:>7.0}({:>5.1})", db(*a)))
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            println!(
                "  {t_c:>4.2} {r:>6.2}  {p0:>7.0} {p1:>7.0} {p2:>7.0}   {}   {}",
                fmt(&pc),
                fmt(&ps)
            );
            off += HOP * 3;
        }
        println!();

        // 半整数像の総エネルギーを直接測る。cont と step の差が答え。
        println!("  半整数像 (m=0..5) の合計エネルギー [dB]。**step に無ければ確定。**");
        println!();
        println!("  時刻  rate     cont [dB]   step [dB]    差 [dB]");
        let mut off = 0usize;
        while off + FRAME <= n {
            let t_c = (off + FRAME / 2) as f64 / SR;
            let r = start + (end - start) * (t_c / SECS);
            let f_sh = SR / r;
            let bin_hz = SR / FRAME as f64;
            let sc = frame_spectrum(&cont, off);
            let ss = frame_spectrum(&step, off);
            let sum = |spec: &Vec<f64>| -> f64 {
                let mut e = 0.0;
                for m in 0..6 {
                    let f = fold((m as f64 + 0.5) * f_sh);
                    if !(200.0..22_000.0).contains(&f) {
                        continue;
                    }
                    let b = (f / bin_hz).round() as usize;
                    for d in 0..5 {
                        let i = b + d - 2;
                        if i < spec.len() {
                            e += spec[i] * spec[i];
                        }
                    }
                }
                e.sqrt()
            };
            let (ec, es) = (sum(&sc), sum(&ss));
            println!(
                "  {t_c:>4.2} {r:>6.2} {:>11.1} {:>11.1} {:>10.1}",
                db(ec),
                db(es),
                db(ec) - db(es)
            );
            off += HOP * 3;
        }
        println!();
    }
}
