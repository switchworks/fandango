//! 分数遅延対応リングバッファ。

/// リングバッファ・整数/分数遅延どちらも読める遅延線。
///
/// `write` で 1 サンプル書き込み、書込みポインタが進行。
/// `read_int(d)` は `d` サンプル前 (0 = 直前に書いた値)、
/// `read_frac(d, frac)` は `d + frac` サンプル前を線形補間で返す。
#[derive(Debug, Clone)]
pub struct DelayLine {
    buf: Vec<f32>,
    write: usize,
}

impl DelayLine {
    /// 最大遅延サンプル数を確保。実際のバッファは 2 の冪乗にアップサイズ。
    pub fn with_capacity(max_delay: usize) -> Self {
        let cap = (max_delay + 1).next_power_of_two().max(2);
        Self { buf: vec![0.0; cap], write: 0 }
    }

    /// 内部バッファをゼロで埋める。
    pub fn reset(&mut self) {
        self.buf.fill(0.0);
        self.write = 0;
    }

    /// 最大遅延サンプル数を変更 (再確保する)。呼び出し後は内容ゼロ。
    pub fn resize(&mut self, max_delay: usize) {
        let cap = (max_delay + 1).next_power_of_two().max(2);
        self.buf = vec![0.0; cap];
        self.write = 0;
    }

    /// バッファ容量 (常に 2 の冪乗)。
    #[inline]
    pub fn capacity(&self) -> usize {
        self.buf.len()
    }

    /// 1 サンプル書込み。書込みポインタが 1 進む。
    #[inline]
    pub fn write(&mut self, x: f32) {
        self.buf[self.write] = x;
        self.write = (self.write + 1) & (self.buf.len() - 1);
    }

    /// `d` サンプル前の値を返す (0 = 直前に書き込んだ値)。
    #[inline]
    pub fn read_int(&self, d: usize) -> f32 {
        let mask = self.buf.len() - 1;
        // write は「次の書込み位置」なので `write - 1` が最後に書いた位置。
        let idx = (self.write + self.buf.len() - 1 - (d & mask)) & mask;
        self.buf[idx]
    }

    /// `d + frac` サンプル前の値を線形補間して返す。`frac ∈ [0, 1)`。
    #[inline]
    pub fn read_frac(&self, d: usize, frac: f32) -> f32 {
        let a = self.read_int(d);
        let b = self.read_int(d + 1);
        a + frac * (b - a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_int_matches_write_order() {
        let mut dl = DelayLine::with_capacity(16);
        for i in 1..=10 {
            dl.write(i as f32);
        }
        assert_eq!(dl.read_int(0), 10.0);
        assert_eq!(dl.read_int(1), 9.0);
        assert_eq!(dl.read_int(9), 1.0);
    }

    #[test]
    fn read_frac_linear() {
        let mut dl = DelayLine::with_capacity(16);
        for i in 0..8 {
            dl.write(i as f32);
        }
        // 直前=7.0, 1 前=6.0
        let v = dl.read_frac(0, 0.25);
        assert!((v - 6.75).abs() < 1e-6);
    }
}
