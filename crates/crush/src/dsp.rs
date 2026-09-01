//! crush DSP コア — ADAA (Antiderivative Anti-Aliasing) 1 次適用の
//! mid-tread ビットクラッシャー。

/// 量子化ステップ幅。`b` = 連続ビット数 (1.0..=16.0)。
#[inline]
fn delta(b: f32) -> f32 {
    2.0 / (2.0_f32.powf(b) - 1.0)
}

/// mid-tread 量子化器。`m(x) = floor(x/Δ + 1/2)`、`q(x) = Δ·m(x)`。
#[inline]
fn quantize(x: f32, b: f32) -> f32 {
    let d = delta(b);
    let m = (x / d + 0.5).floor();
    d * m
}

/// `q(x)` の不定積分。`F(x) = Δ·(m·x - Δ·m²/2)`、`m = floor(x/Δ + 1/2)`。ADAA 差分商の本体で使う。
/// f32 で差分商を取ると `bits=16` 付近 (Δ が小さく F(x) の差が桁落ちする域) で
/// 誤差が dry の 5 倍に達するため、不定積分と差分商は f64 で計算する。
#[inline]
fn f_antideriv_f64(x: f64, b: f64) -> f64 {
    let d = 2.0 / (2.0_f64.powf(b) - 1.0);
    let m = (x / d + 0.5).floor();
    d * (m * x - d * m * m * 0.5)
}

/// `quantize` の f64 版。`bits=16` では `x/Δ` が 32768 付近に達し、
/// f32 の刻み (2^-8 ≒ 3.9e-3) が floor をまたいで量子化段を 128 段飛ばすため、
/// フォールバック経路も f64 で評価する。
#[inline]
fn quantize_f64(x: f64, b: f64) -> f64 {
    let d = 2.0 / (2.0_f64.powf(b) - 1.0);
    d * (x / d + 0.5).floor()
}

/// ADAA 差分式のゼロ割回避閾値。`|x[n]-x[n-1]| <= ADAA_EPS` で q() の直接評価に切替。
const ADAA_EPS: f64 = 1e-6;

/// ADAA 1 次量子化。`y = (F(x)-F(x_prev)) / (x-x_prev)`、`|Δx|` が小さい時は
/// `q((x+x_prev)/2)` へフォールバック (ステートレス、履歴 1 サンプルのみ)。
/// 差分商は f64 で計算する (`f_antideriv_f64` のコメント参照)。
#[inline]
fn shape_adaa_quant(x: f32, x_prev: f64, b: f32) -> f32 {
    let xf = x as f64;
    let dx = xf - x_prev;
    if dx.abs() > ADAA_EPS {
        ((f_antideriv_f64(xf, b as f64) - f_antideriv_f64(x_prev, b as f64)) / dx) as f32
    } else {
        quantize_f64((xf + x_prev) * 0.5, b as f64) as f32
    }
}

// ---- ここから `crates/excite/src/dsp.rs` から逐語転記 (改変禁止) ----

/// Denormal ゲート閾値。これ未満の絶対値は 0.0 に丸める。
/// x86 CPU の subnormal microcode fallback (100倍以上遅延) を回避、
/// ホスト側「暴走プラグイン」誤判定 → kill を防ぐ最重要対策。
const DENORMAL_GATE: f32 = 1e-30;

/// Denormal ゲート適用。|x| < DENORMAL_GATE → 0.0。
#[inline]
fn denormal_gate(x: f32) -> f32 {
    if x.abs() < DENORMAL_GATE { 0.0 } else { x }
}

/// 出力段 DC ブロッカー (1-pole highpass, `y[n] = x[n]-x[n-1]+R·y[n-1]`)。
/// カットオフ ≈ `(1-R)·sr/(2π)`。Chebyshev エンジンのエンベロープ追従で
/// 出力に直流が乗るための対策として導入し、合流後は mix 値に関わらず
/// 常時 (legacy 単独時も含め) 適用する。
/// DC ブロッカーの目標カットオフ [Hz]。
///
/// 旧実装は `R = 0.999` を固定していたが、カットオフ `(1-R)·sr/(2π)` は
/// sr に比例するため 192 kHz では約 30 Hz まで上がり可聴帯域の低域を削っていた
/// (実測: 20 Hz の減衰が 48 kHz で -0.59 dB、192 kHz で -5.23 dB)。
///
/// 値の決め方: **可聴下限 20 Hz での振幅偏差を 0.1 dB 以内に収める最大の fc**。
/// 1 次ハイパスの振幅は `|H(f)| = f / √(f² + fc²)` なので、偏差 `A` [dB] を
/// 与えたときの閉形式は
///
/// ```text
/// fc = F_MIN · √(10^(A/10) − 1) = 20 · √(10^0.01 − 1) ≈ 3.0524 Hz
/// ```
///
/// fc は上を取るほど直流の除去が速く (時定数 `1/(2π·fc)` ≈ 52 ms)、
/// 下げるほど可聴帯を汚さない。上限側の制約が可聴帯の偏差だけなので、
/// 「制約を満たす最大値」で一意に決まり、旧実装への逆算 (自己参照) は不要になる。
/// 0.1 dB は振幅の弁別限 (概ね 0.5〜1 dB) の 1/5 以下に取った安全側の値。
///
/// 定数は closed form の数値そのもの。式との一致は
/// `dc_block_fc_matches_closed_form` が、20 Hz での実測偏差は
/// `dc_block_deviation_at_20hz_within_budget` が機械判定する。
const DC_BLOCK_FC_HZ: f64 = 3.052_408_379_018_384_5;

/// `DC_BLOCK_FC_HZ` を導く可聴下限 [Hz] と、そこで許す振幅偏差 [dB]。
/// `f64::sqrt` / `powf` が const fn でないため定数畳み込みには使えず、
/// 参照はテスト (`dc_block_fc_matches_closed_form`) のみになる。
#[allow(dead_code)]
const DC_BLOCK_F_MIN_HZ: f64 = 20.0;
#[allow(dead_code)]
const DC_BLOCK_MAX_DEV_DB: f64 = 0.1;

/// `set_sample_rate` を呼ばない経路 (テスト等) の既定サンプルレート [Hz]。
const DC_BLOCK_REF_SR: f32 = 48_000.0;

/// 目標カットオフからサンプルレートごとの `R` を導く。
///
/// 係数は f64 で計算する。`1 - 2π·fc/sr` は高い sr で 1 に極めて近く、
/// f32 のまま引くと仮数が落ちるため (tilt / nimbus と同じ理由)。
pub fn dc_block_r(sample_rate: f32) -> f32 {
    (1.0 - 2.0 * std::f64::consts::PI * DC_BLOCK_FC_HZ / sample_rate as f64) as f32
}

pub struct DcBlocker {
    x_prev: f32,
    y_prev: f32,
    /// `dc_block_r(sr)`。既定は `DC_BLOCK_REF_SR` 相当で、
    /// `set_sample_rate` を呼ばない経路 (テスト等) が従来と同じ挙動になる。
    r: f32,
}

impl DcBlocker {
    pub fn new() -> Self {
        Self { x_prev: 0.0, y_prev: 0.0, r: dc_block_r(DC_BLOCK_REF_SR) }
    }
    /// サンプルレートを設定する。`initialize` / sr 変更時に呼ぶ
    /// (`process` 内で再計算しないため)。
    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.r = dc_block_r(sample_rate);
    }
    pub fn reset(&mut self) {
        self.x_prev = 0.0;
        self.y_prev = 0.0;
    }
    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = x - self.x_prev + self.r * self.y_prev;
        self.x_prev = x;
        let y = if y.is_finite() { denormal_gate(y) } else { 0.0 };
        self.y_prev = y;
        y
    }
}

// ---- 転記ここまで ----

/// 1 チャンネル分の crush パイプライン。信号順: 入力ガード → S&H → ADAA 量子化 →
/// denormal_gate → DcBlocker → mix 補間 (dry は S&H 前の x) → 出力ガード。
pub struct CrushChannel {
    /// ADAA 用直前サンプル (S&H 後の値)。差分商の桁落ち回避のため f64 で保持する。
    x_prev: f64,
    /// 直前の入力サンプル `x[n-1]`。段の値を非整数時刻で線形補間して読むのに使う。
    x_in_prev: f32,
    /// 現区間の開始時点で有効な段の値。
    hold: f32,
    /// 区間 `[n-1, n]` の開始 `t = n-1` から次の段境界までの距離。
    /// `rate >= 1` が保証される限り `0 <= edge_dist` が保たれ、
    /// 1 区間に境界は高々 1 つになる (§4-2)。
    /// 初期値 1.0 は「n=0 の区間 `[-1, 0]` の境界が `t = 0` にある」ことを表す。
    edge_dist: f64,
    dc_blocker: DcBlocker,
}

impl CrushChannel {
    pub fn new() -> Self {
        Self {
            x_prev: 0.0,
            x_in_prev: 0.0,
            hold: 0.0,
            edge_dist: 1.0,
            dc_blocker: DcBlocker::new(),
        }
    }

    /// サンプルレートを設定する。現状 DC ブロッカーのみが sr に依存する。
    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.dc_blocker.set_sample_rate(sample_rate);
    }
    pub fn reset(&mut self) {
        self.x_prev = 0.0;
        self.x_in_prev = 0.0;
        self.hold = 0.0;
        self.edge_dist = 1.0;
        self.dc_blocker.reset();
    }

    /// `bits`: 1.0..=16.0、`rate`: 1.0.. (S&H ダウンサンプル係数、実数)、`mix`: 0.0..=1.0。
    ///
    /// S&H は区間平均 (経路 F)。サンプル `n` の
    /// ホールド値を区間 `[n-1, n]` で段がとった値の時間平均とし、段の値は隣接 2 サンプルの
    /// 線形補間で読む。`rate >= 1` なので区間内の境界は高々 1 つ、加重和は 2 項で済む。
    /// 整数 `rate` では境界が `δ = 0` に来て混合項が式から消えるため、
    /// 出力は現行実装を 1 サンプル遅らせた列と厳密に一致する
    /// (この 1 サンプルは実レイテンシで、`set_latency_samples(1)` で報告する。§0-2b)。
    #[inline]
    pub fn process(&mut self, x: f32, bits: f32, rate: f32, mix: f32) -> f32 {
        // 非有限のみ弾く。振幅の clamp は置かない (:tilt と同じ裁定。
        // 旧 `clamp(-4.0, 4.0)` / 出力 `clamp(-6.0, 6.0)` はどちらも
        // `attic/excite` 由来のコピーで、本 crate の DSP からは導かれない)。
        //
        // 撤去の決め手は **`mix = 0` の厳密恒等**。`out = (1-mix)·x_dry + mix·y_wet`
        // なので `mix = 0` では出力が入力そのものでなければならないが、
        // 入力 clamp があると `|x| > 4` で恒等が破れていた
        // (実測: 振幅 8 の正弦で最大 |out - in| = 4.0)。
        // ピーク管理は `finalist` の管轄であり、crush は代行しない。
        let x_dry = if x.is_finite() { x } else { 0.0 };

        let rate = if rate.is_finite() { rate.max(1.0) as f64 } else { 1.0 };

        let x_held = if self.edge_dist < 1.0 {
            // 区間内に境界がある。`d` は区間開始からの位置。
            let d = self.edge_dist as f32;
            let new_hold = self.x_in_prev * (1.0 - d) + x_dry * d;
            let mixed = self.hold * d + new_hold * (1.0 - d);
            self.hold = new_hold;
            self.edge_dist += rate - 1.0;
            mixed
        } else {
            // 境界なし。区間全体が同じ段。
            self.edge_dist -= 1.0;
            self.hold
        };
        self.x_in_prev = x_dry;

        let y_raw = shape_adaa_quant(x_held, self.x_prev, bits);
        self.x_prev = x_held as f64;
        let y_gated = denormal_gate(y_raw);
        let y_wet = self.dc_blocker.process(y_gated);

        let out = (1.0 - mix) * x_dry + mix * y_wet;
        // 非有限時は状態も掃除する。
        //
        // **本 crate では現時点でこの枝に到達しない** (実測: `±f32::MAX` 交互 20k
        // サンプルを bits/rate 全域で通しても非有限は 0 個。`DcBlocker` が
        // `y_prev` へ書く前に自己サニタイズし、`CrushChannel` の遅延段は
        // すべて有限な入力由来のため)。到達不能を保つのは
        // `hostile_input_stays_finite_and_recovers` の役目。
        //
        // それでも `reset()` を置くのは、:tilt で同型のガードが
        // **状態に残った非有限を隠して永久ミュートを作っていた**実績があるため。
        // 出力のゼロ化だけでは「壊れたまま無音が続く」を検出できない。
        // `shape_adaa_quant` の内部が変われば到達可能になりうる。
        if out.is_finite() {
            out
        } else {
            self.reset();
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantize_16bit_near_transparent() {
        let y = quantize(0.5, 16.0);
        assert!((y - 0.5).abs() < 1e-4, "y={y}");
    }

    #[test]
    fn quantize_1bit_fixed_expected() {
        // Δ=2、m(x)=floor(x/Δ+1/2) (round-half-up = 正方向優先の丸め)。
        // 境界点は Δ·(m+1/2) = 奇数、つまり x=-1,1 がちょうど境界。
        // x=-1: -1/2+1/2=0.0 → floor=0 → m=0 (round-half-up は +∞ 側優先のため
        // タイでは 0 側 = m=-1 ではなく m=0 を選ぶ)。
        // x=1: 1/2+1/2=1.0 → floor=1 → m=1。
        // 初期設計で置いた期待値 [-2,0,0,0,2] は
        // x=-1 の要素が誤り (同計画書内の式 floor(x/Δ+1/2) から実際に導出すると
        // m=0 になり y=0 が正しい)。式を正として実測値に合わせて修正。
        let expected = [0.0_f32, 0.0, 0.0, 0.0, 2.0];
        let inputs = [-1.0_f32, -0.5, 0.0, 0.5, 1.0];
        for (x, e) in inputs.iter().zip(expected.iter()) {
            let y = quantize(*x, 1.0);
            assert!((y - e).abs() < 1e-6, "x={x} expected={e} got={y}");
        }
    }

    #[test]
    fn quantize_4bit_boundary_has_one_step_discontinuity() {
        let b = 4.0_f32;
        let d = delta(b);
        for m in -5..5 {
            let x_boundary = (m as f32 + 0.5) * d;
            let just_before = quantize(x_boundary - 1e-5, b);
            let just_after = quantize(x_boundary + 1e-5, b);
            assert!(
                (just_after - just_before - d).abs() < 1e-4,
                "m={m} before={just_before} after={just_after} d={d}"
            );
        }
    }

    #[test]
    fn adaa_eps_guard_no_nan_on_constant_input() {
        // x_prev == x が連続する定常入力 (dx=0) で ε ガードが常に発火 → NaN/Inf 不発生。
        let b = 8.0_f32;
        let x = 0.3_f32;
        for _ in 0..1000 {
            let y = shape_adaa_quant(x, x as f64, b);
            assert!(y.is_finite(), "y non-finite: y={y}");
        }
    }

    /// future_plugins.md §測定用ユーティリティ転記 (fs=48000、整数周期でリーク回避)。
    fn goertzel_amp(x: &[f32], f: f64, fs: f64) -> f64 {
        let w = 2.0 * std::f64::consts::PI * f / fs;
        let coeff = 2.0 * w.cos();
        let (mut s1, mut s2) = (0.0f64, 0.0f64);
        for &v in x {
            let s0 = v as f64 + coeff * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        let power = s1 * s1 + s2 * s2 - coeff * s1 * s2;
        2.0 * power.max(0.0).sqrt() / x.len() as f64
    }

    #[test]
    fn adaa_quant_16bit_transparent_997hz() {
        let sr = 48000.0_f64;
        let f = 997.0_f64;
        let n = 48000usize; // 997 周期ちょうど、リーク回避
        let b = 16.0_f32;

        let mut x_prev = 0.0_f64;
        let out: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f64 / sr;
                let x = (2.0 * std::f64::consts::PI * f * t).sin() as f32;
                let y = shape_adaa_quant(x, x_prev, b);
                x_prev = x as f64;
                y
            })
            .collect();

        // 計画書 Step 4 記載の「基本波振幅比 1.0±0.001」は数学的に成立しない値。
        // ADAA 1 次差分 (F(x)-F(x_prev))/(x-x_prev) は Δ→0 の極限でも
        // y=(x[n]+x[n-1])/2 の 2 点移動平均に一致し、周波数応答は |cos(w/2)|
        // (w=2π·997/48000) になる (計画書 §リスク「ADAA 実効 0.5 サンプル遅れ」
        // が指摘する減衰そのもの)。実測 fundamental=0.997871、
        // 理論値 cos(w/2)=0.997872 で誤差 1e-6 と一致し、バグではなく ADAA の
        // 原理的特性と確認済み。よって基本波振幅は理論値近傍であることのみ検証し、
        // 公式受け入れ基準 1 (非基本波 -80dB 未満) を主張として検証する。
        let fundamental = goertzel_amp(&out, f, sr);
        let w = 2.0 * std::f64::consts::PI * f / sr;
        let theoretical = (w / 2.0).cos();
        assert!(
            (fundamental - theoretical).abs() < 1e-4,
            "fundamental amp {fundamental} deviates from ADAA theoretical response {theoretical}"
        );
        for bin in [500.0, 1500.0, 2500.0, 3500.0] {
            let a = goertzel_amp(&out, bin, sr);
            let db = 20.0 * a.max(1e-12).log10();
            assert!(db < -80.0, "bin {bin} Hz not below -80dB: {db} dB");
        }
    }

    #[test]
    fn f_antideriv_continuous_at_all_boundaries() {
        let b = 4.0_f64;
        let d = 2.0 / (2.0_f64.powf(b) - 1.0);
        for m in -100..=100 {
            let x_boundary = (m as f64 + 0.5) * d;
            let left = f_antideriv_f64(x_boundary - 1e-12, b);
            let right = f_antideriv_f64(x_boundary + 1e-12, b);
            assert!(
                (right - left).abs() < 1e-9,
                "m={m} x_boundary={x_boundary} left={left} right={right} diff={}",
                (right - left).abs()
            );
        }
    }

    /// 受け入れ基準 1 (公式版): CrushChannel フル経路 (S&H→ADAA→DcBlocker→mix)
    /// で bits=16, rate=1, mix=1.0、997 Hz 正弦、基本波以外の Goertzel 測定点
    /// (500/1500/2500/3500 Hz) 合計 < -80 dB 相当。
    #[test]
    fn channel_997hz_nonfundamental_below_minus80db() {
        let sr = 48000.0_f64;
        let f = 997.0_f64;
        let n = 48000usize;
        let mut ch = CrushChannel::new();
        let out: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f64 / sr;
                let x = (2.0 * std::f64::consts::PI * f * t).sin() as f32;
                ch.process(x, 16.0, 1.0, 1.0)
            })
            .collect();
        for bin in [500.0, 1500.0, 2500.0, 3500.0] {
            let a = goertzel_amp(&out, bin, sr);
            let db = 20.0 * a.max(1e-12).log10();
            assert!(db < -80.0, "bin {bin} Hz not below -80dB: {db} dB");
        }
    }

    /// 受け入れ基準 4: x[n]==x[n-1] 定常入力でチャンネル出力が全て有限。
    #[test]
    fn channel_constant_input_stays_finite() {
        let mut ch = CrushChannel::new();
        for _ in 0..2000 {
            let y = ch.process(0.3, 8.0, 1.0, 1.0);
            assert!(y.is_finite(), "y non-finite: {y}");
        }
    }

    /// 受け入れ基準 5: 敵対的入力で出力が全て有限。
    #[test]
    fn channel_hostile_input_stays_finite() {
        let mut ch = CrushChannel::new();
        for &x in &[1e9_f32, -1e9, f32::INFINITY, f32::NAN, 100.0, -100.0, 0.0] {
            let y = ch.process(x, 8.0, 4.0, 0.5);
            assert!(y.is_finite(), "output non-finite for x={x}: y={y}");
        }
    }

    /// 受け入れ基準 6: mix=0 で入出力誤差 < 1e-6 (dry は S&H 前の x を保持)。
    #[test]
    fn channel_mix_zero_is_transparent() {
        let mut ch = CrushChannel::new();
        for n in 0..1000 {
            let x = 0.7 * ((n as f32) * 0.037).sin();
            let y = ch.process(x, 4.0, 8.0, 0.0);
            assert!((y - x).abs() < 1e-6, "n={n} x={x} y={y}");
        }
    }

    /// S&H: rate>1 でホールド期間中 (同一グループ内、2 サンプル目以降) は
    /// x_held が変化しないため ADAA が dx=0 の ε ガードへ落ち、一定値になること。
    /// CrushChannel 全体だと DcBlocker (R=0.999) が定常値でも緩やかに減衰させる
    /// ため、S&H+ADAA 単体の組合せをここで直接検証する (DcBlocker は Step 5 の
    /// 別テストで個別に検証済み)。
    #[test]
    fn sample_and_hold_plateaus_within_group_pre_dc_block() {
        let rate = 4u32;
        let bits = 6.0_f32;
        let mut hold = 0.0_f32;
        let mut hold_counter = 0u32;
        let mut x_prev = 0.0_f64;
        // pos_in_group==0 はグループ切替の遷移サンプル (dx≠0、実 ADAA 補間値)。
        // pos_in_group>=1 は x_held が前サンプルと同一 (dx=0) になるため
        // ε ガードへ落ち、グループ内で一定値が続くはず。遷移サンプルとは
        // 比較せず、内部プラトー同士 (pos>=1) のみ比較する。
        let mut prev_interior: Option<f32> = None;
        for n in 0..40u32 {
            let x = 0.5 * ((n as f32) * 0.25).sin();
            if hold_counter == 0 {
                hold = x;
            }
            hold_counter = (hold_counter + 1) % rate;
            let x_held = hold;
            let y = shape_adaa_quant(x_held, x_prev, bits);
            x_prev = x_held as f64;

            let pos_in_group = n % rate;
            if pos_in_group == 0 {
                prev_interior = None;
            } else {
                if let Some(p) = prev_interior {
                    assert!(
                        (y - p).abs() < 1e-6,
                        "n={n} pos={pos_in_group} y={y} prev={p} (S&H plateau violated)"
                    );
                }
                prev_interior = Some(y);
            }
        }
    }

    /// 受け入れ基準 3: fs=48k, 1046.5 Hz 正弦, bits=4 でナイーブ量子化 vs ADAA の
    /// 折返しエイリアス成分が 10 dB 以上改善。
    ///
    /// **計画書 Step 6 記載のテストコードは実行不能な設計バグを含んでいたため
    /// 手法を修正した** (実装のバグではなく計測方法のバグ、根拠は以下):
    /// 1. 計画書の窓長 `n=48000` (1 秒) は f=1046.5Hz に対し非整数周期
    ///    (1046.5 周期) となり、`測定用ユーティリティ` 節が明記する
    ///    「テスト信号は必ず整数周期」というリーク回避規則に違反していた。
    ///    実測: 未処理の 1046.5Hz 正弦単体でも bin=2000Hz に -74.7dB の
    ///    スペクトルリークが漏れ込み、naive/ADAA 双方の測定値がこのリーク
    ///    (実体はほぼ同一の基本波成分) に支配され、実質的に何も測っていない
    ///    状態だった (実測 improvement=0.5dB という結果はこのリーク由来)。
    ///    修正: 窓長を `n=96000` (2 秒 = 1046.5Hz のちょうど 2093 周期) に
    ///    変更しリークを排除 (整数周期規則の遵守)。
    /// 2. 固定 bin=2000Hz は 1046.5Hz の高調波格子 (1046.5, 2093, 3139.5, …)
    ///    上になく、リークを除去すると naive 側はほぼ厳密に 0 (浮動小数点
    ///    ノイズ床未満) となり、比較が数値ノイズ同士の比になり無意味だった。
    ///    修正: 単一 bin ではなく Nyquist 未満の正規高調波 (k=1..22、
    ///    1046.5Hz の整数倍) のエネルギー合計をパワースペクトルの全エネルギー
    ///    から差し引いた残余 = 折返し (Nyquist 超の高調波が鏡映する) エイリ
    ///    アス成分のみを抽出して比較する (正規高調波は ADAA の対象外の意図的
    ///    歪みであり、除外しないと改善率が希釈される)。
    ///
    /// 修正後、計画書が指定した bits=4・amplitude=0.8 のまま実測
    /// improvement_db ≈ 10.7 dB で 10 dB 基準を満たす (実装は元々正しかった)。
    #[test]
    fn adaa_reduces_alias_vs_naive() {
        let sr = 48000.0_f64;
        let f = 1046.5_f64; // C6、非整数分周比で強エイリアス
        let n = 96000usize; // 2 秒 = 2093 周期ちょうど (整数周期、リーク回避)
        let bits = 4.0_f32;
        let amp = 0.8_f32;
        let max_harmonic = (sr / 2.0 / f).floor() as i32; // Nyquist 未満の正規高調波次数上限

        // naive: ADAA 無しで直接 quantize
        let naive: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f64 / sr;
                let x = (2.0 * std::f64::consts::PI * f * t).sin() as f32 * amp;
                quantize(x, bits)
            })
            .collect();

        // ADAA: shape_adaa_quant
        let mut x_prev = 0.0_f64;
        let adaa: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f64 / sr;
                let x = (2.0 * std::f64::consts::PI * f * t).sin() as f32 * amp;
                let y = shape_adaa_quant(x, x_prev, bits);
                x_prev = x as f64;
                y
            })
            .collect();

        let mean_sq = |v: &[f32]| -> f64 {
            v.iter().map(|&x| (x as f64) * (x as f64)).sum::<f64>() / v.len() as f64
        };
        // Nyquist 未満の正規高調波 (基本波含む k=1..max_harmonic) のエネルギー合計。
        let inband_energy = |v: &[f32]| -> f64 {
            (1..=max_harmonic)
                .map(|k| {
                    let a = goertzel_amp(v, k as f64 * f, sr);
                    0.5 * a * a
                })
                .sum()
        };

        // 総エネルギーから正規高調波を引いた残余 = 折返しエイリアス成分。
        let alias_naive = (mean_sq(&naive) - inband_energy(&naive)).max(0.0);
        let alias_adaa = (mean_sq(&adaa) - inband_energy(&adaa)).max(0.0);
        let improvement_db = 10.0 * (alias_naive / alias_adaa.max(1e-18)).log10();
        assert!(
            improvement_db > 10.0,
            "ADAA improvement {improvement_db:.1} dB < 10 dB target"
        );
    }

    /// nice-bulk の `no_click_on_steady_tone` と同型の回帰基準。
    /// 定常正弦に対する出力の 2 階差分の最大値が dry の 2 倍未満であること。
    /// f32 差分商では bits=16 で dry 比 5.4 倍に達していた。
    #[test]
    fn no_click_on_steady_tone_16bit() {
        let sr = 48000.0_f64;
        let f = 60.0_f64;
        let n = 4800;
        let bits = 16.0_f32;

        let dry: Vec<f32> = (0..n)
            .map(|i| (2.0 * std::f64::consts::PI * f * i as f64 / sr).sin() as f32)
            .collect();
        let mut x_prev = 0.0_f64;
        let wet: Vec<f32> = dry
            .iter()
            .map(|&x| {
                let y = shape_adaa_quant(x, x_prev, bits);
                x_prev = x as f64;
                y
            })
            .collect();

        let d2_max = |v: &[f32]| -> f64 {
            v.windows(3)
                .map(|w| ((w[2] as f64) - 2.0 * (w[1] as f64) + (w[0] as f64)).abs())
                .fold(0.0_f64, f64::max)
        };
        // 先頭 1 周期は x_prev=0 からの起動過渡 (ADAA 1 次の半サンプル遅れ) なので除く。
        let warmup = (sr / f) as usize;
        let (dw, dd) = (d2_max(&wet[warmup..]), d2_max(&dry[warmup..]));
        assert!(
            dw < 2.0 * dd,
            "d2_max wet={dw:.3e} dry={dd:.3e} ratio={:.2}x >= 2x",
            dw / dd
        );
    }

    /// 敵対入力で出力が非有限にならず、かつスパイク後に正常レベルへ戻ること。
    ///
    /// 振幅 clamp を撤去したので `±f32::MAX` がそのまま DSP へ入る。守るのは 2 つ:
    ///   1. 出力に非有限が漏れない（＝`process` の非有限枝が到達不能であり続ける）
    ///   2. スパイク 1 発の後に信号レベルが戻る（状態に残骸が居座らない）
    ///
    /// **復帰時間は閉形式で決まる。** 残骸を落とすのは DC ブロッカの 1 極 HP なので
    /// 時定数は `τ = 1/(2π·DC_BLOCK_FC_HZ) ≈ 52.1 ms`、つまり
    /// `復帰時間 ≈ τ · ln(spike)` = **0.120 秒 / decade**。
    /// 実測（0.5 正弦を流し続けた場合の |y| < 2.0 到達時刻）:
    /// `1e3` 0.002 s、`1e6` 0.265 s、`1e12` 0.987 s、`1e38` 4.11 s、`f32::MAX` 4.17 s。
    /// 差分は 6 decade で 0.72 s / 26 decade で 3.12 s = いずれも 0.120 s/decade。
    ///
    /// 現実的な入力（`1e3` = +60 dBFS）では 2 ms で、旧 `clamp(-6, 6)` の有無は問題にならない。
    #[test]
    fn hostile_input_stays_finite_and_recovers() {
        // (1) 交互フルスケール極値。ADAA の差分商が最も大きくなる形。
        for &bits in &[1.0f32, 8.0, 16.0] {
            for &rate in &[1.0f32, 1.5, 32.0] {
                let mut ch = CrushChannel::new();
                ch.set_sample_rate(48_000.0);
                for i in 0..20_000 {
                    let x = if i % 2 == 0 { f32::MAX } else { -f32::MAX };
                    let y = ch.process(x, bits, rate, 1.0);
                    assert!(y.is_finite(), "bits={bits} rate={rate} i={i}: y={y}");
                }
            }
        }

        // (2) 敵対スパイク 1 発 → 通常信号へ復帰。
        // 上限は閉形式の 0.120 s/decade に 1.5 倍の余裕を取る。
        let tau = 1.0 / (2.0 * std::f64::consts::PI * DC_BLOCK_FC_HZ);
        for &(spike, decades) in
            &[(f32::INFINITY, 0.0), (f32::NAN, 0.0), (1e3f32, 3.0), (1e12, 12.0), (f32::MAX, 38.6)]
        {
            let budget_s = (tau * decades * std::f64::consts::LN_10 * 1.5).max(0.05);
            let n_total = (48_000.0 * (budget_s + 0.1)) as usize;
            let n_check = (48_000.0 * budget_s) as usize;

            let mut ch = CrushChannel::new();
            ch.set_sample_rate(48_000.0);
            let mut out = Vec::with_capacity(n_total);
            for i in 0..n_total {
                let x = if i == 0 {
                    spike
                } else {
                    0.5 * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / 48_000.0).sin()
                };
                out.push(ch.process(x, 8.0, 1.0, 1.0));
            }
            assert!(out.iter().all(|v| v.is_finite()), "spike={spike}: 非有限が漏れた");

            let tail = out[n_check..].iter().fold(0.0f32, |a, &v| a.max(v.abs()));
            assert!(
                (0.1..2.0).contains(&tail),
                "spike={spike}: {budget_s:.3} 秒で復帰していない (tail peak={tail})"
            );
        }
    }

    /// `mix = 0` は入力振幅に依らず**厳密恒等**であること。
    ///
    /// 旧実装の入力 `clamp(-4, 4)` はこれを `|x| > 4` で破っていた
    /// (実測: 振幅 8 の正弦で最大 |out - in| = 4.0)。撤去の決め手がこの性質なので、
    /// 回帰として固定する。
    #[test]
    fn mix_zero_is_exact_identity_at_any_amplitude() {
        for &amp in &[0.5f32, 1.0, 4.0, 8.0, 100.0] {
            let mut ch = CrushChannel::new();
            ch.set_sample_rate(48_000.0);
            for i in 0..2_000 {
                let x = amp * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / 48_000.0).sin();
                let y = ch.process(x, 8.0, 4.0, 0.0);
                assert_eq!(y, x, "amp={amp} i={i}: mix=0 で恒等が破れた");
            }
        }
    }

    /// `DC_BLOCK_FC_HZ` の定数値が閉形式 `F_MIN·√(10^(A/10)−1)` と一致すること。
    /// 自己参照 (旧 `R = 0.999` からの逆算) を切った根拠がこの式であり、
    /// 定数を手で書き換えたら落ちる。
    #[test]
    fn dc_block_fc_matches_closed_form() {
        let expected =
            DC_BLOCK_F_MIN_HZ * (10f64.powf(DC_BLOCK_MAX_DEV_DB / 10.0) - 1.0).sqrt();
        assert!(
            (DC_BLOCK_FC_HZ - expected).abs() < 1e-12,
            "DC_BLOCK_FC_HZ={DC_BLOCK_FC_HZ} expected={expected}"
        );
    }

    /// 実際の離散系 `H(z) = (1−z⁻¹)/(1−R·z⁻¹)` の 20 Hz 振幅偏差が
    /// 予算 0.1 dB 以内であること。対応 sr 全域で確認する。
    #[test]
    fn dc_block_deviation_at_20hz_within_budget() {
        for &sr in &[44_100.0f32, 48_000.0, 88_200.0, 96_000.0, 176_400.0, 192_000.0] {
            let r = dc_block_r(sr) as f64;
            let w = 2.0 * std::f64::consts::PI * DC_BLOCK_F_MIN_HZ / sr as f64;
            let (c, s_) = ((-w).cos(), (-w).sin());
            let (num_re, num_im) = (1.0 - c, -s_);
            let (den_re, den_im) = (1.0 - r * c, -r * s_);
            let amp = ((num_re * num_re + num_im * num_im)
                / (den_re * den_re + den_im * den_im))
                .sqrt();
            let dev_db = 20.0 * amp.log10();
            assert!(
                dev_db >= -DC_BLOCK_MAX_DEV_DB && dev_db <= 0.0,
                "sr={sr}: 20 Hz deviation {dev_db} dB out of budget"
            );
        }
    }
}
