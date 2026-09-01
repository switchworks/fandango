//! Drawl DSP — 別プロジェクトの母音フォルマント実装から F1/F2 バンドパス2段 + Q ジッタを抽出。
//!
//! スコープ: 連続 vowel モーフ + Q ジッタ + Mix。

use std::f32::consts::PI;

/// フォルマント帯域幅 `B(f) = B*_INTERCEPT_HZ + B_SLOPE * f` [Hz]。
///
/// **Q ではなく B (帯域幅) を定数として持つ。** 物理的には B が生理量
/// (熱伝導・粘性・壁振動・唇からの放射・声門インピーダンス実部による損失) で、
/// `Q = f / B` は導出値。旧実装の `Q_BASE = 4.0` は Q 固定だったため
/// `B = f/4` を意味し、F2 = 2700 Hz の I で B2 = 675 Hz と実測の 3〜6 倍に広がっていた。
///
/// **出典: Klatt, D. H. (1980), "Software for a cascade/parallel formant synthesizer",
/// J. Acoust. Soc. Am. 67(3), 971-995, Table I** の固定デフォルト `B1 = 50, B2 = 70`。
/// Klatt 自身が §III-D で母音非依存の固定運用を明示的に許容している
/// ("Bandwidth variation is small enough that all formant bandwidths might be held
/// constant in some applications")。母音ごとのテーブル (同 Table II) は採らない。
/// 定数が 20 個に増える上、drawl のゴール (単位音量あたりの母音知覚の強度) に対して
/// 実測で最弱だったため。詳細は `plans/drawl_q_separation_v01.md` §7。
pub const B1_INTERCEPT_HZ: f32 = 50.0;
pub const B2_INTERCEPT_HZ: f32 = 70.0;

/// `B(f)` の周波数依存項。**Klatt Table I は母音非依存の固定値なので 0。**
///
/// 0 でない値を持つのは Fant 系の閉形式 `B(f) = 80 + 120 f/5000` へ退避する場合
/// (`B1_INTERCEPT_HZ = B2_INTERCEPT_HZ = 80.0`, `B_SLOPE = 120.0/5000.0`)。
/// 退避が係数差し替えだけで済むよう、傾き項を構造として残してある。
/// **中間値を作らないこと** (出典が消える)。上の 2 案のどちらかのみ。
pub const B_SLOPE: f32 = 0.0;

/// Q ジッタのサンプル&ホールド レート (Hz)。
///
/// ブロック長からレートを切り離すための固定値。値は
/// plans/drawl_formant_v01.md §6-4 の実測で決定。
pub const JITTER_RATE_HZ: f32 = 20.0;

/// `jitter = 1` での帯域幅 B の可動幅 (オクターブ)。
///
/// `b = B(f) * 2^(r * jitter * JITTER_OCT)`、`r ∈ [-1, 1]`。
/// 対数軸で対称に定義するので `clamp` による到達不能域が生じない
/// (旧実装は `Q_BASE + r*jitter*2.0` を `clamp(2, 10)` していたため
/// 上半分が到達不能だった)。
pub const JITTER_OCT: f32 = 1.0;

/// F1/F2 (Hz)。スライダー経路は F2 単調増加順 U/O/A/E/I。
/// 五十音順 (A-E-I-O-U) だと I(F2=2700) → O(F2=900) で F1/F2 平面上を
/// 大きく飛び、スイープが不連続になるため。
pub const VOWELS: [(f32, f32); 5] = [
    (320.0, 800.0),   // U
    (500.0, 900.0),   // O
    (800.0, 1200.0),  // A
    (550.0, 1800.0),  // E
    (280.0, 2700.0),  // I
];

/// `VOWELS` と同順の表示名。
pub const VOWEL_NAMES: [&str; 5] = ["U", "O", "A", "E", "I"];

/// `VOWELS` 上の A の位置（デフォルト値・テスト用）。
pub const VOWEL_A: f32 = 2.0;

/// `vowel_pos` ∈ [0, 4] 連続 → 補間済み `(F1, F2)` (Hz)。
///
/// 整数部で `VOWELS` の 2 頂点を選び、小数部で線形補間する。
/// DSP と GUI の双方がこれを呼ぶので、図の山の位置は実装と一致する。
pub fn vowel_formants(vowel_pos: f32) -> (f32, f32) {
    let vp = vowel_pos.clamp(0.0, 4.0);
    let idx = vp.floor() as usize;
    let frac = vp - idx as f32;
    let (sf1, sf2) = VOWELS[idx.min(4)];
    let (df1, df2) = VOWELS[(idx + 1).min(4)];
    (sf1 + frac * (df1 - sf1), sf2 + frac * (df2 - sf2))
}

/// `(F1, F2)` → `(B1, B2)` [Hz]。
pub fn bandwidths(f1_hz: f32, f2_hz: f32) -> (f32, f32) {
    (
        B1_INTERCEPT_HZ + B_SLOPE * f1_hz,
        B2_INTERCEPT_HZ + B_SLOPE * f2_hz,
    )
}

/// `(F1, F2)` と帯域幅倍率 `b_mul` → `(Q1, Q2)`。
///
/// `b_mul` はジッタによる帯域幅の倍率 (1.0 でジッタなし)。B を広げると Q は下がる。
pub fn q_pair(f1_hz: f32, f2_hz: f32, b_mul: f32) -> (f32, f32) {
    let (b1, b2) = bandwidths(f1_hz, f2_hz);
    (f1_hz / (b1 * b_mul), f2_hz / (b2 * b_mul))
}

/// `jitter` に対する帯域幅倍率の下端・上端。`b = B(f) * 2^(r * jitter * JITTER_OCT)`、
/// `r ∈ [-1, 1]` の両端。**下端 (B が狭い) が Q の上端**に対応する。
pub fn b_mul_range(jitter: f32) -> (f32, f32) {
    let d = (2.0_f32).powf(jitter * JITTER_OCT);
    (1.0 / d, d)
}

/// `chain_power` と同じアナログ原型 `H(s)` の `s = jw` での複素値。
///
/// `H(s) = s/(s+wh) * w1^2/(s^2+(w1/Q1)s+w1^2) * w2^2/(s^2+(w2/Q2)s+w2^2)`
///
/// 離散化前の原型を返すのは、never-boost 正規化 (`never_boost_scale`) がこの原型の
/// 振幅から導かれているため。図と正規化の根拠を同じ式に揃える。
pub fn chain_response(f_hz: f32, f1_hz: f32, f2_hz: f32, q1: f32, q2: f32, hp_hz: f32)
    -> (f32, f32) {
    let tau = 2.0 * PI;
    let (w, w1, w2, wh) = (tau * f_hz, tau * f1_hz, tau * f2_hz, tau * hp_hz);

    // (a + jb) / (c + jd)
    let div = |a: f32, b: f32, c: f32, d: f32| -> (f32, f32) {
        let den = c * c + d * d;
        ((a * c + b * d) / den, (b * c - a * d) / den)
    };
    let mul = |a: f32, b: f32, c: f32, d: f32| -> (f32, f32) {
        (a * c - b * d, a * d + b * c)
    };

    // jw / (jw + wh)
    let (mut re, mut im) = div(0.0, w, wh, w);
    for (wn, q) in [(w1, q1), (w2, q2)] {
        // wn^2 / (wn^2 - w^2 + j w wn / Q)
        let (nr, ni) = div(wn * wn, 0.0, wn * wn - w * w, w * wn / q);
        let (r, i) = mul(re, im, nr, ni);
        re = r;
        im = i;
    }
    (re, im)
}

/// S&H ホールド長 (サンプル)。ブロック長ではなく `sr` のみから決まる。
fn hold_len_for(sr: f32) -> usize {
    ((sr / JITTER_RATE_HZ).round() as usize).max(1)
}

/// 前段ハイパスのカットオフ分周比。`hp_fc = f1_hz / HP_DIVISOR`。
///
/// 共振ローパスは DC ゲイン 1 なので F1 より下が素通りし、低域主体素材で
/// フォルマント効果が薄まる (= 素材依存の混入)。固定 Hz の定数を置くと
/// 手チューニング定数の再導入になるため、F1 に連動する写像として定義する。
/// 値は plans/drawl_formant_v01.md §6-3 の実測で決定。
pub const HP_DIVISOR: f32 = 3.0;

/// `power_integral_*` が対応済みの最大次数。
///
/// **この値を上げる前に必ず読むこと。**
///
/// 次数を上げると Hurwitz 行列の係数スケール差が `w^N` で開く
/// (F2 = 2700 Hz なら 5 次で約 1.4e21、7 次で約 4e29)。f64 の相対精度は約 1e-16 なので、
/// 7 次では行列式のガウス消去が桁を落とし、**電力正規化が静かに誤った値を返す**
/// (NaN にはならないので `debug_assert` も素通りする)。
///
/// **対策: 角周波数を F1 で無次元化してから積分すること。**
/// アルゴリズム自体は次数に依存しない (`det` は一般形) が、無次元化なしで次数だけ
/// 上げてはならない。→ `plans/parked-decisions.md`「drawl — F3 の追加 (`I5` → `I7` 拡張)」
///
/// なお `power_integral_5` 本体の一般化には `a: [f64; N+1]` / `prod: [f64; 2N-1]` が要り、
/// `generic_const_exprs` が未安定 (rustc 1.93 で確認済み) のため const パラメータを
/// 3 個足すか固定バッファを置くことになる。**「機械的な拡張」ではない。**
const MAX_SUPPORTED_ORDER: usize = 5;

/// NxN 行列式 (部分ピボット付きガウス消去)。
///
/// **`N > MAX_SUPPORTED_ORDER` でコンパイルエラーになる。** これは意図的な関門であり、
/// 高次へ拡張する実装は必ずここを通るため、無次元化の要件を取りこぼさない。
/// 関門を外すときは `MAX_SUPPORTED_ORDER` の doc を読むこと。
///
/// **注意: post-monomorphization エラーなので `cargo check` では出ない。**
/// `cargo test` / `cargo build` / `cargo nih-plug bundle` で発火する
/// (いずれも出荷前に必ず通る経路なので取りこぼしはない)。
/// また `det::<N>` が実際に単相化される経路から呼ばれて初めて発火する
/// (dead code のままでは出ない)。
fn det<const N: usize>(m: &[[f64; N]; N]) -> f64 {
    const {
        assert!(
            N <= MAX_SUPPORTED_ORDER,
            "次数が MAX_SUPPORTED_ORDER を超えた。MAX_SUPPORTED_ORDER の doc を読むこと。
             角周波数を F1 で無次元化せずに次数を上げると電力正規化が静かに誤る。"
        )
    };
    let mut a = *m;
    let mut d = 1.0_f64;
    for i in 0..N {
        let p = (i..N).max_by(|x, y| a[*x][i].abs().total_cmp(&a[*y][i].abs())).unwrap();
        if a[p][i].abs() < 1e-300 {
            return 0.0;
        }
        if p != i {
            a.swap(i, p);
            d = -d;
        }
        d *= a[i][i];
        for r in i + 1..N {
            let f = a[r][i] / a[i][i];
            for c in i..N {
                a[r][c] -= f * a[i][c];
            }
        }
    }
    d
}

/// `I5 = (1/2pi) ∫_{-inf}^{inf} |B(jw)/A(jw)|^2 dw` の閉形式。
///
/// `A(s) = a[0] s^5 + ... + a[5]` (Hurwitz)、`B(s) = b[0] s^4 + ... + b[4]`。
/// Hurwitz 行列 `D_ij = a[2i-j+1]` の第 1 列を `B(s)B(-s)` の偶次係数
/// `c_i = [s^(2(4-i))] B(s)B(-s)` に差し替えた行列を `N` として
///
/// ```text
/// I5 = (-1)^(5+1) det(N) / (2 a[0] det(D))   ((-1)^6 = +1)
/// ```
///
/// ルックアップテーブルではなく係数から代数的に定まる閉形式。数値積分との
/// 一致は `power_norm_closed_form_matches_numeric` で検証する。
pub fn power_integral_5(a: &[f64; 6], b: &[f64; 5]) -> f64 {
    // B(s)B(-s) の偶次係数。
    let mut prod = [0.0_f64; 9];
    for (i, bi) in b.iter().enumerate() {
        for (j, bj) in b.iter().enumerate() {
            let (pi, pj) = (4 - i, 4 - j);
            let sign = if pj % 2 == 0 { 1.0 } else { -1.0 };
            prod[pi + pj] += bi * bj * sign;
        }
    }

    let mut d = [[0.0_f64; 5]; 5];
    for i in 0..5 {
        for j in 0..5 {
            let k = 2 * i + 1;
            if k >= j && k - j <= 5 {
                d[i][j] = a[k - j];
            }
        }
    }
    let mut n = d;
    for i in 0..5 {
        n[i][0] = prod[2 * (4 - i)];
    }

    det(&n) / (2.0 * a[0] * det(&d))
}

/// 1 次ハイパス + 直列 2 段共振ローパスの総合特性に対する電力積分。
///
/// `H(s) = s/(s+wh) * w1^2/(s^2+(w1/Q1)s+w1^2) * w2^2/(s^2+(w2/Q2)s+w2^2)`
pub fn chain_power(f1_hz: f32, f2_hz: f32, q1: f32, q2: f32, hp_hz: f32) -> f64 {
    let tau = 2.0 * std::f64::consts::PI;
    let (w1, w2, wh) = (tau * f1_hz as f64, tau * f2_hz as f64, tau * hp_hz as f64);
    let (c1, c2) = (w1 / q1 as f64, w2 / q2 as f64);
    let (w1s, w2s) = (w1 * w1, w2 * w2);

    // A4 = (s^2 + c1 s + w1^2)(s^2 + c2 s + w2^2)
    let a4 = [
        1.0,
        c1 + c2,
        w1s + w2s + c1 * c2,
        c1 * w2s + c2 * w1s,
        w1s * w2s,
    ];
    // A5 = A4 * (s + wh)
    let mut a = [0.0_f64; 6];
    for (i, ai) in a4.iter().enumerate() {
        a[i] += ai;
        a[i + 1] += ai * wh;
    }
    // B5 = w1^2 w2^2 * s  (deg 4 表現)
    let b = [0.0, 0.0, 0.0, w1s * w2s, 0.0];

    power_integral_5(&a, &b)
}

/// never-boost 正規化スカラ。`|s·H(jw)| ≤ 1` を満たすように総合特性の
/// 振幅ピークで割る。
///
/// `s = 1 / max(|H(j·wp1)|, |H(j·wp2)|)`、
/// `wp_i = w_i * sqrt(1 - 1/(2 Q_i^2))` (2 次共振 LP のピーク角周波数、閉形式)。
///
/// 旧実装の電力正規化 (白色雑音の全帯域電力を 1 に揃える**等式**) は
/// 素材スペクトルによってはブーストし得た。本式は**不等式** `|H| ≤ 1` を保証するので
/// 任意の入力スペクトルに対し出力電力 ≤ 入力電力。ただし**時間領域の真ピークは
/// 縛らない** (インパルス応答の L1 ノルム分の余地が残る)。
/// 詳細は `plans/drawl_never_boost_v01.md` §3。
///
/// セクション別のピーク (`Q/sqrt(1-1/(4Q^2))`) で割る形は採らない。
/// 出力天井が真の最大より約 `Q2` 分低くなり (案 K の /i/ で Q2 = 38.6 = 約 -31 dB)、
/// 過剰減衰で製品にならない。→ 同書 §4-1。
pub fn never_boost_scale(f1_hz: f32, f2_hz: f32, q1: f32, q2: f32, hp_hz: f32) -> f32 {
    // 2 次共振 LP のピーク周波数。Q ≤ 1/sqrt(2) では実数解がなく、
    // 応答は単調減少でピークは DC 側にあるので 0 Hz を候補にする。
    let peak_hz = |f0: f32, q: f32| -> f32 {
        let k = 1.0 - 1.0 / (2.0 * q * q);
        if k > 0.0 { f0 * k.sqrt() } else { 0.0 }
    };

    let mut m = 0.0_f32;
    for f in [peak_hz(f1_hz, q1), peak_hz(f2_hz, q2)] {
        let (re, im) = chain_response(f, f1_hz, f2_hz, q1, q2, hp_hz);
        m = m.max((re * re + im * im).sqrt());
    }
    debug_assert!(m.is_finite() && m > 0.0, "候補点での振幅が不正: {m}");
    1.0 / m
}

/// 1 次ハイパス (TPT、ステレオ 1 チャンネル内蔵)。`H(s) = s / (s + wh)`。
#[derive(Clone)]
pub struct OnePoleHpf {
    z: [f32; 2],
    g: f32,
    g_tgt: f32,
}

impl OnePoleHpf {
    pub fn new() -> Self {
        Self { z: [0.0; 2], g: 0.0, g_tgt: 0.0 }
    }

    pub fn set_params(&mut self, fc_hz: f32, sr: f32) {
        let t = (PI * fc_hz / sr).tan().clamp(1e-5, 0.999);
        self.g_tgt = t / (1.0 + t);
    }

    pub fn snap_params(&mut self) {
        self.g = self.g_tgt;
    }

    #[inline]
    pub fn process(&mut self, ch: usize, x: f32, smooth: f32) -> f32 {
        self.g += smooth * (self.g_tgt - self.g);
        let v = (x - self.z[ch]) * self.g;
        let lp = v + self.z[ch];
        self.z[ch] = lp + v;
        x - lp
    }
}

/// SVF ベースの 2 次共振ローパス (ステレオ 1 チャンネル内蔵)。
///
/// `H(s) = w0^2 / (s^2 + (w0/Q) s + w0^2)`。`|H(0)| = 1`、`fc` でピーク ≈ `Q`、
/// 高域 -12 dB/oct。band タップではなく lowpass タップを返すのは、直列接続で
/// F1/F2 の山を両方残しつつ F1 の高域ロールオフを F2 に掛けるため
/// (声道の全極モデル = 共振器直列)。band 直列だと積が中間に単一の山を作り、
/// フォルマント 2 本という構造が出力に現れない。
#[derive(Clone)]
pub struct FormantResonator {
    ic1: [f32; 2],
    ic2: [f32; 2],
    a1: f32,
    a2: f32,
    a3: f32,
    a1_tgt: f32,
    a2_tgt: f32,
    a3_tgt: f32,
}

impl FormantResonator {
    pub fn new() -> Self {
        Self {
            ic1: [0.0; 2],
            ic2: [0.0; 2],
            a1: 0.0, a2: 0.0, a3: 0.0,
            a1_tgt: 0.0, a2_tgt: 0.0, a3_tgt: 0.0,
        }
    }

    pub fn set_params(&mut self, fc_hz: f32, q: f32, sr: f32) {
        let g = (PI * fc_hz / sr).tan().clamp(1e-4, 0.999);
        let k = 1.0 / q;
        let a1 = 1.0 / (1.0 + g * (g + k));
        self.a1_tgt = a1;
        self.a2_tgt = g * a1;
        self.a3_tgt = g * self.a2_tgt;
    }

    pub fn snap_params(&mut self) {
        self.a1 = self.a1_tgt;
        self.a2 = self.a2_tgt;
        self.a3 = self.a3_tgt;
    }

    #[inline]
    pub fn process(&mut self, ch: usize, x: f32, smooth: f32) -> f32 {
        self.a1 += smooth * (self.a1_tgt - self.a1);
        self.a2 += smooth * (self.a2_tgt - self.a2);
        self.a3 += smooth * (self.a3_tgt - self.a3);
        let v3 = x - self.ic2[ch];
        let v1 = self.a1 * self.ic1[ch] + self.a2 * v3;
        let v2 = self.ic2[ch] + self.a2 * self.ic1[ch] + self.a3 * v3;
        self.ic1[ch] = 2.0 * v1 - self.ic1[ch];
        self.ic2[ch] = 2.0 * v2 - self.ic2[ch];
        v2
    }
}

pub struct DrawlDsp {
    sr: f32,
    smooth: f32,
    hp: OnePoleHpf,
    f1: FormantResonator,
    f2: FormantResonator,
    /// S&H で保持している帯域幅倍率。`1.0` でジッタなし。
    /// Q ではなく倍率を保持するのは、`vowel` が動くと同じ倍率でも Q が変わるため。
    b_mul: f32,
    noise_rng: u32,
    /// S&H ホールド長 (サンプル)。`sr / JITTER_RATE_HZ`。
    hold_len: usize,
    /// ホールド内の絶対位置。ホスト側ブロック境界とは独立。
    hold_pos: usize,
    /// never-boost 正規化スカラ (サンプル単位で `smooth` 補間。ブロック境界での段差防止)。
    scale: f32,
    scale_tgt: f32,
}

impl DrawlDsp {
    pub fn new(sr: f32) -> Self {
        let mut d = Self {
            sr,
            smooth: 1.0 - (-1.0_f32 / (0.005 * sr)).exp(),
            hp: OnePoleHpf::new(),
            f1: FormantResonator::new(),
            f2: FormantResonator::new(),
            b_mul: 1.0,
            noise_rng: 0x1234_5678,
            hold_len: hold_len_for(sr),
            hold_pos: 0,
            scale: 1.0,
            scale_tgt: 1.0,
        };
        let (f1, f2) = VOWELS[VOWEL_A as usize];
        let (q1, q2) = q_pair(f1, f2, 1.0);
        d.f1.set_params(f1, q1, sr);
        d.f1.snap_params();
        d.f2.set_params(f2, q2, sr);
        d.f2.snap_params();
        d.hp.set_params(f1 / HP_DIVISOR, sr);
        d.hp.snap_params();
        d.scale_tgt = never_boost_scale(f1, f2, q1, q2, f1 / HP_DIVISOR);
        d.scale = d.scale_tgt;
        d
    }

    pub fn set_sr(&mut self, sr: f32) {
        self.sr = sr;
        self.smooth = 1.0 - (-1.0_f32 / (0.005 * sr)).exp();
        self.hold_len = hold_len_for(sr);
        self.hold_pos = 0;
    }

    pub fn reset(&mut self) {
        self.hp = OnePoleHpf::new();
        self.f1 = FormantResonator::new();
        self.f2 = FormantResonator::new();
        self.noise_rng = 0x1234_5678;
        self.hold_pos = 0;
        self.b_mul = 1.0;
        let (f1, f2) = VOWELS[VOWEL_A as usize];
        let (q1, q2) = q_pair(f1, f2, 1.0);
        self.hp.set_params(f1 / HP_DIVISOR, self.sr);
        self.hp.snap_params();
        self.scale_tgt = never_boost_scale(f1, f2, q1, q2, f1 / HP_DIVISOR);
        self.scale = self.scale_tgt;
    }

    #[inline]
    fn next_noise(rng: &mut u32) -> f32 {
        *rng ^= *rng << 13;
        *rng ^= *rng >> 17;
        *rng ^= *rng << 5;
        (*rng as i32) as f32 / 2_147_483_647.0
    }

    /// `vowel_pos` ∈ [0, 4] 連続。整数部で 2 頂点、小数部で線形補間。
    /// `jitter` ∈ [0, 1]、`mix` ∈ [0, 1]。
    pub fn process_block(&mut self, out_l: &mut [f32], out_r: &mut [f32],
                         vowel_pos: f32, jitter: f32, mix: f32) {
        let n = out_l.len().min(out_r.len());
        if n == 0 { return; }

        let (f1_hz, f2_hz) = vowel_formants(vowel_pos);

        let hp_hz = f1_hz / HP_DIVISOR;
        let smooth = self.smooth;

        // ホスト側ブロック境界ではなく S&H 境界で区切る。
        // これによりジッタレートがバッファサイズから独立する。
        let mut i = 0;
        while i < n {
            if self.hold_pos == 0 {
                // ホールド境界: 新しい帯域幅倍率を引く。jitter = 0 なら 1.0 へ戻る
                // (旧実装は最後にジッタした値を保持し続けていた)。
                // 乱数は境界ごとに 1 個だけ引き、F1/F2 で共有する。
                // **消費回数を変えないこと** (決定論の基準が変わる)。
                self.b_mul = if jitter > 0.0 {
                    let r = Self::next_noise(&mut self.noise_rng);
                    (2.0_f32).powf(r * jitter * JITTER_OCT)
                } else {
                    1.0
                };
                let (q1, q2) = q_pair(f1_hz, f2_hz, self.b_mul);
                self.f1.set_params(f1_hz, q1, self.sr);
                self.f2.set_params(f2_hz, q2, self.sr);
                self.hp.set_params(hp_hz, self.sr);
                self.scale_tgt = never_boost_scale(f1_hz, f2_hz, q1, q2, hp_hz);
            }
            let take = (self.hold_len - self.hold_pos).min(n - i);
            for i in i..i + take {
                let l = out_l[i];
                let r = out_r[i];
                self.scale += smooth * (self.scale_tgt - self.scale);
                let g = self.scale;
                let wl = g * self.f2.process(
                    0,
                    self.f1.process(0, self.hp.process(0, l, smooth), smooth),
                    smooth,
                );
                let wr = g * self.f2.process(
                    1,
                    self.f1.process(1, self.hp.process(1, r, smooth), smooth),
                    smooth,
                );
                out_l[i] = (1.0 - mix) * l + mix * wl;
                out_r[i] = (1.0 - mix) * r + mix * wr;
            }
            self.hold_pos += take;
            if self.hold_pos >= self.hold_len {
                self.hold_pos = 0;
            }
            i += take;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mix_zero_bypass() {
        let mut d = DrawlDsp::new(48000.0);
        let mut l = vec![0.5_f32; 128];
        let mut r = vec![-0.3_f32; 128];
        d.process_block(&mut l, &mut r, 0.0, 0.0, 0.0);
        assert!((l[10] - 0.5).abs() < 1e-6);
        assert!((r[10] + 0.3).abs() < 1e-6);
    }

    /// 連続時間プロトタイプ `|H(j2pi f)|^2` (直列 2 段共振ローパス)。
    fn mag2(f: f64, f1: f64, f2: f64, q1: f64, q2: f64, hp: f64) -> f64 {
        let w = 2.0 * std::f64::consts::PI * f;
        let sec = |w0: f64, q: f64| {
            let w0s = w0 * w0;
            let re = w0s - w * w;
            let im = (w0 / q) * w;
            w0s * w0s / (re * re + im * im)
        };
        let wh = 2.0 * std::f64::consts::PI * hp;
        let hpf = w * w / (w * w + wh * wh);
        hpf * sec(2.0 * std::f64::consts::PI * f1, q1)
            * sec(2.0 * std::f64::consts::PI * f2, q2)
    }

    /// `∫_0^inf |H(j2pi f)|^2 df` を対数グリッド台形則で数値積分。
    fn numeric_power(f1: f64, f2: f64, q1: f64, q2: f64, hp: f64) -> f64 {
        let (lo, hi, n) = (1e-2_f64, 1e8_f64, 800_000);
        let step = (hi / lo).ln() / n as f64;
        let mut acc = 0.0;
        let mut prev_f = lo;
        let mut prev_y = mag2(lo, f1, f2, q1, q2, hp);
        for i in 1..=n {
            let f = lo * (step * i as f64).exp();
            let y = mag2(f, f1, f2, q1, q2, hp);
            acc += 0.5 * (prev_y + y) * (f - prev_f);
            prev_f = f;
            prev_y = y;
        }
        acc
    }

    #[test]
    fn power_norm_closed_form_matches_numeric() {
        let mut worst = 0.0_f64;
        for step in 0..=8 {
            let vp = step as f32 * 0.5;
            let idx = vp.floor() as usize;
            let frac = vp - idx as f32;
            let (sf1, sf2) = VOWELS[idx.min(4)];
            let (df1, df2) = VOWELS[(idx + 1).min(4)];
            let f1 = sf1 + frac * (df1 - sf1);
            let f2 = sf2 + frac * (df2 - sf2);
            // Q1 と Q2 を独立に振る。実運用値 (`q_pair`) に加え、
            // 分離の効いた比 (Q2/Q1 が 1 から離れた組) も含める。
            let (bq1, bq2) = q_pair(f1, f2, 1.0);
            let pairs = [
                (bq1, bq2),
                (bq1 * 0.5, bq2 * 2.0),
                (bq1 * 2.0, bq2 * 0.5),
                (2.0, 40.0),
                (40.0, 2.0),
            ];
            for (q1, q2) in pairs {
                // 閉形式は (1/2pi)∫_{-inf}^{inf}、数値積分は ∫_0^inf なので 1/2 倍で比較。
                let hp = f1 / HP_DIVISOR;
                let closed = chain_power(f1, f2, q1, q2, hp) / 2.0;
                let num = numeric_power(f1 as f64, f2 as f64, q1 as f64, q2 as f64, hp as f64);
                let rel = ((closed - num) / num).abs();
                worst = worst.max(rel);
                assert!(rel < 1e-3,
                    "f1={f1} f2={f2} q1={q1} q2={q2}: closed={closed} numeric={num} rel={rel}");
            }
        }
        println!("power_norm 最大相対誤差 = {:.3e}", worst);
    }

    /// never-boost の機械判定 (`plans/drawl_never_boost_v01.md` §6)。
    ///
    /// `VOWELS` 補間格子 × 帯域幅倍率の全点で `max|s·H|` を対数グリッド密走査で
    /// 数値算出し、0.5 dB を超えないことを確かめる。超えたら候補点の選定が誤り。
    #[test]
    fn never_boost_holds_over_grid() {
        let mut worst_db = f64::NEG_INFINITY;
        let mut worst_at = (0.0_f32, 0.0_f32);
        for step in 0..=16 {
            let vp = step as f32 * 0.25;
            let (f1, f2) = vowel_formants(vp);
            for &b_mul in &[0.5_f32, 1.0, 2.0] {
                let (q1, q2) = q_pair(f1, f2, b_mul);
                let hp = f1 / HP_DIVISOR;
                let s = never_boost_scale(f1, f2, q1, q2, hp) as f64;

                // 対数グリッド密走査。流儀は `power_norm_closed_form_matches_numeric` に揃える。
                let (lo, hi, n) = (1e-2_f64, 1e6_f64, 400_000);
                let dstep = (hi / lo).ln() / n as f64;
                let mut m = 0.0_f64;
                for i in 0..=n {
                    let f = lo * (dstep * i as f64).exp();
                    let mag2 = mag2(f, f1 as f64, f2 as f64, q1 as f64, q2 as f64, hp as f64);
                    m = m.max(mag2);
                }
                let db = 20.0 * (s * m.sqrt()).log10();
                if db > worst_db {
                    worst_db = db;
                    worst_at = (vp, b_mul);
                }
            }
        }
        println!("never-boost 最大超過 = {worst_db:+.4} dB (vowel={}, b_mul={})",
                 worst_at.0, worst_at.1);
        assert!(worst_db <= 0.5, "max|sH| = {worst_db:+.4} dB > 0.5 dB (vowel={}, b_mul={})",
                worst_at.0, worst_at.1);
    }

    /// ジッタ由来の音量揺れが代数上限内 (`plans/drawl_never_boost_v01.md` §6)。
    ///
    /// ピークを 1 に固定するので通過帯域幅の変化がそのまま白色雑音の出力電力に出る。
    /// 帯域幅は 2 セクション × 2 倍 = 電力 4 倍 = 振幅 2 倍 = ±3 dB が代数上限
    /// (`chain_power * scale^2` の実測でも b_mul 0.5→2 で 5.81 dB = ±2.9 dB)。
    /// 余裕 0.5 dB を足した ±3.5 dB (幅 7 dB) を判定に使う。
    ///
    /// **窓 RMS を 1 実現から測ってはいけない。** 通過帯域は数十 Hz しかないので
    /// 50 ms 窓あたりの独立標本は約 4 個しかなく、推定ばらつきだけで数 dB 出る
    /// (1 実現で測ると 8.2 dB になり、ジッタ由来かどうかが分離できない)。
    /// S&H の乱数列は `reset` 後決定的で入力に依存しないので、
    /// **同一ジッタ系列に独立な雑音実現を複数通し、窓ごとに電力を平均**して
    /// 推定分散を落とす。
    #[test]
    fn jitter_level_swing_within_algebraic_bound() {
        let sr = 48_000.0_f32;
        let hold = (sr / JITTER_RATE_HZ) as usize;
        let windows = 200;
        let realizations = 64;
        let n = hold * windows;

        // 窓ごとの平均電力。実現をまたいで積む。
        let mut acc = vec![0.0_f64; windows];
        for k in 0..realizations {
            let mut rng = 0x9E37_79B9_u32.wrapping_add(k as u32 * 0x0100_0193);
            let mut noise = move || {
                rng ^= rng << 13;
                rng ^= rng >> 17;
                rng ^= rng << 5;
                (rng as i32) as f32 / 2_147_483_647.0
            };
            let mut l: Vec<f32> = (0..n).map(|_| noise()).collect();
            let mut r = l.clone();

            let mut d = DrawlDsp::new(sr);
            d.reset();
            d.process_block(&mut l, &mut r, VOWEL_A, 1.0, 1.0);

            for (w, a) in acc.iter_mut().enumerate() {
                let seg = &l[w * hold..(w + 1) * hold];
                *a += seg.iter().map(|v| (*v as f64) * (*v as f64)).sum::<f64>() / hold as f64;
            }
        }

        // 先頭 2 窓はスカラ補間の立ち上がりが乗るので捨てる。
        let mut lo = f64::INFINITY;
        let mut hi = 0.0_f64;
        for a in &acc[2..] {
            let rms = (a / realizations as f64).sqrt();
            lo = lo.min(rms);
            hi = hi.max(rms);
        }
        let spread = 20.0 * (hi / lo).log10();
        println!("ジッタ区間 RMS の幅 = {spread:.3} dB (max/min、{realizations} 実現平均)");
        assert!(spread <= 7.0, "区間 RMS の幅 {spread:.3} dB > 7 dB (±3.5 dB)");
    }

    #[test]
    fn no_nan_over_param_sweep() {
        for &sr in &[44100.0_f32, 48000.0, 96000.0, 192000.0] {
            let mut d = DrawlDsp::new(sr);
            for vi in 0..=16 {
                for ji in 0..=4 {
                    for mi in 0..=2 {
                        let mut l = vec![0.3_f32; 256];
                        let mut r = vec![-0.3_f32; 256];
                        d.process_block(&mut l, &mut r, vi as f32 * 0.25,
                                        ji as f32 * 0.25, mi as f32 * 0.5);
                        assert!(l.iter().chain(r.iter()).all(|v| v.is_finite()),
                                "sr={sr} vowel={} jitter={}", vi as f32 * 0.25, ji as f32 * 0.25);
                    }
                }
            }
        }
    }

    #[test]
    fn deterministic_after_reset() {
        let run = || {
            let mut d = DrawlDsp::new(48000.0);
            d.reset();
            let mut out = Vec::new();
            for b in 0..40 {
                let mut l: Vec<f32> = (0..128).map(|i| ((b * 128 + i) as f32 * 0.01).sin()).collect();
                let mut r = l.clone();
                d.process_block(&mut l, &mut r, 1.7, 0.6, 1.0);
                out.extend_from_slice(&l);
            }
            out
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn block_size_invariance() {
        let n = 48_000;
        let input: Vec<f32> = (0..n)
            .map(|i| (i as f32 * 0.017).sin() * 0.4 + (i as f32 * 0.003).sin() * 0.2)
            .collect();
        let run = |bs: usize| {
            let mut d = DrawlDsp::new(48000.0);
            d.reset();
            let mut out = Vec::with_capacity(n);
            let mut pos = 0;
            while pos + bs <= n {
                let mut l: Vec<f32> = input[pos..pos + bs].to_vec();
                let mut r = l.clone();
                d.process_block(&mut l, &mut r, 1.7, 1.0, 1.0);
                out.extend_from_slice(&l);
                pos += bs;
            }
            out
        };
        let base = run(64);
        for &bs in &[128_usize, 256, 512, 1024] {
            let other = run(bs);
            let len = base.len().min(other.len());
            let worst = (0..len)
                .map(|i| (base[i] - other[i]).abs())
                .fold(0.0_f32, f32::max);
            assert!(worst < 1e-5, "block {bs}: 最大絶対誤差 {worst}");
        }
    }

    #[test]
    fn silent_input_stays_silent() {
        let mut d = DrawlDsp::new(48000.0);
        let mut l = vec![0.0_f32; 512];
        let mut r = vec![0.0_f32; 512];
        d.process_block(&mut l, &mut r, 2.5, 0.5, 1.0);
        assert!(l.iter().all(|v| v.abs() < 1e-6));
    }
}
