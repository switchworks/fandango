//! 各 crate 固有の背景画像と、その上に敷く暗幕。
//!
//! 図式は `plans/gui_overhaul_v01.md` の背景試行 F 案:
//! - 全面に一様な暗幕
//! - ウィジェットが載る帯（ヘッダ・フッタ）へさらに暗幕を重ね、文字とスライダーの
//!   可読性を背景の明暗から切り離す
//! - メイン領域は上下端が濃く中央が薄い縦グラデーション。主図がここへ上乗せされる
//!
//! **画像側でコントラストを落とさない。** 元画像のまま持ち、暗さはここの定数だけで
//! 決める。焼き込むと調整のたびに画像編集へ戻ることになる。

use nih_plug_egui::egui::{self, Color32, Mesh, Rect, TextureHandle, TextureOptions};

/// 全面の暗幕。
const BASE_VEIL: f32 = 0.45;
/// ヘッダ・フッタ帯へ追加で重ねる暗幕。
const BAND_VEIL: f32 = 0.50;
/// メイン領域の中央（最も薄いところ）。
const MAIN_VEIL_MIN: f32 = 0.20;
/// メイン領域の上下端（最も濃いところ）。帯と揃えて段差を消す。
const MAIN_VEIL_MAX: f32 = BAND_VEIL;

/// 背景画像 1 枚分。デコード結果はテクスチャとして保持する。
///
/// **`build` コールバックから [`Self::invalidate`] を必ず呼ぶこと。**
/// `nih_plug_egui` の `user_state` はエディタを閉じても保持されるが `egui::Context` は
/// ウィンドウごとに作り直されるため、持ち越したテクスチャは再オープン後に描画されない
/// （`crates/shared/src/ui/relief.rs` の同じ注記を参照）。
pub struct Background {
    bytes: &'static [u8],
    texture: Option<TextureHandle>,
    /// 暗幕全体に掛かる係数。1.0 = 上の定数どおり。元画像が暗い crate 向けに下げる。
    veil_scale: f32,
}

impl Background {
    /// `include_bytes!` した PNG / JPEG を受け取る。デコードは初回描画時に一度だけ。
    pub const fn new(bytes: &'static [u8]) -> Self {
        Self {
            bytes,
            texture: None,
            veil_scale: 1.0,
        }
    }

    /// 暗幕を `scale` 倍にする。元画像の平均輝度が他 crate より低い場合に使う
    /// （画像側でコントラストを持ち上げない、という上の方針の裏返し）。
    pub fn with_veil_scale(mut self, scale: f32) -> Self {
        self.veil_scale = scale;
        self
    }

    /// 保持しているテクスチャを捨てる。`build` から呼ぶ。
    pub fn invalidate(&mut self) {
        self.texture = None;
    }

    /// 背景画像と暗幕を `rect` 全体へ描く。`main` はメイン領域の矩形。
    pub fn paint(&mut self, ui: &egui::Ui, rect: Rect, main: Rect) {
        let texture = self.texture.get_or_insert_with(|| {
            let image = decode(self.bytes);
            ui.ctx()
                .load_texture("fandango-background", image, TextureOptions::LINEAR)
        });

        let painter = ui.painter();
        painter.image(
            texture.id(),
            rect,
            Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        painter.rect_filled(rect, 0.0, veil_color(BASE_VEIL * self.veil_scale));

        // ヘッダ帯とフッタ帯。メイン領域の外側すべてを覆う。
        let above = Rect::from_min_max(rect.min, egui::pos2(rect.max.x, main.min.y));
        let below = Rect::from_min_max(egui::pos2(rect.min.x, main.max.y), rect.max);
        painter.rect_filled(above, 0.0, veil_color(BAND_VEIL * self.veil_scale));
        painter.rect_filled(below, 0.0, veil_color(BAND_VEIL * self.veil_scale));

        // メイン領域は縦グラデーション。上半分と下半分で 2 枚に分けて張る。
        let middle = main.center().y;
        vertical_gradient(
            painter,
            Rect::from_min_max(main.min, egui::pos2(main.max.x, middle)),
            MAIN_VEIL_MAX * self.veil_scale,
            MAIN_VEIL_MIN * self.veil_scale,
        );
        vertical_gradient(
            painter,
            Rect::from_min_max(egui::pos2(main.min.x, middle), main.max),
            MAIN_VEIL_MIN * self.veil_scale,
            MAIN_VEIL_MAX * self.veil_scale,
        );
    }
}

#[inline]
fn veil_color(alpha: f32) -> Color32 {
    Color32::from_black_alpha((alpha * 255.0).round() as u8)
}

/// 上端 `top` から下端 `bottom` へ不透明度が変わる黒の矩形。
fn vertical_gradient(painter: &egui::Painter, rect: Rect, top: f32, bottom: f32) {
    let mut mesh = Mesh::default();
    let (top_color, bottom_color) = (veil_color(top), veil_color(bottom));
    mesh.colored_vertex(rect.left_top(), top_color);
    mesh.colored_vertex(rect.right_top(), top_color);
    mesh.colored_vertex(rect.left_bottom(), bottom_color);
    mesh.colored_vertex(rect.right_bottom(), bottom_color);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 2, 3);
    painter.add(mesh);
}

/// デコード失敗時は 1x1 透明で縮退する（ロゴと同じ方針。パニックさせない）。
fn decode(bytes: &[u8]) -> egui::ColorImage {
    match image::load_from_memory(bytes) {
        Ok(img) => {
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], rgba.as_raw())
        }
        Err(_) => egui::ColorImage::new([1, 1], Color32::TRANSPARENT),
    }
}
