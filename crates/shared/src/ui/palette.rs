//! Radix Colors パレット (CC0)。全プラグイン共通の配色システム。

use nih_plug_egui::egui::{self, Color32, Stroke};

/// Radix Colors dark scale。step 1 (app bg) → step 12 (high-contrast text)。
pub type RadixScale = [Color32; 12];

/// 中性 scale + アクセント scale のペア。
#[derive(Copy, Clone)]
pub struct PaletteData {
    pub neutral: RadixScale,
    pub accent: RadixScale,
}

/// crate 固有のアクセント色。中性 scale は常に gray-dark。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Palette {
    Amber,
    Blue,
    Crimson,
    Cyan,
    Grass,
    Indigo,
    Jade,
    Orange,
    Plum,
    Teal,
    Tomato,
    Violet,
}

impl Palette {
    pub fn data(self) -> PaletteData {
        PaletteData {
            neutral: GRAY_DARK,
            accent: match self {
                Palette::Amber => AMBER_DARK,
                Palette::Blue => BLUE_DARK,
                Palette::Crimson => CRIMSON_DARK,
                Palette::Cyan => CYAN_DARK,
                Palette::Grass => GRASS_DARK,
                Palette::Indigo => INDIGO_DARK,
                Palette::Jade => JADE_DARK,
                Palette::Orange => ORANGE_DARK,
                Palette::Plum => PLUM_DARK,
                Palette::Teal => TEAL_DARK,
                Palette::Tomato => TOMATO_DARK,
                Palette::Violet => VIOLET_DARK,
            },
        }
    }
}

/// Radix Colors `gray-dark` 12 step。crate 固有の固定アクセント向け無彩色ベース。
const GRAY_DARK: RadixScale = [
    Color32::from_rgb(0x11, 0x11, 0x11),
    Color32::from_rgb(0x19, 0x19, 0x19),
    Color32::from_rgb(0x22, 0x22, 0x22),
    Color32::from_rgb(0x2A, 0x2A, 0x2A),
    Color32::from_rgb(0x31, 0x31, 0x31),
    Color32::from_rgb(0x3A, 0x3A, 0x3A),
    Color32::from_rgb(0x48, 0x48, 0x48),
    Color32::from_rgb(0x60, 0x60, 0x60),
    Color32::from_rgb(0x6E, 0x6E, 0x6E),
    Color32::from_rgb(0x7B, 0x7B, 0x7B),
    Color32::from_rgb(0xB4, 0xB4, 0xB4),
    Color32::from_rgb(0xEE, 0xEE, 0xEE),
];

/// Radix Colors `amber-dark` 12 step。https://www.radix-ui.com/colors
const AMBER_DARK: RadixScale = [
    Color32::from_rgb(0x16, 0x12, 0x0C),
    Color32::from_rgb(0x1D, 0x18, 0x0F),
    Color32::from_rgb(0x30, 0x20, 0x08),
    Color32::from_rgb(0x3F, 0x27, 0x00),
    Color32::from_rgb(0x4D, 0x30, 0x00),
    Color32::from_rgb(0x5C, 0x3D, 0x05),
    Color32::from_rgb(0x71, 0x4F, 0x19),
    Color32::from_rgb(0x8F, 0x64, 0x24),
    Color32::from_rgb(0xFF, 0xC5, 0x3D),
    Color32::from_rgb(0xFF, 0xD6, 0x0A),
    Color32::from_rgb(0xFF, 0xCA, 0x16),
    Color32::from_rgb(0xFF, 0xE7, 0xB3),
];

/// Radix Colors `blue-dark` 12 step。nimbus の固定アクセント。
const BLUE_DARK: RadixScale = [
    Color32::from_rgb(0x0D, 0x15, 0x20),
    Color32::from_rgb(0x11, 0x19, 0x27),
    Color32::from_rgb(0x0D, 0x28, 0x47),
    Color32::from_rgb(0x00, 0x33, 0x62),
    Color32::from_rgb(0x00, 0x40, 0x74),
    Color32::from_rgb(0x10, 0x4D, 0x87),
    Color32::from_rgb(0x20, 0x5D, 0x9E),
    Color32::from_rgb(0x28, 0x70, 0xBD),
    Color32::from_rgb(0x00, 0x90, 0xFF),
    Color32::from_rgb(0x3B, 0x9E, 0xFF),
    Color32::from_rgb(0x70, 0xB8, 0xFF),
    Color32::from_rgb(0xC2, 0xE6, 0xFF),
];

/// Radix Colors `crimson-dark` 12 step。https://www.radix-ui.com/colors
const CRIMSON_DARK: RadixScale = [
    Color32::from_rgb(0x19, 0x11, 0x14),
    Color32::from_rgb(0x20, 0x13, 0x18),
    Color32::from_rgb(0x38, 0x15, 0x25),
    Color32::from_rgb(0x4D, 0x12, 0x2F),
    Color32::from_rgb(0x5C, 0x18, 0x39),
    Color32::from_rgb(0x6D, 0x25, 0x45),
    Color32::from_rgb(0x87, 0x33, 0x56),
    Color32::from_rgb(0xB0, 0x43, 0x6E),
    Color32::from_rgb(0xE9, 0x3D, 0x82),
    Color32::from_rgb(0xEE, 0x51, 0x8A),
    Color32::from_rgb(0xFF, 0x92, 0xAD),
    Color32::from_rgb(0xFD, 0xD3, 0xE8),
];

/// Radix Colors `cyan-dark` 12 step。https://www.radix-ui.com/colors
const CYAN_DARK: RadixScale = [
    Color32::from_rgb(0x0B, 0x16, 0x1A),
    Color32::from_rgb(0x10, 0x1B, 0x20),
    Color32::from_rgb(0x08, 0x2C, 0x36),
    Color32::from_rgb(0x00, 0x38, 0x48),
    Color32::from_rgb(0x00, 0x45, 0x58),
    Color32::from_rgb(0x04, 0x54, 0x68),
    Color32::from_rgb(0x12, 0x67, 0x7E),
    Color32::from_rgb(0x11, 0x80, 0x9C),
    Color32::from_rgb(0x00, 0xA2, 0xC7),
    Color32::from_rgb(0x23, 0xAF, 0xD0),
    Color32::from_rgb(0x4C, 0xCC, 0xE6),
    Color32::from_rgb(0xB6, 0xEC, 0xF7),
];

/// Radix Colors `grass-dark` 12 step。https://www.radix-ui.com/colors
const GRASS_DARK: RadixScale = [
    Color32::from_rgb(0x0E, 0x15, 0x11),
    Color32::from_rgb(0x14, 0x1A, 0x15),
    Color32::from_rgb(0x1B, 0x2A, 0x1E),
    Color32::from_rgb(0x1D, 0x3A, 0x24),
    Color32::from_rgb(0x25, 0x48, 0x2D),
    Color32::from_rgb(0x2D, 0x57, 0x36),
    Color32::from_rgb(0x36, 0x67, 0x40),
    Color32::from_rgb(0x3E, 0x79, 0x49),
    Color32::from_rgb(0x46, 0xA7, 0x58),
    Color32::from_rgb(0x53, 0xB3, 0x65),
    Color32::from_rgb(0x71, 0xD0, 0x83),
    Color32::from_rgb(0xC2, 0xF0, 0xC2),
];

/// Radix Colors `indigo-dark` 12 step。https://www.radix-ui.com/colors
const INDIGO_DARK: RadixScale = [
    Color32::from_rgb(0x11, 0x13, 0x1F),
    Color32::from_rgb(0x14, 0x17, 0x26),
    Color32::from_rgb(0x18, 0x24, 0x49),
    Color32::from_rgb(0x1D, 0x2E, 0x62),
    Color32::from_rgb(0x25, 0x39, 0x74),
    Color32::from_rgb(0x30, 0x43, 0x84),
    Color32::from_rgb(0x3A, 0x4F, 0x97),
    Color32::from_rgb(0x43, 0x5D, 0xB1),
    Color32::from_rgb(0x3E, 0x63, 0xDD),
    Color32::from_rgb(0x54, 0x72, 0xE4),
    Color32::from_rgb(0x9E, 0xB1, 0xFF),
    Color32::from_rgb(0xD6, 0xE1, 0xFF),
];

/// Radix Colors `jade-dark` 12 step。https://www.radix-ui.com/colors
const JADE_DARK: RadixScale = [
    Color32::from_rgb(0x0D, 0x15, 0x12),
    Color32::from_rgb(0x12, 0x1C, 0x18),
    Color32::from_rgb(0x0F, 0x2E, 0x22),
    Color32::from_rgb(0x0B, 0x3B, 0x2C),
    Color32::from_rgb(0x11, 0x48, 0x37),
    Color32::from_rgb(0x1B, 0x57, 0x45),
    Color32::from_rgb(0x24, 0x68, 0x54),
    Color32::from_rgb(0x2A, 0x7E, 0x68),
    Color32::from_rgb(0x29, 0xA3, 0x83),
    Color32::from_rgb(0x27, 0xB0, 0x8B),
    Color32::from_rgb(0x1F, 0xD8, 0xA4),
    Color32::from_rgb(0xAD, 0xF0, 0xD4),
];

/// Radix Colors `orange-dark` 12 step。https://www.radix-ui.com/colors
const ORANGE_DARK: RadixScale = [
    Color32::from_rgb(0x17, 0x12, 0x0E),
    Color32::from_rgb(0x1E, 0x16, 0x0F),
    Color32::from_rgb(0x33, 0x1E, 0x0B),
    Color32::from_rgb(0x46, 0x21, 0x00),
    Color32::from_rgb(0x56, 0x28, 0x00),
    Color32::from_rgb(0x66, 0x35, 0x0C),
    Color32::from_rgb(0x7E, 0x45, 0x1D),
    Color32::from_rgb(0xA3, 0x58, 0x29),
    Color32::from_rgb(0xF7, 0x6B, 0x15),
    Color32::from_rgb(0xFF, 0x80, 0x1F),
    Color32::from_rgb(0xFF, 0xA0, 0x57),
    Color32::from_rgb(0xFF, 0xE0, 0xC2),
];

/// Radix Colors `plum-dark` 12 step。https://www.radix-ui.com/colors
const PLUM_DARK: RadixScale = [
    Color32::from_rgb(0x18, 0x11, 0x18),
    Color32::from_rgb(0x20, 0x13, 0x20),
    Color32::from_rgb(0x35, 0x1A, 0x35),
    Color32::from_rgb(0x45, 0x1D, 0x47),
    Color32::from_rgb(0x51, 0x24, 0x54),
    Color32::from_rgb(0x5E, 0x30, 0x61),
    Color32::from_rgb(0x73, 0x40, 0x79),
    Color32::from_rgb(0x92, 0x54, 0x9C),
    Color32::from_rgb(0xAB, 0x4A, 0xBA),
    Color32::from_rgb(0xB6, 0x58, 0xC4),
    Color32::from_rgb(0xE7, 0x96, 0xF3),
    Color32::from_rgb(0xF4, 0xD4, 0xF4),
];

/// Radix Colors `teal-dark` 12 step。https://www.radix-ui.com/colors
const TEAL_DARK: RadixScale = [
    Color32::from_rgb(0x0D, 0x15, 0x14),
    Color32::from_rgb(0x11, 0x1C, 0x1B),
    Color32::from_rgb(0x0D, 0x2D, 0x2A),
    Color32::from_rgb(0x02, 0x3B, 0x37),
    Color32::from_rgb(0x08, 0x48, 0x43),
    Color32::from_rgb(0x14, 0x57, 0x50),
    Color32::from_rgb(0x1C, 0x69, 0x61),
    Color32::from_rgb(0x20, 0x7E, 0x73),
    Color32::from_rgb(0x12, 0xA5, 0x94),
    Color32::from_rgb(0x0E, 0xB3, 0x9E),
    Color32::from_rgb(0x0B, 0xD8, 0xB6),
    Color32::from_rgb(0xAD, 0xF0, 0xDD),
];

/// Radix Colors `tomato-dark` 12 step。https://www.radix-ui.com/colors
const TOMATO_DARK: RadixScale = [
    Color32::from_rgb(0x18, 0x11, 0x11),
    Color32::from_rgb(0x1F, 0x15, 0x13),
    Color32::from_rgb(0x39, 0x17, 0x14),
    Color32::from_rgb(0x4E, 0x15, 0x11),
    Color32::from_rgb(0x5E, 0x1C, 0x16),
    Color32::from_rgb(0x6E, 0x29, 0x20),
    Color32::from_rgb(0x85, 0x3A, 0x2D),
    Color32::from_rgb(0xAC, 0x4D, 0x39),
    Color32::from_rgb(0xE5, 0x4D, 0x2E),
    Color32::from_rgb(0xEC, 0x61, 0x42),
    Color32::from_rgb(0xFF, 0x97, 0x7D),
    Color32::from_rgb(0xFB, 0xD3, 0xCB),
];

/// Radix Colors `violet-dark` 12 step。https://www.radix-ui.com/colors
const VIOLET_DARK: RadixScale = [
    Color32::from_rgb(0x14, 0x12, 0x1F),
    Color32::from_rgb(0x1B, 0x15, 0x25),
    Color32::from_rgb(0x29, 0x1F, 0x43),
    Color32::from_rgb(0x33, 0x25, 0x5B),
    Color32::from_rgb(0x3C, 0x2E, 0x69),
    Color32::from_rgb(0x47, 0x38, 0x76),
    Color32::from_rgb(0x56, 0x46, 0x8B),
    Color32::from_rgb(0x69, 0x58, 0xAD),
    Color32::from_rgb(0x6E, 0x56, 0xCF),
    Color32::from_rgb(0x7D, 0x66, 0xD9),
    Color32::from_rgb(0xBA, 0xA7, 0xFF),
    Color32::from_rgb(0xE2, 0xDD, 0xFE),
];

/// Radix step の意味論を Visuals にマップ。
/// neutral: 1-2=bg (少し明るく step 2 を panel), 3-5=UI element、10-11=text
/// accent: 4-5=薄背景、8-9=solid border → ツマミ枠・選択強調で palette 差を可視化
pub fn apply_palette(ctx: &egui::Context, palette: Palette) {
    let PaletteData {
        neutral: n,
        accent: a,
    } = palette.data();
    let mut v = egui::Visuals::dark();
    // 全体 bg は step 2 に上げる (step 1 は真っ黒すぎて palette 差が消える)。
    v.panel_fill = n[1];
    v.window_fill = n[1];
    v.extreme_bg_color = n[0];
    v.override_text_color = Some(n[11]);

    v.widgets.noninteractive.weak_bg_fill = n[1];
    v.widgets.noninteractive.bg_fill = n[2];
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, n[5]);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, n[10]);

    // ボタン等の非アクティブ状態: 枠のみ accent、面は中性 → 常時 palette hue を認識させる。
    v.widgets.inactive.weak_bg_fill = n[2];
    v.widgets.inactive.bg_fill = n[3];
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, a[8]);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, n[10]);

    v.widgets.hovered.weak_bg_fill = n[3];
    v.widgets.hovered.bg_fill = a[4];
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, a[8]);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, n[11]);

    // ドラッグ中: 面 も accent。ツマミ引っ張ると温↔寒色ハッキリ。
    v.widgets.active.weak_bg_fill = a[4];
    v.widgets.active.bg_fill = a[5];
    v.widgets.active.bg_stroke = Stroke::new(1.0, a[9]);
    v.widgets.active.fg_stroke = Stroke::new(1.0, n[11]);

    v.widgets.open.weak_bg_fill = n[3];
    v.widgets.open.bg_fill = a[4];
    v.widgets.open.bg_stroke = Stroke::new(1.0, a[7]);
    v.widgets.open.fg_stroke = Stroke::new(1.0, n[11]);

    // Character 選択・popup 選択の背景 = accent[5] (色つきで palette 差クリア)。
    v.selection.bg_fill = a[5];
    v.selection.stroke = Stroke::new(1.0, a[9]);

    ctx.set_visuals(v);
}
