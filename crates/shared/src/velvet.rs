//! ヴェルベットノイズ (OVN) タップ列生成と疎 FIR 畳み込み。
//!
//! **PRNG バイト再現性について**: `Xorshift64` は fandango 独自の固定シード
//! 自前実装。`std` の乱数・`rand` crate 等への置き換え禁止。

/// xorshift64* PRNG (fandango 独自、std 非依存、固定シード)。
pub struct Xorshift64 {
    state: u64,
}

impl Xorshift64 {
    pub fn new(seed: u64) -> Self {
        Self {
            state: seed.max(1),
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }

    pub fn next_f64_01(&mut self) -> f64 {
        // 上位 53 bit を [0, 1) に写像
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }
}

/// ヴェルベットノイズタップ列。
pub struct VelvetSequence {
    /// 位置 (サンプル)
    pub taps_k: Vec<u32>,
    /// g(m) · s(m) を事前乗算
    pub taps_gs: Vec<f32>,
}

/// ヴェルベットノイズタップ列を生成する。
///
/// - `seed`: xorshift64* 初期シード (固定定数)
/// - `fs`: サンプルレート
/// - `length_sec`: シーケンス長 L (秒)
/// - `density`: パルス密度 ρ (個/秒)
/// - `gain_at`: タップ時刻 k (サンプル) に対するゲイン包絡
pub fn build_sequence(
    seed: u64,
    fs: f32,
    length_sec: f32,
    density: f32,
    gain_at: impl Fn(u32) -> f32,
) -> VelvetSequence {
    let td = fs / density;
    let m_count = (density * length_sec).ceil() as usize;
    let mut prng = Xorshift64::new(seed);
    let mut taps_k = Vec::with_capacity(m_count);
    let mut taps_gs = Vec::with_capacity(m_count);
    for m in 0..m_count {
        let r1 = prng.next_f64_01();
        let r2 = prng.next_f64_01();
        let k = ((m as f64) * td as f64 + r1 * (td as f64 - 1.0)).round() as u32;
        let s = if r2 >= 0.5 { 1.0f32 } else { -1.0f32 };
        taps_k.push(k);
        taps_gs.push(gain_at(k) * s);
    }
    VelvetSequence { taps_k, taps_gs }
}

/// 疎 FIR 畳み込みの状態。リングバッファ 1 本 (書込 1 回 + タップ M 回参照)。
pub struct DecorrelatorState {
    ring: Vec<f32>,
    write_pos: usize,
    mask: usize,
}

impl DecorrelatorState {
    pub fn new(max_length_samples: u32) -> Self {
        let cap = (max_length_samples as usize + 1)
            .next_power_of_two()
            .max(64);
        Self {
            ring: vec![0.0; cap],
            write_pos: 0,
            mask: cap - 1,
        }
    }

    pub fn reset(&mut self) {
        for v in &mut self.ring {
            *v = 0.0;
        }
        self.write_pos = 0;
    }

    /// リングへ書き込むだけで畳み込みを行わない。wet を捨てる区間でも
    /// 履歴を切らさないために使う (止めると再有効化時に古いサンプルが噴き出す)。
    #[inline]
    pub fn push(&mut self, x: f32) {
        self.ring[self.write_pos] = x;
        self.write_pos = (self.write_pos + 1) & self.mask;
    }

    /// `process` の並列アキュムレータ版。tap 数が多い呼び出し側 (nimbus の拡散段)
    /// 向け。単一アキュムレータでは f32 加算の依存チェーンが tap 数に比例して
    /// 伸び、スループットが latency 律速になる。4 本に分けて依存を切る。
    ///
    /// `process` と**加算順序が異なる**ため結果は bit-exact ではない。
    /// velvet crate の再現性を壊さないよう、`process` 側は変更していない。
    #[inline]
    pub fn process_wide(&mut self, x: f32, seq: &VelvetSequence) -> f32 {
        self.ring[self.write_pos] = x;
        let base = self.write_pos + self.mask + 1;
        let mut acc = [0.0f32; 4];
        let mut k_chunks = seq.taps_k.chunks_exact(4);
        let mut g_chunks = seq.taps_gs.chunks_exact(4);
        for (k, gs) in k_chunks.by_ref().zip(g_chunks.by_ref()) {
            for lane in 0..4 {
                acc[lane] += self.ring[(base - k[lane] as usize) & self.mask] * gs[lane];
            }
        }
        let mut tail = 0.0f32;
        for (k, gs) in k_chunks.remainder().iter().zip(g_chunks.remainder()) {
            tail += self.ring[(base - *k as usize) & self.mask] * *gs;
        }
        self.write_pos = (self.write_pos + 1) & self.mask;
        (acc[0] + acc[1]) + (acc[2] + acc[3]) + tail
    }

    #[inline]
    pub fn process(&mut self, x: f32, seq: &VelvetSequence) -> f32 {
        self.ring[self.write_pos] = x;
        let mut acc = 0.0f32;
        for (k, gs) in seq.taps_k.iter().zip(seq.taps_gs.iter()) {
            let idx = (self.write_pos + self.mask + 1 - *k as usize) & self.mask;
            acc += self.ring[idx] * *gs;
        }
        self.write_pos = (self.write_pos + 1) & self.mask;
        acc
    }
}
