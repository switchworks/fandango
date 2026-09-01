//! Step Gate DSP — 無状態閉形式実装 (`plans/gate_v01.md`)。
//!
//! TRIG `|` / HOLD `-` / REST `_` の 3-state ステップシーケンサ。
//! ゲイン曲線は自身の過去出力の積分ではなく **transport 位置の明示関数**として書く。
//! 音声状態変数を持たないため、ブロック分割不変・transport locate 正当性が構造から保証される。
//!
//! - ON 区間は TRIG 開始 + 連続 HOLD 延長で構成し、`[k, k + L - 1 + duty)` を占める
//! - 区間の両端に片側幅 `W_e = edge * duty / 2` (step 単位) の raised-cosine エッジを置く
//! - エッジ形状は Möbius 位相ワープ `W_s(u) = u / (u + s(1-u))` で歪める。
//!   開きエッジは `1/s`、閉じエッジは `s` を使い、1 パラメータで attack/cut 非対称を表す

use std::f32::consts::PI;

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum StepCell {
    Trig,
    Hold,
    Rest,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum StepLength {
    Quarter,
    Eighth,
    Sixteenth,
    ThirtySecond,
    EighthTriplet,
    SixteenthTriplet,
}

impl StepLength {
    pub fn beats_per_step(self) -> f64 {
        match self {
            StepLength::Quarter => 1.0,
            StepLength::Eighth => 0.5,
            StepLength::Sixteenth => 0.25,
            StepLength::ThirtySecond => 0.125,
            StepLength::EighthTriplet => 1.0 / 3.0,
            StepLength::SixteenthTriplet => 1.0 / 6.0,
        }
    }
}

/// パターンの位相原点をどこに取るか。
#[derive(Copy, Clone, Eq, PartialEq, Debug, Default)]
pub enum SyncMode {
    /// ソング先頭（`pos_beats = 0`）起点。
    Song,
    /// 小節頭起点。パターンは小節ごとに頭へ戻る。
    #[default]
    Bar,
    /// 直近の NoteOn 起点。NoteOn を一度も受けていない間は [`SyncMode::Song`] と同じ。
    Note,
}

/// 位相原点の決め方一式。
///
/// `Song` / `Bar` は `pos_beats` の**明示関数**であり状態を持たない。よってブロック
/// 分割不変性と transport locate 正当性がそのまま保たれる。`Note` だけは原点が
/// 過去のイベントに依存するため、**locate 正当性は成立しない**（巻き戻しても原点は
/// 最後の NoteOn のまま）。これは note sync の性質であって欠陥ではない。
#[derive(Copy, Clone, Debug)]
pub struct PhaseSync {
    pub mode: SyncMode,
    /// 1 小節の拍数（`time_sig_numerator / denominator * 4`）。取れない時は 4.0。
    pub beats_per_bar: f64,
    /// 直近の NoteOn の `pos_beats`。未受信は 0.0（= Song と同じ原点）。
    pub note_origin_beats: f64,
}

impl Default for PhaseSync {
    fn default() -> Self {
        Self {
            mode: SyncMode::default(),
            beats_per_bar: 4.0,
            note_origin_beats: 0.0,
        }
    }
}

impl PhaseSync {
    /// 拍位置 `b` に対する位相原点（拍）。`b` だけの関数なので分割不変。
    #[inline]
    pub fn origin(&self, b: f64) -> f64 {
        match self.mode {
            SyncMode::Song => 0.0,
            // `bar_start_pos_beats()` は使わない。あれはブロック頭の値なので、
            // ブロックが小節をまたぐと「transport 位置の明示関数」が壊れる。
            SyncMode::Bar => (b / self.beats_per_bar).floor() * self.beats_per_bar,
            SyncMode::Note => self.note_origin_beats,
        }
    }
}

/// Möbius 位相ワープ。`s > 0` で `W(0) = 0`, `W(1) = 1`, 単調増加。
#[inline]
fn warp(x: f32, s: f32) -> f32 {
    x / (x + s * (1.0 - x))
}

/// 立ち上がりエッジ。`u = 0` で 0、`u = 1` で 1。
#[inline]
fn edge_rise(u: f32, s: f32) -> f32 {
    let t = PI * warp(u, s) * 0.5;
    let v = t.sin();
    v * v
}

/// `shape` (-1..1) を Möbius パラメータへ写像。`shape = 0` で `s = 1` (対称)。
#[inline]
pub fn shape_to_s(shape: f32) -> f32 {
    (2.0f32).powf(2.0 * shape)
}

/// `steps` 内の ON 区間のうち、絶対ステップ位置 `pos` を含むものを探す。
///
/// 戻り値は `(区間開始, 区間終了)` を絶対ステップ座標で表したもの。
/// `pos` がどの区間にも属さなければ `None`。
///
/// 区間は TRIG で始まり、直後の連続 HOLD で 1 ステップずつ延びる。末尾に `duty` を足す。
/// 隣接して接する区間 (間隙ゼロ) は連結された 1 区間として返す。
fn active_interval(steps: &[StepCell], pos: f64, duty: f32) -> Option<(f64, f64)> {
    let n = steps.len() as i64;
    if n == 0 {
        return None;
    }
    let cur = pos.floor() as i64;

    // pos を含みうる区間の開始 TRIG を後方に探す。区間長は最大 n ステップ。
    let mut start_step: Option<i64> = None;
    for back in 0..n {
        let k = cur - back;
        let cell = steps[k.rem_euclid(n) as usize];
        match cell {
            StepCell::Trig => {
                start_step = Some(k);
                break;
            }
            StepCell::Hold => continue,
            StepCell::Rest => return None,
        }
    }
    let mut start = start_step?;

    // 区間終端を前方へ伸ばす。連続 HOLD で 1 ステップずつ延長し、
    // 末尾で duty を足す。duty で間隙が生じなければ次の区間と連結する。
    let mut end_step = start;
    let mut guard = 0i64;
    loop {
        // end_step の次のセルが HOLD なら区間を延長。
        let next = steps[(end_step + 1).rem_euclid(n) as usize];
        if next == StepCell::Hold {
            end_step += 1;
        } else if duty >= 1.0 && next == StepCell::Trig {
            // 間隙ゼロで次の TRIG 区間に接続 → 連結。
            end_step += 1;
        } else {
            break;
        }
        guard += 1;
        if guard >= n {
            // パターン全体が途切れなく連結 → 常時 ON。
            return Some((f64::NEG_INFINITY, f64::INFINITY));
        }
    }

    // duty=1 の連結時は開始側も後方へ伸ばす必要があるが、
    // 上の後方走査が Rest で止まるため start は既に最初の非 Rest 位置にある。
    // 連結ケースでは start が cur より前方の TRIG を指しうるので補正不要。
    if duty >= 1.0 {
        // 後方連結: start の 1 つ前が TRIG/HOLD なら更に遡る。
        let mut g2 = 0i64;
        loop {
            let prev = steps[(start - 1).rem_euclid(n) as usize];
            if prev == StepCell::Trig || prev == StepCell::Hold {
                start -= 1;
            } else {
                break;
            }
            g2 += 1;
            if g2 >= n {
                return Some((f64::NEG_INFINITY, f64::INFINITY));
            }
        }
    }

    let end = end_step as f64 + duty as f64;
    let start_f = start as f64;
    if pos >= start_f && pos < end {
        Some((start_f, end))
    } else {
        None
    }
}

/// ゲート曲線。`pos` は絶対ステップ座標 (整数部 = ステップ番号)。
///
/// 不変条件 (`plans/gate_v01.md` §4):
/// - 値域は 0..1 厳密 (`sin^2` の値域。clamp を使わない)
/// - エッジ内で単調 (`warp` が単調、`sin^2(pi x/2)` が x∈[0,1] で単調)
/// - 区間内部では厳密に `1.0` (リテラル代入。数値評価に頼らない)
/// - 状態を持たないので同一 `pos` に対し常に同一値
pub fn gate_curve(
    steps: &[StepCell],
    pos: f64,
    duty: f32,
    edge: f32,
    shape: f32,
) -> f32 {
    let Some((start, end)) = active_interval(steps, pos, duty) else {
        return 0.0;
    };

    // 片側エッジ幅 (step 単位)。2*w = edge*duty <= duty <= 区間長 が無条件成立するため
    // 場合分け無しで両エッジが区間内に収まる。
    let w = (edge * duty * 0.5) as f64;
    if w <= 0.0 {
        return 1.0;
    }

    let s = shape_to_s(shape);
    let from_start = pos - start;
    let to_end = end - pos;

    if from_start < w {
        // 立ち上がり: 1/s
        edge_rise((from_start / w) as f32, 1.0 / s)
    } else if to_end < w {
        // 立ち下がり: s (u を反転して rise を再利用)
        edge_rise((to_end / w) as f32, s)
    } else {
        1.0
    }
}

/// 無状態ステップゲート。`sample_rate` は位相前進の設定値であり音声状態ではない。
pub struct StepGate {
    sample_rate: f32,
}

impl StepGate {
    pub fn new() -> Self {
        Self {
            sample_rate: 44100.0,
        }
    }

    /// 位相前進に使うサンプルレート。モノ経路の分割処理が位置を進めるのに参照する。
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }

    pub fn reset(&mut self) {}

    /// `steps` 空 / `depth=0` で完全 bypass (bit-exact)。
    #[allow(clippy::too_many_arguments)]
    pub fn process(
        &mut self,
        out_l: &mut [f32],
        out_r: &mut [f32],
        steps: &[StepCell],
        step_length: StepLength,
        pos_beats: f64,
        sync: PhaseSync,
        tempo_bpm: f32,
        duty: f32,
        edge: f32,
        shape: f32,
        depth: f32,
    ) {
        if steps.is_empty() || depth <= 0.0 {
            return;
        }
        let n = out_l.len().min(out_r.len());
        if n == 0 {
            return;
        }

        let beats_per_step = step_length.beats_per_step();
        let dt_beats = tempo_bpm as f64 / 60.0 / self.sample_rate as f64;

        for i in 0..n {
            // 累積加算ではなく乗算で求める。長いブロックでの誤差蓄積を避け、
            // ブロック分割不変性 (I7) をブロック長に依らず保つため。
            // 原点は拍位置 `b` だけの関数なので、この性質は sync mode を問わず保たれる。
            let b = pos_beats + i as f64 * dt_beats;
            let pos = (b - sync.origin(b)) / beats_per_step;
            let g = gate_curve(steps, pos, duty, edge, shape);
            let gain = g * depth + (1.0 - depth);
            out_l[i] *= gain;
            out_r[i] *= gain;
        }
    }
}

impl Default for StepGate {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: StepCell = StepCell::Trig;
    const H: StepCell = StepCell::Hold;
    const R: StepCell = StepCell::Rest;

    fn ramp(n: usize) -> Vec<f32> {
        (0..n).map(|i| ((i % 97) as f32 - 48.0) / 48.0).collect()
    }

    /// 既存テストは Song 固定（sync mode 導入前と同じ原点）。
    fn run(
        steps: &[StepCell],
        n: usize,
        block: usize,
        duty: f32,
        edge: f32,
        shape: f32,
        depth: f32,
    ) -> (Vec<f32>, Vec<f32>) {
        run_sync(
            steps,
            n,
            block,
            duty,
            edge,
            shape,
            depth,
            PhaseSync {
                mode: SyncMode::Song,
                ..PhaseSync::default()
            },
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn run_sync(
        steps: &[StepCell],
        n: usize,
        block: usize,
        duty: f32,
        edge: f32,
        shape: f32,
        depth: f32,
        sync: PhaseSync,
    ) -> (Vec<f32>, Vec<f32>) {
        // sr / tempo は 2 進で厳密表現できる値を選ぶ。
        // 120 BPM / 32768 Hz → 1 サンプル = 2^-14 拍、1/16 ステップ換算で 2^-12 ステップ。
        // これにより `pos_beats` の算出が分割位置に依らず bit-exact になり、
        // A4 が浮動小数の結合則ではなく無状態性そのものを検証する。
        let mut sg = StepGate::new();
        sg.prepare(32768.0);
        let mut l = ramp(n);
        let mut r = ramp(n);
        let sr = 32768.0f64;
        let tempo = 120.0f32;
        let mut off = 0usize;
        while off < n {
            let end = (off + block).min(n);
            let pos_beats = off as f64 * (tempo as f64 / 60.0 / sr);
            sg.process(
                &mut l[off..end],
                &mut r[off..end],
                steps,
                StepLength::Sixteenth,
                pos_beats,
                sync,
                tempo,
                duty,
                edge,
                shape,
                depth,
            );
            off = end;
        }
        (l, r)
    }

    /// A1: 全 Trig + duty=1.0 + depth=1.0 でビット完全一致。
    #[test]
    fn a1_identity_all_trig_full_duty() {
        let steps = [T; 16];
        let n = 4096;
        let (l, _) = run(&steps, n, n, 1.0, 1.0, 0.0, 1.0);
        let dry = ramp(n);
        assert_eq!(l, dry, "全 Trig + duty=1.0 が恒等でない");
    }

    /// A1 補強: shape / edge をどう振っても恒等は破れない (不変条件 I5)。
    #[test]
    fn a1_identity_holds_for_any_shape_and_edge() {
        let steps = [T; 16];
        let n = 2048;
        let dry = ramp(n);
        for k in -10..=10 {
            let shape = k as f32 / 10.0;
            for e in 0..=10 {
                let edge = e as f32 / 10.0;
                let (l, _) = run(&steps, n, n, 1.0, edge, shape, 1.0);
                assert_eq!(l, dry, "shape={shape} edge={edge} で恒等が破れた");
            }
        }
    }

    /// A2: depth=0 でビット完全一致。
    #[test]
    fn a2_identity_depth_zero() {
        let steps = [R; 16];
        let n = 4096;
        let (l, _) = run(&steps, n, n, 0.5, 0.5, 0.0, 0.0);
        assert_eq!(l, ramp(n), "depth=0 が恒等でない");
    }

    /// A4 を Bar sync でも通す。原点が `b` の明示関数である限り分割不変は保たれる。
    #[test]
    fn a4_block_split_invariance_bar_sync() {
        let steps = [T, H, R, T, R, H, T, R];
        let n = 4096;
        for beats_per_bar in [4.0, 3.0, 3.5] {
            let sync = PhaseSync {
                mode: SyncMode::Bar,
                beats_per_bar,
                note_origin_beats: 0.0,
            };
            let (whole, _) = run_sync(&steps, n, n, 0.5, 0.6, 0.3, 1.0, sync);
            let (split, _) = run_sync(&steps, n, 64, 0.5, 0.6, 0.3, 1.0, sync);
            assert_eq!(whole, split, "beats_per_bar={beats_per_bar} で分割不変が破れた");
        }
    }

    /// Bar sync の位相が小節頭で 0 に戻ること。拍子を変えても成立する。
    #[test]
    fn bar_sync_resets_phase_at_bar_start() {
        for beats_per_bar in [4.0, 3.0, 3.5, 7.0 / 2.0] {
            let sync = PhaseSync {
                mode: SyncMode::Bar,
                beats_per_bar,
                note_origin_beats: 0.0,
            };
            for bar in 0..8 {
                let b = bar as f64 * beats_per_bar;
                assert_eq!(b - sync.origin(b), 0.0, "小節 {bar} の頭で位相が 0 でない");
                // 小節の途中では 0 にならない（原点が前進しすぎていない）。
                let mid = b + beats_per_bar * 0.5;
                assert_eq!(mid - sync.origin(mid), beats_per_bar * 0.5);
            }
        }
    }

    /// Note sync は NoteOn 未受信（origin = 0）なら Song と完全一致すること。
    #[test]
    fn note_sync_without_note_matches_song() {
        let steps = [T, H, R, T, R, H, T, R];
        let n = 4096;
        let song = PhaseSync {
            mode: SyncMode::Song,
            ..PhaseSync::default()
        };
        let note = PhaseSync {
            mode: SyncMode::Note,
            beats_per_bar: 4.0,
            note_origin_beats: 0.0,
        };
        let (a, _) = run_sync(&steps, n, n, 0.5, 0.6, 0.3, 1.0, song);
        let (b, _) = run_sync(&steps, n, n, 0.5, 0.6, 0.3, 1.0, note);
        assert_eq!(a, b, "NoteOn 未受信の Note が Song と一致しない");
    }

    /// Song sync の原点は常に 0（拍子を渡しても影響を受けない）。
    #[test]
    fn song_sync_origin_is_zero() {
        let sync = PhaseSync {
            mode: SyncMode::Song,
            beats_per_bar: 3.0,
            note_origin_beats: 12.0,
        };
        for b in [0.0, 1.5, 7.25, -3.0] {
            assert_eq!(sync.origin(b), 0.0);
        }
    }

    /// A4: ブロック分割不変 (1x4096 と 64x64 がビット完全一致)。
    #[test]
    fn a4_block_split_invariance() {
        let steps = [T, R, H, T, R, R, T, H, H, R, T, R, T, T, R, H];
        let n = 4096;
        let (whole, _) = run(&steps, n, n, 0.5, 0.6, 0.3, 1.0);
        let (split, _) = run(&steps, n, 64, 0.5, 0.6, 0.3, 1.0);
        assert_eq!(whole, split, "ブロック分割で出力が変わった");
    }

    /// A5: 全パラメータグリッドで値域 0..1。
    #[test]
    fn a5_bounded_on_grid() {
        let steps = [T, R, H, T, R, R, T, H, H, R, T, R, T, T, R, H];
        for d in 0..=10 {
            for e in 0..=10 {
                for s in -10..=10 {
                    let (duty, edge, shape) =
                        (d as f32 / 10.0, e as f32 / 10.0, s as f32 / 10.0);
                    for i in 0..2000 {
                        let pos = i as f64 * 16.0 / 2000.0;
                        let g = gate_curve(&steps, pos, duty, edge, shape);
                        assert!(
                            (0.0..=1.0).contains(&g),
                            "g={g} が範囲外 (duty={duty} edge={edge} shape={shape} pos={pos})"
                        );
                    }
                }
            }
        }
    }

    /// A6: エッジ内で単調 (区間内の 1 階差分が符号反転しない)。
    #[test]
    fn a6_monotone_within_edges() {
        let steps = [T; 1];
        for e in 1..=10 {
            for s in -10..=10 {
                let (edge, shape) = (e as f32 / 10.0, s as f32 / 10.0);
                let duty = 0.5f32;
                let w = (edge * duty * 0.5) as f64;
                // 立ち上がりエッジ内は単調増加。
                let mut prev = gate_curve(&steps, 0.0, duty, edge, shape);
                for i in 1..=1000 {
                    let pos = w * i as f64 / 1000.0 * 0.999;
                    let g = gate_curve(&steps, pos, duty, edge, shape);
                    assert!(
                        g >= prev - 1e-7,
                        "立ち上がりが単調でない (edge={edge} shape={shape})"
                    );
                    prev = g;
                }
            }
        }
    }

    /// エッジ端点が厳密に 0 / 1 (不変条件 I3)。
    #[test]
    fn i3_edge_endpoints_exact() {
        for s in -10..=10 {
            let shape = s as f32 / 10.0;
            let sv = shape_to_s(shape);
            assert_eq!(edge_rise(0.0, sv), 0.0, "shape={shape} で rise(0) != 0");
            assert_eq!(edge_rise(1.0, sv), 1.0, "shape={shape} で rise(1) != 1");
        }
    }

    /// Rest のみのパターンで完全に閉じる。
    #[test]
    fn all_rest_is_silent() {
        let steps = [R; 16];
        let n = 1024;
        let (l, _) = run(&steps, n, n, 0.5, 0.5, 0.0, 1.0);
        assert!(l.iter().all(|v| *v == 0.0), "全 Rest で無音にならない");
    }

    /// duty=0.5 の Trig セルで前半 ON / 後半 OFF。
    #[test]
    fn duty_half_creates_off_zone() {
        let steps = [T; 16];
        let g_on = gate_curve(&steps, 0.25, 0.5, 0.2, 0.0);
        let g_off = gate_curve(&steps, 0.75, 0.5, 0.2, 0.0);
        assert!(g_on > 0.99, "ON 区間中央が開いていない: {g_on}");
        assert_eq!(g_off, 0.0, "OFF 区間が閉じていない: {g_off}");
    }

    /// Hold が区間を延長する。
    #[test]
    fn hold_extends_interval() {
        let steps = [T, H, R, R];
        // Trig(0) + Hold(1) → 区間 [0, 1+duty)
        let g = gate_curve(&steps, 1.2, 0.5, 0.0, 0.0);
        assert_eq!(g, 1.0, "Hold で区間が延びていない");
        let g2 = gate_curve(&steps, 1.8, 0.5, 0.0, 0.0);
        assert_eq!(g2, 0.0, "Hold 後の duty ギャップが無い");
    }
}
