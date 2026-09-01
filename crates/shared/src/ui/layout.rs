//! 全 crate 共通の 800×400・3 帯レイアウト。

use nih_plug::prelude::{BoolParam, ParamSetter};
use nih_plug_egui::egui::{
    self, Align, Align2, FontId, Layout, Rect, Sense, UiBuilder,
};

use super::background::Background;
use super::formula::{paint_scripted, Script};
use super::header::bypass_button;

pub const WINDOW_W: u32 = 800;
pub const WINDOW_H: u32 = 400;
pub const OUTER_MARGIN: f32 = 8.0;
pub const HEADER_H: f32 = 40.0;
pub const MAIN_H: f32 = 224.0;
pub const FOOTER_H: f32 = 120.0;

pub struct Shell<'a> {
    pub wordmark: &'a str,
    pub formula: &'a [(&'a str, Script)],
    pub kind: &'a str,
    pub bypass: Option<(&'a BoolParam, &'a ParamSetter<'a>)>,
}

/// 上下外余白 8px、ヘッダ 40 / メイン 224 / フッタ 120。
pub fn show_shell(
    ctx: &egui::Context,
    shell: Shell<'_>,
    add_main: impl FnMut(&mut egui::Ui, Rect),
    add_footer: impl FnMut(&mut egui::Ui),
) {
    show_shell_with_background(ctx, shell, None, add_main, add_footer);
}

/// [`show_shell`] に crate 固有の背景画像を敷いた版。
///
/// 背景を持たない crate は [`show_shell`] を呼ぶ（`background = None` と同義）。
pub fn show_shell_with_background(
    ctx: &egui::Context,
    shell: Shell<'_>,
    background: Option<&mut Background>,
    mut add_main: impl FnMut(&mut egui::Ui, Rect),
    mut add_footer: impl FnMut(&mut egui::Ui),
) {
    let fill = ctx.style().visuals.panel_fill;
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(fill))
        .show(ctx, |ui| {
            let content = ui.max_rect().shrink(OUTER_MARGIN);
            // 背景はウィジェットより先に敷く。メイン領域の矩形は定数から決まるので
            // allocate を待たずに求められる。
            if let Some(background) = background {
                let main = Rect::from_min_size(
                    egui::pos2(content.left(), content.top() + HEADER_H),
                    egui::vec2(content.width(), MAIN_H),
                );
                background.paint(ui, ui.max_rect(), main);
            }
            ui.allocate_new_ui(
                UiBuilder::new()
                    .max_rect(content)
                    .layout(Layout::top_down(Align::Min)),
                |ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                    let w = content.width();
                    let (header_rect, _) =
                        ui.allocate_exact_size(egui::vec2(w, HEADER_H), Sense::hover());
                    draw_header(ui, header_rect, &shell);

                    let (main_rect, _) =
                        ui.allocate_exact_size(egui::vec2(w, MAIN_H), Sense::hover());
                    add_main(ui, main_rect);

                    let (footer_rect, _) =
                        ui.allocate_exact_size(egui::vec2(w, FOOTER_H), Sense::hover());
                    ui.allocate_new_ui(
                        UiBuilder::new()
                            .max_rect(footer_rect)
                            .layout(Layout::top_down(Align::Min)),
                        |ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(8.0, 4.0);
                            add_footer(ui);
                        },
                    );
                },
            );
        });
}

fn draw_header(ui: &mut egui::Ui, rect: Rect, shell: &Shell<'_>) {
    let strong = ui.visuals().strong_text_color();
    let weak = ui.visuals().weak_text_color();
    {
        let painter = ui.painter_at(rect);
        painter.text(
            rect.left_center(),
            Align2::LEFT_CENTER,
            shell.wordmark,
            FontId::monospace(16.0),
            strong,
        );
        let bypass_w = if shell.bypass.is_some() { 74.0 } else { 0.0 };
        if !shell.formula.is_empty() {
            // 左のワードマークと右の種別 + bypass に挟まれた幅へ収める。
            // 中央寄せなので、左右の余裕の小さい方の 2 倍が使える幅。
            let gap = 12.0;
            let wordmark_w = painter
                .layout_no_wrap(shell.wordmark.to_owned(), FontId::monospace(16.0), weak)
                .size()
                .x;
            let kind_w = if shell.kind.is_empty() {
                0.0
            } else {
                painter
                    .layout_no_wrap(shell.kind.to_owned(), FontId::monospace(16.0), weak)
                    .size()
                    .x
            };
            let center_x = rect.center().x;
            let left_room = center_x - (rect.left() + wordmark_w + gap);
            let right_room = (rect.right() - bypass_w - kind_w - gap) - center_x;
            let max_w = 2.0 * left_room.min(right_room).max(0.0);
            paint_scripted(&painter, rect.center(), shell.formula, 16.0, weak, max_w);
        }
        if !shell.kind.is_empty() {
            painter.text(
                egui::pos2(rect.right() - bypass_w, rect.center().y),
                Align2::RIGHT_CENTER,
                shell.kind,
                FontId::monospace(16.0),
                weak,
            );
        }
    }
    if let Some((param, setter)) = shell.bypass {
        let bypass_rect = Rect::from_min_size(
            egui::pos2(rect.right() - 66.0, rect.center().y - 10.0),
            egui::vec2(66.0, 20.0),
        );
        ui.allocate_new_ui(UiBuilder::new().max_rect(bypass_rect), |ui| {
            bypass_button(ui, param, setter);
        });
    }
}
