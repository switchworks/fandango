//! nice-bulk DSP コア — 帯域別パワーローゲイン (`plans/nice_bulk_v01.md` 案1)。
//!
//! ```text
//! u         = ((env + eps) / env_typical)^(-alpha)
//! g_clamped = g_max * u / (g_max + u - 1)   // u=1 -> 1 (較正点が固定点), u→∞ -> g_max
//! G0(alpha) = 1 + (G0_MAX - 1) * alpha      // 静的メイクアップ (alpha=0 で 1)
//! g_final   = G0(alpha) * (1 + max(g_clamped - 1, 0))   // カット側を落とす
//! y         = sat(low_out; L*G0(a_bass)) + mid + sat(high_out; L*G0(a_hi))
//! ```
//! `g_clamped` の式は当初案 `g*g_max/(g+g_max)` から変更 (計画書 Step 1 訂正)。
//! 旧式は `g=1` (無処理相当) で `g_max/(1+g_max)` を返し 1.0 からズレる
//! (bypass 相当のはずが常時減衰する欠陥) ため、`g=1` を厳密に固定点とする式に
//! 差し替え済み。パワーローの基準を較正点に統一した結果、較正メイクアップと
//! 2 段目 clamp が不要になり定数が 2 個減っている
//! (`plans/nice_bulk_constant_reduction_v01.md`)。
//!
//! 最終段の写像は `plans/nice_bulk_reinforce_v01.md` (2026-08-03)。旧
//! `plans/nice_bulk_asymmetric_gain_v01.md` の `M_UP`/`M_DOWN` 非対称パラレルは
//! 廃止済み: 較正点を軸にした対称シーソーである以上、通常のマスタ2mix のように
//! 帯域 env が較正点を上回る素材では alpha を上げるほど正味が下がる欠陥が
//! 残っていたため (`M_DOWN=0.30` は減衰を 1/3 に浅くしただけで符号を変えて
//! いなかった)。新写像はカット側を完全に落とし、ノブに静的メイクアップ
//! `G0(alpha)` を持たせることで `g_final >= 1` と alpha 単調増加を恒等式として
//! 保証する。

use fandango_shared::Lr4Crossover;

/// ホスト側「暴走プラグイン」誤判定 → kill を防ぐ最重要対策。
const DENORMAL_GATE: f32 = 1e-30;

#[inline]
fn denormal_gate(x: f32) -> f32 {
    if x.abs() < DENORMAL_GATE { 0.0 } else { x }
}

/// シンプルなピーク・エンベロープ・フォロワ (attack/release 別)。
/// `crates/excite/src/dsp.rs` の同名構造体と同一実装 (excite crate は
/// `plans/nice_bulk_v01.md` 変更禁止領域のため参照のみ、複製して独立管理)。
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

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        if !self.env.is_finite() {
            self.env = 0.0;
        }
        let target = x.abs();
        let coef = if target > self.env { self.attack_coef } else { self.release_coef };
        self.env += coef * (target - self.env);
        self.env = denormal_gate(self.env);
        self.env
    }
}

/// パワーロー式の内部定数。
///
/// **素材の絶対レベルに紐づく定数は `ENV_TYPICAL` と `BASS_ENV_TYPICAL` の 2 個のみ**
/// (後者は前者の +6dB という代数関係で導出されるため実質 1 個)。
/// この 2 個が `:nice-bulk` の弱点そのものであり、較正点起因のバグが 2 回発生している
/// (2026-07-19「上げると削れる」、2026-08-03「上げると逆に下がる」)。
/// 安易に増やさないこと (`plans/fandango_design.md` §至上命題 優先順位 3)。
///
/// 経緯: かつては「圧縮開始の閾値」`REF_LEVEL=0.03` を較正点と別に持っていた
/// (2026-07-19、較正点の上下**両方**に作用する対称シーソー型の写像だったため)。
/// `plans/nice_bulk_reinforce_v01.md` でカット側を `max(g-1,0)` で落とした結果、
/// 較正点より上は一切作用しなくなり「圧縮開始の閾値」という概念自体が消滅。
/// `REF_LEVEL` は役目を終えたため廃止し、パワーローの基準を較正点に統一した
/// (`plans/nice_bulk_constant_reduction_v01.md`)。これにより makeup 項と
/// 2 段目 clamp (`G_MAX_FINAL`) も同時に不要になっている。
const EPS: f32 = 1e-9;
const ENV_TYPICAL: f32 = 0.25;
const G_MAX: f32 = 8.0;

/// ノブ最大時 (`alpha=1`) の静的メイクアップ量 (+6.0dB)。
/// UI 非公開。実機評価で不足なら 2.5、過大なら 1.5 まで調整可
/// (`plans/nice_bulk_reinforce_v01.md` §2.2、いずれの値でも下限 1.0 と
/// alpha 単調性は式の構造から保たれる)。
const G0_MAX: f32 = 2.0;

/// 最終段サチュレータの出力上限。旧 `out.clamp(-6.0, 6.0)` のハードクリップを
/// 置換する安全網であり、リミッターではない。
///
/// 1.0 (0dBFS) にすると事実上のリミッターとなり、本強化の目的である
/// トランジェント復活を打ち消す (減衰キック実測: L=1.0 でピーク +1.06dB 止まり、
/// L=4.0 で +10.13dB)。一方 L=4.0 は通常レベルでほぼ透明 (x=1.0 で -0.26dB)。
/// ブースト分の絶対レベル調整はホスト側フェーダーの役割。
const SAT_CEILING: f32 = 4.0;

/// ADAA の 0/0 回避しきい値。差分がこれ未満なら中点での直接評価に落とす。
const ADAA_EPS: f64 = 1e-9;

/// Bass帯域の較正点。`ENV_TYPICAL` に対し固定 +6dB オフセット
/// (`10^(6/20)≈1.9953`, `0.25*1.9953≈0.499`→`0.5`に丸め)。
/// 当初 `bass_threshold` ノブでユーザー調整可能にしていたが、複数曲での
/// 実機検証の結果「+6dB固定で足りる」と判断されノブ削除・固定化
/// (2026-07-19)。Hi帯域は据え置き較正点 (`ENV_TYPICAL`) のままで
/// 十分な効きが確認されているため非対称のまま (Bassのみ固定オフセット)。
const BASS_ENV_TYPICAL: f32 = 0.5;

/// 較正点を基準とした一次ゲイン (発散防止の有理 clamp 込み)。
///
/// `env = env_typical` のとき `u = 1` となり、clamp は `G/(G+1-1) = 1` を返す。
/// つまり**較正点が clamp の固定点そのもの**であり、alpha に依らず厳密に 1。
/// この性質により、旧実装が必要としていた較正メイクアップ
/// (`1/g_clamped(env_typical)`) と、その適用後のズレを直すための 2 段目 clamp が
/// どちらも不要になる (`plans/nice_bulk_constant_reduction_v01.md` §1.3)。
#[inline]
fn g_clamped(env: f32, alpha: f32, env_typical: f32) -> f32 {
    let u = ((env + EPS) / env_typical).powf(-alpha);
    let u = if u.is_finite() { u } else { G_MAX };
    G_MAX * u / (G_MAX + u - 1.0)
}

/// ノブ値がそのまま持つ静的な帯域ゲイン。`alpha=0` で 1 (恒等)。
#[inline]
fn static_makeup(alpha: f32) -> f32 {
    1.0 + (G0_MAX - 1.0) * alpha
}

/// `env`/`alpha`/`env_typical` からゲインを計算。`alpha=0` または `env` が
/// 有限でない場合は `1.0` (恒等) を返す。
///
/// カット側 (`g_clamped<1`) を `max(·,0)` で落とし、静的メイクアップ `G0(alpha)` を
/// 乗じる。これにより式の構造から次が保証される
/// (`plans/nice_bulk_reinforce_v01.md` §2.2):
/// - `g >= G0(alpha) >= 1` — ノブを上げて音量が下がることが原理的に起きない
/// - alpha に対し厳密単調増加 (`env>=env_typical` では `g=G0(alpha)` そのもの)
/// - `alpha=0` で完全恒等、上限は `G0_MAX * G_MAX` で有界
///
/// `env_typical` を引数化しているのは Bass/Hi で異なる較正点
/// (`BASS_ENV_TYPICAL`/`ENV_TYPICAL`) を同一ロジックで扱うため
/// (経緯: 当初は `bass_threshold` ノブでユーザーが曲ごとに手動調整する
/// 設計だったが、複数曲検証の結果 Bass は+6dB固定で足りると判断されノブ
/// 削除・定数化、Hi は据え置き較正点のまま。2026-07-19)。
#[inline]
fn power_law_gain(env: f32, alpha: f32, env_typical: f32) -> f32 {
    if alpha <= 1e-4 {
        return 1.0;
    }
    let env = if env.is_finite() { env.max(0.0) } else { 0.0 };
    let g = g_clamped(env, alpha, env_typical);
    static_makeup(alpha) * (1.0 + (g - 1.0).max(0.0))
}

/// 静的伝達特性（GUI 可視化用）。定常状態では包絡が入力振幅へ収束するため、
/// 振幅 `a` の入力に対する帯域出力は `sat(a·g(a), L(alpha))` になる。
///
/// **GUI 側でこの式を書き直さず、この関数を直接呼ぶこと**
/// (`plans/gui_overhaul_v01.md` §Step 5 受け入れ基準)。音声処理には使わない
/// （実処理は `PowerLawGain::process` + `AdaaSaturator::process` の経路）。
pub fn static_transfer(a: f32, alpha: f32, env_typical: f32) -> f32 {
    let g = power_law_gain(a, alpha, env_typical);
    sat_f(a * g, band_sat_ceiling(alpha))
}

/// Bass 帯域の較正点（GUI 可視化用）。
pub const fn bass_env_typical() -> f32 {
    BASS_ENV_TYPICAL
}

/// Hi 帯域の較正点（GUI 可視化用）。
pub const fn hi_env_typical() -> f32 {
    ENV_TYPICAL
}

/// GUI 可視化用の周波数応答。オーディオ処理には使わない。
///
/// 「そのレベルの信号がその周波数にあるとき、何 dB 持ち上がるか」を、
/// **処理と同じクロスオーバー係数**と [`static_transfer`] から合成する。
pub struct VisualResponse {
    crossover: ThreeWayCrossover,
    sr: f32,
}

impl VisualResponse {
    pub fn new(sr: f32) -> Self {
        Self {
            crossover: ThreeWayCrossover::new(sr),
            sr,
        }
    }

    /// 周波数 `f` [Hz]、帯域包絡レベル `a` におけるゲイン [dB]。
    pub fn gain_db(&self, f: f32, a: f32, bass_alpha: f32, hi_alpha: f32) -> f32 {
        let (low, mid, high) = self.crossover.magnitudes(self.sr, f);
        let g_bass = static_transfer(a, bass_alpha, BASS_ENV_TYPICAL) / a;
        let g_hi = static_transfer(a, hi_alpha, ENV_TYPICAL) / a;
        let total = low * g_bass + mid + high * g_hi;
        20.0 * total.max(1.0e-6).log10()
    }
}

/// 閉形式 1 次 ADAA ソフトサチュレータ。
///
/// `f(x) = L·x/√(L²+x²)` (奇関数・`C^∞`・分岐なし・出力上限 `L`)、
/// 不定積分 `F(x) = L·√(L²+x²)` も閉形式のため、1 次 ADAA
/// `y = (F(x)-F(x₁))/(x-x₁)` が LUT・反復なしで書ける
/// (`:excite` の Chebyshev ADAA、`:crush` の量子化 ADAA と同型の設計)。
pub struct AdaaSaturator {
    x1: f64,
}

#[inline]
fn sat_f(x: f32, l: f32) -> f32 {
    l * x / (l * l + x * x).sqrt()
}

/// 帯域サチュレータの天井。静的メイクアップに比例させることで、
/// ブーストがサチュレーション深度を増やさない
/// (`plans/nice_bulk_band_saturation_v01.md` §2.3)。
#[inline]
fn band_sat_ceiling(alpha: f32) -> f32 {
    SAT_CEILING * static_makeup(alpha)
}

/// `sat_f` の不定積分から定数 `L²` を引いた等価形。
///
/// `F(x) = L·√(L²+x²)` をそのまま使うと、小信号でも値が `L²` 付近 (L=4 なら 16)
/// に張り付くため、ADAA の `F(x)−F(x₁)` が「大きな値同士の減算」になり f32 で
/// 桁落ちする。実測 (振幅 0.05 / 200Hz の正弦) で SNR 13.7dB まで劣化し、
/// 可聴の常時クラックルノイズとして現れた (2026-08-03 ユーザー報告)。
///
/// ADAA は差分しか使わないため定数項は完全に相殺され、
/// `L(√(L²+x²) − L) = L·x²/(√(L²+x²)+L)` へ有理化すると小 x で 0 に近い値に
/// なり減算が桁落ちしない。同条件の SNR 91.5dB。**この形を素朴な
/// `L·√(L²+x²)` に戻してはならない。**
///
/// ただし有理化だけでは不十分で、評価は f64 で行う必要がある
/// (理由は [`AdaaSaturator::process`] 参照)。
#[inline]
fn sat_antiderivative(x: f64, l: f64) -> f64 {
    let s = (l * l + x * x).sqrt();
    l * x * x / (s + l)
}

impl AdaaSaturator {
    pub fn new() -> Self {
        Self { x1: 0.0 }
    }

    pub fn reset(&mut self) {
        self.x1 = 0.0;
    }

    /// サチュレータを通さない経路でも ADAA の前値を追従させる。
    #[inline]
    pub fn prime(&mut self, x: f32) {
        self.x1 = if x.is_finite() { x as f64 } else { 0.0 };
    }

    /// 差分商は f64 で計算する。
    ///
    /// `x` と `x₁` が近づくと `F(x)−F(x₁)` は元の値より数桁小さくなる。振幅 1 の
    /// 60Hz 正弦のピーク付近では `F≈0.52` に対し差が `1e-5` オーダーまで縮み、
    /// f32 (相対誤差 ~1e-7) では商に約 5e-3 (−45dB) の誤差スパイクが乗る。
    /// 正弦の山と谷ごと (60Hz なら毎秒 120 回) に発生するため、散発的な
    /// プチノイズとして可聴だった (2026-08-03 ユーザー報告)。有理化 (`sat_antiderivative`)
    /// だけでは足りず、f64 の 53bit 仮数が必要。**f32 に戻してはならない。**
    #[inline]
    pub fn process(&mut self, x: f32, l: f32) -> f32 {
        let x = if x.is_finite() { x as f64 } else { 0.0 };
        let delta = x - self.x1;
        // Δ→0 では 0/0 になるため、極限値である中点評価に落とす。
        let y = if delta.abs() < ADAA_EPS {
            sat_f((0.5 * (x + self.x1)) as f32, l) as f64
        } else {
            let l = l as f64;
            (sat_antiderivative(x, l) - sat_antiderivative(self.x1, l)) / delta
        };
        self.x1 = x;
        let y = y as f32;
        if y.is_finite() { denormal_gate(y) } else { 0.0 }
    }
}

impl Default for AdaaSaturator {
    fn default() -> Self {
        Self::new()
    }
}

/// 1 帯域分のパワーローゲイン処理 (エンベロープ検出 + ゲイン適用)。
pub struct PowerLawGain {
    env: EnvFollower,
}

impl PowerLawGain {
    pub fn new(sr: f32, attack_ms: f32, release_ms: f32) -> Self {
        Self { env: EnvFollower::new(sr, attack_ms, release_ms) }
    }

    pub fn reset(&mut self) {
        self.env.reset();
    }

    /// `env_typical` (較正点、通常は `ENV_TYPICAL` 定数だが `bass_threshold`
    /// ノブで曲ごとに調整可能な帯域はこれを外部から渡す)。
    #[inline]
    pub fn process(&mut self, x: f32, alpha: f32, env_typical: f32) -> f32 {
        let env = self.env.process(x);
        let g = power_law_gain(env, alpha, env_typical);
        let y = x * g;
        if y.is_finite() { denormal_gate(y) } else { 0.0 }
    }
}

/// 3-way LR4クロスオーバー。既存 `fandango_shared::Lr4Crossover` を
/// 150Hz/4kHzで2段カスケードするだけ (新規フィルタ実装なし、
/// `plans/nice_bulk_v01.md` Step 0.5 実測により既存実装の振幅flat性を確認済み)。
pub struct ThreeWayCrossover {
    stage_bass: Lr4Crossover,
    stage_hi: Lr4Crossover,
}

/// bass / mid 分割点 [Hz]。**オーナーの聴取で決定（2026-08-30）**。
///
/// 旧値は `:excite` から継承した `150.0` で、**導出記録がプロジェクト内に存在しなかった**
/// （`plans/` / git history / `attic/excite/src/dsp.rs:356` のすべてを確認。
/// attic の該当行はコメント無しの裸の定数）。
///
/// 探索は一時ビルド（`split-probe` feature、決定後に撤去）で分割点をスライダーに出し、
/// 実素材を通して行った。**最後まで競ったのは 141 と 160**。採用理由は次の 2 点:
///
/// - `200` まで広げると低域から上への**マスキング**が出て、上の帯域が覆われる。
///   装置の定義が「bulk = body を足す」である以上、広域を持ち上げる挙動は定義に反する
/// - `100` は「これ」と言い切れる決め手が出なかった
///
/// **代数的な錨（可聴帯の幾何中心・log 3 等分）は根拠として採らなかった。**
/// 可聴帯の端をどの丸い数字に取るかで答えが動くため（20–20k なら 100、16–16k なら 64）。
/// 装置の式のどこにも 20 Hz / 20 kHz は現れず、必然性が無い。
/// `100` `200` が Bark 臨界帯域の境界であることは実測由来の錨だが、現行の `SPLIT_HI_HZ`
/// （4000）は Bark 境界ではない（境界は 3700 / 4400）ため、根拠の種類が揃わない。
///
/// 感度は実測済み（`bass_split_sensitivity`）。ラウドネスには鈍感（60〜400 Hz で 0.32 dB 以内）、
/// **スペクトルには鈍感でない**。したがって判定器なしにこの値を動かしてはならない。
/// → `plans/release-checklist.md` §5-2-8 / `plans/nice_bulk_crossreview_v01.md`
pub const SPLIT_BASS_HZ: f32 = 141.0;
/// mid / hi 分割点 [Hz]。**由来不明のまま据え置き（2026-08-30 オーナー裁定）**。
/// `:excite` からの継承で導出記録は無い。hi 側は聴取のうえ現行で確定とした。
/// 時定数の非対称（`1/30 ms`）も excite から一度も見直されていないが、
/// 自帯域の周期に対する比では bass より保守的であることを実測済み。
pub const SPLIT_HI_HZ: f32 = 4000.0;

impl ThreeWayCrossover {
    pub fn new(sr: f32) -> Self {
        Self {
            stage_bass: Lr4Crossover::new(sr, SPLIT_BASS_HZ),
            stage_hi: Lr4Crossover::new(sr, SPLIT_HI_HZ),
        }
    }

    pub fn reset(&mut self) {
        self.stage_bass.reset();
        self.stage_hi.reset();
    }

    /// 周波数 `f` [Hz] での (low, mid, high) の振幅応答。処理と同じ係数から求める。
    pub fn magnitudes(&self, sr: f32, f: f32) -> (f32, f32, f32) {
        let (low, rest) = self.stage_bass.magnitudes(sr, f);
        let (mid, high) = self.stage_hi.magnitudes(sr, f);
        (low, rest * mid, rest * high)
    }

    /// 戻り値 (low, mid, high)。
    #[inline]
    pub fn split(&mut self, x: f32) -> (f32, f32, f32) {
        let (low, rest) = self.stage_bass.split(x);
        let (mid, high) = self.stage_hi.split(rest);
        (low, mid, high)
    }
}

/// 1 チャンネル分の nice-bulk パイプライン。
pub struct NiceBulkChannel {
    crossover: ThreeWayCrossover,
    bass_gain: PowerLawGain,
    hi_gain: PowerLawGain,
    sat_bass: AdaaSaturator,
    sat_hi: AdaaSaturator,
}

impl NiceBulkChannel {
    pub fn new(sr: f32) -> Self {
        Self {
            crossover: ThreeWayCrossover::new(sr),
            // bass の attack が速すぎるとキックの立ち上がりで即ゲインが下がり
            // トランジェントを潰す。減衰キック実測で 30ms がピーク最大
            // (`plans/nice_bulk_reinforce_v01.md` §2.3)。hi は据え置き。
            bass_gain: PowerLawGain::new(sr, 30.0, 150.0),
            hi_gain: PowerLawGain::new(sr, 1.0, 30.0),
            sat_bass: AdaaSaturator::new(),
            sat_hi: AdaaSaturator::new(),
        }
    }

    pub fn set_sr(&mut self, sr: f32) {
        *self = Self::new(sr);
    }

    pub fn reset(&mut self) {
        self.crossover.reset();
        self.bass_gain.reset();
        self.hi_gain.reset();
        self.sat_bass.reset();
        self.sat_hi.reset();
    }

    #[inline]
    pub fn process(&mut self, x: f32, bass_amount: f32, hi_amount: f32) -> f32 {
        let x = if x.is_finite() { x.clamp(-4.0, 4.0) } else { 0.0 };
        let (low, mid, high) = self.crossover.split(x);

        let low_out = self.bass_gain.process(low, bass_amount, BASS_ENV_TYPICAL);
        let high_out = self.hi_gain.process(high, hi_amount, ENV_TYPICAL);

        // 安全網は帯域内に置く。合算後に 1 個だけ置くと、その瞬時ゲインが
        // 低域振幅で変調され mid/high にも乗る (帯域間 AM)。
        // ノブ 0 の帯域は非線形段を通さず、ADAA の前値だけ追従させる
        // (ノブが 0 から動いた瞬間の不連続を防ぐ)。
        let low_sat = if bass_amount <= 1e-4 {
            self.sat_bass.prime(low_out);
            low_out
        } else {
            self.sat_bass.process(low_out, band_sat_ceiling(bass_amount))
        };
        let high_sat = if hi_amount <= 1e-4 {
            self.sat_hi.prime(high_out);
            high_out
        } else {
            self.sat_hi.process(high_out, band_sat_ceiling(hi_amount))
        };

        let out = low_sat + mid + high_sat;
        if !out.is_finite() {
            return 0.0;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 対応サンプルレート。SR 依存の不変条件はこの 3 点で確認する。
    ///
    /// 2026-08-30 追加。それまで本 crate の SR 依存テストは 48 kHz のみで、
    /// 44.1 / 96 kHz の再構成恒等性が一度も表明されていなかった
    /// （`plans/release-checklist.md` §5-2-8 の Fable 第1便が指摘、実測で確認）。
    /// バンドル全体の 44.1/48/96 kHz 実測は DAW 実機の挙動確認であって、
    /// 優先順位 1（恒等点は厳密に恒等）の表明にはなっていない。
    const SUPPORTED_SRS: [f32; 3] = [44_100.0, 48_000.0, 96_000.0];

    /// 優先順位 1: `alpha = 0` で帯域分割の再構成そのものに一致する（全対応 SR）。
    #[test]
    fn bypass_at_zero_matches_dry() {
        for sr in SUPPORTED_SRS {
            let mut dry = ThreeWayCrossover::new(sr); // 参照用: 帯域分割のみ (ゲインなし)
            let mut ch = NiceBulkChannel::new(sr);
            let n = (sr * 0.1) as usize;
            let mut max_delta: f32 = 0.0;
            for i in 0..n {
                let t = i as f32 / sr;
                let x = 0.5 * (2.0 * std::f32::consts::PI * 250.0 * t).sin();
                let (low, mid, high) = dry.split(x);
                let recon = low + mid + high;
                let out = ch.process(x, 0.0, 0.0);
                max_delta = max_delta.max((out - recon).abs());
            }
            assert!(
                max_delta < 1e-5,
                "sr = {sr}: bypass mismatch vs pure crossover recon: {max_delta}"
            );
        }
    }

    /// 優先順位 1・2: 3 分割の再構成が全対応 SR で振幅平坦であること。
    ///
    /// `bypass_at_zero_matches_dry` は「ゲイン段が恒等」を見るが、**分割器自身が
    /// 平坦か**は見ていない（参照側も同じ分割器なので誤差が相殺する）。
    /// LR4 カスケードの再構成平坦性は分割点 150 / 4000 Hz の妥当性とは独立に
    /// 成立すべき不変条件なので、分けて表明する。
    #[test]
    fn crossover_reconstruction_is_flat_all_srs() {
        for sr in SUPPORTED_SRS {
            let xo = ThreeWayCrossover::new(sr);
            let mut worst = (0.0_f32, 0.0_f32); // (f, |dB|)
            let mut f = 20.0_f32;
            while f < sr * 0.45 {
                let (low, mid, high) = xo.magnitudes(sr, f);
                let sum = low + mid + high;
                let db = 20.0 * sum.max(1e-12).log10();
                if db.abs() > worst.1 {
                    worst = (f, db.abs());
                }
                f *= 1.02;
            }
            assert!(
                worst.1 < 0.1,
                "sr = {sr}: 再構成が平坦でない。最悪 {:.4} dB @ {:.1} Hz",
                worst.1,
                worst.0
            );
        }
    }

    /// 分割点が SR に依らず同じ物理周波数に置かれること（SR 不変性）。
    /// 各分割点で low/mid, mid/high の交差が -6 dB 近傍に来ることで確認する。
    #[test]
    fn split_points_are_sr_invariant() {
        for sr in SUPPORTED_SRS {
            let xo = ThreeWayCrossover::new(sr);
            let (low_at_split, _, _) = xo.magnitudes(sr, SPLIT_BASS_HZ);
            let (_, _, high_at_4k) = xo.magnitudes(sr, 4000.0);
            let db = |a: f32| 20.0 * a.max(1e-12).log10();
            assert!(
                (db(low_at_split) + 6.0).abs() < 0.5,
                "sr = {sr}: {SPLIT_BASS_HZ} Hz での low が {:.3} dB (-6 dB 近傍でない)",
                db(low_at_split)
            );
            assert!(
                (db(high_at_4k) + 6.0).abs() < 0.5,
                "sr = {sr}: 4 kHz での high が {:.3} dB (-6 dB 近傍でない)",
                db(high_at_4k)
            );
        }
    }

    #[test]
    fn gain_bounded_at_extremes() {
        // 新写像: [1, G0_MAX*G_MAX]
        let g_lo = 1.0f32;
        let g_hi = G0_MAX * G_MAX;
        for &alpha in &[0.0f32, 0.3, 0.6, 0.9, 0.999, 1.0] {
            for &env in &[0.0f32, 1e-12, 1e-6, 0.01, 0.1, 1.0, 1e6] {
                let g = power_law_gain(env, alpha, ENV_TYPICAL);
                assert!(g.is_finite(), "non-finite g at alpha={alpha} env={env}");
                assert!(
                    g >= g_lo - 1e-3 && g <= g_hi + 1e-3,
                    "g out of bounds: alpha={alpha} env={env} g={g} lo={g_lo} hi={g_hi}"
                );
                }
        }
    }

    #[test]
    fn gain_is_unity_when_alpha_zero() {
        for &env in &[0.0f32, 0.01, 0.1, 1.0, 10.0] {
            let g = power_law_gain(env, 0.0, ENV_TYPICAL);
            assert!((g - 1.0).abs() < 1e-6, "alpha=0 should be pure passthrough: env={env} g={g}");
        }
    }

    /// 較正点では対称パワーローの寄与が消え、静的メイクアップそのものになる。
    /// (2026-08-03 以前は「較正点で g=1」だったが、それがノブを上げても正味が
    /// 上がらない原因だったため `static_makeup(alpha)` へ期待値を更新。)
    #[test]
    fn gain_equals_makeup_at_env_typical() {
        for &alpha in &[0.1f32, 0.5, 0.9] {
            let g = power_law_gain(ENV_TYPICAL, alpha, ENV_TYPICAL);
            let expect = static_makeup(alpha);
            assert!((g - expect).abs() < 1e-3, "alpha={alpha} g={g} expect={expect}");
        }
    }

    /// 較正点より上 (`env > env_typical`) では対称項が完全に落ち、ゲインは
    /// 静的メイクアップに一致する。ここが「上げると下がる」の再発防止点。
    #[test]
    fn gain_equals_makeup_above_calibration() {
        for &alpha in &[0.1f32, 0.5, 0.9] {
            for &env in &[0.6f32, 1.0, 2.0, 10.0] {
                let g = power_law_gain(env, alpha, ENV_TYPICAL);
                let expect = static_makeup(alpha);
                assert!(
                    (g - expect).abs() < 1e-5,
                    "alpha={alpha} env={env} g={g} expect={expect}"
                );
            }
        }
    }

    /// ゲインが 1.0 を下回らない (ノブを上げて音量が下がらない) ことを
    /// 両較正点・広い env 範囲で確認。2026-08-03 ユーザー報告の再発防止。
    #[test]
    fn gain_never_below_unity() {
        for &et in &[ENV_TYPICAL, BASS_ENV_TYPICAL] {
            for a in 0..=100 {
                let alpha = a as f32 * 0.01;
                for &env in &[0.0f32, 1e-9, 0.01, 0.1, 0.5, 1.0, 10.0, 1e6] {
                    let g = power_law_gain(env, alpha, et);
                    assert!(
                        g >= 1.0 - 1e-6,
                        "gain below unity: et={et} alpha={alpha} env={env} g={g}"
                    );
                }
            }
        }
    }

    /// alpha に対して単調非減少。旧 `M_DOWN` 方式では大 env 側で逆行していた。
    #[test]
    fn gain_monotonic_in_alpha() {
        for &et in &[ENV_TYPICAL, BASS_ENV_TYPICAL] {
            for e in 1..=400 {
                let env = e as f32 * 0.01;
                let mut prev = power_law_gain(env, 0.0, et);
                for a in 1..=200 {
                    let alpha = a as f32 * 0.005;
                    let g = power_law_gain(env, alpha, et);
                    assert!(
                        g >= prev - 1e-6,
                        "not monotonic in alpha: et={et} env={env} alpha={alpha} g={g} prev={prev}"
                    );
                    prev = g;
                }
            }
        }
    }

    #[test]
    fn low_level_signal_boosted() {
        let sr = 48000.0;
        let mut ch_dry = NiceBulkChannel::new(sr);
        let mut ch_wet = NiceBulkChannel::new(sr);
        let n = 48000;
        let mut max_gain_db: f32 = f32::MIN;
        for i in 0..n {
            let t = i as f32 / sr;
            // 小振幅 (較正点 BASS_ENV_TYPICAL=0.5 より十分小さい) 40Hz sine。
            let x = 0.01 * (2.0 * std::f32::consts::PI * 40.0 * t).sin();
            let dry = ch_dry.process(x, 0.0, 0.0);
            let wet = ch_wet.process(x, 0.8, 0.0);
            if dry.abs() > 1e-6 {
                let db = 20.0 * (wet.abs() / dry.abs()).log10();
                max_gain_db = max_gain_db.max(db);
            }
        }
        assert!(max_gain_db > 3.0, "low-level bass signal not boosted: max_gain_db={max_gain_db}");
    }

    /// 2026-07-19 ユーザー報告のバグ再現テスト:「bass/hi を上げると通常演奏
    /// レベルの帯域が削れて中域だけ残る」。ENV_TYPICAL 相当の通常レベル信号
    /// (bass/hi 混合) で amount を上げても、bass/hi 帯域が -6dB を超えて
    /// 削れないことを確認 (makeup 較正により env=ENV_TYPICAL 近辺は
    /// ほぼ無処理相当のはず)。
    #[test]
    fn typical_level_signal_not_excessively_attenuated() {
        let sr = 48000.0;
        let n = 48000usize;
        for &amount in &[0.3f32, 0.6, 0.9] {
            let mut ch_dry = NiceBulkChannel::new(sr);
            let mut ch_wet = NiceBulkChannel::new(sr);
            let mut sum_sq_dry = 0.0f64;
            let mut sum_sq_wet = 0.0f64;
            for i in 0..n {
                let t = i as f32 / sr;
                // ENV_TYPICAL(0.25) 相当の定常振幅を持つ bass+hi 混合信号。
                let x = 0.35 * (2.0 * std::f32::consts::PI * 60.0 * t).sin()
                    + 0.35 * (2.0 * std::f32::consts::PI * 6000.0 * t).sin();
                let dry = ch_dry.process(x, 0.0, 0.0);
                let wet = ch_wet.process(x, amount, amount);
                if i > sr as usize / 4 {
                    sum_sq_dry += (dry as f64).powi(2);
                    sum_sq_wet += (wet as f64).powi(2);
                }
            }
            let db = 10.0 * (sum_sq_wet / sum_sq_dry).log10();
            // 2026-08-03: 新写像では「減衰しない」だけでなく「必ず増える」が要件。
            assert!(
                db > -0.5,
                "amount={amount}: 通常演奏レベルで減衰 ({db:.2} dB) — 「上げると削れる」再発"
            );
        }
    }

    /// `BASS_ENV_TYPICAL` が `ENV_TYPICAL` に対し正しく +6dB (約2倍) に
    /// なっていること、かつ `env=BASS_ENV_TYPICAL` で `power_law_gain` の
    /// 固定点性質 (`g=1`) が Bass 較正点でも成り立つことを確認
    /// (2026-07-19: `bass_threshold` ノブを複数曲検証の末 +6dB固定に
    /// 変更・削除した経緯の回帰テスト)。
    #[test]
    fn bass_env_typical_is_6db_above_env_typical() {
        let ratio_db = 20.0 * (BASS_ENV_TYPICAL / ENV_TYPICAL).log10();
        assert!((ratio_db - 6.0).abs() < 0.2, "BASS_ENV_TYPICAL offset={ratio_db:.2}dB, expect ~6dB");

        for &alpha in &[0.1f32, 0.5, 0.9] {
            let g = power_law_gain(BASS_ENV_TYPICAL, alpha, BASS_ENV_TYPICAL);
            let expect = static_makeup(alpha);
            assert!((g - expect).abs() < 1e-3, "alpha={alpha} g={g} expect={expect}");
        }
    }

    #[test]
    fn hostile_input_stays_finite() {
        let mut ch = NiceBulkChannel::new(48000.0);
        for &x in &[1e9_f32, -1e9, f32::INFINITY, f32::NAN, 100.0, -100.0, 0.0] {
            let y = ch.process(x, 0.9, 0.9);
            assert!(y.is_finite(), "output non-finite for x={x}: y={y}");
        }
    }

    #[test]
    fn long_silence_after_signal_stays_finite_and_zero() {
        let sr = 48000.0;
        let mut ch = NiceBulkChannel::new(sr);
        for n in 0..48000 {
            let x = 0.5 * (2.0 * std::f32::consts::PI * 40.0 * (n as f32) / sr).sin();
            let _ = ch.process(x, 0.9, 0.9);
        }
        for n in 0..48000 * 5 {
            let y = ch.process(0.0, 0.9, 0.9);
            assert!(y.is_finite(), "non-finite during silence at n={n}: y={y}");
            if n > 48000 * 3 {
                assert!(y.abs() < 1e-15, "silence residue too large at n={n}: y={y}");
            }
        }
    }

    /// ブースト側 (`env < env_typical`) は clamp 項がそのまま乗る。
    #[test]
    fn boost_side_scales_clamp_by_makeup() {
        let alpha = 0.8f32;
        for &env in &[0.01f32, 0.05, 0.1, 0.2] {
            assert!(env < ENV_TYPICAL);
            let g = g_clamped(env, alpha, ENV_TYPICAL);
            let g_final = power_law_gain(env, alpha, ENV_TYPICAL);
            assert!(g > 1.0, "expected boost side: env={env} g={g}");
            let expected = static_makeup(alpha) * g;
            assert!(
                (g_final - expected).abs() < 1e-5,
                "env={env}: g_final={g_final} expected={expected}"
            );
        }
    }

    /// 減衰キックのアタックが dry より削られないこと。
    ///
    /// ピーク/テイル比で測ってはいけない: 小レベル成分を持ち上げるのが本
    /// プラグインの目的であり、テイルが上がれば比は必然的に縮む。要件は
    /// 「トランジェントの絶対値が削られない」= wet ピーク >= dry ピーク。
    /// 旧実装 (bass attack 5ms + カット側あり) ではここが下がっていた。
    #[test]
    fn transient_not_compressed() {
        let sr = 48000.0;
        let kick = |n: usize| -> f32 {
            let t = n as f32 / sr;
            let tp = t % 0.5;
            0.9 * (-tp / 0.09).exp() * (2.0 * std::f32::consts::PI * 55.0 * tp).sin()
        };
        let mut ch_dry = NiceBulkChannel::new(sr);
        let mut ch_wet = NiceBulkChannel::new(sr);
        let (mut pk_dry, mut pk_wet) = (0.0f32, 0.0f32);
        for n in 0..(sr as usize) {
            let x = kick(n);
            let dry = ch_dry.process(x, 0.0, 0.0);
            let wet = ch_wet.process(x, 0.75, 0.0);
            let t = n as f32 / sr;
            if t >= 0.5 && t % 0.5 < 0.02 {
                pk_dry = pk_dry.max(dry.abs());
                pk_wet = pk_wet.max(wet.abs());
            }
        }
        let db = 20.0 * (pk_wet / pk_dry).log10();
        assert!(db > 3.0, "transient not preserved/boosted: {db:.2}dB (dry peak {pk_dry:.3})");
    }

    #[test]
    fn saturator_is_bounded() {
        for &alpha in &[0.0f32, 0.5, 1.0] {
            let l = band_sat_ceiling(alpha);
            let mut sat = AdaaSaturator::new();
            for &x in &[0.0f32, 1e6, -1e6, 100.0, -100.0, f32::NAN, f32::INFINITY, 1.0, -1.0] {
                let y = sat.process(x, l);
                assert!(y.is_finite(), "non-finite for x={x}: y={y}");
                assert!(y.abs() <= l + 1e-6, "over ceiling for x={x} alpha={alpha}: y={y}");
            }
        }
    }

    #[test]
    fn band_sat_ceiling_is_base_at_zero() {
        assert_eq!(band_sat_ceiling(0.0), SAT_CEILING);
    }

    #[test]
    fn band_sat_ceiling_monotonic() {
        let mut prev = f32::NEG_INFINITY;
        for i in 0..=100 {
            let l = band_sat_ceiling(i as f32 / 100.0);
            assert!(l > prev, "not monotonic at alpha={}", i as f32 / 100.0);
            prev = l;
        }
        assert!((band_sat_ceiling(1.0) - SAT_CEILING * G0_MAX).abs() < 1e-6);
    }

    /// §2.3 の恒等式: 静的メイクアップはサチュレーション深度を増やさない。
    /// この 1 本が落ちたら設計が崩れている。
    #[test]
    fn sat_relative_drive_invariant_to_makeup() {
        for &alpha in &[0.0f32, 0.1, 0.37, 0.5, 0.75, 1.0] {
            let g0 = static_makeup(alpha);
            let l = band_sat_ceiling(alpha);
            for &x in &[0.0f32, 0.01, 0.1, 0.5, 1.0, 2.0, 4.0, -3.0] {
                let scaled = sat_f(g0 * x, l) / g0;
                let base = sat_f(x, SAT_CEILING);
                assert!(
                    (scaled - base).abs() < 1e-6,
                    "drive not invariant (alpha={alpha} x={x}): {scaled} vs {base}"
                );
            }
        }
    }

    /// 合算出力が §5-4 の理論上限を超えない。
    #[test]
    fn channel_output_within_theoretical_bound() {
        let bound = SAT_CEILING * G0_MAX + 4.0 + SAT_CEILING * G0_MAX;
        let sr = 48000.0;
        let mut ch = NiceBulkChannel::new(sr);
        for i in 0..48000 {
            let t = i as f32 / sr;
            let x = 3.9 * (2.0 * std::f32::consts::PI * 60.0 * t).sin()
                + 0.1 * (2.0 * std::f32::consts::PI * 9000.0 * t).sin();
            let y = ch.process(x, 1.0, 1.0);
            assert!(y.is_finite() && y.abs() <= bound, "out of bound: {y}");
        }
    }

    /// §5-2: mid のゲインは低域振幅に依存しない (帯域間結合ゼロ)。
    #[test]
    fn mid_is_independent_of_bass_level() {
        let sr = 48000.0;
        let n = 24000;
        let run = |low_amp: f32| {
            let mut ch = NiceBulkChannel::new(sr);
            let mut out = Vec::with_capacity(n);
            for i in 0..n {
                let t = i as f32 / sr;
                let x = low_amp * (2.0 * std::f32::consts::PI * 50.0 * t).sin()
                    + 0.3 * (2.0 * std::f32::consts::PI * 1000.0 * t).sin();
                out.push(ch.process(x, 1.0, 0.0));
            }
            out
        };
        let (a, b) = (run(0.2), run(3.0));
        // 1kHz 成分を Hann 窓付き直接 DFT で取り出して比較する。窓長 0.5s は
        // 50Hz/1kHz の両方で整数周期。窓なしだと低域のスペクトル漏れが
        // 1kHz ビンを汚し、帯域間結合と区別できない。サチュレータは奇関数
        // なので 50Hz の偶数次 (20 次 = 1kHz) は生じない。
        let bin = |y: &[f32]| -> f64 {
            let (mut re, mut im) = (0.0f64, 0.0f64);
            for (i, &v) in y.iter().enumerate() {
                let w = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n as f64).cos();
                let ph = 2.0 * std::f64::consts::PI * 1000.0 * i as f64 / sr as f64;
                re += v as f64 * w * ph.cos();
                im -= v as f64 * w * ph.sin();
            }
            (re * re + im * im).sqrt() / n as f64
        };
        let (ma, mb) = (bin(&a), bin(&b));
        let db = 20.0 * (mb / ma).log10();
        assert!(db.abs() < 0.05, "mid modulated by bass level: {db:.3}dB");
    }

    /// §2.3 の恒等式をフル経路で測る。較正点以上の env なら、
    /// bass ノブを上げてもサチュレーション深度は増えない。
    #[test]
    fn makeup_does_not_increase_saturation() {
        let sr = 48000.0;
        let n = 24000;
        let run = |amount: f32| {
            let mut ch = NiceBulkChannel::new(sr);
            let mut out = Vec::with_capacity(n);
            for i in 0..n {
                // 較正点 (BASS_ENV_TYPICAL=0.5) を十分に上回る定常低域。
                let x = 1.5 * (2.0 * std::f32::consts::PI * 60.0 * (i as f32) / sr).sin();
                out.push(ch.process(x, amount, 0.0));
            }
            out
        };
        // 較正点以上では out(a) = G0(a)·S + M (S = sat(low; SAT_CEILING)、
        // M = mid + high は a に依らない)。2 点から S と M を解き、第 3 点を
        // 予測できれば、メイクアップがサチュレーション深度を変えていない。
        // 出力から mid/high を分離せずに割り戻すと 60Hz のクロスオーバー漏れが
        // 混入するため、この線形分解で測る。
        let (a0, a1, a2) = (0.001f32, 1.0f32, 0.5f32);
        let (o0, o1, o2) = (run(a0), run(a1), run(a2));
        let (g0, g1, g2) = (static_makeup(a0), static_makeup(a1), static_makeup(a2));
        // 過渡を除いた後半で比較する。
        for i in n / 2..n {
            let s = (o1[i] - o0[i]) / (g1 - g0);
            let m = o0[i] - g0 * s;
            let predicted = g2 * s + m;
            assert!(
                (predicted - o2[i]).abs() < 1e-4,
                "saturation depth changed by makeup at i={i}: {predicted} vs {}",
                o2[i]
            );
        }
    }

    /// 通常レベルではサチュレータがほぼ透明。
    ///
    /// 瞬時比 `y/x` では測れない: ADAA は 2 点平均を含むためゼロクロス近傍で
    /// 比が発散する (これは非透明性でなく半サンプル遅延の帰結)。静特性
    /// `sat_f` の減衰量で判定する。
    #[test]
    fn saturator_transparent_at_low_level() {
        for &x in &[0.05f32, 0.1, 0.25, 0.5, 1.0] {
            let db = 20.0 * (sat_f(x, SAT_CEILING) / x).log10();
            assert!(db > -0.3, "not transparent at x={x}: {db:.3}dB");
        }
    }

    /// ADAA の数値精度回帰テスト (2026-08-03「常時パリパリ」バグ)。
    ///
    /// 不定積分を素朴な `L·√(L²+x²)` に戻すと小信号で桁落ちし、この SNR が
    /// 13.7dB まで落ちる (可聴のクラックル)。有理化形なら 90dB 前後。
    /// 参照は ADAA の Δ→0 極限である中点評価 `f((x+x₁)/2)`。
    #[test]
    fn saturator_no_catastrophic_cancellation() {
        let sr = 48000.0;
        // 天井は基準値と、帯域サチュレータの最大値 (SAT_CEILING*G0_MAX=8) の両方で
        // 測る。天井引数化で桁落ち耐性が失われないことを確認する。
        for &l in &[SAT_CEILING, SAT_CEILING * G0_MAX] {
            // 大振幅も含める: ブースト後の信号は 1〜4 のオーダーに達する。
            for &(amp, freq) in &[
                (0.05f32, 200.0f32),
                (0.2, 100.0),
                (0.5, 50.0),
                (1.0, 60.0),
                (3.0, 60.0),
            ] {
                let mut sat = AdaaSaturator::new();
                let (mut sig_sq, mut err_sq) = (0.0f64, 0.0f64);
                let mut max_err = 0.0f32;
                let mut prev = 0.0f32;
                for i in 0..20000 {
                    let x = amp * (2.0 * std::f32::consts::PI * freq * (i as f32) / sr).sin();
                    let y = sat.process(x, l);
                    let reference = sat_f(0.5 * (x + prev), l);
                    prev = x;
                    sig_sq += (reference as f64).powi(2);
                    err_sq += ((y - reference) as f64).powi(2);
                    max_err = max_err.max((y - reference).abs());
                }
                let snr = 10.0 * (sig_sq / err_sq).log10();
                assert!(
                    snr > 60.0,
                    "ADAA numerical noise (amp={amp} freq={freq}): SNR={snr:.1}dB"
                );
                // RMS 平均だけでは稀な誤差スパイクが埋もれる (実際に f32 版の
                // プチノイズを見逃した)。最大誤差でも判定する。
                assert!(
                    max_err < amp * 1e-4,
                    "ADAA error spike (amp={amp} freq={freq} l={l}): max_err={max_err:.3e}"
                );
            }
        }
    }

    /// プチノイズ回帰テスト (2026-08-03 第 2 報)。
    ///
    /// 定常正弦はゲインが一定なので、出力の不連続はサチュレータ経路にしか
    /// 由来しない。ADAA の差分商を f32 で計算していた版では、正弦のピーク付近
    /// (Δ→0) の桁落ちで 2 階差分が dry の 6.5 倍に跳ね、散発的なプチノイズと
    /// して可聴だった。f64 化後は 0.6 倍 (サチュにより dry より滑らか)。
    #[test]
    fn no_click_on_steady_tone() {
        let sr = 48000.0;
        let n = 48000;
        let mut wet = NiceBulkChannel::new(sr);
        let mut dry = NiceBulkChannel::new(sr);
        let (mut yw, mut yd) = (Vec::with_capacity(n), Vec::with_capacity(n));
        for i in 0..n {
            let x = 0.5 * (2.0 * std::f32::consts::PI * 60.0 * (i as f32) / sr).sin();
            yw.push(wet.process(x, 0.75, 0.0));
            yd.push(dry.process(x, 0.0, 0.0));
        }
        // 2 階差分の最大値 = クリックらしさ。
        let jerk = |v: &[f32]| {
            (2..v.len()).fold(0.0f32, |m, i| m.max((v[i] - 2.0 * v[i - 1] + v[i - 2]).abs()))
        };
        let (jw, jd) = (jerk(&yw), jerk(&yd));
        assert!(
            jw < jd * 2.0,
            "click detected on steady tone: wet_jerk={jw:.6} dry_jerk={jd:.6} ({:.1}x)",
            jw / jd
        );
    }

    /// ADAA がナイキスト折返し成分を naive 適用より抑えること。
    ///
    /// 閾値 3dB の根拠: 奇関数なので f0=7kHz では 5/7/9 次のみが折返す。
    /// ADAA 1 次の抑制はナイキストから遠い低次ほど弱いため、低次だけを測る
    /// 本条件は最も厳しい側にあたる (高次まで含めた総電力比では約 10dB)。
    /// 実測 4.2dB。
    #[test]
    fn saturator_adaa_reduces_alias() {
        let sr = 48000.0;
        let n = 1usize << 13;
        let f0 = 7000.0f32;
        // SAT_CEILING=4.0 を十分に飽和させる振幅。
        let amp = 12.0f32;
        let mut sat = AdaaSaturator::new();
        let mut y_adaa = Vec::with_capacity(n);
        let mut y_naive = Vec::with_capacity(n);
        for i in 0..n {
            let x = amp * (2.0 * std::f32::consts::PI * f0 * (i as f32) / sr).sin();
            y_adaa.push(sat.process(x, SAT_CEILING));
            y_naive.push(sat_f(x, SAT_CEILING));
        }
        // 折返した高調波 (|k·f0 - m·sr|) の電力を Hann 窓付き DFT で直接測る。
        // 窓なしだと基本波のスペクトル漏れが支配して差が潰れる。
        let alias_power = |y: &[f32]| -> f64 {
            let mut p = 0.0f64;
            for k in 3..=9usize {
                let f = k as f32 * f0;
                let folded = (f - (f / sr).round() * sr).abs();
                if !(20.0..sr / 2.0 - 20.0).contains(&folded) {
                    continue;
                }
                let (mut re, mut im) = (0.0f64, 0.0f64);
                for (i, &v) in y.iter().enumerate() {
                    let w = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n as f64).cos();
                    let ph = 2.0 * std::f64::consts::PI * folded as f64 * i as f64 / sr as f64;
                    re += v as f64 * w * ph.cos();
                    im -= v as f64 * w * ph.sin();
                }
                p += re * re + im * im;
            }
            p
        };
        let p_adaa = alias_power(&y_adaa);
        let p_naive = alias_power(&y_naive);
        let improve_db = 10.0 * (p_naive / p_adaa).log10();
        assert!(improve_db > 3.0, "ADAA alias reduction too small: {improve_db:.2}dB");
    }

    /// GUI が描く静的伝達特性が、実処理の定常応答と一致すること
    /// (`plans/gui_overhaul_v01.md` §Step 5「一致確認は目視ではなく数値で行う」)。
    ///
    /// 帯域内の正弦波を十分長く通し、定常状態のピークを `static_transfer` の
    /// 予測値と突き合わせる。
    ///
    /// **照合は bass 帯域のみで行う。** `EnvFollower` は `|x|` へ一次追従するため、
    /// 正弦波では env が振幅へ収束せず、周期が短いほど `|x|` の平均側 (2/π·amp)
    /// へ寄る。低い周波数では release が追いつかず env ≈ 振幅になるが、8kHz では
    /// 実測で −1.9 dB ずれる。これは曲線の誤りではなく、静的伝達特性が
    /// **env に対する特性**であって正弦波振幅に対する特性ではないことによる
    /// (GUI の軸ラベルもそれに合わせてある)。周波数ごとの補正を曲線へ入れると
    /// 素材依存の仮定を持ち込むことになるため、やらない。
    #[test]
    fn static_transfer_matches_the_running_dsp() {
        const SR: f32 = 48_000.0;
        for (freq, alpha, env_typical, is_bass) in [
            (60.0_f32, 0.8_f32, BASS_ENV_TYPICAL, true),
            (60.0, 0.4, BASS_ENV_TYPICAL, true),
        ] {
            for amp in [0.05_f32, 0.15, 0.35, 0.7] {
                let mut ch = NiceBulkChannel::new(SR);
                let (bass_amount, hi_amount) =
                    if is_bass { (alpha, 0.0) } else { (0.0, alpha) };
                // 包絡と ADAA 状態を定常へ持っていく。
                let total = (SR * 0.6) as usize;
                let mut peak = 0.0_f32;
                for n in 0..total {
                    let x = amp
                        * (2.0 * std::f32::consts::PI * freq * n as f32 / SR).sin();
                    let y = ch.process(x, bass_amount, hi_amount);
                    if n > (SR * 0.5) as usize {
                        peak = peak.max(y.abs());
                    }
                }
                let predicted = static_transfer(amp, alpha, env_typical);
                let error_db = 20.0 * (peak / predicted).log10();
                assert!(
                    error_db.abs() < 1.5,
                    "freq={freq} alpha={alpha} amp={amp}: 実測 {peak:.4} / 予測 {predicted:.4} = {error_db:+.2} dB"
                );
            }
        }
    }

    /// 恒等点。ノブ 0 では伝達特性が入力そのもの（サチュレータの天井のみ）。
    #[test]
    fn static_transfer_is_identity_at_zero() {
        for amp in [0.01_f32, 0.1, 0.5, 1.0] {
            let y = static_transfer(amp, 0.0, ENV_TYPICAL);
            // alpha=0 では g=1 なので sat(a, SAT_CEILING) そのもの。
            assert!((y - sat_f(amp, SAT_CEILING)).abs() < 1.0e-6, "amp={amp}");
        }
    }

    /// 較正点では `g = G0(alpha)` であり **1 ではない**（静的メイクアップの分だけ上がる）。
    /// `gain-formula-fixed-point` の罠: 固定点がどこかを式の構造から確認しておく。
    #[test]
    fn the_calibration_point_carries_the_static_makeup() {
        for alpha in [0.25_f32, 0.5, 1.0] {
            let g = power_law_gain(ENV_TYPICAL, alpha, ENV_TYPICAL);
            assert!(
                (g - static_makeup(alpha)).abs() < 1.0e-5,
                "alpha={alpha}: 較正点のゲイン {g} が G0={} と一致しない",
                static_makeup(alpha)
            );
            // g_clamped 単体では較正点が厳密に 1 の固定点であること。
            assert!((g_clamped(ENV_TYPICAL, alpha, ENV_TYPICAL) - 1.0).abs() < 1.0e-5);
        }
    }
    // ================= 2026-08-30 クロスレビュー由来の実測 =================
    // `plans/nice_bulk_crossreview_v01.md` の未決着 1・2 を機械判定へ移す。

    /// 可変分割点のチャンネル。`NiceBulkChannel` の構造を分割周波数だけ可変にした写し。
    /// **`mirror_matches_real_channel` で本物と bit 一致を確認してから使う。**
    struct VarSplitChannel {
        stage_bass: fandango_shared::Lr4Crossover,
        stage_hi: fandango_shared::Lr4Crossover,
        bass_gain: PowerLawGain,
        hi_gain: PowerLawGain,
        sat_bass: AdaaSaturator,
        sat_hi: AdaaSaturator,
    }

    impl VarSplitChannel {
        fn new(sr: f32, f_low: f32, f_high: f32) -> Self {
            Self {
                stage_bass: fandango_shared::Lr4Crossover::new(sr, f_low),
                stage_hi: fandango_shared::Lr4Crossover::new(sr, f_high),
                bass_gain: PowerLawGain::new(sr, 30.0, 150.0),
                hi_gain: PowerLawGain::new(sr, 1.0, 30.0),
                sat_bass: AdaaSaturator::new(),
                sat_hi: AdaaSaturator::new(),
            }
        }

        fn process(&mut self, x: f32, bass_amount: f32, hi_amount: f32) -> f32 {
            let x = if x.is_finite() { x.clamp(-4.0, 4.0) } else { 0.0 };
            let (low, rest) = self.stage_bass.split(x);
            let (mid, high) = self.stage_hi.split(rest);
            let low_out = self.bass_gain.process(low, bass_amount, BASS_ENV_TYPICAL);
            let high_out = self.hi_gain.process(high, hi_amount, ENV_TYPICAL);
            let low_sat = if bass_amount <= 1e-4 {
                self.sat_bass.prime(low_out);
                low_out
            } else {
                self.sat_bass.process(low_out, band_sat_ceiling(bass_amount))
            };
            let high_sat = if hi_amount <= 1e-4 {
                self.sat_hi.prime(high_out);
                high_out
            } else {
                self.sat_hi.process(high_out, band_sat_ceiling(hi_amount))
            };
            let out = low_sat + mid + high_sat;
            if out.is_finite() { denormal_gate(out) } else { 0.0 }
        }
    }

    /// 決定論的な広帯域テスト信号（合成のみ。外部音源を使わない）。
    /// 低域質量・中域基音・高域成分を同時に持たせる。
    fn broadband(n: usize, sr: f32) -> Vec<f32> {
        let mut st = 0x2545F4914F6CDD1Du64;
        let mut rng = move || {
            st ^= st << 13;
            st ^= st >> 7;
            st ^= st << 17;
            ((st >> 40) as f32 / 8_388_608.0) - 1.0
        };
        (0..n)
            .map(|i| {
                let t = i as f32 / sr;
                let tp = std::f32::consts::TAU;
                0.30 * (tp * 45.0 * t).sin()      // サブ〜キック胴鳴り
                    + 0.25 * (tp * 110.0 * t).sin()  // ベース基音
                    + 0.20 * (tp * 220.0 * t).sin()  // ボーカル/スネア胴の基音域
                    + 0.12 * (tp * 1500.0 * t).sin() // 中域
                    + 0.10 * (tp * 6000.0 * t).sin() // presence 帯
                    + 0.05 * rng()
            })
            .collect()
    }

    fn rms(x: &[f32]) -> f64 {
        (x.iter().map(|&v| (v as f64) * (v as f64)).sum::<f64>() / x.len() as f64).sqrt()
    }

    /// 写しが本物と一致すること。これが落ちたら以降の実測は無効。
    #[test]
    fn mirror_matches_real_channel() {
        let sr = 48_000.0;
        let x = broadband(4800, sr);
        for &(b, h) in &[(0.0f32, 0.0f32), (0.5, 0.0), (0.0, 0.5), (1.0, 1.0), (0.37, 0.73)] {
            let mut real = NiceBulkChannel::new(sr);
            let mut mirror = VarSplitChannel::new(sr, SPLIT_BASS_HZ, SPLIT_HI_HZ);
            for &v in &x {
                let a = real.process(v, b, h);
                let c = mirror.process(v, b, h);
                assert_eq!(a, c, "mirror mismatch at bass={b} hi={h}");
            }
        }
    }

    /// **分割点への感度**（2026-08-30。当初「区間 [100,200] 内の鈍感性」を測る
    /// テストだったが、その区間に根拠が無いことが判明したため書き直した）。
    ///
    /// [100, 200] は Fable の第2便が初めて口にした区間で、プロジェクト内に前例が無く、
    /// 挙げられた理由（下限=キック胴鳴りを収める / 上限=ボーカル基音を外す）は
    /// 両端とも自らの理屈と逆向きだった（胴鳴り 60〜100 Hz は 100 で切ると外れ、
    /// 男声基音 85〜180 Hz は 200 で切ると入る）。
    /// **根拠の無い区間を検査に焼き込むと、根拠の無い値に検証済みの見た目を与える。**
    /// → `plans/nice_bulk_crossreview_v01.md`
    ///
    /// よってここでは区間を主張せず、**言える 2 つだけ**を表明する:
    /// (1) 分割点はラウドネスにほぼ影響しない、(2) スペクトルには大きく影響する。
    /// (2) が成り立つ以上、判定器なしに値を動かすのは純リスクである。
    #[test]
    fn bass_split_sensitivity() {
        let sr = 48_000.0;
        let x = broadband(48_000, sr);
        let run = |f: f32| -> Vec<f32> {
            let mut ch = VarSplitChannel::new(sr, f, 4000.0);
            x.iter().map(|&v| ch.process(v, 1.0, 0.0)).collect()
        };
        let base = run(SPLIT_BASS_HZ);
        let base_rms = rms(&base);

        let mut worst_lvl = 0.0f64;
        let mut worst_spec = f64::NEG_INFINITY;
        println!("
== bass 分割点の感度 (bass_amount=1.0, hi=0, 基準 150 Hz) ==");
        // 150 = 旧値（`:excite` 由来）。160 = 採用時に最後まで競った値。
        for &f in &[60.0f32, 100.0, 125.0, SPLIT_BASS_HZ, 150.0, 160.0, 200.0, 300.0, 400.0] {
            let y = run(f);
            let d: Vec<f32> = y.iter().zip(&base).map(|(a, b)| a - b).collect();
            let spec = 20.0 * (rms(&d) / base_rms).max(1e-12).log10();
            let lvl = 20.0 * (rms(&y) / base_rms).max(1e-12).log10();
            println!("  f = {f:6.1} Hz : スペクトル差 {spec:7.2} dB(rel),  ラウドネス差 {lvl:+6.3} dB");
            if f != SPLIT_BASS_HZ {
                worst_lvl = worst_lvl.max(lvl.abs());
                worst_spec = worst_spec.max(spec);
            }
        }
        println!("  最悪ラウドネス差   : {worst_lvl:.3} dB  (60〜400 Hz 全域)");
        println!("  最悪スペクトル差   : {worst_spec:.2} dB(rel)");

        // (1) ラウドネスへの鈍感性。60〜400 Hz のどこへ動かしても 0.5 dB 以内。
        assert!(
            worst_lvl < 0.5,
            "ラウドネス差 {worst_lvl:.3} dB が 0.5 dB 以上。鈍感性が崩れている"
        );
        // (2) スペクトルは大きく変わる。「動かしても同じ」ではないことの表明。
        assert!(
            worst_spec > -12.0,
            "スペクトル差の最大が {worst_spec:.2} dB(rel) しかない。             分割点が音に効かないなら本テストの前提が崩れている"
        );
    }

    /// **未決着 2**: 時定数の非対称は「自帯域の周期に対する比」で説明できるか。
    ///
    /// 各帯域の代表周波数の純音を入れ、エンベロープ追従器のリップル深さを測る。
    /// リップルはゲイン変調 = 変調歪みの源。比が大きいほど保守的。
    #[test]
    fn envelope_ripple_is_smaller_for_hi_band() {
        let sr = 48_000.0;
        let ripple = |f: f32, atk: f32, rel: f32| -> f64 {
            let mut env = EnvFollower::new(sr, atk, rel);
            let n = (sr * 0.5) as usize;
            let mut vals = Vec::with_capacity(n / 2);
            for i in 0..n {
                let t = i as f32 / sr;
                let e = env.process((std::f32::consts::TAU * f * t).sin());
                if i >= n / 2 {
                    vals.push(e as f64);
                }
            }
            let lo = vals.iter().cloned().fold(f64::INFINITY, f64::min);
            let hi = vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            20.0 * (hi.max(1e-12) / lo.max(1e-12)).log10()
        };

        println!("\n== エンベロープ リップル深さ (純音・定常部) ==");
        println!("  帯域   信号周波数   時定数            周期比(atk)   リップル");
        let cases = [
            ("bass", 20.0f32, 30.0f32, 150.0f32),
            ("bass", 40.0, 30.0, 150.0),
            ("bass", 100.0, 30.0, 150.0),
            ("hi", 4000.0, 1.0, 30.0),
            ("hi", 8000.0, 1.0, 30.0),
            ("hi", 16000.0, 1.0, 30.0),
        ];
        let mut worst_bass = 0.0f64;
        let mut worst_hi = 0.0f64;
        for &(band, f, atk, rel) in &cases {
            let r = ripple(f, atk, rel);
            let ratio = atk as f64 / (1000.0 / f as f64);
            println!("  {band:5}  {f:8.0} Hz  {atk:5.1}/{rel:5.1} ms  {ratio:9.2}      {r:6.2} dB");
            if band == "bass" {
                worst_bass = worst_bass.max(r);
            } else {
                worst_hi = worst_hi.max(r);
            }
        }
        println!("  bass 側 最悪リップル : {worst_bass:.2} dB");
        println!("  hi   側 最悪リップル : {worst_hi:.2} dB");
        assert!(
            worst_hi < worst_bass,
            "hi のリップル {worst_hi:.2} dB が bass {worst_bass:.2} dB 以上。\
             「自帯域に対して hi のほうが保守的」が成立しない"
        );
    }

    /// **未決着 2 の続き**: リップルが実出力の変調歪みとして現れる量。
    /// 各帯域の代表純音を通し、基音以外へ漏れた成分の比を測る。
    #[test]
    fn band_modulation_distortion_hi_not_worse_than_bass() {
        let sr = 48_000.0;
        let n = 48_000usize;
        let goertzel = |x: &[f32], f: f64| -> f64 {
            let w = std::f64::consts::TAU * f / sr as f64;
            let c = 2.0 * w.cos();
            let (mut s1, mut s2) = (0.0f64, 0.0f64);
            for &v in x {
                let s0 = v as f64 + c * s1 - s2;
                s2 = s1;
                s1 = s0;
            }
            2.0 * (s1 * s1 + s2 * s2 - c * s1 * s2).max(0.0).sqrt() / x.len() as f64
        };
        let measure = |f: f32, bass: f32, hi: f32| -> f64 {
            let mut ch = NiceBulkChannel::new(sr);
            let y: Vec<f32> = (0..n)
                .map(|i| {
                    let t = i as f32 / sr;
                    ch.process(0.3 * (std::f32::consts::TAU * f * t).sin(), bass, hi)
                })
                .collect();
            let tail = &y[n / 2..];
            let fund = goertzel(tail, f as f64);
            let total = rms(tail);
            let resid = (total * total - fund * fund / 2.0).max(0.0).sqrt();
            20.0 * (resid / fund.max(1e-12)).max(1e-12).log10()
        };
        println!("\n== 帯域別 変調歪み (基音以外 / 基音, ノブ最大) ==");
        let b40 = measure(40.0, 1.0, 0.0);
        let b100 = measure(100.0, 1.0, 0.0);
        let h6k = measure(6000.0, 0.0, 1.0);
        let h12k = measure(12000.0, 0.0, 1.0);
        println!("  bass  40 Hz (bass=1.0) : {b40:7.2} dB");
        println!("  bass 100 Hz (bass=1.0) : {b100:7.2} dB");
        println!("  hi    6 kHz (hi=1.0)   : {h6k:7.2} dB");
        println!("  hi   12 kHz (hi=1.0)   : {h12k:7.2} dB");
        let worst_bass = b40.max(b100);
        let worst_hi = h6k.max(h12k);
        println!("  bass 最悪 {worst_bass:.2} dB / hi 最悪 {worst_hi:.2} dB");
        assert!(
            worst_hi < worst_bass,
            "hi の変調歪み {worst_hi:.2} dB が bass {worst_bass:.2} dB 以上"
        );
    }

}
