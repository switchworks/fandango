//! spread 用 egui エディタ実装。
//!
//! 主図は M/S ゲイン楕円。行列 `[[a, c], [c, a]]` は
//! M/S 基底で対角化され、固有値が `alpha = a + c`（Mid）/ `beta = a - c`（Side）に
//! なる。無相関ステレオ単位入力の像は縦半径 alpha・横半径 beta の楕円である。
//!
//! 半径は必ず [`Coefs::from_params`] から求める。GUI 側で閉形式解を書き直さない
//! （閉形式は dsp 側の単一実装に集約する）。

use std::sync::Arc;

use nih_plug::prelude::{Editor, Param, ParamSetter};
use nih_plug_egui::egui::{self, Align2, Color32, FontId, Pos2, Rect, Stroke};
use nih_plug_egui::{create_egui_editor, EguiState};

use fandango_shared::ui::{
    apply_palette, bead_slider_row, choice_row, formula, install_fonts, show_shell_with_background,
    Background, Palette, Shell,
};

use crate::{dsp::Coefs, BassPreserve, SpreadParams};

pub const WINDOW_W: u32 = 800;
pub const WINDOW_H: u32 = 400;

/// crate 固有の背景画像。暗さは `Background` 側の定数だけで決める（焼き込まない）。
const BACKGROUND: &[u8] = include_bytes!("../../../assets/backgrounds/spread.jpg");

/// identity（真円）の半径。メイン領域高さに対する比。見た目係数。
const UNIT_RADIUS_RATIO: f32 = 0.30;
/// 楕円の輪郭の分割数。
const ELLIPSE_SEGMENTS: usize = 96;

/// エディタが持ち越す状態。Context 依存のテクスチャを抱えるので `build` で必ず捨てる。
pub struct EditorState {
    background: Background,
}

/// ウィンドウのタイトル帯に出す表記。
pub const PLUGIN_LABEL: &str = "fandango : spread";

pub fn new_editor_state() -> EditorState {
    EditorState {
        background: Background::new(BACKGROUND),
    }
}

/// ウィンドウを開くたびに **新しい Context** で呼ばれる。
/// 持ち越したテクスチャは古い Context のものなので必ず捨てる。
pub fn build(ctx: &egui::Context, state: &mut EditorState) {
    install_fonts(ctx);
    state.background.invalidate();
}

pub fn draw(
    ctx: &egui::Context,
    setter: &ParamSetter,
    params: &SpreadParams,
    state: &mut EditorState,
) {
    apply_palette(ctx, Palette::Cyan);
    let palette = Palette::Cyan.data();
    let EditorState { background } = state;
    show_shell_with_background(
        ctx,
        Shell {
            wordmark: PLUGIN_LABEL,
            formula: formula::SPREAD,
            kind: "WIDTH MATRIX",
            bypass: Some((&params.bypass, setter)),
        },
        Some(background),
        |ui, rect| {
            let coefs = Coefs::from_params(
                params.expander.modulated_plain_value(),
                params.center_amount.modulated_plain_value(),
            );
            paint_field(
                ui,
                rect,
                FieldInput {
                    mid: coefs.a + coefs.c,
                    side: coefs.a - coefs.c,
                    bass_step: bass_step(params.bass_preserve.value()),
                    inverse: params.inverse.value(),
                    bypass: params.bypass.value(),
                    accent: palette.accent[8],
                    highlight: palette.accent[10],
                    guide: palette.neutral[5],
                    label: palette.neutral[9],
                },
            );
        },
        |ui| {
            ui.columns(2, |cols| {
                bead_slider_row(&mut cols[0], "expander", &params.expander, setter);
                bead_slider_row(&mut cols[1], "center mix", &params.center_amount, setter);
            });
            choice_row(
                ui,
                "inverse",
                &params.inverse,
                setter,
                &[("normal", false), ("inverse", true)],
            );
            choice_row(
                ui,
                "bass preserve",
                &params.bass_preserve,
                setter,
                &[
                    ("off", BassPreserve::Off),
                    ("60 hz", BassPreserve::Hz60),
                    ("100 hz", BassPreserve::Hz100),
                    ("150 hz", BassPreserve::Hz150),
                    ("200 hz", BassPreserve::Hz200),
                ],
            );
        },
    );
}

pub fn create(
    params: Arc<SpreadParams>,
    editor_state: Arc<EguiState>,
    _build_timestamp: &'static str,
) -> Option<Box<dyn Editor>> {
    create_egui_editor(
        editor_state,
        new_editor_state(),
        |ctx, state: &mut EditorState| build(ctx, state),
        move |ctx, setter, state| draw(ctx, setter, &params, state),
    )
}

/// bass preserve の段。半径は表示上の順序記号であり、周波数から半径への物理写像は
/// 主張しない。
fn bass_step(bass: BassPreserve) -> u8 {
    match bass {
        BassPreserve::Off => 0,
        BassPreserve::Hz60 => 1,
        BassPreserve::Hz100 => 2,
        BassPreserve::Hz150 => 3,
        BassPreserve::Hz200 => 4,
    }
}

/// 主図 1 枚分の入力。半径は DSP 側で評価済みの値を受け取る。
struct FieldInput {
    /// Mid ゲイン `alpha`。楕円の縦半径（identity で 1.0）。
    mid: f32,
    /// Side ゲイン `beta`。楕円の横半径（identity で 1.0）。
    side: f32,
    /// bass preserve の段（0 = off、1..=4 が 60/100/150/200 Hz）。
    bass_step: u8,
    inverse: bool,
    bypass: bool,
    accent: Color32,
    highlight: Color32,
    guide: Color32,
    label: Color32,
}

fn paint_field(ui: &egui::Ui, rect: Rect, input: FieldInput) {
    let painter = ui.painter_at(rect);
    let center = rect.center();
    let unit = rect.height() * UNIT_RADIUS_RATIO;
    // bypass 中は処理が止まっている。形を identity へ差し替えず全体を減光する
    // （形で嘘をつかない）。
    let dim = |color: Color32| {
        if input.bypass {
            color.gamma_multiply(0.35)
        } else {
            color
        }
    };

    // identity のガイド円。現在の楕円がどちらへずれたかの基準。
    paint_ellipse(
        &painter,
        center,
        unit,
        unit,
        Stroke::new(1.0, dim(input.guide)),
    );

    // 低域が素通りする分（bass preserve）。行列を通らないので常に真円。
    if input.bass_step > 0 {
        let radius = unit * (0.20 + 0.06 * input.bass_step as f32);
        paint_ellipse(
            &painter,
            center,
            radius,
            radius,
            Stroke::new(1.0, dim(input.highlight.gamma_multiply(0.6))),
        );
    }

    // 本体。縦 = Mid、横 = Side。
    paint_ellipse(
        &painter,
        center,
        unit * input.side,
        unit * input.mid,
        Stroke::new(2.0, dim(input.accent)),
    );

    let font = FontId::monospace(12.0);
    let label = dim(input.label);
    painter.text(
        egui::pos2(center.x, rect.top() + 10.0),
        Align2::CENTER_TOP,
        "MID",
        font.clone(),
        label,
    );
    painter.text(
        egui::pos2(center.x, rect.bottom() - 10.0),
        Align2::CENTER_BOTTOM,
        "SIDE",
        font.clone(),
        label,
    );
    // inverse では L/R が入れ替わる。楕円は左右対称で形が変わらないため、
    // 左右のマーカーだけがこの状態を伝える。
    let (left, right) = if input.inverse {
        ("R", "L")
    } else {
        ("L", "R")
    };
    painter.text(
        egui::pos2(center.x - unit * 2.4, center.y),
        Align2::LEFT_CENTER,
        left,
        font.clone(),
        label,
    );
    painter.text(
        egui::pos2(center.x + unit * 2.4, center.y),
        Align2::RIGHT_CENTER,
        right,
        font,
        label,
    );
}

/// 半径 0 でも線分として描けるよう、閉じた折れ線で近似する。
fn paint_ellipse(painter: &egui::Painter, center: Pos2, rx: f32, ry: f32, stroke: Stroke) {
    let points: Vec<Pos2> = (0..=ELLIPSE_SEGMENTS)
        .map(|i| {
            let theta = i as f32 / ELLIPSE_SEGMENTS as f32 * std::f32::consts::TAU;
            egui::pos2(center.x + rx * theta.sin(), center.y - ry * theta.cos())
        })
        .collect();
    painter.add(egui::Shape::line(points, stroke));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 図の半径は必ずこの経路で求める（GUI 側で閉形式解を書き直さないことの担保）。
    fn radii(expander: f32, center_amount: f32) -> (f32, f32) {
        let k = Coefs::from_params(expander, center_amount);
        (k.a + k.c, k.a - k.c)
    }

    #[test]
    fn identity_is_a_circle() {
        let (mid, side) = radii(0.5, 0.0);
        assert!((mid - 1.0).abs() < 1e-6, "mid = {mid}");
        assert!((side - 1.0).abs() < 1e-6, "side = {side}");
    }

    #[test]
    fn mono_collapses_to_vertical_line() {
        let (mid, side) = radii(0.0, 0.0);
        assert!(side.abs() < 1e-6, "side = {side}");
        assert!(mid > 1.0, "mid = {mid}");
    }

    #[test]
    fn wide_without_center_mix_kills_mid() {
        let (mid, _side) = radii(1.0, 0.0);
        assert!(mid.abs() < 1e-6, "mid = {mid}");
    }

    #[test]
    fn wide_with_center_mix_keeps_mid() {
        let (mid, side) = radii(1.0, 1.0);
        assert!((mid - 1.0).abs() < 1e-6, "mid = {mid}");
        assert!((side - 2.0).abs() < 1e-6, "side = {side}");
    }

    /// 描画スケールの決め打ちが領域内に収まること。
    #[test]
    fn radii_fit_in_main_area() {
        let unit = 224.0 * UNIT_RADIUS_RATIO;
        let mut max_mid: f32 = 0.0;
        let mut max_side: f32 = 0.0;
        for i in 0..=100 {
            let t = i as f32 / 100.0;
            for c in [0.0, 0.5, 1.0] {
                let (mid, side) = radii(t, c);
                max_mid = max_mid.max(mid);
                max_side = max_side.max(side);
            }
        }
        assert!(unit * max_mid <= 112.0, "mid {} px", unit * max_mid);
        assert!(unit * max_side <= 392.0, "side {} px", unit * max_side);
    }
}
