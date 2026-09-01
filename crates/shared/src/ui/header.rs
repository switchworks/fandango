//! ロゴ + byline + timestamp のヘッダー行。全プラグイン共通。

use nih_plug::prelude::{BoolParam, ParamSetter};
use nih_plug_egui::egui::{
    self, Align, Color32, FontDefinitions, FontFamily, Layout, RichText, TextureHandle, Vec2,
};


/// UI 全体で egui 標準の等幅フォントを使う。
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let monospace = fonts
        .families
        .get(&FontFamily::Monospace)
        .expect("egui default monospace family must exist")
        .clone();
    fonts.families.insert(FontFamily::Proportional, monospace);
    ctx.set_fonts(fonts);
}

/// ── 上段: 左にロゴ (+ プラグイン名)、右に (by switchworks + timestamp) ──
pub fn header_row(
    ui: &mut egui::Ui,
    logo: Option<&TextureHandle>,
    plugin_label: &str,
    build_timestamp: &'static str,
    bypass: Option<(&BoolParam, &ParamSetter)>,
) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            if let Some(tex) = logo {
                let sz = tex.size_vec2();
                let logo_w = 180.0_f32;
                let logo_h = sz.y * (logo_w / sz.x);
                ui.add(
                    egui::Image::from_texture(tex).fit_to_exact_size(Vec2::new(logo_w, logo_h)),
                );
            }
            ui.label(
                RichText::new(plugin_label).monospace().size(16.0),
            );
        });
        ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
            if let Some((param, setter)) = bypass {
                bypass_button(ui, param, setter);
            }
            ui.add_space(6.0);
            ui.vertical(|ui| {
                ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                    ui.label(RichText::new("by switchworks").weak().size(10.0));
                });
                ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                    ui.label(
                        RichText::new(build_timestamp)
                            .weak()
                            .monospace()
                            .size(9.0),
                    );
                });
            });
        });
    });
}

/// LED ドット + "BYPASS" ラベル常時表示。状態は色のみで切替
/// (緑=動作中/赤=バイパス中)、ラベル自体は常に見えるため見落としにくい。
pub(crate) fn bypass_button(ui: &mut egui::Ui, param: &BoolParam, setter: &ParamSetter) {
    let bypassed = param.value();
    let (led_color, text_color) = if bypassed {
        (Color32::from_rgb(0xE5, 0x34, 0x2E), Color32::WHITE)
    } else {
        (Color32::from_rgb(0x4C, 0xAF, 0x50), ui.visuals().weak_text_color())
    };

    let desired_size = Vec2::new(66.0, 20.0);
    let (rect, resp) = ui.allocate_exact_size(desired_size, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let dot_r = 4.0;
        let dot_center = egui::pos2(rect.left() + dot_r + 2.0, rect.center().y);
        painter.circle_filled(dot_center, dot_r, led_color);
        painter.text(
            egui::pos2(dot_center.x + dot_r + 6.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            "BYPASS",
            egui::FontId::monospace(12.0),
            text_color,
        );
    }
    if resp.on_hover_text("click to toggle bypass").clicked() {
        setter.begin_set_parameter(param);
        setter.set_parameter(param, !bypassed);
        setter.end_set_parameter(param);
    }
}
