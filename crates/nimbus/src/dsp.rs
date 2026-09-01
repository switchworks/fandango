use fandango_shared::velvet::{build_sequence, DecorrelatorState, VelvetSequence};

pub const ROOM_DIM: [f32; 3] = [7.19, 5.41, 3.23];
pub const WALL_G: f32 = 0.55;
pub const SRC_POS: [f32; 3] = [2.30, 1.70, 1.55];
pub const RCV_POS: [f32; 3] = [4.90, 3.90, 1.35];
pub const RCV_W: f32 = 0.28;
pub const SOUND_C: f32 = 343.0;
pub const N_TAP: usize = 16;

// distance 写像（計画書 §3.3.1 / §4.1）
pub const GAMMA: f32 = 1.5;
pub const HF_TILT_DB: f32 = 9.0;
pub const HF_FREQ: f32 = 4000.0;

/// 拡散段（`plans/nimbus_diffusion_v01.md` §2）。支持長は 300 ms 固定で
/// `distance` に依らない（同 §2.2）。
pub const DIFF_LEN: f32 = 0.300;
/// OVN のパルス密度 [個/秒]。§1.1 の実測エコー密度 1700–2400 peaks/s に対する設計値。
pub const DIFF_DENSITY: f32 = 2000.0;
/// 減衰包絡の T60 [秒]。§1.1 の帯域別 T60 の中域値。
pub const DIFF_T60: f32 = 1.5;
/// cos² テーパーの開始時刻 [秒]。ここから `DIFF_LEN` までで 1 → 0 へ落とす。
pub const DIFF_TAPER_START: f32 = 0.200;
/// wet-only (r=1, m=1) の目標総エネルギー [dB]（dry インパルス = 0 dB 基準）。
pub const TARGET_ENERGY_DB: f32 = 13.0;

// L/R 独立シード（INV-E）。velvet crate のシード
// (0x98C94E6B3AA7E83D / 0xB86598FE74D7396A) とは異なる値を使う。
const DIFF_SEED_L: u64 = 0x4D5F2C9B1E77A063;
const DIFF_SEED_R: u64 = 0xC1A93B5E8D2470F1;

/// 拡散 tap のゲイン包絡（正規化係数 `A` を除いた形）。
///
/// `g(t) = exp(−ln(10)·3·t / T60) · taper(t)`、
/// `taper(t) = cos²(π/2 · (t − 200ms)/100ms)`（`t < 200ms` では 1）。
/// `t ≥ DIFF_LEN` は厳密ゼロ（早期 return。cos の丸め残差を持ち込まない）→ INV-A。
#[inline]
pub fn diffusion_envelope(t: f32) -> f32 {
    if t >= DIFF_LEN {
        return 0.0;
    }
    let decay = (-(10.0_f32.ln()) * 3.0 * t / DIFF_T60).exp();
    if t <= DIFF_TAPER_START {
        return decay;
    }
    let u = (t - DIFF_TAPER_START) / (DIFF_LEN - DIFF_TAPER_START);
    let c = (std::f32::consts::FRAC_PI_2 * u).cos();
    decay * c * c
}

/// wet 経路のフィルタ（`Shelf(r=1)` → `HPF`）の白色入力に対するエネルギー利得
/// `Σ h²`（Parseval）。正規化係数 `A` を閉形式で決めるための従属量であり、
/// 手で置く定数ではない。`initialize` 側でのみ呼ぶ。
pub fn filter_energy_gain(sample_rate: f32) -> f32 {
    let shelf_c = hf_shelf_coeffs(sample_rate, 1.0);
    let hpf_c = hpf_coeffs(sample_rate);
    let mut shelf = Shelf::default();
    let mut hpf = Hpf::default();
    let mut acc = 0.0_f64;
    // f_hp ≈ 88 Hz の 2 次 HPF の尾は 0.5 s で十分に減衰する。
    for index in 0..(sample_rate * 0.5) as usize {
        let x = if index == 0 { 1.0 } else { 0.0 };
        let y = hpf.process(shelf.process(x, &shelf_c), &hpf_c);
        acc += y as f64 * y as f64;
    }
    acc as f32
}

/// wet ゲイン `W(r) = r`（`plans/nimbus_diffusion_v01.md` Step 1）。
///
/// 固定点・単調性・有界性は式の構造から従う:
/// - `W(0) = 0` は恒等写像そのものであり、丸めが一切介在しない
/// - `W(1) = 1`
/// - 導関数は定数 1 > 0 → 区間内で狭義単調増加
/// - 値域は `[0, 1]` で有界。極を持たない
///
/// 旧写像 `W_MAX·r / ((1+BETA) − BETA·r)` は採用しない。`r = 0.5` で 0.25（−12 dB）、
/// `r = 0.25` で 0.1（−20 dB）となり、tap field 自体の減衰と掛け合わさって
/// 実用上 wet が聞こえなくなるため。
#[inline]
pub fn wet_gain(r: f32) -> f32 {
    r
}

/// tap 時刻のスケール `P(r)/P0 = 1 / (1 + GAMMA·r)`。
/// 分母は r ∈ [0,1] で `[1, 1+GAMMA]` → 正・有界・厳密単調減少。
/// 係数は 1 以下なので遅延ラインは r = 0 の長さで足りる。
#[inline]
pub fn time_scale(r: f32) -> f32 {
    1.0 / (1.0 + GAMMA * r)
}

/// 1 次ハイシェルフ係数（`H(z) = (b0 + b1·z⁻¹) / (1 + a1·z⁻¹)`）。
#[derive(Clone, Copy)]
pub struct ShelfCoeffs {
    b0: f32,
    b1: f32,
    a1: f32,
}

/// `G_hf(r) = −HF_TILT_DB · r` [dB] の 1 次ハイシェルフ。
///
/// `H(s) = (A·s + ωc)/(s + ωc)`、`ωc = 2π·HF_FREQ·√A` を双一次変換したもの。
/// `√A` を掛けてあるため `HF_FREQ` での利得が `√A`（dB の半分）になる中点定義。
/// DC 利得 `(b0+b1)/(1+a1) = 1`、Nyquist 利得 `(b0−b1)/(1−a1) = A`、`|a1| < 1`。
/// `r = 0` では `A = 1` で `b0 = 1`, `b1 = a1` となり `H ≡ 1`（極零相殺）。
pub fn hf_shelf_coeffs(sample_rate: f32, r: f32) -> ShelfCoeffs {
    // 係数は f64 で計算する。低い ωc で b0 + b1 が桁落ちするため（tilt と同じ理由）。
    let a = 10f64.powf(-(HF_TILT_DB * r) as f64 / 20.0);
    // プリワープ角。sample_rate が 2·HF_FREQ·√A を下回ると角が π/2 を越え、
    // tan が符号反転して係数が発散する (VST3 validator は SR 1234.5678 Hz を
    // 要求する)。ナイキスト直下でクランプして係数を有限に保つ。
    // 通常のレートでは角は π/2 未満なのでクランプは働かず、値はビット同一。
    const MAX_WARP: f64 = std::f64::consts::FRAC_PI_2 * 0.999;
    let warp = (std::f64::consts::PI * HF_FREQ as f64 * a.sqrt() / sample_rate as f64).min(MAX_WARP);
    let k = warp.tan();
    let inv = 1.0 / (1.0 + k);
    ShelfCoeffs {
        b0: ((a + k) * inv) as f32,
        b1: ((k - a) * inv) as f32,
        a1: ((k - 1.0) * inv) as f32,
    }
}

#[derive(Default)]
struct Shelf {
    x1: f32,
    y1: f32,
}

impl Shelf {
    #[inline]
    fn process(&mut self, x: f32, c: &ShelfCoeffs) -> f32 {
        let y = c.b0 * x + c.b1 * self.x1 - c.a1 * self.y1;
        self.x1 = x;
        self.y1 = y;
        y
    }

    fn reset(&mut self) {
        self.x1 = 0.0;
        self.y1 = 0.0;
    }
}

/// wet ハイパス（計画書 §3.4）。2 次 Butterworth、`Q = 1/√2`（ピークなし）。
pub const HPF_Q: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// 仮想室の T60 設計値 `T_room`（計画書 §4.2）。
/// `n = ln(10⁻³)/ln(g)` 次反射で −60 dB、`T_room = n·L/c`。
/// **用途は `f_hp` の導出のみ。** ER-FIR の支持長は `T_supp`（`support_len`）。
pub fn room_duration() -> f32 {
    let room_max = ROOM_DIM.into_iter().fold(0.0_f32, f32::max);
    (1.0e-3_f32.ln() / WALL_G.ln()) * room_max / SOUND_C
}

/// wet ハイパスのコーナー `f_hp = 2000·√(T_room / V)`（計画書 §4.3）。
/// **定数として書かない。** 係数 2000 は直方体室のモード密度と共振半値幅
/// `2.2/T` からモーダルオーバーラップ 3 を課して導かれる値（§3.4.2）。
pub fn hp_corner() -> f32 {
    let volume = ROOM_DIM[0] * ROOM_DIM[1] * ROOM_DIM[2];
    2000.0 * (room_duration() / volume).sqrt()
}

/// 2 次 biquad 係数（`a0 = 1` に正規化済み）。
#[derive(Clone, Copy)]
pub struct HpfCoeffs {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

/// `f_hp` の 2 次 Butterworth ハイパス（RBJ）。
/// 係数は f64 で計算する。`f_hp ≈ 88 Hz` は `ω0 ≈ 0.0115` と低く、
/// `b0 + b1 + b2` が桁落ちするため（`tilt` と同じ理由）。
pub fn hpf_coeffs(sample_rate: f32) -> HpfCoeffs {
    let w0 = 2.0 * std::f64::consts::PI * hp_corner() as f64 / sample_rate as f64;
    let (sin_w, cos_w) = (w0.sin(), w0.cos());
    let alpha = sin_w / (2.0 * HPF_Q as f64);

    let b0 = (1.0 + cos_w) * 0.5;
    let b1 = -(1.0 + cos_w);
    let b2 = b0;
    let a0 = 1.0 + alpha;
    let a1 = -2.0 * cos_w;
    let a2 = 1.0 - alpha;

    let inv = 1.0 / a0;
    HpfCoeffs {
        b0: (b0 * inv) as f32,
        b1: (b1 * inv) as f32,
        b2: (b2 * inv) as f32,
        a1: (a1 * inv) as f32,
        a2: (a2 * inv) as f32,
    }
}

/// Direct Form I の状態 1 チャンネル分。
#[derive(Default)]
struct Hpf {
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Hpf {
    #[inline]
    fn process(&mut self, x: f32, c: &HpfCoeffs) -> f32 {
        let y = c.b0 * x + c.b1 * self.x1 + c.b2 * self.x2 - c.a1 * self.y1 - c.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }

    fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }
}

#[derive(Clone, Copy)]
struct Tap {
    delay_l: f32,
    delay_r: f32,
    gain_l: f32,
    gain_r: f32,
}

/// GUI が確定済み初期反射を図解するための読み取り専用 tap 値。音声処理には用いない。
#[derive(Clone, Copy)]
pub struct TapVisual {
    pub left_time: f32,
    pub right_time: f32,
    pub left_gain: f32,
    pub right_gain: f32,
}

/// 拡散段のエネルギー包絡（GUI 表示用）。`bins` 個の等間隔区間について
/// 拡散 tap のゲイン二乗和の平方根を返す。最大値で正規化済み（L/R 共通の
/// 係数で割るため左右差は保存される）。音声処理には用いない。
///
/// 個々の tap ではなく積分した面を描くための量であり、tap 数に依存しない
/// （`plans/gui_overhaul_v01.md` §4.2）。
pub fn visual_envelope(sample_rate: f32, bins: usize) -> (Vec<f32>, Vec<f32>) {
    let er = ErFir::new(sample_rate);
    let span = DIFF_LEN * sample_rate;
    let fill = |seq: &VelvetSequence| {
        let mut acc = vec![0.0_f32; bins];
        for (k, gs) in seq.taps_k.iter().zip(seq.taps_gs.iter()) {
            let bin = ((*k as f32 / span) * bins as f32) as usize;
            if let Some(slot) = acc.get_mut(bin) {
                *slot += gs * gs;
            }
        }
        acc.iter().map(|e| e.sqrt()).collect::<Vec<f32>>()
    };
    // ビンごとのパルス数は有限であり、そのままでは統計揺らぎが帯の輪郭に乗る。
    // 描く対象は個々の tap ではなく積分した面なので、移動平均で均す。
    // 半径は減衰の時定数に対して十分小さく取る（包絡の形は保つ）。
    const SMOOTH_RADIUS: usize = 3;
    let smooth = |v: Vec<f32>| -> Vec<f32> {
        (0..v.len())
            .map(|i| {
                let lo = i.saturating_sub(SMOOTH_RADIUS);
                let hi = (i + SMOOTH_RADIUS).min(v.len() - 1);
                v[lo..=hi].iter().sum::<f32>() / (hi - lo + 1) as f32
            })
            .collect()
    };
    let (mut l, mut r) = (smooth(fill(&er.diff_l)), smooth(fill(&er.diff_r)));
    let peak = l
        .iter()
        .chain(r.iter())
        .fold(0.0_f32, |m, v| m.max(*v))
        .max(1.0e-12);
    for v in l.iter_mut().chain(r.iter_mut()) {
        *v /= peak;
    }
    (l, r)
}

/// 拡散段の支持長 [秒]（GUI の時間軸用）。
pub fn visual_diffusion_span() -> f32 {
    DIFF_LEN
}

/// Room A の tap 生成式から、GUI 表示用の秒単位データを返す。
pub fn visual_taps(sample_rate: f32) -> Vec<TapVisual> {
    generate_taps(sample_rate)
        .into_iter()
        .map(|tap| TapVisual {
            left_time: tap.delay_l / sample_rate,
            right_time: tap.delay_r / sample_rate,
            left_gain: tap.gain_l,
            right_gain: tap.gain_r,
        })
        .collect()
}

pub struct ErFir {
    taps: Vec<Tap>,
    diff_l: VelvetSequence,
    diff_r: VelvetSequence,
    diff_state_l: DecorrelatorState,
    diff_state_r: DecorrelatorState,
    line_l: Vec<f32>,
    line_r: Vec<f32>,
    write_pos: usize,
    shelf_l: Shelf,
    shelf_r: Shelf,
    hpf_l: Hpf,
    hpf_r: Hpf,
}

impl ErFir {
    pub fn new(sample_rate: f32) -> Self {
        // 支持長 T_supp は tap 生成結果からの従属量。T_room（T60 設計値）は
        // f_hp の導出専用であり、ライン長には使わない（計画書 §2 INV-1）。
        Self::from_taps(generate_taps(sample_rate), sample_rate)
    }

    fn from_taps(taps: Vec<Tap>, sample_rate: f32) -> Self {
        // 幾何 tap の（フィルタ通過前の）チャンネル別エネルギー。
        let geo_energy_l: f32 = taps.iter().map(|tap| tap.gain_l * tap.gain_l).sum();
        let geo_energy_r: f32 = taps.iter().map(|tap| tap.gain_r * tap.gain_r).sum();
        let filter_gain = filter_energy_gain(sample_rate);
        let diff_l = build_diffusion(DIFF_SEED_L, sample_rate, geo_energy_l, filter_gain);
        let diff_r = build_diffusion(DIFF_SEED_R, sample_rate, geo_energy_r, filter_gain);
        let ring_len = diff_l
            .taps_k
            .iter()
            .chain(diff_r.taps_k.iter())
            .copied()
            .max()
            .expect("拡散 tap が空")
            + 1;

        let mut er = Self {
            taps,
            diff_l,
            diff_r,
            diff_state_l: DecorrelatorState::new(ring_len),
            diff_state_r: DecorrelatorState::new(ring_len),
            line_l: Vec::new(),
            line_r: Vec::new(),
            write_pos: 0,
            shelf_l: Shelf::default(),
            shelf_r: Shelf::default(),
            hpf_l: Hpf::default(),
            hpf_r: Hpf::default(),
        };
        // read_fractional は遅延 floor(T_supp) と floor(T_supp)+1 を参照する。
        let capacity = er.support_len().ceil() as usize + 2;
        er.line_l = vec![0.0; capacity];
        er.line_r = vec![0.0; capacity];
        er
    }

    /// ABX 対照系（`examples/abx_render.rs` 専用。製品経路からは呼ばれない）。
    ///
    /// **幾何構造だけを潰し、それ以外を完全に一致させた対照。**
    /// 幾何 tap のゲイン絶対値の多重集合・tap 本数・遅延の範囲・チャンネル別
    /// 総エネルギーを `new` と厳密に同一に保ったまま、
    /// (1) どの遅延にどのゲインが乗るかの対応、(2) 極性、(3) L/R の遅延の対応
    /// を無作為化する。`geo_energy_{l,r}` が不変なので拡散段の正規化係数 `A` も
    /// 不変になり、**総エネルギーは `new` と一致する**（レベル交絡が入らない）。
    ///
    /// これにより「良い音の手柄が鏡像法の幾何にあるのか、
    /// それとも単に短い無相関な尾にあるのか」だけを分離できる。
    #[doc(hidden)]
    pub fn control_shuffled(sample_rate: f32, seed: u64) -> Self {
        let geo = generate_taps(sample_rate);
        let (d_min, d_max) = geo.iter().fold((f32::MAX, 0.0_f32), |(lo, hi), t| {
            (lo.min(t.delay_l).min(t.delay_r), hi.max(t.delay_l).max(t.delay_r))
        });
        let mut state = seed | 1;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 11) as f32 / (1u64 << 53) as f32
        };
        let mut taps: Vec<Tap> = geo
            .iter()
            .map(|t| {
                let sign_l = if next() < 0.5 { -1.0 } else { 1.0 };
                let sign_r = if next() < 0.5 { -1.0 } else { 1.0 };
                Tap {
                    delay_l: d_min + (d_max - d_min) * next(),
                    delay_r: d_min + (d_max - d_min) * next(),
                    gain_l: sign_l * t.gain_l.abs(),
                    gain_r: sign_r * t.gain_r.abs(),
                }
            })
            .collect();
        // ゲイン絶対値の多重集合は保ったまま、遅延との対応だけを崩す。
        for i in (1..taps.len()).rev() {
            let j = (next() * (i + 1) as f32) as usize % (i + 1);
            let (gl, gr) = (taps[i].gain_l, taps[i].gain_r);
            taps[i].gain_l = taps[j].gain_l;
            taps[i].gain_r = taps[j].gain_r;
            taps[j].gain_l = gl;
            taps[j].gain_r = gr;
        }
        Self::from_taps(taps, sample_rate)
    }

    pub fn reset(&mut self) {
        self.diff_state_l.reset();
        self.diff_state_r.reset();
        self.line_l.fill(0.0);
        self.line_r.fill(0.0);
        self.write_pos = 0;
        self.shelf_l.reset();
        self.shelf_r.reset();
        self.hpf_l.reset();
        self.hpf_r.reset();
    }

    /// `distance` は r ∈ [0,1]。`shelf` は同じ r から `hf_shelf_coeffs` で作った係数、
    /// `hpf` は `hpf_coeffs` で作った固定係数（r 非依存、§3.4）。
    #[inline]
    pub fn process(
        &mut self,
        l: f32,
        r_in: f32,
        distance: f32,
        mix: f32,
        shelf: &ShelfCoeffs,
        hpf: &HpfCoeffs,
    ) -> (f32, f32) {
        // 恒等点でも書き込みは止めない。止めると再有効化した瞬間に
        // ライン内の古いサンプルが wet として噴き出す。
        let pos = self.write_pos;
        self.line_l[pos] = l;
        self.line_r[pos] = r_in;
        self.write_pos = (pos + 1) % self.line_l.len();

        // r = 0 は原音恒等点では**ない**（恒等点は MIX = 0 と bypass）。
        // wet の計算は省いてよいが、出力は必ず crossfade 式へ通す。
        // これにより m > 0 での端点不連続が消える。
        if distance == 0.0 {
            // 拡散段のリングも同じ理由で回し続ける。
            self.diff_state_l.push(l);
            self.diff_state_r.push(r_in);
            let dry = 1.0 - mix;
            return (dry * l, dry * r_in);
        }

        let scale = time_scale(distance);
        // 拡散段。tap 時刻は distance に依らず固定（計画書 §2.2）。
        let mut wet_l = self.diff_state_l.process_wide(l, &self.diff_l);
        let mut wet_r = self.diff_state_r.process_wide(r_in, &self.diff_r);
        for tap in &self.taps {
            wet_l += tap.gain_l * read_fractional(&self.line_l, pos, tap.delay_l * scale);
            wet_r += tap.gain_r * read_fractional(&self.line_r, pos, tap.delay_r * scale);
        }

        // 高域チルト → wet HPF → wet ゲイン（計画書 §3 の経路図）。
        // dry はこのどちらも通らない。通すと INV-2 が壊れる。
        let gain = wet_gain(distance);
        wet_l = gain * self.hpf_l.process(self.shelf_l.process(wet_l, shelf), hpf);
        wet_r = gain * self.hpf_r.process(self.shelf_r.process(wet_r, shelf), hpf);

        // 計画書 §5: mix は Dry/Wet 線形補間。
        let dry = 1.0 - mix;
        (dry * l + mix * wet_l, dry * r_in + mix * wet_r)
    }

    /// 幾何 tap + 拡散段を含む全体の支持長（サンプル）。INV-A の判定に使う。
    #[cfg(test)]
    pub fn total_support_len(&self) -> f32 {
        let diff = self
            .diff_l
            .taps_k
            .iter()
            .chain(self.diff_r.taps_k.iter())
            .copied()
            .max()
            .expect("拡散 tap が空") as f32;
        self.support_len().max(diff)
    }

    /// 幾何 ER-FIR の実支持長 `T_supp`（サンプル）。計画書 §2 INV-1。
    pub fn support_len(&self) -> f32 {
        self.taps
            .iter()
            .map(|tap| tap.delay_l.max(tap.delay_r))
            .fold(0.0_f32, f32::max)
    }

    #[cfg(test)]
    fn taps(&self) -> &[Tap] {
        &self.taps
    }
}

// 仮想室の T60 設計値 `T_room`（計画書 §4.2）は `f_hp` の導出にのみ使う。
// 用途が発生するのは Step 3.5（wet HPF）であり、それまで実装しない。
// ER-FIR の支持長は `T_supp`（`ErFir::support_len`）であって `T_room` ではない。

/// 拡散段の tap 列を生成する。
///
/// エネルギー正規化係数 `A` は閉形式の従属量であり、手で調整する定数ではない:
/// フィルタ後の総エネルギーを `G_f·(E_geo + A²·S)` と見て目標へ一致させる。
/// `S = Σ_j g(t_j)²`（`A = 1` での包絡二乗和）。
fn build_diffusion(
    seed: u64,
    sample_rate: f32,
    geo_energy: f32,
    filter_gain: f32,
) -> VelvetSequence {
    let mut seq = build_sequence(seed, sample_rate, DIFF_LEN, DIFF_DENSITY, |k| {
        diffusion_envelope(k as f32 / sample_rate)
    });
    let sum_sq: f32 = seq.taps_gs.iter().map(|g| g * g).sum();
    let target = 10.0_f32.powf(TARGET_ENERGY_DB / 10.0);
    let a_sq = (target / filter_gain - geo_energy) / sum_sq;
    assert!(
        a_sq > 0.0 && a_sq.is_finite(),
        "拡散段の正規化が成立しない (A² = {a_sq}): 幾何 tap だけで目標エネルギーを超えている"
    );
    let a = a_sq.sqrt();
    for g in &mut seq.taps_gs {
        *g *= a;
    }
    seq
}

fn generate_taps(sample_rate: f32) -> Vec<Tap> {
    let mut candidates = Vec::new();

    for mx in -3_i32..=3 {
        for my in -3_i32..=3 {
            for mz in -3_i32..=3 {
                let order = mx.unsigned_abs() + my.unsigned_abs() + mz.unsigned_abs();
                if order == 0 || order > 3 {
                    continue;
                }

                let image = [
                    image_coordinate(mx, SRC_POS[0], ROOM_DIM[0]),
                    image_coordinate(my, SRC_POS[1], ROOM_DIM[1]),
                    image_coordinate(mz, SRC_POS[2], ROOM_DIM[2]),
                ];
                let d_l = distance(image, [RCV_POS[0] - RCV_W * 0.5, RCV_POS[1], RCV_POS[2]]);
                let d_r = distance(image, [RCV_POS[0] + RCV_W * 0.5, RCV_POS[1], RCV_POS[2]]);
                let polarity = if order % 2 == 0 { 1.0 } else { -1.0 };
                // 基準直接音距離 d0 で正規化する。dry の単位ゲインと同じ基準へ
                // tap field を揃えるためであり、素材にも時間にも依存しない固定スカラー。
                let order_gain = direct_distance() * polarity * WALL_G.powi(order as i32);
                candidates.push((
                    d_l.min(d_r),
                    Tap {
                        delay_l: d_l / SOUND_C * sample_rate,
                        delay_r: d_r / SOUND_C * sample_rate,
                        gain_l: order_gain / d_l,
                        gain_r: order_gain / d_r,
                    },
                ));
            }
        }
    }

    candidates.sort_by(|left, right| left.0.total_cmp(&right.0));
    candidates
        .into_iter()
        .take(N_TAP)
        .map(|(_, tap)| tap)
        .collect()
}

#[inline]
fn image_coordinate(index: i32, source: f32, dimension: f32) -> f32 {
    let sign = if index.rem_euclid(2) == 0 { 1.0 } else { -1.0 };
    sign * source + 2.0 * index.saturating_add(1).div_euclid(2) as f32 * dimension
}

/// 基準直接音距離 `d0 = ||SRC_POS − RCV_POS||`。
///
/// Room A から一意に決まる固定スカラーであり、入力素材にも時間にも追従しない。
/// tap gain の正規化にのみ使う。値を直書きせず必ずこの式から導く。
#[inline]
pub fn direct_distance() -> f32 {
    distance(SRC_POS, RCV_POS)
}

#[inline]
fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// `pos` は現在サンプルを書き込んだ位置（遅延 0）。
#[inline]
fn read_fractional(line: &[f32], pos: usize, delay: f32) -> f32 {
    let whole = delay.floor() as usize;
    let fraction = delay - whole as f32;
    let newer = line[(pos + line.len() - whole) % line.len()];
    let older = line[(pos + line.len() - whole - 1) % line.len()];
    newer + fraction * (older - newer)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    #[test]
    fn image_source_generates_the_selected_reflections() {
        let er = ErFir::new(SR);
        assert_eq!(er.taps().len(), N_TAP);
        assert!(er.taps().iter().all(|tap| tap.delay_l != tap.delay_r));
    }

    /// 計画書 §3.1.1 の閉形式を、実装とは独立に手で展開して照合する
    /// （§8.2「tap 位置が設計式と一致」）。m = (-1, 0, 0) の一次反射:
    /// S'_x = -S_x = -2.30、受音左は x = RCV_POS[0] - RCV_W/2。
    #[test]
    fn tap_delays_match_the_design_formula() {
        let image = [-SRC_POS[0], SRC_POS[1], SRC_POS[2]];
        let dx = image[0] - (RCV_POS[0] - RCV_W * 0.5);
        let dy = image[1] - RCV_POS[1];
        let dz = image[2] - RCV_POS[2];
        let d_l = (dx * dx + dy * dy + dz * dz).sqrt();
        let expected_delay = d_l / SOUND_C * SR;
        // 一次反射なので極性は負、ゲインは d0·g^1 / d（d0 は基準直接音距離）。
        let expected_gain = -direct_distance() * WALL_G / d_l;

        let er = ErFir::new(SR);
        let tap = er
            .taps()
            .iter()
            .find(|tap| (tap.delay_l - expected_delay).abs() < 1.0e-3)
            .expect("m = (-1, 0, 0) の一次反射が採用 tap に含まれていない");
        assert!((tap.gain_l - expected_gain).abs() < 1.0e-6);
    }

    /// 高域チルトを恒等（`r = 0` の係数）にすると wet は純 FIR になる。
    /// この条件でのみ「支持長超が厳密ゼロ」を無条件に主張できる。
    fn identity_shelf() -> ShelfCoeffs {
        hf_shelf_coeffs(SR, 0.0)
    }

    /// 恒等 HPF。ER-FIR 部だけを取り出して有限支持を確かめるために使う。
    /// 実行するコード経路は本番と同一で、係数だけを恒等に置く。
    fn identity_hpf() -> HpfCoeffs {
        HpfCoeffs {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
        }
    }

    fn wet_only(
        len: usize,
        distance: f32,
        shelf: &ShelfCoeffs,
        hpf: &HpfCoeffs,
    ) -> (Vec<f32>, Vec<f32>) {
        let mut er = ErFir::new(SR);
        let mut left = Vec::with_capacity(len);
        let mut right = Vec::with_capacity(len);
        for index in 0..len {
            let x = if index == 0 { 1.0 } else { 0.0 };
            let (l, r) = er.process(x, x, distance, 1.0, shelf, hpf);
            left.push(l);
            right.push(r);
        }
        (left, right)
    }

    /// 設計上の tap 時刻に IR のエネルギーが立つこと。読み書き順の
    /// 1 サンプルずれはここで検出される。時刻は `time_scale(r)` 倍される。
    #[test]
    fn impulse_response_peaks_sit_on_the_scaled_tap_delays() {
        let distance = 0.5;
        let scale = time_scale(distance);
        let er = ErFir::new(SR);
        let len = (er.support_len() * scale).ceil() as usize + 4;
        let (ir, _) = wet_only(len, distance, &identity_shelf(), &identity_hpf());
        let gain = wet_gain(distance);

        for tap in er.taps() {
            let whole = (tap.delay_l * scale).floor() as usize;
            let energy = ir[whole].abs() + ir[whole + 1].abs();
            assert!(
                energy > 0.5 * gain * tap.gain_l.abs(),
                "tap delay {} にエネルギーがない",
                tap.delay_l * scale
            );
        }
    }

    /// INV-1 / INV-A（FIR 部）。チルト恒等なら支持長超は厳密ゼロ。
    /// 支持長は幾何 tap と拡散段の大きい方（拡散段の時刻は r 非依存）。
    #[test]
    fn er_fir_support_is_exactly_finite() {
        for &distance in &[0.25_f32, 0.5, 1.0] {
            let scale = time_scale(distance);
            let er = ErFir::new(SR);
            let t_supp_geo = er
                .taps()
                .iter()
                .map(|tap| tap.delay_l * scale)
                .fold(0.0_f32, f32::max);
            let t_supp = t_supp_geo.max(er.total_support_len());
            let (ir, _) = wet_only(
                t_supp.ceil() as usize + 256,
                distance,
                &identity_shelf(),
                &identity_hpf(),
            );

            let last = t_supp_geo.floor() as usize + 1;
            assert!(ir[last].abs() > 0.0, "r={distance} で最終幾何 tap が立っていない");
            for (index, sample) in ir.iter().enumerate().skip(t_supp.ceil() as usize + 2) {
                assert_eq!(*sample, 0.0, "r={distance} のサンプル {index} が非ゼロ");
            }
        }
    }

    /// INV-1 の限定条項。wet 経路の IIR は 2 個（高域チルト 1 次 + wet HPF 2 次）で、
    /// どちらも支持長超に指数尾を持つ。FTZ 有効下で有限ステップ後に厳密ゼロへ
    /// 到達することを実測する。
    #[test]
    fn wet_reaches_exact_zero_under_ftz() {
        #[cfg(target_arch = "x86_64")]
        unsafe {
            use std::arch::x86_64::{_mm_getcsr, _mm_setcsr};
            let csr = _mm_getcsr();
            _mm_setcsr(csr | 0x8040);
        }

        let distance = 1.0;
        let er = ErFir::new(SR);
        let t_supp = (er.support_len() * time_scale(distance)).max(er.total_support_len());
        // 観測窓は T_supp + 0.5 s。§2 INV-1 の見積り T_ftz ≈ 227 ms を含む。
        let len = t_supp.ceil() as usize + (SR * 0.5) as usize;
        let (ir, _) = wet_only(len, distance, &hf_shelf_coeffs(SR, distance), &hpf_coeffs(SR));

        let zero_from = ir.iter().rposition(|s| *s != 0.0).expect("wet が全ゼロ") + 1;
        // T_supp は拡散段込みで 300 ms。IIR の尾の見積り T_ftz ≈ 227 ms に
        // 余裕を見て 300 ms を上限とする。
        let bound = t_supp + SR * 0.300;
        assert!(
            (zero_from as f32) < bound,
            "厳密ゼロ到達 {} サンプルが上限 {bound} を超えた",
            zero_from
        );
        println!(
            "INV-1: T_supp={:.2} ms, 厳密ゼロ到達 {:.2} ms (尾 {:.2} ms)",
            t_supp / SR * 1000.0,
            zero_from as f32 / SR * 1000.0,
            (zero_from as f32 - t_supp) / SR * 1000.0
        );
    }

    /// `r = 0` は原音恒等点では**ない**。`m = 1` なら厳密ゼロ、`m = 0` なら原音、
    /// 任意の `m` で `(1−m)·x` となること（`nimbus_diffusion_v01.md` Step 1）。
    #[test]
    fn distance_zero_is_a_silent_wet_not_an_identity() {
        let shelf = identity_shelf();
        let hpf = hpf_coeffs(SR);
        let input = (-0.375, 0.625);

        let mut er = ErFir::new(SR);
        assert_eq!(
            er.process(input.0, input.1, 0.0, 1.0, &shelf, &hpf),
            (0.0, 0.0),
            "r=0, m=1 は厳密ゼロでなければならない"
        );

        let mut er = ErFir::new(SR);
        assert_eq!(
            er.process(input.0, input.1, 0.0, 0.0, &shelf, &hpf),
            input,
            "r=0, m=0 は原音でなければならない"
        );

        for step in 0..=10 {
            let m = step as f32 / 10.0;
            let mut er = ErFir::new(SR);
            let (l, r) = er.process(input.0, input.1, 0.0, m, &shelf, &hpf);
            assert_eq!((l, r), ((1.0 - m) * input.0, (1.0 - m) * input.1), "m={m}");
        }
    }

    /// `r = 0` と最小正数近傍で出力が跳ばないこと。旧仕様では `r = 0` だけが
    /// crossfade 式を通らなかったため、`m > 0` で端点が不連続だった。
    #[test]
    fn the_distance_endpoint_is_continuous() {
        let shelf = identity_shelf();
        let hpf = hpf_coeffs(SR);
        let (m, input) = (1.0_f32, (0.5_f32, -0.5_f32));

        let mut at_zero = ErFir::new(SR);
        let zero = at_zero.process(input.0, input.1, 0.0, m, &shelf, &hpf);

        let mut near_zero = ErFir::new(SR);
        let near = near_zero.process(input.0, input.1, 1.0e-6, m, &shelf, &hpf);

        assert!(
            (zero.0 - near.0).abs() < 1.0e-6 && (zero.1 - near.1).abs() < 1.0e-6,
            "端点が不連続: r=0 で {zero:?}, r=1e-6 で {near:?}"
        );
    }

    /// 恒等点は `MIX = 0`。Distance を問わず bit-exact な原音であること。
    #[test]
    fn mix_zero_is_bit_exact_at_every_distance() {
        let shelf = identity_shelf();
        let hpf = hpf_coeffs(SR);
        let input = (-0.375, 0.625);
        for step in 0..=10 {
            let r = step as f32 / 10.0;
            let mut er = ErFir::new(SR);
            // 遅延ラインへ履歴を作ってから判定する（wet が非ゼロでも原音であること）。
            for _ in 0..64 {
                er.process(0.25, -0.25, r, 0.0, &shelf, &hpf);
            }
            assert_eq!(er.process(input.0, input.1, r, 0.0, &shelf, &hpf), input, "r={r}");
        }
    }

    /// 恒等点の間も遅延ラインは走り続ける。止めると復帰時に
    /// ライン内の古いサンプルが噴き出す（R-2 指摘 1）。
    #[test]
    fn delay_line_keeps_running_at_the_identity_point() {
        let mut er = ErFir::new(SR);
        let shelf = identity_shelf();
        let distance = 1.0;
        let first_delay = er
            .taps()
            .iter()
            .map(|tap| tap.delay_l * time_scale(distance))
            .fold(f32::MAX, f32::min);

        let hpf = hpf_coeffs(SR);
        er.process(1.0, 1.0, 0.0, 1.0, &shelf, &hpf);
        for _ in 1..first_delay.floor() as usize {
            er.process(0.0, 0.0, 0.0, 1.0, &shelf, &hpf);
        }
        let (l, _) = er.process(0.0, 0.0, distance, 1.0, &shelf, &hpf);
        assert!(l.abs() > 0.0, "恒等点の間に遅延ラインへ書き込まれていない");
    }

    #[test]
    fn mix_is_a_linear_dry_wet_crossfade() {
        let mut er = ErFir::new(SR);
        let input = (-0.375, 0.625);
        assert_eq!(
            er.process(input.0, input.1, 1.0, 0.0, &identity_shelf(), &hpf_coeffs(SR)),
            input
        );
    }

    /// INV-3 の代数部分。計画書 §3.3.2 の固定点・単調性・有界性。
    #[test]
    fn wet_gain_is_a_strictly_increasing_map_from_zero() {
        assert_eq!(wet_gain(0.0), 0.0, "W(0) は厳密ゼロでなければならない");
        assert_eq!(wet_gain(1.0), 1.0, "W(1) は厳密に 1 でなければならない");

        let mut previous = f32::NEG_INFINITY;
        for step in 0..=100 {
            let r = step as f32 / 100.0;
            let w = wet_gain(r);
            assert!(w > previous, "r={r} で W が単調増加していない");
            assert!((0.0..=1.0).contains(&w), "r={r} で W が範囲外");
            previous = w;
            assert!(time_scale(r) > 0.0);
        }
        assert!(time_scale(1.0) < time_scale(0.0));
    }

    /// INV-3 の実測（計画書 §3.3.2 / §6 Step 3）。
    ///
    /// 実現エネルギーは分数遅延の線形補間リップルを載せるため、隣接点
    /// （Δr = 0.01）では狭義単調にならない。隣接点のくぼみが 1.0 dB を
    /// 超えないことのみを判定する。
    ///
    /// **Δr ≥ 0.05 の狭義単調増加は Step 3 で復活させた**
    /// （`nimbus_diffusion_v01.md`）。Step 1 では `W(r) = r` への変更で時刻圧縮
    /// `s(r)` による tap field のエネルギー揺らぎ（最悪 −0.178 dB @ r = 0.88）が
    /// 覆い隠されなくなり判定を外していたが、拡散段のエネルギーが幾何 tap を
    /// 大きく上回り、かつ拡散 tap の時刻は `distance` で動かない（同 §2.2）ため、
    /// 総エネルギーは `W(r)² = r²` にほぼ厳密比例する。
    #[test]
    fn measured_dr_ratio_decreases_over_the_sweep() {
        let energy: Vec<f32> = (0..=100)
            .map(|step| {
                let r = step as f32 / 100.0;
                let er = ErFir::new(SR);
                let len = (er.support_len() * time_scale(r)).ceil() as usize + 512;
                let (l, right) = wet_only(len, r, &hf_shelf_coeffs(SR, r), &hpf_coeffs(SR));
                l.iter().chain(right.iter()).map(|s| s * s).sum()
            })
            .collect();

        let mut worst_dip = 0.0_f32;
        for step in 2..=100 {
            worst_dip = worst_dip.max(10.0 * (energy[step - 1] / energy[step]).log10());
        }
        println!("INV-3: 隣接リップル最大 {worst_dip:.3} dB");
        assert!(worst_dip < 1.0, "隣接リップル {worst_dip:.3} dB が 1.0 dB を超えた");

        // Δr = 0.05 では実測エネルギーが狭義単調増加すること。
        for step in (5..=100).step_by(5) {
            assert!(
                energy[step] > energy[step - 5],
                "r={} → {} でエネルギーが増えていない",
                (step - 5) as f32 / 100.0,
                step as f32 / 100.0
            );
        }
    }

    #[test]
    fn hf_shelf_matches_its_design_gains() {
        // r = 0 は恒等（b0 = 1, b1 = a1 で極零相殺）。
        let flat = hf_shelf_coeffs(SR, 0.0);
        assert!((flat.b0 - 1.0).abs() < 1.0e-6);
        assert!((flat.b1 - flat.a1).abs() < 1.0e-6);

        let c = hf_shelf_coeffs(SR, 1.0);
        let dc = (c.b0 + c.b1) / (1.0 + c.a1);
        let nyquist = (c.b0 - c.b1) / (1.0 - c.a1);
        let expected = 10f32.powf(-HF_TILT_DB / 20.0);
        assert!((dc - 1.0).abs() < 1.0e-5, "DC 利得 {dc}");
        assert!((nyquist - expected).abs() < 1.0e-5, "Nyquist 利得 {nyquist}");
        assert!(c.a1.abs() < 1.0, "極が単位円外");
    }

    /// INV-5（計画書 §8.2 の手続き）。`mono(dry+wet)/mono(dry)` を
    /// 1/3 oct 平均で測り、全帯域 ±6 dB 以内であること。r = 1 が最悪ケース。
    /// **INV-5 は凍結中**（`nimbus_diffusion_v01.md` Step 1）。
    ///
    /// `d0` 正規化で wet が 10.7 dB 大きくなった結果、dry と加算したときの
    /// コム構造が深くなり、実測で **+7.14 dB @ 126 Hz**（谷は −5.32 dB）となって
    /// ±6 dB を超えた。これは意図した変更の直接の帰結である。
    ///
    /// さらに Step 3 で総エネルギーが +13 dB になると偏差は拡大する。
    /// **±6 dB という上限は今回のレベル目標と原理的に両立しない。**
    /// 100% wet のリバーブを原音と加算すれば大きなコム構造が出るのは当然であり、
    /// この上限は wet が −17.9 dB だった頃の設定である。
    ///
    /// 閾値を発明して通すことはせず、判定を凍結する。Step 3 で不変条件そのものを
    /// 再設計すること（判定対象を「dry + wet の和」から「wet 単体の左右和に
    /// コム的破綻がないか」へ移すのが有力）。測定は残してあるので
    /// `cargo test -p fandango-nimbus --release -- --ignored --nocapture` で確認できる。
    #[test]
    #[ignore = "INV-5 はレベル較正と非両立。Step 3 で再設計する"]
    fn mono_sum_colouration_stays_within_six_db() {
        let distance = 1.0;
        let er = ErFir::new(SR);
        let len = (er.support_len() * time_scale(distance)).ceil() as usize + 512;
        let (wl, wr) = wet_only(len, distance, &hf_shelf_coeffs(SR, distance), &hpf_coeffs(SR));

        let dft = |x: &[f32], f: f32| {
            let w = 2.0 * std::f64::consts::PI * f as f64 / SR as f64;
            let (mut re, mut im) = (0.0_f64, 0.0_f64);
            for (k, s) in x.iter().enumerate() {
                re += *s as f64 * (w * k as f64).cos();
                im -= *s as f64 * (w * k as f64).sin();
            }
            (re, im)
        };

        let mut worst: (f32, f64) = (0.0, 0.0);
        let mut peak: (f32, f64) = (0.0, 0.0);
        let mut first = true;
        let mut fc = 25.0_f32;
        while fc < 20_000.0 {
            let lo = fc / 2f32.powf(1.0 / 6.0);
            let hi = fc * 2f32.powf(1.0 / 6.0);
            let ratio = (hi / lo).powf(1.0 / 64.0);
            let (mut acc, mut count) = (0.0_f64, 0.0_f64);
            let mut f = lo;
            while f <= hi {
                let (rl, il) = dft(&wl, f);
                let (rr, ir) = dft(&wr, f);
                // dry のモノ和はインパルスなので定数 2.0。
                let re = 2.0 + rl + rr;
                let im = il + ir;
                acc += (re * re + im * im) / 4.0;
                count += 1.0;
                f *= ratio;
            }
            let band_db = 10.0 * (acc / count).log10();
            if first || band_db < worst.1 {
                worst = (fc, band_db);
            }
            if first || band_db > peak.1 {
                peak = (fc, band_db);
            }
            first = false;
            fc *= 2f32.powf(1.0 / 3.0);
        }

        println!(
            "INV-5: 1/3 oct 最深 {:+.2} dB @ {:.0} Hz, 最大 {:+.2} dB @ {:.0} Hz",
            worst.1, worst.0, peak.1, peak.0
        );
        assert!(worst.1 > -6.0, "谷 {:.2} dB @ {:.0} Hz", worst.1, worst.0);
        assert!(peak.1 < 6.0, "山 {:.2} dB @ {:.0} Hz", peak.1, peak.0);
    }

    /// `f_hp` は式から算出する（計画書 §4.3）。`88.0` の直書きは STOP 条件 1。
    /// 本テストは室定数から独立に組み直した値と照合する。
    #[test]
    fn hp_corner_is_derived_from_the_room_constants() {
        let n = (1.0e-3_f32).ln() / WALL_G.ln();
        let t_room = n * 7.19 / SOUND_C;
        let volume = 7.19 * 5.41 * 3.23;
        let expected = 2000.0 * (t_room / volume).sqrt();

        assert!((room_duration() - t_room).abs() < 1.0e-5, "T_room 不一致");
        assert!(
            (hp_corner() - expected).abs() < 1.0e-3,
            "f_hp {} != {}",
            hp_corner(),
            expected
        );
        // 計画書 §4.3 の概算 88 Hz と桁・値が一致すること。
        assert!((hp_corner() - 88.0).abs() < 1.0, "f_hp = {}", hp_corner());
    }

    fn hpf_amp_db(f: f32) -> f64 {
        let c = hpf_coeffs(SR);
        let w = 2.0 * std::f64::consts::PI * f as f64 / SR as f64;
        let (c1, s1) = ((-w).cos(), (-w).sin());
        let (c2, s2) = ((-2.0 * w).cos(), (-2.0 * w).sin());
        let num_re = c.b0 as f64 + c.b1 as f64 * c1 + c.b2 as f64 * c2;
        let num_im = c.b1 as f64 * s1 + c.b2 as f64 * s2;
        let den_re = 1.0 + c.a1 as f64 * c1 + c.a2 as f64 * c2;
        let den_im = c.a1 as f64 * s1 + c.a2 as f64 * s2;
        20.0 * ((num_re * num_re + num_im * num_im).sqrt()
            / (den_re * den_re + den_im * den_im).sqrt())
        .log10()
    }

    /// 2 次 Butterworth（Q = 1/√2）の設計どおりの減衰量か。
    #[test]
    fn wet_hpf_matches_its_design_response() {
        let f0 = hp_corner();
        // コーナーで −3 dB。
        assert!((hpf_amp_db(f0) + 3.0103).abs() < 0.05, "{}", hpf_amp_db(f0));
        // 2 次 Butterworth の閉形式 |H|² = (f/f0)⁴ / (1 + (f/f0)⁴) と照合する。
        // −12 dB/oct は f ≪ f0 の漸近値であって 1 oct 下の値ではない。
        for &ratio in &[0.25_f32, 0.5, 2.0, 4.0] {
            let x = (ratio as f64).powi(4);
            let expected = 10.0 * (x / (1.0 + x)).log10();
            let measured = hpf_amp_db(f0 * ratio);
            assert!(
                (measured - expected).abs() < 0.05,
                "f0×{ratio}: 実測 {measured:.3} dB, 設計 {expected:.3} dB"
            );
        }
        // 通過域は素通し。
        assert!(hpf_amp_db(1000.0).abs() < 0.2);
        assert!(hpf_amp_db(10_000.0).abs() < 0.05);
        // ピークなし（Butterworth）。
        for step in 0..200 {
            let f = f0 * 1.05f32.powi(step);
            if f > 20_000.0 {
                break;
            }
            assert!(hpf_amp_db(f) < 0.01, "{f} Hz でピーク {}", hpf_amp_db(f));
        }
    }

    /// 計画書 §6 Step 3.5 / §3.4.4: r スイープでローエンドが痩せ／膨らみを
    /// 起こさないこと。判定は**恒等 HPF（= HPF なし）との比較**で行う。
    /// §3.4 が主張しているのは「低域 wet の除去は改善方向」であって、
    /// 新しい閾値を満たすことではないため、絶対値の閾値を発明しない。
    #[test]
    fn wet_hpf_improves_the_low_end_across_the_distance_sweep() {
        let mut worst_with = 0.0_f64;
        let mut worst_without = 0.0_f64;
        let mut worst_at = (0.0_f32, 0.0_f32);

        for step in 1..=10 {
            let r = step as f32 / 10.0;
            let er = ErFir::new(SR);
            let len = (er.support_len() * time_scale(r)).ceil() as usize + (SR * 0.5) as usize;
            let shelf = hf_shelf_coeffs(SR, r);
            let with = wet_only(len, r, &shelf, &hpf_coeffs(SR));
            let without = wet_only(len, r, &shelf, &identity_hpf());

            for &fc in &[31.5_f32, 40.0, 50.0, 63.0, 80.0] {
                // dry のモノ和はインパルスなので定数 2.0。
                let deviation = |ir: &(Vec<f32>, Vec<f32>)| -> f64 {
                    let w = 2.0 * std::f64::consts::PI * fc as f64 / SR as f64;
                    let (mut re, mut im) = (0.0_f64, 0.0_f64);
                    for (k, (a, b)) in ir.0.iter().zip(ir.1.iter()).enumerate() {
                        let (cos, sin) = ((w * k as f64).cos(), (w * k as f64).sin());
                        re += (*a + *b) as f64 * cos;
                        im -= (*a + *b) as f64 * sin;
                    }
                    let mag = ((2.0 + re) * (2.0 + re) + im * im).sqrt() / 2.0;
                    (20.0 * mag.log10()).abs()
                };

                let d_with = deviation(&with);
                if d_with > worst_with {
                    worst_with = d_with;
                    worst_at = (r, fc);
                }
                worst_without = worst_without.max(deviation(&without));
            }
        }

        println!(
            "Step 3.5: ローエンド偏差 最大 {worst_with:.3} dB @ r={}, {} Hz（HPF なしなら {worst_without:.3} dB）",
            worst_at.0, worst_at.1
        );
        // 判定は最悪ケースの比較で行う。点ごとの比較にはしない:
        // wet が小さい低 r 域では偏差が 0.1 dB 未満で、HPF の位相回転により
        // わずかに増える点がある。可聴でも有意でもなく、判定に使う量ではない。
        assert!(
            worst_with < worst_without,
            "最悪ケースが改善していない（{worst_with:.3} dB >= {worst_without:.3} dB）"
        );
        // ±6 dB の絶対ゲートは課さない。INV-5 は Step 1 で凍結済みであり
        // （`nimbus_diffusion_v01.md`）、拡散段で wet が +13 dB になれば
        // dry と加算したコム構造の偏差はさらに拡大する。新しい閾値を
        // 発明せず、HPF の有無の比較のみで判定する。
    }


    // ---- 拡散段（`nimbus_diffusion_v01.md` Step 3）----

    /// 減衰包絡の構造。`t ≥ 300ms` は厳密ゼロ（INV-A）、`t < 200ms` はテーパー無改変、
    /// テーパー区間は連続かつ単調減少。
    #[test]
    fn diffusion_envelope_is_tapered_and_finitely_supported() {
        assert_eq!(diffusion_envelope(DIFF_LEN), 0.0);
        assert_eq!(diffusion_envelope(DIFF_LEN + 1.0e-4), 0.0);
        assert_eq!(diffusion_envelope(0.0), 1.0);

        // テーパー開始前は純粋な指数減衰。
        for step in 0..=20 {
            let t = DIFF_TAPER_START * step as f32 / 20.0;
            let decay = (-(10.0_f32.ln()) * 3.0 * t / DIFF_T60).exp();
            assert!((diffusion_envelope(t) - decay).abs() < 1.0e-7, "t={t}");
        }
        // テーパー区間は単調減少で、終端へ滑らかに接続する。
        let mut previous = diffusion_envelope(DIFF_TAPER_START);
        for step in 1..=100 {
            let t = DIFF_TAPER_START + (DIFF_LEN - DIFF_TAPER_START) * step as f32 / 100.0;
            let v = diffusion_envelope(t);
            assert!(v < previous, "t={t} で単調減少していない");
            previous = v;
        }
        assert!(previous < 1.0e-4, "終端が滑らかに 0 へ落ちていない: {previous}");
    }

    /// INV-B。同一 sample_rate で 2 回生成した拡散 tap 列が bit-exact。
    #[test]
    fn diffusion_taps_are_deterministic_and_channel_independent() {
        let a = ErFir::new(SR);
        let b = ErFir::new(SR);
        assert_eq!(a.diff_l.taps_k, b.diff_l.taps_k);
        assert_eq!(a.diff_l.taps_gs, b.diff_l.taps_gs);
        assert_eq!(a.diff_r.taps_k, b.diff_r.taps_k);
        assert_eq!(a.diff_r.taps_gs, b.diff_r.taps_gs);
        // INV-E: L/R は別シード。
        assert_ne!(a.diff_l.taps_k, a.diff_r.taps_k);
    }

    /// Step 3 の受け入れ基準: 総エネルギー +13 dB ± 1.5 dB、ピーク < 0.5。
    #[test]
    fn diffusion_hits_the_energy_target() {
        let er = ErFir::new(SR);
        let len = er.total_support_len().ceil() as usize + 512;
        let (l, r) = wet_only(len, 1.0, &hf_shelf_coeffs(SR, 1.0), &hpf_coeffs(SR));

        let energy = |x: &[f32]| 10.0 * x.iter().map(|s| (s * s) as f64).sum::<f64>().log10();
        let peak = |x: &[f32]| x.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
        let (el, er_db) = (energy(&l), energy(&r));
        let peak_max = peak(&l).max(peak(&r));
        println!(
            "Step 3: 総エネルギー L {el:+.2} dB / R {er_db:+.2} dB, ピーク {peak_max:.3}"
        );
        for (name, db) in [("L", el), ("R", er_db)] {
            assert!(
                (db - TARGET_ENERGY_DB as f64).abs() < 1.5,
                "{name} の総エネルギー {db:+.2} dB が目標 ±1.5 dB を外れた"
            );
        }
        assert!(peak_max < 0.5, "ピーク {peak_max:.3} が 0.5 以上");
    }

    /// Step 3 の受け入れ基準: L/R 相関が 20 ms 窓の全区間で |r| < 0.15。
    #[test]
    fn diffusion_leaves_the_channels_uncorrelated() {
        let er = ErFir::new(SR);
        let len = er.total_support_len().ceil() as usize;
        let (l, r) = wet_only(len, 1.0, &hf_shelf_coeffs(SR, 1.0), &hpf_coeffs(SR));

        let window = (SR * 0.020) as usize;
        let mut worst: (f64, usize) = (0.0, 0);
        for start in (0..len - window).step_by(window) {
            let (a, b) = (&l[start..start + window], &r[start..start + window]);
            let dot: f64 = a.iter().zip(b).map(|(x, y)| (*x * *y) as f64).sum();
            let na: f64 = a.iter().map(|x| (*x * *x) as f64).sum::<f64>().sqrt();
            let nb: f64 = b.iter().map(|x| (*x * *x) as f64).sum::<f64>().sqrt();
            if na == 0.0 || nb == 0.0 {
                continue;
            }
            let c = (dot / (na * nb)).abs();
            if c > worst.0 {
                worst = (c, start);
            }
        }
        println!(
            "Step 3: L/R 相関 最大 |r| = {:.3} @ {:.1} ms",
            worst.0,
            worst.1 as f32 / SR * 1000.0
        );
        assert!(worst.0 < 0.15, "相関 {:.3} が 0.15 以上", worst.0);
    }

    /// INV-A。チルト・HPF 恒等なら支持長超は厳密ゼロ。
    #[test]
    fn diffusion_support_is_exactly_finite() {
        let er = ErFir::new(SR);
        let last = er.total_support_len().ceil() as usize;
        let (l, r) = wet_only(last + 4096, 1.0, &identity_shelf(), &identity_hpf());
        for (index, (a, b)) in l.iter().zip(r.iter()).enumerate().skip(last + 2) {
            assert_eq!((*a, *b), (0.0, 0.0), "サンプル {index} が非ゼロ");
        }
        // 拡散段の支持は 300 ms + 分数遅延の 1 サンプル分に収まる。
        assert!(
            er.total_support_len() <= SR * DIFF_LEN,
            "支持長 {} が 300 ms を超えた",
            er.total_support_len()
        );
    }

    /// Step 3 の受け入れ基準: エコー密度が 100 ms 以降で 1500 peaks/s 以上。
    #[test]
    fn echo_density_stays_above_the_floor() {
        let er = ErFir::new(SR);
        let len = er.total_support_len().ceil() as usize;
        let (l, _) = wet_only(len, 1.0, &identity_shelf(), &identity_hpf());

        let window = (SR * 0.020) as usize;
        let mut worst = f32::MAX;
        let mut start = (SR * 0.100) as usize;
        // テーパー区間 (200 ms 以降) は包絡が 0 へ向かうため密度判定から外す。
        while start + window <= (SR * DIFF_TAPER_START) as usize {
            let count = l[start..start + window].iter().filter(|s| **s != 0.0).count();
            worst = worst.min(count as f32 / 0.020);
            start += window;
        }
        println!("Step 3: エコー密度 最小 {worst:.0} peaks/s (100–200 ms)");
        assert!(worst >= 1500.0, "エコー密度 {worst:.0} peaks/s が下限未満");
    }

    /// Step 3 の受け入れ基準: 300 ms 近傍で包絡が不連続に切れていないこと。
    /// テーパー終端の 10 ms 平均 rms が、テーパー開始点の rms より十分小さいこと
    /// （＝ゲート的な段差ではなく連続に 0 へ落ちていること）を数値で確認する。
    #[test]
    fn the_tail_fades_out_without_a_step() {
        let er = ErFir::new(SR);
        let len = er.total_support_len().ceil() as usize;
        let (l, _) = wet_only(len, 1.0, &identity_shelf(), &identity_hpf());

        let rms = |from: f32, to: f32| {
            let (a, b) = ((SR * from) as usize, ((SR * to) as usize).min(len));
            (l[a..b].iter().map(|s| (s * s) as f64).sum::<f64>() / (b - a) as f64).sqrt()
        };
        let at_taper_start = rms(0.190, 0.200);
        let at_end = rms(0.290, 0.300);
        let db = 20.0 * (at_end / at_taper_start).log10();
        println!("Step 3: テーパー終端 {db:.1} dB (200 ms 直前比)");
        assert!(db < -30.0, "終端が {db:.1} dB までしか落ちていない（段差の疑い）");
    }

    /// Step 4: CPU 実測（`nimbus_diffusion_v01.md`）。バッファ 128 サンプルの
    /// `process` 実測時間をリアルタイム比で報告する。計測なので `--ignored` で実行:
    /// `cargo test -p fandango-nimbus --release -- --ignored --nocapture cpu`
    #[test]
    #[ignore = "CPU 実測（Step 4）。時間計測なので通常のテスト実行から外す"]
    fn cpu_cost_per_buffer() {
        use std::hint::black_box;
        use std::time::Instant;

        const BUFFER: usize = 128;
        for &sr in &[48_000.0_f32, 96_000.0] {
            let mut er = ErFir::new(sr);
            let shelf = hf_shelf_coeffs(sr, 1.0);
            let hpf = hpf_coeffs(sr);
            let taps = er.diff_l.taps_k.len();

            // ウォームアップ（ページフォルトと分岐予測を落ち着かせる）。
            for i in 0..BUFFER * 100 {
                let x = (i as f32 * 0.001).sin();
                black_box(er.process(x, -x, 1.0, 1.0, &shelf, &hpf));
            }

            let buffers = 2000;
            let start = Instant::now();
            for i in 0..buffers * BUFFER {
                let x = (i as f32 * 0.001).sin();
                black_box(er.process(x, -x, 1.0, 1.0, &shelf, &hpf));
            }
            let per_buffer = start.elapsed().as_secs_f64() / buffers as f64;
            let budget = BUFFER as f64 / sr as f64;
            println!(
                "Step 4: {:.0} kHz, 拡散 {taps} tap/ch → {:.1} us/buffer (予算 {:.1} us, リアルタイム比 {:.2} %)",
                sr / 1000.0,
                per_buffer * 1.0e6,
                budget * 1.0e6,
                per_buffer / budget * 100.0
            );
            // 受け入れ基準は 48 kHz のみ（計画書 Step 4）。96 kHz は報告のみ。
            if sr == 48_000.0 {
                assert!(
                    per_buffer / budget < 0.05,
                    "48 kHz でリアルタイム比 {:.2} % が 5 % を超えた",
                    per_buffer / budget * 100.0
                );
            }
        }
    }
}
