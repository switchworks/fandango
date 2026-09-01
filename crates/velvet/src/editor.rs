//! velvet 用 egui エディタ実装。

use std::sync::Arc;

use nih_plug::prelude::{Editor, Param, ParamSetter};
use nih_plug_egui::egui::{self, Color32, Pos2, Rect, Stroke};
use nih_plug_egui::{create_egui_editor, EguiState};

use fandango_shared::ui::{
    apply_palette, bead_slider_row, choice_row, formula, install_fonts, show_shell_with_background,
    Background, Palette, Shell,
};

use crate::dsp;
use crate::{LengthMs, VelvetParams};

pub const WINDOW_W: u32 = 800;
pub const WINDOW_H: u32 = 400;

/// crate 固有の背景画像。暗さは `Background` 側の定数だけで決める（焼き込まない）。
const BACKGROUND: &[u8] = include_bytes!("../../../assets/backgrounds/velvet.jpg");

/// 図で使うサンプルレート。タップ位置は `fs` に依存するので基準値を置く
/// （実行時 `sr` は GUI スレッドから見えない）。タップ**時刻**は `fs` にほぼ
/// 依らないので、図の形は実機と一致する。
const SR_REF: f32 = 48_000.0;

/// 時間軸の右端 (ms)。`length` 最長段に合わせて固定する。軸を `length` に
/// 追従させると帯の長さが変わって見えなくなるため、あえて固定。
const AXIS_MS: f32 = 50.0;

/// エディタが持ち越す状態。Context 依存のテクスチャを抱えるので `build` で必ず捨てる。
pub struct EditorState {
    background: Background,
}

/// 図に渡す値。DSP の実挙動と一致させるため、タップ列は `dsp::build_sequence` を
/// そのまま呼んで得る（位置・極性・減衰は固定シードなので実機と完全一致）。
struct TapInput {
    length_sec: f32,
    width: f32,
    color: f32,
    bypass: bool,
}

/// ウィンドウのタイトル帯に出す表記。
pub const PLUGIN_LABEL: &str = "fandango : velvet";

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
    params: &VelvetParams,
    state: &mut EditorState,
) {
    apply_palette(ctx, Palette::Violet);
    let EditorState { background } = state;
    show_shell_with_background(
        ctx,
        Shell {
            wordmark: PLUGIN_LABEL,
            formula: formula::VELVET,
            kind: "OVN",
            bypass: Some((&params.bypass, setter)),
        },
        Some(background),
        |ui, rect| {
            let bypass = params.bypass.value();
            paint_taps(
                ui,
                rect.shrink(6.0),
                TapInput {
                    length_sec: params.length.value().seconds(),
                    // bypass 中は wet が加算されない。図も潰す。
                    width: if bypass {
                        0.0
                    } else {
                        params.width.modulated_plain_value()
                    },
                    color: params.color.modulated_plain_value(),
                    bypass,
                },
            );
        },
        |ui| {
            ui.columns(2, |cols| {
                bead_slider_row(&mut cols[0], "width", &params.width, setter);
                bead_slider_row(&mut cols[1], "color", &params.color, setter);
            });
            choice_row(
                ui,
                "length",
                &params.length,
                setter,
                &[
                    ("10 ms", LengthMs::Ms10),
                    ("20 ms", LengthMs::Ms20),
                    ("30 ms", LengthMs::Ms30),
                    ("40 ms", LengthMs::Ms40),
                    ("50 ms", LengthMs::Ms50),
                ],
            );
        },
    );
}

pub fn create(
    params: Arc<VelvetParams>,
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

/// L / R のタップ署名を上下 2 レーンに描く。
///
/// 縦線の**向き**が極性 `s(m) = ±1`、長さが `width · g(m)`（実際に加算される
/// wet の相対量）。L と R で線の並びが違うことが装置の本体（デコリレーション）。
fn paint_taps(ui: &egui::Ui, rect: Rect, input: TapInput) {
    let painter = ui.painter_at(rect);
    let palette = Palette::Violet.data();
    let grid = palette.neutral[5];

    let seq_l = dsp::build_sequence(crate::L_SEED, SR_REF, input.length_sec, dsp::DENSITY);
    let seq_r = dsp::build_sequence(crate::R_SEED, SR_REF, input.length_sec, dsp::DENSITY);

    let lane_h = rect.height() * 0.22;
    let lane_l = rect.center().y - rect.height() * 0.24;
    let lane_r = rect.center().y + rect.height() * 0.24;
    let x_of = |ms: f32| rect.left() + (ms / AXIS_MS).clamp(0.0, 1.0) * rect.width();

    // レーン基線と 10 ms 目盛り。
    let faint = Color32::from_rgba_unmultiplied(grid.r(), grid.g(), grid.b(), 60);
    let mut ms = 10.0;
    while ms < AXIS_MS {
        let x = x_of(ms);
        painter.line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            Stroke::new(1.0, faint),
        );
        ms += 10.0;
    }
    for y in [lane_l, lane_r] {
        painter.line_segment(
            [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
            Stroke::new(1.0, grid),
        );
    }

    // color は wet の高域を沈める。図では線の色を明→暗へ寄せて表す
    // （振幅ではなく色で表すのは、色軸が時間図に描けないため）。
    // 両端は明度差を大きく取る。背景が同系の紫なので、近い 2 段だと差が沈む。
    let bright = palette.accent[11];
    let dark = palette.accent[6];
    let tint = |t: f32| -> Color32 {
        let l = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t) as u8;
        Color32::from_rgb(
            l(bright.r(), dark.r()),
            l(bright.g(), dark.g()),
            l(bright.b(), dark.b()),
        )
    };
    let accent = if input.bypass {
        palette.neutral[8]
    } else {
        tint(input.color)
    };

    for (seq, base) in [(&seq_l, lane_l), (&seq_r, lane_r)] {
        for (&k, &gs) in seq.taps_k.iter().zip(seq.taps_gs.iter()) {
            let x = x_of(k as f32 / SR_REF * 1000.0);
            // gs = g(m)·s(m)。符号がそのまま向き、大きさがそのまま長さ。
            let h = (gs * input.width).clamp(-1.0, 1.0) * lane_h;
            painter.line_segment(
                [Pos2::new(x, base), Pos2::new(x, base - h)],
                Stroke::new(1.5, accent),
            );
        }
    }

    painter.text(
        Pos2::new(rect.left() + 2.0, lane_l - lane_h),
        egui::Align2::LEFT_TOP,
        "L",
        egui::FontId::proportional(11.0),
        grid,
    );
    painter.text(
        Pos2::new(rect.left() + 2.0, lane_r + lane_h),
        egui::Align2::LEFT_BOTTOM,
        "R",
        egui::FontId::proportional(11.0),
        grid,
    );
    painter.text(
        Pos2::new(rect.right(), rect.bottom()),
        egui::Align2::RIGHT_BOTTOM,
        "0 — 50 ms",
        egui::FontId::proportional(11.0),
        grid,
    );
}
