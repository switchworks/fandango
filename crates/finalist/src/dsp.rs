//! finalist の純 DSP 部。nih_plug 非依存・f32・process 内アロケーション無し
//! （fb2k への `#[path]` include 移植の前提条件）。

/// lookahead 長（ms）。ノブ化しない固定値。
///
/// N = ceil(LOOKAHEAD_MS/1000·sr) が決めるのは**ゲインリップル歪の下限周波数**。
/// ゲイン経路の総インパルス応答長は peak-hold N + min-hold (N-1) + boxcar ×2 (2N-2)
/// で 4N-3 サンプルあり、これが 1 周期を覆う正弦ではゲインが定数になって歪が消える。
/// すなわち歪ゼロの下限は f_min ≈ sr/(4N)、ms 表記で LOOKAHEAD_MS ≈ 250/f_min。
///
/// 実測（48 kHz・amp 0.5・input +12 dB・ceiling -1.0 dB・THD 2〜20 次）:
/// f_min が 0 になる最小 N は 50 Hz で 237、100 Hz で 119、200 Hz で 61、400 Hz で 31 と
/// 閉形式 sr/(4f) に ±1.5 % で一致する。2.5 ms = 120 サンプル @48 kHz は
/// **100 Hz をリップルフリーにする最小の窓長**であり、この値の出所はそこにある。
/// 1.0 ms へ縮めると 100 Hz の THD は 0.664 %、50 Hz は 2.42 % まで増える。
/// 逆に伸ばすと総群遅延 D = 2N-2 が線形に増える（2.5 ms で 4.96 ms、6.0 ms で 11.96 ms）。
/// 1 kHz 以上の THD と過渡時のゲイン下降量は N に依存しない（`examples/lookahead_sweep.rs`）。
pub const LOOKAHEAD_MS: f32 = 2.5;

/// f32 の丸め分を吸収する天井マージン。実数モデル上の凸結合保証を
/// IEEE 浮動小数点実装へ持ち込むため、内部では `ceiling·(1-CEILING_MARGIN)` を使う。
/// 1e-6 は f32 の相対丸め (≈6e-8) の十数倍で、boxcar の累積和は f64 で持つ。
const CEILING_MARGIN: f32 = 1e-6;


/// ADAA 差分商のゼロ割回避閾値。`|Δu| ≤ DRIVE_ADAA_EPS` で中点直接評価に切り替える。
const DRIVE_ADAA_EPS: f32 = 1e-5;

/// 整形関数 `f(u) = u + k·(s(u) - u)`。
/// `s(u) = (3u - u³)/2` (|u| ≤ 1)、`sign(u)` (|u| > 1) の C¹ 連続な 3 次ソフトクリップ。
/// `k = 0` で厳密恒等（`u + 0·(…) = u`）。
pub fn drive_shape(u: f32, k: f32) -> f32 {
    let s = if u.abs() <= 1.0 {
        (3.0 * u - u * u * u) * 0.5
    } else {
        u.signum()
    };
    u + k * (s - u)
}

/// 整形関数の**非線形残差** `s(u) - u` の不定積分 `R(u) = S(u) - u²/2`。
///
/// ADAA を掛けるのは残差だけで足りる。`f_k(u) = u + k·(s(u) - u)` の線形項 `u` は
/// エイリアスを生まないうえ、これを ADAA に通すと差分商が `(u + u_prev)/2` になり
/// **信号全体にローパスが掛かる**（減衰量は `|Δu|` すなわち高域成分の量に比例する）。
/// その結果が `k = 0` での不連続で、旧実装は `k == 0` の早期 return でそれを隠していた
/// （旧実装は `k = 1e-6` でも広帯域素材で `max|y-u| ≈ 0.25`、本実装は 1.19e-7 で k に比例する）。
/// 残差だけに掛ければ `k = 0` は分岐なしに厳密恒等になり、`k → 0` も連続になる。
/// エイリアス抑制は変わらない（11 kHz・amp 1.4・k=1 の折返し成分で差 ≤ 0.13 dB）。
///
/// `S(u) = 3u²/4 - u⁴/8` (|u| ≤ 1)。`S(1) = 5/8` なので、`|u| > 1` 側の
/// `S(u) = |u| + C` は u = ±1 での連続条件 `1 + C = 5/8` から `C = -3/8`、
/// すなわち `S(u) = |u| - 3/8`。ここから
/// `R(u) = u²/4 - u⁴/8` (|u| ≤ 1)、`R(u) = |u| - 3/8 - u²/2` (|u| > 1)。
///
/// f32 の桁落ちを避けるため f64 で評価する。
fn drive_resid_antideriv(u: f64) -> f64 {
    let u2 = u * u;
    if u.abs() <= 1.0 {
        0.25 * u2 - 0.125 * u2 * u2
    } else {
        u.abs() - 0.375 - 0.5 * u2
    }
}

/// 1 チャンネル分の drive stage（1 次 ADAA）。
struct DriveStage {
    u_prev: f32,
}

impl DriveStage {
    fn new() -> Self {
        Self { u_prev: 0.0 }
    }

    fn reset(&mut self) {
        self.u_prev = 0.0;
    }

    fn process(&mut self, u: f32, k: f32) -> f32 {
        // 残差だけを ADAA するので `k = 0` は `u + 0.0·(…) = u` で厳密恒等。
        // 恒等点のための場合分けは持たない（`drive_resid_antideriv` の doc を参照）。
        let du = u - self.u_prev;
        let y = if du.abs() > DRIVE_ADAA_EPS {
            // 分子・分母とも f64 で評価する（分母だけ f32 減算にすると桁落ちが残る）。
            let (uf, pf) = (u as f64, self.u_prev as f64);
            let r_now = drive_resid_antideriv(uf);
            let r_prev = drive_resid_antideriv(pf);
            u + k * ((r_now - r_prev) / (uf - pf)) as f32
        } else {
            // 中点での残差 `s(m) - m` を直接使う。
            let m = (u + self.u_prev) * 0.5;
            u + k * (drive_shape(m, 1.0) - m)
        };
        self.u_prev = u;
        y
    }
}

/// 単調 deque による窓内 max / min（Lemire 法、償却 O(1)）。
/// `is_max = true` で sliding max、`false` で sliding min。
struct Monotonic {
    /// (サンプル index, 値) のリングバッファ。窓長 N を超える要素は残らないので容量 N で足りる。
    buf: Vec<(u64, f32)>,
    head: usize,
    len: usize,
    window: usize,
    is_max: bool,
}

impl Monotonic {
    fn new(window: usize, is_max: bool) -> Self {
        Self {
            buf: vec![(0, 0.0); window],
            head: 0,
            len: 0,
            window,
            is_max,
        }
    }

    fn reset(&mut self) {
        self.head = 0;
        self.len = 0;
    }

    fn at(&self, i: usize) -> (u64, f32) {
        self.buf[(self.head + i) % self.window]
    }

    /// index `n` の値 `v` を投入し、窓 `[n-window+1, n]` の max（または min）を返す。
    fn push(&mut self, n: u64, v: f32) -> f32 {
        // 窓から出た先頭を先に捨てる（後回しにすると満杯時に容量 N を溢れて上書きになる）。
        while self.len > 0 && self.at(0).0 + self.window as u64 <= n {
            self.head = (self.head + 1) % self.window;
            self.len -= 1;
        }
        // 後方から、v に負ける（max なら v 以下の）要素を捨てる。
        while self.len > 0 {
            let back = self.at(self.len - 1).1;
            let dominated = if self.is_max { back <= v } else { back >= v };
            if !dominated {
                break;
            }
            self.len -= 1;
        }
        let tail = (self.head + self.len) % self.window;
        self.buf[tail] = (n, v);
        self.len += 1;
        self.at(0).1
    }
}

/// 長さ N の移動平均（非負・係数和 1 の FIR）。累積和は f64 で持ちドリフトを避ける。
struct Boxcar {
    buf: Vec<f32>,
    idx: usize,
    sum: f64,
    n: usize,
}

impl Boxcar {
    /// 初期状態はゲイン 1.0 で満たす（無音開始時に恒等を保つ）。
    fn new(n: usize) -> Self {
        Self {
            buf: vec![1.0; n],
            idx: 0,
            sum: n as f64,
            n,
        }
    }

    fn reset(&mut self) {
        self.buf.iter_mut().for_each(|v| *v = 1.0);
        self.idx = 0;
        self.sum = self.n as f64;
    }

    fn push(&mut self, v: f32) -> f32 {
        self.sum += v as f64 - self.buf[self.idx] as f64;
        self.buf[self.idx] = v;
        self.idx = (self.idx + 1) % self.n;
        (self.sum / self.n as f64) as f32
    }
}

/// finalist のサンプル処理本体。
pub struct Finalist {
    sample_rate: f32,
    /// lookahead 窓長 N。
    n: usize,
    /// ゲイン経路の総群遅延 D = 2N-2。
    delay: usize,
    drive_l: DriveStage,
    drive_r: DriveStage,
    peak_max: Monotonic,
    gain_min: Monotonic,
    smooth1: Boxcar,
    smooth2: Boxcar,
    /// wet 遅延線（L/R 各 D サンプルのリング）。入力ゲイン・drive 適用後の信号。
    dl: Vec<f32>,
    dr: Vec<f32>,
    /// dry 遅延線。bypass 時に出す素の信号。wet 線と別に持たないと、
    /// bypass へ切り替えた直後の D サンプルがゲイン適用済みのまま
    /// リミッタを通らずに出てしまう（天井保証が bypass 状態に依存してしまう）。
    dry_l: Vec<f32>,
    dry_r: Vec<f32>,
    dl_idx: usize,
    /// one-pole release の状態（1.0 = 減衰なし）。
    g_rel: f32,
    /// サンプル通番（deque の窓管理用）。
    n_idx: u64,
}

impl Finalist {
    pub fn new(sample_rate: f32) -> Self {
        // N ≥ 2 に丸める（D = 2N-2 ≥ 2 を保証し、遅延線長 0 の縮退を排除する）。
        let n = (LOOKAHEAD_MS / 1000.0 * sample_rate).ceil().max(2.0) as usize;
        // 遅延整列の導出（この値が 1 サンプルでもずれると天井保証が崩れる）:
        //   出力 g[n] は boxcar 2 段（台 0..2N-2）を通った g_min の凸結合。
        //   g_min[n-k] は g_rel[j], j ∈ [n-k-N+1, n-k] の最小値であり、
        //   g_rel[j] ≤ g_req[j] ≤ ceiling / p[m]  (∀ m ∈ [j-N+1, j])。
        //   よって「乗算対象 x[n-D] を全ての k ∈ [0, 2N-2] が押さえる」条件は
        //   各窓 [n-k-N+1, n-k] と [n-D, n-D+N-1] が交わること:
        //     k = 0     → D ≤ 2N-2
        //     k = 2N-2  → D ≥ 2N-2
        //   ゆえに D = 2N-2 が唯一解（= sliding_min の群遅延 (N-1) と
        //   boxcar ×2 の群遅延 (N-1) の和）。
        let delay = 2 * n - 2;
        Self {
            sample_rate,
            n,
            delay,
            drive_l: DriveStage::new(),
            drive_r: DriveStage::new(),
            peak_max: Monotonic::new(n, true),
            gain_min: Monotonic::new(n, false),
            smooth1: Boxcar::new(n),
            smooth2: Boxcar::new(n),
            // リング長 = D。読み出し→書き込みの順で D サンプル遅延になる。
            dl: vec![0.0; delay],
            dr: vec![0.0; delay],
            dry_l: vec![0.0; delay],
            dry_r: vec![0.0; delay],
            dl_idx: 0,
            g_rel: 1.0,
            n_idx: 0,
        }
    }

    /// ゲイン経路の総群遅延（サンプル）。ホストへ報告する値と遅延線長はこの 1 箇所で決める。
    pub fn latency_samples(&self) -> u32 {
        self.delay as u32
    }

    /// one-pole release 係数 r = exp(-1/(release_ms/1000·sr))。ブロック単位で組む。
    pub fn release_coeff(&self, release_ms: f32) -> f32 {
        (-1.0 / (release_ms / 1000.0 * self.sample_rate)).exp()
    }

    /// 内部状態を初期化する。
    pub fn reset(&mut self) {
        self.drive_l.reset();
        self.drive_r.reset();
        self.peak_max.reset();
        self.gain_min.reset();
        self.smooth1.reset();
        self.smooth2.reset();
        self.dl.iter_mut().for_each(|v| *v = 0.0);
        self.dr.iter_mut().for_each(|v| *v = 0.0);
        self.dry_l.iter_mut().for_each(|v| *v = 0.0);
        self.dry_r.iter_mut().for_each(|v| *v = 0.0);
        self.dl_idx = 0;
        self.g_rel = 1.0;
        self.n_idx = 0;
    }

    /// ステレオ 1 サンプルを処理する。
    ///
    /// - `input_gain`: 線形値
    /// - `ceiling`: 線形値。天井保証を成立させるためブロック内で一定にすること
    /// - `drive`: 多項式ステージ量 k ∈ [0, 1]。0 で厳密恒等
    /// - `release_coeff`: [`Self::release_coeff`] の値
    /// - `bypass`: true なら遅延した dry 信号を返す。**wet 経路の計算は止めない**
    ///   （入力ゲイン・drive・検出器・遅延線とも通常どおり回す）。止めると遅延線の中身と
    ///   出力段のゲインが食い違い、切替の前後 D サンプルで天井保証が破れる。
    pub fn process(
        &mut self,
        l: f32,
        r: f32,
        input_gain: f32,
        drive: f32,
        ceiling: f32,
        release_coeff: f32,
        bypass: bool,
    ) -> (f32, f32) {
        let (dry_l, dry_r) = (l, r);
        let l = self.drive_l.process(l * input_gain, drive);
        let r = self.drive_r.process(r * input_gain, drive);

        let ceiling_safe = ceiling * (1.0 - CEILING_MARGIN);
        let p = l.abs().max(r.abs());
        let pk = self.peak_max.push(self.n_idx, p);
        // pk ≤ ceiling_safe では厳密に 1.0（分岐なし・0 除算なし）。
        let g_req = ceiling_safe / pk.max(ceiling_safe);
        self.g_rel = g_req.min(1.0 - (1.0 - self.g_rel) * release_coeff);
        let g_min = self.gain_min.push(self.n_idx, self.g_rel);
        let g = self.smooth2.push(self.smooth1.push(g_min));

        // 遅延線: 書き込み位置に現在値を入れ、同じ位置から D サンプル前を読む。
        let wet_l = self.dl[self.dl_idx];
        let wet_r = self.dr[self.dl_idx];
        let old_dry_l = self.dry_l[self.dl_idx];
        let old_dry_r = self.dry_r[self.dl_idx];
        self.dl[self.dl_idx] = l;
        self.dr[self.dl_idx] = r;
        self.dry_l[self.dl_idx] = dry_l;
        self.dry_r[self.dl_idx] = dry_r;
        self.dl_idx = (self.dl_idx + 1) % self.dl.len();

        self.n_idx += 1;
        if bypass {
            (old_dry_l, old_dry_r)
        } else {
            (wet_l * g, wet_r * g)
        }
    }
}
