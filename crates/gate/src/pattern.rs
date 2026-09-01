//! パターンを「文字列 1 本」で保持する容器。
//!
//! 設計（`plans/gate_gui_v01.md` §8）:
//!
//! - 保存形式は `_` `|` `-` の文字列。ホストのパラメータ一覧には出さない
//!   （`#[persist]`）。公開パラメータは 5 本のままで、ステップ数を増やしても増えない
//! - 音声スレッドは `AtomicU64` 1 本だけを読む。**ロックを取らない**
//! - 2 bit/セル × 32 セル = 64 bit ちょうど。未使用セルは `3` を入れて長さも同じ語に載せる。
//!   語が 1 本なので**引き裂け（tear）が原理的に起きない** — 長さとセルが食い違う瞬間が無い
//!
//! 永続化と音声側ストレージが同一オブジェクトなので、状態ロード後に
//! ミラーを作り直すフックが要らない（作り直し忘れが構造的に起きない）。

use std::sync::atomic::{AtomicU64, Ordering};

use nih_plug::params::persist::PersistentField;

use crate::dsp::StepCell;

/// パターン長の上限。2 bit × 32 = 64 bit = `AtomicU64` 1 本に収まる境界。
pub const MAX_STEPS: usize = 32;

/// 未使用セルを表す 2 bit 値。長さはこの値が最初に現れる位置で決まる。
const UNUSED: u64 = 3;

/// 既定パターン。`|||-` の 4 周期。
pub const DEFAULT_PATTERN: &str = "|||-|||-|||-|||-";

/// セル → 文字。`_` = Rest、`|` = Trig、`-` = Hold。
pub fn cell_glyph(cell: StepCell) -> char {
    match cell {
        StepCell::Rest => '_',
        StepCell::Trig => '|',
        StepCell::Hold => '-',
    }
}

/// 文字 → セル。未知の文字は `None`（黙って Rest に落とさない）。
pub fn glyph_cell(c: char) -> Option<StepCell> {
    match c {
        '_' => Some(StepCell::Rest),
        '|' => Some(StepCell::Trig),
        '-' => Some(StepCell::Hold),
        _ => None,
    }
}

fn cell_bits(cell: StepCell) -> u64 {
    match cell {
        StepCell::Rest => 0,
        StepCell::Trig => 1,
        StepCell::Hold => 2,
    }
}

fn bits_cell(bits: u64) -> Option<StepCell> {
    match bits {
        0 => Some(StepCell::Rest),
        1 => Some(StepCell::Trig),
        2 => Some(StepCell::Hold),
        _ => None,
    }
}

/// 文字列 → セル列。
///
/// - 空白は無視する。`char::is_whitespace` の定義そのものなので、半角スペースだけでなく
///   **タブ・改行 (`\n` `\r`)・全角スペース (U+3000)** も落ちる。複数行に折り返して
///   書いたパターンや、他所からコピーして改行が混ざった文字列がそのまま通る
/// - 使える字は `_` `|` `-` のみ。1 文字でも未知字があれば **全体を捨てる**（部分適用しない）
/// - 長さは空白を除いた実効長で 1..=[`MAX_STEPS`]。範囲外は捨てる
///
/// 短い文字列の敷き詰めはここでは行わない。長さはユーザーが書いたとおりに保つ
/// （`|||-` は 4 ステップのパターンであり、DSP 側が周期で回す）。
pub fn parse(text: &str) -> Option<Vec<StepCell>> {
    let cells: Vec<StepCell> = text
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(glyph_cell)
        .collect::<Option<Vec<_>>>()?;
    if cells.is_empty() || cells.len() > MAX_STEPS {
        return None;
    }
    Some(cells)
}

/// セル数を `delta` だけ増減した列を返す。
///
/// 伸ばす分は [`StepCell::Rest`]、縮める分は末尾から落とす。長さは 1..=[`MAX_STEPS`]
/// で飽和する（範囲外にはならない・空にもならない）。
pub fn resized(cells: &[StepCell], delta: isize) -> Vec<StepCell> {
    let len = (cells.len() as isize + delta).clamp(1, MAX_STEPS as isize) as usize;
    let mut out = cells.to_vec();
    out.resize(len, StepCell::Rest);
    out
}

/// 削ったセルの控え（「尻尾」）。
///
/// `-` で長さを縮めた時に落ちたセルをここへ積み、`+` で押し戻せるようにする。
/// **正本ではない。** 音声スレッドは触らず、[`PatternCode`] とは独立に保存される。
/// 本体と食い違った場合は常に本体を優先し、尻尾のほうを捨てる。
///
/// 文字列の先頭が「次に戻すセル」。グリフは本体と同じ `_` `|` `-`。
/// 末尾セルを 1 つ削り、落ちたセルを尻尾の先頭へ積む。
///
/// 長さ 1 では削らない（[`resized`] が飽和するので尻尾にも積まない）。
pub fn shrink_with_tail(cells: &[StepCell], tail: &mut String) -> Vec<StepCell> {
    let out = resized(cells, -1);
    if out.len() < cells.len() {
        if let Some(dropped) = cells.last() {
            tail.insert(0, cell_glyph(*dropped));
        }
    }
    out
}

/// 尻尾があれば先頭のセルを末尾へ戻し、無ければ [`StepCell::Rest`] を足す。
///
/// 尻尾に解釈できない文字が混じっていたら**尻尾ごと捨てて** `Rest` を足す
/// （本体を汚さない。尻尾は補助データなので復旧を試みない）。
pub fn grow_with_tail(cells: &[StepCell], tail: &mut String) -> Vec<StepCell> {
    let mut out = resized(cells, 1);
    if out.len() == cells.len() {
        // 上限に張り付いている。尻尾は温存する。
        return out;
    }
    let restored = match tail.chars().next() {
        Some(c) => match glyph_cell(c) {
            Some(cell) => {
                tail.remove(0);
                cell
            }
            None => {
                tail.clear();
                StepCell::Rest
            }
        },
        None => StepCell::Rest,
    };
    let last = out.len() - 1;
    out[last] = restored;
    out
}

/// 保存から読んだ尻尾を検証する。本体長と合わせて [`MAX_STEPS`] を超える分と、
/// 解釈できない文字を含む尻尾は**捨てる**（本体を優先する）。
pub fn sanitize_tail(tail: &str, used: usize) -> String {
    if tail.chars().any(|c| glyph_cell(c).is_none()) {
        return String::new();
    }
    let room = MAX_STEPS.saturating_sub(used);
    tail.chars().take(room).collect()
}

/// 2 bit/セルに詰めたパターン。`#[persist]` の対象であり、同時に音声側の読み取り先。
pub struct PatternCode {
    /// 下位から 2 bit ずつセル。未使用位置は [`UNUSED`]。
    code: AtomicU64,
}

impl Default for PatternCode {
    fn default() -> Self {
        let this = Self {
            code: AtomicU64::new(0),
        };
        // 既定文字列は定数なのでパースは必ず成功する。失敗したらプログラムの誤り。
        let cells = parse(DEFAULT_PATTERN).expect("既定パターンが解釈できない");
        this.store(&cells);
        this
    }
}

impl PatternCode {
    /// セル列を 1 語に詰めて書く。
    pub fn store(&self, cells: &[StepCell]) {
        debug_assert!(!cells.is_empty() && cells.len() <= MAX_STEPS);
        let mut code = 0u64;
        for k in 0..MAX_STEPS {
            let v = cells.get(k).map_or(UNUSED, |c| cell_bits(*c));
            code |= v << (2 * k);
        }
        self.code.store(code, Ordering::Relaxed);
    }

    /// 文字列として書く。解釈できなければ **書かずに `false`** を返す（黙って既定へ落とさない）。
    pub fn store_str(&self, text: &str) -> bool {
        match parse(text) {
            Some(cells) => {
                self.store(&cells);
                true
            }
            None => false,
        }
    }

    /// 音声スレッド用。1 回の `load` だけで長さもセルも決まる。
    ///
    /// 戻り値は `(バッファ, 長さ)`。長さは 1..=[`MAX_STEPS`]。
    pub fn load(&self) -> ([StepCell; MAX_STEPS], usize) {
        let code = self.code.load(Ordering::Relaxed);
        let mut cells = [StepCell::Rest; MAX_STEPS];
        let mut len = 0;
        for k in 0..MAX_STEPS {
            match bits_cell((code >> (2 * k)) & 0b11) {
                Some(cell) => {
                    cells[k] = cell;
                    len = k + 1;
                }
                // 未使用に当たったらそこで終わり。以降は見ない。
                None => break,
            }
        }
        (cells, len)
    }

    /// 文字列表現。GUI の入力欄と保存の両方がこれを使う。
    pub fn to_text(&self) -> String {
        let (cells, len) = self.load();
        cells[..len].iter().map(|c| cell_glyph(*c)).collect()
    }
}

// `Arc<PatternCode>` への impl は orphan rule で書けないため、型そのものへ実装する。
// `GateParams` 自体が `Arc` で共有されるので、エディタとの共有に別の `Arc` は要らない。
impl PersistentField<'_, String> for PatternCode {
    fn set(&self, new_value: String) {
        if !self.store_str(&new_value) {
            // 壊れた状態を黙って既定で埋めない。読めなかった事実を出す。
            nih_plug::nih_error!("gate: パターン文字列を解釈できない: {new_value:?}");
        }
    }

    fn map<F, R>(&self, f: F) -> R
    where
        F: Fn(&String) -> R,
    {
        f(&self.to_text())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: StepCell = StepCell::Trig;
    const H: StepCell = StepCell::Hold;
    const R: StepCell = StepCell::Rest;

    #[test]
    fn default_roundtrip() {
        let p = PatternCode::default();
        assert_eq!(p.to_text(), DEFAULT_PATTERN);
        assert_eq!(p.load().1, DEFAULT_PATTERN.len());
    }

    #[test]
    fn store_str_rejects_invalid() {
        let p = PatternCode::default();
        assert!(!p.store_str("|||x"));
        assert!(!p.store_str(""));
        assert!(!p.store_str(&"|".repeat(MAX_STEPS + 1)));
        // 拒否後も元の値が残っている（部分適用も既定落ちもしない）。
        assert_eq!(p.to_text(), DEFAULT_PATTERN);
    }

    #[test]
    fn store_str_accepts_boundary_and_whitespace() {
        let p = PatternCode::default();
        assert!(p.store_str(&"|".repeat(MAX_STEPS)));
        assert_eq!(p.load().1, MAX_STEPS);
        assert!(p.store_str("| | | -"));
        assert_eq!(p.to_text(), "|||-");
    }

    /// `#[persist]` 経路（保存 → 読込）で往復すること。
    #[test]
    fn persistent_field_roundtrip() {
        let p = PatternCode::default();
        PersistentField::set(&p, "_|-_|-".to_owned());
        let saved: String = PersistentField::map(&p, |s| s.clone());
        assert_eq!(saved, "_|-_|-");

        let loaded = PatternCode::default();
        PersistentField::set(&loaded, saved);
        assert_eq!(loaded.load(), p.load());
    }

    /// タブ・改行・全角スペースも空白として落ちること。
    #[test]
    fn whitespace_includes_tab_and_newline() {
        let p = PatternCode::default();
        assert!(p.store_str("|||-\n|||-\r\n|||-\t|||-"));
        assert_eq!(p.to_text(), "|||-|||-|||-|||-");
        assert!(p.store_str("|\u{3000}|\u{3000}|-"));
        assert_eq!(p.to_text(), "|||-");
    }

    /// 空白を含めて 32 文字を超えても、実効長が 32 以内なら通ること。
    #[test]
    fn whitespace_does_not_count_toward_limit() {
        let p = PatternCode::default();
        let spaced: String = std::iter::repeat("| ").take(MAX_STEPS).collect();
        assert!(spaced.len() > MAX_STEPS);
        assert!(p.store_str(&spaced));
        assert_eq!(p.load().1, MAX_STEPS);
    }

    #[test]
    fn all_lengths_roundtrip() {
        let p = PatternCode::default();
        for len in 1..=MAX_STEPS {
            let text: String = (0..len)
                .map(|i| match i % 3 {
                    0 => '_',
                    1 => '|',
                    _ => '-',
                })
                .collect();
            assert!(p.store_str(&text), "len {len} が拒否された");
            assert_eq!(p.to_text(), text, "len {len} で往復しない");
        }
    }

    /// `+` / `-` に相当する長さ変更が 1..=MAX_STEPS の外へ出ないこと。
    #[test]
    fn resized_saturates_at_bounds() {
        let one = vec![StepCell::Trig];
        assert_eq!(resized(&one, -1).len(), 1);
        assert_eq!(resized(&one, -100).len(), 1);
        assert_eq!(resized(&one, 1), vec![StepCell::Trig, StepCell::Rest]);

        let full = vec![StepCell::Trig; MAX_STEPS];
        assert_eq!(resized(&full, 1).len(), MAX_STEPS);
        assert_eq!(resized(&full, 100).len(), MAX_STEPS);
        assert_eq!(resized(&full, -1).len(), MAX_STEPS - 1);

        // 全長で store しても正本が壊れないこと。
        let p = PatternCode::default();
        for delta in [-1isize, 1] {
            let mut cells = p.load().0[..p.load().1].to_vec();
            for _ in 0..MAX_STEPS + 4 {
                cells = resized(&cells, delta);
                p.store(&cells);
                let len = p.load().1;
                assert!((1..=MAX_STEPS).contains(&len), "長さ {len} が範囲外");
                assert_eq!(len, cells.len());
            }
        }
    }

    #[test]
    fn tail_roundtrip_restores_cells() {
        let cells = vec![T, H, R, T, T, H];
        let mut tail = String::new();

        // 3 つ削る。
        let mut cur = cells.clone();
        for _ in 0..3 {
            cur = shrink_with_tail(&cur, &mut tail);
        }
        assert_eq!(cur.len(), 3);
        // 削った順に先頭へ積むので、末尾から T, T, H の順 → "||-"。
        assert_eq!(tail, "||-");

        // 3 つ戻すと元に戻る。
        for _ in 0..3 {
            cur = grow_with_tail(&cur, &mut tail);
        }
        assert_eq!(cur, cells, "削って戻したら元のセル列に戻らない");
        assert!(tail.is_empty(), "戻しきったのに尻尾が残っている");
    }

    /// 尻尾が空なら `+` は Rest を足す（従来どおり）。
    #[test]
    fn grow_without_tail_appends_rest() {
        let mut tail = String::new();
        let out = grow_with_tail(&[T, T], &mut tail);
        assert_eq!(out, vec![T, T, R]);
        assert!(tail.is_empty());
    }

    /// 境界で飽和し、尻尾も壊れない。
    #[test]
    fn tail_respects_length_bounds() {
        // 長さ 1 では削れない → 尻尾にも積まれない。
        let mut tail = String::new();
        let out = shrink_with_tail(&[T], &mut tail);
        assert_eq!(out, vec![T]);
        assert!(tail.is_empty(), "削っていないのに尻尾へ積んだ");

        // 上限では増やせない → 尻尾は温存される。
        let mut tail = "|".to_owned();
        let full = vec![T; MAX_STEPS];
        let out = grow_with_tail(&full, &mut tail);
        assert_eq!(out.len(), MAX_STEPS);
        assert_eq!(tail, "|", "増やせなかったのに尻尾を消費した");
    }

    /// 壊れた尻尾は本体を汚さずに捨てる。
    #[test]
    fn broken_tail_is_discarded_not_applied() {
        let mut tail = "x|".to_owned();
        let out = grow_with_tail(&[T, T], &mut tail);
        assert_eq!(out, vec![T, T, R], "壊れた尻尾が本体へ適用された");
        assert!(tail.is_empty());
    }

    /// 保存から読んだ尻尾の検証。
    #[test]
    fn sanitize_tail_clamps_and_rejects() {
        // 本体 30 + 尻尾 5 → 2 個ぶんしか入らない。
        assert_eq!(sanitize_tail("|||||", 30), "||");
        // 本体が上限なら尻尾は入らない。
        assert_eq!(sanitize_tail("|||", MAX_STEPS), "");
        // 未知の文字を含むものは丸ごと捨てる（部分適用しない）。
        assert_eq!(sanitize_tail("||x", 4), "");
        // 正常なものはそのまま通る。
        assert_eq!(sanitize_tail("|-_", 4), "|-_");
    }
}
