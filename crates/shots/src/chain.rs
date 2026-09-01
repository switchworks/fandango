//! チェーン表示 (型 C, `plans/promo_video_v01.md` §5.5 C9)。
//!
//! 4 crate を 2×2 の等倍で 1920×1080 に並べる。全 GUI が 800×400 で統一されているため
//! 800×2 + 間隔 64 = 1664、400×2 + 64 = 864 となり、余白 128/108 で収まる。
//! 型 A (1 枚を 2 倍 = 1600×800) とほぼ同じ占有面積になる。
//!
//! **タイル (`tile.rs`) と違って縮小しない。** 等倍なので GUI の描画結果がそのまま出る
//! (不変条件 2: 既存 GUI の描画結果を 1 px も変えない)。
//!
//! 段は時刻で 1 枚ずつ増える。まだ挿さっていない枠は**何も描かない**（地のまま）。

use std::collections::BTreeMap;

use image::{Rgba, RgbaImage};

use crate::offscreen::Renderer;
use crate::targets::Target;

pub const CANVAS_W: u32 = 1920;
pub const CANVAS_H: u32 = 1080;

/// GUI 1 枚の大きさ。等倍。
pub const CELL_W: u32 = 800;
pub const CELL_H: u32 = 400;

/// 2×2 の間隔。
pub const GAP: u32 = 64;

pub const COLS: u32 = 2;
pub const ROWS: u32 = 2;

/// 左上の原点。(1920 - (800×2 + 64)) / 2 = 128、(1080 - (400×2 + 64)) / 2 = 108。
pub const ORIGIN_X: u32 = (CANVAS_W - (CELL_W * COLS + GAP)) / 2;
pub const ORIGIN_Y: u32 = (CANVAS_H - (CELL_H * ROWS + GAP)) / 2;

/// チェーンの 1 段。
pub struct Stage {
    pub target: Target,
    /// この段が画面に現れる時刻 [秒]。
    pub at: f32,
    /// id → 正規化値。C9 は掃引しないので固定値のみ。
    pub values: BTreeMap<String, f32>,
    /// `#[persist]` の文字列状態 (gate のステップパターン)。
    pub texts: BTreeMap<String, String>,
}

/// 読み順 (1 行 1 列 → 1 行 2 列 → 2 行 1 列 → 2 行 2 列) の左上座標。
fn slot_origin(i: usize) -> (u32, u32) {
    let col = i as u32 % COLS;
    let row = i as u32 / COLS;
    (
        ORIGIN_X + col * (CELL_W + GAP),
        ORIGIN_Y + row * (CELL_H + GAP),
    )
}

/// 時刻 `t` の 1 フレームを描く。`t` 以前に `at` を迎えた段だけが出る。
pub fn frame_at(
    renderer: &mut Renderer,
    stages: &[Stage],
    background: Rgba<u8>,
    t: f32,
) -> RgbaImage {
    assert!(
        stages.len() as u32 <= COLS * ROWS,
        "チェーンは {COLS}×{ROWS} = {} 段まで。段を増やすならレイアウトを決め直すこと",
        COLS * ROWS
    );

    let mut canvas = RgbaImage::from_pixel(CANVAS_W, CANVAS_H, background);

    for (i, stage) in stages.iter().enumerate() {
        if t < stage.at {
            continue;
        }
        let (w, h) = stage.target.window_size();
        assert_eq!(
            (w, h),
            (CELL_W, CELL_H),
            "{} の GUI が {CELL_W}×{CELL_H} でない。2×2 等倍のレイアウトが崩れる",
            stage.target.name()
        );
        renderer.resize(w, h);
        let mut scene = stage.target.scene_with_text(&stage.values, &stage.texts);
        let shot = renderer.frame(scene.as_mut());
        let (x, y) = slot_origin(i);
        image::imageops::overlay(&mut canvas, &shot, x as i64, y as i64);
    }

    canvas
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_fit_inside_the_canvas() {
        for i in 0..(COLS * ROWS) as usize {
            let (x, y) = slot_origin(i);
            assert!(x + CELL_W <= CANVAS_W, "スロット {i} が右へはみ出す");
            assert!(y + CELL_H <= CANVAS_H, "スロット {i} が下へはみ出す");
        }
    }

    #[test]
    fn origin_is_centred() {
        assert_eq!(ORIGIN_X, 128);
        assert_eq!(ORIGIN_Y, 108);
    }
}
