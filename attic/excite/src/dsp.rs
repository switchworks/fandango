//! Excite DSP — bounded tanh 波形整形 (Even/Odd ブレンド) + Chebyshev 多項式
//! 波形整形 (`plans/excite_chebyshev.md` Step 5 合流) の
//! bass_legacy_mix/hi_legacy_mix 混合。
//!
//! Legacy (tanh) shaper は ADAA 無し (v0.2 送り、設計方針: `plans/fandango_design.md`)。
//! Chebyshev shaper は ADAA 1 次を代数展開形式 (分岐・除算無し) で実装済み。
//! 両エンジンは同一の帯域フィルタ/エンベロープ出力 (`filt`/`env`) を共有し、
//! wet 段階で `bass_legacy_mix`/`hi_legacy_mix` (0.0=Chebyshev, 1.0=Legacy、
//! デフォルト 0.0=Chebyshev) により線形補間する
//! (`plans/excite_chebyshev.md` 2026-07-13 決定事項: 加算ではなく補間、
//! soft-limit/DC ブロッカーは合成後の wet に対して 1 回のみ適用)。

use fandango_shared::Biquad;

/// shaper 入力の絶対値上限。tanh(x) は x=10 で 1 に飽和、それ以上不要。
const SHAPER_INPUT_CLAMP: f32 = 10.0;

/// Chebyshev シェイパー入力の正規化ヘッドルーム。帯域フィルタ出力
/// (`[-2, 2]` にクランプ済) をこれで割ることでシェイパー入力を `[-1, 1]` へ
/// 写像する (`|x|>1` での Chebyshev 多項式の爆発的増加を防ぐハード制約)。
const CHEBY_HEADROOM: f32 = 2.0;

/// Chebyshev シェイパー入力の pre-gain。headroom 正規化後に掛けて `[-1,1]` へ
/// hard clamp、実効振幅を引き上げ Poisson 重みの振幅依存減衰を補う。
/// `crates/excite-ab` での A/B 計測・官能評価により確定 (Step 3 完了記録)。
const CHEBY_DRIVE: f32 = 3.0;

/// Denormal ゲート閾値。これ未満の絶対値は 0.0 に丸める。
/// x86 CPU の subnormal microcode fallback (100倍以上遅延) を回避、
/// ホスト側「暴走プラグイン」誤判定 → kill を防ぐ最重要対策。
const DENORMAL_GATE: f32 = 1e-30;

/// Denormal ゲート適用。|x| < DENORMAL_GATE → 0.0。
#[inline]
fn denormal_gate(x: f32) -> f32 {
    if x.abs() < DENORMAL_GATE { 0.0 } else { x }
}

/// Bass 帯域の内部ドライブゲイン。
const BASS_DRIVE: f32 = 6.0;
/// Hi 帯域の内部ドライブゲイン。tanh 飽和抑制で刺々しさ回避。
const HI_DRIVE: f32 = 5.0;
/// Bass 倍音の最終メイクアップ。dry 支配環境で知覚可能な wet 量を確保。
const BASS_MAKEUP: f32 = 4.0;
/// Hi 倍音の最終メイクアップ。
const HI_MAKEUP: f32 = 6.0;

/// wet 単独 soft-limit 上限 (dry 加算前)。
/// makeup 増加分の headroom 確保のため 1.5 → 2.5 へ。dry+wet が clip しない設計は維持。
const WET_SOFT_LIMIT: f32 = 2.5;

/// γ (偶数倍音の比率) — Character 選択に対応。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Character {
    Warm,    // γ = 0.7
    Neutral, // γ = 0.5
    Bright,  // γ = 0.3
}

impl Character {
    pub fn gamma(self) -> f32 {
        match self {
            Character::Warm => 0.7,
            Character::Neutral => 0.5,
            Character::Bright => 0.3,
        }
    }

    /// Chebyshev シェイパー用 (奇数次重み, 偶数次重み)。
    /// `plans/excite_chebyshev.md`「Character パラメータの再定義」参照。
    #[inline]
    pub fn odd_even_weights(self) -> (f32, f32) {
        match self {
            Character::Warm => (0.3, 1.0),
            Character::Neutral => (0.7, 0.7),
            Character::Bright => (1.0, 0.3),
        }
    }
}

/// Odd 関数 (tanh)。奇数倍音源、対称飽和。
#[inline]
fn odd_f(x: f32) -> f32 {
    x.tanh()
}

/// Even 関数 (tanh²·sign)。偶数倍音源、非対称飽和、有界。
#[inline]
fn even_f(x: f32) -> f32 {
    let t = x.tanh();
    t * t * x.signum()
}

/// γ ブレンド波形整形。ADAA 無し、単純評価。
#[inline]
fn shape(x: f32, gamma: f32) -> f32 {
    gamma * even_f(x) + (1.0 - gamma) * odd_f(x)
}

/// Poisson 重み計算に渡す `a` の事前補正。呼び出し側 (`wet_cheby`) で既に
/// `bass_amt`/`hi_amt` を 1 乗で掛けている上、この関数内 `k=2` 項が
/// 追加で `a^1` を掛けるため、無補正だと wet 全体が実質 `amt^2` (`k=5` 項は
/// `amt^5`) に比例してしまう。`amt = 10^((db-40)/20)` は既に指数写像な為、
/// 二重指数で 30〜40dB 付近にのみ急峻な knee が生じる (2026-07-17 ユーザー報告:
/// 「効きが30〜40dBでだけ顕著」)。`sqrt` で緩和し 0〜40dB 全域へ効きを分散、
/// 40dB (a=1.0) 到達点はそのまま維持。
#[inline]
fn poisson_input(amt: f32) -> f32 {
    amt.sqrt()
}

/// Poisson 分布重み `w_k(a) = a^(k-1) / (k-1)!`。`k = 2..=5` のみ使用
/// (`k=0,1` は定数・恒等写像で倍音生成に寄与しないため除外)。
#[inline]
fn poisson_weight(a: f32, k: u32) -> f32 {
    let factorial = match k - 1 {
        1 => 1.0,
        2 => 2.0,
        3 => 6.0,
        4 => 24.0,
        _ => unreachable!("k must be in 2..=5"),
    };
    a.powi((k - 1) as i32) / factorial
}

/// 次数 `k` (2..=5) の合成重み = Poisson 重み × Character 偶奇重み。
/// Hi 帯域専用 (`plans/excite_bass_high_order.md`: Hi は K_max=5 据え置き、
/// 基音自体が高いため 5 次でも十分な帯域まで倍音が届く)。
#[inline]
fn combined_weights_hi(a: f32, character: Character) -> [f32; 4] {
    let a = poisson_input(a);
    let (w_odd, w_even) = character.odd_even_weights();
    [
        poisson_weight(a, 2) * w_even,
        poisson_weight(a, 3) * w_odd,
        poisson_weight(a, 4) * w_even,
        poisson_weight(a, 5) * w_odd,
    ]
}

/// Chebyshev シェイパー ADAA 1 次評価 (代数展開形式、分岐・除算なし)。
/// `crates/excite-chebyshev/src/dsp.rs` の `shape_adaa` から移植 (同一実装)。
/// 導出の詳細はそちらの doc コメント参照。Hi 帯域専用 (K_max=5)。
#[inline]
fn shape_adaa_hi(x: f32, x_prev: f32, weights: [f32; 4]) -> f32 {
    let [w2, w3, w4, w5] = weights;
    let s = x + x_prev;
    let p = x * x_prev;

    let p2 = s;
    let p3 = s * p2 - p;
    let p4 = s * p3 - p * p2;
    let p5 = s * p4 - p * p3;
    let p6 = s * p5 - p * p4;

    let q2 = (2.0 / 3.0) * p3;
    let q3 = p4 - 1.5 * p2;
    let q4 = 1.6 * p5 - (8.0 / 3.0) * p3;
    let q5 = (8.0 / 3.0) * p6 - 5.0 * p4 + 2.5 * p2;

    w2 * q2 + w3 * q3 + w4 * q4 + w5 * q5
}

/// Bass 帯域の倍音減衰率 ρ。`w_k(a) = a^(k-1) · ρ^(k-2)` の幾何減衰。
/// `plans/excite_bass_high_order.md`: Poisson (factorial) 減衰は k=6 以降が
/// 事実上ゼロになり 40Hz 基音で 200Hz 止まりだった実測を受け、tanh 系と同様の
/// 指数減衰に置換 (Fable セカンドオピニオン 2026-07-17 の提案採用)。
/// 初期値はシミュレーション比較 (K_max=9, ρ=0.6 が滑らかな減衰、ρ=0.75 は
/// 相互変調由来の不自然な盛り上がりを確認) に基づく、実機官能評価で再調整可。
const BASS_HARMONIC_DECAY: f32 = 0.6;

/// Bass 帯域用幾何減衰重み。`poisson_weight` の Bass 版
/// (`plans/excite_bass_high_order.md` 参照)。
#[inline]
fn geometric_weight(a: f32, k: u32) -> f32 {
    a.powi((k - 1) as i32) * BASS_HARMONIC_DECAY.powi((k - 2) as i32)
}

/// 次数 `k=2..=9` (K_max=9) の合成重み = 幾何減衰重み × Character 偶奇重み。
/// Bass 帯域専用。150Hz LPF 通過後の基音上限が 150Hz のため、K_max=9 でも
/// 生成倍音は最大 1350Hz 止まりでエイリアシング懸念は実質ゼロ (Fable
/// セカンドオピニオン参照)。Hi 帯域は基音が高くエイリアシング増のリスクが
/// あるため K_max=5 のまま据え置き (`combined_weights_hi`)。
#[inline]
fn combined_weights_bass(a: f32, character: Character) -> [f32; 8] {
    let a = poisson_input(a);
    let (w_odd, w_even) = character.odd_even_weights();
    let w_of = |k: u32| if k % 2 == 0 { w_even } else { w_odd };
    [
        geometric_weight(a, 2) * w_of(2),
        geometric_weight(a, 3) * w_of(3),
        geometric_weight(a, 4) * w_of(4),
        geometric_weight(a, 5) * w_of(5),
        geometric_weight(a, 6) * w_of(6),
        geometric_weight(a, 7) * w_of(7),
        geometric_weight(a, 8) * w_of(8),
        geometric_weight(a, 9) * w_of(9),
    ]
}

/// Chebyshev シェイパー ADAA 1 次評価、Bass 帯域専用 (K_max=9)。
/// `shape_adaa_hi` と同一手法 (`P_j` 漸化式による代数展開、除算・分岐なし) を
/// `k=6..9` まで延長。`q6..q9` の係数は SymPy 自動導出
/// (`plans/excite_bass_high_order.md`「数式導出の検証」節参照、
/// `F_k(x)=∫T_k(x)dx` の各項係数をそのまま `P_j` 係数として使い、
/// DC 相殺分 `-T_k(0)` を引く手法を q2..q5 で再現・一致確認済みの上で延長)。
#[inline]
fn shape_adaa_bass(x: f32, x_prev: f32, weights: [f32; 8]) -> f32 {
    let [w2, w3, w4, w5, w6, w7, w8, w9] = weights;
    let s = x + x_prev;
    let p = x * x_prev;

    let p2 = s;
    let p3 = s * p2 - p;
    let p4 = s * p3 - p * p2;
    let p5 = s * p4 - p * p3;
    let p6 = s * p5 - p * p4;
    let p7 = s * p6 - p * p5;
    let p8 = s * p7 - p * p6;
    let p9 = s * p8 - p * p7;
    let p10 = s * p9 - p * p8;

    let q2 = (2.0 / 3.0) * p3;
    let q3 = p4 - 1.5 * p2;
    let q4 = 1.6 * p5 - (8.0 / 3.0) * p3;
    let q5 = (8.0 / 3.0) * p6 - 5.0 * p4 + 2.5 * p2;
    let q6 = (32.0 / 7.0) * p7 - (48.0 / 5.0) * p5 + 6.0 * p3;
    let q7 = 8.0 * p8 - (56.0 / 3.0) * p6 + 14.0 * p4 - 3.5 * p2;
    let q8 = (128.0 / 9.0) * p9 - (256.0 / 7.0) * p7 + 32.0 * p5 - (32.0 / 3.0) * p3;
    let q9 = (128.0 / 5.0) * p10 - 72.0 * p8 + 72.0 * p6 - 30.0 * p4 + 4.5 * p2;

    w2 * q2 + w3 * q3 + w4 * q4 + w5 * q5 + w6 * q6 + w7 * q7 + w8 * q8 + w9 * q9
}

/// シンプルなピーク・エンベロープ・フォロワ (attack/release 別)。
pub struct EnvFollower {
    env: f32,
    attack_coef: f32,
    release_coef: f32,
}

impl EnvFollower {
    pub fn new(sr: f32, attack_ms: f32, release_ms: f32) -> Self {
        let sr = sr.max(1.0);
        Self {
            env: 0.0,
            attack_coef: 1.0 - (-1.0 / (attack_ms * 0.001 * sr)).exp(),
            release_coef: 1.0 - (-1.0 / (release_ms * 0.001 * sr)).exp(),
        }
    }

    pub fn reset(&mut self) {
        self.env = 0.0;
    }

    pub fn set_times(&mut self, sr: f32, attack_ms: f32, release_ms: f32) {
        let sr = sr.max(1.0);
        self.attack_coef = 1.0 - (-1.0 / (attack_ms * 0.001 * sr)).exp();
        self.release_coef = 1.0 - (-1.0 / (release_ms * 0.001 * sr)).exp();
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        if !self.env.is_finite() {
            self.env = 0.0;
        }
        let target = x.abs();
        let coef = if target > self.env { self.attack_coef } else { self.release_coef };
        self.env += coef * (target - self.env);
        // Denormal 化した env は 0 に丸めて subnormal CPU fallback 阻止。
        self.env = denormal_gate(self.env);
        self.env
    }
}

/// 帯域: 2 段 Butterworth LP または HP (= LR4 相当)。
pub struct BandFilter {
    stage1: Biquad,
    stage2: Biquad,
}

impl BandFilter {
    pub fn butterworth_lpf(sr: f32, fc: f32) -> Self {
        Self {
            stage1: Biquad::butterworth_lpf(sr, fc),
            stage2: Biquad::butterworth_lpf(sr, fc),
        }
    }
    pub fn butterworth_hpf(sr: f32, fc: f32) -> Self {
        Self {
            stage1: Biquad::butterworth_hpf(sr, fc),
            stage2: Biquad::butterworth_hpf(sr, fc),
        }
    }
    pub fn reset(&mut self) {
        self.stage1.reset();
        self.stage2.reset();
    }
    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        // 入力側で微小 DC bias 加算 → biquad 内部 state を denormal 領域から常時逃がす。
        // 加算値 1e-25 は音として不可聴 (-500dBFS)、しかし subnormal 化は完全に阻止。
        let x_bias = x + 1e-25;
        let y = self.stage2.process(self.stage1.process(x_bias));
        if y.is_finite() { denormal_gate(y) } else { self.reset(); 0.0 }
    }
}

/// 出力段 DC ブロッカー (1-pole highpass, `y[n] = x[n]-x[n-1]+R·y[n-1]`)。
/// カットオフ ≈ `(1-R)·sr/(2π)`、48kHz で ≈ 7.6 Hz。Chebyshev エンジンの
/// エンベロープ追従 DC 対策 (`plans/excite_chebyshev.md` Step 0.5 完了記録) として
/// 導入、合流後は mix 値に関わらず常時 (legacy 単独時も含め) 適用する。
/// 可聴帯域 (20Hz 以上) への影響は無視できるレベルのため、bass_mix=0/hi_mix=0
/// 時も出力は legacy 単独実装と聴感上等価 (bit-exact ではない、Step 5 実装ノート)。
const DC_BLOCK_R: f32 = 0.999;

pub struct DcBlocker {
    x_prev: f32,
    y_prev: f32,
}

impl DcBlocker {
    pub fn new() -> Self {
        Self { x_prev: 0.0, y_prev: 0.0 }
    }
    pub fn reset(&mut self) {
        self.x_prev = 0.0;
        self.y_prev = 0.0;
    }
    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = x - self.x_prev + DC_BLOCK_R * self.y_prev;
        self.x_prev = x;
        let y = if y.is_finite() { denormal_gate(y) } else { 0.0 };
        self.y_prev = y;
        y
    }
}

/// 1 チャンネル分の excite パイプライン。Legacy (tanh) / Chebyshev 両エンジンを
/// 内包し、`bass_mix`/`hi_mix` で wet を線形補間する。
pub struct ExciteChannel {
    lp_bass: BandFilter,
    hp_hi: BandFilter,
    env_bass: EnvFollower,
    env_hi: EnvFollower,
    /// Chebyshev ADAA 用の帯域別直前サンプル (正規化後)。
    x_prev_bass_norm: f32,
    x_prev_hi_norm: f32,
    dc_blocker: DcBlocker,
}

impl ExciteChannel {
    pub fn new(sr: f32) -> Self {
        Self {
            lp_bass: BandFilter::butterworth_lpf(sr, 150.0),
            hp_hi: BandFilter::butterworth_hpf(sr, 4000.0),
            env_bass: EnvFollower::new(sr, 5.0, 80.0),
            env_hi: EnvFollower::new(sr, 1.0, 30.0),
            x_prev_bass_norm: 0.0,
            x_prev_hi_norm: 0.0,
            dc_blocker: DcBlocker::new(),
        }
    }

    pub fn set_sr(&mut self, sr: f32) {
        self.lp_bass = BandFilter::butterworth_lpf(sr, 150.0);
        self.hp_hi = BandFilter::butterworth_hpf(sr, 4000.0);
        self.env_bass.set_times(sr, 5.0, 80.0);
        self.env_hi.set_times(sr, 1.0, 30.0);
    }

    pub fn reset(&mut self) {
        self.lp_bass.reset();
        self.hp_hi.reset();
        self.env_bass.reset();
        self.env_hi.reset();
        self.x_prev_bass_norm = 0.0;
        self.x_prev_hi_norm = 0.0;
        self.dc_blocker.reset();
    }

    #[inline]
    #[allow(clippy::too_many_arguments)]
    pub fn process(
        &mut self,
        x: f32,
        bass_amt: f32,
        hi_amt: f32,
        character: Character,
        bass_legacy_mix: f32,
        hi_legacy_mix: f32,
    ) -> f32 {
        let x = if x.is_finite() { x.clamp(-4.0, 4.0) } else { 0.0 };
        let gamma = character.gamma();
        let mut wet = 0.0;

        if bass_amt > 1e-4 {
            let filt = self.lp_bass.process(x).clamp(-2.0, 2.0);
            let env = self.env_bass.process(filt);

            let driven = (filt * BASS_DRIVE).clamp(-SHAPER_INPUT_CLAMP, SHAPER_INPUT_CLAMP);
            let shaped_legacy = denormal_gate(shape(driven, gamma));
            let wet_legacy = bass_amt * BASS_MAKEUP * env * shaped_legacy;

            let x_norm = ((filt / CHEBY_HEADROOM) * CHEBY_DRIVE).clamp(-1.0, 1.0);
            let weights = combined_weights_bass(bass_amt, character);
            let shaped_cheby =
                denormal_gate(shape_adaa_bass(x_norm, self.x_prev_bass_norm, weights));
            self.x_prev_bass_norm = x_norm;
            let wet_cheby = bass_amt * BASS_MAKEUP * env * shaped_cheby;

            let contrib = bass_legacy_mix * wet_legacy + (1.0 - bass_legacy_mix) * wet_cheby;
            if contrib.is_finite() {
                wet += denormal_gate(contrib);
            }
        }
        if hi_amt > 1e-4 {
            let filt = self.hp_hi.process(x).clamp(-2.0, 2.0);
            let env = self.env_hi.process(filt);

            let driven = (filt * HI_DRIVE).clamp(-SHAPER_INPUT_CLAMP, SHAPER_INPUT_CLAMP);
            let shaped_legacy = denormal_gate(shape(driven, gamma));
            let wet_legacy = hi_amt * HI_MAKEUP * env * shaped_legacy;

            let x_norm = ((filt / CHEBY_HEADROOM) * CHEBY_DRIVE).clamp(-1.0, 1.0);
            let weights = combined_weights_hi(hi_amt, character);
            let shaped_cheby = denormal_gate(shape_adaa_hi(x_norm, self.x_prev_hi_norm, weights));
            self.x_prev_hi_norm = x_norm;
            let wet_cheby = hi_amt * HI_MAKEUP * env * shaped_cheby;

            let contrib = hi_legacy_mix * wet_legacy + (1.0 - hi_legacy_mix) * wet_cheby;
            if contrib.is_finite() {
                wet += denormal_gate(contrib);
            }
        }

        // wet 自体を先に denormal_gate → subnormal cancellation (a - a + tiny) 抹消。
        let wet = denormal_gate(wet);
        // wet を tanh で soft-limit → dry と加算しても hard-clip 起きない。
        // soft-limit・DC ブロッカーは合成後の wet に対して各 1 回のみ適用
        // (エンジンごとの多重適用禁止、過大化防止の根拠が崩れるため)。
        let wet_limited = (wet / WET_SOFT_LIMIT).tanh() * WET_SOFT_LIMIT;
        let wet_dc_blocked = self.dc_blocker.process(denormal_gate(wet_limited));
        let out = x + denormal_gate(wet_dc_blocked);
        if out.is_finite() { out.clamp(-6.0, 6.0) } else { 0.0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_amounts_pass_through() {
        let mut ch = ExciteChannel::new(48000.0);
        let out = ch.process(0.5, 0.0, 0.0, Character::Neutral, 0.0, 0.0);
        assert!((out - 0.5).abs() < 1e-6);
    }

    #[test]
    fn shape_bounded() {
        for x in [-100.0, -10.0, -1.0, 0.0, 1.0, 10.0, 100.0] {
            for gamma in [0.0, 0.5, 1.0] {
                let y: f32 = shape(x, gamma);
                assert!(y.is_finite(), "shape non-finite: x={x} gamma={gamma}");
                assert!(y.abs() < 1.001, "shape unbounded: y={y}");
            }
        }
    }

    /// `shape_adaa_bass` の数値安定性検証 (`plans/excite_bass_high_order.md`
    /// Step 4)。理論上限重み (全 1.0) でも `x,x_prev ∈ [-1,1]` 格子全探索で
    /// 有限・非爆発であることを確認 (既存 `bounded_output_for_clamped_input`
    /// パターン踏襲、f32 桁落ちによる異常値が出ないかの実測)。
    #[test]
    fn bass_shaper_bounded_at_extremes() {
        let weights = [1.0f32; 8];
        let mut max_abs: f32 = 0.0;
        let n = 41;
        for i in 0..n {
            for j in 0..n {
                let x = -1.0 + 2.0 * (i as f32) / (n as f32 - 1.0);
                let x_prev = -1.0 + 2.0 * (j as f32) / (n as f32 - 1.0);
                let y = shape_adaa_bass(x, x_prev, weights);
                assert!(y.is_finite(), "non-finite at x={x} x_prev={x_prev}: y={y}");
                max_abs = max_abs.max(y.abs());
            }
        }
        assert!(max_abs < 250.0, "shape_adaa_bass unbounded: max_abs={max_abs}");
    }

    /// `shape_adaa_bass(x,x,w)` が `x=x_prev` 極限で `Σw_k·(T_k(x)-T_k(0))`
    /// に一致することを確認 (`chebyshev_polynomial_correctness` パターン踏襲、
    /// SymPy 自動導出した q6..q9 の転記ミス検出用)。
    #[test]
    fn bass_shaper_matches_chebyshev_at_limit() {
        fn chebyshev_t(k: u32, x: f64) -> f64 {
            match k {
                0 => 1.0,
                1 => x,
                _ => {
                    let mut t0 = 1.0;
                    let mut t1 = x;
                    for _ in 2..=k {
                        let t2 = 2.0 * x * t1 - t0;
                        t0 = t1;
                        t1 = t2;
                    }
                    t1
                }
            }
        }
        let weights = [1.0f32; 8];
        for &x in &[-0.9_f32, -0.3, 0.0, 0.4, 0.85] {
            let got = shape_adaa_bass(x, x, weights);
            let xd = x as f64;
            let expect: f64 =
                (2..=9).map(|k| chebyshev_t(k, xd) - chebyshev_t(k, 0.0)).sum();
            assert!(
                (got as f64 - expect).abs() < 1e-3,
                "mismatch at x={x}: got={got} expect={expect}"
            );
        }
    }

    /// K_max=9 幾何減衰 (Bass専用) が、K_max=5 Poisson (旧アプローチ相当、
    /// `combined_weights_hi`/`shape_adaa_hi` を同一入力で流用) と比べ
    /// h=6〜9 (240〜360Hz、40Hz基音) のパワーが有意に増加することを確認
    /// (`plans/excite_bass_high_order.md` 受け入れ基準 1)。
    #[test]
    fn bass_high_order_weights_extend_spectrum() {
        let sr = 48000.0_f32;
        let fund = 40.0_f32;
        // SR/n=10Hz/bin として 40Hz の倍数がすべて厳密に bin 中心に乗るように
        // n を選ぶ (スペクトルリーケージ排除、`plans/excite_chebyshev.md`
        // Step 3 の計測手法踏襲)。
        let n = 4800usize;

        fn goertzel_power(buf: &[f32], target_freq: f32, sr: f32) -> f64 {
            let w = 2.0 * std::f32::consts::PI * target_freq / sr;
            let coeff = 2.0 * w.cos() as f64;
            let mut s1 = 0.0f64;
            let mut s2 = 0.0f64;
            for &xv in buf {
                let s0 = xv as f64 + coeff * s1 - s2;
                s2 = s1;
                s1 = s0;
            }
            s1 * s1 + s2 * s2 - coeff * s1 * s2
        }

        let weights_bass = combined_weights_bass(1.0, Character::Neutral);
        let weights_hi5 = combined_weights_hi(1.0, Character::Neutral);

        let mut buf_bass = vec![0.0f32; n];
        let mut buf_hi5 = vec![0.0f32; n];
        let mut xp_bass = 0.0f32;
        let mut xp_hi5 = 0.0f32;
        for (i, (b, h)) in buf_bass.iter_mut().zip(buf_hi5.iter_mut()).enumerate() {
            let t = i as f32 / sr;
            let x_in = 0.5 * (2.0 * std::f32::consts::PI * fund * t).sin();
            let x_norm = ((x_in / CHEBY_HEADROOM) * CHEBY_DRIVE).clamp(-1.0, 1.0);
            *b = shape_adaa_bass(x_norm, xp_bass, weights_bass);
            *h = shape_adaa_hi(x_norm, xp_hi5, weights_hi5);
            xp_bass = x_norm;
            xp_hi5 = x_norm;
        }

        for harmonic in 6..=9u32 {
            let target = fund * harmonic as f32;
            let p_bass = goertzel_power(&buf_bass, target, sr);
            let p_hi5 = goertzel_power(&buf_hi5, target, sr);
            assert!(
                p_bass > p_hi5 * 5.0,
                "h={harmonic} ({target}Hz): bass(K_max=9) power {p_bass} not significantly > K_max=5 power {p_hi5}"
            );
        }
    }

    #[test]
    fn bass_amt_produces_audible_delta() {
        let sr = 48000.0;
        let mut ch_dry = ExciteChannel::new(sr);
        let mut ch_wet = ExciteChannel::new(sr);
        let mut max_delta: f32 = 0.0;
        for n in 0..4800 {
            let x = 0.3 * (2.0 * std::f32::consts::PI * 50.0 * (n as f32) / sr).sin();
            let dry = ch_dry.process(x, 0.0, 0.0, Character::Neutral, 0.0, 0.0);
            let wet = ch_wet.process(x, 1.0, 0.0, Character::Neutral, 0.0, 0.0);
            max_delta = max_delta.max((wet - dry).abs());
        }
        assert!(max_delta > 0.05, "bass wet-dry delta too small: {max_delta}");
    }

    #[test]
    fn hi_amt_produces_audible_delta() {
        let sr = 48000.0;
        let mut ch_dry = ExciteChannel::new(sr);
        let mut ch_wet = ExciteChannel::new(sr);
        let mut max_delta: f32 = 0.0;
        for n in 0..4800 {
            let x = 0.3 * (2.0 * std::f32::consts::PI * 5000.0 * (n as f32) / sr).sin();
            let dry = ch_dry.process(x, 0.0, 0.0, Character::Neutral, 0.0, 0.0);
            let wet = ch_wet.process(x, 0.0, 1.0, Character::Neutral, 0.0, 0.0);
            max_delta = max_delta.max((wet - dry).abs());
        }
        assert!(max_delta > 0.05, "hi wet-dry delta too small: {max_delta}");
    }

    #[test]
    fn bass_high_amt_kick_input_bounded() {
        let sr = 48000.0;
        let mut ch = ExciteChannel::new(sr);
        let mut max_abs: f32 = 0.0;
        for n in 0..48000 {
            let t = n as f32 / sr;
            let kick_env = (-t * 15.0).exp();
            let x = 0.9 * kick_env * (2.0 * std::f32::consts::PI * 40.0 * t).sin();
            let y = ch.process(x, 0.9, 0.0, Character::Neutral, 0.0, 0.0);
            assert!(y.is_finite(), "non-finite output at n={n}: y={y}");
            max_abs = max_abs.max(y.abs());
        }
        assert!(max_abs < 6.5, "output exploded: max_abs={max_abs}");
    }

    #[test]
    fn hostile_input_stays_finite() {
        let mut ch = ExciteChannel::new(48000.0);
        for &x in &[1e9_f32, -1e9, f32::INFINITY, f32::NAN, 100.0, -100.0, 0.0] {
            let y = ch.process(x, 1.0, 1.0, Character::Neutral, 0.0, 0.0);
            assert!(y.is_finite(), "output non-finite for x={x}: y={y}");
        }
    }

    #[test]
    fn long_silence_after_signal_stays_finite_and_zero() {
        // 信号 → 完全silence の遷移で subnormal 経路を炙り出す。
        // 実 DAW で「BASS=90 で数秒経つと落ちる」相当の状況再現。
        let sr = 48000.0;
        let mut ch = ExciteChannel::new(sr);
        // 1 秒間: 40Hz 0.5 amplitude sine で env をチャージ
        for n in 0..48000 {
            let x = 0.5 * (2.0 * std::f32::consts::PI * 40.0 * (n as f32) / sr).sin();
            let _ = ch.process(x, 0.9, 0.9, Character::Warm, 0.0, 0.0);
        }
        // 5 秒間 完全silence: env は指数減衰 → subnormal 領域通過
        for n in 0..48000 * 5 {
            let y = ch.process(0.0, 0.9, 0.9, Character::Warm, 0.0, 0.0);
            assert!(y.is_finite(), "non-finite during silence at n={n}: y={y}");
            // 後半は wet が完全 0 に収束していること
            if n > 48000 * 3 {
                assert!(y.abs() < 1e-20, "silence residue too large at n={n}: y={y}");
            }
        }
    }

    #[test]
    fn rapid_hi_amt_sweep_stays_finite() {
        // 高速 hi knob 動作を再現 (0 ↔ 1 を 1ms 周期でスイープ)。
        let sr = 48000.0;
        let mut ch = ExciteChannel::new(sr);
        for n in 0..48000 * 3 {
            let t = n as f32 / sr;
            let x = 0.3 * (2.0 * std::f32::consts::PI * 200.0 * t).sin();
            // 1kHz でスイープ = 1ms 周期でフル振れ
            let hi = 0.5 + 0.5 * (2.0 * std::f32::consts::PI * 1000.0 * t).sin();
            let y = ch.process(x, 0.0, hi.clamp(0.0, 1.0), Character::Neutral, 0.0, 0.0);
            assert!(y.is_finite(), "non-finite during hi sweep at n={n}");
        }
    }

    #[test]
    fn env_follower_tracks_peak() {
        let mut e = EnvFollower::new(48000.0, 1.0, 100.0);
        for _ in 0..2000 {
            e.process(1.0);
        }
        assert!(e.env > 0.99);
    }

    /// bass_legacy_mix=0.0(Chebyshev)/1.0(Legacy) で挙動が実際に切り替わって
    /// いること (合流実装の主目的、`plans/excite_chebyshev.md` Step 5) を、
    /// 同一入力に対する出力差で確認。
    #[test]
    fn bass_legacy_mix_extremes_differ() {
        let sr = 48000.0;
        let mut ch_cheby = ExciteChannel::new(sr);
        let mut ch_legacy = ExciteChannel::new(sr);
        let mut max_delta: f32 = 0.0;
        for n in 0..4800 {
            let x = 0.5 * (2.0 * std::f32::consts::PI * 50.0 * (n as f32) / sr).sin();
            let y_cheby = ch_cheby.process(x, 1.0, 0.0, Character::Neutral, 0.0, 0.0);
            let y_legacy = ch_legacy.process(x, 1.0, 0.0, Character::Neutral, 1.0, 0.0);
            max_delta = max_delta.max((y_legacy - y_cheby).abs());
        }
        assert!(max_delta > 0.02, "bass_legacy_mix 0 vs 1 output too similar: {max_delta}");
    }

    #[test]
    fn hi_legacy_mix_extremes_differ() {
        let sr = 48000.0;
        let mut ch_cheby = ExciteChannel::new(sr);
        let mut ch_legacy = ExciteChannel::new(sr);
        let mut max_delta: f32 = 0.0;
        for n in 0..4800 {
            let x = 0.5 * (2.0 * std::f32::consts::PI * 4500.0 * (n as f32) / sr).sin();
            let y_cheby = ch_cheby.process(x, 0.0, 1.0, Character::Neutral, 0.0, 0.0);
            let y_legacy = ch_legacy.process(x, 0.0, 1.0, Character::Neutral, 0.0, 1.0);
            max_delta = max_delta.max((y_legacy - y_cheby).abs());
        }
        assert!(max_delta > 0.02, "hi_legacy_mix 0 vs 1 output too similar: {max_delta}");
    }

    /// mix 中間値 (加算ではなく補間) で kick 型の激しい入力を通しても
    /// 出力が有限かつ soft-limit 相当の範囲に収まること
    /// (計画書「新たな懸念とその解決」: 単純加算方式の過大化リスクへの対策確認)。
    #[test]
    fn legacy_mix_interpolation_stays_bounded() {
        let sr = 48000.0;
        for &(bass_legacy_mix, hi_legacy_mix) in &[(0.25, 0.25), (0.5, 0.5), (0.75, 0.75), (1.0, 1.0)] {
            let mut ch = ExciteChannel::new(sr);
            let mut max_abs: f32 = 0.0;
            for n in 0..48000 {
                let t = n as f32 / sr;
                let kick_env = (-t * 15.0).exp();
                let x = 0.9 * kick_env * (2.0 * std::f32::consts::PI * 40.0 * t).sin();
                let y = ch.process(x, 0.9, 0.9, Character::Neutral, bass_legacy_mix, hi_legacy_mix);
                assert!(y.is_finite(), "non-finite at mix=({bass_legacy_mix},{hi_legacy_mix}) n={n}: y={y}");
                max_abs = max_abs.max(y.abs());
            }
            assert!(
                max_abs < 6.5,
                "output exploded at mix=({bass_legacy_mix},{hi_legacy_mix}): max_abs={max_abs}"
            );
        }
    }
}
